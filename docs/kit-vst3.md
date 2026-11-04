# Kit VST3 host adoption

**Taken 2026-11-04 by the bootstrap ([handoff 00](handoffs/00-bootstrap.md)), at the kit's
release tag `kit-source.json` pins; pending [handoff 04](handoffs/04-plugins.md), which proves
it with a real plugin and flips this record to adopted.** Vix Mix hosts VST3 effects on every
channel's and both mixes' chains through the kit's `chimaeraproject-vst3-host` (imported as
`chimaeraproject_vst3_host`). The kit's
[vst3-host reference](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)
defines what the crate guarantees — its API, its thread table and the consumer recipe this
record follows; this record says what Vix Mix takes, what it keeps, which thread does what, and
what changed observably. It is a git dependency on the kit's release tag at the same tag as the
other three kit crates. Vix's
[kit-vst3.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/kit-vst3.md) is the
first consumer's record of the same crate.

## What Vix Mix takes

- **Scanning**: `scan` over the directories the catalog is given, keeping the audio classes
  whose sub-categories include `Fx` (`vixmix_plugins::Catalog::scan`).
- **Loading and the two halves**: `Module::load` (cached per bundle path), `Module::create`,
  `Vst3Processor` into the rack, `Vst3Controller` kept by `Instances` on the UI thread.
- **State**: `Vst3Controller::save_state` / `load_state` with `PluginState::encode` / `decode`:
  the bytes a setup's insert stores.
- **Controller events**: `drain_events` → `PerformEdit`, `Dirty`, and a bus change
  (`RESTART_IO_CHANGED`) that moves the rack's generation.
- **The editor**: `create_editor` and `Vst3Editor` behind `EditorSession`, opened with a raw
  window handle the app makes.

## What stays in Vix Mix

Where plugins are scanned from (the SDK's places plus the preferences' extras) and the
`plugins.ron` cache; how a setup stores a plugin (`vst3:<hex>` and the bundle's file name);
the rack that lends instances to chains, its generation, and that it never waits; the editor's
window (winit, on X11 under Linux, because the kit refuses Wayland handles); that plugins run
in process ([the charter](charter.md)).

## Which thread does what

| Thread | Calls |
|---|---|
| UI (the window's) | `scan`, `Module::load` / `create`, every `Vst3Controller` and `Vst3Editor` call, `Rack::add` / `retire` / `reap`, dropping a reaped processor and then its controller |
| the engine's preparing thread (the host's, today the UI thread) | `Rack`'s lending and every handle's drop; `set_bus_layout`, `setup_processing`, `set_active` through the chain |
| PipeWire's data thread | `Vst3Processor::process` through a lent handle, nothing else |

## What changed observably

Nothing yet: the bootstrap compiles the path against the crate and tests the rack with the
kit's reference `Gain`. Handoff 04 adds the kit's test plugin and records what a user sees.

## How you know it worked

`cargo test -p vixmix-plugins` (the rack's lending and reaping, a missing plugin refused with
the fix); handoff 04's acceptance with the test plugin: scan finds and classifies it; create →
lend → process → state saved → a second instance loads it → equal output.
