# Plugins (`vixmix-plugins`)
**Owns:** `crates/vixmix-plugins/`

**Reference — the contract as built by [handoff 04](../handoffs/04-plugins.md).**
`vixmix-plugins` is everything between the kit's `chimaeraproject-vst3-host`
([kit-vst3.md](../kit-vst3.md)) and the mixer: the catalog the plugin browser searches and the
scan that fills it, the instances that live on the UI thread, the rack that lends their audio
halves to the engine's chains, the edit path from a plugin's own editor to its lent processor,
state for `setup.ron`, and editor sessions. It follows the kit's thread table: scan, load,
create, every controller call and every editor call on the UI thread; only a lent processor
reaches the audio side. It owns no window: the app makes the editor's window and hands its raw
handle in. `examples/test_gain.rs` is the test plugin the integration tests load (below).

## What it owns and what the others keep

| The plugins crate | The others |
|---|---|
| where to scan, scanning, classifying and caching (`plugins.ron`), favourites and recent | the window: the browser, its keys and its layout; the model: the extra scan directories (`Prefs::plugin_dirs`) |
| creating instances, loading and saving their state, retiring them in the kit's order, making them match a setup's inserts | the model: the `InstanceId`s and the state bytes in the setup; the host: when to sync, create and retire, and capturing state before a save |
| the `Rack`, implementing the engine's `Lender`, and the forwarding handle it lends | the engine: when to lend, clearing chains before filling them, what each insert's `ProcessorParams` carries |
| the edit ring from an instance's controller to its lent processor | the host: calling `Instances::tick` every UI tick |
| `EditorSession` over `Vst3Editor` | the window: the editor's winit window, its title and size |

## Scanning and the cache

`search_dirs(extras)` is where to look: the SDK's standard places (`default_search_dirs()`),
then the preferences' extra directories, each once. `Catalog::scan(dirs, host)` describes
every bundle; `catalog.rescan(dirs, host, &cache)` describes only the bundles that are new or
changed since `cache` was made and carries the catalog's favourites and recent over. Both walk
the directories as the kit's `scan` does (a directory named in `dirs` that is itself a bundle
counts, ordinary subdirectories are descended into, a bundle never is, entries in name order)
and list each bundle once.

A bundle is unchanged when its modification time equals the cached one, the time being the
newest of the bundle, its binary for this platform (`resolve_binary`) and its
`Contents/Resources/moduleinfo.json`, so an update that rewrites only the binary inside the
bundle is seen; a bundle whose time cannot be read is always described. An unchanged bundle
gives what it gave last time without the kit's `scan_bundle` running (which reads
`moduleinfo.json` or loads the plugin): its effects, or its failure, which is reported again.
A changed one is described through the kit; its warnings are logged.

Only audio classes whose sub-categories say `Fx` are kept (`ClassInfo::is_effect`), one entry
per class id (the first bundle in path order wins), sorted by name. The result is a
`ScanOutcome { catalog, failures, cache, described, reused }`: each failure is the bundle's
path and the kit's message, which ends with the fix.

`plugins.ron`, beside `setup.ron`, holds the catalog's three lists and the scan's per-bundle
record (`bundles`: path, time, effects, failure). `Catalog::load(dir)` reads the catalog and
`ScanCache::load(dir)` the record, both with the kit's tolerant load (no file is an empty
catalog and an empty record; every field defaults, so a file without the record loads).
`ScanOutcome::save(dir)` writes both; `Catalog::save(dir)` writes the catalog and keeps the
record already in the file, so a star or a recent pick saved later never costs the next rescan.
Both write atomically.

## The catalog

`Catalog { plugins, favorites, recent }`, each entry a `PluginEntry { id, name, vendor, kind,
bundle, path }` with `id` the `vst3:<hex>` a `PluginRef` stores and `bundle` the file name a
setup stores beside it. `PluginKind` is the prototype's Type list — `Eq` ("EQ"), `Dynamics`,
`Reverb`, `Delay`, `Distortion`, `Modulation`, `PitchVocal` ("Pitch & vocal"), `Restoration`,
`MultiEffect`, `Utility` — and `from_sub_categories` reads the plugin's sub-categories in its
own order, splitting on `|`, so its first meaningful one wins: `Pitch Shift` or anything
containing `vocal` → `PitchVocal`, `Tools` or `Analyzer` → `Utility`, nothing recognised →
`MultiEffect`; `Fx` itself says nothing.

`search(query, filter)` is the prototype's `listFor`: a query searches every plugin whatever
the filter, ranked name-starts-with, then name-contains, then vendor, then type, each group by
name; no query lists the filter (`All`, `Favorites`, `Kind`, `Vendor`) by name, or `Recent` in
its own order. `vendors()` is the browser's Maker list, each vendor with its count, by name.
`used(id)` puts a plugin at the front of the recent list (at most `RECENT_LIMIT`, 8);
`set_favorite` stars it.

## Instances

`Instances::new(HostContext, Arc<Rack>)`, UI thread only, holding the live instances, the
retired ones whose processors are not home yet, and a `Module` per bundle path (a bundle loads
once however many instances it makes).

- `create(instance, &PluginRef, &Catalog, state)` finds the bundle's path through the catalog
  (`PluginError::Missing`, "rescan plugins", when it is not there; nothing is racked), loads
  the module, creates the class's two halves, loads the state bytes into the controller, puts
  the processor in the rack with a new edit ring, and keeps the controller with the ring's UI
  end. An instance already live under that id is retired first. A state that does not load is
  not a failure: the plugin starts from its defaults and a notice says so.
- `sync(inserts, &Catalog)` makes the live instances match a setup's `inserts()`: it retires an
  instance no insert names, or whose insert now names another plugin id, creates every insert's
  instance that is not live (with its stored state), and reaps. An insert whose plugin cannot
  be made is reported **once**, as a notice whose message ends with the fix, and its insert
  passes audio through (the rack lends nothing for it, `Ok(None)`); it is tried again only after
  `retry_failed()`, which the host calls after a rescan. The failures kept are only those of
  inserts the last sync named. This is Vix's missing-plugin rule.
- `take_notices()` returns what went wrong that no return value carried: inserts `sync` could
  not make, states that did not load, edits dropped. At most the newest 32 are kept.
- `save_state(instance)` is the controller's `PluginState` encoded: the bytes a setup's insert
  stores. It reflects every edit the processor has heard.
- `set_param(instance, param, value)` is the mixer's own move of a parameter (a generic control
  for a plugin without an editor): it tells the controller (`set_param_normalized`, so the
  plugin's editor shows it) and sends the value through the edit ring.
- `tick()`, every UI tick, drains each controller: `PerformEdit` goes to the processor through
  the edit ring and is reported as `InstanceEvent::Edit { instance, param, value }` (for the
  host: the stored state is stale); `Dirty(true)` is `InstanceEvent::Dirty`; a restart with
  `RESTART_IO_CHANGED` moves the rack's generation and is `InstanceEvent::IoChanged`; gestures,
  a latency restart (the kit refreshed it) and editor requests are not the mixer's. Values
  waiting for room in a ring are flushed, and the meters the processors reported are drained
  into their controllers (`poll_output_changes`), since that ring is bounded too.
- `editor(instance)` is an `EditorSession` (closed) or `None` for a plugin without one.
- `retire(instance)` retires it in the rack and keeps its controller until its processor is
  home; `reap()` drops every retired processor that is home and then its controller, on the UI
  thread, matching the two by the rack entry's serial (an instance id re-created before its old
  processor came home is a second entry), and returns the ids it dropped. `retiring()` counts
  those still out.
- Dropping `Instances` retires and reaps everything; a processor still in a chain cannot be
  reaped, and the log says to drop the engine's graphs first.

## The rack

`Rack::new() -> Arc<Rack>` holds each instance's audio half, keyed by `InstanceId` and the
processor id it was made as (both, because an instance id that comes to name another plugin
must not get the old one's processor), with a serial per entry and the consumer end of the
instance's edit ring beside the processor. `add(instance, plugin_id, processor)` (UI thread,
state already loaded) retires any earlier entry for that instance and returns the
`EditSender`; `retire(instance)` stops it being lent; `reap()` hands back every retired
processor that is home, for the UI thread to drop before its controller (a rack `Instances`
fills is reaped through `Instances::reap`); `touch()` moves the generation (a plugin that
changed its buses); `holds` answers whether an instance is in the rack and not retired,
`is_lent` whether it is out in a chain, `len` how many entries (retired ones included) it has.
Every change moves the generation.

`impl Lender for Rack`: `lend` takes the processor and its ring out and returns a forwarding
handle whose `Drop` deactivates the processor and puts both back — `Ok(None)` for an instance it
does not hold (a missing plugin: the engine makes the insert a reported pass-through), `Busy`
while lent, `OtherPlugin` for a mismatched id. The rack keeps a weak reference to itself so a
handle made from `&Rack` can hold the `Arc` it needs to come home. The tick never touches the
rack: a lent processor is called through its handle with no lock. Vix's
[`rack.rs`](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-audio/src/rack.rs)
is the pattern, without its export reservation.

## The edit path

How an edit made in a plugin's own editor reaches the processor a chain is borrowing (change
request 1 in the [build map](../build-map.md), agreed with lane 02). The kit never short-cuts an
edit to the processor: `performEdit` becomes `ControllerEvent::PerformEdit` on the UI thread,
and the processor hears a value only in a block's `ParamChanges`. The setup stores no plugin
parameters (a plugin's parameters live in its state), so the engine's `ProcessorParams` cannot
carry it either.

- **One bounded single-producer, single-consumer ring per instance** (`rtrb`, `EDIT_RING` =
  256 changes), made by `Rack::add`. The producer, `EditSender`, stays with `Instances` on the UI
  thread and is fed by `tick()` (from `PerformEdit`) and `set_param()`. `send` clamps the value
  to 0..=1 and pushes it at offset 0. While the ring is full (the instance is in no running
  chain) the latest value of each parameter waits on the UI side, at most 256 parameters, and
  goes first once there is room, so the processor always ends up hearing the last value sent;
  past that an edit is dropped, counted, and said once as a notice.
- **The consumer travels with the forwarding handle.** It lives in the rack beside the
  processor; `lend` hands both to the handle, and the handle's `Drop` gives both back, so edits
  made while the instance is home wait in the ring and arrive with its next block anywhere.
- **The handle merges them into the block.** With no edit waiting, `process` is a plain
  forward. Otherwise it copies the chain's sparse changes (and their first-block flag) into a
  list sized in `lend` (the processor's parameters plus the chain's room plus a full ring), then
  as many edits as fit, oldest first, so an edit wins over the chain's value at the same offset;
  the rest wait for the next block. Because the nested bus slices of an `AudioProcessData`
  cannot be re-pointed (`&mut` is invariant), the block is rebuilt from reborrows on the stack,
  as the kit's `BusBuffer::with_process_data` builds one, for up to eight output buses of up to
  eight channels; a block past that, or one whose chain changes fill the list, runs with the
  chain's changes alone and the edits wait. Nothing allocates (`tests/no_alloc.rs`).
- **The engine is untouched** and the `Lender` contract unchanged. What the engine's
  `ProcessorParams` for a VST3 insert should carry is the instance and the processor id with no
  parameter values: the chain's first block sends every value it carries with `is_initial`, and
  a value there would override the state the plugin loaded.
- **The host wires it** by calling `Instances::tick` every UI tick (lane 06).

## Editor sessions

`EditorSession` wraps one instance's `Vst3Editor`: `open(RawWindowHandle) -> EditorSize`
(Win32, Xlib or Xcb; the kit refuses Wayland with the fix), `tick()` every UI tick (on Linux it
serves the plugin's run loop first, `RunLoop::poll`, then returns `poll_events()`:
`ResizeRequested` means resize the window, then `resize(size)`), `can_resize()`,
`check_size(size)` (the size a dragged window should snap to), `size()`, `set_scale_factor`,
`run_loop()` for a caller that serves it itself, `is_open()`, and `close()`, before the window
goes and before the instance is retired.

## The test plugin

`examples/test_gain.rs` is a copy of the kit's test plugin (itself derived from the `vst3`
crate's gain example; the attribution is in the file), here because `cargo test` builds the
examples of the workspace's own packages only. It is "Vix Mix Test Gain" by "chimaeraproject",
with fresh class ids and the sub-categories `Fx|Tools`; parameter 0 is the gain as its
normalized value, applied at every point's offset; parameter 1 "Level" is a read-only meter; its
component state is the gain as 8 bytes; it has no editor. `cargo test -p vixmix-plugins` builds
it as a `cdylib`, and `tests/common/mod.rs` lays it out as `VixMixTestGain.vst3` under
`CARGO_TARGET_TMPDIR`, one copy per test, failing with the build command if it is absent.

## Verification

`cargo test -p vixmix-plugins`, and `dev check` for the workspace. The unit tests:
sub-categories classify in the plugin's own order and every kind's label reads back; a query
ranks name start, name, vendor, type and ignores the filter; filters list by name, recent keeps
its own order and its limit; the Maker list counts each vendor by name; favourites and recent
survive `plugins.ron`; saving the catalog keeps the scan's record and a file without it still
loads; the extra directories follow the standard places once each; bundles are found as the
kit walks, never inside a bundle and never twice; an unchanged bundle (a failing one here) is
not described again and is reported again, a changed one is described; edits arrive in order
and a full ring keeps each parameter's latest value; the waiting list is bounded and counts
what it drops; a merge keeps the chain's changes first, keeps the first-block flag and leaves
what does not fit; a rebuilt block runs on the same samples; a lent instance comes home and
only one chain holds it (`Busy`, `OtherPlugin`); a retired instance is reaped only once it is
home, and dropped on the thread that reaps, not the one that lent it; an instance made again
retires the old entry and each comes home to its own serial; an edit reaches the lent processor
in its next block, whether sent while home or while lent; controller events become instance
events and edits go to the ring; a plugin the catalog lacks is refused with the fix and nothing
is racked; a missing plugin is reported once until a retry and lends nothing.

`tests/test_plugin.rs`, with the real test plugin: the scan finds it and files it under
Utility, `plugins.ron` round-trips it, an unchanged bundle is reused and one whose binary was
rewritten is described again; create, lend to a chain filled as the engine fills one, process,
move the gain through `set_param` and hear it in the next block, save, and a second instance
loaded from that state sounds the same from its first block (and its controller shows it),
while bytes that are not a state start the plugin from its defaults with a notice; a retired
instance lent on another thread is not reaped until that thread clears its chain, then drops on
the UI thread; `sync` makes the setup's instances and retires what it no longer names; the
plugin has no editor and `tick` drains it quietly. `tests/no_alloc.rs`: blocks through the lent
handle with one edit, several and none waiting allocate nothing.

What is not tested here: an editor in a real window (the test plugin has none; lane 05 opens
one), a plugin whose buses change, and the engine's own chains, which lane 02 builds; the lent
handle is proven through the kit's `ProcessorChain::fill_with`, which the engine uses.
