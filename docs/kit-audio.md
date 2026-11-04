# Kit parameter and audio processor adoption

**Adopted 2026-11-04, at the kit's release tag `kit-source.json` pins: the contract and the
strip maths by the bootstrap ([handoff 00](handoffs/00-bootstrap.md)), the processor chain by
the engine ([handoff 02](handoffs/02-engine.md)).** Vix Mix takes two crates of the kit's
[Rust channel](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/rust.md):
`chimaeraproject-param` (imported as `chimaeraproject_param`) and
`chimaeraproject-audio-processor` (imported as `chimaeraproject_audio_processor`). The kit's
[param reference](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/param.md)
and
[audio-processor reference](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/audio-processor.md)
define what they guarantee; this record says what Vix Mix takes, what it keeps, and what
changed observably. Both are git dependencies on the kit's release tag, at the same tag as
`chimaeraproject-fs` and `-vst3-host` (two tags would bring the parameter types in twice).

## What Vix Mix takes

- **The processor contract.** `AudioProcessor` is what a hosted plugin is to the engine:
  `vixmix_engine::Lender` lends `Box<dyn AudioProcessor>`, and `vixmix_plugins::Rack`'s
  forwarding handle implements it. `EngineConfig` is the engine's shape (`Engine::new`,
  re-exported by `vixmix-engine`), with the block always the graph's quantum limit.
- **The chain.** Every channel and both mixes run a `ProcessorChain`, filled through
  `fill_with` on the host thread and run in place by the tick over a `BusBuffer` (one working
  bus of two channels, reused strip after strip). Each processor is set up with
  `ProcessSetup::realtime` at `max_block` `MAX_QUANTUM` (8192), because PipeWire's quantum
  varies from tick to tick, with `BusLayout::stereo_insert()`.
- **The strip maths.** The tick sums a send into a mix with `mix_into` and applies a mix's
  level with `apply_gain`, both of which skip a multiply by exactly one, so a send and a mix at
  unity are bit-exact; the model's taper gives exactly 0 dB at unity so that path is taken.
- **The parameter types.** `ParamId` and `NormalizedValue` carry a plugin editor's own edits
  (`vixmix_plugins::InstanceEvent::Edit`); `ProcessorParams` and `ProcessorInstanceId` name a
  chain's inserts.

## What stays in Vix Mix

The thread model and the graph, which the kit's charter keeps out of its crates: which thread
processes (PipeWire's data thread, through the tick), the prepared graph and its acknowledged
handoff, sends, mixes, MON/MST routing, preview, meters and stats
([reference/engine.md](reference/engine.md)). Two departures from the kit's lending recipe,
both forced by preparing off the processing thread:

- **An instance is lent once and shared between graphs.** The kit's recipe clears a stale
  chain and fills it again on the thread that processes, so every instance is home when its
  new chain asks. Vix Mix prepares on the host thread while the tick runs the current graph,
  whose chains hold every instance, so the engine lends an instance once, sets it up itself and
  hands each chain a forwarding processor onto it. The chain's own calls to `set_bus_layout`,
  `setup_processing` and `set_active` on that handle never reach the plugin.
- **No refill on a generation move.** A move checks each held instance with the lender (`Busy`
  means still ours) instead of clearing every chain; an instance that must be set up again (a
  new rate, new buses) leaves the running graphs first and is set up in place.

The mixer has no automation and no `ParamSource` of its own.

## What changed observably

- **Inserts run.** Each channel's chain feeds its Monitor and Master sends, processed or dry
  per MON and MST, and each mix's chain runs before its level; before handoff 02 inserts were
  skipped.
- **A plugin's loaded state stands.** An insert's `ProcessorParams` carries no values, so the
  chain's first-block "every parameter" send is an empty initial list and a plugin keeps the
  state the plugins crate loaded into it.
- **A failed insert is a pass-through**, reported in `Engine::problems` with the engine's own
  wording (what went wrong, then what to do); the kit's `ProcessorChain::errors` stays empty,
  because a failed insert is left out of its chain rather than kept as a failed slot.

## How you know it worked

`cargo test -p vixmix-engine`: `tests/graph.rs` puts a sine through a chain at −6.02 dB and
reads half its amplitude in Monitor, bit for bit what the plugin made, and the dry sine
bit-exact in Master; `tests/no_alloc.rs` runs eight channels with chains on three and on both
mixes under `assert_no_alloc` from the second tick.
