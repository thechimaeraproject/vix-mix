//! The realtime graph. The host prepares a graph from an
//! [`EngineSnapshot`](vixmix_model::EngineSnapshot) off the audio thread and swaps it in;
//! the backend's data thread calls [`Engine::process`] once per PipeWire cycle with every
//! channel's input and the two mixes' outputs, and nothing in it allocates, locks or logs.
//!
//! **The graph.** Each channel's input runs through its chain of inserts (the kit's
//! `ProcessorChain`); its Monitor and Master sends each take the processed or the dry signal
//! (MON, MST) at their gains into the two mixes; each mix runs its own chain, then its level
//! or mute; with the ear button on, the Monitor output plays the Master mix. Meters record
//! post-send and post-mix peaks into a [`MeterBank`]; [`Stats`] count frames and DSP load. Both
//! are the kit's `chimaeraproject_audio_io`, at this mixer's shape. Beside them
//! [`DspCounters`] keep the busy time and the worst cycle, for a load averaged over a window.
//!
//! **The handoff.** [`Engine::prepare`] builds a graph (allocating as it pleases), swaps it
//! into an `ArcSwap` and keeps its own clone of the graph it replaced. The tick loads the
//! current graph and stores that graph's generation as its acknowledgement. The host drops a
//! replaced graph, on its own thread, once the acknowledgement has passed it or no tick is
//! running at all, so the last reference to a graph is never dropped on the audio thread. No
//! graveyard ring.
//!
//! **Hosted plugins** arrive through [`Lender`], lent once and shared by every graph that
//! names them, so a new graph never sets a running plugin up again and never finds it busy in
//! the graph it replaces. An insert that cannot be lent or set up is a pass-through, reported
//! in [`Engine::problems`].
//!
//! The kit's `chimaeraproject_audio_processor` supplies the processor contract, the chain and
//! the strip maths, and its `chimaeraproject_audio_io` the meters and the counters; the thread
//! model and the graph are this crate's (the kit's charter: no shared crate owns either). See
//! `docs/reference/engine.md`.

mod dsp;
mod engine;
mod graph;
mod instance;
mod lender;
mod null;
mod registry;
mod tick_only;

pub use chimaeraproject_audio_io::{PeakMeter, Stats, StereoMeter};
pub use chimaeraproject_audio_processor::EngineConfig;
pub use dsp::DspCounters;
pub use engine::{ChannelInput, Engine, MAX_QUANTUM, Tick};
pub use graph::InsertProblem;
pub use lender::{LendError, Lender, NoPlugins};
pub use null::NullBackend;

/// Every meter the tick writes and the window reads: each channel's Monitor and Master sends,
/// post-send-gain, by the channel's position in the snapshot, and the two mixes post-level.
/// The kit's bank at this mixer's shape: [`MAX_CHANNELS`](vixmix_model::MAX_CHANNELS) strips
/// of two taps, and two buses. Tap and bus [`MONITOR`] are Monitor's, [`MASTER`] Master's.
pub type MeterBank = chimaeraproject_audio_io::MeterBank<{ vixmix_model::MAX_CHANNELS }, 2, 2>;

/// The index of the Monitor send among a channel's taps, and of the Monitor mix's bus.
pub const MONITOR: usize = 0;
/// The index of the Master send among a channel's taps, and of the Master mix's bus.
pub const MASTER: usize = 1;
