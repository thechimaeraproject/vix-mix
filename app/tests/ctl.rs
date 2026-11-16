//! `vix-mix ctl`, run as the program it is, against a control socket this test listens on:
//! what it prints, how it exits, the relay of standard input, and a `watch` that goes on.
//!
//! The mixer at the other end is the control crate's service over the example console,
//! served on a thread as the window serves its host; the window itself is `control.rs`'s
//! tests. Each socket is in a short path under the system's temporary directory, because a
//! socket's path holds about a hundred bytes.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread::JoinHandle;
use std::time::Duration;

use vixmix_control::{Listener, SERVE_BUDGET, Session, Target, service};
use vixmix_model::{Applied, Refused, Setup, apply, example};

static SCRATCH: AtomicU64 = AtomicU64::new(0);

struct Desk(Setup);

impl Target for Desk {
    fn setup(&self) -> &Setup {
        &self.0
    }
    fn apply(&mut self, command: vixmix_model::Command) -> Result<Applied, Refused> {
        apply(&mut self.0, command)
    }
}

/// The example console behind a control socket, served on a thread; its directory goes when
/// it does.
struct Mixer {
    dir: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _listener: Listener,
}

impl Mixer {
    fn start() -> Mixer {
        let dir = std::env::temp_dir().join(format!(
            "vmc{}-{}",
            std::process::id(),
            SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let socket = dir.join("c.sock");
        assert!(
            socket.as_os_str().len() < 90,
            "{} is too long for a socket: set TMPDIR to a shorter directory",
            socket.display()
        );
        // The wake is a slot: one waiting wake is enough.
        let (wake, woken) = sync_channel::<()>(1);
        let (mut service, connector) = service(move || {
            let _ = wake.try_send(());
        });
        let listener = Listener::open(&socket, connector).expect("listening");
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let mut desk = Desk(example::setup());
            while !stopped.load(Ordering::Acquire) {
                if woken.recv_timeout(Duration::from_millis(20)).is_ok() {
                    service.serve(SERVE_BUDGET, &mut desk);
                    service.publish(&desk.0);
                }
            }
        });
        Mixer {
            dir,
            socket,
            stop,
            thread: Some(thread),
            _listener: listener,
        }
    }

    /// `vix-mix ctl --socket <this mixer's>`, ready for its request.
    fn ctl(&self) -> Command {
        let mut command = vix_mix();
        command.arg("ctl").arg("--socket").arg(&self.socket);
        command
    }

    /// `vix-mix ctl` with `request`, run to its end.
    fn ask(&self, request: &[&str]) -> Output {
        self.ctl().args(request).output().expect("vix-mix runs")
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn vix_mix() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vix-mix"))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_request_prints_its_answer_and_exits_0() {
    let mixer = Mixer::start();
    let read = mixer.ask(&["get", "/ch/music/master/mute"]);
    assert_eq!(stdout(&read), "/ch/3/master/mute true\n");
    assert_eq!(read.status.code(), Some(0), "{}", stderr(&read));
    assert_eq!(stderr(&read), "");

    // A write prints what it changed, as it reads now: Music by name, its mute both sends.
    let written = mixer.ask(&["toggle", "/ch/music/mute"]);
    assert_eq!(
        stdout(&written),
        "/ch/3/mute true\n/ch/3/monitor/mute true\n"
    );
    assert_eq!(written.status.code(), Some(0), "{}", stderr(&written));

    // The next run of the command sees what the last one did: the mixer holds the state.
    let nudged = mixer.ask(&["nudge", "/mix/monitor/db", "-6"]);
    assert_eq!(
        stdout(&nudged),
        "/mix/monitor/db -6\n/mix/monitor/position 0.7\n"
    );
    let read = mixer.ask(&["/mix/monitor/db"]);
    assert_eq!(stdout(&read), "/mix/monitor/db -6\n");

    // Written to what it already is: carried out, nothing to print.
    let again = mixer.ask(&["set", "/ch/music/mute", "true"]);
    assert_eq!(
        (stdout(&again).as_str(), again.status.code()),
        ("", Some(0))
    );
}

#[test]
fn a_refusal_is_on_standard_error_and_exits_1() {
    let mixer = Mixer::start();
    for (request, expected) in [
        (
            vec!["set", "/ch/9/mute", "true"],
            "there is no channel 9: the channels are",
        ),
        (
            vec!["set", "/ch/mic/monitor/position", "2"],
            "clamp it to 0..=1 before sending",
        ),
        (
            vec!["set", "/ch/mic/name", "Voice"],
            "a channel is renamed in the window",
        ),
    ] {
        let refused = mixer.ask(&request);
        assert_eq!(refused.status.code(), Some(1), "{request:?}");
        assert_eq!(stdout(&refused), "", "{request:?}");
        let said = stderr(&refused);
        assert!(
            said.starts_with("vix-mix ctl: ") && said.contains(expected),
            "{request:?} said `{said}`"
        );
    }
}

#[test]
fn what_is_not_a_request_exits_2_without_asking_and_says_what_one_is() {
    // No mixer at all: none is needed to say what a request is.
    for (words, expected) in [
        (vec!["ctl"], "wants a request after it"),
        (
            vec!["ctl", "mute", "/ch/1"],
            "get, set, toggle, nudge, watch, unwatch",
        ),
        (vec!["ctl", "set", "/ch/1/mute"], "as `set /ch/1/mute true`"),
        (
            vec!["ctl", "--socket"],
            "wants the path of a mixer's control socket",
        ),
    ] {
        let refused = vix_mix().args(&words).output().expect("vix-mix runs");
        assert_eq!(refused.status.code(), Some(2), "{words:?}");
        let said = stderr(&refused);
        assert!(
            said.contains(expected) && said.contains("usage: vix-mix ctl"),
            "{words:?} said `{said}`"
        );
    }
    let help = vix_mix()
        .args(["ctl", "--help"])
        .output()
        .expect("vix-mix runs");
    assert_eq!(help.status.code(), Some(0));
    assert!(
        stdout(&help).starts_with("usage: vix-mix ctl"),
        "{}",
        stdout(&help)
    );
}

#[test]
fn no_vix_mix_at_the_socket_exits_3_and_says_to_start_it() {
    let dir = std::env::temp_dir().join(format!("vmc{}-none", std::process::id()));
    let nobody = vix_mix()
        .args(["ctl", "--socket"])
        .arg(dir.join("c.sock"))
        .args(["get", "/"])
        .output()
        .expect("vix-mix runs");
    assert_eq!(nobody.status.code(), Some(3));
    assert_eq!(stdout(&nobody), "");
    assert!(
        stderr(&nobody).contains("start Vix Mix, then send the request again"),
        "{}",
        stderr(&nobody)
    );
}

#[test]
fn a_dash_relays_standard_input_and_ends_when_it_does() {
    let mixer = Mixer::start();
    let mut child = mixer
        .ctl()
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("vix-mix runs");
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(b"get /mix/master/mute\ntoggle /mix/master/mute\n\nset /ch/9/mute true\n")
        .expect("the requests are written");
    // Standard input is closed: every request is answered, then the command ends.
    let output = child.wait_with_output().expect("vix-mix ends");
    let said: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    assert_eq!(said[0], vixmix_control::greeting());
    assert_eq!(
        said[1..5],
        [
            "/mix/master/mute false",
            "ok",
            "/mix/master/mute true",
            "ok"
        ]
    );
    // The blank line was no request; the last was refused on the line that ends its answer.
    assert_eq!(said.len(), 6, "{said:?}");
    assert!(
        said[5].starts_with("error: there is no channel 9"),
        "{said:?}"
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn a_watch_prints_the_value_and_then_each_change_until_it_is_stopped() {
    let mixer = Mixer::start();
    let mut child = mixer
        .ctl()
        .args(["watch", "/mix/master/mute"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("vix-mix runs");
    let mut said = BufReader::new(child.stdout.take().expect("piped")).lines();
    let mut next = || said.next().expect("a line").expect("readable");
    assert_eq!(next(), "/mix/master/mute false");
    // Another program changes it, twice.
    let mut other = Session::connect(Path::new(&mixer.socket), Some(Duration::from_secs(10)))
        .expect("the test connects");
    assert_eq!(other.ask("toggle /mix/master/mute").unwrap().error, None);
    assert_eq!(next(), "/mix/master/mute true");
    assert_eq!(other.ask("set /mix/master/mute false").unwrap().error, None);
    assert_eq!(next(), "/mix/master/mute false");
    assert_eq!(
        child.try_wait().expect("ours").map(|s| s.to_string()),
        None,
        "it goes on"
    );
    child.kill().expect("stopped");
    let _ = child.wait();
}
