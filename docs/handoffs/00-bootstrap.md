# 00 — Bootstrap

**Record — shipped 2026-11-04.** The root, the five crates and the app with the shared
contracts as compiling skeletons, the kit adopted, the docs and every handoff written, CI on two
legs; `dev check` green on Windows and `cargo run -p vix-mix -- --quit-after-frames 3` exits 0.
Deviations from the steps below, all deliberate:

- **Step 1** used a plain `mv`, not `git mv`: the design pack was never tracked.
  `.gitattributes` also marks `*.png` binary. `node_modules/` is gitignored with `/target`,
  `.kit/` and `.idea/`.
- **Step 4** did not run `kit init`: it had already been run as `mixer` before this lane, and it
  refuses to overwrite, so `kit.repo.json` was rewritten by hand to the manifest below. The
  global `kit` was already the release the repository pins, so no global install was needed.
- **The contracts** were refined where Rust or a stated rule forced it, each recorded in its
  reference doc and summarised as change request 0 in the [build map](../build-map.md):
  `Command`'s variants carry named fields; `DeviceInfo` lives in the model (the Linux-only
  backend and the host both name it); `Backend::connect` takes a bounded `SyncSender`;
  `Host::tick_editors` pairs each event with its instance; `Rack` implements `Lender` through a
  weak reference to itself; `Instances::create` takes the `Catalog`. One question was opened
  rather than decided: how a plugin editor's own edit reaches the lent processor (change
  request 1, for 02 and 04).
- **More than a skeleton where it was cheap and pinned by a test**: the taper is the
  prototype's, exactly; `apply` already validates, moves apps, links faders and moves inserts
  as the prototype does; the engine's handoff, acknowledgement, meters and no-alloc proof are
  real; the rack lends and reaps; `RealHost` boots, pumps the null backend and saves;
  `FakeHost` serves the prototype's example console with its simulated meters, so lane 05 can
  start at once. `chimaeraproject-fs` is therefore adopted already ([kit-fs.md](../kit-fs.md)),
  not at lane 01.
- **The window** shows the header, the banner and a one-line channel count rather than an
  empty frame, and takes `--fake` (FakeHost) beside `--quit-after-frames`.
- **CI's Linux leg runs on `ubuntu-latest`**, not the organisation's self-hosted runner the
  kit's adoption cookbook names: it installs packages and starts its own PipeWire daemon. The
  daemon setup is a script, `scripts/ci/headless-pipewire.sh`, so WSL2, the VM and lane 03 run
  the same thing; it adds `dbus` to the packages (WirePlumber wants a session bus) and also
  proves the `default` metadata exists. No CI run exists yet (no push); the Linux leg was
  proven before the first push in an `ubuntu:24.04` container with the same packages and
  vendored sources: `cargo fmt --check`, `clippy -D warnings` (building `pipewire-sys`) and
  `cargo test --workspace` green, the script listing the null sink, the null source and the
  `default` metadata. WSL2 could not be used: its user has no passwordless `sudo` for the
  packages.
- **No cookbook** was written: the two named (`run-headless-pipewire.md`, `by-hand-pass.md`)
  are lanes 03's and 06's, so the index has no Cookbook section yet.
- **Step 7** (the repository, the first push, `kit protect`) is the main session's: an agent's
  push is refused by the hooks. The GitHub repository already existed with a generated README.
  `kit doctor`: every row ok but `protect.ruleset`, which `kit protect --apply` fixes after the
  merge (`ci.required-known` is skipped until the first push records a run).

**Needs:** nothing. **Runs alongside:** nothing (serial, first). **Size:** S.

## Goal

Everything every other lane stands on: the repository at its final layout, a Cargo workspace
whose five crates and app compile on Windows and Linux with the shared contracts in them, the
kit adopted (manifest, `dev` registry, hooks, skills, the Cargo pin, the docs standard), the
docs skeleton with a reference per crate so the ownership partition is complete from day one,
every lane's handoff written in full, and CI that runs the gates on Linux (with a headless
PipeWire daemon) and Windows. No real behaviour beyond what the contracts need to be honest.

## Read first

- The plan of record: [build-map.md](../build-map.md) and [charter.md](../charter.md).
- The kit's
  [CLAUDE.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/CLAUDE.md),
  [rust.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/rust.md),
  [docs.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/docs.md),
  [dev-commands.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/dev-commands.md),
  [kit-cli.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/kit-cli.md)
  § The manifest and § The doctor checklist,
  [Adopt the kit CLI](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/cookbook/adopt-the-kit-cli.md),
  [Set up a project](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/cookbook/set-up-a-project.md),
  and the four crate references
  ([fs](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/fs.md),
  [param](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/param.md),
  [audio-processor](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/audio-processor.md),
  [vst3-host](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)).
- Vix, the template (`A:\RustroverProjects\Vix` on the development machine):
  [kit.repo.json](https://github.com/thechimaeraproject/Vix/blob/master/kit.repo.json),
  [package.json](https://github.com/thechimaeraproject/Vix/blob/master/package.json),
  [Cargo.toml](https://github.com/thechimaeraproject/Vix/blob/master/Cargo.toml),
  [scripts/dev/tasks.mjs](https://github.com/thechimaeraproject/Vix/blob/master/scripts/dev/tasks.mjs),
  [.github/workflows/ci.yml](https://github.com/thechimaeraproject/Vix/blob/master/.github/workflows/ci.yml),
  [docs/handoffs/08-audio-engine.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/handoffs/08-audio-engine.md)
  (the handoff shape), [docs/kit-cli.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/kit-cli.md)
  (the adoption-record shape), and
  [app/src/main.rs](https://github.com/thechimaeraproject/Vix/blob/master/app/src/main.rs) and
  [app/src/gpu.rs](https://github.com/thechimaeraproject/Vix/blob/master/app/src/gpu.rs) (the
  window shell).
- The design pack, [design/README.md](../design/README.md).

## What this lane owns, what others keep

It owns everything at the root and every file it creates; each crate's skeleton then passes to
its lane (01–06), which replaces it behind the same signatures. Nothing is inherited.

## Steps

1. **Layout.** Move `refs/Inital Plan/mixer-design` to `docs/design` and delete `refs/`;
   `.gitignore` (`/target`, `/node_modules`, `.kit/`, `.idea/`); `.gitattributes`
   (`* text=auto eol=lf`, `*.bat text eol=crlf`). The root `Cargo.toml` becomes a virtual
   workspace (`crates/*`, `app`; `[workspace.package]` version, edition 2024, `rust-version`
   1.95, MIT); `[profile.dev.package."*"] opt-level = 3`; `rust-toolchain.toml` 1.95.0 with
   rustfmt and clippy; `.cargo/config.toml` with `[net] git-fetch-with-cli = true`;
   `Cargo.lock` committed.
2. **Crates.** `vixmix-model`, `-engine`, `-plugins`, `-pipewire` (Linux only; empty
   elsewhere), `-host`, and `app/` (package and binary `vix-mix`), with the contract types,
   each with a `lib.rs` doc comment naming its reference doc and tests that pin real
   behaviour.
3. **Kit dependencies** at kit-v0.23.0 under `[workspace.dependencies]`, all four at one tag (or
   the param types arrive twice); the first adoption by hand, per the kit's issue #188:
   the tag rows, `kit-source.json` with the tag's commit (`git ls-remote --tags` the kit),
   `CARGO_NET_GIT_FETCH_WITH_CLI=true cargo fetch`; never `kit bump` for this one.
4. **The kit CLI.** `kit.repo.json` (name `vix-mix`, role consumer, `dev` node, `kit.npm`,
   `kit.cargo { manifests: ["Cargo.toml"], lock: "Cargo.lock" }`, release none,
   `ci.required ["check (linux)", "check (windows)"]`, `protect.merges` admins); `package.json`
   and `.npmrc` (the tooling from `npm.rhyes.org`); `scripts/dev/tasks.mjs` in Vix's shape
   (`check` needs `fmt`, `clippy`, `test`, `check-docs`); `kit hooks install`,
   `kit skills install`, `kit workspace add .`.
5. **Docs.** `README.md` (five sections), `docs/README.md`, `charter.md`, `roadmap.md`,
   `build-map.md`, `handoffs/README.md` and 00–08, `reference/*.md` with `**Owns:**` lines so
   the partition is complete, the four `kit-*.md` adoption records, `kit.docs.config.mjs`,
   `CLAUDE.md` (the kit's verbatim git block, § Status, § Rules) and `AGENTS.md`.
6. **CI.** `.github/workflows/ci.yml` on `pull_request` and `workflow_dispatch`: a matrix
   named `check (linux)` on `ubuntu-latest` (apt `libpipewire-0.3-dev pipewire wireplumber
   pipewire-audio-client-libraries libclang-dev`; a headless `pipewire` and `wireplumber` under
   their own `XDG_RUNTIME_DIR` with a `support.null-audio-sink` sink and source, proven with
   `pw-cli ls Node`) and `check (windows)` on `windows-latest`, each running `dev check`;
   `KIT_TOKEN` as in Vix's workflow.
7. **The repository** (main session): check the name once more, create
   `thechimaeraproject/vix-mix` private if it does not exist, push the bootstrap PR with
   `kit push --yes`, `kit merge`, then `kit protect --apply`.

## Files you own

Everything at the root (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`,
`kit.repo.json`, `kit-source.json`, `package.json`, `package-lock.json`, `.npmrc`, `dev.bat`,
`dev.sh`, `scripts/`, `kit.docs.config.mjs`, `.gitignore`, `.gitattributes`, `.githooks/`,
`.claude/`, `.agents/`, `.github/`, `README.md`, `CLAUDE.md`, `AGENTS.md`), `docs/`, and the
first version of every crate and of `app/`.

## Acceptance

- `dev check` green on Windows and in CI on both legs.
- `kit doctor` green (`cargo.tag-agrees`, `cargo.one-source`, `docs.standard` among them) but
  for the rows that need the remote PR flow, named in the status line.
- `cargo run -p vix-mix` opens a window on Windows with the no-backend banner;
  `cargo run -p vix-mix -- --quit-after-frames 3` exits 0.
- The handoffs are in the index, and every lane's crate compiles against the contracts.

## Out of scope

Any real behaviour beyond the contracts: chains (02), the PipeWire client (03), real plugin
instances (04), the console (05), routing and autosave (06).
