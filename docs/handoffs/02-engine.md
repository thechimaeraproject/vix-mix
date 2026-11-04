# 02 — Engine

**Record — shipped 2026-11-04.** The real graph: every channel through its chain, MON and MST
choosing processed or dry per send, both mixes through their chains before level and mute,
preview, meters and DSP load, with every hosted plugin lent through `Lender` and every replaced
graph dropped on the host thread; the acceptance below is green, the contract is
[reference/engine.md](../reference/engine.md), and `dev check` passes. Deviations from the steps
below, each with its reason:

- **Step 1: an instance is lent once and shared between graphs**, not lent afresh into each
  graph's chains. `prepare` runs on the host thread while the tick runs the current graph,
  whose chains hold every lent instance, so lending again finds each one `Busy`, and nothing on
  the host thread may take one out of a running chain. The engine keeps each lent instance in
  a cell, set up by the engine on the host thread, and fills every chain with `fill_with` and a
  forwarding processor onto the cell; one graph runs per tick, so a fader move, a new insert or
  a move between strips swaps in with every plugin untouched. The lender is asked again only
  for a new instance or when its generation moves (`Busy` then means still ours).
- **Step 1: one working bus, not dry and processed buffers per channel.** The dry signal is the
  channel's input itself and strips run one after another, so a single `BusBuffer` of
  `MAX_QUANTUM` frames serves every channel and both mixes.
- **Step 1: bypass leaves the insert out of its chain but keeps its instance lent**, rather than
  sending it home or keeping it in the chain. Sent home, a quick toggle would lend an instance
  the replaced graph still holds; kept in the chain, it would cost the tick a check per insert.
- **Step 1: a failed insert is left out of its chain**, not kept as the kit's failed slot, and
  reported through a new `Engine::problems()` (`InsertProblem`), because the engine rebuilds
  every chain on every `prepare` and needs the strip and instance of each failure, which
  `ProcessorChain::errors` does not carry.
- **Step 3: replaced graphs also drop when no tick is running**, not only behind the
  acknowledgement: a `SeqCst` fence pair between the swap and the tick's flag proves no tick
  holds one. So the list of kept graphs is bounded (empty after any `prepare` or `reap` with no
  tick mid-cycle) and a stopped engine releases them at once. `prepare` may drop graphs itself.
  Lane 00's test that two unticked graphs survive a `reap` became two stronger ones: a unit
  test that they are dropped at once with nothing ticking, and `tests/handoff.rs`, where two
  graphs prepared while a gated probe holds the tick mid-cycle are kept until its
  acknowledgement passes them, and then dropped on the host thread.
- **Step 4: one tick at a time** is enforced (a `ticking` flag; a concurrent call plays
  silence) because a chain needs `&mut` and `process` takes `&self`. The `unsafe impl Sync` is
  one type, `TickOnly<T>`, holding the working bus, each graph's chains and each lent
  processor.
- **Step 7: the rate arrives through a new `Engine::set_rate`**, and an instance that asks for
  new buses through a new `Engine::set_up_again`: setting a VST3 plugin up again needs it out
  of every running graph, which only the engine knows, so it marks the instance, plays its
  insert as a pass-through, and sets it up in place once `Arc::get_mut` shows no graph holds
  it (inside the call when nothing ticks, else at the next `reap`). The plugins crate's
  `Rack::touch` alone no longer sets an instance up again. `Engine::new` replaces the given
  block and channels with `MAX_QUANTUM` and 2. These additions are change request 2 in the
  [build map](../build-map.md), proposed for lanes 04 and 06.
- **Step 8: change request 1 was decided for this lane**: the engine does not change; the
  plugins crate's handle drains editor edits. An insert's `ProcessorParams` carries no values,
  so the chain's first-block send is empty and a plugin's loaded state stands.
- **Acceptance, "a linked snapshot change arrives once"**, is read as: one command moving both
  linked sends prepares one graph, the next tick plays both new gains together, later ticks the
  same, and no plugin is set up again or told anything. **"Drops only after the
  acknowledgement"** holds while a tick is mid-cycle, which the test stages with a gated probe;
  with no tick running Step 3's decision applies.
- **DSP load rounds up**, so any tick that took time reads at least 1 ‰; the tick's transport
  is `TransportInfo::stopped()`, since a live mixer has no timeline.

**Needs:** 00. **Runs alongside:** 01, 03, 04, 05. **Size:** L.

## Goal

The realtime graph. Prepare off the audio thread, process on the data thread with nothing
allocated, locked or logged: every channel's input through its chain, split dry/processed by
MON and MST, into both mixes at its send gains, through each mix's chain, level and mute, out —
with every hosted plugin lent through `Lender` and every replaced graph dropped on the host
thread.

## Read first

- [reference/engine.md](../reference/engine.md): the contract as lane 00 left it.
  `crates/vixmix-engine/` already has `Engine` with the `ArcSwap<Prepared>` handoff and its
  generation acknowledgement, `reap`, a tick that sums inputs into the mixes with send gains and
  mutes, `MeterBank` (fetch-max peak meters), `Stats`, `Lender` / `LendError` / `NoPlugins`, the
  `NullBackend`, and `tests/no_alloc.rs`. This lane adds the chains and completes the rest.
- [build-map.md](../build-map.md) § The shared contracts and § Change requests (request 1: how
  a plugin editor's own edit reaches the lent processor — agree it with lane 04).
- The kit's
  [audio-processor.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/audio-processor.md)
  (§ The chain, § Realtime rules — a consumer processing in a callback prepares chains
  elsewhere and hands them over),
  [vst3-host.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)
  § Threads and § Lending a hosted plugin to a chain, and
  [rust.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/rust.md).
- Vix (`A:\RustroverProjects\Vix`): [crates/vix-audio/src/mixer.rs](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-audio/src/mixer.rs)
  (`Mixer::with_rack` and `prepare` are the model for filling chains from a rack),
  [crates/vix-audio/src/rack.rs](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-audio/src/rack.rs),
  [crates/vix-audio/tests/no_alloc.rs](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-audio/tests/no_alloc.rs),
  and [docs/kit-audio.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/kit-audio.md).

## What this lane owns, what others keep

It owns the graph and the thread rule on the tick. The model owns the snapshot it prepares
from (01); the backend owns the data thread, the rings and which stream is the tick (03); the
plugins crate owns the rack that lends (04); the host calls `prepare` and `reap` (06).

## Steps

1. **`Prepared`.** Per channel: a `ProcessorChain` or none, dry and processed buffers for the
   split, each send's gain linear with the ×1.0 skip, mutes; per mix: its chain, gain and mute;
   the preview flag. Build chains with `ProcessorChain::fill_with`, lending through `Lender`
   (Vix's `Mixer::with_rack` is the model); an insert that cannot be lent or set up is a
   reported pass-through, reported once. Decide bypass (`enabled: false`): leave the insert out
   of the chain so its instance stays home, or keep it and pass through.
2. **Buffers for the graph's quantum.** Size every buffer and `ProcessSetup::max_block` for the
   graph's `quantum-limit` (`MAX_QUANTUM`, 8192), because the quantum is PipeWire's and
   `num_samples` varies from tick to tick.
3. **The handoff** stays `ArcSwap<Prepared>` with the generation acknowledgement: the tick
   stores the generation it now runs in an `AtomicU64`, and the host keeps its clone of the
   previous `Prepared` until the acknowledgement passes it, then drops it on the host thread
   (no graveyard ring). The first tick warms `load` / `load_full` once (the thread-local slot
   registration can allocate on first touch). Decide what happens to kept graphs when no tick
   runs at all: today they accumulate one per `prepare`, and `CLAUDE.md` § Rules wants every
   list bounded.
4. **`Send + Sync`.** `Prepared` is shared with the tick, so it must be `Send + Sync`, and the
   tick needs `&mut` on its chains and buffers: interior mutability touched by the tick alone,
   and the VST3 processor wrappers carry an `unsafe impl` justified by the kit's thread table
   (only the tick calls `process`; the preparing thread sets up and drops). Write the
   justification beside each `unsafe`.
5. **`process`.** Channel input → chain → MON/MST source choice (processed or dry per send) →
   send gain → `mix_into` the Monitor and Master buses → mix chains → mix gain and mute →
   outputs; preview copies the Master mix to the Monitor output. Meters post-send and post-mix
   as `f32` bits in relaxed atomics (the existing `MeterBank`). `Stats` including DSP load from
   a monotonic timer.
6. **`NullBackend::pump`** keeps working for tests and Windows.
7. **`EngineConfig` from the backend's rate**: prepare again on a rate change only.
8. **Parameters for a VST3 insert.** Verify what `ProcessorParams` must carry so the chain's
   first-block "every parameter" send does not reset the plugin's loaded state (read the kit's
   audio-processor.md § The chain and Vix's kit-audio.md); the answer goes in
   `reference/engine.md`. Settle change request 1 with lane 04 (the likely shape: a bounded
   ring of `(instance, param, value)` the tick drains into the chain's `ProcessorParams`).
9. **Docs.** Complete [reference/engine.md](../reference/engine.md) (the thread table, the
   forbidden list for the tick, the handoff) and [kit-audio.md](../kit-audio.md) (the chain
   adopted).

## Files you own

`crates/vixmix-engine/**`, `docs/reference/engine.md`, `docs/kit-audio.md`, this handoff,
your rows in `docs/roadmap.md`, one paragraph in `CLAUDE.md` § Status.

## Acceptance

- `tests/no_alloc.rs` under `assert_no_alloc` from block 2 with 8 channels, chains on 3, both
  mix chains, at varying `num_samples` (64, 256, 1024) within one prepared graph.
- A sine through a channel with MON on and MST off at −6.02 dB lands in Monitor at half
  amplitude and not in Master (bit-exact where the kit says bit-exact).
- Mute beats everything.
- A linked snapshot change arrives once.
- The previous graph drops on the host thread only after the acknowledgement (a test with a
  drop counter and a thread id).
- DSP load reads above 0 under the null backend.
- `reference/engine.md` complete; `dev check` green.

## Out of scope

The data thread, the rings and the streams (03); creating and holding plugin instances (04);
when to prepare (06).
