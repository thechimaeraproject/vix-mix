//! The ids the setup hands out. Each is a small `Copy` value that the engine, the backend and
//! the UI can hold without borrowing the setup.

use serde::{Deserialize, Serialize};

/// One channel, stable for its lifetime: a rename, a source change or a reorder keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChannelId(pub u32);

/// One insert's plugin instance. A moved insert keeps it (the same live instance moves); a
/// copied one gets a new one (a second instance, loaded with the first one's state).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstanceId(pub u32);

/// The two mixes. There are exactly two in v1 (`docs/charter.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MixId {
    /// What you hear: always plays to a device.
    Monitor,
    /// The main output: may have no device, and is also published as a virtual source other
    /// apps record.
    Master,
}

impl MixId {
    /// The name the UI and every message use.
    pub fn name(self) -> &'static str {
        match self {
            MixId::Monitor => "Monitor",
            MixId::Master => "Master",
        }
    }
}
