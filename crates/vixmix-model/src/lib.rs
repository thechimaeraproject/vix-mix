//! The mixer's whole setup as pure data: up to eight channels, each with a source (a hardware
//! input or a group of apps), a chain of VST3 inserts and two sends; the Monitor and Master
//! mixes with their own chains and devices; the commands that change it, validated, saying
//! what they changed; the fader taper; RON persistence over `chimaeraproject_fs`; and the
//! flat [`EngineSnapshot`] the engine prepares from.
//!
//! **Data only.** No thread, device handle, plugin instance or window type lives here; the
//! engine, the backend, the plugins crate and the app all read this crate and none of them
//! is read by it. A channel's apps and devices are *references* (identities that survive a
//! restart), never live stream or node ids.
//!
//! **A name to import qualified.** [`Send`] is a send's fader and mute, as the plan names it;
//! it shadows the standard `Send` marker trait wherever it is imported by name, so write
//! `vixmix_model::Send` (or import it under another name) in any module with thread bounds.
//!
//! See `docs/reference/model.md`.

mod command;
pub mod example;
mod ids;
pub mod persist;
mod setup;
mod snapshot;
pub mod taper;

pub use command::{Applied, Command, Refused, SlotRef, Strip, apply};
pub use ids::{ChannelId, InstanceId, MixId};
pub use setup::{
    AppRef, Channel, ChannelKind, DeviceDirection, DeviceInfo, DeviceRef, Insert, MAX_CHANNELS,
    Mix, PluginRef, Prefs, Send, Setup, Source,
};
pub use snapshot::{ChannelSnap, EngineSnapshot, InsertSnap, MixSnap, SendSnap, snapshot};
