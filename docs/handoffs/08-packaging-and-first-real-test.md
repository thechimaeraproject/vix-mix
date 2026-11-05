# 08 — Packaging and the first real test

**Record — shipped 2026-11-04 (step 4 waits for Fedora KDE on the NV3).** Vix Mix installs as a
desktop app: `packaging/linux/` holds the desktop entry, a scalable icon in the console's
colours, the AppStream metadata and `install.sh`; `cargo install --path app --locked` and the
release tarball (`scripts/package/linux-tarball.sh`, `dev package`) are the two install paths,
with the Fedora prerequisites, in
[cookbook/install-on-fedora.md](../cookbook/install-on-fedora.md). The packaging question is
answered in [the charter](../charter.md) § Open: native, with the evidence and the three
options. Fedora KDE is not on the NV3 yet, so the by-hand pass (step 4) has not run:
[handoff 09](09-round-1-fixes.md) is the handoff that pass fills, seeded with the fifteen items
the earlier lanes left open. Of the acceptance, the charter bullet is met; the
install on Fedora and the pass wait for the machine. Proven in the `vixmix-linux-dev`
container (Ubuntu, not Fedora): `cargo install --path app --locked --offline` and the tarball
build, `install.sh` from the checkout and from the unpacked tarball into `~/.local` and
`/usr/local` and back out (nothing left behind), `Exec` always double-quoted (a prefix with a
space and one with each other reserved character of the Desktop Entry spec install and
validate; `"`, `` ` ``, `$`, `\`, `%` and control characters are refused before anything is
written), `desktop-file-validate` silent and `appstreamcli validate --pedantic` successful,
the binary linking only glibc, `libgcc_s` and `libpipewire-0.3`. `dev check` is green on
Windows.
Deviations from the steps below, each with its reason:

- **The desktop entry is `org.chimaeraproject.vixmix.desktop`, not `vix-mix.desktop`.** One id
  names the desktop entry, the AppStream component (its `launchable`) and the icon, as the
  Desktop Entry and AppStream specifications recommend, and a Wayland app id finds its entry by
  that name. `StartupWMClass=vix-mix` ties the window to it on X11, where winit names the class
  after the binary; on Wayland the app sets no app id, which handoff 09 item 9 checks rather
  than a change to lane 05's window now. `StartupNotify` is left out: the app does not read
  the launcher's activation token yet, so `true` would promise what it does not do and `false`
  would turn off the launch feedback that `StartupWMClass` ends on X11; item 9 checks Plasma's
  feedback and names winit's fix.
- **The packaging files sit in `packaging/linux/`, outside the code roots**, so no reference
  owns them; the cookbook documents what goes where, and each file says what it is in its
  header. `install.sh` writes `Exec` and `TryExec` as the absolute path of the binary it
  installed or found, because the desktop session's `PATH` need not hold `~/.cargo/bin`.
- **The Fedora prerequisites are more than the two the step names**: `gcc` (the `libspa`
  crate's C shim and the linker), `pkgconf-pkg-config`, and `git` with `gh` for the kit's
  private crates, besides `pipewire-devel` (`libpipewire-0.3` and `libspa-0.2`, read through
  pkg-config) and `clang-devel` (`libclang` for bindgen, with `clang-libs` carrying the
  compiler's own headers). They come from `pipewire-sys`'s and `libspa`'s build scripts and the
  Fedora package database; the kit's VST3 host needs nothing at build time. They were not run
  on Fedora (handoff 09 item 11).
- **The tarball carries the fonts' licence** (`share/licenses/vix-mix/OFL.txt`), because the
  binary embeds IBM Plex, and the metadata's licence is `MIT AND OFL-1.1`. The repository has
  no `LICENSE` file yet (the charter keeps the licence open until a public release), so the
  tarball has none for the code. The metadata names the developer "The Chimaera Project"; the
  GitHub organisation has no display name to take.
- **The metadata's newest `<release>` must name the workspace's version**: the tarball script
  refuses otherwise and says what to add, so the two cannot drift.
- **The packaging finding recommends native and builds no Flatpak manifest.** It also found that
  a Flatpak would need `vixmix-pipewire` to declare itself a PipeWire manager to route other
  apps; that is recorded as a cost, not proposed as a change request, since the
  recommendation is native.
- **Step 4 is deferred** to the Fedora pass, which runs [handoff 09](09-round-1-fixes.md)'s
  first steps; the measured figures in [reference/pipewire.md](../reference/pipewire.md)
  § Latency wait for it.
- **Step 5 needed no edit**: the Windows backend was already the roadmap's first row under
  *Named but not built*; this lane's roadmap rows add the RPM spec and a COPR beside it.
- **Two files outside this lane's list gained an addition each, and two indexes a line.**
  `README.md` gained § Install, the front page's pointer to the install cookbook, where a reader
  of the repository looks first; `scripts/dev/tasks.mjs` gained the `Package` section with
  `dev package`, which runs `scripts/package/linux-tarball.sh` as the cookbook's step 5 says.
  Neither file is in this handoff's Files you own or in [build-map.md](../build-map.md)
  § Conflict rules' append-only list, and lane 05 touches neither, so the main session accepts
  or drops them at the merge. `docs/cookbook/README.md` and this folder's `README.md` gained
  the index lines of the cookbook and the handoff this lane adds. `CLAUDE.md` § Layout
  (`packaging/linux/`, `scripts/package/`) and a lane 09 row in the build map are not this
  lane's to edit: the main session adds them at the merge.

**Needs:** 06. **Runs alongside:** 07. **Size:** S.

## Goal

Vix Mix installable on the user's Fedora as a desktop app, the packaging question answered,
and the first by-hand pass on the real machine with its findings filed.

## Read first

- [charter.md](../charter.md) § Open (Flatpak or native) and § Decisions (the Linux machine).
- `docs/cookbook/by-hand-pass.md` (written by handoff 06) and
  [reference/host.md](../reference/host.md).
- Vix's [docs/cookbook/by-hand-pass.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/cookbook/by-hand-pass.md)
  and [docs/handoffs/22-round-1-fixes.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/handoffs/22-round-1-fixes.md)
  (how a by-hand pass's findings became a handoff).

## What this lane owns, what others keep

It owns the packaging files and the first real pass. Every crate stays with its lane; a finding
becomes an issue or a `09-round-1-fixes` handoff, not a fix made here.

## Steps

1. **Desktop integration:** `vix-mix.desktop`, an icon, and
   `org.chimaeraproject.vixmix.metainfo.xml`.
2. **An install path:** `cargo install` and a tarball, with the Fedora prerequisites
   (`pipewire-devel`, `clang-devel`) documented.
3. **Investigate Flatpak.** The user's Fedora is Flatpak-first, but a VST3 host inside the
   sandbox cannot load the host's plugins. Record the finding in the charter's open list with
   the options: native RPM / COPR, AppImage, Flatpak with `--filesystem=host`.
4. **Run `cookbook/by-hand-pass.md`** on the real Fedora install (Fedora KDE on the NV3); record
   the latency figure and the `pw-top` xrun count at 256 in
   [reference/pipewire.md](../reference/pipewire.md); file what it finds as issues or a
   `09-round-1-fixes` handoff.
5. **The Windows backend** (`vixmix-cpal`, over the kit's `audio-io`) is named in the roadmap,
   not built.

## Files you own

The packaging files (their directory is this lane's choice, with a reference that owns it if it
holds anything under a code root), the charter's open-list entry for packaging, this handoff,
your rows in `docs/roadmap.md`, one paragraph in `CLAUDE.md` § Status, and the measured figures
(latency, xruns at 256) in `docs/reference/pipewire.md`.

## Acceptance

- Vix Mix installs on Fedora by the documented path and appears in the KDE launcher with its
  icon.
- The packaging finding is recorded in the charter.
- The by-hand pass has been run on the real machine and every finding is filed; the latency
  figure and the `pw-top` xrun count at 256 are in `reference/pipewire.md`.

## Out of scope

Fixing what the pass finds (a following handoff); a Windows build.
