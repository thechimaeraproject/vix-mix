# Install Vix Mix on Fedora

Vix Mix as a desktop app on Fedora KDE: built from the checkout, installed with `cargo install`
or from the release tarball, and in the KDE launcher with its icon. It is a native install, not
a Flatpak: a sandboxed VST3 host cannot load the plugins the machine has, and yabridge does not
work inside one ([the charter](../charter.md) § Open, Packaging). The packaging files are
`packaging/linux/`; [handoff 08](../handoffs/08-packaging-and-first-real-test.md) made them.

What goes where. `<data>` is `~/.local/share` for an install for you alone (`$XDG_DATA_HOME`
when it is set), or `<prefix>/share` with `--prefix` (`/usr/local/share` for every user):

| From | Installed as |
|---|---|
| the `vix-mix` binary | `~/.cargo/bin/vix-mix` (step 4), or `<prefix>/bin/vix-mix` from the tarball (`~/.local/bin` by default; step 5) |
| `packaging/linux/org.chimaeraproject.vixmix.desktop` | `<data>/applications/`, its `Exec` and `TryExec` the binary's absolute path |
| `packaging/linux/org.chimaeraproject.vixmix.svg` | `<data>/icons/hicolor/scalable/apps/` |
| `packaging/linux/org.chimaeraproject.vixmix.metainfo.xml` | `<data>/metainfo/` |
| `app/assets/fonts/OFL.txt`, the licence of the fonts built into the binary | `<data>/licenses/vix-mix/` |

`org.chimaeraproject.vixmix` is the one id the desktop entry, the icon and the AppStream
metadata share. Your setup lives in `~/.config/vix-mix/` and no step here touches it.

## 1. Install what the build needs

```bash
sudo dnf install gcc clang-devel pipewire-devel pkgconf-pkg-config git gh
```

- `pipewire-devel`: the headers and the pkg-config files of `libpipewire-0.3` and `libspa-0.2`,
  which the `pipewire-sys` and `libspa-sys` crates find through pkg-config and read with bindgen.
- `clang-devel`: `libclang`, which bindgen loads to read those headers; it brings `clang-libs`,
  which carries the compiler's own headers (`stddef.h`) bindgen needs.
- `gcc`: the C compiler the `libspa` crate builds a small shim with, and the linker Rust calls.
- `pkgconf-pkg-config`: `pkg-config` itself, usually installed already.
- `git` and `gh`: the kit's crates come from its private repository through the git CLI
  (`.cargo/config.toml`), with the credentials `gh auth login` gives git.

What it needs to run, all of it Fedora KDE's default: PipeWire and WirePlumber running
(`systemctl --user status pipewire wireplumber`), `pipewire-libs`, a graphics driver with
Vulkan or OpenGL (Mesa's for AMD and Intel; for NVIDIA the driver from RPM Fusion or Mesa's
NVK), and XWayland, because a VST3 plugin's editor embeds only in an X11 window. The kit's VST3
host needs nothing at build time: it loads each plugin when the scan or the setup asks for it.

## 2. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Open a new shell afterwards. The first `cargo` command in the checkout installs the toolchain
`rust-toolchain.toml` pins.

## 3. Get the source

```bash
gh auth login
git clone https://github.com/thechimaeraproject/vix-mix && cd vix-mix
git ls-remote https://github.com/thechimaeraproject/chimaeraproject-kit HEAD
```

The last command prints one line without asking for a password; if it asks, `gh auth login`
did not reach git (`gh auth setup-git` connects them).

## 4. Install with cargo

```bash
cargo install --path app --locked
packaging/linux/install.sh
```

The first is a release build (several minutes the first time) that puts `vix-mix` in
`~/.cargo/bin`, with the dependencies `Cargo.lock` names. The second installs the desktop
entry, the icon and the metadata into `~/.local/share` and points the entry at that `vix-mix`
by its absolute path, so the launcher starts it whatever the desktop session's `PATH` is; it
finds the binary on `PATH` or in `~/.cargo/bin`, and says to run the first command when there
is none. To update later: `git pull`, then the same two commands.

## 5. Or install from the tarball

On a Linux machine with steps 1 to 3 done, in the checkout:

```bash
scripts/package/linux-tarball.sh
```

`./dev.sh package` (it needs Node: `sudo dnf install nodejs npm`) runs the same script once `npm install` has put the kit's tooling in the
checkout. It builds the release binary and writes
`target/package/vix-mix-<version>-linux-x86_64.tar.gz` with its `.sha256`; arguments go to
`cargo build` (`--offline`, say). The tarball holds `bin/vix-mix`, the files of the table above
under `share/`, and `install.sh`. It refuses when the newest `<release>` in the metadata does
not name the workspace's version (`Cargo.toml`), and says what to add.

On the machine to install on (it needs only step 1's run-time list):

```bash
tar xzf vix-mix-<version>-linux-x86_64.tar.gz
vix-mix-<version>-linux-x86_64/install.sh
```

That installs for you alone, the binary in `~/.local/bin`. For every user of the machine:
`sudo vix-mix-<version>-linux-x86_64/install.sh --prefix /usr/local`. When the binary's
directory is not on `PATH` the script says so; the launcher starts it anyway. A prefix whose
path holds `"`, `` ` ``, `$`, `\`, `%`, a tab or a newline is refused before anything is
written, because the desktop entry could not name the binary there; any other character,
spaces included, is fine.

## 6. Start it from the launcher

Open the application launcher and type "Vix Mix": the entry shows the icon, three channel
strips with their meters under blue faders, and sits under Multimedia. Start it. A first start
scans the VST3 folders before the window appears, which takes a while with many plugins.
Fedora's packages put VST3 plugins in `/usr/lib64/vst3`, which Vix Mix does not search yet:
[the by-hand pass](by-hand-pass.md) step 1 says how to add it.

## 7. Uninstall

```bash
packaging/linux/install.sh --uninstall && cargo uninstall vix-mix     # what step 4 installed
vix-mix-<version>-linux-x86_64/install.sh --uninstall                 # what step 5 installed
```

Give `--uninstall` the same `--prefix` the install had. It removes the desktop entry, the
icon, the metadata, the licence and, for the tarball, the binary; `~/.config/vix-mix/` stays
until you delete it.

## 8. How you know it worked

```bash
grep -E '^(Exec|TryExec)=' ~/.local/share/applications/org.chimaeraproject.vixmix.desktop
desktop-file-validate ~/.local/share/applications/org.chimaeraproject.vixmix.desktop
appstreamcli validate --no-net ~/.local/share/metainfo/org.chimaeraproject.vixmix.metainfo.xml
```

(`sudo dnf install desktop-file-utils appstream` if the last two are missing.) `Exec` names
the installed `vix-mix` by its absolute path in double quotes and `TryExec` by the same path
bare, `desktop-file-validate` prints nothing, and `appstreamcli` ends with
`Validation was successful`. In KDE, the launcher lists Vix Mix with its icon, and starting it
opens the window titled "Vix Mix". While it runs, its entry in the task manager should show
the same icon, and the launch cursor should stop bouncing once the window is up; a generic
icon or a cursor that bounces on until Plasma gives up is
[handoff 09](../handoffs/09-round-1-fixes.md)'s item 9.
