# Handoffs

One document per lane of the build described in [build-map.md](../build-map.md), which also
holds the waves, the worktrees, the conflict rules and the change requests to the shared
contracts. Each handoff is self-contained for one person or agent: goal, what to read, what the
lane owns and what the others keep, numbered steps, the files it owns, acceptance, and what is
out of scope. Its first line is its status; flip it to `**Record — shipped <date>.**` with
every deviation and its reason when the acceptance section is green, and complete the reference
doc it names.

- [00-bootstrap.md](00-bootstrap.md) — the workspace and toolchain, the shared contracts as
  compiling skeletons, the kit adoption, the docs and the handoffs, CI on two legs.
- [01-model.md](01-model.md) — `vixmix-model`: the setup as data, commands with `Applied`, the
  taper, `setup.ron` with a migration seam, the engine snapshot.
- [02-engine.md](02-engine.md) — `vixmix-engine`: chains lent through `Lender`, the
  dry/processed split, mix chains, the acknowledged handoff, nothing allocated on the tick.
- [03-pipewire-backend.md](03-pipewire-backend.md) — `vixmix-pipewire`: streams, rings, the
  tick on the data thread, tick election, registry, routing, reconnect; Linux only.
- [04-plugins.md](04-plugins.md) — `vixmix-plugins`: scan and cache, instances, the rack,
  state, editor sessions, tested with the kit's test plugin.
- [05-app.md](05-app.md) — the window: the look gate first, then the console exactly as
  prototype v5 against `FakeHost`.
- [06-host-and-integration.md](06-host-and-integration.md) — `vixmix-host`: boot, routing,
  saving, shutdown; the first by-hand cookbook.
- [07-kit-lifts.md](07-kit-lifts.md) — `audio-io` and `egui-audio` lifted into the kit and
  taken back by Vix and Vix Mix.
- [08-packaging-and-first-real-test.md](08-packaging-and-first-real-test.md) — the desktop
  entry and metainfo, an install path, Flatpak investigated, the first real test on Fedora.
- [09-round-1-fixes.md](09-round-1-fixes.md) — what the first by-hand pass on Fedora finds,
  fixed; seeded with the items the earlier lanes left open for it.
