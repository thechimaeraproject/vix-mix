//! Every command's `Applied` against the diff it made. From the first start or the example
//! console, any sequence of commands, valid or not: a refused command changes nothing; an
//! applied one answers an `Applied` whose every part is set exactly when its part of the setup
//! changed; and the setup that results keeps every rule, so it round-trips through
//! `setup.ron`.
//!
//! The parts are written here from the outside, from what each consumer reads: `save` the
//! whole setup, `engine` the snapshot, `graph` what the backend's nodes derive from, `routing`
//! which app goes to which channel, `created` and `removed` the instances in the chains,
//! `channel` the channel that appeared.

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use vixmix_model::persist::{parse, to_ron};
use vixmix_model::taper::UNITY;
use vixmix_model::{
    AppRef, Applied, ChannelId, ChannelKind, Command, DeviceRef, InstanceId, MixId, PluginRef,
    Prefs, Setup, SlotRef, Source, Strip, apply, example, snapshot,
};

// --- What each part of `Applied` answers for -------------------------------------------------

/// What the backend's nodes derive from: each channel in order with its name and its input (an
/// app sink, or a capture on a device), the two mixes' devices, and the quantum asked for.
type GraphView = (
    Vec<(ChannelId, String, Option<Option<DeviceRef>>)>,
    Option<DeviceRef>,
    Option<DeviceRef>,
    u32,
);

fn graph(setup: &Setup) -> GraphView {
    let channels = setup
        .channels
        .iter()
        .map(|c| {
            let input = match &c.source {
                Source::Apps(_) => None,
                Source::Hardware(device) => Some(device.clone()),
            };
            (c.id, c.name.clone(), input)
        })
        .collect();
    (
        channels,
        setup.monitor.device.clone(),
        setup.master.device.clone(),
        setup.prefs.quantum,
    )
}

/// Which app goes to which channel, as a set.
fn routing(setup: &Setup) -> Vec<(ChannelId, Option<String>, Option<String>, String)> {
    let mut pairs: Vec<_> = setup
        .channels
        .iter()
        .flat_map(|c| match &c.source {
            Source::Apps(apps) => apps
                .iter()
                .map(|a| (c.id, a.app_id.clone(), a.binary.clone(), a.name.clone()))
                .collect(),
            Source::Hardware(_) => Vec::new(),
        })
        .collect();
    pairs.sort();
    pairs
}

fn instances(setup: &Setup) -> Vec<InstanceId> {
    setup.inserts().map(|i| i.instance).collect()
}

/// The `Applied` the diff between `before` and `after` calls for.
fn expected(before: &Setup, after: &Setup) -> Applied {
    let (was, now) = (instances(before), instances(after));
    Applied {
        save: before != after,
        engine: snapshot(before) != snapshot(after),
        graph: graph(before) != graph(after),
        routing: routing(before) != routing(after),
        created: now.iter().filter(|i| !was.contains(i)).copied().collect(),
        removed: was.iter().filter(|i| !now.contains(i)).copied().collect(),
        channel: after
            .channels
            .iter()
            .map(|c| c.id)
            .find(|id| before.channel(*id).is_none()),
    }
}

// --- Commands, drawn against whatever setup the sequence has reached -------------------------

/// An index into what exists, modulo its length plus one: the extra one is an id that does
/// not exist, so refusals are drawn too.
type Pick = u8;

#[derive(Clone, Debug)]
enum Seed {
    AddChannel(bool),
    RemoveChannel(Pick),
    Rename(Pick, u8),
    SetSource(Pick, Option<Vec<u8>>, Option<u8>),
    SetSend(Pick, bool, f32),
    SetMute(Pick, bool, bool),
    SetLink(Pick, bool),
    SetChainRouting(Pick, bool, bool),
    AddInsert(Pick, u8, u8),
    RemoveInsert(Pick),
    MoveInsert(Pick, u8, Pick, u8, bool),
    SetInsertEnabled(Pick, bool),
    SetInsertState(Pick, Option<u8>),
    SetMixDevice(bool, Option<u8>),
    SetMixLevel(bool, f32),
    SetMixMute(bool, bool),
    SetPreview(bool),
    SetPrefs(u8, bool),
}

/// The index of each variant, by an exhaustive match: a new `Command` fails to compile here
/// until it has a seed above and a place in [`KINDS`].
fn kind(cmd: &Command) -> usize {
    match cmd {
        Command::AddChannel { .. } => 0,
        Command::RemoveChannel(_) => 1,
        Command::Rename { .. } => 2,
        Command::SetSource { .. } => 3,
        Command::SetSend { .. } => 4,
        Command::SetMute { .. } => 5,
        Command::SetLink { .. } => 6,
        Command::SetChainRouting { .. } => 7,
        Command::AddInsert { .. } => 8,
        Command::RemoveInsert { .. } => 9,
        Command::MoveInsert { .. } => 10,
        Command::SetInsertEnabled { .. } => 11,
        Command::SetInsertState { .. } => 12,
        Command::SetMixDevice { .. } => 13,
        Command::SetMixLevel { .. } => 14,
        Command::SetMixMute { .. } => 15,
        Command::SetPreview(_) => 16,
        Command::SetPrefs(_) => 17,
    }
}

const KINDS: usize = 18;

const NAMES: [&str; 6] = ["Mic", "Game", "  ", "Voice chat", " Music ", "Synth"];

/// Apps that match each other in each of the three ways, and some that match nothing.
fn app(index: u8) -> AppRef {
    let (app_id, binary, name) = match index % 7 {
        0 => (None, Some("firefox"), "Firefox"),
        1 => (Some("org.mozilla.firefox"), Some("firefox"), "Firefox"),
        2 => (
            Some("org.mozilla.Thunderbird"),
            Some("firefox"),
            "Thunderbird",
        ),
        3 => (None, Some("spotify"), "Spotify"),
        4 => (None, None, "Discord"),
        5 => (Some("com.discordapp.Discord"), None, "Discord"),
        _ => (None, Some("steam"), "Steam"),
    };
    AppRef {
        app_id: app_id.map(str::to_string),
        binary: binary.map(str::to_string),
        name: name.to_string(),
    }
}

fn device(index: u8) -> DeviceRef {
    let name = ["alsa.wave", "alsa.scarlett", "alsa.hdmi"][usize::from(index % 3)];
    DeviceRef {
        name: name.to_string(),
        description: name.to_uppercase(),
    }
}

/// Three plugins a setup accepts, one with a bad id and one stored by path.
fn plugin(index: u8) -> PluginRef {
    match index % 5 {
        3 => PluginRef {
            id: "vst3:not-hex".to_string(),
            ..example::plugin("Broken")
        },
        4 => PluginRef {
            bundle: "/usr/lib/vst3/EQ.vst3".to_string(),
            ..example::plugin("EQ")
        },
        n => example::plugin(["Pro-Q 3", "OTT", "TDR Nova"][usize::from(n)]),
    }
}

fn channel(setup: &Setup, pick: Pick) -> ChannelId {
    let n = setup.channels.len();
    match usize::from(pick) % (n + 1) {
        i if i < n => setup.channels[i].id,
        _ => ChannelId(9_999),
    }
}

fn instance(setup: &Setup, pick: Pick) -> InstanceId {
    let all: Vec<InstanceId> = setup.inserts().map(|i| i.instance).collect();
    match usize::from(pick) % (all.len() + 1) {
        i if i < all.len() => all[i],
        _ => InstanceId(9_999),
    }
}

fn strip(setup: &Setup, pick: Pick) -> Strip {
    let n = setup.channels.len();
    match usize::from(pick) % (n + 3) {
        i if i < n => Strip::Channel(setup.channels[i].id),
        i if i == n => Strip::Mix(MixId::Monitor),
        i if i == n + 1 => Strip::Mix(MixId::Master),
        _ => Strip::Channel(ChannelId(9_999)),
    }
}

fn chain_len(setup: &Setup, strip: Strip) -> usize {
    match strip {
        Strip::Channel(ch) => setup.channel(ch).map_or(0, |c| c.chain.len()),
        Strip::Mix(mix) => setup.mix(mix).chain.len(),
    }
}

/// A slot of `strip`, up to one past the last gap so a bad slot is drawn too.
fn slot(setup: &Setup, strip: Strip, index: u8) -> SlotRef {
    SlotRef {
        strip,
        index: usize::from(index) % (chain_len(setup, strip) + 2),
    }
}

fn mix(master: bool) -> MixId {
    if master {
        MixId::Master
    } else {
        MixId::Monitor
    }
}

fn command(seed: &Seed, setup: &Setup) -> Command {
    match seed.clone() {
        Seed::AddChannel(apps) => Command::AddChannel {
            kind: if apps {
                ChannelKind::Apps
            } else {
                ChannelKind::Hardware
            },
        },
        Seed::RemoveChannel(pick) => Command::RemoveChannel(channel(setup, pick)),
        Seed::Rename(pick, name) => Command::Rename {
            ch: channel(setup, pick),
            name: NAMES[usize::from(name) % NAMES.len()].to_string(),
        },
        Seed::SetSource(pick, apps, dev) => Command::SetSource {
            ch: channel(setup, pick),
            source: match apps {
                Some(apps) => Source::Apps(apps.into_iter().map(app).collect()),
                None => Source::Hardware(dev.map(device)),
            },
        },
        Seed::SetSend(pick, master, position) => Command::SetSend {
            ch: channel(setup, pick),
            mix: mix(master),
            position,
        },
        Seed::SetMute(pick, master, mute) => Command::SetMute {
            ch: channel(setup, pick),
            mix: mix(master),
            mute,
        },
        Seed::SetLink(pick, link) => Command::SetLink {
            ch: channel(setup, pick),
            link,
        },
        Seed::SetChainRouting(pick, master, on) => Command::SetChainRouting {
            ch: channel(setup, pick),
            mix: mix(master),
            on,
        },
        Seed::AddInsert(pick, which, at) => {
            let strip = strip(setup, pick);
            Command::AddInsert {
                strip,
                plugin: plugin(which),
                at: slot(setup, strip, at).index,
            }
        }
        Seed::RemoveInsert(pick) => Command::RemoveInsert {
            instance: instance(setup, pick),
        },
        Seed::MoveInsert(from, from_index, to, to_index, copy) => {
            let (from, to) = (strip(setup, from), strip(setup, to));
            Command::MoveInsert {
                from: slot(setup, from, from_index),
                to: slot(setup, to, to_index),
                copy,
            }
        }
        Seed::SetInsertEnabled(pick, enabled) => Command::SetInsertEnabled {
            instance: instance(setup, pick),
            enabled,
        },
        Seed::SetInsertState(pick, state) => Command::SetInsertState {
            instance: instance(setup, pick),
            state: state.map(|n| (0..n % 5).collect()),
        },
        Seed::SetMixDevice(master, dev) => Command::SetMixDevice {
            mix: mix(master),
            device: dev.map(device),
        },
        Seed::SetMixLevel(master, position) => Command::SetMixLevel {
            mix: mix(master),
            position,
        },
        Seed::SetMixMute(master, mute) => Command::SetMixMute {
            mix: mix(master),
            mute,
        },
        Seed::SetPreview(on) => Command::SetPreview(on),
        Seed::SetPrefs(quantum, dirs) => Command::SetPrefs(Prefs {
            quantum: [128, 256, 512, 300][usize::from(quantum % 4)],
            plugin_dirs: if dirs {
                vec!["/opt/vst3".into()]
            } else {
                Vec::new()
            },
        }),
    }
}

fn position() -> impl Strategy<Value = f32> {
    prop_oneof![
        3 => 0.0f32..=1.0,
        1 => Just(0.0f32),
        1 => Just(0.001f32),
        1 => Just(UNITY),
        1 => Just(1.0f32),
        1 => Just(f32::NAN),
        1 => Just(-0.25f32),
        1 => Just(1.5f32),
    ]
}

fn seed() -> impl Strategy<Value = Seed> {
    let pick = any::<u8>;
    prop_oneof![
        2 => any::<bool>().prop_map(Seed::AddChannel),
        1 => pick().prop_map(Seed::RemoveChannel),
        1 => (pick(), any::<u8>()).prop_map(|(c, n)| Seed::Rename(c, n)),
        3 => (
            pick(),
            proptest::option::of(proptest::collection::vec(any::<u8>(), 0..4)),
            proptest::option::of(any::<u8>()),
        )
            .prop_map(|(c, apps, dev)| Seed::SetSource(c, apps, dev)),
        3 => (pick(), any::<bool>(), position()).prop_map(|(c, m, p)| Seed::SetSend(c, m, p)),
        1 => (pick(), any::<bool>(), any::<bool>()).prop_map(|(c, m, x)| Seed::SetMute(c, m, x)),
        1 => (pick(), any::<bool>()).prop_map(|(c, l)| Seed::SetLink(c, l)),
        1 => (pick(), any::<bool>(), any::<bool>())
            .prop_map(|(c, m, on)| Seed::SetChainRouting(c, m, on)),
        3 => (pick(), any::<u8>(), any::<u8>()).prop_map(|(s, p, at)| Seed::AddInsert(s, p, at)),
        1 => pick().prop_map(Seed::RemoveInsert),
        3 => (pick(), any::<u8>(), pick(), any::<u8>(), any::<bool>())
            .prop_map(|(f, fi, t, ti, c)| Seed::MoveInsert(f, fi, t, ti, c)),
        1 => (pick(), any::<bool>()).prop_map(|(i, e)| Seed::SetInsertEnabled(i, e)),
        1 => (pick(), proptest::option::of(any::<u8>()))
            .prop_map(|(i, s)| Seed::SetInsertState(i, s)),
        1 => (any::<bool>(), proptest::option::of(any::<u8>()))
            .prop_map(|(m, d)| Seed::SetMixDevice(m, d)),
        1 => (any::<bool>(), position()).prop_map(|(m, p)| Seed::SetMixLevel(m, p)),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(m, x)| Seed::SetMixMute(m, x)),
        1 => any::<bool>().prop_map(Seed::SetPreview),
        1 => (any::<u8>(), any::<bool>()).prop_map(|(q, d)| Seed::SetPrefs(q, d)),
    ]
}

/// A starting setup and the seeds to run against it.
fn run() -> impl Strategy<Value = (bool, Vec<Seed>)> {
    (any::<bool>(), proptest::collection::vec(seed(), 1..48))
}

/// Applies each seed's command in turn, checking it against the diff; returns which kinds
/// changed the setup at least once.
fn check(example: bool, seeds: &[Seed]) -> Result<[bool; KINDS], TestCaseError> {
    let mut setup = if example {
        example::setup()
    } else {
        Setup::default()
    };
    let mut changed = [false; KINDS];
    for seed in seeds {
        let cmd = command(seed, &setup);
        let before = setup.clone();
        match apply(&mut setup, cmd.clone()) {
            Err(refused) => {
                prop_assert_eq!(
                    &setup,
                    &before,
                    "{:?} was refused ({}) but changed the setup",
                    cmd,
                    refused
                );
                let message = refused.to_string();
                prop_assert!(message.contains(':'), "{} says no fix", message);
            }
            Ok(applied) => {
                prop_assert_eq!(&applied, &expected(&before, &setup), "{:?}", cmd);
                prop_assert_eq!(applied.changed(), applied.save);
                changed[kind(&cmd)] |= applied.save;
                // Whatever commands make, the file keeps and gives back as it was.
                let text = to_ron(&setup);
                match parse(text.as_bytes()) {
                    Ok(read) => prop_assert_eq!(&read, &setup, "after {:?}", cmd),
                    Err(error) => {
                        return Err(TestCaseError::fail(format!(
                            "after {cmd:?} the setup does not read back: {error}"
                        )));
                    }
                }
            }
        }
    }
    Ok(changed)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn every_applied_matches_the_diff_it_made((example, seeds) in run()) {
        check(example, &seeds)?;
    }
}

#[test]
fn the_sequences_change_the_setup_through_every_command() {
    // A fixed sample of the same sequences the property draws: every kind of command must
    // land, or the property above proves nothing about it.
    let mut runner = TestRunner::deterministic();
    let mut changed = [false; KINDS];
    for _ in 0..300 {
        let (example, seeds) = run().new_tree(&mut runner).unwrap().current();
        let landed = check(example, &seeds).unwrap();
        for (seen, now) in changed.iter_mut().zip(landed) {
            *seen |= now;
        }
    }
    let missing: Vec<usize> = (0..KINDS).filter(|k| !changed[*k]).collect();
    assert!(
        missing.is_empty(),
        "no sequence changed the setup through kinds {missing:?}"
    );
}
