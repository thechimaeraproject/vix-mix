//! Linux only: the mixer as a PipeWire client. A virtual sink per app channel, a capture per
//! hardware channel, the Monitor and Master outputs, Master as a virtual source, app
//! discovery and routing through the `default` metadata, device enumeration, reconnect — and
//! [`Engine::process`](vixmix_engine::Engine::process) driven from PipeWire's data thread.
//!
//! On every other target this crate is empty, so the workspace builds and tests on Windows
//! without libpipewire or libclang.
//!
//! **Threads.** [`Backend::connect`] starts one control thread, `vix-mix-pipewire`, that owns
//! the PipeWire main loop, the context, the core, the registry, the `default` metadata and
//! every stream; nothing PipeWire hands out leaves it. The host talks to it through a mailbox
//! woken by a `pipewire::channel`, and hears from it through the bounded event channel
//! `connect` was given. Every stream's `process` runs on the context's one data thread: the
//! inputs copy into their rings, and the tick governs the rings, calls
//! [`Engine::process`](vixmix_engine::Engine::process) and writes the outputs, allocating,
//! locking and logging nothing.
//!
//! See `docs/reference/pipewire.md` for the contract, the node and property table, the two
//! PipeWire facts the design rests on and the forbidden list for the callbacks.

#![cfg(target_os = "linux")]

mod control;
mod registry;
mod rt;
mod streams;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use vixmix_engine::Engine;
use vixmix_model::{AppRef, ChannelId, DeviceInfo, DeviceRef};

/// `application.name` on every stream, and the prefix of every node name.
pub const APP_NAME: &str = "vix-mix";

/// What the user reads beside our node names: the prefix of every `node.description`.
pub const DISPLAY_NAME: &str = "Vix Mix";

/// How the backend presents itself and what it asks of the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendConfig {
    /// `application.name`, `node.group`, `node.link-group` and the node-name prefix.
    pub app_name: &'static str,
    /// The quantum asked for through `node.latency` (`<quantum>/48000`): a request, never
    /// `node.force-quantum`.
    pub wanted_quantum: u32,
}

impl Default for BackendConfig {
    fn default() -> BackendConfig {
        BackendConfig {
            app_name: APP_NAME,
            wanted_quantum: 256,
        }
    }
}

/// What the backend tells the host, through a bounded channel.
#[derive(Clone, Debug, PartialEq)]
pub enum BackendEvent {
    /// The tick ran its first cycle since the daemon answered: the graph runs at this rate and
    /// quantum.
    Connected { rate: u32, quantum: u32 },
    /// The daemon went away; the backend reconnects with backoff.
    Disconnected,
    /// A device that is not ours appeared.
    DeviceAdded(DeviceInfo),
    /// A device went away, by `node.name`.
    DeviceRemoved(String),
    /// An app's playback stream appeared.
    AppStreamAdded(AppStream),
    /// An app's playback stream went away, by node id.
    AppStreamRemoved(u32),
    /// A cycle came late or short (one event per housekeeping pass that saw any; the count is
    /// [`BackendStats::xruns`] and the engine's `Stats::xruns`). The backend's own detection,
    /// not PipeWire's count: a cycle our nodes finished late while the driver's clock went on
    /// as usual shows in `pw-top`'s ERR column and never here.
    Xrun,
    /// The graph's quantum changed (another client asked for less, or the default moved).
    QuantumChanged(u32),
    /// Which output now drives the engine. [`TickSource::MasterSource`] and
    /// [`TickSource::Stopped`] are states the window shows: the engine runs only while
    /// something records Master, or not at all.
    TickSource(TickSource),
}

/// The output stream whose cycles run the engine (the tick election,
/// `docs/reference/pipewire.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TickSource {
    /// The Monitor output, while it plays to its device: the normal case.
    Monitor,
    /// The Master output: Monitor has no device or its device went away.
    MasterOut,
    /// The Master source: no output plays to a device, so the engine runs only while
    /// something records `<app>.master`.
    MasterSource,
    /// No output stream exists: the engine does not run.
    #[default]
    Stopped,
}

/// An app's playback stream as the registry reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppStream {
    /// The stream's node id: what [`Backend::route`] takes. Live only; never stored.
    pub node: u32,
    /// Its identity: `pipewire.access.portal.app_id`, `application.process.binary`,
    /// `application.name`.
    pub app: AppRef,
    /// `media.name`: what it is playing, when it says.
    pub media_name: Option<String>,
}

/// The nodes the backend keeps, derived from the setup by the host.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackendGraph {
    /// One input per channel, in the console's order (the engine's input order). At most
    /// [`MAX_CHANNELS`](vixmix_model::MAX_CHANNELS); the rest are ignored.
    pub channels: Vec<GraphChannel>,
    /// The Monitor output's device.
    pub monitor: Option<DeviceRef>,
    /// The Master output's device; `None` is "No device".
    pub master: Option<DeviceRef>,
    /// Whether the Master source (`<app>.master`, `Audio/Source`) is published.
    pub master_source: bool,
}

/// One channel's input node.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphChannel {
    pub id: ChannelId,
    /// `node.description`: the channel's name.
    pub name: String,
    pub input: GraphInput,
}

/// What kind of input a channel needs.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphInput {
    /// A virtual sink apps are routed to (`media.class = Audio/Sink`).
    AppSink,
    /// A capture stream on a device (`target.object`); `None` until one is picked.
    Capture(Option<DeviceRef>),
}

/// The backend's counters since [`Backend::connect`], read without stopping anything: what
/// the data thread saw. The acceptance run reads them (`docs/reference/pipewire.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackendStats {
    /// Which output drives the engine now.
    pub tick_source: TickSource,
    /// Cycles the engine ran.
    pub ticks: u64,
    /// The graph's rate and the last tick's quantum (0 before the first tick).
    pub rate: u32,
    pub quantum: u32,
    /// Cycles that came late or short: a running input that delivered less than a quantum by
    /// its tick, or a tick more than one and a half periods after the one before. This is what
    /// the data thread can see for itself; it misses a cycle our nodes finished after the
    /// driver's next one began while the clock advanced normally, which PipeWire counts (the
    /// ERR column of `pw-top`), so it may read lower than what was heard.
    pub xruns: u64,
    /// The part of `xruns` that were inputs short at their tick.
    pub underruns: u64,
    /// Times an input's ring was full when its stream delivered (nothing ticked).
    pub overflows: u64,
    /// Checks of a running input's ring at a tick, and how many of them found the ring not
    /// holding exactly one quantum ahead of the tick (the latency governor had to act).
    pub ring_checks: u64,
    pub ring_off: u64,
    /// Cycles each output wrote the mix the tick computed in that same cycle.
    pub monitor_cycles: u64,
    pub master_out_cycles: u64,
    pub master_source_cycles: u64,
    /// Cycles an output ran without a mix from that cycle and wrote silence.
    pub stale_cycles: u64,
}

/// Why the backend could not start. Each message ends with what to do.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error(
        "PipeWire refused to {what} ({source}): check `systemctl --user status pipewire wireplumber`, and that XDG_RUNTIME_DIR points at the session the daemon runs in"
    )]
    PipeWire {
        what: &'static str,
        #[source]
        source: pipewire::Error,
    },
}

/// What the host asks of the control thread: the newest graph, and the newest routing for
/// each app node. Latest wins for both, so the mailbox stays bounded.
#[derive(Default)]
pub(crate) struct Mailbox {
    pub graph: Option<BackendGraph>,
    pub routes: Vec<(u32, Option<String>)>,
    pub quit: bool,
}

/// The most routings that wait in the mailbox: routing more apps than this between two passes
/// of the control thread drops the oldest.
const MAX_PENDING_ROUTES: usize = 256;

/// The host's side of the mailbox, shared with the control thread.
pub(crate) struct Inbox {
    mailbox: Mutex<Mailbox>,
    /// One wake per batch: the control thread empties the whole mailbox on each.
    wake_pending: AtomicBool,
}

impl Inbox {
    /// Control thread: everything posted since the last take.
    pub(crate) fn take(&self) -> Mailbox {
        self.wake_pending.store(false, Ordering::SeqCst);
        std::mem::take(&mut *self.mailbox.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// The PipeWire client: one control thread with the main loop, the context and the core;
/// every stream's `process` on the context's data thread.
pub struct Backend {
    inbox: Arc<Inbox>,
    wake: pipewire::channel::Sender<()>,
    shared: Arc<rt::Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Backend {
    /// Connects to the daemon and starts the control thread; the streams follow the first
    /// [`apply`](Backend::apply). Reports through `events`, a bounded channel: when the host
    /// lags, events wait in a bounded backlog on the control thread rather than block it.
    ///
    /// Fails when the daemon does not answer; a daemon lost later is reported as
    /// [`BackendEvent::Disconnected`] and reconnected with backoff.
    pub fn connect(
        config: BackendConfig,
        engine: Arc<Engine>,
        events: SyncSender<BackendEvent>,
    ) -> Result<Backend, BackendError> {
        let inbox = Arc::new(Inbox {
            mailbox: Mutex::new(Mailbox::default()),
            wake_pending: AtomicBool::new(false),
        });
        let (plans, shared) = rt::Shared::new(engine);
        let (wake, wake_rx) = pipewire::channel::channel::<()>();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let refused = || BackendError::PipeWire {
            what: "start its control thread",
            source: pipewire::Error::CreationFailed,
        };
        let thread = {
            let inbox = Arc::clone(&inbox);
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("vix-mix-pipewire".into())
                .spawn(move || {
                    control::run(control::Start {
                        config,
                        shared,
                        plans,
                        events,
                        inbox,
                        wake: wake_rx,
                        ready: ready_tx,
                    })
                })
                .map_err(|_| refused())?
        };
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Backend {
                inbox,
                wake,
                shared,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(refused())
            }
        }
    }

    /// Makes the streams match `graph`: creates, destroys or retargets only what changed.
    pub fn apply(&self, graph: &BackendGraph) {
        self.post(|mailbox| mailbox.graph = Some(graph.clone()));
    }

    /// Routes the app stream `app_node` to the sink named `to` (a channel's `node.name`), or
    /// back to the default with `None`, through the `default` metadata's `target.object`. A
    /// graph posted before the next pass is applied first: `to` names a sink of the newest
    /// graph.
    pub fn route(&self, app_node: u32, to: Option<&str>) {
        self.post(|mailbox| {
            mailbox.routes.retain(|(node, _)| *node != app_node);
            if mailbox.routes.len() >= MAX_PENDING_ROUTES {
                mailbox.routes.remove(0);
            }
            mailbox.routes.push((app_node, to.map(str::to_string)));
        });
    }

    /// The counters the data thread keeps; see [`BackendStats`].
    pub fn stats(&self) -> BackendStats {
        self.shared.stats()
    }

    /// Destroys every stream and leaves the daemon. Routings are left as they are, except that
    /// the ones posted before this call are written first: the backend leaves once the daemon
    /// has answered a roundtrip behind them (at most a second), so a host may clear its
    /// routings and disconnect at once.
    pub fn disconnect(mut self) {
        self.stop();
    }

    fn post(&self, change: impl FnOnce(&mut Mailbox)) {
        change(
            &mut self
                .inbox
                .mailbox
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        if !self.inbox.wake_pending.swap(true, Ordering::SeqCst) {
            // Fails only once the control thread has stopped: then nobody is left to wake.
            let _ = self.wake.send(());
        }
    }

    fn stop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        self.inbox
            .mailbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .quit = true;
        let _ = self.wake.send(());
        let _ = thread.join();
    }
}

// The host may keep the backend on any thread and share it.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<Backend>();
};

impl Drop for Backend {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A channel's sink or capture `node.name`: `<app>.ch<id>.<slug of the channel's name>`. The
/// id keeps two channels whose names slug alike apart; the slug is for whoever reads
/// `pw-top`. A rename makes the sink again, under this name whether or not it changed, and the
/// backend moves the old sink's apps to the new one.
pub fn channel_node_name(app_name: &str, id: ChannelId, channel_name: &str) -> String {
    format!("{app_name}.ch{}.{}", id.0, slug(channel_name))
}

/// The Master source's `node.name`: `<app>.master`.
pub fn master_node_name(app_name: &str) -> String {
    format!("{app_name}.master")
}

/// The Monitor output's `node.name`: `<app>.monitor-out`.
pub fn monitor_out_node_name(app_name: &str) -> String {
    format!("{app_name}.monitor-out")
}

/// The Master output's `node.name`: `<app>.master-out`.
pub fn master_out_node_name(app_name: &str) -> String {
    format!("{app_name}.master-out")
}

/// Lower-case ASCII letters and digits, every other run of characters one `-`, none at the
/// ends; `channel` when nothing is left.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "channel".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_names_are_the_app_the_channel_id_and_a_slug_of_its_name() {
        assert_eq!(
            channel_node_name(APP_NAME, ChannelId(3), "Voice chat"),
            "vix-mix.ch3.voice-chat"
        );
        assert_eq!(
            channel_node_name(APP_NAME, ChannelId(1), "  Mic #2 (Wave:3) "),
            "vix-mix.ch1.mic-2-wave-3"
        );
        assert_eq!(
            channel_node_name(APP_NAME, ChannelId(7), "\u{1f3b5}"),
            "vix-mix.ch7.channel"
        );
        assert_eq!(master_node_name(APP_NAME), "vix-mix.master");
        assert_eq!(monitor_out_node_name(APP_NAME), "vix-mix.monitor-out");
        assert_eq!(master_out_node_name(APP_NAME), "vix-mix.master-out");
    }

    #[test]
    fn two_channels_whose_names_slug_alike_get_distinct_node_names() {
        let a = channel_node_name(APP_NAME, ChannelId(1), "Music!");
        let b = channel_node_name(APP_NAME, ChannelId(2), "music");
        assert_ne!(a, b);
        assert!(a.ends_with(".music") && b.ends_with(".music"), "{a} {b}");
    }
}
