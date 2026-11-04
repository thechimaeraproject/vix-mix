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
//! even when the setup did not change. The engine lends each instance once and shares it
//! between its graphs (change request 5): on a move it asks again for every insert, keeps an
//! instance it already holds (`Busy` means still the engine's), takes a new instance under a
//! retired id, and sets a held instance up again only through `Engine::set_up_again`.
//!
//! **Edits travel with the processor.** Each entry keeps the consumer end of its instance's
//! edit ring (`crate::edits`) beside the processor; the handle takes both, merges what the ring
//! holds into each block's changes, and gives both back when it drops.
//!
//! **Teardown stays on the UI thread.** A retired instance is not dropped when its handle
//! goes home: it waits, inactive, until [`Rack::reap`] hands it to the UI thread, which drops
//! it before the instance's controller, as the kit asks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use chimaeraproject_audio_processor::{
    AudioError, AudioProcessData, AudioProcessor, BusLayout, ProcessSetup, ProcessorDescriptor,
};
use chimaeraproject_param::ParamChanges;
use vixmix_engine::{LendError, Lender};
use vixmix_model::InstanceId;

use crate::edits::{self, EditReceiver, EditSender};

struct Entry {
    /// Tells two entries of one instance apart: a re-created instance is a new entry.
    serial: u64,
    instance: InstanceId,
    plugin_id: String,
    /// `None` while lent.
    processor: Option<Box<dyn AudioProcessor>>,
    /// The consumer end of the instance's edit ring; `None` while lent.
    edits: Option<EditReceiver>,
    retired: bool,
}

/// A retired entry that came home, for the UI thread to drop.
pub(crate) struct Reaped {
    pub(crate) serial: u64,
    pub(crate) instance: InstanceId,
    pub(crate) processor: Box<dyn AudioProcessor>,
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
    next_serial: AtomicU64,
}

impl Rack {
    /// An empty rack.
    pub fn new() -> Arc<Rack> {
        Arc::new_cyclic(|me| Rack {
            me: me.clone(),
            entries: Mutex::new(Vec::new()),
            generation: AtomicU64::new(0),
            next_serial: AtomicU64::new(1),
        })
    }

    fn entries(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// UI thread: puts `instance`'s audio half here, its saved state already loaded, for the
    /// chain whose insert names `instance` as `plugin_id`, and returns the UI thread's end of
    /// its edit ring (drop it for a processor only its chain's parameters reach). A previous
    /// entry for the same instance is retired.
    pub fn add(
        &self,
        instance: InstanceId,
        plugin_id: impl Into<String>,
        processor: Box<dyn AudioProcessor>,
    ) -> EditSender {
        self.add_entry(instance, plugin_id.into(), processor).1
    }

    /// [`add`](Rack::add), also returning the entry's serial.
    pub(crate) fn add_entry(
        &self,
        instance: InstanceId,
        plugin_id: String,
        processor: Box<dyn AudioProcessor>,
    ) -> (u64, EditSender) {
        let serial = self.next_serial.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = edits::ring();
        let mut entries = self.entries();
        for entry in entries.iter_mut().filter(|e| e.instance == instance) {
            entry.retired = true;
        }
        entries.push(Entry {
            serial,
            instance,
            plugin_id,
            processor: Some(processor),
            edits: Some(receiver),
            retired: false,
        });
        drop(entries);
        self.bump();
        (serial, sender)
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
    /// to drop before its controller. A rack that [`Instances`](crate::Instances) fills is
    /// reaped through [`Instances::reap`](crate::Instances::reap), which matches each
    /// processor to its controller.
    pub fn reap(&self) -> Vec<(InstanceId, Box<dyn AudioProcessor>)> {
        self.reap_entries()
            .into_iter()
            .map(|r| (r.instance, r.processor))
            .collect()
    }

    pub(crate) fn reap_entries(&self) -> Vec<Reaped> {
        let mut entries = self.entries();
        let mut out = Vec::new();
        let mut index = 0;
        while index < entries.len() {
            if entries[index].retired && entries[index].processor.is_some() {
                let entry = entries.remove(index);
                if let Some(processor) = entry.processor {
                    out.push(Reaped {
                        serial: entry.serial,
                        instance: entry.instance,
                        processor,
                    });
                }
            } else {
                index += 1;
            }
        }
        out
    }

    /// Moves the generation, so the engine prepares again. A plugin that changed its buses
    /// also needs `Engine::set_up_again` (the host calls it on `InstanceEvent::IoChanged`,
    /// change request 5): the engine keeps a held instance across generations and sets it
    /// up again only when asked.
    pub fn touch(&self) {
        self.bump();
    }

    /// Whether `instance` is in the rack and not retired (lent or home).
    pub fn holds(&self, instance: InstanceId) -> bool {
        self.entries()
            .iter()
            .any(|e| e.instance == instance && !e.retired)
    }

    /// Whether `instance`'s live entry is lent to a chain right now.
    pub fn is_lent(&self, instance: InstanceId) -> bool {
        self.entries()
            .iter()
            .any(|e| e.instance == instance && !e.retired && e.processor.is_none())
    }

    /// Entries in the rack, retired ones still waiting to be reaped included.
    pub fn len(&self) -> usize {
        self.entries().len()
    }

    /// Whether the rack holds nothing at all.
    pub fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    fn give_back(
        &self,
        serial: u64,
        processor: Box<dyn AudioProcessor>,
        edits: Option<EditReceiver>,
    ) {
        let mut entries = self.entries();
        match entries.iter_mut().find(|e| e.serial == serial) {
            Some(entry) => {
                entry.processor = Some(processor);
                entry.edits = edits;
            }
            // An entry is only removed by `reap`, which skips a lent one, so this is never
            // reached; if it were, the processor is dropped here rather than lost.
            None => {
                drop(entries);
                drop(processor);
            }
        }
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
        // Sized here, on the thread that prepares chains, so a block never grows it.
        let merged = ParamChanges::with_capacity(edits::merged_capacity(
            processor.descriptor().params.len(),
        ));
        Ok(Some(Box::new(Lent {
            rack,
            serial: entry.serial,
            inner: Some(processor),
            edits: entry.edits.take(),
            merged,
        })))
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

/// What a chain holds: forwards every call to the lent processor, merges the instance's
/// pending edits into each block's changes, and on drop deactivates the processor and puts it
/// back in the rack with its edit ring.
struct Lent {
    rack: Arc<Rack>,
    serial: u64,
    /// Always `Some` until `drop`.
    inner: Option<Box<dyn AudioProcessor>>,
    edits: Option<EditReceiver>,
    /// The chain's changes plus the edits, for a block that has edits; sized in `lend`.
    merged: ParamChanges,
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

    /// Audio thread. With no edit waiting this is a plain forward. Otherwise the chain's
    /// changes and the edits go to the processor together; when the chain's changes alone
    /// fill the merged list, or the block has more buses than can be rebuilt, the edits wait
    /// for the next block.
    fn process(&mut self, data: &mut AudioProcessData<'_>) {
        let Lent {
            inner,
            edits,
            merged,
            ..
        } = self;
        let Some(inner) = inner.as_deref_mut() else {
            return;
        };
        match edits {
            Some(edits)
                if !edits.is_empty()
                    && data.param_changes.len() < merged.capacity()
                    && edits::fits(data) =>
            {
                edits::merge(data.param_changes, edits, merged);
                edits::process_with(inner, data, merged);
            }
            _ => inner.process(data),
        }
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
            self.rack
                .give_back(self.serial, processor, self.edits.take());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::thread::ThreadId;

    use super::*;
    use chimaeraproject_audio_processor::{
        BusBuffer, EngineConfig, Gain, ProcessorChain, ProcessorInstanceId, ProcessorParams,
        TransportInfo,
    };

    const GAIN: &str = "vixmix.test.gain";

    /// A processor that records the thread it was dropped on.
    struct Probe {
        descriptor: ProcessorDescriptor,
        dropped_on: Arc<Mutex<Option<ThreadId>>>,
    }

    impl Probe {
        fn new(dropped_on: &Arc<Mutex<Option<ThreadId>>>) -> Probe {
            Probe {
                descriptor: Gain::new(GAIN).descriptor().clone(),
                dropped_on: Arc::clone(dropped_on),
            }
        }
    }

    impl AudioProcessor for Probe {
        fn descriptor(&self) -> &ProcessorDescriptor {
            &self.descriptor
        }
        fn set_bus_layout(&mut self, _layout: &BusLayout) -> Result<(), AudioError> {
            Ok(())
        }
        fn setup_processing(&mut self, _setup: &ProcessSetup) -> Result<(), AudioError> {
            Ok(())
        }
        fn set_active(&mut self, _active: bool) {}
        fn process(&mut self, _data: &mut AudioProcessData<'_>) {}
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            *self.dropped_on.lock().unwrap() = Some(std::thread::current().id());
        }
    }

    /// One block of ones through a chain the rack lends `instance` to, as the engine fills
    /// one; the first sample out.
    fn run(rack: &Rack, chain: &mut ProcessorChain, instance: InstanceId) -> f32 {
        let desired: [ProcessorParams; 1] = [ProcessorParams::new(
            ProcessorInstanceId(u64::from(instance.0)),
            GAIN,
        )];
        if chain.is_empty() {
            chain.fill_with(&desired, |_, key| match rack.lend(instance, key) {
                Ok(Some(processor)) => Ok(processor),
                Ok(None) => Err(AudioError::UnknownProcessor(key.to_string())),
                Err(error) => Err(AudioError::SetupFailed {
                    processor: key.to_string(),
                    reason: error.to_string(),
                }),
            });
        }
        let mut bus = BusBuffer::new(2, 16);
        for channel in 0..2 {
            bus.channel_mut(channel)[..16].fill(1.0);
        }
        chain.process(&mut bus, 16, &TransportInfo::stopped(), &desired);
        bus.channel(0)[0]
    }

    fn chain() -> ProcessorChain {
        let config = EngineConfig::new(48_000, 16, 2);
        ProcessorChain::new(ProcessSetup::realtime(&config), BusLayout::stereo_insert())
    }

    fn normalized_db(db: f64) -> f64 {
        (db - Gain::MIN_DB) / (Gain::MAX_DB - Gain::MIN_DB)
    }

    #[test]
    fn a_lent_instance_comes_home_and_only_one_chain_holds_it() {
        let rack = Rack::new();
        let instance = InstanceId(1);
        assert_eq!(rack.lend(instance, GAIN).unwrap().map(|_| ()), None);
        let before = rack.generation();
        let _edits = rack.add(instance, GAIN, Box::new(Gain::new(GAIN)));
        assert!(rack.generation() > before, "adding moves the generation");

        let lent = rack.lend(instance, GAIN).unwrap().expect("held");
        assert_eq!(lent.descriptor().id, GAIN);
        assert!(rack.is_lent(instance));
        assert!(matches!(
            rack.lend(instance, GAIN),
            Err(LendError::Busy { .. })
        ));
        assert!(matches!(
            rack.lend(instance, "vst3:other"),
            Err(LendError::OtherPlugin { .. })
        ));
        drop(lent);
        assert!(!rack.is_lent(instance));
        assert!(rack.lend(instance, GAIN).unwrap().is_some(), "home again");
    }

    #[test]
    fn a_retired_instance_is_reaped_only_once_it_is_home() {
        let rack = Rack::new();
        let instance = InstanceId(4);
        let _edits = rack.add(instance, GAIN, Box::new(Gain::new(GAIN)));
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
        assert!(rack.is_empty());
    }

    #[test]
    fn an_edit_reaches_the_lent_processor_in_its_next_block() {
        let rack = Rack::new();
        let instance = InstanceId(2);
        let mut edits = rack.add(instance, GAIN, Box::new(Gain::new(GAIN)));
        // Sent while the instance is home: it waits in the ring and goes with the processor.
        assert!(edits.send(Gain::GAIN_DB_ID, normalized_db(-6.0)));
        let mut chain = chain();
        let half = 10f32.powf(-6.0 / 20.0);
        assert!((run(&rack, &mut chain, instance) - half).abs() < 1e-6);
        assert!(
            (run(&rack, &mut chain, instance) - half).abs() < 1e-6,
            "the plugin keeps it"
        );
        // Sent while lent: the next block hears it.
        assert!(edits.send(Gain::GAIN_DB_ID, normalized_db(-12.0)));
        let quarter = 10f32.powf(-12.0 / 20.0);
        assert!((run(&rack, &mut chain, instance) - quarter).abs() < 1e-6);
        // Home and lent again: the ring came back with it.
        chain.clear();
        assert!(edits.send(Gain::GAIN_DB_ID, normalized_db(0.0)));
        assert!((run(&rack, &mut chain, instance) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_retired_instance_is_dropped_on_the_thread_that_reaps_not_the_one_that_lent_it() {
        let rack = Rack::new();
        let instance = InstanceId(3);
        let dropped_on = Arc::new(Mutex::new(None));
        let _edits = rack.add(instance, GAIN, Box::new(Probe::new(&dropped_on)));
        let lender = Arc::clone(&rack);
        let (clear, cleared) = std::sync::mpsc::sync_channel::<()>(1);
        let preparing = std::thread::spawn(move || {
            let lent = lender.lend(instance, GAIN).unwrap().expect("held");
            // The UI thread retires it while the chain still holds it.
            let _ = cleared.recv();
            drop(lent);
            std::thread::current().id()
        });
        while !rack.is_lent(instance) {
            std::thread::yield_now();
        }
        rack.retire(instance);
        assert!(rack.reap().is_empty(), "still lent");
        clear.send(()).unwrap();
        let preparing_id = preparing.join().unwrap();
        assert_eq!(*dropped_on.lock().unwrap(), None, "home, not dropped");
        let reaped = rack.reap();
        assert_eq!(reaped.len(), 1);
        drop(reaped);
        let here = std::thread::current().id();
        assert_eq!(*dropped_on.lock().unwrap(), Some(here));
        assert_ne!(here, preparing_id);
    }

    #[test]
    fn an_instance_made_again_retires_the_old_entry_and_each_comes_home_to_its_own() {
        let rack = Rack::new();
        let instance = InstanceId(5);
        let (old_dropped, new_dropped) = (Arc::new(Mutex::new(None)), Arc::new(Mutex::new(None)));
        let (old_serial, _old_edits) = rack.add_entry(
            instance,
            GAIN.to_string(),
            Box::new(Probe::new(&old_dropped)),
        );
        let old = rack.lend(instance, GAIN).unwrap().expect("held");
        let (new_serial, _new_edits) = rack.add_entry(
            instance,
            GAIN.to_string(),
            Box::new(Probe::new(&new_dropped)),
        );
        assert_ne!(old_serial, new_serial);
        // The new entry lends while the old one is still out.
        let new = rack.lend(instance, GAIN).unwrap().expect("the new entry");
        assert!(rack.reap_entries().is_empty());
        drop(old);
        let reaped = rack.reap_entries();
        assert_eq!(
            reaped.iter().map(|r| r.serial).collect::<Vec<_>>(),
            [old_serial]
        );
        drop(reaped);
        assert!(old_dropped.lock().unwrap().is_some());
        assert!(new_dropped.lock().unwrap().is_none());
        drop(new);
        assert!(rack.holds(instance));
        assert!(
            rack.reap_entries().is_empty(),
            "the new entry is not retired"
        );
    }
}
