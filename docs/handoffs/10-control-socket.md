# 10 — Control socket

**Record — not yet built.** The intention, taken with the user on 2026-11-16: other programs
read, change and follow the console through a Unix socket that speaks address lines, with
`vix-mix ctl` as its command. The decision and what was considered and left are the
[charter](../charter.md)'s (§ Decisions, *Control from outside*, and § Rejected); the change
to the shared contracts is the [build map](../build-map.md)'s change request 14.

## Goal

A hotkey mutes the mic. A Stream Deck key turns one plugin off and shows whether it is on. A
script takes the music down two decibels. Today none of them can: the window is the only
caller of `Host`, and nothing outside the process reaches it.

Each of those changes is already a `Command` the window sends (`SetMute`, `SetInsertEnabled`,
`SetSend`, `SetMixLevel`, `SetMixMute`, `SetLink`, `SetChainRouting`, `SetPreview`), validated
by the model, with a refusal that says what to do. So this lane adds no behaviour to the
mixer. It adds a second caller, and a way for that caller to read and follow what the
commands change.

### Choices, taken with the user

- **Standard input and output, through a socket.** The mixer is a window, so its own stdin
  is nobody's to write to. `vix-mix ctl` is a client whose stdin and stdout are the protocol,
  connected to the running mixer over a Unix socket. A shortcut runs it once; a deck plugin
  runs it and exchanges lines, or opens the socket itself.
- **OSC's addresses, not its transport.** Every control is an address with a value
  (`/ch/1/monitor/mute true`). The lines are text and every request is answered, which UDP
  cannot do. Each address maps onto an OSC message, so a UDP listener can be added over the
  same addresses.
- **Every switch and fader of the console, and nothing that changes its shape.** Mutes,
  levels, the link, MON and MST, the ear, a plugin's bypass. Adding, removing, moving and
  renaming carry an undo toast and a plugin's lifetime, and stay the window's.
- **`/ch/<id>/mute` is both sends.** A channel has a mute on each send and none of its own;
  "mute the mic" as one address is what a shortcut wants, beside the two it is made of.
- **Any program of the same user may use it**, as any may run `wpctl`. The socket is a file
  in the session's runtime directory, in a directory only the user can enter; nothing listens
  on the network.

## Read first

- `CLAUDE.md`, and the charter's entry and change request 14 named above.
- [reference/model.md](../reference/model.md) § Commands and § The taper;
  [reference/host.md](../reference/host.md) § The trait;
  [reference/app.md](../reference/app.md) § The shell, § Pacing and § The command line.
- `crates/vixmix-model/src/command.rs`, `setup.rs`, `ids.rs`, `taper.rs`: the commands, the
  ids that never come back, positions and decibels.
- `crates/vixmix-host/src/host.rs`: `Host`, UI thread only; both hosts report
  `HostEvent::SetupChanged` at the poll after a command changed the setup.
- `app/src/app.rs`, `pacing.rs`, `shell.rs`, `main.rs`, and `signals.rs`, which is the
  pattern for a thread that asks the event loop for something through its proxy.
- `app/src/test_support.rs`: `Recorder` keeps every `Command` the window sent, which is how
  a test holds a request from outside to the command the matching button sends.

## What this lane owns, what others keep

| This lane | The others |
|---|---|
| the request and reply lines, the addresses and what each reads and writes, the protocol's number | the model: what a command means, and its refusals, passed on word for word |
| the clients, what each follows, the bounded queues between their threads and the UI thread | the host: applying a command, saving, the engine, the backend; `Host` is unchanged |
| the socket: where it is, who may hold it, its threads; the `ctl` command | the window: every gesture, the toasts, undo; a request from outside shows no toast |
| when the app serves requests and tells followers, and the hidden window's poll after a change | the pacer's rules, which gain one method and lose none |

## The protocol

A request is one line, `verb address [value]`; an address alone reads it, and an address
with a value sets it. The verbs:

| Verb | What it does | Answers with |
|---|---|---|
| `get <address>` | reads a value, or every value under an address | the values |
| `set <address> <value>` | writes a switch or a fader | what changed |
| `toggle <address>` | flips a switch | what changed |
| `nudge <address> <amount>` | moves a fader by an amount, kept within its travel | what changed |
| `watch <address>` | follows an address from now on | the values as they stand |
| `unwatch <address>` | stops following it | nothing |

Every request is answered by its lines, each `address value`, and then exactly one `ok` or
`error: ` and a message that ends with what to send instead. A change to something a client
follows comes as `event address value`, and `event address gone` when a channel or a plugin
went; an event is never inside an answer. The mixer greets a connection with one line that
carries the protocol's number. A value is `true` or `false`, a number, or text in double
quotes as JSON writes it; a silent fader's decibels are `-inf`.

What the mixer prints always names a channel and a plugin by id:

| Address | Value | A write sends |
|---|---|---|
| `/ch/<id>/name`, `/ch/<id>/kind` | text | read only |
| `/ch/<id>/link` | switch | `SetLink` |
| `/ch/<id>/mute` | switch: both sends | `SetMute` twice |
| `/ch/<id>/{monitor,master}/mute` | switch | `SetMute` |
| `/ch/<id>/{monitor,master}/db` | decibels, through the taper | `SetSend` |
| `/ch/<id>/{monitor,master}/position` | 0 to 1, the fader's travel | `SetSend` |
| `/ch/<id>/{monitor,master}/processed` | switch: MON, MST | `SetChainRouting` |
| `/mix/{monitor,master}/{mute,db,position}` | | `SetMixMute`, `SetMixLevel` |
| `/mix/master/preview` | switch: the ear | `SetPreview` |
| `/fx/<instance>/enabled` | switch: the bypass square | `SetInsertEnabled` |
| `/fx/<instance>/{name,strip,slot}` | | read only |

A person may write a channel by its name (`/ch/mic/…`, compared as `slug` makes its node
name), and a plugin under its strip by its slot, counted from 1, or by its name:
`/ch/mic/fx/2/enabled`, `/mix/master/fx/limiter/enabled`. A name two channels or two plugins
of a strip share is refused, naming each.

## Steps

1. **The decision in the docs**: the charter's entry, change request 14, this lane's rows
   in the build map and the roadmap, this handoff in the indexes.
2. **The protocol**, in a new crate `crates/vixmix-control` that depends on `vixmix-model`
   only: values, addresses resolved against a `Setup`, the table of every address with its
   value (`leaves`) and the difference between two tables, a request turned into the
   commands it sends. `slug` moves from `vixmix-pipewire` into the model, re-exported where
   it was. `docs/reference/control.md` owns the crate.
3. **The service**: the clients and what each follows, `serve` (a bounded number of
   requests against a `Target`, which is `setup()` and `apply()`), `publish` (the events for
   a setup that changed). Every queue bounded: 8 clients, a request line's length, the
   requests waiting, each client's answers waiting; a client whose answers are not read is
   dropped. The wake to the UI thread is one flag, however many requests wait.
4. **The socket and the client**, Unix only: the path from the session's runtime directory
   and the profile's name, a lock file so one mixer holds a socket and only its holder
   replaces a file a dead mixer left, the accepting thread and a reading and a writing
   thread a connection; `client` for one request and for the relay of stdin and stdout.
5. **The app**: the event loop's own event becomes an enum with a second case; the socket
   is opened once the host has booted and closed before the host goes; a waiting request is
   served against the host with `Host::apply`, never through the toast line; a change asks
   for a frame, or, while the window is hidden, for a poll of the host through
   `Pacer::serve_soon`, so the followers are told and the save comes due; `serve_host`, the
   one place the host is polled, publishes when a poll reports `SetupChanged`, so a change
   made in the window reaches a follower too.
6. **`vix-mix ctl`**, before any flag is parsed, with no window and no host: one request
   and its answer (exit 0, 1 for `error:`, 3 when no mixer answers), `watch` until stopped,
   and `-` for the relay.
7. **The close**: the cookbook `control-from-outside.md`, a step in the by-hand pass, this
   record with every deviation and why, the roadmap, `CLAUDE.md`.

## Files you own

`crates/vixmix-control/**`; `app/src/control.rs`, `app/src/ctl.rs`, `app/tests/ctl.rs`; in
`app/src/app.rs`, `main.rs`, `pacing.rs` and `test_support.rs`, what steps 5 and 6 name;
`crates/vixmix-model/src/slug.rs` and the re-export in `crates/vixmix-pipewire/src/lib.rs`;
`docs/reference/control.md`, `docs/cookbook/control-from-outside.md` and this handoff.

## Acceptance

- `dev check` is green on Linux and on Windows, where everything but the socket and the
  client builds and is tested.
- A test fails to compile when a `Command` variant is added without saying whether a
  request can send it, and a property holds that a request sends only the eight commands
  this lane names.
- A request for a mute sends the `Command` the mute button sends, shown with `Recorder`; a
  refused request changes nothing and its answer ends with what to send instead; a change
  made in the window reaches a client that follows it.
- A client that stops reading is dropped and the others are served; a ninth connection is
  told why it is refused; a second mixer leaves a running mixer's socket alone, and a file
  a dead mixer left is replaced.
- `vix-mix ctl` against `vix-mix --fake`: a request changes the console, `watch` prints a
  change made in the window, and each exit code is as step 6 says.
- **Not this lane's to prove**: a mute heard on a real microphone. A debug build on the
  real host would be a second mixer beside the installed one
  ([#34](https://github.com/thechimaeraproject/vix-mix/issues/34)); the cookbook's last
  step is the user's check.

## Out of scope

The commands that add, remove, move or rename, and sources, devices and preferences; the
meters; an OSC listener over UDP; a Stream Deck plugin; a transport on Windows; MIDI. A
second Vix Mix is still not stopped
([#34](https://github.com/thechimaeraproject/vix-mix/issues/34)): it runs without the socket
and says so, and the lock this lane adds is what stopping it can use.
