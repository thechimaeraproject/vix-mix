//! What each frame draws and senses: the 34-point header (the menu button, the title, the
//! display with the rate, the quantum, its milliseconds and the DSP load with its peak), the
//! banner when the audio needs the user (the host's `BackendState::banner`: no backend, the
//! mixer idle with what would drive it, the daemon gone), the console under them filling the
//! rest of the window ([`console::Geometry`]), then the open popover and the toast line over
//! everything.
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

use crate::console::{self, Anchor, ConsoleView, Cx, Geometry, Layout};
use crate::icons;
use crate::popover::{self, Outcome, PopCx, Popover};
use crate::shell::{Intent, Shell};
use crate::strip::{self, Spacing};
use crate::theme::{self, colour, font, size};
use crate::widgets;

/// The window's title in the header.
pub const TITLE: &str = "Vix Mix";

/// The window's content at its natural size: the console's width, the header and the
/// console's height (no banner). The window opens at it, and the look gate is drawn at it.
pub fn natural_size(setup: &Setup) -> Vec2 {
    let console = console::natural_size(setup.channels.len());
    vec2(console.x, size::HEADER + console.y)
}

/// The window's content at its least: the header over the console at its least (the first
/// frame adds the header's own width and the banner, [`Shell::least`]).
pub fn least_size(setup: &Setup) -> Vec2 {
    let console = console::least_size(setup.channels.len());
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
        let area = ui.max_rect();
        let channels = host.setup().channels.len();
        let geometry = Geometry::new(area, channels);
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
        started = console::draw(ui, &geometry, &view, &mut cx);
        // The window needs the console at its least under the header (and the banner), and
        // the header's own width, whichever is wider.
        let least = console::least_size(channels);
        shell.least = vec2(least.x.max(header_width), area.top() + least.y);
        shell.console = geometry.rect;
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
            // What it hung from is gone: its channel was removed (the popover has nothing to
            // send), or scrolled out of view (a name being typed is kept).
            None => Outcome::Close,
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

/// The DSP cell's reading of the host's stats: the number, the bar's fill, where its peak
/// tick sits, and whether a cycle ran over its budget.
#[derive(Clone, Debug, PartialEq)]
pub struct DspCell {
    /// The average as a percentage ([`dsp_percent`]), or a dash before the host reports a
    /// rate.
    pub text: String,
    /// The bar's fill, the average, 0 to 1.
    pub fill: f32,
    /// The peak tick's position on the bar, 0 to 1: the held peak, pinned at the bar's end
    /// above the budget. None with no rate, or a peak too small to draw apart from the start.
    pub peak: Option<f32>,
    /// The held peak is over the budget: the number takes the mute colour.
    pub over: bool,
}

impl DspCell {
    /// What the cell shows for `stats`: a dash, no fill and no tick until the host reports a
    /// rate.
    pub fn of(stats: StatsFrame) -> DspCell {
        if stats.rate == 0 {
            return DspCell {
                text: "\u{2014}".to_string(),
                fill: 0.0,
                peak: None,
                over: false,
            };
        }
        let fill = if stats.dsp_load.is_finite() {
            stats.dsp_load.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let peak = stats.dsp_peak.is_finite().then_some(stats.dsp_peak);
        DspCell {
            text: dsp_percent(stats.dsp_load),
            fill,
            peak: peak
                .filter(|p| *p > size::DSP_PEAK_SHOWN_ABOVE)
                .map(|p| p.min(1.0)),
            over: peak.is_some_and(|p| p > 1.0),
        }
    }
}

/// The DSP load as the header writes it: a whole percent from 1 % up (above 100 % too),
/// exactly zero as `0%`, and tenths below 1 % (`0.4%`), so a small load never reads as none.
pub fn dsp_percent(load: f32) -> String {
    let percent = (load * 100.0).max(0.0);
    if percent == 0.0 {
        return "0%".to_string();
    }
    let tenths = (percent * 10.0).round();
    if tenths < 10.0 {
        format!("{:.1}%", tenths.max(1.0) / 10.0)
    } else {
        format!("{percent:.0}%")
    }
}

/// One piece of a display cell.
enum Item {
    Text(Arc<Galley>),
    /// The DSP bar: its fill and its peak tick ([`DspCell`]).
    Bar {
        fill: f32,
        peak: Option<f32>,
    },
}

impl Item {
    fn width(&self) -> f32 {
        match self {
            Item::Text(galley) => galley.size().x,
            Item::Bar { .. } => size::DSP_BAR[0],
        }
    }
}

/// The DSP bar's peak tick at `peak` (0 to 1): a hairline across the bar whose left edge sits
/// at the peak, as a meter's peak-hold line sits on its level, kept inside the bar's end.
fn peak_tick(bar: Rect, peak: f32) -> Rect {
    let x = (bar.left() + bar.width() * peak).min(bar.right() - size::PEAK_LINE);
    Rect::from_x_y_ranges(x..=x + size::PEAK_LINE, bar.y_range())
}

fn cell_width(items: &[Item]) -> f32 {
    2.0 * size::DISPLAY_PAD_X
        + items.iter().map(Item::width).sum::<f32>()
        + size::DISPLAY_GAP * items.len().saturating_sub(1) as f32
}

/// The display, centred on `centre`: rate, quantum and milliseconds, then DSP with its bar
/// (the average, and a tick at the held peak) and the average's percentage, in the mute
/// colour while the peak is over the budget, in cells split by hairlines on black. Returns
/// its width.
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
    let dsp = DspCell::of(stats);
    let ink = if dsp.over { colour::MUTE } else { colour::TEXT };
    cells.push(vec![
        Item::Text(run(painter, "DSP", false)),
        Item::Bar {
            fill: dsp.fill,
            peak: dsp.peak,
        },
        Item::Text(run_in(painter, &dsp.text, true, ink)),
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
                Item::Bar { fill, peak } => {
                    let [bar_w, bar_h] = size::DSP_BAR;
                    let bar = Rect::from_min_size(
                        pos2(item_x, centre.y - bar_h / 2.0),
                        vec2(bar_w, bar_h),
                    );
                    painter.rect_filled(bar, CornerRadius::ZERO, colour::CTL);
                    let mut filled = bar;
                    filled.set_width(bar_w * fill);
                    painter.rect_filled(filled, CornerRadius::ZERO, colour::LIVE);
                    if let Some(peak) = peak {
                        painter.rect_filled(
                            peak_tick(bar, peak),
                            CornerRadius::ZERO,
                            colour::PEAK.gamma_multiply(colour::PEAK_OPACITY),
                        );
                    }
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
    let color = if number { colour::TEXT } else { colour::FAINT };
    run_in(painter, text, number, color)
}

/// A display run in `color`: the DSP number takes the mute colour over its budget.
fn run_in(painter: &Painter, text: &str, number: bool, color: Color32) -> Arc<Galley> {
    let font_id = if number {
        font::mono_medium(font::DISPLAY)
    } else {
        font::mono(font::DISPLAY)
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

    /// Every shape a frame of the whole window drew.
    fn frame(backend: BackendState, host: &FakeHost) -> Vec<Shape> {
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
        let mut shapes = Vec::new();
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
            shapes = output.shapes.into_iter().map(|c| c.shape).collect();
        }
        shapes
    }

    /// Every shape the header display alone drew from `stats`, in a headless frame.
    fn display_frame(stats: StatsFrame) -> Vec<Shape> {
        let ctx = egui::Context::default();
        install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, size::HEADER))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |root| {
            display(root.painter(), pos2(300.0, size::HEADER / 2.0), stats);
        });
        output.textures_delta.clear();
        output.shapes.into_iter().map(|c| c.shape).collect()
    }

    fn drawn_in(shapes: &[Shape]) -> Vec<Drawn> {
        let mut out = Vec::new();
        for shape in shapes {
            collect(shape, &mut out);
        }
        out
    }

    fn drawn_with(backend: BackendState, host: &FakeHost) -> Vec<Drawn> {
        drawn_in(&frame(backend, host))
    }

    /// Every filled rectangle drawn, with its fill.
    fn rects(shapes: &[Shape]) -> Vec<(Rect, Color32)> {
        fn walk(shape: &Shape, out: &mut Vec<(Rect, Color32)>) {
            match shape {
                Shape::Rect(rect) => out.push((rect.rect, rect.fill)),
                Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|s| walk(s, &mut out));
        out
    }

    /// The header's DSP bar, its fill and its peak ticks: drawn in the header, the bar the
    /// control face at the bar's size, the fill the live green, a tick the peak-hold colour.
    fn dsp_bar(shapes: &[Shape]) -> (Rect, Option<Rect>, Vec<Rect>) {
        let in_header = rects(shapes)
            .into_iter()
            .filter(|(rect, _)| rect.bottom() <= size::HEADER);
        let (mut bars, mut fills, mut ticks) = (Vec::new(), Vec::new(), Vec::new());
        let tick_ink = colour::PEAK.gamma_multiply(colour::PEAK_OPACITY);
        for (rect, fill) in in_header {
            if fill == colour::CTL && rect.size() == vec2(size::DSP_BAR[0], size::DSP_BAR[1]) {
                bars.push(rect);
            } else if fill == colour::LIVE {
                fills.push(rect);
            } else if fill == tick_ink {
                ticks.push(rect);
            }
        }
        assert_eq!(bars.len(), 1, "one DSP bar: {bars:?}");
        assert!(fills.len() <= 1, "{fills:?}");
        (bars[0], fills.first().copied(), ticks)
    }

    /// The DSP number as drawn: the last number run in the display.
    fn dsp_number(shapes: &[Shape]) -> Drawn {
        drawn_in(shapes)
            .into_iter()
            .rfind(|d| d.text.ends_with('%') || d.text == "\u{2014}")
            .expect("the DSP number")
    }

    /// Two lengths the same to within float rounding of the positions they come from.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn stats(dsp_load: f32, dsp_peak: f32) -> StatsFrame {
        StatsFrame {
            rate: 48_000,
            quantum: 256,
            dsp_load,
            dsp_peak,
            xruns: 0,
        }
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
        let shapes = frame(BackendState::Simulated, &host);
        let drawn = drawn_in(&shapes);
        let texts = texts(&drawn);
        let stats = host.stats();
        let load = dsp_percent(stats.dsp_load);
        for expected in [
            TITLE, "48", "kHz", "256", "samples", "5.3", "ms", "DSP", &load,
        ] {
            assert!(texts.contains(&expected), "no `{expected}` in {texts:?}");
        }
        assert_eq!(dsp_number(&shapes).colour, colour::TEXT);
        // The bar fills to the average and ticks at the peak, three points past it.
        let (bar, fill, ticks) = dsp_bar(&shapes);
        let fill = fill.expect("the fill");
        assert_eq!(fill.left(), bar.left());
        assert!(close(fill.width(), bar.width() * stats.dsp_load));
        assert_eq!(ticks.len(), 1, "{ticks:?}");
        let tick = ticks[0];
        assert!(close(
            tick.left(),
            bar.left() + bar.width() * stats.dsp_peak
        ));
        assert!(close(tick.width(), size::PEAK_LINE));
        assert_eq!(tick.y_range(), bar.y_range());
        assert!(tick.left() > fill.right());
    }

    #[test]
    fn the_dsp_number_shows_tenths_below_one_percent_and_zero_only_for_none() {
        for (load, text) in [
            (0.0, "0%"),
            (0.004, "0.4%"),
            (0.0001, "0.1%"),
            (0.0096, "1%"),
            (0.012, "1%"),
            (0.07, "7%"),
            (0.124, "12%"),
            (1.3, "130%"),
            (-0.5, "0%"),
            (f32::NAN, "0%"),
        ] {
            assert_eq!(dsp_percent(load), text, "{load}");
        }
        // As drawn: a small load reads as itself, not as none.
        let drawn = dsp_number(&display_frame(stats(0.004, 0.01)));
        assert_eq!((drawn.text.as_str(), drawn.colour), ("0.4%", colour::TEXT));
        let drawn = dsp_number(&display_frame(stats(0.0, 0.0)));
        assert_eq!(drawn.text, "0%");
    }

    #[test]
    fn the_dsp_peak_is_a_tick_on_the_bar_and_over_the_budget_it_pins_and_turns_the_number_red() {
        // Within the budget: the fill at the average, the tick at the peak, the number bright.
        let shapes = display_frame(stats(0.25, 0.5));
        let (bar, fill, ticks) = dsp_bar(&shapes);
        assert!(close(fill.unwrap().width(), bar.width() * 0.25));
        assert_eq!(ticks.len(), 1);
        assert!(close(ticks[0].left(), bar.left() + bar.width() * 0.5));
        let number = dsp_number(&shapes);
        assert_eq!((number.text.as_str(), number.colour), ("25%", colour::TEXT));

        // A cycle over its budget: the tick pinned at the bar's end, the number in the mute
        // colour, the meters' red, while the average is still under.
        let shapes = display_frame(stats(0.9, 1.4));
        let (bar, fill, ticks) = dsp_bar(&shapes);
        assert!(close(fill.unwrap().width(), bar.width() * 0.9));
        assert_eq!(ticks.len(), 1);
        assert!(close(ticks[0].right(), bar.right()));
        assert!(ticks[0].left() >= bar.left());
        let number = dsp_number(&shapes);
        assert_eq!((number.text.as_str(), number.colour), ("90%", colour::MUTE));

        // The average over too: the fill whole, the number past 100.
        let shapes = display_frame(stats(1.2, 1.5));
        let (bar, fill, ticks) = dsp_bar(&shapes);
        assert!(close(fill.unwrap().width(), bar.width()));
        assert!(close(ticks[0].right(), bar.right()));
        let number = dsp_number(&shapes);
        assert_eq!(
            (number.text.as_str(), number.colour),
            ("120%", colour::MUTE)
        );

        // A peak too small to stand apart from the bar's start draws no tick.
        let (_, _, ticks) = dsp_bar(&display_frame(stats(0.001, 0.01)));
        assert!(ticks.is_empty(), "{ticks:?}");
    }

    #[test]
    fn with_no_rate_the_dsp_cell_is_a_dash_with_no_tick() {
        let shapes = display_frame(StatsFrame {
            rate: 0,
            dsp_load: 0.3,
            dsp_peak: 0.6,
            ..StatsFrame::default()
        });
        let drawn = drawn_in(&shapes);
        assert_eq!(count(&drawn, "\u{2014}"), 4, "{:?}", texts(&drawn));
        let (_, fill, ticks) = dsp_bar(&shapes);
        assert!(fill.is_none_or(|f| f.width() == 0.0), "{fill:?}");
        assert!(ticks.is_empty(), "{ticks:?}");
        assert_eq!(dsp_number(&shapes).colour, colour::TEXT);
    }

    #[test]
    fn the_display_says_khz_without_a_needless_decimal() {
        let at = |rate| {
            let [(khz, _), _, _] = display_cells(StatsFrame {
                rate,
                quantum: 128,
                dsp_load: 0.0,
                dsp_peak: 0.0,
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
