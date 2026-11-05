# Reference

A reference explains what one crate (or the app) does and requires today, and owns the source
files it explains on an `**Owns:**` line — the first non-blank line after its H1, holding
repository-relative globs in backticks. Together the references partition the code roots
declared in `kit.docs.config.mjs` (`crates/*` and `app`): every file under them is owned by
exactly one reference or exempted there with a written reason (tests, fixtures, build output).
Code depends on a reference, so keep it exact; each ends with `## Verification`, naming the
tests and checks that pin it.

Each was written by lane 00 with the contract as it stands; the lane that owns the crate
completes it when it ships ([build map](../build-map.md)).

- [app.md](app.md) — the `vix-mix` window: the winit and egui-wgpu shell, the pacing rule,
  the command line, what each frame draws.
- [engine.md](engine.md) — the realtime graph: the prepared graph and its acknowledged handoff,
  the tick, meters, stats, the null backend.
- [host.md](host.md) — the orchestrator: the `Host` trait, `RealHost`, `FakeHost`.
- [model.md](model.md) — the setup as data: types, commands, the taper, `setup.ron`, the
  snapshot.
- [pipewire.md](pipewire.md) — the Linux backend: the contract and the design as handoff 03
  built them.
- [plugins.md](plugins.md) — catalog, rack, instances, editor sessions.

[Back to the docs index](../README.md)
