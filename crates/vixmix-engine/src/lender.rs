//! [`Lender`]: how a prepared graph gets a hosted plugin's audio half without building it.
//!
//! A VST3 instance is made once, on the UI thread, by the plugins crate; its processor lives
//! in that crate's `Rack`, which lends it once to the engine. The engine shares it between
//! every graph that names it and sends it home when the snapshot stops naming it and the last
//! such graph is dropped. The engine asks through this trait so it depends on no plugin type,
//! and `Rack` implements it.

use chimaeraproject_audio_processor::AudioProcessor;
use vixmix_model::InstanceId;

/// Lends hosted processors to the chains [`Engine::prepare`](crate::Engine::prepare) builds.
pub trait Lender {
    /// The processor for `instance`, which the setup says is `plugin_id`. `Ok(None)` when no
    /// instance is held for it (a missing plugin: the insert becomes a reported pass-through);
    /// `Err` when one is held but cannot be lent now ([`LendError`] says which).
    fn lend(
        &self,
        instance: InstanceId,
        plugin_id: &str,
    ) -> Result<Option<Box<dyn AudioProcessor>>, LendError>;

    /// Moves whenever what the lender holds changes (an instance added or retired). On a
    /// move the engine's next prepare asks again for every instance it holds (`Busy` means
    /// still the engine's, kept as it is; `Ok(Some(_))` a new instance under that id, set up
    /// in its place) and retries every insert that failed. A move sets no held instance up
    /// again: new buses need [`Engine::set_up_again`](crate::Engine::set_up_again), a new
    /// rate [`Engine::set_rate`](crate::Engine::set_rate).
    fn generation(&self) -> u64;
}

/// Why a held instance could not be lent.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LendError {
    /// Lent and not home yet. For an instance the engine holds, the borrower is the engine
    /// itself and the instance is kept; for any other, the insert is a reported pass-through
    /// until the instance comes home and the lender's generation moves.
    #[error(
        "plugin instance {instance:?} ({plugin_id}) is lent to another borrower, so its slot passes audio through until it comes home: the engine lends each instance once and shares it between its graphs, so drop the other borrower's processor and move the lender's generation, and the next prepare lends it here"
    )]
    Busy {
        instance: InstanceId,
        plugin_id: String,
    },
    /// The instance now holds another plugin than the setup names.
    #[error(
        "plugin instance {instance:?} holds {held}, not {plugin_id}: the setup now names another plugin for it; retire the old instance and create the new one before preparing"
    )]
    OtherPlugin {
        instance: InstanceId,
        plugin_id: String,
        held: String,
    },
}

/// A lender with nothing to lend: every insert is a missing plugin. For tests, and for a
/// host that has not created any instance yet.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoPlugins;

impl Lender for NoPlugins {
    fn lend(
        &self,
        _instance: InstanceId,
        _plugin_id: &str,
    ) -> Result<Option<Box<dyn AudioProcessor>>, LendError> {
        Ok(None)
    }

    fn generation(&self) -> u64 {
        0
    }
}
