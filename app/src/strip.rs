//! One strip, drawn as prototype v5 draws it. A channel strip: the header (icon, name, live
//! dot), the FX well with MON and MST and its inserts, two faders over their meters with the
//! scale between, the readouts, and mute, link, mute. A mix strip: the header (and the MST tag
//! on Monitor while Master is previewed), the FX well, one wide fader beside its scale, the
//! readout, the mute and, on Master, the ear.
//!
//! **Static, for the look gate** (handoff 05, step 1): everything here paints into rects the
//! layout ([`Rows`], [`fader_y`]) names, and nothing senses the pointer. Step 3 puts the
//! gestures over these same rects.

use std::sync::Arc;

use egui::epaint::{Mesh, PathShape, TextShape};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect, Stroke, StrokeKind, pos2,
    vec2,
};
use vixmix_model::{Channel, Insert, Mix, MixId, Source, taper};

use crate::icons::{self, Icon};
use crate::meters::Pair;
use crate::theme::{colour, font, size};

/// What a channel strip shows.
pub struct ChannelView<'a> {
    pub channel: &'a Channel,
    /// The header's dot: a hardware input, or an app channel with a running app.
    pub live: bool,
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

/// The height on a fader column where `position` sits: position 0 at the bars' bottom, 1 at
/// their top, [`size::FADER_INSET`] in from the column's ends.
pub fn fader_y(column: Rect, position: f32) -> f32 {
    let travel = column.height() - 2.0 * size::FADER_INSET;
    column.bottom() - size::FADER_INSET - position.clamp(0.0, 1.0) * travel
}

/// The scale's marks: position and label (`+6, 0, 6, 12, 24, 48`).
pub const SCALE: [(f32, &str); 6] = [
    (1.0, "+6"),
    (taper::UNITY, "0"),
    (0.70, "6"),
    (0.56, "12"),
    (0.36, "24"),
    (0.14, "48"),
];

/// A channel strip, filling `rect` (104 × 450).
pub fn channel(painter: &Painter, rect: Rect, view: &ChannelView<'_>) {
    let channel = view.channel;
    painter.rect_filled(rect, CornerRadius::ZERO, colour::STRIP);
    let rows = Rows::of(rect);
    let icon = match channel.source {
        Source::Hardware(_) => &icons::MIC,
        Source::Apps(_) => &icons::APPS,
    };
    head(
        painter,
        rows.head,
        icon,
        &channel.name,
        Trailing::Dot(view.live),
    );
    well(
        painter,
        rows.fx,
        &channel.chain,
        Some([channel.chain_to_monitor, channel.chain_to_master]),
    );

    // The faders: a 20-wide column each side, the scale between.
    let row = rows.faders;
    let left = Rect::from_x_y_ranges(row.left()..=row.left() + size::FADER, row.y_range());
    let right = Rect::from_x_y_ranges(row.right() - size::FADER..=row.right(), row.y_range());
    let middle = Rect::from_x_y_ranges(left.right()..=right.left(), row.y_range());
    let sends = [
        (&channel.monitor, Side::Left, left),
        (&channel.master, Side::Right, right),
    ];
    for ((send, side, column), bars) in sends.into_iter().zip(view.meters) {
        fader(painter, column, side, false, send.position, bars, send.mute);
    }
    scale(painter, middle, true);

    for (send, column) in [(&channel.monitor, left), (&channel.master, right)] {
        let centre = pos2(column.center().x, rows.readouts.center().y);
        readout(painter, centre, send.position, send.mute);
    }

    let y = rows.buttons.center().y;
    mute_button(
        painter,
        pos2(left.center().x, y),
        &icons::PHONES,
        channel.monitor.mute,
    );
    link_button(painter, pos2(middle.center().x, y), channel.link);
    mute_button(
        painter,
        pos2(right.center().x, y),
        &icons::MASTER,
        channel.master.mute,
    );
}

/// A mix strip, filling `rect` (112 × 450).
pub fn mix(painter: &Painter, rect: Rect, view: &MixView<'_>) {
    let mix = view.mix;
    painter.rect_filled(rect, CornerRadius::ZERO, colour::MIX);
    let rows = Rows::of(rect);
    let (icon, trailing) = match mix.id {
        MixId::Monitor => (&icons::PHONES, Trailing::Tag(view.preview.then_some("MST"))),
        MixId::Master => (&icons::MASTER, Trailing::Tag(None)),
    };
    head(painter, rows.head, icon, mix.id.name(), trailing);
    well(painter, rows.fx, &mix.chain, None);

    // One wide fader and its scale, centred together.
    let row = rows.faders;
    let width = size::FADER_BIG + size::MIX_FADER_GAP + size::MIX_SCALE;
    let start = row.center().x - width / 2.0;
    let column = Rect::from_x_y_ranges(start..=start + size::FADER_BIG, row.y_range());
    let scale_rect = Rect::from_x_y_ranges(
        column.right() + size::MIX_FADER_GAP..=start + width,
        row.y_range(),
    );
    let level = &mix.level;
    fader(
        painter,
        column,
        Side::Right,
        true,
        level.position,
        view.meters,
        level.mute,
    );
    scale(painter, scale_rect, false);

    readout(painter, rows.readouts.center(), level.position, level.mute);

    let centre = rows.buttons.center();
    match mix.id {
        MixId::Monitor => mute_button(painter, centre, &icons::SPEAKER, level.mute),
        MixId::Master => {
            let offset = (size::MUTE_BUTTON[0] + size::MIX_BUTTON_GAP) / 2.0;
            mute_button(
                painter,
                centre - vec2(offset, 0.0),
                &icons::SPEAKER,
                level.mute,
            );
            ear_button(painter, centre + vec2(offset, 0.0), view.preview);
        }
    }
}

/// What a strip's header ends with: a channel's live dot, or the tag a mix may show.
enum Trailing {
    Dot(bool),
    Tag(Option<&'static str>),
}

fn head(painter: &Painter, row: Rect, icon: &Icon, name: &str, trailing: Trailing) {
    let y = row.center().y;
    let mut x = row.left() + size::HEAD_PAD_X;
    let icon_rect = Rect::from_center_size(
        pos2(x + size::HEAD_ICON / 2.0, y),
        vec2(size::HEAD_ICON, size::HEAD_ICON),
    );
    icons::paint(painter, icon, icon_rect, colour::DIM);
    x = icon_rect.right() + size::HEAD_GAP;
    let mut end = row.right() - size::HEAD_PAD_X;
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
}

/// The FX well: the header (FX, and MON / MST on a channel), each insert, "+ Add plugin".
fn well(painter: &Painter, rect: Rect, chain: &[Insert], toggles: Option<[bool; 2]>) {
    painter.rect_filled(rect, size::CORNER, colour::WELL);
    let head = Rect::from_x_y_ranges(rect.x_range(), rect.top()..=rect.top() + size::FX_HEAD);
    // The rule is the header's last point; the label centres in what is above it.
    painter.hline(
        head.x_range(),
        head.bottom() - size::HAIRLINE_INSET,
        Stroke::new(size::HAIRLINE, colour::LINE),
    );
    let y = head.top() + (size::FX_HEAD - size::HAIRLINE) / 2.0;
    let fx = galley(
        painter,
        "FX",
        font::sans_semibold(font::FX_HEAD),
        colour::FAINT,
        Spacing::Em(font::FX_HEAD_SPACING),
    );
    place(
        painter,
        fx,
        Align2::LEFT_CENTER,
        pos2(head.left() + size::FX_HEAD_PAD_LEFT, y),
    );
    if let Some([monitor, master]) = toggles {
        let mut right = head.right() - size::FX_HEAD_PAD_RIGHT;
        for (label, on) in [("MST", master), ("MON", monitor)] {
            right = toggle(painter, pos2(right, y), label, on) - size::TOGGLE_GAP;
        }
    }

    let list = Rect::from_x_y_ranges(rect.x_range(), head.bottom()..=rect.bottom())
        .shrink(size::SLOTS_PAD);
    let clipped = painter.with_clip_rect(list.intersect(painter.clip_rect()));
    let mut top = list.top();
    for insert in chain {
        let slot = Rect::from_x_y_ranges(list.x_range(), top..=top + size::SLOT);
        slot_row(&clipped, slot, insert);
        top += size::SLOT;
    }
    let slot = Rect::from_x_y_ranges(list.x_range(), top..=top + size::SLOT);
    let y = slot.center().y;
    let plus = Rect::from_center_size(
        pos2(slot.left() + size::SLOT_PAD_X + size::ADD_ICON / 2.0, y),
        vec2(size::ADD_ICON, size::ADD_ICON),
    );
    icons::paint(&clipped, &icons::PLUS, plus, colour::FAINT);
    let text = galley(
        &clipped,
        "Add plugin",
        font::sans(font::SLOT),
        colour::FAINT,
        Spacing::None,
    );
    place(
        &clipped,
        text,
        Align2::LEFT_CENTER,
        pos2(plus.right() + size::ADD_GAP, y),
    );
}

/// MON or MST, its right edge at `right_centre`; returns its left edge.
fn toggle(painter: &Painter, right_centre: Pos2, label: &str, on: bool) -> f32 {
    let (fill, ink) = if on {
        (colour::ACCENT, colour::ON_ACCENT)
    } else {
        (colour::CTL, colour::FAINT)
    };
    let text = galley(
        painter,
        label,
        font::mono_medium(font::TOGGLE),
        ink,
        Spacing::Em(font::TOGGLE_SPACING),
    );
    let width = text.size().x + 2.0 * size::TOGGLE_PAD_X;
    let rect = Rect::from_min_max(
        pos2(
            right_centre.x - width,
            right_centre.y - size::TOGGLE_HEIGHT / 2.0,
        ),
        pos2(right_centre.x, right_centre.y + size::TOGGLE_HEIGHT / 2.0),
    );
    painter.rect_filled(rect, size::CORNER, fill);
    place(painter, text, Align2::CENTER_CENTER, rect.center());
    rect.left()
}

/// One insert: the bypass square (filled when the insert is on) and its name, faint when
/// bypassed.
fn slot_row(painter: &Painter, slot: Rect, insert: &Insert) {
    let y = slot.center().y;
    let square = Rect::from_min_size(
        pos2(slot.left() + size::SLOT_PAD_X, y - size::BYPASS / 2.0),
        vec2(size::BYPASS, size::BYPASS),
    );
    if insert.enabled {
        painter.rect_filled(square, size::BYPASS_CORNER, colour::ACCENT);
    } else {
        painter.rect_stroke(
            square,
            size::BYPASS_CORNER,
            Stroke::new(size::HAIRLINE, colour::FAINT),
            StrokeKind::Inside,
        );
    }
    let x = square.right() + size::SLOT_GAP;
    let ink = if insert.enabled {
        colour::TEXT
    } else {
        colour::FAINT
    };
    let max = slot.right() - size::SLOT_PAD_X - x;
    let text = truncated(
        painter,
        &insert.plugin.name,
        font::sans(font::SLOT),
        ink,
        max,
    );
    place(painter, text, Align2::LEFT_CENTER, pos2(x, y));
}

/// Which side of its column a fader's triangle is on: a channel's Monitor fader has it on the
/// left pointing in, every other fader on the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// A fader: the stereo bars, the fader line across them and its triangle, at `position`.
/// A muted fader's line is faint and its bars at 40 %.
fn fader(
    painter: &Painter,
    column: Rect,
    side: Side,
    big: bool,
    position: f32,
    bars: &Pair,
    muted: bool,
) {
    let bar = if big { size::BAR_BIG } else { size::BAR };
    let width = 2.0 * bar + size::BAR_GAP;
    let x = match side {
        Side::Left => column.right() - width,
        Side::Right => column.left(),
    };
    let y = column.top() + size::FADER_INSET..=column.bottom() - size::FADER_INSET;
    let opacity = if muted {
        colour::MUTED_BARS_OPACITY
    } else {
        1.0
    };
    meter_bar(
        painter,
        Rect::from_x_y_ranges(x..=x + bar, y.clone()),
        &bars[0],
        opacity,
    );
    meter_bar(
        painter,
        Rect::from_x_y_ranges(x + bar + size::BAR_GAP..=x + width, y),
        &bars[1],
        opacity,
    );

    // The line reaches from the triangle's base to a point past the bars' far side
    // (`.vf.l .ln{left:6px;right:-1px}`, `.vf.r .ln{left:-1px;right:6px}`, 7 on a mix).
    let ink = if muted { colour::FAINT } else { colour::ACCENT };
    let line_y = fader_y(column, position);
    let [depth, height] = size::TRIANGLE;
    let (from, to) = match side {
        Side::Left => (
            column.left() + depth,
            column.right() + size::FADER_LINE_OVERHANG,
        ),
        Side::Right => (
            column.left() - size::FADER_LINE_OVERHANG,
            column.right() - depth - if big { size::BIG_LINE_SHORTFALL } else { 0.0 },
        ),
    };
    painter.rect_filled(
        Rect::from_x_y_ranges(
            from..=to,
            line_y - size::FADER_LINE / 2.0..=line_y + size::FADER_LINE / 2.0,
        ),
        CornerRadius::ZERO,
        ink,
    );
    let (base, tip) = match side {
        Side::Left => (column.left(), column.left() + depth),
        Side::Right => (column.right(), column.right() - depth),
    };
    painter.add(PathShape::convex_polygon(
        vec![
            pos2(base, line_y - height / 2.0),
            pos2(tip, line_y),
            pos2(base, line_y + height / 2.0),
        ],
        ink,
        Stroke::NONE,
    ));
}

/// One meter bar: the gradient up to the level, black above it, the peak-hold line.
fn meter_bar(painter: &Painter, rect: Rect, bar: &crate::meters::Bar, opacity: f32) {
    let level = bar.level.clamp(0.0, 1.0);
    let height = rect.height();
    let at = |position: f32| rect.bottom() - position * height;
    let black = Rect::from_x_y_ranges(rect.x_range(), rect.top()..=at(level));
    painter.rect_filled(
        black,
        CornerRadius::ZERO,
        colour::BLACK.gamma_multiply(opacity),
    );
    let mut mesh = Mesh::default();
    for pair in colour::METER.windows(2) {
        let ((from, low), (to, high)) = (pair[0], pair[1]);
        if from >= level {
            break;
        }
        let top = to.min(level);
        let top_colour = low.lerp_to_gamma(high, (top - from) / (to - from));
        let base = mesh.vertices.len() as u32;
        for (p, c) in [
            (pos2(rect.left(), at(from)), low),
            (pos2(rect.right(), at(from)), low),
            (pos2(rect.right(), at(top)), top_colour),
            (pos2(rect.left(), at(top)), top_colour),
        ] {
            mesh.colored_vertex(p, c.gamma_multiply(opacity));
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(mesh);
    // The peak line's bottom edge sits at the peak; the bar clips it at the top.
    if bar.peak > size::PEAK_SHOWN_ABOVE {
        let y = at(bar.peak.min(1.0));
        if y - size::PEAK_LINE >= rect.top() {
            let ink = colour::PEAK.gamma_multiply(colour::PEAK_OPACITY * opacity);
            painter.rect_filled(
                Rect::from_x_y_ranges(rect.x_range(), y - size::PEAK_LINE..=y),
                CornerRadius::ZERO,
                ink,
            );
        }
    }
}

/// The scale: a tick, the label, and (between a channel's faders) a second tick, at each
/// mark's position; the 0 a shade brighter.
fn scale(painter: &Painter, column: Rect, both_ticks: bool) {
    for (position, label) in SCALE {
        let y = fader_y(column, position);
        let half = size::HAIRLINE / 2.0;
        let tick = |x: f32| Rect::from_x_y_ranges(x..=x + size::SCALE_TICK, y - half..=y + half);
        painter.rect_filled(tick(column.left()), CornerRadius::ZERO, colour::CTL_HI);
        let ink = if label == "0" {
            colour::DIM
        } else {
            colour::FAINT
        };
        let text = galley(painter, label, font::mono(font::SCALE), ink, Spacing::None);
        if both_ticks {
            painter.rect_filled(
                tick(column.right() - size::SCALE_TICK),
                CornerRadius::ZERO,
                colour::CTL_HI,
            );
            place(
                painter,
                text,
                Align2::CENTER_CENTER,
                pos2(column.center().x, y),
            );
        } else {
            place(painter, text, Align2::RIGHT_CENTER, pos2(column.right(), y));
        }
    }
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

/// A mute button: the icon on the control face, or white with the slash on red when pressed.
fn mute_button(painter: &Painter, centre: Pos2, icon: &Icon, pressed: bool) {
    let rect = Rect::from_center_size(centre, size::MUTE_BUTTON.into());
    let (fill, ink) = if pressed {
        (colour::MUTE, colour::ON_MUTE)
    } else {
        (colour::CTL, colour::DIM)
    };
    painter.rect_filled(rect, size::CORNER, fill);
    let icon_rect = Rect::from_center_size(centre, vec2(size::BUTTON_ICON, size::BUTTON_ICON));
    icons::paint(painter, icon, icon_rect, ink);
    if pressed {
        icons::paint(painter, &icons::SLASH, icon_rect, ink);
    }
}

/// The link toggle: no face, the links faint, or in the accent when linked, centred in its
/// 20 × 18 button.
fn link_button(painter: &Painter, centre: Pos2, pressed: bool) {
    let ink = if pressed {
        colour::ACCENT
    } else {
        colour::FAINT
    };
    let button = Rect::from_center_size(centre, size::LINK_BUTTON.into());
    let icon_rect = Rect::from_center_size(button.center(), vec2(size::LINK_ICON, size::LINK_ICON));
    icons::paint(painter, &icons::LINK, icon_rect, ink);
}

/// The ear on Master: preview Master in the Monitor output; the accent face when on.
fn ear_button(painter: &Painter, centre: Pos2, pressed: bool) {
    let rect = Rect::from_center_size(centre, size::MUTE_BUTTON.into());
    let (fill, ink) = if pressed {
        (colour::ACCENT, colour::ON_ACCENT)
    } else {
        (colour::CTL, colour::DIM)
    };
    painter.rect_filled(rect, size::CORNER, fill);
    let icon_rect = Rect::from_center_size(centre, vec2(size::BUTTON_ICON, size::BUTTON_ICON));
    icons::paint(painter, &icons::EAR, icon_rect, ink);
}

/// The add-channel column: a plus over "ADD CHANNEL" set vertically, top to bottom, centred.
pub fn add_column(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::ZERO, colour::WIN);
    let text = galley(
        painter,
        "ADD CHANNEL",
        font::sans_semibold(font::ADD_COLUMN),
        colour::FAINT,
        Spacing::Em(font::ADD_COLUMN_SPACING),
    );
    let (length, thickness) = (text.size().x, text.size().y);
    let total = size::ADD_COLUMN_ICON + size::ADD_COLUMN_GAP + length;
    let top = rect.center().y - total / 2.0;
    let plus = Rect::from_center_size(
        pos2(rect.center().x, top + size::ADD_COLUMN_ICON / 2.0),
        vec2(size::ADD_COLUMN_ICON, size::ADD_COLUMN_ICON),
    );
    icons::paint(painter, &icons::PLUS, plus, colour::FAINT);
    // `writing-mode: vertical-rl`: turned a quarter clockwise about the galley's top-left
    // corner, the text runs down and its height lies to the left of that corner.
    let corner = pos2(
        rect.center().x + thickness / 2.0,
        plus.bottom() + size::ADD_COLUMN_GAP,
    );
    painter
        .add(TextShape::new(corner, text, colour::FAINT).with_angle(std::f32::consts::FRAC_PI_2));
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
        let zero = SCALE.iter().find(|(_, label)| *label == "0").unwrap().0;
        assert_eq!(fader_y(column, taper::UNITY), fader_y(column, zero));
        assert_eq!(fader_y(column, 2.0), 106.0, "clamped to the top");
        // The scale's marks are the taper's: +6 at the top, 48 at -48 dB.
        assert_eq!(taper::position_to_db(SCALE[0].0), 6.0);
        assert_eq!(taper::position_to_db(SCALE[5].0), -48.0);
    }
}
