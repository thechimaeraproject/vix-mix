//! The console row, laid out as prototype v5's `.console`: the channel strips left to right,
//! the add-channel column after them while there is room for another channel, then — past a
//! wider gap — Monitor and Master. Black shows between the strips.
//!
//! Each frame draws the strips from the host's setup and senses their gestures: what a
//! gesture asks becomes an [`Intent`] in [`Cx::out`], the rects a popover hangs from and a
//! drag lands in are recorded in [`Layout`], and a drag or a fader's travel carries over to
//! the next frame in [`Gestures`]. After the last strip the insert drag is resolved
//! ([`drag::resolve`]), because where it lands depends on every strip.

use egui::{CornerRadius, Pos2, Rect, Ui, Vec2, pos2, vec2};
use vixmix_host::{AppPresence, Host};
use vixmix_model::{ChannelId, MAX_CHANNELS, MixId, Setup, Source, Strip};

use crate::drag::{self, InsertDrag};
use crate::fader;
use crate::meters::Meters;
use crate::shell::Intent;
use crate::strip::{self, ChannelView, MixView};
use crate::theme::{colour, size};

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

/// The console's size for `channels` channels: the padding, each channel and the gap after
/// it, the add-channel column and its gap, the margin before the mixes, the two mixes.
pub fn size(channels: usize) -> Vec2 {
    let channel_count = channels.min(MAX_CHANNELS) as f32;
    let add = if has_add_column(channels) {
        size::ADD_COLUMN + size::CONSOLE_GAP
    } else {
        0.0
    };
    let width = size::CONSOLE_PAD
        + channel_count * (size::STRIP + size::CONSOLE_GAP)
        + add
        + size::MIXES_MARGIN
        + 2.0 * size::MIX
        + size::CONSOLE_GAP
        + size::CONSOLE_PAD;
    vec2(width, size::STRIP_HEIGHT + 2.0 * size::CONSOLE_PAD)
}

/// Where each strip goes with the console's top-left corner at `origin`: the channels in
/// order, the add-channel column when there is one, Monitor, Master.
pub fn strips(origin: Pos2, setup: &Setup) -> (Vec<(Strip, Rect)>, Option<Rect>) {
    let top = origin.y + size::CONSOLE_PAD;
    let rect =
        |x: f32, width: f32| Rect::from_min_size(pos2(x, top), vec2(width, size::STRIP_HEIGHT));
    let mut out = Vec::with_capacity(MAX_CHANNELS + 2);
    let mut x = origin.x + size::CONSOLE_PAD;
    for channel in setup.channels.iter().take(MAX_CHANNELS) {
        out.push((Strip::Channel(channel.id), rect(x, size::STRIP)));
        x += size::STRIP + size::CONSOLE_GAP;
    }
    let add = has_add_column(setup.channels.len()).then(|| {
        let column = rect(x, size::ADD_COLUMN);
        x += size::ADD_COLUMN + size::CONSOLE_GAP;
        column
    });
    x += size::MIXES_MARGIN;
    for mix in [MixId::Monitor, MixId::Master] {
        out.push((Strip::Mix(mix), rect(x, size::MIX)));
        x += size::MIX + size::CONSOLE_GAP;
    }
    (out, add)
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

/// Draws the console with its top-left corner at `origin` and senses its gestures; returns
/// the rect it filled and whether an insert drag began (the open popover closes).
pub fn draw(ui: &mut Ui, origin: Pos2, view: &ConsoleView<'_>, cx: &mut Cx<'_>) -> (Rect, bool) {
    let setup = view.setup;
    let rect = Rect::from_min_size(origin, size(setup.channels.len()));
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, colour::EDGE);
    let (rects, add) = strips(origin, setup);
    for (strip_key, strip_rect) in rects {
        match strip_key {
            Strip::Channel(ch) => {
                let Some(index) = setup.channels.iter().position(|c| c.id == ch) else {
                    continue;
                };
                let channel = &setup.channels[index];
                let name = match view.renaming {
                    Some((id, name)) if id == ch && !name.trim().is_empty() => name.trim(),
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
                strip::channel(ui, strip_rect, &channel_view, cx);
            }
            Strip::Mix(id) => {
                let mix_view = MixView {
                    mix: setup.mix(id),
                    preview: setup.preview_master,
                    meters: view.meters.mix(id),
                };
                strip::mix(ui, strip_rect, &mix_view, cx);
            }
        }
    }
    if let Some(column) = add {
        strip::add_column(ui, column, setup.channels.len(), cx);
    }
    let started = drag::resolve(
        ui,
        ui.painter(),
        &mut cx.gestures.insert,
        &cx.layout.wells,
        cx.out,
    );
    (rect, started)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::example;

    #[test]
    fn the_example_console_is_the_prototypes_size() {
        // Six channels, the add column, two mixes: 900 x 454 points, the prototype's 902 x 490
        // less its 1-point border and its 34-point header.
        assert_eq!(size(6), vec2(900.0, 454.0));
        // Eight channels: no add column (2 + 8 x 106 + 6 + 112 + 2 + 112 + 2).
        assert_eq!(size(8).x, 1084.0);
        assert!(has_add_column(7));
        assert!(!has_add_column(8));
    }

    #[test]
    fn the_strips_sit_in_setup_order_with_the_add_column_before_the_mixes() {
        let setup = example::setup();
        let (rects, add) = strips(Pos2::ZERO, &setup);
        assert_eq!(rects.len(), 8);
        assert_eq!(rects[0].1.min, pos2(2.0, 2.0));
        assert_eq!(rects[1].1.left(), 108.0, "104 and the 2 of black");
        let add = add.expect("six channels leave room for more");
        assert_eq!(add.left(), 2.0 + 6.0 * 106.0);
        assert_eq!(rects[6].0, Strip::Mix(MixId::Monitor));
        assert_eq!(rects[6].1.left(), add.right() + 2.0 + 6.0);
        assert_eq!(
            rects[7].1.right(),
            size(6).x - 2.0,
            "the padding after Master"
        );
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
