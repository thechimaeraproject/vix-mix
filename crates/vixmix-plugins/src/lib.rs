//! Everything between the kit's VST3 host (`chimaeraproject_vst3_host`) and the mixer: the
//! [`Catalog`] the plugin browser searches, the [`Instances`] that live on the UI thread, the
//! [`Rack`] that lends their audio halves to the engine's chains (it implements
//! [`vixmix_engine::Lender`]), state capture for `setup.ron`, and [`EditorSession`]s.
//!
//! The thread rule is the kit's: scanning, loading, creating, every controller call and every
//! editor call happen on the UI thread; only the lent processor reaches the audio thread, and
//! it comes home (inactive) through its handle's `Drop` on the thread that prepares chains.
//! A retired instance is dropped on the UI thread, processor before controller.
//!
//! See `docs/reference/plugins.md`.

mod catalog;
mod editor;
mod instances;
mod rack;

pub use catalog::{Catalog, Filter, PluginEntry, PluginKind, RECENT_LIMIT, ScanOutcome};
pub use editor::EditorSession;
pub use instances::{InstanceEvent, Instances, PluginError};
pub use rack::Rack;
