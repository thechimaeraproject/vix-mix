# 03 — PipeWire backend

**Record — shipped 2026-11-04.** The backend runs against a real daemon: streams, rings and
the latency governor, the tick on the data thread, the registry with app identity, routing
through the `default` metadata, `apply` diffing, reconnect with backoff, the cookbook, and the
reference complete ([reference/pipewire.md](../reference/pipewire.md)). The integration tests
ran in an `ubuntu:24.04` container (PipeWire and WirePlumber as Ubuntu 24.04 ships them, no
rtkit) because WSL2's packages are not installed yet, and run in CI's Linux leg inside
`dev check`; no change to `ci.yml` was needed (the daemon script, which this lane owns, now
also exports the marker the tests require). Deviations from the design and steps below, each
forced by what the daemon did:

- **The tick is whichever output runs first in a cycle, not Monitor triggering Master out and
  the Master source (step 4).** Built as designed first (the lane's first commit) and measured
  at 256: steady state was clean, but removing Monitor's device left Master out silent for
  1280 frames (five quanta) — a TRIGGER stream runs only when triggered, so Master stopped with
  Monitor's link until the control thread heard of the removal (11 ms) and reconnected Master
  out without the flag. No control-thread reaction is faster than the registry's news, so the
  acceptance's one quantum was out of reach. Now every output is driven by the graph, the first
  to run in a cycle ticks and stages the mix stamped with the cycle, and the others copy it:
  the same latency (no ring on the outputs), no trigger pointers shared with the data thread,
  and the same removal measures 0 silent frames. Also found: `pw_stream_connect` with `TRIGGER`
  leaves `node.trigger = true` in the stream's properties for good, and a TRIGGER node in a
  running group must be triggered every cycle or no cycle completes. The election survives as
  what the host is told: `BackendEvent::TickSource`.
- **The governor holds the ring one quantum ahead of the tick, not "discard down to n at 2n"
  (step 3).** Read literally, that rule gives a lag of zero or one quantum depending on which
  runs first, with a lost or repeated quantum at each order flip. Each input stamps the cycle
  it delivered in (the driver's `clock.nsec`), so the tick knows whether to expect two quanta
  or one: the lag is exactly one quantum whatever the order, which is what the design asks for.
  A shortfall counts as an underrun only when the input delivers again within eight ticks; an
  app that quits is no xrun.
- **Routing writes the sink's `object.serial` as `Spa:Id`, not its `node.name` as
  `Spa:String`, and clears `target.node` (step 6).** WirePlumber's `restore-stream.lua` looks a
  `target.object` value up as a serial to persist it, so a name is routed but never remembered,
  and it restores through `target.node`. WirePlumber's memory also proved unreliable: it forgets
  a routing when the metadata is cleared while the app's node is still known, which happens
  when an app quits. So the host's apply-on-appear rule is the routing of record, and the
  backend helps it win: for three seconds after an app appears, a routing someone else sets
  over the host's is put back. The un-assignment test writes the restore itself (what
  WirePlumber writes) before and after the host's routing, rather than relying on that memory.
- **App identity comes from the node and its client.** `application.process.binary` and the
  portal app id live on the client for a native PipeWire app (`pw-play`'s node carries
  neither), so the backend binds clients too; a stream pipewire-pulse made keeps the identity
  pipewire-pulse copied onto it.
- **Node names are `<app>.ch<id>.<slug>`**, the second point the contract left open: the id
  keeps alike-slugged channels apart, and a rename makes the sink again under the new name and
  moves the apps the metadata sent to the old one. Monitor out and Master out are
  `<app>.monitor-out` and `<app>.master-out`; descriptions read "Vix Mix <name>".
- **Streams that name a device carry `node.dont-reconnect` and `node.dont-fallback`**, so an
  unplugged device's stream waits for it (WirePlumber's policy destroys its node; the backend
  connects the same stream again when the device returns) instead of following the default.
- **The control thread runs a `MainLoop`, not a `ThreadLoop` (step 1).** Every proxy and stream
  is made and dropped on that thread, so `Backend` holds only `Send` things and the host may
  keep it anywhere; a `ThreadLoop`'s objects would be made from the caller's thread under its
  lock. PipeWire's callbacks only queue notes, handled at loop-source safe points, because
  objects may not be destroyed from inside their own callbacks.
- **Additive contract changes**, change request 6 in the [build map](../build-map.md):
  `BackendEvent::TickSource` and `TickSource`, `Backend::stats` and `BackendStats`,
  `channel_node_name` taking the channel's id, `monitor_out_node_name`,
  `master_out_node_name`, `DISPLAY_NAME`, `Backend: Send + Sync`; `BackendError::NotBuilt` is
  gone.
- **`vixmix-host`'s Linux test `it_boots_on_the_null_backend_and_says_so_first` now fails**: it
  asserts the "not built yet" banner, and `connect` succeeds against CI's daemon, which
  `start_backend` turns into `Connecting`: no banner, and the null backend no longer pumped.
  This lane may not edit lane 06's crate, so change request 7 carries the fix, proven on this
  branch in the container (the whole Linux gate green with it): until 06 keeps the backend,
  the host reports `Absent` with a banner saying PipeWire answers but is not played through
  yet, and the null backend keeps the meters live. Until it is applied, `check (linux)` fails on
  that test alone.
- **Shared docs beyond the conflict rules' list:** the trust table's Cookbook row in
  `docs/README.md` said no cookbook was written; it now says lane 03 wrote the first, so the
  index stays true once this lane's cookbook is in. The index lines and
  `docs/reference/README.md` otherwise keep their wording.
- **The integration tests** need the marker the daemon script exports: without it they say how
  to start the daemon and pass without touching anything (a desktop session must never get
  their routings), and under `CI` they fail. The two-minute run and a daemon restart are
  `#[ignore]`d and run on request (the cookbook, step 6).
- No `kit-*.md` record names this handoff, so none was flipped.

**Acceptance, as run on 2026-11-04 in the container:** `tests/headless.rs` 10 passed (2 on
request); clippy clean on Linux with `-D warnings`, and on Windows the crate compiles to nothing
inside a green `dev check`. The daemon-restart run passed: `Disconnected`, then `Connected` and
Monitor out linked again after the daemon came back.

**The two-minute bullet stays open:** it is not yet met reproducibly, and it is to be run on a
Linux shell with realtime scheduling (WSL2 with its packages and rtkit, or the Fedora box)
before it is called met. The container has none: Docker gives it no realtime budget, so `chrt
-f` is refused even with `SYS_NICE` added, and the data threads run at ordinary priority. There
`two_minutes_at_256_without_an_xrun` passed 7 times out of 10, and 2 out of 2 in the
verifier's second round. Each pass: about 22 500 cycles, 0 xruns, every ring check exactly one
quantum ahead, Monitor out, Master out and the Master source each writing every cycle's own
mix, 0 stale. The last five passes were after the verifier's first round (the latest on the
branch rebased on master), with `pw-top`'s ERR compared over the window: no ERR added on any
node, `pw-play`'s and `pw-record`'s included. Two of the failures were stalls of the whole machine,
`pw-play` late in the same cycles as our nodes. In one, while other lanes were building on the
machine, every node of the graph showed 26 ERR and the backend counted 5 xruns (it misses a
late finish, [reference/pipewire.md](../reference/pipewire.md) § The rings). In the other, our
outputs showed 1 ERR and the backend counted the same cycle. The third failed on `pw-top` alone:
2 ERR on our nodes, counted then from their making, while the backend's window was clean.

**Needs:** 00. **Runs alongside:** 01, 02, 04, 05. **Size:** L. **Linux only.**

## Goal

The mixer as a PipeWire client: virtual sinks per app channel, captures per hardware channel,
Monitor and Master outputs, Master as a virtual source, app discovery and routing, device
enumeration, reconnect — and the engine tick driven from the data thread.

## Read first

- [reference/pipewire.md](../reference/pipewire.md): the contract as lane 00 left it.
  `crates/vixmix-pipewire/` already has `BackendConfig`, `BackendEvent`, `AppStream`,
  `BackendGraph` / `GraphChannel` / `GraphInput`, `BackendError`, `Backend` with the plan's
  signatures (`connect` answers `NotBuilt` until this lane), and the node-name helpers. The
  whole crate is behind `#![cfg(target_os = "linux")]`, and `pipewire` (with the `v1_0_0`
  feature) is a Linux-only dependency.
- [reference/engine.md](../reference/engine.md) (`Engine::process`, `Tick`, `ChannelInput`,
  `MAX_QUANTUM`) and [build-map.md](../build-map.md) § The shared contracts and § Change
  requests.
- The kit's [rust.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/rust.md)
  (platform-only code behind `cfg`; nothing allocates on an audio thread).
- pipewire-rs at the version the workspace `Cargo.toml` pins, feature `v1_0_0` (`pipewire` on
  crates.io; its source is in Cargo's registry once the workspace is fetched):
  `examples/audio-capture.rs` (the format pod), `src/stream/mod.rs`
  (`StreamFlags::TRIGGER`, `Stream::trigger_process`), `src/thread_loop`, `src/channel.rs`,
  `src/metadata.rs`.
- PipeWire itself: `module-loopback` (the pattern: a sink/source pair with `node.group` and
  `node.link-group`), the scheduling documentation, and WirePlumber's
  `linking.follow-default-target` and `stream.restore-target` settings.
- CI's Linux leg in `.github/workflows/ci.yml` and `scripts/ci/headless-pipewire.sh`: the
  packages, and a headless PipeWire and WirePlumber with a null sink (`vixmix-ci-null-sink`,
  `Audio/Sink`) and a null source (`vixmix-ci-null-source`, `Audio/Source/Virtual`), proven to
  answer before the gates run. `source scripts/ci/headless-pipewire.sh` starts the same in your
  Linux shell (below; it needs `pipewire`, `wireplumber`, `pipewire-audio-client-libraries` and
  `dbus`); your integration tests run against it. Note the source's class when you write the
  device filter: `Audio/Source/Virtual`, not `Audio/Source`.
- **A Linux box.** The development machine is Windows, and its WSL2 Ubuntu has nothing this
  lane needs yet: no Rust, no C compiler, no `pkg-config`, no PipeWire packages. Its user has no
  passwordless `sudo`, so an agent cannot install them; the user does it once, in a WSL2 shell:
  `sudo apt-get install libpipewire-0.3-dev pipewire wireplumber pipewire-audio-client-libraries
  libclang-dev dbus pkg-config build-essential`, then rustup (<https://rustup.rs>) with the
  toolchain `rust-toolchain.toml` pins. Until then, take lane 00's route: an `ubuntu:24.04`
  container under the running Docker Desktop, with the worktree mounted
  (`docker run --rm -it -v <worktree>:/src -w /src -e CARGO_TARGET_DIR=/root/target
  ubuntu:24.04 bash`, so Linux build output stays off the Windows `target/`), the same packages
  plus `curl`, `ca-certificates` and `git`, and rustup inside it. The container holds no
  credential for the kit's private repository, so vendor the crates on the Windows side
  (`cargo vendor` into a directory outside the worktree, mounted too) and give the container the
  `[source]` table it prints, its `directory` pointed at the mount. The Fedora VM is the other
  Linux shell once it is set up.

## What this lane owns, what others keep

It owns the PipeWire client: the loop, the streams, the rings, the tick on the data thread,
the registry, routing through metadata, reconnect. The engine owns what the tick computes (02);
the model owns the setup a `BackendGraph` is derived from (01); the host derives the graph,
decides routing from app identities and reacts to events (06).

## Design

Reviewed against the PipeWire sources, module-loopback, the scheduling page and the
WirePlumber settings; the review's evidence links go into `reference/pipewire.md`.

- **Context and loop.** `pipewire::init()`, one `ThreadLoopBox` + `ContextBox` with
  `context.num-data-loops = 1` (so every node of ours runs on one data thread whatever the
  user's `client.conf` says), one `CoreBox`; every proxy stays on that control thread
  (`StreamBox`/`ThreadLoopBox` are not `Send`); the host talks to it through
  `pipewire::channel`.
- **Every stream** carries `node.group = <app>`, `node.link-group = <app>` (module-loopback sets
  both; the link-group makes WirePlumber treat our pair as a filter, exempt from
  `linking.follow-default-target` retargeting), `node.virtual = true`,
  `node.latency = <wanted quantum>/48000` (a request, never `node.force-quantum`),
  `session.suspend-timeout-seconds = 0`. One driver schedules the group; drift between devices
  is absorbed by the follower device nodes' adaptive resampling, not by us.
- **Nodes.** App channel = INPUT stream, `media.class = Audio/Sink`,
  `node.name = <app>.ch.<slug>`, `node.description = <channel name>`, F32P stereo FL/FR, flags
  `RT_PROCESS | MAP_BUFFERS`, no autoconnect. Hardware channel = INPUT `Stream/Input/Audio`
  with `target.object`, `AUTOCONNECT | RT_PROCESS | MAP_BUFFERS`. Monitor out = OUTPUT with
  `target.object`, `AUTOCONNECT | RT_PROCESS | MAP_BUFFERS`: **the tick**. Master out and
  Master source (`media.class = Audio/Source`, `node.name = <app>.master`) = OUTPUT streams
  connected with `TRIGGER`, driven by `trigger_process()` from the tick every cycle (silence
  when needed), so they run in the same cycle after the tick with no second ring. Both exist
  in the pinned pipewire-rs (`StreamFlags::TRIGGER`, `Stream::trigger_process`, feature
  `v1_0_0`).
- **Two PipeWire facts the design is built on.** (1) An unlinked stream's `process` never runs
  (no `SPA_IO_Buffers`): an app sink with no app linked pushes nothing and is silence, not an
  underrun — count underruns only while that input's state is `Streaming`; the Master source
  runs only while something records it, so it can never be the tick. (2) Order among unlinked
  nodes within a cycle is unspecified (only links order nodes), which is why Master out/source
  are `TRIGGER`ed rather than fed by rings.
- **Data flow.** Input `process`: dequeue, copy each plane into the channel's `rtrb` ring (a
  frame FIFO, capacity ≥ 3 × `default.clock.quantum-limit` = 8192 frames), requeue. Tick:
  `n = Buffer::requested()` (the quantum is the graph's, never assumed); per channel, if the
  ring holds ≥ 2n frames discard down to n (the latency governor: input lag is exactly one
  quantum whatever the order flip), if < n write silence; `Engine::process`; write Monitor
  planes; write the Master mix into two fixed staging buffers; `trigger_process()` on Master
  out and Master source, whose `process` copies the staging buffer.
- **Tick election.** Monitor out while it is linked; if the Monitor device vanishes
  (unplugged interface) reconnect Master out without `TRIGGER` as the tick; if both are gone,
  Master source is the tick and the engine runs only while something records it — the host
  shows that state. Reconnect each stream rather than re-creating the graph.
- **Registry and routing.** Devices = Node globals with `media.class` `Audio/Sink`/`Audio/Source`
  that are not ours; app streams = `Stream/Output/Audio` with `application.name`,
  `application.process.binary`, `pipewire.access.portal.app_id`, `media.name`, `object.serial`
  → `BackendEvent`s. Routing = the `default` metadata object (bind the `Metadata` global whose
  `metadata.name` is `default`):
  `set_property(node_id, "target.object", Some("Spa:String"), Some(<our sink node.name>))`;
  un-assign = `set_property(node_id, "target.object", None, None)`. WirePlumber's
  `stream.restore-target` **persists what we write** across our restarts, so an un-assignment
  must be applied when the app's node appears, not only while it runs; on clean exit nothing is
  cleared (apps keep routing to our sink until we say otherwise). Device choice =
  `target.object` on our own stream; change = reconnect that stream. Daemon loss →
  `Disconnected`, reconnect with backoff.
- **Latency.** App → audible ≈ app queue (≥ 1 q) + 1 q ring + 1 q device period ≈ 3 q = 16 ms
  at 256/48 k; mic → headphones ≈ 3 q plus converters. 128 gives ≈ 8–9 ms and is workable with
  class-compliant USB and rtkit, at a global cost (every app runs at 128). Prefs offer
  128/256/512 through `node.latency`.
- **Format pod**: `libspa::param::audio::{AudioInfoRaw, AudioFormat::F32P}` →
  `Object { SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat }` serialized with `PodSerializer`
  (pipewire-rs's `examples/audio-capture.rs`); buffers through `Buffer::datas_mut()`,
  `Data::data()`, `chunk_mut()` (per plane size = n × 4, stride 4).
- **Later (recorded, not built):** inputs without rings through an arrival bitmask (the
  last-arriving input runs the tick and triggers all outputs) for one quantum less; a single
  `pw_filter` node through `pipewire-sys`.

Two points the contract leaves to this lane: channel names that slug alike need distinct node
names (`channel_node_name` today gives both the same; suffix the channel id, or key node names
by id and keep the slug for readability), and whether a channel's node name follows a rename
(WirePlumber's restored `target.object` names the old one; routing on appear covers it).

## Steps

1. The loop, the context and the core, on their own control thread, with the
   `pipewire::channel` the host's commands arrive on.
2. The stream set with its properties and the format pod.
3. The rings, the latency governor and the tick calling `Engine::process`, allocating nothing,
   locking nothing, logging nothing on the data thread.
4. The `TRIGGER` outputs and the tick election.
5. The registry and the `BackendEvent`s (devices, app streams with their identity, xruns,
   quantum changes) through the bounded channel `connect` was given.
6. Metadata routing, including the apply-on-appear rule.
7. `apply(graph)` diffing: create, destroy or retarget streams without touching the rest.
8. Reconnect with backoff after daemon loss.
9. `cookbook/run-headless-pipewire.md`: WSL2, the VM and CI — `pipewire` + `wireplumber` with
   `support.null-audio-sink` devices, `pw-dump`, `pw-top`, `pw-play`, `pw-record`,
   `pw-metadata`, building on `scripts/ci/headless-pipewire.sh` — with its index entry under a new
   Cookbook section.
10. Integration tests against the headless daemon: a `pw-play` stream routed to `<app>.ch.test`
    reaches the ring with the governor holding one quantum; `pw-record` from `<app>.master`
    receives the master mix in the same cycle as the tick; Monitor out reaches the null sink; an
    app un-assigned while absent is returned to default when it appears; devices and app streams
    are reported with their identity; the Monitor device removed mid-run hands the tick to
    Master out without an audible gap longer than one quantum.
11. Docs: complete [reference/pipewire.md](../reference/pipewire.md).

## Files you own

`crates/vixmix-pipewire/**`, `docs/reference/pipewire.md`,
`docs/cookbook/run-headless-pipewire.md` (and its lines in `docs/cookbook/README.md` and the
index), `scripts/ci/headless-pipewire.sh`, this handoff, your rows in `docs/roadmap.md`, one
paragraph in `CLAUDE.md` § Status. A change to CI is to `.github/workflows/ci.yml`'s Linux steps
only, noted in this handoff's status line.

## Acceptance

- The integration tests run in CI's Linux leg and in WSL2 (in the `ubuntu:24.04` container
  until the user has installed WSL2's packages; the status line says which).
- `cargo clippy` clean on Linux, and the crate compiles to nothing elsewhere.
- A two-minute run with null devices at 256 shows zero xruns and every ring holding exactly one
  quantum at each tick.
- `reference/pipewire.md` with the thread table, the node/property table, the two PipeWire
  facts with their evidence, the latency figures and the forbidden list for the callbacks.

## Out of scope

Windows; anything in the engine's graph (that is 02); the UI.
