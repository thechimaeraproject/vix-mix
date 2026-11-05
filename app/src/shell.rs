//! The window's state between frames and what it does with a frame's gestures. A frame
//! ([`crate::ui::draw`]) reads the host and pushes [`Intent`]s; [`Shell::dispatch`] then
//! carries them out against the host: commands applied (each refusal a toast, which says
//! what to do), popovers opened and closed, the toasts a removal or a move earns, Undo,
//! editors queued to open. Nothing here owns a window or a device, so the headless tests
//! drive the same `Shell` the window does.

use std::collections::BTreeMap;

use vixmix_host::{BackendState, Host, HostEvent};
use vixmix_model::{ChannelKind, Command, InstanceId, Setup, Strip};

use crate::console::{Anchor, Gestures};
use crate::editors::{EditorWindows, MAX_EDITORS};
use crate::meters::Meters;
use crate::popover::{Popover, SourcePanel};
use crate::theme::pop;
use crate::toast::{self, Toasts, Undo};

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
    /// The toast's Undo.
    Undo,
    /// A star in the plugin browser.
    Favorite { id: String, on: bool },
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
    /// Stars toggled in the browser this session, over the catalog's own favourites.
    pub favorites: Favorites,
    /// Show the focus ring: true from a Tab until the next press of the pointer.
    pub keyboard: bool,
    /// The window's clock at the last frame, in seconds.
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
            favorites: Favorites::default(),
            keyboard: false,
            now: 0.0,
            to_open: Vec::new(),
            content: egui::Rect::NOTHING,
        }
    }
}

/// The browser's stars as the window shows them: the catalog's favourites with this
/// session's toggles over them. Bounded: only a plugin the catalog lists can be toggled.
///
/// The `Host` trait has no way to save a star yet (the build map's change request 8), so a
/// toggle lasts until the window closes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Favorites {
    toggled: BTreeMap<String, bool>,
}

impl Favorites {
    /// Whether `id` is a favourite.
    pub fn is(&self, host: &dyn Host, id: &str) -> bool {
        self.toggled
            .get(id)
            .copied()
            .unwrap_or_else(|| host.catalog().favorites.contains(id))
    }

    /// Stars or unstars `id`, when the catalog lists it.
    pub fn set(&mut self, host: &dyn Host, id: String, on: bool) {
        if host.catalog().get(&id).is_some() {
            self.toggled.insert(id, on);
        }
    }
}

/// What a command earns on the toast line when it changes something: the words, and what
/// Undo puts back.
#[derive(Clone, Debug, PartialEq)]
enum Note {
    None,
    Info(String),
    Undo(String, Undo),
}

/// The name a strip goes by.
fn strip_name(setup: &Setup, strip: Strip) -> Option<&str> {
    match strip {
        Strip::Channel(ch) => setup.channel(ch).map(|c| c.name.as_str()),
        Strip::Mix(mix) => Some(mix.name()),
    }
}

impl Note {
    /// The note `cmd` earns, read from the setup before it applies.
    fn before(cmd: &Command, setup: &Setup) -> Note {
        match cmd {
            Command::RemoveInsert { instance } => {
                let found = setup
                    .channels
                    .iter()
                    .map(|c| (Strip::Channel(c.id), &c.chain))
                    .chain([
                        (Strip::Mix(setup.monitor.id), &setup.monitor.chain),
                        (Strip::Mix(setup.master.id), &setup.master.chain),
                    ])
                    .find_map(|(strip, chain)| {
                        let at = chain.iter().position(|i| i.instance == *instance)?;
                        Some((strip, at, chain[at].clone()))
                    });
                match found {
                    Some((strip, at, insert)) => Note::Undo(
                        format!(
                            "Removed {} from {}",
                            insert.plugin.name,
                            strip_name(setup, strip).unwrap_or_default()
                        ),
                        Undo::Insert { strip, at, insert },
                    ),
                    None => Note::None,
                }
            }
            Command::RemoveChannel(ch) => match setup.channels.iter().position(|c| c.id == *ch) {
                Some(at) => {
                    let channel = setup.channels[at].clone();
                    Note::Undo(
                        format!("Removed {}", channel.name),
                        Undo::Channel { at, channel },
                    )
                }
                None => Note::None,
            },
            Command::MoveInsert { from, to, copy } if from.strip != to.strip || *copy => {
                let name = match from.strip {
                    Strip::Channel(ch) => setup.channel(ch).map(|c| &c.chain),
                    Strip::Mix(mix) => Some(&setup.mix(mix).chain),
                }
                .and_then(|chain| chain.get(from.index))
                .map(|i| i.plugin.name.clone());
                match (name, strip_name(setup, to.strip)) {
                    (Some(name), Some(to)) => Note::Info(format!(
                        "{} {name} to {to}",
                        if *copy { "Copied" } else { "Moved" }
                    )),
                    _ => Note::None,
                }
            }
            Command::AddInsert { strip, plugin, .. } => match strip_name(setup, *strip) {
                Some(to) => Note::Info(format!("Added {} to {to}", plugin.name)),
                None => Note::None,
            },
            Command::SetPreview(on) => Note::Info(
                if *on {
                    "Monitor output now plays Master"
                } else {
                    "Monitor output back to the Monitor mix"
                }
                .to_string(),
            ),
            _ => Note::None,
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
    /// A host event: the backend's state, a message for the toast line, an editor the host
    /// closed itself.
    pub fn event(&mut self, event: HostEvent, editors: &mut EditorWindows) {
        match event {
            HostEvent::Backend(state) => self.backend = state,
            HostEvent::Toast(message) => {
                log::warn!("{message}");
                self.toasts.show(message, None, self.now, pop::TOAST_HOST_S);
            }
            HostEvent::EditorClosed(instance) => editors.forget(instance),
            _ => {}
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
                    if let Some(undo) = self.toasts.take_undo(self.now)
                        && let Err(refused) = toast::replay(host, undo)
                    {
                        self.toasts
                            .show(refused.to_string(), None, self.now, pop::TOAST_HOST_S);
                    }
                }
                Intent::Favorite { id, on } => self.favorites.set(host, id, on),
            }
        }
        self.toasts.expire(self.now);
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
    /// the toast line; a change earns its note.
    fn send(
        &mut self,
        host: &mut dyn Host,
        editors: &mut EditorWindows,
        cmd: Command,
    ) -> Option<vixmix_model::Applied> {
        let note = Note::before(&cmd, host.setup());
        for instance in removing(&cmd, host.setup()) {
            editors.close(host, instance);
        }
        match host.apply(cmd) {
            Ok(applied) => {
                if applied.changed() {
                    match note {
                        Note::None => {}
                        Note::Info(message) => self.toasts.info(message, self.now),
                        Note::Undo(message, undo) => self.toasts.show(
                            message,
                            Some(undo),
                            self.now,
                            pop::TOAST_WITH_ACTION_S,
                        ),
                    }
                }
                Some(applied)
            }
            Err(refused) => {
                self.toasts
                    .show(refused.to_string(), None, self.now, pop::TOAST_HOST_S);
                None
            }
        }
    }
}
