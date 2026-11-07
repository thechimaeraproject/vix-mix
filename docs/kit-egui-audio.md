# Kit audio widgets adoption

**Adopted 2026-11-07 by the kit lifts ([handoff 07](handoffs/07-kit-lifts.md)), at the kit's
release tag `kit-source.json` pins.** Vix Mix takes `chimaeraproject-egui-audio` (imported as
`chimaeraproject_egui_audio`) for the console's meter faders, their scale, peak hold and the
taper they read through: the crate was lifted from this window's `fader.rs`, `meters.rs` and
the scale in `strip.rs`, together with Vix's mixer panel. The kit's
[egui-audio reference](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/egui-audio.md)
defines what the crate guarantees; this record says what Vix Mix takes, what stays here, and
what changed observably. It is a git dependency on the kit's release tag in the root
`Cargo.toml`'s *The kit* group, taken by the app alone, at the same tag as the other kit
crates; it pins egui at the major the workspace already takes.

## What Vix Mix takes

- **`meter_fader`**: the fader's gestures (a relative drag, Shift for a fifth, a double-click
  to unity, the arrows, Home and End once focused, the arrows kept from moving the focus) and
  its painting (the two bars with their gradient, black above and the peak-hold line, the line
  and its triangle, the muted look), returning the positions a gesture asked for and where the
  bars are. `fader.rs` keeps `Target`, the command each position becomes, the steps and the
  focus ring, and builds the `MeterFaderStyle` from `theme.rs`.
- **`PeakHold`, `Hold` and `meter_position`**: each bar's level and hold line (900 ms, then
  0.006 a frame) and a linear peak read on the fader's taper (`meters.rs`).
- **The `Taper` trait**: `meters::Prototype` is the model's taper (`vixmix_model::taper`, the
  prototype's table) as a `Taper`, which the meters and the scale read through.
- **`paint_scale`** with edge ticks, and **`fader_y`**: the scale's marks given in decibels,
  the travel 6 points in from the column's ends.

## What stays in Vix Mix

Every colour, size and font (`theme.rs`), the taper itself (the model's, which the engine's
gain also reads), which fader is which and what its moves send, the steps and the hold's
timings, where each strip puts its faders and scale, the focus ring, the readouts and every
other widget of the console.

## What changed observably

Nothing: the console draws the same pixels. `cargo run -p vix-mix -- --dump-png <file>` writes
a file byte-identical to `docs/design/prototype/egui-look-gate.png`, the capture the user
passed at the look gate, and every gesture sends the commands it sent. Two names moved for
the tests: `strip::SCALE` is a list of `Mark { db, label }` where it was positions with labels,
and `strip::fader_y` is the kit's travel at the theme's inset, kept for the tests.

## How you know it worked

The look gate's capture compared byte for byte, and `cargo test -p vix-mix`: the headless
tests drag, Shift-drag, double-click and key the faders and read the commands
(`acceptance.rs`), and the unit tests read the meters on the taper with their hold and put
unity on the scale's 0.
