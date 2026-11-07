# Roadmap

**As of 2026-11-07.** What is built, what is next, and what the docs name that nobody has
built. A lane that ships flips its rows here in the same commit as its handoff's status line;
the [build map](build-map.md) holds the order and the conflict rules.

## Built

| What | Where | Since |
|---|---|---|
| The workspace: five crates and the app, the toolchain pin, the kit's four crates at one tag | [handoff 00](handoffs/00-bootstrap.md) | 2026-11-04 |
| The shared contracts as compiling skeletons with real minimal behaviour: the setup and its commands, the taper, `setup.ron`, the engine snapshot; the engine's handoff, tick, meters and null backend; the catalog, rack and instances; the backend's signatures; the `Host` trait with `RealHost` and `FakeHost` | [reference/](reference/README.md) | 2026-11-04 |
| The window: a winit and egui-wgpu shell titled Vix Mix, its header, the no-backend banner, `--fake`, `--quit-after-frames` | [reference/app.md](reference/app.md) | 2026-11-04 |
| The look gate, passed by the user: prototype v5's theme and Plex fonts, the console drawn from the host, `--dump-png` and [its capture](design/prototype/egui-look-gate.png) beside the screenshot | [handoff 05](handoffs/05-app.md) step 1 | 2026-11-04 |
| The console as prototype v5, interactive over the `Host` trait: faders (drag, Shift, double-click, keys), mute, link, the ear, MON/MST, the FX well with bypass, gear, bin and a scrolling list, inserts dragged within and between strips (Ctrl copies), the source and output panels, the add-channel menu, the plugin browser, the menu and preferences, toasts with Undo, plugin editor windows (X11 on Linux), 60 Hz pacing on a grid while the window can be seen and none while it is hidden; every gesture's command asserted headless | [reference/app.md](reference/app.md), [handoff 05](handoffs/05-app.md) | 2026-11-04 |
| The kit CLI: manifest, `dev` registry, hooks, skills, `kit-check-docs` | [kit-cli.md](kit-cli.md) | 2026-11-04 |
| CI: `check (linux)` with a headless PipeWire daemon, `check (windows)` | [kit-cli.md](kit-cli.md) | 2026-11-04 |
| The model: every command's `Applied` exact and proven against the diff, ids that never come back, `setup.ron` format 2 with the migration from format 1 and the rules checked at load | [reference/model.md](reference/model.md) | 2026-11-04 |
| The plugins: the scan with a per-bundle cache in `plugins.ron`, instances made to match a setup with the missing-plugin rule, the rack and the edit path from a plugin's editor to its lent processor, state, editor sessions; tested with a real VST3 plugin on Windows and Linux | [reference/plugins.md](reference/plugins.md) | 2026-11-04 |
| The engine's graph: chains on every channel and both mixes with each plugin lent once and shared between graphs, the MON/MST split, mix chains, replaced graphs dropped on the host thread (behind the acknowledgement, or at once when nothing ticks), setting instances up again for a new rate or new buses, pass-through problems, no allocation on the tick | [reference/engine.md](reference/engine.md) | 2026-11-04 |
| The PipeWire backend: the streams, the rings and their latency governor, the tick in the first output to run, the registry with app identity, routing by serial through the `default` metadata, `apply` diffing, reconnect; integration tests against a headless daemon; the two-minute run at 256 with zero xruns passes on a quiet machine and waits for a shell with realtime scheduling to be called met | [reference/pipewire.md](reference/pipewire.md) | 2026-11-04 |
| The cookbook for a headless PipeWire in CI, WSL2, a VM or a container | [cookbook/run-headless-pipewire.md](cookbook/run-headless-pipewire.md) | 2026-11-04 |
| The host: boot in order, PipeWire kept on Linux with every app routed as it appears, `Applied` acted on part by part, the debounced save, undo of a removed channel or insert (the model's restore commands, change request 2), pass-throughs and notices surfaced, shutdown in the kit's order; proven on the null backend with a real plugin on both legs and against the headless daemon on Linux | [reference/host.md](reference/host.md) | 2026-11-04 |
| The by-hand pass for the first real test on Fedora, waiting for handoff 08 to run it | [cookbook/by-hand-pass.md](cookbook/by-hand-pass.md) | 2026-11-04 |
| Packaging for Linux: the desktop entry, the icon and the AppStream metadata in `packaging/linux/` with `install.sh`, installed after `cargo install` or from the release tarball (`dev package`); proven in the Ubuntu container, the install on Fedora waiting for Fedora KDE on the NV3 | [cookbook/install-on-fedora.md](cookbook/install-on-fedora.md), [handoff 08](handoffs/08-packaging-and-first-real-test.md) | 2026-11-04 |
| The packaging finding: native, not Flatpak, with the evidence and the three options | [charter.md](charter.md) § Open | 2026-11-04 |
| The kit lifts: the engine's meter bank and counters taken from the kit's `audio-io`, the console's meter faders, scale, peak hold and taper trait from its `egui-audio`, the copies deleted, the look gate's capture byte-identical | [kit-audio-io.md](kit-audio-io.md), [kit-egui-audio.md](kit-egui-audio.md), [handoff 07](handoffs/07-kit-lifts.md) | 2026-11-07 |

## Next

Wave 2, in parallel, each in its own worktree ([build map](build-map.md) § Waves):

| Lane | What it delivers | Status |
|---|---|---|
| [01 — Model](handoffs/01-model.md) | the whole setup as data, `Applied` proven against the diff, the taper, persistence with a migration seam | shipped 2026-11-04 |
| [02 — Engine](handoffs/02-engine.md) | chains lent through `Lender`, the dry/processed split, mix chains, the no-alloc proof with chains on | shipped 2026-11-04 |
| [03 — PipeWire backend](handoffs/03-pipewire-backend.md) | the streams, rings, tick election, registry, routing and reconnect, tested against a headless daemon | shipped 2026-11-04 (see Built) |
| [04 — Plugins](handoffs/04-plugins.md) | scan and cache, instances, the rack, state, editor sessions, tested with the kit's test plugin | shipped 2026-11-04 (see Built) |
| [05 — App](handoffs/05-app.md) | the look gate, then the console exactly as prototype v5 against `FakeHost` | shipped 2026-11-04 (see Built); its CPU acceptance met per machine (the main session's ruling); change request 10 (agreed) and a frame for the meters alone left to round 1 |

Then wave 3: [06 — Host and integration](handoffs/06-host-and-integration.md), shipped
2026-11-04 (see Built; the acceptance's Fedora bullet waits for the by-hand pass); then
[07 — Kit lifts](handoffs/07-kit-lifts.md), shipped 2026-11-07 (see Built), and
[08 — Packaging and the first real test](handoffs/08-packaging-and-first-real-test.md), shipped
2026-11-04 (see Built) but for its step 4, the by-hand pass, which waits for Fedora KDE on the
NV3 and fills [09 — Round 1 fixes](handoffs/09-round-1-fixes.md), seeded with the fifteen items
the earlier lanes left open for it.

## Named but not built

- **A Windows backend**, `vixmix-cpal`, over the kit's `audio-io` crate with its device layer
  (handoff 07 lifted the crate; Vix Mix takes it without that layer today).
- **An RPM spec** that builds from a source tarball with the crates vendored, built with
  `rpmbuild` while the repository is private and through a COPR once it is public (the
  charter's packaging finding).
- **Plugin isolation**: plugins out of process (v2), over the kit's planned `proc` crate.
- **Chain import and export**, a presets browser, MIDI and instruments.
- **More than two mixes.**
- **The lower-latency backend paths** handoff 03 records but does not build: inputs without
  rings through an arrival bitmask (one quantum less), and a single `pw_filter` node through
  `pipewire-sys`.
- **Editors on their own X11 connection**, so the console can run on Wayland.
