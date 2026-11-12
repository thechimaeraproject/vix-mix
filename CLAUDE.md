# CLAUDE.md

Guidance for Claude Code when working in Vix Mix.

## What this repo is

Vix Mix (`vix-mix`) is a Wave Link class desktop mixer for Linux in Rust: up to eight input
channels (a hardware input or a group of apps), a VST3 chain per channel, two sends per channel
(Monitor, Master) with a level and a mute, two mixes with their own chains and output devices,
and Master also published as a virtual microphone. PipeWire's native API for audio, egui on wgpu
through winit for the window, and the chimaeraproject kit's Rust crates
(`chimaeraproject-fs`, `-param`, `-audio-processor`, `-vst3-host`) for what it shares with
[Vix](https://github.com/thechimaeraproject/Vix). The kit's shared crates own no thread model,
no window toolkit and no audio graph; those three are this repository's.

**The plan of record is [docs/build-map.md](docs/build-map.md)**: nine lanes, each with a
self-contained handoff in [docs/handoffs/](docs/handoffs/README.md), the conflict rules, and the
change requests to the shared contracts. **Read [docs/charter.md](docs/charter.md) before making
a design decision here** — the settled decisions with their reasons, what was rejected, what is
open. [docs/roadmap.md](docs/roadmap.md) says what is built, what is next, and what is only
named.

## Status

**Bootstrapped (2026-11-04):** [handoff 00](docs/handoffs/00-bootstrap.md) shipped the
workspace (`vixmix-model`, `-engine`, `-plugins`, `-pipewire`, `-host` and the `vix-mix` app),
the shared contracts as compiling skeletons with real minimal behaviour pinned by tests (the
prototype's taper, validated commands, the engine's acknowledged handoff with a no-alloc tick,
the rack's lending, `RealHost` on the null backend, `FakeHost` with the prototype's console),
the kit adoption (manifest, `dev`, hooks, skills, the Cargo pin, the docs standard), every
handoff, and CI on two legs with a headless PipeWire on Linux. Nothing makes sound yet. Each lane
that ships appends a dated paragraph here in the shape *"X is implemented (date): what it owns,
what it does not, see `docs/reference/x.md` and the handoff."*

**The model is implemented (2026-11-04):** `vixmix-model` owns the setup as data and its rules:
every command validated and applied whole or refused, its `Applied` exact part by part (proven
against the diff by a property test), channel and instance ids from counters so a removed id
never comes back, the prototype's taper, the engine snapshot, and `setup.ron` at format 2 (LF,
plugin state as base64) with the migration from format 1 and the rules checked at load. It does
not decide when to save, prepare, route or retire (the host), nor hold anything live. See
[docs/reference/model.md](docs/reference/model.md) and
[handoff 01](docs/handoffs/01-model.md).

**Plugins are implemented (2026-11-04):** `vixmix-plugins` owns the scan (the SDK's places plus
the preferences' extras, a per-bundle cache in `plugins.ron` so an unchanged bundle is not
described again, and since round 1 a bundle that must be loaded described in a child process,
`vix-mix --scan-bundle`, so one that aborts or hangs fails alone), the catalog's search,
favourites and recent, the instances on the UI thread
(`sync` to a setup's inserts, a missing plugin reported once and passed through, state, `reap`
in the kit's order), the `Rack` that lends processors to chains, the edit path of change
request 1 (a bounded ring per instance, merged into each block by the lent handle with nothing
allocated), and editor sessions; it is tested with a real VST3 plugin, its own copy of the
kit's test plugin, on Windows and Linux. It does not own when instances are synced or retired,
calling `Instances::tick`, or the shutdown order (the host's, 06), the chains themselves (the
engine's, 02), or the editor's window (the app's, 05). See
[docs/reference/plugins.md](docs/reference/plugins.md) and
[handoff 04](docs/handoffs/04-plugins.md).

**The engine is implemented (2026-11-04):** `vixmix-engine` owns the realtime graph: each
channel through its chain, MON/MST choosing processed or dry per send, both mixes through their
chains before level and mute, preview, meters and DSP load, nothing allocated on the tick from
its second block. It lends each plugin instance once and shares it between graphs, so `prepare`
never sets a running plugin up again; it drops replaced graphs on the host thread, and sets
instances up again for a new rate (`set_rate`) or new buses (`set_up_again`). It does not own
the data thread or the rings (03), the instances (04) or when to prepare (06). See
[docs/reference/engine.md](docs/reference/engine.md) and
[handoff 02](docs/handoffs/02-engine.md).

**The window is implemented (2026-11-04):** the `vix-mix` app owns prototype v5's console as a
winit and egui-wgpu window over the `Host` trait — the look the user passed at the gate (the
theme, the embedded Plex fonts, `--dump-png`), every gesture as the command it sends (faders,
mutes, link, the ear, MON/MST, bypass, inserts dragged within and between strips), the source
and output panels, the add-channel menu, the plugin browser, the menu and preferences, toasts
with Undo, plugin editor windows on X11 on Linux, and 60 Hz pacing while the window can be
seen (nothing drawn while it is hidden); each gesture, the pacing and the editor windows'
lifecycle are asserted headless against `FakeHost`; rebased onto lane 06's host, Undo is the
host's restore command, stars and Rescan plugins go to the host, a pass-through insert is
marked on its slot, and the host is polled when it asks between frames. It owns no setup,
audio or plugin (the host does); round 1 added telling an editor the size its window took and
the model's bound on plugin folders (change request 10, applied 2026-11-10), closing in order
when `SIGTERM`, `SIGINT` or `SIGHUP` asks, and its windows named for the desktop entry and
made with the launcher's activation token (2026-11-11); a frame for the meters alone is still
left to it (handoff 09). Its CPU acceptance is met, read per machine as the main session ruled (0.16 to
0.46 %; of one core, 3.8 to 6.3 % in a release build). See
[docs/reference/app.md](docs/reference/app.md) and
[handoff 05](docs/handoffs/05-app.md).

**The PipeWire backend is implemented (2026-11-04):** `vixmix-pipewire` owns the control thread,
every stream and proxy, the input rings and their latency governor (exactly one quantum), the
tick on the data thread (whichever output runs first in a cycle; the others copy that cycle's
mix), the registry with app identity, routing by sink serial through the `default` metadata
and reconnect with backoff; it does not decide routing (the host does, on each app's
appearance) or compute the mix (the engine does). See `docs/reference/pipewire.md`, the
cookbook `docs/cookbook/run-headless-pipewire.md` and
[handoff 03](docs/handoffs/03-pipewire-backend.md), whose record lists the deviations and
keeps the two-minute acceptance open until a shell with realtime scheduling. Change requests 6
(the backend's additions) and 7 (the host reports `Absent` with a banner until 06 keeps the
backend) are agreed and applied.

**The host is implemented (2026-11-04):** `vixmix-host` owns the orchestration: boot in order
(setup, plugins rescanned against their cache, instances with their state, the engine prepared,
then the backend), PipeWire kept for the whole run on Linux (a new quantum asked for in place,
change request 9) with every app routed the moment it appears and again when the setup moves
it, each command's `Applied` acted on, the save debounced
500 ms, a removed channel's or insert's instances kept for its undo (handed out as
`HostEvent::Removed`; the model's `RestoreChannel` and `RestoreInsert`, change request 2),
pass-throughs and notices surfaced, and shutdown in the kit's order (routings cleared, the
backend gone, the final save, every instance home and retired on the UI thread); the window
polls it when it asks and shows its notes on the toast line. It does not own what a command
means (01), the tick (02), the streams and routing writes (03), how instances are made (04) or
how anything is drawn (05). Proven on the null backend with a real plugin on both legs and
against the headless daemon on Linux; the Fedora pass ran on 2026-11-10 (handoff 09). See
`docs/reference/host.md`, `docs/cookbook/by-hand-pass.md` and
[handoff 06](docs/handoffs/06-host-and-integration.md).

**Packaging is implemented (2026-11-04); the first real test ran on 2026-11-10:**
[handoff 08](docs/handoffs/08-packaging-and-first-real-test.md) put the desktop entry, a
scalable icon in the console's colours and the AppStream metadata, under one id
(`org.chimaeraproject.vixmix`), in `packaging/linux/` with `install.sh`, and the release tarball
in `scripts/package/linux-tarball.sh` (`dev package`): Vix Mix installs after
`cargo install --path app --locked` or from the tarball, into `~/.local` or `/usr/local`, proven
in the Ubuntu container. The charter's packaging finding recommends native, not Flatpak, and
the user took it (2026-11-10): an RPM (`packaging/linux/vix-mix.spec`, built from a source
tarball with every crate vendored by `scripts/package/rpm.sh`, `dev rpm`), which
`.github/workflows/linux-build.yml` builds in a Fedora container beside the tarball after every
merge; a COPR once public. It does not own what the by-hand pass found: that
pass (handoff 08 step 4) ran as hours of daily use on Fedora KDE from the RPM, and
[handoff 09](docs/handoffs/09-round-1-fixes.md) holds its two findings (a plugin that aborts
while loading, the DSP readout), the items that held, the figures it did not measure and the
round 1 items still open. See
`docs/cookbook/install-on-fedora.md` and the two handoffs.

**The kit lifts are implemented (2026-11-07):** the engine's meter bank and counters are the
kit's `chimaeraproject-audio-io` (without its device layer: PipeWire stays the driver), and the
console's meter faders, their scale, peak hold and the taper trait they read through are the
kit's `chimaeraproject-egui-audio`; this repository's copies are deleted, and the look gate's
capture is byte-identical. Vix took both crates in the same kit release. Vix Mix keeps the
graph, the driver, which meter is which, the taper itself, the theme and what each gesture
sends; since round 1 also the DSP load over a window and its peak, which the kit's last-cycle
counter cannot give, and the look gate's reference was retaken for the header's peak tick.
See `docs/kit-audio-io.md`, `docs/kit-egui-audio.md` and
[handoff 07](docs/handoffs/07-kit-lifts.md).

**Round 1's first batch is in (2026-11-10):** the first pass on Fedora ran as daily use, and
[handoff 09](docs/handoffs/09-round-1-fixes.md) records it. Five issues are fixed: the editor
told the size its window took and the plugin-folder bound in the model (11), Monitor's missing
device named when only the Master source can tick (14), the Preferences Tab order (15), a DSP
load read over a window with its peak (25), and the scan describing a bundle that must be
loaded in a child process (26). A second batch (2026-11-11) fixed two more: a signal that asks
the window to close runs the shutdown order (12), and the window's names and the launcher's
activation token (18). Issue 13 was measured the same day and needs no change: a mixer
started again the instant it quit takes a playing app back on the PipeWire Fedora ships, and
where it breaks (Ubuntu 24.04's, which CI runs) the break is that PipeWire's. Still open in
round 1: issues 16 and 17, the measured figures of handoff 09 items 1, 2 and 5, and the
by-hand check on Fedora that each fix left in its item.

## Git workflow (read first)

**Work on a branch, never on `master`/`main`.** Every change is a PR: `kit push --yes` runs
the gates, pushes the branch and opens or updates the PR; `kit merge` merges when the checks
are green. Never run `git push`, `git tag`, `npm publish`, `gh pr merge` or `gh workflow run`
yourself — a hook refuses them and the default branch is protected on GitHub. Consumers take a
release with `kit bump`; when something is off, `kit doctor` says what and how to fix it. When
the kit misbehaves, or you make a mistake a clearer kit doc would have prevented, file it on the
kit's GitHub issues at once — the `kit-report-an-issue` skill says how — tell the user, and
carry on. The loop itself — branch, commit with `-F`, push, each refusal and its fix, merge,
`kit sync` — is the `kit-work-on-a-change` skill in `.claude/skills/` (installed by
`kit skills install`); its long form is [Work on a change](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/cookbook/work-on-a-change.md).

- The remote is `origin`, the private repository `thechimaeraproject/vix-mix`. Small commits
  with a subject that says what changed and why. A lane agent commits on its own branch in its
  own worktree and never pushes; the main session pushes and merges
  ([docs/build-map.md](docs/build-map.md) § How to run a lane).
- When a milestone or an ownership boundary changes, update this file's § Status, the roadmap
  rows, the handoff's status line and the affected reference in the **same commit**. When a doc
  and the code disagree, fix one of them in that commit — no dead notes.

## Layout

```
Cargo.toml                 the workspace: crates/*, app; the kit's four crates at one tag
crates/vixmix-model/       the setup as data: types, commands with validation, the taper, setup.ron, the engine snapshot
crates/vixmix-engine/      the realtime graph: the prepared graph and its handoff, the tick, meters, stats, the null backend
crates/vixmix-plugins/     the catalog, instances on the UI thread, the rack that lends them, editor sessions
crates/vixmix-pipewire/    Linux only (empty elsewhere): the PipeWire client
crates/vixmix-host/        the orchestrator behind the Host trait: RealHost, FakeHost
app/                       the vix-mix binary: winit + egui-winit + egui-wgpu over a Host
docs/                      index, charter, roadmap, build map, reference, handoffs, the design pack, kit-*.md records
scripts/dev/tasks.mjs      the dev registry; scripts/dev.mjs, dev.bat and dev.sh are its front door
scripts/package/           the Linux release tarball and the RPM (dev package, dev rpm)
packaging/linux/           the desktop entry, icon, AppStream metainfo, install.sh and the RPM spec
.github/workflows/ci.yml   check (linux) with a headless PipeWire, check (windows)
.github/workflows/linux-build.yml   the tarball and the RPM as artifacts, on master after every merge (ci.onMerge)
```

The dependency direction: the model depends on no workspace crate; the engine on the model; the
plugins crate and the PipeWire crate on the model and the engine; the host on all four; the app
on the host and the model.

## Rules every crate obeys

- **Nothing allocates, locks or logs on the audio thread.** `Engine::process` and everything it
  calls, and PipeWire's `process` callbacks, are pinned by `assert_no_alloc` tests; setup work
  happens on the host thread and is handed over.
- **No thread, device handle, plugin instance or window type in `vixmix-model`.** The model is
  data; live things are referenced by id.
- **Every list and channel is bounded.** A list has its limit (8 channels, 8 recent plugins,
  the graphs the engine keeps until the tick acknowledges them); a queue between threads has a
  fixed capacity (`sync_channel`, an `rtrb` ring, an atomic slot); "latest wins" is a slot, not
  a queue.
- **The shared contracts change only through a change request** in
  [docs/build-map.md](docs/build-map.md) § Change requests, agreed in the main session, because
  every other lane compiles against them.
- **Error messages teach the fix.** A refusal says what was wrong and what to do instead.
- **Tests pin behaviour, not code paths.** A test asserts something a user or another crate
  would notice; no `assert!(true)`.
- **No version numbers in prose.** Manifests own them (`Cargo.toml`, `package.json`,
  `kit-source.json`).
- **Docs follow the standard.** A new source file under `crates/*` or `app` is owned by exactly
  one reference doc's `Owns:` line or exempted with a reason in `kit.docs.config.mjs`; a new doc
  is an index entry under its class; a handoff's first line is its status; cross-repository
  links are absolute GitHub URLs.

## Commands

Rust is pinned by `rust-toolchain.toml`. The kit's crates come from its private repository
through the git CLI (`.cargo/config.toml`), with this machine's credentials. Dev builds optimise
their dependencies, so the first build after a dependency changes is the slow one.

```
npm install                          # once: the kit's tooling (dev, the hooks, kit-check-docs)
dev check                            # every gate in order: fmt, clippy, test, check-docs; kit push runs it
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p vix-mix                 # the window on the real host: PipeWire on Linux (it routes the
                                     # desktop's apps while it runs), the no-backend banner elsewhere;
                                     # the VST3 folders are scanned before the window opens
cargo run -p vix-mix -- --fake       # the window on FakeHost: the prototype's example console
cargo run -p vix-mix -- --quit-after-frames 3   # draws three frames and exits 0
kit doctor                           # what is off in this checkout, and the fix for each
```

## Docs

[docs/README.md](docs/README.md) is the index, and its trust table says how much to believe each
class: Start here (charter, build map, roadmap), Reference (owns source files), Record (the
design pack, the handoffs, the `kit-*.md` adoption records), and, once written, Cookbook
(numbered, ends in *How you know it worked*) and Postmortem.
