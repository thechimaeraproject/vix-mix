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
| GUI | **egui, egui-wgpu and egui-winit on wgpu and winit**, at the majors Vix pins (the workspace `Cargo.toml` holds them); winit and egui-winit directly, not eframe. | The kit's charter already records the mixer as egui and plans a shared `egui-audio` crate; pure Rust, no C++ in CI; Windows later costs nothing. Not eframe, because each VST3 editor needs a winit window of its own and its raw handle, as Vix's shell does. The user doubts egui can reach the look, so lane 05 starts with a look gate. |
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
| Packaging | **Native: an RPM** (`packaging/linux/vix-mix.spec`, built by `scripts/package/rpm.sh` in a Fedora container by `.github/workflows/linux-build.yml` after every merge), with the tarball and `cargo install` beside it; a COPR once the repository and its licence are public, so `dnf` updates it. | The user's decision (2026-11-10), on handoff 08's finding (under Rejected, Flatpak): a native install sees every plugin the machine has, yabridge works, and nothing needs granting. |

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
- **Flatpak, and AppImage.** Handoff 08's finding (2026-11-04), which the user took
  (2026-11-10). A Flatpak, even with `--filesystem=host`, sees the home's `~/.vst3` and the
  LinuxAudio extensions, the host's `/usr/lib64/vst3` only through `/run/host` with library
  mismatches, no yabridge, and needs a Manager category in the backend and the whole home and
  every disk granted, with the software centre's risk label that comes with it. An AppImage is
  unsandboxed but adds no launcher entry without an integration tool, needs FUSE at run time and
  a build on the oldest glibc it targets, and gives nothing over the tarball for a binary that
  needs only glibc and libpipewire. The evidence:
  - *The sandbox cannot see the host's plugins where Fedora puts them.* `/usr`, `/lib64` and
    `/etc` are reserved paths no `--filesystem` grant opens
    ([Flatpak's sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html));
    with `--filesystem=host` (or `host-os`) the host's `/usr/lib64/vst3` appears only as
    `/run/host/usr/lib64/vst3`, and a plugin loaded from there is linked against Fedora's
    libraries while the process runs on the Flatpak runtime's. Ardour's Flathub notes say plugins
    in `~/.vst3` "may work if they are built properly. There is no universal solution if they
    depend on .so that are not available or incompatible", and "There is no support for Win32
    emulated VST" ([org.ardour.Ardour](https://github.com/flathub/org.ardour.Ardour)).
  - *yabridge* "doesn't work with `flatpak` packages"
    ([its README](https://github.com/robbert-vdh/yabridge)), and the FabFilter, iZotope and
    Kilohearts plugins the by-hand pass bridges reach Linux only through it.
  - *Flathub's VST3 hosts* load plugins packaged for Flatpak, the
    `org.freedesktop.LinuxAudio.Plugins` extensions mounted at `/app/extensions/Plugins` with
    `VST3_PATH` pointing there, plus the home folder: `--filesystem=home` for Ardour and
    REAPER, `--filesystem=host` for Bitwig, whose Flathub page rates it medium risk
    ([com.bitwig.BitwigStudio](https://github.com/flathub/com.bitwig.BitwigStudio)). Of the
    plugins the by-hand pass names, the open-source ones (Airwindows, Surge XT, Dragonfly, LSP)
    exist as such extensions; TDR's, u-he's and every Windows plugin do not.
  - *Routing other apps needs a manager's rights.* The host writes each app's target into
    PipeWire's `default` metadata; WirePlumber gives a Flatpak client read and execute only
    (`rx`, no `m` to set metadata) unless it declares `media.category = Manager`
    ([WirePlumber's access configuration](https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/access.html)),
    as EasyEffects does for its Flatpak. `vixmix-pipewire` declares no `media.category = Manager` on its client (its streams say Capture or Playback), so a Flatpak
    would also need a change there.
  - *What the binary needs is small*: the release build links only glibc, `libgcc_s` and
    `libpipewire-0.3` (`ldd`, in the container), and loads the window system and the GPU driver
    at run time.

## Open

Not blocking; each is decided by the lane named, or later.

- **Plugin isolation** (v2): out-of-process plugins, so a crashing plugin cannot take the mixer
  down. Needs the kit's `proc` crate.
- **A Windows backend**: ASIO or WASAPI through the kit's planned `audio-io` crate (`vixmix-cpal`).
- **Chain import and export.**
- **More than two mixes** (Wave Link 3 has five).
- **The licence before any public release** (MIT assumed).
- **Plugin editors on Wayland**: later, editors on an X11 connection of their own while the
  console runs on Wayland.
