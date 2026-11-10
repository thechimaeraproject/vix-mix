# Host (`vixmix-host`)
**Owns:** `crates/vixmix-host/`

**Reference — the contract as [handoff 06](../handoffs/06-host-and-integration.md) built it on
2026-11-04.** `vixmix-host` is the orchestrator: one `Host` owns the setup, the engine, the
plugin instances and the backend, and is the only thing the window talks to. `RealHost` wires
them in the order this page sets out (boot, routing, acting on each command's `Applied`, the
debounced autosave, the instances' lifecycle, shutdown); `FakeHost` serves prototype v5's
example data with simulated meters and no audio, for the window's tests and the look gate.
Every `Host` method runs on the UI thread; the host reaches the tick only through
`Engine::prepare`, `reap`, `set_rate` and `set_up_again` and the meters, stats and problems
it reads. It draws nothing and owns no window: the app hands it raw window handles for
editors.

## What it owns and what the others keep

| The host | The others |
|---|---|
| boot and shutdown order, acting on `Applied`, the debounced save, moving an unreadable file aside | the model: what a command means and what it changed ([model.md](model.md)) |
| choosing the backend, deriving its graph from the setup, routing every app as it appears, the state the banner shows | the backend: the streams, the rings, the routing writes ([pipewire.md](pipewire.md)); the engine: the tick ([engine.md](engine.md)) |
| when instances are made, kept for an undo and retired; capturing their state before a save | the plugins crate: how ([plugins.md](plugins.md)) |
| when to scan, the favourites and recent it keeps in `plugins.ron` | the plugins crate: the scan and its cache |
| `FakeHost`, the window's fixture | the window: drawing all of it ([app.md](app.md)) |

## The trait

```rust
pub trait Host {
    fn setup(&self) -> &Setup;
    fn apply(&mut self, cmd: Command) -> Result<Applied, Refused>;
    fn poll(&mut self) -> Vec<HostEvent>;
    fn meters(&self) -> MeterFrame;
    fn stats(&self) -> StatsFrame;
    fn catalog(&self) -> &Catalog;
    fn apps(&self) -> &[AppPresence];
    fn devices(&self) -> &[DeviceInfo];
    fn open_editor(&mut self, instance: InstanceId, parent: RawWindowHandle) -> Result<EditorSize, HostError>;
    fn tick_editors(&mut self) -> Vec<(InstanceId, EditorEvent)>;
    fn close_editor(&mut self, instance: InstanceId);
    fn save_now(&mut self);
    fn problems(&self) -> &[InsertProblem];
    fn set_favorite(&mut self, plugin_id: &str, favorite: bool);
    fn rescan_plugins(&mut self);
    fn next_poll(&self) -> Option<Duration>;
}
```

`HostEvent`: `SetupChanged`, `Toast(String)` (a message that says what to do: an unreadable
file, a plugin that cannot be made, a state that did not load, scan failures, Monitor's device
gone), `Apps`, `Devices`, `Backend(BackendState)` (the first poll always reports it),
`PluginsRescanned`, `EditorClosed(InstanceId)`, `Problems(Vec<InsertProblem>)` (the inserts
playing as pass-throughs changed; `problems()` lists them, each with its strip, instance,
plugin id and a message that ends with the fix), and `Removed { message, undo }`: a channel or
an insert was removed, the window shows `message` ("Removed channel Music", "Removed Pro-Q 3
from Mic") with an Undo for `UNDO_TOAST` (5 s), and Undo applies `undo`, a `RestoreChannel` or
`RestoreInsert` ([model.md](model.md) § Commands) that puts it back as it was, plugin state
included. `InsertProblem` is the engine's, re-exported.

`BackendState`: `Absent { reason }` (no backend; the null backend runs the engine, silent),
`Simulated` (`FakeHost`), `Connecting`, `Running { rate, quantum }`, `Idle { reason }`
(connected, but nothing drives the mixer) and `Disconnected` (the daemon went away; the
backend reconnects). `banner()` is the reason of `Absent` and `Idle`, and `DISCONNECTED` for
`Disconnected`; `NO_BACKEND` is exactly "No audio backend on this platform". `live()` (meters
move, which sets the window's 60 Hz repaint) is `Simulated` or `Running`. `MeterFrame
{ channels: [ChannelLevels; 8], monitor, master }` holds linear peaks (`Stereo { left, right
}`) by channel position, copied once per frame; `StatsFrame { rate, quantum, dsp_load, xruns
}` with `quantum_ms()`. `AppPresence { app, running }`. `tick_editors` pairs each event with
its instance, because the window keeps one editor window per instance.

`next_poll` is how long the window may wait before it polls again although nothing happened on
its side: `None` when the host waits for nothing. `set_favorite` stars a plugin and keeps it;
`rescan_plugins` is the menu's Rescan plugins.

## `RealHost`

`RealHost::boot(BootOptions { config_dir, debug_build, audio, plugin_dirs })`. The directory
is `config_dir` or `persist::profile(debug_build)` (`HostError::NoConfigDir` when the platform
names none). `audio` is an `AudioBackend`: `Null` (the default, so a test never touches a
desktop's audio) or `System { app_name }` (`AudioBackend::APP` is `vix-mix`, what the app
asks for; a test against the headless daemon names its own). `plugin_dirs` is `None` for the
SDK's standard places and the preferences' extra directories, or `Some` for only these.

### Boot

In this order:

1. `setup.ron` and `plugins.ron` are read (an unreadable file is a toast and the default; an
   unreadable `setup.ron` is moved aside at the first save), and the scan's record
   (`ScanCache::load`).
2. The plugins are rescanned against the record: only a new or changed bundle is described.
   `plugins.ron` is written only when the scan described something or a bundle went, so a
   start that learned nothing writes nothing. Scan failures are logged and make one toast that
   names the first two and counts the rest. The scan runs on the UI thread before the window
   opens (the kit's thread table); a first start with many plugins takes a while, and later
   starts reuse the record.
3. The engine is made (48 kHz until the backend says, `MAX_QUANTUM`), the rack and the
   instances; `Instances::sync` makes every insert's instance with its stored state (a plugin
   the catalog lacks is said once and its insert passes audio through, Vix's rule); the engine
   is prepared, so it has a graph before any tick asks.
4. The backend starts with the setup's graph. On Linux with `System`, `vixmix_pipewire::Backend`
   is connected as `app_name`, asking for the setup's quantum, and kept for the whole run; if
   PipeWire refuses, the null backend runs with "No audio backend: " and the refusal, and
   PipeWire is asked again every 5 seconds. On any other platform `System` is the null backend
   with `NO_BACKEND`; `Null` says "Started without an audio backend: the mixer runs silent".

Booting writes nothing but `plugins.ron`, and that only when the scan learned something.

### Acting on a command

`apply(cmd)` runs the model's `apply` and then acts on `Applied`, part by part:

- **Before**, a command that removes or copies inserts (`RemoveChannel`, `RemoveInsert`,
  `MoveInsert` with `copy`) takes each live instance's current state into the setup, so the
  undo and the copy carry the plugin as it sounds now; this is worked out on a copy of the
  setup, so a refused command changes nothing.
- **`removed`**: the instances stay live, kept for the undo for `UNDO_TOAST` and a second (at
  most 64 kept; past that the oldest is let go early, and its undo makes it again from the state
  it was removed with); their editors close (`EditorClosed`); `Removed { message, undo }` is
  reported. Once the time is up the next poll retires them.
- **`created`**: an instance kept for an undo is simply named again (the engine takes it back,
  runtime state and all); anything else is made by `Instances::sync` with its stored state.
  `AddInsert` also puts the plugin at the front of the catalog's recent list and keeps it.
- **`engine`** (or any instance made or retired): the snapshot is prepared.
- **`graph`**: the backend's graph is applied — the channels in order (an app sink, or a
  capture on its device), Monitor's and Master's devices, the Master source always published.
  A new quantum is asked for in place first (`Backend::set_quantum`, change request 9): no
  node is made again, so every app stays in its channel and every link stays (a recorder's of
  the Master source, one made by hand); a larger quantum costs one short gap while the
  backend's rings grow (measured at 256 to 512: one gap of 1024 frames on the Master
  source).
- **`graph` or `routing`**: every app playing now whose sink changed is routed again, after
  the graph (a routing names a sink of the newest graph). A rename changes the channel's sink
  name, so its apps are routed to the new name (the backend also moves them itself); a removed
  channel's apps go home to the default; a restored channel's apps come back to it.
- **`save`**: `SetupChanged`, and the autosave is due 500 ms after the last change, never more
  than 5 s after the first unsaved one.

### Routing

On PipeWire, every app stream is routed **the moment it appears** (`AppStreamAdded`): to the
`node.name` of the channel whose source names it (`AppRef::matches`: portal app id, else
binary, else name; `<app>.ch<id>.<slug>`), or home to the default with `route(node, None)`.
This is the apply-on-appear rule: WirePlumber restores routings it remembers, unreliably, so
the host's rule is the routing of record. The host remembers its newest routing per app
stream and routes again only when it changes. After a reconnect the backend reports every app
again and each is routed again. At most 128 devices and 256 app streams are followed.

### Every poll

`poll()` pumps the null backend one quantum (256 frames), or handles PipeWire's events (at most
1024 a poll): `Connected { rate }` calls `Engine::set_rate`, `TickSource` and `QuantumChanged`
update the state, Master out or the Master source driving the tick while the device the setup
names for Monitor is not among the devices the backend reports is a toast, said once for that
device until Monitor plays again (not at each reconnect while it stays away, and never on a
first start before Monitor's device is picked): "Monitor's output device is not there…" when
Master out plays, or, when no mix plays to a device, "Monitor's output device “Headphones” is
not there and no mix plays to a device: the mixer waits for it…", which names the device, says
the mixer runs only while an app records “Vix Mix Master” until then, and says to plug it in or
pick another output for Monitor. Monitor's stream refused while the backend reports its device
says neither: both say the device is not there, and the backend connects that stream again each
second while the device is there, so the tick can move back and forth each second; devices and
app streams update `devices()` and `apps()`, and each app that appeared is routed.
Then `Engine::reap` and `Instances::reap`; `Instances::tick` (an editor's edit goes to its
processor and makes the autosave due; a dirty plugin is `SetupChanged` and due;
`IoChanged(instance)` calls `Engine::set_up_again(instance)`); the instances kept for an undo
whose time is up are retired; the engine is prepared again if the rack's generation moved past
the running graph's; the instances' notices become toasts; `Engine::problems()` is compared
and reported as `Problems` when it changed; the backend's state is reported when it changed;
the save runs when due.

**The state on PipeWire**: `Disconnected` while the daemon is gone; `Connecting` until the
backend says which output drives the tick; `Idle` with "The mixer is stopped…" when nothing
can (`TickSource::Stopped`), or with "No mix plays to a device, so the mixer runs only while an
app records “Vix Mix Master”…" when only the Master source could and the engine has not run a
cycle for 250 ms; `Running { rate, quantum }` once the tick has run. While the device the setup
names for Monitor is not among the devices the backend reports, both `Idle` banners name it
instead and say the mixer waits for it: with nothing driving it, "The mixer is stopped:
Monitor's output device “Headphones” is not there and no other output runs…" (`Stopped` comes
so when the Master source's stream was given up on too, and for a moment at each connection
before the streams are made); with only the Master source, the words of its toast above. The
name follows each tick source, device and setup change (another device picked for Monitor is
the one named while it is not there: the backend says nothing new when the tick stays where it
is) and goes once Monitor plays; a device the backend reports is never called missing.

`apps()` lists every app playing now, then every app a channel names that is not playing,
then every other app seen since start (the newest 64), each once. `stats()` is the engine's:
the rate, the last cycle's quantum, its DSP load and the xruns the backend counted.

`next_poll()` on `RealHost` is the earliest of: the autosave's due time, the first kept
instance's end, 50 ms while an editor is open or an instance is still retiring, 50 ms while
PipeWire is kept (its news comes by itself), and the next ask on Linux when PipeWire refused.
On the null backend with nothing pending it is `None`, so the window can idle.

### Saving

`save_now` takes every live instance's state into the setup (`SetInsertState`), creates the
directory, moves an unreadable `setup.ron` aside to `setup.ron.unreadable` rather than saving
over it, and writes atomically (`persist::save`). A failure is a toast; the next change tries
again.

### Shutdown

Dropping `RealHost` shuts down in this order: every editor closes (before its window and its
instance go); every app the host routed to a channel is routed home and the backend leaves
(`Backend::disconnect` writes those routings and waits for the daemon's roundtrip first, change
request 8), so no tick runs again; the final save, if anything changed, while every plugin is
still live to give its state; an empty graph is prepared, so every lent instance comes home on
this thread; then every instance is retired and reaped here, processor before controller (the
kit's [vst3-host.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)
§ Lending). The app drops the host before its window.

For tests, `config_dir()`, `backend()`, `engine()`, `is_live(instance)` and
`null_input(index)` (the null backend's input for a channel, which every later poll plays)
read and feed it from outside, and `set_param(instance, param, value)` moves a plugin's
parameter as the mixer's own control would (`Instances::set_param`: the controller shows it,
the processor plays it from its next block, and the autosave comes due), which is how a test
changes a plugin's state while it plays.

## `FakeHost`

`FakeHost::new()` (or `with_seed`) serves `vixmix_model::example::setup()`, the example's 13
apps with their running flags, its 4 inputs and 4 outputs, and its 42-plugin catalog with
favourites and recent. Its meters are the prototype's `tick`: each channel chases a level its
style's `target` picks every 110–490 ms (speech, game, steady, low, burst), easing 0.22 a frame
with jitter; a channel whose apps are none of them running is silent; a send scales the level by
its fader's linear gain, a muted send reads zero, and each mix reads the loudest unmuted send
into it times its own level (Monitor reads the Master sum while previewing). The randomness is a
seeded xorshift, so a seed gives the same signal every run; `advance(ms)` steps it a 60 Hz
frame at a time and `poll` advances to the wall clock. DSP load reads 6–9 %, as the
prototype's. It hands out the same `Removed { message, undo }` as `RealHost`, stars plugins in
its catalog, answers a rescan with `PluginsRescanned`, has no problems, and `next_poll` is
`None` (its meters are live, so the window draws at its frame rate). It never writes a file,
and `open_editor` answers `HostError::Simulated`.

## Verification

`cargo test -p vixmix-host` (with the test plugin built: `cargo test --workspace` builds it):

- **Unit tests.** `real`: it boots on the null backend and says so first, pumps a quantum a
  poll and writes nothing when nothing changed
  (`it_boots_on_the_null_backend_and_says_so_first`); `System` on a platform without a backend
  is the null backend with `NO_BACKEND`; a change is saved once the setup has been quiet and
  restored at the next boot; quitting with a change pending saves it; an unreadable setup is
  kept aside; a removed channel comes back through the undo the host hands out, and a refused
  command changes nothing; a missing plugin is said once and listed as a problem; scan failures
  make one toast. `autosave`: 500 ms of quiet, never longer than 5 s. `apps`: playing first,
  then the setup's, then those seen before, each once, and the seen list bounded. `undo`: a
  removal's undo puts back exactly what it took. `audio::linux` (Linux, no daemon): the graph
  is the setup's channels in order with the Master source; an app goes to its channel's sink,
  follows a rename, and goes home when its channel is removed; the state follows what the
  backend says (connecting, idle while only an unrecorded Master source could drive the mixer,
  running at the quantum it reports, nothing said when Master out ticks before Monitor has a
  device, disconnected until the daemon is back, idle when nothing drives it); Monitor's device
  gone is said once when the setup names it, in Master out's words or the Master source's, not
  again at a reconnect while it stays away, again once Monitor played and lost it again, and
  not when Monitor's stream is refused while the backend reports its device
  (`monitor_gone_is_said_once_when_the_device_the_setup_names_is_not_there`); the idle banner
  names Monitor's missing device, stopped or with only the Master source, until Monitor plays,
  never names one the backend reports, and follows another device picked for Monitor
  (`the_idle_banner_names_monitors_device_while_the_mixer_waits_for_it`); every app is
  routed the moment it appears, to its channel or home, follows a rename and is forgotten when
  it goes. `fake`: the first poll reports a simulated
  backend with the example's apps and devices; meters move, a muted send and a muted mix read
  zero, the same seed gives the same frames; previewing shows the Master sum on Monitor; a
  removal hands out the undo that puts it back.
- **`tests/null_backend.rs`**, every platform, with `vixmix-plugins`' test plugin
  (`tests/common/mod.rs` lays it out as an installed bundle): a constant through the test
  plugin at gain 0.25 on a channel with MON on and MST off reads 0.2 on the Monitor send and mix
  and 0.8 dry on Master, and a save and a reload give the same setup (the captured state is the
  one loaded) and the same frame
  (`the_test_plugin_plays_through_a_channel_chain_and_comes_back_after_a_reload`); a removed
  insert's instance stays live while its toast shows, comes back playing on undo, is retired
  once the toast has run out (the test sleeps as `next_poll` lets it) and is made again from
  its state by a late undo
  (`a_removed_insert_keeps_its_instance_until_its_undo_toast_dies`); a copy on the Master mix
  sounds like its original (`a_copied_insert_sounds_like_its_original`); the plugin moved while
  it plays (`set_param`) is heard from the next block, is on disk once the debounce has run out
  with the state taken from the live plugin, moved again and quit at once is kept by the quit's
  save, and sounds so after a relaunch
  (`a_plugin_moved_while_it_plays_is_saved_with_its_new_state_and_sounds_so_after_a_relaunch`).
- **`tests/headless.rs`**, Linux, against the daemon of `scripts/ci/headless-pipewire.sh`
  ([Run a headless PipeWire](../cookbook/run-headless-pipewire.md); without its marker each test
  says how to start it and passes, under `CI` it fails), one test at a time, under
  `assert_no_alloc`'s allocator:
  `an_app_plays_through_its_channel_to_monitor_and_master_and_comes_back_after_a_relaunch` —
  the host boots on the daemon with a setup naming a player, the test plugin at half gain and
  Monitor on the null sink, and runs at 48 kHz and 256; a `pw-play` sine is listed as playing,
  routed into Music's sink and read through the plugin on Music's meter; the null sink and
  `pw-record` of the Master source both hear it at the plugin's level; the plugin is moved
  while it plays (`set_param`) and Music follows; quitting at once clears the player's routing,
  it goes back to the default, and the quit's save takes the plugin's new state; the relaunch
  gives the same setup with that state and the player back in Music, sounding as moved.
  `a_new_quantum_keeps_the_app_in_its_channel_and_the_master_source_recording` — with the
  player in Music, a link made by hand into Music's sink and `pw-record` on the Master source,
  `SetPrefs` asks for 512: the host runs at 512, every node of ours keeps its `object.serial`
  (none made again), three seconds later the player is still in Music and heard there, the
  hand-made link stands, and the recording carries the mix throughout but for one gap of at
  most two cycles at 512 (measured: 1024 frames, every run).
- **`tests/refused.rs`**, Linux, alone in its binary because it changes the environment:
  `a_pipewire_that_refuses_is_the_null_backend_with_its_reason_and_is_asked_again` — with
  `PIPEWIRE_REMOTE` naming a socket that does not exist, `System` boots on the null backend with
  "No audio backend: PipeWire refused to connect to the daemon…" and the check it names, polls
  pump it, and `next_poll` is at most 5 s; with the variable put back, the next ask (not before
  5 s) takes PipeWire.

The by-hand pass on real hardware is [the cookbook](../cookbook/by-hand-pass.md).
