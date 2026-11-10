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
handle in. `examples/test_gain.rs` is the test plugin the integration tests load, with
`examples/test_abort.rs`, `examples/test_hang.rs` and `examples/scan_bundle.rs` for the scan's
child process (below).

## What it owns and what the others keep

| The plugins crate | The others |
|---|---|
| where to scan, scanning (in process, or a child process per bundle, both its sides), classifying and caching (`plugins.ron`), favourites and recent | the window: the browser, its keys and its layout; the model: the extra scan directories (`Prefs::plugin_dirs`); the host and the app: which way to scan, and the program the child is (`vix-mix --scan-bundle`) |
| creating instances, loading and saving their state, retiring them in the kit's order, making them match a setup's inserts | the model: the `InstanceId`s and the state bytes in the setup; the host: when to sync, create and retire, and capturing state before a save |
| the `Rack`, implementing the engine's `Lender`, and the forwarding handle it lends | the engine: when to lend, clearing chains before filling them, what each insert's `ProcessorParams` carries |
| the edit ring from an instance's controller to its lent processor | the host: calling `Instances::tick` every UI tick |
| `EditorSession` over `Vst3Editor` | the window: the editor's winit window, its title and size |

## Scanning and the cache

`search_dirs(extras)` is where to look: the SDK's standard places and, on Linux,
`/usr/lib64/vst3`, where Fedora's packages install them (`default_search_dirs()`), then the
preferences' extra directories, each once. `Catalog::scan(dirs, host)` describes
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

A failure can have a cause outside the bundle that its time does not show (a runtime library
installed later, a file locked during the scan), so `cache.forget_failures()` drops every
failed bundle from the record and the next rescan describes those again, reusing only the
bundles that worked. Which call when, for the host (06) and the menu (05):

- **At start:** `Catalog::load(dir)` and `ScanCache::load(dir)`, then
  `catalog.rescan(dirs, host, &cache)` and `outcome.save(dir)`. A failure stays cached, so a
  broken plugin is not loaded at every start.
- **The menu's Rescan plugins:** the last outcome's `cache.forget_failures()`, then
  `catalog.rescan(dirs, host, &cache)`, `outcome.save(dir)` and `Instances::retry_failed()`
  (then `sync`), so an insert whose plugin was missing is tried again.
- In both, the host calls `rescan_with` with the `Describe` its boot options name (the app's
  is a child process per bundle, below); `rescan` is `rescan_with` in process.
- **`Catalog::scan(dirs, host)`** describes everything and starts favourites and recent empty:
  for a first run without `plugins.ron`, or a test, never for a rescan.

Only audio classes whose sub-categories say `Fx` are kept (`ClassInfo::is_effect`), one entry
per class id (the first bundle in path order wins), sorted by name. The result is a
`ScanOutcome { catalog, failures, cache, described, reused }`: each failure is the bundle's
path and the kit's message, or what its child process did (below), and each ends with the fix.

### Loading a bundle in a child process

A plugin that aborts while its module loads (issue
[#26](https://github.com/thechimaeraproject/vix-mix/issues/26): a bridged plugin whose
`ModuleEntry` calls `std::terminate`) cannot be caught: it takes the process that loads it
down, and with it every bundle after it. So `catalog.rescan_with(dirs, host, &cache, &how)`
takes how to describe a bundle it cannot reuse, a `Describe`: `InProcess` (what `rescan` and
`scan` do, and the default) or `InChild(ScanCommand)`. With `InChild`, a bundle whose
`moduleinfo.json` is readable is described in this process, because the kit then runs none of
its code (the kit's `scan_bundle` with `ScanOptions::load_without_moduleinfo` off, whose one
refusal is a bundle it would have to load); every other bundle is loaded in a child process,
one at a time:

- **The command.** `ScanCommand { program, args, timeout }` runs `program`, then `args`, then
  the bundle's path; `ScanCommand::new(program, args)` waits `SCAN_TIMEOUT` (60 seconds), and
  `with_timeout` changes it (a test's). The app's is its own executable with `--scan-bundle`
  ([app.md](app.md)); this crate's tests use `examples/scan_bundle.rs`. stdin is closed and
  stderr inherited, so a plugin's own messages still reach the terminal or the journal; on
  Unix the child leads a process group of its own.
- **The child's side.** `answer_scan(bundle, host) -> i32` describes the bundle as an
  in-process scan does (loading it when it must), writes the description (its effects, or the
  kit's failure, and its warnings) in RON on one line of stdout that starts with `ANSWER_MARK`,
  after a newline that ends any line a plugin left unfinished, and returns 0; 1 when stdout
  would not take it. A plugin, and Wine under a bridge, write to stdout too, so the mark is how
  the answer is found. On Windows it turns the crash dialogs off first (`SetErrorMode` and
  `WerSetFlags`), so a plugin that aborts ends the child at once.
- **The parent's side.** A thread drains the child's stdout to its end, keeping only the
  newest marked line, at most 4 MiB of it, in a slot (latest wins); everything else is dropped
  as it streams past. The scan polls the child every 5 ms and kills it at the timeout (on Unix
  with its process group, so the helper processes a bridge starts go with it), and kills one
  that has answered but not ended 5 seconds later. An answer stands whenever there is one: it
  is written after the bundle was described and unloaded, so a crash after it is the process's
  own teardown and is logged. Without one, the bundle fails: "the plugin crashed while it
  loaded (signal 6, SIGABRT)" (on Windows an exception code, or exit code 3 for the C
  runtime's abort), "the plugin took longer than 60 seconds to load, and its scan was
  stopped", or "the scan's process ended without describing it (exit code N)", each followed
  by "so it was skipped and the other plugins were scanned; fix or remove the plugin …, then
  rescan plugins (menu, Rescan plugins)". An answer that does not read back, or one past the
  bound, fails the bundle the same way.
- **The cache covers it like any other failure.** A crash, a timeout or an end without an
  answer is the bundle's failure, kept in `plugins.ron` and said again while the bundle is
  unchanged, so a start does not load a broken plugin again, and the menu's Rescan plugins
  (`forget_failures`) tries it again. A child that could not be started or watched at all is a
  cause outside the bundle: it is reported and not kept, so the next scan tries the bundle
  again.

Known limits. Loading a plugin into a channel stays in process (the v1 decision), so a plugin
that crashes when it is instantiated still takes Vix Mix down. The scan still runs on the UI
thread before the window's first frame (issue
[#16](https://github.com/thechimaeraproject/vix-mix/issues/16)), now a process start slower
for each bundle without a `moduleinfo.json`. On Unix the child's own process group is not the
terminal's foreground group: Ctrl+C in a terminal stops Vix Mix but not a child loading a
plugin, which runs on until its plugin has loaded (one that never finishes stays until it is
killed by hand); and a terminal set to `stty tostop` stops a child whose plugin writes to it,
until the timeout kills it. A process a bridge starts in a session of its own (Wine's
`wineserver`) is not in the group and is left to end by itself, as it does. A crash in the
child is a crash of a `vix-mix` process: a desktop that collects core dumps (systemd-coredump,
and KDE's crash handler that reads them) may report it although the window runs on.

`plugins.ron`, beside `setup.ron`, holds the catalog's three lists and the scan's per-bundle
record (`bundles`: path, time, effects, failure). `Catalog::load(dir)` reads the catalog and
`ScanCache::load(dir)` the record, both with the kit's tolerant load (no file is an empty
catalog and an empty record; every field defaults, so a file without the record loads).
`ScanOutcome::save(dir)` writes both; `Catalog::save(dir)` writes the catalog and keeps the
record already in the file, so a star or a recent pick saved later never costs the next rescan.
Both write atomically, and refuse (`FsError::Write`) rather than write an empty file if the
catalog does not serialize; a bundle whose path is not UTF-8 is therefore reported as a failure
and never cached. Times are kept as seconds and nanoseconds since the Unix epoch.

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
Every change moves the generation. What a move means to the engine is change request 5 in
[the build map](../build-map.md): the engine lends each instance once and shares it between
its graphs, so on a move it asks again for every insert, keeps an instance it already holds
(`Busy` means still the engine's), takes a new instance under a retired id, and sets a held
instance up again only through `Engine::set_up_again`, which the host calls on
`InstanceEvent::IoChanged`.

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

Three more examples serve the scan's child process. `examples/test_abort.rs` and
`examples/test_hang.rs` are VST3 modules only as far as their entry points go: the hook the
kit's `Module::load` calls first on each platform (`InitDll`, `ModuleEntry`, `bundleEntry`)
and `GetPluginFactory` call `std::process::abort()`, or sleep forever. `cargo test` builds both
as `cdylib`s, and `install_example(dir, bundle_name, example)` lays any of the three plugins
out as a bundle. `examples/scan_bundle.rs` is the child for the tests, standing in for
`vix-mix --scan-bundle`: `answer_scan` on its first argument, with noise printed to stdout
around the answer (a line before it left without its end), so every scan through it proves the
answer survives what a plugin prints; `scan_child()` finds it.

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
is racked; a missing plugin is reported once until a retry and lends nothing. The child's
answer (`child.rs`): it is one line and reads back whatever its text holds (quotes, newlines,
a CRLF); noise around the marked line, lookalikes of the mark and a 3 MiB line of noise do not
disturb it, whole or in pieces of 4096, 7 or 1 bytes; the newest answer wins, a CRLF is
dropped and an answer without its newline at the end counts; an answer past the bound is said
and a flood of noise holds no memory; a crash (SIGABRT, or 0xC0000409 on Windows), a timeout,
an end without an answer at exit code 0 and a refusal at exit code 2 each fail the bundle with
what happened, "skipped", the fix and Rescan plugins, and only the crash says it crashed; an
answer stands when the process ends badly after it or is stopped, and one that does not read
back or is past the bound fails; a command waits 60 seconds unless told otherwise.

`tests/test_plugin.rs`, with the real test plugin: the scan finds it and files it under
Utility, `plugins.ron` round-trips it, an unchanged bundle is reused and one whose binary was
rewritten is described again, and forgetting failures keeps it reused; a bundle that failed and
was mended without its time moving keeps its cached failure at a start and is described again,
and found, after `forget_failures`, with the favourites kept; create, lend to a chain filled as
the engine fills one, process, move the gain through `set_param` and hear it in the next block,
save, and a second instance loaded from that state sounds the same from its first block (and
its controller shows it), while bytes that are not a state start the plugin from its defaults
with a notice; a retired instance lent on another thread is not reaped until that thread clears
its chain, then drops on the UI thread; `sync` makes the setup's instances and retires what it
no longer names; the plugin has no editor and `tick` drains it quietly. `tests/no_alloc.rs`:
blocks through the lent handle with one edit, several and none waiting allocate nothing.

`tests/scan_child.rs`, through the test child: with a bundle described by its
`moduleinfo.json`, the aborting plugin and the test plugin in name order, the scan finishes,
both good plugins are in the catalog (the one after the crash loaded in a child of its own)
and the aborting one is one failure naming it, saying it crashed, and the fix (on Windows a
crash dialog would have held the child until the timeout); a second scan with that record
reuses the failure and spawns nothing (its command cannot even start); after
`forget_failures` it is loaded again and fails again, alone. A plugin that never finishes
loading is stopped at a 2-second timeout, the scan goes on to the test plugin, and the
failure is kept. A bundle with a `moduleinfo.json` needs no child (described with a command
that cannot start), while one that must be loaded is a failure naming the command, not kept,
and found by the next scan with a child that runs. The child answers on one marked line among
its noise and exits 0.

What is not tested here: an editor in a real window (the test plugin has none; lane 05 opens
one), a plugin whose buses change, and the engine's own chains, which lane 02 builds; the lent
handle is proven through the kit's `ProcessorChain::fill_with`, which the engine uses.
