# Host (`vixmix-host`)
**Owns:** `crates/vixmix-host/`

**Reference — the contract as lane 00 wrote it; [handoff 06](../handoffs/06-host-and-integration.md)
completes it.** `vixmix-host` is the orchestrator: one `Host` owns the setup, the engine, the
plugin instances and the backend, and is the only thing the window talks to. `RealHost` wires
them; `FakeHost` serves prototype v5's example data with simulated meters and no audio, for
the window's tests and the look gate. Every `Host` method runs on the UI thread; the host
reaches the tick only through `Engine::prepare`, `Engine::reap` and the meters and stats. It
draws nothing and owns no window: the app hands it raw window handles for editors.

## What it owns and what the others keep

| The host | The others |
|---|---|
| boot and shutdown order, acting on `Applied`, saving | the model: what a command means |
| choosing the backend and showing why there is none | the backend: the streams; the engine: the tick |
| creating and retiring instances as inserts come and go | the plugins crate: how |
| `FakeHost`, the window's fixture | the window: drawing it |

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
}
```

`HostEvent`: `SetupChanged`, `Toast(String)` (a message that says what to do), `Apps`,
`Devices`, `Backend(BackendState)` (the first poll always reports it), `PluginsRescanned`,
`EditorClosed(InstanceId)`. `BackendState`: `Absent { reason }` (the banner's text; on a
platform with no backend it is exactly "No audio backend on this platform"), `Simulated`,
`Connecting`, `Running { rate, quantum }`, `Disconnected`; `banner()` and `live()` (whether
meters move, which sets the window's 60 Hz repaint). `MeterFrame { channels: [ChannelLevels;
8], monitor, master }` holds linear peaks (`Stereo { left, right }`) by channel position,
copied once per frame; `StatsFrame { rate, quantum, dsp_load, xruns }` with `quantum_ms()`.
`AppPresence { app, running }`. `tick_editors` pairs each event with its instance, because the
window keeps one editor window per instance and must know which one to resize.

## `RealHost`

`RealHost::boot(BootOptions { config_dir, debug_build })`: the directory is `config_dir` or
`persist::profile(debug_build)` (`HostError::NoConfigDir` when the platform names none); it
loads `setup.ron` and `plugins.ron` (an unreadable file becomes a toast and the default),
makes the engine (48 kHz, `MAX_QUANTUM`), the rack and the instances, creates every insert's
instance the catalog can find (each failure a toast that says to rescan), prepares the engine,
and starts the backend: on Linux `vixmix_pipewire::Backend::connect`, which answers
`NotBuilt` until handoff 03, so the state is `Absent` with that message; elsewhere `Absent`
with "No audio backend on this platform". Booting writes nothing.

`apply` runs the model's `apply`, retires `removed` instances, creates `created` ones (with
their state, for a copy), prepares the engine when anything audible changed, and reports
`SetupChanged`. `poll` pumps the null backend one quantum (256 frames) while there is no
backend, reaps the engine's replaced graphs and the retired instances, and drains instance
events (a dirty plugin is `SetupChanged`; a bus change prepares again). `save_now` captures every
live instance's state into the setup, creates the directory, moves an unreadable `setup.ron`
aside to `setup.ron.unreadable` rather than saving over it, and writes atomically.
`open_editor`, `tick_editors` (polling each editor's Linux run loop) and `close_editor` drive
the plugins crate's sessions; dropping the host closes every editor. Routing, the debounced
autosave and the shutdown order are handoff 06's.

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
prototype's. It never writes a file, and `open_editor` answers `HostError::Simulated`.

## Verification

`cargo test -p vixmix-host`: `RealHost` boots on the null backend, reports the banner first
(the not-built message on Linux, "No audio backend on this platform" elsewhere), pumps a quantum
a poll and writes nothing; a command changes the setup and a save restores it at the next
boot; an unreadable setup is kept aside rather than saved over. `FakeHost`'s first poll reports
a simulated backend with the example's apps and devices; its meters move, a muted send and a
muted mix read zero, the same seed gives the same frames, and previewing shows the Master sum
on Monitor.
