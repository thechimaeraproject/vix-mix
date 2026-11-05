//! A strip's FX well: the header (FX, and on a channel the MON and MST toggles), the inserts
//! with their bypass squares, and "+ Add plugin", in a list that scrolls when it is longer
//! than the well (the charter: no cap on plugins per strip).
//!
//! **Gestures.** MON and MST toggle whether that mix's send takes the chain. A slot's square
//! bypasses it; under the pointer (or with focus inside it) the slot shows a gear, which
//! opens the plugin's editor, and a bin, which removes it (with Undo on the toast);
//! double-clicking the slot opens the editor too. A press on the slot that moves 5 points
//! starts a drag ([`crate::drag`]). With focus on a slot: Enter opens the editor, Space
//! bypasses, Delete or Backspace removes, Alt with Up or Down moves it, Up or Down moves the
//! focus. "+ Add plugin" opens the plugin browser beside it.
//!
//! **A slot playing as a pass-through** (its plugin missing, or it could not be set up:
//! [`Host::problems`](vixmix_host::Host::problems)) is marked in the mute colour, its square
//! outlined and its name, and says why and what to do under the pointer. The prototype has
//! no such state; the host's banner line used to carry the first one.

use egui::containers::scroll_area::{DragScroll, ScrollSource};
use egui::{
    Align, Align2, CursorIcon, Event, EventFilter, Id, Key, Layout, Rect, ScrollArea, Sense,
    Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};
use vixmix_model::{ChannelId, Command, Insert, MixId, SlotRef, Strip};

use crate::console::{Anchor, Cx, WellRects};
use crate::drag::InsertDrag;
use crate::icons;
use crate::shell::Intent;
use crate::strip::{self, Spacing};
use crate::theme::{colour, font, size};
use crate::widgets;

/// What a well shows.
pub struct WellView<'a> {
    pub strip: Strip,
    pub chain: &'a [Insert],
    /// A channel's MON and MST: the channel, and whether each mix's send takes the chain.
    pub toggles: Option<(ChannelId, [bool; 2])>,
}

/// The widget id of an insert's slot.
pub fn slot_id(insert: &Insert) -> Id {
    Id::new(("slot", insert.instance))
}

/// The well over `rect`: paints it, senses its gestures, records its rects for a drag.
pub fn well(ui: &mut Ui, rect: Rect, strip_rect: Rect, view: &WellView<'_>, cx: &mut Cx<'_>) {
    let painter = ui.painter().clone();
    painter.rect_filled(rect, size::CORNER, colour::WELL);
    let head = Rect::from_x_y_ranges(rect.x_range(), rect.top()..=rect.top() + size::FX_HEAD);
    // The rule is the header's last point; the label centres in what is above it.
    painter.hline(
        head.x_range(),
        head.bottom() - size::HAIRLINE_INSET,
        Stroke::new(size::HAIRLINE, colour::LINE),
    );
    let y = head.top() + (size::FX_HEAD - size::HAIRLINE) / 2.0;
    let fx = strip::galley(
        &painter,
        "FX",
        font::sans_semibold(font::FX_HEAD),
        colour::FAINT,
        Spacing::Em(font::FX_HEAD_SPACING),
    );
    strip::place(
        &painter,
        fx,
        Align2::LEFT_CENTER,
        pos2(head.left() + size::FX_HEAD_PAD_LEFT, y),
    );
    if let Some((ch, [monitor, master])) = view.toggles {
        let mut right = head.right() - size::FX_HEAD_PAD_RIGHT;
        for (mix, on) in [(MixId::Master, master), (MixId::Monitor, monitor)] {
            right = toggle(ui, pos2(right, y), ch, mix, on, cx) - size::TOGGLE_GAP;
        }
    }

    let list = Rect::from_x_y_ranges(rect.x_range(), head.bottom()..=rect.bottom());
    let inner = list.shrink(size::SLOTS_PAD);
    let mut rects = WellRects {
        strip: view.strip,
        strip_rect,
        list,
        slots: Vec::with_capacity(view.chain.len()),
        add: Rect::NOTHING,
    };
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .id_salt(("well", view.strip))
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    let mut slots = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let width = ui.available_width();
        for (index, insert) in view.chain.iter().enumerate() {
            let (_, slot) = ui.allocate_space(vec2(width, size::SLOT));
            slot_row(ui, slot, view, index, insert, cx);
            rects.slots.push(slot);
        }
        let (_, add) = ui.allocate_space(vec2(width, size::SLOT));
        add_row(ui, add, view.strip, cx);
        rects.add = add;
    };
    // A chain that fits needs no scrolling, and a scroll area costs every frame.
    if (view.chain.len() + 1) as f32 * size::SLOT <= inner.height() {
        slots(&mut child);
    } else {
        ScrollArea::vertical()
            .id_salt(("slots", view.strip))
            .auto_shrink([false, false])
            .scroll_source(ScrollSource {
                drag: DragScroll::Never,
                ..ScrollSource::default()
            })
            .show(&mut child, slots);
    }
    cx.layout
        .anchors
        .push((Anchor::AddPlugin(view.strip), rects.add));
    cx.layout.wells.push(rects);
}

/// MON or MST, its right edge at `right_centre`; returns its left edge.
fn toggle(
    ui: &Ui,
    right_centre: egui::Pos2,
    ch: ChannelId,
    mix: MixId,
    on: bool,
    cx: &mut Cx<'_>,
) -> f32 {
    let label = match mix {
        MixId::Monitor => "MON",
        MixId::Master => "MST",
    };
    let (fill, ink) = if on {
        (colour::ACCENT, colour::ON_ACCENT)
    } else {
        (colour::CTL, colour::FAINT)
    };
    let painter = ui.painter();
    let text = strip::galley(
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
    let response = widgets::press(ui, rect, Id::new(("chain", ch, mix))).on_hover_text(match mix {
        MixId::Monitor => "Monitor gets this chain",
        MixId::Master => "Master gets this chain",
    });
    if response.clicked() {
        cx.out
            .push(Intent::Apply(Command::SetChainRouting { ch, mix, on: !on }));
    }
    painter.rect_filled(rect, size::CORNER, fill);
    strip::place(painter, text, Align2::CENTER_CENTER, rect.center());
    if cx.keyboard && response.has_focus() {
        widgets::ring(painter, rect, size::FOCUS_OFFSET);
    }
    rect.left()
}

/// One insert: its slot senses a press (a drag's start), a double-click and its keys; the
/// square, and under the pointer the gear and the bin, sit on top of it.
fn slot_row(
    ui: &Ui,
    slot: Rect,
    view: &WellView<'_>,
    index: usize,
    insert: &Insert,
    cx: &mut Cx<'_>,
) {
    let instance = insert.instance;
    let id = slot_id(insert);
    let response = ui.interact(slot, id, Sense::click_and_drag());
    let name = &insert.plugin.name;
    let problem = cx
        .host
        .problems()
        .iter()
        .find(|p| p.instance == instance)
        .map(|p| p.message.as_str());
    let vendor = cx
        .host
        .catalog()
        .get(&insert.plugin.id)
        .map(|p| p.vendor.clone());
    let response = match (problem, vendor) {
        (Some(problem), _) => response.on_hover_text(problem),
        (None, Some(vendor)) => response.on_hover_text(format!("{name} \u{b7} {vendor}")),
        (None, None) => response.on_hover_text(name.as_str()),
    };
    let from = SlotRef {
        strip: view.strip,
        index,
    };
    if response.is_pointer_button_down_on()
        && ui.input(|i| i.pointer.primary_pressed())
        && cx.gestures.insert.is_none()
    {
        crate::fader::take_focus(&response);
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .unwrap_or(slot.center());
        cx.gestures.insert = Some(InsertDrag::new(from, instance, name.clone(), origin));
    }
    let dragging = cx
        .gestures
        .insert
        .as_ref()
        .is_some_and(|d| d.active && d.from == from);
    if response.hovered() && !dragging {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    if response.clicked() {
        crate::fader::take_focus(&response);
    }
    if response.double_clicked() {
        cx.out.push(Intent::OpenEditor(instance));
    }
    if response.has_focus() {
        slot_keys(ui, id, view, index, insert, cx);
    }

    let opacity = if dragging {
        colour::DRAGGED_OPACITY
    } else {
        1.0
    };
    let painter = ui.painter();
    let gear_id = Id::new(("gear", instance));
    let bin_id = Id::new(("bin", instance));
    let focus_within = [id, gear_id, bin_id, Id::new(("bypass", instance))]
        .into_iter()
        .any(|w| ui.memory(|m| m.has_focus(w)));
    let hot = !dragging && (widgets::hovering(ui, slot) || focus_within);
    if hot {
        painter.rect_filled(slot, size::CORNER, colour::CTL);
    }

    // The bypass square: filled when the insert is on.
    let y = slot.center().y;
    let square = Rect::from_min_size(
        pos2(slot.left() + size::SLOT_PAD_X, y - size::BYPASS / 2.0),
        vec2(size::BYPASS, size::BYPASS),
    );
    let bypass =
        widgets::press(ui, square, Id::new(("bypass", instance))).on_hover_text(match problem {
            Some(problem) => problem.to_string(),
            None if insert.enabled => format!("{name} on"),
            None => format!("{name} bypassed"),
        });
    if bypass.clicked() {
        cx.out.push(Intent::Apply(Command::SetInsertEnabled {
            instance,
            enabled: !insert.enabled,
        }));
    }
    if problem.is_some() {
        painter.rect_stroke(
            square,
            size::BYPASS_CORNER,
            Stroke::new(size::HAIRLINE, colour::MUTE.gamma_multiply(opacity)),
            StrokeKind::Inside,
        );
    } else if insert.enabled {
        painter.rect_filled(
            square,
            size::BYPASS_CORNER,
            colour::ACCENT.gamma_multiply(opacity),
        );
    } else {
        painter.rect_stroke(
            square,
            size::BYPASS_CORNER,
            Stroke::new(size::HAIRLINE, colour::FAINT.gamma_multiply(opacity)),
            StrokeKind::Inside,
        );
    }
    let x = square.right() + size::SLOT_GAP;
    let ink = if problem.is_some() {
        colour::MUTE
    } else if insert.enabled {
        colour::TEXT
    } else {
        colour::FAINT
    };
    let text = strip::truncated(
        painter,
        name,
        font::sans(font::SLOT),
        ink.gamma_multiply(opacity),
        slot.right() - size::SLOT_PAD_X - x,
    );
    strip::place(painter, text, Align2::LEFT_CENTER, pos2(x, y));
    if cx.keyboard && response.has_focus() {
        widgets::ring(painter, slot, size::FOCUS_OFFSET);
    }

    if hot {
        hover_controls(ui, slot, insert, gear_id, bin_id, cx);
    }
}

/// The gear and the bin at a slot's right end, on the control face.
fn hover_controls(ui: &Ui, slot: Rect, insert: &Insert, gear_id: Id, bin_id: Id, cx: &mut Cx<'_>) {
    let painter = ui.painter();
    let [inset_x, inset_y] = size::HOVER_INSET;
    let right = slot.right() - inset_x;
    let top = slot.top() + inset_y;
    let width = size::HOVER_PAD_LEFT + 2.0 * size::HOVER_BUTTON + size::HOVER_GAP;
    let strip_rect = Rect::from_min_max(
        pos2(right - width, top),
        pos2(right, top + size::HOVER_BUTTON),
    );
    painter.rect_filled(strip_rect, egui::CornerRadius::ZERO, colour::CTL);
    let name = &insert.plugin.name;
    let buttons = [
        (gear_id, &icons::GEAR, format!("Open {name}")),
        (bin_id, &icons::TRASH, format!("Remove {name}")),
    ];
    let mut x = strip_rect.left() + size::HOVER_PAD_LEFT;
    for (id, icon, tip) in buttons {
        let rect = Rect::from_min_size(pos2(x, top), vec2(size::HOVER_BUTTON, size::HOVER_BUTTON));
        let response = widgets::press(ui, rect, id).on_hover_text(tip);
        let ink = if response.hovered() {
            painter.rect_filled(rect, size::CORNER, colour::CTL_HI);
            colour::TEXT
        } else {
            colour::DIM
        };
        icons::paint(
            painter,
            icon,
            Rect::from_center_size(rect.center(), vec2(size::HOVER_ICON, size::HOVER_ICON)),
            ink,
        );
        if response.clicked() {
            cx.out.push(if id == gear_id {
                Intent::OpenEditor(insert.instance)
            } else {
                Intent::Apply(Command::RemoveInsert {
                    instance: insert.instance,
                })
            });
        }
        if cx.keyboard && response.has_focus() {
            widgets::ring(painter, rect, 0.0);
        }
        x += size::HOVER_BUTTON + size::HOVER_GAP;
    }
}

/// A focused slot's keys (the prototype's): Enter opens, Space bypasses, Delete or
/// Backspace removes, Alt with Up or Down moves the insert, Up or Down moves the focus.
fn slot_keys(ui: &Ui, id: Id, view: &WellView<'_>, index: usize, insert: &Insert, cx: &mut Cx<'_>) {
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            EventFilter {
                vertical_arrows: true,
                ..EventFilter::default()
            },
        );
    });
    let chain = view.chain;
    let focus = |at: usize| {
        if let Some(next) = chain.get(at) {
            ui.memory_mut(|m| m.request_focus(slot_id(next)));
        }
    };
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
        match key {
            Key::Enter => cx.out.push(Intent::OpenEditor(insert.instance)),
            Key::Space => cx.out.push(Intent::Apply(Command::SetInsertEnabled {
                instance: insert.instance,
                enabled: !insert.enabled,
            })),
            Key::Delete | Key::Backspace => {
                cx.out.push(Intent::Apply(Command::RemoveInsert {
                    instance: insert.instance,
                }));
                // The focus goes to the insert that takes this one's place.
                if index + 1 < chain.len() {
                    focus(index + 1);
                } else if index > 0 {
                    focus(index - 1);
                }
            }
            Key::ArrowUp | Key::ArrowDown if modifiers.alt => {
                crate::fader::keep_focus(ui);
                let to = match key {
                    Key::ArrowUp if index > 0 => index - 1,
                    // The gap after the next insert: below it.
                    Key::ArrowDown if index + 1 < chain.len() => index + 2,
                    _ => continue,
                };
                cx.out.push(Intent::Apply(Command::MoveInsert {
                    from: SlotRef {
                        strip: view.strip,
                        index,
                    },
                    to: SlotRef {
                        strip: view.strip,
                        index: to,
                    },
                    copy: false,
                }));
            }
            Key::ArrowUp | Key::ArrowDown => {
                crate::fader::keep_focus(ui);
                match key {
                    Key::ArrowUp if index > 0 => focus(index - 1),
                    Key::ArrowDown => focus(index + 1),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// "+ Add plugin": the browser opens beside it.
fn add_row(ui: &Ui, rect: Rect, strip: Strip, cx: &mut Cx<'_>) {
    let response = widgets::press(ui, rect, Id::new(("add plugin", strip)));
    if response.clicked() {
        cx.out.push(Intent::Toggle(Anchor::AddPlugin(strip)));
    }
    let painter = ui.painter();
    let ink = if response.hovered() {
        painter.rect_filled(rect, size::CORNER, colour::CTL);
        colour::TEXT
    } else {
        colour::FAINT
    };
    let y = rect.center().y;
    let plus = Rect::from_center_size(
        pos2(rect.left() + size::SLOT_PAD_X + size::ADD_ICON / 2.0, y),
        vec2(size::ADD_ICON, size::ADD_ICON),
    );
    icons::paint(painter, &icons::PLUS, plus, ink);
    let text = strip::galley(
        painter,
        "Add plugin",
        font::sans(font::SLOT),
        ink,
        Spacing::None,
    );
    strip::place(
        painter,
        text,
        Align2::LEFT_CENTER,
        pos2(plus.right() + size::ADD_GAP, y),
    );
    if cx.keyboard && response.has_focus() {
        widgets::ring(painter, rect, size::FOCUS_OFFSET);
    }
}
