//! Every address of a setup with its value, and what changed between two setups.
//!
//! [`leaves`] is the one list the protocol is read from: what `get` prints, what a write may
//! name and what it sends ([`Kind`]), and what a follower is told ([`diff`]). A channel's
//! plugins are not under the channel: a plugin keeps its address when it is moved to another
//! strip, and says where it is in `strip` and `slot`.

use std::collections::{HashMap, HashSet};
use std::fmt;

use vixmix_model::taper::position_to_db;
use vixmix_model::{ChannelId, ChannelKind, InstanceId, MixId, Send, Setup, Strip};

use crate::address;
use crate::value::{Value, rounded};

/// What a value is, and so what a write to it sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A channel's name. Read only.
    Name(ChannelId),
    /// `hardware` or `apps`. Read only.
    ChannelKind(ChannelId),
    /// The link between a channel's two faders.
    Link(ChannelId),
    /// Both of a channel's sends muted.
    ChannelMute(ChannelId),
    /// One send's mute.
    SendMute(ChannelId, MixId),
    /// One send's fader, in decibels.
    SendDb(ChannelId, MixId),
    /// One send's fader, as its position.
    SendPosition(ChannelId, MixId),
    /// MON or MST: whether the send takes the chain's output.
    Processed(ChannelId, MixId),
    /// A mix's mute.
    MixMute(MixId),
    /// A mix's fader, in decibels.
    MixDb(MixId),
    /// A mix's fader, as its position.
    MixPosition(MixId),
    /// The ear: Monitor's output plays the Master mix.
    Preview,
    /// A plugin's bypass square.
    Enabled(InstanceId),
    /// A plugin's name. Read only.
    PluginName(InstanceId),
    /// The strip a plugin is on, as that strip's address. Read only.
    PluginStrip(InstanceId),
    /// A plugin's place in its strip's chain, counted from 1. Read only.
    PluginSlot(InstanceId),
}

/// One address with its value.
#[derive(Clone, Debug, PartialEq)]
pub struct Leaf {
    pub address: String,
    pub value: Value,
    pub kind: Kind,
}

impl fmt::Display for Leaf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.address, self.value)
    }
}

/// The decimals a position is printed to: finer than a fader's pixel or a controller's step.
const POSITION_PLACES: i32 = 4;
/// The decimals decibels are printed to.
const DB_PLACES: i32 = 2;

fn position(send: &Send) -> Value {
    Value::Number(rounded(f64::from(send.position), POSITION_PLACES))
}

fn db(send: &Send) -> Value {
    Value::Number(rounded(f64::from(position_to_db(send.position)), DB_PLACES))
}

/// Every address of `setup` with its value: the channels left to right, then Monitor and
/// Master, then every plugin, strip by strip in the same order.
pub fn leaves(setup: &Setup) -> Vec<Leaf> {
    let mut out = Vec::new();
    let mut leaf = |home: &str, word: &str, value: Value, kind: Kind| {
        out.push(Leaf {
            address: format!("{home}/{word}"),
            value,
            kind,
        });
    };
    for ch in &setup.channels {
        let home = address::channel(ch.id);
        leaf(
            &home,
            "name",
            Value::Text(ch.name.clone()),
            Kind::Name(ch.id),
        );
        let kind = match ch.kind() {
            ChannelKind::Hardware => "hardware",
            ChannelKind::Apps => "apps",
        };
        leaf(
            &home,
            "kind",
            Value::Text(kind.to_string()),
            Kind::ChannelKind(ch.id),
        );
        leaf(&home, "link", Value::Switch(ch.link), Kind::Link(ch.id));
        leaf(
            &home,
            "mute",
            Value::Switch(ch.monitor.mute && ch.master.mute),
            Kind::ChannelMute(ch.id),
        );
        for mix in [MixId::Monitor, MixId::Master] {
            let send = ch.send(mix);
            let home = format!("{home}/{}", address::mix_word(mix));
            leaf(
                &home,
                "mute",
                Value::Switch(send.mute),
                Kind::SendMute(ch.id, mix),
            );
            leaf(&home, "db", db(send), Kind::SendDb(ch.id, mix));
            leaf(
                &home,
                "position",
                position(send),
                Kind::SendPosition(ch.id, mix),
            );
            leaf(
                &home,
                "processed",
                Value::Switch(ch.chain_to(mix)),
                Kind::Processed(ch.id, mix),
            );
        }
    }
    for mix in [MixId::Monitor, MixId::Master] {
        let level = &setup.mix(mix).level;
        let home = address::mix(mix);
        leaf(&home, "mute", Value::Switch(level.mute), Kind::MixMute(mix));
        leaf(&home, "db", db(level), Kind::MixDb(mix));
        leaf(&home, "position", position(level), Kind::MixPosition(mix));
        if mix == MixId::Master {
            leaf(
                &home,
                "preview",
                Value::Switch(setup.preview_master),
                Kind::Preview,
            );
        }
    }
    let strips = setup
        .channels
        .iter()
        .map(|ch| (Strip::Channel(ch.id), &ch.chain))
        .chain([
            (Strip::Mix(MixId::Monitor), &setup.monitor.chain),
            (Strip::Mix(MixId::Master), &setup.master.chain),
        ]);
    for (strip, chain) in strips {
        for (index, insert) in chain.iter().enumerate() {
            let id = insert.instance;
            let home = address::plugin(id);
            leaf(
                &home,
                "name",
                Value::Text(insert.plugin.name.clone()),
                Kind::PluginName(id),
            );
            leaf(
                &home,
                "strip",
                Value::Text(address::strip(strip)),
                Kind::PluginStrip(id),
            );
            leaf(
                &home,
                "slot",
                Value::Number((index + 1) as f64),
                Kind::PluginSlot(id),
            );
            leaf(
                &home,
                "enabled",
                Value::Switch(insert.enabled),
                Kind::Enabled(id),
            );
        }
    }
    out
}

/// One thing a follower is told.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// An address has this value now: it changed, or it is new.
    Set { address: String, value: Value },
    /// A channel or a plugin is no longer there, with every address under it.
    Gone(String),
}

impl Change {
    /// The address the change is about.
    pub fn address(&self) -> &str {
        match self {
            Change::Set { address, .. } | Change::Gone(address) => address,
        }
    }
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Change::Set { address, value } => write!(f, "{address} {value}"),
            Change::Gone(address) => write!(f, "{address} {GONE}"),
        }
    }
}

/// What a reply and an event say in place of a value for something that went.
pub const GONE: &str = "gone";

/// What turned `before` into `after`: first each channel and plugin that went, in the order
/// they were listed, then each address that is new or reads differently, in `after`'s order.
pub fn diff(before: &[Leaf], after: &[Leaf]) -> Vec<Change> {
    let there: HashSet<&str> = after.iter().map(|l| address::node(&l.address)).collect();
    let mut gone: Vec<&str> = Vec::new();
    for leaf in before {
        let node = address::node(&leaf.address);
        if !there.contains(node) && !gone.contains(&node) {
            gone.push(node);
        }
    }
    let was: HashMap<&str, &Value> = before
        .iter()
        .map(|l| (l.address.as_str(), &l.value))
        .collect();
    gone.into_iter()
        .map(|node| Change::Gone(node.to_string()))
        .chain(
            after
                .iter()
                .filter(|l| was.get(l.address.as_str()) != Some(&&l.value))
                .map(|l| Change::Set {
                    address: l.address.clone(),
                    value: l.value.clone(),
                }),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::{Command, SlotRef, apply, example};

    fn lines(setup: &Setup, under: &str) -> Vec<String> {
        leaves(setup)
            .iter()
            .filter(|l| address::covers(under, &l.address))
            .map(Leaf::to_string)
            .collect()
    }

    #[test]
    fn a_channel_reads_its_name_its_switches_and_both_sends() {
        let setup = example::setup();
        // Music: not linked, Monitor at 0.68, Master at 0.58 and muted, MON and MST on. Both
        // faders sit between the taper's -12 dB at 0.56 and -6 dB at 0.70, which is 6 dB over
        // 0.14 of travel: 0.12 up is -12 + 5.14, and 0.02 up is -12 + 0.86.
        assert_eq!(
            lines(&setup, "/ch/3"),
            [
                "/ch/3/name \"Music\"",
                "/ch/3/kind \"apps\"",
                "/ch/3/link false",
                "/ch/3/mute false",
                "/ch/3/monitor/mute false",
                "/ch/3/monitor/db -6.86",
                "/ch/3/monitor/position 0.68",
                "/ch/3/monitor/processed true",
                "/ch/3/master/mute true",
                "/ch/3/master/db -11.14",
                "/ch/3/master/position 0.58",
                "/ch/3/master/processed true",
            ]
        );
        // Mic's Master send is at unity: exactly 0 dB.
        assert!(lines(&setup, "/ch/1/master").contains(&"/ch/1/master/db 0".to_string()));
        assert!(lines(&setup, "/ch/1").contains(&"/ch/1/kind \"hardware\"".to_string()));
    }

    #[test]
    fn the_mixes_read_their_level_and_master_the_ear() {
        let setup = example::setup();
        assert_eq!(
            lines(&setup, "/mix"),
            [
                "/mix/monitor/mute false",
                "/mix/monitor/db 0",
                "/mix/monitor/position 0.84",
                "/mix/master/mute false",
                "/mix/master/db 0",
                "/mix/master/position 0.84",
                "/mix/master/preview false",
            ]
        );
    }

    #[test]
    fn a_plugin_reads_its_name_where_it_is_and_its_bypass() {
        let setup = example::setup();
        assert_eq!(
            lines(&setup, "/fx/4"),
            [
                "/fx/4/name \"OTT\"",
                "/fx/4/strip \"/ch/3\"",
                "/fx/4/slot 1",
                "/fx/4/enabled false",
            ]
        );
        assert_eq!(
            lines(&setup, "/fx/9"),
            [
                "/fx/9/name \"Pro-L 2\"",
                "/fx/9/strip \"/mix/master\"",
                "/fx/9/slot 2",
                "/fx/9/enabled true",
            ]
        );
        // Every plugin of the example, strip by strip.
        let plugins: HashSet<String> = leaves(&setup)
            .iter()
            .filter(|l| address::covers(address::PLUGINS, &l.address))
            .map(|l| address::node(&l.address).to_string())
            .collect();
        assert_eq!(plugins.len(), setup.inserts().count());
    }

    #[test]
    fn both_sends_muted_is_the_channel_muted_and_a_silent_fader_reads_minus_infinity() {
        let mut setup = example::setup();
        let ch = ChannelId(3);
        let _ = apply(
            &mut setup,
            Command::SetMute {
                ch,
                mix: MixId::Monitor,
                mute: true,
            },
        )
        .unwrap();
        let _ = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Monitor,
                position: 0.0,
            },
        )
        .unwrap();
        let music = lines(&setup, "/ch/3");
        assert!(music.contains(&"/ch/3/mute true".to_string()), "{music:?}");
        assert!(
            music.contains(&"/ch/3/monitor/db -inf".to_string()),
            "{music:?}"
        );
        assert!(
            music.contains(&"/ch/3/monitor/position 0".to_string()),
            "{music:?}"
        );
    }

    /// `before` with `changes` made to it, by address.
    fn patched(before: &[Leaf], changes: &[Change]) -> HashMap<String, Value> {
        let mut table: HashMap<String, Value> = before
            .iter()
            .map(|l| (l.address.clone(), l.value.clone()))
            .collect();
        for change in changes {
            match change {
                Change::Set { address, value } => {
                    table.insert(address.clone(), value.clone());
                }
                Change::Gone(node) => table.retain(|address, _| !address::covers(node, address)),
            }
        }
        table
    }

    #[test]
    fn the_changes_between_two_setups_turn_the_first_table_into_the_second() {
        let mut setup = example::setup();
        let removed = setup.channels[2].clone();
        let commands = [
            Command::SetMute {
                ch: ChannelId(1),
                mix: MixId::Master,
                mute: true,
            },
            // Game is linked: both of its sends move.
            Command::SetSend {
                ch: ChannelId(2),
                mix: MixId::Monitor,
                position: 0.5,
            },
            Command::SetInsertEnabled {
                instance: InstanceId(4),
                enabled: true,
            },
            // A plugin moved to another strip keeps its address and says where it is.
            Command::MoveInsert {
                from: SlotRef {
                    strip: Strip::Channel(ChannelId(1)),
                    index: 0,
                },
                to: SlotRef {
                    strip: Strip::Mix(MixId::Master),
                    index: 0,
                },
                copy: false,
            },
            Command::RemoveInsert {
                instance: InstanceId(7),
            },
            Command::RemoveChannel(ChannelId(3)),
            Command::AddChannel {
                kind: ChannelKind::Apps,
            },
            Command::AddInsert {
                strip: Strip::Channel(ChannelId(2)),
                plugin: example::plugin("OTT"),
                at: 0,
            },
            Command::RestoreChannel {
                at: 2,
                channel: removed,
            },
            Command::SetPreview(true),
        ];
        for command in commands {
            let before = leaves(&setup);
            let said = format!("{command:?}");
            let _ = apply(&mut setup, command).unwrap_or_else(|e| panic!("{said}: {e}"));
            let after = leaves(&setup);
            let changes = diff(&before, &after);
            assert!(!changes.is_empty(), "{said} changed nothing");
            let expected: HashMap<String, Value> = after
                .iter()
                .map(|l| (l.address.clone(), l.value.clone()))
                .collect();
            assert_eq!(patched(&before, &changes), expected, "{said}: {changes:?}");
        }
        // Nothing changed: nothing said.
        assert_eq!(diff(&leaves(&setup), &leaves(&setup)), Vec::new());
    }

    #[test]
    fn what_went_is_said_once_as_gone_before_anything_else() {
        let mut setup = example::setup();
        let before = leaves(&setup);
        // Music goes, with its one plugin (instance 4).
        let _ = apply(&mut setup, Command::RemoveChannel(ChannelId(3))).unwrap();
        let said: Vec<String> = diff(&before, &leaves(&setup))
            .iter()
            .map(Change::to_string)
            .collect();
        assert_eq!(said, ["/ch/3 gone", "/fx/4 gone"]);
        // A linked fader says both sends, each with its decibels.
        let before = leaves(&setup);
        let _ = apply(
            &mut setup,
            Command::SetSend {
                ch: ChannelId(2),
                mix: MixId::Monitor,
                position: 0.84,
            },
        )
        .unwrap();
        let said: Vec<String> = diff(&before, &leaves(&setup))
            .iter()
            .map(Change::to_string)
            .collect();
        assert_eq!(
            said,
            [
                "/ch/2/monitor/db 0",
                "/ch/2/monitor/position 0.84",
                "/ch/2/master/db -4.29",
                "/ch/2/master/position 0.74",
            ]
        );
    }
}
