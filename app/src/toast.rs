//! The toast line (`.toast`): one message at a time at the bottom of the window, centred,
//! fading in and out. A message with an action (Undo) shows 5 s, one without 2.4 s, and a
//! message from the host (which says what went wrong and what to do) 8 s. A new message
//! takes the old one's place, and the old one's Undo with it.
//!
//! **Undo.** Removing an insert or a channel shows the host's "Removed …" with Undo
//! ([`HostEvent::Removed`](vixmix_host::HostEvent::Removed)), for the host's
//! [`UNDO_TOAST`](vixmix_host::UNDO_TOAST). Undo applies the one command the host handed
//! out with it, a `RestoreChannel` or `RestoreInsert`: the channel comes back at its place,
//! the insert in its slot, each with the plugin instances the host kept (their state and a
//! reverb's tail included). The Undo dies with its toast, and a new message takes its place.

use egui::{
    Align2, Area, Context, CornerRadius, Frame, Id, Margin, Order, Rect, Stroke, pos2, vec2,
};
use vixmix_model::Command;

use crate::shell::Intent;
use crate::strip;
use crate::theme::{colour, font, pop, size};
use crate::widgets::{self, ButtonKind};

/// One message.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub message: String,
    /// What the Undo button applies.
    pub undo: Option<Command>,
    /// When it starts to fade, in the window's seconds.
    pub until: f64,
}

/// The toast line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Toasts {
    current: Option<Toast>,
}

impl Toasts {
    /// Shows `message` from `now` for `seconds`, with `undo` behind its button.
    pub fn show(
        &mut self,
        message: impl Into<String>,
        undo: Option<Command>,
        now: f64,
        seconds: f64,
    ) {
        self.current = Some(Toast {
            message: message.into(),
            undo,
            until: now + seconds,
        });
    }

    /// Shows one of the window's own messages, with no action, for 2.4 s.
    pub fn info(&mut self, message: impl Into<String>, now: f64) {
        self.show(message, None, now, pop::TOAST_S);
    }

    /// The toast showing at `now`, if any.
    pub fn current(&self, now: f64) -> Option<&Toast> {
        self.current.as_ref().filter(|t| now < t.until)
    }

    /// The Undo button: takes the command that puts it back, and the toast goes.
    pub fn take_undo(&mut self, now: f64) -> Option<Command> {
        let toast = self.current.take_if(|t| now < t.until)?;
        toast.undo
    }

    /// Forgets a toast that has gone, and what its Undo would have put back with it.
    pub fn expire(&mut self, now: f64) {
        let fade = f64::from(pop::TOAST_FADE_S);
        if self.current.as_ref().is_some_and(|t| now >= t.until + fade) {
            self.current = None;
        }
    }

    /// Draws the toast at the bottom of `screen`, fading as it comes and goes; its Undo
    /// pushes [`Intent::Undo`].
    pub fn draw(&self, ctx: &Context, screen: Rect, now: f64, out: &mut Vec<Intent>) {
        // A toast past its time still draws while it fades.
        let Some(toast) = self.current(now).or(self.current.as_ref()) else {
            return;
        };
        let showing = now < toast.until;
        let opacity = ctx.animate_bool_with_time(Id::new("toast fade"), showing, pop::TOAST_FADE_S);
        if opacity <= 0.0 {
            return;
        }
        if showing {
            ctx.request_repaint_after_secs((toast.until - now) as f32);
        }
        // Laid out before its area, so a new message is centred from its first frame.
        let [top, right, bottom, left] = pop::TOAST_PAD;
        let button = toast
            .undo
            .as_ref()
            .map(|_| widgets::button_width(ctx, "Undo"));
        let wrap = (screen.width() - 2.0 * pop::EDGE_MARGIN - left - right).max(0.0);
        let room = wrap - button.map_or(0.0, |w| w + pop::TOAST_GAP);
        let text = ctx.fonts_mut(|f| {
            f.layout_job(egui::text::LayoutJob::simple(
                toast.message.clone(),
                font::sans(font::BODY),
                colour::TEXT,
                room,
            ))
        });
        let text_size = text.size();
        let inner = vec2(
            text_size.x + button.map_or(0.0, |w| w + pop::TOAST_GAP),
            text_size.y.max(if button.is_some() {
                pop::BUTTON_HEIGHT
            } else {
                0.0
            }),
        );
        let outer = inner + vec2(left + right, top + bottom) + vec2(2.0, 2.0) * size::HAIRLINE;
        let at = pos2(
            screen.center().x - outer.x / 2.0,
            screen.bottom() - pop::TOAST_BOTTOM - outer.y,
        );
        Area::new(Id::new("toast"))
            .order(Order::Tooltip)
            .fade_in(false)
            .fixed_pos(at)
            .show(ctx, |ui| {
                ui.set_opacity(opacity);
                Frame::NONE
                    .fill(colour::CTL)
                    .stroke(Stroke::new(size::HAIRLINE, colour::EDGE))
                    .corner_radius(CornerRadius::same(pop::CORNER))
                    .inner_margin(Margin {
                        left: left as i8,
                        right: right as i8,
                        top: top as i8,
                        bottom: bottom as i8,
                    })
                    .show(ui, |ui| {
                        let (rect, _) = ui.allocate_exact_size(inner, egui::Sense::hover());
                        strip::place(
                            ui.painter(),
                            text,
                            Align2::LEFT_CENTER,
                            pos2(rect.left(), rect.center().y),
                        );
                        if button.is_some() {
                            let x = rect.left() + text_size.x + pop::TOAST_GAP;
                            let response = widgets::button(
                                ui,
                                Id::new("toast undo"),
                                x,
                                rect.center().y,
                                "Undo",
                                ButtonKind::Action,
                            );
                            if response.clicked() && showing {
                                out.push(Intent::Undo);
                            }
                        }
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toast_shows_for_its_time_and_its_undo_goes_with_it() {
        let mut toasts = Toasts::default();
        let undo = Command::RestoreChannel {
            at: 0,
            channel: vixmix_model::example::setup().channels[0].clone(),
        };
        toasts.show(
            "Removed Mic",
            Some(undo.clone()),
            10.0,
            pop::TOAST_WITH_ACTION_S,
        );
        assert!(toasts.current(14.9).is_some());
        assert!(toasts.current(15.0).is_none(), "gone after five seconds");
        assert_eq!(toasts.take_undo(15.0), None, "and its Undo with it");
        toasts.show(
            "Removed Mic",
            Some(undo.clone()),
            20.0,
            pop::TOAST_WITH_ACTION_S,
        );
        // A new message takes the old one's place, and its Undo.
        toasts.info("Added OTT to Game", 21.0);
        assert_eq!(toasts.take_undo(21.5), None);
        toasts.show(
            "Removed Mic",
            Some(undo.clone()),
            30.0,
            pop::TOAST_WITH_ACTION_S,
        );
        assert_eq!(toasts.take_undo(31.0), Some(undo));
        assert!(toasts.current(31.0).is_none(), "Undo hides it");
        toasts.info("Moved", 40.0);
        toasts.expire(40.0 + pop::TOAST_S + 1.0);
        assert_eq!(toasts, Toasts::default());
    }
}
