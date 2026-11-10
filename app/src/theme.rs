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
    /// An insert being dragged stays in its slot at this opacity (`.slot.dragging`).
    pub const DRAGGED_OPACITY: f32 = 0.3;

    /// `.pop`: a popover's face (the source and mix panels, the menus, the browser).
    pub const POPOVER: Color32 = Color32::from_rgb(0x1d, 0x1e, 0x21);
    /// `rgba(0,0,0,.6)`: a popover's shadow.
    pub const SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 153);
    /// `color-mix(in srgb, var(--accent) 22%, transparent)`: the browser's picked row.
    pub const ACTIVE_ROW: Color32 = Color32::from_rgba_premultiplied(
        (0x5a * 56 / 255) as u8,
        (0xa8 * 56 / 255) as u8,
        (0xff * 56 / 255) as u8,
        56,
    );

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

    /// A hairline: the header's bottom edge, the FX header's rule, the display's separators,
    /// a scale tick's thickness, an off bypass square's outline, a muted readout's strike.
    pub const HAIRLINE: f32 = 1.0;
    /// Where a hairline's centre sits inside the edge it runs along: half its width, so it
    /// fills one whole row of points rather than straddling two.
    pub const HAIRLINE_INSET: f32 = HAIRLINE / 2.0;

    /// The banner under the header when the audio needs the user (no backend, the mixer
    /// idle, the daemon gone: `BackendState::banner`): its padding and its
    /// margin, each across then down, and its outline (a hairline in the mute colour).
    pub const BANNER_PAD: [i8; 2] = [10, 6];
    pub const BANNER_MARGIN: [i8; 2] = [8, 6];

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
    /// How far the fader line reaches past its column's far side (`.vf.l .ln{right:-1px}`).
    pub const FADER_LINE_OVERHANG: f32 = 1.0;
    /// How much further short of its triangle a mix's line stops (`.vf.big.r .ln{right:7px}`
    /// against a channel's 6).
    pub const BIG_LINE_SHORTFALL: f32 = 1.0;
    /// The peak-hold line's thickness.
    pub const PEAK_LINE: f32 = 1.0;
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

    /// The caret a strip header shows under the pointer, and while its popover is open.
    pub const CARET: f32 = 10.0;
    /// A fader line under the pointer (`.vf:hover .ln{height:3px}`).
    pub const FADER_LINE_HOVER: f32 = 3.0;
    /// The keyboard focus ring: its width, and how far outside the control it sits (two
    /// points around a fader's bars, one elsewhere).
    pub const FOCUS_RING: f32 = 1.0;
    pub const FOCUS_OFFSET: f32 = 1.0;
    pub const FADER_FOCUS_OFFSET: f32 = 2.0;
    /// A slot's hover controls (`.hov`): how far the box sits in from the slot's right and
    /// top, the padding on its left; each button and its icon, and the gap between them.
    pub const HOVER_INSET: [f32; 2] = [1.0, 2.0];
    pub const HOVER_PAD_LEFT: f32 = 4.0;
    pub const HOVER_BUTTON: f32 = 16.0;
    pub const HOVER_ICON: f32 = 11.0;
    pub const HOVER_GAP: f32 = 1.0;
    /// The drop line where a dragged insert lands (`.slot.drop-before::before`): its
    /// thickness (it sits that far above the slot it precedes) and its inset at each side.
    pub const DROP_LINE: f32 = 2.0;
    pub const DROP_INSET: f32 = 2.0;
    /// A press on a slot becomes a drag once the pointer has moved this far.
    pub const DRAG_START: f32 = 5.0;
}

/// The popovers' sizes (`.pop`, `.ap`, `.menu`, `.br`, `.toast`, `.ghost`), in points.
pub mod pop {
    /// A popover's corner, and its shadow (`0 12px 32px`).
    pub const CORNER: u8 = 3;
    pub const SHADOW_Y: i8 = 12;
    pub const SHADOW_BLUR: u8 = 32;
    /// How far below its anchor a popover opens, how far beside it when it opens to the
    /// side, and how close it may come to the window's edge.
    pub const BELOW_GAP: f32 = 2.0;
    pub const BESIDE_GAP: f32 = 4.0;
    pub const EDGE_MARGIN: f32 = 8.0;

    /// The source and mix panels (`.ap`), and the preferences.
    pub const PANEL_WIDTH: f32 = 270.0;
    /// The name row (`.nm-row`): its padding across and down, the gap between label and
    /// field, a field's height and inner padding.
    pub const NAME_PAD: [f32; 2] = [10.0, 8.0];
    pub const NAME_GAP: f32 = 8.0;
    pub const FIELD_HEIGHT: f32 = 24.0;
    pub const FIELD_PAD_X: f32 = 6.0;
    /// The search row (`.search`): its height, side padding, the gap after the magnifier,
    /// the magnifier.
    pub const SEARCH_HEIGHT: f32 = 38.0;
    pub const SEARCH_PAD_X: f32 = 10.0;
    pub const SEARCH_GAP: f32 = 8.0;
    pub const SEARCH_ICON: f32 = 14.0;
    /// A panel's list (`.ap-list`): its tallest, its padding.
    pub const LIST_MAX: f32 = 300.0;
    pub const LIST_PAD: f32 = 4.0;
    /// A row of a panel's list (`.ap-row`): height, side padding, the gap between its
    /// columns, the tick box or radio, the app's initial badge.
    pub const ROW: f32 = 24.0;
    pub const ROW_PAD_X: f32 = 6.0;
    pub const ROW_GAP: f32 = 8.0;
    pub const TICK: f32 = 13.0;
    /// A picked radio's ring and the radius of its dot.
    pub const RADIO_RING: f32 = 1.5;
    pub const RADIO_DOT: f32 = 3.0;
    pub const BADGE: f32 = 18.0;
    /// A list heading (`.ap-h`): padding above and below, and across inside a list and
    /// outside one (`Input`, `Output`).
    pub const HEADING_TOP: f32 = 8.0;
    pub const HEADING_BOTTOM: f32 = 3.0;
    pub const HEADING_PAD_X: f32 = 6.0;
    pub const HEADING_OUTSIDE_PAD_X: f32 = 10.0;
    /// The note under Master's outputs: padding above and below.
    pub const NOTE_TOP: f32 = 4.0;
    pub const NOTE_BOTTOM: f32 = 8.0;
    /// The footer (`.ap-foot`): padding across and down; a button (`.btn`): height and
    /// side padding.
    pub const FOOT_PAD: [f32; 2] = [8.0, 6.0];
    pub const BUTTON_HEIGHT: f32 = 22.0;
    pub const BUTTON_PAD_X: f32 = 9.0;
    /// An empty list's message (`.br-empty`): padding across and down.
    pub const EMPTY_PAD: [f32; 2] = [12.0, 20.0];

    /// A menu (`.menu`): its width and padding, a row's height and side padding, the gap
    /// after its icon, the icon.
    pub const MENU_WIDTH: f32 = 180.0;
    pub const MENU_PAD: f32 = 4.0;
    pub const MENU_ROW: f32 = 26.0;
    pub const MENU_ROW_PAD_X: f32 = 8.0;
    pub const MENU_GAP: f32 = 8.0;
    pub const MENU_ICON: f32 = 13.0;

    /// The plugin browser (`.br`): its size, its footer, the nav column.
    pub const BROWSER: [f32; 2] = [620.0, 440.0];
    pub const BROWSER_FOOT: f32 = 32.0;
    pub const NAV_WIDTH: f32 = 172.0;
    /// A nav item (`.nav-i`): height and side padding; a nav heading (`.nav-h`): padding
    /// above, below and across.
    pub const NAV_ROW: f32 = 22.0;
    pub const NAV_PAD_X: f32 = 8.0;
    pub const NAV_HEADING: [f32; 3] = [12.0, 4.0, 8.0];
    /// A plugin row (`.br-row`): height, side padding, the gap between columns, the star's
    /// column and the star, the vendor and type columns.
    pub const PLUGIN_ROW: f32 = 24.0;
    pub const PLUGIN_PAD_X: f32 = 8.0;
    pub const PLUGIN_GAP: f32 = 8.0;
    pub const STAR_COLUMN: f32 = 16.0;
    pub const STAR: f32 = 12.0;
    pub const VENDOR_COLUMN: f32 = 128.0;
    pub const KIND_COLUMN: f32 = 92.0;
    /// An A–Z group heading (`.br-group`): padding above, below and across.
    pub const GROUP: [f32; 3] = [8.0, 3.0, 8.0];
    /// How far past the top and bottom of the browser's list and nav their rows are still
    /// drawn and sensed (the rest only take their space): a row and a heading, so Tab
    /// reaches the next row out of view, which then scrolls in.
    pub const LIST_REACH: f32 = 2.0 * PLUGIN_ROW;
    /// The browser footer's padding on its left.
    pub const FOOT_PAD_LEFT: f32 = 12.0;

    /// The toast: its distance from the window's bottom, padding (top, right, bottom,
    /// left), the gap before its button, its corner.
    pub const TOAST_BOTTOM: f32 = 12.0;
    pub const TOAST_PAD: [f32; 4] = [5.0, 6.0, 5.0, 12.0];
    pub const TOAST_GAP: f32 = 10.0;
    /// How long a toast shows, in seconds: with an action (Undo: the prototype's 5 s, which
    /// is the host's `UNDO_TOAST`, as long as it keeps what was removed), and any other
    /// message (a refusal, the host's), which says what to do and wants reading. A change
    /// that worked has no toast: the console shows it.
    pub const TOAST_WITH_ACTION_S: f64 = vixmix_host::UNDO_TOAST.as_secs_f64();
    pub const TOAST_HOST_S: f64 = 8.0;
    /// The toast's fade in and out (`transition: opacity .15s`).
    pub const TOAST_FADE_S: f32 = 0.15;

    /// The drag ghost: its offset from the pointer, its padding across and down, the gap
    /// after "+ Copy".
    pub const GHOST_OFFSET: [f32; 2] = [14.0, 10.0];
    pub const GHOST_PAD: [f32; 2] = [8.0, 3.0];
    pub const GHOST_GAP: f32 = 4.0;
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
    /// A label in a popover (`Name`), a button, the browser's footer, the drag ghost.
    pub const SMALL: f32 = 11.5;
    /// The name field and a mix's fixed name in its panel.
    pub const NAME: f32 = 12.5;
    /// A search field.
    pub const SEARCH: f32 = 13.0;
    /// A list heading in a popover (600, spaced, upper case), the app's initial badge.
    pub const HEADING: f32 = 10.0;
    /// The note beside an app ("in Music"), a count in the browser's nav (mono).
    pub const NOTE: f32 = 10.5;
    /// The vendor and type columns of the browser.
    pub const COLUMN: f32 = 11.0;
    /// The keyboard hint in the browser's search row (mono).
    pub const KBD: f32 = 10.0;

    /// Letter spacing, as a fraction of the size: the FX header, the toggles, the add column.
    pub const FX_HEAD_SPACING: f32 = 0.08;
    pub const TOGGLE_SPACING: f32 = 0.03;
    /// The MST tag: the prototype sets it in mono 600, which is not embedded; it is mono 500.
    pub const TAG_SPACING: f32 = 0.04;
    pub const ADD_COLUMN_SPACING: f32 = 0.1;
    /// A list heading in a popover (`.ap-h`, `.nav-h`, `.br-group`).
    pub const HEADING_SPACING: f32 = 0.08;

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
    // One pass a frame: a gesture is sensed once, and its command sent once.
    ctx.options_mut(|o| o.max_passes = std::num::NonZeroUsize::MIN);
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
