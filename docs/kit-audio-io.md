# Kit audio I/O adoption

**Adopted 2026-11-07 by the kit lifts ([handoff 07](handoffs/07-kit-lifts.md)), at the kit's
release tag `kit-source.json` pins.** Vix Mix takes `chimaeraproject-audio-io` (imported as
`chimaeraproject_audio_io`) for the meters its tick writes and the counters it keeps, and
deleted its own: the crate was lifted from this engine's `meters.rs` together with Vix's device
outputs and clock. The kit's
[audio-io reference](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/audio-io.md)
defines what the crate guarantees; this record says what Vix Mix takes, what stays here, and
what changed observably. It is a git dependency on the kit's release tag in the root
`Cargo.toml`'s *The kit* group, at the same tag as the other kit crates, **without its default
`cpal` feature**: PipeWire is this mixer's driver, so it takes no device layer and builds
against no ALSA.

## What Vix Mix takes

- **`MeterBank`, `StereoMeter`, `PeakMeter`.** The engine's meters are the kit's bank at this
  mixer's shape, `MeterBank<MAX_CHANNELS, 2, 2>` (`vixmix_engine::MeterBank`, with the tap and
  bus indices `MONITOR` and `MASTER`): each channel's two sends post-send and each mix
  post-level, `f32` bits in relaxed atomics, `record` a `fetch_max` on the tick and `take` a
  swap to zero on the host's frame ([reference/engine.md](reference/engine.md) § Meters and
  stats).
- **`Stats`**, the five counters (`xruns`, `frames`, `dsp_load_permille`, `rate`, `quantum`)
  under the same names: the tick's `record_cycle` adds the frames, stores the quantum and the
  load rounded up; the backend's data thread counts `xruns` into the same field.

## What stays in Vix Mix

The thread model, the graph and the driver: which thread ticks, the prepared graph and its
handoff, sends and mixes, which meter means what (the channel's position in the snapshot, the
two taps and buses), what the host makes of them (`MeterFrame`, `StatsFrame`), and the PipeWire
client (`vixmix-pipewire`) with its own rings and counters (`BackendStats`). The device layer,
`TimerOutput`, `NullOutput` and `AudioClock` are not used: the engine's `NullBackend` already
drives the tick headless, and a Windows backend over the kit's devices is named in the roadmap.

## What changed observably

Nothing a user or the window could see: the same meters at the same places, the same counters
with the same arithmetic. The engine's public names moved: `ChannelMeters` is gone, and a
channel's sends are read as `meters().strip(index)[MONITOR]` and `[MASTER]` (or
`tap(index, MONITOR)`) and the mixes as `meters().bus(MONITOR)` and `bus(MASTER)`, where they
were `channel(index).monitor`, `.master`, `monitor()` and `master()`.

## How you know it worked

`cargo test -p vixmix-engine` (`tests/graph.rs` reads the post-send and post-mix meters and the
counters; `tests/no_alloc.rs` runs eight channels with the meters and `record_cycle` under
`assert_no_alloc`), `cargo test -p vixmix-host` (the meters through `RealHost` on the null
backend) and, on Linux, `cargo test -p vixmix-pipewire` against the headless daemon. The kit's
own `tests/no_alloc.rs` proves the bank and the counters allocation-free.
