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
    Backend, BackendConfig, BackendEvent, BackendGraph, GraphChannel, GraphInput, channel_node_name,
};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const NULL_SINK: &str = "vixmix-ci-null-sink";
const RATE: usize = 48_000;
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

fn sine(i: usize) -> f32 {
    (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.5
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
            wanted_quantum: 256,
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

    /// The channel's sink: `<app>.ch1.test`.
    fn sink(&self) -> String {
        channel_node_name(self.app, ChannelId(1), "Test")
    }

    fn graph(&self, monitor: Option<&str>, master: Option<&str>, source: bool) -> BackendGraph {
        let device = |name: &str| DeviceRef {
            name: name.to_string(),
            description: name.to_string(),
        };
        BackendGraph {
            channels: vec![GraphChannel {
                id: ChannelId(1),
                name: "Test".into(),
                input: GraphInput::AppSink,
            }],
            monitor: monitor.map(device),
            master: master.map(device),
            master_source: source,
        }
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

    /// Waits for the engine's channel 1 to hear something on its Monitor send (or for
    /// silence when `heard` is false), for up to `timeout`.
    fn channel_hears(&self, heard: bool, timeout: Duration) -> bool {
        let meters = self.engine.meters();
        let deadline = Instant::now() + timeout;
        let _ = meters.channel(0).monitor.take();
        let mut quiet_since = Instant::now();
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            let (left, _) = meters.channel(0).monitor.take();
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

#[test]
#[ignore = "exploration: prints what the daemon does"]
fn explore() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("explore");
    let wav = dir.join("sine.wav");
    write_wav(&wav, 8.0, sine);
    let mut rig = Rig::new("vixmix-explore");
    rig.backend()
        .apply(&rig.graph(Some(NULL_SINK), Some(NULL_SINK), true));
    let connected = rig.wait(Duration::from_secs(5), |e| {
        matches!(e, BackendEvent::Connected { .. })
    });
    eprintln!("connected: {connected:?}");
    let play = Proc::spawn("pw-play", &[wav.to_str().unwrap()]);
    let node = rig.app_stream("pw-play");
    eprintln!("pw-play is node {node}");
    rig.backend().route(node, Some(&rig.sink()));
    eprintln!("hears: {}", rig.channel_hears(true, Duration::from_secs(3)));
    let record = dir.join("master.wav");
    let rec = Proc::spawn(
        "pw-record",
        &[
            "--target",
            &vixmix_pipewire::master_node_name(rig.app),
            "--format",
            "f32",
            "--rate",
            "48000",
            "--channels",
            "2",
            record.to_str().unwrap(),
        ],
    );
    for _ in 0..6 {
        std::thread::sleep(Duration::from_millis(500));
        eprintln!("{:?}", rig.backend().stats());
    }
    eprintln!("{}", run("pw-top", &["-b", "-n", "2"]));
    eprintln!("{}", run("pw-link", &["-l"]));
    rec.interrupt();
    let recorded = read_wav_left(&record);
    let peak = recorded.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    eprintln!("recorded {} frames, peak {peak}", recorded.len());
    drop(play);
    eprintln!("events: {:?}", rig.seen);
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

/// A null sink made for one test and destroyed when it ends (or earlier, by `remove`).
struct NullSink {
    name: String,
}

impl NullSink {
    fn new(name: &str) -> NullSink {
        let spec = format!(
            "{{ factory.name=support.null-audio-sink node.name={name} node.description={name} media.class=Audio/Sink object.linger=true audio.position=[FL FR] monitor.channel-volumes=true }}"
        );
        run("pw-cli", &["create-node", "adapter", &spec]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while node_id(name).is_none() {
            assert!(Instant::now() < deadline, "pw-cli did not create {name}");
            std::thread::sleep(Duration::from_millis(50));
        }
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

/// The longest run of exact silence in `samples`, and how many places the signal breaks
/// from a 440 Hz sine's continuation (a jump: frames lost or repeated).
fn gaps(samples: &[f32]) -> (usize, usize) {
    let w = 440.0 * std::f32::consts::TAU / RATE as f32;
    let k = 2.0 * w.cos();
    let (mut longest, mut run_len, mut breaks) = (0, 0, 0);
    for (i, s) in samples.iter().enumerate() {
        if *s == 0.0 {
            run_len += 1;
            longest = longest.max(run_len);
        } else {
            run_len = 0;
        }
        if i >= 2 {
            let predicted = k * samples[i - 1] - samples[i - 2];
            if (samples[i] - predicted).abs() > 0.05 {
                breaks += 1;
            }
        }
    }
    (longest, breaks)
}

#[test]
#[ignore = "exploration: prints what the daemon does when Monitor's device goes"]
fn explore_failover() {
    let Some(_daemon) = headless() else {
        return;
    };
    let dir = scratch("failover");
    let wav = dir.join("sine.wav");
    write_wav(&wav, 10.0, sine);
    let monitor = NullSink::new("vixmix-test-monitor-device");
    let mut rig = Rig::new("vixmix-failover");
    rig.backend()
        .apply(&rig.graph(Some(&monitor.name), Some(NULL_SINK), true));
    let connected = rig.wait(Duration::from_secs(5), |e| {
        matches!(e, BackendEvent::Connected { .. })
    });
    eprintln!("connected: {connected:?}");
    let play = Proc::spawn("pw-play", &[wav.to_str().unwrap()]);
    let node = rig.app_stream("pw-play");
    rig.backend().route(node, Some(&rig.sink()));
    assert!(rig.channel_hears(true, Duration::from_secs(3)));
    let record = dir.join("master.wav");
    let rec = Proc::spawn(
        "pw-record",
        &[
            "--target",
            NULL_SINK,
            "-P",
            "stream.capture.sink=true",
            "--format",
            "f32",
            "--rate",
            "48000",
            "--channels",
            "2",
            record.to_str().unwrap(),
        ],
    );
    std::thread::sleep(Duration::from_millis(1500));
    eprintln!("before: {:?}", rig.backend().stats());
    let removed_at = Instant::now();
    monitor.remove();
    let moved = rig.wait(Duration::from_secs(5), |e| {
        matches!(
            e,
            BackendEvent::TickSource(vixmix_pipewire::TickSource::MasterOut)
        )
    });
    eprintln!("moved after {:?}: {moved:?}", removed_at.elapsed());
    std::thread::sleep(Duration::from_millis(1500));
    eprintln!("after: {:?}", rig.backend().stats());
    eprintln!("{}", run("pw-top", &["-b", "-n", "2"]));
    rec.interrupt();
    let recorded = read_wav_left(&record);
    eprintln!(
        "recorded {} frames; longest silence and breaks: {:?}",
        recorded.len(),
        gaps(&recorded[4800..])
    );
    drop(play);
    eprintln!("events: {:?}", rig.seen);
}
