//! The data thread's side of the backend: everything PipeWire's `process` callbacks touch.
//!
//! **Forbidden here** (`docs/reference/pipewire.md`): allocating or freeing, taking a lock
//! that can wait, logging, any system call of ours, touching a proxy. Everything the data
//! thread reaches is either owned by one callback's user data, an atomic, a lock-free ring
//! (`rtrb`), or the tick's state behind [`TickCell`], which a callback claims with one
//! compare-and-swap and never waits for. The one exception is the first engine tick on a
//! thread, which `arc_swap` lets allocate once (`docs/reference/engine.md`).
//!
//! Each callback body runs inside `assert_no_alloc`, so a test binary with its allocator
//! (`tests/headless.rs`) aborts on the first allocation; in a release build it is a no-op.

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool, AtomicPtr, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};

use assert_no_alloc::{assert_no_alloc, permit_alloc};
use pipewire::spa::buffer::Data;
use pipewire::stream::Stream;
use pipewire::sys as pw_sys;
use rtrb::{Consumer, Producer, RingBuffer};
use vixmix_engine::{ChannelInput, Engine, MAX_QUANTUM, Tick};
use vixmix_model::MAX_CHANNELS;

use crate::{BackendStats, TickSource};

/// One stereo frame in an input's ring.
pub(crate) type Frame = [f32; 2];

/// An input ring's capacity in frames: three times PipeWire's default quantum limit, so a
/// tick that comes two cycles late at the largest quantum loses nothing.
pub(crate) const RING_FRAMES: usize = 3 * MAX_QUANTUM;

/// Plans in flight each way between the control thread and the tick.
const PLAN_SLOTS: usize = 16;

/// The three output streams; each can be the tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Role {
    Monitor = 0,
    MasterOut = 1,
    MasterSource = 2,
}

/// `elected` when no output ticks.
const NO_ROLE: u8 = 3;

impl Role {
    pub(crate) const ALL: [Role; 3] = [Role::Monitor, Role::MasterOut, Role::MasterSource];

    pub(crate) fn tick_source(self) -> TickSource {
        match self {
            Role::Monitor => TickSource::Monitor,
            Role::MasterOut => TickSource::MasterOut,
            Role::MasterSource => TickSource::MasterSource,
        }
    }
}

/// What the data thread counts; [`Shared::stats`] reads it.
#[derive(Default)]
pub(crate) struct Counters {
    ticks: AtomicU64,
    rate: AtomicU32,
    quantum: AtomicU32,
    xruns: AtomicU64,
    underruns: AtomicU64,
    overflows: AtomicU64,
    ring_checks: AtomicU64,
    ring_off: AtomicU64,
    cycles: [AtomicU64; 3],
    stale: AtomicU64,
}

/// What one input stream shares with the tick: whether it runs, and which cycle it last
/// delivered in (the driver's `clock.nsec`, the same for every node of one cycle).
#[derive(Default)]
pub(crate) struct ChannelShared {
    pub streaming: AtomicBool,
    last_write: AtomicU64,
}

/// An input stream's `process` user data: the producing end of its ring.
pub(crate) struct InputRt {
    producer: Producer<Frame>,
    channel: Arc<ChannelShared>,
    shared: Arc<Shared>,
}

/// The tick's end of one input: the ring's consumer and the planes the tick hands the engine.
pub(crate) struct Tap {
    consumer: Consumer<Frame>,
    channel: Arc<ChannelShared>,
    /// The ring has held one quantum ahead of the tick since it last ran short.
    primed: bool,
    left: Box<[f32]>,
    right: Box<[f32]>,
}

/// Makes an input's ring: the producer for its stream's `process`, the tap for the tick.
pub(crate) fn input_ring(shared: &Arc<Shared>) -> (InputRt, Tap, Arc<ChannelShared>) {
    let (producer, consumer) = RingBuffer::new(RING_FRAMES);
    let channel = Arc::new(ChannelShared::default());
    let rt = InputRt {
        producer,
        channel: Arc::clone(&channel),
        shared: Arc::clone(shared),
    };
    let tap = Tap {
        consumer,
        channel: Arc::clone(&channel),
        primed: false,
        left: vec![0.0; MAX_QUANTUM].into_boxed_slice(),
        right: vec![0.0; MAX_QUANTUM].into_boxed_slice(),
    };
    (rt, tap, channel)
}

/// A change to the tick's inputs, built on the control thread and taken by the tick at the
/// start of a cycle; the tick sends it back holding the taps it replaced, so they are freed
/// on the control thread.
pub(crate) struct TickPlan {
    /// For each slot marked in `replace`, the tap that takes it (`None` empties it).
    pub taps: [Option<Tap>; MAX_CHANNELS],
    pub replace: [bool; MAX_CHANNELS],
    /// The engine's input order: position `i` reads slot `order[i]`, silence when `None`.
    pub order: [Option<usize>; MAX_CHANNELS],
    /// How many channels the engine has.
    pub count: usize,
}

impl TickPlan {
    pub(crate) fn new() -> Box<TickPlan> {
        Box::new(TickPlan {
            taps: std::array::from_fn(|_| None),
            replace: [false; MAX_CHANNELS],
            order: [None; MAX_CHANNELS],
            count: 0,
        })
    }
}

/// The control thread's ends of the plan rings.
pub(crate) struct PlanPort {
    pub to_tick: Producer<Box<TickPlan>>,
    pub from_tick: Consumer<Box<TickPlan>>,
}

/// The mix the tick computed this cycle, for the outputs that run after it in the same cycle.
/// Atomics rather than a buffer behind a lock: the tick stores the samples and then the
/// cycle's stamp (release); an output copies them only when the stamp is its own cycle's.
struct Staging {
    stamp: AtomicU64,
    frames: AtomicUsize,
    /// Monitor left and right, Master left and right.
    planes: [Box<[AtomicU32]>; 4],
}

impl Staging {
    fn new() -> Staging {
        Staging {
            stamp: AtomicU64::new(0),
            frames: AtomicUsize::new(0),
            planes: std::array::from_fn(|_| (0..MAX_QUANTUM).map(|_| AtomicU32::new(0)).collect()),
        }
    }

    fn publish(&self, stamp: u64, n: usize, mixes: [&[f32]; 4]) {
        for (plane, samples) in self.planes.iter().zip(mixes) {
            for (slot, sample) in plane.iter().zip(&samples[..n]) {
                slot.store(sample.to_bits(), Ordering::Relaxed);
            }
        }
        self.frames.store(n, Ordering::Relaxed);
        self.stamp.store(stamp, Ordering::Release);
    }

    /// Copies this cycle's mix for `role` into `left` and `right`; false when the tick has not
    /// staged one in the cycle stamped `stamp`.
    fn read(&self, stamp: u64, role: Role, left: &mut [f32], right: &mut [f32]) -> bool {
        if stamp == 0 || self.stamp.load(Ordering::Acquire) != stamp {
            return false;
        }
        let n = self.frames.load(Ordering::Relaxed);
        let first = if role == Role::Monitor { 0 } else { 2 };
        for (out, plane) in [left, right]
            .into_iter()
            .zip(&self.planes[first..first + 2])
        {
            for (i, sample) in out.iter_mut().enumerate() {
                *sample = if i < n {
                    f32::from_bits(plane[i].load(Ordering::Relaxed))
                } else {
                    0.0
                };
            }
        }
        true
    }
}

/// The streams the tick triggers each cycle (`PW_STREAM_FLAG_TRIGGER`), by role. The control
/// thread publishes a stream after connecting it and retracts it before disconnecting it;
/// `firing` counts ticks between their loads and their last trigger, so a retraction waits
/// out a tick that may still hold the old pointer.
pub(crate) struct Triggers {
    streams: [AtomicPtr<pw_sys::pw_stream>; 3],
    firing: AtomicUsize,
}

impl Triggers {
    fn new() -> Triggers {
        Triggers {
            streams: std::array::from_fn(|_| AtomicPtr::new(std::ptr::null_mut())),
            firing: AtomicUsize::new(0),
        }
    }

    /// Control thread: the tick triggers `stream` from now on. It stays valid until
    /// [`retract`](Triggers::retract).
    pub(crate) fn publish(&self, role: Role, stream: &Stream) {
        self.streams[role as usize].store(stream.as_raw_ptr(), Ordering::SeqCst);
    }

    /// Control thread: the tick stops triggering `role`'s stream; when this returns no tick
    /// can still reach it, so it may be disconnected.
    pub(crate) fn retract(&self, role: Role) {
        self.streams[role as usize].store(std::ptr::null_mut(), Ordering::SeqCst);
        while self.firing.load(Ordering::SeqCst) != 0 {
            std::thread::yield_now();
        }
    }

    /// Data thread: triggers every published stream but the tick's own.
    fn fire(&self, from: Role) {
        self.firing.fetch_add(1, Ordering::SeqCst);
        for role in Role::ALL {
            if role == from {
                continue;
            }
            let stream = self.streams[role as usize].load(Ordering::SeqCst);
            if !stream.is_null() {
                // SAFETY: a published pointer is a live, connected `pw_stream` until
                // `retract` returns, and `retract` cannot return while `firing` counts us.
                // `pw_stream_trigger_process` is meant to be called from another stream's
                // realtime `process` (PipeWire's module-loopback does exactly this).
                unsafe {
                    pw_sys::pw_stream_trigger_process(stream);
                }
            }
        }
        self.firing.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Everything the control thread and the data thread share. Lives as long as the backend,
/// across reconnects.
pub(crate) struct Shared {
    pub engine: Arc<Engine>,
    counters: Counters,
    /// The role whose `process` runs the tick ([`Role`] as `u8`, or `NO_ROLE`).
    elected: AtomicU8,
    pub triggers: Triggers,
    staging: Staging,
    tick: TickCell,
}

impl Shared {
    pub(crate) fn new(engine: Arc<Engine>) -> (PlanPort, Arc<Shared>) {
        let (to_tick, inbox) = RingBuffer::new(PLAN_SLOTS);
        let (outbox, from_tick) = RingBuffer::new(PLAN_SLOTS);
        let state = TickState {
            inbox,
            outbox,
            slots: std::array::from_fn(|_| None),
            order: [None; MAX_CHANNELS],
            count: 0,
            silence: vec![0.0; MAX_QUANTUM].into_boxed_slice(),
            monitor: [
                vec![0.0; MAX_QUANTUM].into_boxed_slice(),
                vec![0.0; MAX_QUANTUM].into_boxed_slice(),
            ],
            master: [
                vec![0.0; MAX_QUANTUM].into_boxed_slice(),
                vec![0.0; MAX_QUANTUM].into_boxed_slice(),
            ],
            warm: false,
            last_stamp: 0,
            last_frames: 0,
        };
        let shared = Arc::new(Shared {
            engine,
            counters: Counters::default(),
            elected: AtomicU8::new(NO_ROLE),
            triggers: Triggers::new(),
            staging: Staging::new(),
            tick: TickCell {
                busy: AtomicBool::new(false),
                state: UnsafeCell::new(state),
            },
        });
        (PlanPort { to_tick, from_tick }, shared)
    }

    /// Control thread: which output's `process` runs the tick from the next cycle.
    pub(crate) fn elect(&self, role: Option<Role>) {
        self.elected
            .store(role.map_or(NO_ROLE, |r| r as u8), Ordering::SeqCst);
    }

    pub(crate) fn elected(&self) -> Option<Role> {
        let elected = self.elected.load(Ordering::SeqCst);
        Role::ALL.into_iter().find(|r| *r as u8 == elected)
    }

    pub(crate) fn stats(&self) -> BackendStats {
        let c = &self.counters;
        let load = |a: &AtomicU64| a.load(Ordering::Relaxed);
        BackendStats {
            tick_source: self
                .elected()
                .map_or(TickSource::Stopped, Role::tick_source),
            ticks: load(&c.ticks),
            rate: c.rate.load(Ordering::Relaxed),
            quantum: c.quantum.load(Ordering::Relaxed),
            xruns: load(&c.xruns),
            underruns: load(&c.underruns),
            overflows: load(&c.overflows),
            ring_checks: load(&c.ring_checks),
            ring_off: load(&c.ring_off),
            monitor_cycles: load(&c.cycles[Role::Monitor as usize]),
            master_out_cycles: load(&c.cycles[Role::MasterOut as usize]),
            master_source_cycles: load(&c.cycles[Role::MasterSource as usize]),
            stale_cycles: load(&c.stale),
        }
    }

    fn xrun(&self) {
        self.counters.xruns.fetch_add(1, Ordering::Relaxed);
        self.engine.stats().xruns.fetch_add(1, Ordering::Relaxed);
    }
}

/// The tick's state, claimed by whichever output runs the tick. The claim is one
/// compare-and-swap that never waits: a second claimant (which the single data thread never
/// produces) gets `None` and writes silence.
struct TickCell {
    busy: AtomicBool,
    state: UnsafeCell<TickState>,
}

// SAFETY: `state` is reached only through `claim`, which hands `&mut TickState` to one caller
// at a time (the `busy` flag, acquired by compare-and-swap and released when the closure
// returns or unwinds), and `TickState` is `Send`.
unsafe impl Sync for TickCell {}

const _: () = {
    const fn send<T: Send>() {}
    send::<TickState>();
};

impl TickCell {
    fn claim<R>(&self, f: impl FnOnce(&mut TickState) -> R) -> Option<R> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        struct Release<'a>(&'a AtomicBool);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _release = Release(&self.busy);
        // SAFETY: the flag is ours until `_release` drops; see `unsafe impl Sync`.
        Some(f(unsafe { &mut *self.state.get() }))
    }
}

struct TickState {
    inbox: Consumer<Box<TickPlan>>,
    outbox: Producer<Box<TickPlan>>,
    slots: [Option<Tap>; MAX_CHANNELS],
    order: [Option<usize>; MAX_CHANNELS],
    count: usize,
    silence: Box<[f32]>,
    monitor: [Box<[f32]>; 2],
    master: [Box<[f32]>; 2],
    /// The engine has ticked on this thread before (its first tick may allocate once).
    warm: bool,
    last_stamp: u64,
    last_frames: usize,
}

impl TickState {
    /// Takes every plan the control thread sent, as long as there is room to send each back.
    fn take_plans(&mut self) {
        while self.outbox.slots() > 0 {
            let Ok(mut plan) = self.inbox.pop() else {
                break;
            };
            for (slot, (tap, replace)) in self
                .slots
                .iter_mut()
                .zip(plan.taps.iter_mut().zip(plan.replace))
            {
                if replace {
                    std::mem::swap(slot, tap);
                }
            }
            self.order = plan.order;
            self.count = plan.count.min(MAX_CHANNELS);
            // Room was checked above, so the plan never drops here.
            let _ = self.outbox.push(plan);
        }
    }

    /// One cycle of `n` frames: the plans, the rings, the engine, the staging.
    fn run(&mut self, shared: &Shared, n: usize, stamp: u64, rate: u32) {
        self.take_plans();
        let counters = &shared.counters;

        // A tick more than one and a half periods after the last one: the graph skipped a
        // cycle. Much longer gaps are a graph that stopped (nothing linked), not an xrun.
        if self.last_stamp != 0 && stamp > self.last_stamp && rate > 0 && self.last_frames > 0 {
            let period = self.last_frames as u64 * 1_000_000_000 / u64::from(rate);
            let gap = stamp - self.last_stamp;
            if gap > period * 3 / 2 && gap < period * 16 {
                shared.xrun();
            }
        }

        for tap in self.slots.iter_mut().flatten() {
            tap.govern(n, stamp, shared);
        }

        let TickState {
            slots,
            order,
            count,
            silence,
            monitor: [monitor_l, monitor_r],
            master: [master_l, master_r],
            warm,
            ..
        } = self;
        let silence = &silence[..n];
        let inputs: [ChannelInput<'_>; MAX_CHANNELS] =
            std::array::from_fn(|i| match order[i].and_then(|slot| slots[slot].as_ref()) {
                Some(tap) => ChannelInput {
                    left: &tap.left[..n],
                    right: &tap.right[..n],
                },
                None => ChannelInput {
                    left: silence,
                    right: silence,
                },
            });
        let tick = Tick {
            frames: n,
            channel_inputs: &inputs[..*count],
            monitor_out: [&mut monitor_l[..n], &mut monitor_r[..n]],
            master_out: [&mut master_l[..n], &mut master_r[..n]],
        };
        if *warm {
            shared.engine.process(tick);
        } else {
            // The engine's first tick on a thread registers arc_swap's debt slot.
            permit_alloc(|| shared.engine.process(tick));
            *warm = true;
        }
        shared.staging.publish(
            stamp,
            n,
            [
                &monitor_l[..n],
                &monitor_r[..n],
                &master_l[..n],
                &master_r[..n],
            ],
        );

        counters.ticks.fetch_add(1, Ordering::Relaxed);
        counters.quantum.store(n as u32, Ordering::Relaxed);
        if rate > 0 {
            counters.rate.store(rate, Ordering::Relaxed);
        }
        self.last_stamp = stamp;
        self.last_frames = n;
    }

    /// What the tick computed for `role`'s output.
    fn copy_out(&self, role: Role, left: &mut [f32], right: &mut [f32]) {
        let [l, r] = if role == Role::Monitor {
            &self.monitor
        } else {
            &self.master
        };
        let n = left.len();
        left.copy_from_slice(&l[..n]);
        right.copy_from_slice(&r[..n]);
    }
}

impl Tap {
    /// The latency governor: the ring should hold exactly one quantum ahead of this tick (two
    /// when the input already delivered this cycle, one when it delivers after the tick), so
    /// the tick always reads the frames from one cycle ago whichever runs first. More is
    /// discarded, oldest first; less is silence until the ring holds one quantum ahead again.
    fn govern(&mut self, n: usize, stamp: u64, shared: &Shared) {
        let counters = &shared.counters;
        let delivered = stamp != 0 && self.channel.last_write.load(Ordering::Acquire) == stamp;
        let ahead = if delivered { 2 * n } else { n };
        let streaming = self.channel.streaming.load(Ordering::Relaxed);
        let was_primed = self.primed;
        let mut held = self.consumer.slots();
        let on_target = held == ahead;
        if held > ahead {
            if let Ok(chunk) = self.consumer.read_chunk(held - ahead) {
                chunk.commit_all();
            }
            held = ahead;
        }
        if held < ahead {
            if was_primed && streaming {
                counters.underruns.fetch_add(1, Ordering::Relaxed);
                shared.xrun();
            }
            self.primed = false;
            self.left[..n].fill(0.0);
            self.right[..n].fill(0.0);
        } else {
            if let Ok(chunk) = self.consumer.read_chunk(n) {
                let (first, second) = chunk.as_slices();
                for (i, [l, r]) in first.iter().chain(second).enumerate() {
                    self.left[i] = *l;
                    self.right[i] = *r;
                }
                chunk.commit_all();
            }
            self.primed = true;
        }
        if was_primed && streaming {
            counters.ring_checks.fetch_add(1, Ordering::Relaxed);
            if !on_target {
                counters.ring_off.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// The cycle's stamp and the graph's rate, from the stream's time: `clock.nsec` of the
/// driver's position, the same for every node in one cycle.
fn cycle(stream: &Stream) -> (u64, u32) {
    match stream.time() {
        Ok(time) => (time.now().max(0) as u64, time.rate().denom),
        Err(_) => (0, 0),
    }
}

/// An input plane's samples: the chunk's valid bytes as `f32`, empty when unmapped or
/// misaligned.
fn in_plane(data: &mut Data) -> &[f32] {
    let (offset, size) = {
        let chunk = data.chunk();
        (chunk.offset() as usize, chunk.size() as usize)
    };
    let Some(bytes) = data.data() else {
        return &[];
    };
    let start = offset.min(bytes.len());
    let end = start.saturating_add(size).min(bytes.len());
    // SAFETY: every bit pattern is a valid f32, and `align_to` only reinterprets the aligned
    // middle of the slice.
    let (head, samples, _) = unsafe { bytes[start..end].align_to::<f32>() };
    if head.is_empty() { samples } else { &[] }
}

/// An output plane's first `n` samples to fill, or fewer when the buffer is smaller.
fn out_plane(data: &mut Data, n: usize) -> &mut [f32] {
    let Some(bytes) = data.data() else {
        return &mut [];
    };
    // SAFETY: as in `in_plane`.
    let (head, samples, _) = unsafe { bytes.align_to_mut::<f32>() };
    if head.is_empty() {
        let n = n.min(samples.len());
        &mut samples[..n]
    } else {
        &mut []
    }
}

fn finish(data: &mut Data, frames: usize) {
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = std::mem::size_of::<f32>() as i32;
    *chunk.size_mut() = (frames * std::mem::size_of::<f32>()) as u32;
}

/// An input stream's `process`: this cycle's planes into the ring, stamped with the cycle.
pub(crate) fn input_process(stream: &Stream, rt: &mut InputRt) {
    assert_no_alloc(|| {
        let (stamp, _) = cycle(stream);
        let Some(mut buffer) = stream.dequeue_buffer() else {
            return;
        };
        let datas = buffer.datas_mut();
        if datas.len() < 2 {
            return;
        }
        let (first, rest) = datas.split_at_mut(1);
        let left = in_plane(&mut first[0]);
        let right = in_plane(&mut rest[0]);
        let n = left.len().min(right.len());
        let free = rt.producer.slots();
        if free < n {
            rt.shared.counters.overflows.fetch_add(1, Ordering::Relaxed);
        }
        if let Ok(chunk) = rt.producer.write_chunk_uninit(n.min(free)) {
            chunk.fill_from_iter(left.iter().zip(right).map(|(l, r)| [*l, *r]));
        }
        rt.channel.last_write.store(stamp, Ordering::Release);
    });
}

/// An output stream's `process` user data.
pub(crate) struct OutputRt {
    pub role: Role,
    pub shared: Arc<Shared>,
}

/// An output stream's `process`: the tick when its role is elected (then it triggers the
/// others), otherwise a copy of the mix the tick staged earlier in this cycle.
pub(crate) fn output_process(stream: &Stream, rt: &mut OutputRt) {
    assert_no_alloc(|| {
        let shared = &*rt.shared;
        let role = rt.role;
        let (stamp, rate) = cycle(stream);
        let Some(mut buffer) = stream.dequeue_buffer() else {
            return;
        };
        let requested = match buffer.requested() as usize {
            0 => shared.counters.quantum.load(Ordering::Relaxed) as usize,
            n => n,
        }
        .clamp(1, MAX_QUANTUM);
        let datas = buffer.datas_mut();
        if datas.len() < 2 {
            return;
        }
        let (first, rest) = datas.split_at_mut(1);
        let capacity = |data: &Data| data.as_raw().maxsize as usize / std::mem::size_of::<f32>();
        let n = requested.min(capacity(&first[0])).min(capacity(&rest[0]));
        {
            let left = out_plane(&mut first[0], n);
            let right = out_plane(&mut rest[0], n);
            if left.len() != n || right.len() != n {
                left.fill(0.0);
                right.fill(0.0);
            } else if shared.elected.load(Ordering::Relaxed) == role as u8 {
                let ticked = shared.tick.claim(|state| {
                    state.run(shared, n, stamp, rate);
                    state.copy_out(role, left, right);
                });
                if ticked.is_some() {
                    shared.counters.cycles[role as usize].fetch_add(1, Ordering::Relaxed);
                } else {
                    left.fill(0.0);
                    right.fill(0.0);
                    shared.counters.stale.fetch_add(1, Ordering::Relaxed);
                }
                shared.triggers.fire(role);
            } else if shared.staging.read(stamp, role, left, right) {
                shared.counters.cycles[role as usize].fetch_add(1, Ordering::Relaxed);
            } else {
                left.fill(0.0);
                right.fill(0.0);
                shared.counters.stale.fetch_add(1, Ordering::Relaxed);
            }
        }
        finish(&mut first[0], n);
        finish(&mut rest[0], n);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_engine::EngineConfig;

    fn shared() -> (PlanPort, Arc<Shared>) {
        Shared::new(Arc::new(Engine::new(EngineConfig::new(
            48_000,
            MAX_QUANTUM,
            2,
        ))))
    }

    /// Delivers `n` frames of `value` into the ring, stamped `stamp`, as the input's
    /// `process` would.
    fn deliver(rt: &mut InputRt, n: usize, value: f32, stamp: u64) {
        let chunk = rt.producer.write_chunk_uninit(n).unwrap();
        chunk.fill_from_iter(std::iter::repeat_n([value, value], n));
        rt.channel.last_write.store(stamp, Ordering::Release);
    }

    #[test]
    fn the_governor_reads_the_frames_from_one_cycle_ago_whichever_runs_first() {
        let (_plans, shared) = shared();
        let (mut rt, mut tap, channel) = input_ring(&shared);
        channel.streaming.store(true, Ordering::Relaxed);
        let n = 4;
        // Cycle 1, input first: priming, silence, nothing consumed.
        deliver(&mut rt, n, 1.0, 1);
        tap.govern(n, 1, &shared);
        assert_eq!(&tap.left[..n], &[0.0; 4]);
        // Cycle 2, input first: two quanta held, the tick reads cycle 1's.
        deliver(&mut rt, n, 2.0, 2);
        tap.govern(n, 2, &shared);
        assert_eq!(&tap.left[..n], &[1.0; 4]);
        // Cycle 3, the order flips: the tick runs first and reads cycle 2's.
        tap.govern(n, 3, &shared);
        assert_eq!(&tap.left[..n], &[2.0; 4]);
        deliver(&mut rt, n, 3.0, 3);
        // Cycle 4, tick first again: cycle 3's.
        tap.govern(n, 4, &shared);
        assert_eq!(&tap.left[..n], &[3.0; 4]);
        deliver(&mut rt, n, 4.0, 4);
        // Cycle 5 flips back to input first: still cycle 4's, nothing lost, nothing doubled.
        deliver(&mut rt, n, 5.0, 5);
        tap.govern(n, 5, &shared);
        assert_eq!(&tap.left[..n], &[4.0; 4]);
        let stats = shared.stats();
        assert_eq!((stats.ring_checks, stats.ring_off, stats.xruns), (3, 0, 0));
    }

    #[test]
    fn a_ring_that_ran_ahead_is_cut_to_one_quantum_and_a_short_one_is_an_underrun() {
        let (_plans, shared) = shared();
        let (mut rt, mut tap, channel) = input_ring(&shared);
        channel.streaming.store(true, Ordering::Relaxed);
        let n = 4;
        deliver(&mut rt, n, 1.0, 1);
        tap.govern(n, 2, &shared);
        assert_eq!(&tap.left[..n], &[1.0; 4]);
        // Three cycles delivered while no tick ran: only the newest before this cycle is read.
        deliver(&mut rt, n, 2.0, 2);
        deliver(&mut rt, n, 3.0, 3);
        deliver(&mut rt, n, 4.0, 4);
        tap.govern(n, 5, &shared);
        assert_eq!(&tap.left[..n], &[4.0; 4]);
        assert_eq!(shared.stats().ring_off, 1);
        // The input misses a cycle while it runs: silence, one underrun, one xrun.
        tap.govern(n, 6, &shared);
        assert_eq!(&tap.left[..n], &[0.0; 4]);
        let stats = shared.stats();
        assert_eq!((stats.underruns, stats.xruns), (1, 1));
        assert_eq!(shared.engine.stats().xruns.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn an_input_that_is_not_running_is_silence_and_never_an_underrun() {
        let (_plans, shared) = shared();
        let (_rt, mut tap, _channel) = input_ring(&shared);
        tap.primed = true;
        tap.govern(8, 1, &shared);
        assert_eq!(&tap.left[..8], &[0.0; 8]);
        let stats = shared.stats();
        assert_eq!((stats.underruns, stats.xruns, stats.ring_checks), (0, 0, 0));
    }

    #[test]
    fn a_plan_installs_taps_in_the_engine_order_and_comes_back_with_the_ones_it_replaced() {
        let (mut plans, shared) = shared();
        let (_rt, tap, _channel) = input_ring(&shared);
        let mut plan = TickPlan::new();
        plan.taps[5] = Some(tap);
        plan.replace[5] = true;
        plan.order[0] = Some(5);
        plan.count = 1;
        plans.to_tick.push(plan).unwrap();
        shared.tick.claim(|state| state.take_plans()).unwrap();
        let back = plans.from_tick.pop().unwrap();
        assert!(back.taps[5].is_none(), "the slot was empty before");
        shared
            .tick
            .claim(|state| {
                assert!(state.slots[5].is_some());
                assert_eq!((state.order[0], state.count), (Some(5), 1));
            })
            .unwrap();
    }

    #[test]
    fn staging_hands_an_output_the_mix_of_its_own_cycle_only() {
        let (_plans, shared) = shared();
        let monitor = [0.25f32; 8];
        let master = [0.5f32; 8];
        shared
            .staging
            .publish(7, 8, [&monitor, &monitor, &master, &master]);
        let (mut l, mut r) = ([0.0f32; 8], [0.0f32; 8]);
        assert!(shared.staging.read(7, Role::MasterSource, &mut l, &mut r));
        assert_eq!((l, r), (master, master));
        assert!(shared.staging.read(7, Role::Monitor, &mut l, &mut r));
        assert_eq!(l, monitor);
        assert!(!shared.staging.read(8, Role::MasterOut, &mut l, &mut r));
    }
}
