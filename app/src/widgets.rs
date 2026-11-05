//! The small pieces every surface shares: whether the pointer is over a rect, the keyboard
//! focus ring, the prototype's text button (`.btn`), tick box and radio, a list heading, the
//! single-line text field, and a popover's frame.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Margin, Painter, Rect, Response,
    Sense, Shadow, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::icons;
use crate::strip::{self, Spacing};
use crate::theme::{colour, font, pop, size};

/// Whether the pointer is over `rect` on this ui's layer, with nothing else held: the CSS
/// `:hover` of a row whose own buttons sit on top of it.
pub fn hovering(ui: &Ui, rect: Rect) -> bool {
    ui.rect_contains_pointer(rect) && !ui.input(|i| i.pointer.any_down())
}

/// The keyboard focus ring round `rect`, `offset` outside it (CSS `:focus-visible`). The
/// window draws it on the focused control while `Shell::keyboard` holds: from a Tab until
/// the next press of the pointer.
pub fn ring(painter: &Painter, rect: Rect, offset: f32) {
    painter.rect_stroke(
        rect.expand(offset + size::FOCUS_RING),
        size::CORNER,
        Stroke::new(size::FOCUS_RING, colour::ACCENT),
        StrokeKind::Inside,
    );
}

/// A pressable rect: `response` with the hand cursor over it.
pub fn press(ui: &Ui, rect: Rect, id: Id) -> Response {
    // A button that also senses a drag keeps a press on it from starting the drag of the
    // row under it (a slot), as the prototype's buttons do.
    let response = ui.interact(rect, id, Sense::click_and_drag());
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// How a text button reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// `.btn`: text on the control face.
    Plain,
    /// `.btn.danger`: the mute colour, no face.
    Danger,
    /// `.toast .btn`: the accent on the lighter face.
    Action,
}

/// The width of a text button reading `label`.
pub fn button_width(ctx: &egui::Context, label: &str) -> f32 {
    let text = ctx
        .fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font::sans(font::SMALL), colour::TEXT));
    text.size().x + 2.0 * pop::BUTTON_PAD_X
}

/// A text button (`.btn`) with its left edge at `left`, centred on `y`.
pub fn button(ui: &Ui, id: Id, left: f32, y: f32, label: &str, kind: ButtonKind) -> Response {
    let width = button_width(ui.ctx(), label);
    let rect = Rect::from_min_size(
        pos2(left, y - pop::BUTTON_HEIGHT / 2.0),
        vec2(width, pop::BUTTON_HEIGHT),
    );
    let response = press(ui, rect, id);
    let hover = response.hovered();
    let (fill, ink) = match kind {
        ButtonKind::Plain => (
            if hover { colour::CTL_HI } else { colour::CTL },
            colour::TEXT,
        ),
        ButtonKind::Danger => (
            if hover {
                colour::CTL_HI
            } else {
                Color32::TRANSPARENT
            },
            colour::MUTE,
        ),
        ButtonKind::Action => (colour::CTL_HI, colour::ACCENT),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, size::CORNER, fill);
    let text = strip::galley(painter, label, font::sans(font::SMALL), ink, Spacing::None);
    strip::place(painter, text, Align2::CENTER_CENTER, rect.center());
    response
}

/// A tick box at `centre`: a dark square, or the accent with a tick when on.
pub fn tick_box(painter: &Painter, centre: egui::Pos2, on: bool) {
    let rect = Rect::from_center_size(centre, vec2(pop::TICK, pop::TICK));
    if on {
        painter.rect_filled(rect, size::CORNER, colour::ACCENT);
        icons::paint(painter, &icons::TICK, rect, colour::ON_ACCENT);
    } else {
        painter.rect_filled(rect, size::CORNER, colour::BLACK);
        painter.rect_stroke(
            rect,
            size::CORNER,
            Stroke::new(size::HAIRLINE, colour::FAINT),
            StrokeKind::Inside,
        );
    }
}

/// A radio at `centre`: a dark ring, or the accent ring with its dot when picked.
pub fn radio(painter: &Painter, centre: egui::Pos2, on: bool) {
    let r = pop::TICK / 2.0;
    let (ring, ink) = if on {
        (pop::RADIO_RING, colour::ACCENT)
    } else {
        (size::HAIRLINE, colour::FAINT)
    };
    // The ring's stroke is centred on its radius: half of it inside the 13-point box.
    painter.circle(
        centre,
        r - ring / 2.0,
        colour::BLACK,
        Stroke::new(ring, ink),
    );
    if on {
        painter.circle_filled(centre, pop::RADIO_DOT, colour::ACCENT);
    }
}

/// A list heading (`.ap-h`, `.nav-h`, `.br-group`): 600, spaced, upper case, faint, its
/// left edge at `x`, centred on `y`.
pub fn heading(painter: &Painter, x: f32, y: f32, text: &str) {
    let text = strip::galley(
        painter,
        &text.to_uppercase(),
        font::sans_semibold(font::HEADING),
        colour::FAINT,
        Spacing::Em(font::HEADING_SPACING),
    );
    strip::place(painter, text, Align2::LEFT_CENTER, pos2(x, y));
}

/// The height of a heading's line of text.
pub fn heading_height(painter: &Painter) -> f32 {
    painter
        .layout_no_wrap(
            "A".into(),
            font::sans_semibold(font::HEADING),
            colour::FAINT,
        )
        .size()
        .y
}

/// A single-line text field over `rect`: no frame of its own (the caller paints the
/// prototype's), in `font`, with a faint `hint`, taking at most `limit` characters.
pub fn text_field(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    text: &mut String,
    (font, limit): (FontId, usize),
    hint: &str,
) -> Response {
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .char_limit(limit)
        .frame(egui::Frame::NONE)
        .font(font)
        .text_color(colour::TEXT)
        .hint_text(egui::RichText::new(hint).color(colour::FAINT))
        .margin(Margin::ZERO)
        .desired_width(rect.width())
        .vertical_align(egui::Align::Center);
    let height = rect.height();
    ui.put(
        Rect::from_min_size(rect.min, vec2(rect.width(), height)),
        edit,
    )
}

/// A popover's frame (`.pop`): its face, a black outline, the 3-point corner, its shadow.
pub fn popover_frame() -> Frame {
    Frame::NONE
        .fill(colour::POPOVER)
        .stroke(Stroke::new(size::HAIRLINE, colour::EDGE))
        .corner_radius(CornerRadius::same(pop::CORNER))
        .shadow(Shadow {
            offset: [0, pop::SHADOW_Y],
            blur: pop::SHADOW_BLUR,
            spread: 0,
            color: colour::SHADOW,
        })
}

/// A horizontal rule along `y` across `x` (the hairline under a name row, over a footer).
pub fn rule(painter: &Painter, x: egui::Rangef, y: f32) {
    painter.hline(
        x,
        y + size::HAIRLINE_INSET,
        Stroke::new(size::HAIRLINE, colour::LINE),
    );
}
