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
    Null(Null),
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

    /// The setup's graph changed: the backend makes its nodes match. A new quantum connects
    /// the backend again (`node.latency` is a stream's property from its making), which
    /// reports every device and app afresh.
    pub fn apply_graph(&mut self, setup: &Setup, engine: &Arc<Engine>) -> Heard {
        #[cfg(target_os = "linux")]
        if let Kind::PipeWire(pw) = &mut self.kind {
            if pw.quantum() == setup.prefs.quantum {
                pw.apply_graph(setup);
                return Heard::default();
            }
            let app_name = pw.app_name();
            pw.shutdown();
            self.devices.clear();
            self.streams.clear();
            self.kind = system(app_name, engine, setup);
            return Heard {
                devices: true,
                apps: true,
                toasts: Vec::new(),
            };
        }
        let _ = (setup, engine);
        Heard::default()
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
    Kind::Null(Null {
        pump: NullBackend::new(Arc::clone(engine), NULL_QUANTUM),
        reason,
        retry,
    })
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
    use vixmix_model::{AppRef, DeviceInfo, Setup, Source};
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

    /// The toast when Monitor's device goes and Master's output drives the mixer.
    pub(crate) const MONITOR_GONE: &str = "Monitor's output device is not there: Monitor is silent until it comes back or you pick another; Master still plays";

    pub(crate) struct PipeWire {
        /// `None` once it has left.
        backend: Option<Backend>,
        events: Receiver<BackendEvent>,
        app_name: &'static str,
        /// The quantum asked for at connect (`node.latency`).
        quantum: u32,
        /// The rate and quantum since the tick last ran its first cycle on this connection.
        running: Option<(u32, u32)>,
        /// The daemon went away and has not come back yet.
        lost: bool,
        /// Which output drives the engine; `None` until the backend says.
        tick: Option<TickSource>,
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
                routed: Vec::new(),
                frames: engine.stats().frames.load(Ordering::Relaxed),
                moved_at: Instant::now(),
                ticking: false,
            })
        }

        pub fn app_name(&self) -> &'static str {
            self.app_name
        }

        pub fn quantum(&self) -> u32 {
            self.quantum
        }

        /// Connected but not running yet, running, idle with why, or lost.
        pub fn state(&self) -> BackendState {
            if self.lost {
                return BackendState::Disconnected;
            }
            match (self.tick, self.running) {
                (None, _) => BackendState::Connecting,
                (Some(TickSource::Stopped), _) => BackendState::Idle {
                    reason: STOPPED.to_string(),
                },
                (Some(TickSource::MasterSource), _) if !self.ticking => BackendState::Idle {
                    reason: only_recorded(),
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
                    if source == TickSource::MasterOut && self.tick != Some(source) {
                        heard.toasts.push(MONITOR_GONE.to_string());
                    }
                    self.tick = Some(source);
                }
                BackendEvent::DeviceAdded(info) => {
                    devices.retain(|d| d.device.name != info.device.name);
                    if devices.len() < MAX_DEVICES {
                        devices.push(info);
                    }
                    heard.devices = true;
                }
                BackendEvent::DeviceRemoved(name) => {
                    devices.retain(|d| d.device.name != name);
                    heard.devices = true;
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

        fn route(&mut self, node: u32, to: Option<String>) {
            if let Some(backend) = &self.backend {
                backend.route(node, to.as_deref());
            }
            self.routed.retain(|(n, _)| *n != node);
            self.routed.push((node, to));
        }

        pub fn apply_graph(&mut self, setup: &Setup) {
            if let Some(backend) = &self.backend {
                backend.apply(&graph(setup));
            }
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
        use vixmix_model::{Command, apply, example};

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
