# Design pack

**Record — the design pack as handed over on 2026-11-02, moved here from `refs/` by
[handoff 00](../handoffs/00-bootstrap.md); intention, not yet built.** It defines the product:
a Linux take on Wave Link, a multi-channel VST3 mixer in Rust with two mixes, Monitor and
Master. Prototype v5 is the look the window is built to; [the charter](../charter.md) settles
the questions the pack left open and wins where the two differ.

## Contents

- [what-it-is.md](what-it-is.md) — the product definition: the Wave Link parity target, the
  core concepts, app routing on PipeWire, the v1 scope, the tech direction and the questions it
  left open.
- [signal-flow.png](signal-flow.png) — the signal-flow diagram the definition refers to: one
  channel, its chain, two sends, two mixes.
- [prototype/](prototype/README.md) — prototype v5, the target look: one self-contained HTML
  file, its screenshot, and the earlier versions.

## Prototype controls

- Click a strip's name to set its name and its input or apps, or a mix's output.
- Drag a meter to move its fader line. Hold Shift for fine moves, and double-click for 0 dB.
- Drag plugins to reorder them or move them onto another strip. Hold Ctrl to copy.
- Double-click a plugin to open its window. Hover a plugin for settings and remove. Click its
  square to bypass it.
- "Add plugin" opens the search-first browser. Use the arrow keys and Enter to add.
- MON / MST on a channel's FX header choose which mixes get the processed chain.
- The ear button on Master previews it through the Monitor output.

All data in the prototype is example data, and the meters show simulated signal; `FakeHost`
serves the same data to the window ([reference/host.md](../reference/host.md)).

## Live versions

- Doc: https://claude.ai/artifact/6skM7r2xxhVW8t74kKjFHP
- Prototype: https://claude.ai/artifact/Xt91Wx7DrX7JSRte8ZaHdu

[Back to the docs index](../README.md)
