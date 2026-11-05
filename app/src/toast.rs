//! The toast line (`.toast`): one message at a time at the bottom of the window, centred,
//! fading in and out. A message with an action (Undo) shows 5 s, one without 2.4 s, and a
//! message from the host (which says what went wrong and what to do) 8 s. A new message
//! takes the old one's place, and the old one's Undo with it.
//!
//! **Undo.** Removing an insert or a channel shows "Removed …" with Undo. The window keeps
//! what was removed while the toast shows and, on Undo, puts it back through the commands
//! the `Host` trait has today ([`replay`]): an insert is added again at its place (bypassed
//! again if it was); a channel is added again with its name, source (less any app another
//! channel has taken since), faders, mutes, link, MON and MST and its inserts. Until the
//! model has change request 2's `RestoreChannel` and `RestoreInsert`, a restored channel
//! comes back at the right end rather than its place, and a restored insert is a new
//! instance that starts from the plugin's defaults rather than the state the host kept.

use egui::{
    Align2, Area, Context, CornerRadius, Frame, Id, Margin, Order, Rect, Stroke, pos2, vec2,
};
use vixmix_host::Host;
use vixmix_model::{Channel, Command, Insert, Refused, Source, Strip};

use crate::shell::Intent;
use crate::strip;
use crate::theme::{colour, font, pop, size};
use crate::widgets::{self, ButtonKind};

/// What an Undo puts back.
#[derive(Clone, Debug, PartialEq)]
pub enum Undo {
    /// An insert, at its place in its strip's chain.
    Insert {
        strip: Strip,
        at: usize,
        insert: Insert,
    },
    /// A channel, which was at `at` among the channels.
    Channel { at: usize, channel: Channel },
}

/// One message.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub message: String,
    pub undo: Option<Undo>,
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
    pub fn show(&mut self, message: impl Into<String>, undo: Option<Undo>, now: f64, seconds: f64) {
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

    /// The Undo button: takes what it puts back, and the toast goes.
    pub fn take_undo(&mut self, now: f64) -> Option<Undo> {
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
        let [top, right, bottom, left] = pop::TOAST_PAD;
        let id = Id::new("toast");
        let width = ctx.memory(|m| m.area_rect(id)).map_or(0.0, |r| r.width());
        let height = ctx.memory(|m| m.area_rect(id)).map_or(0.0, |r| r.height());
        let at = pos2(
            screen.center().x - width / 2.0,
            screen.bottom() - pop::TOAST_BOTTOM - height,
        );
        let wrap = (screen.width() - 2.0 * pop::EDGE_MARGIN - left - right).max(0.0);
        Area::new(id)
            .order(Order::Tooltip)
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
                        let painter = ui.painter().clone();
                        let button = toast
                            .undo
                            .as_ref()
                            .map(|_| widgets::button_width(&painter, "Undo"));
                        let room = wrap - button.map_or(0.0, |w| w + pop::TOAST_GAP);
                        let job = egui::text::LayoutJob::simple(
                            toast.message.clone(),
                            font::sans(font::BODY),
                            colour::TEXT,
                            room,
                        );
                        let text = painter.layout_job(job);
                        let text_size = text.size();
                        let height = text_size.y.max(if button.is_some() {
                            pop::BUTTON_HEIGHT
                        } else {
                            0.0
                        });
                        let width = text_size.x + button.map_or(0.0, |w| w + pop::TOAST_GAP);
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
                        strip::place(
                            &painter,
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

/// Puts back what was removed, through the host's commands; stops at the first refusal
/// (the strip it belonged to is gone, the channels are full) and returns it.
pub fn replay(host: &mut dyn Host, undo: Undo) -> Result<(), Refused> {
    match undo {
        Undo::Insert { strip, at, insert } => {
            restore_insert(host, strip, at, &insert)?;
        }
        Undo::Channel { channel, .. } => {
            let applied = host.apply(Command::AddChannel {
                kind: channel.kind(),
            })?;
            let ch = applied
                .channel
                .expect("AddChannel says which channel it made");
            let strip = Strip::Channel(ch);
            let now = |host: &dyn Host| host.setup().channel(ch).cloned();
            let Some(fresh) = now(host) else {
                return Ok(());
            };
            if fresh.name != channel.name {
                send(
                    host,
                    Command::Rename {
                        ch,
                        name: channel.name.clone(),
                    },
                )?;
            }
            // An app another channel has taken since stays there.
            let source = match &channel.source {
                Source::Apps(apps) => Source::Apps(
                    apps.iter()
                        .filter(|a| host.setup().channel_of_app(a).is_none())
                        .cloned()
                        .collect(),
                ),
                hardware => hardware.clone(),
            };
            if fresh.source != source {
                send(host, Command::SetSource { ch, source })?;
            }
            // Unlinked while the sends are set, so neither drags the other.
            let sends = [
                (vixmix_model::MixId::Monitor, channel.monitor, fresh.monitor),
                (vixmix_model::MixId::Master, channel.master, fresh.master),
            ];
            if fresh.link && sends.iter().any(|(_, was, is)| was.position != is.position) {
                send(host, Command::SetLink { ch, link: false })?;
            }
            for (mix, was, is) in sends {
                if was.position != is.position {
                    send(
                        host,
                        Command::SetSend {
                            ch,
                            mix,
                            position: was.position,
                        },
                    )?;
                }
                if was.mute != is.mute {
                    send(
                        host,
                        Command::SetMute {
                            ch,
                            mix,
                            mute: was.mute,
                        },
                    )?;
                }
                if channel.chain_to(mix) != fresh.chain_to(mix) {
                    send(
                        host,
                        Command::SetChainRouting {
                            ch,
                            mix,
                            on: channel.chain_to(mix),
                        },
                    )?;
                }
            }
            if now(host).is_some_and(|c| c.link != channel.link) {
                send(
                    host,
                    Command::SetLink {
                        ch,
                        link: channel.link,
                    },
                )?;
            }
            for (at, insert) in channel.chain.iter().enumerate() {
                restore_insert(host, strip, at, insert)?;
            }
        }
    }
    Ok(())
}

/// Applies `cmd` for an Undo: what it changed matters only through the setup.
fn send(host: &mut dyn Host, cmd: Command) -> Result<(), Refused> {
    host.apply(cmd).map(drop)
}

/// Adds `insert`'s plugin to `strip` at `at` (or the end, when the chain is shorter now),
/// bypassed again if it was.
fn restore_insert(
    host: &mut dyn Host,
    strip: Strip,
    at: usize,
    insert: &Insert,
) -> Result<(), Refused> {
    let setup = host.setup();
    let len = match strip {
        Strip::Channel(ch) => setup.channel(ch).map_or(at, |c| c.chain.len()),
        Strip::Mix(mix) => setup.mix(mix).chain.len(),
    };
    let applied = host.apply(Command::AddInsert {
        strip,
        plugin: insert.plugin.clone(),
        at: at.min(len),
    })?;
    if !insert.enabled
        && let Some(&instance) = applied.created.first()
    {
        send(
            host,
            Command::SetInsertEnabled {
                instance,
                enabled: false,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toast_shows_for_its_time_and_its_undo_goes_with_it() {
        let mut toasts = Toasts::default();
        let undo = Undo::Channel {
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
