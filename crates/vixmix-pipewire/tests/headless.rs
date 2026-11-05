//! The backend against a real daemon: the headless PipeWire and WirePlumber
//! `scripts/ci/headless-pipewire.sh` starts (CI's Linux leg, a WSL2 or VM shell, the
//! container in docs/cookbook/run-headless-pipewire.md), with its null sink and null source.
//!
//! The tests run one at a time (they share the daemon's devices and WirePlumber's memory),
//! each with its own app name so their nodes never meet. Without the script's marker they say
//! how to start the daemon and pass without touching anything — except under `CI`, where a
//! missing daemon fails. Every realtime callback runs inside `assert_no_alloc`, and this binary
//! installs its allocator: an allocation on the data thread aborts the run.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use assert_no_alloc::AllocDisabler;
use vixmix_engine::{Engine, EngineConfig, MAX_QUANTUM, NoPlugins};
use vixmix_model::{ChannelId, DeviceRef, Setup, snapshot};
use vixmix_pipewire::{
    Backend, BackendConfig, BackendEvent, BackendGraph, BackendStats, GraphChannel, GraphInput,
    TickSource, channel_node_name, master_node_name, master_out_node_name, monitor_out_node_name,
};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const NULL_SINK: &str = "vixmix-ci-null-sink";
const NULL_SOURCE: &str = "vixmix-ci-null-source";
const RATE: usize = 48_000;
const QUANTUM: usize = 256;
const HOW: &str = "the backend's integration tests need the headless daemon: run `source scripts/ci/headless-pipewire.sh` in this shell first (docs/cookbook/run-headless-pipewire.md); they never run against a desktop session";

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

/// Whether some port of the node `from` is linked to some port of the node `to`
/// (`pw-link -l` lists each port as `node:port`, then its peers as `|-> node:port`).
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

/// A scratch directory for this test's files.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vixmix-pw-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A 32-bit float stereo WAV at 48 kHz, `seconds` long, each frame `sample(i)` on both sides.
fn write_wav(path: &Path, seconds: f32, sample: impl Fn(usize) -> f32) {
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
        let s = sample(i).to_le_bytes();
        out.extend_from_slice(&s);
        out.extend_from_slice(&s);
    }
    std::fs::write(path, out).unwrap();
}

/// The left channel of a float WAV pw-record wrote (its sizes may be unset when cut short).
fn read_wav_left(path: &Path) -> Vec<f32> {
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
                .map(|frame| f32::from_le_bytes(frame[..4].try_into().unwrap()))
                .collect();
        }
        at += size + (size & 1);
    }
    Vec::new()
}

/// A 440 Hz sine at half scale.
fn sine(i: usize) -> f32 {
    (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.5
}

/// A sine WAV of `seconds` in this test's scratch directory.
fn sine_wav(dir: &Path, seconds: f32) -> PathBuf {
    let path = dir.join("sine.wav");
    write_wav(&path, seconds, sine);
    path
}

/// The longest run of exact silence in `samples`, and how many places the signal breaks from
/// the sine's continuation (frames lost, repeated or replaced).
fn gaps(samples: &[f32]) -> (usize, usize) {
    let w = 440.0 * std::f32::consts::TAU / RATE as f32;
    let k = 2.0 * w.cos();
    let (mut longest, mut silent, mut breaks) = (0, 0, 0);
    for (i, s) in samples.iter().enumerate() {
        if *s == 0.0 {
            silent += 1;
            longest = longest.max(silent);
        } else {
            silent = 0;
        }
        if i >= 2 && (samples[i] - (k * samples[i - 1] - samples[i - 2])).abs() > 0.05 {
            breaks += 1;
        }
    }
    (longest, breaks)
}

/// `pw-play` of `file` as an app named `name`, whose media role is its own, so WirePlumber
/// remembers its routing apart from every other test's.
fn play(file: &Path, name: &str) -> Proc {
    let props = format!("{{ application.name = \"{name}\" node.name = \"{name}\" }}");
    Proc::spawn(
        "pw-play",
        &["-P", &props, "--media-role", name, file.to_str().unwrap()],
    )
}

/// `pw-record` of `target` (a sink's monitor when `sink`) as 32-bit float into `file`.
fn record(target: &str, sink: bool, file: &Path) -> Proc {
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
    Proc::spawn("pw-record", &args)
}

/// One backend with an engine of one channel (id 1, both sends at unity), and the events it
/// sent so far.
struct Rig {
    backend: Option<Backend>,
    events: Receiver<BackendEvent>,
    seen: Vec<BackendEvent>,
    engine: Arc<Engine>,
    app: &'static str,
}

impl Rig {
    fn new(app: &'static str) -> Rig {
        let engine = Arc::new(Engine::new(EngineConfig::new(48_000, MAX_QUANTUM, 2)));
        engine.prepare(&snapshot(&Setup::default()), &NoPlugins);
        let (tx, events) = sync_channel(256);
        let config = BackendConfig {
            app_name: app,
            wanted_quantum: QUANTUM as u32,
        };
        let backend = Backend::connect(config, Arc::clone(&engine), tx)
            .unwrap_or_else(|e| panic!("{e}: {HOW}"));
        Rig {
            backend: Some(backend),
            events,
            seen: Vec::new(),
            engine,
            app,
        }
    }

    fn backend(&self) -> &Backend {
        self.backend.as_ref().unwrap()
    }

    fn stats(&self) -> BackendStats {
        self.backend().stats()
    }

    /// Channel 1's sink, `<app>.ch1.<slug of name>`.
    fn sink(&self, name: &str) -> String {
        channel_node_name(self.app, ChannelId(1), name)
    }

    /// Channel 1 as an app sink named `name`, and the outputs.
    fn graph(
        &self,
        name: &str,
        monitor: Option<&str>,
        master: Option<&str>,
        source: bool,
    ) -> BackendGraph {
        let device = |name: &str| DeviceRef {
            name: name.to_string(),
            description: name.to_string(),
        };
        BackendGraph {
            channels: vec![GraphChannel {
                id: ChannelId(1),
                name: name.into(),
                input: GraphInput::AppSink,
            }],
            monitor: monitor.map(device),
            master: master.map(device),
            master_source: source,
        }
    }

    /// Applies `graph` and waits until the tick runs.
    fn start(&mut self, graph: &BackendGraph) {
        self.backend().apply(graph);
        let connected = self.wait(Duration::from_secs(5), |e| {
            matches!(e, BackendEvent::Connected { .. })
        });
        assert!(
            connected.is_some(),
            "the tick did not run within 5 s; saw {:?}",
            self.seen
        );
    }

    /// Waits up to `timeout` for an event `want` accepts; every event is kept in `seen`.
    fn wait(
        &mut self,
        timeout: Duration,
        want: impl Fn(&BackendEvent) -> bool,
    ) -> Option<BackendEvent> {
        if let Some(found) = self.seen.iter().find(|e| want(e)) {
            return Some(found.clone());
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => {
                    self.seen.push(event.clone());
                    if want(&event) {
                        return Some(event);
                    }
                }
                Err(_) => return None,
            }
        }
    }

    /// The node id of the next app stream named `name` to appear.
    fn app_stream(&mut self, name: &str) -> u32 {
        let found = self.wait(
            Duration::from_secs(5),
            |e| matches!(e, BackendEvent::AppStreamAdded(s) if s.app.name == name),
        );
        match found {
            Some(BackendEvent::AppStreamAdded(stream)) => {
                self.seen.retain(
                    |e| !matches!(e, BackendEvent::AppStreamAdded(s) if s.node == stream.node),
                );
                stream.node
            }
            _ => panic!("no app stream named {name} within 5 s; saw {:?}", self.seen),
        }
    }

    /// Whether the engine's channel 1 hears a signal (`true`) or stays silent for 300 ms
    /// (`false`) within `timeout`.
    fn channel_hears(&self, heard: bool, timeout: Duration) -> bool {
        let meter = &self.engine.meters().channel(0).monitor;
        let _ = meter.take();
        let deadline = Instant::now() + timeout;
        let mut quiet_since = Instant::now();
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            let (left, _) = meter.take();
            if heard && left > 0.1 {
                return true;
            }
            if !heard {
                if left > 0.0 {
                    quiet_since = Instant::now();
                } else if quiet_since.elapsed() > Duration::from_millis(300) {
                    return true;
                }
            }
        }
        false
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(backend) = self.backend.take() {
            backend.disconnect();
        }
    }
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

/// The `object.serial` of the node named `name`, from `pw-cli ls Node`. Unlike its id, which
/// PipeWire hands to the next object once the node is gone, a serial is never used twice.
fn node_serial(name: &str) -> Option<u64> {
    let listing = run("pw-cli", &["ls", "Node"]);
    let mut serial = None;
    for line in listing.lines() {
        let line = line.trim();
        if line.starts_with("id ") {
            serial = None;
        } else if let Some(rest) = line.strip_prefix("object.serial = ") {
            serial = rest.trim_matches('"').parse().ok();
        } else if line == format!("node.name = \"{name}\"") {
            return serial;
        }
    }
    None
}

/// `pw-top`'s ERR column — PipeWire's own count of the cycles a node had not finished when the
/// next began, since the node was made — for every node, by name, from the second of two
/// samples (`pw-top -b -n 2`).
fn pw_top_errors() -> Vec<(String, u64)> {
    let top = run("pw-top", &["-b", "-n", "2"]);
    let last = top.rsplit("S   ID").next().unwrap_or_default();
    last.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            fields.get(1)?.parse::<u32>().ok()?;
            let name = *fields.last()?;
            let err = fields
                .get(8)
                .and_then(|e| e.parse().ok())
                .unwrap_or(u64::MAX);
            Some((name.to_string(), err))
        })
        .collect()
}

/// A null sink made for one test, destroyed when it ends or earlier by `remove`.
struct NullSink {
    name: String,
}

impl NullSink {
    fn new(name: &str) -> NullSink {
        let spec = format!(
            "{{ factory.name=support.null-audio-sink node.name={name} node.description={name} media.class=Audio/Sink object.linger=true audio.position=[FL FR] monitor.channel-volumes=true }}"
        );
        run("pw-cli", &["create-node", "adapter", &spec]);
        assert!(
            eventually(Duration::from_secs(5), || node_id(name).is_some()),
            "pw-cli did not create {name}"
        );
        NullSink {
            name: name.to_string(),
        }
    }

    fn remove(&self) {
        if let Some(id) = node_id(&self.name) {
            run("pw-cli", &["destroy", &id.to_string()]);
        }
    }
}

impl Drop for NullSink {
    fn drop(&mut self) {
        self.remove();
    }
}

#[test]
fn devices_and_app_streams_are_reported_with_their_identity() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("identity");
    let mut rig = Rig::new("vixmix-identity");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    for (name, output) in [(NULL_SINK, true), (NULL_SOURCE, false)] {
        let device = rig.wait(
            Duration::from_secs(2),
            |e| matches!(e, BackendEvent::DeviceAdded(d) if d.device.name == name),
        );
        let Some(BackendEvent::DeviceAdded(info)) = device else {
            panic!("{name} was not reported; saw {:?}", rig.seen);
        };
        assert_eq!(
            info.direction == vixmix_model::DeviceDirection::Output,
            output,
            "{info:?}"
        );
    }
    assert!(
        !rig.seen
            .iter()
            .any(|e| matches!(e, BackendEvent::DeviceAdded(d)
            if d.device.name.starts_with("vixmix-identity."))),
        "our own nodes are not devices: {:?}",
        rig.seen
    );

    let app = play(&sine_wav(&dir, 5.0), "vixmix-identity-app");
    let found = rig.wait(
        Duration::from_secs(5),
        |e| matches!(e, BackendEvent::AppStreamAdded(s) if s.app.name == "vixmix-identity-app"),
    );
    let Some(BackendEvent::AppStreamAdded(stream)) = found else {
        panic!("the app stream was not reported; saw {:?}", rig.seen);
    };
    assert!(
        stream
            .app
            .binary
            .as_deref()
            .is_some_and(|b| b.starts_with("pw-")),
        "{stream:?}"
    );
    drop(app);
    assert!(
        rig.wait(Duration::from_secs(5), |e| {
            *e == BackendEvent::AppStreamRemoved(stream.node)
        })
        .is_some(),
        "the app's going was not reported; saw {:?}",
        rig.seen
    );
}

#[test]
fn an_app_routed_to_a_channel_reaches_the_engine_one_quantum_behind() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("route");
    let mut rig = Rig::new("vixmix-route");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    let _app = play(&sine_wav(&dir, 8.0), "vixmix-route-app");
    let node = rig.app_stream("vixmix-route-app");
    rig.backend().route(node, Some(&rig.sink("Test")));
    assert!(
        rig.channel_hears(true, Duration::from_secs(3)),
        "the app routed to {} never reached the engine",
        rig.sink("Test")
    );
    assert!(linked("vixmix-route-app", &rig.sink("Test")));
    // Once it runs (moving an app between sinks may cost the cycle the link changes in),
    // every check of its ring finds it exactly one quantum ahead.
    std::thread::sleep(Duration::from_millis(200));
    let before = rig.stats();
    std::thread::sleep(Duration::from_secs(2));
    let after = rig.stats();
    assert!(
        after.ring_checks - before.ring_checks > 300,
        "{before:?} {after:?}"
    );
    assert_eq!(after.ring_off - before.ring_off, 0, "{before:?} {after:?}");
    assert_eq!(after.xruns - before.xruns, 0, "{before:?} {after:?}");
    assert_eq!(after.quantum, QUANTUM as u32, "{after:?}");
}

#[test]
fn the_master_source_carries_the_mix_of_the_cycle_the_tick_ran_in() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("source");
    let mut rig = Rig::new("vixmix-source");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, true));
    let _app = play(&sine_wav(&dir, 8.0), "vixmix-source-app");
    let node = rig.app_stream("vixmix-source-app");
    rig.backend().route(node, Some(&rig.sink("Test")));
    assert!(rig.channel_hears(true, Duration::from_secs(3)));
    let before = rig.stats();
    let file = dir.join("master.wav");
    let recorder = record(&master_node_name(rig.app), false, &file);
    std::thread::sleep(Duration::from_millis(1500));
    recorder.interrupt();
    let after = rig.stats();
    // Each cycle Master ran in, it wrote the mix the tick made in that same cycle.
    assert!(
        after.master_source_cycles - before.master_source_cycles > 100,
        "{after:?}"
    );
    assert_eq!(after.stale_cycles, 0, "{after:?}");
    let recorded = read_wav_left(&file);
    assert!(recorded.len() > RATE / 2, "{} frames", recorded.len());
    let steady = &recorded[RATE / 10..];
    let peak = steady.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    assert!((peak - 0.5).abs() < 0.01, "peak {peak}");
    let (silence, breaks) = gaps(steady);
    assert!(
        silence <= 8 && breaks == 0,
        "{silence} silent frames, {breaks} breaks"
    );
}

#[test]
fn monitor_out_plays_to_its_device() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("monitor");
    let mut rig = Rig::new("vixmix-monitor");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    assert!(linked(&monitor_out_node_name(rig.app), NULL_SINK));
    let _app = play(&sine_wav(&dir, 8.0), "vixmix-monitor-app");
    let node = rig.app_stream("vixmix-monitor-app");
    rig.backend().route(node, Some(&rig.sink("Test")));
    assert!(rig.channel_hears(true, Duration::from_secs(3)));
    let file = dir.join("monitor.wav");
    let recorder = record(NULL_SINK, true, &file);
    std::thread::sleep(Duration::from_millis(1500));
    recorder.interrupt();
    let recorded = read_wav_left(&file);
    let steady = &recorded[(RATE / 10).min(recorded.len())..];
    let peak = steady.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    assert!(
        (peak - 0.5).abs() < 0.01,
        "peak {peak} over {} frames",
        steady.len()
    );
    assert_eq!(gaps(steady).1, 0);
}

#[test]
fn a_capture_channel_records_from_its_device() {
    let Some(_daemon) = headless() else {
        return;
    };
    let mut rig = Rig::new("vixmix-capture");
    let mut graph = rig.graph("Test", Some(NULL_SINK), None, false);
    graph.channels.push(GraphChannel {
        id: ChannelId(2),
        name: "Mic".into(),
        input: GraphInput::Capture(Some(DeviceRef {
            name: NULL_SOURCE.into(),
            description: NULL_SOURCE.into(),
        })),
    });
    rig.start(&graph);
    let capture = channel_node_name(rig.app, ChannelId(2), "Mic");
    assert!(
        eventually(Duration::from_secs(3), || linked(NULL_SOURCE, &capture)),
        "{capture} is not linked to {NULL_SOURCE}:\n{}",
        run("pw-link", &["-l"])
    );
}

#[test]
fn an_app_unassigned_while_absent_goes_back_to_the_default_when_it_appears() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("absent");
    let wav = sine_wav(&dir, 8.0);
    let name = "vixmix-absent-app";
    let mut rig = Rig::new("vixmix-absent");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    let sink = rig.sink("Test");
    let sink_id = node_id(&sink).expect("our sink is a node").to_string();
    // What WirePlumber's restore-stream writes when an app it routed before appears.
    let restore = |node: u32| {
        run(
            "pw-metadata",
            &[
                "-n",
                "default",
                &node.to_string(),
                "target.node",
                &sink_id,
                "Spa:Id",
            ],
        )
    };
    let home = || {
        eventually(Duration::from_secs(3), || {
            linked(name, NULL_SINK) && !linked(name, &sink)
        })
    };

    // Routed to the channel while it plays, then gone.
    let app = play(&wav, name);
    let node = rig.app_stream(name);
    rig.backend().route(node, Some(&sink));
    assert!(rig.channel_hears(true, Duration::from_secs(3)));
    drop(app);
    assert!(rig.channel_hears(false, Duration::from_secs(3)));

    // The host un-assigned it meanwhile. It appears, the session manager restores it to our
    // sink, and the host's routing on appear sends it home.
    let app = play(&wav, name);
    let node = rig.app_stream(name);
    restore(node);
    assert!(
        rig.channel_hears(true, Duration::from_secs(3)),
        "the restore did not take"
    );
    rig.backend().route(node, None);
    assert!(
        home(),
        "the app did not go back to the default:\n{}",
        run("pw-link", &["-l"])
    );
    assert!(rig.channel_hears(false, Duration::from_secs(1)));
    drop(app);

    // The same when the restore lands after the host's routing: the host's routing stands.
    let _app = play(&wav, name);
    let node = rig.app_stream(name);
    rig.backend().route(node, None);
    std::thread::sleep(Duration::from_millis(200));
    restore(node);
    assert!(home(), "a late restore won:\n{}", run("pw-link", &["-l"]));
    std::thread::sleep(Duration::from_millis(500));
    assert!(linked(name, NULL_SINK) && !linked(name, &sink));
    assert!(rig.channel_hears(false, Duration::from_secs(1)));
}

/// Whether the `default` metadata holds any key for `subject`.
fn routed_in_metadata(subject: u32) -> bool {
    let prefix = format!("id:{subject} ");
    run("pw-metadata", &["-n", "default"])
        .lines()
        .any(|line| line.trim_start_matches("update: ").starts_with(&prefix))
}

/// The `target.object` the `default` metadata holds for `subject`, as `pw-metadata` prints it.
fn metadata_target(subject: u32) -> Option<String> {
    let prefix = format!("id:{subject} key:'target.object' value:'");
    run("pw-metadata", &["-n", "default"])
        .lines()
        .find_map(|line| {
            let rest = line.trim_start_matches("update: ").strip_prefix(&prefix)?;
            Some(rest.split('\'').next()?.to_string())
        })
}

#[test]
fn routings_posted_just_before_disconnect_reach_the_daemon() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("leave");
    let mut rig = Rig::new("vixmix-leave");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    let name = "vixmix-leave-app";
    let _app = play(&sine_wav(&dir, 8.0), name);
    let node = rig.app_stream(name);
    let sink = rig.sink("Test");
    rig.backend().route(node, Some(&sink));
    assert!(eventually(Duration::from_secs(3), || linked(name, &sink)));
    // A host's shutdown: its last routings, then disconnect at once. The routing names a
    // node that outlives us, so only our write can have put its serial there.
    let null_sink = node_serial(NULL_SINK).expect("the null sink has a serial");
    let backend = rig.backend.take().unwrap();
    backend.route(node, Some(NULL_SINK));
    backend.disconnect();
    assert!(
        eventually(Duration::from_secs(2), || metadata_target(node)
            == Some(null_sink.to_string())),
        "the routing posted with the disconnect never reached the daemon: {:?}\n{}",
        metadata_target(node),
        run("pw-metadata", &["-n", "default"])
    );
}

#[test]
fn a_renamed_channel_keeps_its_apps_after_another_has_quit() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("rename");
    let wav = sine_wav(&dir, 10.0);
    let (stays, quits) = ("vixmix-rename-app", "vixmix-rename-quitter");
    let mut rig = Rig::new("vixmix-rename");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    let sink = rig.sink("Test");
    let _app = play(&wav, stays);
    let node = rig.app_stream(stays);
    rig.backend().route(node, Some(&sink));
    let quitter = play(&wav, quits);
    let other = rig.app_stream(quits);
    rig.backend().route(other, Some(&sink));
    assert!(
        eventually(Duration::from_secs(3), || linked(stays, &sink)
            && linked(quits, &sink)),
        "the two apps did not reach {sink}:\n{}",
        run("pw-link", &["-l"])
    );

    // One app quits, and the session manager clears every key of its subject; another client
    // clears a subject that has nothing to do with us. Neither may cost the other app its
    // routing.
    drop(quitter);
    assert!(
        eventually(Duration::from_secs(3), || !routed_in_metadata(other)),
        "the quitter's routing stayed:\n{}",
        run("pw-metadata", &["-n", "default"])
    );
    let unrelated = node_id(NULL_SOURCE).expect("the null source is a node");
    let unrelated = unrelated.to_string();
    run(
        "pw-metadata",
        &["-n", "default", &unrelated, "vixmix.test", "1"],
    );
    run("pw-metadata", &["-n", "default", "-d", &unrelated]);
    assert!(eventually(Duration::from_secs(3), || !routed_in_metadata(
        unrelated.parse().unwrap()
    )));
    std::thread::sleep(Duration::from_millis(200));
    assert!(rig.channel_hears(true, Duration::from_secs(3)));

    rig.backend()
        .apply(&rig.graph("Renamed", Some(NULL_SINK), None, false));
    let renamed = rig.sink("Renamed");
    assert!(
        eventually(Duration::from_secs(3), || linked(stays, &renamed)),
        "the app did not follow the rename to {renamed}:\n{}\n{}",
        run("pw-link", &["-l"]),
        run("pw-metadata", &["-n", "default"])
    );
    assert!(!linked(stays, NULL_SINK));
    assert!(rig.channel_hears(true, Duration::from_secs(3)));
}

#[test]
fn a_rename_that_keeps_the_slug_keeps_its_apps() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("sameslug");
    let wav = sine_wav(&dir, 12.0);
    let (routed, joins) = ("vixmix-sameslug-app", "vixmix-sameslug-joiner");
    let mut rig = Rig::new("vixmix-sameslug");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    let sink = rig.sink("Test");
    let first = node_serial(&sink).expect("the channel's sink is a node");
    let _app = play(&wav, routed);
    let node = rig.app_stream(routed);
    rig.backend().route(node, Some(&sink));
    let _joiner = play(&wav, joins);
    let joiner = rig.app_stream(joins);
    assert!(
        eventually(Duration::from_secs(3), || linked(routed, &sink)
            && linked(joins, NULL_SINK)),
        "the apps did not settle:\n{}",
        run("pw-link", &["-l"])
    );

    // A capitalisation fix keeps the slug, so the sink is made again under the same node name.
    // One app was routed to it before the rename; the other is routed to it in the same breath
    // as the rename, which the backend takes in one batch or in two.
    rig.backend().route(joiner, Some(&sink));
    rig.backend()
        .apply(&rig.graph("TEST", Some(NULL_SINK), None, false));
    assert_eq!(rig.sink("TEST"), sink);
    assert!(
        eventually(Duration::from_secs(3), || node_serial(&sink)
            .is_some_and(|now| now != first)),
        "the sink was not made again:\n{}",
        run("pw-cli", &["ls", "Node"])
    );
    let followed = || {
        linked(routed, &sink)
            && linked(joins, &sink)
            && !linked(routed, NULL_SINK)
            && !linked(joins, NULL_SINK)
    };
    assert!(
        eventually(Duration::from_secs(3), followed),
        "the apps did not follow a rename that keeps the slug:\n{}\n{}",
        run("pw-link", &["-l"]),
        run("pw-metadata", &["-n", "default"])
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(followed(), "an app left the renamed sink");
    assert!(rig.channel_hears(true, Duration::from_secs(3)));
}

#[test]
fn when_the_monitor_device_goes_master_out_ticks_without_a_gap() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("failover");
    let monitor = NullSink::new("vixmix-failover-monitor-device");
    let mut rig = Rig::new("vixmix-failover");
    rig.start(&rig.graph("Test", Some(&monitor.name), Some(NULL_SINK), false));
    assert!(
        rig.wait(Duration::from_secs(2), |e| {
            *e == BackendEvent::TickSource(TickSource::Monitor)
        })
        .is_some()
    );
    let _app = play(&sine_wav(&dir, 10.0), "vixmix-failover-app");
    let node = rig.app_stream("vixmix-failover-app");
    rig.backend().route(node, Some(&rig.sink("Test")));
    assert!(rig.channel_hears(true, Duration::from_secs(3)));
    assert!(linked(&master_out_node_name(rig.app), NULL_SINK));

    let file = dir.join("master.wav");
    let recorder = record(NULL_SINK, true, &file);
    std::thread::sleep(Duration::from_millis(1000));
    let before = rig.stats();
    monitor.remove();
    assert!(
        rig.wait(Duration::from_secs(2), |e| {
            *e == BackendEvent::TickSource(TickSource::MasterOut)
        })
        .is_some(),
        "the tick did not move to Master out; saw {:?}",
        rig.seen
    );
    std::thread::sleep(Duration::from_millis(1000));
    recorder.interrupt();
    let after = rig.stats();
    // About a second of cycles, every one ticked: none lost to the move.
    assert!(after.ticks - before.ticks > 150, "{before:?} {after:?}");
    assert_eq!(after.xruns - before.xruns, 0, "{before:?} {after:?}");
    assert_eq!(
        after.master_out_cycles - before.master_out_cycles,
        after.ticks - before.ticks
    );
    let recorded = read_wav_left(&file);
    let steady = &recorded[(RATE / 10).min(recorded.len())..];
    assert!(steady.len() > RATE, "{} frames", steady.len());
    let (silence, breaks) = gaps(steady);
    assert!(
        silence <= QUANTUM && breaks <= 2,
        "Master's gap: {silence} silent frames, {breaks} breaks"
    );
}

/// Puts `clock.force-quantum` back to "not forced" however the test ends.
struct ForcedQuantum;

impl ForcedQuantum {
    fn set(quantum: u32) -> ForcedQuantum {
        run(
            "pw-metadata",
            &[
                "-n",
                "settings",
                "0",
                "clock.force-quantum",
                &quantum.to_string(),
            ],
        );
        ForcedQuantum
    }
}

impl Drop for ForcedQuantum {
    fn drop(&mut self) {
        run(
            "pw-metadata",
            &["-n", "settings", "0", "clock.force-quantum", "0"],
        );
    }
}

#[test]
fn a_quantum_change_is_reported_and_the_tick_follows_it() {
    let Some(_daemon) = headless() else {
        return;
    };
    let mut rig = Rig::new("vixmix-quantum");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    assert_eq!(rig.stats().quantum, QUANTUM as u32);
    let forced = ForcedQuantum::set(512);
    assert!(
        rig.wait(Duration::from_secs(3), |e| *e
            == BackendEvent::QuantumChanged(512))
            .is_some(),
        "no QuantumChanged(512); saw {:?}",
        rig.seen
    );
    assert_eq!(rig.stats().quantum, 512);
    drop(forced);
    assert!(
        rig.wait(Duration::from_secs(3), |e| *e
            == BackendEvent::QuantumChanged(QUANTUM as u32))
            .is_some(),
        "the quantum did not come back; saw {:?}",
        rig.seen
    );
}

/// The acceptance run (`docs/handoffs/03-pipewire-backend.md`): two minutes with null devices
/// at 256, zero xruns, every ring exactly one quantum ahead at every tick. Two minutes is too
/// long for every push, so it runs on request:
/// `cargo test -p vixmix-pipewire --test headless -- --ignored two_minutes`.
#[test]
#[ignore = "two minutes long: the acceptance run, on request"]
fn two_minutes_at_256_without_an_xrun() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("two-minutes");
    let mut rig = Rig::new("vixmix-acceptance");
    rig.start(&rig.graph("Test", Some(NULL_SINK), Some(NULL_SINK), true));
    let _app = play(&sine_wav(&dir, 130.0), "vixmix-acceptance-app");
    let node = rig.app_stream("vixmix-acceptance-app");
    rig.backend().route(node, Some(&rig.sink("Test")));
    assert!(rig.channel_hears(true, Duration::from_secs(3)));
    let recorder = record(&master_node_name(rig.app), false, &dir.join("master.wav"));
    std::thread::sleep(Duration::from_secs(1));
    // PipeWire's own count is cumulative from each node's making, setup included: the window's
    // is the difference of two samples taken around the backend's own.
    let top_before = pw_top_errors();
    let before = rig.stats();
    std::thread::sleep(Duration::from_secs(120));
    let after = rig.stats();
    let top_after = pw_top_errors();
    recorder.interrupt();
    // Every node's ERR over the window, ours and the test's own pw-play and pw-record: an ERR
    // on theirs as well is a stall of the whole machine rather than of the backend.
    let window: Vec<(String, Option<u64>)> = top_after
        .iter()
        .map(|(node, err)| {
            let was = top_before.iter().find(|(name, _)| name == node);
            (node.clone(), was.and_then(|(_, was)| err.checked_sub(*was)))
        })
        .collect();
    eprintln!("before {before:?}\nafter  {after:?}\npw-top ERR over the window {window:?}");
    let ticks = after.ticks - before.ticks;
    assert!(
        ticks >= 120 * RATE as u64 / QUANTUM as u64 - 100,
        "{ticks} ticks"
    );
    assert_eq!(after.quantum, QUANTUM as u32);
    assert_eq!(after.xruns - before.xruns, 0);
    assert_eq!(after.ring_off - before.ring_off, 0);
    assert_eq!(after.ring_checks - before.ring_checks, ticks);
    assert_eq!(after.stale_cycles, 0);
    for counted in [
        after.monitor_cycles - before.monitor_cycles,
        after.master_out_cycles - before.master_out_cycles,
        after.master_source_cycles - before.master_source_cycles,
    ] {
        assert_eq!(counted, ticks);
    }
    // PipeWire's own count over the window: no ERR on any of our four nodes. (It also sees a
    // late finish, which the backend's own count cannot.)
    let ours: Vec<_> = window
        .iter()
        .filter(|(node, _)| node.starts_with(&format!("{}.", rig.app)))
        .collect();
    assert_eq!(ours.len(), 4, "pw-top: {window:?}");
    for (node, err) in ours {
        assert_eq!(*err, Some(0), "pw-top's ERR of {node} over the window");
    }
}

/// The daemon goes away and comes back: the backend says so, reconnects with backoff, and
/// makes the same streams again. It restarts the test daemon, so it runs on request:
/// `cargo test -p vixmix-pipewire --test headless -- --ignored daemon_restarts`.
#[test]
#[ignore = "restarts the daemon the other tests share: on request"]
fn the_backend_reconnects_after_the_daemon_restarts() {
    let Some(_daemon) = headless() else {
        return;
    };
    let mut rig = Rig::new("vixmix-reconnect");
    rig.start(&rig.graph("Test", Some(NULL_SINK), None, false));
    let _ = run("pkill", &["-x", "pipewire"]);
    assert!(
        rig.wait(Duration::from_secs(5), |e| *e == BackendEvent::Disconnected)
            .is_some(),
        "the daemon's going was not reported; saw {:?}",
        rig.seen
    );
    rig.seen.clear();
    std::thread::sleep(Duration::from_millis(500));
    // Started again as scripts/ci/headless-pipewire.sh does; left running for what follows.
    std::mem::forget(Proc::spawn("pipewire", &[]));
    std::thread::sleep(Duration::from_millis(500));
    if run("pgrep", &["-x", "wireplumber"]).trim().is_empty() {
        std::mem::forget(Proc::spawn("wireplumber", &[]));
    }
    assert!(
        rig.wait(Duration::from_secs(15), |e| {
            matches!(e, BackendEvent::Connected { .. })
        })
        .is_some(),
        "the backend did not come back; saw {:?}",
        rig.seen
    );
    assert!(eventually(Duration::from_secs(3), || linked(
        &monitor_out_node_name(rig.app),
        NULL_SINK
    )));
}
