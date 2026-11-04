# Model (`vixmix-model`)
**Owns:** `crates/vixmix-model/`

**Reference — the contract as lane 00 wrote it; [handoff 01](../handoffs/01-model.md)
completes it.** `vixmix-model` is the whole setup as pure data: up to eight channels with
their sources, chains and sends; the Monitor and Master mixes; the commands that change it,
validated and applied whole or not at all, each saying what it changed; the fader taper; RON
persistence of `setup.ron` over the kit's `chimaeraproject-fs`; and the flat
`EngineSnapshot` the engine prepares from. It holds **no thread, device handle, plugin
instance or window type**, and depends on no other workspace crate: everything else reads it,
and it reads nothing of theirs.

## What it owns and what the others keep

| The model | The others |
|---|---|
| the setup's types and their invariants (≤ 8 channels, an app in one channel only, Monitor always has a device, a plugin stored by id and bundle name) | the engine: what a snapshot sounds like; the backend: which nodes exist; the plugins crate: the live instances an `InstanceId` names |
| `apply`: validation, the change, and `Applied` (what to do next) | the host: doing it — prepare, apply the graph, route, create or retire instances, save |
| the taper: position ↔ dB, the readout text | the window: drawing a fader and its meter at a position |
| `setup.ron`: where, the format, the version | the host: when to save (debounced) and what to do with an unreadable file |

## The setup

`Setup { channels, monitor, master, preview_master, prefs, version }`. A `Channel` is
`{ id, name, source, chain, chain_to_monitor, chain_to_master, monitor, master, link }`;
`Source` is `Hardware(Option<DeviceRef>)` or `Apps(Vec<AppRef>)`; `Send { position, mute }`
is a fader (a position in 0..=1, the taper's domain) and its mute, used for each channel's two
sends and for each mix's level; `Mix { id, device, level, chain }`; `Insert { instance,
plugin, enabled, state }` with `state` the plugin's `PluginState::encode` bytes, captured
before a save. `PluginRef { id, bundle, name }` stores `vst3:<32 hex>` and the bundle's file
name, never a path. `AppRef { app_id, binary, name }` stores PipeWire's
`pipewire.access.portal.app_id`, `application.process.binary` and `application.name`, and
`AppRef::matches` compares app id when both have one, else binary, else name.
`DeviceRef { name, description }` is `node.name` and `node.description`; `DeviceInfo` is a
present device with its direction, here because both the Linux-only backend and the host name
it. `Prefs { quantum, plugin_dirs }`: 128, 256 or 512 frames asked through `node.latency`, and
extra VST3 directories. `Setup::default()` is the first start: one hardware channel named Mic,
no device yet, everything at unity.

`ChannelId(u32)` and `InstanceId(u32)` are `Copy` with public fields; `MixId` is `Monitor` or
`Master`. **`Send` shadows the standard `Send` marker trait** where it is imported by name:
write `vixmix_model::Send` (or import it under another name) in any module with thread bounds.

## Commands

`apply(&mut Setup, Command) -> Result<Applied, Refused>` applies a command whole or refuses
it and leaves the setup as it was. The variants: `AddChannel { kind }`, `RemoveChannel(id)`,
`Rename { ch, name }`, `SetSource { ch, source }`, `SetSend { ch, mix, position }`,
`SetMute { ch, mix, mute }`, `SetLink { ch, link }`, `SetChainRouting { ch, mix, on }`,
`AddInsert { strip, plugin, at }`, `RemoveInsert { instance }`,
`MoveInsert { from, to, copy }` (two `SlotRef { strip, index }`), `SetInsertEnabled`,
`SetInsertState`, `SetMixDevice { mix, device }`, `SetMixLevel { mix, position }`,
`SetMixMute { mix, mute }`, `SetPreview(bool)`, `SetPrefs(Prefs)`. `Strip` is
`Channel(ChannelId)` or `Mix(MixId)`.

`Applied { save, engine, graph, routing, created, removed, channel }` says what changed:
`engine` — derive the snapshot and prepare; `graph` — the backend's nodes or devices;
`routing` — app assignments; `created` / `removed` — instances to make or retire; `channel` —
what `AddChannel` made. All false and empty is no change (a drop on its own place); `changed()`
says whether to save.

What lane 00's `apply` already does, as the prototype does it:

- **Limits and ids.** A ninth channel is refused (`TooManyChannels`, "remove a channel before
  adding another"); an unknown channel, instance or slot is refused with what to read again.
  A new channel or instance takes one more than the largest id in use (handoff 01 decides
  whether a removed id may come back, since the host keeps a removed instance alive while its
  undo toast shows).
- **An app in one channel only.** `SetSource` with apps removes each ticked app from every
  other channel and de-duplicates the list (the prototype's checklist).
- **Linked faders.** `SetSend` on a linked channel moves the other send by the same position
  delta, clamped to 0..=1. A position outside 0..=1 (or NaN) is refused.
- **Moves.** `MoveInsert` within a strip is a no-op when dropped on its own place or the gap
  below it, and otherwise removes then inserts with the index adjusted, as the prototype's
  drop does; across strips the same instance moves; with `copy` a new instance is made with the
  same plugin, enabled flag and state.
- **Devices and plugins.** `SetMixDevice { Monitor, None }` is refused; an `AddInsert` whose id
  is not `vst3:` and 32 hex digits, or whose bundle is a path, is refused; a quantum other
  than 128, 256 or 512 is refused.

Every `Refused` message ends with what to do instead.

## The taper

`taper::position_to_db`, `db_to_position`, `format_db` and `UNITY = 0.84`, ported from
prototype v5's `TAPER`, `posToDb`, `dbToPos` and `dbTxt` with the table and the ramp unchanged:

| position | 0 | 0.02 | 0.14 | 0.36 | 0.56 | 0.70 | 0.84 | 1 |
|---|---|---|---|---|---|---|---|---|
| dB | −∞ | −70 | −48 | −24 | −12 | −6 | 0 | +6 |

Linear between points; below 0.02 a ramp from −90 dB at 0 to −70 dB; at or below 0.002 silent;
+6 dB at and past 1. `db_to_position` reads −∞, NaN, +∞ (the prototype's `!isFinite`) and
anything at or below −90 dB as 0. `format_db` prints `−∞` (U+2212), `0.0` within 0.05 dB of
unity, else one decimal with a `+` on boosts (`+3.1`, `-8.6`), rounding a tie away from zero
as JavaScript's `toFixed` does. `position_to_gain` is the linear amplitude, 0 when silent.

Two refinements, because positions are `f32` where the prototype's were doubles: every table
point gives its decibels **exactly** (0.84 is exactly 0 dB, so a send at unity takes the
engine's bit-exact ×1 path), and the arithmetic runs in `f64`. One visible consequence: 0.9 is
+2.25 dB as a double but +2.249999 as an `f32`, so it reads `+2.2` here and `+2.3` in the
prototype.

## Persistence

`persist::profile(debug_build)` is `user_config_base("vix-mix")` resolved with the kit's
`resolve_from_env` (a debug build in `profiles/dev`, `CHIMAERA_CONFIG_DIR` and
`CHIMAERA_PROFILE` honoured); pass `cfg!(debug_assertions)` from the binary.
`persist::load(dir) -> Loaded<Setup>` reads `dir/setup.ron` through `load_or_default`: no file
is the default with no problem; an unreadable or unparseable file is the default with the
reason. `persist::save(dir, &Setup)` writes it with `atomic_write`; `dir` must exist. The text
is pretty RON without struct names, so `parse` reads `version` first: the current `VERSION` is
read, a higher one is refused as written by a newer Vix Mix (`PersistError::Newer`), and a lower
one goes through the migration seam (none exists yet). Never save over a file that did not
load: the host moves it aside first.

## The engine snapshot

`snapshot(&Setup) -> EngineSnapshot { channels, monitor, master, preview_master }`: per
channel `ChannelSnap { id, monitor, master, chain_to_monitor, chain_to_master, chain }`, per
mix `MixSnap { id, level, chain }`, each send a `SendSnap { gain, mute }` with the gain linear
(exactly 1.0 at unity, 0.0 silent) and each insert an `InsertSnap { instance, plugin_id,
enabled }`. It is flat, not `Copy` (plugin ids are strings), and the channel order is the
backend's input order.

## The example

`example` holds prototype v5's example data: the `APPS`, `INPUTS`, `OUTPUTS`, `CATEGORIES`,
`PLUGINS`, `FAVORITES`, `RECENT` and `STYLES` tables and `example::setup()`, the six-channel
console (Mic, Game, Music, Browser, Voice chat, Synth) with its levels, mutes, link flags,
MON/MST and inserts. `FakeHost` serves it; nothing in a real run reads it.

## Verification

`cargo test -p vixmix-model`: the taper at every table point exactly, the ramp, the clamps, the
readout strings, and two property tests (a position survives a round trip through decibels;
louder never reads quieter); a ninth channel refused with the limit and nothing changed; an
app ticked in channel B leaves channel A; unknown ids refused; linked faders; inserts moved
within and between strips and copied with their state; plugin references refused by id or
path; Monitor keeps a device; the example is the prototype's console; the snapshot at unity is
exactly 1.0 and the example derives in order; `setup.ron` round-trips byte for byte, a missing
file is the first start, a bad one says why, a newer format is refused, and the directory
follows the profile rule. `kit-check-docs` holds this page's `Owns:` line against the crate.
