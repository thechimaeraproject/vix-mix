//! A fader (`MeterFader`): the stereo meter bars with their peak-hold lines, the fader line
//! across them and its triangle at the taper's position, and the prototype's gestures.
//!
//! - **Drag anywhere** on the column: the position moves by the pointer's travel over the
//!   bars' height, relative to where it was (a press does not jump it); with Shift a fifth
//!   as far. Each move sends the fader's command; on a linked channel the model moves the
//!   other send by as much.
//! - **Double-click**: back to unity (0 dB).
//! - **Keys**, once the fader has focus (a press gives it): Up or Right 0.01, Down or Left
//!   0.01 the other way, a fifth of that with Shift; Home to the bottom, End to the top.
//! - **Muted**: the line and triangle faint, the bars at 40 %.

use egui::epaint::{Mesh, PathShape};
use egui::{
    CornerRadius, CursorIcon, Event, EventFilter, Id, Key, Painter, Rect, Sense, Stroke, Ui, pos2,
};
use vixmix_model::{ChannelId, Command, MixId, taper};

use crate::meters::{Bar, Pair};
use crate::shell::Intent;
use crate::strip::fader_y;
use crate::theme::{colour, size};
use crate::widgets;

/// Which fader: a channel's send to a mix, or a mix's own level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Send { ch: ChannelId, mix: MixId },
    Level(MixId),
}

impl Target {
    /// The command that puts this fader at `position`.
    pub fn command(self, position: f32) -> Command {
        match self {
            Target::Send { ch, mix } => Command::SetSend { ch, mix, position },
            Target::Level(mix) => Command::SetMixLevel { mix, position },
        }
    }

    /// The widget id of this fader's column.
    pub fn id(self) -> Id {
        Id::new(("fader", self))
    }
}

/// Which side of its column a fader's triangle is on: a channel's Monitor fader has it on the
/// left pointing in, every other fader on the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A drag in progress: which fader, where it has got to, the pointer's last height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub target: Target,
    pub position: f32,
    pub last_y: f32,
}

/// How far a key moves a fader, and with Shift.
pub const KEY_STEP: f32 = 0.01;
pub const FINE_KEY_STEP: f32 = 0.002;
/// How much of the pointer's travel moves a fader with Shift held.
pub const FINE_DRAG: f32 = 0.2;

/// What one fader shows.
pub struct Look<'a> {
    pub side: Side,
    /// A mix's wide fader (`.vf.big`).
    pub big: bool,
    pub position: f32,
    pub bars: &'a Pair,
    pub muted: bool,
}

/// One fader over `column`: senses its gestures, pushes their commands to `out`, keeps a
/// drag in `drag`, and paints it.
pub fn fader(
    ui: &Ui,
    column: Rect,
    target: Target,
    look: &Look<'_>,
    drag: &mut Option<Drag>,
    keyboard: bool,
    out: &mut Vec<Intent>,
) {
    let id = target.id();
    let response = ui.interact(column, id, Sense::click_and_drag());
    let travel = column.height() - 2.0 * size::FADER_INSET;
    let held = response.is_pointer_button_down_on();
    if held {
        let (y, shift) = ui.input(|i| {
            (
                i.pointer.interact_pos().map_or(column.center().y, |p| p.y),
                i.modifiers.shift,
            )
        });
        let current = match drag {
            Some(d) if d.target == target => d,
            _ => {
                take_focus(&response);
                let origin = ui.input(|i| i.pointer.press_origin()).map_or(y, |p| p.y);
                drag.insert(Drag {
                    target,
                    position: look.position,
                    last_y: origin,
                })
            }
        };
        if y != current.last_y {
            let scale = if shift { FINE_DRAG } else { 1.0 };
            let moved = (current.position + (current.last_y - y) / travel * scale).clamp(0.0, 1.0);
            current.last_y = y;
            if moved != current.position {
                current.position = moved;
                out.push(Intent::Apply(target.command(moved)));
            }
        }
    } else if drag.is_some_and(|d| d.target == target) {
        *drag = None;
    }
    if response.clicked() {
        take_focus(&response);
    }
    if response.double_clicked() {
        out.push(Intent::Apply(target.command(taper::UNITY)));
    }
    if response.has_focus() {
        keys(ui, id, target, look.position, out);
    }
    let hot = response.hovered() || held;
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    paint(ui.painter(), column, look, hot);
    if keyboard && response.has_focus() {
        widgets::ring(
            ui.painter(),
            bars_rect(column, look),
            size::FADER_FOCUS_OFFSET,
        );
    }
}

/// Gives a pressed fader (or slot) the keys.
pub fn take_focus(response: &egui::Response) {
    if !response.has_focus() {
        response.request_focus();
    }
}

/// Keeps the focus where it is this frame: egui moves it with an arrow key until the
/// focused control's filter says otherwise, which it can say only from its second frame.
pub fn keep_focus(ui: &Ui) {
    ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
}

/// The keys a focused fader answers, each from where the last one left it.
fn keys(ui: &Ui, id: Id, target: Target, position: f32, out: &mut Vec<Intent>) {
    // The arrows move the fader, not the focus.
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..EventFilter::default()
            },
        );
    });
    let mut at = position;
    let events = ui.input(|i| i.events.clone());
    for event in events {
        let Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            continue;
        };
        let step = if modifiers.shift {
            FINE_KEY_STEP
        } else {
            KEY_STEP
        };
        let next = match key {
            Key::ArrowUp | Key::ArrowRight => at + step,
            Key::ArrowDown | Key::ArrowLeft => at - step,
            Key::Home => 0.0,
            Key::End => 1.0,
            _ => continue,
        }
        .clamp(0.0, 1.0);
        keep_focus(ui);
        if next != at {
            at = next;
            out.push(Intent::Apply(target.command(next)));
        }
    }
}

/// The rect the two bars fill inside `column`.
fn bars_rect(column: Rect, look: &Look<'_>) -> Rect {
    let bar = if look.big { size::BAR_BIG } else { size::BAR };
    let width = 2.0 * bar + size::BAR_GAP;
    let x = match look.side {
        Side::Left => column.right() - width,
        Side::Right => column.left(),
    };
    Rect::from_x_y_ranges(
        x..=x + width,
        column.top() + size::FADER_INSET..=column.bottom() - size::FADER_INSET,
    )
}

/// Paints the fader: the bars, the line (thicker when `hot`) and its triangle.
pub fn paint(painter: &Painter, column: Rect, look: &Look<'_>, hot: bool) {
    let bar = if look.big { size::BAR_BIG } else { size::BAR };
    let bars = bars_rect(column, look);
    let opacity = if look.muted {
        colour::MUTED_BARS_OPACITY
    } else {
        1.0
    };
    meter_bar(
        painter,
        Rect::from_x_y_ranges(bars.left()..=bars.left() + bar, bars.y_range()),
        &look.bars[0],
        opacity,
    );
    meter_bar(
        painter,
        Rect::from_x_y_ranges(bars.right() - bar..=bars.right(), bars.y_range()),
        &look.bars[1],
        opacity,
    );

    // The line reaches from the triangle's base to a point past the bars' far side
    // (`.vf.l .ln{left:6px;right:-1px}`, `.vf.r .ln{left:-1px;right:6px}`, 7 on a mix).
    let ink = if look.muted {
        colour::FAINT
    } else {
        colour::ACCENT
    };
    let line_y = fader_y(column, look.position);
    let [depth, height] = size::TRIANGLE;
    let (from, to) = match look.side {
        Side::Left => (
            column.left() + depth,
            column.right() + size::FADER_LINE_OVERHANG,
        ),
        Side::Right => (
            column.left() - size::FADER_LINE_OVERHANG,
            column.right()
                - depth
                - if look.big {
                    size::BIG_LINE_SHORTFALL
                } else {
                    0.0
                },
        ),
    };
    let thickness = if hot {
        size::FADER_LINE_HOVER
    } else {
        size::FADER_LINE
    };
    painter.rect_filled(
        Rect::from_x_y_ranges(
            from..=to,
            line_y - thickness / 2.0..=line_y + thickness / 2.0,
        ),
        CornerRadius::ZERO,
        ink,
    );
    let (base, tip) = match look.side {
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
fn meter_bar(painter: &Painter, rect: Rect, bar: &Bar, opacity: f32) {
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
