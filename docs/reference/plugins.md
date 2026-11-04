# Plugins (`vixmix-plugins`)
**Owns:** `crates/vixmix-plugins/`

**Reference — the contract as lane 00 wrote it; [handoff 04](../handoffs/04-plugins.md)
completes it.** `vixmix-plugins` is everything between the kit's `chimaeraproject-vst3-host`
([kit-vst3.md](../kit-vst3.md)) and the mixer: the catalog the plugin browser searches, the
instances that live on the UI thread, the rack that lends their audio halves to the engine's
chains, state for `setup.ron`, and editor sessions. It follows the kit's thread table: scan,
load, create, every controller call and every editor call on the UI thread; only a lent
processor reaches the audio side. It owns no window: the app makes the editor's window and
hands its raw handle in.

## What it owns and what the others keep

| The plugins crate | The others |
|---|---|
| scanning, classifying and caching (`plugins.ron`), favourites and recent | the window: the browser, its keys and its layout |
| creating instances, loading and saving their state, retiring them in the kit's order | the model: the `InstanceId`s and the state bytes in the setup; the host: when to create and retire |
| the `Rack`, implementing the engine's `Lender` | the engine: when to lend, and clearing chains before filling them |
| `EditorSession` over `Vst3Editor` | the window: the editor's winit window, its title and size |

## The catalog

`Catalog { plugins, favorites, recent }`, each entry a `PluginEntry { id, name, vendor, kind,
bundle, path }` with `id` the `vst3:<hex>` a `PluginRef` stores. `Catalog::scan(dirs, host)`
runs the kit's `scan` over the directories (the SDK's `default_search_dirs()` plus the
preferences' extras) and keeps the audio classes whose sub-categories include `Fx`, sorted by
name; it returns `ScanOutcome { catalog, failures }`, each failure a bundle's path and the
kit's message. `PluginKind` is the prototype's Type list — `Eq` ("EQ"), `Dynamics`, `Reverb`,
`Delay`, `Distortion`, `Modulation`, `PitchVocal` ("Pitch & vocal"), `Restoration`,
`MultiEffect`, `Utility` — and `from_sub_categories` reads the plugin's sub-categories in its
own order, splitting on `|`, so its first meaningful one wins: `Pitch Shift` or anything
containing `vocal` → `PitchVocal`, `Tools` or `Analyzer` → `Utility`, nothing recognised →
`MultiEffect`.

`search(query, filter)` is the prototype's: a query searches every plugin whatever the filter,
ranked name-starts-with, then name-contains, then vendor, then type, each group by name; no
query lists the filter (`All`, `Favorites`, `Kind`, `Vendor`) by name, or `Recent` in its own
order. `used(id)` puts a plugin at the front of the recent list (at most `RECENT_LIMIT`, 8);
`set_favorite` stars it. `load(dir)` / `save(dir)` read and write `plugins.ron` beside
`setup.ron` with the kit's tolerant load and atomic write. Scan caching by bundle modification
time is handoff 04's.

## The rack

`Rack::new() -> Arc<Rack>` holds each instance's audio half, keyed by `InstanceId` and the
processor id it was made as (both, because an instance id that comes to name another plugin
must not get the old one's processor). `add(instance, plugin_id, processor)` (UI thread, state
already loaded) retires any earlier entry for that instance; `retire(instance)` stops it being
lent; `reap()` hands back every retired processor that is home, for the UI thread to drop before
its controller; `touch()` moves the generation (a plugin that changed its buses); `holds`
answers whether an instance is in the rack and not retired. Every change moves the generation.

`impl Lender for Rack`: `lend` takes the processor out and returns a forwarding handle whose
`Drop` deactivates it and puts it back — `Ok(None)` for an instance it does not hold, `Busy`
while lent, `OtherPlugin` for a mismatched id. The rack keeps a weak reference to itself so a
handle made from `&Rack` can hold the `Arc` it needs to come home. The tick never touches the
rack: a lent processor is called through its handle with no lock. Vix's
[`rack.rs`](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-audio/src/rack.rs)
is the pattern, without its export reservation.

## Instances

`Instances::new(HostContext, Arc<Rack>)`, UI thread only. `create(instance, &PluginRef,
&Catalog, state)` finds the bundle's path through the catalog (`Missing`, "rescan plugins",
when it is not there), loads the module once per bundle path, creates the class's two halves,
loads the state bytes into the controller, puts the processor in the rack and keeps the
controller. `save_state(instance)` is the controller's `PluginState` encoded; `editor(instance)`
is an `EditorSession` (closed) or `None` for a plugin without one; `retire(instance)` retires it
in the rack and keeps its controller until `reap()` has dropped its processor, then drops the
controller. `tick()` drains every controller's events into `InstanceEvent`s: `Edit { instance,
param, value }` (the plugin's own editor moved a parameter), `Dirty(instance)`, and
`IoChanged(instance)` after moving the rack's generation. How an `Edit` reaches the lent
processor is change request 1 in the [build map](../build-map.md).

## Editor sessions

`EditorSession` wraps one instance's `Vst3Editor`: `open(RawWindowHandle) -> EditorSize`
(Win32, Xlib or Xcb; the kit refuses Wayland), `poll_events()` (`ResizeRequested`: resize the
window, then `resize(size)`), `run_loop()` (Linux: poll it every UI tick), `is_open()`,
`close()` (before the window goes).

## Verification

`cargo test -p vixmix-plugins`: sub-categories classify in the plugin's own order and every
kind's label reads back; a query ranks name start, name, vendor, type and ignores the filter;
filters list by name and recent keeps its own order and its limit; favourites and recent
survive `plugins.ron`; a lent instance comes home when its handle drops, a second lend while
lent is `Busy` and a mismatched id is `OtherPlugin`; a retired instance is reaped only once it
is home; a plugin the catalog lacks is refused with the fix and nothing is racked. Handoff 04
adds the kit's test plugin and the real scan, create, lend, process and state tests.
