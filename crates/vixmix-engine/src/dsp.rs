//! [`DspCounters`]: what the tick keeps beside the kit's [`Stats`](crate::Stats) so a reader
//! can average the DSP load over a window of its own and still see the worst cycle in it.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Two relaxed atomics the tick writes once a cycle and a reader polls.
///
/// The kit's `Stats::dsp_load_permille` is the last cycle's load, which is all a reader that
/// samples it can see: a plugin that works in blocks of its own (480 frames fed 256 at a time)
/// makes cheap and expensive cycles in turn, and a window sampling them reads one or the other.
/// With these, a reader takes the busy time and the kit's cumulative `Stats::frames` at two
/// moments and divides the differences (busy over frames ÷ rate): the load over everything that
/// ran between them, every cycle counted once and none lost to sampling. The worst cycle is
/// kept apart, so an average never hides a cycle that came near its deadline.
#[derive(Debug, Default)]
pub struct DspCounters {
    /// Processing time since the engine was made, in nanoseconds: each tick adds its own.
    pub busy_ns: AtomicU64,
    /// The highest single-cycle load since the reader last took it, in thousandths of that
    /// cycle's budget (frames ÷ rate), rounded up as the kit's `dsp_load_permille` is. Not
    /// capped at 1000: a late cycle reads above it.
    pub peak_permille: AtomicU32,
}

impl DspCounters {
    /// Processing time since the engine was made, in nanoseconds.
    pub fn busy_ns(&self) -> u64 {
        self.busy_ns.load(Ordering::Relaxed)
    }

    /// The worst cycle's load since the last call, in permille, and zero in its place: a cycle
    /// between two reads is never lost. Zero when no cycle ran.
    pub fn take_peak(&self) -> u32 {
        self.peak_permille.swap(0, Ordering::Relaxed)
    }

    /// One cycle of `frames` frames at `sample_rate` that took `busy`: adds the busy time and,
    /// for a cycle with frames at a known rate, keeps its load if it is the worst since the
    /// last [`take_peak`](DspCounters::take_peak). Allocation-free and lock-free: the tick
    /// calls it beside `Stats::record_cycle`.
    pub fn record(&self, frames: usize, sample_rate: u32, busy: Duration) {
        let nanos = u64::try_from(busy.as_nanos()).unwrap_or(u64::MAX);
        self.busy_ns.fetch_add(nanos, Ordering::Relaxed);
        if frames > 0 && sample_rate > 0 {
            let budget = frames as f64 / f64::from(sample_rate);
            // Rounded up, as the kit's: a cycle that took any time at all reads at least 1.
            let load = (busy.as_secs_f64() / budget * 1000.0).ceil();
            self.peak_permille
                .fetch_max(load.min(f64::from(u32::MAX)) as u32, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_time_adds_up_and_the_worst_cycle_is_kept_until_taken() {
        let dsp = DspCounters::default();
        // 480 frames at 48 kHz is a 10 ms budget: 2.5 ms is 250 permille, 0.1 ms is 10.
        dsp.record(480, 48_000, Duration::from_micros(2_500));
        dsp.record(480, 48_000, Duration::from_micros(100));
        assert_eq!(dsp.busy_ns(), 2_600_000);
        assert_eq!(dsp.take_peak(), 250, "the worst, not the last");
        assert_eq!(dsp.take_peak(), 0, "taken");
        // Late: 15 ms of a 10 ms budget reads above 1000.
        dsp.record(480, 48_000, Duration::from_millis(15));
        dsp.record(256, 48_000, Duration::from_nanos(1));
        assert_eq!(dsp.take_peak(), 1_500);
        // An empty cycle, or one at no rate, adds its time and no load.
        dsp.record(0, 48_000, Duration::from_secs(1));
        dsp.record(64, 0, Duration::from_secs(1));
        assert_eq!(dsp.take_peak(), 0);
        assert_eq!(dsp.busy_ns(), 2_600_000 + 15_000_000 + 1 + 2_000_000_000);
    }
}
