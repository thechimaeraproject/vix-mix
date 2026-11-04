# App (`vix-mix`)
**Owns:** `app/`

**Reference — the window as lane 00 wrote it; [handoff 05](../handoffs/05-app.md) builds the
console and completes this page.** `app/` is the `vix-mix` binary: a winit window with egui
drawn through egui-winit and egui-wgpu — Vix's shell
([`app/src/main.rs`](https://github.com/thechimaeraproject/Vix/blob/master/app/src/main.rs),
[`app/src/gpu.rs`](https://github.com/thechimaeraproject/Vix/blob/master/app/src/gpu.rs)), not
eframe, because each VST3 editor later needs a winit window of its own and its raw handle. It
drives one `Host` (`RealHost`, or `FakeHost` with `--fake`) and owns nothing the host could:
no setup, no audio, no plugin. It depends on `vixmix-host` and `vixmix-model` only.

## The shell

`main.rs` parses the command line and runs the winit event loop; `app.rs` is the
`ApplicationHandler`: on `resumed` it boots the host, makes the window (titled "Vix Mix", dark,
1280 × 720, at least 480 × 320), the surface and the egui context; on `RedrawRequested` it
draws a frame; on `CloseRequested` it exits; on `exiting` it drops the host before the window,
so editors close first. `gpu.rs` is the surface (a non-sRGB format, as egui-wgpu prefers, FIFO
present) and the egui renderer, cleared to the prototype's window colour (`#131416`); a lost
or outdated surface is reconfigured and the frame skipped.

## Pacing

The loop is `ControlFlow::Wait`. A frame is drawn when egui wants one for input, when egui's
`repaint_delay` comes due, and — while the host's `BackendState::live()` (running or simulated)
or a `--quit-after-frames` count is running — every `FRAME_INTERVAL` (16.667 ms). The next
frame is never scheduled sooner than one interval after the last, so the window repaints at
most 60 times a second; with no backend and nothing animating it is idle.

## The command line

For checks, not users: `--fake` runs against `FakeHost`; `--quit-after-frames N` (N ≥ 1)
closes the window once N frames have reached the screen and exits 0, which is the headless
proof that the window works (`cargo run -p vix-mix -- --quit-after-frames 3`). Anything else is
refused with exit 2 and what to write instead.

## What a frame draws today

`ui.rs`: the 34-pixel header on the strip colour with "Vix Mix" and the display's numbers
(`48.0 kHz · 256 · 5.3 ms · DSP 7%`), and — when the host's backend is `Absent` — a banner with
its reason ("No audio backend on this platform" on Windows; the not-built message on Linux
until handoff 03). Below, one line counting the channels. The console, the theme and fonts,
the widgets, popovers, the browser and editor windows are handoff 05's; the look gate comes
first.

## Verification

`cargo test -p vix-mix`: the flags parse and every refusal names what to write instead; a
headless egui frame of `ui::draw` contains "Vix Mix" and the display's numbers, and the
banner appears exactly when the backend is absent. By hand (and in lane 00's record):
`cargo run -p vix-mix -- --quit-after-frames 3` exits 0 on Windows, logging the adapter and
"drew 3 frames".
