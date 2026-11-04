# Charter

**Settled 2026-11-03.** What Vix Mix is for, the decisions taken before the first line of code
with the reason for each, what was considered and rejected, and what is still open. A decision
here changes only by a new dated entry that says what it replaces and why; the
[build map](build-map.md) carries out what this page decides, and the
[roadmap](roadmap.md) says how far it has got.

## What it is for

A Wave Link class mixer for Linux ([the product definition](design/what-it-is.md)): up to eight
input channels, each a hardware input or a group of apps, each with its own VST3 chain and two
sends (Monitor, Master) with a level and a mute; two mixes with their own chains and output
devices; Master also published as a virtual microphone other apps record. Prototype v5
(`docs/design/prototype/index.html`) is the look.

It is the second consumer of the kit's Rust channel. The kit already carries what the mixer
would otherwise write: `chimaeraproject-fs` (atomic writes, the configuration directory),
`chimaeraproject-param`, `chimaeraproject-audio-processor` (the processor contract, the chain
with sparse sample-accurate parameter delivery, bus buffers, the strip maths) and
`chimaeraproject-vst3-host` (scan, load, process, state, presets, an editor over a raw window
handle). The kit's charter says what its shared crates may never own — **a thread model, a
window toolkit, an audio graph** — and those three are this repository's.
[Vix](https://github.com/thechimaeraproject/Vix) is the first consumer and the template for how
a kit-managed Rust repository is laid out.

## Decisions

| Decision | Answer | Why |
|---|---|---|
| Name | **vix-mix** (shown as "Vix Mix"): repository `thechimaeraproject/vix-mix`, binary `vix-mix`, kit manifest name `vix-mix`, configuration directory `vix-mix`, PipeWire `application.name` and node names `vix-mix.*`. **Crates are `vixmix-<name>`** (`vixmix_<name>` as paths), so they never read as Vix's own `vix-*` crates. | The user's call: it shares its VST3 hosting with Vix, will share Vix's mixer and the egui kit widgets, and the rhyme is the point. |
| GUI | **egui, egui-wgpu and egui-winit on wgpu and winit**, at the majors Vix pins (egui 0.36, wgpu 30, winit 0.30 on this date); winit and egui-winit directly, not eframe. | The kit's charter already records the mixer as egui and plans a shared `egui-audio` crate; pure Rust, no C++ in CI; Windows later costs nothing. Not eframe, because each VST3 editor needs a winit window of its own and its raw handle, as Vix's shell does. The user doubts egui can reach the look, so lane 05 starts with a look gate. |
| Plugins | In-process for v1; VST3 only. | The kit's `vst3-host` and Vix's rack pattern exist; out of process needs the kit's unbuilt `proc` crate, so it is a v2 lane. |
| Audio API | PipeWire's native API (the `pipewire` crate), not JACK. | App routing, virtual sinks and the `default` metadata need the native API. No `pw_filter` wrapper exists, so v1 uses `pw_stream`s and rings (handoff 03). |
| Channels | Eight in all; the mic is an ordinary hardware-input channel. | The prototype's "Add channel (n of 8)". |
| Plugins per strip | No cap; the well scrolls. | Wave Link's ten is a UI limit, not ours. |
| App identity | Store `{portal app_id, process binary, application.name}`; match app_id, then binary, then name. | Flatpak, Electron and browsers report these inconsistently; storing all three survives any one of them changing. |
| Quantum | Follow the graph; ask for 256 frames at 48 kHz through `node.latency`; preferences offer 128 and 512. | PipeWire decides the quantum; we ask, never force. |
| Devices per mix | One. | v1 scope. |
| Monitor and Master devices | Monitor always has a device; Master may have "No device" and is still a virtual source. | The prototype. |
| Licence | MIT. | The kit crates are MIT; the `vst3` and `pipewire` crates are MIT or Apache-2.0. The user can change it before the first public release. |
| Linux machine | Fedora KDE on the NV3 once installed (the first real test); a VMware Fedora VM and WSL2 (a headless PipeWire) until then; CI on `ubuntu-latest` and `windows-latest`. | The user's answer. |
| Windows | Out of scope for v1, but the workspace **builds and tests the headless crates and runs the window with a null backend on Windows**. | Windows is the development machine today, so the window lane can run there now; a cpal/WASAPI backend is a later lane. |
| Plugin editors on Linux | winit runs on X11 (XWayland under KDE Wayland). | The kit's VST3 host refuses Wayland handles: a VST3 editor embeds only in an X11 window. |
| Default branch | `master`. | As the kit, Nexus and Vix. |

## Rejected

- **Qt 6 through cxx-qt.** C++ and a Qt install in CI and on every development machine, and a
  second build system beside Cargo, for a console whose every control is custom-drawn anyway.
- **Slint.** A designed, declarative look, but its licence terms need checking before a
  public release and it shares nothing with Vix's egui panels or the planned `egui-audio`.
  It stays the fallback the look gate names: if egui cannot get close to prototype v5, the
  user decides between continuing and Slint before lane 05 spends more.
- **JACK, through PipeWire's JACK layer.** No virtual sinks, no app routing through the
  `default` metadata, no stream properties to identify apps by; the native API has all three.
- **Plugins out of process for v1.** Crash isolation needs the kit's `proc` crate, which is not
  built, and a per-plugin process with its own audio round trip; it is the v2 plugin-isolation
  lane, named in the [roadmap](roadmap.md).
- **wiremix as the name.** `github.com/tsowell/wiremix` is a published Rust PipeWire mixer.

## Open

Not blocking; each is decided by the lane named, or later.

- **Plugin isolation** (v2): out-of-process plugins, so a crashing plugin cannot take the mixer
  down. Needs the kit's `proc` crate.
- **A Windows backend**: ASIO or WASAPI through the kit's planned `audio-io` crate (`vixmix-cpal`).
- **Chain import and export.**
- **More than two mixes** (Wave Link 3 has five).
- **Packaging: Flatpak or native.** A VST3 host inside a Flatpak sandbox cannot see the host's
  plugins; handoff 08 investigates native RPM/COPR, AppImage, and Flatpak with
  `--filesystem=host`, and records the finding here.
- **The licence before any public release** (MIT assumed).
- **Plugin editors on Wayland**: later, editors on an X11 connection of their own while the
  console runs on Wayland.
