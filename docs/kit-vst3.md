# Kit VST3 host adoption

**Adopted 2026-11-04 by [handoff 04](handoffs/04-plugins.md), at the kit's release tag
`kit-source.json` pins; taken at the bootstrap ([handoff 00](handoffs/00-bootstrap.md)).** Vix
Mix hosts VST3 effects on every channel's and both mixes' chains through the kit's
`chimaeraproject-vst3-host` (imported as `chimaeraproject_vst3_host`), proven against a real
plugin on Windows and Linux. The kit's
[vst3-host reference](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)
defines what the crate guarantees — its API, its thread table and the consumer recipe this
record follows; this record says what Vix Mix takes, what it keeps, which thread does what, and
what changed observably. It is a git dependency on the kit's release tag at the same tag as the
other three kit crates. Vix's
[kit-vst3.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/kit-vst3.md) is the
first consumer's record of the same crate. [reference/plugins.md](reference/plugins.md) is the
contract built on it.

## What Vix Mix takes

- **Scanning**: `default_search_dirs()`, `scan_bundle` for each bundle the catalog's walk finds
  (the walk is the kit's `scan`'s, rewritten so a bundle can be skipped), `resolve_binary` for
  a bundle's modification time, `ClassInfo::is_effect` to keep the `Fx` classes
  (`vixmix_plugins::Catalog::scan` and `rescan`).
- **Loading and the two halves**: `Module::load` (cached per bundle path), `Module::create`,
  `Vst3Processor` into the rack, `Vst3Controller` kept by `Instances` on the UI thread.
- **State**: `Vst3Controller::save_state` / `load_state` with `PluginState::encode` / `decode`:
  the bytes a setup's insert stores.
- **Controller events**: `drain_events` → `PerformEdit` (into the instance's edit ring),
  `Dirty`, and a bus change (`RESTART_IO_CHANGED`) that moves the rack's generation;
  `poll_output_changes` every tick; `set_param_normalized` for the mixer's own moves.
- **The editor**: `create_editor` and `Vst3Editor` behind `EditorSession`, opened with a raw
  window handle the app makes, its run loop served each tick on Linux.
- **The test plugin**: the kit's `examples/test_gain.rs`, copied into `vixmix-plugins` with its
  attribution, fresh ids and `Fx|Tools`, because `cargo test` builds only the workspace's own
  examples.

## What stays in Vix Mix

Where plugins are scanned from (the SDK's places plus the preferences' extras) and the
`plugins.ron` cache with its per-bundle record; how a setup stores a plugin (`vst3:<hex>` and
the bundle's file name, no parameter values); the rack that lends instances to chains, its
generation, and that it never waits; **the edit path**: the kit hands a `PerformEdit` to the
consumer's model, and Vix Mix's model keeps no plugin parameters, so each instance has a
bounded ring from its controller to the processor the rack lends, merged into the block's
changes by the forwarding handle; the editor's window (winit, on X11 under Linux, because the
kit refuses Wayland handles); that plugins run in process ([the charter](charter.md)).

## Which thread does what

| Thread | Calls |
|---|---|
| UI (the window's) | `scan_bundle`, `Module::load` / `create`, every `Vst3Controller` and `Vst3Editor` call, `Rack::add` / `retire` / `reap`, the edit ring's producer, dropping a reaped processor and then its controller |
| the engine's preparing thread (the host's, today the UI thread) | `Rack`'s lending and every handle's drop; `set_bus_layout`, `setup_processing`, `set_active` through the chain |
| PipeWire's data thread | `Vst3Processor::process` through a lent handle, which drains the edit ring first; nothing else |

## What changed observably

Nothing a user sees yet: the window's browser is lane 05's and the host's wiring lane 06's.
What a user will see, built here: a rescan describes only the bundles that changed since the
last one; a plugin the setup names that is not installed is reported once with the fix and its
insert passes audio through; a saved state that will not load starts the plugin from its
defaults with a notice; a knob turned in a plugin's own editor is heard in the next block.

## How you know it worked

`cargo test -p vixmix-plugins` on Windows and Linux (both legs of CI run it), with the test
plugin: the scan finds it and files it under Utility; create → lend to a chain → process → the
gain moved and heard in the next block → state saved → a second instance loads it → equal
output; a retired instance comes home and drops on the UI thread; nothing allocates in the lent
handle with edits merged. [reference/plugins.md](reference/plugins.md) § Verification lists
every test.
