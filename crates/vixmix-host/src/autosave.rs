//! When the setup is written: [`DEBOUNCE`] after the last change, so a fader drag saves once
//! when it ends, and never later than [`AT_MOST`] after the first unsaved change, so a plugin
//! knob turned for a minute still reaches the disk.

use std::time::{Duration, Instant};

/// How long the setup must stay unchanged before it is written.
pub(crate) const DEBOUNCE: Duration = Duration::from_millis(500);

/// The longest a change waits, however often the setup keeps changing.
pub(crate) const AT_MOST: Duration = Duration::from_secs(5);

/// The unsaved changes, by when the first and the last came.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Autosave {
    pending: Option<(Instant, Instant)>,
}

impl Autosave {
    /// The setup changed at `now`.
    pub fn touch(&mut self, now: Instant) {
        let first = self.pending.map_or(now, |(first, _)| first);
        self.pending = Some((first, now));
    }

    /// Whether anything waits to be written.
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// When the write is due, if anything waits.
    pub fn due_at(&self) -> Option<Instant> {
        self.pending
            .map(|(first, last)| (last + DEBOUNCE).min(first + AT_MOST))
    }

    /// Whether the write is due at `now`.
    pub fn due(&self, now: Instant) -> bool {
        self.due_at().is_some_and(|at| at <= now)
    }

    /// The setup was written.
    pub fn saved(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_waits_for_quiet_and_never_longer_than_its_limit() {
        let start = Instant::now();
        let ms = |n: u64| start + Duration::from_millis(n);
        let mut autosave = Autosave::default();
        assert!(!autosave.pending() && !autosave.due(ms(10_000)));
        autosave.touch(ms(0));
        assert!(!autosave.due(ms(499)));
        assert!(autosave.due(ms(500)), "500 ms of quiet");
        // A drag: a change every 100 ms keeps putting it off, up to five seconds.
        for n in 1..60 {
            autosave.touch(ms(n * 100));
            if n * 100 + 500 < 5_000 {
                assert!(!autosave.due(ms(n * 100 + 499)), "at {n}");
            }
        }
        assert!(autosave.due(ms(5_000)), "never later than five seconds");
        autosave.saved();
        assert!(!autosave.pending() && autosave.due_at().is_none());
    }
}
