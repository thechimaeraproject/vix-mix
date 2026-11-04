# Roadmap

**As of 2026-11-04.** What is built, what is next, and what the docs name that nobody has
built. A lane that ships flips its rows here in the same commit as its handoff's status line;
the [build map](build-map.md) holds the order and the conflict rules.

## Built

| What | Where | Since |
|---|---|---|
| The workspace: five crates and the app, the toolchain pin, the kit's four crates at one tag | [handoff 00](handoffs/00-bootstrap.md) | 2026-11-04 |
| The shared contracts as compiling skeletons with real minimal behaviour: the setup and its commands, the taper, `setup.ron`, the engine snapshot; the engine's handoff, tick, meters and null backend; the catalog, rack and instances; the backend's signatures; the `Host` trait with `RealHost` and `FakeHost` | [reference/](reference/README.md) | 2026-11-04 |
| The window: a winit and egui-wgpu shell titled Vix Mix, its header, the no-backend banner, `--fake`, `--quit-after-frames` | [reference/app.md](reference/app.md) | 2026-11-04 |
| The kit CLI: manifest, `dev` registry, hooks, skills, `kit-check-docs` | [kit-cli.md](kit-cli.md) | 2026-11-04 |
| CI: `check (linux)` with a headless PipeWire daemon, `check (windows)` | [kit-cli.md](kit-cli.md) | 2026-11-04 |
| The model: every command's `Applied` exact and proven against the diff, ids that never come back, `setup.ron` format 2 with the migration from format 1 and the rules checked at load | [reference/model.md](reference/model.md) | 2026-11-04 |
| The plugins: the scan with a per-bundle cache in `plugins.ron`, instances made to match a setup with the missing-plugin rule, the rack and the edit path from a plugin's editor to its lent processor, state, editor sessions; tested with a real VST3 plugin on Windows and Linux | [reference/plugins.md](reference/plugins.md) | 2026-11-04 |

## Next

Wave 2, in parallel, each in its own worktree ([build map](build-map.md) § Waves):

| Lane | What it delivers | Status |
|---|---|---|
| [01 — Model](handoffs/01-model.md) | the whole setup as data, `Applied` proven against the diff, the taper, persistence with a migration seam | shipped 2026-11-04 |
| [02 — Engine](handoffs/02-engine.md) | chains lent through `Lender`, the dry/processed split, mix chains, the no-alloc proof with chains on | not yet built |
| [03 — PipeWire backend](handoffs/03-pipewire-backend.md) | the streams, rings, tick election, registry, routing and reconnect, tested against a headless daemon | not yet built |
| [04 — Plugins](handoffs/04-plugins.md) | scan and cache, instances, the rack, state, editor sessions, tested with the kit's test plugin | shipped 2026-11-04 (see Built) |
| [05 — App](handoffs/05-app.md) | the look gate, then the console exactly as prototype v5 against `FakeHost` | not yet built |

Then wave 3: [06 — Host and integration](handoffs/06-host-and-integration.md) once 01–04 have
merged; then [07 — Kit lifts](handoffs/07-kit-lifts.md) and
[08 — Packaging and the first real test](handoffs/08-packaging-and-first-real-test.md).

## Named but not built

- **Cookbooks.** `cookbook/run-headless-pipewire.md` (a PipeWire and WirePlumber daemon with
  null devices on WSL2, a VM or CI; handoff 03) and `cookbook/by-hand-pass.md` (the first real
  test on Fedora; handoff 06 writes it, handoff 08 runs it).
- **A Windows backend**, `vixmix-cpal`, over the kit's `audio-io` crate once handoff 07 lifts it.
- **Plugin isolation**: plugins out of process (v2), over the kit's planned `proc` crate.
- **Chain import and export**, a presets browser, MIDI and instruments.
- **More than two mixes.**
- **The lower-latency backend paths** handoff 03 records but does not build: inputs without
  rings through an arrival bitmask (one quantum less), and a single `pw_filter` node through
  `pipewire-sys`.
- **Editors on their own X11 connection**, so the console can run on Wayland.
