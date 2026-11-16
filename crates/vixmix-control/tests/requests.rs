//! What a request can do to a setup, held from the outside: it sends only the commands the
//! console's switches and faders send, it never changes which channels and plugins there are,
//! and what it writes reads back.

use proptest::prelude::*;
use vixmix_control::{Change, Kind, Plan, Value, diff, leaves, parse, plan};
use vixmix_model::{Command, Setup, apply, example};

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
        // The window's: each adds, removes, moves or renames something, or is a device, a
        // source, a preference or a plugin's saved state.
        Command::AddChannel { .. }
        | Command::RemoveChannel(_)
        | Command::Rename { .. }
        | Command::SetSource { .. }
        | Command::AddInsert { .. }
        | Command::RemoveInsert { .. }
        | Command::MoveInsert { .. }
        | Command::SetInsertState { .. }
        | Command::SetMixDevice { .. }
        | Command::SetPrefs(_)
        | Command::RestoreChannel { .. }
        | Command::RestoreInsert { .. } => None,
    }
}

/// The eight lines of [`line_for`].
const LINES: [&str; 8] = [
    "set /ch/1/monitor/position 0.5",
    "toggle /ch/1/monitor/mute",
    "toggle /ch/1/link",
    "toggle /ch/1/monitor/processed",
    "toggle /fx/1/enabled",
    "set /mix/master/db -6",
    "toggle /mix/master/mute",
    "toggle /mix/master/preview",
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
        | Kind::Enabled(_) => true,
        Kind::Name(_)
        | Kind::ChannelKind(_)
        | Kind::PluginName(_)
        | Kind::PluginStrip(_)
        | Kind::PluginSlot(_) => false,
    }
}

fn written(setup: &Setup, line: &str) -> Result<Vec<Command>, String> {
    let request = parse(line)?.ok_or("a blank line")?;
    match plan(setup, &request)? {
        Plan::Write(commands) => Ok(commands),
        other => Err(format!("not a write: {other:?}")),
    }
}

#[test]
fn each_of_the_eight_commands_has_a_line_that_sends_it_and_no_other() {
    let setup = example::setup();
    let mut kinds = std::collections::HashSet::new();
    for line in LINES {
        let commands = written(&setup, line).unwrap_or_else(|e| panic!("{line}: {e}"));
        assert_eq!(commands.len(), 1, "{line} sent {commands:?}");
        assert_eq!(
            line_for(&commands[0]),
            Some(line),
            "{line} sent {commands:?}"
        );
        kinds.insert(std::mem::discriminant(&commands[0]));
    }
    assert_eq!(
        kinds.len(),
        LINES.len(),
        "eight lines, eight kinds of command"
    );
}

#[test]
fn every_value_a_write_may_name_reads_back_what_was_written() {
    let start = example::setup();
    let mut tried = 0;
    for leaf in leaves(&start) {
        let (token, expected) = match (&leaf.value, leaf.kind) {
            (Value::Switch(on), _) => (format!("{}", !on), Value::Switch(!on)),
            (Value::Number(_), Kind::SendDb(..) | Kind::MixDb(_)) => {
                ("-12".to_string(), Value::Number(-12.0))
            }
            (Value::Number(_), Kind::SendPosition(..) | Kind::MixPosition(_)) => {
                ("0.5".to_string(), Value::Number(0.5))
            }
            // A slot, a name, a strip: read only, below.
            _ => ("1".to_string(), leaf.value.clone()),
        };
        let line = format!("set {} {token}", leaf.address);
        let mut setup = start.clone();
        if !writable(leaf.kind) {
            let refused = written(&setup, &line).expect_err(&line);
            assert!(refused.contains("read only"), "{line} said `{refused}`");
            continue;
        }
        for command in written(&setup, &line).unwrap_or_else(|e| panic!("{line}: {e}")) {
            let said = format!("{command:?}");
            let _ = apply(&mut setup, command).unwrap_or_else(|e| panic!("{line}: {said}: {e}"));
        }
        let read = leaves(&setup)
            .into_iter()
            .find(|l| l.address == leaf.address)
            .unwrap_or_else(|| panic!("{} went", leaf.address));
        assert_eq!(read.value, expected, "{line}");
        tried += 1;
    }
    // The example: six channels of ten writable values, seven on the mixes, nine plugins.
    assert_eq!(tried, 6 * 10 + 7 + 9);
}

/// Every address the example holds, as the mixer prints it and as a person might write it,
/// and some that name nothing.
fn addresses() -> Vec<String> {
    let mut all: Vec<String> = leaves(&example::setup())
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
            "/ch/9/mute",
            "/ch/1/monitor/volume",
            "/fx/99/enabled",
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
        "", "true", "false", "on", "0", "1", "0.5", "-12", "6", "7", "-inf", "inf", "-2", "0.05",
        "-200", "1.5", "-0.1", "loud", "\"Name\"",
    ]);
    (verbs, prop::sample::select(addresses()), values)
        .prop_map(|(verb, address, value)| format!("{verb} {address} {value}"))
}

proptest! {
    // No file of past failures beside the test: a failure prints the run that made it.
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Any run of requests, well formed or not: each either is refused with a message, or
    /// sends only the eight commands, which the model takes or refuses; and nothing a request
    /// did added, removed, moved or renamed anything.
    #[test]
    fn a_request_sends_only_the_eight_commands_and_never_changes_the_consoles_shape(
        run in prop::collection::vec(lines(), 1..24)
    ) {
        let mut setup = example::setup();
        for line in run {
            let request = match parse(&line) {
                Ok(Some(request)) => request,
                Ok(None) => continue,
                Err(refused) => {
                    prop_assert!(!refused.is_empty(), "{line} was refused without a word");
                    continue;
                }
            };
            let commands = match plan(&setup, &request) {
                Ok(Plan::Write(commands)) => commands,
                Ok(_) => continue,
                Err(refused) => {
                    prop_assert!(!refused.is_empty(), "{line} was refused without a word");
                    continue;
                }
            };
            prop_assert!(!commands.is_empty(), "{line} is a write that sends nothing");
            let before = leaves(&setup);
            for command in commands {
                prop_assert!(line_for(&command).is_some(), "{line} sent {command:?}");
                // The model may refuse (a position past 1); its refusal changes nothing.
                let _ = apply(&mut setup, command);
            }
            let after = leaves(&setup);
            for change in diff(&before, &after) {
                let Change::Set { address, .. } = &change else {
                    return Err(TestCaseError::fail(format!("{line} made {change}")));
                };
                let kind = after.iter().find(|l| &l.address == address).map(|l| l.kind);
                prop_assert!(kind.is_some_and(writable), "{line} changed {address}");
            }
        }
    }
}
