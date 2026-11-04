# Engine (`vixmix-engine`)
**Owns:** `crates/vixmix-engine/`

**Reference — the contract as lane 00 wrote it; [handoff 02](../handoffs/02-engine.md)
completes it.** `vixmix-engine` is the realtime graph. The host prepares a graph from an
`EngineSnapshot` on its own thread and swaps it in; the backend's data thread calls
`Engine::process` once per PipeWire cycle with every channel's input and the two mixes'
outputs, and nothing on that path allocates, locks or logs. It owns the thread rule and the
graph, which the kit's charter keeps out of the shared crates; the processor contract, the
chain and the strip maths are the kit's `chimaeraproject-audio-processor`
([kit-audio.md](../kit-audio.md)). It knows no device, no PipeWire type and no plugin type:
hosted processors arrive through the `Lender` trait.

## What it owns and what the others keep

| The engine | The others |
|---|---|
| `Prepared`, the swap, the acknowledgement, dropping replaced graphs on the host thread | the host: calling `prepare` after a command whose `Applied` says `engine`, and `reap` every tick |
| `process`: inputs → sends → mixes → outputs, meters, stats | the backend: the data thread, the rings, which stream is the tick, writing the outputs to PipeWire |
| asking `Lender` for each insert's processor | the plugins crate's `Rack`: holding instances and lending them |
| `NullBackend`: the buffers a tick borrows, for tests and for Windows | — |

## The handoff

`Engine::new(EngineConfig)` (the kit's `EngineConfig`, re-exported: rate, largest block,
channels) starts with a silent graph at generation 0. `prepare(&self, &EngineSnapshot,
&dyn Lender)`, on the host thread, builds a `Prepared` at the next generation (allocating as it
needs), swaps it into an `ArcSwap` and keeps the graph it replaced in a list only the host
thread touches. The tick loads the current graph and stores its generation in an `AtomicU64`:
its acknowledgement. `reap(&self) -> usize`, on the host thread, drops every kept graph whose
generation is below the acknowledgement and returns how many. So the last reference to a graph
is always dropped on the host thread: a guard the tick still holds keeps its graph's
generation at or above the acknowledgement, and an `Arc` the tick drops is never the last one,
because the host still holds its clone. There is no graveyard ring. `generations()` returns
`(newest prepared, last acknowledged)`.

If nothing ticks, kept graphs accumulate one per `prepare` until something does; handoff 02
decides whether a stopped engine (no data thread at all) may release them at once.

## The tick

`process(&self, Tick { frames, channel_inputs, monitor_out, master_out })`: `frames` is the
cycle's length, which varies (PipeWire's quantum is the graph's), at most `MAX_QUANTUM` (8192,
PipeWire's default `quantum-limit`); `channel_inputs` is a slice of `ChannelInput { left,
right }` in the snapshot's channel order, a missing entry being silence; the outputs are
planar left/right. Today it zeroes the outputs; for each channel adds its input into Monitor
and Master at each send's linear gain (`mix_into`, which adds without multiplying at exactly
1.0, so a send at unity is bit-exact), skipping a muted or silent send; applies each mix's level
(`apply_gain`, the same ×1 skip) or silences a muted one; and with the ear button on, copies
the Master mix into the Monitor output. Inserts are not run yet: chains, the dry/processed
split and the mix chains are handoff 02's.

The first tick on a thread may allocate once: `arc_swap` registers the thread's debt slot on
first use. Every later tick allocates nothing (`tests/no_alloc.rs`).

## Meters and stats

`MeterBank` is a fixed array: for each of the eight channel positions a `ChannelMeters
{ monitor, master }` post-send, and one `StereoMeter` for each mix post-level (Monitor's is the
Master mix while previewing). Each `PeakMeter` holds `f32` bits in an `AtomicU32`; the tick
calls `record`, a `fetch_max` on the bits (non-negative floats order as their bits do, so a
peak between two UI frames is never lost), and the window's frame calls `take`, which swaps in
zero. Peak hold and fall-off are the reader's. `Stats { xruns, frames, dsp_load_permille, rate,
quantum }` are relaxed atomics: frames processed, the last cycle's length, and its processing
time in thousandths of its budget (frames ÷ rate), timed with `Instant`.

## Lending

`trait Lender { fn lend(&self, instance, plugin_id) -> Result<Option<Box<dyn
AudioProcessor>>, LendError>; fn generation(&self) -> u64; }`. `Ok(None)` is an instance the
lender does not hold (a missing plugin: its insert becomes a reported pass-through); `Err` is
one it holds and cannot lend now — `LendError::Busy` (still in another chain: clear every stale
chain before filling any) or `OtherPlugin` (the instance now names another plugin). The
generation moves whenever what the lender holds changes, so the engine prepares again even
when the snapshot did not; `Prepared` records the generation it was built against
(`lender_generation()`). `NoPlugins` lends nothing. `prepare` does not lend yet.

## The null backend

`NullBackend::new(Arc<Engine>, max_frames)` owns eight channels of planar input, both outputs,
all allocated up front. `input_mut(index)` gives a channel's left and right to fill,
`silence()` zeroes them, `pump(frames)` builds the `Tick` on the stack and calls `process`,
allocating nothing, and `monitor()` / `master()` return what the last pump wrote. `RealHost`
pumps it one quantum per poll on every platform until a real backend runs.

## Verification

`cargo test -p vixmix-engine`: a channel at unity reaches Monitor bit-exact while its muted
Master send reaches nothing, and its meters read the input's peak; −6 dB on a send is
10^(−6/20) on Master and the ear button plays Master in Monitor; a replaced graph is not reaped
before the tick acknowledges the new one and is reaped once it has; stats count frames, the
quantum and the rate; a peak meter keeps the loudest of several records and a take resets it.
`tests/no_alloc.rs` runs eight channels (one send muted, preview on) through the null backend
under `assert_no_alloc` from the second tick, at 64, 256 and 1024 frames within one prepared
graph, reading meters as it goes; an allocation aborts the test binary.
