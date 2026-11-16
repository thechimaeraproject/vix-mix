//! The Unix socket: where it is, who may hold it, and the threads that carry a connection.
//!
//! **Where.** [`socket_path`]: in the session's runtime directory (`XDG_RUNTIME_DIR`), in a
//! directory of our own, named from the configuration profile so a debug build or a named
//! profile never takes the installed mixer's; or wherever `VIXMIX_CONTROL_SOCKET` says.
//!
//! **Who.** One mixer holds a socket. [`Listener::open`] takes a lock on a file beside it
//! (`control.lock`) and keeps it for as long as it listens; the kernel lets it go when the
//! process ends, however it ends. A second mixer finds the lock held and is refused
//! ([`ListenError::Held`]); the first is left as it is. Only the holder removes a socket file
//! a dead mixer left behind.
//!
//! **Whose.** The directory, when this makes it, is the user's alone (0700), and so is the
//! socket (0600): any program of the same user may connect, and nobody else's.
//!
//! **Threads.** One accepts. Each connection has one that reads its lines into the service
//! and one that writes what the service sends. None of them touches the mixer: they queue
//! and are queued for (`service.rs`). A connection ends when its client closes it, when the
//! service drops the client (the writer finishes what was queued, then closes), or when a
//! write has not gone through in [`WRITE_TIMEOUT`].

use std::ffi::OsStr;
use std::fs::{self, DirBuilder, File, Permissions, TryLockError};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use vixmix_model::slug;

use crate::service::{Client, Connector, Full, MAX_LINE};

/// The environment variable that says where the socket is, for the mixer and for `ctl` alike.
pub const SOCKET_ENV: &str = "VIXMIX_CONTROL_SOCKET";
/// The environment variable the session's runtime directory is read from.
pub const RUNTIME_ENV: &str = "XDG_RUNTIME_DIR";
/// Our directory inside the runtime directory.
pub const DIR_NAME: &str = "vix-mix";
/// The profile whose socket has the plain name: the installed mixer's.
pub const DEFAULT_PROFILE: &str = "default";

/// How long a write to a client may take before its connection is closed.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// Where the socket of the mixer running under `profile` is. `explicit` is the value of
/// [`SOCKET_ENV`] and `runtime_dir` of [`RUNTIME_ENV`]; a blank one counts as unset.
///
/// `explicit` is used as it is. Otherwise it is `<runtime_dir>/vix-mix/control.sock` for
/// the profile `default`, and `control-<profile>.sock` beside it for any other, the name
/// made one plain word.
pub fn socket_path(
    explicit: Option<&OsStr>,
    runtime_dir: Option<&OsStr>,
    profile: &str,
) -> Result<PathBuf, String> {
    let set = |value: Option<&OsStr>| value.filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(path) = set(explicit) {
        return Ok(path);
    }
    let Some(runtime) = set(runtime_dir) else {
        return Err(format!(
            "the control socket lives in the session's runtime directory and {RUNTIME_ENV} is not set: start Vix Mix from a desktop session, or set {SOCKET_ENV} to the path the socket should have"
        ));
    };
    let file = if profile.eq_ignore_ascii_case(DEFAULT_PROFILE) {
        "control.sock".to_string()
    } else {
        format!("control-{}.sock", slug(profile))
    };
    Ok(runtime.join(DIR_NAME).join(file))
}

/// [`socket_path`] with this process's environment.
pub fn socket_path_from_env(profile: &str) -> Result<PathBuf, String> {
    socket_path(
        std::env::var_os(SOCKET_ENV).as_deref(),
        std::env::var_os(RUNTIME_ENV).as_deref(),
        profile,
    )
}

/// Why the socket could not be opened. Each message ends with what to do.
#[derive(Debug, thiserror::Error)]
pub enum ListenError {
    #[error(
        "another Vix Mix holds the control socket at {path}: this one runs without it, so `vix-mix ctl` and every controller reach the other; close one of the two"
    )]
    Held { path: PathBuf },
    #[error(
        "{path} is too long to be a socket's path ({len} bytes): set {SOCKET_ENV} to a shorter one"
    )]
    TooLong { path: PathBuf, len: usize },
    #[error(
        "the control socket could not be opened: {what} {path} failed ({source}); check that the directory is yours to write, or set {SOCKET_ENV} to a path that is"
    )]
    Io {
        what: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

/// The connections that are open, so they can be closed when the listener goes.
type Open = Arc<Mutex<Vec<(u64, UnixStream)>>>;

/// The socket, listening. Dropping it stops the accepting thread, closes every connection
/// and removes the socket file; the lock goes with it.
pub struct Listener {
    path: PathBuf,
    /// Held for as long as this listens: what makes a second mixer leave the socket alone.
    _lock: File,
    stop: Arc<AtomicBool>,
    open: Open,
    acceptor: Option<JoinHandle<()>>,
}

impl Listener {
    /// Listens at `path` and brings every connection to `connector`'s service.
    ///
    /// The directory is made, the user's alone, when it is not there. The lock beside the
    /// socket is taken, or the socket is another mixer's ([`ListenError::Held`]) and nothing
    /// is touched. A socket file left by a mixer that is gone is replaced.
    pub fn open(path: &Path, connector: Connector) -> Result<Listener, ListenError> {
        let io = |what: &'static str, at: &Path| {
            let at = at.to_path_buf();
            move |source| ListenError::Io {
                what,
                path: at,
                source,
            }
        };
        // Before anything is made: a path a socket cannot have.
        if SocketAddr::from_pathname(path).is_err() {
            return Err(ListenError::TooLong {
                path: path.to_path_buf(),
                len: path.as_os_str().len(),
            });
        }
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(io("making the directory", dir))?;
        }
        let lock_path = path.with_extension("lock");
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(io("opening the lock file", &lock_path))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(ListenError::Held {
                    path: path.to_path_buf(),
                });
            }
            Err(TryLockError::Error(source)) => {
                return Err(io("locking", &lock_path)(source));
            }
        }
        // Ours now: whatever is at the path was left by a mixer that is gone.
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(io("removing the old socket at", path)(source)),
        }
        let listener = UnixListener::bind(path).map_err(io("binding", path))?;
        fs::set_permissions(path, Permissions::from_mode(0o600))
            .map_err(io("making private", path))?;

        let stop = Arc::new(AtomicBool::new(false));
        let open: Open = Arc::new(Mutex::new(Vec::new()));
        let acceptor = {
            let (stop, open) = (Arc::clone(&stop), Arc::clone(&open));
            thread::Builder::new()
                .name("vix-mix-control".to_string())
                .spawn(move || accept(&listener, &connector, &stop, &open))
                .map_err(io("starting a thread for", path))?
        };
        Ok(Listener {
            path: path.to_path_buf(),
            _lock: lock,
            stop,
            open,
            acceptor: Some(acceptor),
        })
    }

    /// Where it listens.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // The accepting thread waits in `accept`: a connection of our own lets it see the
        // flag. If that cannot be made it is left where it is, and ends with the process.
        if UnixStream::connect(&self.path).is_ok()
            && let Some(acceptor) = self.acceptor.take()
        {
            let _ = acceptor.join();
        }
        for (_, stream) in self
            .open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let _ = fs::remove_file(&self.path);
    }
}

/// The accepting thread: each connection becomes a client, or is told why it is not one.
fn accept(listener: &UnixListener, connector: &Connector, stop: &AtomicBool, open: &Open) {
    let mut next = 0u64;
    for stream in listener.incoming() {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let Ok(stream) = stream else {
            // Out of descriptors, most likely: wait rather than spin on the same refusal.
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
        match connector.open() {
            Ok(client) => {
                next += 1;
                carry(stream, client, next, open);
            }
            Err(Full) => {
                let _ = (&stream).write_all(format!("{}\n", Full::line()).as_bytes());
            }
        }
    }
}

/// Starts the two threads of one connection. If either cannot start, the connection closes:
/// dropping the client's halves tells the service it went.
fn carry(stream: UnixStream, client: Client, id: u64, open: &Open) {
    let Client { requests, answers } = client;
    let (Ok(reading), Ok(kept)) = (stream.try_clone(), stream.try_clone()) else {
        return;
    };
    open.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, kept));

    // What the service sends, written until the service lets the client go; then the
    // connection closes, which also ends the reading thread.
    let writer = thread::Builder::new()
        .name("vix-mix-control-out".to_string())
        .spawn(move || {
            let mut stream = stream;
            for message in answers {
                if stream.write_all(message.as_bytes()).is_err() {
                    break;
                }
            }
            let _ = stream.shutdown(Shutdown::Both);
        });
    if writer.is_err() {
        forget(open, id);
        return;
    }

    // The client's lines, until it closes its side or the service is gone. The connection
    // is not closed here: what the service still has to answer is written first.
    let open = Arc::clone(open);
    let reader = thread::Builder::new()
        .name("vix-mix-control-in".to_string())
        .spawn(move || {
            let mut lines = BufReader::new(reading);
            while let Some(line) = read_line(&mut lines) {
                if !requests.send(&line) {
                    break;
                }
            }
            drop(requests);
            forget(&open, id);
        });
    // A reader that could not start has dropped `requests` with its closure: the service
    // lets the client go and the writer closes the connection.
    let _ = reader;
}

fn forget(open: &Open, id: u64) {
    open.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|(other, _)| *other != id);
}

/// The next line, without its line end; `None` at the end of the stream or on an error. A
/// line longer than the service takes is cut just past its limit, which is enough for the
/// service to refuse it, and never read whole.
fn read_line(lines: &mut impl BufRead) -> Option<String> {
    let mut bytes = Vec::new();
    let read = lines
        .by_ref()
        .take(MAX_LINE as u64 + 2)
        .read_until(b'\n', &mut bytes)
        .ok()?;
    if read == 0 {
        return None;
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_in_the_runtime_directory_under_the_profiles_name() {
        let runtime = OsStr::new("/run/user/1000");
        assert_eq!(
            socket_path(None, Some(runtime), "default").unwrap(),
            PathBuf::from("/run/user/1000/vix-mix/control.sock")
        );
        // A debug build, a named profile: beside it, so neither takes the installed mixer's.
        assert_eq!(
            socket_path(None, Some(runtime), "dev").unwrap(),
            PathBuf::from("/run/user/1000/vix-mix/control-dev.sock")
        );
        assert_eq!(
            socket_path(None, Some(runtime), "Stream Rig").unwrap(),
            PathBuf::from("/run/user/1000/vix-mix/control-stream-rig.sock")
        );
        // Said outright, it is where it is said to be, whatever the profile.
        assert_eq!(
            socket_path(Some(OsStr::new("/tmp/mixer.sock")), Some(runtime), "dev").unwrap(),
            PathBuf::from("/tmp/mixer.sock")
        );
        // A blank variable is an unset one.
        assert_eq!(
            socket_path(Some(OsStr::new("")), Some(runtime), "default").unwrap(),
            PathBuf::from("/run/user/1000/vix-mix/control.sock")
        );
        for no_runtime in [None, Some(OsStr::new(""))] {
            let refused = socket_path(None, no_runtime, "default").expect_err("no runtime dir");
            assert!(
                refused.contains("XDG_RUNTIME_DIR is not set")
                    && refused.contains("set VIXMIX_CONTROL_SOCKET"),
                "{refused}"
            );
        }
    }

    #[test]
    fn a_line_is_read_without_its_end_and_a_long_one_is_cut_not_read_whole() {
        let mut lines = io::Cursor::new(b"get /\r\nset /ch/1/mute true\nlast".to_vec());
        assert_eq!(read_line(&mut lines).as_deref(), Some("get /"));
        assert_eq!(
            read_line(&mut lines).as_deref(),
            Some("set /ch/1/mute true")
        );
        // The stream ended in the middle of a line: it is a line.
        assert_eq!(read_line(&mut lines).as_deref(), Some("last"));
        assert_eq!(read_line(&mut lines), None);
        // A megabyte with no line end: cut past the limit, so the service refuses it.
        let mut flood = io::Cursor::new(vec![b'x'; 1 << 20]);
        let cut = read_line(&mut flood).expect("a cut line");
        assert!(
            cut.len() > MAX_LINE && cut.len() <= MAX_LINE + 2,
            "{}",
            cut.len()
        );
    }
}
