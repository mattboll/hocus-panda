#!/bin/sh
# Installs what a package of Hocuspanda holds, from release binaries
# already built (`cargo build --release -p hocuspanda -p hocus
# -p hocus-settings`), into a staging directory for the .deb and the .rpm,
# or into the system.
#
#   packaging/install.sh [--pam debian|redhat] DESTDIR
#
#   PREFIX            where it goes below DESTDIR (default /usr/local)
#   SYSCONFDIR        where the PAM service goes (default /etc)
#   CARGO_TARGET_DIR  where the binaries were built (default target)
#
# The PAM service is only installed when asked: its stack differs between
# distributions, and the lock screen falls back to `login` without it.
set -eu

pam=
while [ $# -gt 1 ]; do
    case "$1" in
        --pam) pam=$2; shift 2 ;;
        *) break ;;
    esac
done
[ $# -eq 1 ] || { sed -n '2,15p' "$0"; exit 2; }
destdir=$1
here=$(dirname "$0")
prefix=${PREFIX:-/usr/local}
sysconfdir=${SYSCONFDIR:-/etc}
release=${CARGO_TARGET_DIR:-target}/release

# The desktop, its command-line tool and its settings app; the test tools
# of hocus-test (hocus-scenario, hocus-test-client) stay out.
for program in hocuspanda hocus hocus-settings; do
    install -D -m 755 "$release/$program" "$destdir$prefix/bin/$program"
done
install -D -m 644 "$here/hocuspanda.desktop" \
    "$destdir$prefix/share/wayland-sessions/hocuspanda.desktop"
case "$pam" in
    '') ;;
    debian | redhat)
        install -D -m 644 "$here/pam/$pam" "$destdir$sysconfdir/pam.d/hocuspanda" ;;
    *) echo "unknown PAM flavour: $pam" >&2; exit 2 ;;
esac
