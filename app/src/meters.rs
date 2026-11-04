//! Meter levels as the strips draw them, the prototype's `setM`: the host's linear peak
//! becomes a position on the fader's own taper (so a level at 0 dB sits on the "0" of the
//! scale, beside a fader at unity), and each bar keeps a peak-hold line that holds
//! [`HOLD_MS`] and then falls [`FALL_PER_FRAME`] a frame, never below the level.
//!
//! This is window state, kept from frame to frame by whoever draws: the window's frame loop,
//! or the look gate's capture stepping `FakeHost` frame by frame.

use vixmix_host::{MeterFrame, Stereo};
use vixmix_model::{MAX_CHANNELS, MixId, taper};

/// One animation frame at 60 Hz, in milliseconds (the prototype's `requestAnimationFrame`).
pub const FRAME_MS: f64 = 1000.0 / 60.0;
/// How long a peak holds before it falls.
pub const HOLD_MS: f64 = 900.0;
/// How far a peak falls each frame once its hold is over, in fader positions.
pub const FALL_PER_FRAME: f32 = 0.006;

/// The fader position a linear peak reads at: the prototype's `dbToPos(amp2db(a))`, with
/// anything at or below 0.0001 (−80 dB) silent.
pub fn position(amplitude: f32) -> f32 {
    if amplitude.is_nan() || amplitude <= 0.0001 {
        return 0.0;
    }
    taper::db_to_position(20.0 * amplitude.log10()).clamp(0.0, 1.0)
}

/// One meter bar: where its level is, and its peak-hold line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bar {
    /// The level as a fader position, 0..=1.
    pub level: f32,
    /// The peak-hold line as a fader position, 0..=1.
    pub peak: f32,
    held_since_ms: f64,
}

impl Bar {
    /// The bar after a frame whose peak was `amplitude`, at `now_ms`.
    pub fn update(&mut self, amplitude: f32, now_ms: f64) {
        self.level = position(amplitude);
        if self.level >= self.peak {
            self.peak = self.level;
            self.held_since_ms = now_ms;
        } else if now_ms - self.held_since_ms > HOLD_MS {
            self.peak = self.level.max(self.peak - FALL_PER_FRAME);
        }
    }
}

/// A fader's two bars, left and right.
pub type Pair = [Bar; 2];

fn update_pair(pair: &mut Pair, levels: Stereo, now_ms: f64) {
    pair[0].update(levels.left, now_ms);
    pair[1].update(levels.right, now_ms);
}

/// Every bar the console draws, by channel position as [`MeterFrame`] has them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meters {
    /// Each channel's sends: Monitor, then Master.
    pub channels: [[Pair; 2]; MAX_CHANNELS],
    pub monitor: Pair,
    pub master: Pair,
}

impl Meters {
    /// Takes one frame of the host's meters, read at `now_ms`.
    pub fn update(&mut self, frame: &MeterFrame, now_ms: f64) {
        for (bars, levels) in self.channels.iter_mut().zip(&frame.channels) {
            update_pair(&mut bars[0], levels.monitor, now_ms);
            update_pair(&mut bars[1], levels.master, now_ms);
        }
        update_pair(&mut self.monitor, frame.monitor, now_ms);
        update_pair(&mut self.master, frame.master, now_ms);
    }

    /// The bars of channel `index`'s send to `mix`.
    pub fn send(&self, index: usize, mix: MixId) -> &Pair {
        &self.channels[index][mix_index(mix)]
    }

    /// The bars of `mix`'s own fader.
    pub fn mix(&self, mix: MixId) -> &Pair {
        match mix {
            MixId::Monitor => &self.monitor,
            MixId::Master => &self.master,
        }
    }
}

fn mix_index(mix: MixId) -> usize {
    match mix {
        MixId::Monitor => 0,
        MixId::Master => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_host::ChannelLevels;

    #[test]
    fn a_level_reads_on_the_faders_taper() {
        assert_eq!(position(1.0), taper::UNITY, "0 dB sits on the scale's 0");
        assert_eq!(position(0.0), 0.0);
        assert_eq!(position(0.0001), 0.0, "-80 dB and below is silence");
        assert_eq!(position(f32::NAN), 0.0);
        assert_eq!(position(4.0), 1.0, "past +6 dB the bar is full");
        let half = position(0.5);
        assert!(
            (half - taper::db_to_position(-6.0206)).abs() < 1e-4,
            "{half}"
        );
    }

    #[test]
    fn a_peak_holds_900_ms_then_falls_a_little_each_frame_to_the_level() {
        let mut bar = Bar::default();
        bar.update(1.0, 0.0);
        assert_eq!(bar.peak, taper::UNITY);
        // The level drops; the peak holds for 900 ms.
        let mut now = 0.0;
        while now < HOLD_MS {
            now += FRAME_MS;
            bar.update(0.01, now.min(HOLD_MS));
        }
        assert_eq!(bar.peak, taper::UNITY, "held through {HOLD_MS} ms");
        // Then it falls one step a frame.
        bar.update(0.01, HOLD_MS + FRAME_MS);
        assert!((bar.peak - (taper::UNITY - FALL_PER_FRAME)).abs() < 1e-6);
        // And never below the level.
        for frame in 2..400 {
            bar.update(0.01, HOLD_MS + f64::from(frame) * FRAME_MS);
        }
        assert_eq!(bar.peak, bar.level);
        // A louder frame takes the peak at once.
        bar.update(0.5, 10_000.0);
        assert_eq!(bar.peak, bar.level);
    }

    #[test]
    fn the_console_meters_follow_the_hosts_frame_by_position() {
        let mut frame = MeterFrame::default();
        frame.channels[2] = ChannelLevels {
            monitor: Stereo {
                left: 1.0,
                right: 0.5,
            },
            master: Stereo::default(),
        };
        frame.master = Stereo {
            left: 1.0,
            right: 1.0,
        };
        let mut meters = Meters::default();
        meters.update(&frame, 0.0);
        assert_eq!(meters.send(2, MixId::Monitor)[0].level, taper::UNITY);
        assert!(meters.send(2, MixId::Monitor)[1].level < taper::UNITY);
        assert_eq!(meters.send(2, MixId::Master)[0].level, 0.0);
        assert_eq!(meters.mix(MixId::Master)[1].level, taper::UNITY);
        assert_eq!(meters.mix(MixId::Monitor)[0].level, 0.0);
    }
}
