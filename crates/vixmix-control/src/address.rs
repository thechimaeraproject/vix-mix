//! Addresses: what a request names and what the mixer prints.
//!
//! What the mixer prints is the address as it is kept: a channel by its id (`/ch/3`), a mix
//! by its word (`/mix/master`), a plugin by its instance id (`/fx/7`), each followed by the
//! value's own words (`/ch/3/monitor/mute`). Ids never change and never come back
//! (`docs/reference/model.md` § Ids), so an address a controller holds stays the same thing
//! for as long as that thing exists.
//!
//! What a request names may also be written as a person reads the console: a channel by its
//! name, and a plugin under its strip by its slot, counted from 1, or by its name.
//! [`resolve`] turns either into the addresses as they are kept, or says which names there
//! are.

use vixmix_model::{Channel, ChannelId, Insert, InstanceId, MixId, Setup, Strip, slug};

/// Everything.
pub const ROOT: &str = "/";

/// Every channel, every mix, every plugin: the three places an address starts.
pub const CHANNELS: &str = "/ch";
pub const MIXES: &str = "/mix";
pub const PLUGINS: &str = "/fx";

/// Whether `address` is `place` or lies under it. Parts are compared whole: `/ch/1` does not
/// cover `/ch/10`.
pub fn covers(place: &str, address: &str) -> bool {
    place == ROOT
        || address == place
        || (address.starts_with(place) && address.as_bytes().get(place.len()) == Some(&b'/'))
}

/// A channel's address.
pub fn channel(id: ChannelId) -> String {
    format!("{CHANNELS}/{}", id.0)
}

/// The word a mix is written with.
pub fn mix_word(id: MixId) -> &'static str {
    match id {
        MixId::Monitor => "monitor",
        MixId::Master => "master",
    }
}

/// A mix's address.
pub fn mix(id: MixId) -> String {
    format!("{MIXES}/{}", mix_word(id))
}

/// A plugin's address, by the instance of its insert.
pub fn plugin(instance: InstanceId) -> String {
    format!("{PLUGINS}/{}", instance.0)
}

/// A strip's address: a channel's or a mix's.
pub fn strip(strip: Strip) -> String {
    match strip {
        Strip::Channel(id) => channel(id),
        Strip::Mix(id) => mix(id),
    }
}

/// The channel, mix or plugin an address belongs to: its first two parts (`/ch/3` of
/// `/ch/3/monitor/mute`). An address of fewer parts is its own.
pub fn node(address: &str) -> &str {
    address
        .match_indices('/')
        .nth(2)
        .map_or(address, |(at, _)| &address[..at])
}

/// The addresses, as they are kept, that `written` names in `setup`: one for a value or for
/// a channel, a mix or a plugin, and one for each plugin of a strip for `…/fx` alone (none
/// when the strip has no plugins). An address that names nothing is refused with the names
/// there are.
pub fn resolve(setup: &Setup, written: &str) -> Result<Vec<String>, String> {
    let Some(rest) = written.strip_prefix('/') else {
        return Err(format!(
            "`{written}` is not an address: an address starts with `/`, as `/ch/1/monitor/mute` does, and `get /` lists every one"
        ));
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Ok(vec![ROOT.to_string()]);
    }
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err(format!(
            "`{written}` has an empty part between two slashes: write each part once, as `/ch/1/monitor/mute`"
        ));
    }
    match parts[0].to_ascii_lowercase().as_str() {
        "ch" => match parts.get(1) {
            None => Ok(vec![CHANNELS.to_string()]),
            Some(named) => {
                let found = find_channel(setup, named)?;
                under_strip(channel(found.id), &found.name, &found.chain, &parts[2..])
            }
        },
        "mix" => match parts.get(1) {
            None => Ok(vec![MIXES.to_string()]),
            Some(named) => {
                let id = find_mix(named)?;
                under_strip(mix(id), id.name(), &setup.mix(id).chain, &parts[2..])
            }
        },
        "fx" => match parts.get(1) {
            None => Ok(vec![PLUGINS.to_string()]),
            Some(named) => {
                let instance = find_instance(setup, named)?;
                Ok(vec![join(plugin(instance), &parts[2..])])
            }
        },
        other => Err(format!(
            "there is nothing at `/{other}`: an address starts with `{CHANNELS}` (a channel), `{MIXES}` (Monitor or Master) or `{PLUGINS}` (a plugin by its instance id)"
        )),
    }
}

/// `tail` under a strip at `home`: the strip's own values, or, after `fx`, its plugins.
fn under_strip(
    home: String,
    name: &str,
    chain: &[Insert],
    tail: &[&str],
) -> Result<Vec<String>, String> {
    if !tail
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("fx"))
    {
        return Ok(vec![join(home, tail)]);
    }
    match tail.get(1) {
        None => Ok(chain.iter().map(|insert| plugin(insert.instance)).collect()),
        Some(named) => {
            let insert = find_insert(&home, name, chain, named)?;
            Ok(vec![join(plugin(insert.instance), &tail[2..])])
        }
    }
}

/// `home` with each of `tail`'s parts after it, in lower case as every value's words are.
fn join(mut home: String, tail: &[&str]) -> String {
    for part in tail {
        home.push('/');
        home.push_str(&part.to_ascii_lowercase());
    }
    home
}

/// Whether a part is a number: an id or a slot, never a name.
fn is_number(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
}

fn find_channel<'a>(setup: &'a Setup, named: &str) -> Result<&'a Channel, String> {
    let listed = || {
        if setup.channels.is_empty() {
            "this setup has no channels".to_string()
        } else {
            let each: Vec<String> = setup
                .channels
                .iter()
                .map(|c| format!("`{}` ({})", channel(c.id), c.name))
                .collect();
            format!("the channels are {}", each.join(", "))
        }
    };
    if is_number(named) {
        return named
            .parse::<u32>()
            .ok()
            .and_then(|id| setup.channel(ChannelId(id)))
            .ok_or_else(|| format!("there is no channel {named}: {}", listed()));
    }
    let wanted = slug(named);
    let found: Vec<&Channel> = setup
        .channels
        .iter()
        .filter(|c| slug(&c.name) == wanted)
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no channel is named `{named}`: {}", listed())),
        several => {
            let each: Vec<String> = several
                .iter()
                .map(|c| format!("`{}`", channel(c.id)))
                .collect();
            Err(format!(
                "`{named}` names {} channels: write the one meant by its id, {}",
                several.len(),
                each.join(" or ")
            ))
        }
    }
}

fn find_mix(named: &str) -> Result<MixId, String> {
    match named.to_ascii_lowercase().as_str() {
        "monitor" => Ok(MixId::Monitor),
        "master" => Ok(MixId::Master),
        _ => Err(format!(
            "there is no mix `{named}`: the mixes are `{MIXES}/monitor` and `{MIXES}/master`"
        )),
    }
}

fn find_instance(setup: &Setup, named: &str) -> Result<InstanceId, String> {
    if !is_number(named) {
        return Err(format!(
            "after `{PLUGINS}` a plugin is written by its instance id, and `{named}` is not a number: write it under its strip by slot or name, as `/ch/1/fx/{named}`, or read the ids with `get {PLUGINS}`"
        ));
    }
    named
        .parse::<u32>()
        .ok()
        .map(InstanceId)
        .filter(|id| setup.inserts().any(|insert| insert.instance == *id))
        .ok_or_else(|| {
            format!(
                "there is no plugin with instance {named} (it may have just been removed): `get {PLUGINS}` lists the ones there are"
            )
        })
}

fn find_insert<'a>(
    home: &str,
    strip: &str,
    chain: &'a [Insert],
    named: &str,
) -> Result<&'a Insert, String> {
    let listed = || {
        if chain.is_empty() {
            format!("{strip} has no plugins")
        } else {
            let each: Vec<String> = chain
                .iter()
                .enumerate()
                .map(|(index, insert)| {
                    format!("`{home}/fx/{}` ({})", index + 1, insert.plugin.name)
                })
                .collect();
            format!("{strip} has {}", each.join(", "))
        }
    };
    if is_number(named) {
        return named
            .parse::<usize>()
            .ok()
            .and_then(|slot| slot.checked_sub(1))
            .and_then(|index| chain.get(index))
            .ok_or_else(|| {
                format!(
                    "there is no slot {named} on {strip}, where slots count from 1: {}",
                    listed()
                )
            });
    }
    let wanted = slug(named);
    let found: Vec<(usize, &Insert)> = chain
        .iter()
        .enumerate()
        .filter(|(_, insert)| slug(&insert.plugin.name) == wanted)
        .collect();
    match found.as_slice() {
        [(_, one)] => Ok(one),
        [] => Err(format!(
            "no plugin on {strip} is named `{named}`: {}",
            listed()
        )),
        several => {
            let each: Vec<String> = several
                .iter()
                .map(|(index, _)| format!("`{home}/fx/{}`", index + 1))
                .collect();
            Err(format!(
                "`{named}` names {} plugins on {strip}: write the one meant by its slot, {}",
                several.len(),
                each.join(" or ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::{Command, apply, example};

    fn one(setup: &Setup, written: &str) -> String {
        let found = resolve(setup, written).unwrap_or_else(|e| panic!("{written}: {e}"));
        assert_eq!(found.len(), 1, "{written} named {found:?}");
        found.into_iter().next().unwrap()
    }

    #[test]
    fn an_address_covers_itself_and_what_is_under_it_part_by_part() {
        assert!(covers("/ch/1", "/ch/1"));
        assert!(covers("/ch/1", "/ch/1/monitor/mute"));
        assert!(!covers("/ch/1", "/ch/10/monitor/mute"));
        assert!(!covers("/ch/1/monitor", "/ch/1"));
        assert!(covers(ROOT, "/fx/7/enabled"));
        assert!(covers(CHANNELS, "/ch/3/name"));
        assert_eq!(node("/ch/3/monitor/mute"), "/ch/3");
        assert_eq!(node("/fx/7/enabled"), "/fx/7");
        assert_eq!(node("/mix/master"), "/mix/master");
        assert_eq!(node("/ch"), "/ch");
    }

    #[test]
    fn a_channel_is_written_by_id_or_by_name_and_kept_by_id() {
        let setup = example::setup();
        assert_eq!(one(&setup, "/ch/1/monitor/mute"), "/ch/1/monitor/mute");
        assert_eq!(one(&setup, "/ch/mic/monitor/mute"), "/ch/1/monitor/mute");
        // The node name's rule: case and punctuation do not matter.
        assert_eq!(one(&setup, "/ch/Voice-Chat/Master/DB"), "/ch/5/master/db");
        assert_eq!(one(&setup, "/ch/voice_chat"), "/ch/5");
        assert_eq!(one(&setup, "/mix/Master/preview"), "/mix/master/preview");
        assert_eq!(one(&setup, "/"), "/");
        assert_eq!(one(&setup, "/ch/"), "/ch");
    }

    #[test]
    fn a_plugin_is_written_by_instance_or_under_its_strip_by_slot_or_name() {
        let setup = example::setup();
        // Mic's chain: Noise Suppression for Voice, Pro-C 2, Pro-L 2, instances 1 to 3.
        assert_eq!(one(&setup, "/fx/2/enabled"), "/fx/2/enabled");
        assert_eq!(one(&setup, "/ch/mic/fx/2/enabled"), "/fx/2/enabled");
        assert_eq!(one(&setup, "/ch/1/fx/pro-c-2/enabled"), "/fx/2/enabled");
        assert_eq!(one(&setup, "/ch/mic/fx/Pro-L-2"), "/fx/3");
        // Master's chain: Pro-Q 3 and Pro-L 2, instances 8 and 9.
        assert_eq!(one(&setup, "/mix/master/fx/1/enabled"), "/fx/8/enabled");
        assert_eq!(one(&setup, "/mix/master/fx/pro-l-2/name"), "/fx/9/name");
        // `fx` alone is every plugin of the strip, in order; a strip without any is none.
        assert_eq!(
            resolve(&setup, "/ch/mic/fx").unwrap(),
            ["/fx/1", "/fx/2", "/fx/3"]
        );
        assert_eq!(
            resolve(&setup, "/ch/game/fx").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_number_is_always_an_id_or_a_slot_never_a_name() {
        let mut setup = example::setup();
        let renamed = apply(
            &mut setup,
            Command::Rename {
                ch: ChannelId(2),
                name: "1".to_string(),
            },
        );
        assert!(renamed.is_ok());
        // Channel 2 is now named "1": `/ch/1` is still channel 1, Mic.
        assert_eq!(one(&setup, "/ch/1/name"), "/ch/1/name");
        assert_eq!(one(&setup, "/ch/2/name"), "/ch/2/name");
    }

    #[test]
    fn what_names_nothing_is_refused_with_the_names_there_are() {
        let mut setup = example::setup();
        for (written, expected) in [
            ("ch/1", "starts with `/`"),
            ("/ch//mute", "empty part"),
            (
                "/channel/1",
                "`/ch` (a channel), `/mix` (Monitor or Master) or `/fx`",
            ),
            (
                "/ch/9/mute",
                "there is no channel 9: the channels are `/ch/1` (Mic)",
            ),
            (
                "/ch/drums/mute",
                "no channel is named `drums`: the channels are `/ch/1` (Mic)",
            ),
            (
                "/mix/stream",
                "the mixes are `/mix/monitor` and `/mix/master`",
            ),
            ("/fx/pro-q-3", "as `/ch/1/fx/pro-q-3`"),
            ("/fx/99/enabled", "`get /fx` lists the ones there are"),
            (
                "/ch/mic/fx/4",
                "slots count from 1: Mic has `/ch/1/fx/1` (Noise Suppression for Voice)",
            ),
            ("/ch/mic/fx/0", "slots count from 1"),
            (
                "/ch/mic/fx/ott",
                "no plugin on Mic is named `ott`: Mic has `/ch/1/fx/1`",
            ),
            ("/ch/game/fx/1", "Game has no plugins"),
        ] {
            let refused = resolve(&setup, written).expect_err(written);
            assert!(refused.contains(expected), "{written} said `{refused}`");
        }
        // Two channels whose names are the same word: the name alone is refused, naming both.
        let renamed = apply(
            &mut setup,
            Command::Rename {
                ch: ChannelId(2),
                name: "MIC!".to_string(),
            },
        );
        assert!(renamed.is_ok());
        let refused = resolve(&setup, "/ch/mic/mute").expect_err("two channels");
        assert!(
            refused.contains("names 2 channels") && refused.contains("`/ch/1` or `/ch/2`"),
            "{refused}"
        );
        // Two of one plugin on a strip: Master gets a second Pro-Q 3.
        let added = apply(
            &mut setup,
            Command::AddInsert {
                strip: Strip::Mix(MixId::Master),
                plugin: example::plugin("Pro-Q 3"),
                at: 2,
            },
        );
        assert!(added.is_ok());
        let refused = resolve(&setup, "/mix/master/fx/pro-q-3/enabled").expect_err("two plugins");
        assert!(
            refused.contains("names 2 plugins on Master")
                && refused.contains("`/mix/master/fx/1` or `/mix/master/fx/3`"),
            "{refused}"
        );
    }
}
