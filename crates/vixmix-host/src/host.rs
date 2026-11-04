//! The [`Host`] trait the window drives, and what it hands back.

use chimaeraproject_vst3_host::{EditorEvent, EditorSize};
use raw_window_handle::RawWindowHandle;
use vixmix_model::{
    AppRef, Applied, Command, DeviceInfo, InstanceId, MAX_CHANNELS, Refused, Setup,
};
use vixmix_plugins::{Catalog, PluginError};

/// What the window can ask of the mixer. UI thread only.
pub trait Host {
    /// The setup as it stands; the window draws from it every frame.
    fn setup(&self) -> &Setup;
    /// Applies a gesture's command. A refusal changes nothing and its message says what to do.
    fn apply(&mut self, cmd: Command) -> Result<Applied, Refused>;
    /// Everything that happened since the last call; call once a frame.
    fn poll(&mut self) -> Vec<HostEvent>;
    /// Every meter's peak since the last call, copied for one frame.
    fn meters(&self) -> MeterFrame;
    /// The header display's numbers.
    fn stats(&self) -> StatsFrame;
    /// The plugin browser's catalog.
    fn catalog(&self) -> &Catalog;
    /// Every app the source panel lists: the setup's, and any that played since start.
    fn apps(&self) -> &[AppPresence];
    /// The devices present now.
    fn devices(&self) -> &[DeviceInfo];
    /// Opens `instance`'s editor in the window `parent` and returns the size it wants.
    fn open_editor(
        &mut self,
        instance: InstanceId,
        parent: RawWindowHandle,
    ) -> Result<EditorSize, HostError>;
    /// Polls every open editor (and, on Linux, its run loop); what each asked of its window.
    fn tick_editors(&mut self) -> Vec<(InstanceId, EditorEvent)>;
    /// Closes `instance`'s editor; do it before its window goes.
    fn close_editor(&mut self, instance: InstanceId);
    /// Captures every plugin's state and writes the setup now.
    fn save_now(&mut self);
}

/// What changed, for the window to react to.
#[derive(Clone, Debug, PartialEq)]
pub enum HostEvent {
    /// The setup changed (a command, or a plugin that marked itself dirty).
    SetupChanged,
    /// A message for the toast line; it says what to do when something failed.
    Toast(String),
    /// The app list changed.
    Apps(Vec<AppPresence>),
    /// The device list changed.
    Devices(Vec<DeviceInfo>),
    /// The backend's state changed; the first poll always reports it.
    Backend(BackendState),
    /// A rescan finished; the catalog is new.
    PluginsRescanned,
    /// An editor closed itself (or its plugin went away).
    EditorClosed(InstanceId),
}

/// Where the audio is.
#[derive(Clone, Debug, PartialEq)]
pub enum BackendState {
    /// No real backend: the engine runs on the null backend, silent. `reason` is what the
    /// window's banner says.
    Absent { reason: String },
    /// `FakeHost`: simulated meters, no audio.
    Simulated,
    /// Connecting to the daemon.
    Connecting,
    /// Running at this rate and quantum.
    Running { rate: u32, quantum: u32 },
    /// Lost the daemon; reconnecting.
    Disconnected,
}

impl BackendState {
    /// What the banner says on a platform with no audio backend.
    pub const NO_BACKEND: &'static str = "No audio backend on this platform";

    /// Whether the window should show the no-backend banner.
    pub fn banner(&self) -> Option<&str> {
        match self {
            BackendState::Absent { reason } => Some(reason),
            _ => None,
        }
    }

    /// Whether meters move: the window repaints at most 60 times a second while they do.
    pub fn live(&self) -> bool {
        matches!(self, BackendState::Simulated | BackendState::Running { .. })
    }
}

/// An app the source panel lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPresence {
    pub app: AppRef,
    /// Playing now ("Playing now") or only seen before ("Seen before").
    pub running: bool,
}

/// A stereo peak, linear amplitude.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stereo {
    pub left: f32,
    pub right: f32,
}

/// One channel's two sends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChannelLevels {
    pub monitor: Stereo,
    pub master: Stereo,
}

/// Every meter for one frame, by channel position in the setup.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeterFrame {
    pub channels: [ChannelLevels; MAX_CHANNELS],
    /// The Monitor output (the Master mix while previewing).
    pub monitor: Stereo,
    pub master: Stereo,
}

/// The header display's numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StatsFrame {
    pub rate: u32,
    pub quantum: u32,
    /// The last cycle's processing time as a fraction of its budget.
    pub dsp_load: f32,
    pub xruns: u64,
}

impl StatsFrame {
    /// One quantum in milliseconds, what the header shows beside it.
    pub fn quantum_ms(&self) -> f32 {
        if self.rate == 0 {
            0.0
        } else {
            self.quantum as f32 * 1000.0 / self.rate as f32
        }
    }
}

/// Why the host could not do what the window asked. Each message ends with what to do.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error(transparent)]
    Plugin(#[from] PluginError),
    #[error("plugin instance {0:?} has no editor of its own: use its slot's controls instead")]
    NoEditor(InstanceId),
    #[error(
        "FakeHost has example data and no plugin instances: run with the real host to open a plugin's editor"
    )]
    Simulated,
    #[error(
        "this platform names no per-user configuration directory for vix-mix: set CHIMAERA_CONFIG_DIR to a directory it may write"
    )]
    NoConfigDir,
}
