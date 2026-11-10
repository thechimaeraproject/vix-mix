//! What the window's headless tests build on (Vix's `test_support.rs` pattern): a
//! [`Harness`] that runs the window's own frame — [`ui::draw`] then [`Shell::dispatch`] —
//! in egui without a window or a GPU, against `FakeHost` wrapped in a [`Recorder`] that
//! keeps every call the window made, so a test presses, drags and types as a user does and
//! asserts the [`Command`] that went to the host; and [`FakeWindows`], editor windows with no
//! event loop, which keep every call the editors made of them.

use std::cell::RefCell;
use std::num::NonZeroIsize;
use std::rc::Rc;
use std::time::Duration;

use chimaeraproject_vst3_host::{EditorEvent, EditorSize};
use egui::{Color32, Event, Key, Modifiers, PointerButton, Pos2, Rect, vec2};
use vixmix_host::{
    AppPresence, FakeHost, Host, HostError, HostEvent, InsertProblem, MeterFrame, StatsFrame,
};
use vixmix_model::{
    Applied, ChannelId, Command, DeviceInfo, InstanceId, MixId, Refused, Setup, Strip,
};
use vixmix_plugins::Catalog;
use winit::raw_window_handle::{RawWindowHandle, Win32WindowHandle};
use winit::window::WindowId;

use crate::console::Geometry;
use crate::editors::{EditorWindow, EditorWindows, Opener};
use crate::fader::Target;
use crate::shell::{Intent, Shell};
use crate::strip::{self, Rows};
use crate::theme::size;
use crate::ui;

/// What the window asked of the host, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Apply(Command),
    OpenEditor(InstanceId),
    CloseEditor(InstanceId),
    Favorite(String, bool),
    Rescan,
}

/// `FakeHost`, keeping every call the window made.
pub struct Recorder {
    pub inner: FakeHost,
    pub calls: Vec<Call>,
    /// Open every editor asked for at [`Recorder::EDITOR_SIZE`], as a real host with real
    /// plugins does, rather than refusing as `FakeHost` does.
    pub opens_editors: bool,
    /// What the next `tick_editors` answers.
    pub editor_events: Vec<(InstanceId, EditorEvent)>,
    /// The inserts playing as pass-throughs (`FakeHost` has none).
    pub problems: Vec<InsertProblem>,
    /// Events to report at the next poll, after `FakeHost`'s own.
    pub news: Vec<HostEvent>,
    /// Answer no poll while set: what the host has to report waits for the first poll after,
    /// as a host busy on the window's thread (a rescan describing new bundles) answers late.
    pub busy: bool,
    /// A catalog in place of `FakeHost`'s: what a rescan found.
    pub catalog: Option<Catalog>,
    /// Devices in place of `FakeHost`'s: a machine with more than a panel's list shows.
    pub devices: Option<Vec<DeviceInfo>>,
}

impl Recorder {
    /// The size every editor asks for when [`Recorder::opens_editors`].
    pub const EDITOR_SIZE: EditorSize = EditorSize {
        width: 552,
        height: 404,
    };

    /// `FakeHost`'s example console, refusing editors as `FakeHost` does.
    pub fn new() -> Recorder {
        Recorder {
            inner: FakeHost::new(),
            calls: Vec::new(),
            opens_editors: false,
            editor_events: Vec::new(),
            problems: Vec::new(),
            news: Vec::new(),
            busy: false,
            catalog: None,
            devices: None,
        }
    }

    /// The same, opening every editor asked for.
    pub fn with_editors() -> Recorder {
        Recorder {
            opens_editors: true,
            ..Recorder::new()
        }
    }

    /// The commands the window sent since call `from`.
    pub fn sent_since(&self, from: usize) -> Vec<Command> {
        self.calls[from..]
            .iter()
            .filter_map(|c| match c {
                Call::Apply(cmd) => Some(cmd.clone()),
                _ => None,
            })
            .collect()
    }
}

impl Host for Recorder {
    fn setup(&self) -> &Setup {
        self.inner.setup()
    }
    fn apply(&mut self, cmd: Command) -> Result<Applied, Refused> {
        self.calls.push(Call::Apply(cmd.clone()));
        self.inner.apply(cmd)
    }
    fn poll(&mut self) -> Vec<HostEvent> {
        if self.busy {
            return Vec::new();
        }
        let mut events = self.inner.poll();
        events.append(&mut self.news);
        events
    }
    fn meters(&self) -> MeterFrame {
        self.inner.meters()
    }
    fn stats(&self) -> StatsFrame {
        self.inner.stats()
    }
    fn catalog(&self) -> &Catalog {
        if let Some(catalog) = &self.catalog {
            return catalog;
        }
        self.inner.catalog()
    }
    fn apps(&self) -> &[AppPresence] {
        self.inner.apps()
    }
    fn devices(&self) -> &[DeviceInfo] {
        if let Some(devices) = &self.devices {
            return devices;
        }
        self.inner.devices()
    }
    fn open_editor(
        &mut self,
        instance: InstanceId,
        parent: RawWindowHandle,
    ) -> Result<EditorSize, HostError> {
        if self.opens_editors {
            self.calls.push(Call::OpenEditor(instance));
            return Ok(Recorder::EDITOR_SIZE);
        }
        self.inner.open_editor(instance, parent)
    }
    fn tick_editors(&mut self) -> Vec<(InstanceId, EditorEvent)> {
        let mut events = self.inner.tick_editors();
        events.append(&mut self.editor_events);
        events
    }
    fn close_editor(&mut self, instance: InstanceId) {
        self.calls.push(Call::CloseEditor(instance));
        self.inner.close_editor(instance);
    }
    fn save_now(&mut self) {
        self.inner.save_now();
    }
    fn problems(&self) -> &[InsertProblem] {
        &self.problems
    }
    fn set_favorite(&mut self, plugin_id: &str, favorite: bool) {
        self.calls
            .push(Call::Favorite(plugin_id.to_string(), favorite));
        self.inner.set_favorite(plugin_id, favorite);
    }
    fn rescan_plugins(&mut self) {
        self.calls.push(Call::Rescan);
        self.inner.rescan_plugins();
    }
    fn next_poll(&self) -> Option<Duration> {
        self.inner.next_poll()
    }
}

/// The window without a window: egui, the shell and the recorded host.
pub struct Harness {
    pub ctx: egui::Context,
    pub host: Recorder,
    pub shell: Shell,
    pub editors: EditorWindows,
    /// The modifier keys held during the next frames.
    pub modifiers: Modifiers,
    /// The modifiers egui was last told of.
    sent_modifiers: Modifiers,
    /// The screen: room for the console and a popover beside its last channel.
    pub screen: Rect,
    /// The window's clock, a 60 Hz frame a pass.
    time: f64,
    /// Every text the last frame painted: what it says, where, in which colour.
    pub texts: Vec<(String, Rect, Color32)>,
    /// Every text the last frame painted whole inside what clips it (a scrolled list's view),
    /// and where: what a user sees whole.
    pub whole: Vec<(String, Rect)>,
    /// What the last frame's gestures asked.
    pub intents: Vec<Intent>,
}

impl Harness {
    /// `FakeHost`'s example console on a simulated backend, one frame drawn so the next can
    /// be pressed.
    pub fn new() -> Harness {
        let ctx = egui::Context::default();
        ui::install(&ctx);
        let mut harness = Harness {
            ctx,
            host: Recorder::new(),
            shell: Shell {
                backend: vixmix_host::BackendState::Simulated,
                ..Shell::default()
            },
            editors: EditorWindows::default(),
            modifiers: Modifiers::NONE,
            sent_modifiers: Modifiers::NONE,
            screen: Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0)),
            time: 1.0,
            texts: Vec::new(),
            whole: Vec::new(),
            intents: Vec::new(),
        };
        harness.idle(1);
        harness
    }

    /// One frame with `events`; answers the commands the window sent the host in it.
    pub fn pass(&mut self, events: Vec<Event>) -> Vec<Command> {
        let from = self.host.calls.len();
        // A change of the modifiers held is an event of its own, first, as a window sends it.
        let changed = (self.modifiers != self.sent_modifiers)
            .then_some(Event::ModifiersChanged(self.modifiers));
        self.sent_modifiers = self.modifiers;
        self.serve();
        let input = egui::RawInput {
            screen_rect: Some(self.screen),
            time: Some(self.time),
            events: changed.into_iter().chain(events).collect(),
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let mut intents = Vec::new();
        let (shell, host) = (&mut self.shell, &self.host);
        let mut output = self.ctx.run_ui(input, |root| {
            intents = ui::draw(root, shell, host);
        });
        // Nothing uploads the font atlas here, and egui asserts a delta was handled.
        output.textures_delta.clear();
        self.read(&output.shapes);
        self.intents = intents.clone();
        let acted = !intents.is_empty();
        let asked = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|v| v.repaint_delay.is_zero());
        self.shell
            .dispatch(&mut self.host, &mut self.editors, intents);
        if acted || asked {
            // The window draws again at once after acting or when egui asks, so what it
            // shows (and where a press lands next) is what the frame made.
            self.again();
        }
        self.host.sent_since(from)
    }

    /// The frame the window draws right after one that acted or asked for it: no input, its
    /// own gestures carried out but not followed by another.
    fn again(&mut self) {
        self.serve();
        let input = egui::RawInput {
            screen_rect: Some(self.screen),
            time: Some(self.time),
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let mut intents = Vec::new();
        let (shell, host) = (&mut self.shell, &self.host);
        let mut output = self.ctx.run_ui(input, |root| {
            intents = ui::draw(root, shell, host);
        });
        output.textures_delta.clear();
        self.read(&output.shapes);
        self.shell
            .dispatch(&mut self.host, &mut self.editors, intents);
    }

    /// Keeps the texts a frame painted, and those it painted whole inside their clip.
    fn read(&mut self, shapes: &[egui::epaint::ClippedShape]) {
        self.texts.clear();
        self.whole.clear();
        for clipped in shapes {
            let from = self.texts.len();
            collect(&clipped.shape, &mut self.texts);
            self.whole.extend(
                self.texts[from..]
                    .iter()
                    .filter(|(_, rect, _)| clipped.clip_rect.contains_rect(*rect))
                    .map(|(text, rect, _)| (text.clone(), *rect)),
            );
        }
    }

    /// What the window does before each frame: takes in what the host reported, stamped with
    /// the window's clock.
    fn serve(&mut self) {
        let events = self.host.poll();
        self.shell.events(events, self.time, &mut self.editors);
    }

    /// One frame per entry, the commands of all of them in order.
    pub fn passes(&mut self, frames: Vec<Vec<Event>>) -> Vec<Command> {
        frames.into_iter().flat_map(|f| self.pass(f)).collect()
    }

    /// `n` frames with nothing happening (a popover's first frame sizes it unseen).
    pub fn idle(&mut self, n: usize) {
        for _ in 0..n {
            let _ = self.pass(Vec::new());
        }
    }

    /// Lets `seconds` go by on the window's clock, then draws a frame.
    pub fn wait(&mut self, seconds: f64) {
        self.quiet(seconds);
        self.idle(1);
    }

    /// Lets `seconds` go by on the window's clock with no frame and no poll, as they go by
    /// while the window draws nothing (the mixer idle, the daemon gone) or the host holds its
    /// thread; the next pass polls the host, then draws.
    pub fn quiet(&mut self, seconds: f64) {
        self.time += seconds;
    }

    /// The pointer moves to `at` and stays a frame.
    pub fn hover(&mut self, at: Pos2) -> Vec<Command> {
        self.pass(vec![Event::PointerMoved(at)])
    }

    /// A click at `at` in one frame: move there, press, release.
    pub fn click(&mut self, at: Pos2) -> Vec<Command> {
        let m = self.modifiers;
        self.pass(vec![
            Event::PointerMoved(at),
            button(at, true, m),
            button(at, false, m),
        ])
    }

    /// Two clicks at `at`, a frame apart.
    pub fn double_click(&mut self, at: Pos2) -> Vec<Command> {
        let mut sent = self.click(at);
        sent.extend(self.click(at));
        sent
    }

    /// A press at `from`, a move through each of `to` a frame each, the release at the last.
    pub fn drag(&mut self, from: Pos2, to: &[Pos2]) -> Vec<Command> {
        let m = self.modifiers;
        let mut frames = vec![vec![Event::PointerMoved(from), button(from, true, m)]];
        frames.extend(to.iter().map(|at| vec![Event::PointerMoved(*at)]));
        let last = *to.last().unwrap_or(&from);
        frames.push(vec![button(last, false, m)]);
        self.passes(frames)
    }

    /// `key` pressed and released with `modifiers`.
    pub fn key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Command> {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.pass(vec![event(true), event(false)])
    }

    /// `text` typed into the focused field.
    pub fn type_text(&mut self, text: &str) -> Vec<Command> {
        self.pass(vec![Event::Text(text.to_string())])
    }

    /// The middle of the last frame's text that reads exactly `text`, the last painted when
    /// several do (a popover paints over the console).
    pub fn text(&self, text: &str) -> Pos2 {
        self.find(text)
            .unwrap_or_else(|| panic!("no `{text}` drawn; drew {:?}", self.drawn()))
    }

    /// The rect of the last frame's text that reads exactly `text`.
    pub fn text_rect(&self, text: &str) -> Rect {
        self.texts
            .iter()
            .rev()
            .find(|(t, _, _)| t == text)
            .map(|(_, rect, _)| *rect)
            .unwrap_or_else(|| panic!("no `{text}` drawn; drew {:?}", self.drawn()))
    }

    /// The same, or `None`.
    pub fn find(&self, text: &str) -> Option<Pos2> {
        self.texts
            .iter()
            .rev()
            .find(|(t, _, _)| t == text)
            .map(|(_, rect, _)| rect.center())
    }

    /// Every text the last frame drew.
    pub fn drawn(&self) -> Vec<&str> {
        self.texts.iter().map(|(t, _, _)| t.as_str()).collect()
    }

    /// The setup as the host has it now.
    pub fn setup(&self) -> &Setup {
        self.host.setup()
    }

    /// The channel at position `index`.
    pub fn channel(&self, index: usize) -> ChannelId {
        self.setup().channels[index].id
    }

    /// The console as the last frame laid it out, for the setup as it is now.
    pub fn geometry(&self) -> Geometry {
        Geometry::new(self.shell.console, self.setup().channels.len())
    }

    /// How far the channels are scrolled: where the next frame draws them.
    pub fn scroll(&self) -> f32 {
        self.shell.gestures.scroll.offset
    }

    /// Where `strip` is drawn, scrolled as the channels are (the part out of view included).
    pub fn strip(&self, strip: Strip) -> Rect {
        self.geometry()
            .strip(strip, self.setup(), self.scroll())
            .expect("the strip is on the console")
    }

    /// Where the add-channel column is drawn.
    pub fn add_column(&self) -> Rect {
        self.geometry()
            .add_column(self.setup(), self.scroll())
            .expect("the console has room for a channel")
    }

    /// The travel of a fader: its column less the inset at each end.
    pub fn travel(&self, target: Target) -> f32 {
        self.fader(target).height() - 2.0 * size::FADER_INSET
    }

    /// A fader's column.
    pub fn fader(&self, target: Target) -> Rect {
        match target {
            Target::Send { ch, mix } => {
                let rows = Rows::of(self.strip(Strip::Channel(ch)));
                let [left, _, right] = strip::channel_columns(rows.faders);
                match mix {
                    MixId::Monitor => left,
                    MixId::Master => right,
                }
            }
            Target::Level(mix) => {
                strip::mix_columns(Rows::of(self.strip(Strip::Mix(mix))).faders)[0]
            }
        }
    }

    /// The point on a fader's column where its line sits now.
    pub fn fader_line(&self, target: Target) -> Pos2 {
        let column = self.fader(target);
        let position = match target {
            Target::Send { ch, mix } => self.setup().channel(ch).unwrap().send(mix).position,
            Target::Level(mix) => self.setup().mix(mix).level.position,
        };
        Pos2::new(column.center().x, strip::fader_y(column, position))
    }

    /// Insert `index`'s slot in `strip`'s well (the well unscrolled).
    pub fn slot(&self, strip: Strip, index: usize) -> Rect {
        let fx = Rows::of(self.strip(strip)).fx;
        let top = fx.top() + size::FX_HEAD + size::SLOTS_PAD + size::SLOT * index as f32;
        Rect::from_min_size(
            Pos2::new(fx.left() + size::SLOTS_PAD, top),
            vec2(fx.width() - 2.0 * size::SLOTS_PAD, size::SLOT),
        )
    }

    /// A strip's header.
    pub fn head(&self, strip: Strip) -> Rect {
        Rows::of(self.strip(strip)).head
    }

    /// A strip's button row.
    pub fn buttons(&self, strip: Strip) -> Rect {
        Rows::of(self.strip(strip)).buttons
    }
}

fn button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers,
    }
}

fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect, Color32)>) {
    match shape {
        egui::Shape::Text(text) => {
            let colour = text
                .galley
                .job
                .sections
                .first()
                .map_or(Color32::PLACEHOLDER, |s| s.format.color);
            out.push((
                text.galley.text().to_owned(),
                text.galley.rect.translate(text.pos.to_vec2()),
                colour,
            ));
        }
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

/// What the editors asked of a fake window; windows are numbered from 0 as they are made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowCall {
    Create {
        window: usize,
        title: String,
        size: [u32; 2],
    },
    Resize(usize, EditorSize),
    Show(usize),
    Title(usize, String),
    Raise(usize),
}

/// Editor windows with no event loop: each call the editors make of them is kept, in order.
#[derive(Clone, Default)]
pub struct FakeWindows {
    calls: Rc<RefCell<Vec<WindowCall>>>,
    made: Rc<RefCell<usize>>,
    /// Refuse to make a window, as a desktop out of resources does.
    refuse: bool,
    /// Make windows with no handle a plugin can draw into.
    handleless: bool,
}

impl FakeWindows {
    /// Windows that cannot be made.
    pub fn refusing() -> FakeWindows {
        FakeWindows {
            refuse: true,
            ..FakeWindows::default()
        }
    }

    /// Windows with no handle.
    pub fn without_handles() -> FakeWindows {
        FakeWindows {
            handleless: true,
            ..FakeWindows::default()
        }
    }

    /// The id of window `n`.
    pub fn id(n: usize) -> WindowId {
        WindowId::from(n as u64 + 1)
    }

    /// Every call so far.
    pub fn calls(&self) -> Vec<WindowCall> {
        self.calls.borrow().clone()
    }
}

struct FakeWindow {
    n: usize,
    calls: Rc<RefCell<Vec<WindowCall>>>,
    handleless: bool,
}

impl Opener for FakeWindows {
    fn create(&self, title: &str, size: [u32; 2]) -> Result<Box<dyn EditorWindow>, String> {
        if self.refuse {
            return Err("the desktop is out of windows".to_string());
        }
        let n = {
            let mut made = self.made.borrow_mut();
            *made += 1;
            *made - 1
        };
        self.calls.borrow_mut().push(WindowCall::Create {
            window: n,
            title: title.to_string(),
            size,
        });
        Ok(Box::new(FakeWindow {
            n,
            calls: Rc::clone(&self.calls),
            handleless: self.handleless,
        }))
    }
}

impl EditorWindow for FakeWindow {
    fn id(&self) -> WindowId {
        FakeWindows::id(self.n)
    }
    fn handle(&self) -> Result<RawWindowHandle, String> {
        if self.handleless {
            return Err("the window has not been mapped".to_string());
        }
        let hwnd = NonZeroIsize::new(self.n as isize + 1).expect("not zero");
        Ok(RawWindowHandle::Win32(Win32WindowHandle::new(hwnd)))
    }
    fn resize(&self, size: EditorSize) {
        self.calls
            .borrow_mut()
            .push(WindowCall::Resize(self.n, size));
    }
    fn show(&self) {
        self.calls.borrow_mut().push(WindowCall::Show(self.n));
    }
    fn set_title(&self, title: &str) {
        self.calls
            .borrow_mut()
            .push(WindowCall::Title(self.n, title.to_string()));
    }
    fn raise(&self) {
        self.calls.borrow_mut().push(WindowCall::Raise(self.n));
    }
}
