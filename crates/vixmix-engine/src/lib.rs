//! The realtime graph. The host prepares a graph from an
//! [`EngineSnapshot`](vixmix_model::EngineSnapshot) off the audio thread and swaps it in;
//! the backend's data thread calls [`Engine::process`] once per PipeWire cycle with every
//! channel's input and the two mixes' outputs, and nothing in it allocates, locks or logs.
//!
//! **The handoff.** [`Engine::prepare`] builds a `Prepared` graph (allocating as it pleases),
//! swaps it into an `ArcSwap` and keeps its own clone of the graph it replaced. The tick
//! loads the current graph, stores that graph's generation as its acknowledgement, and runs
//! it. [`Engine::reap`], on the host thread, drops every previous graph the acknowledgement
//! has passed, so the last reference to a graph is always dropped on the host thread and
//! never on the audio thread. No graveyard ring.
//!
//! **What the tick does today.** Each channel's input is summed into the Monitor and Master
//! outputs at its send gains (a muted send contributes nothing), each mix's level and mute
//! are applied, and with the ear button on Master the Monitor output plays the Master mix.
//! Meters record post-send and post-mix peaks; [`Stats`] count frames and DSP load. Chains
//! (each strip's inserts, lent through [`Lender`]) are handoff 02's.
//!
//! The kit's `chimaeraproject_audio_processor` supplies the processor contract, the chain and
//! the strip maths; the thread model and the graph are this crate's (the kit's charter: no
//! shared crate owns either). See `docs/reference/engine.md`.

mod engine;
mod lender;
mod meters;
mod null;

pub use chimaeraproject_audio_processor::EngineConfig;
pub use engine::{ChannelInput, Engine, MAX_QUANTUM, Tick};
pub use lender::{LendError, Lender, NoPlugins};
pub use meters::{ChannelMeters, MeterBank, PeakMeter, Stats, StereoMeter};
pub use null::NullBackend;
