# Prototype

**Record — prototype v5, the target look, as handed over on 2026-11-02.** It is the look
handoff 05 builds the window to; its look gate puts a headless capture of the egui console
beside [screenshot.png](screenshot.png) for the user to judge before any interaction work.

- [index.html](index.html) — the current UI prototype (v5): one self-contained file; open it
  in any browser. It loads its fonts from Google Fonts and falls back to system fonts offline.
  Its `TAPER`, `posToDb`, `dbToPos` and `dbTxt` are what `vixmix_model::taper` ports; its
  `:root` colours, sizes and fonts are handoff 05's theme; its example data and `target(style)`
  meters are what `FakeHost` serves.
- [screenshot.png](screenshot.png) — a static capture of v5.
- [history/](history/README.md) — the earlier versions, for reference.

[Back to the design pack](../README.md)
