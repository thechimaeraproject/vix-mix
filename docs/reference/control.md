# Control (`vixmix-control`)
**Owns:** `crates/vixmix-control/`

**Reference — the control from outside as [handoff 10](../handoffs/10-control-socket.md)
built it and [handoff 11](../handoffs/11-control-devices.md) added the devices to it.**
`vixmix-control` is how another program reads, changes and follows the console: a request is
one line of text, an address names one value of the setup, and what a request may change is
what the window's switches, faders and device lists change, by the same commands. It depends on `vixmix-model` and on no other workspace crate, and holds no
host, no window and no audio.

## What it owns and what the others keep

| This crate | The others |
|---|---|
| the request line and its verbs; the addresses and what each reads; the value's text; the protocol's number | the model: what a command means and when it is refused ([model.md](model.md) § Commands); a refusal is passed on word for word |
| which commands a request can send, and that it sends no other | the host: applying a command, the save, the engine, the backend ([host.md](host.md)) |
| the table of every address with its value, and the changes between two tables | the window: every gesture, the toasts and undo, and everything that adds, removes, moves or renames ([app.md](app.md)) |
| the clients, what each follows, the bounded queues between their threads and the mixer's thread, and that nothing waits on a client | whoever owns the mixer: when the requests are served and when the followers are told |
| on Unix, the socket: where it is, the lock that gives it to one mixer, its threads; and the client at its other end | the app: which profile's socket to open, what to say when it cannot be, and the `ctl` command ([app.md](app.md)) |

## A request

`parse(line) -> Result<Option<Request>, String>` reads one line into a
`Request { verb, address, value }`. A blank line is `Ok(None)`: no request and no error. The
line is `verb address [value]`, the verb in any case, the parts separated by spaces or tabs;
everything after the address is the value.

| Verb | Takes | What it asks |
|---|---|---|
| `get` | an address | every value at or under it |
| `set` | an address and a value | write a switch or a fader |
| `toggle` | an address | flip a switch |
| `nudge` | an address and an amount | move a fader by that much, no further than its ends |
| `watch` | an address | follow it: be told of every change at or under it |
| `unwatch` | an address | stop following it |

A line that begins with an address has no verb written: the address alone is a `get`, and an
address followed by a value is a `set`, which is how OSC writes a message. A line that is not
a request is refused with what one is: an unknown verb lists the six, a verb without an
address or a `set` without a value shows the line that was meant, and a value after a verb
that takes none shows the `set` that writes it.

`plan(console, &Request) -> Result<Plan, String>` says what the request asks of a mixer whose
console that is ([Addresses](#addresses)): `Plan::Read(addresses)` and `Plan::Watch(addresses)` name what to print or
follow, `Plan::Unwatch(addresses)` what to stop following, and `Plan::Write(commands)` the
commands to apply, in order. Every `Err` ends with what to send instead.

## Addresses

`leaves(console) -> Vec<Leaf>` is every address with its value and its `Kind`, in the
console's order: the channels left to right, Monitor, Master, then every plugin strip by
strip, then the devices the machine has now, inputs before outputs. It is the one list the
protocol is read from: what `get` prints, what a write may name, and what a follower is told.

**A `Console`** is what a request is asked against: `Console { setup, devices }`, the setup
and the devices the machine has now, as the host lists them (`DeviceInfo`: a `DeviceRef` and
its direction). The devices are the one part that is not the setup's: a setup names the
device a mix or a hardware channel uses whether or not it is plugged in. `leaves`, `plan`,
`resolve` and `Service::publish` take anything that becomes a console, and a `&Setup` alone
is one with no device present.

### As the mixer prints them

A channel is named by its `ChannelId` and a plugin by the `InstanceId` of its insert. Neither
changes while the thing exists and neither is ever handed out again ([model.md](model.md)
§ Ids), so an address a controller keeps stays the same thing.

| Address | Value | A write sends |
|---|---|---|
| `/ch/<id>/name` | text | read only |
| `/ch/<id>/kind` | `"hardware"` or `"apps"` | read only |
| `/ch/<id>/link` | switch | `SetLink` |
| `/ch/<id>/mute` | switch: true when both sends are muted | `SetMute` for Monitor, then for Master |
| `/ch/<id>/monitor/mute`, `/ch/<id>/master/mute` | switch | `SetMute` |
| `/ch/<id>/monitor/db`, `/ch/<id>/master/db` | number: the fader in decibels | `SetSend` |
| `/ch/<id>/monitor/position`, `/ch/<id>/master/position` | number, 0 to 1: the fader's travel | `SetSend` |
| `/ch/<id>/monitor/processed`, `/ch/<id>/master/processed` | switch: MON, MST | `SetChainRouting` |
| `/mix/monitor/mute`, `/mix/master/mute` | switch | `SetMixMute` |
| `/mix/monitor/db`, `/mix/master/db` | number | `SetMixLevel` |
| `/mix/monitor/position`, `/mix/master/position` | number | `SetMixLevel` |
| `/mix/master/preview` | switch: the ear | `SetPreview` |
| `/fx/<instance>/name` | text | read only |
| `/fx/<instance>/strip` | text: the strip's address, `"/ch/3"` or `"/mix/master"` | read only |
| `/fx/<instance>/slot` | number: its place in the chain, from 1 | read only |
| `/fx/<instance>/enabled` | switch: the bypass square | `SetInsertEnabled` |
| `/mix/monitor/device/name`, `/mix/master/device/name` | text: the PipeWire node name of the output the mix plays to; `""` for none | `SetMixDevice` |
| `/ch/<id>/device/name`, on a hardware channel only | text: the node name of the input it captures; `""` for none | `SetSource` with `Source::Hardware` |
| `…/device/description` beside either | text: that device's description; `""` for none | read only |
| `…/device/present` beside either | switch: whether that device is there now | read only |
| `/in/<word>/name`, `/in/<word>/description` | text: an input the machine has now | read only |
| `/out/<word>/name`, `/out/<word>/description` | text: an output the machine has now | read only |

A plugin is not under its channel: an insert dragged to another strip keeps its instance, so
it keeps its address and its `strip` and `slot` change.

**Devices.** A mix's `device` is the output it plays to and a hardware channel's the input
it captures; an app channel has none, and no `device` under it. The setup names a device by
its node name, which PipeWire keeps across restarts and replugging, so `name` and
`description` read the same while the device is unplugged and `present` says whether it is
there now. `/in` and `/out` list the devices the machine has now, each under `<word>`, its
node name as one plain word (`Console::listed`: the model's `slug`, with a number after it
when two names make the same word): `alsa_output.usb-Scarlett_2i2-00.analog-stereo` is
`/out/alsa-output-usb-scarlett-2i2-00-analog-stereo`. A device that is unplugged is no
longer listed.

`address::covers(place, address)` is whether an address is a place or lies under it, compared
part by part (`/ch/1` does not cover `/ch/10`); `/` covers everything. `address::node` is the
channel, mix, plugin or listed device an address belongs to, its first two parts.

### As a person may write them

`resolve(console, written) -> Result<Vec<String>, String>` turns what a request wrote into
the addresses above:

- **A channel by its name**: `/ch/mic/monitor/mute`. The name is compared as the model's
  `slug` makes it (lower case, every run of other characters one `-`), which is the rule a
  channel's PipeWire node name ends by, so `voice-chat`, `Voice_Chat` and `VOICE-CHAT` all
  name *Voice chat*.
- **A plugin under its strip**, after `fx`, by its slot counted from 1 or by its name:
  `/ch/mic/fx/2/enabled`, `/mix/master/fx/pro-l-2/enabled`. Each becomes the plugin's
  `/fx/<instance>/…`.
- **`…/fx` alone** is every plugin of that strip, in order: one address each, and none for a
  strip with no plugins.
- **A part that is all digits is never a name**: after `/ch` it is an id, after a strip's
  `fx` a slot, after `/fx` an instance. A channel named `2` is written by its id.
- **A listed device by its description**, or by the node name its word was made from:
  `/out/built-in-speakers`, `/out/alsa_output.built_in_speakers`.
- The words after the channel, mix, plugin or device are taken in any case; a trailing slash
  is ignored; `/` is everything, and `/ch`, `/mix`, `/fx`, `/in` and `/out` are every
  channel, mix, plugin, input and output.

What names nothing is refused with the names there are: an unknown channel lists every
channel with its address, an unknown plugin or slot lists the strip's plugins, an unknown
device lists the devices of that direction as they are listed, with their descriptions, and
an address outside `/ch`, `/mix`, `/fx`, `/in` and `/out` names the five. A name that two
channels, or two plugins of one strip, share is refused naming each by its id or its slot,
and a description two devices share naming each as it is listed.

## Values

`Value` is `Switch(bool)`, `Number(f64)` or `Text(String)`, and its `Display` is what a line
carries:

- **A switch** prints `true` or `false`. A request may also write `on`, `off`, `1` and `0`,
  in any case.
- **A number** prints as short as it reads back (`0.84`, `-12`, `1`). A position is printed
  to four decimals and decibels to two, so the same fader always reads the same. A silent
  fader's decibels are `-inf` (`SILENT`); a request may write `-inf` or `-infinity`, and
  never `inf` or `nan`.
- **Text** prints in double quotes as JSON writes it, so a quote, a backslash or a line end
  in a channel's name stays inside its one line, and any JSON reader reads it back. A
  request writes text the same way or bare, as it stands to the end of the line
  (`Built-in speakers`); text that opens a quote and does not close it is refused.

## What a write sends

`set`, `toggle` and `nudge` name exactly one value. A place that holds several (`/ch/1`,
`/ch/1/monitor`, a strip's `fx`, a strip's `device`) is refused with what is under it and one
address to write.

- **`set`** of a switch sends its command with the value. Of a `position` it sends the
  number as the fader's position; one outside 0 to 1 is the model's refusal
  (`Refused::NotAPosition`). Of a `db` it sends the position the taper gives
  (`taper::db_to_position`): 0 dB is exactly `taper::UNITY`, `-inf` and anything at or below
  the taper's floor is 0, and more than `TOP_DB` (+6, the taper's top) is refused here.
- **`toggle`** sends the switch set to what it is not. For `/ch/<id>/mute` that is: unmute
  both sends when both are muted, otherwise mute both.
- **`nudge`** reads where the fader stands in the setup, not in its printed value. A
  `position` moves by the amount and stops at 0 and at 1. A `db` moves by that many decibels,
  stops at `TOP_DB`, and falls silent below the taper's floor; from silence it is refused,
  because silence is no number of decibels to move from, with the two ways on (`set` a
  level, or nudge the `position`).
- **A linked channel**: `SetSend` moves the other send by the same amount, as the fader in
  the window does ([model.md](model.md) § Commands). Nothing here adds to that.
- **A device**: `set` of a strip's `device/name` sends `SetMixDevice` for a mix and
  `SetSource` with `Source::Hardware` for a hardware channel, carrying the `DeviceRef` the
  host lists, description and all. The device is written as its node name, as the word it
  is listed under, or as its description, the last two compared as `slug` makes them
  (`Built-in speakers`, `built-in-speakers` and `BUILT_IN_SPEAKERS` are one). It must be one
  the machine has now, an output for a mix and an input for a channel, or the answer lists
  the ones there are; the one exception is the device the setup already names, which may be
  named again while it is unplugged and changes nothing. `none` (`NO_DEVICE`) or `""` is no
  device: Master may have none and so may a hardware channel, and for Monitor it is the
  model's refusal (`Refused::MonitorNeedsDevice`). A description two devices share is
  refused naming both by their node names. An app channel is refused, saying that it has
  apps and no input: a request never makes an app channel a hardware one.
- **Read only**: a name, a kind, a plugin's name, strip and slot, a device's description and
  whether it is there, and the devices the machine has are refused, saying where each is
  changed.

**A request sends ten kinds of command and no other**: `SetSend`, `SetMute`, `SetLink`,
`SetChainRouting`, `SetInsertEnabled`, `SetMixLevel`, `SetMixMute`, `SetPreview`,
`SetMixDevice`, and `SetSource` for a hardware channel's input alone, never for an app
channel and never with apps. The commands that add, remove, move or rename, and `SetPrefs`
and `SetInsertState`, stay the window's and the host's.

## Changes

`diff(before, after) -> Vec<Change>` is what turned one table into another: first
`Change::Gone(node)` for each channel and plugin that is no longer there (`/ch/3`, `/fx/4`),
each once, then `Change::Set { address, value }` for every address that is new or reads
differently, in the console's order. A `Change` prints as `address value`, or `address gone`
(`GONE`). A value printed the same reads as unchanged: a fader moved by less than its
printed decimals says nothing.

## The lines' fixed words

`PROTOCOL` is the protocol's number, and `greeting()` the line that carries it,
`vix-mix control ` and the number. It moves when an address is renamed or a line changes its
meaning; a new address or a new verb keeps it. `OK` (`ok`) ends an answer that was carried
out, `ERROR` (`error: `) begins the line that ends one that was not, and `EVENT` (`event `)
begins a line that tells a follower of a change.

## The service

`service(wake) -> (Service, Connector)` makes the two sides of the conversation.

**The `Connector`** brings clients in; it is `Send`, `Sync` and cheap to clone, for the
threads of a transport or for a test. `open()` gives a `Client { requests, answers }`, or
`Err(Full)` when `MAX_CLIENTS` (8) are connected, in which case the service never hears of
it and `Full::line()` is the one line to send before closing. `Requests::send(line)` queues
one request line and blocks while `INBOX` (256) requests wait, which slows only that client's
own thread; it answers `false` once the service is gone. A line over `MAX_LINE` (4096) bytes
is not queued: the client is told and dropped. Dropping `Requests` tells the service the
client went. `answers` is a `Receiver<String>` of messages, each whole lines ending in a
line end; it ends when the service dropped the client or is gone, which is when to close the
connection.

**The `Service`** belongs to the thread that owns the mixer and is driven by two calls:

- **`serve(budget, &mut dyn Target) -> Served`** carries out what waits, at most `budget`
  of it (`SERVE_BUDGET`, 64, is the usual one). A client that came is greeted with
  `greeting()`. Each request line is answered with one message: its `address value` lines
  and then exactly one `ok` or `error: …`; a blank line is not answered. `Target` is the
  mixer as a request needs it, `setup()`, `devices()` (the devices it has now; none unless a
  target says otherwise) and `apply(Command)`; the commands of a write are
  applied in order and stop at the first the model refuses, whose words are the error. A
  write answers with what it changed as it reads now, so a linked fader's other send comes
  with the one that was set, and a write to what already is answers `ok` alone.
  `Served { requests, changed }` says how many lines were answered and whether any command
  changed the setup.
- **`publish(console)`** tells the followers what changed since they were last told: for each
  client, the changes at or under what it follows, as `event address value` lines in one
  message. Something that went is told as `event address gone` to whoever follows it, a
  place above it, or an address under it. With no follower it does nothing
  (`followed()`). The first follower starts from the setup as it stands when it asks; a
  later one does not disturb what the earlier ones are still owed. **A follower is told
  where things stand, not every step on the way**: a value that changed and changed back
  between two calls is not told, and one that moved three times is told once, as it reads
  at the call. The window calls it when its host's poll reports that the setup or the list
  of devices changed, at most 60 times a second; a device unplugged is `gone` from its list,
  and `present` turns false on the strip that uses it.

A client that asks is always answered before it is told: its answer is sent in `serve`, and
the event for its own change only at the next `publish`.

`watch` answers with the values there now and then `ok`. A client follows at most
`MAX_WATCHES` (32) addresses, and one more is refused with the way out: follow a place above
them. An address followed twice is followed once. `unwatch` always answers `ok`.

**Nothing waits on a client.** Each has an outbox of `OUTBOX` (64) messages, an answer or
one change's events being one message whatever its length. When a client's outbox is full
the service drops that client, logs why, and serves the others. **One wake however many
requests**: the connector calls `wake` only when it queues something while no wake was
owed, and a `serve` that stops at its budget asks for one more.

## The socket

Unix only (`cfg(unix)`); on any other platform the crate has the protocol and the service
and no transport.

**Where.** `socket_path(explicit, runtime_dir, profile)` is the path of the mixer that runs
under a configuration profile ([model.md](model.md) § Persistence):

| | Path |
|---|---|
| `VIXMIX_CONTROL_SOCKET` (`SOCKET_ENV`) is set | that path, as it is |
| the profile `default`: the installed mixer | `$XDG_RUNTIME_DIR/vix-mix/control.sock` |
| any other profile (`dev` for a debug build) | `$XDG_RUNTIME_DIR/vix-mix/control-<profile>.sock`, the name made one word by `slug` |

A blank variable is an unset one. With neither set it is refused, saying to start Vix Mix
from a desktop session or to set the variable. `socket_path_from_env(profile)` reads this
process's environment; the mixer and its `ctl` command both call it, so they agree.

**Who.** `Listener::open(path, connector)` listens and brings every connection to the
connector's service. One mixer holds a socket: `open` takes a lock (`File::try_lock`) on
the file beside it with the extension `lock`, and keeps it while it listens; the kernel lets
it go when the process ends, however it ends. A second mixer is refused with
`ListenError::Held`, which says that this one runs without the socket and to close one of
the two, and nothing of the first is touched. Only the holder of the lock removes what is at
the path, so a socket file a dead mixer left is replaced. A path too long to be a socket's
is `ListenError::TooLong`, found before anything is made; anything else that fails is
`ListenError::Io` with what was being done.

**Whose.** The directory is made, with every directory above it that is missing, as the
user's alone (0700); one that is already there is left as it is. The socket is made 0600.
Any program of the same user may connect and nobody else's; nothing listens on the network.

**Threads.** One accepts (`vix-mix-control`). Each connection has one that reads its lines
into the service (`vix-mix-control-in`) and one that writes what the service sends
(`vix-mix-control-out`); none of them touches the mixer. A line is read without its line end
(`\n` or `\r\n`), bytes that are not UTF-8 replaced; one longer than `MAX_LINE` is cut just
past the limit and never read whole, which is enough for the service to refuse it. A ninth
connection is sent `Full::line()` and closed.

**How a connection ends.** When the client closes its sending side, the service answers
everything it was sent and lets the client go, the writer writes what was queued and closes
the connection: a client that sends its requests and then says it is done reads every answer
and then the end. When the service drops a client, the same. When a write has not gone
through in `WRITE_TIMEOUT` (2 s), the connection is closed. When the `Listener` is dropped,
it stops accepting, closes every connection and removes the socket file; the lock goes with
it.

## The client

`Session::connect(path, patience)` connects and reads the greeting; `patience` is how long
to wait for each line, `None` for as long as it takes. It fails as `ClientError::NoMixer`
when nothing answers at the path (start Vix Mix), `Refused` with the mixer's own words when
the connection was answered with a refusal (the ninth), `NotAMixer` when what answers does
not greet as a mixer, `Silent` when a line did not come within the patience (a plugin scan
holds the mixer's window), and `Closed` when the mixer closed before it answered.
`protocol()` is the number the mixer said, to compare with `PROTOCOL`.

`ask(line) -> Answer { lines, events, error }` sends one request and reads to the line that
ends its answer: `lines` are the answer's, `error` the message of an `error:` line, and
`events` any event that arrived on the way, without its prefix, so a follower's news is not
lost to a request on the same connection. `send(line)` and `line()` are the two halves for
a caller that reads as it goes (`line()` is `None` at the end of the connection);
`finish()` closes the sending side, after which reading gives every answer and then the
end. `sender()` is the sending side for a second thread.

## Verification

`cargo test -p vixmix-control`, on every platform:

- **`tests/requests.rs`**, from the outside.
  `each_of_the_ten_commands_has_a_line_that_sends_it_and_no_other`: a `match` over
  `Command` with no wildcard gives each variant its line or names it the window's, so a new
  variant does not compile until it is decided, and each of the ten lines sends its own
  kind of command.
  `every_value_a_write_may_name_reads_back_what_was_written`: for every address of the
  example console with its eight devices, a `set` then the table reads the value written (a
  switch flipped, a position at 0.5, decibels at −12, another device than the one in use,
  written as it is described), and every read-only address is refused as such; a `match`
  over `Kind` with no wildcard says which is which.
  `a_request_sends_only_the_ten_commands_and_never_changes_the_consoles_shape`: a
  property over 256 runs of up to 23 lines, well formed or not, written by id, by name, by
  slot and by device: every command sent is one of the ten, a `SetSource` only ever carries
  a hardware input, and the changes each run of commands made are never a `Gone` and never
  to what the console is made of (a name, a kind, a plugin's name, strip or slot, a listed
  device), so no app channel becomes a hardware one.
- **`console`**: a device listed under its node name as one word, each direction apart, two
  names that make one word numbered; a mix and a hardware channel have a device and an app
  channel none.
- **`tests/socket.rs`**, on Unix, over a real socket in a short path under the system's
  temporary directory (a socket's path holds about a hundred bytes, which Cargo's own
  temporary directory exceeds), with the model served on a thread as the app serves its
  host; it needs no display and no PipeWire, and removes what it made.
  `a_request_over_the_socket_is_answered_and_a_follower_is_told_what_another_changed`: the
  greeting's number, a read, a follower told of another connection's toggle, a refusal as
  the answer's error, and an event that came before an answer kept in `events`.
  `a_second_mixer_leaves_a_running_mixers_socket_alone_and_a_dead_mixers_file_is_replaced`:
  `Held` with its words while the first still answers, the file gone with the first
  listener, and a socket file with no lock held replaced.
  `the_directory_it_makes_and_the_socket_are_the_users_alone`: 0700 and 0600.
  `a_path_too_long_for_a_socket_is_refused_before_anything_is_made`.
  `a_ninth_connection_is_told_why_and_one_that_leaves_makes_room`.
  `nothing_at_the_path_is_no_mixer_and_says_to_start_it`.
  `a_client_that_says_it_is_done_is_answered_first_and_then_closed`: three requests and the
  end of the sending side before anything is read give three answers and then the end.
  `a_line_past_the_limit_is_refused_and_its_connection_closed`, the mixer serving on.
  `a_listener_that_goes_closes_its_connections`.
  A write that times out on a client that reads nothing is not tested over the socket; the
  drop of a client that does not read is, in `service`.
- **`socket`**: the path under the runtime directory for the default profile, a debug build
  and a profile of two words, the variable winning, a blank variable unset, and no runtime
  directory refused with both ways on; a line read without either line end, a stream that
  ends mid-line, and a megabyte without a line end cut just past the limit.
- **`service`**, against the model alone as the target: a client is greeted and each of
  eleven requests, good and bad, ends in exactly one `ok` or `error:` with no event inside;
  a read prints the values and a write what it changed, a linked fader's two sends among
  them, and a write to what already is says `ok` alone; a request applies its commands and
  says whether the setup changed; a refused request changes nothing and ends with what to
  send instead, the model's refusal word for word; a follower hears a change another client
  made and one made past every client, only under what it follows, nothing when nothing
  changed and nothing once it unfollowed; a follower that makes a change is answered first
  and told after; a follower is told where things stand, not every step (a mute toggled
  twice between two publishes says nothing, a fader moved three times is told once); a
  second follower does not take a change owed to the first; a channel
  removed is `gone` to who follows it, a place above it or a value under it, and its values
  come back when it is restored; a client that does not read is dropped at its sixty-fifth
  unread message and the others are served; a ninth connection is refused until one closes;
  a flood of requests wakes once and a drain stops at its budget and asks for one more
  wake; a line past the limit is refused and its connection closed, one of exactly the
  limit is a request; the thirty-third address followed is refused with the way out; a
  client that went is forgotten, and a service that went stops its clients.
- **Devices**, in `request`, `address`, `tree` and `service`: a device set by its node name,
  as it is listed, and as it is described, bare with its space and in double quotes, and
  `none` in three spellings; Monitor without a device is the model's refusal; every device
  that cannot be set says which there are (not there, the other direction, an app channel, a
  place that is not a value, a description, `present`, a listed device, an unclosed quote),
  and a description two devices share names both; the device a setup names can be named
  again while it is unplugged; a listed device written as it is listed, by its node name
  and by its description, an output not an input, a setup alone listing none; a strip reads
  its device and whether it is there, an app channel none, no device as empty text; the
  machine's devices listed last, inputs first; a device unplugged is `gone` and the strip
  that uses it not `present`, and plugged back it is listed again; through the service, a
  device set against the target's devices answers with what changed, a follower of the list
  and of the strip is told when it is unplugged, and it can then no longer be picked.
- **`request`**: a line read into its verb, address and value, an address alone and an
  address with a value read as OSC writes them, a blank line no request; every line that is
  no request says what one is; each switch and fader sends the command its control sends; a
  channel's mute is both sends, and its toggle mutes both unless both are muted; 0 dB is
  unity bit for bit, −12 dB is the taper's 0.56, the top and silence, `TOP_DB` held to the
  taper; a nudge by decibels and by position, stopped at both ends, refused from silence;
  every write that cannot be made says what to send instead; a read names what to print, an
  empty place is no error, and what went can still be unfollowed.
- **`address`**: covering part by part; a channel by id and by name; a plugin by instance,
  slot and name, on a channel and on a mix; `fx` alone; a number never a name; every
  address that names nothing refused with the names there are, and a name two channels or
  two plugins share refused naming each.
- **`tree`**: a channel's twelve values in order with their decibels worked out by hand,
  the mixes' seven, a plugin's four; both sends muted is the channel muted and a silent
  fader reads `-inf`; the changes between two setups turn the first table into the second
  through ten commands that mute, move a linked fader, move a plugin between strips, remove
  and restore; what went is said once as gone, before anything else.
- **`value`**: each value written as one word that reads back; a name with a quote and a
  line end stays on one line; the switch's six spellings; a number finite or silence and
  nothing else; rounding keeps a reading steady and zero unsigned.

`kit-check-docs` holds this page's `Owns:` line against the crate.
