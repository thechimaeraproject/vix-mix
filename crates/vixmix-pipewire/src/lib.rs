//! Linux only: the mixer as a PipeWire client. A virtual sink per app channel, a capture per
//! hardware channel, the Monitor and Master outputs, Master as a virtual source, app
//! discovery and routing through the `default` metadata, device enumeration, reconnect — and
//! [`Engine::process`](vixmix_engine::Engine::process) driven from PipeWire's data thread.
//!
//! On every other target this crate is empty, so the workspace builds and tests on Windows
//! without libpipewire or libclang.
//!
//! **Not built yet.** This is the contract the host and the other lanes compile against:
//! [`BackendConfig`], [`BackendEvent`], [`BackendGraph`] and [`Backend`]'s signatures.
//! [`Backend::connect`] answers [`BackendError::NotBuilt`] until handoff 03
//! (`docs/handoffs/03-pipewire-backend.md`) builds it, and the host falls back to the null
//! backend with that message on its banner.
//!
//! See `docs/reference/pipewire.md`.

#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use vixmix_engine::Engine;
use vixmix_model::{AppRef, ChannelId, DeviceInfo, DeviceRef};

/// `application.name` on every stream, and the prefix of every node name.
pub const APP_NAME: &str = "vix-mix";

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
    /// The daemon answered and the graph runs at this rate and quantum.
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
    /// A cycle came late or short.
    Xrun,
    /// The graph's quantum changed (another client asked for less, or the default moved).
    QuantumChanged(u32),
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
    /// One input per channel, in the console's order (the engine's input order).
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

/// Why the backend could not start. Each message ends with what to do.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error(
        "the PipeWire backend is not built yet (docs/handoffs/03-pipewire-backend.md): vix-mix runs silent on the null backend until it is"
    )]
    NotBuilt,
    #[error(
        "PipeWire refused to {what} ({source}): check `systemctl --user status pipewire wireplumber`, and that XDG_RUNTIME_DIR points at the session the daemon runs in"
    )]
    PipeWire {
        what: &'static str,
        #[source]
        source: pipewire::Error,
    },
}

/// The PipeWire client: one thread loop, one context with a single data loop, one core.
pub struct Backend {
    _not_built: (),
}

impl Backend {
    /// Connects to the daemon, creates the streams and starts driving `engine` from the
    /// Monitor output's data thread; reports through `events`.
    pub fn connect(
        config: BackendConfig,
        engine: Arc<Engine>,
        events: SyncSender<BackendEvent>,
    ) -> Result<Backend, BackendError> {
        let _ = (config, engine, events);
        Err(BackendError::NotBuilt)
    }

    /// Makes the streams match `graph`: creates, destroys or retargets only what changed.
    pub fn apply(&self, graph: &BackendGraph) {
        let _ = graph;
    }

    /// Routes the app stream `app_node` to the sink named `to` (a channel's `node.name`), or
    /// back to the default with `None`, through the `default` metadata's `target.object`.
    pub fn route(&self, app_node: u32, to: Option<&str>) {
        let _ = (app_node, to);
    }

    /// Destroys every stream and leaves the daemon. Routings are left as they are.
    pub fn disconnect(self) {}
}

/// A channel's sink `node.name`: `<app>.ch.<slug of the channel's name>`.
pub fn channel_node_name(app_name: &str, channel_name: &str) -> String {
    format!("{app_name}.ch.{}", slug(channel_name))
}

/// The Master source's `node.name`: `<app>.master`.
pub fn master_node_name(app_name: &str) -> String {
    format!("{app_name}.master")
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
    use vixmix_engine::{EngineConfig, MAX_QUANTUM};

    #[test]
    fn node_names_are_the_app_and_a_slug_of_the_channel() {
        assert_eq!(
            channel_node_name(APP_NAME, "Voice chat"),
            "vix-mix.ch.voice-chat"
        );
        assert_eq!(
            channel_node_name(APP_NAME, "  Mic #2 (Wave:3) "),
            "vix-mix.ch.mic-2-wave-3"
        );
        assert_eq!(
            channel_node_name(APP_NAME, "\u{1f3b5}"),
            "vix-mix.ch.channel"
        );
        assert_eq!(master_node_name(APP_NAME), "vix-mix.master");
    }

    #[test]
    fn until_handoff_03_connect_says_it_is_not_built_and_what_happens_instead() {
        let engine = Arc::new(Engine::new(EngineConfig::new(48_000, MAX_QUANTUM, 2)));
        let (events, _rx) = std::sync::mpsc::sync_channel(64);
        let Err(error) = Backend::connect(BackendConfig::default(), engine, events) else {
            panic!("connect cannot succeed before handoff 03");
        };
        let message = error.to_string();
        assert!(message.contains("03-pipewire-backend"), "{message}");
        assert!(message.contains("null backend"), "{message}");
    }
}
