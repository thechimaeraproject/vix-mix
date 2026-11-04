# 04 — Plugins

**Record — shipped 2026-11-04.** The catalog with a per-bundle scan cache, instances with
`sync` and the missing-plugin rule, the rack with the edit path of change request 1, state, and
editor sessions, tested with a real VST3 plugin: `dev check` green on Windows, and
`cargo test -p vixmix-plugins` green on Linux in an `ubuntu:24.04` container with vendored
sources. The contract as built is [reference/plugins.md](../reference/plugins.md);
[kit-vst3.md](../kit-vst3.md) is adopted. Deviations from the steps below, all deliberate:

- **The test plugin is this crate's example** (`examples/test_gain.rs`), the fallback the
  acceptance allows. Cargo builds a git dependency's example when asked for it by name, but
  `cargo test --workspace` (`dev check`, CI) builds only the workspace's own examples, so tests
  on the kit's would need a step no gate runs. The copy keeps the attribution and has fresh
  class ids, the name "Vix Mix Test Gain" and the sub-categories `Fx|Tools`, so the scan test
  checks a real classification (Utility) rather than the fallback.
- **Step 1 caches the whole per-bundle result, in a record beside the catalog.** `plugins.ron`
  gains `bundles` (path, time, effects, failure) next to the catalog's three lists, held in
  memory as `ScanCache`, because `Catalog` and `PluginEntry` are built as struct literals by
  `FakeHost` (not this lane's file) and could not gain a field without a change request;
  `Catalog::save` keeps the record already in the file. The time is the newest of the bundle,
  its binary and its `moduleinfo.json`, since a directory's own time does not move when an
  update rewrites the binary inside it. An unchanged failing bundle is reported again without
  being loaded. The walk is the kit's `scan`'s, rewritten here (its `collect` is private) so a
  bundle can be skipped before `scan_bundle` runs. One entry per class id.
- **Step 2** was already the prototype's list (lane 00); `Catalog::vendors()` was added for the
  browser's Maker list.
- **Step 4 added `sync`, notices, `set_param`, and `reap` by serial.** Step 7's "reported once"
  needs memory across calls, so it lives in `Instances::sync(inserts, catalog)`, with
  `retry_failed()` after a rescan; `take_notices()` carries what no return value does, as
  Vix's host does. A state that does not load is a notice, not a failure: the plugin runs from
  its defaults (Vix's rule). `reap` matches each processor to its controller by the rack
  entry's serial, because an instance id can be re-created before its old processor is home,
  and returns the ids it dropped; `create` retires a live instance of the same id (lane 00's
  kept both controllers). `tick` also drains the meters (`poll_output_changes`), whose ring is
  bounded. `set_param` is the mixer's own move of a parameter through the same edit ring, and
  how the edit path is tested end to end, since the test plugin never calls `performEdit`.
- **Step 5:** `Rack::add` keeps its arguments and now returns the instance's `EditSender`;
  entries carry a serial and the ring's consumer; `is_lent`, `len` and `is_empty` were added.
- **Step 6:** `EditorSession::tick` serves the Linux run loop and polls the events;
  `can_resize`, `check_size`, `size` and `set_scale_factor` were added. No test opens an
  editor: the test plugin has none.
- **Step 8 is change request 1 as the main session decided it:** a bounded ring per instance,
  its consumer travelling with the rack's forwarding handle, which merges the edits into the
  chain's changes and rebuilds the block's bus slices on the stack; the engine and `Lender` are
  untouched, and lane 06 wires `Instances::tick`. The reference also says what a VST3 insert's
  `ProcessorParams` should carry (no values), the question handoff 02's step 8 asks.
- **"Through an engine chain"** is the kit's `ProcessorChain::fill_with` lending from the rack
  exactly as the engine will, because the engine's chains are lane 02's, built alongside;
  handoff 06's integration test runs the test plugin through the engine itself.
- **Not done:** resolving a stored id through a bundle's `Compatibility` list (the catalog
  resolves by id), and replacing an instance whose plugin adds or removes a bus (the kit reads
  the buses once at `create`; the host would save, retire and create on `IoChanged`).
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
