//! The clients, what each follows, and the two things the mixer's own thread does for them:
//! [`Service::serve`] carries out the requests that wait, and [`Service::publish`] tells the
//! followers what changed.
//!
//! **Two sides.** A [`Connector`] is what brings a client in: the socket's threads hold one,
//! and so does a test. It is `Send` and `Sync`; opening a client gives a [`Client`] whose
//! `requests` half takes lines and whose `answers` half gives the text to write back. The
//! [`Service`] is the other side, on the thread that owns the mixer (the UI thread, for a
//! `Host`): it reads what the clients sent and answers through each one's outbox.
//!
//! **Every queue is bounded.** At most [`MAX_CLIENTS`] are connected. The requests that wait
//! are at most [`INBOX`]: a client that sends faster than the mixer serves blocks on its own
//! thread, which is its own connection going slow. Each client has an outbox of [`OUTBOX`]
//! messages (an answer is one message whatever its length, and so are the events of one
//! change); the service never waits on one: a client that does not read what it is sent is
//! dropped, and the others are served. A client follows at most [`MAX_WATCHES`] addresses, and
//! a request line is at most [`MAX_LINE`] bytes.
//!
//! **One wake, however many requests.** The connector calls the wake it was made with only
//! when it queues something while nothing waited; [`Service::serve`] takes at most a budget
//! of requests and asks to be woken again when it stopped there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use vixmix_model::{Applied, Command, Refused, Setup};

use crate::address::covers;
use crate::request::{Plan, parse, plan};
use crate::tree::{Change, Leaf, diff, leaves};
use crate::{ERROR, EVENT, OK, greeting};

/// The most clients connected at once.
pub const MAX_CLIENTS: usize = 8;
/// The most bytes in one request line, its line end apart.
pub const MAX_LINE: usize = 4096;
/// The most requests waiting to be served.
pub const INBOX: usize = 256;
/// The most messages a client may leave unread before it is dropped.
pub const OUTBOX: usize = 64;
/// The most addresses one client follows.
pub const MAX_WATCHES: usize = 32;
/// How many requests one [`Service::serve`] takes when its caller has no other number.
pub const SERVE_BUDGET: usize = 64;

/// What applies a request: the mixer's setup, and the way a command is applied to it. The
/// app implements it over its `Host`; the model's own `apply` over a `Setup` is enough for a
/// test.
pub trait Target {
    /// The setup as it stands.
    fn setup(&self) -> &Setup;
    /// Applies one command, whole or not at all.
    fn apply(&mut self, command: Command) -> Result<Applied, Refused>;
}

/// One connected client.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClientId(u64);

enum Inbound {
    Opened {
        client: ClientId,
        outbox: SyncSender<String>,
    },
    Line {
        client: ClientId,
        line: String,
    },
    TooLong {
        client: ClientId,
    },
    Closed {
        client: ClientId,
    },
}

struct Shared {
    inbox: SyncSender<Inbound>,
    /// Set while something is queued that no wake has been answered for.
    waiting: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
    connected: AtomicUsize,
    next: AtomicU64,
}

impl Shared {
    /// Queues `inbound` and wakes the service unless a wake is already owed. False when the
    /// service is gone.
    fn push(&self, inbound: Inbound) -> bool {
        if self.inbox.send(inbound).is_err() {
            return false;
        }
        self.rouse();
        true
    }

    fn rouse(&self) {
        if !self.waiting.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }
}

/// How a client comes in. Cheap to clone; every clone brings clients to the same service.
#[derive(Clone)]
pub struct Connector {
    shared: Arc<Shared>,
}

/// Why a client was not taken: [`MAX_CLIENTS`] are connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

impl Full {
    /// The one line a refused connection is sent before it is closed.
    pub fn line() -> String {
        format!(
            "{ERROR}Vix Mix serves {MAX_CLIENTS} control connections at once and {MAX_CLIENTS} are open: close one and connect again"
        )
    }
}

impl Connector {
    /// Brings a client in: the service greets it with its first message. `Err(Full)` when
    /// [`MAX_CLIENTS`] are connected, and the service never hears of it.
    pub fn open(&self) -> Result<Client, Full> {
        let taken = self
            .shared
            .connected
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_CLIENTS).then_some(n + 1)
            });
        if taken.is_err() {
            return Err(Full);
        }
        let client = ClientId(self.shared.next.fetch_add(1, Ordering::Relaxed));
        let (outbox, answers) = sync_channel(OUTBOX);
        let requests = Requests {
            client,
            shared: Arc::clone(&self.shared),
        };
        self.shared.push(Inbound::Opened { client, outbox });
        Ok(Client { requests, answers })
    }

    /// How many clients are connected.
    pub fn connected(&self) -> usize {
        self.shared.connected.load(Ordering::Acquire)
    }
}

/// A connected client's two halves, for the two threads that serve its connection.
pub struct Client {
    /// Where its request lines go.
    pub requests: Requests,
    /// What to write back to it, each message whole lines ending in a line end. It ends
    /// (`recv` fails) when the service has dropped the client or is gone: close the
    /// connection then.
    pub answers: Receiver<String>,
}

/// Where a client's request lines go. Dropping it tells the service the client is gone.
pub struct Requests {
    client: ClientId,
    shared: Arc<Shared>,
}

impl Requests {
    /// Queues one request line, without its line end, and blocks while [`INBOX`] requests
    /// already wait. A line over [`MAX_LINE`] bytes is not queued: the service tells the
    /// client so and drops it. False when the service is gone: stop reading.
    pub fn send(&self, line: &str) -> bool {
        let client = self.client;
        if line.len() > MAX_LINE {
            return self.shared.push(Inbound::TooLong { client });
        }
        self.shared.push(Inbound::Line {
            client,
            line: line.to_string(),
        })
    }
}

impl Drop for Requests {
    fn drop(&mut self) {
        self.shared.connected.fetch_sub(1, Ordering::AcqRel);
        self.shared.push(Inbound::Closed {
            client: self.client,
        });
    }
}

struct Entry {
    id: ClientId,
    outbox: SyncSender<String>,
    /// The addresses it follows, as they are kept.
    watches: Vec<String>,
}

/// What one [`Service::serve`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Served {
    /// How many request lines it answered.
    pub requests: usize,
    /// Whether a command it applied changed the setup: the caller draws, and polls whatever
    /// reports the change, so the followers are told ([`Service::publish`]).
    pub changed: bool,
}

/// The clients and what each follows; see the module.
pub struct Service {
    inbox: Receiver<Inbound>,
    shared: Arc<Shared>,
    clients: Vec<Entry>,
    /// Every address with its value as the followers were last told; kept only while someone
    /// follows something.
    told: Vec<Leaf>,
}

/// A service, and the connector that brings clients to it. `wake` is called, from whichever
/// thread queued a request, when the service has something to serve and was not already
/// owed a wake; it must not block.
pub fn service(wake: impl Fn() + Send + Sync + 'static) -> (Service, Connector) {
    let (inbox, waiting) = sync_channel(INBOX);
    let shared = Arc::new(Shared {
        inbox,
        waiting: AtomicBool::new(false),
        wake: Box::new(wake),
        connected: AtomicUsize::new(0),
        next: AtomicU64::new(1),
    });
    let service = Service {
        inbox: waiting,
        shared: Arc::clone(&shared),
        clients: Vec::new(),
        told: Vec::new(),
    };
    (service, Connector { shared })
}

impl Service {
    /// Carries out what waits, at most `budget` of it, against `target`: each client that
    /// came is greeted, each request answered with its lines and one `ok` or `error:`, each
    /// client that went forgotten. When it stops at the budget it asks to be woken again.
    pub fn serve(&mut self, budget: usize, target: &mut dyn Target) -> Served {
        // From here on, whatever is queued wakes again.
        self.shared.waiting.store(false, Ordering::Release);
        let mut served = Served::default();
        let mut taken = 0;
        while taken < budget {
            let Ok(inbound) = self.inbox.try_recv() else {
                return served;
            };
            taken += 1;
            match inbound {
                Inbound::Opened { client, outbox } => {
                    self.clients.push(Entry {
                        id: client,
                        outbox,
                        watches: Vec::new(),
                    });
                    self.say(client, format!("{}\n", greeting()));
                }
                Inbound::Closed { client } => self.forget(client),
                Inbound::TooLong { client } => {
                    self.say(
                        client,
                        format!(
                            "{ERROR}a request is one line of at most {MAX_LINE} bytes and this one is longer: send `verb address [value]`, one request a line, and connect again\n"
                        ),
                    );
                    self.forget(client);
                }
                Inbound::Line { client, line } => {
                    if self.clients.iter().any(|c| c.id == client) {
                        served.requests += 1;
                        let (answer, changed) = self.answer(client, &line, target);
                        served.changed |= changed;
                        if let Some(answer) = answer {
                            self.say(client, answer);
                        }
                    }
                }
            }
        }
        // The budget is spent and more may wait.
        self.shared.rouse();
        served
    }

    /// Tells every follower what changed since they were last told, each the changes at or
    /// under what it follows as `event` lines in one message; something that went is also
    /// told to whoever follows an address under it. Call it when the setup may have changed
    /// (the host's `SetupChanged`); with no follower it does nothing.
    pub fn publish(&mut self, setup: &Setup) {
        if !self.followed() {
            return;
        }
        let now = leaves(setup);
        let changes = diff(&self.told, &now);
        self.told = now;
        if changes.is_empty() {
            return;
        }
        let messages: Vec<(ClientId, String)> = self
            .clients
            .iter()
            .filter_map(|client| {
                let text: String = changes
                    .iter()
                    .filter(|change| hears(&client.watches, change))
                    .map(|change| format!("{EVENT}{change}\n"))
                    .collect();
                (!text.is_empty()).then_some((client.id, text))
            })
            .collect();
        for (client, text) in messages {
            self.say(client, text);
        }
    }

    /// Whether any client follows anything: only then is there someone to [`publish`] to.
    ///
    /// [`publish`]: Service::publish
    pub fn followed(&self) -> bool {
        self.clients.iter().any(|c| !c.watches.is_empty())
    }

    /// How many clients the service knows.
    pub fn clients(&self) -> usize {
        self.clients.len()
    }

    /// The answer to one request line (`None` for a blank one), and whether it changed the
    /// setup.
    fn answer(
        &mut self,
        client: ClientId,
        line: &str,
        target: &mut dyn Target,
    ) -> (Option<String>, bool) {
        let request = match parse(line) {
            Ok(Some(request)) => request,
            Ok(None) => return (None, false),
            Err(refused) => return (Some(error(&refused)), false),
        };
        let asked = match plan(target.setup(), &request) {
            Ok(asked) => asked,
            Err(refused) => return (Some(error(&refused)), false),
        };
        match asked {
            Plan::Read(places) => (Some(listed(&leaves(target.setup()), &places)), false),
            Plan::Watch(places) => (Some(self.watch(client, places, target.setup())), false),
            Plan::Unwatch(places) => {
                if let Some(entry) = self.clients.iter_mut().find(|c| c.id == client) {
                    entry.watches.retain(|watch| !places.contains(watch));
                }
                if !self.followed() {
                    self.told.clear();
                }
                (Some(format!("{OK}\n")), false)
            }
            Plan::Write(commands) => {
                let before = leaves(target.setup());
                let mut changed = false;
                let mut refused = None;
                for command in commands {
                    match target.apply(command) {
                        Ok(applied) => changed |= applied.changed(),
                        Err(why) => {
                            refused = Some(why.to_string());
                            break;
                        }
                    }
                }
                // What the commands changed, as it reads now: a linked fader's other send
                // with the one that was set.
                let mut answer: String = diff(&before, &leaves(target.setup()))
                    .iter()
                    .map(|change| format!("{change}\n"))
                    .collect();
                match refused {
                    Some(why) => answer.push_str(&error(&why)),
                    None => answer.push_str(&format!("{OK}\n")),
                }
                (Some(answer), changed)
            }
        }
    }

    /// Adds `places` to what `client` follows and answers with what is there now.
    fn watch(&mut self, client: ClientId, places: Vec<String>, setup: &Setup) -> String {
        let first = !self.followed();
        let Some(entry) = self.clients.iter_mut().find(|c| c.id == client) else {
            return String::new();
        };
        let new: Vec<String> = places
            .iter()
            .filter(|place| !entry.watches.contains(place))
            .cloned()
            .collect();
        if entry.watches.len() + new.len() > MAX_WATCHES {
            return error(&format!(
                "a connection follows at most {MAX_WATCHES} addresses and this would make {}: follow a place above them, as `watch /ch` or `watch /`, or `unwatch` one first",
                entry.watches.len() + new.len()
            ));
        }
        entry.watches.extend(new);
        let now = leaves(setup);
        let answer = listed(&now, &places);
        // The first follower starts from the setup as it stands. With followers already,
        // what they were last told stays: a change not yet told is still owed to them.
        if first && self.followed() {
            self.told = now;
        }
        answer
    }

    /// Sends `text` to `client`, or drops the client when its outbox is full or gone: the
    /// service never waits for a client to read.
    fn say(&mut self, client: ClientId, text: String) {
        let Some(entry) = self.clients.iter().find(|c| c.id == client) else {
            return;
        };
        match entry.outbox.try_send(text) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                log::warn!(
                    "a control connection left {OUTBOX} messages unread and was closed: a client must read what it is sent, events included"
                );
                self.forget(client);
            }
            Err(TrySendError::Disconnected(_)) => self.forget(client),
        }
    }

    /// Forgets `client`: its outbox closes, which ends its connection.
    fn forget(&mut self, client: ClientId) {
        self.clients.retain(|c| c.id != client);
        if !self.followed() {
            self.told.clear();
        }
    }
}

/// Whether a client that follows `watches` is told `change`: the change is at or under one of
/// them, or something went that one of them is under.
fn hears(watches: &[String], change: &Change) -> bool {
    watches.iter().any(|watch| {
        covers(watch, change.address())
            || (matches!(change, Change::Gone(_)) && covers(change.address(), watch))
    })
}

/// An answer that lists every value at or under `places`, each once in the table's order.
fn listed(table: &[Leaf], places: &[String]) -> String {
    let mut answer: String = table
        .iter()
        .filter(|leaf| places.iter().any(|place| covers(place, &leaf.address)))
        .map(|leaf| format!("{leaf}\n"))
        .collect();
    answer.push_str(OK);
    answer.push('\n');
    answer
}

/// The line that ends an answer that was refused. A message never spans lines.
fn error(message: &str) -> String {
    format!("{ERROR}{}\n", message.replace(['\n', '\r'], " "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::{ChannelId, MixId, apply, example};

    /// The model alone as a target, keeping what was applied.
    struct Desk {
        setup: Setup,
        applied: Vec<Command>,
    }

    impl Desk {
        fn example() -> Desk {
            Desk {
                setup: example::setup(),
                applied: Vec::new(),
            }
        }
    }

    impl Target for Desk {
        fn setup(&self) -> &Setup {
            &self.setup
        }
        fn apply(&mut self, command: Command) -> Result<Applied, Refused> {
            self.applied.push(command.clone());
            apply(&mut self.setup, command)
        }
    }

    /// A service whose wakes are counted, its connector, and a target.
    struct Rig {
        service: Service,
        connector: Connector,
        wakes: Arc<AtomicUsize>,
        desk: Desk,
    }

    impl Rig {
        fn new() -> Rig {
            let wakes = Arc::new(AtomicUsize::new(0));
            let counted = Arc::clone(&wakes);
            let (service, connector) = service(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            });
            Rig {
                service,
                connector,
                wakes,
                desk: Desk::example(),
            }
        }

        /// A client, greeted.
        fn client(&mut self) -> Client {
            let client = self.connector.open().expect("room for a client");
            self.serve();
            assert_eq!(said(&client), [greeting()]);
            client
        }

        fn serve(&mut self) -> Served {
            self.service.serve(SERVE_BUDGET, &mut self.desk)
        }

        /// One request and its answer's lines.
        fn ask(&mut self, client: &Client, line: &str) -> Vec<String> {
            assert!(client.requests.send(line));
            self.serve();
            said(client)
        }
    }

    /// Every line `client` was sent and has not read.
    fn said(client: &Client) -> Vec<String> {
        client
            .answers
            .try_iter()
            .flat_map(|message| {
                assert!(message.ends_with('\n'), "{message:?} is whole lines");
                message.lines().map(str::to_string).collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn a_client_is_greeted_and_each_request_ends_in_exactly_one_ok_or_error() {
        let mut rig = Rig::new();
        let client = rig.client();
        for line in [
            "get /ch/mic/master",
            "set /ch/mic/monitor/mute true",
            "set /ch/mic/monitor/mute true",
            "toggle /mix/master/preview",
            "nudge /mix/monitor/db -2",
            "watch /ch/1",
            "unwatch /ch/1",
            "set /ch/9/mute true",
            "mute everything",
            "set /ch/1/monitor/position 2",
            "set /ch/1/name \"Voice\"",
        ] {
            let lines = rig.ask(&client, line);
            let ends: Vec<&String> = lines
                .iter()
                .filter(|l| *l == OK || l.starts_with(ERROR))
                .collect();
            assert_eq!(ends.len(), 1, "{line} answered {lines:?}");
            assert_eq!(
                lines.last(),
                ends.first().copied(),
                "{line} answered {lines:?}"
            );
            assert!(
                lines.iter().all(|l| !l.starts_with(EVENT)),
                "{line}: {lines:?}"
            );
        }
        // A blank line is no request: nothing is said.
        assert_eq!(rig.ask(&client, "   "), Vec::<String>::new());
    }

    #[test]
    fn a_read_prints_the_values_and_a_write_what_it_changed() {
        let mut rig = Rig::new();
        let client = rig.client();
        assert_eq!(
            rig.ask(&client, "get /ch/music/master"),
            [
                "/ch/3/master/mute true",
                "/ch/3/master/db -11.14",
                "/ch/3/master/position 0.58",
                "/ch/3/master/processed true",
                "ok",
            ]
        );
        // The channel's mute is both sends; Music's Master send is muted already.
        assert_eq!(
            rig.ask(&client, "toggle /ch/music/mute"),
            ["/ch/3/mute true", "/ch/3/monitor/mute true", "ok"]
        );
        // Game is linked: the other send moves with the one set.
        assert_eq!(
            rig.ask(&client, "set /ch/game/monitor/db 0"),
            [
                "/ch/2/monitor/db 0",
                "/ch/2/monitor/position 0.84",
                "/ch/2/master/db -4.29",
                "/ch/2/master/position 0.74",
                "ok",
            ]
        );
        // Written to what it already is: carried out, and nothing changed.
        assert_eq!(rig.ask(&client, "set /ch/game/monitor/db 0"), ["ok"]);
        // A strip's plugins, and a strip with none.
        assert_eq!(
            rig.ask(&client, "get /ch/music/fx"),
            [
                "/fx/4/name \"OTT\"",
                "/fx/4/strip \"/ch/3\"",
                "/fx/4/slot 1",
                "/fx/4/enabled false",
                "ok",
            ]
        );
        assert_eq!(rig.ask(&client, "get /ch/game/fx"), ["ok"]);
    }

    #[test]
    fn a_request_applies_the_commands_and_says_whether_the_setup_changed() {
        let mut rig = Rig::new();
        let client = rig.client();
        assert!(client.requests.send("set /ch/mic/master/mute true"));
        let served = rig.serve();
        assert_eq!(
            served,
            Served {
                requests: 1,
                changed: true
            }
        );
        assert_eq!(
            rig.desk.applied,
            [Command::SetMute {
                ch: ChannelId(1),
                mix: MixId::Master,
                mute: true,
            }]
        );
        // The same again: applied, and nothing changed.
        assert!(client.requests.send("set /ch/mic/master/mute true"));
        assert!(!rig.serve().changed);
        // A read changes nothing and applies nothing.
        assert!(client.requests.send("get /"));
        assert!(!rig.serve().changed);
        assert_eq!(rig.desk.applied.len(), 2);
    }

    #[test]
    fn a_refused_request_changes_nothing_and_ends_with_what_to_send_instead() {
        let mut rig = Rig::new();
        let client = rig.client();
        let before = rig.desk.setup.clone();
        for (line, expected) in [
            (
                "set /ch/9/mute true",
                "there is no channel 9: the channels are",
            ),
            (
                "mute everything",
                "one of get, set, toggle, nudge, watch, unwatch",
            ),
            // The model's own refusal, word for word.
            (
                "set /ch/1/monitor/position 2",
                "clamp it to 0..=1 before sending",
            ),
            (
                "set /ch/1/name \"Voice\"",
                "a channel is renamed in the window",
            ),
        ] {
            let lines = rig.ask(&client, line);
            assert_eq!(lines.len(), 1, "{line} answered {lines:?}");
            assert!(
                lines[0].starts_with(ERROR) && lines[0].contains(expected),
                "{line} answered {lines:?}"
            );
        }
        assert_eq!(rig.desk.setup, before);
    }

    #[test]
    fn a_follower_hears_a_change_it_did_not_make_and_only_under_what_it_follows() {
        let mut rig = Rig::new();
        let (follower, other) = (rig.client(), rig.client());
        // Following answers with what is there.
        assert_eq!(
            rig.ask(&follower, "watch /ch/mic/master/mute"),
            ["/ch/1/master/mute false", "ok"]
        );
        // The other client mutes Mic's Master send and Music's Monitor send.
        assert_eq!(
            rig.ask(&other, "set /ch/mic/master/mute true"),
            ["/ch/1/master/mute true", "ok"]
        );
        assert_eq!(
            rig.ask(&other, "toggle /ch/music/monitor/mute")
                .last()
                .unwrap(),
            "ok"
        );
        // Nothing is said until the mixer publishes; then the follower hears Mic and not
        // Music, and the other, which follows nothing, hears nothing.
        assert_eq!(said(&follower), Vec::<String>::new());
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&follower), ["event /ch/1/master/mute true"]);
        assert_eq!(said(&other), Vec::<String>::new());
        // A change made past every client (a gesture in the window) is heard the same way.
        let _ = apply(
            &mut rig.desk.setup,
            Command::SetMute {
                ch: ChannelId(1),
                mix: MixId::Master,
                mute: false,
            },
        )
        .unwrap();
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&follower), ["event /ch/1/master/mute false"]);
        // Published again with nothing changed: nothing said.
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&follower), Vec::<String>::new());
        // Unfollowed: no longer told.
        assert_eq!(rig.ask(&follower, "unwatch /ch/1/master/mute"), ["ok"]);
        assert!(!rig.service.followed());
        let _ = apply(&mut rig.desk.setup, Command::SetPreview(true)).unwrap();
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&follower), Vec::<String>::new());
    }

    #[test]
    fn a_follower_that_makes_a_change_is_answered_first_and_told_after() {
        let mut rig = Rig::new();
        let client = rig.client();
        assert_eq!(rig.ask(&client, "watch /mix").last().unwrap(), "ok");
        assert_eq!(
            rig.ask(&client, "toggle /mix/master/mute"),
            ["/mix/master/mute true", "ok"]
        );
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&client), ["event /mix/master/mute true"]);
    }

    #[test]
    fn a_follower_is_told_where_things_stand_not_every_step_on_the_way() {
        let mut rig = Rig::new();
        let (follower, other) = (rig.client(), rig.client());
        assert_eq!(rig.ask(&follower, "watch /mix").last().unwrap(), "ok");
        // Muted and unmuted again before the mixer publishes: the value is where the
        // follower was last told it is, so nothing is said.
        assert_eq!(
            rig.ask(&other, "toggle /mix/master/mute").last().unwrap(),
            "ok"
        );
        assert_eq!(
            rig.ask(&other, "toggle /mix/master/mute").last().unwrap(),
            "ok"
        );
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&follower), Vec::<String>::new());
        // Moved three times before the mixer publishes: told once, as it reads then.
        for db in ["-3", "-6", "-12"] {
            let line = format!("set /mix/master/db {db}");
            assert_eq!(rig.ask(&other, &line).last().unwrap(), "ok");
        }
        rig.service.publish(&rig.desk.setup);
        assert_eq!(
            said(&follower),
            [
                "event /mix/master/db -12",
                "event /mix/master/position 0.56"
            ]
        );
    }

    #[test]
    fn a_second_follower_does_not_take_a_change_owed_to_the_first() {
        let mut rig = Rig::new();
        let (first, second) = (rig.client(), rig.client());
        assert_eq!(
            rig.ask(&first, "watch /mix/master/preview").last().unwrap(),
            "ok"
        );
        // The ear is pressed and not yet published when the second starts following.
        let _ = apply(&mut rig.desk.setup, Command::SetPreview(true)).unwrap();
        assert_eq!(
            rig.ask(&second, "watch /mix/master/preview"),
            ["/mix/master/preview true", "ok"]
        );
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&first), ["event /mix/master/preview true"]);
    }

    #[test]
    fn something_that_went_is_told_as_gone_to_who_follows_it_or_anything_under_it() {
        let mut rig = Rig::new();
        let (all, one, plugin, other) = (rig.client(), rig.client(), rig.client(), rig.client());
        assert_eq!(rig.ask(&all, "watch /ch").last().unwrap(), "ok");
        assert_eq!(
            rig.ask(&one, "watch /ch/music/master/mute").last().unwrap(),
            "ok"
        );
        assert_eq!(
            rig.ask(&plugin, "watch /ch/music/fx/1/enabled")
                .last()
                .unwrap(),
            "ok"
        );
        assert_eq!(rig.ask(&other, "watch /ch/mic").last().unwrap(), "ok");
        // Music is removed in the window, with its one plugin.
        let removed = rig.desk.setup.channels[2].clone();
        let _ = apply(&mut rig.desk.setup, Command::RemoveChannel(ChannelId(3))).unwrap();
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&all), ["event /ch/3 gone"]);
        assert_eq!(said(&one), ["event /ch/3 gone"]);
        assert_eq!(said(&plugin), ["event /fx/4 gone"]);
        assert_eq!(said(&other), Vec::<String>::new());
        // Undone: the channel is back under its id, and who followed it is told its values.
        let _ = apply(
            &mut rig.desk.setup,
            Command::RestoreChannel {
                at: 2,
                channel: removed,
            },
        )
        .unwrap();
        rig.service.publish(&rig.desk.setup);
        assert_eq!(said(&one), ["event /ch/3/master/mute true"]);
        assert_eq!(said(&plugin), ["event /fx/4/enabled false"]);
        assert_eq!(said(&all).len(), 12, "Music's twelve values");
    }

    #[test]
    fn a_client_that_does_not_read_is_dropped_and_the_others_are_served() {
        let mut rig = Rig::new();
        let (deaf, other) = (rig.client(), rig.client());
        // The deaf client asks and never reads: one answer over its outbox drops it.
        for _ in 0..=OUTBOX {
            assert!(deaf.requests.send("get /mix/master/mute"));
            rig.serve();
        }
        assert_eq!(rig.service.clients(), 1);
        // Its outbox is closed behind what it was sent, which is what ends its connection.
        assert_eq!(deaf.answers.iter().count(), OUTBOX);
        // What it sends now is not answered, and the other client is.
        assert!(deaf.requests.send("get /mix/master/mute"));
        assert_eq!(rig.serve().requests, 0);
        assert_eq!(
            rig.ask(&other, "get /mix/master/mute"),
            ["/mix/master/mute false", "ok"]
        );
    }

    #[test]
    fn a_ninth_connection_is_refused_until_one_closes() {
        let mut rig = Rig::new();
        let clients: Vec<Client> = (0..MAX_CLIENTS).map(|_| rig.client()).collect();
        assert_eq!(rig.connector.connected(), MAX_CLIENTS);
        assert!(matches!(rig.connector.open(), Err(Full)));
        assert!(
            Full::line().starts_with(ERROR) && Full::line().contains("close one and connect again"),
            "{}",
            Full::line()
        );
        drop(clients);
        rig.serve();
        assert_eq!(rig.service.clients(), 0);
        let _again = rig.client();
    }

    #[test]
    fn a_flood_of_requests_wakes_once_and_a_drain_stops_at_its_budget() {
        let mut rig = Rig::new();
        let client = rig.client();
        let woken = rig.wakes.load(Ordering::SeqCst);
        // Fewer than an outbox holds: the client reads only at the end.
        let flood = OUTBOX - 4;
        for _ in 0..flood {
            assert!(client.requests.send("get /mix/master/mute"));
        }
        assert_eq!(
            rig.wakes.load(Ordering::SeqCst),
            woken + 1,
            "one wake for {flood} requests"
        );
        // A drain takes its budget and asks to be woken for the rest.
        let served = rig.service.serve(10, &mut rig.desk);
        assert_eq!(served.requests, 10);
        assert_eq!(rig.wakes.load(Ordering::SeqCst), woken + 2);
        // The rest in one drain, which takes all there is and asks for nothing more.
        assert_eq!(rig.service.serve(1000, &mut rig.desk).requests, flood - 10);
        assert_eq!(rig.wakes.load(Ordering::SeqCst), woken + 2);
        assert_eq!(said(&client).len(), 2 * flood, "a value and an ok for each");
        // Served dry: the next request wakes again.
        assert!(client.requests.send("get /mix/master/mute"));
        assert_eq!(rig.wakes.load(Ordering::SeqCst), woken + 3);
    }

    #[test]
    fn a_line_past_the_limit_is_refused_and_its_connection_closed() {
        let mut rig = Rig::new();
        let client = rig.client();
        let long = format!("get /{}", "x".repeat(MAX_LINE));
        assert!(client.requests.send(&long));
        assert_eq!(rig.serve().requests, 0);
        let lines: Vec<String> = client
            .answers
            .iter()
            .flat_map(|m| m.lines().map(str::to_string).collect::<Vec<_>>())
            .collect();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with(ERROR) && lines[0].contains("one request a line"),
            "{lines:?}"
        );
        assert_eq!(rig.service.clients(), 0);
        // A line of exactly the limit is a request like any other.
        let client = rig.client();
        let most = format!("get /{}", "x".repeat(MAX_LINE - 5));
        assert_eq!(most.len(), MAX_LINE);
        assert!(rig.ask(&client, &most)[0].starts_with(ERROR));
        assert_eq!(rig.service.clients(), 1);
    }

    #[test]
    fn a_client_follows_at_most_so_many_addresses_and_is_told_the_way_out() {
        let mut rig = Rig::new();
        let client = rig.client();
        // Each of Mic's twelve values, then Game's, then Music's: thirty-six.
        let addresses: Vec<String> = leaves(&rig.desk.setup)
            .into_iter()
            .map(|l| l.address)
            .take(MAX_WATCHES + 4)
            .collect();
        for address in &addresses[..MAX_WATCHES] {
            assert_eq!(
                rig.ask(&client, &format!("watch {address}"))
                    .last()
                    .unwrap(),
                "ok"
            );
        }
        let refused = rig.ask(&client, &format!("watch {}", addresses[MAX_WATCHES]));
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(
            refused[0].starts_with(ERROR) && refused[0].contains("as `watch /ch` or `watch /`"),
            "{refused:?}"
        );
        // Followed already: asked again it is no new address.
        assert_eq!(
            rig.ask(&client, &format!("watch {}", addresses[0]))
                .last()
                .unwrap(),
            "ok"
        );
        // One let go makes room.
        assert_eq!(
            rig.ask(&client, &format!("unwatch {}", addresses[0])),
            ["ok"]
        );
        assert_eq!(
            rig.ask(&client, &format!("watch {}", addresses[MAX_WATCHES]))
                .last()
                .unwrap(),
            "ok"
        );
    }

    #[test]
    fn a_client_that_went_is_forgotten_and_a_service_that_went_stops_its_clients() {
        let mut rig = Rig::new();
        let client = rig.client();
        assert_eq!(rig.ask(&client, "watch /").last().unwrap(), "ok");
        assert!(rig.service.followed());
        drop(client);
        rig.serve();
        assert_eq!(rig.service.clients(), 0);
        assert!(!rig.service.followed());
        assert_eq!(rig.connector.connected(), 0);
        // The service gone: a client's next line says so, and its answers end.
        let client = rig.client();
        let Rig { service, .. } = rig;
        drop(service);
        assert!(!client.requests.send("get /"));
        assert_eq!(client.answers.iter().count(), 0);
    }
}
