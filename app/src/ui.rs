//! What each frame draws. Today: the 34-pixel header with the name and the display's
//! numbers, and the banner that says when there is no audio backend. The console itself —
//! strips, meters, faders, the FX wells, the popovers and the browser — is handoff 05's, and
//! starts with the look gate.

use egui::{Align, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Stroke};
use vixmix_host::{BackendState, StatsFrame};

/// The prototype's colours this file uses (`docs/design/prototype/index.html`, `:root`).
pub mod palette {
    use egui::Color32;

    /// `--win`: the window behind everything.
    pub const WIN: Color32 = Color32::from_rgb(0x13, 0x14, 0x16);
    /// `--strip`: a strip's face, and the header.
    pub const STRIP: Color32 = Color32::from_rgb(0x21, 0x22, 0x26);
    /// `--line`: hairlines.
    pub const LINE: Color32 = Color32::from_rgb(0x2a, 0x2b, 0x2f);
    /// `--text`.
    pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xee, 0xf1);
    /// `--dim`.
    pub const DIM: Color32 = Color32::from_rgb(0x9a, 0x9e, 0xa7);
    /// `--mute`: the banner's edge, as a muted button's red.
    pub const MUTE: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);
}

/// The header's height in points (the prototype's `header 34 px`).
pub const HEADER_HEIGHT: f32 = 34.0;

/// What one frame needs from the host.
pub struct View<'a> {
    pub backend: &'a BackendState,
    pub stats: StatsFrame,
    pub channels: usize,
}

/// Draws the whole window into `root`.
pub fn draw(root: &mut egui::Ui, view: &View<'_>) {
    egui::Panel::top("vix-mix header")
        .exact_size(HEADER_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::NONE
                .fill(palette::STRIP)
                .stroke(Stroke::new(1.0, palette::LINE))
                .inner_margin(Margin::symmetric(10, 0)),
        )
        .show(root, |ui| header(ui, view));
    if let Some(reason) = view.backend.banner() {
        egui::Panel::top("vix-mix banner")
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(palette::STRIP)
                    .stroke(Stroke::new(1.0, palette::MUTE))
                    .corner_radius(CornerRadius::same(2))
                    .inner_margin(Margin::symmetric(10, 6))
                    .outer_margin(Margin::symmetric(8, 6)),
            )
            .show(root, |ui| {
                ui.label(RichText::new(reason).color(palette::TEXT));
            });
    }
    egui::CentralPanel::no_frame().show(root, |ui| {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!(
                    "{} channel{} \u{b7} the console arrives with handoff 05",
                    view.channels,
                    if view.channels == 1 { "" } else { "s" }
                ))
                .color(palette::DIM),
            );
        });
    });
}

fn header(ui: &mut egui::Ui, view: &View<'_>) {
    ui.horizontal_centered(|ui| {
        ui.label(
            RichText::new("Vix Mix")
                .font(FontId::proportional(14.0))
                .strong()
                .color(palette::TEXT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let stats = view.stats;
            let lcd = if stats.rate == 0 {
                "\u{2014}".to_string()
            } else {
                format!(
                    "{:.1} kHz \u{b7} {} \u{b7} {:.1} ms \u{b7} DSP {:.0}%",
                    stats.rate as f32 / 1000.0,
                    stats.quantum,
                    stats.quantum_ms(),
                    stats.dsp_load * 100.0
                )
            };
            ui.label(
                RichText::new(lcd)
                    .font(FontId::monospace(11.0))
                    .color(palette::DIM),
            );
        });
    });
}

/// The visuals every frame uses: egui's dark theme on the prototype's window colour.
pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = palette::WIN;
    visuals.window_fill = palette::STRIP;
    visuals.override_text_color = Some(palette::TEXT);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
}

/// The window's clear colour.
pub fn clear_colour() -> Color32 {
    palette::WIN
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::Shape;

    fn texts(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Text(text) => out.push(text.galley.text().to_string()),
            Shape::Vec(shapes) => shapes.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }

    fn drawn(backend: BackendState) -> Vec<String> {
        let ctx = egui::Context::default();
        install(&ctx);
        let view = View {
            backend: &backend,
            stats: StatsFrame {
                rate: 48_000,
                quantum: 256,
                dsp_load: 0.07,
                xruns: 0,
            },
            channels: 6,
        };
        let mut out = Vec::new();
        // Two passes: egui lays out panels from the first frame's sizes.
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |root| draw(root, &view));
            // A test has no renderer to hand the font atlas to; egui asserts it was handled.
            output.textures_delta.clear();
            out.clear();
            for clipped in &output.shapes {
                texts(&clipped.shape, &mut out);
            }
        }
        out
    }

    #[test]
    fn the_header_names_the_app_and_shows_the_display() {
        let texts = drawn(BackendState::Simulated);
        assert!(texts.iter().any(|t| t == "Vix Mix"), "{texts:?}");
        assert!(
            texts
                .iter()
                .any(|t| t.contains("48.0 kHz") && t.contains("5.3 ms")),
            "{texts:?}"
        );
    }

    #[test]
    fn the_banner_shows_only_when_there_is_no_backend() {
        let absent = drawn(BackendState::Absent {
            reason: BackendState::NO_BACKEND.to_string(),
        });
        assert!(
            absent.iter().any(|t| t == BackendState::NO_BACKEND),
            "{absent:?}"
        );
        let running = drawn(BackendState::Running {
            rate: 48_000,
            quantum: 256,
        });
        assert!(!running.iter().any(|t| t.contains("No audio backend")));
    }
}
