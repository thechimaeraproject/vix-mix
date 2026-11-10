#!/usr/bin/env bash
# rpm.sh: builds Vix Mix's RPM on Fedora (or another rpm distribution) from the commit checked
# out: packaging/linux/vix-mix.spec, built from a source tarball of HEAD with every crate
# vendored, so rpmbuild reads nothing from the network. Written to <target>/package/ as
# vix-mix-<version>-1.<snapshot>.<dist>.<arch>.rpm with its .sha256 (<target> is
# $CARGO_TARGET_DIR or target/). `dev rpm` runs it; .github/workflows/linux-build.yml runs it in
# a Fedora container after every merge.
#
#   rpm.sh            a snapshot of HEAD: the release is 1.<commit's UTC time>git<short id>, so
#                     each newer commit's RPM upgrades the last (dnf upgrade ./the.rpm)
#   rpm.sh --release  the version itself (vix-mix-<version>-1), for a release
#
# It needs rpmbuild, the spec's BuildRequires and rustup's cargo: `sudo dnf install rpm-build
# rustup gcc clang-devel pipewire-devel pkgconf-pkg-config desktop-file-utils libappstream-glib
# git`, then `rustup-init -y` (docs/cookbook/install-on-fedora.md step 5). Vendoring fetches the
# kit's private crates through git, with the credentials `gh auth login` gave it (in CI, the
# KIT_TOKEN rewrite).
set -euo pipefail

die() {
  echo "rpm.sh: $*" >&2
  exit 1
}

snapshot=1
case "${1:-}" in
  "") ;;
  --release) snapshot=0 ;;
  -h | --help)
    sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  *) die "unknown argument '$1': write nothing for a snapshot of HEAD, or --release" ;;
esac

[ "$(uname -s)" = Linux ] || die "the RPM is built on Linux: run it on the Fedora machine (dev rpm), not on $(uname -s)"
command -v rpmbuild >/dev/null || die "no rpmbuild: sudo dnf install rpm-build"
command -v cargo >/dev/null || die "no cargo on PATH: sudo dnf install rustup && rustup-init -y, then open a new shell"
command -v git >/dev/null || die "no git: sudo dnf install git"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
spec=packaging/linux/vix-mix.spec
meta=packaging/linux/org.chimaeraproject.vixmix.metainfo.xml

version="$(sed -n '/^\[workspace\.package\]/,/^\[/{s/^version = "\([^"]*\)".*/\1/p}' Cargo.toml)"
[ -n "$version" ] || die "no version in Cargo.toml's [workspace.package]: the RPM is named after it"
newest="$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' "$meta" | head -n 1)"
[ "$newest" = "$version" ] || die "$meta's newest <release> is '$newest' but the workspace is $version: add <release version=\"$version\" date=\"YYYY-MM-DD\"> at the top of its <releases>, then run this again"

git rev-parse --verify -q HEAD >/dev/null || die "no commit checked out: the RPM is built from HEAD"
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "rpm.sh: building HEAD ($(git rev-parse --short HEAD)); your uncommitted changes are not in it" >&2
fi

defines=(--define "vixmix_version $version")
if [ "$snapshot" = 1 ]; then
  stamp="$(TZ=UTC git log -1 --format=%cd --date=format-local:%Y%m%d%H%M HEAD)"
  defines+=(--define "vixmix_snapshot ${stamp}git$(git rev-parse --short=8 HEAD)")
fi

target="${CARGO_TARGET_DIR:-$root/target}"
out="$target/package"
top="$target/rpmbuild"
name="vix-mix-$version"
stage="$top/stage"
rm -rf "$top"
mkdir -p "$top/SOURCES" "$stage" "$out"

# The source: HEAD's tree, and every crate it builds with vendored beside it. `cargo vendor`
# runs with the checkout's .cargo/config.toml (git through the CLI, for the kit), then the
# configuration it prints replaces it, so the build reads only vendor/.
git archive --format=tar --prefix="$name/" HEAD | tar -x -C "$stage"
(
  cd "$stage/$name"
  cargo vendor --locked --versioned-dirs vendor >"$top/vendor.toml"
  mv "$top/vendor.toml" .cargo/config.toml
)
tar -C "$stage" --owner=0 --group=0 --numeric-owner -czf "$top/SOURCES/$name.tar.gz" "$name"
rm -rf "$stage"

rpmbuild -bb "$spec" --define "_topdir $top" "${defines[@]}"

built=()
for rpm in "$top"/RPMS/*/*.rpm; do
  [ -e "$rpm" ] || die "rpmbuild made no RPM under $top/RPMS: read its output above"
  base="$(basename "$rpm")"
  rm -f "$out/$base" "$out/$base.sha256"
  cp "$rpm" "$out/$base"
  (cd "$out" && sha256sum "$base" >"$base.sha256")
  built+=("$base")
done
rm -rf "$top"

for base in "${built[@]}"; do
  echo "built $out/$base"
done
echo "install it: sudo dnf install ./${built[0]}   (a newer one: sudo dnf upgrade ./the-newer.rpm)"
