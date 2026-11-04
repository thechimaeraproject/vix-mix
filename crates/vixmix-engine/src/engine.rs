//! [`Engine`]: the handoff between the host thread and the tick, and the tick.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering, fence};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use arc_swap::ArcSwap;
use chimaeraproject_audio_processor::{
    BusBuffer, BusLayout, EngineConfig, ProcessSetup, TransportInfo, apply_gain, mix_into,
};
use vixmix_model::{EngineSnapshot, InstanceId};

use crate::graph::{self, InsertProblem, Prepared, StripChain};
use crate::lender::Lender;
use crate::meters::{MeterBank, Stats, StereoMeter};
use crate::registry::{Registry, SetUpWith};
use crate::tick_only::TickOnly;

/// PipeWire's default `default.clock.quantum-limit`: the most frames one cycle can ask for.
/// The quantum is the graph's, not ours, so every buffer and every processor's `max_block` is
/// sized for this and a tick takes any count up to it.
pub const MAX_QUANTUM: usize = 8192;

/// One channel's input for one tick: planar left and right, `frames` long, zeros when the
/// channel's stream delivered nothing.
#[derive(Clone, Copy, Debug)]
pub struct ChannelInput<'a> {
    pub left: &'a [f32],
    pub right: &'a [f32],
}

/// One cycle's buffers, borrowed from the backend for the length of [`Engine::process`].
pub struct Tick<'a> {
    /// The frames this cycle: PipeWire's quantum now, which varies; at most [`MAX_QUANTUM`].
    pub frames: usize,
    /// Every channel's input, in the snapshot's channel order. A missing entry is silence.
    pub channel_inputs: &'a [ChannelInput<'a>],
    /// The Monitor output, planar left and right.
    pub monitor_out: [&'a mut [f32]; 2],
    /// The Master mix, planar left and right: the Master output and the Master source.
    pub master_out: [&'a mut [f32]; 2],
}

/// What only the host thread touches, under one lock the tick never takes.
struct HostSide {
    /// The rate every instance is set up at; the block is always [`MAX_QUANTUM`].
    config: EngineConfig,
    /// The last snapshot prepared: what `reap` and `set_rate` build from again.
    snapshot: Option<EngineSnapshot>,
    registry: Registry,
    /// Graphs swapped out and not yet dropped: kept until no tick can reach them.
    previous: Vec<Arc<Prepared>>,
}

impl HostSide {
    fn setup(&self) -> ProcessSetup {
        ProcessSetup::realtime(&self.config)
    }
}

/// The mixer's realtime graph; see the crate documentation for the handoff.
pub struct Engine {
    prepared: ArcSwap<Prepared>,
    /// The generation the newest graph was swapped in at (written under the host lock).
    newest: AtomicU64,
    /// The generation the tick last ran: its acknowledgement.
    acked: AtomicU64,
    /// Set for the length of a tick: what keeps ticks exclusive, and what the host's
    /// quiescence check reads (`drop_replaced`).
    ticking: AtomicBool,
    /// The tick's working bus: a channel's input copied in and run through its chain, or a
    /// mix run through its chain. One, reused strip after strip; only the tick touches it.
    scratch: TickOnly<BusBuffer>,
    host: Mutex<HostSide>,
    meters: MeterBank,
    stats: Stats,
}

// `Engine` is shared between the host thread and the data thread as `Arc<Engine>`.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<Engine>();
};

impl Engine {
    /// An engine at `config.sample_rate` that plays silence until the first
    /// [`prepare`](Engine::prepare). The engine is stereo and takes any tick up to
    /// [`MAX_QUANTUM`] frames, so `config.block` and `config.channels` are replaced by
    /// [`MAX_QUANTUM`] and 2: the quantum is PipeWire's to choose, cycle by cycle.
    pub fn new(config: EngineConfig) -> Engine {
        let config = EngineConfig::new(config.sample_rate, MAX_QUANTUM, 2);
        let registry = Registry::default();
        let stats = Stats::default();
        stats.rate.store(config.sample_rate, Ordering::Relaxed);
        Engine {
            prepared: ArcSwap::from_pointee(Prepared::build(0, &config, None, &registry)),
            newest: AtomicU64::new(0),
            acked: AtomicU64::new(0),
            ticking: AtomicBool::new(false),
            scratch: TickOnly::new(BusBuffer::new(2, MAX_QUANTUM)),
            host: Mutex::new(HostSide {
                config,
                snapshot: None,
                registry,
                previous: Vec::new(),
            }),
            meters: MeterBank::default(),
            stats,
        }
    }

    fn host(&self) -> MutexGuard<'_, HostSide> {
        self.host.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The rate the engine runs at, the largest block ([`MAX_QUANTUM`]) and its two channels.
    pub fn config(&self) -> EngineConfig {
        self.host().config
    }

    /// Host thread: builds the graph `snap` describes and swaps it in for the next tick.
    ///
    /// Every insert's instance is lent from `lender` the first time a snapshot names it, set
    /// up here, and then shared by every later graph that names it, so a graph that only
    /// moves a fader, or adds, removes, moves or bypasses an insert, swaps in without setting
    /// any plugin up again. The lender is asked again only when its generation has moved. An
    /// insert that cannot be lent or set up plays as a pass-through, reported in
    /// [`problems`](Engine::problems). The replaced graph is kept until no tick can reach it
    /// and dropped here or in [`reap`](Engine::reap), on the host thread.
    pub fn prepare(&self, snap: &EngineSnapshot, lender: &dyn Lender) {
        let mut host = self.host();
        let named: Vec<(InstanceId, &str)> = graph::inserts(snap)
            .map(|insert| (insert.instance, insert.plugin_id.as_str()))
            .collect();
        let (layout, setup) = (BusLayout::stereo_insert(), host.setup());
        let with = SetUpWith {
            layout: &layout,
            setup: &setup,
        };
        host.registry.resolve(&named, lender, &with);
        host.snapshot = Some(snap.clone());
        self.publish(&mut host);
        self.settle(&mut host);
    }

    /// Host thread: drops every replaced graph no tick can reach any more, here, and returns
    /// how many it dropped. Then finishes what a [`set_rate`](Engine::set_rate) or
    /// [`set_up_again`](Engine::set_up_again) began: an instance waiting to be set up again
    /// that no graph holds now is set up and the graph built again to include it. Call it
    /// every host tick.
    pub fn reap(&self) -> usize {
        let mut host = self.host();
        self.settle(&mut host)
    }

    /// Host thread: the graph now runs at `sample_rate` (the backend reported a new one).
    /// Every instance is set up again at that rate once no running graph holds it, playing
    /// as a pass-through until then, and the graph is built again from the last snapshot.
    /// Returns whether the rate changed; at the same rate nothing happens.
    pub fn set_rate(&self, sample_rate: u32) -> bool {
        let mut host = self.host();
        if host.config.sample_rate == sample_rate {
            return false;
        }
        host.config.sample_rate = sample_rate;
        self.stats.rate.store(sample_rate, Ordering::Relaxed);
        host.registry.reset_all();
        self.publish(&mut host);
        self.settle(&mut host);
        true
    }

    /// Host thread: `instance` asked for new buses (VST3's `kIoChanged`): it is set up again
    /// once no running graph holds it, playing as a pass-through until then. Returns whether
    /// the engine holds it.
    pub fn set_up_again(&self, instance: InstanceId) -> bool {
        let mut host = self.host();
        if !host.registry.reset(instance) {
            return false;
        }
        self.publish(&mut host);
        self.settle(&mut host);
        true
    }

    /// Every insert the running graph plays as a pass-through because it could not be lent
    /// or set up, with why, in strip order. Bypassed inserts and inserts being set up again
    /// are pass-throughs too, but not problems.
    pub fn problems(&self) -> Vec<InsertProblem> {
        self.prepared.load().problems.clone()
    }

    /// The generation the newest graph was swapped in at, and the one the tick last ran.
    pub fn generations(&self) -> (u64, u64) {
        (
            self.newest.load(Ordering::Acquire),
            self.acked.load(Ordering::Acquire),
        )
    }

    /// The lender generation the running graph was prepared against.
    pub fn lender_generation(&self) -> u64 {
        self.prepared.load().lender_generation
    }

    /// The peak meters the tick writes and the UI reads.
    pub fn meters(&self) -> &MeterBank {
        &self.meters
    }

    /// The counters the tick keeps.
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Builds a graph from the host side's snapshot and holdings and swaps it in.
    fn publish(&self, host: &mut HostSide) {
        let generation = self.newest.load(Ordering::Relaxed) + 1;
        let graph = Prepared::build(
            generation,
            &host.config,
            host.snapshot.as_ref(),
            &host.registry,
        );
        self.newest.store(generation, Ordering::Release);
        let replaced = self.prepared.swap(Arc::new(graph));
        host.previous.push(replaced);
    }

    /// Drops what no tick can reach, then sets up again what waited for that, rebuilding
    /// the graph once per round that changed something. Ends, because a round that changes
    /// nothing stops it and each other round moves at least one instance out of waiting.
    fn settle(&self, host: &mut HostSide) -> usize {
        let layout = BusLayout::stereo_insert();
        let mut dropped = 0;
        loop {
            dropped += self.drop_replaced(host);
            let setup = host.setup();
            let with = SetUpWith {
                layout: &layout,
                setup: &setup,
            };
            if !host.registry.finish_resets(&with) {
                return dropped;
            }
            self.publish(host);
        }
    }

    /// Drops the replaced graphs no tick can reach, on this (the host) thread.
    ///
    /// **Behind the acknowledgement.** A graph older than the generation the tick last
    /// stored is unreachable: that store came after the tick loaded a newer graph, and ticks
    /// are exclusive, so the tick that ran the old one has dropped its guard.
    ///
    /// **When no tick is running**, every replaced graph is unreachable, acknowledged or not:
    /// so graphs never pile up while nothing ticks (no backend, a backend between streams).
    /// The proof is the two `SeqCst` fences, this one and the one in `Ticking::take`. The
    /// swap that replaced a graph is sequenced before this fence. If the tick's fence comes
    /// later in the single order of `SeqCst` fences, that tick's `load` sees the swap and
    /// loads a newer graph; if it comes earlier, the read of `ticking` below must see that
    /// tick's `true` or a later value, so reading `false` means it has finished, its guard
    /// dropped before its `Release` store. Either way nothing holds a replaced graph.
    fn drop_replaced(&self, host: &mut HostSide) -> usize {
        let before = host.previous.len();
        if before == 0 {
            return 0;
        }
        fence(Ordering::SeqCst);
        if self.ticking.load(Ordering::Acquire) {
            let acked = self.acked.load(Ordering::Acquire);
            host.previous.retain(|graph| graph.generation >= acked);
        } else {
            host.previous.clear();
        }
        before - host.previous.len()
    }

    /// The realtime entry, on the backend's data thread: no allocation, no lock, no log once
    /// the thread has run one tick (`tests/no_alloc.rs`).
    ///
    /// Each channel's input runs through its chain; its Monitor and Master sends each take the
    /// processed or the dry signal (MON, MST), at their gains, into the two mixes; each mix
    /// runs its chain, then its level or mute; with the ear button on, Monitor's output plays
    /// the Master mix. One tick runs at a time: a call made while another thread is inside
    /// this one writes silence and returns.
    pub fn process(&self, tick: Tick<'_>) {
        let started = Instant::now();
        let Tick {
            frames,
            channel_inputs,
            monitor_out: [monitor_l, monitor_r],
            master_out: [master_l, master_r],
        } = tick;
        let n = frames
            .min(MAX_QUANTUM)
            .min(monitor_l.len())
            .min(monitor_r.len())
            .min(master_l.len())
            .min(master_r.len());
        let (monitor_l, monitor_r) = (&mut monitor_l[..n], &mut monitor_r[..n]);
        let (master_l, master_r) = (&mut master_l[..n], &mut master_r[..n]);
        for out in [
            &mut *monitor_l,
            &mut *monitor_r,
            &mut *master_l,
            &mut *master_r,
        ] {
            out.fill(0.0);
        }

        let Some(_ticking) = Ticking::take(&self.ticking) else {
            return;
        };
        // Declared after `_ticking`, so dropped before it: the guard is gone before the flag
        // says the tick is over.
        let prepared = self.prepared.load();
        self.acked.store(prepared.generation, Ordering::Release);
        // SAFETY: this is the tick and it holds `ticking`, so no other tick runs; the host
        // thread touches `scratch` only when it builds or drops the engine (`TickOnly`).
        let scratch = unsafe { &mut *self.scratch.get() };
        // SAFETY: as above, and the host thread built this graph before publishing it and
        // drops it only once no tick can reach it (`drop_replaced`).
        let chains = unsafe { &mut *prepared.chains.get() };
        let transport = TransportInfo::stopped();

        for (index, (channel, strip)) in prepared
            .channels
            .iter()
            .zip(chains.channels.iter_mut())
            .enumerate()
        {
            let dry: (&[f32], &[f32]) = match channel_inputs.get(index) {
                Some(input) => (
                    &input.left[..n.min(input.left.len())],
                    &input.right[..n.min(input.right.len())],
                ),
                None => (&[], &[]),
            };
            let processed = if strip.is_live() {
                run(strip, scratch, n, &transport, dry);
                (&scratch.channel(0)[..n], &scratch.channel(1)[..n])
            } else {
                dry
            };
            let meters = self.meters.channel(index);
            let to_monitor = if channel.chain_to_monitor {
                processed
            } else {
                dry
            };
            let to_master = if channel.chain_to_master {
                processed
            } else {
                dry
            };
            send(
                [&mut *monitor_l, &mut *monitor_r],
                to_monitor,
                channel.monitor,
                &meters.monitor,
            );
            send(
                [&mut *master_l, &mut *master_r],
                to_master,
                channel.master,
                &meters.master,
            );
        }

        mix(
            [&mut *monitor_l, &mut *monitor_r],
            &mut chains.monitor,
            prepared.monitor,
            scratch,
            &transport,
        );
        mix(
            [&mut *master_l, &mut *master_r],
            &mut chains.master,
            prepared.master,
            scratch,
            &transport,
        );
        if prepared.preview_master {
            monitor_l.copy_from_slice(master_l);
            monitor_r.copy_from_slice(master_r);
        }
        self.meters
            .monitor()
            .record(peak(monitor_l), peak(monitor_r));
        self.meters.master().record(peak(master_l), peak(master_r));

        self.stats.frames.fetch_add(n as u64, Ordering::Relaxed);
        self.stats.quantum.store(n as u32, Ordering::Relaxed);
        if n > 0 && prepared.sample_rate > 0 {
            let budget = n as f64 / f64::from(prepared.sample_rate);
            // Rounded up: a tick that took any time at all reads at least 1 ‰.
            let load = (started.elapsed().as_secs_f64() / budget * 1000.0).ceil();
            self.stats
                .dsp_load_permille
                .store(load.min(f64::from(u32::MAX)) as u32, Ordering::Relaxed);
        }
    }
}

/// `Engine::ticking`, held for one tick and released when dropped.
struct Ticking<'a>(&'a AtomicBool);

impl<'a> Ticking<'a> {
    /// Takes the flag, or `None` when another tick holds it. The fence pairs with the one in
    /// `Engine::drop_replaced`, which says why it is there.
    fn take(flag: &'a AtomicBool) -> Option<Ticking<'a>> {
        if flag.swap(true, Ordering::Acquire) {
            return None;
        }
        fence(Ordering::SeqCst);
        Some(Ticking(flag))
    }
}

impl Drop for Ticking<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Copies `input` into `bus` (zeros past its end) and runs `strip`'s chain over it in place.
fn run(
    strip: &mut StripChain,
    bus: &mut BusBuffer,
    n: usize,
    transport: &TransportInfo,
    input: (&[f32], &[f32]),
) {
    for (channel, samples) in [input.0, input.1].into_iter().enumerate() {
        let lane = &mut bus.channel_mut(channel)[..n];
        let len = samples.len().min(n);
        lane[..len].copy_from_slice(&samples[..len]);
        lane[len..].fill(0.0);
    }
    strip.chain.process(bus, n, transport, &strip.params);
}

/// Adds `source` into `out` at `gain` (`mix_into`: no multiply at exactly 1.0) and records the
/// post-send peak; a muted or silent send (`None`) adds nothing and records nothing.
fn send(out: [&mut [f32]; 2], source: (&[f32], &[f32]), gain: Option<f32>, meter: &StereoMeter) {
    let Some(gain) = gain else {
        return;
    };
    let [left, right] = out;
    mix_into(left, source.0, gain);
    mix_into(right, source.1, gain);
    meter.record(peak(source.0) * gain, peak(source.1) * gain);
}

/// A mix: its chain over the summed sends, then its level (`apply_gain`: no multiply at
/// exactly 1.0), or silence when muted.
fn mix(
    out: [&mut [f32]; 2],
    strip: &mut StripChain,
    level: Option<f32>,
    bus: &mut BusBuffer,
    transport: &TransportInfo,
) {
    let [left, right] = out;
    let n = left.len();
    if strip.is_live() {
        run(strip, bus, n, transport, (&*left, &*right));
        left.copy_from_slice(&bus.channel(0)[..n]);
        right.copy_from_slice(&bus.channel(1)[..n]);
    }
    match level {
        Some(gain) => {
            apply_gain(left, gain);
            apply_gain(right, gain);
        }
        None => {
            left.fill(0.0);
            right.fill(0.0);
        }
    }
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoPlugins, NullBackend};
    use vixmix_model::{Command, MixId, Setup, apply, snapshot, taper};

    fn engine() -> Arc<Engine> {
        Arc::new(Engine::new(EngineConfig::new(48_000, MAX_QUANTUM, 2)))
    }

    fn sine(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
            .collect()
    }

    #[test]
    fn a_channel_at_unity_reaches_monitor_bit_exact_and_a_muted_send_reaches_nothing() {
        let mut setup = Setup::default();
        let ch = setup.channels[0].id;
        let _ = apply(
            &mut setup,
            Command::SetMute {
                ch,
                mix: MixId::Master,
                mute: true,
            },
        )
        .unwrap();
        let engine = engine();
        engine.prepare(&snapshot(&setup), &NoPlugins);
        let mut null = NullBackend::new(Arc::clone(&engine), 256);
        let signal = sine(256);
        let (left, right) = null.input_mut(0).unwrap();
        left.copy_from_slice(&signal);
        right.copy_from_slice(&signal);
        null.pump(256);
        assert_eq!(null.monitor().0, &signal[..]);
        assert_eq!(null.monitor().1, &signal[..]);
        assert!(null.master().0.iter().all(|s| *s == 0.0));
        let (peak_l, _) = engine.meters().channel(0).monitor.take();
        assert!((peak_l - 0.5).abs() < 1e-3);
        assert_eq!(engine.meters().channel(0).master.take(), (0.0, 0.0));
    }

    #[test]
    fn send_and_mix_gains_follow_the_taper_and_preview_plays_master_in_monitor() {
        let mut setup = Setup::default();
        let ch = setup.channels[0].id;
        // −6 dB on the Master send (the link moves Monitor down by the same amount).
        let _ = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Master,
                position: taper::db_to_position(-6.0),
            },
        )
        .unwrap();
        let _ = apply(&mut setup, Command::SetPreview(true)).unwrap();
        let engine = engine();
        engine.prepare(&snapshot(&setup), &NoPlugins);
        let mut null = NullBackend::new(Arc::clone(&engine), 64);
        let (left, right) = null.input_mut(0).unwrap();
        left.fill(1.0);
        right.fill(1.0);
        null.pump(64);
        let half = 10f32.powf(-6.0 / 20.0);
        assert!(null.master().0.iter().all(|s| (s - half).abs() < 1e-6));
        // With the ear button on, Monitor's output is the Master mix.
        assert_eq!(null.monitor().0, null.master().0);
    }

    #[test]
    fn with_no_tick_running_a_replaced_graph_is_dropped_at_once_and_the_ack_follows_the_tick() {
        let engine = engine();
        let setup = Setup::default();
        engine.prepare(&snapshot(&setup), &NoPlugins);
        engine.prepare(&snapshot(&setup), &NoPlugins);
        // Nothing ticks, so no tick can reach a replaced graph: `prepare` dropped both (the
        // silent start and generation 1) and there is nothing left to reap.
        assert_eq!(engine.reap(), 0);
        assert_eq!(engine.generations(), (2, 0));
        let mut null = NullBackend::new(Arc::clone(&engine), 32);
        null.pump(32);
        assert_eq!(engine.generations(), (2, 2));
    }

    #[test]
    fn stats_count_frames_and_the_quantum() {
        let engine = engine();
        engine.prepare(&snapshot(&Setup::default()), &NoPlugins);
        let mut null = NullBackend::new(Arc::clone(&engine), 1024);
        null.pump(256);
        null.pump(1024);
        let stats = engine.stats();
        assert_eq!(stats.frames.load(Ordering::Relaxed), 1280);
        assert_eq!(stats.quantum.load(Ordering::Relaxed), 1024);
        assert_eq!(stats.rate.load(Ordering::Relaxed), 48_000);
    }

    #[test]
    fn the_engine_runs_at_the_quantum_limit_whatever_block_it_was_given() {
        let engine = Engine::new(EngineConfig::new(44_100, 512, 6));
        assert_eq!(engine.config(), EngineConfig::new(44_100, MAX_QUANTUM, 2));
        assert!(!engine.set_rate(44_100), "the same rate changes nothing");
        assert!(engine.set_rate(48_000));
        assert_eq!(engine.config().sample_rate, 48_000);
        assert_eq!(engine.stats().rate.load(Ordering::Relaxed), 48_000);
    }
}
