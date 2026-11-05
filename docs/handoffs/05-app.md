# 05 — App

**Record — shipped 2026-11-04.** The look gate passed: the user judged
[egui-look-gate.png](../design/prototype/egui-look-gate.png) beside the prototype's
[screenshot.png](../design/prototype/screenshot.png) — "looks great, somehow better than the
reference" — so egui stays and Slint is not needed. Then the console became the prototype's,
interactive, over the `Host` trait: every gesture the acceptance lists sends its command
(asserted headless in `app/src/acceptance.rs` on both CI legs), the popovers, the plugin
browser, the toasts with Undo, the header's menu and preferences, editor windows (proven by
hand on Windows with Goodhertz's Tone Control 3, before and after they moved behind a trait the
tests fake), 60 Hz pacing while the window can be seen and no frame while it is hidden (600
frames in 9.99 s on `FakeHost`; 0.00 % of a core minimised). The CPU acceptance waits on the
user's reading of "5 % CPU": see its deviation below. The branch is rebased onto master with
lane 06's host, and the window takes what the trait gained there: the removal's Undo is the
host's `RestoreChannel`/`RestoreInsert`, the browser's star and the menu's Rescan plugins go
to the host, an insert playing as a pass-through is marked on its slot, the banner shows the
mixer idle and the daemon gone, and the pacing serves the host when it asks between frames
(lane 06's banner-line stopgap, its toast and problem fallback, gave way to the window's
toasts and slot marks). [reference/app.md](../reference/app.md) is the contract as built.
Nothing outside `app/` changed: `crates/vixmix-host/src/fake.rs` is unchanged, because the
example data held everything the window shows. The build map's change request 10 proposes
the two things the window still needs (an editor told its size, the model's bound on plugin
folders). Deviations, each with its reason:

- **The capture is the whole console, not one strip of each.** Every channel strip is the
  one `strip::channel` and both mixes the one `strip::mix`, so six and two cost nothing more,
  and the screenshot is the whole console: like beside like is what the user judged.
- **`--dump-png` and egui's own headless run, not `egui_kittest`.** The flag draws through
  the window's own paint path (`gpu::upload`, `gpu::encode`), so the PNG is what the window
  shows; the gesture tests run `ui::draw` and `Shell::dispatch` in a plain `egui::Context`
  (Vix's `test_support.rs` pattern), which needs no GPU and no new dependency. With the
  gestures built the capture still draws the look gate's PNG byte for byte.
- **The peak-hold line was built at the look gate**, as window state in `meters.rs`, because
  the peak lines are part of the look.
- **Icons are the prototype's SVG paths followed with egui's painter** (`icons.rs`), because
  egui draws no SVG and a rasterizing crate would add a dependency for fourteen strokes; each
  is read once and kept (the look gate's verifier found them re-read every frame).
- **Type.** Sans 500 is embedded as listed but prototype v5 sets nothing in it (its 500s are
  all mono). The MST tag's mono 600 is drawn in mono 500, the heaviest mono embedded.
- **The header** says "Vix Mix" (the charter's name), not the prototype's "Mixer", draws no
  in-app window buttons (step 6: the desktop's decorations), and the display is the
  prototype's cells. Its menu button opens a menu (Preferences, Rescan plugins) where the
  prototype showed a toast naming them; the preferences panel (the quantum, at most 16 extra
  plugin folders, because every list is bounded) is drawn in the source panel's style, the
  prototype having none.
- **A rename is sent once, when the source panel closes**, not on every keystroke as the
  prototype's input handler renames: a rename re-describes the channel's PipeWire node, and
  the header already follows the field as it is typed.
- **The removal toast says the host's words** ("Removed channel Music", where the prototype
  says "Removed Music"): `HostEvent::Removed` carries the message and its Undo, and the host
  owns both, so a channel removed by any means reads the same.
- **An editor's window takes the size its plugin asks for without the plugin being told it
  has it** (change request 10, proposed): the trait has no `resize_editor`, and the kit's
  resize is asynchronous.
- **The browser lists the types the catalog has**, in the prototype's order, and searches with
  its own ranking (the prototype's, as `Catalog::search` has it): the app depends on the host
  and the model only, so it cannot name `PluginKind::ALL` or `Filter`. The example catalog has
  all ten types. Import chain and Export chain are not drawn: chain import and export is named
  in the charter's Open list, not built.
- **The window opens at the console's size**, not lane 00's 1280 × 720, as the prototype's
  app box is the console's, and its least size follows the content (the banner, the header's
  display beside the title on a one-channel console).
- **Pacing is a 60 Hz grid while the window can be seen, presented by mailbox**: lane 00's
  rule timed the next frame from the end of the last, and measured 54 frames a second; frames
  now count from start to start (a timer frame from when it was due), and input frames wait
  for the same cap. Lane 00's FIFO present stalled for up to 80 ms behind Windows' compositor
  (39 to 57 frames a second), so the surface presents by mailbox where it offers it, FIFO
  otherwise; the pacing, not vsync, holds the rate. Step 8's "while the window is visible" is
  the `Pacer`'s: minimised, zero-sized or covered the window draws nothing and serves the host
  four times a second (an editor's rate while one is open), and a frame the surface refuses
  still takes its slot, so a refusing surface cannot spin the loop. Between frames the host is
  served when it asks (`Host::next_poll`, lane 06's), at most 60 times a second and with a
  frame only for news. egui's viewport facts are asked of the desktop when an event may have
  changed them, not every frame as egui-winit's `update_viewport_info` would (a round trip
  each on X11).
- **The CPU acceptance, "under 5 % CPU", has two readings, and the user has not said which.**
  Measured on the rebased branch on the 24-core machine, `--fake` at 60 frames a second, debug
  and release run in turn four times each (20 s after 4 s of warm-up, the process's CPU time,
  all of it on the window's thread): of the machine, 0.16 to 0.46 %, more than ten times
  under; of one core, 3.8 to 6.3 % in a release build (4.9 the middle of the runs; the
  branch's verifier measured 4.4 to 5.2 %) and 5.5 to 11.0 % in the debug build `cargo run`
  makes (6.4 the middle; the verifier 5.5 to 7.3 %). So per core neither build is reliably
  under 5 % on this machine: release straddles it and debug is over. The machine's other work
  moves the figure by more than the difference between the builds. A frame, timed in place in
  the debug build (release in brackets): the console's drawing 0.38 ms (0.16), egui's share of
  its pass 0.05 (0.04), tessellating the whole console 0.24 (0.2), encoding and upload 0.09,
  the surface's acquire 0.05, the driver's submit and present 0.45 (0.43, most of it CPU on
  the window's thread); an empty egui frame presented at 60 Hz costs 2.1 % of a core by itself.
  Tried and not kept, because side by side it moved none of those phases: each icon's strokes
  tessellated once and the mesh reused, moved into place or kept where it is drawn (thin
  strokes tessellate about as fast as a mesh is copied; the record's earlier estimate of 1 % of
  a core for it was wrong). Earlier cuts with no difference measurable over the noise on
  Windows: egui's viewport facts asked only when they may have changed (which matters on X11,
  a round trip each) and no scroll area for a well whose chain fits. What would bring a frame
  under 5 % of a core on either build is not built, because it is the user's to choose: a
  frame for the meters alone when nothing else changed, skipping egui's pass and keeping the
  rest of the console's tessellation (about 0.6 ms of the debug build's 1.1, but a second
  drawing path that must know everything that can change), or meters at a lower rate than
  60 Hz. Under the per-machine reading the acceptance is met as it stands. For comparison, the
  user's Wave Link took 18.3 % of one core over the same kind of sample.
- **What the prototype does not have**: an input or output that is in use but absent is listed
  first, marked "not connected"; a new hardware channel starts with no input picked (the
  model's `AddChannel`; the prototype picked one); the keyboard focus ring follows a Tab, for
  every control; popovers show without egui's fade, but each kind's first appearance is drawn
  unseen for one frame (egui sizes a new area first); an insert the host plays as a
  pass-through (its plugin missing or not set up) has its square outlined and its name in the
  mute colour, and says why and what to do under the pointer; the menu's Rescan plugins says
  on the toast line what the catalog holds after it.
- **Dependencies**: the app names the kit's `EditorEvent` and `EditorSize` (what
  `Host::tick_editors` and `Host::open_editor` return), so it takes
  `chimaeraproject-vst3-host` directly, already built for the host at the kit's one tag; its
  tests take `vixmix-plugins`, because their `Recorder` implements `Host`, whose `catalog`
  names `Catalog`. The lock gains two edges and no crate.
- **One sentence of lane 06's paragraph in CLAUDE.md § Status changed**, beyond this lane's own
  paragraph: "shows its notes in the banner line" became "on the toast line". Handoff 06 asks
  lane 05 to retire that stopgap ([its deviations](06-host-and-integration.md), "the banner
  line"): `Running::banner()`'s toast and problem fallback gave way to the window's toasts and
  slot marks, so the sentence would otherwise be false.
- **The browser keeps its list between frames and draws only the rows in view**, which the
  handoff does not ask for: with this machine's catalog (915 plugins) rebuilding and drawing
  the whole list cost 3.15 ms a frame in a debug build; kept, 0.48 (see
  [reference/app.md](../reference/app.md) § The plugin browser). Tab still walks every row: a
  row that takes the focus scrolls in, which the prototype's browser does by itself.

**Needs:** 00. **Runs alongside:** 01, 02, 03, 04. **Size:** XL. Starts on Windows against
`FakeHost`.

## Goal

The console exactly as prototype v5, as a real window over the `Host` trait.

## Read first

- [reference/app.md](../reference/app.md): the window as lane 00 left it. `app/` already has
  Vix's winit + egui-winit + egui-wgpu shell (`main.rs`, `app.rs`, `gpu.rs`), the 60 Hz pacing
  rule, `--fake` and `--quit-after-frames`, and `ui.rs` drawing the header and the no-backend
  banner.
- [reference/host.md](../reference/host.md) (the `Host` trait, `FakeHost`'s data and
  simulated meters, `MeterFrame` in linear peaks) and [reference/model.md](../reference/model.md)
  (every `Command`, the taper: `position_to_db`, `db_to_position`, `format_db`, `UNITY`).
- The prototype: `docs/design/prototype/index.html` (open it in a browser and read its CSS and
  script) and `docs/design/prototype/screenshot.png`; the [design pack](../design/README.md)'s
  control list.
- Vix (`A:\RustroverProjects\Vix`): [app/src/app.rs](https://github.com/thechimaeraproject/Vix/blob/master/app/src/app.rs)
  (`open_editor` and `EditorWindow`: the model for editor windows),
  [app/src/test_support.rs](https://github.com/thechimaeraproject/Vix/blob/master/app/src/test_support.rs)
  (headless egui tests that assert the command a gesture sends),
  [app/src/panels/mixer.rs](https://github.com/thechimaeraproject/Vix/blob/master/app/src/panels/mixer.rs)
  (Vix's fader and meter, which handoff 07 lifts into the kit's `egui-audio`), and
  [docs/reference/app.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/reference/app.md).
- The kit's [vst3-host.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)
  § The API (the editor) and § Decisions (Wayland handles are refused).
- [build-map.md](../build-map.md) § Conflict rules: you may edit
  `crates/vixmix-host/src/fake.rs` (only) while lane 06 has not started.

## What this lane owns, what others keep

It owns the window: theme, fonts, widgets, the console, popovers, the plugin browser, editor
windows, the header display, and the commands every gesture sends. The host owns what a command
does and everything behind it (06); the plugins crate owns the editor's plugin side (04).

## Steps

1. **The look gate, before any interaction work.** Theme, fonts, one static channel strip and
   one mix strip rendered from `FakeHost`'s example data, captured headless (an
   `egui_kittest` snapshot or a `--dump-png`), and put beside
   `docs/design/prototype/screenshot.png` for the user to judge. The gate is a review, not a
   pixel diff. If egui cannot get close, the user decides — keep going, or Slint — before the
   lane spends more. Stop and report at this step.
2. **Theme** (from the prototype's CSS): `win #131416, strip #212226 (mix #26272c),
   well #18191b, ctl #2c2d32, ctl-hi #383a40, line #2a2b2f, black #0c0d0e, text #eceef1,
   dim #9a9ea7, faint #62666e, accent #5aa8ff, mute #e5484d`; meter gradient stops
   `#2c9a55 0%, #4cc46d 48%, #9fcf43 64%, #e2b83a 76%, #e5484d 84%`; 2 px corners, no shadows
   except popovers; IBM Plex Sans Condensed 400/500/600 and IBM Plex Mono 400/500 embedded
   (OFL, its licence file beside them); strip 104 px (mix 112), rows 22/151/210/12/20 with 5 px
   gaps, padding 7 6 8; header 34 px.
3. **Widgets.** `MeterFader`: stereo bars with a peak-hold line that holds 900 ms then falls
   0.006 a frame; the fader line and triangle at the taper position; drag anywhere, Shift ×0.2,
   double-click → `UNITY`, ↑↓ 0.01 / Shift 0.002, Home/End; muted = faint line, bars at 40 %.
   The scale (+6, 0, 6, 12, 24, 48 at 1, .84, .70, .56, .36, .14). Readouts (mono,
   strikethrough when muted). Mute, link and ear buttons. The strip header (icon, name, live
   dot, caret on hover). The FX well (header FX plus MON/MST toggles on channels; slots with the
   bypass square; hover gear and trash; "+ Add plugin"). Insert drag and drop within and between
   strips with the ghost ("+ Copy" on Ctrl) and the drop line. The add-channel column with
   vertical text and its menu (App channel / Hardware input).
4. **Popovers.** The source panel: name field, hardware radios or the app checklist ("Playing
   now" / "Seen before", "in <other channel>" notes, a search field), Remove channel / Done.
   The mix panel: output radios, "No device" on Master, "Apps can also record Master as an
   input."
5. **The plugin browser** (620 × 440): search-first, ↑↓ Enter, the nav with counts for
   Favorites / Recent / All / Type / Maker, A–Z groups, star toggles, the footer "Adds to
   <strip>, slot N".
6. **Toasts with Undo** (5 s; remove channel, remove plugin — the host keeps the removed thing
   until the toast dies), the header display (rate, quantum, ms, a DSP bar) and the menu
   (Preferences, Rescan plugins). Native window decorations (KDE draws them; the prototype's
   in-app window buttons are not built).
7. **Editor windows.** A winit window per open editor (Vix's `app/src/app.rs::open_editor` is
   the model), titled "<plugin> · <strip> · slot N"; `Host::open_editor` with its handle;
   resize on `ResizeRequested` (`Host::tick_editors` names the instance); the Linux run loop is
   polled every tick by the host; closed through the host before the window. On Linux the
   event loop is created with the X11 backend (XWayland under KDE Wayland) because the kit
   refuses Wayland handles — record it in `reference/app.md` with the later option of a
   separate X11 connection for editors only.
8. **Repaint**: at most every 16 ms while the window is visible and the backend is running
   (lane 00's pacing does this from `BackendState::live()`); `Wait` otherwise.
9. **Docs.** Complete [reference/app.md](../reference/app.md).

## Files you own

`app/**`, `docs/reference/app.md`, this handoff, your rows in `docs/roadmap.md`, one paragraph
in `CLAUDE.md` § Status; `crates/vixmix-host/src/fake.rs` while lane 06 has not started. A new
dependency (`egui_kittest`, fonts) goes in `app/Cargo.toml`, and a workspace one in its group in
the root `Cargo.toml`.

## Acceptance

- The look gate passed by the user.
- Headless egui tests assert the `Command` each gesture sends (Vix's `test_support.rs`
  pattern) for: a fader drag with and without Shift and link, double-click to unity, the
  keyboard, mute, MON/MST, bypass, a drag within and between strips with and without Ctrl,
  browser search + Enter, an app tick that moves an app between channels, remove + undo.
- `cargo run -p vix-mix -- --fake` on Windows runs against `FakeHost` with simulated meters at 60 fps
  under 5 % CPU. *(Open: of the machine (met: 0.16 to 0.46 %) or of one core (release 3.8 to
  6.3 %, debug 5.5 to 11.0 %, not met) is the user's to say; the record above has the
  measurements and what each reading would need.)*
- `reference/app.md` complete; `dev check` green.

## Out of scope

Anything the host does (06); the editor's plugin behaviour (04).
