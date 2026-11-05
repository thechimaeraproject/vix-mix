//! The orchestrator: one [`Host`] owns the setup, the engine, the plugin instances and the
//! backend, and is the only thing the window talks to. [`RealHost`] wires them; [`FakeHost`]
//! serves prototype v5's example data with simulated meters, for the app's tests and the
//! look gate, with no audio at all.
//!
//! Every method of [`Host`] runs on the UI thread. The engine's tick runs on the backend's
//! data thread (or, on the null backend, inside [`Host::poll`]); the host reaches it only
//! through `Engine::prepare`, `Engine::reap`, `Engine::set_rate`, `Engine::set_up_again` and
//! the meters, stats and problems it reads.
//!
//! See `docs/reference/host.md`.

mod apps;
mod audio;
mod autosave;
mod fake;
mod host;
mod real;
mod undo;

pub use audio::AudioBackend;
pub use fake::FakeHost;
pub use host::{
    AppPresence, BackendState, ChannelLevels, Host, HostError, HostEvent, MeterFrame, StatsFrame,
    Stereo, UNDO_TOAST,
};
pub use real::{BootOptions, RealHost};
/// An insert playing as a pass-through, and why: what [`Host::problems`] lists.
pub use vixmix_engine::InsertProblem;
