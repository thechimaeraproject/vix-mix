//! The setup's types: what `setup.ron` holds and what every command edits.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{ChannelId, InstanceId, MixId};
use crate::persist::VERSION;
use crate::taper::UNITY;

/// The most channels a setup holds: the prototype's "Add channel (n of 8)". The mic is an
/// ordinary hardware channel among them.
pub const MAX_CHANNELS: usize = 8;

/// Everything the user set up, saved as `setup.ron` and restored at the next start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    /// The file format this setup was written in; [`crate::persist`] migrates older ones on
    /// load, so in memory it is always [`VERSION`]. First, so it is the file's first line.
    pub version: u32,
    /// The id the next new channel takes. It is above every channel id this setup ever
    /// handed out and only grows, so a removed channel's id never comes back: the host keeps
    /// a removed channel while its undo toast shows, and a new one must not take its id.
    pub next_channel: ChannelId,
    /// The id the next new insert's instance takes, by the same rule: the host keeps a
    /// removed insert's live instance while its undo toast shows.
    pub next_instance: InstanceId,
    /// The input channels, left to right as the console shows them. At most [`MAX_CHANNELS`].
    pub channels: Vec<Channel>,
    /// The Monitor mix: what you hear.
    pub monitor: Mix,
    /// The Master mix: the main output and the virtual source other apps record.
    pub master: Mix,
    /// The ear button on Master: Monitor's output plays the Master mix instead of its own.
    pub preview_master: bool,
    /// Preferences that are not part of the console.
    pub prefs: Prefs,
}

/// One input channel: a source, a chain, and a send to each mix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    /// What the strip's header shows, and the PipeWire node's description.
    pub name: String,
    pub source: Source,
    /// The inserts, top to bottom: processed in this order.
    pub chain: Vec<Insert>,
    /// MON on the FX header: the Monitor send takes the processed signal (else the dry one).
    pub chain_to_monitor: bool,
    /// MST on the FX header: the Master send takes the processed signal (else the dry one).
    pub chain_to_master: bool,
    /// The send to the Monitor mix.
    pub monitor: Send,
    /// The send to the Master mix.
    pub master: Send,
    /// The link toggle: moving one send's fader moves the other by the same amount.
    pub link: bool,
}

impl Channel {
    /// The send to `mix`.
    pub fn send(&self, mix: MixId) -> &Send {
        match mix {
            MixId::Monitor => &self.monitor,
            MixId::Master => &self.master,
        }
    }

    /// The send to `mix`, to change.
    pub fn send_mut(&mut self, mix: MixId) -> &mut Send {
        match mix {
            MixId::Monitor => &mut self.monitor,
            MixId::Master => &mut self.master,
        }
    }

    /// Whether the chain's output (rather than the dry input) feeds `mix`'s send.
    pub fn chain_to(&self, mix: MixId) -> bool {
        match mix {
            MixId::Monitor => self.chain_to_monitor,
            MixId::Master => self.chain_to_master,
        }
    }

    /// Which kind of channel this is.
    pub fn kind(&self) -> ChannelKind {
        match self.source {
            Source::Hardware(_) => ChannelKind::Hardware,
            Source::Apps(_) => ChannelKind::Apps,
        }
    }
}

/// Where a channel's audio comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Source {
    /// A capture device (a mic, an interface input); `None` until one is picked.
    Hardware(Option<DeviceRef>),
    /// The apps that play into this channel's virtual sink. An app is in one channel only.
    Apps(Vec<AppRef>),
}

/// The two kinds of channel the add-channel menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelKind {
    /// "Hardware input".
    Hardware,
    /// "App channel".
    Apps,
}

/// A fader and its mute: one send of a channel, or a mix's level.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Send {
    /// The fader's position, 0..=1: the [taper](crate::taper)'s domain, not decibels.
    /// [`UNITY`] is 0 dB.
    pub position: f32,
    /// Silent whatever the position.
    pub mute: bool,
}

impl Send {
    /// At 0 dB, not muted: what a new channel's sends start at.
    pub const UNITY: Send = Send {
        position: UNITY,
        mute: false,
    };
}

/// One of the two mixes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mix {
    pub id: MixId,
    /// The output device. Monitor always has one once the user or the host picked it; Master
    /// may have none ("No device") and is still published as a virtual source.
    pub device: Option<DeviceRef>,
    /// The mix's own fader and mute, after its chain.
    pub level: Send,
    /// The mix's inserts, top to bottom.
    pub chain: Vec<Insert>,
}

impl Mix {
    /// An empty mix at unity with no device.
    pub fn new(id: MixId) -> Mix {
        Mix {
            id,
            device: None,
            level: Send::UNITY,
            chain: Vec::new(),
        }
    }
}

/// One slot of a chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Insert {
    /// The live instance the plugins crate keeps for this slot.
    pub instance: InstanceId,
    pub plugin: PluginRef,
    /// The bypass square: a disabled insert passes its input through.
    pub enabled: bool,
    /// The plugin's state as `PluginState::encode` bytes, captured before every save; `None`
    /// until the first capture (a fresh insert starts from the plugin's defaults). Written to
    /// `setup.ron` as one base64 string.
    #[serde(with = "crate::base64::option")]
    pub state: Option<Vec<u8>>,
}

/// How a setup names a plugin: enough to find it again after a rescan, never a path.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginRef {
    /// `vst3:` and the class id's 32 hexadecimal digits.
    pub id: String,
    /// The bundle's file name (`Pro-Q 3.vst3`), resolved through the scan cache.
    pub bundle: String,
    /// What the slot shows when the plugin is missing.
    pub name: String,
}

/// How a setup names an app. PipeWire reports these three inconsistently across Flatpak,
/// Electron and browsers, so all three are stored and matched in that order
/// ([`AppRef::matches`]): storing all three survives any one of them changing.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppRef {
    /// `pipewire.access.portal.app_id` (a Flatpak's `org.mozilla.firefox`).
    pub app_id: Option<String>,
    /// `application.process.binary` (`firefox`).
    pub binary: Option<String>,
    /// `application.name` (`Firefox`): always present, what the checklist shows.
    pub name: String,
}

impl AppRef {
    /// Whether `self` and `other` are the same app: by portal app id when both have one, else
    /// by binary when both have one, else by name.
    pub fn matches(&self, other: &AppRef) -> bool {
        if let (Some(a), Some(b)) = (&self.app_id, &other.app_id) {
            return a == b;
        }
        if let (Some(a), Some(b)) = (&self.binary, &other.binary) {
            return a == b;
        }
        self.name == other.name
    }
}

/// How a setup names a device: its PipeWire `node.name`, and what to show while it is absent.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceRef {
    /// `node.name`: stable across restarts and replugging.
    pub name: String,
    /// `node.description`: what the user reads.
    pub description: String,
}

/// Which way a device's audio goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceDirection {
    /// An `Audio/Source`: a channel can capture it.
    Input,
    /// An `Audio/Sink`: a mix can play to it.
    Output,
}

/// A device that is present now, as the backend reports it and the host lists it. It lives
/// here, beside [`DeviceRef`], because both the Linux-only backend and the host name it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device: DeviceRef,
    pub direction: DeviceDirection,
}

/// Preferences that are not part of the console.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// The quantum asked of PipeWire through `node.latency` at 48 kHz: 128, 256 or 512. A
    /// request, never forced; the graph decides.
    pub quantum: u32,
    /// Directories scanned for VST3 bundles besides the SDK's standard places. Written as
    /// text; a path that is not UTF-8 is written with its unreadable part replaced.
    #[serde(serialize_with = "lossy_paths")]
    pub plugin_dirs: Vec<PathBuf>,
}

/// Paths as text, lossily: serde's own `Path` serializer refuses one that is not UTF-8, and a
/// refusal would leave `persist::to_ron` nothing to write.
fn lossy_paths<S: serde::Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(paths.iter().map(|path| path.to_string_lossy()))
}

impl Prefs {
    /// The quanta the preferences offer.
    pub const QUANTA: [u32; 3] = [128, 256, 512];
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            quantum: 256,
            plugin_dirs: Vec::new(),
        }
    }
}

impl Setup {
    /// The channel with `id`.
    pub fn channel(&self, id: ChannelId) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// The channel with `id`, to change.
    pub fn channel_mut(&mut self, id: ChannelId) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    /// The mix `id`.
    pub fn mix(&self, id: MixId) -> &Mix {
        match id {
            MixId::Monitor => &self.monitor,
            MixId::Master => &self.master,
        }
    }

    /// The mix `id`, to change.
    pub fn mix_mut(&mut self, id: MixId) -> &mut Mix {
        match id {
            MixId::Monitor => &mut self.monitor,
            MixId::Master => &mut self.master,
        }
    }

    /// Every insert on every strip: the channels left to right, then Monitor, then Master.
    pub fn inserts(&self) -> impl Iterator<Item = &Insert> {
        self.channels
            .iter()
            .flat_map(|c| c.chain.iter())
            .chain(self.monitor.chain.iter())
            .chain(self.master.chain.iter())
    }

    /// The channel `app` plays into, if any channel has it.
    pub fn channel_of_app(&self, app: &AppRef) -> Option<ChannelId> {
        self.channels.iter().find_map(|c| match &c.source {
            Source::Apps(apps) if apps.iter().any(|a| a.matches(app)) => Some(c.id),
            _ => None,
        })
    }

    /// Raises [`next_channel`](Setup::next_channel) and
    /// [`next_instance`](Setup::next_instance) above every id in use, where a hand edit or
    /// an older format left them at or below one. Never lowers them.
    pub(crate) fn raise_counters(&mut self) {
        let channels = self.channels.iter().map(|c| c.id.0).max();
        let instances = self.inserts().map(|i| i.instance.0).max();
        if let Some(top) = channels {
            self.next_channel.0 = self.next_channel.0.max(top.saturating_add(1));
        }
        if let Some(top) = instances {
            self.next_instance.0 = self.next_instance.0.max(top.saturating_add(1));
        }
    }
}

impl Default for Setup {
    /// What a first start shows: one hardware channel named Mic with no device picked yet,
    /// both sends and both mixes at unity, no plugins.
    fn default() -> Setup {
        Setup {
            version: VERSION,
            next_channel: ChannelId(2),
            next_instance: InstanceId(1),
            channels: vec![Channel {
                id: ChannelId(1),
                name: "Mic".to_string(),
                source: Source::Hardware(None),
                chain: Vec::new(),
                chain_to_monitor: true,
                chain_to_master: true,
                monitor: Send::UNITY,
                master: Send::UNITY,
                link: true,
            }],
            monitor: Mix::new(MixId::Monitor),
            master: Mix::new(MixId::Master),
            preview_master: false,
            prefs: Prefs::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(app_id: Option<&str>, binary: Option<&str>, name: &str) -> AppRef {
        AppRef {
            app_id: app_id.map(str::to_string),
            binary: binary.map(str::to_string),
            name: name.to_string(),
        }
    }

    #[test]
    fn an_app_matches_by_portal_id_then_binary_then_name() {
        let flatpak = app(Some("org.mozilla.firefox"), Some("firefox"), "Firefox");
        // The portal id decides when both carry one, whatever the rest says.
        assert!(flatpak.matches(&app(Some("org.mozilla.firefox"), None, "Firefox Nightly")));
        assert!(!flatpak.matches(&app(
            Some("org.mozilla.Thunderbird"),
            Some("firefox"),
            "Firefox"
        )));
        // Without a portal id on one side, the binary decides.
        assert!(flatpak.matches(&app(None, Some("firefox"), "Mozilla Firefox")));
        assert!(!flatpak.matches(&app(None, Some("firefox-bin"), "Firefox")));
        // With neither, the name.
        assert!(app(None, None, "Firefox").matches(&app(Some("x"), None, "Firefox")));
    }

    #[test]
    fn the_first_start_is_one_mic_channel_at_unity() {
        let setup = Setup::default();
        assert_eq!(setup.channels.len(), 1);
        assert_eq!(setup.channels[0].name, "Mic");
        assert_eq!(setup.channels[0].source, Source::Hardware(None));
        assert_eq!(setup.channels[0].monitor, Send::UNITY);
        assert_eq!(setup.monitor.level.position, UNITY);
        assert_eq!(setup.version, VERSION);
    }
}
