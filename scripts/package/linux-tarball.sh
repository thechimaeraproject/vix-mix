#!/usr/bin/env bash
# linux-tarball.sh: builds Vix Mix's release tarball on Linux. A release build of the vix-mix
# binary, with the desktop entry, the icon, the AppStream metainfo, the fonts' licence and
# packaging/linux/install.sh, laid out as the XDG tree install.sh copies from:
#
#   vix-mix-<version>-linux-<arch>/
#     install.sh
#     bin/vix-mix
#     share/applications/org.chimaeraproject.vixmix.desktop
#     share/icons/hicolor/scalable/apps/org.chimaeraproject.vixmix.svg
#     share/metainfo/org.chimaeraproject.vixmix.metainfo.xml
#     share/licenses/vix-mix/OFL.txt
#
# written to <target>/package/ as a .tar.gz with its .sha256 (<target> is $CARGO_TARGET_DIR or
# target/). `dev package` runs it; arguments go to `cargo build` (for example --offline). It
# needs what the build needs (docs/cookbook/install-on-fedora.md step 1).
set -euo pipefail

die() {
  echo "linux-tarball.sh: $*" >&2
  exit 1
}

[ "$(uname -s)" = Linux ] || die "the tarball is built on Linux, where the binary links libpipewire: run it on the Linux machine (dev package), not on $(uname -s)"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
ID=org.chimaeraproject.vixmix
meta="packaging/linux/$ID.metainfo.xml"

version="$(sed -n '/^\[workspace\.package\]/,/^\[/{s/^version = "\([^"]*\)".*/\1/p}' Cargo.toml)"
[ -n "$version" ] || die "no version in Cargo.toml's [workspace.package]: the tarball is named after it"
newest="$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' "$meta" | head -n 1)"
[ "$newest" = "$version" ] || die "$meta's newest <release> is '$newest' but the workspace is $version: add <release version=\"$version\" date=\"YYYY-MM-DD\"> at the top of its <releases>, then run this again"

cargo build --release --locked -p vix-mix "$@"

target="${CARGO_TARGET_DIR:-$root/target}"
name="vix-mix-$version-linux-$(uname -m)"
out="$target/package"
stage="$out/$name"
rm -rf "$stage" "$out/$name.tar.gz" "$out/$name.tar.gz.sha256"
mkdir -p "$stage"

install -Dm755 "$target/release/vix-mix" "$stage/bin/vix-mix"
install -Dm755 packaging/linux/install.sh "$stage/install.sh"
install -Dm644 "packaging/linux/$ID.desktop" "$stage/share/applications/$ID.desktop"
install -Dm644 "packaging/linux/$ID.svg" "$stage/share/icons/hicolor/scalable/apps/$ID.svg"
install -Dm644 "$meta" "$stage/share/metainfo/$ID.metainfo.xml"
install -Dm644 app/assets/fonts/OFL.txt "$stage/share/licenses/vix-mix/OFL.txt"

tar -C "$out" --owner=0 --group=0 --numeric-owner -czf "$out/$name.tar.gz" "$name"
(cd "$out" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
rm -rf "$stage"

echo "built $out/$name.tar.gz"
echo "install it: tar xzf $name.tar.gz && $name/install.sh   (sudo ... --prefix /usr/local for everyone)"
