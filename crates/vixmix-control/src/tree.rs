//! Every address of a setup with its value, and what changed between two setups.
//!
//! [`leaves`] is the one list the protocol is read from: what `get` prints, what a write may
//! name and what it sends ([`Kind`]), and what a follower is told ([`diff`]). A channel's
//! plugins are not under the channel: a plugin keeps its address when it is moved to another
//! strip, and says where it is in `strip` and `slot`. The devices the machine has now are
//! listed last, and are the one part that is not the setup's ([`Console`]).

use std::collections::{HashMap, HashSet};
use std::fmt;

use vixmix_model::taper::position_to_db;
use vixmix_model::{
    ChannelId, ChannelKind, DeviceDirection, DeviceRef, InstanceId, MixId, Send, Strip,
};

use crate::address;
use crate::console::Console;
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
    /// The device a strip uses, by its node name: the output a mix plays to, the input a
    /// hardware channel captures. Empty text when it has none.
    Device(Strip),
    /// That device's description, and whether it is there now. Read only: they follow the
    /// device.
    DeviceDetail(Strip),
    /// A device the machine has now: its node name, its description. Read only.
    Listed(DeviceDirection),
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

/// Every address of a console with its value: the channels left to right, then Monitor and
/// Master, then every plugin strip by strip in the same order, then the devices the machine
/// has now, inputs before outputs.
pub fn leaves<'a>(console: impl Into<Console<'a>>) -> Vec<Leaf> {
    let console = console.into();
    let setup = console.setup;
    let mut out = Vec::new();
    for ch in &setup.channels {
        let home = address::channel(ch.id);
        leaf(
            &mut out,
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
            &mut out,
            &home,
            "kind",
            Value::Text(kind.to_string()),
            Kind::ChannelKind(ch.id),
        );
        leaf(
            &mut out,
            &home,
            "link",
            Value::Switch(ch.link),
            Kind::Link(ch.id),
        );
        leaf(
            &mut out,
            &home,
            "mute",
            Value::Switch(ch.monitor.mute && ch.master.mute),
            Kind::ChannelMute(ch.id),
        );
        for mix in [MixId::Monitor, MixId::Master] {
            let send = ch.send(mix);
            let home = format!("{home}/{}", address::mix_word(mix));
            leaf(
                &mut out,
                &home,
                "mute",
                Value::Switch(send.mute),
                Kind::SendMute(ch.id, mix),
            );
            leaf(&mut out, &home, "db", db(send), Kind::SendDb(ch.id, mix));
            leaf(
                &mut out,
                &home,
                "position",
                position(send),
                Kind::SendPosition(ch.id, mix),
            );
            leaf(
                &mut out,
                &home,
                "processed",
                Value::Switch(ch.chain_to(mix)),
                Kind::Processed(ch.id, mix),
            );
        }
        device_leaves(&mut out, &console, &home, Strip::Channel(ch.id));
    }
    for mix in [MixId::Monitor, MixId::Master] {
        let level = &setup.mix(mix).level;
        let home = address::mix(mix);
        leaf(
            &mut out,
            &home,
            "mute",
            Value::Switch(level.mute),
            Kind::MixMute(mix),
        );
        leaf(&mut out, &home, "db", db(level), Kind::MixDb(mix));
        leaf(
            &mut out,
            &home,
            "position",
            position(level),
            Kind::MixPosition(mix),
        );
        if mix == MixId::Master {
            leaf(
                &mut out,
                &home,
                "preview",
                Value::Switch(setup.preview_master),
                Kind::Preview,
            );
        }
        device_leaves(&mut out, &console, &home, Strip::Mix(mix));
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
                &mut out,
                &home,
                "name",
                Value::Text(insert.plugin.name.clone()),
                Kind::PluginName(id),
            );
            leaf(
                &mut out,
                &home,
                "strip",
                Value::Text(address::strip(strip)),
                Kind::PluginStrip(id),
            );
            leaf(
                &mut out,
                &home,
                "slot",
                Value::Number((index + 1) as f64),
                Kind::PluginSlot(id),
            );
            leaf(
                &mut out,
                &home,
                "enabled",
                Value::Switch(insert.enabled),
                Kind::Enabled(id),
            );
        }
    }
    for direction in [DeviceDirection::Input, DeviceDirection::Output] {
        for (word, device) in console.listed(direction) {
            let home = address::device(direction, &word);
            let listed = Kind::Listed(direction);
            leaf(
                &mut out,
                &home,
                "name",
                Value::Text(device.name.clone()),
                listed,
            );
            leaf(
                &mut out,
                &home,
                "description",
                Value::Text(device.description.clone()),
                listed,
            );
        }
    }
    out
}

fn leaf(out: &mut Vec<Leaf>, home: &str, word: &str, value: Value, kind: Kind) {
    out.push(Leaf {
        address: format!("{home}/{word}"),
        value,
        kind,
    });
}

/// The three values of the device a strip uses, under `device`: its node name, its
/// description and whether it is there now. Nothing for a strip with no device of its own (an
/// app channel).
fn device_leaves(out: &mut Vec<Leaf>, console: &Console<'_>, home: &str, strip: Strip) {
    let Some((direction, device)) = console.device(strip) else {
        return;
    };
    let home = format!("{home}/device");
    let text = |pick: fn(&DeviceRef) -> &String| {
        Value::Text(device.map(pick).cloned().unwrap_or_default())
    };
    leaf(out, &home, "name", text(|d| &d.name), Kind::Device(strip));
    leaf(
        out,
        &home,
        "description",
        text(|d| &d.description),
        Kind::DeviceDetail(strip),
    );
    let present = device.is_some_and(|d| console.present(direction, d));
    leaf(
        out,
        &home,
        "present",
        Value::Switch(present),
        Kind::DeviceDetail(strip),
    );
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
    use vixmix_model::{Command, Setup, SlotRef, apply, example};

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
                "/mix/monitor/device/name \"alsa_output.scarlett_2i2___phones\"",
                "/mix/monitor/device/description \"Scarlett 2i2 \u{b7} Phones\"",
                "/mix/monitor/device/present false",
                "/mix/master/mute false",
                "/mix/master/db 0",
                "/mix/master/position 0.84",
                "/mix/master/preview false",
                "/mix/master/device/name \"alsa_output.scarlett_2i2___out_1_2\"",
                "/mix/master/device/description \"Scarlett 2i2 \u{b7} Out 1\u{2013}2\"",
                "/mix/master/device/present false",
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

    #[test]
    fn a_strip_reads_the_device_it_uses_and_whether_it_is_there() {
        let mut setup = example::setup();
        let devices = crate::console::example_devices();
        let with =
            |setup: &Setup, devices: &[vixmix_model::DeviceInfo], under: &str| -> Vec<String> {
                leaves(Console { setup, devices })
                    .iter()
                    .filter(|l| address::covers(under, &l.address))
                    .map(Leaf::to_string)
                    .collect()
            };
        // Mic captures the Wave:3, which is plugged in.
        assert_eq!(
            with(&setup, &devices, "/ch/1/device"),
            [
                "/ch/1/device/name \"alsa_input.wave_3\"",
                "/ch/1/device/description \"Wave:3\"",
                "/ch/1/device/present true",
            ]
        );
        // Unplugged: the setup still names it, and it is not there.
        let without: Vec<_> = devices
            .iter()
            .filter(|d| d.device.description != "Wave:3")
            .cloned()
            .collect();
        assert_eq!(
            with(&setup, &without, "/ch/1/device"),
            [
                "/ch/1/device/name \"alsa_input.wave_3\"",
                "/ch/1/device/description \"Wave:3\"",
                "/ch/1/device/present false",
            ]
        );
        // An app channel has no device of its own.
        assert_eq!(with(&setup, &devices, "/ch/2/device"), Vec::<String>::new());
        // Master with no device, and a hardware channel with none picked: empty text.
        let _ = apply(
            &mut setup,
            Command::SetMixDevice {
                mix: MixId::Master,
                device: None,
            },
        )
        .unwrap();
        assert_eq!(
            with(&setup, &devices, "/mix/master/device"),
            [
                "/mix/master/device/name \"\"",
                "/mix/master/device/description \"\"",
                "/mix/master/device/present false",
            ]
        );
    }

    #[test]
    fn the_devices_the_machine_has_are_listed_last_inputs_before_outputs() {
        let setup = example::setup();
        let devices = crate::console::example_devices();
        let table = leaves(Console {
            setup: &setup,
            devices: &devices,
        });
        let listed: Vec<String> = table
            .iter()
            .filter(|l| matches!(l.kind, Kind::Listed(_)))
            .map(Leaf::to_string)
            .collect();
        assert_eq!(
            listed.len(),
            16,
            "eight devices, a name and a description each"
        );
        assert_eq!(
            listed[..2],
            [
                "/in/alsa-input-wave-3/name \"alsa_input.wave_3\"",
                "/in/alsa-input-wave-3/description \"Wave:3\"",
            ]
        );
        assert_eq!(
            listed[14..],
            [
                "/out/alsa-output-built-in-speakers/name \"alsa_output.built_in_speakers\"",
                "/out/alsa-output-built-in-speakers/description \"Built-in speakers\"",
            ]
        );
        // Nothing follows them, and a setup alone lists none.
        assert!(matches!(table.last().unwrap().kind, Kind::Listed(_)));
        assert!(
            !leaves(&setup)
                .iter()
                .any(|l| matches!(l.kind, Kind::Listed(_)))
        );
    }

    #[test]
    fn a_device_unplugged_is_gone_and_the_strip_that_uses_it_says_it_is_not_there() {
        let setup = example::setup();
        let devices = crate::console::example_devices();
        // Monitor's phones are unplugged.
        let without: Vec<_> = devices
            .iter()
            .filter(|d| d.device.description != example::OUTPUTS[0])
            .cloned()
            .collect();
        let before = leaves(Console {
            setup: &setup,
            devices: &devices,
        });
        let after = leaves(Console {
            setup: &setup,
            devices: &without,
        });
        let said: Vec<String> = diff(&before, &after)
            .iter()
            .map(Change::to_string)
            .collect();
        assert_eq!(
            said,
            [
                "/out/alsa-output-scarlett-2i2-phones gone",
                "/mix/monitor/device/present false",
            ]
        );
        // Plugged back: listed again, and there.
        let said: Vec<String> = diff(&after, &before)
            .iter()
            .map(Change::to_string)
            .collect();
        assert_eq!(
            said,
            [
                "/mix/monitor/device/present true",
                "/out/alsa-output-scarlett-2i2-phones/name \"alsa_output.scarlett_2i2___phones\"",
                "/out/alsa-output-scarlett-2i2-phones/description \"Scarlett 2i2 \u{b7} Phones\"",
            ]
        );
    }
}
