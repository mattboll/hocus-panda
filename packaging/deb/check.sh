#!/bin/sh
# Checks that a .deb holds the files packaging/README.md lists, no more and
# no fewer, and that its programs can be run. CI runs it on the package it
# builds from the release build.
#
#   packaging/deb/check.sh PACKAGE.deb
set -eu

[ $# -eq 1 ] || { sed -n '2,6p' "$0"; exit 2; }
deb=$1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cat > "$work/expected" <<LIST
-rw-r--r-- ./etc/pam.d/hocuspanda
-rwxr-xr-x ./usr/bin/hocus
-rwxr-xr-x ./usr/bin/hocus-secretary
-rwxr-xr-x ./usr/bin/hocus-settings
-rwxr-xr-x ./usr/bin/hocuspanda
-rwxr-xr-x ./usr/bin/hocuspanda-session
-rwxr-xr-x ./usr/bin/hocuspanda-shell
-rw-r--r-- ./usr/share/applications/hocus-settings.desktop
-rw-r--r-- ./usr/share/doc/hocuspanda/config.toml
-rw-r--r-- ./usr/share/doc/hocuspanda/copyright
-rw-r--r-- ./usr/share/hocuspanda/avatars/panda-1.png
-rw-r--r-- ./usr/share/hocuspanda/avatars/panda-2.png
-rw-r--r-- ./usr/share/hocuspanda/avatars/panda-3.png
-rw-r--r-- ./usr/share/hocuspanda/avatars/panda-4.png
-rw-r--r-- ./usr/share/hocuspanda/avatars/panda-5.png
-rw-r--r-- ./usr/share/hocuspanda/avatars/panda-6.png
-rw-r--r-- ./usr/share/hocuspanda/avatars/panda-7.png
-rw-r--r-- ./usr/share/hocuspanda/avatars/panda-8.png
-rw-r--r-- ./usr/share/wayland-sessions/hocuspanda.desktop
-rw-r--r-- ./usr/share/xdg-desktop-portal/hocuspanda-portals.conf
-rw-r--r-- ./usr/share/xdg-desktop-portal/portals/hocuspanda.portal
LIST

# Its files (not its directories), each with its mode, owned by root.
dpkg-deb --contents "$deb" > "$work/contents"
if grep -v ' root/root ' "$work/contents"; then
    echo "$deb: files not owned by root" >&2
    exit 1
fi
grep -v '^d' "$work/contents" | awk '{ print $1, $6 }' | LC_ALL=C sort -k 2 > "$work/found"
LC_ALL=C sort -k 2 -o "$work/expected" "$work/expected"
if ! diff -u "$work/expected" "$work/found"; then
    echo "$deb: not the files expected (-), see packaging/README.md and packaging/deb/check.sh" >&2
    exit 1
fi

# The lock's PAM service is a conffile, and the package says what it needs.
dpkg-deb --control "$deb" "$work/control"
grep -qx '/etc/pam.d/hocuspanda' "$work/control/conffiles" || {
    echo "$deb: /etc/pam.d/hocuspanda is not a conffile" >&2
    exit 1
}
for field in Package Version Architecture Depends; do
    [ -n "$(dpkg-deb --field "$deb" "$field")" ] || {
        echo "$deb: no $field" >&2
        exit 1
    }
done
echo "$deb: $(wc -l < "$work/found") files, as expected"
