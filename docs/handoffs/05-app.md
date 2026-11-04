# 05 — App

**Record — look gate rendered 2026-11-04, awaiting the user's verdict; steps 2–9 not started.**
Step 1 is built: the theme as named constants (`app/src/theme.rs`: every colour, the meter
gradient, the corners, the strip and header sizes, the font sizes), IBM Plex Sans Condensed
400/500/600 and Plex Mono 400/500 embedded with their OFL licence (`app/assets/fonts/`), the
console drawn static from `FakeHost` (`strip.rs`, `console.rs`, `meters.rs`, `icons.rs`,
`ui.rs`), and the headless capture `--dump-png` (`capture.rs`), whose output is
[egui-look-gate.png](../design/prototype/egui-look-gate.png) beside
[screenshot.png](../design/prototype/screenshot.png). `cargo run -p vix-mix -- --fake
--quit-after-frames 3` still exits 0. [reference/app.md](../reference/app.md) says what is
built. `crates/vixmix-host/src/fake.rs` is unchanged: the example data already held everything
the strips show. Deviations, each with its reason:

- **The capture is the whole console, not one strip of each.** Every channel strip is the
  one `strip::channel` and both mixes the one `strip::mix`, so six and two cost nothing more,
  and the screenshot is the whole console: like beside like is what the user judges. The
  header's display and the add-channel column (steps 6 and 3) are drawn too, static, for the
  same reason; nothing senses the pointer.
- **`--dump-png`, not `egui_kittest`.** The flag draws through the window's own paint path
  (`gpu::upload`, `gpu::encode`, now shared), so the PNG is what the window shows; it adds
  only `png`, already in the lock; and `cargo test` stays free of a GPU, which neither CI leg
  has. It is deterministic (`FakeHost` is seeded): the same adapter draws the same PNG.
- **The peak-hold line is built now** (step 3's `MeterFader` holds 900 ms, then falls 0.006
  a frame), as window state in `meters.rs`, because the peak lines are part of the look; the
  fader's gestures are not.
- **Icons are the prototype's SVG paths followed with egui's painter** (`icons.rs`), because
  egui draws no SVG and a rasterizing crate would add a dependency for fourteen strokes.
- **Type.** Sans 500 is embedded as listed but prototype v5 sets nothing in it (its 500s are
  all mono). The MST tag's mono 600 is drawn in mono 500, the heaviest mono embedded.
- **The header** says "Vix Mix" (the charter's name), not the prototype's "Mixer", draws no
  in-app window buttons (step 6: the desktop's decorations), and the display is the
  prototype's cells (`48 kHz`, `256 samples`, `5.3 ms`, `DSP` with its bar) instead of lane
  00's one line; its test changed with it. The window has no 1-point border of its own.

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
  under 5 % CPU.
- `reference/app.md` complete; `dev check` green.

## Out of scope

Anything the host does (06); the editor's plugin behaviour (04).
