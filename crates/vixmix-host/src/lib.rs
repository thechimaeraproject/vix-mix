//! The orchestrator: one [`Host`] owns the setup, the engine, the plugin instances and the
//! backend, and is the only thing the window talks to. [`RealHost`] wires them; [`FakeHost`]
//! serves prototype v5's example data with simulated meters, for the app's tests and the
//! look gate, with no audio at all.
//!
//! Every method of [`Host`] runs on the UI thread. The engine's tick runs on the backend's
//! data thread; the host reaches it only through `Engine::prepare` and `Engine::reap` and the
//! meters and stats it reads.
//!
//! See `docs/reference/host.md`.

mod fake;
mod host;
mod real;

pub use fake::FakeHost;
pub use host::{
    AppPresence, BackendState, ChannelLevels, Host, HostError, HostEvent, MeterFrame, StatsFrame,
    Stereo,
};
pub use real::{BootOptions, RealHost};
