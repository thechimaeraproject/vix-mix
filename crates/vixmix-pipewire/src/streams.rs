//! The backend's streams: their properties, the format pod, their flags, and the two listeners
//! each carries — one whose `process` runs on the data thread with that stream's realtime
//! state, one whose state changes arrive on the control thread. Keeping them apart means the
//! data thread's user data is never touched from the control thread.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use pipewire::core::CoreRc;
use pipewire::properties::{PropertiesBox, properties};
use pipewire::spa;
use pipewire::spa::pod::Pod;
use pipewire::stream::{StreamFlags, StreamListener, StreamRc, StreamState};

use crate::rt::{self, InputRt, OutputRt, Role, Shared};
use crate::{BackendConfig, DISPLAY_NAME};

/// What one stream is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Spec {
    /// An app channel's virtual sink (`Audio/Sink`), which apps are routed to.
    AppSink { node_name: String, name: String },
    /// A hardware channel's capture on `target`.
    Capture {
        node_name: String,
        name: String,
        target: String,
    },
    /// The Monitor or Master output, playing to `target`.
    Output { role: Role, target: String },
    /// The Master source (`Audio/Source`), which other apps record.
    MasterSource,
}

impl Spec {
    pub(crate) fn node_name(&self, config: &BackendConfig) -> String {
        match self {
            Spec::AppSink { node_name, .. } | Spec::Capture { node_name, .. } => node_name.clone(),
            Spec::Output {
                role: Role::Monitor,
                ..
            } => crate::monitor_out_node_name(config.app_name),
            Spec::Output { .. } => crate::master_out_node_name(config.app_name),
            Spec::MasterSource => crate::master_node_name(config.app_name),
        }
    }

    /// The device this stream plays to or records from, when it names one.
    pub(crate) fn target(&self) -> Option<&str> {
        match self {
            Spec::Capture { target, .. } | Spec::Output { target, .. } => Some(target),
            Spec::AppSink { .. } | Spec::MasterSource => None,
        }
    }

    fn description(&self) -> String {
        match self {
            Spec::AppSink { name, .. } | Spec::Capture { name, .. } => {
                format!("{DISPLAY_NAME} {name}")
            }
            Spec::Output {
                role: Role::Monitor,
                ..
            } => format!("{DISPLAY_NAME} Monitor"),
            Spec::Output { .. } => format!("{DISPLAY_NAME} Master output"),
            Spec::MasterSource => format!("{DISPLAY_NAME} Master"),
        }
    }

    fn is_input(&self) -> bool {
        matches!(self, Spec::AppSink { .. } | Spec::Capture { .. })
    }

    /// The properties `docs/reference/pipewire.md` § Nodes lists for this stream.
    fn properties(&self, config: &BackendConfig) -> PropertiesBox {
        let app = config.app_name;
        let mut props = properties! {
            "media.type" => "Audio",
            "node.name" => self.node_name(config),
            "node.description" => self.description(),
            "media.name" => self.description(),
            "node.group" => app,
            "node.link-group" => app,
            "node.virtual" => "true",
            "node.latency" => latency(config),
            "session.suspend-timeout-seconds" => "0",
        };
        match self {
            Spec::AppSink { .. } => props.insert("media.class", "Audio/Sink"),
            Spec::MasterSource => props.insert("media.class", "Audio/Source"),
            Spec::Capture { target, .. } | Spec::Output { target, .. } => {
                props.insert("target.object", target.as_str());
                // When the device goes away the stream waits for it (we reconnect it when
                // the registry shows it again) instead of following the default device: the
                // WirePlumber Ubuntu 24.04 ships reads the first key, newer ones the second.
                props.insert("node.dont-reconnect", "true");
                props.insert("node.dont-fallback", "true");
                props.insert(
                    "media.category",
                    if self.is_input() {
                        "Capture"
                    } else {
                        "Playback"
                    },
                );
            }
        }
        props
    }

    /// The connect flags: every stream processes on the data thread with mapped buffers;
    /// those that name a device ask the session manager to link them.
    pub(crate) fn flags(&self) -> StreamFlags {
        let mut flags = StreamFlags::RT_PROCESS | StreamFlags::MAP_BUFFERS;
        if matches!(self, Spec::Capture { .. } | Spec::Output { .. }) {
            flags |= StreamFlags::AUTOCONNECT;
        }
        flags
    }

    fn direction(&self) -> spa::utils::Direction {
        if self.is_input() {
            spa::utils::Direction::Input
        } else {
            spa::utils::Direction::Output
        }
    }

    fn role(&self) -> Option<Role> {
        match self {
            Spec::Output { role, .. } => Some(*role),
            Spec::MasterSource => Some(Role::MasterSource),
            Spec::AppSink { .. } | Spec::Capture { .. } => None,
        }
    }
}

/// `node.latency` as every stream asks for it: `<quantum>/48000`.
fn latency(config: &BackendConfig) -> String {
    format!("{}/48000", config.wanted_quantum)
}

/// The one format every stream offers: 32-bit float planar stereo (FL, FR) at the graph's
/// rate (no rate in the pod, so the stream follows the graph). pipewire-rs's
/// `examples/audio-capture.rs` builds its pod the same way.
pub(crate) fn format_pod() -> Vec<u8> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32P);
    info.set_channels(2);
    let mut position = [0u32; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .unwrap_or_default()
}

/// A stream's state, as the control thread hears it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Unconnected,
    Connecting,
    Paused,
    Streaming,
    Error(String),
}

impl From<StreamState> for Phase {
    fn from(state: StreamState) -> Phase {
        match state {
            StreamState::Unconnected => Phase::Unconnected,
            StreamState::Connecting => Phase::Connecting,
            StreamState::Paused => Phase::Paused,
            StreamState::Streaming => Phase::Streaming,
            StreamState::Error(message) => Phase::Error(message),
        }
    }
}

/// What a stream's control listener reports: its serial (`Live::serial`), which of its
/// connections the change belongs to (`Live::connection`), and its new phase.
pub(crate) type Notify = Rc<dyn Fn(u64, u64, Phase)>;

/// What every stream is made with.
pub(crate) struct Factory<'a> {
    pub core: &'a CoreRc,
    pub config: &'a BackendConfig,
    pub shared: &'a Arc<Shared>,
    pub notify: &'a Notify,
}

/// The data thread's listener, kept for its `Drop` (which unregisters it).
#[expect(
    dead_code,
    reason = "held only so that it unregisters when the stream goes"
)]
enum RtListener {
    Input(StreamListener<InputRt>),
    Output(StreamListener<OutputRt>),
}

/// One connected (or connecting) stream. Dropping it disconnects it (which removes its node
/// from the data loop before returning), then drops its listeners, then the stream: the order
/// the fields are declared in.
pub(crate) struct Live {
    _rt: RtListener,
    _ctl: StreamListener<()>,
    pub stream: StreamRc,
    pub spec: Spec,
    /// Unique per stream this backend ever made: stale notes name an old serial.
    pub serial: u64,
    pub phase: Phase,
    /// Counts this stream's connections. A reconnect's own disconnect reports `Unconnected`
    /// after the new connection has begun: the count tells such a note from a real one.
    connection: Rc<Cell<u64>>,
    /// When a stream the session manager gave up on may be connected again.
    pub retry_at: Option<Instant>,
    pod: Vec<u8>,
}

impl Live {
    /// Makes the stream `spec` describes with its realtime side (`input` for an input stream,
    /// built by [`rt::input_ring`]) and connects it.
    pub(crate) fn create(
        make: &Factory<'_>,
        spec: Spec,
        input: Option<InputRt>,
        serial: u64,
    ) -> Result<Live, pipewire::Error> {
        let name = spec.node_name(make.config);
        let stream = StreamRc::new(make.core.clone(), &name, spec.properties(make.config))?;
        let rt = match (spec.role(), input) {
            (Some(role), _) => RtListener::Output(
                stream
                    .add_local_listener_with_user_data(OutputRt {
                        role,
                        shared: Arc::clone(make.shared),
                    })
                    .process(rt::output_process)
                    .register()?,
            ),
            (None, Some(input)) => RtListener::Input(
                stream
                    .add_local_listener_with_user_data(input)
                    .process(rt::input_process)
                    .register()?,
            ),
            (None, None) => return Err(pipewire::Error::CreationFailed),
        };
        let notify = Rc::clone(make.notify);
        let connection = Rc::new(Cell::new(0));
        let ctl = stream
            .add_local_listener_with_user_data(())
            .state_changed({
                let connection = Rc::clone(&connection);
                move |_, _, _, new| notify(serial, connection.get(), Phase::from(new))
            })
            .register()?;
        let mut live = Live {
            _rt: rt,
            _ctl: ctl,
            stream,
            spec,
            serial,
            phase: Phase::Connecting,
            connection,
            retry_at: None,
            pod: format_pod(),
        };
        live.connect()?;
        Ok(live)
    }

    /// Connecting, paused or streaming: not given up on by the daemon or the session manager.
    pub(crate) fn alive(&self) -> bool {
        matches!(
            self.phase,
            Phase::Connecting | Phase::Paused | Phase::Streaming
        )
    }

    fn connect(&mut self) -> Result<(), pipewire::Error> {
        let Some(pod) = Pod::from_bytes(&self.pod) else {
            return Err(pipewire::Error::CreationFailed);
        };
        self.stream
            .connect(self.spec.direction(), None, self.spec.flags(), &mut [pod])
    }

    /// Disconnects and connects again: the same node name and properties, so the session
    /// manager links it as before (to its device, once that is back).
    pub(crate) fn reconnect(&mut self) -> Result<(), pipewire::Error> {
        let _ = self.stream.disconnect();
        self.connection.set(self.connection.get() + 1);
        self.phase = Phase::Connecting;
        self.connect()
    }

    /// Asks for the config's quantum on the live stream: its `node.latency` is updated in
    /// place (`pw_stream_update_properties`, which pipewire-rs does not wrap), the daemon
    /// recalculates the graph's quantum, and the node, its links and its ring stay. A later
    /// [`reconnect`](Live::reconnect) keeps the updated property.
    pub(crate) fn ask_latency(&self, config: &BackendConfig) -> Result<(), pipewire::Error> {
        let props = properties! { "node.latency" => latency(config) };
        // SAFETY: the stream is ours and alive for the call, on the thread that made it; the
        // dict is borrowed for the call only, and PipeWire copies what it keeps.
        let result = unsafe {
            pipewire::sys::pw_stream_update_properties(
                self.stream.as_raw_ptr(),
                props.dict().as_raw(),
            )
        };
        spa::utils::result::SpaResult::from_c(result).into_sync_result()?;
        Ok(())
    }

    /// Which connection a note must name to be about the stream as it is now.
    pub(crate) fn connection(&self) -> u64 {
        self.connection.get()
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.stream.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::ChannelId;

    fn config() -> BackendConfig {
        BackendConfig::default()
    }

    #[test]
    fn an_app_sink_is_an_audio_sink_in_the_app_group_that_waits_to_be_linked() {
        let spec = Spec::AppSink {
            node_name: crate::channel_node_name("vix-mix", ChannelId(2), "Music"),
            name: "Music".into(),
        };
        let props = spec.properties(&config());
        assert_eq!(props.get("media.class"), Some("Audio/Sink"));
        assert_eq!(props.get("node.name"), Some("vix-mix.ch2.music"));
        assert_eq!(props.get("node.description"), Some("Vix Mix Music"));
        assert_eq!(props.get("node.group"), Some("vix-mix"));
        assert_eq!(props.get("node.link-group"), Some("vix-mix"));
        assert_eq!(props.get("node.virtual"), Some("true"));
        assert_eq!(props.get("node.latency"), Some("256/48000"));
        assert_eq!(props.get("session.suspend-timeout-seconds"), Some("0"));
        assert_eq!(props.get("target.object"), None);
        assert!(!spec.flags().contains(StreamFlags::AUTOCONNECT));
        assert!(
            spec.flags()
                .contains(StreamFlags::RT_PROCESS | StreamFlags::MAP_BUFFERS)
        );
    }

    #[test]
    fn outputs_and_captures_name_their_device_and_never_fall_back_to_the_default() {
        for spec in [
            Spec::Output {
                role: Role::Monitor,
                target: "headphones".into(),
            },
            Spec::Capture {
                node_name: "vix-mix.ch1.mic".into(),
                name: "Mic".into(),
                target: "headphones".into(),
            },
        ] {
            let props = spec.properties(&config());
            assert_eq!(props.get("target.object"), Some("headphones"));
            assert_eq!(props.get("node.dont-reconnect"), Some("true"));
            assert_eq!(props.get("node.dont-fallback"), Some("true"));
            assert_eq!(props.get("media.class"), None, "pw_stream sets Stream/*");
            assert!(spec.flags().contains(StreamFlags::AUTOCONNECT));
        }
        let master = Spec::Output {
            role: Role::MasterOut,
            target: "speakers".into(),
        };
        assert!(!master.flags().contains(StreamFlags::TRIGGER));
        assert_eq!(
            master.properties(&config()).get("node.name"),
            Some("vix-mix.master-out")
        );
    }

    #[test]
    fn the_master_source_is_an_audio_source_other_apps_record() {
        let props = Spec::MasterSource.properties(&config());
        assert_eq!(props.get("media.class"), Some("Audio/Source"));
        assert_eq!(props.get("node.name"), Some("vix-mix.master"));
        assert_eq!(props.get("node.description"), Some("Vix Mix Master"));
        assert!(
            !Spec::MasterSource
                .flags()
                .contains(StreamFlags::AUTOCONNECT)
        );
    }

    #[test]
    fn the_format_pod_is_f32_planar_stereo_with_no_rate() {
        let bytes = format_pod();
        let pod = Pod::from_bytes(&bytes).expect("a pod");
        let mut info = spa::param::audio::AudioInfoRaw::new();
        info.parse(pod).expect("an audio format");
        assert_eq!(info.format(), spa::param::audio::AudioFormat::F32P);
        assert_eq!(info.channels(), 2);
        assert_eq!(info.rate(), 0);
        assert_eq!(
            &info.position()[..2],
            &[
                spa::sys::SPA_AUDIO_CHANNEL_FL,
                spa::sys::SPA_AUDIO_CHANNEL_FR
            ]
        );
    }
}
