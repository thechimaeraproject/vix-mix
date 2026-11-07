//! One strip, drawn as prototype v5 draws it and answering as it answers. A channel strip:
//! the header (icon, name, live dot), the FX well with MON and MST and its inserts
//! ([`crate::well`]), two faders over their meters with the scale between
//! ([`crate::fader`]), the readouts, and mute, link, mute. A mix strip: the header (and the
//! MST tag on Monitor while Master is previewed), the FX well, one wide fader beside its
//! scale, the readout, the mute and, on Master, the ear.
//!
//! **Gestures.** The header opens the strip's popover (a channel's name and source, a mix's
//! output) and shows its caret under the pointer or while the popover is open. Mute, link
//! and the ear toggle. Every gesture becomes an [`Intent`] for the shell to carry out after
//! the frame; nothing here changes the setup.

use std::sync::Arc;

use chimaeraproject_egui_audio::{LabelAt, Labels, Mark, ScaleStyle, Ticks, paint_scale};
use egui::epaint::TextShape;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, FontId, Galley, Id, Painter, Pos2, Rect, Stroke, Ui, pos2, vec2,
};
use vixmix_host::AppPresence;
use vixmix_model::{Channel, ChannelId, Command, Mix, MixId, Source, Strip, taper};

use crate::console::{Anchor, Cx};
use crate::fader::{self, Look, Side, Target};
use crate::icons::{self, Icon};
use crate::meters::{Pair, Prototype};
use crate::shell::Intent;
use crate::theme::{colour, font, size};
use crate::well::{self, WellView};
use crate::widgets;

/// What a channel strip shows.
pub struct ChannelView<'a> {
    pub channel: &'a Channel,
    /// The name to show: the channel's, or what its name field holds while being edited.
    pub name: &'a str,
    /// The header's dot: a hardware input, or an app channel with a running app.
    pub live: bool,
    /// The apps the source panel lists, for the header's tooltip.
    pub apps: &'a [AppPresence],
    /// The Monitor send's bars, then the Master send's.
    pub meters: [&'a Pair; 2],
}

/// What a mix strip shows.
pub struct MixView<'a> {
    pub mix: &'a Mix,
    /// Master previewed in the Monitor output: the tag on Monitor, the ear pressed on Master.
    pub preview: bool,
    pub meters: &'a Pair,
}

/// A strip's rows: the header, the FX well, the faders, the readouts, the buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rows {
    pub head: Rect,
    pub fx: Rect,
    pub faders: Rect,
    pub readouts: Rect,
    pub buttons: Rect,
}

impl Rows {
    /// The rows inside a strip's `rect`: its padding off, the rows top to bottom at their
    /// heights with the gap between.
    pub fn of(rect: Rect) -> Rows {
        let left = rect.left() + size::PAD_X;
        let right = rect.right() - size::PAD_X;
        let mut top = rect.top() + size::PAD_TOP;
        let mut next = |height: f32| {
            let row = Rect::from_x_y_ranges(left..=right, top..=top + height);
            top += height + size::ROW_GAP;
            row
        };
        Rows {
            head: next(size::ROWS[0]),
            fx: next(size::ROWS[1]),
            faders: next(size::ROWS[2]),
            readouts: next(size::ROWS[3]),
            buttons: next(size::ROWS[4]),
        }
    }
}

/// A channel strip's two fader columns and the scale between them.
pub fn channel_columns(faders: Rect) -> [Rect; 3] {
    let left = Rect::from_x_y_ranges(
        faders.left()..=faders.left() + size::FADER,
        faders.y_range(),
    );
    let right = Rect::from_x_y_ranges(
        faders.right() - size::FADER..=faders.right(),
        faders.y_range(),
    );
    let middle = Rect::from_x_y_ranges(left.right()..=right.left(), faders.y_range());
    [left, middle, right]
}

/// A mix strip's fader column and its scale, centred together.
pub fn mix_columns(faders: Rect) -> [Rect; 2] {
    let width = size::FADER_BIG + size::MIX_FADER_GAP + size::MIX_SCALE;
    let start = faders.center().x - width / 2.0;
    let column = Rect::from_x_y_ranges(start..=start + size::FADER_BIG, faders.y_range());
    let scale = Rect::from_x_y_ranges(
        column.right() + size::MIX_FADER_GAP..=start + width,
        faders.y_range(),
    );
    [column, scale]
}

/// The height on a fader column where `position` sits: position 0 at the bars' bottom, 1 at
/// their top, [`size::FADER_INSET`] in from the column's ends (the kit's travel). The fader
/// and the scale reach it through the kit's widgets; the tests press where it says.
#[cfg(test)]
pub fn fader_y(column: Rect, position: f32) -> f32 {
    chimaeraproject_egui_audio::fader_y(column, size::FADER_INSET, position)
}

/// The scale's marks: decibels and label (`+6, 0, 6, 12, 24, 48`), each a point of the
/// taper's table, so each sits exactly on its position.
pub const SCALE: [Mark<'static>; 6] = [
    Mark {
        db: 6.0,
        label: "+6",
    },
    Mark {
        db: 0.0,
        label: "0",
    },
    Mark {
        db: -6.0,
        label: "6",
    },
    Mark {
        db: -12.0,
        label: "12",
    },
    Mark {
        db: -24.0,
        label: "24",
    },
    Mark {
        db: -48.0,
        label: "48",
    },
];

/// What a channel's header says its source is (the prototype's `srcText`): the device, or
/// the apps with the playing ones first, two named and the rest counted.
pub fn source_text(channel: &Channel, apps: &[AppPresence]) -> String {
    match &channel.source {
        Source::Hardware(Some(device)) => format!("Input: {}", device.description),
        Source::Hardware(None) => "Input: none picked yet".to_string(),
        Source::Apps(list) if list.is_empty() => "Apps: none ticked yet".to_string(),
        Source::Apps(list) => {
            let running =
                |app: &vixmix_model::AppRef| apps.iter().any(|p| p.running && p.app.matches(app));
            let mut sorted: Vec<_> = list.iter().collect();
            sorted.sort_by_key(|app| !running(app));
            let named: Vec<&str> = sorted.iter().take(2).map(|a| a.name.as_str()).collect();
            let more = sorted.len().saturating_sub(2);
            let mut text = format!("Apps: {}", named.join(", "));
            if more > 0 {
                text.push_str(&format!(" +{more}"));
            }
            text
        }
    }
}

/// A channel strip, filling `rect` (104 x 450).
pub fn channel(ui: &mut Ui, rect: Rect, view: &ChannelView<'_>, cx: &mut Cx<'_>) {
    let channel = view.channel;
    let ch = channel.id;
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, colour::STRIP);
    let rows = Rows::of(rect);
    let icon = match channel.source {
        Source::Hardware(_) => &icons::MIC,
        Source::Apps(_) => &icons::APPS,
    };
    head(
        ui,
        rows.head,
        Strip::Channel(ch),
        icon,
        view.name,
        Trailing::Dot(view.live),
        source_text(channel, view.apps),
        cx,
    );
    well::well(
        ui,
        rows.fx,
        rect,
        &WellView {
            strip: Strip::Channel(ch),
            chain: &channel.chain,
            toggles: Some((ch, [channel.chain_to_monitor, channel.chain_to_master])),
        },
        cx,
    );

    // The faders: a 20-wide column each side, the scale between.
    let [left, middle, right] = channel_columns(rows.faders);
    let sends = [
        (MixId::Monitor, Side::Left, left),
        (MixId::Master, Side::Right, right),
    ];
    for ((mix, side, column), bars) in sends.into_iter().zip(view.meters) {
        let send = channel.send(mix);
        fader::fader(
            ui,
            column,
            Target::Send { ch, mix },
            &Look {
                side,
                big: false,
                position: send.position,
                bars,
                muted: send.mute,
            },
            &mut cx.gestures.fader,
            cx.keyboard,
            cx.out,
        );
    }
    scale(ui.painter(), middle, true);

    for (mix, column) in [(MixId::Monitor, left), (MixId::Master, right)] {
        let send = channel.send(mix);
        let centre = pos2(column.center().x, rows.readouts.center().y);
        readout(ui.painter(), centre, send.position, send.mute);
    }

    let y = rows.buttons.center().y;
    for (mix, column, icon, tip) in [
        (MixId::Monitor, left, &icons::PHONES, "Mute in Monitor"),
        (MixId::Master, right, &icons::MASTER, "Mute in Master"),
    ] {
        let mute = channel.send(mix).mute;
        mute_button(
            ui,
            pos2(column.center().x, y),
            icon,
            mute,
            Target::Send { ch, mix },
            tip,
            cx,
        );
    }
    link_button(ui, pos2(middle.center().x, y), ch, channel.link, cx);
}

/// A mix strip, filling `rect` (112 x 450).
pub fn mix(ui: &mut Ui, rect: Rect, view: &MixView<'_>, cx: &mut Cx<'_>) {
    let mix = view.mix;
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, colour::MIX);
    let rows = Rows::of(rect);
    let (icon, trailing) = match mix.id {
        MixId::Monitor => (&icons::PHONES, Trailing::Tag(view.preview.then_some("MST"))),
        MixId::Master => (&icons::MASTER, Trailing::Tag(None)),
    };
    let output = mix
        .device
        .as_ref()
        .map_or("No device", |d| d.description.as_str());
    head(
        ui,
        rows.head,
        Strip::Mix(mix.id),
        icon,
        mix.id.name(),
        trailing,
        format!("Output: {output}"),
        cx,
    );
    well::well(
        ui,
        rows.fx,
        rect,
        &WellView {
            strip: Strip::Mix(mix.id),
            chain: &mix.chain,
            toggles: None,
        },
        cx,
    );

    let [column, scale_rect] = mix_columns(rows.faders);
    let level = &mix.level;
    fader::fader(
        ui,
        column,
        Target::Level(mix.id),
        &Look {
            side: Side::Right,
            big: true,
            position: level.position,
            bars: view.meters,
            muted: level.mute,
        },
        &mut cx.gestures.fader,
        cx.keyboard,
        cx.out,
    );
    scale(ui.painter(), scale_rect, false);

    readout(
        ui.painter(),
        rows.readouts.center(),
        level.position,
        level.mute,
    );

    let centre = rows.buttons.center();
    let tip = match mix.id {
        MixId::Monitor => "Mute Monitor",
        MixId::Master => "Mute Master",
    };
    match mix.id {
        MixId::Monitor => mute_button(
            ui,
            centre,
            &icons::SPEAKER,
            level.mute,
            Target::Level(mix.id),
            tip,
            cx,
        ),
        MixId::Master => {
            let offset = (size::MUTE_BUTTON[0] + size::MIX_BUTTON_GAP) / 2.0;
            mute_button(
                ui,
                centre - vec2(offset, 0.0),
                &icons::SPEAKER,
                level.mute,
                Target::Level(mix.id),
                tip,
                cx,
            );
            ear_button(ui, centre + vec2(offset, 0.0), view.preview, cx);
        }
    }
}

/// What a strip's header ends with: a channel's live dot, or the tag a mix may show.
enum Trailing {
    Dot(bool),
    Tag(Option<&'static str>),
}

/// The widget id of a strip's header.
pub fn head_id(strip: Strip) -> Id {
    Id::new(("head", strip))
}

#[allow(clippy::too_many_arguments)]
fn head(
    ui: &Ui,
    row: Rect,
    strip: Strip,
    icon: &Icon,
    name: &str,
    trailing: Trailing,
    tooltip: String,
    cx: &mut Cx<'_>,
) {
    cx.layout.anchors.push((Anchor::Head(strip), row));
    let response = widgets::press(ui, row, head_id(strip)).on_hover_text(tooltip);
    if response.clicked() {
        cx.out.push(Intent::Toggle(Anchor::Head(strip)));
    }
    let expanded = cx.open == Some(Anchor::Head(strip));
    let focused = cx.keyboard && response.has_focus();
    let painter = ui.painter();
    if response.hovered() || expanded {
        painter.rect_filled(row, size::CORNER, colour::CTL);
    }
    let y = row.center().y;
    let mut x = row.left() + size::HEAD_PAD_X;
    let icon_rect = Rect::from_center_size(
        pos2(x + size::HEAD_ICON / 2.0, y),
        vec2(size::HEAD_ICON, size::HEAD_ICON),
    );
    icons::paint(painter, icon, icon_rect, colour::DIM);
    x = icon_rect.right() + size::HEAD_GAP;
    let mut end = row.right() - size::HEAD_PAD_X;
    if response.hovered() || expanded || focused {
        let caret = Rect::from_center_size(
            pos2(end - size::CARET / 2.0, y),
            vec2(size::CARET, size::CARET),
        );
        icons::paint(painter, &icons::CARET, caret, colour::FAINT);
        end = caret.left() - size::HEAD_GAP;
    }
    match trailing {
        Trailing::Dot(live) => {
            let fill = if live { colour::LIVE } else { colour::FAINT };
            painter.circle_filled(pos2(end - size::DOT / 2.0, y), size::DOT / 2.0, fill);
            end -= size::DOT + size::HEAD_GAP;
        }
        Trailing::Tag(Some(tag)) => {
            let text = galley(
                painter,
                tag,
                font::mono_medium(font::TOGGLE),
                colour::ON_ACCENT,
                Spacing::Em(font::TAG_SPACING),
            );
            let tag_rect = Rect::from_min_max(
                pos2(
                    end - text.size().x - 2.0 * size::TAG_PAD_X,
                    y - size::TAG_HEIGHT / 2.0,
                ),
                pos2(end, y + size::TAG_HEIGHT / 2.0),
            );
            painter.rect_filled(tag_rect, size::CORNER, colour::ACCENT);
            place(painter, text, Align2::CENTER_CENTER, tag_rect.center());
            end = tag_rect.left() - size::HEAD_GAP;
        }
        Trailing::Tag(None) => {}
    }
    let text = truncated(
        painter,
        name,
        font::sans_semibold(font::STRIP_NAME),
        colour::TEXT,
        end - x,
    );
    place(painter, text, Align2::LEFT_CENTER, pos2(x, y));
    if focused {
        widgets::ring(painter, row, size::FOCUS_OFFSET);
    }
}

/// The scale (the kit's, on the taper): a tick, the label, and (between a channel's faders) a
/// second tick, at each mark's position; the 0 a shade brighter.
fn scale(painter: &Painter, column: Rect, both_ticks: bool) {
    let style = ScaleStyle {
        inset: size::FADER_INSET,
        ticks: Ticks::Edges {
            length: size::SCALE_TICK,
            thickness: size::HAIRLINE,
            colour: colour::CTL_HI,
            both: both_ticks,
        },
        labels: Some(Labels {
            font: font::mono(font::SCALE),
            colour: colour::FAINT,
            strong_colour: colour::DIM,
            at: if both_ticks {
                LabelAt::Centre
            } else {
                LabelAt::Right
            },
            strong_only: false,
        }),
    };
    paint_scale(painter, column, &Prototype, &SCALE, &style);
}

/// The decibels a fader reads, struck through and faint while muted.
fn readout(painter: &Painter, centre: Pos2, position: f32, muted: bool) {
    let ink = if muted { colour::FAINT } else { colour::DIM };
    let mut job = LayoutJob::single_section(
        taper::format_db(position),
        TextFormat {
            font_id: font::mono_medium(font::READOUT),
            color: ink,
            strikethrough: if muted {
                Stroke::new(size::HAIRLINE, ink)
            } else {
                Stroke::NONE
            },
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping::no_max_width();
    let text = painter.layout_job(job);
    place(painter, text, Align2::CENTER_CENTER, centre);
}

/// The command that sets a fader's mute.
fn mute_command(target: Target, mute: bool) -> Command {
    match target {
        Target::Send { ch, mix } => Command::SetMute { ch, mix, mute },
        Target::Level(mix) => Command::SetMixMute { mix, mute },
    }
}

/// A mute button: the icon on the control face (lighter under the pointer), or white with
/// the slash on red when pressed.
fn mute_button(
    ui: &Ui,
    centre: Pos2,
    icon: &Icon,
    pressed: bool,
    target: Target,
    tip: &str,
    cx: &mut Cx<'_>,
) {
    let rect = Rect::from_center_size(centre, size::MUTE_BUTTON.into());
    let response = widgets::press(ui, rect, Id::new(("mute", target))).on_hover_text(tip);
    if response.clicked() {
        cx.out.push(Intent::Apply(mute_command(target, !pressed)));
    }
    let (fill, ink) = if pressed {
        (colour::MUTE, colour::ON_MUTE)
    } else if response.hovered() {
        (colour::CTL_HI, colour::TEXT)
    } else {
        (colour::CTL, colour::DIM)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, size::CORNER, fill);
    let icon_rect = Rect::from_center_size(centre, vec2(size::BUTTON_ICON, size::BUTTON_ICON));
    icons::paint(painter, icon, icon_rect, ink);
    if pressed {
        icons::paint(painter, &icons::SLASH, icon_rect, ink);
    }
    if cx.keyboard && response.has_focus() {
        widgets::ring(painter, rect, size::FOCUS_OFFSET);
    }
}

/// The link toggle: no face, the links faint (bright under the pointer), or in the accent
/// when linked, centred in its 20 x 18 button.
fn link_button(ui: &Ui, centre: Pos2, ch: ChannelId, pressed: bool, cx: &mut Cx<'_>) {
    let button = Rect::from_center_size(centre, size::LINK_BUTTON.into());
    let response =
        widgets::press(ui, button, Id::new(("link", ch))).on_hover_text("Move both together");
    if response.clicked() {
        cx.out
            .push(Intent::Apply(Command::SetLink { ch, link: !pressed }));
    }
    let ink = if pressed {
        colour::ACCENT
    } else if response.hovered() {
        colour::TEXT
    } else {
        colour::FAINT
    };
    let painter = ui.painter();
    let icon_rect = Rect::from_center_size(button.center(), vec2(size::LINK_ICON, size::LINK_ICON));
    icons::paint(painter, &icons::LINK, icon_rect, ink);
    if cx.keyboard && response.has_focus() {
        widgets::ring(painter, button, size::FOCUS_OFFSET);
    }
}

/// The ear on Master: preview Master in the Monitor output; the accent face when on.
fn ear_button(ui: &Ui, centre: Pos2, pressed: bool, cx: &mut Cx<'_>) {
    let rect = Rect::from_center_size(centre, size::MUTE_BUTTON.into());
    let response = widgets::press(ui, rect, Id::new("ear"))
        .on_hover_text("Preview Master in your Monitor output");
    if response.clicked() {
        cx.out.push(Intent::Apply(Command::SetPreview(!pressed)));
    }
    let (fill, ink) = if pressed {
        (colour::ACCENT, colour::ON_ACCENT)
    } else if response.hovered() {
        (colour::CTL_HI, colour::TEXT)
    } else {
        (colour::CTL, colour::DIM)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, size::CORNER, fill);
    let icon_rect = Rect::from_center_size(centre, vec2(size::BUTTON_ICON, size::BUTTON_ICON));
    icons::paint(painter, &icons::EAR, icon_rect, ink);
    if cx.keyboard && response.has_focus() {
        widgets::ring(painter, rect, size::FOCUS_OFFSET);
    }
}

/// The add-channel column: a plus over "ADD CHANNEL" set vertically, top to bottom,
/// centred; under the pointer, the strip's face and bright text. A press opens its menu.
pub fn add_column(ui: &Ui, rect: Rect, channels: usize, cx: &mut Cx<'_>) {
    cx.layout.anchors.push((Anchor::AddColumn, rect));
    let response = widgets::press(ui, rect, Id::new("add channel")).on_hover_text(format!(
        "{channels} of {} channels",
        vixmix_model::MAX_CHANNELS
    ));
    if response.clicked() {
        cx.out.push(Intent::Toggle(Anchor::AddColumn));
    }
    let (fill, ink) = if response.hovered() {
        (colour::STRIP, colour::TEXT)
    } else {
        (colour::WIN, colour::FAINT)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    let text = galley(
        painter,
        "ADD CHANNEL",
        font::sans_semibold(font::ADD_COLUMN),
        ink,
        Spacing::Em(font::ADD_COLUMN_SPACING),
    );
    let (length, thickness) = (text.size().x, text.size().y);
    let total = size::ADD_COLUMN_ICON + size::ADD_COLUMN_GAP + length;
    let top = rect.center().y - total / 2.0;
    let plus = Rect::from_center_size(
        pos2(rect.center().x, top + size::ADD_COLUMN_ICON / 2.0),
        vec2(size::ADD_COLUMN_ICON, size::ADD_COLUMN_ICON),
    );
    icons::paint(painter, &icons::PLUS, plus, ink);
    // `writing-mode: vertical-rl`: turned a quarter clockwise about the galley's top-left
    // corner, the text runs down and its height lies to the left of that corner.
    let corner = pos2(
        rect.center().x + thickness / 2.0,
        plus.bottom() + size::ADD_COLUMN_GAP,
    );
    painter.add(TextShape::new(corner, text, ink).with_angle(std::f32::consts::FRAC_PI_2));
    if cx.keyboard && response.has_focus() {
        widgets::ring(painter, rect, 0.0);
    }
}

/// Letter spacing: none, or a fraction of the font's size (CSS `letter-spacing: .08em`).
#[derive(Clone, Copy, Debug)]
pub enum Spacing {
    None,
    Em(f32),
}

/// `text` laid out on one line.
pub fn galley(
    painter: &Painter,
    text: &str,
    font: FontId,
    colour: Color32,
    spacing: Spacing,
) -> Arc<Galley> {
    let extra_letter_spacing = match spacing {
        Spacing::None => 0.0,
        Spacing::Em(em) => em * font.size,
    };
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color: colour,
            extra_letter_spacing,
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping::no_max_width();
    painter.layout_job(job)
}

/// `text` on one line, cut to `max_width` with an ellipsis (CSS `text-overflow: ellipsis`).
pub fn truncated(
    painter: &Painter,
    text: &str,
    font: FontId,
    colour: Color32,
    max_width: f32,
) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color: colour,
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
    painter.layout_job(job)
}

/// Paints `galley` with its `anchor` at `at`.
pub fn place(painter: &Painter, galley: Arc<Galley>, anchor: Align2, at: Pos2) {
    let rect = anchor.anchor_size(at, galley.size());
    painter.galley(rect.min, galley, Color32::PLACEHOLDER);
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::example;

    #[test]
    fn a_strips_rows_are_the_prototypes_heights_inside_its_padding() {
        let strip = Rect::from_min_size(pos2(10.0, 20.0), vec2(size::STRIP, size::STRIP_HEIGHT));
        assert_eq!(size::STRIP_HEIGHT, 450.0);
        let rows = Rows::of(strip);
        let heights =
            [rows.head, rows.fx, rows.faders, rows.readouts, rows.buttons].map(|r| r.height());
        assert_eq!(heights, [22.0, 151.0, 210.0, 12.0, 20.0]);
        assert_eq!(rows.head.top(), 27.0, "7 of padding on top");
        assert_eq!(rows.fx.top() - rows.head.bottom(), 5.0, "5 between rows");
        assert_eq!(
            strip.bottom() - rows.buttons.bottom(),
            8.0,
            "8 at the bottom"
        );
        assert_eq!(rows.head.width(), 92.0, "6 at each side");
    }

    #[test]
    fn a_fader_at_unity_sits_on_the_scales_zero_and_its_ends_six_in() {
        let column = Rect::from_min_size(pos2(0.0, 100.0), vec2(20.0, 210.0));
        assert_eq!(fader_y(column, 0.0), 304.0);
        assert_eq!(fader_y(column, 1.0), 106.0);
        let zero = SCALE.iter().find(|mark| mark.label == "0").unwrap().db;
        let at = |db: f64| taper::db_to_position(db as f32);
        assert_eq!(fader_y(column, taper::UNITY), fader_y(column, at(zero)));
        assert_eq!(fader_y(column, 2.0), 106.0, "clamped to the top");
        // The scale's marks are the taper's table: +6 at the top, 0 at unity, 48 at 0.14.
        assert_eq!(at(SCALE[0].db), 1.0);
        assert_eq!(at(SCALE[1].db), taper::UNITY);
        assert_eq!(at(SCALE[5].db), 0.14);
    }

    #[test]
    fn the_header_names_the_source_as_the_prototype_does() {
        let setup = example::setup();
        let apps: Vec<AppPresence> = example::APPS
            .iter()
            .map(|(id, _, running)| AppPresence {
                app: example::app(id),
                running: *running,
            })
            .collect();
        let text = |at: usize| source_text(&setup.channels[at], &apps);
        assert_eq!(text(0), "Input: Wave:3");
        assert_eq!(text(1), "Apps: Hades II, Steam");
        // Spotify and Feishin play; mpv and VLC do not, and are counted.
        assert_eq!(text(2), "Apps: Spotify, Feishin +2");
        assert_eq!(text(3), "Apps: Firefox, Chromium");
    }
}
