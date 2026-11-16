//! The socket and its client, end to end over a real Unix socket: a request answered, a
//! follower told, who may hold the socket, whose it is, and how a connection ends.
//!
//! The mixer here is the model alone on a thread of its own, served when the connector wakes
//! it and published after every drain. Socket paths are short ones under the system's
//! temporary directory: a socket's path holds about a hundred bytes, and Cargo's own
//! temporary directory under `target/` is longer than that on most checkouts.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use vixmix_control::{
    ClientError, Connector, ERROR, ListenError, Listener, MAX_CLIENTS, MAX_LINE, PROTOCOL,
    SERVE_BUDGET, Session, Target, service,
};
use vixmix_model::{Applied, Command, Refused, Setup, apply, example};

/// How long a test waits for the other side before it calls that a failure.
const PATIENCE: Duration = Duration::from_secs(10);

static SCRATCH: AtomicU64 = AtomicU64::new(0);

/// A directory of this test's own, with a path short enough to hold a socket; removed when
/// the test is done with it.
struct Scratch(PathBuf);

impl Scratch {
    fn join(&self, name: impl AsRef<Path>) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch() -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "vm{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    assert!(
        dir.as_os_str().len() < 80,
        "{} is too long for a socket beside it: set TMPDIR to a shorter directory",
        dir.display()
    );
    Scratch(dir)
}

/// Waits until `met` says so, without sleeping a fixed time.
fn until(what: &str, mut met: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !met() {
        assert!(Instant::now() < deadline, "waited {PATIENCE:?} for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

struct Desk(Setup);

impl Target for Desk {
    fn setup(&self) -> &Setup {
        &self.0
    }
    fn apply(&mut self, command: Command) -> Result<Applied, Refused> {
        apply(&mut self.0, command)
    }
}

/// The example console served on a thread, as the app serves its host: woken by the
/// connector, a drain, then the followers told.
struct Mixer {
    connector: Connector,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Mixer {
    fn start() -> Mixer {
        // The wake is a slot: one waiting wake is enough.
        let (wake, woken) = sync_channel::<()>(1);
        let (mut service, connector) = service(move || {
            let _ = wake.try_send(());
        });
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
            connector,
            stop,
            thread: Some(thread),
        }
    }

    fn listen(&self, path: &Path) -> Result<Listener, ListenError> {
        Listener::open(path, self.connector.clone())
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn connect(path: &Path) -> Session {
    Session::connect(path, Some(PATIENCE)).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn a_request_over_the_socket_is_answered_and_a_follower_is_told_what_another_changed() {
    let scratch = scratch();
    let path = scratch.join("c.sock");
    let mixer = Mixer::start();
    let _listener = mixer.listen(&path).expect("listening");

    let mut asker = connect(&path);
    assert_eq!(asker.protocol(), PROTOCOL);
    let answer = asker.ask("get /ch/music/master/mute").unwrap();
    assert_eq!(answer.lines, ["/ch/3/master/mute true"]);
    assert_eq!(answer.error, None);

    let mut follower = connect(&path);
    let answer = follower.ask("watch /ch/music/master/mute").unwrap();
    assert_eq!(answer.lines, ["/ch/3/master/mute true"]);

    // The first client unmutes it: answered with what changed, and the follower is told.
    let answer = asker.ask("toggle /ch/music/master/mute").unwrap();
    assert_eq!(answer.lines, ["/ch/3/master/mute false"]);
    assert_eq!(
        follower.line().unwrap().as_deref(),
        Some("event /ch/3/master/mute false")
    );

    // A refusal comes back as the answer's error, ending with what to send instead.
    let answer = asker.ask("set /ch/music/master/db loud").unwrap();
    assert_eq!(answer.lines, Vec::<String>::new());
    let refused = answer.error.expect("refused");
    assert!(refused.contains("send it plain, as `-12`"), "{refused}");

    // A follower that asks on its own connection does not lose the event that came first:
    // the mixer told it of the toggle before it answered the request.
    let _ = asker.ask("toggle /ch/music/master/mute").unwrap();
    let answer = follower.ask("get /mix/master/preview").unwrap();
    assert_eq!(answer.events, ["/ch/3/master/mute true"]);
    assert_eq!(answer.lines, ["/mix/master/preview false"]);
}

#[test]
fn a_second_mixer_leaves_a_running_mixers_socket_alone_and_a_dead_mixers_file_is_replaced() {
    let scratch = scratch();
    let path = scratch.join("c.sock");
    let first = Mixer::start();
    let listener = first.listen(&path).expect("the first mixer listens");

    // A second mixer: refused, told who holds it, and the first still answers.
    let second = Mixer::start();
    let refused = second.listen(&path).err().expect("held by the first");
    assert!(matches!(refused, ListenError::Held { .. }), "{refused}");
    let said = refused.to_string();
    assert!(
        said.contains("another Vix Mix holds the control socket")
            && said.contains("close one of the two"),
        "{said}"
    );
    assert_eq!(
        connect(&path).ask("get /mix/master/mute").unwrap().lines,
        ["/mix/master/mute false"]
    );

    // The first mixer goes: its socket file goes with it, and the second may listen.
    drop(listener);
    assert!(
        !path.exists(),
        "the socket file is removed when the listener goes"
    );
    let listener = second.listen(&path).expect("the second mixer listens now");
    drop(listener);

    // A mixer that died left its socket file and holds no lock: it is replaced.
    drop(UnixListener::bind(&path).expect("a socket file"));
    assert!(
        path.exists(),
        "a dropped std listener leaves its file, as a killed mixer does"
    );
    let _listener = second
        .listen(&path)
        .expect("a dead mixer's file is replaced");
    assert_eq!(
        connect(&path).ask("get /mix/master/mute").unwrap().lines,
        ["/mix/master/mute false"]
    );
}

#[test]
fn the_directory_it_makes_and_the_socket_are_the_users_alone() {
    let scratch = scratch();
    let dir = scratch.join("made");
    let path = dir.join("c.sock");
    let mixer = Mixer::start();
    let _listener = mixer.listen(&path).expect("listening");
    let mode = |at: &Path| std::fs::metadata(at).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700, "the directory it made");
    assert_eq!(mode(&path), 0o600, "the socket");
}

#[test]
fn a_path_too_long_for_a_socket_is_refused_before_anything_is_made() {
    let scratch = scratch();
    let dir = scratch.join("x".repeat(200));
    let path = dir.join("c.sock");
    let mixer = Mixer::start();
    let refused = mixer.listen(&path).err().expect("too long");
    assert!(matches!(refused, ListenError::TooLong { .. }), "{refused}");
    assert!(
        refused
            .to_string()
            .contains("set VIXMIX_CONTROL_SOCKET to a shorter one"),
        "{refused}"
    );
    assert!(
        !dir.exists(),
        "nothing was made for a socket that cannot be"
    );
}

#[test]
fn a_ninth_connection_is_told_why_and_one_that_leaves_makes_room() {
    let scratch = scratch();
    let path = scratch.join("c.sock");
    let mixer = Mixer::start();
    let _listener = mixer.listen(&path).expect("listening");
    let mut eight: Vec<Session> = (0..MAX_CLIENTS).map(|_| connect(&path)).collect();
    let refused = Session::connect(&path, Some(PATIENCE))
        .err()
        .expect("a ninth");
    assert!(
        matches!(&refused, ClientError::Refused(said) if said.contains("close one and connect again")),
        "{refused}"
    );
    // The eight are still served.
    assert_eq!(
        eight[7].ask("get /mix/master/mute").unwrap().lines,
        ["/mix/master/mute false"]
    );
    drop(eight.pop());
    until("the eighth client to be gone", || {
        mixer.connector.connected() < MAX_CLIENTS
    });
    let _ninth = connect(&path);
}

#[test]
fn nothing_at_the_path_is_no_mixer_and_says_to_start_it() {
    let scratch = scratch();
    let path = scratch.join("c.sock");
    let refused = Session::connect(&path, Some(PATIENCE))
        .err()
        .expect("no mixer");
    assert!(matches!(refused, ClientError::NoMixer { .. }), "{refused}");
    assert!(
        refused
            .to_string()
            .contains("start Vix Mix, then send the request again"),
        "{refused}"
    );
}

#[test]
fn a_client_that_says_it_is_done_is_answered_first_and_then_closed() {
    let scratch = scratch();
    let path = scratch.join("c.sock");
    let mixer = Mixer::start();
    let _listener = mixer.listen(&path).expect("listening");
    let mut session = connect(&path);
    // Three requests and the end of the sending side, all before anything is read.
    session.send("get /mix/master/mute").unwrap();
    session.send("toggle /mix/master/mute").unwrap();
    session.send("get /mix/master/mute").unwrap();
    session.finish();
    let mut said = Vec::new();
    while let Some(line) = session.line().unwrap() {
        said.push(line);
    }
    assert_eq!(
        said,
        [
            "/mix/master/mute false",
            "ok",
            "/mix/master/mute true",
            "ok",
            "/mix/master/mute true",
            "ok",
        ]
    );
}

#[test]
fn a_line_past_the_limit_is_refused_and_its_connection_closed() {
    let scratch = scratch();
    let path = scratch.join("c.sock");
    let mixer = Mixer::start();
    let _listener = mixer.listen(&path).expect("listening");
    let mut session = connect(&path);
    session
        .send(&format!("get /{}", "x".repeat(MAX_LINE * 4)))
        .unwrap();
    let said = session.line().unwrap().expect("a refusal");
    assert!(
        said.starts_with(ERROR) && said.contains("one request a line"),
        "{said}"
    );
    assert_eq!(
        session.line().unwrap(),
        None,
        "and the connection is closed"
    );
    // The mixer serves on.
    assert_eq!(
        connect(&path).ask("get /mix/master/mute").unwrap().lines,
        ["/mix/master/mute false"]
    );
}

#[test]
fn a_listener_that_goes_closes_its_connections() {
    let scratch = scratch();
    let path = scratch.join("c.sock");
    let mixer = Mixer::start();
    let listener = mixer.listen(&path).expect("listening");
    let mut session = connect(&path);
    assert_eq!(session.ask("get /mix/master/mute").unwrap().error, None);
    drop(listener);
    assert_eq!(
        session.line().unwrap(),
        None,
        "the connection ended with the listener"
    );
    assert!(matches!(
        Session::connect(&path, Some(PATIENCE)),
        Err(ClientError::NoMixer { .. })
    ));
}
