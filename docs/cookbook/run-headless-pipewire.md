# Run a headless PipeWire

A PipeWire daemon and WirePlumber with a null sink and a null source, in a Linux shell that has
no audio session — CI's Linux leg, a WSL2 or VM shell, an `ubuntu:24.04` container — so the
backend (`vixmix-pipewire`) can be run and its integration tests can pass against something
real. Everything here builds on `scripts/ci/headless-pipewire.sh`, which CI runs before its
gates; [reference/pipewire.md](../reference/pipewire.md) is what the backend does once the
daemon answers. Never point the tests at a desktop session: they write routings into the
`default` metadata, which WirePlumber remembers.

## 1. Pick the shell

- **CI** needs nothing: `check (linux)` installs the packages and runs the script
  (`.github/workflows/ci.yml`).
- **WSL2 or the Fedora VM**: a shell with `sudo`. Step 2 is done once, by the user (an agent
  cannot answer `sudo`).
- **A container**, when the shell above is not ready: `ubuntu:24.04` under Docker Desktop with the
  worktree mounted and Linux build output kept off the Windows `target/`:

  ```bash
  docker run --rm -it -v <worktree>:/src -w /src -e CARGO_TARGET_DIR=/root/target ubuntu:24.04 bash
  ```

  The container has no credential for the kit's private repository: vendor the crates on the
  Windows side (`cargo vendor <dir outside the worktree>`), mount that directory too, and give
  the container the `[source]` table `cargo vendor` prints, its `directory` pointed at the
  mount, in the container's own `~/.cargo/config.toml` (never in the repository). The container
  runs as root with no rtkit, so the data thread has no realtime priority: right for
  correctness, not for latency figures.

## 2. Install the packages and the toolchain

```bash
sudo apt-get install libpipewire-0.3-dev pipewire wireplumber pipewire-audio-client-libraries \
  libclang-dev dbus pkg-config build-essential
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # then open a new shell
```

In the container, drop `sudo` and add `curl ca-certificates git`. Fedora names the same
packages `pipewire-devel pipewire wireplumber pipewire-utils clang-devel dbus-daemon`. The
toolchain is the one `rust-toolchain.toml` pins; `rustup` installs it on the first `cargo`
command in the worktree.

## 3. Start the daemon

```bash
source scripts/ci/headless-pipewire.sh
```

Sourced, it leaves `pipewire` and `wireplumber` running in the background with their logs in
`$LOGS` (default `/tmp`), and keeps in your shell what the backend needs: `XDG_RUNTIME_DIR` (its
own directory unless one was set), `DBUS_SESSION_BUS_ADDRESS` (a session bus it starts when
there is none), and `VIXMIX_HEADLESS_PIPEWIRE=1`, the marker the integration tests require. It
prints `headless-pipewire: the daemon answers …` once `pw-cli ls Node` lists
`vixmix-ci-null-sink` and `pw-cli ls Metadata` the `default` metadata, and the two logs when it
does not within 15 s. The devices come from a drop-in it writes to
`~/.config/pipewire/pipewire.conf.d/90-vixmix-null-devices.conf`: the null sink is
`Audio/Sink`, the null source `Audio/Source/Virtual`.

## 4. Look at the graph

```bash
pw-cli ls Node          # every node: ids, node.name, media.class
pw-dump | less          # everything, as JSON, with full properties
pw-top -b -n 2          # per node: quantum, rate, wait and busy times, ERR (xruns)
pw-link -l              # every link, port by port
pw-metadata -n default  # the routing keys: target.object / target.node per stream
```

With the backend running, its nodes are `<app>.ch<id>.<slug>` (a channel), `<app>.monitor-out`,
`<app>.master-out` and `<app>.master`; `pw-top` shows them as followers (`+`) of one driver.

## 5. Play, record and route by hand

```bash
pw-play --target vixmix-ci-null-sink some.wav                     # an app stream
pw-record --target vix-mix.master --format f32 master.wav         # record the Master source
pw-record --target vixmix-ci-null-sink -P '{ stream.capture.sink = true }' --format f32 out.wav
pw-metadata -n default <app node id> target.object <sink object.serial> Spa:Id   # route
pw-metadata -n default -d <app node id> target.object                             # back to default
```

`pw-record` of a sink records its monitor (what was played to it), which is how the tests hear
the Monitor and Master outputs. Routing takes the sink's `object.serial` (`pw-cli ls Node`
shows it): that is the form WirePlumber remembers across an app's restarts.

## 6. Run the backend's tests

```bash
cargo test -p vixmix-pipewire                                   # unit and integration, seconds
cargo test -p vixmix-pipewire --test headless -- --ignored two_minutes      # the acceptance run
cargo test -p vixmix-pipewire --test headless -- --ignored daemon_restarts  # restarts the daemon
```

The integration tests (`crates/vixmix-pipewire/tests/headless.rs`) run one at a time with their
own app names. Without `VIXMIX_HEADLESS_PIPEWIRE` they print how to start the daemon and pass
without touching anything; under `CI` they fail instead. The two on-request runs are slow (two
minutes) or disturb the daemon the others share (it is killed and started again), so they are
`#[ignore]`d; the second leaves the restarted daemon running.

## 7. Stop it

```bash
pkill -x wireplumber; pkill -x pipewire
```

Or close the shell or the container. Nothing outside `$XDG_RUNTIME_DIR`, the drop-in and
WirePlumber's state under `~/.local/state/wireplumber` was written.

## 8. How you know it worked

```bash
pw-cli ls Node | grep -E 'vixmix-ci-null-(sink|source)'
cargo test -p vixmix-pipewire
```

The first prints both null devices' `node.name` lines; the second ends with
`test result: ok.` for the unit tests and for `tests/headless.rs`, whose tests ran rather than
printing `skipped:` (run it with `-- --nocapture` to see none of those lines).
