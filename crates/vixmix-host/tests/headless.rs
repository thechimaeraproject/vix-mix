//! The real host against a real daemon: the headless PipeWire and WirePlumber that
//! `scripts/ci/headless-pipewire.sh` starts (CI's Linux leg, the container in
//! docs/cookbook/run-headless-pipewire.md), with its null sink and null source. This is
//! handoff 06's acceptance flow without a person: the host boots with a setup that names an
//! app, a plugin and an output; a `pw-play` stream appears, is matched to its channel and
//! routed into that channel's sink, plays through the plugin to Monitor on the null sink and
//! to the Master source, which `pw-record` records; the plugin is moved while it plays; the
//! host quits, clearing its routing; and a relaunch gives back the same setup with the
//! plugin's new state, sounding so, and the app in its channel. A second test changes the
//! quantum while the app plays and Master is recorded, as the preferences do: the app stays
//! in its channel and the recording goes on.
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
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use assert_no_alloc::AllocDisabler;
use vixmix_host::{AudioBackend, BackendState, Host, RealHost};
use vixmix_model::{
    AppRef, ChannelId, ChannelKind, Command as Cmd, DeviceRef, InstanceId, MixId, Prefs, Setup,
    Source, Strip, apply,
};
use vixmix_pipewire::{channel_node_name, master_node_name, monitor_out_node_name};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const NULL_SINK: &str = "vixmix-ci-null-sink";
const NULL_SOURCE: &str = "vixmix-ci-null-source";
const RATE: usize = 48_000;
/// The player's sine, and the test plugin's gain on its channel.
const AMPLITUDE: f32 = 0.5;
const GAIN: f64 = 0.5;
const HEARD: f32 = AMPLITUDE * GAIN as f32;
/// Where the first test moves the plugin's gain while it plays (parameter 0, normalized, is
/// the gain).
const MOVED: f64 = 0.25;
const HOW: &str = "the host's integration tests need the headless daemon: run `source scripts/ci/headless-pipewire.sh` in this shell first (docs/cookbook/run-headless-pipewire.md); they never run against a desktop session";

/// One test at a time: each moves the daemon's graph (its quantum among it).
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The daemon's go-ahead: `None` (after saying how to start it) when this shell has none.
fn headless() -> Option<MutexGuard<'static, ()>> {
    if std::env::var_os("VIXMIX_HEADLESS_PIPEWIRE").is_none() {
        assert!(std::env::var_os("CI").is_none(), "{HOW}");
        eprintln!("skipped: {HOW}");
        return None;
    }
    Some(ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner))
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

/// The left channel of a float WAV pw-record wrote, past its first tenth of a second.
fn recorded(path: &Path) -> Vec<f32> {
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
                .map(|frame| f32::from_le_bytes(frame[..4].try_into().unwrap()))
                .collect();
        }
        at += size + (size & 1);
    }
    Vec::new()
}

/// The largest absolute sample of what [`recorded`] reads.
fn recorded_peak(path: &Path) -> f32 {
    recorded(path).iter().fold(0.0, |peak, s| peak.max(s.abs()))
}

/// The runs of silence in `samples` longer than 64 frames, as their lengths: a sine is silent
/// for a frame at most.
fn silences(samples: &[f32]) -> Vec<usize> {
    let mut runs = Vec::new();
    let mut run = 0;
    for sample in samples.iter().chain([&1.0]) {
        if sample.abs() < 1e-4 {
            run += 1;
        } else {
            if run > 64 {
                runs.push(run);
            }
            run = 0;
        }
    }
    runs
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

/// The setup the user left, written to `config` after a first boot found the test plugin:
/// Music, an app channel that takes `player`, through the test plugin at half gain; Monitor
/// on the null sink; Master on no device (still a source). The setup, Music's id and the
/// plugin's instance.
fn music(config: &Path, plugins: &Path, player: &str) -> (Setup, ChannelId, InstanceId) {
    let host = common::boot(config, plugins, AudioBackend::Null);
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
                name: player.into(),
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
    common::write_setup(config, &setup);
    (setup, music, instance)
}

/// Boots the host as `app` on the daemon and waits until it runs: Monitor's output plays to
/// the null sink.
fn boot_running(config: &Path, plugins: &Path, app: &'static str) -> RealHost {
    let mut host = common::boot(config, plugins, AudioBackend::System { app_name: app });
    assert!(
        poll_until(&mut host, Duration::from_secs(5), |h| matches!(
            h.backend(),
            BackendState::Running { .. }
        )),
        "the host did not run on the daemon: {:?}",
        host.backend()
    );
    assert!(linked(&monitor_out_node_name(app), NULL_SINK));
    host
}

/// `pw-play` of a 30-second sine as the app `player` (its `application.name`, `node.name` and
/// media role, so WirePlumber remembers its routing apart from every other test's).
fn play(dir: &Path, player: &str) -> Proc {
    Proc::spawn(
        "pw-play",
        &[
            "-P",
            &format!("{{ application.name = \"{player}\" node.name = \"{player}\" }}"),
            "--media-role",
            player,
            sine_wav(dir, 30.0).to_str().unwrap(),
        ],
    )
}

/// Waits until `player` is listed as playing, is routed into `sink`, and Music (the channel at
/// `at`) reads `level` on its Monitor send.
fn heard_in_music(host: &mut RealHost, player: &str, sink: &str, at: usize, level: f32) {
    assert!(
        poll_until(host, Duration::from_secs(5), |h| h.apps().iter().any(|p| p
            .app
            .name
            == player
            && p.running)),
        "the player was not listed as playing: {:?}",
        host.apps()
    );
    assert!(
        poll_until(host, Duration::from_secs(5), |_| linked(player, sink)),
        "the player was not routed into {sink}:\n{}",
        run("pw-link", &["-l"])
    );
    assert!(
        poll_until(host, Duration::from_secs(3), |h| near(
            h.meters().channels[at].monitor.left,
            level
        )),
        "Music's Monitor send did not read {level}: {:?} {:?}\n{}",
        host.backend(),
        host.meters(),
        run("pw-top", &["-b", "-n", "2"])
    );
}

#[test]
fn an_app_plays_through_its_channel_to_monitor_and_master_and_comes_back_after_a_relaunch() {
    let Some(_daemon) = headless() else {
        return;
    };
    // The host's `application.name` and node-name prefix, its own so no other test meets it,
    // and the player's name.
    const APP: &str = "vixmix-host-it";
    const PLAYER: &str = "vixmix-host-music";
    let plugins = common::plugin_dir("headless");
    let config = common::config_dir("headless");
    let dir = common::scratch("headless");
    let (setup, music, instance) = music(&config, &plugins, PLAYER);
    let at = setup.channels.iter().position(|c| c.id == music).unwrap();
    let sink = channel_node_name(APP, music, "Music");

    // Boot: the backend is kept, and runs once Monitor's output plays to the null sink.
    let mut host = boot_running(&config, &plugins, APP);
    let stats = host.stats();
    assert_eq!((stats.rate, stats.quantum), (48_000, 256), "{stats:?}");

    // The player appears, is matched to Music and routed into its sink, and plays through
    // the plugin.
    let _player = play(&dir, PLAYER);
    heard_in_music(&mut host, PLAYER, &sink, at, HEARD);

    // Monitor's output reaches the null sink; the Master source is recordable.
    let heard = record(&mut host, NULL_SINK, true, &dir.join("monitor.wav"), 1.5);
    assert!(near(heard, HEARD), "the null sink heard {heard}");
    let master = master_node_name(APP);
    let heard = record(&mut host, &master, false, &dir.join("master.wav"), 1.5);
    assert!(near(heard, HEARD), "{master} gave {heard}");

    // The plugin is moved while it plays, as an edit in its editor would move it.
    host.set_param(instance, 0, MOVED).unwrap();
    let moved = AMPLITUDE * MOVED as f32;
    assert!(
        poll_until(&mut host, Duration::from_secs(2), |h| near(
            h.meters().channels[at].monitor.left,
            moved
        )),
        "Music did not follow the plugin's move: {:?}",
        host.meters()
    );

    // Quit at once: the routing is cleared (the app's target no longer names our sink) and
    // the setup is kept with the plugin's new state, taken from the live plugin.
    let node = node_id(PLAYER).expect("the player is a node");
    let ours = node_serial(&sink).expect("the sink has a serial");
    assert_eq!(metadata_target(node).as_deref(), Some(ours.as_str()));
    let mut before = host.setup().clone();
    let _ = apply(
        &mut before,
        Cmd::SetInsertState {
            instance,
            state: Some(common::gain_state(MOVED)),
        },
    )
    .unwrap();
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

    // Relaunch: the same setup with the plugin's new state, sounding so, and the player back
    // in Music.
    let mut host = boot_running(&config, &plugins, APP);
    assert_eq!(host.setup(), &before);
    let insert = host.setup().inserts().next().unwrap();
    assert_eq!(insert.state, Some(common::gain_state(MOVED)));
    heard_in_music(&mut host, PLAYER, &sink, at, moved);
}

#[test]
fn a_new_quantum_keeps_the_app_in_its_channel_and_the_master_source_recording() {
    let Some(_daemon) = headless() else {
        return;
    };
    const APP: &str = "vixmix-host-quantum";
    const PLAYER: &str = "vixmix-host-quantum-music";
    let plugins = common::plugin_dir("quantum");
    let config = common::config_dir("quantum");
    let dir = common::scratch("quantum");
    let (setup, music, _) = music(&config, &plugins, PLAYER);
    let at = setup.channels.iter().position(|c| c.id == music).unwrap();
    let sink = channel_node_name(APP, music, "Music");
    let master = master_node_name(APP);

    let mut host = boot_running(&config, &plugins, APP);
    let _player = play(&dir, PLAYER);
    heard_in_music(&mut host, PLAYER, &sink, at, HEARD);
    let ours = [sink.clone(), monitor_out_node_name(APP), master.clone()];
    let serials: Vec<Option<String>> = ours.iter().map(|n| node_serial(n)).collect();
    assert!(serials.iter().all(Option::is_some), "{ours:?} {serials:?}");
    // A link made by hand into Music's sink, as the cookbook's latency step makes one (the
    // null source plays silence, so Music still reads the player alone).
    let source_port = run("pw-link", &["-o"])
        .lines()
        .map(str::trim)
        .find(|port| port.starts_with(&format!("{NULL_SOURCE}:")))
        .expect("the null source has an output port")
        .to_string();
    let _ = run("pw-link", &[&source_port, &format!("{sink}:playback_FL")]);
    assert!(
        linked(NULL_SOURCE, &sink),
        "the hand-made link: {}",
        run("pw-link", &["-l"])
    );

    // Master is recorded (Discord, OBS) while the preferences ask for 512.
    let file = dir.join("master.wav");
    let recorder = Proc::spawn(
        "pw-record",
        &[
            "--target",
            &master,
            "--format",
            "f32",
            "--rate",
            "48000",
            "--channels",
            "2",
            file.to_str().unwrap(),
        ],
    );
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        let _ = host.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    let prefs = Prefs {
        quantum: 512,
        ..host.setup().prefs.clone()
    };
    let applied = host.apply(Cmd::SetPrefs(prefs)).unwrap();
    assert!(applied.graph, "a new quantum is the backend's");
    assert!(
        poll_until(&mut host, Duration::from_secs(3), |h| h.backend()
            == &BackendState::Running {
                rate: 48_000,
                quantum: 512
            }),
        "the graph did not follow to 512: {:?}",
        host.backend()
    );

    // Three seconds later the player is still in Music and heard there, nothing of ours was
    // made again (a node made again has a new serial), and the recording went on throughout:
    // the change itself costs one gap of at most two cycles at 512 (the backend's rings grow
    // to one new quantum ahead; measured: one gap of 1024 frames, every run).
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        let _ = host.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(host.stats().quantum, 512);
    let after: Vec<Option<String>> = ours.iter().map(|n| node_serial(n)).collect();
    assert_eq!(serials, after, "a node of ours was made again: {ours:?}");
    assert!(linked(PLAYER, &sink), "{}", run("pw-link", &["-l"]));
    assert!(
        linked(NULL_SOURCE, &sink),
        "the hand-made link did not outlive the change: {}",
        run("pw-link", &["-l"])
    );
    assert!(
        poll_until(&mut host, Duration::from_secs(1), |h| near(
            h.meters().channels[at].monitor.left,
            HEARD
        )),
        "the player went quiet at 512: {:?}\n{}",
        host.meters(),
        run("pw-top", &["-b", "-n", "2"])
    );
    recorder.interrupt();
    let samples = recorded(&file);
    let peak = samples.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    let gaps = silences(&samples);
    eprintln!(
        "the recording of {master}: {} frames, peak {peak}, gaps {gaps:?}",
        samples.len()
    );
    assert!(
        samples.len() > 3 * RATE
            && near(peak, HEARD)
            && gaps.len() <= 1
            && gaps.iter().all(|g| *g <= 2 * 512),
        "the recording of {master}: {} frames, peak {peak}, silent runs {gaps:?}",
        samples.len()
    );
}
