# Cookbook

A cookbook is a numbered procedure for one task someone will do again. It is not an
explanation of how a crate works — that is a [reference](../reference/README.md) — and it is
not a record of one decision — that is a handoff or a design record.

The shape is fixed, and `kit-check-docs` holds it: one task per doc, named for the task;
numbered steps (`## 1. …`) in the order you do them, each naming what it changes; the last
step is always *How you know it worked*, with the command to run and the output that means it
did. If a step turns out to be wrong, fix the doc in the same commit.

- [run-headless-pipewire.md](run-headless-pipewire.md) — a headless PipeWire and WirePlumber
  with null devices in CI, WSL2, a VM or a container, and the backend's tests against it
  (handoff 03).

The next is named in the [roadmap](../roadmap.md): `by-hand-pass.md` (handoff 06, run by 08). A
lane that adds one lists it here and in the index's Cookbook section.

[Back to the docs index](../README.md)
