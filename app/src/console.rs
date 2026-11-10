//! The console row, laid out as prototype v5's `.console`: the channel strips left to right,
//! the add-channel column after them while there is room for another channel, then — past a
//! wider gap — Monitor and Master. Black shows between the strips.
//!
//! **It fills the window** ([`Geometry`]). Monitor and Master sit against the right edge and
//! never scroll away; the channels and the add column take what is left of the width from
//! the left, and scroll sideways ([`Scroll`]) when they are more than it shows. Every strip
//! takes the height, its well a little more of it and its faders the rest
//! ([`Rows::heights`](crate::strip::Rows::heights)). At the console's natural size (the
//! prototype's, [`natural_size`]) every rect is the prototype's and nothing scrolls.
//!
//! Each frame draws the strips from the host's setup and senses their gestures: what a
//! gesture asks becomes an [`Intent`] in [`Cx::out`], the rects a popover hangs from and a
//! drag lands in are recorded in [`Layout`] (only what shows of them), and a drag, a fader's
//! travel and the channels' scroll carry over to the next frame in [`Gestures`]. After the
//! last strip the insert drag is resolved ([`drag::resolve`]), because where it lands
//! depends on every strip.

use egui::containers::scroll_area::{DragScroll, ScrollSource};
use egui::{CornerRadius, Id, Rect, ScrollArea, Ui, UiBuilder, Vec2, pos2, vec2};
use vixmix_host::{AppPresence, Host};
use vixmix_model::{ChannelId, MAX_CHANNELS, MixId, Setup, Source, Strip};

use crate::drag::{self, InsertDrag};
use crate::fader;
use crate::meters::Meters;
use crate::shell::Intent;
use crate::strip::{self, ChannelView, MixView};
use crate::theme::{self, colour, size};

/// What a popover hangs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    /// A strip's header: a channel's name and source panel, a mix's output panel.
    Head(Strip),
    /// A well's "+ Add plugin": the plugin browser.
    AddPlugin(Strip),
    /// The add-channel column: its menu.
    AddColumn,
    /// The header's menu button: the menu, and the preferences it opens.
    Menu,
}

/// One well's rects this frame: where a dragged insert may land.
#[derive(Clone, Debug, PartialEq)]
pub struct WellRects {
    pub strip: Strip,
    /// The whole strip: a drop anywhere on it lands in its well.
    pub strip_rect: Rect,
    /// What shows of the strip: a channel scrolled partly out of view takes a drop only
    /// where it shows.
    pub clip: Rect,
    /// The list under the FX header (its padding included): outlined when a drag from
    /// another strip is over it.
    pub list: Rect,
    /// Each insert's slot, scrolled into place (some may lie outside `list`).
    pub slots: Vec<Rect>,
    /// "+ Add plugin".
    pub add: Rect,
}

/// The rects one frame recorded.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub anchors: Vec<(Anchor, Rect)>,
    pub wells: Vec<WellRects>,
}

impl Layout {
    /// Where `anchor` was drawn this frame.
    pub fn anchor(&self, anchor: Anchor) -> Option<Rect> {
        self.anchors
            .iter()
            .find(|(a, _)| *a == anchor)
            .map(|(_, r)| *r)
    }
}

/// The gestures that last more than a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gestures {
    /// A fader being dragged.
    pub fader: Option<fader::Drag>,
    /// An insert pressed, and once it has moved, being dragged.
    pub insert: Option<InsertDrag>,
    /// How far the channels are scrolled.
    pub scroll: Scroll,
}

/// The channels' sideways scroll, from frame to frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scroll {
    /// How far the channels are scrolled, in points from their start: where the last frame
    /// left it (the wheel and the bar included), drawn by the next.
    pub offset: f32,
    /// A channel to scroll whole into view before the next frame draws (one just added).
    pub reveal: Option<ChannelId>,
    /// The widget that had the focus last frame: a Tab that moves it into a channel out of
    /// view scrolls that channel in, once.
    pub focused: Option<Id>,
}

/// What a strip's controls need besides their strip.
pub struct Cx<'a> {
    pub host: &'a dyn Host,
    /// What the gestures ask, in order.
    pub out: &'a mut Vec<Intent>,
    pub gestures: &'a mut Gestures,
    pub layout: &'a mut Layout,
    /// Show the focus ring (the last thing the user did was the keyboard's).
    pub keyboard: bool,
    /// The anchor whose popover is open: its header stays lit, its caret shown.
    pub open: Option<Anchor>,
}

impl Cx<'_> {
    /// Records where `anchor` shows this frame: `rect` clipped to what `ui` shows (a channel
    /// scrolled partly out of view), and nothing when none of it shows, so its popover
    /// closes.
    pub fn anchor(&mut self, ui: &Ui, anchor: Anchor, rect: Rect) {
        let shown = rect.intersect(ui.clip_rect());
        if shown.is_positive() {
            self.layout.anchors.push((anchor, shown));
        }
    }
}

/// What the console draws.
pub struct ConsoleView<'a> {
    pub setup: &'a Setup,
    pub meters: &'a Meters,
    pub apps: &'a [AppPresence],
    /// A channel whose name field is being edited, and what it holds.
    pub renaming: Option<(ChannelId, &'a str)>,
}

/// Whether the add-channel column shows: while fewer than eight channels exist.
pub fn has_add_column(channels: usize) -> bool {
    channels < MAX_CHANNELS
}

/// The width of the channels and the add column, the padding before them and the gap after
/// each included: what the channels' view scrolls.
fn channels_width(channels: usize) -> f32 {
    let add = if has_add_column(channels) {
        size::ADD_COLUMN + size::CONSOLE_GAP
    } else {
        0.0
    };
    size::CONSOLE_PAD + channels.min(MAX_CHANNELS) as f32 * (size::STRIP + size::CONSOLE_GAP) + add
}

/// The width the mixes take at the right: the margin before them, both, the gap between
/// them and the padding after Master.
const MIXES_WIDTH: f32 =
    size::MIXES_MARGIN + 2.0 * size::MIX + size::CONSOLE_GAP + size::CONSOLE_PAD;

/// The console's natural size for `channels` channels, the prototype's: every channel in
/// view, strips 450 tall.
pub fn natural_size(channels: usize) -> Vec2 {
    vec2(
        channels_width(channels) + MIXES_WIDTH,
        size::STRIP_HEIGHT + 2.0 * size::CONSOLE_PAD,
    )
}

/// The console's least size: one channel in view (the add column alone with none), both
/// mixes whole, and the faders at their shortest.
pub fn least_size(channels: usize) -> Vec2 {
    let one = size::CONSOLE_PAD + size::STRIP + size::CONSOLE_GAP;
    vec2(
        channels_width(channels).min(one) + MIXES_WIDTH,
        size::STRIP_MIN_HEIGHT + 2.0 * size::CONSOLE_PAD,
    )
}

/// Where everything goes in a console that fills `area`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// The console: the area, at least [`least_size`].
    pub rect: Rect,
    /// Where the channels and the add column show, left of the mixes: they scroll in it.
    pub view: Rect,
    /// The channels' and the add column's whole width ([`channels_width`]).
    pub width: f32,
    /// Every strip's height, in whole points.
    pub height: f32,
    pub monitor: Rect,
    pub master: Rect,
}

impl Geometry {
    /// The console filling `area` with `channels` channels: Master against the right edge,
    /// Monitor before it, the channels' view from the left edge to the margin before Monitor.
    pub fn new(area: Rect, channels: usize) -> Geometry {
        let channels = channels.min(MAX_CHANNELS);
        let rect = Rect::from_min_size(area.min, area.size().max(least_size(channels)));
        let height = (rect.height() - 2.0 * size::CONSOLE_PAD)
            .floor()
            .max(size::STRIP_MIN_HEIGHT);
        let top = rect.top() + size::CONSOLE_PAD;
        let master_left = rect.left() + (rect.width() - size::CONSOLE_PAD - size::MIX).floor();
        let master = Rect::from_min_size(pos2(master_left, top), vec2(size::MIX, height));
        let monitor = Rect::from_min_size(
            pos2(master_left - size::CONSOLE_GAP - size::MIX, top),
            vec2(size::MIX, height),
        );
        let view = Rect::from_x_y_ranges(
            rect.left()..=(monitor.left() - size::MIXES_MARGIN).max(rect.left()),
            rect.y_range(),
        );
        Geometry {
            rect,
            view,
            width: channels_width(channels),
            height,
            monitor,
            master,
        }
    }

    /// How far the channels can scroll: none while they all show.
    pub fn max_scroll(&self) -> f32 {
        (self.width - self.view.width()).max(0.0)
    }

    /// The channels' left edge (their padding's) when scrolled by `offset`, on whole points
    /// as egui's scroll area puts it at one pixel a point (a frame reads it from the scroll
    /// area itself; the tests work it out).
    #[cfg(test)]
    pub fn content_left(&self, offset: f32) -> f32 {
        (self.view.left() - offset).round()
    }

    /// Channel `index`'s left edge from the channels' left edge.
    fn channel_x(index: usize) -> f32 {
        size::CONSOLE_PAD + index as f32 * (size::STRIP + size::CONSOLE_GAP)
    }

    /// Where each channel and the add column go with the channels' left edge at `left`.
    pub fn channels(&self, left: f32, setup: &Setup) -> (Vec<(ChannelId, Rect)>, Option<Rect>) {
        let top = self.rect.top() + size::CONSOLE_PAD;
        let rect = |x: f32, width: f32| Rect::from_min_size(pos2(x, top), vec2(width, self.height));
        let channels: Vec<(ChannelId, Rect)> = setup
            .channels
            .iter()
            .take(MAX_CHANNELS)
            .enumerate()
            .map(|(index, c)| (c.id, rect(left + Geometry::channel_x(index), size::STRIP)))
            .collect();
        let add = has_add_column(setup.channels.len())
            .then(|| rect(left + Geometry::channel_x(channels.len()), size::ADD_COLUMN));
        (channels, add)
    }

    /// Where `strip` is drawn with the channels scrolled by `offset` (the part out of view
    /// included); `None` for a channel the setup does not have.
    #[cfg(test)]
    pub fn strip(&self, strip: Strip, setup: &Setup, offset: f32) -> Option<Rect> {
        match strip {
            Strip::Mix(MixId::Monitor) => Some(self.monitor),
            Strip::Mix(MixId::Master) => Some(self.master),
            Strip::Channel(ch) => self
                .channels(self.content_left(offset), setup)
                .0
                .into_iter()
                .find(|(id, _)| *id == ch)
                .map(|(_, rect)| rect),
        }
    }

    /// The add-channel column, scrolled by `offset`, while there is one.
    #[cfg(test)]
    pub fn add_column(&self, setup: &Setup, offset: f32) -> Option<Rect> {
        self.channels(self.content_left(offset), setup).1
    }

    /// The least scroll from `offset` that shows channel `index` whole with the black either
    /// side of it, within what the channels can scroll.
    pub fn reveal(&self, index: usize, offset: f32) -> f32 {
        let x = Geometry::channel_x(index);
        let (from, to) = (x - size::CONSOLE_PAD, x + size::STRIP + size::CONSOLE_GAP);
        let shown = self.view.width();
        let offset = if from < offset {
            from
        } else if to > offset + shown {
            to - shown
        } else {
            offset
        };
        offset.clamp(0.0, self.max_scroll())
    }
}

/// Whether a channel's header shows the live dot: a hardware input always, an app channel
/// while one of its apps is playing (the prototype's `srcText`).
pub fn live(source: &Source, apps: &[AppPresence]) -> bool {
    match source {
        Source::Hardware(_) => true,
        Source::Apps(list) => list
            .iter()
            .any(|app| apps.iter().any(|p| p.running && p.app.matches(app))),
    }
}

/// Draws the console `geometry` lays out and senses its gestures: the channels and the add
/// column in their view, scrolled, then Monitor and Master at the right; returns whether an
/// insert drag began (the open popover closes).
pub fn draw(ui: &mut Ui, geometry: &Geometry, view: &ConsoleView<'_>, cx: &mut Cx<'_>) -> bool {
    let setup = view.setup;
    ui.painter()
        .rect_filled(geometry.rect, CornerRadius::ZERO, colour::EDGE);
    let offset = settle(ui, geometry, setup, cx);

    // The channels in a scroll area of their own: the wheel over them (the plain wheel or
    // Shift with it) and the bar scroll them; a press never drags them, so a fader's drag
    // stays the fader's.
    let mut channels = ui.new_child(UiBuilder::new().id_salt("channels").max_rect(geometry.view));
    let plain = channels.style().clone();
    theme::console_scroll(channels.style_mut());
    let output = ScrollArea::horizontal()
        .id_salt("console")
        .auto_shrink([false, false])
        .animated(false)
        .min_scrolled_width(0.0)
        .scroll_source(ScrollSource {
            drag: DragScroll::Never,
            ..ScrollSource::default()
        })
        .horizontal_scroll_offset(offset)
        .show_viewport(&mut channels, |ui, _| {
            // The strips inside keep the window's style: a well's own scroll area is not
            // the console's.
            ui.set_style(plain);
            let left = ui.max_rect().left();
            ui.set_min_size(vec2(geometry.width, geometry.view.height()));
            let (rects, add) = geometry.channels(left, setup);
            for (index, (ch, rect)) in rects.iter().enumerate() {
                let channel = &setup.channels[index];
                let name = match view.renaming {
                    Some((id, name)) if id == *ch && !name.trim().is_empty() => name.trim(),
                    _ => channel.name.as_str(),
                };
                let channel_view = ChannelView {
                    channel,
                    name,
                    live: live(&channel.source, view.apps),
                    apps: view.apps,
                    meters: [
                        view.meters.send(index, MixId::Monitor),
                        view.meters.send(index, MixId::Master),
                    ],
                };
                strip::channel(ui, *rect, &channel_view, cx);
            }
            if let Some(column) = add {
                strip::add_column(ui, column, setup.channels.len(), cx);
            }
            follow_focus(ui, geometry, &rects, cx);
        });
    cx.gestures.scroll.offset = output.state.offset.x;

    for (id, rect) in [
        (MixId::Monitor, geometry.monitor),
        (MixId::Master, geometry.master),
    ] {
        let mix_view = MixView {
            mix: setup.mix(id),
            preview: setup.preview_master,
            meters: view.meters.mix(id),
        };
        strip::mix(ui, rect, &mix_view, cx);
    }
    drag::resolve(
        ui,
        ui.painter(),
        &mut cx.gestures.insert,
        &cx.layout.wells,
        cx.out,
    )
}

/// Where the channels are scrolled this frame: where the last left them, a channel just
/// added scrolled whole into view, and an insert dragged near either edge of the view
/// scrolling them that way.
fn settle(ui: &Ui, geometry: &Geometry, setup: &Setup, cx: &mut Cx<'_>) -> f32 {
    let scroll = &mut cx.gestures.scroll;
    let mut offset = scroll.offset.clamp(0.0, geometry.max_scroll());
    if let Some(ch) = scroll.reveal.take()
        && let Some(index) = setup.channels.iter().position(|c| c.id == ch)
    {
        offset = geometry.reveal(index, offset);
    }
    let dragging = cx.gestures.insert.as_ref().is_some_and(|d| d.active);
    let pointer = ui.input(|i| i.pointer.latest_pos());
    if dragging && let Some(at) = pointer {
        let view = geometry.view;
        let way = if !view.y_range().contains(at.y) {
            0.0
        } else if (view.left()..view.left() + size::AUTO_SCROLL_EDGE).contains(&at.x) {
            -1.0
        } else if (view.right() - size::AUTO_SCROLL_EDGE..=view.right()).contains(&at.x) {
            1.0
        } else {
            0.0
        };
        let to = (offset + way * size::AUTO_SCROLL_SPEED * ui.input(|i| i.stable_dt).min(0.1))
            .clamp(0.0, geometry.max_scroll());
        if to != offset {
            offset = to;
            ui.ctx().request_repaint();
        }
    }
    offset
}

/// A Tab that moved the focus into a channel not wholly in view scrolls that channel in,
/// from the next frame on (the frame that saw the focus move has drawn it where it was).
fn follow_focus(ui: &Ui, geometry: &Geometry, rects: &[(ChannelId, Rect)], cx: &mut Cx<'_>) {
    let focused = ui.memory(|m| m.focused());
    let scroll = &mut cx.gestures.scroll;
    if focused == scroll.focused {
        return;
    }
    scroll.focused = focused;
    let Some(widget) = focused.and_then(|id| ui.ctx().read_response(id)) else {
        return;
    };
    if !cx.keyboard || widget.layer_id != ui.layer_id() {
        return;
    }
    let view = geometry.view.x_range();
    let hidden = rects.iter().find(|(_, rect)| {
        rect.contains(widget.rect.center())
            && !(view.contains(rect.left()) && view.contains(rect.right()))
    });
    if let Some((ch, _)) = hidden {
        scroll.reveal = Some(*ch);
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::example;

    /// The setup with `n` channels: the example's, with hardware channels added past six.
    fn with_channels(n: usize) -> Setup {
        let mut setup = example::setup();
        setup.channels.truncate(n);
        while setup.channels.len() < n {
            let _ = vixmix_model::apply(
                &mut setup,
                vixmix_model::Command::AddChannel {
                    kind: vixmix_model::ChannelKind::Hardware,
                },
            )
            .expect("room for another channel");
        }
        setup
    }

    fn area(width: f32, height: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 34.0), vec2(width, height))
    }

    #[test]
    fn the_example_console_is_the_prototypes_size() {
        // Six channels, the add column, two mixes: 900 x 454 points, the prototype's 902 x 490
        // less its 1-point border and its 34-point header.
        assert_eq!(natural_size(6), vec2(900.0, 454.0));
        // Eight channels: no add column (2 + 8 x 106 + 6 + 112 + 2 + 112 + 2).
        assert_eq!(natural_size(8).x, 1084.0);
        assert!(has_add_column(7));
        assert!(!has_add_column(8));
    }

    #[test]
    fn the_strips_sit_in_setup_order_with_the_add_column_before_the_mixes() {
        let setup = example::setup();
        let at = Geometry::new(area(900.0, 454.0), 6);
        assert_eq!(at.rect, area(900.0, 454.0));
        assert_eq!(at.height, 450.0);
        assert_eq!(at.max_scroll(), 0.0, "the natural size shows every channel");
        let (rects, add) = at.channels(at.content_left(0.0), &setup);
        assert_eq!(rects.len(), 6);
        assert_eq!(rects[0].1.min, pos2(2.0, 36.0));
        assert_eq!(rects[1].1.left(), 108.0, "104 and the 2 of black");
        let add = add.expect("six channels leave room for more");
        assert_eq!(add.left(), 2.0 + 6.0 * 106.0);
        assert_eq!(at.monitor.left(), add.right() + 2.0 + 6.0);
        assert_eq!(at.master.left(), at.monitor.right() + 2.0);
        assert_eq!(at.master.right(), 900.0 - 2.0, "the padding after Master");
        assert_eq!(at.view.right(), add.right() + 2.0);
    }

    #[test]
    fn a_wider_console_keeps_the_channels_left_and_the_mixes_at_its_right_edge() {
        let setup = example::setup();
        let at = Geometry::new(area(1400.0, 454.0), 6);
        assert_eq!(at.master.right(), 1398.0);
        assert_eq!(at.monitor.right(), at.master.left() - 2.0);
        let (rects, _) = at.channels(at.content_left(0.0), &setup);
        assert_eq!(rects[0].1.left(), 2.0, "the channels stay at the left");
        assert_eq!(at.max_scroll(), 0.0);
        // A width that is not a whole number of points keeps every strip on whole points.
        let at = Geometry::new(area(1000.5, 454.0), 6);
        assert_eq!(at.master.left(), at.master.left().round());
    }

    #[test]
    fn a_narrow_console_scrolls_its_channels_and_keeps_both_mixes_whole() {
        let setup = with_channels(8);
        let at = Geometry::new(area(600.0, 454.0), 8);
        assert_eq!(at.master.right(), 598.0);
        assert_eq!(at.monitor.left(), 598.0 - 2.0 * 112.0 - 2.0);
        assert_eq!(at.view.width(), at.monitor.left() - 6.0);
        assert_eq!(at.width, 2.0 + 8.0 * 106.0, "no add column with eight");
        assert_eq!(at.max_scroll(), at.width - at.view.width());
        // Scrolled to the end, the last channel ends 2 before the view does.
        let last = at
            .strip(
                Strip::Channel(setup.channels[7].id),
                &setup,
                at.max_scroll(),
            )
            .unwrap();
        assert_eq!(last.right(), at.view.right() - 2.0);
        // The mixes do not move with the channels.
        assert_eq!(
            at.strip(Strip::Mix(MixId::Master), &setup, at.max_scroll()),
            Some(at.master)
        );
    }

    #[test]
    fn its_least_size_is_one_channel_both_mixes_and_the_shortest_faders() {
        assert_eq!(
            least_size(6),
            vec2(2.0 + 106.0 + 6.0 + 224.0 + 2.0 + 2.0, 364.0)
        );
        assert_eq!(least_size(1), least_size(8));
        assert_eq!(least_size(0).x, 2.0 + 28.0 + 234.0, "the add column alone");
        // An area smaller than that still lays the console out at its least.
        let at = Geometry::new(area(100.0, 100.0), 6);
        assert_eq!(at.rect.size(), least_size(6));
        assert_eq!(at.height, size::STRIP_MIN_HEIGHT);
        assert_eq!(
            at.view.width(),
            108.0,
            "one channel and the black either side"
        );
    }

    #[test]
    fn revealing_a_channel_scrolls_it_whole_into_view_and_no_further() {
        let at = Geometry::new(area(600.0, 454.0), 8);
        let shown = at.view.width();
        // Already whole in view: no scroll.
        assert_eq!(at.reveal(0, 0.0), 0.0);
        // Past the right edge: just far enough that it ends at the view's.
        assert_eq!(at.reveal(3, 0.0), 2.0 + 4.0 * 106.0 - shown);
        // Before the left edge: it starts at the view's.
        assert_eq!(at.reveal(1, 300.0), 106.0);
        // Never past the end.
        assert_eq!(at.reveal(7, 0.0), at.max_scroll());
    }

    #[test]
    fn a_channel_is_live_when_it_is_hardware_or_one_of_its_apps_plays() {
        let setup = example::setup();
        let apps: Vec<AppPresence> = example::APPS
            .iter()
            .map(|(id, _, running)| AppPresence {
                app: example::app(id),
                running: *running,
            })
            .collect();
        let lit: Vec<bool> = setup
            .channels
            .iter()
            .map(|c| live(&c.source, &apps))
            .collect();
        assert_eq!(lit, [true, true, true, true, true, true]);
        let nothing_plays: Vec<AppPresence> = apps
            .iter()
            .map(|p| AppPresence {
                running: false,
                ..p.clone()
            })
            .collect();
        assert!(
            live(&setup.channels[0].source, &nothing_plays),
            "Mic is hardware"
        );
        assert!(
            !live(&setup.channels[2].source, &nothing_plays),
            "Music plays nothing"
        );
    }
}
