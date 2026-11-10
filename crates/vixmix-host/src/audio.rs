//! Where the audio goes: PipeWire on Linux, kept for the whole run, or the null backend, which
//! pumps the engine one quantum a poll and plays nothing (every other platform, a test, or a
//! Linux session whose PipeWire does not answer, asked again every few seconds).
//!
//! On PipeWire this module is the host's half of the routing contract: it derives the
//! backend's graph from the setup, routes every app stream the moment it appears (to the
//! channel that names it, or home to the default: WirePlumber may restore an old routing, so
//! the host's rule is the routing of record), routes again when the setup moves an app, and
//! clears its routings before it leaves.

use std::sync::Arc;
use std::time::{Duration, Instant};

use vixmix_engine::{Engine, NullBackend};
use vixmix_model::{AppRef, DeviceInfo, Setup};

use crate::host::BackendState;

/// The quantum the null backend pumps per poll.
pub(crate) const NULL_QUANTUM: usize = 256;

/// How often the host wants a poll while a real backend can bring news by itself.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The banner when the host was started on the null backend on purpose.
pub(crate) const NULL_CHOSEN: &str =
    "Started without an audio backend: the mixer runs silent (a test, or a check)";

/// What the host plays through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioBackend {
    /// The null backend: the engine runs silent and nothing outside the process is touched.
    /// What a test boots with, so no test ever routes a desktop's apps.
    #[default]
    Null,
    /// The system's backend: PipeWire on Linux, as `app_name` (its `application.name` and the
    /// prefix of every node name); on a platform with none yet, the null backend with a
    /// banner that says so.
    System { app_name: &'static str },
}

impl AudioBackend {
    /// The app's: the system's backend as `vix-mix`.
    pub const APP: AudioBackend = AudioBackend::System {
        app_name: "vix-mix",
    };
}

/// An app's playback stream the backend reported, playing now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stream {
    /// The stream's node id: live only, never stored.
    pub node: u32,
    pub app: AppRef,
}

/// What a poll heard that the host passes on to the window.
#[derive(Debug, Default)]
pub(crate) struct Heard {
    pub devices: bool,
    pub apps: bool,
    pub toasts: Vec<String>,
}

/// The backend the host runs on, and what it reported.
pub(crate) struct Audio {
    kind: Kind,
    devices: Vec<DeviceInfo>,
    streams: Vec<Stream>,
}

enum Kind {
    Null(Box<Null>),
    #[cfg(target_os = "linux")]
    PipeWire(Box<linux::PipeWire>),
}

struct Null {
    pump: NullBackend,
    reason: String,
    /// PipeWire refused: asked again at this time, as this app.
    retry: Option<(Instant, &'static str)>,
}

impl Audio {
    /// Starts `choice` with the engine and applies the graph `setup` asks for.
    pub fn start(choice: AudioBackend, engine: &Arc<Engine>, setup: &Setup) -> Audio {
        let kind = match choice {
            AudioBackend::Null => null(engine, NULL_CHOSEN.to_string(), None),
            AudioBackend::System { app_name } => system(app_name, engine, setup),
        };
        Audio {
            kind,
            devices: Vec::new(),
            streams: Vec::new(),
        }
    }

    /// The state the window shows.
    pub fn state(&self) -> BackendState {
        match &self.kind {
            Kind::Null(null) => BackendState::Absent {
                reason: null.reason.clone(),
            },
            #[cfg(target_os = "linux")]
            Kind::PipeWire(pw) => pw.state(),
        }
    }

    /// The output and input devices present now.
    pub fn devices(&self) -> &[DeviceInfo] {
        &self.devices
    }

    /// The app streams playing now.
    pub fn streams(&self) -> &[Stream] {
        &self.streams
    }

    /// On the null backend, channel `index`'s input (left and right, a quantum or more), which
    /// every later pump plays until it is written again.
    pub fn null_input(&mut self, index: usize) -> Option<(&mut [f32], &mut [f32])> {
        match &mut self.kind {
            Kind::Null(null) => null.pump.input_mut(index),
            #[cfg(target_os = "linux")]
            Kind::PipeWire(_) => None,
        }
    }

    /// One host tick: the null backend pumps a quantum (and, on Linux, asks PipeWire again
    /// when it is time); PipeWire's events are handled, every app that appeared routed.
    pub fn poll(&mut self, setup: &Setup, engine: &Arc<Engine>, now: Instant) -> Heard {
        let (heard, ask_again) = match &mut self.kind {
            Kind::Null(null) => {
                null.pump.pump(NULL_QUANTUM);
                let due = null.retry.filter(|(at, _)| now >= *at);
                (Heard::default(), due.map(|(_, app_name)| app_name))
            }
            #[cfg(target_os = "linux")]
            Kind::PipeWire(pw) => {
                let mut heard = Heard::default();
                pw.poll(
                    setup,
                    engine,
                    &mut self.devices,
                    &mut self.streams,
                    &mut heard,
                    now,
                );
                (heard, None)
            }
        };
        if let Some(app_name) = ask_again {
            self.kind = system(app_name, engine, setup);
        }
        heard
    }

    /// The setup's graph changed: the backend makes its nodes match. A new quantum is asked
    /// for in place (change request 9): no node is made again, so every app stays in its
    /// channel and every link to the Master source (a recorder's) stays.
    pub fn apply_graph(&mut self, setup: &Setup) {
        #[cfg(target_os = "linux")]
        if let Kind::PipeWire(pw) = &mut self.kind {
            pw.apply_graph(setup, &self.devices);
        }
        let _ = setup;
    }

    /// The setup moved apps between channels (or renamed, removed or restored one): every
    /// app playing now whose channel's sink changed is routed again.
    pub fn reroute(&mut self, setup: &Setup) {
        #[cfg(target_os = "linux")]
        if let Kind::PipeWire(pw) = &mut self.kind {
            pw.reroute(setup, &self.streams);
        }
        let _ = setup;
    }

    /// How soon the host wants its next poll for this backend's sake.
    pub fn next_poll(&self, now: Instant) -> Option<Duration> {
        match &self.kind {
            Kind::Null(null) => null.retry.map(|(at, _)| at.saturating_duration_since(now)),
            #[cfg(target_os = "linux")]
            Kind::PipeWire(_) => Some(POLL_INTERVAL),
        }
    }

    /// Clears every routing the host made and leaves the daemon: no tick runs afterwards.
    pub fn shutdown(&mut self) {
        match &mut self.kind {
            Kind::Null(null) => null.retry = None,
            #[cfg(target_os = "linux")]
            Kind::PipeWire(pw) => pw.shutdown(),
        }
        self.devices.clear();
        self.streams.clear();
    }
}

fn null(engine: &Arc<Engine>, reason: String, retry: Option<(Instant, &'static str)>) -> Kind {
    Kind::Null(Box::new(Null {
        pump: NullBackend::new(Arc::clone(engine), NULL_QUANTUM),
        reason,
        retry,
    }))
}

/// The system's backend, or the null backend with the reason there is none.
fn system(app_name: &'static str, engine: &Arc<Engine>, setup: &Setup) -> Kind {
    #[cfg(target_os = "linux")]
    {
        match linux::PipeWire::connect(app_name, engine, setup) {
            Ok(pw) => Kind::PipeWire(Box::new(pw)),
            Err(reason) => {
                log::warn!("{reason}");
                null(
                    engine,
                    reason,
                    Some((Instant::now() + linux::RETRY_CONNECT, app_name)),
                )
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app_name, setup);
        null(engine, BackendState::NO_BACKEND.to_string(), None)
    }
}

#[cfg(target_os = "linux")]
pub(crate) mod linux {
    //! PipeWire, kept for the whole run.

    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::{Receiver, sync_channel};
    use std::time::{Duration, Instant};

    use vixmix_engine::Engine;
    use vixmix_model::{AppRef, DeviceInfo, DeviceRef, Setup, Source};
    use vixmix_pipewire::{
        Backend, BackendConfig, BackendEvent, BackendGraph, DISPLAY_NAME, GraphChannel, GraphInput,
        TickSource, channel_node_name,
    };

    use super::{Heard, Stream};
    use crate::host::BackendState;

    /// How long after PipeWire refused the host asks again.
    pub(crate) const RETRY_CONNECT: Duration = Duration::from_secs(5);
    /// The events the backend may send before the host reads them; past this they wait in
    /// the backend's own bounded backlog.
    const EVENTS: usize = 256;
    /// The most events one poll handles, so a flood never stalls a frame.
    const EVENTS_PER_POLL: usize = 1024;
    /// The most devices and app streams the host follows at once.
    const MAX_DEVICES: usize = 128;
    const MAX_STREAMS: usize = 256;
    /// How long the engine may go without a cycle before the Master source alone is idle.
    const TICK_GAP: Duration = Duration::from_millis(250);

    /// The banner while nothing at all drives the mixer.
    pub(crate) const STOPPED: &str =
        "The mixer is stopped: no output stream runs; pick an output device for Monitor";

    /// The banner while only the Master source could drive the mixer and nothing records it.
    pub(crate) fn only_recorded() -> String {
        format!(
            "No mix plays to a device, so the mixer runs only while an app records \u{201c}{DISPLAY_NAME} Master\u{201d}: pick an output device for Monitor to hear it"
        )
    }

    /// The toast when the device the setup names for Monitor is not there and Master's
    /// output drives the mixer.
    pub(crate) const MONITOR_GONE: &str = "Monitor's output device is not there: Monitor is silent until it comes back or you pick another; Master still plays";

    /// The toast, and the banner while nothing records the Master source, when the device the
    /// setup names for Monitor (`device`, its description) is not there and no mix plays to a
    /// device: only the Master source can drive the mixer.
    pub(crate) fn monitor_waits(device: &str) -> String {
        format!(
            "Monitor's output device \u{201c}{device}\u{201d} is not there and no mix plays to a device: the mixer waits for it, and until then runs only while an app records \u{201c}{DISPLAY_NAME} Master\u{201d}; plug it in or pick another output for Monitor"
        )
    }

    /// The banner while nothing drives the mixer and the device the setup names for Monitor
    /// (`device`, its description) is not there.
    pub(crate) fn stopped_waiting(device: &str) -> String {
        format!(
            "The mixer is stopped: Monitor's output device \u{201c}{device}\u{201d} is not there and no other output runs; the mixer waits for it: plug it in or pick another output for Monitor"
        )
    }

    pub(crate) struct PipeWire {
        /// `None` once it has left.
        backend: Option<Backend>,
        events: Receiver<BackendEvent>,
        app_name: &'static str,
        /// The quantum asked for (`node.latency`), at connect or since in place.
        quantum: u32,
        /// The rate and quantum since the tick last ran its first cycle on this connection.
        running: Option<(u32, u32)>,
        /// The daemon went away and has not come back yet.
        lost: bool,
        /// Which output drives the engine; `None` until the backend says.
        tick: Option<TickSource>,
        /// The Monitor device [`MONITOR_GONE`] or [`monitor_waits`] was said for, until
        /// Monitor plays again: said once, not again at each reconnect while it stays away.
        told_gone: Option<String>,
        /// The device the setup names for Monitor while another output, or none, has the tick
        /// and the backend does not report that device: the idle banners name it.
        waiting_for: Option<DeviceRef>,
        /// The newest routing the host asked for, per app stream playing now.
        routed: Vec<(u32, Option<String>)>,
        /// The engine's frame count, and whether it moved within [`TICK_GAP`].
        frames: u64,
        moved_at: Instant,
        ticking: bool,
    }

    impl PipeWire {
        /// Connects as `app_name` asking for the setup's quantum, and applies its graph.
        pub fn connect(
            app_name: &'static str,
            engine: &Arc<Engine>,
            setup: &Setup,
        ) -> Result<PipeWire, String> {
            let (tx, events) = sync_channel(EVENTS);
            let config = BackendConfig {
                app_name,
                wanted_quantum: setup.prefs.quantum,
            };
            let backend = Backend::connect(config, Arc::clone(engine), tx)
                .map_err(|error| format!("No audio backend: {error}"))?;
            backend.apply(&graph(setup));
            log::info!(
                "connected to PipeWire as {app_name}, asking for {} frames",
                config.wanted_quantum
            );
            Ok(PipeWire {
                backend: Some(backend),
                events,
                app_name,
                quantum: config.wanted_quantum,
                running: None,
                lost: false,
                tick: None,
                told_gone: None,
                waiting_for: None,
                routed: Vec::new(),
                frames: engine.stats().frames.load(Ordering::Relaxed),
                moved_at: Instant::now(),
                ticking: false,
            })
        }

        /// Connected but not running yet, running, idle with why (naming Monitor's device when
        /// the mixer waits for it), or lost.
        pub fn state(&self) -> BackendState {
            if self.lost {
                return BackendState::Disconnected;
            }
            let waiting = self.waiting_for.as_ref().map(|d| d.description.as_str());
            match (self.tick, self.running) {
                (None, _) => BackendState::Connecting,
                (Some(TickSource::Stopped), _) => BackendState::Idle {
                    reason: waiting.map_or_else(|| STOPPED.to_string(), stopped_waiting),
                },
                (Some(TickSource::MasterSource), _) if !self.ticking => BackendState::Idle {
                    reason: waiting.map_or_else(only_recorded, monitor_waits),
                },
                (_, Some((rate, quantum))) => BackendState::Running { rate, quantum },
                _ => BackendState::Connecting,
            }
        }

        pub fn poll(
            &mut self,
            setup: &Setup,
            engine: &Engine,
            devices: &mut Vec<DeviceInfo>,
            streams: &mut Vec<Stream>,
            heard: &mut Heard,
            now: Instant,
        ) {
            for _ in 0..EVENTS_PER_POLL {
                let Ok(event) = self.events.try_recv() else {
                    break;
                };
                self.hear(event, setup, engine, devices, streams, heard);
            }
            let frames = engine.stats().frames.load(Ordering::Relaxed);
            if frames != self.frames {
                self.frames = frames;
                self.moved_at = now;
            }
            self.ticking = now.saturating_duration_since(self.moved_at) < TICK_GAP;
        }

        fn hear(
            &mut self,
            event: BackendEvent,
            setup: &Setup,
            engine: &Engine,
            devices: &mut Vec<DeviceInfo>,
            streams: &mut Vec<Stream>,
            heard: &mut Heard,
        ) {
            match event {
                BackendEvent::Connected { rate, quantum } => {
                    // Every plugin is set up again at the graph's rate (change request 5).
                    engine.set_rate(rate);
                    self.running = Some((rate, quantum));
                    self.lost = false;
                }
                BackendEvent::Disconnected => {
                    self.running = None;
                    self.lost = true;
                }
                BackendEvent::QuantumChanged(now) => {
                    if let Some((_, quantum)) = &mut self.running {
                        *quantum = now;
                    }
                }
                BackendEvent::TickSource(source) => {
                    // The backend sends it once per connection: one that is not `Stopped`
                    // says the daemon is back, even before a tick can run.
                    if source != TickSource::Stopped {
                        self.lost = false;
                    }
                    self.tick = Some(source);
                    self.follow(setup, devices);
                    match (source, &self.waiting_for) {
                        // Monitor plays: a later loss of its device is news again.
                        (TickSource::Monitor, _) => self.told_gone = None,
                        // Another output ticks while the device the setup names for Monitor
                        // is not there: said once for that device, in words for whichever
                        // output ticks. Before Monitor's device is picked (a first start)
                        // there is nothing to say, nor while the backend reports it (Monitor's
                        // stream refused on a device that is there): both texts say it is not
                        // there, and the backend connects that stream again each second.
                        (TickSource::MasterOut | TickSource::MasterSource, Some(device))
                            if self.told_gone.as_deref() != Some(device.name.as_str()) =>
                        {
                            heard.toasts.push(if source == TickSource::MasterOut {
                                MONITOR_GONE.to_string()
                            } else {
                                monitor_waits(&device.description)
                            });
                            self.told_gone = Some(device.name.clone());
                        }
                        _ => {}
                    }
                }
                BackendEvent::DeviceAdded(info) => {
                    // Devices are reported again only once the daemon is back.
                    self.lost = false;
                    devices.retain(|d| d.device.name != info.device.name);
                    if devices.len() < MAX_DEVICES {
                        devices.push(info);
                    }
                    heard.devices = true;
                    self.follow(setup, devices);
                }
                BackendEvent::DeviceRemoved(name) => {
                    devices.retain(|d| d.device.name != name);
                    heard.devices = true;
                    self.follow(setup, devices);
                }
                BackendEvent::AppStreamAdded(stream) => {
                    streams.retain(|s| s.node != stream.node);
                    if streams.len() < MAX_STREAMS {
                        // Apply-on-appear: to its channel, or home, whatever the session
                        // manager restored.
                        let to = target(setup, self.app_name, &stream.app);
                        self.route(stream.node, to);
                        streams.push(Stream {
                            node: stream.node,
                            app: stream.app,
                        });
                    }
                    heard.apps = true;
                }
                BackendEvent::AppStreamRemoved(node) => {
                    streams.retain(|s| s.node != node);
                    self.routed.retain(|(n, _)| *n != node);
                    heard.apps = true;
                }
                // Counted in the engine's stats, which the header shows.
                BackendEvent::Xrun => {}
            }
        }

        /// Keeps [`PipeWire::waiting_for`] true to the newest tick source, setup and devices:
        /// Master out or the Master source ticks, or nothing does, while the device the setup
        /// names for Monitor is not among those the backend reports. `None` once Monitor
        /// plays, or before Monitor's device is picked.
        fn follow(&mut self, setup: &Setup, devices: &[DeviceInfo]) {
            let away = matches!(
                self.tick,
                Some(TickSource::MasterOut | TickSource::MasterSource | TickSource::Stopped)
            );
            let reported = |name: &str| devices.iter().any(|d| d.device.name == name);
            self.waiting_for = setup
                .monitor
                .device
                .as_ref()
                .filter(|monitor| away && !reported(monitor.name.as_str()))
                .cloned();
        }

        fn route(&mut self, node: u32, to: Option<String>) {
            if let Some(backend) = &self.backend {
                backend.route(node, to.as_deref());
            }
            self.routed.retain(|(n, _)| *n != node);
            self.routed.push((node, to));
        }

        /// The quantum first, so a stream the graph makes asks for it from the start. Another
        /// device picked for Monitor is the one the banner names while it is not there (the
        /// backend says nothing new when the tick stays where it is).
        pub fn apply_graph(&mut self, setup: &Setup, devices: &[DeviceInfo]) {
            if let Some(backend) = &self.backend {
                if setup.prefs.quantum != self.quantum {
                    backend.set_quantum(setup.prefs.quantum);
                }
                backend.apply(&graph(setup));
            }
            self.quantum = setup.prefs.quantum;
            self.follow(setup, devices);
        }

        pub fn reroute(&mut self, setup: &Setup, streams: &[Stream]) {
            for stream in streams {
                let to = target(setup, self.app_name, &stream.app);
                let now = self
                    .routed
                    .iter()
                    .find(|(n, _)| *n == stream.node)
                    .map(|(_, to)| to);
                if now != Some(&to) {
                    self.route(stream.node, to);
                }
            }
        }

        /// Every app the host sent to a channel goes home, then the backend leaves, after the
        /// daemon has those writes (change request 8).
        pub fn shutdown(&mut self) {
            let Some(backend) = self.backend.take() else {
                return;
            };
            for (node, to) in &self.routed {
                if to.is_some() {
                    backend.route(*node, None);
                }
            }
            backend.disconnect();
            self.routed.clear();
            self.running = None;
            self.tick = Some(TickSource::Stopped);
        }
    }

    /// The nodes the setup asks for: one input per channel in the console's order, the two
    /// outputs' devices, and the Master source, always published.
    pub(crate) fn graph(setup: &Setup) -> BackendGraph {
        BackendGraph {
            channels: setup
                .channels
                .iter()
                .map(|channel| GraphChannel {
                    id: channel.id,
                    name: channel.name.clone(),
                    input: match &channel.source {
                        Source::Apps(_) => GraphInput::AppSink,
                        Source::Hardware(device) => GraphInput::Capture(device.clone()),
                    },
                })
                .collect(),
            monitor: setup.monitor.device.clone(),
            master: setup.master.device.clone(),
            master_source: true,
        }
    }

    /// The sink `app` plays into: its channel's `node.name`, or `None` for the default.
    pub(crate) fn target(setup: &Setup, app_name: &str, app: &AppRef) -> Option<String> {
        let id = setup.channel_of_app(app)?;
        let channel = setup.channel(id)?;
        Some(channel_node_name(app_name, id, &channel.name))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use vixmix_engine::{EngineConfig, MAX_QUANTUM};
        use vixmix_model::{Command, DeviceDirection, MixId, apply, example};
        use vixmix_pipewire::AppStream;

        /// A backend's host half with no daemon behind it: what `hear` makes of each event.
        struct Offline {
            pw: PipeWire,
            setup: Setup,
            engine: Engine,
            devices: Vec<DeviceInfo>,
            streams: Vec<Stream>,
            heard: Heard,
        }

        impl Offline {
            fn new(setup: Setup) -> Offline {
                let (_tx, events) = sync_channel(1);
                Offline {
                    pw: PipeWire {
                        backend: None,
                        events,
                        app_name: "vix-mix",
                        quantum: 256,
                        running: None,
                        lost: false,
                        tick: None,
                        told_gone: None,
                        waiting_for: None,
                        routed: Vec::new(),
                        frames: 0,
                        moved_at: Instant::now(),
                        ticking: false,
                    },
                    setup,
                    engine: Engine::new(EngineConfig::new(48_000, MAX_QUANTUM, 2)),
                    devices: Vec::new(),
                    streams: Vec::new(),
                    heard: Heard::default(),
                }
            }

            fn hear(&mut self, event: BackendEvent) -> BackendState {
                self.pw.hear(
                    event,
                    &self.setup,
                    &self.engine,
                    &mut self.devices,
                    &mut self.streams,
                    &mut self.heard,
                );
                self.pw.state()
            }

            fn routed(&self, node: u32) -> Option<Option<&str>> {
                self.pw
                    .routed
                    .iter()
                    .find(|(n, _)| *n == node)
                    .map(|(_, to)| to.as_deref())
            }
        }

        #[test]
        fn the_state_follows_what_the_backend_says() {
            let mut pw = Offline::new(Setup::default());
            assert_eq!(pw.pw.state(), BackendState::Connecting);
            let idle = pw.hear(BackendEvent::TickSource(TickSource::MasterSource));
            assert_eq!(
                idle,
                BackendState::Idle {
                    reason: only_recorded()
                },
                "only the Master source, and nothing records it"
            );
            assert!(idle.banner().unwrap().contains("Vix Mix Master"));
            assert_eq!(
                pw.hear(BackendEvent::TickSource(TickSource::Monitor)),
                BackendState::Connecting
            );
            let running = BackendState::Running {
                rate: 48_000,
                quantum: 256,
            };
            assert_eq!(
                pw.hear(BackendEvent::Connected {
                    rate: 48_000,
                    quantum: 256
                }),
                running
            );
            assert_eq!(
                pw.hear(BackendEvent::QuantumChanged(512)),
                BackendState::Running {
                    rate: 48_000,
                    quantum: 512
                }
            );
            // A first start, before Monitor's device is picked: Master out ticks, and nothing
            // is said (the model refuses taking Monitor's device away once it has one).
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterOut));
            assert!(
                pw.heard.toasts.is_empty(),
                "Monitor has no device yet: {:?}",
                pw.heard.toasts
            );
            assert_eq!(
                pw.hear(BackendEvent::Disconnected),
                BackendState::Disconnected
            );
            assert_eq!(
                pw.hear(BackendEvent::TickSource(TickSource::Stopped)),
                BackendState::Disconnected,
                "the backend's own Stopped after a loss is still the loss"
            );
            let device = DeviceInfo {
                device: DeviceRef {
                    name: "alsa.headphones".into(),
                    description: "Headphones".into(),
                },
                direction: DeviceDirection::Output,
            };
            assert_eq!(
                pw.hear(BackendEvent::DeviceAdded(device)),
                BackendState::Idle {
                    reason: STOPPED.to_string()
                },
                "back, and nothing drives the mixer yet"
            );
            assert_eq!(
                pw.hear(BackendEvent::TickSource(TickSource::Monitor)),
                BackendState::Connecting,
                "back, waiting for the first tick of this connection"
            );
            assert_eq!(pw.devices.len(), 1);
            let _ = pw.hear(BackendEvent::DeviceRemoved("alsa.headphones".into()));
            assert!(pw.devices.is_empty() && pw.heard.devices);
        }

        #[test]
        fn monitor_gone_is_said_once_when_the_device_the_setup_names_is_not_there() {
            let headphones = DeviceRef {
                name: "alsa.headphones".into(),
                description: "Headphones".into(),
            };
            let mut setup = Setup::default();
            let _ = apply(
                &mut setup,
                Command::SetMixDevice {
                    mix: MixId::Monitor,
                    device: Some(headphones.clone()),
                },
            )
            .unwrap();
            let mut pw = Offline::new(setup.clone());
            let gone = || vec![MONITOR_GONE.to_string()];
            // Started with the headphones unplugged: said once.
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterOut));
            assert_eq!(pw.heard.toasts, gone());
            // The daemon restarts while they are still away: not said again.
            let _ = pw.hear(BackendEvent::Disconnected);
            let _ = pw.hear(BackendEvent::TickSource(TickSource::Stopped));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterOut));
            assert_eq!(pw.heard.toasts, gone());
            // Plugged in, Monitor plays; unplugged again, said again.
            let _ = pw.hear(BackendEvent::TickSource(TickSource::Monitor));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterOut));
            assert_eq!(pw.heard.toasts, [gone(), gone()].concat());
            // Plugged in, Monitor plays, then its stream is refused while the backend still
            // reports the headphones: they are there, so nothing calls them missing.
            let _ = pw.hear(BackendEvent::DeviceAdded(DeviceInfo {
                device: headphones.clone(),
                direction: DeviceDirection::Output,
            }));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::Monitor));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterOut));
            assert_eq!(pw.heard.toasts, [gone(), gone()].concat());

            // With Master on "No device" the Master source ticks instead: said once too, in
            // words that name the device and do not say Master plays.
            let mut pw = Offline::new(setup);
            let waits = || vec![monitor_waits("Headphones")];
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterSource));
            assert_eq!(pw.heard.toasts, waits());
            assert!(
                pw.heard.toasts[0].contains("\u{201c}Headphones\u{201d}")
                    && !pw.heard.toasts[0].contains("Master still plays"),
                "{:?}",
                pw.heard.toasts
            );
            // The daemon restarts while they are still away: not said again.
            let _ = pw.hear(BackendEvent::Disconnected);
            let _ = pw.hear(BackendEvent::TickSource(TickSource::Stopped));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterSource));
            assert_eq!(pw.heard.toasts, waits());
            // Master given a device and taken off it again while they are away: nothing new.
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterOut));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterSource));
            assert_eq!(pw.heard.toasts, waits());
            // Plugged in, Monitor plays; unplugged again, said again.
            let _ = pw.hear(BackendEvent::DeviceAdded(DeviceInfo {
                device: headphones,
                direction: DeviceDirection::Output,
            }));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::Monitor));
            let _ = pw.hear(BackendEvent::DeviceRemoved("alsa.headphones".into()));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterSource));
            assert_eq!(pw.heard.toasts, [waits(), waits()].concat());
        }

        #[test]
        fn the_idle_banner_names_monitors_device_while_the_mixer_waits_for_it() {
            let headphones = DeviceRef {
                name: "alsa.headphones".into(),
                description: "Headphones".into(),
            };
            let output = |device: &DeviceRef| DeviceInfo {
                device: device.clone(),
                direction: DeviceDirection::Output,
            };
            let mut setup = Setup::default();
            let _ = apply(
                &mut setup,
                Command::SetMixDevice {
                    mix: MixId::Monitor,
                    device: Some(headphones.clone()),
                },
            )
            .unwrap();
            let mut pw = Offline::new(setup);
            let _ = pw.hear(BackendEvent::DeviceAdded(output(&headphones)));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::Monitor));
            let running = BackendState::Running {
                rate: 48_000,
                quantum: 256,
            };
            assert_eq!(
                pw.hear(BackendEvent::Connected {
                    rate: 48_000,
                    quantum: 256
                }),
                running
            );
            // Unplugged with Master on "No device", and nothing records the Master source: the
            // banner names the headphones and says the mixer waits for them.
            let _ = pw.hear(BackendEvent::DeviceRemoved("alsa.headphones".into()));
            let waits = BackendState::Idle {
                reason: monitor_waits("Headphones"),
            };
            assert_eq!(
                pw.hear(BackendEvent::TickSource(TickSource::MasterSource)),
                waits
            );
            let banner = waits.banner().unwrap();
            assert!(
                banner.contains("\u{201c}Headphones\u{201d}") && banner.contains("waits"),
                "{banner}"
            );
            // The Master source given up on too: stopped, and still waiting for them.
            assert_eq!(
                pw.hear(BackendEvent::TickSource(TickSource::Stopped)),
                BackendState::Idle {
                    reason: stopped_waiting("Headphones")
                }
            );
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterSource));
            // Plugged back: Monitor plays, and no banner names them.
            let _ = pw.hear(BackendEvent::DeviceAdded(output(&headphones)));
            assert_eq!(
                pw.hear(BackendEvent::TickSource(TickSource::Monitor)),
                running
            );
            // Monitor's stream given up on while the headphones are there: they are not called
            // missing, on the banner or the toast line.
            assert_eq!(
                pw.hear(BackendEvent::TickSource(TickSource::MasterSource)),
                BackendState::Idle {
                    reason: only_recorded()
                }
            );
            assert_eq!(pw.heard.toasts, [monitor_waits("Headphones")]);
            let _ = pw.hear(BackendEvent::TickSource(TickSource::Monitor));
            // Unplugged again, then another output picked for Monitor while the tick stays on
            // the Master source: the banner follows the setup.
            let _ = pw.hear(BackendEvent::DeviceRemoved("alsa.headphones".into()));
            let _ = pw.hear(BackendEvent::TickSource(TickSource::MasterSource));
            let dac = DeviceRef {
                name: "alsa.usb-dac".into(),
                description: "USB DAC".into(),
            };
            let _ = apply(
                &mut pw.setup,
                Command::SetMixDevice {
                    mix: MixId::Monitor,
                    device: Some(dac),
                },
            )
            .unwrap();
            pw.pw.apply_graph(&pw.setup, &pw.devices);
            assert_eq!(
                pw.pw.state(),
                BackendState::Idle {
                    reason: monitor_waits("USB DAC")
                },
                "a device that is not there either is the one it waits for"
            );
            let speakers = DeviceRef {
                name: "alsa.speakers".into(),
                description: "Speakers".into(),
            };
            let _ = pw.hear(BackendEvent::DeviceAdded(output(&speakers)));
            let _ = apply(
                &mut pw.setup,
                Command::SetMixDevice {
                    mix: MixId::Monitor,
                    device: Some(speakers),
                },
            )
            .unwrap();
            pw.pw.apply_graph(&pw.setup, &pw.devices);
            assert_eq!(
                pw.pw.state(),
                BackendState::Idle {
                    reason: only_recorded()
                },
                "a device that is there is never called missing"
            );
            assert_eq!(
                pw.hear(BackendEvent::TickSource(TickSource::Monitor)),
                running
            );
        }

        #[test]
        fn every_app_is_routed_the_moment_it_appears_to_its_channel_or_home() {
            let mut pw = Offline::new(example::setup());
            let music = pw
                .setup
                .channels
                .iter()
                .find(|c| c.name == "Music")
                .unwrap()
                .clone();
            let Source::Apps(apps) = &music.source else {
                panic!("Music is an app channel");
            };
            let sink = format!("vix-mix.ch{}.music", music.id.0);
            let stranger = AppRef {
                app_id: None,
                binary: Some("nobody".into()),
                name: "Nobody".into(),
            };
            for (node, app) in [(40, apps[0].clone()), (41, stranger)] {
                let _ = pw.hear(BackendEvent::AppStreamAdded(AppStream {
                    node,
                    app,
                    media_name: None,
                }));
            }
            assert!(pw.heard.apps);
            assert_eq!(pw.routed(40), Some(Some(sink.as_str())));
            assert_eq!(
                pw.routed(41),
                Some(None),
                "an app no channel names goes home"
            );
            // A rename routes Music's app to the new sink name; the stranger stays.
            let _ = apply(
                &mut pw.setup,
                Command::Rename {
                    ch: music.id,
                    name: "Tunes".into(),
                },
            )
            .unwrap();
            pw.pw.reroute(&pw.setup, &pw.streams);
            let renamed = format!("vix-mix.ch{}.tunes", music.id.0);
            assert_eq!(pw.routed(40), Some(Some(renamed.as_str())));
            assert_eq!(pw.routed(41), Some(None));
            let _ = pw.hear(BackendEvent::AppStreamRemoved(40));
            assert_eq!(pw.routed(40), None);
            assert_eq!(pw.streams.len(), 1);
        }

        #[test]
        fn the_graph_is_the_setups_channels_in_order_with_the_master_source() {
            let setup = example::setup();
            let graph = graph(&setup);
            assert_eq!(graph.channels.len(), setup.channels.len());
            for (node, channel) in graph.channels.iter().zip(&setup.channels) {
                assert_eq!((node.id, &node.name), (channel.id, &channel.name));
                let sink = matches!(channel.source, Source::Apps(_));
                assert_eq!(node.input == GraphInput::AppSink, sink, "{}", channel.name);
            }
            assert_eq!(graph.monitor, setup.monitor.device);
            assert_eq!(graph.master, setup.master.device);
            assert!(graph.master_source, "Master is always a virtual source");
        }

        #[test]
        fn an_app_goes_to_its_channels_sink_or_home() {
            let mut setup = example::setup();
            let music = setup
                .channels
                .iter()
                .find(|c| c.name == "Music")
                .unwrap()
                .clone();
            let Source::Apps(apps) = &music.source else {
                panic!("Music is an app channel");
            };
            assert_eq!(
                target(&setup, "vix-mix", &apps[0]).as_deref(),
                Some(format!("vix-mix.ch{}.music", music.id.0).as_str())
            );
            let stranger = AppRef {
                app_id: None,
                binary: Some("nobody".into()),
                name: "Nobody".into(),
            };
            assert_eq!(target(&setup, "vix-mix", &stranger), None);
            let _ = apply(
                &mut setup,
                Command::Rename {
                    ch: music.id,
                    name: "Tunes".into(),
                },
            )
            .unwrap();
            assert_eq!(
                target(&setup, "vix-mix", &apps[0]),
                Some(format!("vix-mix.ch{}.tunes", music.id.0))
            );
            let _ = apply(&mut setup, Command::RemoveChannel(music.id)).unwrap();
            assert_eq!(target(&setup, "vix-mix", &apps[0]), None);
        }
    }
}
