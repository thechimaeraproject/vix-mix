# 02 — Engine

**Record — not yet built.**
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
   runs at all.
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
