# Cookbook

A cookbook is a numbered procedure for one task someone will do again. It is not an
explanation of how a crate works — that is a [reference](../reference/README.md) — and it is
not a record of one decision — that is a handoff or a design record.

The shape is fixed, and `kit-check-docs` holds it: one task per doc, named for the task;
numbered steps (`## 1. …`) in the order you do them, each naming what it changes; the last
step is always *How you know it worked*, with the command to run and the output that means it
did. If a step turns out to be wrong, fix the doc in the same commit.

- [by-hand-pass.md](by-hand-pass.md) — the first real test on Fedora: the Wave Link scene on
  real devices, a mic through a plugin, apps back in their channels, Master recorded by Discord
  and OBS, save and restore, xruns and the latency measured (handoff 06; handoff 08 runs it).
- [install-on-fedora.md](install-on-fedora.md) — Vix Mix as a desktop app on Fedora: the build
  prerequisites, `cargo install` or the release tarball, `install.sh` putting the desktop
  entry, the icon and the metadata where the launcher finds them (handoff 08).
- [run-headless-pipewire.md](run-headless-pipewire.md) — a headless PipeWire and WirePlumber
  with null devices in CI, WSL2, a VM or a container, and the backend's tests against it
  (handoff 03).

A lane that adds one lists it here and in the index's Cookbook section.

[Back to the docs index](../README.md)
