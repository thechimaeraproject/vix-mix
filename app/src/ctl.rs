//! `vix-mix ctl`: the control socket's command. It asks the Vix Mix that is running, and is
//! what a shortcut or a script runs; `vix-mix ctl -` hands the socket to standard input and
//! output, a line each way, which is what a program that would rather not open a socket
//! starts.
//!
//! It runs before any flag of the window is read, with no window, no host and no
//! configuration touched ([`crate::main`]): the same binary, a second use of it.
//!
//! **What it prints.** One request: its answer's lines on standard output, and nothing for
//! `ok`. `watch` prints the values and then every change, a line each as `address value`,
//! until it is stopped or the mixer goes. `-` prints what the mixer says, as it says it: the
//! greeting first, then answers and events, word for word. A refusal is on standard error.
//!
//! **How it ends.** 0 when the request was carried out; 1 when the mixer refused it or the
//! connection failed, with why; 2 when what was written is not a request, with what one is;
//! 3 when no Vix Mix answers, so a script can tell "start it first" from "that was wrong".

use std::path::PathBuf;

/// The word after `vix-mix` that makes the rest a request to the running mixer.
pub const CTL: &str = "ctl";

/// What `vix-mix ctl` says of itself.
pub const USAGE: &str = "\
usage: vix-mix ctl [--fake | --socket <path>] <verb> <address> [value]
       vix-mix ctl [--fake | --socket <path>] -
  get <address>             print every value at or under an address; `get /` prints them all
  set <address> <value>     write a switch (true or false), a fader (its position, or decibels) or a device (its name)
  toggle <address>          flip a switch
  nudge <address> <amount>  move a fader by an amount
  watch <address>           print the values, then every change, until stopped
  -                         requests on standard input, a line each; what Vix Mix says on standard output
  --fake                    ask `vix-mix --fake` instead of the mixer that plays
  --socket <path>           ask the mixer whose control socket is at this path
exit: 0 done; 1 refused or failed, why on standard error; 2 not a request; 3 no Vix Mix answers";

/// Carried out.
pub const DONE: i32 = 0;
/// Refused by the mixer, or the connection failed.
pub const REFUSED: i32 = 1;
/// What was written is not a request.
pub const NOT_A_REQUEST: i32 = 2;
/// No Vix Mix answers at the socket.
// Where there is no Unix socket there is no mixer to ask, so nothing answers with this or
// reads which mixer and which request were meant; the parsing is the same everywhere.
#[cfg_attr(not(unix), allow(dead_code))]
pub const NO_MIXER: i32 = 3;

/// Which mixer to ask.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Place {
    /// The one that runs under this build's profile.
    Profile,
    /// `vix-mix --fake`.
    Fake,
    /// The one whose socket is here.
    Socket(PathBuf),
}

/// What to do once connected.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum What {
    /// One request, and whether it follows (a `watch`).
    Request { line: String, follows: bool },
    /// Standard input to the socket and the socket to standard output.
    Relay,
    /// Print [`USAGE`].
    Help,
}

#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Asked {
    place: Place,
    what: What,
}

impl Asked {
    fn parse(args: &[String]) -> Result<Asked, String> {
        let mut place = Place::Profile;
        let mut rest = args;
        loop {
            match rest.first().map(String::as_str) {
                Some("--help" | "-h") => {
                    return Ok(Asked {
                        place,
                        what: What::Help,
                    });
                }
                Some("--fake") => {
                    place = Place::Fake;
                    rest = &rest[1..];
                }
                Some("--socket") => {
                    let path = rest.get(1).ok_or(
                        "`--socket` wants the path of a mixer's control socket, e.g. `--socket /run/user/1000/vix-mix/control.sock`",
                    )?;
                    place = Place::Socket(PathBuf::from(path));
                    rest = &rest[2..];
                }
                _ => break,
            }
        }
        if rest.is_empty() {
            return Err(
                "`vix-mix ctl` wants a request after it, e.g. `vix-mix ctl toggle /ch/mic/mute`, or `-` to read requests from standard input"
                    .to_string(),
            );
        }
        if rest == ["-"] {
            return Ok(Asked {
                place,
                what: What::Relay,
            });
        }
        // The words as one line: read here by the mixer's own reader, so what is not a
        // request is said at once, in the mixer's words, without asking it.
        let line = rest.join(" ");
        let request = vixmix_control::parse(&line)?.ok_or(
            "`vix-mix ctl` wants a request after it, e.g. `vix-mix ctl toggle /ch/mic/mute`",
        )?;
        Ok(Asked {
            place,
            what: What::Request {
                line,
                follows: request.verb == vixmix_control::Verb::Watch,
            },
        })
    }
}

/// Runs `vix-mix ctl` with what followed it, and answers the exit code.
pub fn run(args: &[String]) -> i32 {
    let asked = match Asked::parse(args) {
        Ok(asked) => asked,
        Err(problem) => {
            eprintln!("vix-mix ctl: {problem}\n{USAGE}");
            return NOT_A_REQUEST;
        }
    };
    if asked.what == What::Help {
        println!("{USAGE}");
        return DONE;
    }
    #[cfg(unix)]
    {
        unix::run(asked)
    }
    #[cfg(not(unix))]
    {
        let _ = asked;
        eprintln!(
            "vix-mix ctl: the control socket is a Unix socket, and this platform has none: there is no Vix Mix to ask from here yet"
        );
        REFUSED
    }
}

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, Write};
    use std::path::PathBuf;
    use std::time::Duration;

    use vixmix_control::{ClientError, EVENT, GREETING, PROTOCOL, Session, socket_path_from_env};

    use super::{Asked, DONE, NO_MIXER, Place, REFUSED, What};
    use crate::control::{FAKE_PROFILE, profile_name};

    /// How long one request waits for its answer. A mixer that is scanning plugins holds its
    /// window's thread for longer than this; a shortcut should say so rather than hang.
    const PATIENCE: Duration = Duration::from_secs(5);

    pub fn run(asked: Asked) -> i32 {
        let path = match &asked.place {
            Place::Socket(path) => Ok(path.clone()),
            Place::Fake => socket_path_from_env(FAKE_PROFILE),
            Place::Profile => socket_path_from_env(&profile_name(false)),
        };
        let path: PathBuf = match path {
            Ok(path) => path,
            Err(problem) => {
                eprintln!("vix-mix ctl: {problem}");
                return NO_MIXER;
            }
        };
        let mut session = match Session::connect(&path, Some(PATIENCE)) {
            Ok(session) => session,
            Err(problem) => return failed(&problem),
        };
        if session.protocol() != PROTOCOL {
            eprintln!(
                "vix-mix ctl: the Vix Mix that is running speaks control protocol {} and this vix-mix speaks {PROTOCOL}: they are two builds; quit that Vix Mix and start this one, or ask it with its own `vix-mix ctl`",
                session.protocol()
            );
            return REFUSED;
        }
        match asked.what {
            What::Request { line, follows } => request(&mut session, &line, follows),
            What::Relay => relay(session),
            What::Help => DONE,
        }
    }

    /// Says why a connection failed, and which exit that is.
    fn failed(problem: &ClientError) -> i32 {
        eprintln!("vix-mix ctl: {problem}");
        match problem {
            ClientError::NoMixer { .. } => NO_MIXER,
            _ => REFUSED,
        }
    }

    /// One line on standard output; false when nobody reads it any more (a closed pipe).
    fn say(line: &str) -> bool {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{line}").and_then(|()| out.flush()).is_ok()
    }

    /// One request and its answer; a `watch` then prints every change until it is stopped.
    fn request(session: &mut Session, line: &str, follows: bool) -> i32 {
        let answer = match session.ask(line) {
            Ok(answer) => answer,
            Err(problem) => return failed(&problem),
        };
        for line in answer.lines.iter().chain(&answer.events) {
            if !say(line) {
                return DONE;
            }
        }
        if let Some(refused) = answer.error {
            eprintln!("vix-mix ctl: {refused}");
            return REFUSED;
        }
        if !follows {
            return DONE;
        }
        // From here on a line comes when something changes, however long that takes.
        if let Err(problem) = session.patience(None) {
            return failed(&problem);
        }
        loop {
            match session.line() {
                Ok(Some(line)) => {
                    if !say(line.strip_prefix(EVENT).unwrap_or(&line)) {
                        return DONE;
                    }
                }
                Ok(None) => {
                    eprintln!(
                        "vix-mix ctl: Vix Mix closed the connection: it has quit, so there is nothing more to follow"
                    );
                    return DONE;
                }
                Err(problem) => return failed(&problem),
            }
        }
    }

    /// Standard input to the socket, a line a request; everything the mixer says to standard
    /// output as it says it, the greeting first. When standard input ends the mixer answers
    /// what it was sent and closes, and so does this.
    fn relay(mut session: Session) -> i32 {
        let mut sender = match session.sender() {
            Ok(sender) => sender,
            Err(problem) => return failed(&problem),
        };
        if let Err(problem) = session.patience(None) {
            return failed(&problem);
        }
        if !say(&format!("{GREETING}{}", session.protocol())) {
            return DONE;
        }
        let sending = std::thread::Builder::new()
            .name("vix-mix-ctl-in".to_string())
            .spawn(move || {
                for line in std::io::stdin().lock().lines() {
                    let Ok(line) = line else { break };
                    if sender.send(&line).is_err() {
                        break;
                    }
                }
                sender.finish();
            });
        if let Err(problem) = sending {
            eprintln!("vix-mix ctl: no thread to read standard input with ({problem}); try again");
            return REFUSED;
        }
        loop {
            match session.line() {
                Ok(Some(line)) => {
                    if !say(&line) {
                        return DONE;
                    }
                }
                Ok(None) => return DONE,
                Err(problem) => return failed(&problem),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn the_words_after_ctl_are_one_request_to_the_mixer_this_build_runs() {
        assert_eq!(
            Asked::parse(&args(&["toggle", "/ch/mic/mute"])).unwrap(),
            Asked {
                place: Place::Profile,
                what: What::Request {
                    line: "toggle /ch/mic/mute".to_string(),
                    follows: false,
                },
            }
        );
        // A value is the words that follow, as they were written.
        let set = Asked::parse(&args(&["set", "/ch/music/monitor/db", "-12"])).unwrap();
        assert_eq!(
            set.what,
            What::Request {
                line: "set /ch/music/monitor/db -12".to_string(),
                follows: false,
            }
        );
        // An address alone reads, as it does on the socket; a watch goes on.
        assert!(matches!(
            Asked::parse(&args(&["/ch/1/mute"])).unwrap().what,
            What::Request { follows: false, .. }
        ));
        assert!(matches!(
            Asked::parse(&args(&["watch", "/"])).unwrap().what,
            What::Request { follows: true, .. }
        ));
    }

    #[test]
    fn which_mixer_is_said_before_the_request() {
        assert_eq!(
            Asked::parse(&args(&["--fake", "get", "/"])).unwrap().place,
            Place::Fake
        );
        assert_eq!(
            Asked::parse(&args(&["--socket", "/tmp/m.sock", "-"])).unwrap(),
            Asked {
                place: Place::Socket(PathBuf::from("/tmp/m.sock")),
                what: What::Relay,
            }
        );
        assert_eq!(Asked::parse(&args(&["-"])).unwrap().what, What::Relay);
        assert_eq!(Asked::parse(&args(&["--help"])).unwrap().what, What::Help);
    }

    #[test]
    fn what_is_not_a_request_says_what_one_is_without_asking_the_mixer() {
        for (words, expected) in [
            (vec![], "e.g. `vix-mix ctl toggle /ch/mic/mute`"),
            (vec!["--fake"], "wants a request after it"),
            (
                vec!["--socket"],
                "e.g. `--socket /run/user/1000/vix-mix/control.sock`",
            ),
            (
                vec!["mute", "/ch/1"],
                "get, set, toggle, nudge, watch, unwatch",
            ),
            (vec!["set", "/ch/1/mute"], "as `set /ch/1/mute true`"),
            (vec!["toggle"], "wants an address after it"),
            (
                vec!["get", "/ch/1/mute", "true"],
                "send `set /ch/1/mute true`",
            ),
        ] {
            let refused = Asked::parse(&args(&words)).expect_err("refused");
            assert!(refused.contains(expected), "{words:?} said `{refused}`");
        }
        assert_eq!(run(&args(&["mute", "/ch/1"])), NOT_A_REQUEST);
        assert_eq!(run(&[]), NOT_A_REQUEST);
    }

    #[test]
    fn the_usage_names_every_verb_and_every_exit() {
        for verb in vixmix_control::Verb::ALL {
            // `unwatch` is for a connection that stays, which only `-` is.
            if verb != vixmix_control::Verb::Unwatch {
                assert!(USAGE.contains(verb.word()), "{}", verb.word());
            }
        }
        for exit in [DONE, REFUSED, NOT_A_REQUEST, NO_MIXER] {
            assert!(USAGE.contains(&format!("{exit} ")), "exit {exit}");
        }
    }
}
