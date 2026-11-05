# App (`vix-mix`)
**Owns:** `app/`

**Reference — the window at the look gate: [handoff 05](../handoffs/05-app.md) step 1 is
built (the theme, the fonts, the console drawn static from the host, the headless capture) and
waits for the user's verdict; steps 2–9 complete this page.** `app/` is the `vix-mix` binary: a
winit window with egui drawn through egui-winit and egui-wgpu — Vix's shell
([`app/src/main.rs`](https://github.com/thechimaeraproject/Vix/blob/master/app/src/main.rs),
[`app/src/gpu.rs`](https://github.com/thechimaeraproject/Vix/blob/master/app/src/gpu.rs)), not
eframe, because each VST3 editor later needs a winit window of its own and its raw handle. It
drives one `Host` (`RealHost`, or `FakeHost` with `--fake`) and owns nothing the host could:
no setup, no audio, no plugin. It depends on `vixmix-host` and `vixmix-model` only, and on
`png` for the capture.

## The files

| File | What it is |
|---|---|
| `main.rs` | the command line, then the event loop (or the capture) |
| `app.rs` | the winit `ApplicationHandler`: the host and its polls, the window, one frame at a time, the meters carried between frames, the banner line |
| `gpu.rs` | the surface, and the one paint path: `upload` (egui's textures) and `encode` (egui's frame into a target) |
| `ui.rs` | what a frame draws: the header and its display, the banner, the console |
| `console.rs` | the console row: channels, the add-channel column, the mixes, and its size |
| `strip.rs` | a channel strip and a mix strip, their rows, the fader geometry, the text helpers |
| `meters.rs` | a linear peak read on the fader's taper, the peak-hold line |
| `icons.rs` | prototype v5's SVG icons, followed with egui's painter |
| `theme.rs` | the look as constants: colours, gradient, sizes, fonts and their sizes |
| `capture.rs` | `--dump-png`: one frame offscreen, read back, written as a PNG |
| `assets/fonts/` | IBM Plex Sans Condensed (400, 500, 600) and IBM Plex Mono (400, 500), with `OFL.txt`, the SIL Open Font License they come under |

## The shell

`main.rs` parses the command line and runs the winit event loop; `app.rs` is the
`ApplicationHandler`: on `resumed` it boots the host, makes the window (titled "Vix Mix", dark,
1280 × 720, at least 480 × 320), the surface and the egui context; on `RedrawRequested` it
draws a frame; on `CloseRequested` it exits; on `exiting` it drops the host before the window,
so editors close first. `gpu.rs` is the surface (a non-sRGB format, as egui-wgpu prefers, FIFO
present) and the egui renderer, cleared to the window colour (`#131416`); a lost or outdated
surface is reconfigured and the frame skipped. Its `upload` and `encode` are shared with the
capture, so the PNG is drawn exactly as the window draws.

## Pacing

The loop is `ControlFlow::Wait`. A frame is drawn when egui wants one for input, when egui's
`repaint_delay` comes due, and — while the host's `BackendState::live()` (running or simulated)
or a `--quit-after-frames` count is running — every `FRAME_INTERVAL` (16.667 ms). The next
frame is never scheduled sooner than one interval after the last, so the window repaints at
most 60 times a second. Each frame polls the host and reads its `MeterFrame` once into the
window's `Meters`. Between frames the host is polled on its own when it asks
(`Host::next_poll`: PipeWire's news, the debounced save, an undo running out), with no frame
unless the poll brought news; with nothing animating and nothing pending in the host the window
is idle.

## The command line

For checks, not users: `--fake` runs against `FakeHost`; `--quit-after-frames N` (N ≥ 1)
closes the window once N frames have reached the screen and exits 0, which is the headless
proof that the window works (`cargo run -p vix-mix -- --quit-after-frames 3`);
`--dump-png <file>` draws one frame of `FakeHost`'s console offscreen and exits 0 without
opening a window ([The capture](#the-capture)). `--dump-png` with `--quit-after-frames` is
refused, as is anything else, with exit 2 and what to write instead.

## The look

`theme.rs` holds prototype v5's look (`docs/design/prototype/index.html`, its CSS) as named
constants, and nothing else in the app spells a colour or a size:

- **`colour`**: `WIN #131416`, `STRIP #212226`, `MIX #26272c`, `WELL #18191b`, `CTL #2c2d32`,
  `CTL_HI #383a40`, `LINE #2a2b2f`, `BLACK #0c0d0e`, `TEXT #eceef1`, `DIM #9a9ea7`,
  `FAINT #62666e`, `ACCENT #5aa8ff`, `MUTE #e5484d`; the CSS's literals by role (`EDGE #000`,
  `ON_ACCENT #06121f`, `ON_MUTE #fff`, `LIVE #4cc46d`, `PEAK #eef0f2` at 0.85); a muted
  fader's bars at 0.4; the meter gradient `METER`, bottom to top: `#2c9a55 0%, #4cc46d 48%,
  #9fcf43 64%, #e2b83a 76%, #e5484d 84%`, red to the top.
- **`size`**: 2-point corners (1 on the bypass square), no shadows (popovers will have one);
  the header 34; the console's 2-point padding and gaps and the 6 before the mixes; the
  add-channel column 26; a strip 104 (a mix 112), rows 22 / 151 / 210 / 12 / 20 with 5-point
  gaps and padding 7 6 8 (450 tall); and every inner size the CSS gives (the FX header 19,
  slots 20, toggles 14, faders 20 and 28 with 5- and 8-point bars 6 in from each end, the
  triangle 6 × 8, mute buttons 24 × 20, link 20 × 18, icons 12, 13 and 14).
- **`font`**: the families and the size each text is set in (title 13, display 11, strip
  name 12.5, FX 10, toggles 9, slots 11.5, scale 9, readouts 10, add column 10, body 12) and
  the CSS's letter spacing as a fraction of the size.

**Fonts.** egui has no font weights, so each embedded face is a family of its own:
`Proportional` is Plex Sans Condensed 400, `Monospace` is Plex Mono 400, and
`plex-sans-condensed-500`, `-600` and `plex-mono-500` are named families. egui's own fonts
stay behind each Plex face as the fallback for a glyph Plex lacks. `theme::install` sets the
fonts, egui's dark visuals on the window colour, and egui's text styles in Plex. Prototype v5
sets nothing in Sans 500 (its 500s are all mono); it is embedded because the handoff lists it.
The MST tag's mono 600 is drawn in mono 500, the heaviest mono embedded.

**Icons.** `icons.rs` keeps the prototype's icon paths verbatim (a 24-unit box, a 1.9-unit
line, round ends) and follows them with egui's painter: a small reader for the SVG path syntax
the icons use (`M L H V A C Z`, absolute and relative, packed numbers such as `5.9.8`), the
`rect` and `circle` elements, arcs and curves as short polylines, and a dot at each open end
for the round line cap. egui draws no SVG; this keeps each icon one string from the prototype.

## What a frame draws

`ui::draw`, top to bottom:

- **The header** (34 points, the window colour, a black bottom edge): the menu glyph, the
  title "Vix Mix" (600, 13), and the display centred: cells on black split by hairlines,
  `48 kHz`, `256 samples`, `5.3 ms` (the number in mono 500, the unit faint) and `DSP` with
  its 36-point bar and percentage; the rate drops a needless decimal (`48`, `44.1`), and
  every number is a dash until the host reports a rate. The prototype's in-app window buttons
  are not drawn: the desktop draws the window's decorations.
- **The banner line**: the host's `BackendState::banner()` when it has one ("No audio backend
  on this platform" on Windows; on Linux PipeWire's refusal, the mixer idle with what would
  drive it, or the daemon gone), else the newest toast for six seconds (`NOTE_FOR`), else the
  first insert playing as a pass-through. Handoff 06 wired the last two into this one line
  until the window's own toasts and slot marks (steps 6 and 3) take them.
- **The console**, at the top left of what is left, at its natural size
  (`console::size`: 900 × 454 for six channels): black, with each channel strip in setup
  order, the add-channel column (a plus over "ADD CHANNEL" set vertically, as
  `writing-mode: vertical-rl` sets it) while fewer than eight channels exist, then Monitor and
  Master.

A **channel strip** (`strip::channel`): the header (the mic icon for a hardware input, four
squares for an app channel, the name in 600 cut with an ellipsis when long, the live dot green
for a hardware input or an app channel with a running app, else faint); the FX well (FX, MON
and MST lit in the accent when the chain feeds that mix, each insert with its bypass square,
filled when on, its name faint when bypassed, then "+ Add plugin", clipped to the well); two
faders with the scale between (`+6, 0, 6, 12, 24, 48` at the taper's positions 1, .84, .70,
.56, .36, .14, the 0 a shade brighter); the readouts (`taper::format_db` in mono 500, struck
through and faint when muted); and the Monitor mute, the link toggle and the Master mute. A
**mix strip** (`strip::mix`): the header (headphones or bars, and on Monitor the MST tag while
Master is previewed), the FX well, one wide fader beside its scale, the readout, the mute and,
on Master, the ear (accent when previewing).

A **fader** (`strip::fader_y` is its geometry): two meter bars 6 points in from the column's
ends, position 0 at the bottom and 1 at the top; each bar is the gradient up to its level and
black above, with the peak-hold line; the 2-point fader line across the bars at the send's
position and its triangle pointing in; muted, the line and triangle are faint and the bars at
0.4.

**Meters** (`meters.rs`): a host's linear peak reads at `taper::db_to_position` of its
decibels (anything at or below 0.0001 is silent), so a level at 0 dB sits on the scale's 0
beside a fader at unity, as the prototype's `dbToPos(amp2db(a))`. Each bar keeps its peak: a
louder frame takes it at once; otherwise it holds 900 ms, then falls 0.006 a frame, never
below the level. The window keeps the `Meters` between frames on its own clock.

**Static.** Nothing in the console senses the pointer yet: no hover (the header's caret, the
slots' gear and bin, the 3-point fader line), no drag, no click. Step 3 puts the gestures over
the same rects (`strip::Rows`, `strip::fader_y`, the button sizes in `theme::size`).

## The capture

`--dump-png <file>` (`capture.rs`) is the look gate's capture: it makes `FakeHost`, steps its
simulated meters 2.5 seconds a 60 Hz frame at a time (so the peak-hold lines stand above the
levels), runs `ui::draw` at the console's natural size and 2 pixels a point (the prototype's
screenshot is a 2× capture), paints it through `gpu::upload` and `gpu::encode` into an
`Rgba8Unorm` texture on any adapter (`WGPU_BACKEND` is honoured), reads it back, and writes an
8-bit RGBA PNG: 1800 × 976 for the example console. It needs a GPU adapter and no window or
display. `FakeHost` is seeded, so the same adapter draws the same PNG every run.
`docs/design/prototype/egui-look-gate.png` is its output, beside the prototype's
`screenshot.png`:

```
cargo run -p vix-mix -- --dump-png docs/design/prototype/egui-look-gate.png
```

## Verification

`cargo test -p vix-mix`:

- `main.rs`: the flags parse and default to the real host running until closed
  (`the_flags_parse_and_default_to_the_real_host_running_until_closed`), and every refusal
  names what to write instead, `--dump-png` with `--quit-after-frames` among them
  (`every_refusal_names_what_to_write_instead`).
- `theme.rs`: every character the console prints (printable ASCII, `−∞`, the ellipsis, `·`,
  `–`, `—`) comes from the Plex face itself in all five families, not a fallback, and the
  check catches a fallback when there is one
  (`every_character_the_console_prints_is_in_plex_itself`); the plain families are Plex with
  egui's fonts behind it and the faces are embedded TrueType
  (`the_plain_families_are_plex_with_eguis_fonts_behind_it`).
- `icons.rs`: every icon reads and draws inside its box
  (`every_icon_reads_and_draws_inside_its_box`); the path reader follows the SVG grammar —
  implicit lines, packed numbers, closing, an arc's endpoint and peak
  (`a_path_follows_the_svg_grammar`).
- `strip.rs`: a strip's rows are the prototype's heights inside its padding
  (`a_strips_rows_are_the_prototypes_heights_inside_its_padding`); a fader at unity sits on
  the scale's 0, its ends 6 in, and the marks are the taper's
  (`a_fader_at_unity_sits_on_the_scales_zero_and_its_ends_six_in`).
- `console.rs`: the example console is 900 × 454 and eight channels drop the add column
  (`the_example_console_is_the_prototypes_size`); the live dot follows hardware or a running
  app (`a_channel_is_live_when_it_is_hardware_or_one_of_its_apps_plays`).
- `meters.rs`: a level reads on the fader's taper (`a_level_reads_on_the_faders_taper`); a
  peak holds 900 ms then falls 0.006 a frame to the level
  (`a_peak_holds_900_ms_then_falls_a_little_each_frame_to_the_level`); the console's meters
  follow the host's frame by position
  (`the_console_meters_follow_the_hosts_frame_by_position`).
- `ui.rs`, headless egui frames of `ui::draw` on `FakeHost`: the header names the app and
  shows the display (`the_header_names_the_app_and_shows_the_display`,
  `the_display_says_khz_without_a_needless_decimal`); the banner appears exactly when the
  backend is absent (`the_banner_shows_only_when_there_is_no_backend`); the console draws the
  example's eight strips, inserts (a long name cut with an ellipsis), readouts through the
  taper, scales and the add column
  (`the_console_draws_the_examples_strips_as_the_prototype_shows_them`); a muted send reads
  struck through and faint and a bypassed insert faint
  (`a_muted_send_reads_struck_through_and_a_bypassed_insert_faint`); MON and MST light when
  the chain feeds that mix (`mon_and_mst_light_when_the_chain_feeds_that_mix`); previewing
  Master tags Monitor (`previewing_master_tags_monitor`).
- `capture.rs`: read-back rows lose their padding (`read_back_rows_lose_their_padding`); the
  PNG holds the pixels as drawn (`the_png_holds_the_pixels_as_drawn`); an unwritable path
  says what to check (`an_unwritable_path_says_what_to_check`).

By hand (a GPU and a desktop, so not in CI): `cargo run -p vix-mix -- --fake
--quit-after-frames 3` and `cargo run -p vix-mix -- --quit-after-frames 3` exit 0 on Windows,
logging the adapter and "drew 3 frames"; the capture command above exits 0 and writes the
1800 × 976 PNG. The look itself is the user's verdict on the PNG beside the screenshot, not a
pixel diff.
