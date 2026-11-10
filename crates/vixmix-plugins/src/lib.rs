//! Everything between the kit's VST3 host (`chimaeraproject_vst3_host`) and the mixer: the
//! [`Catalog`] the plugin browser searches and the scan that fills it ([`search_dirs`],
//! [`ScanCache`], and [`Describe`] with a [`ScanCommand`] to load each bundle in a child
//! process that [`answer_scan`] serves), the [`Instances`] that live on the UI thread, the
//! [`Rack`] that lends their audio halves to the engine's chains (it implements
//! [`vixmix_engine::Lender`]), the edit path from a plugin's own editor to its lent processor
//! ([`EditSender`]), state capture for `setup.ron`, and [`EditorSession`]s.
//!
//! The thread rule is the kit's: scanning, loading, creating, every controller call and every
//! editor call happen on the UI thread; only the lent processor reaches the audio thread, and
//! it comes home (inactive) through its handle's `Drop` on the thread that prepares chains.
//! A retired instance is dropped on the UI thread, processor before controller.
//!
//! See `docs/reference/plugins.md`.

mod catalog;
mod child;
mod editor;
mod edits;
mod instances;
mod rack;
mod scan;

pub use catalog::{Catalog, FILE_NAME, Filter, PluginEntry, PluginKind, RECENT_LIMIT};
pub use child::{ANSWER_MARK, SCAN_TIMEOUT, ScanCommand, answer_scan};
pub use editor::EditorSession;
pub use edits::{EDIT_RING, EditSender};
pub use instances::{InstanceEvent, Instances, PluginError};
pub use rack::Rack;
pub use scan::{Describe, ScanCache, ScanOutcome, search_dirs};
