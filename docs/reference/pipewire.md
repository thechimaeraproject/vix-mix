# PipeWire backend (`vixmix-pipewire`)
**Owns:** `crates/vixmix-pipewire/`

**Reference — the contract as lane 00 wrote it; [handoff 03](../handoffs/03-pipewire-backend.md)
builds the backend and completes this page.** `vixmix-pipewire` is the mixer as a PipeWire
client: a virtual sink per app channel, a capture stream per hardware channel, the Monitor and
Master outputs, Master also as a virtual source, app discovery and routing through the
`default` metadata, device enumeration and reconnect, with `Engine::process` driven from
PipeWire's data thread. **It is Linux only**: the crate body is behind
`#![cfg(target_os = "linux")]` and the `pipewire` dependency sits under a Linux-only target
table, so on every other target the crate is empty and Windows never builds `pipewire-sys`
(which needs libpipewire-0.3 and libclang). It owns no model type and no graph: it carries the
engine's inputs and outputs and reports what the daemon says.

## The contract today

- `BackendConfig { app_name: &'static str, wanted_quantum: u32 }`, default `vix-mix` and 256.
- `BackendEvent`: `Connected { rate, quantum }`, `Disconnected`, `DeviceAdded(DeviceInfo)`,
  `DeviceRemoved(node name)`, `AppStreamAdded(AppStream { node, app: AppRef, media_name })`,
  `AppStreamRemoved(node id)`, `Xrun`, `QuantumChanged(u32)`.
- `BackendGraph { channels, monitor, master, master_source }`, each channel a
  `GraphChannel { id, name, input }` with `GraphInput::AppSink` or `Capture(Option<DeviceRef>)`:
  what the host derives from the setup and hands to `apply`.
- `Backend::connect(config, Arc<Engine>, SyncSender<BackendEvent>) -> Result<Backend,
  BackendError>`; `apply(&self, &BackendGraph)`; `route(&self, app_node, Option<&str>)` (the
  sink's `node.name`, or `None` for back to the default); `disconnect(self)`. The event
  channel is a bounded `std::sync::mpsc::sync_channel` (every channel here is bounded).
- `BackendError::NotBuilt` — what `connect` answers until handoff 03, whose message names the
  handoff and says the null backend runs meanwhile — and `PipeWire { what, source }`, whose
  message says to check `systemctl --user status pipewire wireplumber` and `XDG_RUNTIME_DIR`.
  `RealHost` shows the error on the window's banner.
- `channel_node_name(app, channel name)` is `<app>.ch.<slug>`, `master_node_name(app)` is
  `<app>.master`, and `slug` keeps lower-case ASCII letters and digits with one `-` for every
  other run (`Voice chat` → `voice-chat`, nothing left → `channel`). Two channels whose names
  slug alike need distinct node names; handoff 03 decides how (a suffix of the channel id).

## The design handoff 03 builds

Reviewed against the PipeWire sources, `module-loopback`, the scheduling documentation and the
WirePlumber settings on 2026-11-03; handoff 03 records the evidence links here.

- **One thread loop, one context, one data loop.** `pipewire::init()`, one `ThreadLoopBox`
  and `ContextBox` with `context.num-data-loops = 1`, one `CoreBox`; every proxy stays on that
  control thread, and the host talks to it through `pipewire::channel`.
- **Every stream** carries `node.group` and `node.link-group` = the app name (so WirePlumber
  treats the pair as a filter, exempt from `linking.follow-default-target`), `node.virtual =
  true`, `node.latency = <quantum>/48000` (a request), and `session.suspend-timeout-seconds =
  0`. One driver schedules the group; device drift is the follower device nodes' resampling.
- **Nodes.** App channel: an INPUT stream, `Audio/Sink`, `node.name = <app>.ch.<slug>`, F32P
  stereo, `RT_PROCESS | MAP_BUFFERS`, no autoconnect. Hardware channel: an INPUT
  `Stream/Input/Audio` with `target.object`, `AUTOCONNECT | RT_PROCESS | MAP_BUFFERS`. Monitor
  out: an OUTPUT with `target.object`, autoconnected — **the tick**. Master out and the Master
  source (`Audio/Source`, `<app>.master`): OUTPUT streams connected with `TRIGGER` and driven by
  `trigger_process()` from the tick every cycle (the workspace enables the crate's `v1_0_0`
  feature for this).
- **The two facts it is built on.** An unlinked stream's `process` never runs, so an app sink
  with no app is silence, not an underrun, and the Master source can never be the tick; and the
  order of unlinked nodes within a cycle is unspecified, which is why Master's outputs are
  triggered rather than fed by rings.
- **Data flow.** Each input's `process` copies its planes into the channel's `rtrb` ring (at
  least 3 × 8192 frames). The tick takes `n = Buffer::requested()`, holds each ring to one
  quantum (discarding down to `n` when it holds 2n or more; silence when it holds less than
  `n`), calls `Engine::process`, writes Monitor, stages Master and triggers Master out and the
  Master source.
- **Tick election**, **registry and routing** (the `default` metadata's `target.object`,
  applied when an app's node appears, since WirePlumber's `stream.restore-target` persists what
  we write), **latency** (≈ 3 quanta, 16 ms at 256/48 kHz) and the **format pod** are in the
  handoff in full.

## Verification

On Linux, `cargo test -p vixmix-pipewire`: node names are the app and a slug of the channel,
and until handoff 03 `connect` answers `NotBuilt` with a message that names the handoff and
the null backend. On Windows the crate compiles to nothing. CI's `check (linux)` builds it
against `libpipewire-0.3-dev` and runs `scripts/ci/headless-pipewire.sh`, which starts a
headless PipeWire and WirePlumber with a null sink (`vixmix-ci-null-sink`, `Audio/Sink`) and a
null source (`vixmix-ci-null-source`, `Audio/Source/Virtual`) and proves the daemon answers
(`pw-cli ls Node` lists the sink and `pw-cli ls Metadata` the `default` metadata); handoff 03's
integration tests run against it, and `source scripts/ci/headless-pipewire.sh` starts the same
in a WSL2 or VM shell.
