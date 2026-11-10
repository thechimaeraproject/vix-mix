# Vix Mix's RPM. scripts/package/rpm.sh builds it (`dev rpm`): it makes the source tarball with
# every crate vendored (the kit's are private, so the build reads nothing from the network) and
# runs rpmbuild with the version from Cargo.toml (`vixmix_version`) and, for a build of a commit
# rather than a release, a snapshot (`vixmix_snapshot`: the commit's UTC time and short id, so
# each newer build upgrades the last). The build uses the toolchain rust-toolchain.toml pins,
# through rustup, not Fedora's packaged Rust, which may be older than the pin.
#
# It installs what packaging/linux/install.sh installs, in Fedora's places: the binary in
# /usr/bin, the desktop entry, the icon and the AppStream metainfo under /usr/share, the fonts'
# licence as the package's licence. The binary links glibc, libgcc_s and libpipewire-0.3, which
# rpm finds as requirements itself.

%{!?vixmix_version:%{error:vixmix_version is not set: build with scripts/package/rpm.sh, which reads it from Cargo.toml}}

%global app_id org.chimaeraproject.vixmix
# Rust's own debug information is not split out the way debugedit expects.
%global debug_package %{nil}

Name:           vix-mix
Version:        %{vixmix_version}
Release:        1%{?vixmix_snapshot:.%{vixmix_snapshot}}%{?dist}
Summary:        Mix microphones and apps through VST3 plugins into a Monitor and a Master mix
# Vix Mix is MIT; the IBM Plex fonts built into the binary are OFL-1.1. The vendored crates
# carry their own licences (mostly MIT OR Apache-2.0).
License:        MIT AND OFL-1.1
URL:            https://github.com/thechimaeraproject/vix-mix
Source0:        %{name}-%{version}.tar.gz

ExclusiveArch:  x86_64 aarch64

BuildRequires:  gcc
BuildRequires:  clang-devel
BuildRequires:  pipewire-devel
BuildRequires:  pkgconf-pkg-config
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

Requires:       pipewire
Requires:       wireplumber
Requires:       hicolor-icon-theme

%description
Vix Mix is a mixer for the Linux desktop in the manner of Wave Link: up to eight input
channels (a hardware input or a group of apps), a VST3 chain on each, two sends per channel
to a Monitor and a Master mix, each mix with its own chain and output device, and Master
published as a virtual microphone. It runs on PipeWire.

%prep
%autosetup -n %{name}-%{version}

%build
if ! command -v cargo >/dev/null; then
  echo "no cargo on PATH: install rustup (sudo dnf install rustup && rustup-init -y), then build with scripts/package/rpm.sh" >&2
  exit 1
fi
cargo build --release --frozen --offline -p vix-mix

%install
install -Dm755 target/release/vix-mix %{buildroot}%{_bindir}/vix-mix
install -Dm644 packaging/linux/%{app_id}.desktop %{buildroot}%{_datadir}/applications/%{app_id}.desktop
install -Dm644 packaging/linux/%{app_id}.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
install -Dm644 packaging/linux/%{app_id}.metainfo.xml %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{app_id}.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml

%files
%license app/assets/fonts/OFL.txt
%{_bindir}/vix-mix
%{_datadir}/applications/%{app_id}.desktop
%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
%{_metainfodir}/%{app_id}.metainfo.xml

%changelog
* Tue Nov 10 2026 The Chimaera Project - %{vixmix_version}-1
- Built from the commit the snapshot names; the release notes are the metainfo's <releases>.
