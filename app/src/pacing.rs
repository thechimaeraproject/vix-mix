//! When the next frame comes: the window's pacing as plain data and two functions, so the
//! tests pin it without a window or a clock. `app.rs` tells the [`Pacer`] what each frame
//! left and does what it answers.
//!
//! - **Visible**: a frame when egui wants one (its `repaint_delay`: a tooltip, the toast's
//!   fade, a text cursor), at once after a frame that acted, on a 60 Hz grid while the meters
//!   move or a `--quit-after-frames` count runs, and at least every [`EDITOR_POLL`] while a
//!   plugin editor is open; never sooner than [`FRAME_INTERVAL`] after the last frame's slot,
//!   so at most 60 a second. A frame's slot is when it began, or when it was due if the timer
//!   woke it, so neither drawing nor the timer's lateness stretches the grid.
//! - **Refused**: a frame the surface did not take (lost, outdated, occluded, timed out)
//!   still takes its slot, so the next waits its interval rather than spinning, and is tried
//!   again so what it held reaches the screen; after [`REFUSALS_BEFORE_BACKING_OFF`] in a row
//!   the window tries every [`HIDDEN_POLL`] instead (an X11 window iconified with no event
//!   that says so can refuse every frame).
//! - **The host's own poll** ([`Host::next_poll`](vixmix_host::Host::next_poll): PipeWire's
//!   news, the debounced save, a removed instance's undo running out): every frame serves the
//!   host, and between frames it is served when it asks, at most 60 times a second, without a
//!   frame unless it brought news (then a frame as soon as the cap allows).
//! - **Hidden** (minimised, zero-sized, fully covered: [`Visibility`]): nothing is drawn.
//!   The host and the open editors are still served, without a frame: every [`EDITOR_POLL`]
//!   while an editor is open (its own window may be in view), every [`HIDDEN_POLL`] while
//!   the meters would move (the host's bounded queues keep draining), otherwise not until an
//!   event. The prototype's `requestAnimationFrame` stops in a hidden window the same way.

use std::time::{Duration, Instant};

use crate::editors::EDITOR_POLL;

/// The shortest time between two frames: 60 Hz.
pub const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// How often the host is served while the window is hidden and its meters would move, and
/// how often a surface that keeps refusing frames is tried.
pub const HIDDEN_POLL: Duration = Duration::from_millis(250);

/// How many frames in a row the surface may refuse before the window tries it only every
/// [`HIDDEN_POLL`].
pub const REFUSALS_BEFORE_BACKING_OFF: u32 = 3;

/// How often a visible window asks the desktop again for egui's viewport facts (where it
/// is, its monitor, whether it is minimised). Its own events say most of it; an X11 window
/// iconified under a compositor says nothing, and asking every frame costs a round trip to
/// the X server for each fact.
pub const VIEWPORT_CHECK: Duration = Duration::from_millis(500);

/// What the loop waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Nothing until an event.
    Wait,
    /// A frame, drawn, at this time.
    Draw(Instant),
    /// The host and the editors served at this time, nothing drawn: the window is hidden,
    /// or the host asked to be polled before the next frame.
    Serve(Instant),
}

impl Next {
    /// When the loop wakes for it.
    pub fn at(self) -> Option<Instant> {
        match self {
            Next::Wait => None,
            Next::Draw(at) | Next::Serve(at) => Some(at),
        }
    }
}

/// What an input event asks of the loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// Draw now: the last frame's slot is an interval behind.
    Now,
    /// Draw at this time, the soonest the 60 Hz cap allows.
    At(Instant),
    /// Nothing more: a frame is due as soon, or the window is hidden.
    Nothing,
}

/// Whether the window can be seen, from what its events and the desktop said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Visibility {
    /// `WindowEvent::Occluded(true)`: fully covered (X11, macOS).
    pub occluded: bool,
    /// The last `Resized` had no area (Windows minimises a window to 0 × 0).
    pub zero_size: bool,
    /// The desktop says it is minimised (`Window::is_minimized`).
    pub minimized: bool,
}

impl Visibility {
    pub fn hidden(&self) -> bool {
        self.occluded || self.zero_size || self.minimized
    }
}

/// What a frame (or a hidden tick) leaves for the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wants {
    /// egui's own repaint delay, counted from the frame's end; `None` for none.
    pub repaint: Option<Duration>,
    /// The meters move (the backend is live) or a `--quit-after-frames` count runs.
    pub animating: bool,
    /// A plugin editor is open: the host serves it only when asked.
    pub editors: bool,
    /// The frame acted: what it changed shows on the next.
    pub acted: bool,
    /// When the host wants its next poll, counted from the frame's end; `None` for never.
    pub poll: Option<Duration>,
}

/// The pacing's memory between frames.
#[derive(Clone, Debug)]
pub struct Pacer {
    /// The last frame's slot: when it began, or when it was due if the timer woke it.
    slot: Instant,
    /// When the next frame is due, if one is.
    draw: Option<Instant>,
    /// When the host is next served without a frame, if it is.
    serve: Option<Instant>,
    /// Frames in a row the surface refused.
    refused: u32,
    /// Whether the window can be seen.
    pub seen: Visibility,
}

impl Pacer {
    /// A pacer whose first frame may come at once.
    pub fn new(now: Instant) -> Pacer {
        Pacer {
            slot: now.checked_sub(FRAME_INTERVAL).unwrap_or(now),
            draw: None,
            serve: None,
            refused: 0,
            seen: Visibility::default(),
        }
    }

    /// What the loop waits for now: the sooner of the next frame and the next serve, the
    /// frame when they tie (a frame serves the host too).
    pub fn next(&self) -> Next {
        match (self.draw, self.serve) {
            (Some(draw), Some(serve)) if serve < draw => Next::Serve(serve),
            (Some(draw), _) => Next::Draw(draw),
            (None, Some(serve)) => Next::Serve(serve),
            (None, None) => Next::Wait,
        }
    }

    /// The slot of a frame or a hidden tick that starts at `started`: when it was due if the
    /// timer woke it within an interval of that, else when it started.
    pub fn begin(&mut self, started: Instant) -> Instant {
        let due = [self.draw, self.serve]
            .into_iter()
            .flatten()
            .filter(|due| started >= *due && started - *due < FRAME_INTERVAL)
            .max();
        self.draw = None;
        self.serve = None;
        due.unwrap_or(started)
    }

    /// After a frame or a hidden tick that took `slot` and ended at `now`: `drawn` says
    /// whether the surface took the frame (`None` for a hidden tick, which draws nothing).
    /// Answers what the loop waits for next.
    pub fn end(&mut self, slot: Instant, now: Instant, drawn: Option<bool>, wants: Wants) -> Next {
        self.slot = slot;
        match drawn {
            Some(true) => self.refused = 0,
            Some(false) => self.refused = self.refused.saturating_add(1),
            None => {}
        }
        (self.draw, self.serve) = plan(slot, now, self.refused, self.seen.hidden(), wants);
        self.next()
    }

    /// After the host was served between frames, at `now`, because it asked: `poll` is when
    /// it wants the next, and `news` whether it reported anything, which the window draws as
    /// soon as the 60 Hz cap allows (never while it is hidden). The next frame keeps its time.
    pub fn polled(&mut self, now: Instant, poll: Option<Duration>, news: bool) -> Next {
        self.serve = host_poll(now, poll);
        if news && !self.seen.hidden() {
            let at = now.max(self.slot + FRAME_INTERVAL);
            self.draw = Some(self.draw.map_or(at, |due| due.min(at)));
        }
        self.next()
    }

    /// An input event that egui wants drawn: as soon as the 60 Hz cap allows, never while
    /// the window is hidden.
    pub fn wake(&mut self, now: Instant) -> Wake {
        if self.seen.hidden() {
            return Wake::Nothing;
        }
        let at = self.slot + FRAME_INTERVAL;
        if now >= at {
            return Wake::Now;
        }
        match self.draw {
            Some(due) if due <= at => Wake::Nothing,
            _ => {
                self.draw = Some(at);
                Wake::At(at)
            }
        }
    }
}

/// When the host asked to be served, `poll` from `now`, but never sooner than an interval:
/// the host is served at most 60 times a second however soon it asks.
fn host_poll(now: Instant, poll: Option<Duration>) -> Option<Instant> {
    poll.filter(|wait| *wait != Duration::MAX)
        .and_then(|wait| now.checked_add(wait.max(FRAME_INTERVAL)))
}

/// What comes after a frame that took `slot` and ended at `now`, with `refused` frames in a
/// row the surface did not take, the window `hidden` or not: when the next frame is due, and
/// when the host is served before it without one.
pub fn plan(
    slot: Instant,
    now: Instant,
    refused: u32,
    hidden: bool,
    wants: Wants,
) -> (Option<Instant>, Option<Instant>) {
    let poll = host_poll(now, wants.poll);
    if hidden {
        let every = if wants.editors {
            Some(EDITOR_POLL)
        } else if wants.animating {
            Some(HIDDEN_POLL)
        } else {
            None
        };
        let tick = every.map(|every| slot + every);
        let serve = match (tick, poll) {
            (Some(tick), Some(poll)) => Some(tick.min(poll)),
            (tick, poll) => tick.or(poll),
        };
        return (None, serve);
    }
    // egui's `Duration::MAX` means never.
    let mut due = wants
        .repaint
        .filter(|delay| *delay != Duration::MAX)
        .and_then(|delay| now.checked_add(delay));
    let mut sooner = |at: Instant| due = Some(due.map_or(at, |d| d.min(at)));
    let grid = if refused >= REFUSALS_BEFORE_BACKING_OFF {
        HIDDEN_POLL
    } else {
        FRAME_INTERVAL
    };
    if wants.animating || refused > 0 {
        sooner(slot + grid);
    }
    if wants.editors {
        sooner(slot + EDITOR_POLL);
    }
    if wants.acted {
        sooner(now);
    }
    let draw = due.map(|at| at.max(slot + FRAME_INTERVAL));
    (draw, poll)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn animating() -> Wants {
        Wants {
            animating: true,
            ..Wants::default()
        }
    }

    #[test]
    fn a_visible_window_with_moving_meters_draws_on_a_60_hz_grid() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        let slot = pacer.begin(t);
        assert_eq!(slot, t, "the first frame comes at once");
        let next = pacer.end(slot, t + 2 * MS, Some(true), animating());
        assert_eq!(next, Next::Draw(t + FRAME_INTERVAL));
        // The timer wakes it 3 ms late: the frame counts from when it was due.
        let slot = pacer.begin(t + FRAME_INTERVAL + 3 * MS);
        assert_eq!(slot, t + FRAME_INTERVAL);
        // A frame that took longer than an interval is followed at once, still on the grid.
        let next = pacer.end(slot, slot + 20 * MS, Some(true), animating());
        assert_eq!(next, Next::Draw(slot + FRAME_INTERVAL));
        assert_eq!(pacer.begin(slot + 20 * MS), slot + FRAME_INTERVAL);
    }

    #[test]
    fn a_refused_frame_takes_its_slot_so_the_next_waits_an_interval() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        let first = pacer.begin(t);
        pacer.end(first, t + MS, Some(true), Wants::default());
        // A second later, woken by input, the surface refuses the frame.
        let started = t + Duration::from_secs(1);
        let slot = pacer.begin(started);
        assert_eq!(slot, started);
        let next = pacer.end(slot, started + MS, Some(false), Wants::default());
        // Not one interval after the last frame drawn, a second ago, which is past and would
        // redraw at once for as long as the surface refuses, but one after the refused frame;
        // and tried even with nothing to animate, so what it held reaches the screen.
        assert_eq!(next, Next::Draw(started + FRAME_INTERVAL));
        assert!(next.at().unwrap() > started + MS);
    }

    #[test]
    fn a_surface_that_keeps_refusing_is_tried_four_times_a_second() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        let mut at = t;
        for refusal in 1..=REFUSALS_BEFORE_BACKING_OFF {
            let slot = pacer.begin(at);
            let next = pacer.end(slot, slot + MS, Some(false), animating());
            let expected = if refusal < REFUSALS_BEFORE_BACKING_OFF {
                FRAME_INTERVAL
            } else {
                HIDDEN_POLL
            };
            assert_eq!(next, Next::Draw(slot + expected), "refusal {refusal}");
            at = next.at().unwrap();
        }
        // Drawn again: back on the grid.
        let slot = pacer.begin(at);
        let next = pacer.end(slot, slot + MS, Some(true), animating());
        assert_eq!(next, Next::Draw(slot + FRAME_INTERVAL));
    }

    #[test]
    fn a_hidden_window_draws_nothing_and_serves_the_host_slowly() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        pacer.seen.zero_size = true;
        let slot = pacer.begin(t);
        // Meters moving, egui asking, a frame that acted: none of it draws while hidden.
        let busy = Wants {
            repaint: Some(Duration::ZERO),
            animating: true,
            editors: false,
            acted: true,
            poll: None,
        };
        assert_eq!(
            pacer.end(slot, t + MS, None, busy),
            Next::Serve(slot + HIDDEN_POLL)
        );
        // An editor open: its window may be in view, so it is served at its own rate.
        let editors = Wants {
            editors: true,
            ..busy
        };
        assert_eq!(
            pacer.end(slot, t + MS, None, editors),
            Next::Serve(slot + EDITOR_POLL)
        );
        // Nothing to serve: the loop sleeps until an event.
        assert_eq!(pacer.end(slot, t + MS, None, Wants::default()), Next::Wait);
        // Input does not wake it either.
        assert_eq!(pacer.wake(t + Duration::from_secs(1)), Wake::Nothing);
        // Seen again: the grid is back.
        pacer.seen.zero_size = false;
        assert_eq!(
            pacer.end(slot, t + MS, Some(true), busy),
            Next::Draw(slot + FRAME_INTERVAL)
        );
    }

    #[test]
    fn minimised_zero_sized_or_covered_is_hidden() {
        assert!(!Visibility::default().hidden());
        for seen in [
            Visibility {
                occluded: true,
                ..Visibility::default()
            },
            Visibility {
                zero_size: true,
                ..Visibility::default()
            },
            Visibility {
                minimized: true,
                ..Visibility::default()
            },
        ] {
            assert!(seen.hidden(), "{seen:?}");
        }
    }

    #[test]
    fn an_idle_window_waits_and_egui_or_an_act_wakes_it_no_faster_than_60_hz() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        let slot = pacer.begin(t);
        assert_eq!(
            pacer.end(slot, t + MS, Some(true), Wants::default()),
            Next::Wait
        );
        // egui's text cursor blinks in half a second.
        let blink = Wants {
            repaint: Some(Duration::from_millis(500)),
            ..Wants::default()
        };
        assert_eq!(
            pacer.end(slot, t + MS, Some(true), blink),
            Next::Draw(t + MS + Duration::from_millis(500))
        );
        // `Duration::MAX` from egui means never.
        let never = Wants {
            repaint: Some(Duration::MAX),
            ..Wants::default()
        };
        assert_eq!(pacer.end(slot, t + MS, Some(true), never), Next::Wait);
        // A frame that acted is followed at once, but not sooner than the cap.
        let acted = Wants {
            acted: true,
            ..Wants::default()
        };
        assert_eq!(
            pacer.end(slot, t + MS, Some(true), acted),
            Next::Draw(slot + FRAME_INTERVAL)
        );
        // An editor open: served at least every poll.
        let editors = Wants {
            editors: true,
            ..Wants::default()
        };
        assert_eq!(
            pacer.end(slot, t + MS, Some(true), editors),
            Next::Draw(slot + EDITOR_POLL)
        );
    }

    #[test]
    fn input_draws_as_soon_as_the_cap_allows() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        let slot = pacer.begin(t);
        pacer.end(slot, t + MS, Some(true), Wants::default());
        // Within an interval of the last frame: at the end of it.
        assert_eq!(pacer.wake(t + 5 * MS), Wake::At(t + FRAME_INTERVAL));
        assert_eq!(pacer.next(), Next::Draw(t + FRAME_INTERVAL));
        // Asked again: already due.
        assert_eq!(pacer.wake(t + 6 * MS), Wake::Nothing);
        // An interval later: now.
        assert_eq!(pacer.wake(t + FRAME_INTERVAL), Wake::Now);
    }

    #[test]
    fn an_idle_window_serves_the_host_when_it_asks_and_draws_only_for_news() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        let slot = pacer.begin(t);
        // No backend, nothing to animate: the host wants a poll in a second (a debounced
        // save), and nothing else is due.
        let asks = Wants {
            poll: Some(Duration::from_secs(1)),
            ..Wants::default()
        };
        let next = pacer.end(slot, t + MS, Some(true), asks);
        assert_eq!(next, Next::Serve(t + MS + Duration::from_secs(1)));
        // Served with no news: no frame, and the next poll when the host says.
        let served = t + Duration::from_secs(1);
        let next = pacer.polled(served, Some(Duration::from_millis(50)), false);
        assert_eq!(next, Next::Serve(served + Duration::from_millis(50)));
        // News: a frame at once (the last was long ago), before the next poll.
        let later = served + Duration::from_millis(50);
        let next = pacer.polled(later, Some(Duration::from_millis(50)), true);
        assert_eq!(next, Next::Draw(later));
        // A host that asks again at once is served no sooner than an interval: no spin.
        let slot = pacer.begin(later);
        let at_once = Wants {
            poll: Some(Duration::ZERO),
            ..Wants::default()
        };
        let next = pacer.end(slot, later + MS, Some(true), at_once);
        assert_eq!(next, Next::Serve(later + MS + FRAME_INTERVAL));
        // Nothing asked of the host and nothing to draw: the loop waits for an event.
        assert_eq!(pacer.polled(later + 2 * MS, None, false), Next::Wait);
    }

    #[test]
    fn a_poll_never_puts_off_a_frame_and_news_waits_for_the_cap() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        let slot = pacer.begin(t);
        // egui's cursor blinks in half a second; the host wants a poll in 50 ms.
        let wants = Wants {
            repaint: Some(Duration::from_millis(500)),
            poll: Some(Duration::from_millis(50)),
            ..Wants::default()
        };
        let next = pacer.end(slot, t + MS, Some(true), wants);
        assert_eq!(next, Next::Serve(t + MS + Duration::from_millis(50)));
        // No news: the blink is still due when it was.
        let served = t + MS + Duration::from_millis(50);
        assert_eq!(
            pacer.polled(served, None, false),
            Next::Draw(t + MS + Duration::from_millis(500))
        );
        // A frame a moment ago: news waits for the end of its interval.
        let slot = pacer.begin(served);
        pacer.end(slot, served + MS, Some(true), Wants::default());
        let next = pacer.polled(served + 2 * MS, None, true);
        assert_eq!(next, Next::Draw(served + FRAME_INTERVAL));
        // Hidden, news draws nothing.
        pacer.seen.minimized = true;
        let slot = pacer.begin(served + FRAME_INTERVAL);
        pacer.end(slot, slot, None, Wants::default());
        assert_eq!(pacer.polled(slot + MS, None, true), Next::Wait);
    }

    #[test]
    fn a_hidden_window_serves_the_host_when_it_asks_sooner_than_its_tick() {
        let t = Instant::now();
        let mut pacer = Pacer::new(t);
        pacer.seen.zero_size = true;
        let slot = pacer.begin(t);
        // Meters would move (every quarter second), but an undo runs out in 100 ms.
        let wants = Wants {
            animating: true,
            poll: Some(Duration::from_millis(100)),
            ..Wants::default()
        };
        assert_eq!(
            pacer.end(slot, t + MS, None, wants),
            Next::Serve(t + MS + Duration::from_millis(100))
        );
        // Nothing animating: only the host's own poll wakes it.
        let quiet = Wants {
            poll: Some(Duration::from_secs(2)),
            ..Wants::default()
        };
        assert_eq!(
            pacer.end(slot, t + MS, None, quiet),
            Next::Serve(t + MS + Duration::from_secs(2))
        );
    }
}
