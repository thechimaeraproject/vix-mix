# 08 — Packaging and the first real test

**Record — not yet built.**
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
