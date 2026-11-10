//! [`LoadWindow`]: the header's DSP load, averaged over a quarter second, with the worst
//! single cycle held for a second beside it.
//!
//! The engine's last-cycle load is what a sampling reader sees, and a plugin that works in
//! blocks of its own (480 frames fed 256 at a time) makes cheap and expensive cycles in turn,
//! so the header read one or the other (issue 25). The window instead takes the engine's
//! cumulative busy time and frames at each poll, and once a window has run divides what each
//! moved: the busy time over the audio those frames make at the rate. Every cycle counts once,
//! and one still running at a reading lands in the next window, so nothing is lost to
//! sampling. The worst cycle, which the engine keeps until it is taken, is held apart so the
//! average never hides a cycle that came near its deadline.
//!
//! It keeps no clock: [`RealHost`](crate::RealHost)'s poll hands it the counters and the
//! time, and a test hands it made-up ones.

use std::time::{Duration, Instant};

use vixmix_engine::{DspCounters, Stats};

/// How long one window of the average runs.
pub const WINDOW: Duration = Duration::from_millis(250);

/// How long the worst cycle shows before it falls to the newest window's.
pub const PEAK_HOLD: Duration = Duration::from_secs(1);

/// The engine's counters at one poll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadReading {
    /// Frames the tick processed since the engine was made (the kit's `Stats::frames`).
    pub frames: u64,
    /// The rate they run at (`Stats::rate`); zero when none is known.
    pub rate: u32,
    /// Processing time since the engine was made, in nanoseconds (`DspCounters::busy_ns`).
    pub busy_ns: u64,
    /// The worst single cycle's load since the last reading, in permille of its budget,
    /// taken from the engine (`DspCounters::take_peak`).
    pub peak_permille: u32,
}

impl LoadReading {
    /// Reads the engine's counters, taking its worst cycle: what each poll does.
    pub fn take(stats: &Stats, dsp: &DspCounters) -> LoadReading {
        LoadReading {
            busy_ns: dsp.busy_ns(),
            frames: stats.frames(),
            rate: stats.rate(),
            peak_permille: dsp.take_peak(),
        }
    }
}

/// The windowed load and the held peak; see the module documentation.
#[derive(Debug, Default)]
pub struct LoadWindow {
    /// When the running window began, and the counters then; `None` before the first reading.
    began: Option<(Instant, LoadReading)>,
    /// The worst cycle in the running window, in permille.
    window_peak: u32,
    /// The last whole window's load: busy time over budget.
    load: f32,
    /// The peak shown, and when it was taken.
    peak: f32,
    peak_at: Option<Instant>,
}

impl LoadWindow {
    /// Takes one reading at `now`. Once a [`WINDOW`] has run since the last one ended, the
    /// load becomes that window's and its worst cycle takes the peak if it is as high, or if
    /// the peak has been held for [`PEAK_HOLD`]. A window in which no frame ran (the engine
    /// idle) reads zero. A new rate begins the window again (its frames so far ran at the
    /// old rate), as do counters that went back; what shows stays until a window ends.
    pub fn feed(&mut self, now: Instant, reading: LoadReading) {
        self.window_peak = self.window_peak.max(reading.peak_permille);
        let Some((began, base)) = self.began else {
            self.began = Some((now, reading));
            return;
        };
        if reading.rate != base.rate
            || reading.frames < base.frames
            || reading.busy_ns < base.busy_ns
        {
            self.began = Some((now, reading));
            return;
        }
        if now.saturating_duration_since(began) < WINDOW {
            return;
        }
        let frames = reading.frames - base.frames;
        let busy = reading.busy_ns - base.busy_ns;
        self.load = if frames == 0 || reading.rate == 0 {
            0.0
        } else {
            let budget_ns = frames as f64 * 1e9 / f64::from(reading.rate);
            (busy as f64 / budget_ns) as f32
        };
        let peak = self.window_peak as f32 / 1000.0;
        self.window_peak = 0;
        let held = self
            .peak_at
            .is_some_and(|at| now.saturating_duration_since(at) < PEAK_HOLD);
        if peak >= self.peak || !held {
            self.peak = peak;
            self.peak_at = Some(now);
        }
        self.began = Some((now, reading));
    }

    /// The last whole window's load as a fraction of its budget (1 is the whole budget); zero
    /// until a window has run.
    pub fn load(&self) -> f32 {
        self.load
    }

    /// The worst single cycle's load as a fraction of its budget, held: not capped, so a late
    /// cycle reads above 1.
    pub fn peak(&self) -> f32 {
        self.peak
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    const SECOND: u64 = 1_000_000_000;
    const RATE: u32 = 48_000;
    /// PipeWire's quantum in the issue.
    const BLOCK: usize = 256;
    /// What the plugin costs on a cycle that only buffers, and for each 480-frame frame it
    /// runs: about 0.4 % and 12 % of a 256-frame cycle at 48 kHz, the issue's two readings.
    const CHEAP: Duration = Duration::from_micros(20);
    const FRAME_COST: Duration = Duration::from_micros(640);
    /// How often the window polls the host while its meters move.
    const POLL_NS: u64 = SECOND / 60;
    /// A window, in the nanoseconds the rig counts in.
    const WINDOW_NS: u64 = WINDOW.as_nanos() as u64;
    const HOLD_NS: u64 = PEAK_HOLD.as_nanos() as u64;

    /// A plugin that works in 480-frame frames, fed blocks of another size: it buffers what
    /// it is given and runs a frame whenever 480 are waiting.
    #[derive(Default)]
    struct Blocky {
        waiting: usize,
    }

    impl Blocky {
        fn cycle(&mut self, frames: usize) -> Duration {
            self.waiting += frames;
            let mut busy = CHEAP;
            while self.waiting >= 480 {
                self.waiting -= 480;
                busy += FRAME_COST;
            }
            busy
        }
    }

    /// The engine's side: its counters written as the tick writes them, from made-up cycle
    /// times, and the made-up clock both run on.
    struct Rig {
        stats: Stats,
        dsp: DspCounters,
        window: LoadWindow,
        start: Instant,
        /// Nanoseconds since `start`, of the next cycle and the next poll.
        next_cycle: u64,
        next_poll: u64,
        /// What the header showed at each poll: the time, the load, the peak, and the last
        /// cycle's load as it showed before (the kit's `dsp_load_permille`).
        shown: Vec<(u64, f32, f32, u32)>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig {
                stats: Stats::at_rate(RATE),
                dsp: DspCounters::default(),
                window: LoadWindow::default(),
                start: Instant::now(),
                next_cycle: 0,
                next_poll: 0,
                shown: Vec::new(),
            }
        }

        /// Runs until `until_ns`: a cycle of `frames` costing what `busy` says every quantum
        /// (none at all when `busy` is `None`: the engine idle), and a poll every 60th of a
        /// second.
        fn run(
            &mut self,
            until_ns: u64,
            frames: usize,
            mut cost: Option<&mut dyn FnMut() -> Duration>,
        ) {
            loop {
                if let Some(cost) = cost.as_mut()
                    && self.next_cycle <= self.next_poll
                {
                    if self.next_cycle >= until_ns {
                        return;
                    }
                    let rate = self.stats.rate();
                    let took = cost();
                    self.stats.record_cycle(frames, rate, took);
                    self.dsp.record(frames, rate, took);
                    self.next_cycle += frames as u64 * SECOND / u64::from(rate);
                    continue;
                }
                let at = self.next_poll;
                if at >= until_ns {
                    // An idle engine's next cycle comes no sooner than it starts again.
                    self.next_cycle = self.next_cycle.max(until_ns);
                    return;
                }
                let now = self.start + Duration::from_nanos(at);
                self.window
                    .feed(now, LoadReading::take(&self.stats, &self.dsp));
                let last = self.stats.dsp_load_permille.load(Ordering::Relaxed);
                self.shown
                    .push((at, self.window.load(), self.window.peak(), last));
                self.next_poll += POLL_NS;
            }
        }

        /// What showed from `from_ns` on, before `to_ns`.
        fn between(&self, from_ns: u64, to_ns: u64) -> Vec<(u64, f32, f32, u32)> {
            self.shown
                .iter()
                .copied()
                .filter(|(at, ..)| (from_ns..to_ns).contains(at))
                .collect()
        }
    }

    /// The load the plugin makes over time at `rate`: a cycle's cheap part and its frames'
    /// share (256 of every 480 frames), over the cycle's budget.
    fn blocky_load(rate: u32) -> f32 {
        let budget = BLOCK as f64 / f64::from(rate);
        let busy = CHEAP.as_secs_f64() + FRAME_COST.as_secs_f64() * BLOCK as f64 / 480.0;
        (busy / budget) as f32
    }

    /// One cycle's load at `rate`, rounded up to permille as the engine keeps it.
    fn cycle_load(busy: Duration, rate: u32) -> f32 {
        let budget = BLOCK as f64 / f64::from(rate);
        (busy.as_secs_f64() / budget * 1000.0).ceil() as f32 / 1000.0
    }

    #[test]
    fn nothing_shows_until_a_window_has_run() {
        let mut rig = Rig::new();
        let mut plugin = Blocky::default();
        rig.run(WINDOW_NS - 1, BLOCK, Some(&mut || plugin.cycle(BLOCK)));
        assert!(rig.shown.len() > 10, "{} polls", rig.shown.len());
        for (at, load, peak, _) in &rig.shown {
            assert_eq!((*load, *peak), (0.0, 0.0), "at {at} ns");
        }
    }

    #[test]
    fn a_480_frame_plugin_fed_256_frame_blocks_reads_its_average_and_its_worst_cycle() {
        let mut rig = Rig::new();
        let mut plugin = Blocky::default();
        rig.run(4 * SECOND, BLOCK, Some(&mut || plugin.cycle(BLOCK)));

        // What the header showed before: the last cycle, sampled. It toggles between the
        // cheap cycle (under 1 %) and the expensive one (about 12 %).
        let after_first = rig.between(WINDOW_NS + POLL_NS, 4 * SECOND);
        let last: Vec<u32> = after_first.iter().map(|s| s.3).collect();
        assert!(
            last.iter().any(|p| *p < 10),
            "a cheap cycle sampled: {last:?}"
        );
        assert!(last.iter().any(|p| *p > 120), "an expensive one: {last:?}");

        // What it shows now: the average, window after window within half a point of what
        // the plugin costs over time, so the whole percent shown stays put.
        let expected = blocky_load(RATE);
        assert!((0.06..0.075).contains(&expected), "{expected}");
        let loads: Vec<f32> = after_first.iter().map(|s| s.1).collect();
        for load in &loads {
            assert!(
                (load - expected).abs() < 0.005,
                "{load} against {expected}: {loads:?}"
            );
        }
        let lowest = loads.iter().copied().fold(f32::INFINITY, f32::min);
        let highest = loads.iter().copied().fold(0.0, f32::max);
        assert!(highest - lowest < 0.006, "{lowest}..{highest}");

        // And the expensive cycle is still there to see, beside it.
        let worst = cycle_load(CHEAP + FRAME_COST, RATE);
        assert!((0.12..0.13).contains(&worst), "{worst}");
        for (at, load, peak, _) in &after_first {
            assert_eq!(*peak, worst, "at {at} ns");
            assert!(peak > load);
        }
    }

    #[test]
    fn an_idle_engine_reads_zero_and_its_peak_falls_once_held() {
        let mut rig = Rig::new();
        let mut plugin = Blocky::default();
        rig.run(SECOND, BLOCK, Some(&mut || plugin.cycle(BLOCK)));
        // The stream stops: no cycle, no frame, while the window keeps polling.
        rig.run(4 * SECOND, BLOCK, None);

        let worst = cycle_load(CHEAP + FRAME_COST, RATE);
        // The last window with the plugin in it ends within a window and a poll of the stop,
        // and the peak holds a second from there.
        let within = WINDOW_NS + POLL_NS;
        for (at, _, peak, _) in rig.between(SECOND, 2 * SECOND) {
            assert_eq!(peak, worst, "held at {at} ns");
        }
        for (at, load, _, _) in rig.between(SECOND + within + WINDOW_NS, 4 * SECOND) {
            assert_eq!(load, 0.0, "idle at {at} ns");
        }
        let after = rig.between(SECOND + within + HOLD_NS + within, 4 * SECOND);
        assert!(!after.is_empty());
        for (at, load, peak, _) in after {
            assert_eq!((load, peak), (0.0, 0.0), "fallen at {at} ns");
        }
    }

    #[test]
    fn a_new_rate_begins_the_window_again_and_reads_the_new_load() {
        let mut rig = Rig::new();
        let mut plugin = Blocky::default();
        rig.run(SECOND, BLOCK, Some(&mut || plugin.cycle(BLOCK)));
        // The daemon comes back at 96 kHz: the engine's rate moves (`set_rate`), and the
        // plugin plays as a pass-through until it is set up again, so a cycle costs only the
        // cheap part, of a budget half as long.
        rig.stats.rate.store(96_000, Ordering::Relaxed);
        rig.run(3 * SECOND, BLOCK, Some(&mut || CHEAP));

        let before = blocky_load(RATE);
        // 20 microseconds of a 256-frame cycle at 96 kHz (2.67 ms): 0.75 %.
        let after = (CHEAP.as_secs_f64() / (BLOCK as f64 / 96_000.0)) as f32;
        assert!((after - 0.0075).abs() < 1e-6, "{after}");
        // Each reading is one rate's or the other's: no window mixes frames of both.
        for (at, load, peak, _) in rig.between(WINDOW_NS + POLL_NS, 3 * SECOND) {
            assert!(load.is_finite() && peak.is_finite());
            assert!(
                (load - before).abs() < 0.005 || (load - after).abs() < 0.0005,
                "{load} at {at} ns, against {before} or {after}"
            );
        }
        // Within a poll of the change the window begins again; a window on, it reads the new
        // rate's load.
        for (at, load, _, _) in rig.between(SECOND + 2 * (WINDOW_NS + POLL_NS), 3 * SECOND) {
            assert!((load - after).abs() < 0.0005, "{load} at {at} ns");
        }
        // The old rate's worst cycle, taken at the change, ends with that window, holds its
        // second, and falls to the pass-through's at the next window's end.
        let fallen = rig.between(
            SECOND + POLL_NS + 2 * (WINDOW_NS + POLL_NS) + HOLD_NS,
            3 * SECOND,
        );
        assert!(!fallen.is_empty());
        for (at, _, peak, _) in fallen {
            assert_eq!(peak, cycle_load(CHEAP, 96_000), "at {at} ns");
        }
    }

    #[test]
    fn counters_that_do_not_move_and_no_rate_read_zero_without_dividing_by_it() {
        let start = Instant::now();
        let at = |poll: u64| start + Duration::from_nanos(poll * POLL_NS);
        let still = LoadReading {
            frames: 48_000,
            rate: RATE,
            busy_ns: 1_000_000,
            peak_permille: 0,
        };
        let mut window = LoadWindow::default();
        for poll in 0..120 {
            window.feed(at(poll), still);
            assert_eq!((window.load(), window.peak()), (0.0, 0.0), "poll {poll}");
        }
        // Frames moving with no rate known: no budget to divide by.
        let mut window = LoadWindow::default();
        for poll in 0..120 {
            let reading = LoadReading {
                frames: poll * 800,
                rate: 0,
                busy_ns: poll * 50_000,
                peak_permille: 0,
            };
            window.feed(at(poll), reading);
            assert_eq!(window.load(), 0.0, "poll {poll}");
        }
        // Counters that went back (another engine's): the window begins again from them.
        let mut window = LoadWindow::default();
        window.feed(start, still);
        let back = LoadReading {
            frames: 10,
            busy_ns: 10,
            ..still
        };
        window.feed(start + 2 * WINDOW, back);
        window.feed(start + 4 * WINDOW, back);
        assert_eq!((window.load(), window.peak()), (0.0, 0.0));
    }

    #[test]
    fn a_late_window_and_a_late_cycle_read_above_the_whole_budget() {
        let start = Instant::now();
        let mut window = LoadWindow::default();
        let begin = LoadReading {
            rate: RATE,
            ..LoadReading::default()
        };
        window.feed(start, begin);
        // A quarter second at 48 kHz, 12 000 frames, that took 300 ms: 120 %, with one cycle
        // in it at 150 %.
        window.feed(
            start + WINDOW,
            LoadReading {
                frames: 12_000,
                rate: RATE,
                busy_ns: 300_000_000,
                peak_permille: 1_500,
            },
        );
        assert!((window.load() - 1.2).abs() < 1e-6, "{}", window.load());
        assert_eq!(window.peak(), 1.5);
    }
}
