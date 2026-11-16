//! A request: one line read into a verb, an address and a value, and what it asks of the
//! mixer ([`plan`]): values to print, commands to apply, or addresses to follow.
//!
//! A request sends only commands the window's switches and faders send: `SetMute`, `SetSend`,
//! `SetLink`, `SetChainRouting`, `SetMixLevel`, `SetMixMute`, `SetPreview` and
//! `SetInsertEnabled`. What they mean, and when one is refused, is the model's
//! (`docs/reference/model.md` § Commands); a refusal is passed on word for word.

use vixmix_model::taper::{db_to_position, position_to_db};
use vixmix_model::{Command, MixId, Setup};

use crate::address::{self, CHANNELS, MIXES, PLUGINS, ROOT};
use crate::tree::{Kind, Leaf, leaves};
use crate::value::{self, SILENT, Value};

/// A fader's top, in decibels: the taper's last point.
pub const TOP_DB: f64 = 6.0;

/// What a request asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    /// Read a value, or every value under an address.
    Get,
    /// Write a switch or a fader.
    Set,
    /// Flip a switch.
    Toggle,
    /// Move a fader by an amount, kept within its travel.
    Nudge,
    /// Follow an address: be told when a value under it changes.
    Watch,
    /// Stop following it.
    Unwatch,
}

impl Verb {
    /// Every verb, in the order the reference lists them.
    pub const ALL: [Verb; 6] = [
        Verb::Get,
        Verb::Set,
        Verb::Toggle,
        Verb::Nudge,
        Verb::Watch,
        Verb::Unwatch,
    ];

    /// The word a request writes it with.
    pub fn word(self) -> &'static str {
        match self {
            Verb::Get => "get",
            Verb::Set => "set",
            Verb::Toggle => "toggle",
            Verb::Nudge => "nudge",
            Verb::Watch => "watch",
            Verb::Unwatch => "unwatch",
        }
    }

    /// Whether the verb is followed by a value after its address.
    fn wants_value(self) -> bool {
        matches!(self, Verb::Set | Verb::Nudge)
    }
}

/// One request, as its line wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub verb: Verb,
    /// The address as written: by id, or by a channel's or a plugin's name.
    pub address: String,
    /// What follows the address, for `set` and `nudge`.
    pub value: Option<String>,
}

/// Reads one line: `verb address [value]`, or an address alone (a `get`), or an address and
/// a value (a `set`), which is how OSC writes a message. A blank line is no request.
pub fn parse(line: &str) -> Result<Option<Request>, String> {
    let line = line.trim();
    let Some(first) = line.split_whitespace().next() else {
        return Ok(None);
    };
    let rest = line[first.len()..].trim();
    if first.starts_with('/') {
        return Ok(Some(Request {
            verb: if rest.is_empty() {
                Verb::Get
            } else {
                Verb::Set
            },
            address: first.to_string(),
            value: (!rest.is_empty()).then(|| rest.to_string()),
        }));
    }
    let Some(verb) = Verb::ALL
        .into_iter()
        .find(|verb| verb.word().eq_ignore_ascii_case(first))
    else {
        let words: Vec<&str> = Verb::ALL.iter().map(|verb| verb.word()).collect();
        return Err(format!(
            "`{first}` is not something a request can ask: a request is `verb address [value]` with one of {}, as `toggle /ch/1/monitor/mute`",
            words.join(", ")
        ));
    };
    let word = verb.word();
    let Some(address) = rest.split_whitespace().next() else {
        return Err(format!(
            "`{word}` wants an address after it, as `{word} /ch/1/monitor/mute`; `get /` lists every address"
        ));
    };
    let value = rest[address.len()..].trim();
    match (verb.wants_value(), value.is_empty()) {
        (true, true) => {
            let example = if verb == Verb::Nudge { "-2" } else { "true" };
            return Err(format!(
                "`{word}` wants a value after the address, as `{word} {address} {example}`"
            ));
        }
        (false, false) => {
            return Err(format!(
                "`{word}` takes an address and nothing after it, and `{value}` follows `{address}`: to write a value send `set {address} {value}`"
            ));
        }
        _ => {}
    }
    Ok(Some(Request {
        verb,
        address: address.to_string(),
        value: (!value.is_empty()).then(|| value.to_string()),
    }))
}

/// What a request asks of the mixer.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Print every value at or under these addresses.
    Read(Vec<String>),
    /// Apply these commands, in order.
    Write(Vec<Command>),
    /// Follow these addresses from now on, and print what is there.
    Watch(Vec<String>),
    /// Stop following these addresses.
    Unwatch(Vec<String>),
}

/// What `request` asks of a mixer whose setup is `setup`, or why it cannot be asked, ending
/// with what to send instead.
pub fn plan(setup: &Setup, request: &Request) -> Result<Plan, String> {
    let table = leaves(setup);
    let written = request.address.as_str();
    match request.verb {
        Verb::Get => Ok(Plan::Read(places(setup, &table, written)?)),
        Verb::Watch => Ok(Plan::Watch(places(setup, &table, written)?)),
        // Something that went can no longer be named, and may still be followed: an address
        // that names nothing is unfollowed as it was written.
        Verb::Unwatch => Ok(Plan::Unwatch(
            address::resolve(setup, written).unwrap_or_else(|_| vec![written.to_ascii_lowercase()]),
        )),
        Verb::Set | Verb::Toggle | Verb::Nudge => {
            let leaf = one(setup, &table, written)?;
            let commands = match request.verb {
                Verb::Set => set(leaf, request.value.as_deref().unwrap_or_default())?,
                Verb::Toggle => toggle(leaf)?,
                _ => nudge(setup, leaf, request.value.as_deref().unwrap_or_default())?,
            };
            Ok(Plan::Write(commands))
        }
    }
}

/// The addresses `written` names, each holding a value or being a place that may be empty.
fn places(setup: &Setup, table: &[Leaf], written: &str) -> Result<Vec<String>, String> {
    let places = address::resolve(setup, written)?;
    for place in &places {
        let lists = [ROOT, CHANNELS, MIXES, PLUGINS].contains(&place.as_str());
        if !lists && !table.iter().any(|l| address::covers(place, &l.address)) {
            return Err(nothing_at(table, place));
        }
    }
    Ok(places)
}

/// The one value `written` names, for a write.
fn one<'a>(setup: &Setup, table: &'a [Leaf], written: &str) -> Result<&'a Leaf, String> {
    let places = address::resolve(setup, written)?;
    let [place] = places.as_slice() else {
        return Err(format!(
            "`{written}` is every plugin of its strip, not one value: name a plugin by its slot or its name and then the value, as `{written}/1/enabled`"
        ));
    };
    if let Some(leaf) = table.iter().find(|l| l.address == *place) {
        return Ok(leaf);
    }
    let under = children(table, place);
    if under.is_empty() {
        return Err(nothing_at(table, place));
    }
    let first = if place == ROOT {
        format!("/{}", under[0])
    } else {
        format!("{place}/{}", under[0])
    };
    Err(format!(
        "`{place}` is not one value: under it there is {}; name one, as `{first}`",
        under.join(", ")
    ))
}

/// The next part of every address under `place`, each once, in the table's order.
fn children<'a>(table: &'a [Leaf], place: &str) -> Vec<&'a str> {
    let skip = if place == ROOT { 1 } else { place.len() + 1 };
    let mut out: Vec<&str> = Vec::new();
    for leaf in table {
        if leaf.address != place && address::covers(place, &leaf.address) {
            let part = leaf.address[skip..].split('/').next().unwrap_or_default();
            if !out.contains(&part) {
                out.push(part);
            }
        }
    }
    out
}

/// Why nothing is at `place`, with what there is beside it.
fn nothing_at(table: &[Leaf], place: &str) -> String {
    let parent = match place.rfind('/') {
        Some(0) | None => ROOT,
        Some(at) => &place[..at],
    };
    let beside = children(table, parent);
    if beside.is_empty() {
        format!("there is nothing at `{place}`: `get /` lists every address")
    } else {
        format!(
            "there is nothing at `{place}`: under `{parent}` there is {}",
            beside.join(", ")
        )
    }
}

fn read_only(leaf: &Leaf, how: &str) -> String {
    format!(
        "`{}` is read only: the control socket changes the console's switches and faders, and {how}",
        leaf.address
    )
}

/// A switch written in a request, or what to send instead.
fn switch(leaf: &Leaf, token: &str) -> Result<bool, String> {
    value::switch(token).ok_or_else(|| {
        format!(
            "`{}` is a switch, and `{token}` is neither on nor off: send `true` or `false`",
            leaf.address
        )
    })
}

/// A fader's position for a number written in a request: the number itself for a position,
/// through the taper for decibels.
fn fader(leaf: &Leaf, decibels: bool, token: &str) -> Result<f32, String> {
    let address = &leaf.address;
    let Some(n) = value::number(token) else {
        let example = if decibels { "-12" } else { "0.84" };
        return Err(format!(
            "`{address}` is a number, and `{token}` is not one: send it plain, as `{example}`"
        ));
    };
    if !decibels {
        if n == f64::NEG_INFINITY {
            return Err(format!(
                "`{address}` is a position from 0 to 1, and `{token}` is not one: send `0` for silence, or write the level in decibels to the `db` beside it"
            ));
        }
        // Out of range is the model's to refuse, in its own words.
        return Ok(n as f32);
    }
    if n > TOP_DB {
        return Err(format!(
            "a fader's top is +{TOP_DB} dB and {n} is above it: send {TOP_DB} or less to `{address}`"
        ));
    }
    Ok(db_to_position(n as f32))
}

/// The commands a `set` of `leaf` to `token` sends.
fn set(leaf: &Leaf, token: &str) -> Result<Vec<Command>, String> {
    Ok(match leaf.kind {
        Kind::Name(_) => return Err(read_only(leaf, "a channel is renamed in the window")),
        Kind::ChannelKind(_) => {
            return Err(read_only(
                leaf,
                "a channel is a hardware input or a group of apps from the moment it is added",
            ));
        }
        Kind::PluginName(_) | Kind::PluginStrip(_) | Kind::PluginSlot(_) => {
            return Err(read_only(
                leaf,
                "a plugin is added, moved and removed in the window",
            ));
        }
        Kind::Link(ch) => vec![Command::SetLink {
            ch,
            link: switch(leaf, token)?,
        }],
        Kind::ChannelMute(ch) => {
            let mute = switch(leaf, token)?;
            [MixId::Monitor, MixId::Master]
                .into_iter()
                .map(|mix| Command::SetMute { ch, mix, mute })
                .collect()
        }
        Kind::SendMute(ch, mix) => vec![Command::SetMute {
            ch,
            mix,
            mute: switch(leaf, token)?,
        }],
        Kind::SendDb(..) | Kind::MixDb(_) => move_fader(leaf.kind, fader(leaf, true, token)?),
        Kind::SendPosition(..) | Kind::MixPosition(_) => {
            move_fader(leaf.kind, fader(leaf, false, token)?)
        }
        Kind::Processed(ch, mix) => vec![Command::SetChainRouting {
            ch,
            mix,
            on: switch(leaf, token)?,
        }],
        Kind::MixMute(mix) => vec![Command::SetMixMute {
            mix,
            mute: switch(leaf, token)?,
        }],
        Kind::Preview => vec![Command::SetPreview(switch(leaf, token)?)],
        Kind::Enabled(instance) => vec![Command::SetInsertEnabled {
            instance,
            enabled: switch(leaf, token)?,
        }],
    })
}

/// The commands a `toggle` of `leaf` sends: the switch set to what it is not.
fn toggle(leaf: &Leaf) -> Result<Vec<Command>, String> {
    match leaf.value {
        Value::Switch(on) => set(leaf, if on { "false" } else { "true" }),
        _ => Err(format!(
            "`{}` is not a switch, so there is nothing to toggle: `set` it to a value, or `nudge` a fader by an amount",
            leaf.address
        )),
    }
}

/// The commands a `nudge` of `leaf` by `token` sends: the fader moved by that much, and no
/// further than its travel.
fn nudge(setup: &Setup, leaf: &Leaf, token: &str) -> Result<Vec<Command>, String> {
    let address = &leaf.address;
    let Some(now) = fader_at(setup, leaf.kind) else {
        return Err(format!(
            "`{address}` is not a fader, so there is nothing to nudge: `set` it, or `toggle` a switch"
        ));
    };
    let amount = value::number(token).filter(|n| n.is_finite()).ok_or_else(|| {
        format!(
            "`nudge` moves a fader by an amount, and `{token}` is not a number: send one, as `nudge {address} -2`"
        )
    })?;
    let to = if matches!(leaf.kind, Kind::SendDb(..) | Kind::MixDb(_)) {
        let db = f64::from(position_to_db(now));
        if db == f64::NEG_INFINITY {
            return Err(format!(
                "the fader at `{address}` is silent (`{SILENT}`), which is no number of decibels to move from: `set {address}` to a level, as `-60`, or nudge the `position` beside it"
            ));
        }
        // Below the taper's floor is silence; the top is the top.
        db_to_position((db + amount).min(TOP_DB) as f32)
    } else {
        (f64::from(now) + amount).clamp(0.0, 1.0) as f32
    };
    Ok(move_fader(leaf.kind, to))
}

/// Where the fader a value belongs to stands; `None` for a value that is not a fader's.
fn fader_at(setup: &Setup, kind: Kind) -> Option<f32> {
    match kind {
        Kind::SendDb(ch, mix) | Kind::SendPosition(ch, mix) => {
            setup.channel(ch).map(|c| c.send(mix).position)
        }
        Kind::MixDb(mix) | Kind::MixPosition(mix) => Some(setup.mix(mix).level.position),
        _ => None,
    }
}

/// The command that puts the fader a value belongs to at `position`.
fn move_fader(kind: Kind, position: f32) -> Vec<Command> {
    match kind {
        Kind::SendDb(ch, mix) | Kind::SendPosition(ch, mix) => {
            vec![Command::SetSend { ch, mix, position }]
        }
        Kind::MixDb(mix) | Kind::MixPosition(mix) => vec![Command::SetMixLevel { mix, position }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::{ChannelId, InstanceId, apply, example};

    fn request(line: &str) -> Request {
        parse(line)
            .unwrap_or_else(|e| panic!("{line}: {e}"))
            .unwrap_or_else(|| panic!("{line} is no request"))
    }

    fn sends(setup: &Setup, line: &str) -> Vec<Command> {
        match plan(setup, &request(line)).unwrap_or_else(|e| panic!("{line}: {e}")) {
            Plan::Write(commands) => commands,
            other => panic!("{line} is not a write: {other:?}"),
        }
    }

    #[test]
    fn a_line_is_a_verb_an_address_and_what_follows() {
        assert_eq!(
            request("set /ch/1/monitor/mute true"),
            Request {
                verb: Verb::Set,
                address: "/ch/1/monitor/mute".to_string(),
                value: Some("true".to_string()),
            }
        );
        assert_eq!(request("  GET   /ch/mic  ").verb, Verb::Get);
        assert_eq!(
            request("nudge /mix/monitor/db -2").value.as_deref(),
            Some("-2")
        );
        // As OSC writes a message: an address alone reads, an address and a value write.
        assert_eq!(request("/ch/1/mute"), request("get /ch/1/mute"));
        assert_eq!(request("/ch/1/mute true"), request("set /ch/1/mute true"));
        // A blank line is no request, and no error.
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("   \t"), Ok(None));
    }

    #[test]
    fn a_line_that_is_no_request_says_what_one_is() {
        for (line, expected) in [
            ("mute /ch/1", "get, set, toggle, nudge, watch, unwatch"),
            ("toggle", "`toggle` wants an address after it"),
            ("set /ch/1/mute", "as `set /ch/1/mute true`"),
            ("nudge /mix/master/db", "as `nudge /mix/master/db -2`"),
            ("get /ch/1/mute true", "send `set /ch/1/mute true`"),
            (
                "toggle /ch/1/mute now",
                "takes an address and nothing after it",
            ),
        ] {
            let refused = parse(line).expect_err(line);
            assert!(refused.contains(expected), "{line} said `{refused}`");
        }
    }

    #[test]
    fn each_switch_and_fader_sends_the_command_its_control_sends() {
        let setup = example::setup();
        let mic = ChannelId(1);
        for (line, expected) in [
            (
                "set /ch/mic/monitor/mute true",
                Command::SetMute {
                    ch: mic,
                    mix: MixId::Monitor,
                    mute: true,
                },
            ),
            (
                "set /ch/1/link on",
                Command::SetLink {
                    ch: mic,
                    link: true,
                },
            ),
            (
                "set /ch/1/master/processed 0",
                Command::SetChainRouting {
                    ch: mic,
                    mix: MixId::Master,
                    on: false,
                },
            ),
            (
                "set /ch/1/monitor/position 0.5",
                Command::SetSend {
                    ch: mic,
                    mix: MixId::Monitor,
                    position: 0.5,
                },
            ),
            (
                "set /mix/master/mute true",
                Command::SetMixMute {
                    mix: MixId::Master,
                    mute: true,
                },
            ),
            (
                "set /mix/monitor/position 1",
                Command::SetMixLevel {
                    mix: MixId::Monitor,
                    position: 1.0,
                },
            ),
            ("set /mix/master/preview true", Command::SetPreview(true)),
            (
                "set /ch/mic/fx/pro-c-2/enabled false",
                Command::SetInsertEnabled {
                    instance: InstanceId(2),
                    enabled: false,
                },
            ),
            (
                "toggle /fx/4/enabled",
                Command::SetInsertEnabled {
                    instance: InstanceId(4),
                    enabled: true,
                },
            ),
            // Music's Master send is muted: a toggle unmutes it.
            (
                "toggle /ch/music/master/mute",
                Command::SetMute {
                    ch: ChannelId(3),
                    mix: MixId::Master,
                    mute: false,
                },
            ),
        ] {
            assert_eq!(sends(&setup, line), [expected], "{line}");
        }
    }

    #[test]
    fn a_channels_mute_is_both_of_its_sends() {
        let mut setup = example::setup();
        let music = ChannelId(3);
        let both = |mute| {
            [MixId::Monitor, MixId::Master].map(|mix| Command::SetMute {
                ch: music,
                mix,
                mute,
            })
        };
        assert_eq!(sends(&setup, "set /ch/music/mute true"), both(true));
        // One send muted is not the channel muted: a toggle mutes both.
        assert_eq!(sends(&setup, "toggle /ch/music/mute"), both(true));
        for command in both(true) {
            let _ = apply(&mut setup, command).unwrap();
        }
        assert_eq!(sends(&setup, "toggle /ch/music/mute"), both(false));
    }

    #[test]
    fn decibels_go_through_the_taper_and_unity_is_exact() {
        let setup = example::setup();
        let position = |line: &str| match sends(&setup, line).as_slice() {
            [Command::SetSend { position, .. }] | [Command::SetMixLevel { position, .. }] => {
                *position
            }
            other => panic!("{line} sent {other:?}"),
        };
        // 0 dB is the taper's unity, bit for bit: the engine's ×1 path.
        assert_eq!(
            position("set /ch/3/monitor/db 0"),
            vixmix_model::taper::UNITY
        );
        assert_eq!(position("set /ch/3/monitor/db -12"), 0.56);
        assert_eq!(position("set /mix/master/db 6"), 1.0);
        assert_eq!(position("set /mix/master/db -inf"), 0.0);
        assert_eq!(position("set /mix/master/db -200"), 0.0);
        assert_eq!(TOP_DB, f64::from(position_to_db(1.0)), "the taper's top");
    }

    #[test]
    fn a_nudge_moves_a_fader_by_an_amount_and_stops_at_its_ends() {
        let mut setup = example::setup();
        let position = |setup: &Setup, line: &str| match sends(setup, line).as_slice() {
            [Command::SetSend { position, .. }] | [Command::SetMixLevel { position, .. }] => {
                *position
            }
            other => panic!("{line} sent {other:?}"),
        };
        // Monitor is at unity: two down is -2 dB, between the taper's -6 at 0.70 and 0 at 0.84.
        let down = position(&setup, "nudge /mix/monitor/db -2");
        assert!((position_to_db(down) + 2.0).abs() < 1e-4, "{down}");
        assert!((0.70..0.84).contains(&down), "{down}");
        // Up past the top stops at the top.
        assert_eq!(position(&setup, "nudge /mix/monitor/db 20"), 1.0);
        // A position moves by its own amount, and stops at 0 and at 1.
        let moved = position(&setup, "nudge /mix/monitor/position -0.04");
        assert!((moved - 0.80).abs() < 1e-6, "{moved}");
        assert_eq!(position(&setup, "nudge /mix/monitor/position 5"), 1.0);
        assert_eq!(position(&setup, "nudge /mix/monitor/position -5"), 0.0);
        // Down past the taper's floor is silence.
        assert_eq!(position(&setup, "nudge /mix/monitor/db -200"), 0.0);
        // From silence there is no number of decibels to move from.
        let _ = apply(
            &mut setup,
            Command::SetMixLevel {
                mix: MixId::Monitor,
                position: 0.0,
            },
        )
        .unwrap();
        let refused = plan(&setup, &request("nudge /mix/monitor/db 2")).expect_err("silent");
        assert!(
            refused.contains("silent") && refused.contains("nudge the `position` beside it"),
            "{refused}"
        );
        assert!(position(&setup, "nudge /mix/monitor/position 0.1") > 0.09);
    }

    #[test]
    fn what_cannot_be_written_says_what_to_send_instead() {
        let setup = example::setup();
        for (line, expected) in [
            ("set /ch/1/name \"Voice\"", "read only"),
            ("set /ch/1/kind \"apps\"", "from the moment it is added"),
            (
                "set /fx/1/slot 2",
                "a plugin is added, moved and removed in the window",
            ),
            ("set /ch/1/monitor/mute loud", "send `true` or `false`"),
            ("set /ch/1/monitor/db loud", "send it plain, as `-12`"),
            ("set /ch/1/monitor/position half", "as `0.84`"),
            ("set /ch/1/monitor/position -inf", "send `0` for silence"),
            ("set /ch/1/monitor/db 7", "send 6 or less"),
            ("set /ch/1/monitor/db inf", "is not one"),
            (
                "set /ch/1 true",
                "under it there is name, kind, link, mute, monitor, master; name one, as `/ch/1/name`",
            ),
            (
                "set /ch/1/monitor true",
                "name one, as `/ch/1/monitor/mute`",
            ),
            ("set / true", "name one, as `/ch`"),
            ("set /ch/mic/fx true", "as `/ch/mic/fx/1/enabled`"),
            (
                "set /ch/1/monitor/volume 1",
                "under `/ch/1/monitor` there is mute, db, position, processed",
            ),
            ("toggle /ch/1/monitor/db", "nothing to toggle"),
            ("toggle /ch/1/name", "nothing to toggle"),
            ("nudge /ch/1/monitor/mute 1", "nothing to nudge"),
            (
                "nudge /ch/1/monitor/db up",
                "as `nudge /ch/1/monitor/db -2`",
            ),
            ("nudge /ch/1/monitor/db -inf", "is not a number"),
            (
                "get /ch/1/monitor/volume",
                "there is nothing at `/ch/1/monitor/volume`",
            ),
            (
                "watch /fx/1/bypass",
                "under `/fx/1` there is name, strip, slot, enabled",
            ),
            ("get /ch/9", "there is no channel 9"),
        ] {
            let refused = plan(&setup, &request(line)).expect_err(line);
            assert!(refused.contains(expected), "{line} said `{refused}`");
        }
    }

    #[test]
    fn a_read_names_what_to_print_and_an_empty_place_is_no_error() {
        let setup = example::setup();
        assert_eq!(
            plan(&setup, &request("get /ch/mic/master/mute")),
            Ok(Plan::Read(vec!["/ch/1/master/mute".to_string()]))
        );
        assert_eq!(
            plan(&setup, &request("get /")),
            Ok(Plan::Read(vec!["/".to_string()]))
        );
        assert_eq!(
            plan(&setup, &request("watch /ch/mic/fx")),
            Ok(Plan::Watch(vec![
                "/fx/1".to_string(),
                "/fx/2".to_string(),
                "/fx/3".to_string()
            ]))
        );
        // Game has no plugins, and a setup may have no channels: nothing to print, no error.
        assert_eq!(
            plan(&setup, &request("get /ch/game/fx")),
            Ok(Plan::Read(Vec::new()))
        );
        let mut empty = example::setup();
        empty.channels.clear();
        assert_eq!(
            plan(&empty, &request("get /ch")),
            Ok(Plan::Read(vec!["/ch".to_string()]))
        );
        // What went can still be unfollowed, as it was written.
        assert_eq!(
            plan(&setup, &request("unwatch /CH/9")),
            Ok(Plan::Unwatch(vec!["/ch/9".to_string()]))
        );
        assert_eq!(
            plan(&setup, &request("unwatch /ch/mic")),
            Ok(Plan::Unwatch(vec!["/ch/1".to_string()]))
        );
    }
}
