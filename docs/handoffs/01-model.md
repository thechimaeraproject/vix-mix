# 01 — Model

**Record — not yet built.**
**Needs:** 00. **Runs alongside:** 02, 03, 04, 05. **Size:** M.

## Goal

The whole setup as pure data, with the commands that change it, the fader taper, persistence
and the engine snapshot — and no thread, device, plugin instance or window type. When this lane
ships, every gesture the window will send is a `Command` whose `Applied` says exactly what
changed, proven against the diff; `setup.ron` round-trips byte for byte and has a migration
seam; and the taper is the prototype's, pinned by its table.

## Read first

- [reference/model.md](../reference/model.md): the contract as lane 00 left it, and what
  `apply` already does. `crates/vixmix-model/` is the starting point: the types, `apply` with
  its validation, the taper, `persist`, `snapshot` and the prototype's example all exist with
  tests; this lane completes them.
- [charter.md](../charter.md) (§ Decisions: channels, app identity, quantum, devices per mix)
  and [build-map.md](../build-map.md) (§ Conflict rules, § The shared contracts, § Change
  requests — every lane compiles against this crate, so a signature change is a change
  request, agreed before it lands).
- The prototype, `docs/design/prototype/index.html` (open it in a browser):
  `TAPER`/`posToDb`/`dbToPos`/`dbTxt`; the example `channels` and `mixes`; the drop handler
  (`from.fx.splice` … `to.fx.splice`, the same-strip no-op); the source popover's app
  checklist (ticking an app removes it from every other channel); "Add channel (n of 8)".
- The kit's [fs reference](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/fs.md)
  (`atomic_write`, `load_or_default`, `app_dir::{user_config_base, resolve_from_env}`) and
  [rust.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/rust.md)
  § The rules every crate obeys.
- Vix's model as a pattern: [crates/vix-core/src/persist.rs](https://github.com/thechimaeraproject/Vix/blob/master/crates/vix-core/src/persist.rs)
  (RON, the version) and
  [docs/cookbook/add-project-command.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/cookbook/add-project-command.md)
  (a command, its validation and its test). Vix is checked out at `A:\RustroverProjects\Vix`.

## What this lane owns, what others keep

It owns the data and its rules. The engine owns what a snapshot sounds like (02), the backend
which nodes exist (03), the plugins crate the live instances an `InstanceId` names (04), the
window how a gesture becomes a command (05), and the host when to prepare, route, create,
retire and save (06).

## Steps

1. **Types.** Settle `Setup`, `Channel`, `Source`, `Send`, `Mix`, `Insert`, `PluginRef`,
   `AppRef`, `DeviceRef`, `DeviceInfo`, `Prefs` and the ids as the reference lists them. Decide
   id allocation: lane 00 takes one more than the largest id in use, but the host keeps a
   removed instance alive while its undo toast shows (5 s), so a removed id must not come back
   while it does — probably a `next_instance` / `next_channel` counter in `Setup`.
2. **`apply` and `Applied`.** Every command validated (≤ 8 channels with a message that says the
   limit; ids exist; an app in one channel only; Monitor keeps a device; a plugin by
   `vst3:<32 hex>` and bundle file name), applied whole or refused with nothing changed, and
   `Applied` saying what changed so the host knows whether to re-prepare the engine (`engine`),
   apply the backend graph (`graph`), re-route apps (`routing`), create or retire instances
   (`created`, `removed`), or just save (`save`).
3. **`MoveInsert` from the prototype.** Same-strip drops on the item's own place or the gap
   below it are no-ops; otherwise remove, then insert at the index adjusted for the removal;
   across strips the same instance moves (no re-create); with `copy` (Ctrl) a new instance
   with the same plugin, enabled flag and state.
4. **Linked faders.** Moving one send of a linked channel moves the other by the same position
   delta, clamped to 0..=1.
5. **The taper**, pinned to the prototype's table: 0.84 → 0 dB, 0.70 → −6, 0.56 → −12,
   0.36 → −24, 0.14 → −48, 0.02 → −70, ≤ 0.002 → −∞, 1.0 → +6, with the −90..−70 ramp below
   0.02; `format_db` gives `−∞`, `0.0`, `+3.1`, `-8.6`. Lane 00's port already passes these;
   keep the table points exact (unity must be the engine's ×1 path).
6. **`snapshot()`**: the flat `EngineSnapshot` the engine prepares from (linear gains, mutes,
   MON/MST, each chain's instances, plugin ids and enabled flags, preview), in the console's
   channel order.
7. **Persistence.** RON through `chimaeraproject_fs::{atomic_write, load_or_default,
   user_config_base("vix-mix"), resolve_from_env}`, with `version` and a migration seam (a
   lower version converts forward; a higher one is refused, never misread).
8. **Defaults and the fixture.** `Setup::default()` is one Mic channel; the prototype's example
   setup (`example::setup()`) is the fixture `FakeHost` serves — keep it the prototype's.
9. **Docs.** Complete [reference/model.md](../reference/model.md); update
   [kit-fs.md](../kit-fs.md) with what the finished persistence takes.

## Files you own

`crates/vixmix-model/**`, `docs/reference/model.md`, `docs/kit-fs.md`, this handoff, your
rows in `docs/roadmap.md`, one paragraph in `CLAUDE.md` § Status.

## Acceptance

- A proptest that every command's `Applied` matches the diff between the setup before and
  after (each flag set exactly when its part of the setup changed).
- A setup round-trips through RON byte-equal.
- The taper table test, and the readout strings.
- An app ticked in channel B leaves channel A.
- A ninth channel is refused with a message that says the limit.
- `reference/model.md` complete; `docs/kit-fs.md` updated; `dev check` green.

## Out of scope

Anything with a thread, a device, a plugin instance or a window; when the host saves (06); how
the window draws a fader (05).
