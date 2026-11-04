# PipeWire backend (`vixmix-pipewire`)
**Owns:** `crates/vixmix-pipewire/`

**Reference — built by [handoff 03](../handoffs/03-pipewire-backend.md) on 2026-11-04.**
`vixmix-pipewire` is the mixer as a PipeWire client: a virtual sink per app channel, a capture
stream per hardware channel, the Monitor and Master outputs, Master also as a virtual source,
app discovery and routing through the `default` metadata, device enumeration and reconnect, with
`Engine::process` driven from PipeWire's data thread. **It is Linux only**: the crate body is
behind `#![cfg(target_os = "linux")]` and `pipewire`, `rtrb`, `assert_no_alloc` and `log` sit
under a Linux-only target table, so on every other target the crate is empty and Windows never
builds `pipewire-sys` (which needs libpipewire-0.3 and libclang). It owns no model type and no
graph: it carries the engine's inputs and outputs and reports what the daemon says. How to run
a daemon to try it against is [Run a headless PipeWire](../cookbook/run-headless-pipewire.md).

## What it owns and what the others keep

| The backend | The others |
|---|---|
| the control thread, the context and its one data loop, every stream and proxy | the engine (02): what a tick computes, `Engine::process` |
| the rings, the latency governor, which output runs the tick, the staged mix | the model (01): the setup a `BackendGraph` is derived from |
| the registry: devices, app streams and their identity | the host (06): deriving the graph, deciding each app's routing from its identity, acting on events |
| writing routings into the `default` metadata, moving a renamed sink's apps | — |
| reconnecting streams and the daemon | — |

## The contract

- `BackendConfig { app_name: &'static str, wanted_quantum: u32 }`, default `vix-mix` and 256.
  `app_name` is `application.name`, `node.group`, `node.link-group` and every node name's
  prefix; `wanted_quantum` goes into `node.latency` as `<quantum>/48000` (a request, never
  `node.force-quantum`).
- `BackendEvent`: `Connected { rate, quantum }` (the tick ran its first cycle since the daemon
  answered), `Disconnected`, `DeviceAdded(DeviceInfo)`, `DeviceRemoved(node name)`,
  `AppStreamAdded(AppStream { node, app: AppRef, media_name })`, `AppStreamRemoved(node id)`,
  `Xrun` (at most one per housekeeping pass that saw any), `QuantumChanged(u32)`, and
  `TickSource(TickSource)` — `Monitor`, `MasterOut`, `MasterSource` or `Stopped`, the output
  expected to drive the engine (§ The tick), sent once per connection and on every change;
  `MasterSource` and `Stopped` are states the window shows.
- `BackendGraph { channels, monitor, master, master_source }`, each channel a
  `GraphChannel { id, name, input }` with `GraphInput::AppSink` or `Capture(Option<DeviceRef>)`;
  at most eight channels are kept, in the engine's input order.
- `Backend::connect(config, Arc<Engine>, SyncSender<BackendEvent>) -> Result<Backend,
  BackendError>` starts the control thread and returns once the daemon answered;
  `apply(&self, &BackendGraph)` and `route(&self, app_node, Option<&str>)` (a channel's
  `node.name`, or `None` for the default) post to a mailbox and return at once (latest wins per
  graph and per app node); `stats(&self) -> BackendStats`; `disconnect(self)` (also on drop)
  destroys every stream and leaves the daemon, leaving routings as they are. `Backend` is
  `Send + Sync`.
- `BackendStats`: `tick_source`, `ticks`, `rate`, `quantum`, `xruns`, `underruns`,
  `overflows`, `ring_checks` and `ring_off` (§ The rings), `monitor_cycles`,
  `master_out_cycles`, `master_source_cycles` (cycles each output wrote the mix of its own
  cycle) and `stale_cycles` (an output that had none and wrote silence). Relaxed atomics, read
  without stopping anything.
- `BackendError::PipeWire { what, source }`: the main loop, the context or the connection was
  refused; the message says to check `systemctl --user status pipewire wireplumber` and
  `XDG_RUNTIME_DIR`.
- Node names: `channel_node_name(app, ChannelId, name)` is `<app>.ch<id>.<slug>`
  (`vix-mix.ch3.voice-chat`), `monitor_out_node_name` `<app>.monitor-out`,
  `master_out_node_name` `<app>.master-out`, `master_node_name` `<app>.master`; `slug` keeps
  lower-case ASCII letters and digits with one `-` for every other run (`channel` when nothing
  is left). The id keeps two channels whose names slug alike apart; the slug is for whoever reads
  `pw-top`. A rename changes the name: the sink is made again under it and the backend moves
  the apps the metadata sent to the old one (§ Routing). `DISPLAY_NAME` (`Vix Mix`) prefixes
  every `node.description`: the Master source is "Vix Mix Master".

## Threads

| Thread | Runs | Touches | Never |
|---|---|---|---|
| the host's (any) | `connect`, `apply`, `route`, `stats`, `disconnect` | the mailbox (a mutex, latest wins), one wake through a `pipewire::channel`, the counters | anything PipeWire hands out |
| `vix-mix-pipewire` (control) | the main loop: the context, the core, the registry, the `default` metadata, every stream and proxy; a housekeeping pass every 20 ms | every PipeWire object; the host's event channel (`try_send`, then a backlog of at most 1024); the plan rings | a stream's realtime user data |
| PipeWire's data loop (one, `context.num-data-loops = 1`) | every stream's `process` | that stream's own user data, the rings, atomics, the tick's state through one compare-and-swap | the forbidden list below |

Every PipeWire callback on the control thread only queues a note and kicks the loop; the notes
are handled at a safe point (the kick, the host's wake or the housekeeping timer — loop
sources, never an object's own callback, so nothing is destroyed from inside its own event),
at most 4096 per pass. A stream's state changes carry which of its connections they belong to,
so the `Unconnected` a reconnect's own disconnect reports is told from a real one. Each stream
has two listeners: one whose `process` runs on the data thread with that stream's realtime state,
one whose state changes arrive on the control thread; the data thread's user data is never
touched from the control thread. Dropping a stream disconnects it first, which removes its node
from the data loop before returning, then its listeners, then the stream.

## Nodes and properties

| Node | `node.name` | `media.class` | Direction | Flags | Its own properties |
|---|---|---|---|---|---|
| app channel | `<app>.ch<id>.<slug>` | `Audio/Sink` | input | `RT_PROCESS`, `MAP_BUFFERS` | — |
| hardware channel | `<app>.ch<id>.<slug>` | `Stream/Input/Audio` | input | + `AUTOCONNECT` | `target.object` = the device, `node.dont-reconnect`, `node.dont-fallback`, `media.category = Capture` |
| Monitor out | `<app>.monitor-out` | `Stream/Output/Audio` | output | + `AUTOCONNECT` | `target.object`, `node.dont-reconnect`, `node.dont-fallback`, `media.category = Playback` |
| Master out | `<app>.master-out` | `Stream/Output/Audio` | output | + `AUTOCONNECT` | as Monitor out |
| Master source | `<app>.master` | `Audio/Source` | output | `RT_PROCESS`, `MAP_BUFFERS` | — |

Every stream also carries `media.type = Audio`, `node.description` and `media.name` ("Vix Mix
<channel name>", "Vix Mix Monitor", "Vix Mix Master output", "Vix Mix Master"),
`node.group = node.link-group = <app>` (one driver schedules them all; the link-group also keeps
WirePlumber from linking a node of ours to another of ours, so a capture can never record our
own Master), `node.virtual = true`, `node.latency = <quantum>/48000` and
`session.suspend-timeout-seconds = 0`. The core connects with `application.name = <app>`. A
capture with no device and an output with no device have no stream. The one format every
stream offers is 32-bit float planar stereo (FL, FR) with no rate, so it follows the graph's
rate: an `EnumFormat` object built from `AudioInfoRaw` and serialized with `PodSerializer`, as
pipewire-rs's `examples/audio-capture.rs` does.

`node.dont-reconnect` and `node.dont-fallback` make a stream whose device goes away wait for it
rather than follow the default device (WirePlumber as Ubuntu 24.04 ships it reads the first and
then destroys the stream's node —
[`policy-node.lua`](https://gitlab.freedesktop.org/pipewire/wireplumber/-/blob/0.4.17/src/scripts/policy-node.lua)
— newer WirePlumber reads the second). The backend connects the same stream again when the
registry shows the device again, and retries any stream the daemon or the session manager
gave up on once a second (while its device is present, when it names one).

## The two PipeWire facts

1. **An unlinked stream's `process` never runs.** A stream's node process returns before calling
   it when the port has no buffers (`impl_node_process_input` and `impl_node_process_output`
   in [`stream.c`](https://gitlab.freedesktop.org/pipewire/pipewire/-/blob/1.0.5/src/pipewire/stream.c)
   return `-EIO` while `io == NULL`). So an app sink with no app linked delivers nothing and is
   silence, not an underrun, and the Master source runs only while something records it — it
   can tick only then.
2. **Nodes not linked to each other run in no set order within a cycle.** Every node of one
   `node.group` or `node.link-group` is moved under one driver (`collect_nodes` in
   [`context.c`](https://gitlab.freedesktop.org/pipewire/pipewire/-/blob/1.0.5/src/pipewire/context.c)),
   the driver triggers each of them at the start of a cycle, and only a link makes one wait for
   another (`add_node` and `trigger_targets` in
   [`impl-node.c`](https://gitlab.freedesktop.org/pipewire/pipewire/-/blob/1.0.5/src/pipewire/impl-node.c)).
   Our inputs and our outputs are not linked to each other, so any of them may run first:
   hence the rings' governor and the first-output tick below.

## The tick

**Whichever output runs first in a cycle runs the tick**; every output that runs later in the
same cycle copies what it staged. The tick takes `n = Buffer::requested()` (the graph's quantum,
never assumed, at most 8192), governs each input's ring, calls `Engine::process` into its own
buffers, and stages the Monitor and Master mixes in atomics stamped with the cycle (the driver's
`clock.nsec`, the same for every node of one cycle, from `pw_stream_get_time_n`): an output
copies the staged mix only when the stamp is its own cycle's, so every output plays the mix of
the cycle it runs in, with no ring and no added quantum. Its state sits behind a flag one
compare-and-swap claims, so a second claimant (which the single data loop never produces)
writes silence instead of waiting. When one output stops running — Monitor's device unplugged —
the next cycle's first output ticks: no hand-over, no gap.

`TickSource` is what the host is told, not what the data thread reads: Monitor while its stream
is alive and its device present, else Master out, else the Master source (the engine then runs
only while something records Master), else `Stopped`.

**Not as handed off.** The handoff's design connected Master out and the Master source with
`PW_STREAM_FLAG_TRIGGER` and triggered them from Monitor's `process` (PipeWire's own
`module-loopback` drives its playback side so), electing Master out by reconnecting it without
the flag when Monitor's device went away. Built and measured against the headless daemon at 256
(the first commit of this lane): steady state was clean, but when Monitor's device was removed
Master out stopped with it — nothing triggered it — until the control thread heard of the
removal (11 ms, more than two quanta) and reconnected it: 1280 frames (five quanta) of silence
on Master, where the acceptance allows one quantum. No reaction on the control thread can be
faster than the registry's news, so the shape changed. Two PipeWire facts found on the way:
`pw_stream_connect` with `TRIGGER` writes `node.trigger = true` into the stream's properties
and nothing removes it, so the same stream connected again without the flag still waits for a
trigger; and a TRIGGER node in a running group must be triggered every cycle or the whole
cycle never completes. The first-output tick measured 0 silent frames and no break across the
same removal.

## The rings and the latency governor

Each input stream's `process` copies its two planes into its channel's `rtrb` ring of stereo
frames (capacity 3 × 8192 frames) and stamps the cycle it delivered in. At each tick the ring
should hold exactly one quantum ahead of the tick: two when the input already delivered in this
cycle, one when it delivers after the tick. More is discarded, oldest first (`ring_off` counts
it while the input runs); less is silence, and the ring is left to fill until it holds one
quantum ahead again. So the tick always reads the frames from one cycle ago, whichever ran
first: input lag is exactly one quantum and an order flip costs nothing. A running input that
came short and delivers again within eight ticks underran (`underruns`, an xrun); one that stays
silent stopped (an app quit or moved), which is no xrun. `ring_checks` counts the checks of
running, primed inputs. A ring full when its input delivers (nothing ticked) counts in
`overflows`. A tick more than one and a half periods after the previous one (and less than
sixteen, which is a graph that stopped) counts as an xrun too. Every xrun also counts in the
engine's `Stats::xruns`.

The tick's inputs change by plan: the control thread builds a boxed plan (which of the eight
slots gets which ring's consumer, and the engine order), sends it through an `rtrb` ring, and
the tick takes it at the start of a cycle and sends it back holding the taps it replaced, so
they are freed on the control thread. Plans the tick has not taken yet are coalesced on the
control thread, at most sixteen in flight each way.

## Registry, identity and routing

- **Devices** are `Node` globals whose `media.class` is `Audio/Sink` (an output) or
  `Audio/Source` or `Audio/Source/Virtual` (an input, as the CI null source is), named by
  `node.name` with `node.description` (else `node.nick`, else the name). A node named `<app>.…`
  is ours and never reported.
- **App streams** are `Stream/Output/Audio` nodes that are not ours. The registry's global
  carries only a few properties, so the backend binds the node and its client for their full
  properties: `application.name` (else `node.name`), `application.process.binary`,
  `pipewire.access.portal.app_id` and `media.name`, the node's own first and its client's
  filling what it lacks — except for a stream pipewire-pulse made (`client.api =
  pipewire-pulse`), whose client is pipewire-pulse itself. A stream is reported once both
  have answered.
- **Routing** writes into the `default` metadata (the `Metadata` global whose `metadata.name` is
  `default`): `target.node` removed and `target.object` set to the sink's `object.serial`
  (`Spa:Id`), or both removed for the default. The serial, not the name: WirePlumber's
  `restore-stream` persists a `target.object` only by looking its value up as a serial, and it
  restores through `target.node`
  ([`restore-stream.lua`](https://gitlab.freedesktop.org/pipewire/wireplumber/-/blob/0.4.17/src/scripts/restore-stream.lua),
  `saveTarget` and `restoreTarget`). A routing waits until the metadata is bound and the sink
  announced (at most 256 waiting).
- **Apply-on-appear** is the host's rule: when an app appears it routes it — to its channel, or
  home to the default — because the session manager may restore an old routing. The backend
  keeps the host's routing standing for three seconds after the app appears: a `target.object`
  or `target.node` someone else sets in that window is written over again (three times at
  most). WirePlumber's own memory of a routing is unreliable when an app quits (its
  `restore-stream` forgets a target whose metadata is cleared while the node is still known),
  which is why the host's rule, not the session manager, is the routing of record.
- **A renamed app sink** keeps its apps: every subject the metadata sent to the old sink (by our
  serial or by WirePlumber's restored id) is routed to the new one once it is announced.
- `Disconnected` is preceded by a `DeviceRemoved` and an `AppStreamRemoved` for everything that
  was reported, so the host's lists empty.

## `apply`: what changes what

A channel whose spec (kind, node name, device) is unchanged keeps its stream and ring. A changed
one has its old stream destroyed first (its node name frees up) and a new one with a new ring
made; a channel gone frees its slot; the engine order goes in the next plan. An output whose
device changed is made again; the Master source exists while `master_source` is true. Graphs are
applied only after the first registry roundtrip (devices known), and the newest graph is applied
again after a reconnect.

## Reconnect

A core error with `EPIPE` on the core is the daemon gone: everything reported is removed,
`Disconnected` and `TickSource(Stopped)` are sent, every stream and proxy is dropped, and the
housekeeping pass connects again after 250 ms, doubling up to 5 s while it fails. Once
connected, the newest graph is applied again and `Connected` follows the first tick.

## Latency

At 256 frames and 48 kHz one quantum is 5.33 ms. App to audible ≈ the app's own queue (at least
one quantum) + one quantum in the ring (exact: the governor holds it) + the device's period ≈
3 quanta ≈ 16 ms; mic to headphones ≈ 3 quanta plus the converters. At 128 it is ≈ 8–9 ms, at a
global cost (the graph's quantum is everyone's); the preferences offer 128, 256 and 512 through
`node.latency`. Master out and the Master source add nothing: they play the mix of the cycle they
run in. The by-hand measurement on real hardware is handoff 06's cookbook, run by handoff 08.

## Forbidden on the data thread

Every `process` callback body runs inside `assert_no_alloc`. In them: no allocation or freeing
(rings, planes and staging are sized up front; plans and taps travel boxed and are freed on the
control thread), no lock that can wait (the tick's state is claimed by one compare-and-swap and
never waited for), no logging, no system call of ours, no proxy and no PipeWire object but the
stream being processed (`dequeue_buffer`, `queue` on drop, `pw_stream_get_time_n`). The one
exception is the engine's first tick on a thread, which `arc_swap` lets allocate once
([engine.md](engine.md)); it runs under `permit_alloc`.

## Verification

On Linux, `cargo test -p vixmix-pipewire` (on Windows the crate compiles to nothing):

- Unit tests (no daemon): node names and their uniqueness; each stream's properties and flags,
  and the format pod (F32P, two channels FL FR, no rate); registry classification (devices with
  their direction, ours never, the `default` metadata only) and app identity with the client
  fallback and the pipewire-pulse exception; the governor reading the frames from one cycle ago
  through order flips, cutting a ring that ran ahead, an underrun counted once the input is back
  and never for an input that stopped or is not running; a plan installing taps and coming back
  with the replaced ones; staging handing an output only its own cycle's mix.
- `tests/headless.rs`, against the daemon of `scripts/ci/headless-pipewire.sh` (CI's Linux
  leg), one test at a time, under `assert_no_alloc`'s allocator (a data-thread allocation aborts
  the run): devices and an app stream reported with their identity, and its going; a `pw-play`
  routed to `<app>.ch1.test` reaching the engine with every ring check exactly one quantum
  ahead and no xrun; `pw-record` of `<app>.master` receiving the mix, every Master cycle the
  mix of its own cycle, no break in the sine; Monitor out linked to and heard on the null sink;
  a capture channel linked to the null source; an app un-assigned while absent going back to
  the default when it appears, whether the session manager's restore lands before or after the
  host's routing; a renamed channel keeping its app; a quantum forced to 512 reported and
  followed, and back; Monitor's device removed mid-run with Master out ticking on, no silent
  frame and no break on Master, every cycle ticked.
- On request (`-- --ignored`, [the cookbook](../cookbook/run-headless-pipewire.md) step 6):
  `two_minutes_at_256_without_an_xrun` — two minutes with null devices at 256, zero xruns, every
  ring check on target, every output every cycle, `pw-top`'s ERR 0 on every node of ours — and
  `the_backend_reconnects_after_the_daemon_restarts`. The two-minute run wants a quiet machine:
  with no realtime priority (a container has no rtkit) a stall of the whole host shows as
  xruns on every node of the graph, the test's `pw-play` and `pw-record` included, which the
  backend then counts too.
