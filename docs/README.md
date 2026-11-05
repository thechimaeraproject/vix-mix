# Vix Mix docs

Every doc belongs to one class, and the class says how much to trust it:

| Class | Where | What it is | Trust |
|---|---|---|---|
| **Start here** | `charter.md`, `build-map.md`, `roadmap.md` | The premise and the settled decisions, the plan of record for the lanes, and the status. | The charter is a decision, not an edit; the build map and the roadmap are true as of their date and are updated in the commit that changes them. |
| **Reference** | `reference/` | What one crate (or the app) does and requires today. Each owns the source files it explains, on an `Owns:` line, and the references together partition `crates/*` and `app`. | Code depends on it; keep it exact. |
| **Cookbook** | `cookbook/` | A numbered procedure for one task, ending in *How you know it worked*. Lane 03 wrote the first; lane 06 the by-hand pass. | Follow it literally; if a step is wrong, fix the doc in the same commit. |
| **Record** | `design/`, `handoffs/`, the `kit-*.md` adoption records | What was decided and why — the product design, one lane of work, what was taken from the kit — and its acceptance evidence once shipped. | History once shipped, intention before; the first paragraph says which. Never an API reference; the reference it produced is. |
| **Postmortem** | `postmortem/` (none yet) | A shipped bug or a retired idea, kept for the lesson. Numbered, immutable. | History, not guidance. |

**Maintenance rules.** When a doc and the code disagree, one of them is fixed in the same
commit — no dead notes. A reference names the exact check, type or behaviour, never a metaphor
for it. A cookbook's last step is always *How you know it worked*. A handoff's first line is its
status. `kit-check-docs` (`dev check-docs`, inside `dev check`) holds this index and every doc
in it to
[the kit's standard](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/docs.md)
on every push and in CI.

## Start here

- [build-map.md](build-map.md) — the plan of record: the nine lanes and their waves, a
  worktree per lane, the conflict rules, the shared contracts and how a change to them is
  requested, how to run a lane.
- [charter.md](charter.md) — what Vix Mix is for, the decisions settled on 2026-11-03 with
  their reasons, what was rejected, what is still open.
- [roadmap.md](roadmap.md) — what is built, what is next, what the docs name that nobody has
  built.

## Reference

- [reference/app.md](reference/app.md) — the `vix-mix` window: the winit and egui-wgpu shell,
  the pacing rule, the command line, what each frame draws today.
- [reference/engine.md](reference/engine.md) — the realtime graph: the prepared graph and its
  acknowledged handoff, the allocation-free tick, meters, stats, the null backend.
- [reference/host.md](reference/host.md) — the orchestrator: the `Host` trait the window
  drives, `RealHost`'s boot, routing, autosave, undo and shutdown order, `FakeHost`'s example
  console and simulated meters.
- [reference/model.md](reference/model.md) — the setup as data: the types, the commands and
  their validation, the fader taper, `setup.ron`, the engine snapshot.
- [reference/pipewire.md](reference/pipewire.md) — the Linux backend's contract: the config,
  the events, the graph, `Backend`'s signatures, the node names; built by handoff 03.
- [reference/plugins.md](reference/plugins.md) — the catalog and its search, the rack that
  lends instances to chains, instances on the UI thread, editor sessions.

## Cookbook

- [cookbook/by-hand-pass.md](cookbook/by-hand-pass.md) — the first real test on Fedora: the Wave
  Link scene on real devices and apps, checked by ear and with PipeWire's tools, the latency
  measured.
- [cookbook/install-on-fedora.md](cookbook/install-on-fedora.md) — Vix Mix as a desktop app
  on Fedora: the build prerequisites, `cargo install` or the release tarball, `install.sh`, and
  the entry in the KDE launcher with its icon.
- [cookbook/run-headless-pipewire.md](cookbook/run-headless-pipewire.md) — a PipeWire and
  WirePlumber daemon with a null sink and a null source in CI, WSL2, a VM or a container, and
  the backend's tests against it.

## Record

### Adoption

- [kit-audio.md](kit-audio.md) — the kit's `chimaeraproject-param` and
  `chimaeraproject-audio-processor`: the processor contract and the strip maths since the
  bootstrap, the chain since handoff 02, and why instances are shared between graphs.
- [kit-cli.md](kit-cli.md) — the kit CLI: what `kit.repo.json` declares, the `dev` registry of
  cargo gates, the hooks and the Claude Code guard, the PR-only CI on two legs, the ruleset.
- [kit-fs.md](kit-fs.md) — the kit's `chimaeraproject-fs`: atomic writes, tolerant loads and
  the per-user directory with its profiles, under `setup.ron` and `plugins.ron`.
- [kit-vst3.md](kit-vst3.md) — the kit's `chimaeraproject-vst3-host`: scanning, instances,
  the lending pattern, the edit path and editors; adopted by handoff 04.

### Design

- [design/what-it-is.md](design/what-it-is.md) — the product definition (2026-11-02): Wave
  Link parity, channels, plugins, sends and mixes, app routing on PipeWire, the v1 scope; with
  the prototype the window is built to.

### Handoffs

- [handoffs/00-bootstrap.md](handoffs/00-bootstrap.md) — the workspace, the shared contracts as
  compiling skeletons, the kit adoption, the docs, CI on two legs.
- [handoffs/01-model.md](handoffs/01-model.md) — `vixmix-model`: the setup as data, commands
  with `Applied`, the taper, RON persistence, the engine snapshot.
- [handoffs/02-engine.md](handoffs/02-engine.md) — `vixmix-engine`: the realtime graph with
  chains, the acknowledged handoff, nothing allocated on the tick.
- [handoffs/03-pipewire-backend.md](handoffs/03-pipewire-backend.md) — `vixmix-pipewire`: the
  mixer as a PipeWire client, the tick on the data thread, routing, reconnect.
- [handoffs/04-plugins.md](handoffs/04-plugins.md) — `vixmix-plugins`: catalog, instances,
  the rack, state, editor sessions over the kit's VST3 host.
- [handoffs/05-app.md](handoffs/05-app.md) — the window: the look gate first, then the console
  exactly as prototype v5.
- [handoffs/06-host-and-integration.md](handoffs/06-host-and-integration.md) — `vixmix-host`:
  one process that boots, routes, mixes, saves and shuts down in order.
- [handoffs/07-kit-lifts.md](handoffs/07-kit-lifts.md) — `audio-io` and `egui-audio` lifted into
  the kit, and taken back by Vix and Vix Mix.
- [handoffs/08-packaging-and-first-real-test.md](handoffs/08-packaging-and-first-real-test.md) —
  the desktop entry and metainfo, an install path, Flatpak investigated, the first by-hand
  pass on Fedora.
- [handoffs/09-round-1-fixes.md](handoffs/09-round-1-fixes.md) — what the first by-hand pass on
  Fedora finds, fixed; seeded with the twelve items the earlier lanes left open for it.
