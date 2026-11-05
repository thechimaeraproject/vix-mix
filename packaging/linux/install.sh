#!/usr/bin/env bash
# install.sh: puts Vix Mix where the desktop finds it (the XDG layout under a prefix), or takes
# it away again. It works from two places:
#   - the release tarball (scripts/package/linux-tarball.sh), beside bin/vix-mix and share/:
#     it installs the binary and the desktop files;
#   - the source tree (packaging/linux/), after `cargo install --path app --locked`: it installs
#     the desktop files and points them at the vix-mix that cargo installed.
# Your setup in ~/.config/vix-mix is never touched. The cookbook is
# docs/cookbook/install-on-fedora.md.
set -euo pipefail

ID=org.chimaeraproject.vixmix
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<USAGE
usage: install.sh [--prefix DIR] [--uninstall]

  --prefix DIR   where to install: ~/.local by default (you alone; the desktop files go in
                 \$XDG_DATA_HOME when it is set), or /usr/local for everyone (run it with sudo)
  --uninstall    take away what this script installs from the same place; your setup in
                 ~/.config/vix-mix stays
USAGE
}

die() {
  echo "install.sh: $*" >&2
  exit 1
}

prefix=""
uninstall=0
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix)
      [ $# -ge 2 ] || die "--prefix needs a directory: --prefix ~/.local or --prefix /usr/local"
      prefix="$2"
      shift 2
      ;;
    --prefix=*)
      prefix="${1#--prefix=}"
      shift
      ;;
    --uninstall)
      uninstall=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "unknown argument '$1': write --prefix DIR, --uninstall or --help"
      ;;
  esac
done

[ "$(uname -s)" = Linux ] || die "Vix Mix is a Linux desktop app: run this on the Linux machine"

if [ -z "$prefix" ]; then
  prefix="$HOME/.local"
  data="${XDG_DATA_HOME:-$HOME/.local/share}"
else
  case "$prefix" in
    /*) ;;
    *) die "--prefix '$prefix' is not an absolute path: write it from / (for example $HOME/.local)" ;;
  esac
  prefix="${prefix%/}"
  data="$prefix/share"
fi
bindir="$prefix/bin"

# Where the files come from: the tarball's tree, or this directory in a checkout.
if [ -f "$here/share/applications/$ID.desktop" ]; then
  layout=tarball
  src_desktop="$here/share/applications/$ID.desktop"
  src_icon="$here/share/icons/hicolor/scalable/apps/$ID.svg"
  src_meta="$here/share/metainfo/$ID.metainfo.xml"
  src_ofl="$here/share/licenses/vix-mix/OFL.txt"
  src_bin="$here/bin/vix-mix"
elif [ -f "$here/$ID.desktop" ]; then
  layout=source
  src_desktop="$here/$ID.desktop"
  src_icon="$here/$ID.svg"
  src_meta="$here/$ID.metainfo.xml"
  src_ofl="$here/../../app/assets/fonts/OFL.txt"
  src_bin=""
else
  die "no $ID.desktop beside this script: run the install.sh inside the unpacked tarball, or packaging/linux/install.sh in a checkout"
fi

dst_desktop="$data/applications/$ID.desktop"
dst_icon="$data/icons/hicolor/scalable/apps/$ID.svg"
dst_meta="$data/metainfo/$ID.metainfo.xml"
dst_licenses="$data/licenses/vix-mix"

# The launcher and the icon theme keep caches; refresh the ones this desktop has, quietly.
refresh() {
  if command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -f "$data/icons/hicolor/index.theme" ]; then
    gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true
  fi
  if [ "$(id -u)" != 0 ] && command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6 >/dev/null 2>&1 || true
  fi
}

if [ "$uninstall" = 1 ]; then
  rm -f "$dst_desktop" "$dst_icon" "$dst_meta"
  rm -rf "$dst_licenses"
  echo "removed the desktop entry, the icon and the metainfo from $data"
  if [ "$layout" = tarball ]; then
    rm -f "$bindir/vix-mix"
    echo "removed $bindir/vix-mix"
  else
    echo "the binary is cargo's: run 'cargo uninstall vix-mix' to remove it"
  fi
  refresh
  echo "your setup in ~/.config/vix-mix is kept: delete that folder to forget it"
  exit 0
fi

# The binary: the tarball's, installed under the prefix once its path is known to fit the
# desktop entry (below); or the one cargo installed.
if [ "$layout" = tarball ]; then
  [ -f "$src_bin" ] || die "the tarball has no bin/vix-mix: unpack it again, or build it with scripts/package/linux-tarball.sh"
  exe="$bindir/vix-mix"
else
  exe="$(command -v vix-mix || true)"
  if [ -z "$exe" ] && [ -x "${CARGO_HOME:-$HOME/.cargo}/bin/vix-mix" ]; then
    exe="${CARGO_HOME:-$HOME/.cargo}/bin/vix-mix"
  fi
  [ -n "$exe" ] || die "no vix-mix on PATH or in ~/.cargo/bin: run 'cargo install --path app --locked' in the checkout first, then this script again"
  exe="$(cd "$(dirname "$exe")" && pwd)/$(basename "$exe")"
fi

# Exec takes the absolute path, always double-quoted: inside the quotes every reserved
# character of the Desktop Entry spec (a space, ( ) ' & ; # | < > ~ * ? and the rest) is plain.
# The five that would still need escaping there (", `, $, \ and %) are refused, and so is a
# control character (a tab, a newline), which no value in the file may hold; TryExec takes the
# path as it is.
case "$exe" in
  *[[:cntrl:]]*) die "the path of vix-mix has a control character in it (a tab or a newline), which a desktop entry cannot hold: install to a prefix without one" ;;
  *[\"\`\$\\%]*) die "the path '$exe' has a character a desktop entry cannot hold plainly: install to a prefix without \", \`, \$, \\ or %" ;;
esac
exec_value="\"$exe\""

if [ "$layout" = tarball ]; then
  install -Dm755 "$src_bin" "$exe"
fi

# sed's replacement reads & and the | delimiter specially.
sed_quote() { printf '%s' "$1" | sed -e 's/[&|]/\\&/g'; }
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
sed -e "s|^Exec=.*|Exec=$(sed_quote "$exec_value")|" -e "s|^TryExec=.*|TryExec=$(sed_quote "$exe")|" \
  "$src_desktop" >"$tmp"
install -Dm644 "$tmp" "$dst_desktop"
install -Dm644 "$src_icon" "$dst_icon"
install -Dm644 "$src_meta" "$dst_meta"
if [ -f "$src_ofl" ]; then
  install -Dm644 "$src_ofl" "$dst_licenses/OFL.txt"
fi
refresh

echo "installed Vix Mix: $exe"
echo "  $dst_desktop"
echo "  $dst_icon"
echo "  $dst_meta"
echo "It is in the launcher as Vix Mix (Multimedia)."

# What the binary needs at run time, said now rather than at the first start.
if command -v ldd >/dev/null 2>&1; then
  missing="$(ldd "$exe" 2>/dev/null | awk '/not found/ { print $1 }' || true)"
  if [ -n "$missing" ]; then
    echo "vix-mix cannot start yet: these libraries are missing: $(echo "$missing" | tr '\n' ' ')" >&2
    echo "on Fedora, 'sudo dnf install pipewire-libs' brings libpipewire-0.3" >&2
  fi
fi
case ":$PATH:" in
  *":$(dirname "$exe"):"*) ;;
  *) echo "$(dirname "$exe") is not on PATH: the launcher still starts Vix Mix; add it to PATH to type vix-mix in a terminal" ;;
esac
