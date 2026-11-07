//! A fader (`MeterFader`) as this console has it: the kit's meter fader
//! (`chimaeraproject_egui_audio::meter_fader`, the prototype's `.vf`: the stereo meter bars with
//! their peak-hold lines, the fader line across them and its triangle at the taper's position)
//! drawn in this console's sizes and colours, with the prototype's gestures at its steps.
//!
//! - **Drag anywhere** on the column: the position moves by the pointer's travel over the
//!   bars' height, relative to where it was (a press does not jump it); with Shift a fifth
//!   as far. Each move sends the fader's command; on a linked channel the model moves the
//!   other send by as much.
//! - **Double-click**: back to unity (0 dB).
//! - **Keys**, once the fader has focus (a press gives it): Up or Right 0.01, Down or Left
//!   0.01 the other way, a fifth of that with Shift; Home to the bottom, End to the top.
//! - **Muted**: the line and triangle faint, the bars at 40 %.
//!
//! What stays here: which fader a column is ([`Target`]) and the command each move sends, the
//! steps, the focus ring, and the theme's sizes and colours.

use chimaeraproject_egui_audio::{
    self as audio, Back, FaderLook, Fill, Gestures, MeterFaderStyle, MeterStyle, PeakLine,
};
use egui::{Id, Rect, Ui};
use vixmix_model::{ChannelId, Command, MixId, taper};

pub use audio::{Side, keep_focus, take_focus};

use crate::meters::Pair;
use crate::shell::Intent;
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

/// A drag in progress: which fader, where it has got to, the pointer's last height.
pub type Drag = audio::FaderDrag<Target>;

/// How far a key moves a fader, and with Shift.
pub const KEY_STEP: f32 = 0.01;
pub const FINE_KEY_STEP: f32 = 0.002;
/// How much of the pointer's travel moves a fader with Shift held.
pub const FINE_DRAG: f32 = 0.2;

/// The prototype's gestures: unity on the taper, the steps above.
const GESTURES: Gestures = Gestures {
    unity: taper::UNITY,
    key_step: KEY_STEP,
    fine_key_step: FINE_KEY_STEP,
    fine_drag: FINE_DRAG,
};

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
    let shown = FaderLook {
        side: look.side,
        position: look.position,
        bars: look.bars,
        muted: look.muted,
    };
    let style = style(look.big);
    let moved = audio::meter_fader(
        ui,
        column,
        target.id(),
        target,
        &shown,
        &style,
        &GESTURES,
        drag,
    );
    out.extend(
        moved
            .moves
            .into_iter()
            .map(|position| Intent::Apply(target.command(position))),
    );
    if keyboard && moved.response.has_focus() {
        widgets::ring(ui.painter(), moved.bars, size::FADER_FOCUS_OFFSET);
    }
}

/// The prototype's `.vf` in the theme's sizes and colours; `big` is a mix's (`.vf.big`): wider
/// bars, and a line that stops a point short of the triangle.
fn style(big: bool) -> MeterFaderStyle<'static> {
    MeterFaderStyle {
        inset: size::FADER_INSET,
        bar: if big { size::BAR_BIG } else { size::BAR },
        bar_gap: size::BAR_GAP,
        triangle: size::TRIANGLE,
        line: size::FADER_LINE,
        line_hot: size::FADER_LINE_HOVER,
        // `.vf.l .ln{left:6px;right:-1px}`, `.vf.r .ln{left:-1px;right:6px}`, 7 on a mix.
        line_overhang: size::FADER_LINE_OVERHANG,
        line_shortfall: if big { size::BIG_LINE_SHORTFALL } else { 0.0 },
        ink: colour::ACCENT,
        muted_ink: colour::FAINT,
        muted_opacity: colour::MUTED_BARS_OPACITY,
        meter: MeterStyle {
            back: Back::AboveLevel(colour::BLACK),
            fill: Fill::Gradient(&colour::METER),
            peak: Some(PeakLine {
                colour: colour::PEAK,
                opacity: colour::PEAK_OPACITY,
                thickness: size::PEAK_LINE,
                shown_above: size::PEAK_SHOWN_ABOVE,
            }),
            outline: None,
        },
    }
}
