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
| `app.rs` | the winit `ApplicationHandler`: the host and its polls, the window, the editor windows, one frame (or, hidden, one tick without a frame) at a time, what the window's events say of whether it can be seen, egui's viewport facts, the window's least size |
| `pacing.rs` | when the next frame comes (`Pacer`, `pacing::plan`): the 60 Hz grid, input under the same cap, a refused frame's slot, the hidden window's ticks, the host's own polls between frames |
| `shell.rs` | the window's state between frames (`Shell`), what it takes from the host's events (`Shell::events`) and what it does with a frame's `Intent`s: commands applied, refusals and notes to the toast line, popovers opened and closed, Undo, stars and a rescan through the host, editors queued and then opened (`open_editors`) |
| `ui.rs` | what a frame draws and senses: the header and its display, the banner, the console, the open popover, the toast line |
| `console.rs` | the console row: where each strip goes, the rects a popover hangs from and a drag lands in, the insert drag resolved after the last strip |
| `strip.rs` | a channel strip and a mix strip: rows, header, scale, readouts, mute, link and ear, the add-channel column, the text helpers |
| `fader.rs` | a fader: the meter bars and peak lines, the line and triangle, the drag, double-click and keys |
| `well.rs` | the FX well: MON and MST, the slots with their square, gear and bin, the mark of a slot playing as a pass-through, the slot keys, "+ Add plugin", the scrolling list |
| `drag.rs` | an insert dragged within or between strips: where it lands, the drop line and outline, the ghost, the command a drop sends |
| `popover.rs` | the popovers: the source panel, the output panel, the add-channel menu, the menu and the preferences; where a popover goes |
| `browser.rs` | the plugin browser: the search, the nav and its counts, the list grouped A to Z, stars, the footer; the list and the nav kept between frames, and only the rows in view drawn |
| `toast.rs` | the toast line, and the Undo command a toast carries |
| `editors.rs` | the plugin editor windows, made by an `Opener` and driven through `EditorWindow` (winit's in the app, a fake in the tests): open, resize, close through the host, titles |
| `widgets.rs` | what every surface shares: hover, the focus ring, the text button, tick box and radio, headings, the text field, a popover's frame |
| `meters.rs` | a linear peak read on the fader's taper, the peak-hold line |
| `icons.rs` | prototype v5's SVG icons, read once and followed with egui's painter, stroked or filled |
| `theme.rs` | the look as constants: colours, gradient, sizes (`size`, `pop`), fonts and their sizes |
| `gpu.rs` | the surface, and the one paint path: `upload` (egui's textures) and `encode` (egui's frame into a target) |
| `capture.rs` | `--dump-png`: one frame offscreen, read back, written as a PNG |
| `test_support.rs` | tests only: the headless `Harness`, the `Recorder` host (which can open editors and answer `tick_editors`), and `FakeWindows`, editor windows with no event loop |
| `acceptance.rs` | tests only: every gesture the handoff lists, with the command it sends |
| `assets/fonts/` | IBM Plex Sans Condensed (400, 500, 600) and IBM Plex Mono (400, 500), with `OFL.txt`, the SIL Open Font License they come under |

## The shell

`main.rs` parses the command line and runs the winit event loop; `app.rs` is the
`ApplicationHandler`. On `resumed` it boots the host, makes the window (titled "Vix Mix",
dark, at the console's natural size), the surface and the egui context. On `RedrawRequested`
it draws a frame, or, while the window is hidden, serves the host without one; the pacer's
timer wakes either ([Pacing](#pacing)). Events for an editor's window go to its editor
([Editor windows](#editor-windows)). On `CloseRequested` it exits; on `exiting` every editor closes
through the host (`Running::close_editors`, one call a shutdown order can place), then the
host goes (its instances after their editors), then the window.
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

`Running::frame`, while the window can be seen ([Pacing](#pacing)), in order: the host's
events (`Shell::events`: the backend's state, a toast, a removal with its Undo, a finished
rescan, an editor the host closed); the host's
meters into the window's `Meters`; the open editors served (`EditorWindows::tick`); then egui
runs `ui::draw`, which reads the host and the shell and returns the frame's `Intent`s;
`Shell::dispatch` carries them out against the host; the editors it queued open in windows of
their own (`Shell::open_editors`, each refusal to the toast line); open editors follow their
insert (a new title when it moved or its strip was renamed, closed when it left the setup); the
window's least size follows the content. egui runs one pass a frame, so a gesture is sensed
once and its command sent once.

**egui's viewport facts** (where the window is, its monitor, its title, whether it is
minimised, maximised or focused) are asked of the desktop when one of the window's events may
have changed them (resized, moved, focused, occluded, a new scale) and otherwise at most every
`VIEWPORT_CHECK` (500 ms), and kept in egui-winit's input between frames. egui-winit's
`update_viewport_info` asks each one, and the frame used to call it every frame: on X11 that is
a round trip to the X server per fact; on Windows, taking the input with the facts took 0.07
to 0.1 ms of every frame.

**An `Intent`** is what a gesture asks: `Apply(Command)`, `OpenEditor(instance)`,
`Toggle(anchor)` (open the popover that hangs from it, or close it), `AddChannel(kind)`,
`Undo`, `Favorite { id, on }`, `Rescan`, `Notice(message)` (a control that refused something
itself). A frame never changes the setup it is drawing; the headless tests drive the same
`draw` and `dispatch` the window does.

**`Shell::dispatch`** applies each command through `Host::apply`. A refusal goes to the toast
line as the model words it (each says what to do). The editors of the inserts a command
removes close through the host first. A change earns its note: "Moved" or "Copied <plugin> to
<strip>" for a drag between strips or a copy, "Added <plugin> to <strip>", "Monitor output now
plays Master" and "Monitor output back to the Monitor mix" (2.4 s). A star goes to
`Host::set_favorite` (the host keeps it in `plugins.ron`), Rescan plugins to
`Host::rescan_plugins`.

**`Shell::events`** takes what one poll of the host reported: the backend's state (the
banner), a `Toast` (8 s; it says what to do), a `Removed { message, undo }` (the host's words
with Undo, [Toasts and Undo](#toasts-and-undo)), `PluginsRescanned` ("Rescanned the plugin
folders: N plugins", only when the host said nothing of its own in the same poll, so a warning
about the rescan stays on the line), `EditorClosed`. The setup, the apps, the devices and the
inserts playing as pass-throughs the next frame reads from the host.

**One clock.** The window's clock counts seconds from when it opened (`Running::opened`). Each
frame gives it to egui as the input's time (egui-winit's own clock starts with its state), so
`Shell::now` is the frame's; and each poll of the host passes it to `Shell::events` as the
moment the messages came, and they are stamped with it. A message shows for its whole time
from when it arrived, however long the last frame was before it: the window draws nothing
while the mixer is idle or the daemon is gone, and `Host::rescan_plugins` holds the window's
thread while it describes new bundles.

## Pacing

`pacing.rs`'s `Pacer` says what comes next, as a plain function of what the last frame left
(`pacing::plan`, pinned by its tests), and `app.rs` sleeps the loop until then
(`ControlFlow::WaitUntil`, or `Wait` when nothing is due): a frame, or a serve of the host
without one (the hidden window's tick, or the host's own poll).

- **Seen**: a frame when egui wants one (input, or its `repaint_delay` coming due: a tooltip,
  the toast's fade, a text cursor), straight after a frame that acted or opened or closed a
  popover (what changed shows at once, even with no backend), on a 60 Hz grid while the host's
  `BackendState::live()` (running or simulated) or a `--quit-after-frames` count runs, and at
  least every `EDITOR_POLL` (33 ms) while a plugin editor is open, which serves the editors. A
  frame's slot is when it began, or when it was due if the timer woke it within an interval of
  that, so neither drawing nor the timer's lateness stretches the grid. No frame, input's
  included, comes sooner than `FRAME_INTERVAL` (16.667 ms) after the last slot, so the window
  never draws more than 60 frames a second.
- **Refused**: a frame the surface does not take (lost, outdated, occluded, timed out) still
  takes its slot, so the next waits its interval rather than coming at once, and is tried
  again so what it held reaches the screen; after three refusals in a row the surface is tried
  every `HIDDEN_POLL` (250 ms). Only a frame that reached the screen counts towards
  `--quit-after-frames`.
- **Hidden**: minimised (Windows sends a 0 × 0 `Resized`; the desktop's `is_minimized`, asked
  with the viewport facts) or `Occluded`, the window draws nothing, as the prototype's
  `requestAnimationFrame` stops. The host's events and the open editors are still served,
  without a frame: every `EDITOR_POLL` while an editor is open (its own window may be in view),
  every `HIDDEN_POLL` while the meters would move, so the host's bounded queues keep draining,
  otherwise not until an event. Input does not wake it. Each hidden tick asks the desktop again
  whether the window is minimised, so a window back in view with no event that says so (an
  X11 window under a compositor) draws within a tick; one whose event says so draws at once.
- **The host's own poll** (`Host::next_poll`: PipeWire's news, the debounced save, a removed
  instance's undo running out): every frame serves the host; between frames, seen or hidden,
  the host is served when it asks, never sooner than `FRAME_INTERVAL` after it was last asked
  (so a host that asks at once cannot spin the loop), with no frame unless it brought news,
  and then a frame as soon as the 60 Hz cap allows. A poll never puts off a frame that is due.
  `FakeHost` asks for none: its meters are live, so its frames poll it.
- With no backend, no editor, nothing to animate and nothing the host waits for, it is idle.

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
- **The banner**, the host's `BackendState::banner()` when it has one, a hairline in the
  mute colour round it: "No audio backend on this platform" on Windows; on Linux PipeWire's
  refusal (`Absent`), the mixer connected but idle with what would drive it (`Idle`), or the
  daemon gone and being reconnected (`Disconnected`). The host's messages go to the toast
  line, and an insert playing as a pass-through is marked on its slot, not in the banner.
- **The console**, at the top left of what is left, at its natural size (`console::size`:
  900 × 454 for six channels): black, with each channel strip in setup order, the add-channel
  column (a plus over "ADD CHANNEL" set vertically, as `writing-mode: vertical-rl` sets it)
  while fewer than eight channels exist, then Monitor and Master.
- **The open popover** over the console, and **the toast line** over everything.

A **channel strip**: the header (the mic icon for a hardware input, four squares for an app
channel, the name in 600 cut with an ellipsis when long, the live dot green for a hardware
input or an app channel with a running app, else faint); the FX well (FX, MON and MST lit in
the accent when the chain feeds that mix, each insert with its bypass square, filled when on,
its name faint when bypassed, then "+ Add plugin"; an insert the host plays as a pass-through,
its plugin missing or not set up (`Host::problems`), has its square outlined and its name in
the mute colour, and says the host's why and what to do under the pointer); two faders with the scale between (`+6, 0,
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
cap on plugins per strip); a chain that fits is laid out without a scroll area, which would
cost every frame for nothing. A slot under the pointer, or with the focus inside it, shows its
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

A panel opens with the focus on its first control, as the prototype's `openSource` puts it: the
name field of a channel just added, an app channel's search field, or the checked radio of a
list of inputs or outputs (the first row when none is checked). A list of radios (the inputs,
the outputs, the quanta) answers the keys as a browser's radio group does: Up and Down move to
the row before or after, round the ends, and pick it (the command a press of it sends); Space
picks the focused row; the arrows stay in the group rather than moving the focus to the console
under the popover.

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
- **The menu**, under the menu button: Preferences, and Rescan plugins
  (`Host::rescan_plugins`, the menu closing; the toast line says what the catalog holds after,
  unless the host said something of its own about it).
- **The preferences** (270 wide): the quantum asked of PipeWire as radios (128, 256, 512
  samples; `SetPrefs`), the extra plugin folders each with its bin, a field and Add for
  another (`SetPrefs` with the folder added or removed, the rest kept; a folder takes at most
  1024 characters), Done. It holds at most 16 folders (`MAX_PLUGIN_DIRS`): an Add past that
  sends nothing, keeps the folder in the field, and says on the toast line to remove one first.

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
keeps its own order, the rest are by name. A row's star toggles the favourite through
`Host::set_favorite`, which keeps it (the starred star filled in the accent); a press on the row, or Enter, sends `AddInsert { strip, plugin,
at: len }` (the plugin as the catalog names it: id, bundle, name) and closes the browser. The
footer says where it goes: "Adds to **<strip>**, slot N". An empty list says "No installed
plugin matches “…”."

**A real catalog has hundreds of plugins** (this machine's: 915 from 322 bundles), so the
browser does not build its list every frame. The list (as indexes into the catalog's plugins)
and the nav are kept from frame to frame and built again only when the query or the filter
changes, or the catalog does: a rescan (`HostEvent::PluginsRescanned`, which `Shell::events`
passes on with `Browser::forget`), a star (the favourites' count), an add (the recent list),
with the count of plugins kept as a guard. Each name is lower-cased once for the sort, not at
every comparison. In the nav and the list only the rows in view, and those within
`pop::LIST_REACH` (a row and a heading) past each edge, are drawn and sensed; the rest only
take their space. A row or nav item that takes the focus scrolls into view at once, so Tab
walks the whole list, out of view included; Up and Down scroll the picked row in as before.
Timed with that catalog in the headless harness (`ui::draw` and `Shell::dispatch`, on Linux),
a frame with the browser open went from 3.15 ms to 0.48 ms in a debug build (1.14 to 0.16 in
release), against 0.23 (0.08) with it closed; a keystroke, which builds the list again, 0.94
(0.41).

## Toasts and Undo

`toast.rs`: one message at a time at the bottom of the window, centred, on the control face
with a black outline, fading in and out over 0.15 s; a new message takes the old one's place,
and the old one's Undo with it. A message wider than the window wraps; each is laid out before
it is placed, so it is centred from its first frame.

Removing an insert or a channel shows the host's words ("Removed Pro-C 2 from Mic", "Removed
channel Music": `HostEvent::Removed`) with Undo for the host's `UNDO_TOAST` (5 s, the
prototype's). Undo applies the one command the host handed out with it: `RestoreChannel { at,
channel }` puts the channel back at its place with its ids, chain and instances,
`RestoreInsert { to, insert }` the insert in its slot, bypassed if it was; the host lends the
instance it kept, its state and a reverb's tail included. A refusal (the console is full again,
the strip it belonged to has gone) goes to the toast line in the model's words. The Undo dies
with its toast; the host keeps the removed instances a little longer than the toast shows.

## Editor windows

`editors.rs`, Vix's `open_editor` and `EditorWindow` as the model: one winit window per open
editor, which the plugin draws into itself (egui never does), titled "<plugin> · <strip> ·
slot N".

- **Open** (the gear, a double-click, Enter): a window is made hidden and fixed-size, its raw
  handle goes to `Host::open_editor`, and the window takes the size the plugin asks for and
  shows. Opening an editor that is open raises its window. A refusal (no editor, `FakeHost`'s
  "run with the real host to open a plugin's editor", a Wayland handle on Linux) drops the
  window and goes to the toast line. At most 16 are open at once.
- **Resize**: `Host::tick_editors`, every frame or hidden tick while one is open (at least
  every 33 ms), names the instance that asked; its window takes the size. (Telling the plugin
  its new size waits for the host; see below.)
- **Close**: always through `Host::close_editor` before the window goes — the window's close
  button, the insert's removal (the shell closes it before the command), the insert leaving the
  setup any other way, the window closing at exit — and an editor the host closed itself
  (`HostEvent::EditorClosed`) loses its window.
- Each opens, resizes and closes with an `info` log line, which a by-hand pass reads.
- **Made by an `Opener`, driven through `EditorWindow`**: the event loop makes winit windows in
  the app, and `EditorWindow` is what the editors ask of one (its id, its raw handle, a size,
  show, a title, raise), so the tests open, resize, follow and close editors with no event loop
  (`test_support::FakeWindows`, against a `Recorder` that opens editors as a real host does).

## What the window needs from the host

The window codes against the `Host` trait as it is. Two things wait, proposed as the build
map's change request 10: `resize_editor(instance, size)` on the host (the kit's resize is
asynchronous: after the window has taken the size a plugin asked for, `EditorSession::resize`
must tell the plugin, and today nothing does), and a bound on the model's extra plugin folders
(`SetPrefs` takes any number; the window holds at most 16 and says to remove one first).

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
`ui::draw` and `Shell::dispatch`, as the window runs them, the host polled before each frame
into `Shell::events`, with every call to the host recorded; each test asserts the command its
gesture sent):

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
  `the_browser_nav_filters_and_a_star_toggles_a_favourite` (the star reaches
  `Host::set_favorite`), `the_browser_s_arrows_reach_a_row_out_of_view_and_enter_adds_it`
  (Down to the last row, which scrolls in; Enter sends its `AddInsert`),
  `tab_walks_the_browser_s_list_into_rows_out_of_view` (Tab from the search field reaches the
  last row, and it scrolls in), `an_open_browser_lists_what_a_rescan_found` (a plugin renamed
  by a rescan, as many as before, takes its new place A to Z);
- the source panel: `an_app_ticked_in_one_channel_leaves_another`,
  `an_app_search_that_finds_nothing_says_so`, `a_hardware_channel_picks_its_input`,
  `the_name_field_renames_once_when_the_panel_closes`;
  the output panel: `the_output_panel_picks_a_device_and_master_can_have_none`;
- remove and Undo: `removing_an_insert_shows_undo_and_undo_puts_it_back` (its editor closed
  through the host first; Undo sends the host's `RestoreInsert`, the same insert back),
  `a_bypassed_insert_comes_back_bypassed_and_undo_dies_with_its_toast`,
  `removing_a_channel_shows_undo_and_undo_brings_it_back` (`RestoreChannel`, back at its
  place), `an_undo_the_host_refuses_says_why_on_the_toast_line`;
- the rest: `a_press_outside_or_escape_closes_a_popover`,
  `the_add_column_adds_a_channel_and_opens_its_panel_to_name_it`,
  `the_gear_or_a_double_click_asks_for_the_insert_s_editor`,
  `a_refusal_reaches_the_toast_line_with_its_fix`, `a_long_chain_scrolls_in_its_well`,
  `the_preferences_ask_for_a_quantum_and_take_plugin_folders` (and Rescan plugins asks the
  host, says what the catalog holds, and leaves a warning of the host's on the line),
  `the_preferences_hold_at_most_sixteen_plugin_folders`,
  `the_window_keeps_room_for_the_header_over_a_narrow_console`,
  `an_editor_the_host_refuses_says_why_on_the_toast_line_and_keeps_no_window` (`FakeHost`'s
  refusal, word for word),
  `a_panel_opens_with_the_focus_on_its_checked_radio_and_the_arrows_pick` (an input panel and
  Master's output panel: Down, round the end, Space, Up to "No device"),
  `an_insert_playing_as_a_pass_through_is_marked_and_says_why`,
  `the_banner_says_when_the_mixer_is_idle_or_the_daemon_is_gone`;
- the clock: `a_host_message_after_a_quiet_spell_shows_for_its_whole_time` (a host's toast
  polled 20 s after the last frame shows, still shows 7 s on, and is gone after its 8 s),
  `a_rescan_that_holds_the_window_still_says_what_it_found` (a rescan answered 5 s after the
  frame that asked still shows its note, for its 2.4 s from the answer).

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
  counts favourites, recent, types and makers in order, and All plugins sort by name, case
  aside; All plugins group by their first letter.
- `toast.rs`: a toast shows for its time and its Undo goes with it.
- `editors.rs`: an editor window is titled plugin, strip and slot; it opens hidden at 640 ×
  480, takes the size its plugin asks for and shows, and opened again is raised; a refusal
  (the host's, no window, no handle, an insert gone) keeps no window and says what to do; at
  most sixteen open; a resize request goes to the window of the editor that asked; its close
  button closes the editor through the host first; one the host closed loses its window without
  a second close; renamed with its strip, and closed through the host once its insert leaves
  the setup; at exit each closes through the host in the order it opened.
- `pacing.rs`: a seen window with moving meters draws on a 60 Hz grid (a late timer counts from
  when it was due); a refused frame takes its slot, so the next waits an interval; a surface
  that keeps refusing is tried four times a second; a hidden window draws nothing and serves
  the host slowly (an editor's rate while one is open; nothing at all with nothing to serve;
  input does not wake it); minimised, zero-sized or covered is hidden; an idle window waits,
  and egui or an act wakes it no faster than 60 Hz; input draws as soon as the cap allows;
  an idle window serves the host when it asks and draws only for news, a host that asks at
  once waiting an interval; a poll never puts off a frame, and news waits for the cap (and
  draws nothing hidden); a hidden window serves the host when it asks sooner than its tick.
- `meters.rs`: a level reads on the fader's taper; a peak holds 900 ms then falls 0.006 a
  frame to the level; the console's meters follow the host's frame by position.
- `ui.rs`, headless frames of `ui::draw` that press nothing and ask nothing: the header names
  the app and shows the display; the display says kHz without a needless decimal; the banner
  shows for no backend and not while the mixer runs (Idle and Disconnected are the
  acceptance's); the console draws the example's strips, inserts,
  readouts, scales and the add column; a muted send reads struck through and a bypassed insert
  faint; MON and MST light when the chain feeds that mix; previewing Master tags Monitor.
- `capture.rs`: read-back rows lose their padding; the PNG holds the pixels as drawn; an
  unwritable path says what to check.

**By hand** (a GPU and a desktop, so not in CI):

- `cargo run -p vix-mix -- --fake --quit-after-frames 600` on Windows logs "drew 600 frames
  in 9.99 s after the first (60.0 a second)". With FIFO present the same machine drew 39 to 57
  frames a second, its acquire stalling up to 80 ms; mailbox holds 60.
- **CPU**, `--fake` at 60 frames a second on an RTX 3090 with 24 logical cores: the process's
  CPU time over 20 s after 4 s of warm-up, read per thread from PowerShell's `Get-Process`
  (all of it is the main thread's), debug and release run in turn four times each on the
  rebased branch. Of one core: 5.5 to 11.0 % in the debug build `cargo run` makes (6.4 the
  middle of the runs) and 3.8 to 6.3 % in a release build (4.9 the middle), on Vulkan; the
  branch's verifier measured 5.5 to 7.3 % and 4.4 to 5.2 % in the same way. Of the machine:
  0.16 to 0.46 %. The figure moves with what else the machine runs, by more than the
  difference between the builds. A frame, timed in place in a debug build (release beside
  it): the console's drawing in egui's pass 0.38 ms (0.16), egui's own share of the pass 0.05
  (0.04), tessellating the whole console 0.24 (0.2), encoding and upload 0.09, the surface's
  acquire 0.05, the driver's submit and present 0.45 (0.43, most of it CPU on the window's
  thread). An empty egui frame presented at 60 Hz costs 2.1 % of one core by itself. Tried and
  not kept: tessellating each icon's strokes once and reusing the mesh, moved into place or
  kept where it is drawn; side by side with the strokes tessellated every frame the frame's
  phases did not move (strokes this thin tessellate about as fast as a mesh is copied), so the
  icons stay as they are. Minimised, 0.00 %: no frame, the host served four times a second.
- Minimised and restored (`ShowWindowAsync` from PowerShell, with the window's handle taken
  before it was minimised): no frame while minimised, drawing again at once when restored.
- `cargo run -p vix-mix -- --quit-after-frames 3` runs on the real host and exits 0.
- The capture command above writes the 1800 × 976 PNG, byte for byte the look gate's (again
  on the branch rebased onto lane 06's host).
- An editor: a scratch profile (`CHIMAERA_CONFIG_DIR`) with Goodhertz's Tone Control 3 on Mic
  opened it in a window titled "Ghz Tone Control 3 · Mic · slot 1" at the 552 × 404 it asked
  for, drawn by the plugin, and closed it through the host when its window closed (again
  after the windows moved behind `Opener` and `EditorWindow`: client area 552 × 404, "closed
  the editor of Ghz Tone Control 3 · Mic · slot 1" on the window's close message).
- The look itself was the user's verdict on the PNG beside the screenshot, not a pixel diff.
