//! The signals that ask the window to close.
//!
//! `SIGTERM` (a `pkill`, a logout, systemd stopping the session), `SIGINT` (Ctrl+C in the
//! terminal that started it) and `SIGHUP` (that terminal closed) each end a process where it
//! stands. For Vix Mix that skips the host's shutdown order: every app it routed stays pointed
//! at a sink that is gone, and a change younger than the autosave is lost. [`ask_to_close`]
//! takes the three instead and hands each to the window, which closes as it does when its
//! close button is pressed.
//!
//! A second signal, while that close has not ended the process, does what the signal always
//! did: a close that hangs (a plugin that never returns) can still be stopped without
//! `kill -9`.
//!
//! Unix only. On Windows the desktop asks through the window.

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use signal_hook::{flag, low_level};

/// The signals taken as a request to close.
pub const CLOSE_SIGNALS: [i32; 3] = [SIGTERM, SIGINT, SIGHUP];

/// From here on, the first of [`CLOSE_SIGNALS`] to arrive calls `ask` with its name (on a
/// thread of its own, so `ask` may do what a signal handler may not) instead of ending the
/// process, and one that arrives after it ends the process as it would have without this.
///
/// An error means a signal could not be taken, and may still end the process where it stands.
pub fn ask_to_close(ask: impl Fn(&'static str) + Send + 'static) -> io::Result<()> {
    let asked = Arc::new(AtomicBool::new(false));
    for signal in CLOSE_SIGNALS {
        // A signal's actions run in the order they were registered: the first signal finds
        // `asked` unset, does nothing here and sets it; the next finds it set and ends the
        // process as the signal's default does.
        flag::register_conditional_default(signal, Arc::clone(&asked))?;
        flag::register(signal, Arc::clone(&asked))?;
    }
    let mut signals = Signals::new(CLOSE_SIGNALS)?;
    std::thread::Builder::new()
        .name("vix-mix-signals".to_string())
        .spawn(move || {
            for signal in signals.forever() {
                ask(low_level::signal_name(signal).unwrap_or("a signal"));
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use super::*;

    /// Set for the child process of the test below, and only for it.
    const CHILD: &str = "VIXMIX_SIGNALS_TEST_CHILD";
    const HANGS: &str = "signals::tests::a_process_asked_to_close_that_never_does";

    /// The other side of `a_signal_asks_to_close_and_a_second_ends_a_close_that_hangs`: a
    /// process that takes the signals, says what it was asked, and never closes. Signals are
    /// the whole process's, so it cannot share one with the other tests.
    #[test]
    #[ignore = "the child process of `a_signal_asks_to_close_and_a_second_ends_a_close_that_hangs`"]
    fn a_process_asked_to_close_that_never_does() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        ask_to_close(|signal| println!("asked by {signal}")).expect("the signals are taken");
        println!("ready");
        // Longer than the test waits, and not for ever if the test dies first.
        std::thread::sleep(Duration::from_secs(30));
    }

    fn send(signal: &str, pid: u32) {
        let sent = Command::new("kill")
            .args(["-s", signal, &pid.to_string()])
            .status()
            .expect("the `kill` program sends a signal");
        assert!(sent.success(), "kill -s {signal} {pid}: {sent}");
    }

    #[test]
    fn a_signal_asks_to_close_and_a_second_ends_a_close_that_hangs() {
        for (name, number) in [("TERM", SIGTERM), ("INT", SIGINT), ("HUP", SIGHUP)] {
            let mut child = Command::new(std::env::current_exe().expect("this test program"))
                .args(["--ignored", "--exact", "--nocapture", HANGS])
                .env(CHILD, "1")
                .stdout(Stdio::piped())
                .spawn()
                .expect("this test program runs again as the child");
            let mut said = BufReader::new(child.stdout.take().expect("piped")).lines();
            let mut wait_for = |line: &str| {
                assert!(
                    said.by_ref().map_while(Result::ok).any(|l| l == line),
                    "the child ended without saying `{line}`"
                );
            };
            wait_for("ready");

            send(name, child.id());
            wait_for(&format!("asked by SIG{name}"));
            assert_eq!(
                child
                    .try_wait()
                    .expect("the child is ours")
                    .map(|s| s.to_string()),
                None,
                "SIG{name} asks; it does not end the process"
            );

            send(name, child.id());
            let ended = child.wait().expect("the child is ours");
            assert_eq!(
                ended.signal(),
                Some(number),
                "a second SIG{name} ends it as the signal does: {ended}"
            );
        }
    }
}
