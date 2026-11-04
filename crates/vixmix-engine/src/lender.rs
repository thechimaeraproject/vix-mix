//! [`Lender`]: how a prepared graph gets a hosted plugin's audio half without building it.
//!
//! A VST3 instance is made once, on the UI thread, by the plugins crate; its processor lives
//! in that crate's `Rack` and is lent to whichever chain names its instance. The engine asks
//! through this trait so it depends on no plugin type, and `Rack` implements it.

use chimaeraproject_audio_processor::AudioProcessor;
use vixmix_model::InstanceId;

/// Lends hosted processors to the chains [`Engine::prepare`](crate::Engine::prepare) builds.
pub trait Lender {
    /// The processor for `instance`, which the setup says is `plugin_id`. `Ok(None)` when no
    /// instance is held for it (a missing plugin: the insert becomes a reported pass-through);
    /// `Err` when one is held but cannot be lent now.
    fn lend(
        &self,
        instance: InstanceId,
        plugin_id: &str,
    ) -> Result<Option<Box<dyn AudioProcessor>>, LendError>;

    /// Moves whenever what the lender holds changes (an instance added, retired, or asking
    /// for new buses), so the engine prepares again even when the snapshot did not change.
    fn generation(&self) -> u64;
}

/// Why a held instance could not be lent.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LendError {
    #[error(
        "plugin instance {instance:?} ({plugin_id}) is still in another chain: clear every stale chain before filling any, so an instance moving between strips is home when its new chain asks"
    )]
    Busy {
        instance: InstanceId,
        plugin_id: String,
    },
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
