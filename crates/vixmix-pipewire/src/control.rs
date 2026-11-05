//! The control thread, `vix-mix-pipewire`: the main loop, the context, the core, the
//! registry, the `default` metadata and every stream, none of which leaves this thread.
//!
//! **Notes and safe points.** PipeWire calls back into us from inside its own calls (a stream
//! reports `Connecting` from inside `connect`, a proxy's events arrive while we hold it), and
//! destroying an object from inside its own callback is not allowed. So every callback only
//! queues a [`Note`] and kicks the loop; the notes are handled at a safe point — the kick, the
//! host's wake, or the housekeeping timer, all loop sources rather than object callbacks —
//! with the state borrowed once. Notes raised while handling are handled in the same pass.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

use pipewire::client::{Client, ClientListener};
use pipewire::context::ContextRc;
use pipewire::core::CoreRc;
use pipewire::main_loop::{MainLoopRc, MainLoopWeak};
use pipewire::metadata::{Metadata, MetadataListener};
use pipewire::node::{Node, NodeListener};
use pipewire::properties::{PropertiesBox, properties};
use pipewire::registry::{GlobalObject, RegistryRc};
use pipewire::spa::utils::result::AsyncSeq;
use vixmix_model::{ChannelId, DeviceInfo, MAX_CHANNELS};

use crate::registry::{self, Global};
use crate::rt::{self, ChannelShared, PlanPort, Role, Shared, Tap, TickPlan};
use crate::streams::{Factory, Live, Notify, Phase, Spec};
use crate::{BackendConfig, BackendError, BackendEvent, BackendGraph, GraphInput, Inbox};

/// How often the control thread reports the data thread's counters, sends waiting plans and
/// events, and retries what is due.
const HOUSEKEEPING: Duration = Duration::from_millis(20);
/// Notes waiting for a safe point; more is a runaway, and the excess is dropped.
const MAX_NOTES: usize = 4096;
/// Events waiting for room in the host's channel.
const MAX_BACKLOG: usize = 1024;
/// Routings waiting for the `default` metadata to be bound.
const MAX_PENDING_ROUTES: usize = 256;
/// How long after an app stream appears a routing written over the host's is put back, and
/// how often at most.
const RESTORE_WINDOW: Duration = Duration::from_secs(3);
const MAX_REWRITES: u8 = 3;
/// Reconnecting to a lost daemon: the first wait, doubled on each failure up to the last.
const BACKOFF_FIRST: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
/// Connecting a stream the session manager gave up on again, while its device is present.
const STREAM_RETRY: Duration = Duration::from_secs(1);

/// What the control thread is started with.
pub(crate) struct Start {
    pub config: BackendConfig,
    pub shared: Arc<Shared>,
    pub plans: PlanPort,
    pub events: SyncSender<BackendEvent>,
    pub inbox: Arc<Inbox>,
    pub wake: pipewire::channel::Receiver<()>,
    pub ready: SyncSender<Result<(), BackendError>>,
}

/// Something that happened, queued by a callback and handled at the next safe point.
enum Note {
    /// The host posted to the mailbox.
    Wake,
    Housekeeping,
    CoreDone(AsyncSeq),
    CoreError {
        id: u32,
        res: i32,
        message: String,
    },
    Global(GlobalObject<PropertiesBox>),
    GlobalRemove(u32),
    NodeInfo {
        id: u32,
        props: PropertiesBox,
    },
    ClientInfo {
        id: u32,
        props: PropertiesBox,
    },
    Metadata {
        subject: u32,
        key: Option<String>,
        value: Option<String>,
    },
    Stream {
        serial: u64,
        connection: u64,
        phase: Phase,
    },
}

/// The thread's body: runs the main loop until the host disconnects.
pub(crate) fn run(start: Start) {
    let Start {
        config,
        shared,
        plans,
        events,
        inbox,
        wake,
        ready,
    } = start;
    let refused = |what, source| BackendError::PipeWire { what, source };
    let mainloop = match MainLoopRc::new(None) {
        Ok(mainloop) => mainloop,
        Err(error) => {
            let _ = ready.send(Err(refused("create its main loop", error)));
            return;
        }
    };
    // One data loop, so every `process` of ours runs on one thread whatever the user's
    // client.conf says (PipeWire releases with several data loops read this key).
    let context = match ContextRc::new(
        &mainloop,
        Some(properties! { "context.num-data-loops" => "1" }),
    ) {
        Ok(context) => context,
        Err(error) => {
            let _ = ready.send(Err(refused("create its context", error)));
            return;
        }
    };
    let (kick, kick_rx) = pipewire::channel::channel::<()>();
    let ctl = Rc::new(Ctl {
        state: RefCell::new(Control {
            config,
            shared,
            plans,
            events: Events {
                tx: events,
                backlog: VecDeque::new(),
                closed: false,
            },
            inbox,
            mainloop: mainloop.downgrade(),
            context,
            conn: None,
            graph: None,
            reconnect_at: None,
            backoff: BACKOFF_FIRST,
            next_serial: 1,
            slots: [None; MAX_CHANNELS],
            pending: Pending::default(),
            seen: Seen::default(),
        }),
        notes: RefCell::new(VecDeque::new()),
        pumping: Cell::new(false),
        kick,
        kicked: Cell::new(false),
    });

    let opened = ctl.state.borrow_mut().open(&ctl);
    if let Err(error) = opened {
        let _ = ready.send(Err(refused("connect to the daemon", error)));
        return;
    }
    let _ = ready.send(Ok(()));

    let loop_ = mainloop.loop_();
    let _kicks = kick_rx.attach(loop_, {
        let ctl = Rc::downgrade(&ctl);
        move |()| {
            if let Some(ctl) = ctl.upgrade() {
                ctl.pump();
            }
        }
    });
    // The host's wake only queues: its channel's lock is held while this runs, and a host
    // posting again would wait on it for as long as a pass takes.
    let _wakes = wake.attach(loop_, {
        let ctl = Rc::downgrade(&ctl);
        move |()| {
            if let Some(ctl) = ctl.upgrade() {
                ctl.note(Note::Wake);
            }
        }
    });
    let timer = loop_.add_timer({
        let ctl = Rc::downgrade(&ctl);
        move |_| {
            if let Some(ctl) = ctl.upgrade() {
                ctl.queue(Note::Housekeeping);
                ctl.pump();
            }
        }
    });
    let _ = timer.update_timer(Some(HOUSEKEEPING), Some(HOUSEKEEPING));
    // Whatever the first registry pass queued.
    ctl.pump();

    mainloop.run();

    // Streams first (each leaves the data loop before its listeners go), then the rest.
    ctl.state.borrow_mut().close();
    drop(timer);
}

/// The control thread's state with its note queue.
struct Ctl {
    state: RefCell<Control>,
    notes: RefCell<VecDeque<Note>>,
    pumping: Cell<bool>,
    kick: pipewire::channel::Sender<()>,
    kicked: Cell<bool>,
}

impl Ctl {
    /// From a callback: queue `note` and make sure a safe point comes.
    fn note(&self, note: Note) {
        self.queue(note);
        if !self.kicked.replace(true) {
            let _ = self.kick.send(());
        }
    }

    fn queue(&self, note: Note) {
        let mut notes = self.notes.borrow_mut();
        if notes.len() < MAX_NOTES {
            notes.push_back(note);
        } else {
            log::warn!("vix-mix-pipewire: {MAX_NOTES} notes waiting; dropping one");
        }
    }

    /// At a safe point: handle every queued note, and those raised while handling.
    ///
    /// No kick is sent while pumping: the notes raised now are handled in this pass, and the
    /// kick channel's callback (one of the safe points) holds that channel's lock while it
    /// runs, so a kick sent from inside it would wait on itself.
    fn pump(self: &Rc<Ctl>) {
        if self.pumping.replace(true) {
            return;
        }
        self.kicked.set(true);
        // A bounded pass: notes that keep raising notes must not starve the loop (the host's
        // wake among it), so what is left after one pass waits for the next kick.
        for _ in 0..MAX_NOTES {
            let next = self.notes.borrow_mut().pop_front();
            let Some(note) = next else {
                break;
            };
            self.state.borrow_mut().handle(self, note);
        }
        self.kicked.set(false);
        self.pumping.set(false);
        if !self.notes.borrow().is_empty() {
            self.kicked.set(true);
            let _ = self.kick.send(());
        }
    }
}

/// `Weak<Ctl>` → a callback that queues a note.
fn noter(ctl: &Rc<Ctl>) -> impl Fn(Note) + Clone + 'static {
    let weak: Weak<Ctl> = Rc::downgrade(ctl);
    move |note| {
        if let Some(ctl) = weak.upgrade() {
            ctl.note(note);
        }
    }
}

/// The host's event channel with a bounded backlog: the control thread never blocks on it.
struct Events {
    tx: SyncSender<BackendEvent>,
    backlog: VecDeque<BackendEvent>,
    closed: bool,
}

impl Events {
    fn send(&mut self, event: BackendEvent) {
        self.flush();
        if self.closed {
            return;
        }
        if self.backlog.is_empty() {
            match self.tx.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(event)) => self.backlog.push_back(event),
                Err(TrySendError::Disconnected(_)) => self.closed = true,
            }
        } else if self.backlog.len() < MAX_BACKLOG {
            self.backlog.push_back(event);
        } else {
            log::warn!("vix-mix-pipewire: the host is {MAX_BACKLOG} events behind; dropping one");
        }
    }

    fn flush(&mut self) {
        while let Some(event) = self.backlog.pop_front() {
            match self.tx.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(event)) => {
                    self.backlog.push_front(event);
                    return;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.closed = true;
                    self.backlog.clear();
                    return;
                }
            }
        }
    }
}

/// The tick's inputs as the control thread wants them, waiting to be sent as a plan.
#[derive(Default)]
struct Pending {
    taps: [Option<Tap>; MAX_CHANNELS],
    replace: [bool; MAX_CHANNELS],
    order: [Option<usize>; MAX_CHANNELS],
    count: usize,
    dirty: bool,
}

/// What the host has been told of the data thread's counters.
#[derive(Default)]
struct Seen {
    connected: bool,
    /// The host has been told the tick source since the connection opened.
    tick_source: bool,
    ticks_at_open: u64,
    quantum: u32,
    xruns: u64,
}

/// One channel's input: its spec (`None`: a capture with no device yet) and its stream.
struct ChannelStream {
    spec: Option<Spec>,
    live: Option<Live>,
    channel: Option<Arc<ChannelShared>>,
}

/// An app stream bound for its full properties. The listener goes before the proxy.
struct AppEntry {
    _listener: NodeListener,
    _node: Node,
    /// Its full properties, once the node's info arrived.
    props: Option<PropertiesBox>,
    announced: bool,
    /// When the registry announced it: the window in which a session manager restores a
    /// routing for it ([`RESTORE_WINDOW`]).
    appeared: Instant,
    /// The routing the host asked for it, and how often it was written again since.
    wish: Option<Option<String>>,
    rewritten: u8,
}

/// A client bound for its full properties. The listener goes before the proxy.
struct ClientEntry {
    _listener: ClientListener,
    _client: Client,
    props: Option<PropertiesBox>,
}

/// Where the `default` metadata sends one subject: the two keys routing reads.
#[derive(Default)]
struct Target {
    object: Option<String>,
    node: Option<String>,
}

/// A node the registry announced: its global id and its `object.serial`.
struct Named {
    id: u32,
    serial: String,
}

/// The `default` metadata, bound. The listener goes before the proxy.
struct Routing {
    _listener: MetadataListener,
    metadata: Metadata,
}

/// One connection to the daemon. Fields drop in order: the streams, then the bound proxies,
/// then the registry and the core, each listener before what it listens to.
struct Conn {
    channels: HashMap<ChannelId, ChannelStream>,
    outputs: [Option<Live>; 3],
    routing: Option<Routing>,
    /// Where the `default` metadata sends each subject, as the metadata reports it.
    targets: HashMap<u32, Target>,
    /// Routings waiting for the metadata to be bound or for their sink to be announced.
    pending_routes: Vec<(u32, Option<String>)>,
    /// Every node the registry announced (ours included), by `node.name`, and back.
    nodes: HashMap<String, Named>,
    node_names: HashMap<u32, String>,
    apps: HashMap<u32, AppEntry>,
    clients: HashMap<u32, ClientEntry>,
    devices: HashMap<u32, DeviceInfo>,
    /// The first registry pass is done: devices are known, graphs may be applied.
    ready: bool,
    sync: AsyncSeq,
    _registry_listener: pipewire::registry::Listener,
    registry: RegistryRc,
    _core_listener: pipewire::core::Listener,
    core: CoreRc,
}

impl Conn {
    /// Writes a routing: `target.object` set to the sink's `object.serial` (or removed for the
    /// default), and `target.node` removed. False while the metadata is not bound or the sink
    /// not announced yet.
    ///
    /// The serial rather than the name: WirePlumber's `restore-stream.lua` (as Ubuntu 24.04
    /// ships it) persists a `target.object` only by looking its value up as a serial, and
    /// restores through `target.node`, which is why both keys are written.
    fn try_route(&self, node: u32, to: Option<&str>) -> bool {
        let Some(routing) = &self.routing else {
            return false;
        };
        let serial = match to {
            Some(name) => match self.nodes.get(name) {
                Some(named) => Some(named.serial.as_str()),
                None => return false,
            },
            None => None,
        };
        let metadata = &routing.metadata;
        metadata.set_property(node, "target.node", None, None);
        metadata.set_property(node, "target.object", serial.map(|_| "Spa:Id"), serial);
        true
    }

    /// A session manager restores a routing it remembers when an app appears (WirePlumber's
    /// `restore-stream`), and may do so after the host has routed the app: within
    /// [`RESTORE_WINDOW`] of its appearance, a routing that is not the host's is written over
    /// again (a few times at most, so two writers never fight forever).
    fn keep_wish(&mut self, subject: u32) {
        let Some(app) = self.apps.get(&subject) else {
            return;
        };
        let Some(wish) = app.wish.clone() else {
            return;
        };
        if app.appeared.elapsed() > RESTORE_WINDOW || app.rewritten >= MAX_REWRITES {
            return;
        }
        let target = self.targets.get(&subject);
        let object = target.and_then(|t| t.object.as_deref());
        let node = target
            .and_then(|t| t.node.as_deref())
            .filter(|n| *n != "-1");
        let kept = match &wish {
            None => object.is_none() && node.is_none(),
            Some(sink) => match self.nodes.get(sink) {
                Some(named) => object == Some(named.serial.as_str()) && node.is_none(),
                // Not announced yet: the routing waits in `pending_routes` anyway.
                None => true,
            },
        };
        if !kept
            && self.try_route(subject, wish.as_deref())
            && let Some(app) = self.apps.get_mut(&subject)
        {
            app.rewritten += 1;
        }
    }

    fn flush_routes(&mut self) {
        for (node, to) in std::mem::take(&mut self.pending_routes) {
            if !self.try_route(node, to.as_deref()) {
                self.pending_routes.push((node, to));
            }
        }
    }

    /// Forgets, by name, the node of an app sink whose stream was just destroyed. Its
    /// `global_remove` arrives later; until then a routing to that name (the same name again
    /// when a rename keeps the slug) would find the old node and write its serial, which
    /// nothing rewrites once the old node is gone. Forgotten, the routing waits in
    /// `pending_routes` until the registry announces the node that takes the name next.
    /// `node_names` keeps the id: `global_remove` removes a `nodes` entry only when its id
    /// matches, so the new node's entry stands.
    fn forget_sink(&mut self, spec: Option<&Spec>) {
        if let Some(Spec::AppSink { node_name, .. }) = spec {
            self.nodes.remove(node_name);
        }
    }

    fn device_present(&self, name: &str) -> bool {
        self.devices.values().any(|d| d.device.name == name)
    }

    fn live_mut(&mut self, serial: u64) -> Option<&mut Live> {
        self.outputs
            .iter_mut()
            .flatten()
            .chain(self.channels.values_mut().filter_map(|c| c.live.as_mut()))
            .find(|live| live.serial == serial)
    }
}

struct Control {
    config: BackendConfig,
    shared: Arc<Shared>,
    plans: PlanPort,
    events: Events,
    inbox: Arc<Inbox>,
    mainloop: MainLoopWeak,
    context: ContextRc,
    conn: Option<Conn>,
    /// The newest graph the host applied, re-applied after a reconnect.
    graph: Option<BackendGraph>,
    reconnect_at: Option<Instant>,
    backoff: Duration,
    next_serial: u64,
    /// Which channel holds each of the tick's input slots.
    slots: [Option<ChannelId>; MAX_CHANNELS],
    pending: Pending,
    seen: Seen,
}

impl Control {
    fn handle(&mut self, ctl: &Rc<Ctl>, note: Note) {
        match note {
            Note::Wake => {
                let mail = self.inbox.take();
                if mail.quit {
                    self.close();
                    if let Some(mainloop) = self.mainloop.upgrade() {
                        mainloop.quit();
                    }
                    return;
                }
                // The graph first: a routing names a sink of the newest graph, the way a host
                // routes its apps again after it applied a setup. Routed first, an app would be
                // sent to a sink this graph is about to make again, and moved a moment later.
                if let Some(graph) = mail.graph {
                    self.graph = Some(graph);
                    self.apply(ctl);
                }
                for (node, to) in mail.routes {
                    self.route(node, to);
                }
            }
            Note::Housekeeping => self.housekeeping(ctl),
            Note::CoreDone(seq) => {
                if let Some(conn) = &mut self.conn
                    && !conn.ready
                    && conn.sync == seq
                {
                    conn.ready = true;
                    self.apply(ctl);
                }
            }
            Note::CoreError { id, res, message } => {
                if id == pipewire::sys::PW_ID_CORE && res == -EPIPE {
                    log::warn!("vix-mix-pipewire: the daemon went away ({message}); reconnecting");
                    self.lose();
                } else {
                    log::info!("vix-mix-pipewire: PipeWire error on object {id}: {message}");
                }
            }
            Note::Global(global) => self.global(ctl, global),
            Note::GlobalRemove(id) => self.global_remove(id),
            Note::NodeInfo { id, props } => {
                if let Some(conn) = &mut self.conn
                    && let Some(app) = conn.apps.get_mut(&id)
                    && app.props.is_none()
                {
                    app.props = Some(props);
                    self.announce_apps();
                }
            }
            Note::ClientInfo { id, props } => {
                if let Some(conn) = &mut self.conn
                    && let Some(client) = conn.clients.get_mut(&id)
                    && client.props.is_none()
                {
                    client.props = Some(props);
                    self.announce_apps();
                }
            }
            Note::Metadata {
                subject,
                key,
                value,
            } => {
                if let Some(conn) = &mut self.conn {
                    match key.as_deref() {
                        // No key: every key of `subject` was removed (WirePlumber does so
                        // when an app it routed goes away). Only `PW_ID_ANY` means every
                        // subject: the mirror of the other apps must stand, or a rename
                        // would no longer find them.
                        None if subject == pipewire::sys::PW_ID_ANY => conn.targets.clear(),
                        None => {
                            conn.targets.remove(&subject);
                        }
                        Some(key @ ("target.object" | "target.node")) => {
                            // Only a key set to something can be a routing not ours: our own
                            // writes remove `target.node` before they set `target.object`.
                            let set = value.is_some();
                            let target = conn.targets.entry(subject).or_default();
                            if key == "target.object" {
                                target.object = value;
                            } else {
                                target.node = value;
                            }
                            if target.object.is_none() && target.node.is_none() {
                                conn.targets.remove(&subject);
                            }
                            if set {
                                conn.keep_wish(subject);
                            }
                        }
                        Some(_) => {}
                    }
                }
            }
            Note::Stream {
                serial,
                connection,
                phase,
            } => {
                let Some(conn) = &mut self.conn else {
                    return;
                };
                // A note from an earlier connection of a stream that was connected again.
                if conn
                    .live_mut(serial)
                    .is_none_or(|live| live.connection() != connection)
                {
                    return;
                }
                if let Some(channel) = conn
                    .channels
                    .values()
                    .find(|c| c.live.as_ref().is_some_and(|l| l.serial == serial))
                    && let Some(shared) = &channel.channel
                {
                    shared.streaming.store(
                        phase == Phase::Streaming,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                let Some(live) = conn.live_mut(serial) else {
                    return;
                };
                if !matches!(phase, Phase::Connecting | Phase::Paused | Phase::Streaming) {
                    log::info!(
                        "vix-mix-pipewire: {} is {phase:?}",
                        live.spec.node_name(&self.config)
                    );
                    live.retry_at = Some(Instant::now() + STREAM_RETRY);
                }
                live.phase = phase;
                self.elect();
            }
        }
    }

    /// Connects to the daemon: the core, its listener, the registry and its first roundtrip.
    fn open(&mut self, ctl: &Rc<Ctl>) -> Result<(), pipewire::Error> {
        let core = self.context.connect_rc(Some(properties! {
            "application.name" => self.config.app_name,
        }))?;
        let note = noter(ctl);
        let core_listener = core
            .add_listener_local()
            .done({
                let note = note.clone();
                move |id, seq| {
                    if id == pipewire::sys::PW_ID_CORE {
                        note(Note::CoreDone(seq));
                    }
                }
            })
            .error({
                let note = note.clone();
                move |id, _seq, res, message| {
                    note(Note::CoreError {
                        id,
                        res,
                        message: message.to_string(),
                    })
                }
            })
            .register();
        let registry = core.get_registry_rc()?;
        let registry_listener = registry
            .add_listener_local()
            .global({
                let note = note.clone();
                move |global| note(Note::Global(global.to_owned()))
            })
            .global_remove(move |id| note(Note::GlobalRemove(id)))
            .register();
        let sync = core.sync(0)?;
        let stats = self.shared.stats();
        self.seen = Seen {
            ticks_at_open: stats.ticks,
            xruns: stats.xruns,
            ..Seen::default()
        };
        self.conn = Some(Conn {
            channels: HashMap::new(),
            outputs: [None, None, None],
            routing: None,
            targets: HashMap::new(),
            pending_routes: Vec::new(),
            nodes: HashMap::new(),
            node_names: HashMap::new(),
            apps: HashMap::new(),
            clients: HashMap::new(),
            devices: HashMap::new(),
            ready: false,
            sync,
            _registry_listener: registry_listener,
            registry,
            _core_listener: core_listener,
            core,
        });
        Ok(())
    }

    /// Tears the connection down: every stream, every proxy, the core.
    fn close(&mut self) {
        self.shared.expect(None);
        self.conn = None;
    }

    /// The daemon went away: report it and everything it held, and reconnect with backoff.
    fn lose(&mut self) {
        let Some(conn) = self.conn.take() else {
            return;
        };
        self.shared.expect(None);
        for info in conn.devices.values() {
            self.events
                .send(BackendEvent::DeviceRemoved(info.device.name.clone()));
        }
        for (id, app) in &conn.apps {
            if app.announced {
                self.events.send(BackendEvent::AppStreamRemoved(*id));
            }
        }
        drop(conn);
        self.events.send(BackendEvent::Disconnected);
        self.events
            .send(BackendEvent::TickSource(crate::TickSource::Stopped));
        self.seen.connected = false;
        self.backoff = BACKOFF_FIRST;
        self.reconnect_at = Some(Instant::now() + self.backoff);
    }

    fn global(&mut self, ctl: &Rc<Ctl>, global: GlobalObject<PropertiesBox>) {
        let Some(conn) = &mut self.conn else {
            return;
        };
        let props = global.props.as_ref().map(|p| p.dict());
        if global.type_ == pipewire::types::ObjectType::Node
            && let Some(props) = props
            && let (Some(name), Some(serial)) = (props.get("node.name"), props.get("object.serial"))
        {
            conn.nodes.insert(
                name.to_string(),
                Named {
                    id: global.id,
                    serial: serial.to_string(),
                },
            );
            conn.node_names.insert(global.id, name.to_string());
            // A routing may have been waiting for this sink.
            conn.flush_routes();
        }
        match registry::classify(&global.type_, props, self.config.app_name) {
            Global::Device(info) => {
                let name = info.device.name.clone();
                conn.devices.insert(global.id, info.clone());
                self.events.send(BackendEvent::DeviceAdded(info));
                self.device_appeared(&name);
            }
            Global::AppStream => {
                let Ok(node) = conn.registry.bind::<Node, _>(&global) else {
                    return;
                };
                let note = noter(ctl);
                let id = global.id;
                let listener = node
                    .add_listener_local()
                    .info(move |info| {
                        if let Some(props) = info.props() {
                            note(Note::NodeInfo {
                                id,
                                props: PropertiesBox::from_dict(props),
                            });
                        }
                    })
                    .register();
                conn.apps.insert(
                    id,
                    AppEntry {
                        _listener: listener,
                        _node: node,
                        props: None,
                        announced: false,
                        appeared: Instant::now(),
                        wish: None,
                        rewritten: 0,
                    },
                );
            }
            Global::Client => {
                let Ok(client) = conn.registry.bind::<Client, _>(&global) else {
                    return;
                };
                let note = noter(ctl);
                let id = global.id;
                let listener = client
                    .add_listener_local()
                    .info(move |info| {
                        if let Some(props) = info.props() {
                            note(Note::ClientInfo {
                                id,
                                props: PropertiesBox::from_dict(props),
                            });
                        }
                    })
                    .register();
                conn.clients.insert(
                    id,
                    ClientEntry {
                        _listener: listener,
                        _client: client,
                        props: None,
                    },
                );
            }
            Global::DefaultMetadata => {
                if conn.routing.is_some() {
                    return;
                }
                let Ok(metadata) = conn.registry.bind::<Metadata, _>(&global) else {
                    return;
                };
                let note = noter(ctl);
                let listener = metadata
                    .add_listener_local()
                    .property(move |subject, key, _type, value| {
                        note(Note::Metadata {
                            subject,
                            key: key.map(str::to_string),
                            value: value.map(str::to_string),
                        });
                        0
                    })
                    .register();
                conn.routing = Some(Routing {
                    _listener: listener,
                    metadata,
                });
                conn.flush_routes();
            }
            Global::Other => {}
        }
    }

    fn global_remove(&mut self, id: u32) {
        let Some(conn) = &mut self.conn else {
            return;
        };
        if let Some(name) = conn.node_names.remove(&id)
            && conn.nodes.get(&name).is_some_and(|named| named.id == id)
        {
            conn.nodes.remove(&name);
        }
        conn.pending_routes.retain(|(node, _)| *node != id);
        if let Some(info) = conn.devices.remove(&id) {
            self.events
                .send(BackendEvent::DeviceRemoved(info.device.name));
            self.elect();
        } else if let Some(app) = conn.apps.remove(&id) {
            if app.announced {
                self.events.send(BackendEvent::AppStreamRemoved(id));
            }
            conn.targets.remove(&id);
        } else if conn.clients.remove(&id).is_some() {
            // Its streams that waited for its properties go out with what they have.
            self.announce_apps();
        }
    }

    /// Reports every app stream whose properties have arrived, once its client's have too
    /// (or its client is gone or unknown).
    fn announce_apps(&mut self) {
        let Some(conn) = &mut self.conn else {
            return;
        };
        let mut ready = Vec::new();
        for (id, app) in &conn.apps {
            let Some(props) = app.props.as_ref().filter(|_| !app.announced) else {
                continue;
            };
            let client = props
                .get("client.id")
                .and_then(|c| c.parse::<u32>().ok())
                .and_then(|c| conn.clients.get(&c));
            if client.is_some_and(|c| c.props.is_none()) {
                continue;
            }
            let client = client.and_then(|c| c.props.as_ref()).map(|p| p.dict());
            ready.push(registry::app_stream(*id, props.dict(), client));
        }
        for stream in ready {
            if let Some(app) = conn.apps.get_mut(&stream.node) {
                app.announced = true;
            }
            self.events.send(BackendEvent::AppStreamAdded(stream));
        }
    }

    /// A device appeared: connect again every stream of ours that waits for it.
    fn device_appeared(&mut self, name: &str) {
        let Some(conn) = &mut self.conn else {
            return;
        };
        let waiting = conn
            .outputs
            .iter_mut()
            .flatten()
            .chain(conn.channels.values_mut().filter_map(|c| c.live.as_mut()));
        for live in waiting {
            if !live.alive() && live.spec.target() == Some(name) {
                reconnect(&self.config, live);
            }
        }
        self.elect();
    }

    /// Routes `node` through the `default` metadata ([`Conn::try_route`]), or keeps the
    /// routing until the metadata is bound and the sink announced. The newest one wins.
    fn route(&mut self, node: u32, to: Option<String>) {
        let Some(conn) = &mut self.conn else {
            return;
        };
        if let Some(app) = conn.apps.get_mut(&node) {
            app.wish = Some(to.clone());
        }
        conn.pending_routes.retain(|(n, _)| *n != node);
        if conn.try_route(node, to.as_deref()) {
            return;
        }
        if conn.pending_routes.len() >= MAX_PENDING_ROUTES {
            conn.pending_routes.remove(0);
        }
        conn.pending_routes.push((node, to));
    }

    /// The output expected to run the tick: Monitor while it has a device and has not been
    /// given up on, else Master out, else the Master source. (The data thread does not ask:
    /// whichever output runs first in a cycle ticks. This is what the host is told.)
    fn wanted_tick(&self) -> Option<Role> {
        let conn = self.conn.as_ref()?;
        let usable = |role: Role| {
            conn.outputs[role as usize].as_ref().is_some_and(|live| {
                live.alive() && live.spec.target().is_none_or(|t| conn.device_present(t))
            })
        };
        Role::ALL.into_iter().find(|role| usable(*role))
    }

    /// Tells the host when the output expected to tick changes.
    fn elect(&mut self) {
        let tick = self.wanted_tick();
        let changed = tick != self.shared.expected() || !self.seen.tick_source;
        if changed {
            self.seen.tick_source = true;
            self.shared.expect(tick);
            let source = tick.map_or(crate::TickSource::Stopped, Role::tick_source);
            log::info!("vix-mix-pipewire: the tick is now {source:?}");
            self.events.send(BackendEvent::TickSource(source));
        }
    }

    /// Makes the streams match the newest graph: only what changed is touched.
    fn apply(&mut self, ctl: &Rc<Ctl>) {
        let Some(graph) = self.graph.clone() else {
            return;
        };
        if !self.conn.as_ref().is_some_and(|c| c.ready) {
            return;
        }
        let wanted: Vec<(ChannelId, Option<Spec>)> = graph
            .channels
            .iter()
            .take(MAX_CHANNELS)
            .map(|c| {
                let node_name = crate::channel_node_name(self.config.app_name, c.id, &c.name);
                let spec = match &c.input {
                    GraphInput::AppSink => Some(Spec::AppSink {
                        node_name,
                        name: c.name.clone(),
                    }),
                    GraphInput::Capture(device) => device.as_ref().map(|d| Spec::Capture {
                        node_name,
                        name: c.name.clone(),
                        target: d.name.clone(),
                    }),
                };
                (c.id, spec)
            })
            .collect();

        // Channels that are gone: their streams, then their slots.
        let gone: Vec<ChannelId> = self
            .conn
            .as_ref()
            .map(|c| {
                c.channels
                    .keys()
                    .filter(|id| !wanted.iter().any(|(w, _)| w == *id))
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        for id in gone {
            if let Some(conn) = &mut self.conn
                && let Some(stream) = conn.channels.remove(&id)
            {
                conn.forget_sink(stream.spec.as_ref());
            }
            if let Some(slot) = self.slots.iter().position(|s| *s == Some(id)) {
                self.slots[slot] = None;
                self.pending.taps[slot] = None;
                self.pending.replace[slot] = true;
                self.pending.dirty = true;
            }
        }

        let notify = self.notifier(ctl);
        for (id, spec) in &wanted {
            let Some(slot) = self.slot_for(*id) else {
                continue;
            };
            let Some(conn) = &mut self.conn else {
                return;
            };
            let current = conn.channels.get(id).and_then(|c| c.spec.clone());
            if current.as_ref() == spec.as_ref() && conn.channels.contains_key(id) {
                continue;
            }
            // The sink this channel had: its name, and its id and serial once announced, to
            // move its apps when it is made again.
            let old_name = match &current {
                Some(Spec::AppSink { node_name, .. }) => Some(node_name.clone()),
                _ => None,
            };
            let old_sink = old_name
                .as_ref()
                .and_then(|name| conn.nodes.get(name))
                .map(|named| (named.id.to_string(), named.serial.clone()));
            // The old stream goes first, so a new one may take its node name, and its name is
            // forgotten until the new node is announced (`Conn::forget_sink`).
            conn.channels.remove(id);
            conn.forget_sink(current.as_ref());
            let mut stream = ChannelStream {
                spec: spec.clone(),
                live: None,
                channel: None,
            };
            let mut tap = None;
            if let Some(spec) = spec {
                let (input, new_tap, channel) = rt::input_ring(&self.shared);
                let serial = self.next_serial;
                self.next_serial += 1;
                let make = Factory {
                    core: &conn.core,
                    config: &self.config,
                    shared: &self.shared,
                    notify: &notify,
                };
                match Live::create(&make, spec.clone(), Some(input), serial) {
                    Ok(live) => {
                        stream.live = Some(live);
                        stream.channel = Some(channel);
                        tap = Some(new_tap);
                    }
                    Err(error) => log::warn!(
                        "vix-mix-pipewire: could not create {}: {error}",
                        spec.node_name(&self.config)
                    ),
                }
            }
            conn.channels.insert(*id, stream);
            self.pending.taps[slot] = tap;
            self.pending.replace[slot] = true;
            self.pending.dirty = true;
            // A renamed app sink keeps its apps, whether or not the rename changed its node
            // name: what the metadata sends to the old sink (our routing by serial,
            // WirePlumber's restore by id), and every routing the host asked for it that the
            // metadata has not echoed yet or that still waits, is sent to the new one as soon
            // as the registry announces it.
            if let (Some(old_name), Some(Spec::AppSink { node_name: new, .. })) = (&old_name, spec)
            {
                let mut moved: Vec<u32> = conn
                    .targets
                    .iter()
                    .filter(|(_, target)| {
                        old_sink.as_ref().is_some_and(|(old_id, old_serial)| {
                            target.object.as_deref() == Some(old_serial.as_str())
                                || target.node.as_deref() == Some(old_id.as_str())
                        })
                    })
                    .map(|(subject, _)| *subject)
                    .collect();
                let wished = |to: &Option<String>| to.as_deref() == Some(old_name.as_str());
                moved.extend(
                    conn.apps
                        .iter()
                        .filter(|(_, app)| app.wish.as_ref().is_some_and(wished))
                        .map(|(subject, _)| *subject),
                );
                moved.extend(
                    conn.pending_routes
                        .iter()
                        .filter(|(_, to)| wished(to))
                        .map(|(subject, _)| *subject),
                );
                moved.sort_unstable();
                moved.dedup();
                for subject in moved {
                    self.route(subject, Some(new.clone()));
                }
            }
        }
        self.pending.order = [None; MAX_CHANNELS];
        for (position, (id, _)) in wanted.iter().enumerate() {
            self.pending.order[position] = self.slots.iter().position(|s| *s == Some(*id));
        }
        self.pending.count = wanted.len();
        self.pending.dirty = true;

        // The outputs.
        let specs = [
            graph.monitor.as_ref().map(|d| Spec::Output {
                role: Role::Monitor,
                target: d.name.clone(),
            }),
            graph.master.as_ref().map(|d| Spec::Output {
                role: Role::MasterOut,
                target: d.name.clone(),
            }),
            graph.master_source.then_some(Spec::MasterSource),
        ];
        for (index, spec) in specs.into_iter().enumerate() {
            let Some(conn) = &mut self.conn else {
                return;
            };
            if conn.outputs[index].as_ref().map(|l| &l.spec) == spec.as_ref() {
                continue;
            }
            conn.outputs[index] = None;
            let Some(spec) = spec else {
                continue;
            };
            let serial = self.next_serial;
            self.next_serial += 1;
            let make = Factory {
                core: &conn.core,
                config: &self.config,
                shared: &self.shared,
                notify: &notify,
            };
            match Live::create(&make, spec.clone(), None, serial) {
                Ok(live) => conn.outputs[index] = Some(live),
                Err(error) => log::warn!(
                    "vix-mix-pipewire: could not create {}: {error}",
                    spec.node_name(&self.config)
                ),
            }
        }
        self.elect();
        self.send_plan();
    }

    /// The tick's input slot for channel `id`, assigning a free one.
    fn slot_for(&mut self, id: ChannelId) -> Option<usize> {
        if let Some(slot) = self.slots.iter().position(|s| *s == Some(id)) {
            return Some(slot);
        }
        let slot = self.slots.iter().position(Option::is_none)?;
        self.slots[slot] = Some(id);
        Some(slot)
    }

    fn notifier(&self, ctl: &Rc<Ctl>) -> Notify {
        let note = noter(ctl);
        Rc::new(move |serial, connection, phase| {
            note(Note::Stream {
                serial,
                connection,
                phase,
            })
        })
    }

    /// Frees the plans the tick sent back, and sends the waiting one when there is room.
    fn send_plan(&mut self) {
        while let Ok(plan) = self.plans.from_tick.pop() {
            drop(plan);
        }
        if !self.pending.dirty || self.plans.to_tick.slots() == 0 {
            return;
        }
        let mut plan = TickPlan::new();
        for slot in 0..MAX_CHANNELS {
            if std::mem::take(&mut self.pending.replace[slot]) {
                plan.taps[slot] = self.pending.taps[slot].take();
                plan.replace[slot] = true;
            }
        }
        plan.order = self.pending.order;
        plan.count = self.pending.count;
        if self.plans.to_tick.push(plan).is_ok() {
            self.pending.dirty = false;
        }
    }

    fn housekeeping(&mut self, ctl: &Rc<Ctl>) {
        if self.conn.is_none()
            && let Some(at) = self.reconnect_at
            && Instant::now() >= at
        {
            match self.open(ctl) {
                Ok(()) => {
                    log::info!("vix-mix-pipewire: reconnected to the daemon");
                    self.reconnect_at = None;
                    self.backoff = BACKOFF_FIRST;
                }
                Err(error) => {
                    self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
                    log::info!(
                        "vix-mix-pipewire: the daemon does not answer yet ({error}); trying again in {:?}",
                        self.backoff
                    );
                    self.reconnect_at = Some(Instant::now() + self.backoff);
                }
            }
        }
        self.retry_streams();
        self.send_plan();
        self.report();
        self.events.flush();
    }

    /// Connects again the streams the daemon or the session manager gave up on, once a second,
    /// while the device they name (if any) is present.
    fn retry_streams(&mut self) {
        let Some(conn) = &mut self.conn else {
            return;
        };
        let now = Instant::now();
        // A stream that names a device waits for it; one that names none is retried anyway.
        let due = |live: &Live, conn_devices: &HashMap<u32, DeviceInfo>| {
            !live.alive()
                && live.retry_at.is_some_and(|at| now >= at)
                && live
                    .spec
                    .target()
                    .is_none_or(|t| conn_devices.values().any(|d| d.device.name == t))
        };
        let mut retried = false;
        let devices = &conn.devices;
        let streams = conn
            .outputs
            .iter_mut()
            .flatten()
            .chain(conn.channels.values_mut().filter_map(|c| c.live.as_mut()));
        for live in streams {
            if due(live, devices) {
                reconnect(&self.config, live);
                retried = true;
            }
        }
        if retried {
            self.elect();
        }
    }

    /// Tells the host what the data thread's counters say.
    fn report(&mut self) {
        if self.conn.is_none() {
            return;
        }
        let stats = self.shared.stats();
        if !self.seen.connected {
            if stats.ticks > self.seen.ticks_at_open && stats.rate > 0 {
                self.seen.connected = true;
                self.seen.quantum = stats.quantum;
                self.events.send(BackendEvent::Connected {
                    rate: stats.rate,
                    quantum: stats.quantum,
                });
            }
        } else if stats.quantum != 0 && stats.quantum != self.seen.quantum {
            self.seen.quantum = stats.quantum;
            self.events
                .send(BackendEvent::QuantumChanged(stats.quantum));
        }
        if stats.xruns > self.seen.xruns {
            self.seen.xruns = stats.xruns;
            self.events.send(BackendEvent::Xrun);
        }
    }
}

/// Connects `live` again, logging a refusal (the next retry tries again).
fn reconnect(config: &BackendConfig, live: &mut Live) {
    live.retry_at = Some(Instant::now() + STREAM_RETRY);
    if let Err(error) = live.reconnect() {
        log::warn!(
            "vix-mix-pipewire: could not connect {} again: {error}",
            live.spec.node_name(config)
        );
    }
}

/// Linux's `EPIPE`: what the core reports (negated) when the daemon closed the connection.
const EPIPE: i32 = 32;
