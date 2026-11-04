# 04 — Plugins

**Record — not yet built.**
**Needs:** 00. **Runs alongside:** 01, 02, 03, 05. **Size:** M.

## Goal

Everything between the kit's `vst3-host` and the mixer: the catalog the browser searches,
instances that live on the UI thread, the rack that lends them to chains, state for the file,
and editor sessions — proven with a real VST3 plugin on Windows and Linux.

## Read first

- [reference/plugins.md](../reference/plugins.md): the contract as lane 00 left it.
  `crates/vixmix-plugins/` already has `Catalog` (scan over the kit's `scan`, the prototype's
  search ranking, favourites, recent, `plugins.ron`), `PluginKind` with `from_sub_categories`,
  `Rack` implementing `Lender` (lend, the forwarding handle, retire, reap, the generation),
  `Instances` (create through the catalog, state, retire, reap, tick) and `EditorSession`. The
  rack is tested with the kit's `Gain`; nothing has run a real plugin yet.
- [kit-vst3.md](../kit-vst3.md) and [build-map.md](../build-map.md) § Change requests (request
  1: how an `InstanceEvent::Edit` reaches the lent processor — agree it with lane 02).
- The kit's [vst3-host.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)
  in full (§ The API, § Threads, § Lending a hosted plugin to a chain, § The consumer recipe,
  § Decisions, § Verification), its test plugin
  [examples/test_gain.rs](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/crates/vst3-host/examples/test_gain.rs)
  and [tests/common/mod.rs](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/crates/vst3-host/tests/common/mod.rs)
  (how it is laid out as a bundle), and
  [audio-processor's tests/lending.rs](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/crates/audio-processor/tests/lending.rs).
- Vix (`A:\RustroverProjects\Vix`): [crates/vix-audio/src/rack.rs](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-audio/src/rack.rs)
  (the rack this one follows) and [docs/kit-vst3.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/kit-vst3.md)
  (Vix's catalog, its missing-plugin rule, its host on the window's thread).
- The prototype's browser (`docs/design/prototype/index.html`: `CATS`, `listFor`, the
  favourites and recent lists) for the Type list and the ranking.

## What this lane owns, what others keep

It owns the plugins: catalog, instances, rack, state, editor sessions. The engine owns lending
from the rack into chains (02); the window owns the browser and the editor's window (05); the
host decides when instances are created and retired and captures state before a save (06).

## Steps

1. **Scan and cache.** Scan dirs = `default_search_dirs()` + the preferences' extras, cached to
   `plugins.ron` with each bundle's modification time so an unchanged bundle is not rescanned;
   `Fx` classes only.
2. **`PluginKind` from sub-categories** (`Dynamics`, `EQ`, `Reverb`, `Delay`, `Distortion`,
   `Modulation`, `Pitch Shift`/`Vocal` → PitchVocal, `Restoration`, `Tools`/`Analyzer` →
   Utility, else MultiEffect), matching the prototype's Type list.
3. **Favourites and recent (8)** persisted; **search** ranking as the prototype's (name starts
   with, name contains, vendor, type).
4. **`Instances`.** A `Module` cache by bundle path; `create` on the UI thread, `load_state`,
   rack `add`; `tick` drains `ControllerEvent`s → `Dirty` / `Restart` (an `IoChanged` moves the
   rack's generation); `save_state`; retire → reap → drop the processor, then the controller,
   on the UI thread.
5. **`Rack`** as Vix's `rack.rs` (generation counter, the forwarding handle whose `Drop`
   deactivates and returns it) implementing `Lender`.
6. **`EditorSession`** over `Vst3Editor`, with `RunLoop::poll` on Linux.
7. **A missing plugin at load** is a pass-through, reported once with the fix (Vix's rule).
8. **The edit path** (change request 1), with lane 02.
9. **Docs.** Complete [reference/plugins.md](../reference/plugins.md); flip
   [kit-vst3.md](../kit-vst3.md) to adopted.

## Files you own

`crates/vixmix-plugins/**`, `docs/reference/plugins.md`, `docs/kit-vst3.md`, this handoff,
your rows in `docs/roadmap.md`, one paragraph in `CLAUDE.md` § Status.

## Acceptance

Tests with the kit's test plugin (`cargo build -p chimaeraproject-vst3-host --example
test_gain`, laid out as a bundle under `CARGO_TARGET_TMPDIR` as the kit's `tests/common/mod.rs`
does; if a git dependency's example cannot be built from the consumer, the lane adds
`examples/test_gain.rs` here from the `vst3` crate's gain example with its attribution):

- the scan finds it and classifies it;
- create → lend → process through an engine chain → state saved → a second instance loads it →
  equal output;
- a retired instance comes home and drops on the UI thread;
- favourites and recent round-trip;
- `reference/plugins.md` complete; `docs/kit-vst3.md` flipped to adopted.

On Windows the same tests run: there is no Linux-only code here.

## Out of scope

The editor's window (05); MIDI and instruments; a presets browser; chain import and export
(named in the roadmap).
