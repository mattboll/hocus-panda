#!/bin/sh
# Installs what a package of Hocuspanda holds, from a release build
# already made (`cargo build --release --locked -p hocus-compositor -p
# hocus-shell -p hocus-cli -p hocus-settings`), into
# a staging directory for the .deb, or into the system.
#
#   packaging/install.sh DESTDIR
#
#   PREFIX            where it goes below DESTDIR (default /usr/local)
#   CARGO_TARGET_DIR  where the binaries were built (default target)
set -eu

[ $# -eq 1 ] || { sed -n '2,9p' "$0"; exit 2; }
destdir=$1
here=$(dirname "$0")
prefix=${PREFIX:-/usr/local}
release=${CARGO_TARGET_DIR:-$here/../target}/release

install -D -m 755 "$release/hocuspanda" "$destdir$prefix/bin/hocuspanda"
# The shell (the bar), which the compositor starts from beside itself.
install -D -m 755 "$release/hocuspanda-shell" "$destdir$prefix/bin/hocuspanda-shell"
# The Settings (6.1), an application of their own, in the launcher.
install -D -m 755 "$release/hocus-settings" "$destdir$prefix/bin/hocus-settings"
install -D -m 644 "$here/hocus-settings.desktop" \
    "$destdir$prefix/share/applications/hocus-settings.desktop"
# What agents' hooks and scripts run to tell the bar their state.
install -D -m 755 "$release/hocus" "$destdir$prefix/bin/hocus"
install -D -m 755 "$here/hocuspanda-session" "$destdir$prefix/bin/hocuspanda-session"
# Where display managers (GDM, SDDM, greetd's greeters) look for sessions.
install -D -m 644 "$here/hocuspanda.desktop" \
    "$destdir$prefix/share/wayland-sessions/hocuspanda.desktop"
# Screen sharing's portal (docs/design/sharing.md): the compositor serves
# it; xdg-desktop-portal finds it by these, in its own data directory.
install -D -m 644 "$here/hocuspanda.portal" \
    "$destdir$prefix/share/xdg-desktop-portal/portals/hocuspanda.portal"
install -D -m 644 "$here/hocuspanda-portals.conf" \
    "$destdir$prefix/share/xdg-desktop-portal/hocuspanda-portals.conf"
# Every setting with its default, to copy to ~/.config/hocuspanda/.
install -D -m 644 "$here/config.toml" "$destdir$prefix/share/doc/hocuspanda/config.toml"
