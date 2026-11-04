//! The console's look as constants: prototype v5's `:root` colours, the meter gradient, the
//! corners, the strip's sizes and rows, the fonts and the size each text is set in
//! (`docs/design/prototype/index.html`, its CSS). Every drawing names these; none spells a
//! colour or a size of its own, so a change to the look is a change here.
//!
//! [`install`] puts the fonts and the visuals into an egui context once, before the first
//! frame. The fonts are IBM Plex Sans Condensed (400, 500, 600) and IBM Plex Mono (400, 500),
//! embedded from `app/assets/fonts/` under the SIL Open Font License (`OFL.txt` beside them).
//! egui has no font weights, so each weight is a family of its own ([`font`]).

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId};

/// The prototype's colours, by role.
pub mod colour {
    use egui::Color32;

    /// `--win`: the window behind everything, the header, the add-channel column.
    pub const WIN: Color32 = Color32::from_rgb(0x13, 0x14, 0x16);
    /// `--strip`: a channel strip's face.
    pub const STRIP: Color32 = Color32::from_rgb(0x21, 0x22, 0x26);
    /// `.strip.mix`: a mix strip's face, a shade lighter than a channel's.
    pub const MIX: Color32 = Color32::from_rgb(0x26, 0x27, 0x2c);
    /// `--well`: the FX well a strip's inserts sit in.
    pub const WELL: Color32 = Color32::from_rgb(0x18, 0x19, 0x1b);
    /// `--ctl`: a control's face (a mute button, an off toggle, a hovered row).
    pub const CTL: Color32 = Color32::from_rgb(0x2c, 0x2d, 0x32);
    /// `--ctl-hi`: a control under the pointer, and the scale's tick marks.
    pub const CTL_HI: Color32 = Color32::from_rgb(0x38, 0x3a, 0x40);
    /// `--line`: hairlines (the FX header's rule, the display's separators).
    pub const LINE: Color32 = Color32::from_rgb(0x2a, 0x2b, 0x2f);
    /// `--black`: behind a meter's level, the header display.
    pub const BLACK: Color32 = Color32::from_rgb(0x0c, 0x0d, 0x0e);
    /// `--text`.
    pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xee, 0xf1);
    /// `--dim`: secondary text, the readouts, icons on controls.
    pub const DIM: Color32 = Color32::from_rgb(0x9a, 0x9e, 0xa7);
    /// `--faint`: labels, the scale, a bypassed insert, a muted fader line.
    pub const FAINT: Color32 = Color32::from_rgb(0x62, 0x66, 0x6e);
    /// `--accent`: the fader line, an enabled insert, a pressed toggle.
    pub const ACCENT: Color32 = Color32::from_rgb(0x5a, 0xa8, 0xff);
    /// `--mute`: a pressed mute button.
    pub const MUTE: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);

    /// `#000`: the gaps between strips, the header's bottom edge.
    pub const EDGE: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
    /// `#06121f`: text and icons on an accent face.
    pub const ON_ACCENT: Color32 = Color32::from_rgb(0x06, 0x12, 0x1f);
    /// `#fff`: the icon on a pressed mute button.
    pub const ON_MUTE: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
    /// `#4cc46d`: the live dot on a strip's header, the DSP bar.
    pub const LIVE: Color32 = Color32::from_rgb(0x4c, 0xc4, 0x6d);
    /// `#eef0f2`: a meter's peak-hold line, drawn at [`PEAK_OPACITY`].
    pub const PEAK: Color32 = Color32::from_rgb(0xee, 0xf0, 0xf2);
    /// The peak-hold line's opacity.
    pub const PEAK_OPACITY: f32 = 0.85;
    /// A muted fader's bars, level and black alike, at this opacity.
    pub const MUTED_BARS_OPACITY: f32 = 0.4;

    /// The meter's gradient, bottom (0) to top (1): `#2c9a55 0%, #4cc46d 48%, #9fcf43 64%,
    /// #e2b83a 76%, #e5484d 84%`, red to the top.
    pub const METER: [(f32, Color32); 6] = [
        (0.0, Color32::from_rgb(0x2c, 0x9a, 0x55)),
        (0.48, Color32::from_rgb(0x4c, 0xc4, 0x6d)),
        (0.64, Color32::from_rgb(0x9f, 0xcf, 0x43)),
        (0.76, Color32::from_rgb(0xe2, 0xb8, 0x3a)),
        (0.84, Color32::from_rgb(0xe5, 0x48, 0x4d)),
        (1.0, Color32::from_rgb(0xe5, 0x48, 0x4d)),
    ];
}

/// The prototype's sizes, in points (a CSS pixel is a point).
pub mod size {
    /// Every control's corner: flat, 2 px. Popovers alone have a shadow (handoff 05, step 4).
    pub const CORNER: u8 = 2;

    /// The header bar, its bottom edge included.
    pub const HEADER: f32 = 34.0;
    /// The header's side padding.
    pub const HEADER_PAD_X: f32 = 6.0;
    /// The menu button left of the title, and the gap after it.
    pub const MENU_BUTTON: [f32; 2] = [26.0, 24.0];
    pub const MENU_GAP: f32 = 4.0;
    /// The header display: its height, each cell's side padding, the gap inside a cell.
    pub const DISPLAY_HEIGHT: f32 = 22.0;
    pub const DISPLAY_PAD_X: f32 = 10.0;
    pub const DISPLAY_GAP: f32 = 5.0;
    /// The DSP bar inside the display.
    pub const DSP_BAR: [f32; 2] = [36.0, 4.0];

    /// The console's padding and the gap between strips (the black between them).
    pub const CONSOLE_PAD: f32 = 2.0;
    pub const CONSOLE_GAP: f32 = 2.0;
    /// The extra space between the last channel and the mixes.
    pub const MIXES_MARGIN: f32 = 6.0;
    /// The add-channel column right of the channels.
    pub const ADD_COLUMN: f32 = 26.0;
    /// The gap between the add-channel column's plus and its label.
    pub const ADD_COLUMN_GAP: f32 = 8.0;

    /// A channel strip's width, and a mix strip's.
    pub const STRIP: f32 = 104.0;
    pub const MIX: f32 = 112.0;
    /// The strip's rows top to bottom: header, FX well, faders, readouts, buttons.
    pub const ROWS: [f32; 5] = [22.0, 151.0, 210.0, 12.0, 20.0];
    /// The gap between rows.
    pub const ROW_GAP: f32 = 5.0;
    /// The strip's padding: 7 top, 6 at the sides, 8 at the bottom.
    pub const PAD_TOP: f32 = 7.0;
    pub const PAD_X: f32 = 6.0;
    pub const PAD_BOTTOM: f32 = 8.0;
    /// A strip's height: the padding, the rows and the gaps between them (450).
    pub const STRIP_HEIGHT: f32 =
        PAD_TOP + ROWS[0] + ROWS[1] + ROWS[2] + ROWS[3] + ROWS[4] + 4.0 * ROW_GAP + PAD_BOTTOM;

    /// The strip header: side padding, the gap between its items, the icon, the live dot.
    pub const HEAD_PAD_X: f32 = 4.0;
    pub const HEAD_GAP: f32 = 5.0;
    pub const HEAD_ICON: f32 = 12.0;
    pub const DOT: f32 = 5.0;
    /// The MST tag on Monitor's header while Master is previewed: side padding, height.
    pub const TAG_PAD_X: f32 = 4.0;
    pub const TAG_HEIGHT: f32 = 13.0;

    /// The FX well's header (its rule included) and its padding, left and right.
    pub const FX_HEAD: f32 = 19.0;
    pub const FX_HEAD_PAD_LEFT: f32 = 6.0;
    pub const FX_HEAD_PAD_RIGHT: f32 = 3.0;
    /// MON and MST: height, side padding, the gap between them.
    pub const TOGGLE_HEIGHT: f32 = 14.0;
    pub const TOGGLE_PAD_X: f32 = 3.0;
    pub const TOGGLE_GAP: f32 = 2.0;
    /// The slot list's padding, a slot's height, its side padding and the gap inside it.
    pub const SLOTS_PAD: f32 = 2.0;
    pub const SLOT: f32 = 20.0;
    pub const SLOT_PAD_X: f32 = 4.0;
    pub const SLOT_GAP: f32 = 5.0;
    /// The bypass square, and its corner.
    pub const BYPASS: f32 = 8.0;
    pub const BYPASS_CORNER: u8 = 1;
    /// "+ Add plugin": the plus and the gap after it.
    pub const ADD_ICON: f32 = 9.0;
    pub const ADD_GAP: f32 = 6.0;

    /// A channel's fader column, a mix's (`.vf.big`), and the mix's scale beside it.
    pub const FADER: f32 = 20.0;
    pub const FADER_BIG: f32 = 28.0;
    pub const MIX_SCALE: f32 = 24.0;
    /// The gap between a mix's fader and its scale.
    pub const MIX_FADER_GAP: f32 = 4.0;
    /// A meter bar's width (a mix's), and the gap between left and right.
    pub const BAR: f32 = 5.0;
    pub const BAR_BIG: f32 = 8.0;
    pub const BAR_GAP: f32 = 2.0;
    /// The bars stop this far short of the fader's top and bottom; positions 0 and 1 sit
    /// there.
    pub const FADER_INSET: f32 = 6.0;
    /// The fader line's thickness.
    pub const FADER_LINE: f32 = 2.0;
    /// The fader triangle: its depth and its height.
    pub const TRIANGLE: [f32; 2] = [6.0, 8.0];
    /// A scale tick's length.
    pub const SCALE_TICK: f32 = 3.0;
    /// The peak-hold line shows above this position.
    pub const PEAK_SHOWN_ABOVE: f32 = 0.02;

    /// The mute and ear buttons, and the link button.
    pub const MUTE_BUTTON: [f32; 2] = [24.0, 20.0];
    pub const LINK_BUTTON: [f32; 2] = [20.0, 18.0];
    /// The gap between a mix's mute and ear buttons.
    pub const MIX_BUTTON_GAP: f32 = 4.0;
    /// The icons on them.
    pub const BUTTON_ICON: f32 = 13.0;
    pub const LINK_ICON: f32 = 12.0;
    /// The menu and add-channel icons.
    pub const MENU_ICON: f32 = 14.0;
    pub const ADD_COLUMN_ICON: f32 = 13.0;
}

/// The fonts: a family per face, and the size each text is set in.
pub mod font {
    use super::{FontFamily, FontId};

    /// The families [`install`](super::install) defines, one per embedded face. The 400 faces
    /// are also egui's `Proportional` and `Monospace`. Sans 500 is embedded as the handoff
    /// lists it, though prototype v5 sets nothing in it (its 500s are all mono).
    pub const SANS: &str = "plex-sans-condensed-400";
    pub const SANS_MEDIUM: &str = "plex-sans-condensed-500";
    pub const SANS_SEMIBOLD: &str = "plex-sans-condensed-600";
    pub const MONO: &str = "plex-mono-400";
    pub const MONO_MEDIUM: &str = "plex-mono-500";

    /// The title in the header (600).
    pub const TITLE: f32 = 13.0;
    /// The header display (400, its numbers 500).
    pub const DISPLAY: f32 = 11.0;
    /// A strip's name (600).
    pub const STRIP_NAME: f32 = 12.5;
    /// "FX" over the well (600, spaced).
    pub const FX_HEAD: f32 = 10.0;
    /// MON and MST (mono 500, spaced), and the MST tag on Monitor while previewing.
    pub const TOGGLE: f32 = 9.0;
    /// An insert's name, and "Add plugin".
    pub const SLOT: f32 = 11.5;
    /// The scale beside the faders (mono).
    pub const SCALE: f32 = 9.0;
    /// The readouts under the faders (mono 500).
    pub const READOUT: f32 = 10.0;
    /// "ADD CHANNEL" (600, spaced).
    pub const ADD_COLUMN: f32 = 10.0;
    /// Body text: the banner, popovers.
    pub const BODY: f32 = 12.0;

    /// Letter spacing, as a fraction of the size: the FX header, the toggles, the add column.
    pub const FX_HEAD_SPACING: f32 = 0.08;
    pub const TOGGLE_SPACING: f32 = 0.03;
    /// The MST tag: the prototype sets it in mono 600, which is not embedded; it is mono 500.
    pub const TAG_SPACING: f32 = 0.04;
    pub const ADD_COLUMN_SPACING: f32 = 0.1;

    fn named(name: &str, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(name.into()))
    }

    /// Plex Sans Condensed at 400.
    pub fn sans(size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }

    /// Plex Sans Condensed at 600.
    pub fn sans_semibold(size: f32) -> FontId {
        named(SANS_SEMIBOLD, size)
    }

    /// Plex Mono at 400.
    pub fn mono(size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }

    /// Plex Mono at 500.
    pub fn mono_medium(size: f32) -> FontId {
        named(MONO_MEDIUM, size)
    }
}

/// The embedded faces: each family's name and its TrueType bytes.
pub const FACES: [(&str, &[u8]); 5] = [
    (
        font::SANS,
        include_bytes!("../assets/fonts/IBMPlexSansCondensed-Regular.ttf"),
    ),
    (
        font::SANS_MEDIUM,
        include_bytes!("../assets/fonts/IBMPlexSansCondensed-Medium.ttf"),
    ),
    (
        font::SANS_SEMIBOLD,
        include_bytes!("../assets/fonts/IBMPlexSansCondensed-SemiBold.ttf"),
    ),
    (
        font::MONO,
        include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    ),
    (
        font::MONO_MEDIUM,
        include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf"),
    ),
];

/// The font definitions: each Plex face first in its family, egui's own fonts after it as the
/// fallback for a glyph Plex lacks. `Proportional` is Plex Sans Condensed 400 and
/// `Monospace` Plex Mono 400, so a plain `ui.label` already reads in the console's type.
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let fallback = |family: &FontFamily| fonts.families.get(family).cloned().unwrap_or_default();
    let fallback_sans = fallback(&FontFamily::Proportional);
    let fallback_mono = fallback(&FontFamily::Monospace);
    for (name, bytes) in FACES {
        fonts
            .font_data
            .insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
        let fallback = if name.starts_with("plex-mono") {
            &fallback_mono
        } else {
            &fallback_sans
        };
        let family = std::iter::once(name.to_string())
            .chain(fallback.iter().cloned())
            .collect();
        let key = match name {
            font::SANS => FontFamily::Proportional,
            font::MONO => FontFamily::Monospace,
            other => FontFamily::Name(other.into()),
        };
        fonts.families.insert(key, family);
    }
    fonts
}

/// Installs the fonts and the visuals: egui's dark theme on the prototype's colours, its
/// text styles in Plex.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = colour::WIN;
    visuals.window_fill = colour::STRIP;
    visuals.extreme_bg_color = colour::BLACK;
    visuals.override_text_color = Some(colour::TEXT);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.all_styles_mut(|style| {
        for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style.text_styles.insert(text_style, font::sans(font::BODY));
        }
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, font::mono(font::BODY));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The characters of `text` that `font` draws from a fallback face rather than its first:
    /// a glyph from another face carries that face's metrics, not the family's.
    fn from_fallback(ctx: &egui::Context, font: &FontId, text: &str) -> String {
        let galley = ctx
            .fonts_mut(|f| f.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE));
        galley
            .rows
            .iter()
            .flat_map(|row| row.row.glyphs.iter())
            .filter(|g| g.font_face_height != g.font_height || g.font_face_ascent != g.font_ascent)
            .map(|g| g.chr)
            .collect()
    }

    #[test]
    fn every_character_the_console_prints_is_in_plex_itself() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        let ascii: String = (' '..='~').collect();
        // The readout's minus and infinity, the ellipsis on a long name, the device names'
        // middle dot and dash, the display's dash.
        let marks = "\u{2212}\u{221e}\u{2026}\u{b7}\u{2013}\u{2014}";
        for font in [
            font::sans(10.0),
            FontId::new(10.0, FontFamily::Name(font::SANS_MEDIUM.into())),
            font::sans_semibold(10.0),
            font::mono(10.0),
            font::mono_medium(10.0),
        ] {
            for text in [ascii.as_str(), marks] {
                let missing = from_fallback(&ctx, &font, text);
                assert!(
                    missing.is_empty(),
                    "{font:?} draws {missing:?} from a fallback"
                );
            }
            // The check can fail: Plex has no headphones emoji; egui's emoji face does.
            assert_eq!(
                from_fallback(&ctx, &font, "\u{1f3a7}"),
                "\u{1f3a7}",
                "{font:?}"
            );
        }
    }

    #[test]
    fn the_plain_families_are_plex_with_eguis_fonts_behind_it() {
        let fonts = fonts();
        let first = |family: FontFamily| fonts.families[&family][0].clone();
        assert_eq!(first(FontFamily::Proportional), font::SANS);
        assert_eq!(first(FontFamily::Monospace), font::MONO);
        assert_eq!(
            first(FontFamily::Name(font::SANS_SEMIBOLD.into())),
            font::SANS_SEMIBOLD
        );
        // egui's own faces stay as the fallback, so a glyph Plex lacks still draws.
        assert!(fonts.families[&FontFamily::Proportional].len() > 1);
        for (name, bytes) in FACES {
            assert!(
                bytes.len() > 100_000 && bytes.starts_with(&[0, 1, 0, 0]),
                "{name} is not an embedded TrueType face"
            );
        }
    }
}
