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
| The console as prototype v5, interactive over the `Host` trait, filling the window (the channels scroll sideways, Monitor and Master pinned at the right, the well and the faders taking the height): faders (a press away from the line jumps there, drag, Shift, double-click, keys), mute, link, the ear, MON/MST, the FX well with bypass, gear, bin and a scrolling list, inserts dragged within and between strips (Ctrl copies), the source and output panels, the add-channel menu, the plugin browser, the menu and preferences, toasts with Undo, plugin editor windows (X11 on Linux), 60 Hz pacing on a grid while the window can be seen and none while it is hidden; every gesture's command asserted headless | [reference/app.md](reference/app.md), [handoff 05](handoffs/05-app.md) | 2026-11-04 |
| The kit CLI: manifest, `dev` registry, hooks, skills, `kit-check-docs` | [kit-cli.md](kit-cli.md) | 2026-11-04 |
| CI: `check (linux)` with a headless PipeWire daemon, `check (windows)` | [kit-cli.md](kit-cli.md) | 2026-11-04 |
| The model: every command's `Applied` exact and proven against the diff, ids that never come back, `setup.ron` format 2 with the migration from format 1 and the rules checked at load | [reference/model.md](reference/model.md) | 2026-11-04 |
| The plugins: the scan with a per-bundle cache in `plugins.ron`, instances made to match a setup with the missing-plugin rule, the rack and the edit path from a plugin's editor to its lent processor, state, editor sessions; tested with a real VST3 plugin on Windows and Linux | [reference/plugins.md](reference/plugins.md) | 2026-11-04 |
| The engine's graph: chains on every channel and both mixes with each plugin lent once and shared between graphs, the MON/MST split, mix chains, replaced graphs dropped on the host thread (behind the acknowledgement, or at once when nothing ticks), setting instances up again for a new rate or new buses, pass-through problems, no allocation on the tick | [reference/engine.md](reference/engine.md) | 2026-11-04 |
| The PipeWire backend: the streams, the rings and their latency governor, the tick in the first output to run, the registry with app identity, routing by serial through the `default` metadata, `apply` diffing, reconnect; integration tests against a headless daemon; the two-minute run at 256 with zero xruns passes on a quiet machine and waits for a shell with realtime scheduling to be called met | [reference/pipewire.md](reference/pipewire.md) | 2026-11-04 |
| The cookbook for a headless PipeWire in CI, WSL2, a VM or a container | [cookbook/run-headless-pipewire.md](cookbook/run-headless-pipewire.md) | 2026-11-04 |
| The host: boot in order, PipeWire kept on Linux with every app routed as it appears, `Applied` acted on part by part, the debounced save, undo of a removed channel or insert (the model's restore commands, change request 2), pass-throughs and notices surfaced, shutdown in the kit's order; proven on the null backend with a real plugin on both legs and against the headless daemon on Linux | [reference/host.md](reference/host.md) | 2026-11-04 |
| The by-hand pass for the first real test on Fedora; it ran on 2026-11-10 as hours of daily use ([#19](https://github.com/thechimaeraproject/vix-mix/issues/19), closed), and [handoff 09](handoffs/09-round-1-fixes.md) holds what it found | [cookbook/by-hand-pass.md](cookbook/by-hand-pass.md) | 2026-11-04 |
| Packaging for Linux: the desktop entry, the icon and the AppStream metadata in `packaging/linux/` with `install.sh`, installed as an RPM (`packaging/linux/vix-mix.spec`, `dev rpm`, built with every crate vendored), after `cargo install` or from the release tarball (`dev package`), the RPM and the tarball both built by GitHub on `master` after every merge; proven in the Ubuntu container, and the RPM installed and started from the launcher on Fedora KDE (2026-11-10) | [cookbook/install-on-fedora.md](cookbook/install-on-fedora.md), [handoff 08](handoffs/08-packaging-and-first-real-test.md) | 2026-11-04 |
| The packaging finding: native, not Flatpak, with the evidence and the three options; the user took it (2026-11-10): an RPM | [charter.md](charter.md) § Decisions and § Rejected | 2026-11-04 |
| The kit lifts: the engine's meter bank and counters taken from the kit's `audio-io`, the console's meter faders, scale, peak hold and taper trait from its `egui-audio`, the copies deleted, the look gate's capture byte-identical | [kit-audio-io.md](kit-audio-io.md), [kit-egui-audio.md](kit-egui-audio.md), [handoff 07](handoffs/07-kit-lifts.md) | 2026-11-07 |

## Next

Wave 2, in parallel, each in its own worktree ([build map](build-map.md) § Waves):

| Lane | What it delivers | Status |
|---|---|---|
| [01 — Model](handoffs/01-model.md) | the whole setup as data, `Applied` proven against the diff, the taper, persistence with a migration seam | shipped 2026-11-04 |
| [02 — Engine](handoffs/02-engine.md) | chains lent through `Lender`, the dry/processed split, mix chains, the no-alloc proof with chains on | shipped 2026-11-04 |
| [03 — PipeWire backend](handoffs/03-pipewire-backend.md) | the streams, rings, tick election, registry, routing and reconnect, tested against a headless daemon | shipped 2026-11-04 (see Built) |
| [04 — Plugins](handoffs/04-plugins.md) | scan and cache, instances, the rack, state, editor sessions, tested with the kit's test plugin | shipped 2026-11-04 (see Built) |
| [05 — App](handoffs/05-app.md) | the look gate, then the console exactly as prototype v5 against `FakeHost` | shipped 2026-11-04 (see Built); its CPU acceptance met per machine (the main session's ruling); change request 10 applied in round 1 (2026-11-10, [#11](https://github.com/thechimaeraproject/vix-mix/issues/11)); a frame for the meters alone left to round 1 |

Then wave 3: [06 — Host and integration](handoffs/06-host-and-integration.md), shipped
2026-11-04 (see Built; the acceptance's Fedora bullet is the by-hand pass, run 2026-11-10); then
[07 — Kit lifts](handoffs/07-kit-lifts.md), shipped 2026-11-07 (see Built), and
[08 — Packaging and the first real test](handoffs/08-packaging-and-first-real-test.md), shipped
2026-11-04 (see Built); its step 4, the by-hand pass, ran on 2026-11-10 as hours of daily use
on Fedora KDE and fills [09 — Round 1 fixes](handoffs/09-round-1-fixes.md), which holds its two
findings beside the fifteen items the earlier lanes left open for it, and a third finding
that daily use made after the pass.

Round 1 is tracked as issues on the repository, one per item that is a fix and one for the pass
itself; an issue closes when its item is struck through in handoff 09.

| Issue | What | Handoff 09 |
|---|---|---|
| [#19](https://github.com/thechimaeraproject/vix-mix/issues/19) | the first real test on Fedora KDE: the by-hand pass, its figures and checks (the umbrella); closed 2026-11-10, the pass ran | items 6, 8, 11 and 12 struck; the figures of items 1, 2 and 5 not measured, open without an issue |
| [#26](https://github.com/thechimaeraproject/vix-mix/issues/26) | found by the pass: a plugin that aborts while loading takes the scan and Vix Mix down; fixed and closed 2026-11-10 | the Goal's first finding, step 5, built |
| [#25](https://github.com/thechimaeraproject/vix-mix/issues/25) | found by the pass: the DSP readout flickers between about 12 % and 0 %; fixed and closed 2026-11-10 | the Goal's second finding, step 6, built |
| [#37](https://github.com/thechimaeraproject/vix-mix/issues/37) | found in daily use after the pass: the window holds 64 MiB of an NVIDIA card's 256 MiB CPU-visible window (BAR1), where 4 MiB does; fixed 2026-11-13 | the Goal's third finding, step 7, built |
| [#11](https://github.com/thechimaeraproject/vix-mix/issues/11) | a plugin editor's window resizes without telling the plugin; the plugin-folder bound (change request 10); fixed and closed 2026-11-10 | item 14, struck |
| [#12](https://github.com/thechimaeraproject/vix-mix/issues/12) | SIGTERM and logout skip the shutdown order; fixed and closed 2026-11-11 | item 10, struck |
| [#13](https://github.com/thechimaeraproject/vix-mix/issues/13) | Vix Mix restarted within milliseconds of quitting can leave a playing app routed but silent; measured 2026-11-11: it holds on a current PipeWire and is the older one's own on Ubuntu 24.04, not worked around | item 3, struck |
| [#14](https://github.com/thechimaeraproject/vix-mix/issues/14) | no toast when Monitor's device goes while Master has no device; fixed and closed 2026-11-10 | item 4, struck |
| [#15](https://github.com/thechimaeraproject/vix-mix/issues/15) | Preferences: the Tab order of the folder field and its Add button; fixed and closed 2026-11-10 | item 15, struck |
| [#16](https://github.com/thechimaeraproject/vix-mix/issues/16) | the first start: the plugin scan before the first frame, Monitor's device not picked | item 7 |
| [#17](https://github.com/thechimaeraproject/vix-mix/issues/17) | a frame for the meters alone, to get under 5 % of one core | item 13 |
| [#18](https://github.com/thechimaeraproject/vix-mix/issues/18) | the window's app id on Wayland and the launcher's activation token; fixed and closed 2026-11-11 | item 9, struck |

## Named but not built

- **A Windows backend**, `vixmix-cpal`, over the kit's `audio-io` crate with its device layer
  (handoff 07 lifted the crate; Vix Mix takes it without that layer today).
- **A COPR** that builds the RPM spec once the repository and its licence are public, so `dnf`
  updates Vix Mix (the charter's packaging decision); until then the RPM is GitHub's artifact.
- **Plugin isolation**: plugins out of process (v2), over the kit's planned `proc` crate.
- **Chain import and export**, a presets browser, MIDI and instruments.
- **More than two mixes.**
- **The lower-latency backend paths** handoff 03 records but does not build: inputs without
  rings through an arrival bitmask (one quantum less), and a single `pw_filter` node through
  `pipewire-sys`.
- **Editors on their own X11 connection**, so the console can run on Wayland.
