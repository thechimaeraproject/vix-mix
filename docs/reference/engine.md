# Engine (`vixmix-engine`)
**Owns:** `crates/vixmix-engine/`

**Reference — the contract as built by [handoff 02](../handoffs/02-engine.md) on 2026-11-04.**
`vixmix-engine` is the realtime graph. The host prepares a graph from an `EngineSnapshot` on
its own thread and swaps it in; the backend's data thread calls `Engine::process` once per
PipeWire cycle with every channel's input and the two mixes' outputs, and nothing on that path
allocates, locks or logs. It owns the thread rule and the graph, which the kit's charter keeps
out of the shared crates; the processor contract, the chain and the strip maths are the kit's
`chimaeraproject-audio-processor` ([kit-audio.md](../kit-audio.md)), and the meters and the
counters the kit's `chimaeraproject-audio-io` ([kit-audio-io.md](../kit-audio-io.md)). It knows no device, no
PipeWire type and no plugin type: hosted processors arrive through the `Lender` trait.

## What it owns and what the others keep

| The engine | The others |
|---|---|
| `Prepared`, the swap, the acknowledgement, dropping replaced graphs on the host thread | the host: calling `prepare` after a command whose `Applied` says `engine`, `reap` every tick, `set_rate` and `set_up_again` when the backend or a plugin asks |
| `process`: inputs → chains → sends → mixes → mix chains → outputs, meters, stats | the backend: the data thread, the rings, which stream is the tick, writing the outputs to PipeWire |
| lending each insert's instance once through `Lender` and sharing it between graphs; setting it up, and up again | the plugins crate's `Rack`: making instances on the UI thread, holding them and lending them; an editor's edits (change request 1) |
| `InsertProblem`: which inserts play as pass-throughs, and why | the host and the window: showing them |
| `NullBackend`: the buffers a tick borrows, for tests and for Windows | — |

## The graph

Each channel, in the snapshot's order (at most eight): its input is copied into the tick's
working bus and run in place through the channel's chain (a kit `ProcessorChain`); the Monitor
send takes the chain's output when MON is on and the dry input when it is off, and the Master
send the same by MST; each send adds its source into its mix at its linear gain with `mix_into`
(no multiply at exactly 1.0, so a send at unity is bit-exact) and records its post-send peak. A
muted or silent send adds nothing and records nothing, whatever its gain and whatever its chain
makes: **mute beats everything**. Then each mix runs its own chain over the summed sends, then
its level with `apply_gain` (the same ×1 skip), or silence when muted. With the ear button on,
the Monitor output plays the Master mix after its chain and level. A channel with no live
insert skips the copy and the chain: its dry and processed signals are the same input.

A chain runs every block whatever its sends' mutes, so a plugin's state (a compressor's
envelope, a reverb's tail) never depends on a mute. A missing input (`channel_inputs` shorter
than the snapshot) is silence and still runs the channel's chain.

**Buffers are sized for the graph's quantum limit.** `MAX_QUANTUM` is 8192, PipeWire's default
`default.clock.quantum-limit`; the quantum is PipeWire's and `frames` varies from tick to tick,
so every processor is set up with `ProcessSetup::realtime` at `max_block` `MAX_QUANTUM`, the
tick's one working bus is a `BusBuffer` of two channels and `MAX_QUANTUM` frames allocated in
`Engine::new`, and a tick takes any length up to it. `Engine::new(config)` keeps the
config's rate and replaces its block and channels with `MAX_QUANTUM` and 2; `config()` returns
what the engine runs at.

The tick's transport is `TransportInfo::stopped()`: a live mixer has no timeline, so a plugin
sees tempo 120, 4/4, not playing, with VST3's continuous time counting up from activation.

## The handoff

`prepare(&self, &EngineSnapshot, &dyn Lender)`, on the host thread, resolves every insert's
instance (§ Lending), builds a `Prepared` at the next generation (allocating as it needs),
swaps it into an `ArcSwap` and keeps the graph it replaced in a list only the host thread
touches, under a lock the tick never takes. The tick takes the `ticking` flag (an atomic swap,
then a `SeqCst` fence), loads the current graph and stores its generation in an `AtomicU64`:
its acknowledgement. `generations()` returns `(newest prepared, last acknowledged)`.

A replaced graph is dropped on the host thread, by `prepare` or `reap(&self) -> usize`, as
soon as no tick can reach it:

- **behind the acknowledgement**: a graph older than the acknowledged generation is
  unreachable, because the tick stored that generation after loading a newer graph and ticks
  are exclusive, so the tick that ran the old one has dropped its guard;
- **or when no tick is running at all**: `drop_replaced` puts a `SeqCst` fence between the
  swap and its read of `ticking`, pairing with the fence the tick puts between taking `ticking`
  and loading. A tick whose fence comes later in the single order of `SeqCst` fences loads the
  new graph; one whose fence came earlier has either finished (the read sees its `Release` of
  `ticking`, after its guard dropped) or makes the read see `true`. Reading `false` therefore
  proves no tick holds any replaced graph.

So the last reference to a graph is always the host's, a guard the tick drops is never the
last one, and there is no graveyard ring. The list of kept graphs is bounded: after any
`prepare` or `reap` with no tick mid-cycle it is empty, and while one tick is mid-cycle it
holds only the graphs prepared since that tick began. A stopped engine (no backend, or a
backend between streams) therefore releases replaced graphs at once instead of keeping one per
`prepare`.

**One tick at a time.** `process` takes `&self`, and a chain needs `&mut`; the `ticking` flag
makes ticks exclusive without a lock. A call made while another thread is inside `process`
zeroes its outputs and returns, touching nothing. `Engine` is `Send + Sync`, asserted at
compile time.

**The first tick on a thread may allocate once**: `arc_swap` registers the thread's debt slot
on its first `load`. Every later tick allocates nothing, including the first block of a graph
swapped in mid-run (`tests/no_alloc.rs`). Drop the last `Arc<Engine>` on the host thread, after
the backend has stopped ticking it: dropping the engine drops every graph and sends every lent
instance home on the dropping thread.

## Lending

`trait Lender { fn lend(&self, instance, plugin_id) -> Result<Option<Box<dyn
AudioProcessor>>, LendError>; fn generation(&self) -> u64; }`. `Ok(None)` is an instance the
lender does not hold (a missing plugin); `Err` is one it holds and cannot lend now —
`LendError::Busy` (lent and not home yet: for an instance the engine holds, to the engine
itself, which keeps it; for any other, to another borrower, and the insert is a reported
pass-through until it comes home and the generation moves) or `OtherPlugin` (the instance now
names another plugin). The generation moves whenever what the lender holds changes (an
instance added or retired); a move alone sets no held instance up again (§ Setting an instance
up again). `NoPlugins` lends nothing.

**An instance is lent once and shared between graphs.** `prepare` runs on the host thread while
the tick is still running the current graph, whose chains hold every lent instance; lending
again would find each one `Busy`, and the host thread cannot take one out of a running chain.
So the engine's registry lends an instance the first time a snapshot names it, sets it up
(`set_bus_layout` with the stereo insert layout, `setup_processing`, `set_active(true)`) on the
host thread before any graph names it, and keeps it in a cell that every later graph's chain
shares through a forwarding processor. Only one graph runs per tick and only the tick calls
`process`, so the cell is never processed twice at once. A chain still calls `set_bus_layout`,
`setup_processing` and `set_active` on its handle when it is filled; the handle answers from the
cell's record and never reaches the plugin, which may be processing in the running graph at that
moment. So a graph that moves a fader, or adds, removes, reorders, moves between strips or
bypasses an insert, swaps in with every running plugin untouched: no new setup, no
reactivation, its runtime state intact.

The registry asks the lender again only for an instance it does not hold, or when the lender's
generation has moved. On a move, each held instance is asked for again: `Busy` means it is
still the engine's and it is kept; `Ok(Some(_))` means the lender holds a new instance under that
id (the plugins crate retired the old one), which is set up and replaces it; `Ok(None)` (retired)
or `OtherPlugin` makes the insert a pass-through. An instance the snapshot stops naming is let
go: it goes home, through the lender's own handle, when the last graph holding it is dropped on
the host thread; named again before that (an undo), it is taken back rather than lent again.

**Bypass** (`enabled: false`) leaves the insert out of its chain, so a bypassed plugin costs
nothing on the tick, while its instance stays lent, set up and active, so turning it back on
is the next graph and nothing more. Sending it home instead would make a quick toggle lend an
instance the replaced graph still holds.

**Pass-throughs.** An insert that cannot be lent or set up, or that names an instance already
placed in another slot of the same snapshot, is left out of its chain and recorded as an
`InsertProblem { strip, instance, plugin_id, message }`; `problems()` returns the running
graph's, in strip order, each message saying what to do. A failure is kept and tried again only
when the lender's generation moves, so a missing plugin is not looked up on every fader move.

**Setting an instance up again** needs it out of every running graph, because a VST3 plugin
must be inactive for `setupProcessing`. `set_rate(rate) -> bool` (the backend reported a new
graph rate; the same rate is a no-op returning `false`) marks every instance, and
`set_up_again(instance) -> bool` (the plugin asked for new buses, `kIoChanged`; `false` when the
engine does not hold it) marks one. A marked instance is left out of every new graph, its insert
a pass-through but not a problem; once no graph holds it (`Arc::get_mut` proves it) `prepare` or
`reap` sets it up again in place, at the engine's rate, and builds the graph again with it. With
no tick running that happens inside the call; otherwise at the first `reap` after the tick has
moved on. The plugins crate's `Rack::touch` alone does not set an instance up again: the lender
answers `Busy` for it and it is kept, so the host calls `set_up_again` on
`InstanceEvent::IoChanged`.

`lender_generation()` is the lender generation the running graph was prepared against; the host
prepares again when the lender's has moved past it.

## Parameters

An insert's `ProcessorParams` carries its instance id and plugin id, **no values and no
lanes**. The kit's chain sends "every parameter" on an insert's first block, `is_initial()`
true, and only what changed afterwards; with no values, that first block is an empty initial
list, which the kit's VST3 adapter turns into no parameter queue at all, so the state the
plugins crate loaded through the controller on the UI thread stands (`tests/graph.rs` pins it).
The model holds no parameter values to send in any case, only each insert's opaque state bytes.
Since every `prepare` builds new chains, a value carried here would be sent again on every fader
move. Change request 1 is decided: an edit made in a plugin's own editor reaches the processor
through the plugins crate's forwarding handle, which drains the edits into the block before it
calls the plugin, so the engine passes the chain's `ProcessorParams` exactly as the kit defines
them and has no edit path of its own.

## Threads

| Thread | Calls |
|---|---|
| the data thread (the tick) | `process` only; through it each chain's `process`, so each lent instance's `process` |
| the host thread | `new`, `prepare`, `reap`, `set_rate`, `set_up_again`, `problems`, `config`, `generations`, `lender_generation`; every lend, setup, activation, deactivation and drop of an instance and of a graph |
| any | `meters()` and `stats()`: relaxed atomics |

**What the tick may not do**: allocate (beyond the first tick's one registration), take a lock,
log, block, drop the last reference to a graph or an instance, call `set_bus_layout`,
`setup_processing` or `set_active` on a plugin, or touch the lender. The `unsafe` this needs is
in one place: `TickOnly<T>`, an `UnsafeCell` with `unsafe impl Sync`, holds the tick's working
bus, each graph's chains and each lent processor. The host thread holds the value alone before
publication and after retirement; while published only the tick dereferences it, and the tick
is exclusive. Each dereference sits beside a `SAFETY` comment saying why.

## Meters and stats

Both are the kit's `chimaeraproject-audio-io` ([kit-audio-io.md](../kit-audio-io.md)),
re-exported here. `MeterBank` is the kit's bank at this mixer's shape,
`MeterBank<MAX_CHANNELS, 2, 2>`: for each of the eight channel positions two taps, its Monitor
and Master sends post-send (`strip(index)[MONITOR]`, `[MASTER]`), and two buses, each mix
post-level (`bus(MONITOR)`, Monitor's being the Master mix while previewing, and
`bus(MASTER)`); an index past the bank reads its last strip, tap or bus, so the tick never
indexes out of it. Each `PeakMeter` holds `f32` bits in an `AtomicU32`; the tick calls
`record`, a `fetch_max` on the bits (non-negative floats order as their bits do, so a peak
between two UI frames is never lost), and the host's frame calls `take`, which swaps in zero.
Peak hold and fall-off are the reader's. `Stats { xruns, frames, dsp_load_permille, rate,
quantum }` are relaxed atomics: the tick's `record_cycle(n, rate, elapsed)` adds the frames
processed, stores the cycle's length, and stores its processing time in thousandths of its
budget (frames ÷ the graph's rate), timed with `Instant` (monotonic) and rounded up, so a tick
that took any time reads at least 1; `rate` is stored by `new` and `set_rate`. `xruns` is the
backend's to count.

## The null backend

`NullBackend::new(Arc<Engine>, max_frames)` owns eight channels of planar input, both outputs,
all allocated up front. `input_mut(index)` gives a channel's left and right to fill,
`silence()` zeroes them, `pump(frames)` builds the `Tick` on the stack and calls `process`,
allocating nothing, and `monitor()` / `master()` return what the last pump wrote. `RealHost`
pumps it one quantum per poll on every platform until a real backend runs.

## Verification

`cargo test -p vixmix-engine`, with fixtures in `tests/common/mod.rs`: a rack shaped on the
plugins crate's (`Busy` while lent, retire, a generation, a handle that records the thread it
came home on), a probe processor that logs its setups, activations, blocks and parameter
changes, and a data thread that pumps on command.

- `src/engine.rs`: a channel at unity reaches Monitor bit-exact while its muted Master send
  reaches nothing; −6 dB on a send follows the taper and the ear button plays Master in Monitor;
  with no tick running a replaced graph is dropped at once and the acknowledgement follows the
  tick; stats count frames, the quantum and the rate;
  the engine runs at the quantum limit whatever block it was given, and `set_rate` changes only
  a different rate.
- `tests/graph.rs`: a sine through a −6.02 dB chain with MON on and MST off lands in Monitor at
  half amplitude, bit for bit what the plugin made, and in Master dry and bit-exact; a send at
  −6.02 dB halves the sine and a muted send carries nothing; mute beats everything (a muted
  send at the top of its fader, a muted mix whose chain makes sound, the ear button on a muted
  Master); a linked send change arrives once, in one graph and one tick, with both sends moved
  and no plugin set up again or told anything; a lent plugin's first block carries no
  parameters; a missing plugin passes audio through bit-exact, is reported, is not looked up
  again until the rack changes and is lent when it turns up; a bypassed insert is left out yet
  stays lent; a mix runs its chain on the summed sends before its level; a tick of the whole
  quantum limit runs every frame through the chain; DSP load reads above 0 under the null
  backend; a second tick at the same time plays silence.
- `tests/handoff.rs`: the previous graph drops on the host thread only after the tick
  acknowledges the next (a homecoming counter and the thread id, with the data thread held
  mid-cycle by a gated probe while two graphs are prepared, both kept until the tick
  acknowledges the newest); with nothing ticking a replaced graph and its instances go home
  at once; an instance survives a new insert and a move to another strip without a new setup;
  a retired instance goes home and its insert is reported; a new instance under a held id takes
  over; a new rate sets every instance up again once no running graph holds it, playing as a
  pass-through until then; an instance that asks for new buses is set up again; an insert put
  back before its instance went home is taken back, not lent again; an instance another
  borrower holds is reported and lent once the rack moves; one instance in two slots plays in
  the first and the second is reported; dropping the engine sends every instance home on the
  dropping thread.
- `tests/no_alloc.rs`: eight channels, chains on three (one of them two inserts deep), both mix
  chains, one send muted, MST off on one channel, preview on, through the null backend under
  `assert_no_alloc` from the second tick, at 64, 256 and 1024 frames within one prepared graph,
  then again across a graph swapped in by a fader move, reading meters and stats as it goes; an
  allocation aborts the test binary.
