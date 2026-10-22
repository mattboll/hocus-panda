#!/bin/sh
# Builds the .deb from release binaries already built, with dpkg-deb: its
# library dependencies are the ones dpkg-shlibdeps finds on this system, so
# the package installs on this release and later ones.
#
#   packaging/deb/build.sh [OUTDIR]     (default: CARGO_TARGET_DIR/packages)
#
# Needs dpkg-dev; the binaries are read from CARGO_TARGET_DIR/release.
set -eu
umask 022

top=$(cd "$(dirname "$0")/../.." && pwd)
target=$(realpath -m "${CARGO_TARGET_DIR:-$top/target}")
out=$(realpath -m "${1:-$target/packages}")
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$top/Cargo.toml" | head -n 1)
revision=1
arch=$(dpkg --print-architecture)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Where dpkg-shlibdeps expects a package's files, in a source tree of its
# own.
root="$work/src/debian/hocuspanda"

PREFIX=/usr CARGO_TARGET_DIR="$target" "$top/packaging/install.sh" --pam debian "$root"

# Debian's place for the licence, in its machine-readable format.
mkdir -p "$root/usr/share/doc/hocuspanda"
{
    printf 'Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n'
    printf 'Upstream-Name: Hocuspanda\n'
    printf 'Source: https://github.com/mattboll/hocus-panda\n\n'
    printf 'Files: *\n'
    printf 'Copyright: %s\n' "$(sed -n 's/^Copyright (c) //p' "$top/LICENSE")"
    printf 'License: Expat\n'
    # The licence's text, indented, with its blank lines as " .".
    sed -e '1,/^Copyright/d' -e 's/^$/./' -e 's/^/ /' "$top/LICENSE" | sed '1{/^ \.$/d}'
} > "$root/usr/share/doc/hocuspanda/copyright"

# dpkg-shlibdeps runs in a source tree that names the package. EGL and
# fontconfig are opened at run time: no binary links them, they are added
# below.
mkdir -p "$root/DEBIAN"
printf 'Source: hocuspanda\n\nPackage: hocuspanda\nArchitecture: any\n' \
    > "$work/src/debian/control"
(cd "$work/src" && dpkg-shlibdeps -O \
    debian/hocuspanda/usr/bin/hocuspanda debian/hocuspanda/usr/bin/hocus \
    debian/hocuspanda/usr/bin/hocus-settings) > "$work/shlibs"
depends=$(sed -n 's/^shlibs:Depends=//p' "$work/shlibs")

cat > "$root/DEBIAN/control" <<EOF
Package: hocuspanda
Version: $version-$revision
Architecture: $arch
Maintainer: Matthieu Bollot <m.bollot@code-troopers.com>
Installed-Size: $(du -sk --exclude=DEBIAN "$root" | cut -f 1)
Depends: $depends, libegl1, libgles2, libfontconfig1
Recommends: libegl-mesa0, libgl1-mesa-dri, dbus-user-session, foot, xwayland,
 fonts-noto-color-emoji
Section: x11
Priority: optional
Homepage: https://github.com/mattboll/hocus-panda
Description: Wayland desktop: compositor and shell in one process
 Hocuspanda is a Wayland compositor and its whole shell (widget column,
 launcher, notifications, quick settings, lock screen) in a single Rust
 process. Display managers offer it as the "Hocuspanda" session.
 .
 This package holds the desktop (hocuspanda), its command-line tool
 (hocus) and its settings app (hocus-settings).
EOF
# The PAM service is the administrator's to change.
printf '/etc/pam.d/hocuspanda\n' > "$root/DEBIAN/conffiles"

mkdir -p "$out"
deb="$out/hocuspanda_$version-${revision}_$arch.deb"
dpkg-deb --root-owner-group --build "$root" "$deb" >/dev/null
echo "$deb"
