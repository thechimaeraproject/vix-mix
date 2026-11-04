//! What the tick reports to the UI: per-send and per-mix peaks, and counters. Everything is
//! an atomic written with `Relaxed` ordering: the tick never waits for a reader.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use vixmix_model::MAX_CHANNELS;

/// The largest absolute sample since the reader last took it. Stored as `f32` bits: for
/// non-negative floats the bit patterns order as the values do, so `fetch_max` on the bits is
/// a max on the samples, and a peak between two UI frames is never lost.
#[derive(Debug, Default)]
pub struct PeakMeter(AtomicU32);

impl PeakMeter {
    /// Tick side: raises the held peak to `amplitude` if it is louder. NaN and negatives
    /// count as silence.
    pub fn record(&self, amplitude: f32) {
        let amplitude = if amplitude >= 0.0 { amplitude } else { 0.0 };
        self.0.fetch_max(amplitude.to_bits(), Ordering::Relaxed);
    }

    /// UI side: the peak since the last take, resetting it. Peak hold and fall-off are the
    /// reader's (the prototype holds 900 ms, then falls 0.006 of the fader a frame).
    pub fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Ordering::Relaxed))
    }
}

/// A left and right [`PeakMeter`].
#[derive(Debug, Default)]
pub struct StereoMeter {
    pub left: PeakMeter,
    pub right: PeakMeter,
}

impl StereoMeter {
    /// Tick side: records a block's left and right peaks.
    pub fn record(&self, left: f32, right: f32) {
        self.left.record(left);
        self.right.record(right);
    }

    /// UI side: both peaks since the last take, linear amplitude.
    pub fn take(&self) -> (f32, f32) {
        (self.left.take(), self.right.take())
    }
}

/// One channel's two sends, post-send-gain.
#[derive(Debug, Default)]
pub struct ChannelMeters {
    pub monitor: StereoMeter,
    pub master: StereoMeter,
}

/// Every meter the console draws: eight channels' sends (by position in the snapshot) and
/// both mixes post-level. A fixed array, so the tick never indexes past it.
#[derive(Debug, Default)]
pub struct MeterBank {
    channels: [ChannelMeters; MAX_CHANNELS],
    monitor: StereoMeter,
    master: StereoMeter,
}

impl MeterBank {
    /// The meters of the channel at `index` in the snapshot; past the last channel, the last
    /// channel's (the model never holds more than [`MAX_CHANNELS`]).
    pub fn channel(&self, index: usize) -> &ChannelMeters {
        &self.channels[index.min(MAX_CHANNELS - 1)]
    }

    /// The Monitor output's meter (the Master mix while the ear button is on).
    pub fn monitor(&self) -> &StereoMeter {
        &self.monitor
    }

    /// The Master mix's meter.
    pub fn master(&self) -> &StereoMeter {
        &self.master
    }
}

/// The counters the header's display reads.
#[derive(Debug, Default)]
pub struct Stats {
    /// Cycles the backend reported late or short.
    pub xruns: AtomicU64,
    /// Frames processed since the engine was made.
    pub frames: AtomicU64,
    /// The last tick's processing time as thousandths of its budget (frames ÷ rate).
    pub dsp_load_permille: AtomicU32,
    /// Frames per second.
    pub rate: AtomicU32,
    /// The last tick's frame count: PipeWire's quantum.
    pub quantum: AtomicU32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peak_between_two_reads_is_never_lost_and_a_read_resets_it() {
        let meter = PeakMeter::default();
        meter.record(0.25);
        meter.record(0.75);
        meter.record(0.5);
        meter.record(f32::NAN);
        meter.record(-1.0);
        assert_eq!(meter.take(), 0.75);
        assert_eq!(meter.take(), 0.0);
    }
}
