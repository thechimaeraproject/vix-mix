# App (`vix-mix`)
**Owns:** `app/`

**Reference — the window as built by [handoff 05](../handoffs/05-app.md), shipped
2026-11-04.** `app/` is the `vix-mix` binary: prototype v5's console as a winit window with
egui drawn through egui-winit and egui-wgpu — Vix's shell
([`app/src/main.rs`](https://github.com/thechimaeraproject/Vix/blob/master/app/src/main.rs),
[`app/src/gpu.rs`](https://github.com/thechimaeraproject/Vix/blob/master/app/src/gpu.rs)), not
eframe, because each VST3 editor needs a winit window of its own and its raw handle. It drives
one `Host` (`RealHost`, or `FakeHost` with `--fake`) and owns nothing the host could: no
setup, no audio, no plugin. Every gesture becomes a `Command` the host applies; the window
draws the setup the host holds, every frame.

It depends on `vixmix-host` and `vixmix-model`; on the kit's `chimaeraproject-vst3-host` only
to name the `EditorEvent` and `EditorSize` that `Host::tick_editors` and `Host::open_editor`
hand back; on `png` for the capture. Its tests also take `vixmix-plugins`, because their
`Recorder` implements `Host`, whose `catalog` names `Catalog`.

## The files

| File | What it is |
|---|---|
| `main.rs` | the command line, the event loop (X11 on Linux), or the capture |
| `app.rs` | the winit `ApplicationHandler`: the host, the window, the editor windows, one frame at a time, pacing, the window's least size |
| `shell.rs` | the window's state between frames (`Shell`) and what it does with a frame's `Intent`s: commands applied, refusals and notes to the toast line, popovers opened and closed, Undo, editors queued |
| `ui.rs` | what a frame draws and senses: the header and its display, the banner, the console, the open popover, the toast line |
| `console.rs` | the console row: where each strip goes, the rects a popover hangs from and a drag lands in, the insert drag resolved after the last strip |
| `strip.rs` | a channel strip and a mix strip: rows, header, scale, readouts, mute, link and ear, the add-channel column, the text helpers |
| `fader.rs` | a fader: the meter bars and peak lines, the line and triangle, the drag, double-click and keys |
| `well.rs` | the FX well: MON and MST, the slots with their square, gear and bin, the slot keys, "+ Add plugin", the scrolling list |
| `drag.rs` | an insert dragged within or between strips: where it lands, the drop line and outline, the ghost, the command a drop sends |
| `popover.rs` | the popovers: the source panel, the output panel, the add-channel menu, the menu and the preferences; where a popover goes |
| `browser.rs` | the plugin browser: the search, the nav and its counts, the list grouped A to Z, stars, the footer |
| `toast.rs` | the toast line and Undo: what a removal keeps, and how Undo puts it back |
| `editors.rs` | the plugin editor windows: open, resize, close through the host, titles |
| `widgets.rs` | what every surface shares: hover, the focus ring, the text button, tick box and radio, headings, the text field, a popover's frame |
| `meters.rs` | a linear peak read on the fader's taper, the peak-hold line |
| `icons.rs` | prototype v5's SVG icons, read once and followed with egui's painter, stroked or filled |
| `theme.rs` | the look as constants: colours, gradient, sizes (`size`, `pop`), fonts and their sizes |
| `gpu.rs` | the surface, and the one paint path: `upload` (egui's textures) and `encode` (egui's frame into a target) |
| `capture.rs` | `--dump-png`: one frame offscreen, read back, written as a PNG |
| `test_support.rs` | tests only: the headless `Harness` and the `Recorder` host |
| `acceptance.rs` | tests only: every gesture the handoff lists, with the command it sends |
| `assets/fonts/` | IBM Plex Sans Condensed (400, 500, 600) and IBM Plex Mono (400, 500), with `OFL.txt`, the SIL Open Font License they come under |

## The shell

`main.rs` parses the command line and runs the winit event loop; `app.rs` is the
`ApplicationHandler`. On `resumed` it boots the host, makes the window (titled "Vix Mix",
dark, at the console's natural size), the surface and the egui context. On `RedrawRequested`
it draws a frame. Events for an editor's window go to its editor ([Editor
windows](#editor-windows)). On `CloseRequested` it exits; on `exiting` every editor closes
through the host, then the host goes (its instances after their editors), then the window.
`gpu.rs` is the surface (a non-sRGB format, as egui-wgpu prefers; mailbox present where the
surface offers it, FIFO otherwise, because the window paces itself and FIFO's acquire stalled
for tens of milliseconds behind Windows' compositor) and the egui renderer, cleared to the window colour (`#131416`); a lost or outdated surface is reconfigured
and the frame skipped. Its `upload` and `encode` are shared with the capture, so the PNG is
drawn exactly as the window draws. Every GPU failure says what to do (update the driver, or
set `WGPU_BACKEND` to another backend).

**The window's size.** It opens at the console's size (`ui::natural_size`: 900 × 488 for the
example, 370 × 488 for a first start's one channel). Each frame reports what it needs
(`Shell::content`): the console under the header and the banner, and the header's own width so
its display clears the title; the window's least size follows, and a window smaller than that
grows to it. A channel added widens it; a channel removed lets it shrink.

**On Linux** the event loop runs on X11 whenever there is an X display (`DISPLAY`), so under
KDE's Wayland session the window is an XWayland one, because a VST3 editor embeds only in an
X11 window and the kit refuses Wayland handles. With no X display it runs on Wayland and logs
that editors cannot open. winit makes one event loop a process, so the choice is made before
trying. The later option, a separate X11 connection for editors only so the console can run
on Wayland, is the charter's open point.

## A frame

`Running::frame`, in order: the host's events (`Shell::event`: the backend's state, a toast,
an editor the host closed); the host's meters into the window's `Meters`; the open editors
served (`EditorWindows::tick`); then egui runs `ui::draw`, which reads the host and the shell
and returns the frame's `Intent`s; `Shell::dispatch` carries them out against the host; the
editors it queued open in windows of their own; open editors follow their insert (a new title
when it moved or its strip was renamed, closed when it left the setup); the window's least size
follows the content. egui runs one pass a frame, so a gesture is sensed once and its command
sent once.

**An `Intent`** is what a gesture asks: `Apply(Command)`, `OpenEditor(instance)`,
`Toggle(anchor)` (open the popover that hangs from it, or close it), `AddChannel(kind)`,
`Undo`, `Favorite { id, on }`. A frame never changes the setup it is drawing; the headless
tests drive the same `draw` and `dispatch` the window does.

**`Shell::dispatch`** applies each command through `Host::apply`. A refusal goes to the toast
line as the model words it (each says what to do). The editors of the inserts a command
removes close through the host first. A change earns its note: "Removed <plugin> from
<strip>" or "Removed <channel>" with Undo (5 s), "Moved" or "Copied <plugin> to <strip>" for a
drag between strips or a copy, "Added <plugin> to <strip>", "Monitor output now plays Master"
and "Monitor output back to the Monitor mix" (2.4 s). A host's `Toast` shows 8 s.

## Pacing

The loop is `ControlFlow::Wait`. A frame is drawn when egui wants one (input, or its
`repaint_delay` coming due: a tooltip, the toast's fade, a text cursor), straight after a
frame that acted or opened or closed a popover (what changed shows at once, even with no
backend), and — while the host's `BackendState::live()` (running or simulated) or a
`--quit-after-frames` count is running — on a 60 Hz grid: a frame the timer wakes counts from
when it was due, and the interval from frame start to frame start, so neither drawing nor the
timer's lateness stretches it. No frame, input's included, is due sooner than
`FRAME_INTERVAL` (16.667 ms) after the last was due, so the window never draws more than 60
frames a second. While a plugin editor is open it draws at least every `EDITOR_POLL` (33 ms),
which serves the editors. With no backend, no editor and nothing to animate it is idle.

## The command line

For checks, not users: `--fake` runs against `FakeHost`; `--quit-after-frames N` (N ≥ 1)
closes the window once N frames have reached the screen and exits 0, logging how long they
took and the rate ("drew 1200 frames in 19.99 s after the first (60.0 a second)"), which is
the headless proof that the window works and paces at 60 Hz; `--dump-png <file>` draws one
frame of `FakeHost`'s console offscreen and exits 0 without opening a window ([The
capture](#the-capture)). `--dump-png` with `--quit-after-frames` is refused, as is anything
else, with exit 2 and what to write instead.

## The look

`theme.rs` holds prototype v5's look (`docs/design/prototype/index.html`, its CSS) as named
constants, and nothing else in the app spells a colour or a size:

- **`colour`**: `WIN #131416`, `STRIP #212226`, `MIX #26272c`, `WELL #18191b`, `CTL #2c2d32`,
  `CTL_HI #383a40`, `LINE #2a2b2f`, `BLACK #0c0d0e`, `TEXT #eceef1`, `DIM #9a9ea7`,
  `FAINT #62666e`, `ACCENT #5aa8ff`, `MUTE #e5484d`; the CSS's literals by role (`EDGE #000`,
  `ON_ACCENT #06121f`, `ON_MUTE #fff`, `LIVE #4cc46d`, `PEAK #eef0f2` at 0.85, a popover's
  `POPOVER #1d1e21` and its `SHADOW` at 0.6, the browser's `ACTIVE_ROW`, the accent at 22 %);
  a muted fader's bars at 0.4, a dragged insert at 0.3; the meter gradient `METER`, bottom to
  top: `#2c9a55 0%, #4cc46d 48%, #9fcf43 64%, #e2b83a 76%, #e5484d 84%`, red to the top.
- **`size`**: 2-point corners (1 on the bypass square); the hairline and its half-point inset;
  the header 34; the banner's padding and margin; the console's 2-point padding and gaps and
  the 6 before the mixes; the add-channel column 26; a strip 104 (a mix 112), rows 22 / 151 /
  210 / 12 / 20 with 5-point gaps and padding 7 6 8 (450 tall); every inner size the CSS gives
  (the FX header 19, slots 20, toggles 14, faders 20 and 28 with 5- and 8-point bars 6 in from
  each end, the line 2 (3 under the pointer) and its overhang, the triangle 6 × 8, the peak
  line, mute buttons 24 × 20, link 20 × 18, icons 12, 13 and 14, the caret 10); the slot's
  hover controls (16-point buttons, 11-point icons, inset 1 and 2); the drop line (2, inset 2);
  the focus ring; a drag's 5-point start.
- **`pop`**: the popovers' sizes — the 3-point corner and the `0 12px 32px` shadow, where a
  popover opens (2 below, 4 beside, 8 from the window's edge); the panels (270 wide, the name
  row, the 38-point search row, lists at most 300 tall, 24-point rows, tick box, badge,
  headings, the footer and its 22-point buttons); the menu (180 wide, 26-point rows); the
  browser (620 × 440, the 172-point nav, 22-point nav items, 24-point plugin rows with the
  16-point star column and the 128 and 92 columns, the A to Z headings, the 32-point footer);
  the toast (12 from the bottom, its padding and gap, 5 s with an action, 2.4 s without, 8 s
  from the host, the 0.15 s fade); the drag ghost (14 and 10 from the pointer, its padding).
- **`font`**: the families and the size each text is set in (title 13, display 11, strip
  name 12.5, FX 10, toggles 9, slots 11.5, scale 9, readouts 10, add column 10, body 12, small
  11.5, name field 12.5, search 13, headings 10, notes 10.5, the browser's columns 11, its
  keyboard hint 10) and the CSS's letter spacing as a fraction of the size.

**Fonts.** egui has no font weights, so each embedded face is a family of its own:
`Proportional` is Plex Sans Condensed 400, `Monospace` is Plex Mono 400, and
`plex-sans-condensed-500`, `-600` and `plex-mono-500` are named families. egui's own fonts
stay behind each Plex face as the fallback for a glyph Plex lacks. `theme::install` sets the
fonts, egui's dark visuals on the window colour, egui's text styles in Plex, and one pass a
frame. Prototype v5 sets nothing in Sans 500 (its 500s are all mono); it is embedded because the
handoff lists it. The MST tag's mono 600 is drawn in mono 500, the heaviest mono embedded.

**Icons.** `icons.rs` keeps the prototype's icon paths verbatim (a 24-unit box, a 1.9-unit
line, round ends), plus a tick for the app checklist, and follows them with egui's painter: a
small reader for the SVG path syntax the icons use (`M L H V A C Z`, absolute and relative,
packed numbers such as `5.9.8`), the `rect` and `circle` elements, arcs and curves as short
polylines, and a dot at each open end for the round line cap. Each icon is a `static` that
reads its path once, the first time it is painted, and keeps the runs; a frame only scales
them. `paint_filled` fills a closed run as a fan from its centre (the browser's starred star).

## What a frame draws

`ui::draw`, top to bottom:

- **The header** (34 points, the window colour, a black bottom edge): the menu button (the
  three lines, lit under the pointer or while its menu is open), the title "Vix Mix" (600, 13),
  and the display centred: cells on black split by hairlines, `48 kHz`, `256 samples`,
  `5.3 ms` (the number in mono 500, the unit faint) and `DSP` with its 36-point bar and
  percentage; the rate drops a needless decimal (`48`, `44.1`), and every number is a dash
  until the host reports a rate. No in-app window buttons: the desktop draws the window's
  decorations.
- **The banner**, when the host's backend is `Absent`, with its reason ("No audio backend on
  this platform" on Windows; on Linux, until handoff 06 plays through PipeWire, that PipeWire
  answers but is not played through yet, or PipeWire's refusal).
- **The console**, at the top left of what is left, at its natural size (`console::size`:
  900 × 454 for six channels): black, with each channel strip in setup order, the add-channel
  column (a plus over "ADD CHANNEL" set vertically, as `writing-mode: vertical-rl` sets it)
  while fewer than eight channels exist, then Monitor and Master.
- **The open popover** over the console, and **the toast line** over everything.

A **channel strip**: the header (the mic icon for a hardware input, four squares for an app
channel, the name in 600 cut with an ellipsis when long, the live dot green for a hardware
input or an app channel with a running app, else faint); the FX well (FX, MON and MST lit in
the accent when the chain feeds that mix, each insert with its bypass square, filled when on,
its name faint when bypassed, then "+ Add plugin"); two faders with the scale between (`+6, 0,
6, 12, 24, 48` at the taper's positions 1, .84, .70, .56, .36, .14, the 0 a shade brighter);
the readouts (`taper::format_db` in mono 500, struck through and faint when muted); and the
Monitor mute, the link toggle and the Master mute. A **mix strip**: the header (headphones or
bars, and on Monitor the MST tag while Master is previewed), the FX well, one wide fader beside
its scale, the readout, the mute and, on Master, the ear (accent when previewing).

A **fader**: two meter bars 6 points in from the column's ends, position 0 at the bottom and 1
at the top; each bar is the gradient up to its level and black above, with the peak-hold
line; the fader line across the bars at the send's position and its triangle pointing in;
muted, the line and triangle are faint and the bars at 0.4.

**Meters** (`meters.rs`): a host's linear peak reads at `taper::db_to_position` of its
decibels (anything at or below 0.0001 is silent), so a level at 0 dB sits on the scale's 0
beside a fader at unity, as the prototype's `dbToPos(amp2db(a))`. Each bar keeps its peak: a
louder frame takes it at once; otherwise it holds 900 ms, then falls 0.006 a frame, never
below the level. The window keeps the `Meters` between frames on its own clock.

## Gestures

Every control answers the pointer as the prototype's CSS and handlers do: the hand cursor
over what presses, `ns-resize` over a fader, a grab hand over a slot; the `:hover` faces (a
header, a slot and "+ Add plugin" on the control face, a mute or the ear on `CTL_HI` with
bright ink, the link bright, the add column on the strip's face); tooltips from the
prototype's `title`s. After a Tab the focused control shows a 1-point accent ring
(`:focus-visible`); a press of the pointer hides it again. Enter or Space presses a focused
button.

| Gesture | Command |
|---|---|
| Drag a fader anywhere on its column: the position moves by the pointer's travel over the bars' height, from where it was (a press does not jump it); Shift a fifth as far | `SetSend { ch, mix, position }` each move (a linked channel's other send moves in the model), `SetMixLevel` on a mix |
| Double-click a fader | the same at `taper::UNITY` |
| With a fader focused (a press gives it the keys): Up or Right, Down or Left 0.01 (Shift 0.002), Home 0, End 1 | the same, from where the last key left it; the arrows never move the focus |
| A mute; the link; the ear | `SetMute`, `SetMixMute`; `SetLink`; `SetPreview` |
| MON, MST | `SetChainRouting { ch, mix, on }` |
| A slot's square; Space on a focused slot | `SetInsertEnabled { instance, enabled }` |
| A slot's gear; double-click a slot; Enter on a focused slot | opens its editor |
| A slot's bin; Delete or Backspace on a focused slot | `RemoveInsert { instance }`, with Undo |
| Alt with Up or Down on a focused slot | `MoveInsert` one place within its strip; Up or Down alone moves the focus |
| Drag a slot (5 points) and drop on any strip | `MoveInsert { from, to, copy }`, `copy` while Ctrl (or Alt) is held at the drop; nothing where it already is |
| A strip's header | its popover: a channel's source panel, a mix's output panel |
| "+ Add plugin" | the plugin browser, beside it |
| The add column | its menu: App channel, Hardware input |
| The menu button | the menu: Preferences, Rescan plugins |

**The insert drag** (`drag.rs`): the slot stays put at 30 %, a ghost beside the pointer reads
the plugin's name ("+ Copy" before it in the accent while Ctrl or Alt is held), and the strip
under the pointer — anywhere on it — shows where the insert would land: a 2-point accent line
before the first insert whose middle is below the pointer, or before "+ Add plugin", and a
1-point accent outline round its list when it is another strip's. Escape cancels; a drop
outside every strip does nothing. Starting a drag closes the open popover.

**The well** scrolls with the wheel when its inserts are more than it holds (the charter: no
cap on plugins per strip); a slot under the pointer, or with the focus inside it, shows its
gear and bin over its right end.

## Popovers

One at a time, each hanging from what opened it (`popover::place`): below it with the left
edges lined up (a header, the menu button), or beside it to the right — to the left when the
right has no room — (the add column, "+ Add plugin"), always 8 points inside the window. A
press outside a popover and outside what it hangs from closes it, and still lands on what was
pressed; Escape closes it and gives the focus back to what it hangs from; its own anchor
toggles it. Popovers show at once (no fade), with the prototype's face, outline, 3-point
corner and shadow. Each kind's first appearance is drawn unseen for one frame (egui sizes a
new area first).

- **The source panel** (270 wide), under a channel's header: the name field (the header shows
  what it holds as it is typed; `Rename` is sent once, when the panel closes, and not at all
  when the name is unchanged or blank); for an app channel a search field ("Find an app") and
  the checklist from `Host::apps`, "Playing now" then "Seen before", each app with its tick
  box, its initial, its name (faint when not playing) and "in <channel>" when another channel
  has it — a tick sends `SetSource { ch, Apps(..) }` with the app added (the model takes it out
  of the other channel) or taken out; for a hardware channel the inputs from `Host::devices`
  as radios (`SetSource { ch, Hardware(Some(device)) }`), the one in use first and marked "not
  connected" when it is absent; then Remove channel (`RemoveChannel`, with Undo) and Done. A
  channel just added opens its panel with the name selected, ready to type over. An empty
  list says why ("No app has played yet: play one and it shows here.", "No app matches.",
  "No input is present: connect one and it shows here."). A name or a search takes at most
  64 characters.
- **The output panel**, under a mix's header: its name, the outputs as radios
  (`SetMixDevice`), "No device" first on Master (`SetMixDevice { mix: Master, device: None }`),
  "Apps can also record Master as an input.", Done.
- **The add-channel menu** (180 wide), beside the add column: App channel, Hardware input
  (`AddChannel { kind }`).
- **The menu**, under the menu button: Preferences, and Rescan plugins drawn faint and inert
  until the host offers a rescan ([What the window needs from the
  host](#what-the-window-needs-from-the-host)).
- **The preferences** (270 wide): the quantum asked of PipeWire as radios (128, 256, 512
  samples; `SetPrefs`), the extra plugin folders each with its bin, a field and Add for
  another (`SetPrefs` with the folder added or removed, the rest kept; a folder takes at most
  1024 characters), Done.

## The plugin browser

`browser.rs`, 620 × 440, beside the "+ Add plugin" that opened it. Search-first: the field
has the focus ("Search 42 plugins by name, maker or type", the hint "↑ ↓ to pick · Enter to
add" at its right). Typing searches every plugin, ranked as the prototype ranks: the name
starts with it, then contains it, then the maker, then the type, each group by name; the match
in the name is drawn in the accent, and All plugins is lit. Up and Down pick a row (the picked
row on the accent at 22 %, scrolled into view); Enter adds it. With the field empty the nav
picks the list: Favorites, Recent and All plugins with their counts, then "Type" with each
type the catalog has, in the prototype's order, and "Maker" with each maker by name, each with
its count; All plugins is grouped A to Z (`#` for a name that starts with no letter); Recent
keeps its own order, the rest are by name. A row's star toggles the favourite (the starred
star filled in the accent); a press on the row, or Enter, sends `AddInsert { strip, plugin,
at: len }` (the plugin as the catalog names it: id, bundle, name) and closes the browser. The
footer says where it goes: "Adds to **<strip>**, slot N". An empty list says "No installed
plugin matches “…”."

## Toasts and Undo

`toast.rs`: one message at a time at the bottom of the window, centred, on the control face
with a black outline, fading in and out over 0.15 s; a new message takes the old one's place,
and the old one's Undo with it. A message wider than the window wraps; each is laid out before
it is placed, so it is centred from its first frame.

Removing an insert or a channel shows "Removed …" with Undo for 5 s. The window keeps what was
removed while the toast shows and, on Undo, puts it back through the commands the `Host` trait
has today (`toast::replay`): an insert is added again at its place (`AddInsert`, then
`SetInsertEnabled` false when it was bypassed); a channel is added again (`AddChannel`) with
its name, its source less any app another channel has taken since, its faders (unlinked while
they are set), mutes, MON and MST, link, and its inserts, each command sent only when the new
channel differs. Until the model has change request 2's `RestoreChannel` and `RestoreInsert`,
a restored channel comes back at the right end rather than its place, and a restored insert is
a new instance that starts from the plugin's defaults rather than the state the host kept. A
refusal (the strip it belonged to has gone) goes to the toast line.

## Editor windows

`editors.rs`, Vix's `open_editor` and `EditorWindow` as the model: one winit window per open
editor, which the plugin draws into itself (egui never does), titled "<plugin> · <strip> ·
slot N".

- **Open** (the gear, a double-click, Enter): a window is made hidden and fixed-size, its raw
  handle goes to `Host::open_editor`, and the window takes the size the plugin asks for and
  shows. Opening an editor that is open raises its window. A refusal (no editor, `FakeHost`'s
  "run with the real host to open a plugin's editor", a Wayland handle on Linux) drops the
  window and goes to the toast line. At most 16 are open at once.
- **Resize**: `Host::tick_editors`, every frame while one is open, names the instance that
  asked; its window takes the size. (Telling the plugin its new size waits for the host; see
  below.)
- **Close**: always through `Host::close_editor` before the window goes — the window's close
  button, the insert's removal (the shell closes it before the command), the insert leaving the
  setup any other way, the window closing at exit — and an editor the host closed itself
  (`HostEvent::EditorClosed`) loses its window.
- Each opens, resizes and closes with an `info` log line, which a by-hand pass reads.

## What the window needs from the host

The window codes against the `Host` trait as it is. Three of its controls want a call the trait
does not have yet, proposed as the build map's change request 8 for lane 06:
`set_favorite(id, on)` (the browser's star; until then a toggle lasts until the window closes,
`Favorites` in `shell.rs`), `rescan_plugins()` reporting `HostEvent::PluginsRescanned` (the
menu's Rescan plugins, inert until then), and `resize_editor(instance, size)` (the kit's
resize is asynchronous: after the window has taken the size, `EditorSession::resize` must tell
the plugin). Undo waits on change request 2's `RestoreChannel` and `RestoreInsert` in the
model, after which it sends one command.

## The capture

`--dump-png <file>` (`capture.rs`) is the look gate's capture: it makes `FakeHost`, steps its
simulated meters 2.5 seconds a 60 Hz frame at a time (so the peak-hold lines stand above the
levels), runs `ui::draw` at the console's natural size and 2 pixels a point (the prototype's
screenshot is a 2× capture), paints it through `gpu::upload` and `gpu::encode` into an
`Rgba8Unorm` texture on any adapter (`WGPU_BACKEND` is honoured), reads it back, and writes an
8-bit RGBA PNG: 1800 × 976 for the example console. It needs a GPU adapter and no window or
display. `FakeHost` is seeded, so the same adapter draws the same PNG every run; with the
gestures built it still draws the look gate's PNG byte for byte, because nothing is pressed.
`docs/design/prototype/egui-look-gate.png` is its output, beside the prototype's
`screenshot.png`:

```
cargo run -p vix-mix -- --dump-png docs/design/prototype/egui-look-gate.png
```

## Verification

`cargo test -p vix-mix`, headless on both CI legs (egui runs without a window or a GPU).

**The gestures** (`acceptance.rs`, on `test_support::Harness`: `FakeHost`'s example console in
`ui::draw` and `Shell::dispatch`, as the window runs them, with every call to the host
recorded; each test asserts the command its gesture sent):

- faders: `a_fader_drag_moves_the_send_by_the_pointers_travel`,
  `with_shift_a_drag_moves_a_fifth_as_far`,
  `on_a_linked_channel_the_other_send_moves_with_it`,
  `a_double_click_puts_a_fader_back_at_unity` (a send and a mix),
  `the_keys_step_a_focused_fader` (Up, Shift+Down, Right, Home, End; the focus stays);
- buttons: `mute_link_and_the_ear_toggle`, `mon_and_mst_choose_which_sends_take_the_chain`,
  `the_square_bypasses_an_insert`;
- the drag: `an_insert_dragged_within_its_strip_moves_there` (and dropped where it is, or not
  moved far enough, sends nothing), `an_insert_dragged_to_another_strip_moves_or_with_ctrl_copies`,
  `a_drag_shows_its_ghost_and_escape_cancels_it`;
- the browser: `the_browser_searches_and_enter_adds_the_first_hit` (type, Down, Enter),
  `the_browser_nav_filters_and_a_star_toggles_a_favourite`;
- the source panel: `an_app_ticked_in_one_channel_leaves_another`,
  `an_app_search_that_finds_nothing_says_so`, `a_hardware_channel_picks_its_input`,
  `the_name_field_renames_once_when_the_panel_closes`;
  the output panel: `the_output_panel_picks_a_device_and_master_can_have_none`;
- remove and Undo: `removing_an_insert_shows_undo_and_undo_puts_it_back` (its editor closed
  through the host first), `a_bypassed_insert_comes_back_bypassed_and_undo_dies_with_its_toast`,
  `removing_a_channel_shows_undo_and_undo_brings_it_back` (the whole replay);
- the rest: `a_press_outside_or_escape_closes_a_popover`,
  `the_add_column_adds_a_channel_and_opens_its_panel_to_name_it`,
  `the_gear_or_a_double_click_asks_for_the_insert_s_editor`,
  `a_refusal_reaches_the_toast_line_with_its_fix`, `a_long_chain_scrolls_in_its_well`,
  `the_preferences_ask_for_a_quantum_and_take_plugin_folders`,
  `the_window_keeps_room_for_the_header_over_a_narrow_console`.

**The parts:**

- `main.rs`: the flags parse and default to the real host running until closed; every refusal
  names what to write instead.
- `theme.rs`: every character the console prints comes from the Plex face itself in all five
  families, and the check catches a fallback; the plain families are Plex with egui's fonts
  behind it.
- `icons.rs`: every icon reads and draws inside its box, read once and kept; the path reader
  follows the SVG grammar.
- `strip.rs`: a strip's rows are the prototype's heights inside its padding; a fader at unity
  sits on the scale's 0, its ends 6 in; the header names the source as the prototype does.
- `console.rs`: the example console is 900 × 454 and eight channels drop the add column; the
  strips sit in setup order with the add column before the mixes; the live dot follows hardware
  or a running app.
- `drag.rs`: a drop lands before the first insert whose middle is below the pointer; dropping
  an insert where it is sends nothing and a copy always lands.
- `popover.rs`: a popover opens below or beside its anchor and stays on screen; ticking an app
  adds it once and unticking takes it out; closing the source panel renames only when the name
  changed.
- `browser.rs`: a search ranks name starts, then contains, then maker, then type; the nav
  counts favourites, recent, types and makers in order; All plugins group by their first
  letter.
- `toast.rs`: a toast shows for its time and its Undo goes with it.
- `editors.rs`: an editor window is titled plugin, strip and slot.
- `meters.rs`: a level reads on the fader's taper; a peak holds 900 ms then falls 0.006 a
  frame to the level; the console's meters follow the host's frame by position.
- `ui.rs`, headless frames of `ui::draw` that press nothing and ask nothing: the header names
  the app and shows the display; the display says kHz without a needless decimal; the banner
  appears exactly when the backend is absent; the console draws the example's strips, inserts,
  readouts, scales and the add column; a muted send reads struck through and a bypassed insert
  faint; MON and MST light when the chain feeds that mix; previewing Master tags Monitor.
- `capture.rs`: read-back rows lose their padding; the PNG holds the pixels as drawn; an
  unwritable path says what to check.

**By hand** (a GPU and a desktop, so not in CI):

- `cargo run -p vix-mix -- --fake --quit-after-frames 1200` on Windows logs "drew 1200 frames
  in 19.99 s after the first (60.0 a second)"; over 20 s of `--fake` at 60 frames a second the
  process used 0.32 % of the machine's CPU (24 logical cores; 7.6 % of one core, most of it in
  the GPU driver and the Vulkan overlay layers installed on that machine; drawing and
  tessellating a frame takes about 0.5 ms), measured from the process's CPU time in
  PowerShell. With FIFO present the same machine drew 39 to 57 frames a second, its acquire
  stalling up to 80 ms; mailbox holds 60.
- `cargo run -p vix-mix -- --quit-after-frames 3` runs on the real host and exits 0.
- The capture command above writes the 1800 × 976 PNG, byte for byte the look gate's.
- An editor: a scratch profile (`CHIMAERA_CONFIG_DIR`) with Goodhertz's Tone Control 3 on Mic
  opened it in a window titled "Ghz Tone Control 3 · Mic · slot 1" at the 552 × 404 it asked
  for, drawn by the plugin, and closed it through the host when its window closed.
- The look itself was the user's verdict on the PNG beside the screenshot, not a pixel diff.
