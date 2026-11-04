//! What the engine's integration tests share: a rack shaped on the plugins crate's `Rack`
//! (lend once, `Busy` while lent, retire, a generation, a handle that comes home on drop and
//! says on which thread), a probe processor that logs what the engine did to it, and
//! snapshots built from the model's own setup so they keep up with it.
//!
//! Each test binary uses part of it.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle, ThreadId};

use chimaeraproject_audio_processor::{
    AudioError, AudioProcessData, AudioProcessor, BusLayout, ProcessSetup, ProcessorDescriptor,
};
use vixmix_engine::{Engine, EngineConfig, LendError, Lender, MAX_QUANTUM, NullBackend};
use vixmix_model::{
    ChannelKind, Command, EngineSnapshot, InsertSnap, InstanceId, Setup, apply, snapshot,
};

/// The plugin id every probe answers to.
pub const PLUGIN: &str = "vst3:0000000000000000000000000000beef";

pub fn engine() -> Arc<Engine> {
    Arc::new(Engine::new(EngineConfig::new(48_000, MAX_QUANTUM, 2)))
}

/// A setup with `channels` channels (at most eight), sends at unity, linked, no plugins.
pub fn setup(channels: usize) -> Setup {
    let mut setup = Setup::default();
    while setup.channels.len() < channels {
        let _ = apply(
            &mut setup,
            Command::AddChannel {
                kind: ChannelKind::Apps,
            },
        )
        .unwrap();
    }
    setup
}

/// `setup(channels)`'s snapshot, to put inserts into by hand.
pub fn snap(channels: usize) -> EngineSnapshot {
    snapshot(&setup(channels))
}

pub fn insert(instance: u32) -> InsertSnap {
    InsertSnap {
        instance: InstanceId(instance),
        plugin_id: PLUGIN.to_string(),
        enabled: true,
    }
}

/// A sine at `amplitude`.
pub fn sine(len: usize, amplitude: f32) -> Vec<f32> {
    (0..len)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * amplitude)
        .collect()
}

/// Fills channel `index`'s left and right with `signal`.
pub fn feed(null: &mut NullBackend, index: usize, signal: &[f32]) {
    let (left, right) = null.input_mut(index).unwrap();
    left[..signal.len()].copy_from_slice(signal);
    right[..signal.len()].copy_from_slice(signal);
}

// ---------------------------------------------------------------- the probe

/// What a test can read of a probe it no longer holds.
#[derive(Default)]
pub struct Log {
    /// `setup_processing` calls, and the rate and block of the last.
    pub setups: AtomicU32,
    pub rate: AtomicU32,
    pub max_block: AtomicU32,
    /// `set_active(true)` calls, and whether it is active now.
    pub activations: AtomicU32,
    pub active: AtomicBool,
    /// Blocks processed.
    pub blocks: AtomicU64,
    /// Parameter changes heard, over every block.
    pub changes: AtomicU64,
    /// Blocks whose change list said `is_initial`.
    pub initial_blocks: AtomicU32,
    /// The thread the probe itself was dropped on.
    pub dropped_on: Mutex<Option<ThreadId>>,
}

impl Log {
    pub fn get(counter: &AtomicU32) -> u32 {
        counter.load(Ordering::SeqCst)
    }
}

/// Blocks the probe's next `process` until the test lets it go: how a test holds a tick
/// mid-cycle on the data thread.
pub struct Gate {
    entered: SyncSender<()>,
    release: Receiver<()>,
}

/// The test's end of a [`Gate`].
pub struct GateControl {
    entered: Receiver<()>,
    release: SyncSender<()>,
}

impl GateControl {
    /// Waits until the probe is inside `process`.
    pub fn wait_entered(&self) {
        self.entered.recv().expect("the probe entered process");
    }

    /// Lets the probe's `process` return.
    pub fn release(&self) {
        self.release.send(()).expect("the probe is waiting");
    }
}

pub fn gate() -> (Gate, GateControl) {
    let (entered_tx, entered_rx) = sync_channel(1);
    let (release_tx, release_rx) = sync_channel(1);
    (
        Gate {
            entered: entered_tx,
            release: release_rx,
        },
        GateControl {
            entered: entered_rx,
            release: release_tx,
        },
    )
}

/// A plugin stand-in: multiplies by `gain`, then adds `tone` (a constant, heard even with
/// silent input), logging what it was asked to do. `process` allocates nothing unless it
/// has a gate.
pub struct Probe {
    descriptor: ProcessorDescriptor,
    gain: f32,
    tone: f32,
    log: Arc<Log>,
    gate: Option<Gate>,
}

impl Probe {
    pub fn new(gain: f32) -> (Probe, Arc<Log>) {
        let log = Arc::new(Log::default());
        (
            Probe {
                descriptor: ProcessorDescriptor {
                    id: PLUGIN.to_string(),
                    name: "Probe".to_string(),
                    version: "1".to_string(),
                    params: Vec::new(),
                },
                gain,
                tone: 0.0,
                log: Arc::clone(&log),
                gate: None,
            },
            log,
        )
    }

    pub fn tone(mut self, tone: f32) -> Probe {
        self.tone = tone;
        self
    }

    pub fn gated(mut self, gate: Gate) -> Probe {
        self.gate = Some(gate);
        self
    }
}

impl AudioProcessor for Probe {
    fn descriptor(&self) -> &ProcessorDescriptor {
        &self.descriptor
    }

    fn set_bus_layout(&mut self, layout: &BusLayout) -> Result<(), AudioError> {
        if *layout == BusLayout::stereo_insert() {
            Ok(())
        } else {
            Err(AudioError::UnsupportedBusLayout {
                processor: PLUGIN.to_string(),
                expected: "a stereo insert".to_string(),
                found: layout.describe(),
            })
        }
    }

    fn setup_processing(&mut self, setup: &ProcessSetup) -> Result<(), AudioError> {
        self.log.setups.fetch_add(1, Ordering::SeqCst);
        self.log
            .rate
            .store(setup.sample_rate as u32, Ordering::SeqCst);
        self.log
            .max_block
            .store(setup.max_block as u32, Ordering::SeqCst);
        Ok(())
    }

    fn set_active(&mut self, active: bool) {
        if active && !self.log.active.load(Ordering::SeqCst) {
            self.log.activations.fetch_add(1, Ordering::SeqCst);
        }
        self.log.active.store(active, Ordering::SeqCst);
    }

    fn process(&mut self, data: &mut AudioProcessData<'_>) {
        if let Some(gate) = self.gate.take() {
            let _ = gate.entered.send(());
            let _ = gate.release.recv();
        }
        self.log.blocks.fetch_add(1, Ordering::SeqCst);
        self.log
            .changes
            .fetch_add(data.param_changes.len() as u64, Ordering::SeqCst);
        if data.param_changes.is_initial() {
            self.log.initial_blocks.fetch_add(1, Ordering::SeqCst);
        }
        let n = data.num_samples;
        for bus in data.outputs.iter_mut() {
            for channel in bus.iter_mut() {
                for sample in &mut channel[..n] {
                    *sample = *sample * self.gain + self.tone;
                }
            }
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        *self.log.dropped_on.lock().unwrap() = Some(thread::current().id());
    }
}

// ---------------------------------------------------------------- the rack

struct Entry {
    instance: InstanceId,
    plugin_id: String,
    /// `None` while lent.
    processor: Option<Box<dyn AudioProcessor>>,
    retired: bool,
}

/// The plugins crate's `Rack` in miniature: holds processors by instance, lends each to one
/// borrower at a time through a handle whose `Drop` deactivates it and brings it home.
pub struct TestRack {
    me: Weak<TestRack>,
    entries: Mutex<Vec<Entry>>,
    generation: AtomicU64,
    lends: AtomicU64,
    homecomings: Mutex<Vec<(InstanceId, ThreadId)>>,
}

impl TestRack {
    pub fn new() -> Arc<TestRack> {
        Arc::new_cyclic(|me| TestRack {
            me: me.clone(),
            entries: Mutex::new(Vec::new()),
            generation: AtomicU64::new(0),
            lends: AtomicU64::new(0),
            homecomings: Mutex::new(Vec::new()),
        })
    }

    /// How many times anything asked to borrow from this rack.
    pub fn lends(&self) -> u64 {
        self.lends.load(Ordering::SeqCst)
    }

    /// Adds a probe for `instance` (retiring any earlier one) and returns its log.
    pub fn add_probe(&self, instance: u32, probe: Probe) -> Arc<Log> {
        let log = Arc::clone(&probe.log);
        self.add(InstanceId(instance), Box::new(probe));
        log
    }

    pub fn add(&self, instance: InstanceId, processor: Box<dyn AudioProcessor>) {
        let mut entries = self.entries.lock().unwrap();
        for entry in entries.iter_mut().filter(|e| e.instance == instance) {
            entry.retired = true;
        }
        entries.push(Entry {
            instance,
            plugin_id: PLUGIN.to_string(),
            processor: Some(processor),
            retired: false,
        });
        drop(entries);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn retire(&self, instance: u32) {
        for entry in self
            .entries
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|e| e.instance == InstanceId(instance))
        {
            entry.retired = true;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Moves the generation without changing anything held (the plugins crate's `touch`).
    pub fn touch(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Whether every entry for `instance` is home (not lent).
    pub fn is_home(&self, instance: u32) -> bool {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.instance == InstanceId(instance))
            .all(|e| e.processor.is_some())
    }

    /// Every handle that came home, with the thread it was dropped on.
    pub fn homecomings(&self) -> Vec<(InstanceId, ThreadId)> {
        self.homecomings.lock().unwrap().clone()
    }

    fn give_back(&self, instance: InstanceId, processor: Box<dyn AudioProcessor>) {
        self.homecomings
            .lock()
            .unwrap()
            .push((instance, thread::current().id()));
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.instance == instance && e.processor.is_none())
        {
            entry.processor = Some(processor);
        }
    }
}

impl Lender for TestRack {
    fn lend(
        &self,
        instance: InstanceId,
        plugin_id: &str,
    ) -> Result<Option<Box<dyn AudioProcessor>>, LendError> {
        self.lends.fetch_add(1, Ordering::SeqCst);
        let rack = self.me.upgrade().expect("a rack lives in an Arc");
        let mut entries = self.entries.lock().unwrap();
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
        Ok(Some(Box::new(Handle {
            rack,
            instance,
            inner: Some(processor),
        })))
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

/// What the engine holds: forwards every call; on drop, deactivates and goes home.
struct Handle {
    rack: Arc<TestRack>,
    instance: InstanceId,
    inner: Option<Box<dyn AudioProcessor>>,
}

impl Handle {
    fn inner(&mut self) -> &mut dyn AudioProcessor {
        self.inner.as_deref_mut().expect("present until dropped")
    }
}

impl AudioProcessor for Handle {
    fn descriptor(&self) -> &ProcessorDescriptor {
        self.inner
            .as_deref()
            .expect("present until dropped")
            .descriptor()
    }

    fn set_bus_layout(&mut self, layout: &BusLayout) -> Result<(), AudioError> {
        self.inner().set_bus_layout(layout)
    }

    fn setup_processing(&mut self, setup: &ProcessSetup) -> Result<(), AudioError> {
        self.inner().setup_processing(setup)
    }

    fn set_active(&mut self, active: bool) {
        self.inner().set_active(active);
    }

    fn process(&mut self, data: &mut AudioProcessData<'_>) {
        self.inner().process(data);
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if let Some(mut processor) = self.inner.take() {
            processor.set_active(false);
            self.rack.give_back(self.instance, processor);
        }
    }
}

// ---------------------------------------------------------------- a data thread

/// What one pump on the data thread wrote, and where it ran.
pub struct Pumped {
    pub monitor: Vec<f32>,
    pub master: Vec<f32>,
    pub thread: ThreadId,
}

/// A thread that owns a `NullBackend` and pumps when told, as PipeWire's data thread calls
/// the tick: the test can do host-thread work while a tick is mid-cycle.
pub struct DataThread {
    commands: SyncSender<(usize, Vec<f32>)>,
    pumped: Receiver<Pumped>,
    thread: JoinHandle<()>,
}

impl DataThread {
    /// A data thread feeding the same signal into each of the first `channels` channels.
    pub fn spawn(engine: Arc<Engine>, channels: usize) -> DataThread {
        let (commands, inbox) = sync_channel::<(usize, Vec<f32>)>(1);
        let (outbox, pumped) = sync_channel(1);
        let thread = thread::spawn(move || {
            let mut null = NullBackend::new(engine, MAX_QUANTUM);
            for (frames, signal) in inbox {
                for index in 0..channels {
                    feed(&mut null, index, &signal);
                }
                null.pump(frames);
                let pumped = Pumped {
                    monitor: null.monitor().0.to_vec(),
                    master: null.master().0.to_vec(),
                    thread: thread::current().id(),
                };
                if outbox.send(pumped).is_err() {
                    return;
                }
            }
        });
        DataThread {
            commands,
            pumped,
            thread,
        }
    }

    /// Starts one tick of `signal.len()` frames; [`result`](DataThread::result) waits for it.
    pub fn pump(&self, signal: &[f32]) {
        self.commands
            .send((signal.len(), signal.to_vec()))
            .expect("the data thread is running");
    }

    /// What the last started tick wrote.
    pub fn result(&self) -> Pumped {
        self.pumped.recv().expect("the data thread pumped")
    }

    /// Stops the thread and waits for it.
    pub fn stop(self) {
        drop(self.commands);
        self.thread.join().expect("the data thread ran to its end");
    }
}
