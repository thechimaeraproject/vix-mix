//! [`Engine`]: the prepared graph, the handoff, and the tick.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use arc_swap::ArcSwap;
use chimaeraproject_audio_processor::{EngineConfig, apply_gain, mix_into};
use vixmix_model::{EngineSnapshot, MAX_CHANNELS, SendSnap};

use crate::lender::Lender;
use crate::meters::{MeterBank, Stats};

/// PipeWire's default `default.clock.quantum-limit`: the most frames one cycle can ask for.
/// The quantum is the graph's, not ours, so buffers are sized for this and a tick takes any
/// count up to it.
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

/// A send or level as the tick applies it: `None` contributes nothing.
fn audible_gain(send: &SendSnap) -> Option<f32> {
    send.audible().then_some(send.gain)
}

struct PreparedChannel {
    monitor: Option<f32>,
    master: Option<f32>,
}

/// One graph, built on the host thread from a snapshot and only ever read by the tick.
struct Prepared {
    generation: u64,
    /// The lender's generation this graph was prepared against (handoff 02 refills on change).
    lender_generation: u64,
    channels: Vec<PreparedChannel>,
    monitor: Option<f32>,
    master: Option<f32>,
    preview_master: bool,
}

impl Prepared {
    fn silent(generation: u64) -> Prepared {
        Prepared {
            generation,
            lender_generation: 0,
            channels: Vec::new(),
            monitor: None,
            master: None,
            preview_master: false,
        }
    }
}

/// The mixer's realtime graph; see the crate documentation for the handoff.
pub struct Engine {
    config: EngineConfig,
    prepared: ArcSwap<Prepared>,
    /// The generation the newest `prepare` swapped in.
    newest: AtomicU64,
    /// The generation the tick last ran: its acknowledgement.
    acked: AtomicU64,
    /// The graphs `prepare` replaced, kept until the acknowledgement passes them. Touched by
    /// the host thread only (`prepare`, `reap`); the tick never takes this lock.
    previous: Mutex<Vec<Arc<Prepared>>>,
    meters: MeterBank,
    stats: Stats,
}

impl Engine {
    /// An engine that runs at `config.sample_rate` and plays silence until the first
    /// [`prepare`](Engine::prepare).
    pub fn new(config: EngineConfig) -> Engine {
        let stats = Stats::default();
        stats.rate.store(config.sample_rate, Ordering::Relaxed);
        Engine {
            config,
            prepared: ArcSwap::from_pointee(Prepared::silent(0)),
            newest: AtomicU64::new(0),
            acked: AtomicU64::new(0),
            previous: Mutex::new(Vec::new()),
            meters: MeterBank::default(),
            stats,
        }
    }

    /// The rate and the largest block this engine was made for.
    pub fn config(&self) -> EngineConfig {
        self.config
    }

    /// Host thread: builds the graph `snap` describes and swaps it in for the next tick. The
    /// replaced graph is kept here until the tick acknowledges the new one ([`reap`]).
    ///
    /// [`reap`]: Engine::reap
    pub fn prepare(&self, snap: &EngineSnapshot, lender: &dyn Lender) {
        let generation = self.newest.fetch_add(1, Ordering::AcqRel) + 1;
        let prepared = Prepared {
            generation,
            lender_generation: lender.generation(),
            channels: snap
                .channels
                .iter()
                .take(MAX_CHANNELS)
                .map(|c| PreparedChannel {
                    monitor: audible_gain(&c.monitor),
                    master: audible_gain(&c.master),
                })
                .collect(),
            monitor: audible_gain(&snap.monitor.level),
            master: audible_gain(&snap.master.level),
            preview_master: snap.preview_master,
        };
        let replaced = self.prepared.swap(Arc::new(prepared));
        self.previous
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(replaced);
    }

    /// Host thread: drops every replaced graph the tick has moved past, here, and returns how
    /// many it dropped. Call it every host tick; a graph the tick may still hold is kept.
    pub fn reap(&self) -> usize {
        let acked = self.acked.load(Ordering::Acquire);
        let mut previous = self.previous.lock().unwrap_or_else(PoisonError::into_inner);
        let before = previous.len();
        previous.retain(|p| p.generation >= acked);
        before - previous.len()
    }

    /// The generation the newest `prepare` swapped in, and the one the tick last ran.
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

    /// The realtime entry, on the backend's data thread: no allocation, no lock, no log once
    /// the thread has run one tick (`tests/no_alloc.rs`).
    pub fn process(&self, tick: Tick<'_>) {
        let started = Instant::now();
        let prepared = self.prepared.load();
        self.acked.store(prepared.generation, Ordering::Release);

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

        for (index, (channel, input)) in prepared.channels.iter().zip(channel_inputs).enumerate() {
            let left = &input.left[..n.min(input.left.len())];
            let right = &input.right[..n.min(input.right.len())];
            let (peak_l, peak_r) = (peak(left), peak(right));
            let meters = self.meters.channel(index);
            if let Some(gain) = channel.monitor {
                mix_into(monitor_l, left, gain);
                mix_into(monitor_r, right, gain);
                meters.monitor.record(peak_l * gain, peak_r * gain);
            }
            if let Some(gain) = channel.master {
                mix_into(master_l, left, gain);
                mix_into(master_r, right, gain);
                meters.master.record(peak_l * gain, peak_r * gain);
            }
        }

        level(monitor_l, monitor_r, prepared.monitor);
        level(master_l, master_r, prepared.master);
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
        if n > 0 && self.config.sample_rate > 0 {
            let budget = n as f64 / f64::from(self.config.sample_rate);
            let load = started.elapsed().as_secs_f64() / budget * 1000.0;
            self.stats
                .dsp_load_permille
                .store(load.min(f64::from(u32::MAX)) as u32, Ordering::Relaxed);
        }
    }
}

fn level(left: &mut [f32], right: &mut [f32], gain: Option<f32>) {
    match gain {
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
    fn a_replaced_graph_is_dropped_on_the_host_thread_only_after_the_tick_moved_on() {
        let engine = engine();
        let setup = Setup::default();
        engine.prepare(&snapshot(&setup), &NoPlugins);
        engine.prepare(&snapshot(&setup), &NoPlugins);
        // No tick has run the newest graph yet: nothing may be dropped.
        assert_eq!(engine.reap(), 0);
        assert_eq!(engine.generations(), (2, 0));
        let mut null = NullBackend::new(Arc::clone(&engine), 32);
        null.pump(32);
        assert_eq!(engine.generations(), (2, 2));
        // The silent start and generation 1 are both behind the acknowledgement.
        assert_eq!(engine.reap(), 2);
        assert_eq!(engine.reap(), 0);
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
}
