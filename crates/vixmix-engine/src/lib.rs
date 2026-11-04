//! The realtime graph. The host prepares a graph from an
//! [`EngineSnapshot`](vixmix_model::EngineSnapshot) off the audio thread and swaps it in;
//! the backend's data thread calls [`Engine::process`] once per PipeWire cycle with every
//! channel's input and the two mixes' outputs, and nothing in it allocates, locks or logs.
//!
//! **The graph.** Each channel's input runs through its chain of inserts (the kit's
//! `ProcessorChain`); its Monitor and Master sends each take the processed or the dry signal
//! (MON, MST) at their gains into the two mixes; each mix runs its own chain, then its level
//! or mute; with the ear button on, the Monitor output plays the Master mix. Meters record
//! post-send and post-mix peaks; [`Stats`] count frames and DSP load.
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
//! the strip maths; the thread model and the graph are this crate's (the kit's charter: no
//! shared crate owns either). See `docs/reference/engine.md`.

mod engine;
mod graph;
mod instance;
mod lender;
mod meters;
mod null;
mod registry;
mod tick_only;

pub use chimaeraproject_audio_processor::EngineConfig;
pub use engine::{ChannelInput, Engine, MAX_QUANTUM, Tick};
pub use graph::InsertProblem;
pub use lender::{LendError, Lender, NoPlugins};
pub use meters::{ChannelMeters, MeterBank, PeakMeter, Stats, StereoMeter};
pub use null::NullBackend;
