# 07 — Kit lifts

**Record — not yet built.**
**Needs:** 06. **Runs alongside:** 08. **Size:** M. Works in the kit's repository, co-developed
with `kit link`.

## Goal

Per the kit's rule, a crate is extracted when a consumer takes it and deletes its copy. With
Vix Mix integrated, two pieces have a second consumer: lift them into the kit, release, and have
both Vix and Vix Mix take them and delete their copies.

## Read first

- The kit's [CLAUDE.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/CLAUDE.md)
  § Rust channel, [rust.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/rust.md),
  [the Rust channel's plan](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/kit-rust-channel-implementation-plan.md)
  (§ 5.5 `audio-io`, the `egui-audio` entry, the order),
  [repos.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/repos.md),
  [kit-cli.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/kit-cli.md)
  (`kit link`, `kit release`, `kit bump`) and
  [Release and propagate](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/cookbook/release-and-propagate.md).
- Vix (`A:\RustroverProjects\Vix`): [crates/vix-audio/src/output.rs](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-audio/src/output.rs)
  (`NullOutput` / `TimerOutput` seeds, the cpal device layer),
  [crates/vix-audio/src/clock.rs](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-audio/src/clock.rs)
  (`AudioClock`), and [app/src/panels/mixer.rs](https://github.com/thechimaeraproject/Vix/blob/master/app/src/panels/mixer.rs).
- Here: [reference/engine.md](../reference/engine.md) (`MeterBank`, `Stats`) and
  [reference/app.md](../reference/app.md) (`MeterFader`, the scale, peak hold).

## What this lane owns, what others keep

It owns the new kit crates and the switch-over commits in Vix and Vix Mix. The kit keeps its
rules (no thread model, no window toolkit, no audio graph in a shared crate — `egui-audio` is
the one crate allowed to pin egui and wgpu); each consumer keeps its graph and its window.

## Steps

1. **`chimaeraproject-audio-io`**: `MeterBank`, `Stats`, the `NullOutput` / `TimerOutput`
   seeds from Vix's `output.rs`, and the cpal device layer with `AudioClock` for Vix.
   Acceptance: Vix deletes `output.rs` / `clock.rs`, and Vix Mix deletes its `MeterBank` and
   `Stats`.
2. **`chimaeraproject-egui-audio`**: `MeterFader`, the scale, the taper trait, peak hold.
   Acceptance: Vix's mixer panel takes it, and so does Vix Mix's console.
3. **`vst3-host` fixes** that lanes 04 and 05 filed with `kit-report-an-issue`.
4. Each lift is its own kit PR, released by `kit release`, and taken by `kit bump` here and in
   Vix.
5. **The kit's docs:** `CLAUDE.md` § Rust channel, `repos.md` (the vix-mix row from `planned`
   to `consumer`), the channel plan's status. Here: the `kit-*.md` records for the new crates,
   and the references whose code moved.

## Files you own

The new crates under the kit's `crates/`, their kit references and index entries; in Vix Mix,
the deletions and the new `docs/kit-*.md` records; in Vix, its switch-over commits.

## Acceptance

Both consumers take each released crate with `kit bump` and delete their copies; `dev check`
(and Vix's gates) green; `kit doctor` green in all three repositories.

## Out of scope

`proc` (plugin isolation, v2); a Windows backend for Vix Mix (named in the roadmap).
