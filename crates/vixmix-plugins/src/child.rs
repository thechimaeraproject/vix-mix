//! Describing a bundle in a child process, so a plugin that aborts or hangs while it loads
//! takes only that process with it: the child's side, [`answer_scan`], and the parent's,
//! [`ScanCommand`], which a scan runs for each bundle that must be loaded
//! ([`Describe::InChild`](crate::Describe::InChild)).
//!
//! The child describes the bundle as an in-process scan does and writes what it found to
//! stdout on one line that starts with [`ANSWER_MARK`], because a plugin, and Wine under a
//! bridge, write to stdout too. stderr is left alone, so a plugin's own messages still reach
//! the terminal or the journal. The parent drains stdout on a thread of its own, keeping the
//! newest marked line and at most [`MAX_ANSWER`] bytes of it, waits at most the command's
//! timeout and kills the child past it. A crash, an end without an answer or the timeout is
//! that bundle's failure; one child runs at a time.

use std::ffi::OsString;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chimaeraproject_vst3_host::{HostContext, ScanOptions};

use crate::scan::{Description, describe_here};

/// What starts the line of a child's stdout that carries its answer: the rest of the line is
/// the bundle's description in RON.
pub const ANSWER_MARK: &str = "vixmix-scan-answer: ";

/// How long a child may take to load and describe one bundle before it is killed and the
/// bundle fails: [`ScanCommand::new`]'s timeout.
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(60);

/// The longest answer kept, in bytes after the mark. A bundle of a thousand effects says about
/// a quarter of it.
const MAX_ANSWER: usize = 4 << 20;

/// How often the parent looks at the child it waits for.
const POLL: Duration = Duration::from_millis(5);

/// How long a child that has answered may take to end before it is stopped; its answer stands.
const AFTER_ANSWER: Duration = Duration::from_secs(5);

/// How long the parent waits, once the child has ended, for the rest of its stdout.
const DRAIN: Duration = Duration::from_secs(2);

/// The end of every failure a child's scan reports.
const RESCAN: &str = "then rescan plugins (menu, Rescan plugins)";

/// The command a scan runs to describe one bundle in a child process: `program`, then `args`,
/// then the bundle's path. The child must call [`answer_scan`] on that path; the app's command
/// is its own executable with `--scan-bundle`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// How long one bundle may take before its child is killed: [`SCAN_TIMEOUT`] unless a
    /// test shortens it.
    pub timeout: Duration,
}

impl ScanCommand {
    /// `program`, with `args` before the bundle's path, and [`SCAN_TIMEOUT`].
    pub fn new<A: Into<OsString>>(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = A>,
    ) -> ScanCommand {
        ScanCommand {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout: SCAN_TIMEOUT,
        }
    }

    /// The same command with another timeout.
    pub fn with_timeout(self, timeout: Duration) -> ScanCommand {
        ScanCommand { timeout, ..self }
    }

    /// Runs the child for `bundle` and waits for it, at most the timeout. `Ok` is the bundle's
    /// description, a failure included (the child's own, or that it crashed, ended without an
    /// answer or was stopped); `Err` is a failure that is not the bundle's: the child could not
    /// be started or watched.
    pub(crate) fn describe(&self, bundle: &Path) -> Result<Description, String> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .arg(bundle)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // The child leads a process group of its own, so stopping it stops the helper
            // processes a bridge starts beside the plugin (`stop`).
            command.process_group(0);
        }
        let not_run = |error: std::io::Error| {
            format!(
                "{}: the scan could not run `{}` to load it ({error}), so it was not scanned this time; start Vix Mix again (an update while it runs moves the program the scan runs), {RESCAN}",
                bundle.display(),
                self.program.display()
            )
        };
        let mut child = command.spawn().map_err(not_run)?;
        let slot = Arc::new(Mutex::new(None));
        let reader = match child.stdout.take() {
            Some(stdout) => {
                let slot = Arc::clone(&slot);
                std::thread::Builder::new()
                    .name("vixmix-scan-child".to_string())
                    .spawn(move || drain(stdout, &slot))
            }
            None => Err(std::io::Error::other("its stdout was not piped")),
        };
        let reader = match reader {
            Ok(reader) => reader,
            Err(error) => {
                stop(&mut child);
                return Err(not_run(error));
            }
        };

        let deadline = Instant::now() + self.timeout;
        let mut answered: Option<Instant> = None;
        let ended = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ended::Exited(status),
                Ok(None) => {}
                Err(error) => {
                    stop(&mut child);
                    return Err(format!(
                        "{}: the scan lost track of the process loading it ({error}), so it was not scanned this time; {RESCAN}",
                        bundle.display()
                    ));
                }
            }
            let now = Instant::now();
            if answered.is_none() && lock(&slot).is_some() {
                answered = Some(now);
            }
            let until = answered.map_or(deadline, |at| deadline.min(at + AFTER_ANSWER));
            if now >= until {
                stop(&mut child);
                break Ended::Stopped;
            }
            std::thread::sleep(POLL);
        };

        // What the child wrote before it ended may still be in the pipe.
        let drained = Instant::now() + DRAIN;
        while !reader.is_finished() && lock(&slot).is_none() && Instant::now() < drained {
            std::thread::sleep(POLL);
        }
        if reader.is_finished() {
            let _ = reader.join();
        }
        // Otherwise a process the child started still holds its stdout: the reader, holding
        // at most one bounded line, ends when that process does.
        let answer = lock(&slot).take();
        Ok(verdict(bundle, ended, answer, self.timeout))
    }
}

/// The child's side of a scan: describes `bundle` as an in-process scan does, loading it when
/// it has no readable `moduleinfo.json`, and writes the description to stdout on one line that
/// starts with [`ANSWER_MARK`]. Returns the exit code: 0 once the answer is written (a bundle
/// that failed included, its failure being the answer), 1 when stdout would not take it. On
/// Windows the crash dialogs are turned off first, so a plugin that aborts ends the process at
/// once instead of waiting on a dialog. For a process that does nothing else.
pub fn answer_scan(bundle: &Path, host: &HostContext) -> i32 {
    #[cfg(windows)]
    no_crash_dialog();
    let description = describe_here(bundle, host, &ScanOptions::default());
    let text = ron::to_string(&description).or_else(|error| {
        ron::to_string(&Description::failed(format!(
            "{}: its description could not be written ({error}); rename the bundle or the folder it is in, {RESCAN}",
            bundle.display()
        )))
    });
    let written = text.map_err(std::io::Error::other).and_then(|text| {
        // A newline first ends a line a plugin left unfinished; the line goes in one write.
        let line = format!("\n{ANSWER_MARK}{text}\n");
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(line.as_bytes())?;
        stdout.flush()
    });
    match written {
        Ok(()) => 0,
        Err(error) => {
            eprintln!(
                "vix-mix: the description of {} could not be written to stdout ({error}); the scan that asked for it reports the bundle as failed",
                bundle.display()
            );
            1
        }
    }
}

/// Turns off Windows' crash dialogs for this process, so a plugin that aborts or faults ends
/// it at once and the scan waiting for it reports the crash.
#[cfg(windows)]
fn no_crash_dialog() {
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    const WER_FAULT_REPORTING_NO_UI: u32 = 0x0020;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
        fn WerSetFlags(flags: u32) -> i32;
    }
    // SAFETY: both change only how this process reports its own faults.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
        WerSetFlags(WER_FAULT_REPORTING_NO_UI);
    }
}

/// How a child the parent waited for ended.
#[derive(Clone, Copy, Debug)]
enum Ended {
    Exited(ExitStatus),
    /// Killed by the parent: past the timeout, or still running a while after it answered.
    Stopped,
}

/// A child's answer: its marked line without the mark, or that it was past the bound.
#[derive(Clone, Debug, PartialEq)]
enum Answer {
    Line(String),
    TooLong,
}

fn lock<T>(slot: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads a child's stdout to its end, keeping the newest answer in `slot` (latest wins).
fn drain(mut stdout: impl Read, slot: &Mutex<Option<Answer>>) {
    let mut marked = Marked::default();
    let mut chunk = [0_u8; 8 << 10];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if let Some(answer) = marked.feed(&chunk[..read]) {
                    *lock(slot) = Some(answer);
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    if let Some(answer) = marked.finish() {
        *lock(slot) = Some(answer);
    }
}

/// Picks the marked line out of everything a child writes to stdout, holding at most the mark
/// and [`MAX_ANSWER`] bytes of it: any other line is dropped as it streams past.
#[derive(Debug, Default)]
struct Marked {
    line: Vec<u8>,
    /// The current line is not an answer, or no longer fits: dropped to its end.
    skipping: bool,
    /// The current line is an answer past the bound.
    too_long: bool,
}

impl Marked {
    /// Takes the next bytes; the newest answer whose line they end, if any.
    fn feed(&mut self, mut bytes: &[u8]) -> Option<Answer> {
        let mut newest = None;
        while let Some(end) = bytes.iter().position(|&b| b == b'\n') {
            self.take(&bytes[..end]);
            if let Some(answer) = self.end_line() {
                newest = Some(answer);
            }
            bytes = &bytes[end + 1..];
        }
        self.take(bytes);
        newest
    }

    /// The end of the stream: an answer whose line has no newline still counts.
    fn finish(mut self) -> Option<Answer> {
        self.end_line()
    }

    fn take(&mut self, mut part: &[u8]) {
        if self.skipping {
            return;
        }
        let mark = ANSWER_MARK.as_bytes();
        if self.line.len() < mark.len() {
            // Until it is as long as the mark, a line is a candidate only while it matches.
            let n = part.len().min(mark.len() - self.line.len());
            self.line.extend_from_slice(&part[..n]);
            part = &part[n..];
            if !mark.starts_with(&self.line) {
                self.skip(false);
                return;
            }
        }
        // Only a line that carries the whole mark gets here with bytes left.
        if self.line.len() + part.len() > mark.len() + MAX_ANSWER {
            self.skip(true);
            return;
        }
        self.line.extend_from_slice(part);
    }

    fn skip(&mut self, too_long: bool) {
        self.skipping = true;
        self.too_long = too_long;
        self.line.clear();
    }

    fn end_line(&mut self) -> Option<Answer> {
        let answer = if self.too_long {
            Some(Answer::TooLong)
        } else if !self.skipping && self.line.starts_with(ANSWER_MARK.as_bytes()) {
            let text = &self.line[ANSWER_MARK.len()..];
            let text = text.strip_suffix(b"\r").unwrap_or(text);
            Some(Answer::Line(String::from_utf8_lossy(text).into_owned()))
        } else {
            None
        };
        self.line.clear();
        self.skipping = false;
        self.too_long = false;
        answer
    }
}

/// What a child's end and answer mean for `bundle`. The answer stands whenever the child gave
/// one: it is written after the bundle was described and unloaded, so a crash after it is the
/// process's own teardown, which is logged.
fn verdict(bundle: &Path, ended: Ended, answer: Option<Answer>, timeout: Duration) -> Description {
    let skipped = |what: String, fix: &str| {
        Description::failed(format!(
            "{}: {what}, so it was skipped and the other plugins were scanned; {fix}, {RESCAN}",
            bundle.display()
        ))
    };
    match answer {
        Some(Answer::Line(text)) => match ron::from_str::<Description>(&text) {
            Ok(description) => {
                match ended {
                    Ended::Exited(status) if !status.success() => log::warn!(
                        "{}: described, then the process that scanned it ended with {}",
                        bundle.display(),
                        how(status)
                    ),
                    Ended::Stopped => log::warn!(
                        "{}: described, then the process that scanned it did not end and was stopped",
                        bundle.display()
                    ),
                    Ended::Exited(_) => {}
                }
                description
            }
            Err(error) => skipped(
                format!("the scan's answer for it could not be read ({error})"),
                "start Vix Mix again (an update while it runs changes the program the scan runs)",
            ),
        },
        Some(Answer::TooLong) => skipped(
            format!(
                "the scan's answer for it is longer than {} MiB",
                MAX_ANSWER >> 20
            ),
            "remove the plugin if it is not needed",
        ),
        None => match ended {
            Ended::Stopped => skipped(
                format!(
                    "the plugin took longer than {} to load, and its scan was stopped",
                    span(timeout)
                ),
                "fix or remove the plugin (one waiting on a dialog or a licence server does this)",
            ),
            Ended::Exited(status) if crashed(status) => skipped(
                format!("the plugin crashed while it loaded ({})", how(status)),
                "fix or remove the plugin (update it, or install or start what it needs)",
            ),
            Ended::Exited(status) => skipped(
                format!(
                    "the scan's process ended without describing it ({})",
                    how(status)
                ),
                "fix or remove the plugin",
            ),
        },
    }
}

/// A timeout in words.
fn span(duration: Duration) -> String {
    match (duration.as_secs(), duration.subsec_nanos()) {
        (1, 0) => "1 second".to_string(),
        (seconds, 0) => format!("{seconds} seconds"),
        _ => format!("{} ms", duration.as_millis()),
    }
}

/// Whether a child that ended this way crashed: killed by a signal, or on Windows ended by an
/// exception or by the C runtime's abort (exit code 3).
fn crashed(status: ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().is_some()
    }
    #[cfg(windows)]
    {
        status
            .code()
            .is_some_and(|code| code == 3 || code as u32 >= 0x8000_0000)
    }
    #[cfg(not(any(unix, windows)))]
    {
        !status.success()
    }
}

/// How a child ended, in words.
fn how(status: ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return match signal_name(signal) {
                Some(name) => format!("signal {signal}, {name}"),
                None => format!("signal {signal}"),
            };
        }
    }
    match status.code() {
        #[cfg(windows)]
        Some(3) => "exit code 3, the C runtime's abort".to_string(),
        #[cfg(windows)]
        Some(code) if code as u32 >= 0x8000_0000 => format!("exception 0x{:08X}", code as u32),
        Some(code) => format!("exit code {code}"),
        None => status.to_string(),
    }
}

#[cfg(unix)]
fn signal_name(signal: i32) -> Option<&'static str> {
    Some(match signal {
        libc::SIGABRT => "SIGABRT",
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGBUS => "SIGBUS",
        libc::SIGILL => "SIGILL",
        libc::SIGFPE => "SIGFPE",
        libc::SIGKILL => "SIGKILL",
        libc::SIGTERM => "SIGTERM",
        _ => return None,
    })
}

/// Kills the child (on Unix with its process group, so a bridge's helpers go with it) and
/// reaps it.
fn stop(child: &mut Child) {
    // SAFETY: killpg only sends a signal. The group is the child's own (it was started as the
    // leader of a new one) and the child is not reaped yet, so its id names no other group.
    #[cfg(unix)]
    if let Ok(group) = libc::pid_t::try_from(child.id())
        && unsafe { libc::killpg(group, libc::SIGKILL) } == 0
    {
        let _ = child.wait();
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{PluginEntry, PluginKind};

    fn description() -> Description {
        Description {
            entries: vec![PluginEntry {
                id: "vst3:7A1C5E934F2B8D06B3E9017C5D84A2F1".to_string(),
                name: "Echo \"Room\"\nTwo lines".to_string(),
                vendor: "Someone, Ltd.".to_string(),
                kind: PluginKind::Delay,
                bundle: "Echo.vst3".to_string(),
                path: PathBuf::from("/plugins/Echo.vst3"),
            }],
            failure: None,
            warnings: vec!["a warning\r\nover two lines".to_string()],
        }
    }

    /// What `answer_scan` writes for `description`.
    fn written(description: &Description) -> String {
        format!("\n{ANSWER_MARK}{}\n", ron::to_string(description).unwrap())
    }

    /// The answer a reader picks out of `stdout` arriving in pieces of `size` bytes.
    fn picked(stdout: &[u8], size: usize) -> Option<Answer> {
        let mut marked = Marked::default();
        let mut newest = None;
        for piece in stdout.chunks(size) {
            if let Some(answer) = marked.feed(piece) {
                newest = Some(answer);
            }
        }
        marked.finish().or(newest)
    }

    fn read(answer: Option<Answer>) -> Description {
        match answer {
            Some(Answer::Line(text)) => ron::from_str(&text).unwrap(),
            other => panic!("no answer: {other:?}"),
        }
    }

    #[test]
    fn the_answer_is_one_line_and_reads_back_whatever_its_text_holds() {
        let line = written(&description());
        assert_eq!(line.matches('\n').count(), 2, "{line}");
        assert_eq!(read(picked(line.as_bytes(), line.len())), description());
    }

    #[test]
    fn noise_around_the_marked_line_does_not_disturb_it_however_it_arrives() {
        let mut stdout = Vec::new();
        stdout.extend_from_slice(b"a plugin says hello\n[Wine] fixme:ole:CoInitialize\n");
        stdout.extend_from_slice(b"a line a plugin left without its end");
        stdout.extend_from_slice(written(&description()).as_bytes());
        stdout
            .extend_from_slice(b"vixmix-scan-answer\nvixmix-scan-ans\n x vixmix-scan-answer: ()\n");
        stdout.extend(std::iter::repeat_n(b'x', 3 << 20));
        stdout.extend_from_slice(b"\nnoise at the end, without a newline");
        for size in [stdout.len(), 4096, 7, 1] {
            assert_eq!(read(picked(&stdout, size)), description(), "{size}");
        }
    }

    #[test]
    fn the_newest_answer_wins_a_crlf_is_dropped_and_one_without_its_newline_counts() {
        let first = Description::failed("first".to_string());
        let mut stdout = written(&first).replace('\n', "\r\n");
        assert_eq!(read(picked(stdout.as_bytes(), 3)), first);
        let last = written(&description());
        stdout.push_str(last.trim_end());
        assert_eq!(read(picked(stdout.as_bytes(), 5)), description());
    }

    #[test]
    fn an_answer_past_the_bound_is_said_and_a_flood_of_noise_holds_no_memory() {
        let mut long = ANSWER_MARK.as_bytes().to_vec();
        long.extend(std::iter::repeat_n(b'(', MAX_ANSWER + 1));
        long.push(b'\n');
        assert_eq!(picked(&long, 64 << 10), Some(Answer::TooLong));

        let mut marked = Marked::default();
        for _ in 0..64 {
            assert_eq!(marked.feed(&[b'x'; 64 << 10]), None);
        }
        assert!(
            marked.line.capacity() <= 4 * ANSWER_MARK.len(),
            "{}",
            marked.line.capacity()
        );
        assert_eq!(marked.finish(), None);
    }

    #[cfg(unix)]
    fn aborted() -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(libc::SIGABRT)
    }

    #[cfg(windows)]
    fn aborted() -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        // What Rust's abort ends a process with: STATUS_STACK_BUFFER_OVERRUN, from __fastfail.
        ExitStatus::from_raw(0xC000_0409)
    }

    #[cfg(unix)]
    fn exited(code: u8) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(i32::from(code) << 8)
    }

    #[cfg(windows)]
    fn exited(code: u8) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(u32::from(code))
    }

    fn failure_of(description: Description) -> String {
        assert!(description.entries.is_empty(), "{description:?}");
        description.failure.expect("a failure")
    }

    #[test]
    fn a_crash_a_timeout_or_an_end_without_an_answer_fails_the_bundle_with_the_fix() {
        let bundle = Path::new("/plugins/Saturation Knob.vst3");
        let crash = failure_of(verdict(
            bundle,
            Ended::Exited(aborted()),
            None,
            SCAN_TIMEOUT,
        ));
        let slow = failure_of(verdict(
            bundle,
            Ended::Stopped,
            None,
            Duration::from_secs(2),
        ));
        let quiet = failure_of(verdict(
            bundle,
            Ended::Exited(exited(0)),
            None,
            SCAN_TIMEOUT,
        ));
        let refused = failure_of(verdict(
            bundle,
            Ended::Exited(exited(2)),
            None,
            SCAN_TIMEOUT,
        ));
        for failure in [&crash, &slow, &quiet, &refused] {
            assert!(
                failure.starts_with("/plugins/Saturation Knob.vst3: "),
                "{failure}"
            );
            assert!(
                failure.contains("so it was skipped and the other plugins were scanned"),
                "{failure}"
            );
            assert!(failure.contains("fix or remove the plugin"), "{failure}");
            assert!(failure.ends_with("(menu, Rescan plugins)"), "{failure}");
        }
        assert!(
            crash.contains("the plugin crashed while it loaded"),
            "{crash}"
        );
        if cfg!(unix) {
            assert!(crash.contains("SIGABRT"), "{crash}");
        } else {
            assert!(crash.contains("exception 0xC0000409"), "{crash}");
        }
        assert!(
            slow.contains("took longer than 2 seconds to load"),
            "{slow}"
        );
        assert!(
            quiet.contains("ended without describing it (exit code 0)"),
            "{quiet}"
        );
        assert!(refused.contains("(exit code 2)"), "{refused}");
        assert!(!refused.contains("crashed"), "{refused}");
    }

    #[test]
    fn an_answer_stands_even_when_the_process_ends_badly_after_it() {
        let answer = || match picked(written(&description()).as_bytes(), 1 << 20) {
            Some(answer) => Some(answer),
            None => panic!("no answer"),
        };
        for ended in [
            Ended::Exited(exited(0)),
            Ended::Exited(aborted()),
            Ended::Stopped,
        ] {
            assert_eq!(
                verdict(Path::new("Echo.vst3"), ended, answer(), SCAN_TIMEOUT),
                description()
            );
        }
        let unreadable = failure_of(verdict(
            Path::new("Echo.vst3"),
            Ended::Exited(exited(0)),
            Some(Answer::Line("not ron".to_string())),
            SCAN_TIMEOUT,
        ));
        assert!(unreadable.contains("could not be read"), "{unreadable}");
        let long = failure_of(verdict(
            Path::new("Echo.vst3"),
            Ended::Exited(exited(0)),
            Some(Answer::TooLong),
            SCAN_TIMEOUT,
        ));
        assert!(long.contains("longer than 4 MiB"), "{long}");
    }

    #[test]
    fn a_command_waits_a_minute_unless_told_otherwise() {
        let command = ScanCommand::new("vix-mix", ["--scan-bundle"]);
        assert_eq!(command.args, [OsString::from("--scan-bundle")]);
        assert_eq!(command.timeout, Duration::from_secs(60));
        let short = command.with_timeout(Duration::from_millis(1500));
        assert_eq!(short.timeout, Duration::from_millis(1500));
        assert_eq!(span(short.timeout), "1500 ms");
        assert_eq!(span(Duration::from_secs(1)), "1 second");
    }
}
