//! The real host against a real daemon: the headless PipeWire and WirePlumber that
//! `scripts/ci/headless-pipewire.sh` starts (CI's Linux leg, the container in
//! docs/cookbook/run-headless-pipewire.md), with its null sink and null source. This is
//! handoff 06's acceptance flow without a person: the host boots with a setup that names an
//! app, a plugin and an output; a `pw-play` stream appears, is matched to its channel and
//! routed into that channel's sink, plays through the plugin to Monitor on the null sink and
//! to the Master source, which `pw-record` records; the host quits, clearing its routing; and
//! a relaunch gives back the same setup, plugin state included, and the app in its channel.
//!
//! Without the script's marker the test says how to start the daemon and passes without
//! touching anything (a desktop session must never get its routings), except under `CI`,
//! where a missing daemon fails. Every realtime callback runs inside `assert_no_alloc`, and
//! this binary installs its allocator: an allocation on the data thread, the test plugin's
//! processing included, aborts the run.

#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use assert_no_alloc::AllocDisabler;
use vixmix_host::{AudioBackend, BackendState, Host, RealHost};
use vixmix_model::{
    AppRef, ChannelKind, Command as Cmd, DeviceRef, MixId, Setup, Source, Strip, apply,
};
use vixmix_pipewire::{channel_node_name, master_node_name, monitor_out_node_name};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const NULL_SINK: &str = "vixmix-ci-null-sink";
/// The host's `application.name` and node-name prefix, its own so no other test meets it.
const APP: &str = "vixmix-host-it";
/// The player's `application.name` and `node.name`.
const PLAYER: &str = "vixmix-host-music";
const RATE: usize = 48_000;
/// The player's sine, and the test plugin's gain on its channel.
const AMPLITUDE: f32 = 0.5;
const GAIN: f64 = 0.5;
const HEARD: f32 = AMPLITUDE * GAIN as f32;
const HOW: &str = "the host's integration test needs the headless daemon: run `source scripts/ci/headless-pipewire.sh` in this shell first (docs/cookbook/run-headless-pipewire.md); it never runs against a desktop session";

/// Whether this shell has the headless daemon; says how to start it when it has not.
fn headless() -> bool {
    if std::env::var_os("VIXMIX_HEADLESS_PIPEWIRE").is_none() {
        assert!(std::env::var_os("CI").is_none(), "{HOW}");
        eprintln!("skipped: {HOW}");
        return false;
    }
    true
}

/// A child process killed when the test ends, however it ends.
struct Proc(Child);

impl Proc {
    fn spawn(program: &str, args: &[&str]) -> Proc {
        let child = Command::new(program)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("{program} did not start ({e}): {HOW}"));
        Proc(child)
    }

    /// Asks it to stop as Ctrl-C would (so pw-record finishes its file) and waits.
    fn interrupt(mut self) {
        let _ = Command::new("kill")
            .args(["-INT", &self.0.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.0.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{program} did not start ({e}): {HOW}"));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Polls `check` every 50 ms for up to `timeout`.
fn eventually(timeout: Duration, check: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if check() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Polls the host every 20 ms, as the window would, until `check` holds or `timeout` passes.
fn poll_until(
    host: &mut RealHost,
    timeout: Duration,
    check: impl Fn(&mut RealHost) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let _ = host.poll();
        if check(host) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether some port of the node `from` is linked to some port of the node `to`.
fn linked(from: &str, to: &str) -> bool {
    let listing = run("pw-link", &["-l"]);
    let mut port = "";
    for line in listing.lines() {
        if !line.starts_with(char::is_whitespace) {
            port = line;
        } else if port.starts_with(&format!("{from}:"))
            && line.trim().starts_with(&format!("|-> {to}:"))
        {
            return true;
        }
    }
    false
}

/// The id of the node named `name`, from `pw-cli ls Node`.
fn node_id(name: &str) -> Option<u32> {
    let listing = run("pw-cli", &["ls", "Node"]);
    let mut current = None;
    for line in listing.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("id ") {
            current = rest.split(',').next().and_then(|id| id.trim().parse().ok());
        } else if line == format!("node.name = \"{name}\"") {
            return current;
        }
    }
    None
}

/// The `object.serial` of the node named `name`, from `pw-cli ls Node`.
fn node_serial(name: &str) -> Option<String> {
    let listing = run("pw-cli", &["ls", "Node"]);
    let mut serial = None;
    for line in listing.lines() {
        let line = line.trim();
        if line.starts_with("id ") {
            serial = None;
        } else if let Some(rest) = line.strip_prefix("object.serial = ") {
            serial = Some(rest.trim_matches('"').to_string());
        } else if line == format!("node.name = \"{name}\"") {
            return serial;
        }
    }
    None
}

/// The `target.object` the `default` metadata holds for `subject`.
fn metadata_target(subject: u32) -> Option<String> {
    let prefix = format!("id:{subject} key:'target.object' value:'");
    run("pw-metadata", &["-n", "default"])
        .lines()
        .find_map(|line| {
            let rest = line.trim_start_matches("update: ").strip_prefix(&prefix)?;
            Some(rest.split('\'').next()?.to_string())
        })
}

/// A 32-bit float stereo WAV at 48 kHz of a 440 Hz sine at [`AMPLITUDE`].
fn sine_wav(dir: &Path, seconds: f32) -> PathBuf {
    let path = dir.join("sine.wav");
    let frames = (seconds * RATE as f32) as usize;
    let data = frames * 2 * 4;
    let mut out = Vec::with_capacity(44 + data);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&(RATE as u32).to_le_bytes());
    out.extend_from_slice(&(RATE as u32 * 8).to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    for i in 0..frames {
        let s = ((i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * AMPLITUDE)
            .to_le_bytes();
        out.extend_from_slice(&s);
        out.extend_from_slice(&s);
    }
    std::fs::write(&path, out).unwrap();
    path
}

/// The largest absolute sample of the left channel of a float WAV pw-record wrote, past its
/// first tenth of a second.
fn recorded_peak(path: &Path) -> f32 {
    let bytes = std::fs::read(path).unwrap_or_default();
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8;
        if id == b"data" {
            let end = if size == 0 || at + size > bytes.len() {
                bytes.len()
            } else {
                at + size
            };
            return bytes[at..end]
                .chunks_exact(8)
                .skip(RATE / 10)
                .map(|frame| f32::from_le_bytes(frame[..4].try_into().unwrap()).abs())
                .fold(0.0, f32::max);
        }
        at += size + (size & 1);
    }
    0.0
}

/// Records `target` (a sink's monitor when `sink`) for `seconds` while polling the host; the
/// peak it heard.
fn record(host: &mut RealHost, target: &str, sink: bool, file: &Path, seconds: f32) -> f32 {
    let mut args = vec![
        "--target",
        target,
        "--format",
        "f32",
        "--rate",
        "48000",
        "--channels",
        "2",
    ];
    if sink {
        args.extend(["-P", "{ stream.capture.sink = true }"]);
    }
    args.push(file.to_str().unwrap());
    let recorder = Proc::spawn("pw-record", &args);
    let until = Instant::now() + Duration::from_secs_f32(seconds);
    while Instant::now() < until {
        let _ = host.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    recorder.interrupt();
    recorded_peak(file)
}

fn near(found: f32, expected: f32) -> bool {
    (found - expected).abs() < 0.01
}

#[test]
fn an_app_plays_through_its_channel_to_monitor_and_master_and_comes_back_after_a_relaunch() {
    if !headless() {
        return;
    }
    let plugins = common::plugin_dir("headless");
    let config = common::config_dir("headless");
    let dir = common::scratch("headless");

    // The setup the user left: Music, an app channel that takes the player, through the test
    // plugin at half gain; Monitor on the null sink; Master on no device (still a source).
    let host = common::boot(&config, &plugins, AudioBackend::Null);
    let plugin = common::test_plugin(&host);
    drop(host);
    let mut setup = Setup::default();
    let music = apply(
        &mut setup,
        Cmd::AddChannel {
            kind: ChannelKind::Apps,
        },
    )
    .unwrap()
    .channel
    .unwrap();
    let instance = apply(
        &mut setup,
        Cmd::AddInsert {
            strip: Strip::Channel(music),
            plugin,
            at: 0,
        },
    )
    .unwrap()
    .created[0];
    for cmd in [
        Cmd::Rename {
            ch: music,
            name: "Music".into(),
        },
        Cmd::SetSource {
            ch: music,
            source: Source::Apps(vec![AppRef {
                app_id: None,
                binary: None,
                name: PLAYER.into(),
            }]),
        },
        Cmd::SetInsertState {
            instance,
            state: Some(common::gain_state(GAIN)),
        },
        Cmd::SetMixDevice {
            mix: MixId::Monitor,
            device: Some(DeviceRef {
                name: NULL_SINK.into(),
                description: "vix-mix CI null sink".into(),
            }),
        },
    ] {
        let _ = apply(&mut setup, cmd).unwrap();
    }
    common::write_setup(&config, &setup);
    let at = setup.channels.iter().position(|c| c.id == music).unwrap();
    let sink = channel_node_name(APP, music, "Music");
    let system = AudioBackend::System { app_name: APP };

    // Boot: the backend is kept, and runs once Monitor's output plays to the null sink.
    let mut host = common::boot(&config, &plugins, system);
    assert!(
        poll_until(&mut host, Duration::from_secs(5), |h| matches!(
            h.backend(),
            BackendState::Running { .. }
        )),
        "the host did not run on the daemon: {:?}",
        host.backend()
    );
    let stats = host.stats();
    assert_eq!((stats.rate, stats.quantum), (48_000, 256), "{stats:?}");
    assert!(linked(&monitor_out_node_name(APP), NULL_SINK));

    // The player appears, is matched to Music and routed into its sink, and plays through
    // the plugin.
    let _player = Proc::spawn(
        "pw-play",
        &[
            "-P",
            &format!("{{ application.name = \"{PLAYER}\" node.name = \"{PLAYER}\" }}"),
            "--media-role",
            PLAYER,
            sine_wav(&dir, 30.0).to_str().unwrap(),
        ],
    );
    assert!(
        poll_until(&mut host, Duration::from_secs(5), |h| h
            .apps()
            .iter()
            .any(|p| p.app.name == PLAYER && p.running)),
        "the player was not listed as playing: {:?}",
        host.apps()
    );
    assert!(
        poll_until(&mut host, Duration::from_secs(5), |_| linked(PLAYER, &sink)),
        "the player was not routed into {sink}:\n{}",
        run("pw-link", &["-l"])
    );
    assert!(
        poll_until(&mut host, Duration::from_secs(3), |h| near(
            h.meters().channels[at].monitor.left,
            HEARD
        )),
        "Music's Monitor send did not read the sine through the plugin: {:?}",
        host.meters()
    );

    // Monitor's output reaches the null sink; the Master source is recordable.
    let heard = record(&mut host, NULL_SINK, true, &dir.join("monitor.wav"), 1.5);
    assert!(near(heard, HEARD), "the null sink heard {heard}");
    let master = master_node_name(APP);
    let heard = record(&mut host, &master, false, &dir.join("master.wav"), 1.5);
    assert!(near(heard, HEARD), "{master} gave {heard}");

    // Quit: the routing is cleared (the app's target no longer names our sink) and the
    // setup is kept with the plugin's state.
    let node = node_id(PLAYER).expect("the player is a node");
    let ours = node_serial(&sink).expect("the sink has a serial");
    assert_eq!(metadata_target(node).as_deref(), Some(ours.as_str()));
    let before = host.setup().clone();
    drop(host);
    assert!(
        eventually(Duration::from_secs(2), || metadata_target(node).as_deref()
            != Some(ours.as_str())),
        "the host's routing outlived it:\n{}",
        run("pw-metadata", &["-n", "default"])
    );
    // The player goes back to the default device and plays there while the mixer is closed,
    // as Spotify would on the headphones. (Relaunched within milliseconds, while the graph
    // was still moving to the player's own quantum, `pw-play` in the container was seen to
    // break: its node erred every cycle and went away. A person relaunches seconds later.)
    assert!(
        eventually(Duration::from_secs(3), || linked(PLAYER, NULL_SINK)),
        "the player did not go back to the default device:\n{}",
        run("pw-link", &["-l"])
    );
    std::thread::sleep(Duration::from_secs(1));

    // Relaunch: the same setup, plugin state included, and the player back in Music.
    let mut host = common::boot(&config, &plugins, system);
    assert_eq!(host.setup(), &before);
    let insert = host.setup().inserts().next().unwrap();
    assert_eq!(insert.state, Some(common::gain_state(GAIN)));
    assert!(
        poll_until(&mut host, Duration::from_secs(5), |_| linked(PLAYER, &sink)),
        "the player was not routed into {sink} again:\n{}",
        run("pw-link", &["-l"])
    );
    assert!(
        poll_until(&mut host, Duration::from_secs(3), |h| near(
            h.meters().channels[at].monitor.left,
            HEARD
        )),
        "Music did not sound the same after the relaunch: {:?} {:?}\n{}",
        host.backend(),
        host.meters(),
        run("pw-top", &["-b", "-n", "2"])
    );
}
