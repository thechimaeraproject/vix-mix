//! The window's state between frames and what it does with a frame's gestures. A frame
//! ([`crate::ui::draw`]) reads the host and pushes [`Intent`]s; [`Shell::dispatch`] then
//! carries them out against the host: commands applied (each refusal a toast, which says
//! what to do; a change that worked says nothing, the console shows it), popovers opened and
//! closed, Undo, a star, a rescan, editors queued to open, then opened. [`Shell::events`] takes in what the host
//! reported: its backend's state, its messages, a removal with its Undo, a finished rescan,
//! an editor it closed. Nothing here owns a window or a device (an editor's window comes
//! from an [`Opener`]), so the headless tests drive the same `Shell` the window does.
//!
//! **The clock.** One clock, the window's, in seconds: the frame reads it from egui's input
//! ([`Shell::now`]), and the host's messages are stamped with it as they arrive, which may be
//! long after the last frame (the window draws nothing while the mixer is idle or the daemon
//! is gone, and a rescan holds the window's thread while it describes new bundles), so each
//! shows for its whole time from when it came.

use vixmix_host::{BackendState, Host, HostEvent};
use vixmix_model::{ChannelKind, Command, InstanceId, Setup};

use crate::console::{Anchor, Gestures};
use crate::editors::{EditorWindows, MAX_EDITORS, Opener};
use crate::meters::Meters;
use crate::popover::{Popover, SourcePanel};
use crate::theme::pop;
use crate::toast::Toasts;

/// What a gesture asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    /// Apply a command on the host.
    Apply(Command),
    /// Open (or raise) an insert's editor window.
    OpenEditor(InstanceId),
    /// Open the popover that hangs from this, or close it when it is the one open.
    Toggle(Anchor),
    /// The add-channel menu: a new channel, and its panel open with the name to type.
    AddChannel(ChannelKind),
    /// The toast's Undo: applies the command the host handed out with the removal.
    Undo,
    /// A star in the plugin browser: the host keeps it in `plugins.ron`.
    Favorite { id: String, on: bool },
    /// The menu's Rescan plugins.
    Rescan,
    /// A message for the toast line: a control refused something itself, and says what to do.
    Notice(String),
}

/// The window's state between frames.
pub struct Shell {
    /// Where the audio is, as the host last said.
    pub backend: BackendState,
    /// The meters' levels and peak-hold lines.
    pub meters: Meters,
    pub gestures: Gestures,
    pub popover: Option<Popover>,
    pub toasts: Toasts,
    /// Show the focus ring: true from a Tab until the next press of the pointer.
    pub keyboard: bool,
    /// The window's clock at the last frame, in seconds (the frame's egui input time).
    pub now: f64,
    /// Editors to open once the frame is done (the window loop has the event loop).
    pub to_open: Vec<InstanceId>,
    /// What the last frame drew under the header: the console, and the banner over it. The
    /// window keeps at least this big.
    pub content: egui::Rect,
}

impl Default for Shell {
    fn default() -> Shell {
        Shell {
            backend: BackendState::Connecting,
            meters: Meters::default(),
            gestures: Gestures::default(),
            popover: None,
            toasts: Toasts::default(),
            keyboard: false,
            now: 0.0,
            to_open: Vec::new(),
            content: egui::Rect::NOTHING,
        }
    }
}

/// The instances whose editors must close before `cmd` removes their inserts.
fn removing(cmd: &Command, setup: &Setup) -> Vec<InstanceId> {
    match cmd {
        Command::RemoveInsert { instance } => vec![*instance],
        Command::RemoveChannel(ch) => setup
            .channel(*ch)
            .map(|c| c.chain.iter().map(|i| i.instance).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

impl Shell {
    /// What one poll of the host reported, in order: the backend's state, a message for the
    /// toast line (it says what to do), a removal shown with its Undo for
    /// [`UNDO_TOAST`](vixmix_host::UNDO_TOAST), a finished rescan (an open browser lists what
    /// it found), an editor the host closed itself. The rest (the setup,
    /// the apps, the devices, the inserts playing as pass-throughs) the next frame reads
    /// from the host. `now` is when the poll answered, on the window's clock: each message
    /// shows for its time from then, however long ago the last frame was.
    pub fn events(&mut self, events: Vec<HostEvent>, now: f64, editors: &mut EditorWindows) {
        for event in events {
            match event {
                HostEvent::Backend(state) => self.backend = state,
                HostEvent::Toast(message) => {
                    log::warn!("{message}");
                    self.toasts.show(message, None, now, pop::TOAST_HOST_S);
                }
                HostEvent::Removed { message, undo } => {
                    log::info!("{message}");
                    self.toasts
                        .show(message, Some(undo), now, pop::TOAST_WITH_ACTION_S);
                }
                HostEvent::PluginsRescanned => {
                    // An open browser lists what the rescan found.
                    if let Some(Popover::Browser(browser)) = &mut self.popover {
                        browser.forget();
                    }
                }
                HostEvent::EditorClosed(instance) => editors.forget(instance),
                HostEvent::SetupChanged
                | HostEvent::Apps(_)
                | HostEvent::Devices(_)
                | HostEvent::Problems(_) => {}
            }
        }
    }

    /// Carries out a frame's intents, in order.
    pub fn dispatch(
        &mut self,
        host: &mut dyn Host,
        editors: &mut EditorWindows,
        intents: Vec<Intent>,
    ) {
        for intent in intents {
            match intent {
                Intent::Apply(cmd) => {
                    self.send(host, editors, cmd);
                }
                Intent::OpenEditor(instance) => {
                    if !self.to_open.contains(&instance) && self.to_open.len() < MAX_EDITORS {
                        self.to_open.push(instance);
                    }
                }
                Intent::Toggle(anchor) => {
                    let same = self.popover.as_ref().map(Popover::anchor) == Some(anchor);
                    self.close_popover(host, editors);
                    if !same {
                        self.popover = Popover::of(anchor, host.setup());
                    }
                }
                Intent::AddChannel(kind) => {
                    if let Some(ch) = self
                        .send(host, editors, Command::AddChannel { kind })
                        .and_then(|a| a.channel)
                        && let Some(channel) = host.setup().channel(ch)
                    {
                        self.popover =
                            Some(Popover::Source(SourcePanel::new(ch, &channel.name, true)));
                    }
                }
                Intent::Undo => {
                    if let Some(undo) = self.toasts.take_undo(self.now) {
                        self.send(host, editors, undo);
                    }
                }
                Intent::Favorite { id, on } => {
                    if host.catalog().get(&id).is_some() {
                        host.set_favorite(&id, on);
                    }
                }
                Intent::Rescan => host.rescan_plugins(),
                Intent::Notice(message) => {
                    self.toasts.show(message, None, self.now, pop::TOAST_HOST_S);
                }
            }
        }
        self.toasts.expire(self.now);
    }

    /// Opens the editors the frame's gestures queued, each in a window from `opener`; a
    /// refusal goes to the toast line, saying what to do.
    pub fn open_editors(
        &mut self,
        opener: &dyn Opener,
        host: &mut dyn Host,
        editors: &mut EditorWindows,
    ) {
        for instance in std::mem::take(&mut self.to_open) {
            if let Err(problem) = editors.open(opener, host, instance) {
                log::warn!("{problem}");
                self.toasts.show(problem, None, self.now, pop::TOAST_HOST_S);
            }
        }
    }

    /// Closes the open popover, sending what it holds (a rename).
    pub fn close_popover(&mut self, host: &mut dyn Host, editors: &mut EditorWindows) {
        if let Some(popover) = self.popover.take()
            && let Some(cmd) = popover.closing(host.setup())
        {
            self.send(host, editors, cmd);
        }
    }

    /// Applies `cmd`: the editors of the inserts it removes close first; a refusal goes to
    /// the toast line. A change that worked says nothing: the console shows it.
    fn send(
        &mut self,
        host: &mut dyn Host,
        editors: &mut EditorWindows,
        cmd: Command,
    ) -> Option<vixmix_model::Applied> {
        for instance in removing(&cmd, host.setup()) {
            editors.close(host, instance);
        }
        match host.apply(cmd) {
            Ok(applied) => Some(applied),
            Err(refused) => {
                self.toasts
                    .show(refused.to_string(), None, self.now, pop::TOAST_HOST_S);
                None
            }
        }
    }
}
