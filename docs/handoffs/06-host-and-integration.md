# 06 — Host and integration

**Record — shipped 2026-11-04.** `RealHost` is the orchestrator: it boots in order (the setup,
the plugins rescanned against their cache, every insert's instance with its state, the engine
prepared, then the backend with the setup's graph), keeps PipeWire for the whole run on Linux
and routes every app the moment it appears and again whenever the setup moves it, acts on each
command's `Applied` part by part, debounces the save, keeps a removed insert's instance for its
undo, surfaces pass-throughs and notices, and shuts down in the kit's order; `FakeHost` stays
the window's fixture. The contract is [reference/host.md](../reference/host.md); the first real
test is [cookbook/by-hand-pass.md](../cookbook/by-hand-pass.md). `dev check` is green on
Windows, and the Linux gate (fmt, clippy `-D warnings`, `cargo test --workspace` with every
one of the host's integration tests) is green in the `ubuntu:24.04` container with the headless
daemon. **The acceptance's first bullet is the user's by-hand pass on Fedora**, which ran on
2026-11-10 as hours of daily use ([handoff 09](09-round-1-fixes.md) holds what it found); no
agent can hear a mic in headphones, and its flow is proven headlessly instead by
`tests/headless.rs`. Deviations from the steps below, each with its reason:

- **Change request 2 is built, in the model** (`RestoreChannel { at, channel }` and
  `RestoreInsert { to, insert }`, refused with `NotRemoved` and `NoPlace`). The toast's undo
  could not live without it: `AddChannel` appends at the right end under a new id, so a channel
  could not come back in its place, its sink would be renamed, and the engine's take-back of an
  instance named again (built for exactly this) would never be reached. The property test draws
  the undos from what each sequence removed. The host hands the undo out as
  `HostEvent::Removed { message, undo }` and `FakeHost` does the same, so the window's toast
  applies a command it was given rather than building one.
- **Change request 8, in the backend**: `Backend::disconnect` returned on the quit before
  writing the routings waiting beside it, so step 6's "clear routings, then disconnect" lost
  them (measured: the app's `target.object` kept naming the sink that went). The control thread
  now writes them and leaves after a core roundtrip, at most a second. Additive; its test fails
  without the change.
- **Step 1: `BootOptions` gained `audio` and `plugin_dirs`.** `AudioBackend::Null` is the
  default so no test ever routes a desktop's apps (the backend's tests keep the same rule);
  the app asks for `AudioBackend::APP`. Change request 7's interim banner is gone: on Linux the
  state is the backend's, and a PipeWire that refuses is asked again every 5 seconds.
- **Step 1 rescans the plugins at boot**, the plugins crate's start sequence, before the
  window opens (the scan belongs to the UI thread); `plugins.ron` is written only when the scan
  learned something, so "booting writes nothing" still holds for an unchanged start.
- **Change request 9, in the backend: a new quantum is asked for in place.** `node.latency`
  was set only when a stream was made, so the first build changed the quantum by connecting
  again, which destroyed and made again every node of ours; the verifier measured an app routed
  back into its channel then erring every cycle and silent (3 of 3 runs), and a recorder of the
  Master source loses its link. `Backend::set_quantum` updates `node.latency` on every live
  stream (`pw_stream_update_properties`) and keeps it for every stream made later. Additive;
  its test fails without the update. The host's test changes the quantum while the app plays,
  a link made by hand stands and Master is recorded: nothing is made again, the app stays heard,
  and the recording has one gap of 1024 frames at the change (the rings growing to one new
  quantum ahead), measured every run.
- **Step 3's save is debounced 500 ms and capped at 5 s** after the first unsaved change, so a
  knob turned for a minute still reaches the disk; an editor's edit and a dirty plugin make it
  due.
- **Step 4 makes instances with `Instances::sync`** over the setup's inserts and the removed
  ones kept for an undo, rather than one `create` and `retire` at a time, which gives Vix's
  missing-plugin rule (said once, a pass-through, tried again after a rescan) for free. The
  state of a removed or copied insert is taken from its live instance before the command,
  worked out on a copy of the setup so a refusal changes nothing. A removed instance is kept
  for `UNDO_TOAST` (5 s) and a second, at most 64 at once.
- **Step 5 surfaces problems and notices through the trait**: `HostEvent::Problems` with
  `Host::problems()` (the engine's `InsertProblem`, re-exported), and every notice as a
  `Toast`. The window shows them only in its banner line (`app.rs`), as agreed with lane 05's
  brief; the slot marks and toasts are lane 05's.
- **`BackendState` gained `Idle { reason }`**, and `Disconnected` has a banner. The backend's
  `TickSource` is a state the window must show: `Stopped`, or the Master source with nothing
  recording it (the engine has not ticked for 250 ms), is idle with what would drive the mixer;
  Master out driving while the setup names a device for Monitor is a toast, said once for that
  device until Monitor plays again (a first start before Monitor's device is picked, or a
  reconnect while it stays away, says nothing).
- **The trait gained `problems`, `set_favorite`, `rescan_plugins` and `next_poll`.** The
  browser's stars and the menu's Rescan plugins (lane 05, steps 5 and 6) need the first three,
  since only the host holds the catalog. `next_poll` because the window idles when nothing
  animates, and the host has work that comes by itself (PipeWire's news, the debounced save, an
  undo running out): the window polls when the host asks, without drawing unless there is news.
- **Step 6's order, refined**: the final save comes before the instances are retired, not
  after — taking a plugin's state needs its live controller, and a plugin that crashes in its
  teardown must not cost the setup. The graphs are dropped by preparing an empty snapshot
  (every lent instance comes home on the UI thread, explicitly) rather than by dropping the
  engine.
- **Step 8 is three test binaries.** `tests/null_backend.rs` (both legs) runs the plugins
  crate's test plugin through a channel chain with MON on and MST off on the null backend,
  through a save and a reload, a plugin moved while it plays (saved by the autosave and by a
  quit at once, and sounding so after a relaunch), a removal and its undo, and a copy, fed
  through `RealHost::null_input` and moved through `RealHost::set_param` (a test hook on
  `Instances::set_param`, not in the trait). `tests/headless.rs` (Linux, the brief's adaptation
  of the Fedora bullet) runs the same flow on the headless daemon with a `pw-play` app, the null
  sink and `pw-record` of the Master source, the plugin moved before a quit at once, and a
  relaunch; and a quantum change while the app plays and Master is recorded. `tests/refused.rs`
  (Linux) points `PIPEWIRE_REMOTE` at no daemon: the null backend with PipeWire's refusal, and
  PipeWire taken at the next ask once it answers. Found on the way: relaunched within
  milliseconds of the quit, `pw-play` in the container broke (its node erred every cycle and
  went away); a second player was heard at once, and one relaunched a second later was fine, so
  the test relaunches once the player is back on the default device, as a person would. The
  by-hand pass watches for it (step 9).
- **Two sentences of [reference/app.md](../reference/app.md)** (pacing, the banner line) and
  its `app.rs` row changed with the wiring in `app.rs` they describe; `ui.rs` is untouched — the
  banner line hands it an `Absent { reason }` for a note. That is the brief's stopgap, and lane
  05 (or the main session at merge) removes it: `Running::banner()`'s toast and problem
  fallback gives way to lane 05's toasts and slot marks, and app.md's banner-line sentence goes
  back to the backend's banner alone. The poll pacing (`next_poll`, `poll_at`) stays.
- **Shared docs beyond the conflict rules' list**: the Cookbook row of `docs/README.md`'s trust
  table said lane 06 "writes the next"; it now says it wrote the by-hand pass. The Reference
  index line for `host.md` names what it now covers.
- **For the main session at merge, outside this lane's files**: `README.md`'s Run it block and
  `CLAUDE.md`'s Commands block still say `cargo run -p vix-mix` opens the window "on the real
  host (no backend yet: a banner says so)". It now plays through PipeWire on Linux and routes
  the desktop's apps while it runs (the banner says so only when PipeWire refuses or nothing
  drives the mixer), shows the no-backend banner elsewhere, and scans the VST3 folders before
  the window opens on every platform (a first start with many plugins takes a while). Change
  requests 8 and 9 wait for agreement in [build-map.md](../build-map.md) § Change requests.
- **Not done**: a first start does not pick Monitor's device (the backend does not report the
  default sink, so the banner asks the user to); the boot scan has no progress display; an
  `IoChanged` plugin is set up again on the same buses, the kit reading buses once at `create`
  (handoff 04's note). Fedora installs native VST3s in `/usr/lib64/vst3`, which the kit's
  standard places do not list: the cookbook says to add it in the preferences.
**Needs:** 01, 02, 03, 04. **Runs alongside:** 05's later steps. **Size:** L.

## Goal

One process that boots, routes, mixes, saves and shuts down in the right order.

## Read first

- [reference/host.md](../reference/host.md): the contract as lane 00 left it.
  `crates/vixmix-host/` already has the `Host` trait, `HostEvent`, `BackendState`,
  `MeterFrame` / `StatsFrame`, `RealHost` (boot, instances from the setup, the null backend
  pumped per poll, `apply` acting on `Applied`, `save_now` with the unreadable-file rule, editor
  sessions) and `FakeHost`. By the time this lane starts, 01–04 have replaced their skeletons.
- Every other reference ([model](../reference/model.md), [engine](../reference/engine.md),
  [pipewire](../reference/pipewire.md), [plugins](../reference/plugins.md),
  [app](../reference/app.md)) as shipped, and the handoffs' status lines for what each lane
  changed.
- [build-map.md](../build-map.md) § Change requests (request 1: wire the edit path the lanes
  agreed).
- The kit's [vst3-host.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)
  § Lending (teardown order: stop the preparing thread and drop its chains, then each instance,
  processor before controller) and
  [fs.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/fs.md).
- Vix's [docs/cookbook/by-hand-pass.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/cookbook/by-hand-pass.md)
  (the shape of a by-hand cookbook).

## What this lane owns, what others keep

It owns the orchestration: boot and shutdown order, acting on `Applied` and on backend events,
routing apps to channels, the autosave, instance lifecycle, the null-backend fallback, and the
wiring in `app/src/main.rs` / `app.rs` that picks the host. Every other crate keeps its own
behaviour; the window keeps drawing (05).

## Steps

1. **`RealHost` boot:** load `setup.ron` and `plugins.ron`; create the instances with their
   state; `Engine::prepare`; `Backend::connect` on Linux, or the `NullBackend` elsewhere (with
   the banner); apply the `BackendGraph` derived from the setup; on
   `BackendEvent::Connected { rate, .. }` call `Engine::set_rate(rate)` (change request 5).
2. **Routing:** on `AppStreamAdded`, match the app against the channels (`AppRef::matches`) and
   `route` it; an app no channel claims is returned to the default when it appears (the
   apply-on-appear rule, because WirePlumber restores what was written).
3. **On `SetupChanged`** decide re-prepare / re-apply the graph / re-route / save, from
   `Applied`; the save debounced 500 ms, atomic.
4. **Instance lifecycle** on insert add, remove and move (a move is the same instance, never a
   re-create); `SetInsertState` before every save and on removal, so an undo brings an insert
   back as it was; the host keeps a removed channel or insert until its undo toast dies. On
   `InstanceEvent::IoChanged(instance)` call `Engine::set_up_again(instance)`: the engine
   keeps a held instance across the rack's generations and sets it up again only when asked
   (change request 5).
5. **Every tick:** `Engine::reap` and `Instances::reap`; editors polled; `Engine::problems()`
   (inserts playing as pass-throughs, with why) and `Instances::take_notices()` surfaced in
   the window.
6. **Shutdown:** clear routings, `Backend::disconnect`, drop the graphs, retire the instances
   (processor before controller, on the UI thread), the final save.
7. **`FakeHost`** stays for the window's tests.
8. **A headless integration test:** the null backend plus the kit's test plugin through a
   channel chain with MON on; meters move; save → reload → the same setup.
9. **`cookbook/by-hand-pass.md`** for the first real test on Fedora, with its index entry. It
   builds the user's Wave Link scene (Main Mic, Index Mic, Music, Web, System, Voice chat) and
   then checks, each as a step that says what to see:
   - a mic through a plugin to the headphones;
   - an app's routing persists across the app's restarts (quit and relaunch Spotify: it comes
     back in Music);
   - "Vix Mix Master" is recordable by Discord and by OBS;
   - Master previewed through Monitor (the ear button);
   - save and restore, plugin state included (quit and relaunch Vix Mix);
   - `pw-top` shows no xruns at 256;
   - the latency figure, measured and recorded in
     [reference/pipewire.md](../reference/pipewire.md).

   The plugins: FabFilter, iZotope and Kilohearts have no Linux builds, so the pass uses native
   Linux VST3s (TDR, u-he, werman, Airwindows, Surge XT, Dragonfly, LSP, ZL) and yabridge-bridged
   Windows VST3s, both appearing as bundles.
10. **Docs:** complete [reference/host.md](../reference/host.md); `CLAUDE.md` § Status;
    roadmap rows flipped.

## Files you own

`crates/vixmix-host/**`, the host wiring in `app/src/main.rs` and `app/src/app.rs` (agree any
other `app/` change with lane 05), `docs/reference/host.md`, `docs/cookbook/by-hand-pass.md`
(and its lines in `docs/cookbook/README.md` and the index), this handoff, your rows in
`docs/roadmap.md`, one paragraph in `CLAUDE.md` § Status.

## Acceptance

- `cargo run -p vix-mix` on Fedora (or the VM): a mic channel audible in headphones through a
  plugin; Spotify ticked into Music and heard in Monitor; Master recorded by another app from
  "Vix Mix Master"; quit and relaunch restores everything, plugin state included.
- The headless integration test above, on both legs.
- `reference/host.md` complete; `CLAUDE.md` § Status paragraph; roadmap rows flipped;
  `dev check` green.

## Out of scope

Packaging (08); the kit lifts (07); anything inside a crate another lane owns.
