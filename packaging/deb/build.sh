#!/bin/sh
# Builds the .deb from a release build already made, with dpkg-deb: its
# library dependencies are the ones dpkg-shlibdeps finds on this system, so
# the package installs on this release and later ones.
#
#   cargo build --release --locked -p hocus-compositor -p hocus-shell
#   packaging/deb/build.sh [OUTDIR]     (default: CARGO_TARGET_DIR/packages)
#
# Needs dpkg-dev; the binaries are read from CARGO_TARGET_DIR/release. Prints
# the package's path.
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

PREFIX=/usr CARGO_TARGET_DIR="$target" "$top/packaging/install.sh" "$root"

# Debian's place for the licence, in its machine-readable format.
{
    printf 'Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n'
    printf 'Upstream-Name: Hocuspanda\n'
    printf 'Source: https://github.com/mattboll/hocus-panda\n\n'
    printf 'Files: *\n'
    printf 'Copyright: %s\n' "$(sed -n 's/^Copyright (c) //p' "$top/LICENSE")"
    printf 'License: Expat\n'
    # The licence's text, indented, with its blank lines as " .".
    sed -e '1,/^Copyright/d' -e 's/^$/./' -e 's/^/ /' "$top/LICENSE" | sed '1{/^ \.$/d}'
    # The fonts the shell carries in its binary, under the SIL Open Font
    # License: each one's copyright line, then the licence once.
    fonts="$top/crates/hocus-shell/fonts"
    for family in MartianMono Manrope; do
        printf '\nFiles: crates/hocus-shell/fonts/%s-*\n' "$family"
        printf 'Copyright: %s\n' "$(sed -n '1s/^Copyright //p' "$fonts/$family-OFL.txt")"
        printf 'License: OFL-1.1\n'
    done
    printf '\nLicense: OFL-1.1\n'
    sed -n -e 's/\r$//' -e '/^-----/,$p' "$fonts/MartianMono-OFL.txt" \
        | sed -e 's/^$/./' -e 's/^/ /'
} > "$root/usr/share/doc/hocuspanda/copyright"

# dpkg-shlibdeps runs in a source tree that names the package. EGL is
# opened at run time: no binary links it, it is added below.
mkdir -p "$root/DEBIAN"
printf 'Source: hocuspanda\n\nPackage: hocuspanda\nArchitecture: any\n' \
    > "$work/src/debian/control"
(cd "$work/src" && dpkg-shlibdeps -O debian/hocuspanda/usr/bin/hocuspanda \
    debian/hocuspanda/usr/bin/hocuspanda-shell) > "$work/shlibs"
depends=$(sed -n 's/^shlibs:Depends=//p' "$work/shlibs")

# Recommends: Mesa's drivers (the shell draws with Vulkan, or on the CPU
# without it), X11 applications, a session bus shared with systemd (what
# the session tells it reaches D-Bus-activated programs), and a terminal
# for Super+Return.
cat > "$root/DEBIAN/control" <<CONTROL
Package: hocuspanda
Version: $version-$revision
Architecture: $arch
Maintainer: Matthieu Bollot <m.bollot@code-troopers.com>
Installed-Size: $(du -sk --exclude=DEBIAN "$root" | cut -f 1)
Depends: $depends, libegl1
Recommends: libegl-mesa0, libgl1-mesa-dri, mesa-vulkan-drivers, xwayland,
 dbus-user-session,
 kitty | foot | alacritty | xterm
Section: x11
Priority: optional
Homepage: https://github.com/mattboll/hocus-panda
Description: Wayland desktop for developers who work with AI agents
 Hocuspanda is a Wayland desktop on smithay: floating windows that snap,
 virtual desktops, and the state of the user's AI agents at a glance.
 Display managers offer it as the "Hocuspanda" session.
 .
 This early version holds the compositor, its session and the base of its
 bar; Super+Return opens a terminal, Super+B switches the bar's form and
 Super+Shift+E logs out.
CONTROL

mkdir -p "$out"
deb="$out/hocuspanda_$version-${revision}_$arch.deb"
dpkg-deb --root-owner-group --build "$root" "$deb" > /dev/null
echo "$deb"
