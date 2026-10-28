#!/bin/sh
# Installs what a package of Hocuspanda holds, from a release build
# already made (`cargo build --release --locked -p hocus-compositor -p
# hocus-shell`), into
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
install -D -m 755 "$here/hocuspanda-session" "$destdir$prefix/bin/hocuspanda-session"
# Where display managers (GDM, SDDM, greetd's greeters) look for sessions.
install -D -m 644 "$here/hocuspanda.desktop" \
    "$destdir$prefix/share/wayland-sessions/hocuspanda.desktop"
# Every setting with its default, to copy to ~/.config/hocuspanda/.
install -D -m 644 "$here/config.toml" "$destdir$prefix/share/doc/hocuspanda/config.toml"
