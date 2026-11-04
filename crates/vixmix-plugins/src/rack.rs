//! [`Rack`]: the audio halves of the hosted plugins, lent to whichever chain names their
//! instance. Vix's `crates/vix-audio/src/rack.rs` is the pattern, without its export
//! reservation: the mixer never renders offline.
//!
//! **Why lend.** A chain is cleared and filled again whenever its strip's shape changes, and a
//! VST3 instance cannot be made again on the thread that prepares chains: making one belongs to
//! the UI thread (the kit's thread table) and makes its controller too, which must stay there.
//! So the instance lives here, a chain holds it through a `Lent` handle, and dropping the
//! handle puts it back, inactive.
//!
//! **Generations.** Every change to what the rack holds (an instance added or retired, a
//! plugin that changed its buses) moves [`Lender::generation`], so the engine prepares again
//! and lends afresh even when the setup did not change.
//!
//! **Teardown stays on the UI thread.** A retired instance is not dropped when its handle
//! goes home: it waits, inactive, until [`Rack::reap`] hands it to the UI thread, which drops
//! it before the instance's controller, as the kit asks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use chimaeraproject_audio_processor::{
    AudioError, AudioProcessData, AudioProcessor, BusLayout, ProcessSetup, ProcessorDescriptor,
};
use vixmix_engine::{LendError, Lender};
use vixmix_model::InstanceId;

struct Entry {
    instance: InstanceId,
    plugin_id: String,
    /// `None` while lent.
    processor: Option<Box<dyn AudioProcessor>>,
    retired: bool,
}

/// The hosted processors, by instance; see the module documentation. Shared as
/// `Arc<Rack>` between the UI thread (add, retire, reap) and the thread that prepares chains
/// (lend, and every handle's drop). The tick never touches it: a lent processor is called
/// through its handle with no lock.
pub struct Rack {
    /// The rack itself, so a handle made in `lend(&self)` can hold it alive.
    me: Weak<Rack>,
    entries: Mutex<Vec<Entry>>,
    generation: AtomicU64,
}

impl Rack {
    /// An empty rack.
    pub fn new() -> Arc<Rack> {
        Arc::new_cyclic(|me| Rack {
            me: me.clone(),
            entries: Mutex::new(Vec::new()),
            generation: AtomicU64::new(0),
        })
    }

    fn entries(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// UI thread: puts `instance`'s audio half here, its saved state already loaded, for the
    /// chain whose insert names `instance` as `plugin_id`. A previous entry for the same
    /// instance is retired.
    pub fn add(
        &self,
        instance: InstanceId,
        plugin_id: impl Into<String>,
        processor: Box<dyn AudioProcessor>,
    ) {
        let mut entries = self.entries();
        for entry in entries.iter_mut().filter(|e| e.instance == instance) {
            entry.retired = true;
        }
        entries.push(Entry {
            instance,
            plugin_id: plugin_id.into(),
            processor: Some(processor),
            retired: false,
        });
        drop(entries);
        self.bump();
    }

    /// UI thread: no chain may borrow `instance` any more. Once its handle has brought it
    /// home, [`reap`](Rack::reap) hands it back to be dropped.
    pub fn retire(&self, instance: InstanceId) {
        for entry in self.entries().iter_mut().filter(|e| e.instance == instance) {
            entry.retired = true;
        }
        self.bump();
    }

    /// UI thread: every retired instance that is home, removed from the rack, for the caller
    /// to drop before its controller.
    pub fn reap(&self) -> Vec<(InstanceId, Box<dyn AudioProcessor>)> {
        let mut entries = self.entries();
        let mut out = Vec::new();
        let mut index = 0;
        while index < entries.len() {
            if entries[index].retired && entries[index].processor.is_some() {
                let entry = entries.remove(index);
                if let Some(processor) = entry.processor {
                    out.push((entry.instance, processor));
                }
            } else {
                index += 1;
            }
        }
        out
    }

    /// Asks the engine to lend again: what a plugin that changed its buses needs, since a
    /// chain sets a processor's layout only when it inserts it.
    pub fn touch(&self) {
        self.bump();
    }

    /// Whether `instance` is in the rack and not retired (lent or home).
    pub fn holds(&self, instance: InstanceId) -> bool {
        self.entries()
            .iter()
            .any(|e| e.instance == instance && !e.retired)
    }

    fn give_back(&self, instance: InstanceId, processor: Box<dyn AudioProcessor>) {
        let mut entries = self.entries();
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.instance == instance && e.processor.is_none())
        {
            entry.processor = Some(processor);
        }
        // An entry is only removed by `reap`, which skips a lent one, so there is always one.
    }
}

impl Lender for Rack {
    fn lend(
        &self,
        instance: InstanceId,
        plugin_id: &str,
    ) -> Result<Option<Box<dyn AudioProcessor>>, LendError> {
        // A rack is only ever made inside an `Arc` (`Rack::new`), and `lend` borrows it, so
        // the upgrade succeeds; the handle needs it to come home.
        let Some(rack) = self.me.upgrade() else {
            return Ok(None);
        };
        let mut entries = self.entries();
        let Some(entry) = entries
            .iter_mut()
            .find(|e| e.instance == instance && !e.retired)
        else {
            return Ok(None);
        };
        if entry.plugin_id != plugin_id {
            return Err(LendError::OtherPlugin {
                instance,
                plugin_id: plugin_id.to_string(),
                held: entry.plugin_id.clone(),
            });
        }
        let Some(processor) = entry.processor.take() else {
            return Err(LendError::Busy {
                instance,
                plugin_id: plugin_id.to_string(),
            });
        };
        Ok(Some(Box::new(Lent {
            rack,
            instance,
            inner: Some(processor),
        })))
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

/// What a chain holds: forwards every call to the lent processor, and on drop deactivates it
/// and puts it back in the rack.
struct Lent {
    rack: Arc<Rack>,
    instance: InstanceId,
    /// Always `Some` until `drop`.
    inner: Option<Box<dyn AudioProcessor>>,
}

impl Lent {
    fn inner(&self) -> &dyn AudioProcessor {
        self.inner
            .as_deref()
            .expect("a lent processor is present until its handle drops")
    }

    fn inner_mut(&mut self) -> &mut dyn AudioProcessor {
        self.inner
            .as_deref_mut()
            .expect("a lent processor is present until its handle drops")
    }
}

impl AudioProcessor for Lent {
    fn descriptor(&self) -> &ProcessorDescriptor {
        self.inner().descriptor()
    }

    fn set_bus_layout(&mut self, layout: &BusLayout) -> Result<(), AudioError> {
        self.inner_mut().set_bus_layout(layout)
    }

    fn setup_processing(&mut self, setup: &ProcessSetup) -> Result<(), AudioError> {
        self.inner_mut().setup_processing(setup)
    }

    fn set_active(&mut self, active: bool) {
        self.inner_mut().set_active(active);
    }

    fn process(&mut self, data: &mut AudioProcessData<'_>) {
        self.inner_mut().process(data);
    }

    fn latency_samples(&self) -> u32 {
        self.inner().latency_samples()
    }

    fn save_state(&self) -> Vec<u8> {
        self.inner().save_state()
    }

    fn load_state(&mut self, bytes: &[u8]) {
        self.inner_mut().load_state(bytes);
    }
}

impl Drop for Lent {
    fn drop(&mut self) {
        if let Some(mut processor) = self.inner.take() {
            processor.set_active(false);
            self.rack.give_back(self.instance, processor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimaeraproject_audio_processor::Gain;

    const GAIN: &str = "vixmix.test.gain";

    #[test]
    fn a_lent_instance_comes_home_and_only_one_chain_holds_it() {
        let rack = Rack::new();
        let instance = InstanceId(1);
        assert_eq!(rack.lend(instance, GAIN).unwrap().map(|_| ()), None);
        let before = rack.generation();
        rack.add(instance, GAIN, Box::new(Gain::new(GAIN)));
        assert!(rack.generation() > before, "adding moves the generation");

        let lent = rack.lend(instance, GAIN).unwrap().expect("held");
        assert_eq!(lent.descriptor().id, GAIN);
        assert!(matches!(
            rack.lend(instance, GAIN),
            Err(LendError::Busy { .. })
        ));
        assert!(matches!(
            rack.lend(instance, "vst3:other"),
            Err(LendError::OtherPlugin { .. })
        ));
        drop(lent);
        assert!(rack.lend(instance, GAIN).unwrap().is_some(), "home again");
    }

    #[test]
    fn a_retired_instance_is_reaped_only_once_it_is_home() {
        let rack = Rack::new();
        let instance = InstanceId(4);
        rack.add(instance, GAIN, Box::new(Gain::new(GAIN)));
        let lent = rack.lend(instance, GAIN).unwrap().expect("held");
        let before = rack.generation();
        rack.retire(instance);
        assert!(rack.generation() > before);
        assert!(!rack.holds(instance));
        assert!(rack.reap().is_empty(), "still lent: not reaped");
        drop(lent);
        let reaped = rack.reap();
        assert_eq!(reaped.len(), 1);
        assert_eq!(reaped[0].0, instance);
        assert!(rack.reap().is_empty());
    }
}
