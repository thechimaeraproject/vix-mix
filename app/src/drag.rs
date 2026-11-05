//! Dragging an insert within its strip or to another (the prototype's drag and drop): a
//! press on a slot that moves [`size::DRAG_START`] points starts it; the slot stays put at
//! 30 %, a ghost with the plugin's name follows the pointer ("+ Copy" before it while Ctrl or
//! Alt is held), and the strip under the pointer shows where it would land: a line before
//! the first insert whose middle is below the pointer, or before "+ Add plugin", and an
//! accent outline round the list when it is another strip's. Releasing sends
//! `MoveInsert` (with `copy` while Ctrl or Alt is held); dropping an insert where it already
//! is sends nothing; Escape cancels.

use egui::{
    Align2, Area, CornerRadius, CursorIcon, Frame, Id, Key, Margin, Order, Painter, Pos2, Rect,
    Stroke, StrokeKind, Ui, vec2,
};
use vixmix_model::{Command, InstanceId, SlotRef};

use crate::console::WellRects;
use crate::shell::Intent;
use crate::strip::{self, Spacing};
use crate::theme::{colour, font, pop, size};

/// A press on a slot, and once it has moved far enough, a drag.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertDrag {
    /// The insert, where it is.
    pub from: SlotRef,
    pub instance: InstanceId,
    /// What the ghost says.
    pub name: String,
    /// Where the press was.
    pub origin: Pos2,
    /// Whether it has moved far enough to be a drag.
    pub active: bool,
}

impl InsertDrag {
    /// A press on `from`'s slot at `origin`, not yet a drag.
    pub fn new(from: SlotRef, instance: InstanceId, name: String, origin: Pos2) -> InsertDrag {
        InsertDrag {
            from,
            instance,
            name,
            origin,
            active: false,
        }
    }
}

/// Where a drop at `pointer` lands: the well under the pointer (its position in `wells`)
/// and the drop place in its chain.
pub fn target_at(wells: &[WellRects], pointer: Pos2) -> Option<(usize, usize)> {
    let (well, rects) = wells
        .iter()
        .enumerate()
        .find(|(_, w)| w.strip_rect.contains(pointer))?;
    let index = rects
        .slots
        .iter()
        .position(|slot| pointer.y < slot.center().y)
        .unwrap_or(rects.slots.len());
    Some((well, index))
}

/// The command a drop sends, or `None` when it would leave the insert where it is.
pub fn drop_command(from: SlotRef, to: SlotRef, copy: bool) -> Option<Command> {
    let same = from.strip == to.strip;
    if !copy && same && (to.index == from.index || to.index == from.index + 1) {
        return None;
    }
    Some(Command::MoveInsert { from, to, copy })
}

/// Whether the modifiers held make a drop a copy: Ctrl (Command on a Mac) or Alt.
fn copying(ui: &Ui) -> bool {
    ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt)
}

/// Carries the drag through one frame, once every well has been drawn and recorded: makes a
/// press a drag once it has moved, paints the drop place and the ghost, and on release
/// pushes the move. Returns whether the drag began this frame (the open popover closes).
pub fn resolve(
    ui: &Ui,
    painter: &Painter,
    drag: &mut Option<InsertDrag>,
    wells: &[WellRects],
    out: &mut Vec<Intent>,
) -> bool {
    let Some(current) = drag else {
        return false;
    };
    let (pointer, down, released, escape) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
            i.key_pressed(Key::Escape),
        )
    });
    let mut started = false;
    if !current.active
        && let Some(at) = pointer
        && at.distance(current.origin) >= size::DRAG_START
    {
        current.active = true;
        started = true;
    }
    if current.active && escape {
        *drag = None;
        return started;
    }
    let copy = copying(ui);
    let target = pointer.and_then(|at| target_at(wells, at));
    if current.active {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        if let Some((well, index)) = target {
            drop_place(painter, &wells[well], index, current.from);
        }
        if let Some(at) = pointer {
            ghost(ui, at, &current.name, copy);
        }
    }
    if released {
        if current.active
            && let Some((well, index)) = target
        {
            let to = SlotRef {
                strip: wells[well].strip,
                index,
            };
            if let Some(command) = drop_command(current.from, to, copy) {
                out.push(Intent::Apply(command));
            }
        }
        *drag = None;
    } else if !down {
        // The press ended somewhere this frame did not see (the pointer left the window).
        *drag = None;
    }
    started
}

/// The drop line before slot `index` of `well` (or before "+ Add plugin"), and the outline
/// round the list when it is another strip's.
fn drop_place(painter: &Painter, well: &WellRects, index: usize, from: SlotRef) {
    let clipped = painter.with_clip_rect(well.list.intersect(painter.clip_rect()));
    let before = well.slots.get(index).copied().unwrap_or(well.add);
    let line = Rect::from_x_y_ranges(
        before.left() + size::DROP_INSET..=before.right() - size::DROP_INSET,
        before.top() - size::DROP_LINE..=before.top(),
    );
    clipped.rect_filled(line, CornerRadius::ZERO, colour::ACCENT);
    if well.strip != from.strip {
        clipped.rect_stroke(
            well.list,
            CornerRadius::ZERO,
            Stroke::new(size::HAIRLINE, colour::ACCENT),
            StrokeKind::Inside,
        );
    }
}

/// The ghost beside the pointer: "+ Copy" while copying, then the plugin's name.
fn ghost(ui: &Ui, pointer: Pos2, name: &str, copy: bool) {
    let [dx, dy] = pop::GHOST_OFFSET;
    let [pad_x, pad_y] = pop::GHOST_PAD;
    Area::new(Id::new("insert drag ghost"))
        .order(Order::Tooltip)
        .fade_in(false)
        .interactable(false)
        .fixed_pos(pointer + vec2(dx, dy))
        .show(ui.ctx(), |ui| {
            Frame::NONE
                .fill(colour::CTL_HI)
                .stroke(Stroke::new(size::HAIRLINE, colour::ACCENT))
                .corner_radius(CornerRadius::same(size::CORNER))
                .inner_margin(Margin::symmetric(pad_x as i8, pad_y as i8))
                .show(ui, |ui| {
                    let painter = ui.painter().clone();
                    let mut runs = Vec::new();
                    if copy {
                        runs.push(strip::galley(
                            &painter,
                            "+ Copy",
                            font::sans(font::SMALL),
                            colour::ACCENT,
                            Spacing::None,
                        ));
                    }
                    runs.push(strip::galley(
                        &painter,
                        name,
                        font::sans(font::SMALL),
                        colour::TEXT,
                        Spacing::None,
                    ));
                    let gaps = pop::GHOST_GAP * (runs.len() - 1) as f32;
                    let width = runs.iter().map(|r| r.size().x).sum::<f32>() + gaps;
                    let height = runs.iter().map(|r| r.size().y).fold(0.0, f32::max);
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
                    let mut x = rect.left();
                    for run in runs {
                        let w = run.size().x;
                        strip::place(
                            &painter,
                            run,
                            Align2::LEFT_CENTER,
                            egui::pos2(x, rect.center().y),
                        );
                        x += w + pop::GHOST_GAP;
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;
    use vixmix_model::{ChannelId, Strip};

    fn well(strip: Strip, left: f32, slots: usize) -> WellRects {
        let top = 50.0;
        WellRects {
            strip,
            strip_rect: Rect::from_min_size(pos2(left, 0.0), vec2(100.0, 450.0)),
            list: Rect::from_min_size(pos2(left + 6.0, top - 2.0), vec2(92.0, 132.0)),
            slots: (0..slots)
                .map(|i| {
                    Rect::from_min_size(pos2(left + 8.0, top + 20.0 * i as f32), vec2(88.0, 20.0))
                })
                .collect(),
            add: Rect::from_min_size(
                pos2(left + 8.0, top + 20.0 * slots as f32),
                vec2(88.0, 20.0),
            ),
        }
    }

    #[test]
    fn a_drop_lands_before_the_first_insert_whose_middle_is_below_the_pointer() {
        let a = Strip::Channel(ChannelId(1));
        let b = Strip::Channel(ChannelId(2));
        let wells = [well(a, 0.0, 3), well(b, 110.0, 1)];
        // Over the top half of the second insert: before it.
        assert_eq!(target_at(&wells, pos2(50.0, 75.0)), Some((0, 1)));
        // Over its bottom half: before the third.
        assert_eq!(target_at(&wells, pos2(50.0, 85.0)), Some((0, 2)));
        // Below every insert (the faders): at the end.
        assert_eq!(target_at(&wells, pos2(50.0, 300.0)), Some((0, 3)));
        // Above them (the header): first.
        assert_eq!(target_at(&wells, pos2(50.0, 10.0)), Some((0, 0)));
        // Another strip; the black between strips is none.
        assert_eq!(target_at(&wells, pos2(150.0, 200.0)), Some((1, 1)));
        assert_eq!(target_at(&wells, pos2(105.0, 200.0)), None);
    }

    #[test]
    fn dropping_an_insert_where_it_is_sends_nothing_and_a_copy_always_lands() {
        let a = Strip::Channel(ChannelId(1));
        let at = |index| SlotRef { strip: a, index };
        assert_eq!(drop_command(at(1), at(1), false), None);
        assert_eq!(drop_command(at(1), at(2), false), None, "the gap below it");
        assert_eq!(
            drop_command(at(1), at(0), false),
            Some(Command::MoveInsert {
                from: at(1),
                to: at(0),
                copy: false
            })
        );
        assert_eq!(
            drop_command(at(1), at(1), true),
            Some(Command::MoveInsert {
                from: at(1),
                to: at(1),
                copy: true
            })
        );
    }
}
