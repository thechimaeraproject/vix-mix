//! What each frame draws and senses: the 34-point header (the menu button, the title, the
//! display with the rate, the quantum, its milliseconds and the DSP load), the banner when
//! the audio needs the user (the host's `BackendState::banner`: no backend, the mixer idle
//! with what would drive it, the daemon gone), the console under them, top left, at its
//! natural size, then the open popover and the toast line over everything.
//!
//! [`draw`] reads the host and the window's [`Shell`], and returns what the gestures asked
//! ([`Intent`]s); the shell carries them out after the frame, so a frame never changes the
//! setup it is drawing.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, Event, Frame, Galley, Id, Key, Margin, Painter, Pos2, Rect,
    RichText, Stroke, Ui, Vec2, pos2, vec2,
};
use vixmix_host::{Host, StatsFrame};
use vixmix_model::Setup;

use crate::console::{self, Anchor, ConsoleView, Cx, Layout};
use crate::icons;
use crate::popover::{self, Outcome, PopCx, Popover};
use crate::shell::{Intent, Shell};
use crate::strip::{self, Spacing};
use crate::theme::{self, colour, font, size};
use crate::widgets;

/// The window's title in the header.
pub const TITLE: &str = "Vix Mix";

/// The window's content at its natural size: the console's width, the header and the
/// console's height (no banner).
pub fn natural_size(setup: &Setup) -> Vec2 {
    let console = console::size(setup.channels.len());
    vec2(console.x, size::HEADER + console.y)
}

/// The widget a popover hangs from, which takes the focus back when Escape closes it.
pub fn anchor_id(anchor: Anchor) -> Id {
    match anchor {
        Anchor::Head(strip) => strip::head_id(strip),
        Anchor::AddPlugin(strip) => Id::new(("add plugin", strip)),
        Anchor::AddColumn => Id::new("add channel"),
        Anchor::Menu => Id::new("menu"),
    }
}

/// Draws the whole window into `root` from `host` and `shell`, and returns what its
/// gestures asked.
pub fn draw(root: &mut Ui, shell: &mut Shell, host: &dyn Host) -> Vec<Intent> {
    let ctx = root.ctx().clone();
    shell.now = ctx.input(|i| i.time);
    let (tab, pressed) = ctx.input(|i| {
        (
            i.events.iter().any(|e| {
                matches!(
                    e,
                    Event::Key {
                        key: Key::Tab,
                        pressed: true,
                        ..
                    }
                )
            }),
            i.pointer.any_pressed(),
        )
    });
    if tab {
        shell.keyboard = true;
    }
    if pressed {
        shell.keyboard = false;
    }
    let mut out = Vec::new();
    let mut layout = Layout::default();
    let open = shell.popover.as_ref().map(Popover::anchor);
    let mut header_width = 0.0;

    egui::Panel::top("vix-mix header")
        .exact_size(size::HEADER)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::NONE.fill(colour::WIN))
        .show(root, |ui| {
            header_width = header(
                ui,
                host.stats(),
                open,
                shell.keyboard,
                &mut layout,
                &mut out,
            );
        });
    if let Some(reason) = shell.backend.banner() {
        egui::Panel::top("vix-mix banner")
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(colour::STRIP)
                    .stroke(Stroke::new(size::HAIRLINE, colour::MUTE))
                    .corner_radius(CornerRadius::same(size::CORNER))
                    .inner_margin(Margin::symmetric(size::BANNER_PAD[0], size::BANNER_PAD[1]))
                    .outer_margin(Margin::symmetric(
                        size::BANNER_MARGIN[0],
                        size::BANNER_MARGIN[1],
                    )),
            )
            .show(root, |ui| {
                ui.label(RichText::new(reason).color(colour::TEXT));
            });
    }
    let renaming = match &shell.popover {
        Some(Popover::Source(panel)) => Some((panel.ch, panel.name.as_str())),
        _ => None,
    };
    let mut started = false;
    egui::CentralPanel::no_frame().show(root, |ui| {
        let origin = ui.max_rect().min;
        let view = ConsoleView {
            setup: host.setup(),
            meters: &shell.meters,
            apps: host.apps(),
            renaming,
        };
        let mut cx = Cx {
            host,
            out: &mut out,
            gestures: &mut shell.gestures,
            layout: &mut layout,
            keyboard: shell.keyboard,
            open,
        };
        let (rect, began) = console::draw(ui, origin, &view, &mut cx);
        // The window needs the console under the header (and the banner), and the header's
        // own width, whichever is wider.
        shell.content = Rect::from_min_max(
            Pos2::ZERO,
            pos2(rect.right().max(header_width), rect.bottom()),
        );
        started = began;
    });

    // A drag closes the popover, as does a press outside it or Escape.
    if started && let Some(popover) = shell.popover.take() {
        out.extend(popover.closing(host.setup()).map(Intent::Apply));
        ctx.request_repaint();
    }
    let screen = ctx.content_rect();
    if let Some(popover) = shell.popover.as_mut() {
        let anchor = layout.anchor(popover.anchor());
        let outcome = match anchor {
            // What it hung from is gone (its channel was removed).
            None => Outcome::Drop,
            Some(anchor) => {
                let mut pop_cx = PopCx {
                    host,
                    out: &mut out,
                    keyboard: shell.keyboard,
                };
                let (outcome, rect) = popover::show(&ctx, popover, anchor, screen, &mut pop_cx);
                let outside = ctx.input(|i| {
                    i.pointer.any_pressed()
                        && i.pointer
                            .interact_pos()
                            .is_some_and(|p| !rect.contains(p) && !anchor.contains(p))
                });
                if outcome == Outcome::Stay && outside {
                    Outcome::Close
                } else {
                    outcome
                }
            }
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        if outcome != Outcome::Stay {
            // This frame drew it; the next must not.
            ctx.request_repaint();
        }
        match outcome {
            Outcome::Stay => {}
            Outcome::Drop => shell.popover = None,
            Outcome::Close | Outcome::Open(_) => {
                if let Some(old) = shell.popover.take() {
                    out.extend(old.closing(host.setup()).map(Intent::Apply));
                    if escape {
                        ctx.memory_mut(|m| m.request_focus(anchor_id(old.anchor())));
                    }
                }
                if let Outcome::Open(next) = outcome {
                    shell.popover = Some(next);
                }
            }
        }
    }
    shell.toasts.draw(&ctx, screen, shell.now, &mut out);
    out
}

/// The header: the menu button and the title on the left, the display centred, the bottom
/// edge in black. Returns the width it needs so the display clears the title.
fn header(
    ui: &Ui,
    stats: StatsFrame,
    open: Option<Anchor>,
    keyboard: bool,
    layout: &mut Layout,
    out: &mut Vec<Intent>,
) -> f32 {
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, colour::WIN);
    painter.hline(
        rect.x_range(),
        rect.bottom() - size::HAIRLINE_INSET,
        Stroke::new(size::HAIRLINE, colour::EDGE),
    );
    let y = rect.top() + (size::HEADER - size::HAIRLINE) / 2.0;
    let [button_w, button_h] = size::MENU_BUTTON;
    let button = Rect::from_center_size(
        pos2(rect.left() + size::HEADER_PAD_X + button_w / 2.0, y),
        vec2(button_w, button_h),
    );
    layout.anchors.push((Anchor::Menu, button));
    let response = widgets::press(ui, button, anchor_id(Anchor::Menu)).on_hover_text("Menu");
    if response.clicked() {
        out.push(Intent::Toggle(Anchor::Menu));
    }
    let ink = if response.hovered() || open == Some(Anchor::Menu) {
        painter.rect_filled(button, size::CORNER, colour::CTL);
        colour::TEXT
    } else {
        colour::DIM
    };
    let menu = Rect::from_center_size(button.center(), vec2(size::MENU_ICON, size::MENU_ICON));
    icons::paint(painter, &icons::MENU, menu, ink);
    if keyboard && response.has_focus() {
        widgets::ring(painter, button, size::FOCUS_OFFSET);
    }
    let title = strip::galley(
        painter,
        TITLE,
        font::sans_semibold(font::TITLE),
        colour::TEXT,
        Spacing::None,
    );
    let title_x = button.right() + size::MENU_GAP;
    let title_right = title_x + title.size().x;
    strip::place(painter, title, Align2::LEFT_CENTER, pos2(title_x, y));
    let display_width = display(painter, pos2(rect.center().x, y), stats);
    2.0 * (title_right - rect.left() + size::HEADER_PAD_X) + display_width
}

/// The display's first three cells: each a number and what it counts. A dash until the
/// backend reports a rate.
pub fn display_cells(stats: StatsFrame) -> [(String, &'static str); 3] {
    if stats.rate == 0 {
        let none = || "\u{2014}".to_string();
        return [(none(), "kHz"), (none(), "samples"), (none(), "ms")];
    }
    let khz = if stats.rate.is_multiple_of(1000) {
        format!("{}", stats.rate / 1000)
    } else {
        format!("{:.1}", stats.rate as f32 / 1000.0)
    };
    [
        (khz, "kHz"),
        (stats.quantum.to_string(), "samples"),
        (format!("{:.1}", stats.quantum_ms()), "ms"),
    ]
}

/// One piece of a display cell.
enum Item {
    Text(Arc<Galley>),
    Bar(f32),
}

impl Item {
    fn width(&self) -> f32 {
        match self {
            Item::Text(galley) => galley.size().x,
            Item::Bar(_) => size::DSP_BAR[0],
        }
    }
}

fn cell_width(items: &[Item]) -> f32 {
    2.0 * size::DISPLAY_PAD_X
        + items.iter().map(Item::width).sum::<f32>()
        + size::DISPLAY_GAP * items.len().saturating_sub(1) as f32
}

/// The display, centred on `centre`: rate, quantum and milliseconds, then DSP with its bar
/// and percentage, in cells split by hairlines on black. Returns its width.
fn display(painter: &Painter, centre: Pos2, stats: StatsFrame) -> f32 {
    let mut cells: Vec<Vec<Item>> = display_cells(stats)
        .into_iter()
        .map(|(value, unit)| {
            vec![
                Item::Text(run(painter, &value, true)),
                Item::Text(run(painter, unit, false)),
            ]
        })
        .collect();
    let load = stats.dsp_load.clamp(0.0, 1.0);
    let percent = if stats.rate == 0 {
        "\u{2014}".to_string()
    } else {
        format!("{:.0}%", load * 100.0)
    };
    cells.push(vec![
        Item::Text(run(painter, "DSP", false)),
        Item::Bar(load),
        Item::Text(run(painter, &percent, true)),
    ]);
    let hairlines = cells.len().saturating_sub(1) as f32;
    let total = cells.iter().map(|c| cell_width(c)).sum::<f32>() + hairlines * size::HAIRLINE;
    let rect = Rect::from_center_size(centre, vec2(total, size::DISPLAY_HEIGHT));
    painter.rect_filled(rect, size::CORNER, colour::BLACK);
    let mut x = rect.left();
    let count = cells.len();
    for (index, items) in cells.into_iter().enumerate() {
        let width = cell_width(&items);
        let mut item_x = x + size::DISPLAY_PAD_X;
        for item in items {
            let item_width = item.width();
            match item {
                Item::Text(galley) => {
                    strip::place(painter, galley, Align2::LEFT_CENTER, pos2(item_x, centre.y));
                }
                Item::Bar(fill) => {
                    let [bar_w, bar_h] = size::DSP_BAR;
                    let bar = Rect::from_min_size(
                        pos2(item_x, centre.y - bar_h / 2.0),
                        vec2(bar_w, bar_h),
                    );
                    painter.rect_filled(bar, CornerRadius::ZERO, colour::CTL);
                    let mut filled = bar;
                    filled.set_width(bar_w * fill);
                    painter.rect_filled(filled, CornerRadius::ZERO, colour::LIVE);
                }
            }
            item_x += item_width + size::DISPLAY_GAP;
        }
        x += width;
        if index + 1 < count {
            painter.vline(
                x + size::HAIRLINE_INSET,
                rect.y_range(),
                Stroke::new(size::HAIRLINE, colour::LINE),
            );
            x += size::HAIRLINE;
        }
    }
    total
}

/// A display run: a number (mono 500, bright) or a label (mono 400, faint).
fn run(painter: &Painter, text: &str, number: bool) -> Arc<Galley> {
    let (font_id, color) = if number {
        (font::mono_medium(font::DISPLAY), colour::TEXT)
    } else {
        (font::mono(font::DISPLAY), colour::FAINT)
    };
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id,
            color,
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping::no_max_width();
    painter.layout_job(job)
}

/// The fonts and visuals every frame uses ([`theme::install`]).
pub fn install(ctx: &egui::Context) {
    theme::install(ctx);
}

/// The window's clear colour.
pub fn clear_colour() -> Color32 {
    colour::WIN
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::Shape;
    use vixmix_host::{BackendState, FakeHost, Host};

    /// A drawn text: what it says, its colour, whether it is struck through.
    #[derive(Debug)]
    struct Drawn {
        text: String,
        colour: Color32,
        struck: bool,
    }

    fn collect(shape: &Shape, out: &mut Vec<Drawn>) {
        match shape {
            Shape::Text(text) => {
                let format = text
                    .galley
                    .job
                    .sections
                    .first()
                    .map(|s| s.format.clone())
                    .unwrap_or_default();
                out.push(Drawn {
                    // What was drawn, glyph by glyph: a truncated name ends in its ellipsis.
                    text: text
                        .galley
                        .rows
                        .iter()
                        .flat_map(|row| row.row.glyphs.iter().map(|g| g.chr))
                        .collect(),
                    colour: format.color,
                    struck: format.strikethrough.width > 0.0,
                });
            }
            Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }

    fn drawn_with(backend: BackendState, host: &FakeHost) -> Vec<Drawn> {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut shell = Shell {
            backend,
            ..Shell::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, natural_size(host.setup()))),
            ..Default::default()
        };
        let mut out = Vec::new();
        // Two passes: egui lays out panels from the first frame's sizes.
        for _ in 0..2 {
            let mut output = ctx.run_ui(input.clone(), |root| {
                let asked = draw(root, &mut shell, host);
                assert!(
                    asked.is_empty(),
                    "nothing pressed, nothing asked: {asked:?}"
                );
            });
            // A test has no renderer to hand the font atlas to; egui asserts it was handled.
            output.textures_delta.clear();
            out.clear();
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut out);
            }
        }
        out
    }

    fn drawn(backend: BackendState) -> Vec<Drawn> {
        drawn_with(backend, &FakeHost::new())
    }

    fn texts(drawn: &[Drawn]) -> Vec<&str> {
        drawn.iter().map(|d| d.text.as_str()).collect()
    }

    fn count(drawn: &[Drawn], text: &str) -> usize {
        drawn.iter().filter(|d| d.text == text).count()
    }

    #[test]
    fn the_header_names_the_app_and_shows_the_display() {
        let host = FakeHost::new();
        let drawn = drawn_with(BackendState::Simulated, &host);
        let texts = texts(&drawn);
        let load = format!("{:.0}%", host.stats().dsp_load * 100.0);
        for expected in [
            TITLE, "48", "kHz", "256", "samples", "5.3", "ms", "DSP", &load,
        ] {
            assert!(texts.contains(&expected), "no `{expected}` in {texts:?}");
        }
    }

    #[test]
    fn the_display_says_khz_without_a_needless_decimal() {
        let at = |rate| {
            let [(khz, _), _, _] = display_cells(StatsFrame {
                rate,
                quantum: 128,
                dsp_load: 0.0,
                xruns: 0,
            });
            khz
        };
        assert_eq!(at(48_000), "48");
        assert_eq!(at(44_100), "44.1");
        assert_eq!(at(0), "\u{2014}", "no stats yet reads as a dash");
    }

    #[test]
    fn the_banner_shows_for_no_backend_and_not_while_running() {
        let absent = drawn(BackendState::Absent {
            reason: BackendState::NO_BACKEND.to_string(),
        });
        assert_eq!(count(&absent, BackendState::NO_BACKEND), 1);
        let running = drawn(BackendState::Running {
            rate: 48_000,
            quantum: 256,
        });
        assert!(
            !texts(&running)
                .iter()
                .any(|t| t.contains("No audio backend"))
        );
    }

    #[test]
    fn the_console_draws_the_examples_strips_as_the_prototype_shows_them() {
        let drawn = drawn(BackendState::Simulated);
        let count = |text: &str| count(&drawn, text);
        for name in [
            "Mic",
            "Game",
            "Music",
            "Browser",
            "Voice chat",
            "Synth",
            "Monitor",
            "Master",
        ] {
            assert_eq!(count(name), 1, "{name} in {:?}", texts(&drawn));
        }
        // Every strip has a well with "Add plugin"; the channels have MON and MST.
        assert_eq!(count("Add plugin"), 8);
        assert_eq!(count("FX"), 8);
        assert_eq!((count("MON"), count("MST")), (6, 6));
        assert_eq!(count("ADD CHANNEL"), 1);
        // The inserts; a long name cut with an ellipsis, as CSS cuts it.
        for plugin in ["Pro-C 2", "OTT", "TDR Nova", "Pro-Q 3", "Pro-L 2"] {
            assert!(count(plugin) >= 1, "{plugin} in {:?}", texts(&drawn));
        }
        assert!(
            drawn
                .iter()
                .any(|d| d.text.starts_with("Noise Supp") && d.text.ends_with('\u{2026}')),
            "{:?}",
            texts(&drawn)
        );
        // The readouts, through the taper, as the prototype's screenshot reads them.
        for readout in [
            "-3.4", "-4.3", "-8.6", "-6.9", "-11.1", "-5.1", "-10.3", "-2.6", "-9.4",
        ] {
            assert!(count(readout) >= 1, "{readout} in {:?}", texts(&drawn));
        }
        assert_eq!(count("0.0"), 3, "Mic's Master send and both mixes at unity");
        // The scale beside every strip's faders.
        assert_eq!(count("+6"), 8);
        assert_eq!(count("24"), 8);
    }

    #[test]
    fn a_muted_send_reads_struck_through_and_a_bypassed_insert_faint() {
        let drawn = drawn(BackendState::Simulated);
        // Music's Master send is the example's one muted send.
        let music_master = drawn.iter().find(|d| d.text == "-11.1").unwrap();
        assert!(music_master.struck);
        assert_eq!(music_master.colour, colour::FAINT);
        let mic_monitor = drawn.iter().find(|d| d.text == "-3.4").unwrap();
        assert!(!mic_monitor.struck);
        assert_eq!(mic_monitor.colour, colour::DIM);
        let ott = drawn.iter().find(|d| d.text == "OTT").unwrap();
        assert_eq!(ott.colour, colour::FAINT, "Music's OTT is bypassed");
        let nova = drawn.iter().find(|d| d.text == "TDR Nova").unwrap();
        assert_eq!(nova.colour, colour::TEXT);
    }

    #[test]
    fn mon_and_mst_light_when_the_chain_feeds_that_mix() {
        let drawn = drawn(BackendState::Simulated);
        let lit = |label: &str| {
            drawn
                .iter()
                .filter(|d| d.text == label && d.colour == colour::ON_ACCENT)
                .count()
        };
        // Voice chat's chain does not feed Monitor; every other toggle is on.
        assert_eq!(lit("MON"), 5);
        assert_eq!(lit("MST"), 6);
    }

    #[test]
    fn previewing_master_tags_monitor() {
        let mut host = FakeHost::new();
        let before = drawn_with(BackendState::Simulated, &host);
        assert_eq!(count(&before, "MST"), 6, "the channels' toggles only");
        let applied = host.apply(vixmix_model::Command::SetPreview(true)).unwrap();
        assert!(applied.changed());
        let after = drawn_with(BackendState::Simulated, &host);
        assert_eq!(count(&after, "MST"), 7, "and the tag on Monitor");
    }
}
