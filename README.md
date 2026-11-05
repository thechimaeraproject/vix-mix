# Vix Mix

## What it is

Vix Mix is a Wave Link class desktop mixer for Linux, written in Rust: up to eight input
channels, each a hardware input or a group of apps, each with its own chain of VST3 plugins
and two sends, one to the **Monitor** mix you hear and one to the **Master** mix that plays to
your main output and that other apps can also record as an input. Both mixes have their own
plugin chains and output devices. Audio is PipeWire's native API; the window is egui on wgpu;
the VST3 host, the processor contract and the filesystem helpers are the chimaeraproject kit's
Rust crates, which it shares with [Vix](https://github.com/thechimaeraproject/Vix).

## Status

2026-11-04: bootstrapped ([handoff 00](docs/handoffs/00-bootstrap.md)). The workspace, the
shared contracts every lane compiles against, the docs and CI exist; the window opens with a
no-backend banner, and nothing makes sound yet. Lanes 01 to 05 run next, in parallel
([docs/build-map.md](docs/build-map.md)).

## Start here

1. [docs/charter.md](docs/charter.md) — what Vix Mix is for, the settled decisions, what was rejected, what is open.
2. [docs/build-map.md](docs/build-map.md) — the lanes, their order, the conflict rules, how to run one.
3. [docs/handoffs/README.md](docs/handoffs/README.md) — one self-contained brief per lane.
4. [docs/roadmap.md](docs/roadmap.md) — what is built, what is next, what is only named.
5. [docs/design/what-it-is.md](docs/design/what-it-is.md) — the product definition, with the prototype the window is built to.

## Run it

Rust is pinned by `rust-toolchain.toml`; Node is here only for the kit's tooling
(`npm install` once). The kit's crates come from its private repository through the git CLI
(`.cargo/config.toml`), with this machine's GitHub credentials.

```
npm install                                  # the kit's tooling: dev, the hooks, kit-check-docs
dev check                                    # every gate: cargo fmt --check, clippy, test, kit-check-docs
cargo run -p vix-mix                         # the window on the real host: PipeWire on Linux (it routes the
                                             # desktop's apps while it runs), the no-backend banner elsewhere;
                                             # the VST3 folders are scanned before the window opens
cargo run -p vix-mix -- --fake               # the window on FakeHost: the prototype's example console
cargo run -p vix-mix -- --quit-after-frames 3   # draws three frames and exits 0: the headless check
kit doctor                                   # what is off in this checkout, and the fix for each
```

## Install

On Fedora, as a desktop app in the KDE launcher with its icon:
[docs/cookbook/install-on-fedora.md](docs/cookbook/install-on-fedora.md) — the build
prerequisites, then `cargo install --path app --locked` and `packaging/linux/install.sh`, or
the release tarball `scripts/package/linux-tarball.sh` builds.

## Docs

[docs/README.md](docs/README.md) is the index; its trust table says how much to believe each
class of document. What this repository takes from the kit is recorded in
[docs/kit-cli.md](docs/kit-cli.md), [docs/kit-fs.md](docs/kit-fs.md),
[docs/kit-audio.md](docs/kit-audio.md) and [docs/kit-vst3.md](docs/kit-vst3.md).
