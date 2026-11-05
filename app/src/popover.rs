//! The popovers (`.pop`): one at a time, each hanging from what opened it.
//!
//! - **The source panel** under a channel's header: its name field (the header follows what
//!   is typed; the rename is sent when the panel closes), then the hardware inputs as radios
//!   or the app checklist ("Playing now", "Seen before", "in <channel>" beside an app another
//!   channel has, a search field), then Remove channel and Done.
//! - **The output panel** under a mix's header: its outputs as radios ("No device" first on
//!   Master, and "Apps can also record Master as an input."), then Done.
//! - **The add-channel menu** beside the add column: App channel, Hardware input.
//! - **The menu** under the header's menu button: Preferences (the quantum asked of the
//!   graph, the extra plugin folders) and Rescan plugins.
//! - **The plugin browser** beside a well's "+ Add plugin" ([`crate::browser`]).
//!
//! A press outside a popover (and outside what it hangs from) closes it, as does Escape,
//! which gives the focus back to what it hangs from.

use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Align2, Area, Context, Id, Key, Layout, Order, Pos2, Rect, ScrollArea, Sense, Ui, Vec2,
    pos2, vec2,
};
use vixmix_host::Host;
use vixmix_model::{
    AppRef, ChannelId, ChannelKind, Command, DeviceDirection, DeviceRef, MixId, Prefs, Setup,
    Source, Strip,
};

use crate::browser::{self, Browser};
use crate::console::Anchor;
use crate::icons::{self, Icon};
use crate::shell::Intent;
use crate::strip::{self, Spacing};
use crate::theme::{colour, font, pop, size};
use crate::widgets::{self, ButtonKind};

/// The longest name or search text a field takes.
pub const FIELD_LIMIT: usize = 64;

/// The open popover and what it holds between frames.
#[derive(Clone, Debug, PartialEq)]
pub enum Popover {
    Source(SourcePanel),
    Output(MixId),
    AddChannel,
    Menu,
    Prefs(PrefsPanel),
    Browser(Browser),
}

/// A channel's source panel.
#[derive(Clone, Debug, PartialEq)]
pub struct SourcePanel {
    pub ch: ChannelId,
    /// What the name field holds.
    pub name: String,
    /// The app search field.
    pub query: String,
    /// Put the focus in the name field with its text selected (a channel just added).
    pub focus_name: bool,
    /// Its first frame: the focus goes in.
    pub fresh: bool,
}

/// The preferences panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrefsPanel {
    /// The folder field.
    pub folder: String,
}

/// What a frame did to the popover.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Close,
    /// Close it without sending what it holds (its channel is going).
    Drop,
    /// Close it and open this one in its place.
    Open(Popover),
}

impl Popover {
    /// What it hangs from.
    pub fn anchor(&self) -> Anchor {
        match self {
            Popover::Source(panel) => Anchor::Head(Strip::Channel(panel.ch)),
            Popover::Output(mix) => Anchor::Head(Strip::Mix(*mix)),
            Popover::AddChannel => Anchor::AddColumn,
            Popover::Menu | Popover::Prefs(_) => Anchor::Menu,
            Popover::Browser(browser) => Anchor::AddPlugin(browser.strip),
        }
    }

    /// The popover `anchor` opens.
    pub fn of(anchor: Anchor, setup: &Setup) -> Option<Popover> {
        Some(match anchor {
            Anchor::Head(Strip::Channel(ch)) => {
                Popover::Source(SourcePanel::new(ch, &setup.channel(ch)?.name, false))
            }
            Anchor::Head(Strip::Mix(mix)) => Popover::Output(mix),
            Anchor::AddPlugin(strip) => Popover::Browser(Browser::new(strip)),
            Anchor::AddColumn => Popover::AddChannel,
            Anchor::Menu => Popover::Menu,
        })
    }

    /// What closing it sends: the source panel's rename, when its name changed.
    pub fn closing(&self, setup: &Setup) -> Option<Command> {
        let Popover::Source(panel) = self else {
            return None;
        };
        let name = panel.name.trim();
        let channel = setup.channel(panel.ch)?;
        (!name.is_empty() && name != channel.name).then(|| Command::Rename {
            ch: panel.ch,
            name: name.to_string(),
        })
    }

    /// Its width, and whether it opens beside what it hangs from rather than below.
    fn shape(&self) -> (f32, bool) {
        match self {
            Popover::Source(_) | Popover::Output(_) | Popover::Prefs(_) => {
                (pop::PANEL_WIDTH, false)
            }
            Popover::AddChannel => (pop::MENU_WIDTH, true),
            Popover::Menu => (pop::MENU_WIDTH, false),
            Popover::Browser(_) => (pop::BROWSER[0], true),
        }
    }

    fn area_id(&self) -> Id {
        Id::new((
            "popover",
            match self {
                Popover::Source(_) => 0,
                Popover::Output(_) => 1,
                Popover::AddChannel => 2,
                Popover::Menu => 3,
                Popover::Prefs(_) => 4,
                Popover::Browser(_) => 5,
            },
        ))
    }
}

impl SourcePanel {
    /// The panel of `ch`, its name field holding `name`.
    pub fn new(ch: ChannelId, name: &str, focus_name: bool) -> SourcePanel {
        SourcePanel {
            ch,
            name: name.to_string(),
            query: String::new(),
            focus_name,
            fresh: true,
        }
    }
}

/// What the popover's controls need.
pub struct PopCx<'a> {
    pub host: &'a dyn Host,
    pub out: &'a mut Vec<Intent>,
    pub keyboard: bool,
    /// The favourites as the window shows them.
    pub favorites: &'a dyn Fn(&str) -> bool,
}

/// Where a popover of `size` goes: below `anchor` (its left edges lined up) or beside it
/// (to its right, or its left when the right has no room), kept inside `screen` with a
/// margin.
pub fn place(anchor: Rect, size: Vec2, beside: bool, screen: Rect) -> Pos2 {
    let margin = pop::EDGE_MARGIN;
    let (mut x, mut y) = if beside {
        (anchor.right() + pop::BESIDE_GAP, anchor.top())
    } else {
        (anchor.left(), anchor.bottom() + pop::BELOW_GAP)
    };
    if beside && x + size.x > screen.right() - margin {
        x = anchor.left() - size.x - pop::BESIDE_GAP;
    }
    x = x
        .min(screen.right() - size.x - margin)
        .max(screen.left() + margin);
    y = y
        .min(screen.bottom() - size.y - margin)
        .max(screen.top() + margin);
    pos2(x, y)
}

/// Draws the popover hanging from `anchor`; returns what happens to it and the rect it took.
pub fn show(
    ctx: &Context,
    popover: &mut Popover,
    anchor: Rect,
    screen: Rect,
    cx: &mut PopCx<'_>,
) -> (Outcome, Rect) {
    let id = popover.area_id();
    let (width, beside) = popover.shape();
    let height = ctx.memory(|m| m.area_rect(id)).map_or(0.0, |r| r.height());
    let at = place(anchor, vec2(width, height), beside, screen);
    let mut outcome = Outcome::Stay;
    let response = Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            widgets::popover_frame().show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.set_width(width);
                outcome = match popover {
                    Popover::Source(panel) => source_panel(ui, panel, cx),
                    Popover::Output(mix) => output_panel(ui, *mix, cx),
                    Popover::AddChannel => add_channel_menu(ui, cx),
                    Popover::Menu => main_menu(ui, cx),
                    Popover::Prefs(panel) => prefs_panel(ui, panel, cx),
                    Popover::Browser(browser) => browser::show(ui, browser, cx),
                };
            });
        })
        .response;
    if ctx.input(|i| i.key_pressed(Key::Escape)) && outcome == Outcome::Stay {
        outcome = Outcome::Close;
    }
    (outcome, response.rect)
}

/// The name row (`.nm-row`): a field for a channel, the fixed name for a mix.
fn name_row(ui: &mut Ui, name: NameRow<'_>) -> Option<egui::Response> {
    let [pad_x, pad_y] = pop::NAME_PAD;
    let (rect, _) = ui.allocate_exact_size(
        vec2(
            ui.available_width(),
            pop::FIELD_HEIGHT + 2.0 * pad_y + size::HAIRLINE,
        ),
        Sense::hover(),
    );
    let painter = ui.painter().clone();
    widgets::rule(&painter, rect.x_range(), rect.bottom() - size::HAIRLINE);
    let y = rect.top() + pad_y + pop::FIELD_HEIGHT / 2.0;
    match name {
        NameRow::Fixed(text) => {
            let text = strip::galley(
                &painter,
                text,
                font::sans_semibold(font::NAME),
                colour::TEXT,
                Spacing::None,
            );
            strip::place(
                &painter,
                text,
                Align2::LEFT_CENTER,
                pos2(rect.left() + pad_x, y),
            );
            None
        }
        NameRow::Field { id, text } => {
            let label = strip::galley(
                &painter,
                "Name",
                font::sans(font::SMALL),
                colour::DIM,
                Spacing::None,
            );
            let label_width = label.size().x;
            strip::place(
                &painter,
                label,
                Align2::LEFT_CENTER,
                pos2(rect.left() + pad_x, y),
            );
            let field = Rect::from_min_max(
                pos2(
                    rect.left() + pad_x + label_width + pop::NAME_GAP,
                    y - pop::FIELD_HEIGHT / 2.0,
                ),
                pos2(rect.right() - pad_x, y + pop::FIELD_HEIGHT / 2.0),
            );
            let focused = ui.memory(|m| m.has_focus(id));
            painter.rect_filled(field, size::CORNER, colour::BLACK);
            painter.rect_stroke(
                field,
                size::CORNER,
                egui::Stroke::new(
                    size::HAIRLINE,
                    if focused {
                        colour::ACCENT
                    } else {
                        colour::LINE
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let inner = field.shrink2(vec2(pop::FIELD_PAD_X, size::HAIRLINE));
            let edit = egui::TextEdit::singleline(text)
                .id(id)
                .frame(egui::Frame::NONE)
                .font(font::sans(font::NAME))
                .text_color(colour::TEXT)
                .char_limit(FIELD_LIMIT)
                .margin(egui::Margin::ZERO)
                .desired_width(inner.width())
                .vertical_align(Align::Center);
            Some(ui.put(inner, edit))
        }
    }
}

enum NameRow<'a> {
    Fixed(&'a str),
    Field { id: Id, text: &'a mut String },
}

/// The search row (`.search`): the magnifier and a field.
fn search_row(ui: &mut Ui, id: Id, text: &mut String, hint: &str) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), pop::SEARCH_HEIGHT),
        Sense::hover(),
    );
    let painter = ui.painter().clone();
    widgets::rule(&painter, rect.x_range(), rect.bottom() - size::HAIRLINE);
    let y = rect.center().y - size::HAIRLINE_INSET;
    let icon = Rect::from_center_size(
        pos2(rect.left() + pop::SEARCH_PAD_X + pop::SEARCH_ICON / 2.0, y),
        vec2(pop::SEARCH_ICON, pop::SEARCH_ICON),
    );
    icons::paint(&painter, &icons::SEARCH, icon, colour::FAINT);
    let field = Rect::from_min_max(
        pos2(icon.right() + pop::SEARCH_GAP, y - pop::FIELD_HEIGHT / 2.0),
        pos2(
            rect.right() - pop::SEARCH_PAD_X,
            y + pop::FIELD_HEIGHT / 2.0,
        ),
    );
    widgets::text_field(ui, field, id, text, font::sans(font::SEARCH), hint)
}

/// The widget id of a popover's search field.
pub fn search_id(name: &str) -> Id {
    Id::new(("search", name))
}

/// The widget id of the source panel's name field.
pub fn name_id() -> Id {
    Id::new("channel name field")
}

/// A list heading row (`.ap-h`): `inside` a list it is padded 6 across, outside one 10.
fn heading_row(ui: &mut Ui, text: &str, inside: bool) {
    let painter = ui.painter().clone();
    let line = widgets::heading_height(&painter);
    let (rect, _) = ui.allocate_exact_size(
        vec2(
            ui.available_width(),
            pop::HEADING_TOP + line + pop::HEADING_BOTTOM,
        ),
        Sense::hover(),
    );
    let pad = if inside {
        pop::HEADING_PAD_X
    } else {
        pop::HEADING_OUTSIDE_PAD_X
    };
    widgets::heading(
        &painter,
        rect.left() + pad,
        rect.top() + pop::HEADING_TOP + line / 2.0,
        text,
    );
}

/// What a list row leads with.
enum Mark {
    Tick(bool),
    Radio(bool),
}

/// One row of a panel's list (`.ap-row`): its mark, an app's badge, the label (faint when
/// `off`), a note at the right; returns whether it was pressed.
#[allow(clippy::too_many_arguments)]
fn list_row(
    ui: &mut Ui,
    id: Id,
    mark: Mark,
    badge: Option<char>,
    label: &str,
    off: bool,
    note: Option<&str>,
    keyboard: bool,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), pop::ROW), Sense::hover());
    let response = widgets::press(ui, rect, id);
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, size::CORNER, colour::CTL);
    }
    let y = rect.center().y;
    let mut x = rect.left() + pop::ROW_PAD_X;
    let centre = pos2(x + pop::TICK / 2.0, y);
    match mark {
        Mark::Tick(on) => widgets::tick_box(painter, centre, on),
        Mark::Radio(on) => widgets::radio(painter, centre, on),
    }
    x += pop::TICK + pop::ROW_GAP;
    if let Some(initial) = badge {
        let square =
            Rect::from_min_size(pos2(x, y - pop::BADGE / 2.0), vec2(pop::BADGE, pop::BADGE));
        painter.rect_filled(square, size::CORNER, colour::CTL_HI);
        let text = strip::galley(
            painter,
            &initial.to_string(),
            font::sans_semibold(font::HEADING),
            colour::DIM,
            Spacing::None,
        );
        strip::place(painter, text, Align2::CENTER_CENTER, square.center());
        x = square.right() + pop::ROW_GAP;
    }
    let mut end = rect.right() - pop::ROW_PAD_X;
    if let Some(note) = note {
        let text = strip::galley(
            painter,
            note,
            font::sans(font::NOTE),
            colour::FAINT,
            Spacing::None,
        );
        let width = text.size().x;
        strip::place(painter, text, Align2::RIGHT_CENTER, pos2(end, y));
        end -= width + pop::ROW_GAP;
    }
    let ink = if off { colour::FAINT } else { colour::TEXT };
    let text = strip::truncated(painter, label, font::sans(font::BODY), ink, end - x);
    strip::place(painter, text, Align2::LEFT_CENTER, pos2(x, y));
    if keyboard && response.has_focus() {
        widgets::ring(painter, rect, 0.0);
    }
    response.clicked()
}

/// A note line (`.ap-h` without its capitals): faint, 10 points.
fn note_row(ui: &mut Ui, text: &str) {
    let painter = ui.painter().clone();
    let width = ui.available_width() - 2.0 * pop::HEADING_OUTSIDE_PAD_X;
    let mut job = egui::text::LayoutJob::simple(
        text.to_string(),
        font::sans(font::HEADING),
        colour::FAINT,
        width,
    );
    job.wrap.max_width = width;
    let galley = painter.layout_job(job);
    let (rect, _) = ui.allocate_exact_size(
        vec2(
            ui.available_width(),
            pop::NOTE_TOP + galley.size().y + pop::NOTE_BOTTOM,
        ),
        Sense::hover(),
    );
    painter.galley(
        pos2(
            rect.left() + pop::HEADING_OUTSIDE_PAD_X,
            rect.top() + pop::NOTE_TOP,
        ),
        galley,
        colour::FAINT,
    );
}

/// The footer (`.ap-foot`): Remove channel at the left when given, Done at the right.
/// Returns which was pressed.
fn footer(ui: &mut Ui, remove: bool, keyboard: bool) -> (bool, bool) {
    let [pad_x, pad_y] = pop::FOOT_PAD;
    let (rect, _) = ui.allocate_exact_size(
        vec2(
            ui.available_width(),
            size::HAIRLINE + 2.0 * pad_y + pop::BUTTON_HEIGHT,
        ),
        Sense::hover(),
    );
    widgets::rule(ui.painter(), rect.x_range(), rect.top());
    let y = rect.center().y + size::HAIRLINE_INSET;
    let mut removed = false;
    if remove {
        let response = widgets::button(
            ui,
            Id::new("remove channel"),
            rect.left() + pad_x,
            y,
            "Remove channel",
            ButtonKind::Danger,
        );
        removed = response.clicked();
    }
    let done_width = widgets::button_width(ui.painter(), "Done");
    let done = widgets::button(
        ui,
        Id::new("popover done"),
        rect.right() - pad_x - done_width,
        y,
        "Done",
        ButtonKind::Plain,
    );
    if keyboard && done.has_focus() {
        widgets::ring(ui.painter(), done.rect, size::FOCUS_OFFSET);
    }
    (removed, done.clicked())
}

/// Puts the focus in `id` on a popover's first frame; with `select`, its text selected.
fn focus_first(ui: &Ui, id: Id, select: Option<usize>) {
    ui.memory_mut(|m| m.request_focus(id));
    if let Some(len) = select {
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
        egui::TextEdit::store_state(ui.ctx(), id, state);
    }
}

/// A channel's source panel.
fn source_panel(ui: &mut Ui, panel: &mut SourcePanel, cx: &mut PopCx<'_>) -> Outcome {
    let setup = cx.host.setup();
    let Some(channel) = setup.channel(panel.ch) else {
        return Outcome::Drop;
    };
    let fresh = std::mem::take(&mut panel.fresh);
    let name = name_row(
        ui,
        NameRow::Field {
            id: name_id(),
            text: &mut panel.name,
        },
    )
    .expect("a field");
    let mut outcome = Outcome::Stay;
    if name.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        outcome = Outcome::Close;
    }
    if fresh && panel.focus_name {
        focus_first(ui, name_id(), Some(panel.name.chars().count()));
    }
    match &channel.source {
        Source::Apps(ticked) => {
            let search = search_id("apps");
            search_row(ui, search, &mut panel.query, "Find an app");
            if fresh && !panel.focus_name {
                focus_first(ui, search, None);
            }
            let query = panel.query.trim().to_lowercase();
            let hits: Vec<_> = cx
                .host
                .apps()
                .iter()
                .filter(|p| p.app.name.to_lowercase().contains(&query))
                .collect();
            let mut send = None;
            list(ui, "app list", |ui| {
                if hits.is_empty() {
                    empty_row(ui, "No app matches.");
                }
                for (heading, running) in [("Playing now", true), ("Seen before", false)] {
                    let group: Vec<_> = hits.iter().filter(|p| p.running == running).collect();
                    if group.is_empty() {
                        continue;
                    }
                    heading_row(ui, heading, true);
                    for presence in group {
                        let app = &presence.app;
                        let on = ticked.iter().any(|a| a.matches(app));
                        let owner = setup
                            .channel_of_app(app)
                            .filter(|id| *id != panel.ch)
                            .and_then(|id| setup.channel(id))
                            .map(|c| format!("in {}", c.name));
                        let pressed = list_row(
                            ui,
                            Id::new(("app", app)),
                            Mark::Tick(on),
                            app.name.chars().next(),
                            &app.name,
                            !presence.running,
                            owner.as_deref(),
                            cx.keyboard,
                        );
                        if pressed {
                            send = Some(Command::SetSource {
                                ch: panel.ch,
                                source: Source::Apps(ticking(ticked, app, !on)),
                            });
                        }
                    }
                }
            });
            if let Some(command) = send {
                cx.out.push(Intent::Apply(command));
            }
        }
        Source::Hardware(current) => {
            heading_row(ui, "Input", false);
            let rows = devices(cx.host, DeviceDirection::Input, current.as_ref());
            let mut send = None;
            list(ui, "input list", |ui| {
                for (device, present) in &rows {
                    let on = current.as_ref().is_some_and(|c| c.name == device.name);
                    let pressed = list_row(
                        ui,
                        Id::new(("input", &device.name)),
                        Mark::Radio(on),
                        None,
                        &device.description,
                        !present,
                        (!present).then_some("not connected"),
                        cx.keyboard,
                    );
                    if pressed && !on {
                        send = Some(Command::SetSource {
                            ch: panel.ch,
                            source: Source::Hardware(Some(device.clone())),
                        });
                    }
                }
            });
            if let Some(command) = send {
                cx.out.push(Intent::Apply(command));
            }
        }
    }
    let (removed, done) = footer(ui, true, cx.keyboard);
    if removed {
        cx.out.push(Intent::Apply(Command::RemoveChannel(panel.ch)));
        return Outcome::Drop;
    }
    if done {
        outcome = Outcome::Close;
    }
    outcome
}

/// The apps of a channel with `app` ticked or not.
pub fn ticking(ticked: &[AppRef], app: &AppRef, on: bool) -> Vec<AppRef> {
    let mut apps: Vec<AppRef> = ticked.iter().filter(|a| !a.matches(app)).cloned().collect();
    if on {
        apps.push(app.clone());
    }
    apps
}

/// The devices a list offers in `direction`, each with whether it is present: the one in
/// use first when it is not (unplugged), then every present one.
fn devices(
    host: &dyn Host,
    direction: DeviceDirection,
    current: Option<&DeviceRef>,
) -> Vec<(DeviceRef, bool)> {
    let present: Vec<DeviceRef> = host
        .devices()
        .iter()
        .filter(|d| d.direction == direction)
        .map(|d| d.device.clone())
        .collect();
    let mut rows = Vec::with_capacity(present.len() + 1);
    if let Some(current) = current
        && !present.iter().any(|d| d.name == current.name)
    {
        rows.push((current.clone(), false));
    }
    rows.extend(present.into_iter().map(|d| (d, true)));
    rows
}

/// A panel's scrolling list (`.ap-list`): padded, at most 300 points tall.
fn list(ui: &mut Ui, salt: &str, add: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    ScrollArea::vertical()
        .id_salt(salt)
        .max_height(pop::LIST_MAX)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(pop::LIST_PAD as i8))
                .show(ui, |ui| {
                    ui.set_width(width - 2.0 * pop::LIST_PAD);
                    add(ui);
                });
        });
}

/// An empty list's message (`.br-empty`).
fn empty_row(ui: &mut Ui, text: &str) {
    let painter = ui.painter().clone();
    let [pad_x, pad_y] = pop::EMPTY_PAD;
    let text = strip::galley(
        &painter,
        text,
        font::sans(font::BODY),
        colour::DIM,
        Spacing::None,
    );
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), text.size().y + 2.0 * pad_y),
        Sense::hover(),
    );
    strip::place(
        &painter,
        text,
        Align2::LEFT_CENTER,
        pos2(rect.left() + pad_x, rect.center().y),
    );
}

/// A mix's output panel.
fn output_panel(ui: &mut Ui, mix: MixId, cx: &mut PopCx<'_>) -> Outcome {
    let setup = cx.host.setup();
    let current = setup.mix(mix).device.as_ref();
    name_row(ui, NameRow::Fixed(mix.name()));
    heading_row(ui, "Output", false);
    let rows = devices(cx.host, DeviceDirection::Output, current);
    let mut send = None;
    list(ui, "output list", |ui| {
        if mix == MixId::Master
            && list_row(
                ui,
                Id::new("no device"),
                Mark::Radio(current.is_none()),
                None,
                "No device",
                false,
                None,
                cx.keyboard,
            )
            && current.is_some()
        {
            send = Some(Command::SetMixDevice { mix, device: None });
        }
        for (device, present) in &rows {
            let on = current.is_some_and(|c| c.name == device.name);
            let pressed = list_row(
                ui,
                Id::new(("output", &device.name)),
                Mark::Radio(on),
                None,
                &device.description,
                !present,
                (!present).then_some("not connected"),
                cx.keyboard,
            );
            if pressed && !on {
                send = Some(Command::SetMixDevice {
                    mix,
                    device: Some(device.clone()),
                });
            }
        }
    });
    if mix == MixId::Master {
        note_row(ui, "Apps can also record Master as an input.");
    }
    if let Some(command) = send {
        cx.out.push(Intent::Apply(command));
    }
    let (_, done) = footer(ui, false, cx.keyboard);
    if done { Outcome::Close } else { Outcome::Stay }
}

/// A menu row (`.menu button`): an icon when given, the label; faint and inert when not
/// `enabled`. Returns whether it was pressed.
fn menu_row(
    ui: &mut Ui,
    id: Id,
    icon: Option<&Icon>,
    label: &str,
    enabled: bool,
    keyboard: bool,
) -> egui::Response {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), pop::MENU_ROW), Sense::hover());
    let response = if enabled {
        widgets::press(ui, rect, id)
    } else {
        ui.interact(rect, id, Sense::hover())
    };
    let painter = ui.painter();
    if enabled && response.hovered() {
        painter.rect_filled(rect, size::CORNER, colour::CTL);
    }
    let y = rect.center().y;
    let mut x = rect.left() + pop::MENU_ROW_PAD_X;
    if let Some(icon) = icon {
        let icon_rect = Rect::from_center_size(
            pos2(x + pop::MENU_ICON / 2.0, y),
            vec2(pop::MENU_ICON, pop::MENU_ICON),
        );
        icons::paint(painter, icon, icon_rect, colour::DIM);
        x = icon_rect.right() + pop::MENU_GAP;
    }
    let ink = if enabled { colour::TEXT } else { colour::FAINT };
    let text = strip::galley(painter, label, font::sans(font::BODY), ink, Spacing::None);
    strip::place(painter, text, Align2::LEFT_CENTER, pos2(x, y));
    if keyboard && response.has_focus() {
        widgets::ring(painter, rect, 0.0);
    }
    response
}

/// A menu's padded column of rows.
fn menu(ui: &mut Ui, rows: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(pop::MENU_PAD as i8))
        .show(ui, |ui| {
            ui.with_layout(Layout::top_down(Align::Min), rows);
        });
}

/// The add-channel menu: App channel, Hardware input.
fn add_channel_menu(ui: &mut Ui, cx: &mut PopCx<'_>) -> Outcome {
    let mut picked = None;
    menu(ui, |ui| {
        for (kind, icon, label) in [
            (ChannelKind::Apps, &icons::APPS, "App channel"),
            (ChannelKind::Hardware, &icons::MIC, "Hardware input"),
        ] {
            if menu_row(
                ui,
                Id::new(("add", label)),
                Some(icon),
                label,
                true,
                cx.keyboard,
            )
            .clicked()
            {
                picked = Some(kind);
            }
        }
    });
    match picked {
        Some(kind) => {
            cx.out.push(Intent::AddChannel(kind));
            Outcome::Close
        }
        None => Outcome::Stay,
    }
}

/// The header's menu: Preferences, and Rescan plugins (which waits for the host to offer a
/// rescan; see the build map's change request 8).
fn main_menu(ui: &mut Ui, cx: &mut PopCx<'_>) -> Outcome {
    let mut outcome = Outcome::Stay;
    menu(ui, |ui| {
        if menu_row(
            ui,
            Id::new("preferences"),
            None,
            "Preferences",
            true,
            cx.keyboard,
        )
        .clicked()
        {
            outcome = Outcome::Open(Popover::Prefs(PrefsPanel::default()));
        }
        menu_row(
            ui,
            Id::new("rescan"),
            None,
            "Rescan plugins",
            false,
            cx.keyboard,
        )
        .on_hover_text("Not in this build yet: the host cannot rescan while it runs");
    });
    outcome
}

/// The preferences: the quantum asked of the graph, and the extra plugin folders.
fn prefs_panel(ui: &mut Ui, panel: &mut PrefsPanel, cx: &mut PopCx<'_>) -> Outcome {
    let prefs = &cx.host.setup().prefs;
    name_row(ui, NameRow::Fixed("Preferences"));
    heading_row(ui, "Quantum", false);
    let mut send = None;
    list(ui, "quantum list", |ui| {
        for quantum in Prefs::QUANTA {
            let on = prefs.quantum == quantum;
            if list_row(
                ui,
                Id::new(("quantum", quantum)),
                Mark::Radio(on),
                None,
                &format!("{quantum} samples"),
                false,
                None,
                cx.keyboard,
            ) && !on
            {
                send = Some(Prefs {
                    quantum,
                    ..prefs.clone()
                });
            }
        }
    });
    note_row(
        ui,
        "Asked of PipeWire at 48 kHz; the graph has the last word.",
    );
    heading_row(ui, "Plugin folders", false);
    list(ui, "folder list", |ui| {
        if prefs.plugin_dirs.is_empty() {
            empty_row(ui, "Only the standard places.");
        }
        for dir in &prefs.plugin_dirs {
            if folder_row(ui, &dir.to_string_lossy(), cx.keyboard) {
                send = Some(Prefs {
                    plugin_dirs: prefs
                        .plugin_dirs
                        .iter()
                        .filter(|d| *d != dir)
                        .cloned()
                        .collect(),
                    ..prefs.clone()
                });
            }
        }
    });
    let field = search_id("folder");
    let added = folder_field(ui, field, &mut panel.folder);
    if added {
        let folder = panel.folder.trim();
        let path = std::path::PathBuf::from(folder);
        if !folder.is_empty() && !prefs.plugin_dirs.contains(&path) {
            let mut plugin_dirs = prefs.plugin_dirs.clone();
            plugin_dirs.push(path);
            send = Some(Prefs {
                plugin_dirs,
                ..prefs.clone()
            });
        }
        panel.folder.clear();
    }
    note_row(ui, "Scanned besides the standard places.");
    if let Some(prefs) = send {
        cx.out.push(Intent::Apply(Command::SetPrefs(prefs)));
    }
    let (_, done) = footer(ui, false, cx.keyboard);
    if done { Outcome::Close } else { Outcome::Stay }
}

/// A plugin folder with its bin; returns whether the bin was pressed.
fn folder_row(ui: &mut Ui, path: &str, keyboard: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), pop::ROW), Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    let bin = Rect::from_center_size(
        pos2(rect.right() - pop::ROW_PAD_X - size::HOVER_BUTTON / 2.0, y),
        vec2(size::HOVER_BUTTON, size::HOVER_BUTTON),
    );
    let response = widgets::press(ui, bin, Id::new(("folder", path))).on_hover_text("Remove");
    let ink = if response.hovered() {
        painter.rect_filled(bin, size::CORNER, colour::CTL_HI);
        colour::TEXT
    } else {
        colour::DIM
    };
    icons::paint(
        painter,
        &icons::TRASH,
        Rect::from_center_size(bin.center(), vec2(size::HOVER_ICON, size::HOVER_ICON)),
        ink,
    );
    let x = rect.left() + pop::ROW_PAD_X;
    let text = strip::truncated(
        painter,
        path,
        font::mono(font::NOTE),
        colour::TEXT,
        bin.left() - pop::ROW_GAP - x,
    );
    strip::place(painter, text, Align2::LEFT_CENTER, pos2(x, y));
    if keyboard && response.has_focus() {
        widgets::ring(painter, bin, 0.0);
    }
    response.clicked()
}

/// The field for another plugin folder and its Add button; returns whether one was added
/// (Add, or Enter in the field).
fn folder_field(ui: &mut Ui, id: Id, text: &mut String) -> bool {
    let [pad_x, pad_y] = pop::NAME_PAD;
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), pop::FIELD_HEIGHT + pad_y),
        Sense::hover(),
    );
    let y = rect.top() + pop::FIELD_HEIGHT / 2.0;
    let add_width = widgets::button_width(ui.painter(), "Add");
    let add = widgets::button(
        ui,
        Id::new("add folder"),
        rect.right() - pad_x - add_width,
        y,
        "Add",
        ButtonKind::Plain,
    );
    let field = Rect::from_min_max(
        pos2(rect.left() + pad_x, y - pop::FIELD_HEIGHT / 2.0),
        pos2(add.rect.left() - pop::NAME_GAP, y + pop::FIELD_HEIGHT / 2.0),
    );
    let focused = ui.memory(|m| m.has_focus(id));
    let painter = ui.painter().clone();
    painter.rect_filled(field, size::CORNER, colour::BLACK);
    painter.rect_stroke(
        field,
        size::CORNER,
        egui::Stroke::new(
            size::HAIRLINE,
            if focused {
                colour::ACCENT
            } else {
                colour::LINE
            },
        ),
        egui::StrokeKind::Inside,
    );
    let inner = field.shrink2(vec2(pop::FIELD_PAD_X, size::HAIRLINE));
    let response = widgets::text_field(
        ui,
        inner,
        id,
        text,
        font::mono(font::NOTE),
        "Another folder to scan",
    );
    add.clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::example;

    #[test]
    fn a_popover_opens_below_or_beside_its_anchor_and_stays_on_screen() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(900.0, 490.0));
        let head = Rect::from_min_size(pos2(12.0, 41.0), vec2(92.0, 22.0));
        // Below, left edges lined up, 2 points down.
        assert_eq!(
            place(head, vec2(270.0, 200.0), false, screen),
            pos2(12.0, 65.0)
        );
        // Beside, 4 points right of it.
        let add = Rect::from_min_size(pos2(110.0, 120.0), vec2(88.0, 20.0));
        assert_eq!(place(add, vec2(620.0, 440.0), true, screen).x, 202.0);
        // No room on the right: to its left; still kept 8 inside the window.
        let far = Rect::from_min_size(pos2(700.0, 120.0), vec2(88.0, 20.0));
        let at = place(far, vec2(620.0, 440.0), true, screen);
        assert_eq!(at.x, 700.0 - 620.0 - 4.0);
        assert_eq!(at.y, 490.0 - 440.0 - 8.0, "pushed up to fit");
        let master = Rect::from_min_size(pos2(780.0, 41.0), vec2(100.0, 22.0));
        assert_eq!(
            place(master, vec2(270.0, 200.0), false, screen).x,
            900.0 - 270.0 - 8.0
        );
    }

    #[test]
    fn ticking_an_app_adds_it_once_and_unticking_takes_it_out() {
        let spotify = example::app("spotify");
        let firefox = example::app("firefox");
        assert_eq!(ticking(&[], &spotify, true), vec![spotify.clone()]);
        assert_eq!(
            ticking(std::slice::from_ref(&spotify), &spotify, true),
            vec![spotify.clone()]
        );
        assert_eq!(
            ticking(&[spotify.clone(), firefox.clone()], &spotify, false),
            vec![firefox]
        );
    }

    #[test]
    fn closing_the_source_panel_renames_only_when_the_name_changed() {
        let setup = example::setup();
        let mic = setup.channels[0].id;
        let mut panel = SourcePanel::new(mic, "Mic", false);
        assert_eq!(Popover::Source(panel.clone()).closing(&setup), None);
        panel.name = "   ".to_string();
        assert_eq!(
            Popover::Source(panel.clone()).closing(&setup),
            None,
            "an empty name keeps the old one"
        );
        panel.name = " Main mic ".to_string();
        assert_eq!(
            Popover::Source(panel).closing(&setup),
            Some(Command::Rename {
                ch: mic,
                name: "Main mic".to_string()
            })
        );
    }
}
