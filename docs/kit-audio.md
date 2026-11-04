# Kit parameter and audio processor adoption

**Adopted 2026-11-04 by the bootstrap ([handoff 00](handoffs/00-bootstrap.md)), at the kit's
release tag `kit-source.json` pins; the processor chain is pending
[handoff 02](handoffs/02-engine.md).** Vix Mix takes two crates of the kit's
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
  re-exported by `vixmix-engine`).
- **The strip maths.** The tick sums a channel into a mix with `mix_into` and applies a mix's
  level with `apply_gain`, both of which skip a multiply by exactly one, so a send and a mix at
  unity are bit-exact; the model's taper gives exactly 0 dB at unity so that path is taken.
- **The parameter types.** `ParamId` and `NormalizedValue` carry a plugin editor's own edits
  (`vixmix_plugins::InstanceEvent::Edit`).
- **Pending handoff 02:** `ProcessorChain` per strip, filled through `fill_with` from the
  rack, with `ProcessorParams` for each insert; `BusBuffer` for the dry/processed split;
  `ProcessSetup::realtime` sized for the graph's quantum limit.

## What stays in Vix Mix

The thread model and the graph, which the kit's charter keeps out of its crates: which thread
processes (PipeWire's data thread, through the tick), the prepared graph and its acknowledged
handoff, sends, mixes, MON/MST routing, preview, meters and stats
([reference/engine.md](reference/engine.md)). The mixer has no automation and no `ParamSource`
of its own; a snapshot's values arrive at offset 0.

## What changed observably

Nothing yet beyond the bootstrap's tick: a channel at unity reaches Monitor bit-exact. Handoff
02 records what the chain changes, including what `ProcessorParams` must carry for a VST3 insert
so the chain's first-block "every parameter" send does not reset a plugin's loaded state.

## How you know it worked

`cargo test -p vixmix-engine`, including `tests/no_alloc.rs`; handoff 02 extends both with
chains on.
