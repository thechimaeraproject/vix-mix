# Model (`vixmix-model`)
**Owns:** `crates/vixmix-model/`

**Reference — the contract as [handoff 01](../handoffs/01-model.md) built it, with the undo
commands of change request 2 that [handoff 06](../handoffs/06-host-and-integration.md) added
and the bound on the plugin folders of change request 10 (round 1).**
`vixmix-model` is the whole setup as pure data: up to eight channels with their sources,
chains and sends; the Monitor and Master mixes; the commands that change it, validated and
applied whole or not at all, each saying exactly what it changed; the fader taper; RON
persistence of `setup.ron` over the kit's `chimaeraproject-fs`, with a format version, a
migration from every older format and the rules checked at load; and the flat
`EngineSnapshot` the engine prepares from. It holds **no thread, device handle, plugin
instance or window type**, and depends on no other workspace crate: everything else reads it,
and it reads nothing of theirs.

## What it owns and what the others keep

| The model | The others |
|---|---|
| the setup's types and their invariants (≤ 8 channels, ids unique and never handed out twice, an app in one channel only, Monitor keeps a device once it has one, a plugin stored by id and bundle name) | the engine: what a snapshot sounds like; the backend: which nodes exist; the plugins crate: the live instances an `InstanceId` names |
| `apply`: validation, the change, and `Applied` (exactly what changed) | the host: acting on it — prepare, apply the graph, route, create or retire instances, save |
| the taper: position ↔ dB, the readout text | the window: drawing a fader and its meter at a position |
| `setup.ron`: where, the format, the version, the migrations, the rules a file must keep | the host: when to save (debounced), creating the directory, and moving aside a file that did not load |

## The setup

`Setup { version, next_channel, next_instance, channels, monitor, master, preview_master,
prefs }`. A `Channel` is `{ id, name, source, chain, chain_to_monitor, chain_to_master,
monitor, master, link }`; `Source` is `Hardware(Option<DeviceRef>)` or `Apps(Vec<AppRef>)`;
`Send { position, mute }` is a fader (a position in 0..=1, the taper's domain) and its mute,
used for each channel's two sends and for each mix's level; `Mix { id, device, level, chain }`;
`Insert { instance, plugin, enabled, state }` with `state` the plugin's `PluginState::encode`
bytes, captured before a save. `PluginRef { id, bundle, name }` stores `vst3:<32 hex>` (either
case) and the bundle's file name, never a path. `AppRef { app_id, binary, name }` stores
PipeWire's `pipewire.access.portal.app_id`, `application.process.binary` and
`application.name`, and `AppRef::matches` compares app id when both have one, else binary,
else name. `DeviceRef { name, description }` is `node.name` and `node.description`;
`DeviceInfo` is a present device with its direction, here because both the Linux-only backend
and the host name it. `Prefs { quantum, plugin_dirs }`: 128, 256 or 512 frames asked through
`node.latency` (`Prefs::QUANTA`), and extra VST3 directories, at most 16
(`Prefs::MAX_PLUGIN_DIRS`, which the window's preferences read). `Setup::default()` is the
first start: one hardware channel named Mic with no device yet, everything at unity, no
plugins.

`ChannelId(u32)` and `InstanceId(u32)` are `Copy` with public fields; `MixId` is `Monitor` or
`Master`. **`Send` shadows the standard `Send` marker trait** where it is imported by name:
write `vixmix_model::Send` (or import it under another name) in any module with thread bounds.

### Ids

A new channel takes `next_channel` and a new insert's instance (added, or copied) takes
`next_instance`; each counter then moves on by one and never moves back. So **a removed id is
never handed out again**, in this run or the next: the host keeps a removed channel or insert,
and its live instance, while the undo toast shows, and a new one must not take its id (the
engine's `Lender` would lend the old processor under the new plugin's name). `u32::MAX` is
never handed out; a counter that reached it refuses with `OutOfIds`. A file whose counters sit
at or below an id it uses (a hand edit, or format 1, which had none) has them raised on load.

## Commands

`apply(&mut Setup, Command) -> Result<Applied, Refused>` applies a command whole or refuses
it and leaves the setup exactly as it was (not even an id is spent). The variants:
`AddChannel { kind }`, `RemoveChannel(id)`, `Rename { ch, name }`, `SetSource { ch, source }`,
`SetSend { ch, mix, position }`, `SetMute { ch, mix, mute }`, `SetLink { ch, link }`,
`SetChainRouting { ch, mix, on }`, `AddInsert { strip, plugin, at }`,
`RemoveInsert { instance }`, `MoveInsert { from, to, copy }` (two `SlotRef { strip, index }`),
`SetInsertEnabled`, `SetInsertState`, `SetMixDevice { mix, device }`,
`SetMixLevel { mix, position }`, `SetMixMute { mix, mute }`, `SetPreview(bool)`,
`SetPrefs(Prefs)`, and the two undos, `RestoreChannel { at, channel }` and
`RestoreInsert { to, insert }` (change request 2). `Strip` is `Channel(ChannelId)` or
`Mix(MixId)`.

What each does, as the prototype does it:

- **Limits and ids.** A ninth channel is refused (`TooManyChannels`: "vix-mix mixes at most 8
  channels … remove a channel before adding another"); an unknown channel, instance or slot is
  refused with what to read again. A new channel is named Input (hardware) or Apps, at the
  right end, sends at unity, linked, MON and MST on.
- **Names.** `Rename` trims; a name that is empty once trimmed is refused (`EmptyName`).
- **An app in one channel only.** `SetSource` with apps removes each ticked app from every
  other channel (by `AppRef::matches`) and lists each app once (the prototype's checklist).
- **Faders.** A position outside 0..=1, or NaN, is refused (`NotAPosition`); −0 is stored as 0.
  `SetSend` on a linked channel moves the other send by the same position delta, clamped to
  0..=1 (the prototype's `setFader`).
- **Moves.** `MoveInsert` within a strip is a no-op when dropped on its own place or the gap
  below it, and otherwise removes, then inserts at the index adjusted for the removal; across
  strips the same instance moves (nothing is created or retired); with `copy` a new instance
  is made with the same plugin, enabled flag and state, and the original stays.
- **Undo.** `RestoreChannel` puts a removed `Channel` back at index `at` with its id, its
  chain and its instance ids, its name trimmed; `RestoreInsert` puts a removed `Insert` back
  before `to` with its instance id, bypass and state. Only what this setup removed can come
  back: an id in use, or at or past its counter (never handed out), is refused (`NotRemoved`:
  "only a removed channel can be put back; add a new one instead"), as are a ninth channel,
  a place past the last channel (`NoPlace`), a slot past the chain's end, a bad fader, name or
  plugin. A restored channel's app that another channel took since stays there. No id is
  spent. The host keeps the removed instances live while the undo toast shows, so `created`
  names instances it still holds ([reference/host.md](host.md)).
- **Devices, plugins, preferences.** `SetMixDevice { Monitor, None }` is refused
  (`MonitorNeedsDevice`); an `AddInsert` whose id is not `vst3:` and 32 hex digits
  (`NotAPluginId`), or whose bundle is a path (`BundleIsAPath`), is refused; a quantum other
  than 128, 256 or 512 is refused (`NotAQuantum`), and so is a `SetPrefs` with more than 16
  extra plugin folders (`TooManyPluginDirs`: "16 plugin folders is the most there can be, not
  17: remove one first", the window's words; change request 10; a list further past the
  bound is told to remove folders until 16 are left).

Every `Refused` message ends with what to do instead.

### What `Applied` says

`Applied { save, engine, graph, routing, created, removed, channel }`, **each part set exactly
when its part of the setup changed**:

| Part | Set when | The host then |
|---|---|---|
| `save` | the setup differs at all (`changed()` reads it) | saves, debounced |
| `engine` | `snapshot()` differs: a gain (not merely a position: a fader moved within silence changes no gain), a mute, MON/MST, a chain's instances, plugins or bypass, the ear button, a channel added or removed | derives the snapshot and prepares |
| `graph` | the channels in order with their names and inputs (an app sink, or a capture on its device), a mix's device, or the quantum | applies the backend's graph |
| `routing` | which app goes to which channel (the set of channel and app pairs) | routes the apps again |
| `created` | instances whose inserts entered the setup, in chain order | creates them (with the state, for a copy) |
| `removed` | instances whose inserts left the setup, in chain order | retires them, or keeps them while an undo toast shows |
| `channel` | the channel `AddChannel` made or `RestoreChannel` put back | |

A command that changes nothing — a drop on its own place, a mute set to what it was, the same
name, the same state — answers `Applied::default()`. By command:

| Command | `save` | `engine` | `graph` | `routing` | Lists |
|---|---|---|---|---|---|
| `AddChannel` | yes | yes | yes | no | `channel` |
| `RemoveChannel` | yes | yes | yes | when it had apps | `removed`: its chain |
| `Rename` | when the trimmed name differs | no | when it differs | no | |
| `SetSource` | when a list or the source differs | no | when the input differs: the kind, or the device | when an app joined or left a channel | |
| `SetSend`, `SetMixLevel` | when a position moved | when a gain moved | no | no | |
| `SetMute`, `SetMixMute`, `SetChainRouting`, `SetInsertEnabled`, `SetPreview` | when it differs | when it differs | no | no | |
| `SetLink`, `SetInsertState` | when it differs | no | no | no | |
| `AddInsert` | yes | yes | no | no | `created` |
| `RemoveInsert` | yes | yes | no | no | `removed` |
| `MoveInsert` | unless a move (not a copy) is dropped in place | the same | no | no | `created` for a copy |
| `SetMixDevice` | when it differs | no | when it differs | no | |
| `SetPrefs` | when it differs | no | when the quantum differs | no | |
| `RestoreChannel` | yes | yes | yes | when it keeps apps | `created`: its chain; `channel` |
| `RestoreInsert` | yes | yes | no | no | `created` |

A rename changes only `graph`, but the backend names a channel's sink after it, so applying
the graph may move routed apps to the renamed node: that is the host's to do (handoff 06).

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
`persist::save(dir, &Setup)` writes `dir/setup.ron` with `atomic_write`; `dir` must exist.
`persist::load(dir) -> Loaded<Setup>` reads it through `load_or_default` and `persist::parse`:
no file is the default with no problem; a file that cannot be read or used is the default with
the reason (`FsError::Parse` around a `PersistError`, ending "fix it by hand, or delete it to
start from the default"). Never save over a file that did not load: the host moves it aside
first.

**The text** (`persist::to_ron`) is pretty RON without struct names, `version` the first field,
LF line ends on every platform (RON's default is CRLF on Windows, which would make the same
setup two different files), a plugin's state as one standard padded base64 string
(`state: Some("AAECf4D+/w==")`, as Vix stores one, rather than a number per line), and the
plugin directories as text, lossily (serde refuses a path that is not UTF-8, which would leave
nothing to write). Serializing cannot fail.

**Reading** (`persist::parse`) is three steps:

1. The bytes must be UTF-8 (`NotText`), and `version` is read alone (`Ron` when it is not RON
   or has none).
2. The current `VERSION` (format 2) is read with the current types. A higher one is refused as
   written by a newer vix-mix (`Newer`), never guessed at. A lower one goes through **the
   migration seam**: each older format is read with its own types and converted forward one
   step at a time (`migrate.rs`). Format 1, the bootstrap's, had no id counters, kept
   `version` last and wrote the state as numbers; it converts to format 2 whole. A version no
   vix-mix wrote (0) is `Unknown`. The next format change bumps `VERSION`, copies the changed
   types' old shapes into `migrate.rs` and adds the step, with a fixture of the old file.
3. The id counters are raised above every id in use, then the setup must keep every rule
   `apply` keeps (`persist::Invalid`, inside `PersistError::Invalid`): at most 8 channels,
   unique channel ids and instance ids, every channel named, an app in one channel only,
   every fader in 0..=1, every plugin stored by id and bundle name, `monitor` and `master`
   in their places, a quantum the preferences offer, at most 16 plugin folders. Each message
   names the rule and the edit that fixes the file.

## The engine snapshot

`snapshot(&Setup) -> EngineSnapshot { channels, monitor, master, preview_master }`: per
channel `ChannelSnap { id, monitor, master, chain_to_monitor, chain_to_master, chain }`, per
mix `MixSnap { id, level, chain }`, each send a `SendSnap { gain, mute }` with the gain linear
(exactly 1.0 at unity, 0.0 silent) and each insert an `InsertSnap { instance, plugin_id,
enabled }`. It is flat, not `Copy` (plugin ids are strings), and the channel order is the
console's, which is the backend's input order.

## The example

`example` holds prototype v5's example data: the `APPS`, `INPUTS`, `OUTPUTS`, `CATEGORIES`,
`PLUGINS`, `FAVORITES`, `RECENT` and `STYLES` tables and `example::setup()`, the six-channel
console (Mic, Game, Music, Browser, Voice chat, Synth) with its levels, mutes, link flags,
MON/MST and inserts, `next_channel` 7 (the prototype's `uid`) and `next_instance` one past its
nine inserts. `FakeHost` serves it; nothing in a real run reads it.

## Verification

`cargo test -p vixmix-model`:

- **`tests/applied.rs`** — `every_applied_matches_the_diff_it_made`: a property over 512
  sequences of up to 47 commands each, from the first start or the example, drawn against the
  setup reached so far (unknown ids, bad slots, bad positions, bad plugins, a seventeenth
  plugin folder and apps that match each other in each of the three ways included). A refused
  command changes nothing and says a fix; an applied one answers exactly the `Applied` the diff
  calls for, part by part, with the parts written in the test from the outside (the whole
  setup, the snapshot, the graph's inputs, the app and channel pairs, the instances, the new
  channel); and the setup reached round-trips through `to_ron` and `parse`, so every setup
  commands make keeps the load rules.
  The undo commands draw from what the sequence removed so far (and from what it did not, to
  be refused).
  `the_sequences_change_the_setup_through_every_command` holds the generator to landing every
  kind of command, and an exhaustive `match` there fails to compile when a `Command` variant is
  added without a seed.
- **`command`** — a ninth channel refused with the limit and nothing changed; an app ticked in
  channel B leaves channel A, listed once, and reordering routes nothing; unknown ids and slots
  refused without spending an id; a removed id never comes back; a spent id space refused; a
  command that changes nothing says so, for every kind that can; a fader moved within silence
  saved but not prepared; linked faders clamped at both ends; inserts moved within and between
  strips and copied with their state and bypass; a removed channel's instances retired and its
  apps routed; plugins refused by id or path; Monitor keeps a device; the quantum; sixteen
  plugin folders taken and a seventeenth refused with the limit and the fix, nothing changed; a
  removed channel back in its place with its ids, bypass and state while an app another
  channel took stays there, a removed insert back in its gap, and a restore of anything not
  removed (in use, never handed out, past the end, past eight channels, nameless) refused.
- **`taper`** — every table point exact both ways, silence at or below 0.002, the ramp, the
  prototype's values between points, the clamps, the readout strings, and two properties (a
  position survives a round trip through decibels; louder never reads quieter).
- **`persist`** — a byte-for-byte round trip through the file; version first, LF, the state as
  base64; `tests/fixtures/setup-v1.ron` (the bootstrap's own output) converted forward with its
  counters and state; a missing file is the first start, a bad one and one that is not text say
  why; a newer format refused and format 0 unknown; a file that breaks a rule refused with its
  fix, one with seventeen plugin folders among them; counters raised, never lowered; the
  profile rule.
- **`check`**, **`base64`**, **`snapshot`**, **`example`**, **`setup`** — each rule a hand edit
  can break, named with its fix; RFC 4648's vectors, refusals and a round-trip property; unity
  exactly 1.0, the example in order with every part the engine runs; the example is the
  prototype's console; app matching.

`kit-check-docs` holds this page's `Owns:` line against the crate.
