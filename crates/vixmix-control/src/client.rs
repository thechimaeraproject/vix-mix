//! The other end of the socket: a connection to a running mixer, for `vix-mix ctl` and for
//! any Rust program that would rather not write the lines by hand.
//!
//! A [`Session`] is connected and greeted. [`Session::ask`] sends one request and reads its
//! answer to the `ok` or `error:` that ends it, keeping apart any event that arrived on the
//! way; [`Session::line`] reads what comes next, which is how a follower waits for events;
//! [`Session::finish`] says no more requests will come, after which the mixer answers what
//! it was sent and closes.

use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::socket::SOCKET_ENV;
use crate::{ERROR, EVENT, GREETING, OK};

/// Why a request did not get its answer. Each message ends with what to do.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(
        "Vix Mix is not running, or runs without its control socket: nothing answers at {path} ({source}); start Vix Mix, then send the request again"
    )]
    NoMixer { path: PathBuf, source: io::Error },
    /// The mixer answered the connection with a refusal instead of its greeting; these are
    /// its words, which end with what to do.
    #[error("{0}")]
    Refused(String),
    #[error(
        "what answers at {path} is not Vix Mix's control socket: it said `{said}` where a mixer says `{GREETING}` and a number; check {SOCKET_ENV}"
    )]
    NotAMixer { path: PathBuf, said: String },
    #[error(
        "Vix Mix did not answer within {} seconds: its window may be busy (a plugin scan holds it); send the request again",
        .0.as_secs()
    )]
    Silent(Duration),
    #[error(
        "Vix Mix closed the connection before it answered: it may have quit; start it, then send the request again"
    )]
    Closed,
    #[error("the connection to Vix Mix failed ({0}); send the request again")]
    Io(io::Error),
}

/// What a request was answered with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// The answer's lines, each `address value`, without the line that ends it.
    pub lines: Vec<String>,
    /// Events that arrived before the answer ended, without their `event ` prefix: a
    /// follower's news is never lost to a request made on the same connection.
    pub events: Vec<String>,
    /// The message of the `error:` line when the request was refused; `None` for `ok`.
    pub error: Option<String>,
}

/// A connection to a running mixer.
pub struct Session {
    stream: UnixStream,
    lines: BufReader<UnixStream>,
    protocol: u32,
    patience: Option<Duration>,
}

impl Session {
    /// Connects to the mixer whose socket is at `path` and reads its greeting. `patience` is
    /// how long to wait for each line, `None` for as long as it takes.
    pub fn connect(path: &Path, patience: Option<Duration>) -> Result<Session, ClientError> {
        let stream = UnixStream::connect(path).map_err(|source| ClientError::NoMixer {
            path: path.to_path_buf(),
            source,
        })?;
        let reading = stream.try_clone().map_err(ClientError::Io)?;
        let mut session = Session {
            stream,
            lines: BufReader::new(reading),
            protocol: 0,
            patience: None,
        };
        session.patience(patience)?;
        let said = session.line()?.ok_or(ClientError::Closed)?;
        if let Some(refusal) = said.strip_prefix(ERROR) {
            return Err(ClientError::Refused(refusal.to_string()));
        }
        let protocol = said
            .strip_prefix(GREETING)
            .and_then(|number| number.trim().parse::<u32>().ok());
        let Some(protocol) = protocol else {
            return Err(ClientError::NotAMixer {
                path: path.to_path_buf(),
                said,
            });
        };
        session.protocol = protocol;
        Ok(session)
    }

    /// The protocol's number as the mixer said it; compare it with [`crate::PROTOCOL`].
    pub fn protocol(&self) -> u32 {
        self.protocol
    }

    /// How long to wait for each line from here on; `None` for as long as it takes, which is
    /// what following wants.
    pub fn patience(&mut self, patience: Option<Duration>) -> Result<(), ClientError> {
        self.stream
            .set_read_timeout(patience)
            .map_err(ClientError::Io)?;
        self.patience = patience;
        Ok(())
    }

    /// Sends one request line.
    pub fn send(&mut self, line: &str) -> Result<(), ClientError> {
        let mut text = line.trim_end_matches(['\r', '\n']).to_string();
        text.push('\n');
        self.stream
            .write_all(text.as_bytes())
            .map_err(ClientError::Io)
    }

    /// The next line the mixer sent, without its line end; `None` when it closed the
    /// connection.
    pub fn line(&mut self) -> Result<Option<String>, ClientError> {
        let mut line = String::new();
        match self.lines.read_line(&mut line) {
            Ok(0) => Ok(None),
            Ok(_) => {
                let end = line.trim_end_matches(['\r', '\n']).len();
                line.truncate(end);
                Ok(Some(line))
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Err(ClientError::Silent(self.patience.unwrap_or_default()))
            }
            Err(e) => Err(ClientError::Io(e)),
        }
    }

    /// Sends one request and reads its answer to the line that ends it.
    pub fn ask(&mut self, line: &str) -> Result<Answer, ClientError> {
        self.send(line)?;
        let mut answer = Answer::default();
        loop {
            let said = self.line()?.ok_or(ClientError::Closed)?;
            if said == OK {
                return Ok(answer);
            } else if let Some(refusal) = said.strip_prefix(ERROR) {
                answer.error = Some(refusal.to_string());
                return Ok(answer);
            } else if let Some(event) = said.strip_prefix(EVENT) {
                answer.events.push(event.to_string());
            } else {
                answer.lines.push(said);
            }
        }
    }

    /// Says that no more requests will come. The mixer answers the ones it was sent and then
    /// closes the connection, so reading on gives every answer and then the end.
    pub fn finish(&self) {
        let _ = self.stream.shutdown(Shutdown::Write);
    }

    /// The sending side for another thread: a relay reads requests on one thread and
    /// answers on another.
    pub fn sender(&self) -> Result<Sender, ClientError> {
        self.stream
            .try_clone()
            .map(|stream| Sender { stream })
            .map_err(ClientError::Io)
    }
}

/// A session's sending side, for a thread of its own.
pub struct Sender {
    stream: UnixStream,
}

impl Sender {
    /// Sends one request line.
    pub fn send(&mut self, line: &str) -> io::Result<()> {
        let mut text = line.trim_end_matches(['\r', '\n']).to_string();
        text.push('\n');
        self.stream.write_all(text.as_bytes())
    }

    /// Says that no more requests will come ([`Session::finish`]).
    pub fn finish(&self) {
        let _ = self.stream.shutdown(Shutdown::Write);
    }
}
