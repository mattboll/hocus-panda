#!/bin/sh
# Installs what a package of Hocuspanda holds, from a release build
# already made (`cargo build --release --locked -p hocus-compositor -p
# hocus-shell -p hocus-cli -p hocus-settings -p hocus-secretary`), into
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
# The secretary (docs/design/secretary.md), which the compositor starts from
# beside itself in `[secretary] mode = "assistant"` only.
install -D -m 755 "$release/hocus-secretary" "$destdir$prefix/bin/hocus-secretary"
# The Settings (6.1), an application of their own, in the launcher.
install -D -m 755 "$release/hocus-settings" "$destdir$prefix/bin/hocus-settings"
install -D -m 644 "$here/hocus-settings.desktop" \
    "$destdir$prefix/share/applications/hocus-settings.desktop"
# The pandas the Compte page offers as avatars (6.11).
for panda in "$here"/../crates/hocus-settings/data/avatars/panda-[1-8].png; do
    install -D -m 644 "$panda" \
        "$destdir$prefix/share/hocuspanda/avatars/$(basename "$panda")"
done
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
# The lock screen's PAM service (docs/design/lock.md), always under /etc.
install -D -m 644 "$here/pam-hocuspanda" "$destdir/etc/pam.d/hocuspanda"
# Every setting with its default, to copy to ~/.config/hocuspanda/.
install -D -m 644 "$here/config.toml" "$destdir$prefix/share/doc/hocuspanda/config.toml"
