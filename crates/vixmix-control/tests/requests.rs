//! What a request can do to a setup, held from the outside: it sends only the commands the
//! console's switches, faders and device lists send, it never changes which channels and
//! plugins there are or what they are, and what it writes reads back.

use proptest::prelude::*;
use vixmix_control::{Change, Console, Kind, Plan, Value, diff, leaves, parse, plan};
use vixmix_model::{Command, DeviceDirection, DeviceInfo, Setup, Source, Strip, apply, example};

/// The example's devices as its host lists them: four inputs, then four outputs.
fn devices() -> Vec<DeviceInfo> {
    let inputs = example::INPUTS
        .iter()
        .map(|d| (d, DeviceDirection::Input, true));
    let outputs = example::OUTPUTS
        .iter()
        .map(|d| (d, DeviceDirection::Output, false));
    inputs
        .chain(outputs)
        .map(|(description, direction, input)| DeviceInfo {
            device: example::device(description, input),
            direction,
        })
        .collect()
}

/// A line that sends `command`'s kind of command, or `None` for a kind no request sends.
///
/// The match has no wildcard: a new `Command` variant fails to compile here until it is given
/// a line, or named among the ones that stay the window's.
fn line_for(command: &Command) -> Option<&'static str> {
    match command {
        Command::SetSend { .. } => Some("set /ch/1/monitor/position 0.5"),
        Command::SetMute { .. } => Some("toggle /ch/1/monitor/mute"),
        Command::SetLink { .. } => Some("toggle /ch/1/link"),
        Command::SetChainRouting { .. } => Some("toggle /ch/1/monitor/processed"),
        Command::SetInsertEnabled { .. } => Some("toggle /fx/1/enabled"),
        Command::SetMixLevel { .. } => Some("set /mix/master/db -6"),
        Command::SetMixMute { .. } => Some("toggle /mix/master/mute"),
        Command::SetPreview(_) => Some("toggle /mix/master/preview"),
        Command::SetMixDevice { .. } => Some("set /mix/master/device/name none"),
        // For a hardware channel's input only; the property below holds it to that.
        Command::SetSource { .. } => Some("set /ch/1/device/name c920-webcam-mic"),
        // The window's: each adds, removes, moves or renames something, or is a preference
        // or a plugin's saved state.
        Command::AddChannel { .. }
        | Command::RemoveChannel(_)
        | Command::Rename { .. }
        | Command::AddInsert { .. }
        | Command::RemoveInsert { .. }
        | Command::MoveInsert { .. }
        | Command::SetInsertState { .. }
        | Command::SetPrefs(_)
        | Command::RestoreChannel { .. }
        | Command::RestoreInsert { .. } => None,
    }
}

/// The ten lines of [`line_for`].
const LINES: [&str; 10] = [
    "set /ch/1/monitor/position 0.5",
    "toggle /ch/1/monitor/mute",
    "toggle /ch/1/link",
    "toggle /ch/1/monitor/processed",
    "toggle /fx/1/enabled",
    "set /mix/master/db -6",
    "toggle /mix/master/mute",
    "toggle /mix/master/preview",
    "set /mix/master/device/name none",
    "set /ch/1/device/name c920-webcam-mic",
];

/// Whether a write may name a value of this kind. No wildcard here either: a new kind of
/// value says whether it can be written.
fn writable(kind: Kind) -> bool {
    match kind {
        Kind::Link(_)
        | Kind::ChannelMute(_)
        | Kind::SendMute(..)
        | Kind::SendDb(..)
        | Kind::SendPosition(..)
        | Kind::Processed(..)
        | Kind::MixMute(_)
        | Kind::MixDb(_)
        | Kind::MixPosition(_)
        | Kind::Preview
        | Kind::Enabled(_)
        | Kind::Device(_) => true,
        Kind::Name(_)
        | Kind::ChannelKind(_)
        | Kind::PluginName(_)
        | Kind::PluginStrip(_)
        | Kind::PluginSlot(_)
        | Kind::DeviceDetail(_)
        | Kind::Listed(_) => false,
    }
}

/// Whether a value says what the console is made of, rather than how it is set: which
/// channels and plugins there are, what each is and where, and the devices the machine has.
/// A request never changes one. (A device's description and whether it is there are neither
/// written nor shape: they follow the device a request may set.)
fn shape(kind: Kind) -> bool {
    match kind {
        Kind::Name(_)
        | Kind::ChannelKind(_)
        | Kind::PluginName(_)
        | Kind::PluginStrip(_)
        | Kind::PluginSlot(_)
        | Kind::Listed(_) => true,
        Kind::Link(_)
        | Kind::ChannelMute(_)
        | Kind::SendMute(..)
        | Kind::SendDb(..)
        | Kind::SendPosition(..)
        | Kind::Processed(..)
        | Kind::MixMute(_)
        | Kind::MixDb(_)
        | Kind::MixPosition(_)
        | Kind::Preview
        | Kind::Enabled(_)
        | Kind::Device(_)
        | Kind::DeviceDetail(_) => false,
    }
}

fn written(console: Console<'_>, line: &str) -> Result<Vec<Command>, String> {
    let request = parse(line)?.ok_or("a blank line")?;
    match plan(console, &request)? {
        Plan::Write(commands) => Ok(commands),
        other => Err(format!("not a write: {other:?}")),
    }
}

#[test]
fn each_of_the_ten_commands_has_a_line_that_sends_it_and_no_other() {
    let setup = example::setup();
    let devices = devices();
    let console = Console {
        setup: &setup,
        devices: &devices,
    };
    let mut kinds = std::collections::HashSet::new();
    for line in LINES {
        let commands = written(console, line).unwrap_or_else(|e| panic!("{line}: {e}"));
        assert_eq!(commands.len(), 1, "{line} sent {commands:?}");
        assert_eq!(
            line_for(&commands[0]),
            Some(line),
            "{line} sent {commands:?}"
        );
        kinds.insert(std::mem::discriminant(&commands[0]));
    }
    assert_eq!(kinds.len(), LINES.len(), "ten lines, ten kinds of command");
}

#[test]
fn every_value_a_write_may_name_reads_back_what_was_written() {
    let start = example::setup();
    let devices = devices();
    let mut tried = 0;
    for leaf in leaves(Console {
        setup: &start,
        devices: &devices,
    }) {
        let (token, expected) = match (&leaf.value, leaf.kind) {
            (Value::Switch(on), _) => (format!("{}", !on), Value::Switch(!on)),
            (Value::Number(_), Kind::SendDb(..) | Kind::MixDb(_)) => {
                ("-12".to_string(), Value::Number(-12.0))
            }
            (Value::Number(_), Kind::SendPosition(..) | Kind::MixPosition(_)) => {
                ("0.5".to_string(), Value::Number(0.5))
            }
            // Another device than the example uses, written as it is described: the last
            // input for a channel, the last output for a mix.
            (Value::Text(_), Kind::Device(strip)) => {
                let input = matches!(strip, Strip::Channel(_));
                let description = if input {
                    example::INPUTS[3]
                } else {
                    example::OUTPUTS[3]
                };
                (
                    description.to_string(),
                    Value::Text(example::device(description, input).name),
                )
            }
            // A slot, a name, a strip, a listed device: read only, below.
            _ => ("1".to_string(), leaf.value.clone()),
        };
        let line = format!("set {} {token}", leaf.address);
        let mut setup = start.clone();
        if !writable(leaf.kind) {
            let console = Console {
                setup: &setup,
                devices: &devices,
            };
            let refused = written(console, &line).expect_err(&line);
            assert!(refused.contains("read only"), "{line} said `{refused}`");
            continue;
        }
        let commands = written(
            Console {
                setup: &setup,
                devices: &devices,
            },
            &line,
        )
        .unwrap_or_else(|e| panic!("{line}: {e}"));
        for command in commands {
            let said = format!("{command:?}");
            let _ = apply(&mut setup, command).unwrap_or_else(|e| panic!("{line}: {said}: {e}"));
        }
        let read = leaves(Console {
            setup: &setup,
            devices: &devices,
        })
        .into_iter()
        .find(|l| l.address == leaf.address)
        .unwrap_or_else(|| panic!("{} went", leaf.address));
        assert_eq!(read.value, expected, "{line}");
        tried += 1;
    }
    // The example: six channels of ten writable values, seven on the mixes, nine plugins,
    // and a device on each of the two hardware channels and the two mixes.
    assert_eq!(tried, 6 * 10 + 7 + 9 + 4);
}

/// Every address the example holds, as the mixer prints it and as a person might write it,
/// and some that name nothing.
fn addresses() -> Vec<String> {
    let setup = example::setup();
    let devices = devices();
    let mut all: Vec<String> = leaves(Console {
        setup: &setup,
        devices: &devices,
    })
    .into_iter()
    .map(|l| l.address)
    .collect();
    all.extend(
        [
            "/",
            "/ch",
            "/ch/mic",
            "/ch/mic/mute",
            "/ch/music/master/db",
            "/ch/voice-chat/monitor/position",
            "/ch/mic/fx",
            "/ch/mic/fx/2/enabled",
            "/ch/mic/fx/pro-l-2/enabled",
            "/mix/master/fx/pro-q-3/enabled",
            "/mix/monitor",
            "/fx",
            "/fx/4",
            "/ch/mic/device",
            "/ch/mic/device/name",
            // An app channel has no device: never a way to make it a hardware one.
            "/ch/game/device/name",
            "/ch/music/device",
            "/mix/monitor/device/name",
            "/mix/master/device/name",
            "/in",
            "/out/built-in-speakers",
            "/out/built-in-speakers/name",
            "/ch/9/mute",
            "/ch/1/monitor/volume",
            "/fx/99/enabled",
            "/out/nowhere",
            "nowhere",
        ]
        .map(str::to_string),
    );
    all
}

fn lines() -> impl Strategy<Value = String> {
    let verbs = prop::sample::select(vec![
        "set", "toggle", "nudge", "get", "watch", "unwatch", "",
    ]);
    let values = prop::sample::select(vec![
        "",
        "true",
        "false",
        "on",
        "0",
        "1",
        "0.5",
        "-12",
        "6",
        "7",
        "-inf",
        "inf",
        "-2",
        "0.05",
        "-200",
        "1.5",
        "-0.1",
        "loud",
        "\"Name\"",
        "none",
        "built-in-speakers",
        "c920-webcam-mic",
        "Wave:3",
        "alsa_output.hdmi___lg_ultragear",
        "\"Scarlett 2i2 \u{b7} In 1\"",
        "\"unclosed",
    ]);
    (verbs, prop::sample::select(addresses()), values)
        .prop_map(|(verb, address, value)| format!("{verb} {address} {value}"))
}

proptest! {
    // No file of past failures beside the test: a failure prints the run that made it.
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Any run of requests, well formed or not: each either is refused with a message, or
    /// sends only the ten commands, which the model takes or refuses; a channel's source is
    /// only ever set to a hardware input; and nothing a request did added, removed, moved or
    /// renamed anything, or made an app channel a hardware one.
    #[test]
    fn a_request_sends_only_the_ten_commands_and_never_changes_the_consoles_shape(
        run in prop::collection::vec(lines(), 1..24)
    ) {
        let mut setup = example::setup();
        let devices = devices();
        for line in run {
            let request = match parse(&line) {
                Ok(Some(request)) => request,
                Ok(None) => continue,
                Err(refused) => {
                    prop_assert!(!refused.is_empty(), "{line} was refused without a word");
                    continue;
                }
            };
            let console = Console { setup: &setup, devices: &devices };
            let commands = match plan(console, &request) {
                Ok(Plan::Write(commands)) => commands,
                Ok(_) => continue,
                Err(refused) => {
                    prop_assert!(!refused.is_empty(), "{line} was refused without a word");
                    continue;
                }
            };
            prop_assert!(!commands.is_empty(), "{line} is a write that sends nothing");
            let before = leaves(console);
            for command in commands {
                prop_assert!(line_for(&command).is_some(), "{line} sent {command:?}");
                if let Command::SetSource { source, .. } = &command {
                    prop_assert!(
                        matches!(source, Source::Hardware(_)),
                        "{line} sent {command:?}"
                    );
                }
                // The model may refuse (a position past 1, Monitor with no device); its
                // refusal changes nothing.
                let _ = apply(&mut setup, command);
            }
            let after = leaves(Console { setup: &setup, devices: &devices });
            for change in diff(&before, &after) {
                let Change::Set { address, .. } = &change else {
                    return Err(TestCaseError::fail(format!("{line} made {change}")));
                };
                let kind = after.iter().find(|l| &l.address == address).map(|l| l.kind);
                prop_assert!(
                    kind.is_some_and(|kind| !shape(kind)),
                    "{line} changed {address}"
                );
            }
        }
    }
}

/// The lines above are written against the example: its hardware channels are Mic and Synth.
#[test]
fn the_examples_hardware_channels_are_mic_and_synth() {
    let setup: Setup = example::setup();
    let hardware: Vec<&str> = setup
        .channels
        .iter()
        .filter(|c| matches!(c.source, Source::Hardware(_)))
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(hardware, ["Mic", "Synth"]);
}
