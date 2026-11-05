# Build map

**As of 2026-11-04: wave 1 (lane 00) is done; wave 2 is next.** The plan of record for building
Vix Mix: nine lanes, the order they run in, how several agents work at once without stepping on
each other, and how the contracts they share change. Each lane's full brief is its
[handoff](handoffs/README.md); this page is what they have in common, and it is updated in the
commit that changes any of it.

## The lanes

| Lane | Owns | Needs | Size |
|---|---|---|---|
| [00 — Bootstrap](handoffs/00-bootstrap.md) | the root, the crate and app skeletons with the shared contracts, the docs, CI | — | S |
| [01 — Model](handoffs/01-model.md) | `crates/vixmix-model/` | 00 | M |
| [02 — Engine](handoffs/02-engine.md) | `crates/vixmix-engine/` | 00 | L |
| [03 — PipeWire backend](handoffs/03-pipewire-backend.md) | `crates/vixmix-pipewire/` (Linux only) | 00 | L |
| [04 — Plugins](handoffs/04-plugins.md) | `crates/vixmix-plugins/` | 00 | M |
| [05 — App](handoffs/05-app.md) | `app/` | 00 | XL |
| [06 — Host and integration](handoffs/06-host-and-integration.md) | `crates/vixmix-host/`, `app/src/main.rs` wiring | 01, 02, 03, 04 | L |
| [07 — Kit lifts](handoffs/07-kit-lifts.md) | `audio-io` and `egui-audio` in the kit repository | 06 | M |
| [08 — Packaging and the first real test](handoffs/08-packaging-and-first-real-test.md) | the desktop entry, metainfo, install path, the first by-hand pass | 06 | S |

## Waves

- **Wave 1: 00**, one session, serial. Ends with the repository on GitHub, CI green on both
  legs, the handoffs in the index. The repository creation, the first push and `kit protect`
  are the main session's (the hooks refuse a push from an agent).
- **Wave 2: 01, 02, 03, 04 and 05 in parallel.** Each lane is one agent in its own git
  worktree on branch `lane/NN-slug` (below), committing locally; the main session runs
  `kit push` and `kit merge` for each. 03 needs a Linux shell for its integration tests: WSL2
  or the Fedora VM once they have the packages and rustup, and until then an `ubuntu:24.04`
  container under Docker Desktop, as lane 00 used; 05 runs on Windows against `FakeHost`.
- **Only the user** can ready WSL2 for 03: its `sudo` asks for a password, so no agent can
  install the packages and rustup that [handoff 03](handoffs/03-pipewire-backend.md) § Read
  first lists. Until the user has, 03 runs its tests in the container.
- **Wave 3: 06** once 01–04 have merged (05 may still be landing polish); then **07 and 08** in
  parallel.

## A worktree per lane

| Lane | Branch | Worktree (beside this checkout) |
|---|---|---|
| 01 | `lane/01-model` | `../vix-mix-01` |
| 02 | `lane/02-engine` | `../vix-mix-02` |
| 03 | `lane/03-pipewire-backend` | `../vix-mix-03` (or a Linux checkout) |
| 04 | `lane/04-plugins` | `../vix-mix-04` |
| 05 | `lane/05-app` | `../vix-mix-05` |
| 06 | `lane/06-host-and-integration` | `../vix-mix-06` |
| 08 | `lane/08-packaging-and-first-real-test` | `../vix-mix-08` |
| 09 | `lane/09-round-1-fixes` | `../vix-mix-09` |

07 works in the kit's repository, co-developed with `kit link` from a Vix Mix worktree.

## How to run a lane

1. **The main session makes the worktree** from the default branch as origin has it:
   `git fetch origin`, then Claude Code's `EnterWorktree` or
   `git worktree add ../vix-mix-NN -b lane/NN-slug origin/master`. In the worktree, `kit sync`
   installs the tooling and fetches the locked crates, and `kit hooks install` is not needed
   (`core.hooksPath` is the relative `.githooks`, shared by every worktree; `kit push` resets an
   absolute one).
2. **The lane agent is given its handoff and nothing else**: `docs/handoffs/NN-slug.md` is
   self-contained. It reads the files under *Read first*, works its *Steps*, and commits on its
   branch with `git commit -F <file>`, subjects that say what and why. It never pushes, tags,
   merges or opens a PR; the Claude Code guard refuses those.
3. **It proves its acceptance locally**: `dev check` green (fmt, clippy, test, check-docs), and
   whatever its *Acceptance* adds. Lane 03 runs its integration tests on Linux against a
   headless daemon; on Windows its crate compiles to nothing.
4. **It finishes its docs in the same branch**: its reference doc complete (the contract as
   built, `## Verification` naming the tests), its handoff's first line flipped to
   `**Record — shipped <date>.**` with every deviation and why, its rows in
   [roadmap.md](roadmap.md), one dated paragraph in `CLAUDE.md` § Status, and any `kit-*.md`
   record it was named in flipped to adopted.
5. **The main session pushes and merges**: `kit push lane/NN-slug --yes` (runs the push gate,
   opens the PR, CI runs both legs), then `kit merge lane/NN-slug` (waits for
   `check (linux)` and `check (windows)`, rebases when behind, merges, removes the worktree).
   Whichever lane is green first merges first; the rest rebase, and `kit push` says when.

## Conflict rules

- **A lane edits only its own files**: its crate directory (or `app/`), its reference doc, its
  handoff, and the `kit-*.md` record its handoff names.
- **Shared files are append-only**, one place each:
  - root `Cargo.toml`: a new workspace dependency is one line in its group, never moving
    another; a crate's own dependencies go in its own `Cargo.toml`.
  - `Cargo.lock`: on a conflict, take the default branch's lock and run `cargo build` (which
    adds only what is missing); never `cargo update`, which would move every other lane's
    dependencies too. `kit bump` alone moves the kit crates.
  - `docs/README.md`: one index line, in alphabetical place, for a doc the lane adds.
  - `CLAUDE.md` § Status: one dated paragraph per lane.
  - `docs/roadmap.md`: the lane's own rows.
- **`FakeHost` is the window's fixture.** While lane 06 has not started, lane 05 may edit
  `crates/vixmix-host/src/fake.rs` (and nothing else in that crate) to give the window the
  example data it needs; the change is noted in its handoff's status line.
- **The shared contracts change only through a change request** (below), because every other
  lane compiles against them.
- **Every PR passes `dev check` locally** (`kit push` runs it) **and `check (linux)` and
  `check (windows)` in CI.**

## The shared contracts

Lane 00 wrote them as compiling skeletons with real minimal behaviour (no `todo!()`); each
lane replaces its skeleton with the real thing behind the same signatures. The ownership and
the direction are fixed; exact shapes are each lane's to refine through a change request.

| Contract | Owner | Read by | Reference |
|---|---|---|---|
| `Setup` and its types, `Command`, `apply` → `Applied` / `Refused`, `taper`, `persist`, `EngineSnapshot` / `snapshot`, `DeviceInfo` | 01 | everyone | [model.md](reference/model.md) |
| `Engine` (`new`, `prepare`, `reap`, `process`, `meters`, `stats`), `Tick`, `ChannelInput`, `Lender` / `LendError`, `MeterBank`, `Stats`, `NullBackend` | 02 | 03, 04, 06 | [engine.md](reference/engine.md) |
| `BackendConfig`, `BackendEvent`, `BackendGraph`, `Backend` (`connect`, `apply`, `route`, `disconnect`) | 03 | 06 | [pipewire.md](reference/pipewire.md) |
| `Catalog`, `PluginKind`, `Rack` (implements `Lender`), `Instances`, `EditorSession` | 04 | 05, 06 | [plugins.md](reference/plugins.md) |
| `Host`, `HostEvent`, `BackendState`, `MeterFrame`, `StatsFrame`, `RealHost`, `FakeHost` | 06 | 05 | [host.md](reference/host.md) |

The dependency direction: `vixmix-model` depends on no other workspace crate; `vixmix-engine`
on the model; `vixmix-plugins` on the model and the engine (for `Lender`); `vixmix-pipewire` on
the model and the engine; `vixmix-host` on all four; `app/` on the host and the model only.

## Change requests

A lane that needs a contract to change writes the request here — what, why, which lanes
compile against it — in its branch, and stops on that point until the main session agrees it
with the lanes it touches. An agreed request is applied by the owning lane (or by the main
session across lanes) in one commit that updates the reference docs it changes.

| # | Contract | Request | Status |
|---|---|---|---|
| 0 | all | Lane 00's refinements of the plan's sketch, each forced by Rust or by a rule the plan states, recorded in the reference doc it changed: `Command`'s variants carry named fields; `DeviceInfo` moved into the model (the Linux-only backend and the host both name it); `Backend::connect` takes a bounded `SyncSender` (every channel is bounded); `Host::tick_editors` returns each event with its instance (the window must know which editor asked); `Rack` lends through a weak reference to itself so `impl Lender for Rack` holds; `Instances::create` takes the `Catalog` (the bundle's path is the scan cache's) | applied 2026-11-04 |
| 1 | `Lender`, `Instances` | A plugin's own editor reports `InstanceEvent::Edit`, and the kit never short-cuts an edit to the processor: lanes 02 and 04 agree how an edit reaches the lent processor's `ParamChanges` (a bounded ring from the UI thread that the tick drains into the chain's `ProcessorParams` is the likely shape), and 06 wires it | agreed 2026-11-04: lane 04 implements it in the rack's forwarding handle; 02 untouched; 06 wires Instances::tick (the shape: [reference/plugins.md](reference/plugins.md) § The edit path) |
| 2 | `Command` | Undo for the toast: `RestoreChannel { at, channel }` puts a removed `Channel` back at its index with its id, chain and instance ids, and `RestoreInsert { to, insert }` puts a removed `Insert` back in a slot with its instance id. Each is refused when an id is in use or not below its counter, past 8 channels or past the chain's end; a restored channel's apps another channel took since stay there. `Applied` lists the restored instances in `created` (the host lends the one it kept, or creates it from the state). Without them the window can only undo by adding anew, which loses the channel's place and re-creates its plugins. Read by 05 (the toasts) and 06 (keeping the removed instance) | agreed 2026-11-04 (main session); built in `vixmix-model` by lane 06 |
| 3 | `Setup`, `Refused`, `persist` | Lane 01's additions, recorded in [reference/model.md](reference/model.md): `Setup::next_channel` and `Setup::next_instance` (handoff 01 step 1; a struct literal of `Setup` outside the model must add them or use `..Setup::default()`), `Refused::OutOfIds`, `persist::Invalid` with `PersistError::NotText` and `PersistError::Invalid`, `persist::VERSION` at 2. Every other signature is unchanged | agreed and applied 2026-11-04 (main session) |
| 4 | `Rack`, `Instances`, `ScanOutcome`, `EditorSession` | Lane 04's refinements of the plugins contract, recorded in [reference/plugins.md](reference/plugins.md) and [handoff 04](handoffs/04-plugins.md); `vixmix-host` compiles against them unchanged. Changed: `Rack::add` returns the instance's `EditSender` (was `()`), for change request 1; `Instances::reap` returns the ids it dropped (was `()`); `Instances::create` treats a state that does not load as a notice in `take_notices()`, not an `Err` (the plugin starts from its defaults, Vix's rule), and retires a live instance of the same id first; `ScanOutcome` moved to `scan.rs` (still re-exported at the crate root) and carries the scan's `cache` and its `described` and `reused` counts. Added: `Instances::sync`, `retry_failed`, `take_notices`, `set_param`, `live`, `retiring`; `Rack::is_lent`, `len`, `is_empty`; `Catalog::rescan`, `vendors`; `ScanCache` (`load`, `forget_failures`); `search_dirs`; `EditorSession::tick`, `can_resize`, `check_size`, `size`, `set_scale_factor`; `FILE_NAME`, `EDIT_RING`, `EditSender` (`send`, `flush`, `waiting`, `dropped`); `ScanOutcome::save`; `ScanCache::len`, `is_empty`. Lanes 05 and 06 compile against them; until 06 drains `take_notices()`, `RealHost` only logs a missing plugin or a state that did not load | agreed and applied 2026-11-04 (main session) |
| 5 | `Engine`, `Lender` | Lane 02's additions and one reading of the lender, all inside `vixmix-engine` and additive: `Engine::set_rate(rate) -> bool` (the backend's `Connected { rate }`; instances are set up again at it), `Engine::set_up_again(instance) -> bool` (call it on `InstanceEvent::IoChanged`: `Rack::touch` alone no longer sets an instance up again, because the engine keeps a held instance whose lend answers `Busy`), `Engine::problems() -> Vec<InsertProblem>` (inserts playing as pass-throughs, with why); `Engine::new` replaces the block and channels with `MAX_QUANTUM` and 2. Why: `prepare` runs while the running graph holds every instance, so the engine lends each once and shares it between graphs, asks the lender again only when its generation moves, and reads `Busy` for an instance it holds as still its own; only the engine knows when an instance is out of every graph and may be set up again ([reference/engine.md](reference/engine.md) § Lending). Two readings change for the lanes that agree it: a lender generation move no longer sets a held instance up again, and `Busy` for an instance the engine holds means still the engine's. Lanes: 06 wires the calls (`set_rate` on `BackendEvent::Connected { rate }`, `set_up_again` on `InstanceEvent::IoChanged`, `problems()` shown in the window, `reap` every tick); 04's `Rack` already answers as required (`Busy` while lent, `Ok(Some(_))` for a new entry under a retired id), and its docs that say a generation move lends afresh or sets an instance up again (`rack.rs`'s module doc and `Rack::touch`, `InstanceEvent::IoChanged`'s doc in `instances.rs`, [reference/plugins.md](reference/plugins.md)'s lending) are reworded when it is agreed; handoff 06 gains those steps | agreed and applied 2026-11-04 (main session); the follow-ups in lanes 04 and 06 are the main session's edits at merge |
| 6 | `BackendEvent`, `Backend`, the node names | Lane 03's additions, all additive for the host (06), which compiles only `Backend::connect` and `BackendConfig` today; made in lane 03's branch and recorded in [reference/pipewire.md](reference/pipewire.md): `BackendEvent::TickSource(TickSource)` (which output drives the engine — the state the handoff says the host shows), `Backend::stats() -> BackendStats` (the data thread's counters), `channel_node_name(app, ChannelId, name)` = `<app>.ch<id>.<slug>` (the point the contract left to 03), `monitor_out_node_name`, `master_out_node_name`, `DISPLAY_NAME`, `Backend: Send + Sync`; `BackendError::NotBuilt` removed | agreed and applied 2026-11-04 (main session) |
| 7 | `vixmix-host` (06's crate) | `RealHost`'s test `it_boots_on_the_null_backend_and_says_so_first` asserts the Linux banner says "not built yet". With the backend built, `connect` succeeds against CI's daemon and `start_backend` turns that into `BackendState::Connecting`, which has no banner, while the null backend is pumped only while `Absent`: `check (linux)` fails on that test alone, and until 06 keeps the backend a Linux desktop with PipeWire would show neither a banner nor meters. Lane 03 may not edit the host. Proposed, and proven on lane 03's branch rebased on master, in the container (the whole Linux workspace green with it: fmt, clippy `-D warnings`, every test): until 06, `start_backend` leaves the daemon again and reports `Absent` with "PipeWire answers, but this build does not play through it yet: the mixer runs silent" (the null backend keeps the meters live, as today), or "No audio backend: " and PipeWire's refusal as now; the test asserts on Linux that the banner names PipeWire and keeps every other check on every platform; in the same commit `docs/reference/host.md` (the boot paragraph's `NotBuilt` and Verification's "not-built message"), `docs/reference/app.md` (§ What a frame draws, the banner's "not-built message on Linux until handoff 03") and the comments at the top of `real.rs` and on `start_backend`. The alternative, accepting `Connecting` on Linux and skipping the stats checks there, leaves the window with no banner and no meters until 06 | agreed and applied 2026-11-04 (main session) |
| 8 | `Backend::disconnect` | Lane 06's shutdown clears every routing and then disconnects, as handoff 06 step 6 says. `disconnect` set the quit flag in the same mailbox the routings wait in, and the control thread returned on the quit before writing them (or, if it wrote them, closed the core before they left): a routing posted just before `disconnect` was lost, measured in the container (the app's `target.object` still named the gone sink). Additive, inside `vixmix-pipewire`: the control thread writes the routings that wait with a quit, then leaves only after a core roundtrip comes back (at most `LEAVE_WAIT`, a second), so every write before it has reached the daemon; a graph that waits with a quit is not applied. Proven by `routings_posted_just_before_disconnect_reach_the_daemon` (fails without the change), recorded in [reference/pipewire.md](reference/pipewire.md). Lanes: 06 relies on it; nothing else calls `disconnect` | agreed and applied 2026-11-04 (main session) |
| 9 | `Backend` | The preferences' quantum (128, 256 or 512) changes while everything plays. `node.latency` was set only when a stream was made, so lane 06's host changed the quantum by disconnecting and connecting again, which destroyed and made again every node of ours: the verifier measured, in the container, a `pw-play` app routed back into its channel and then erring every cycle and silent (3 of 3 runs), and a recorder of `<app>.master` (Discord, OBS) loses its link with the node. Additive, inside `vixmix-pipewire`: `Backend::set_quantum(&self, quantum)` posts the quantum to the mailbox (latest wins); the control thread updates `node.latency` on every live stream in place (`pw_stream_update_properties`), keeps it for every stream made later, and takes it before a graph that waits with it. Proven by `a_quantum_asked_for_in_place_keeps_every_node_and_link` (fails without the update: no `QuantumChanged(512)`) and the host's `a_new_quantum_keeps_the_app_in_its_channel_and_the_master_source_recording`, recorded in [reference/pipewire.md](reference/pipewire.md). Lanes: 06 calls it from `Audio::apply_graph` and no longer reconnects; nothing else changes the quantum | agreed and applied 2026-11-04 (main session) |
| 10 | `Host` (06's crate), `Command::SetPrefs` (the model) | Two things the window (05) still needs, each additive. `resize_editor(&mut self, instance: InstanceId, size: EditorSize)` on `Host`, with `FakeHost`'s side doing nothing: an editor's window takes the size its plugin asks for (`EditorEvent::ResizeRequested`), and the kit's resize is asynchronous (`EditorSession::resize` once the window has it), so without the call the plugin is never told its window has the size; today the window resizes and stops there. And a bound on `SetPrefs`'s extra plugin folders: the model takes any number, while every list is bounded; the window holds 16 (`popover::MAX_PLUGIN_DIRS`) and says to remove one first, so the model should refuse a 17th with the same words (a `Refused` that says to remove a folder first). Lanes: 06 implements `resize_editor` in both hosts; 01 (or the main session) bounds the folders; 05 wires the call at one site (`editors.rs`, `window_event` on `Resized`) | agreed 2026-11-04 (main session); built in round 1 (handoff 09) |
