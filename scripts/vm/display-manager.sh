#!/bin/sh
# The .deb against GDM, in the Debian VM (scripts/vm/debian.sh): installs
# GDM with a GNOME session beside (with a single session, GDM shows no
# chooser), then the package, and logs in as a person would: alex, the
# "Hocuspanda" session picked in GDM's chooser, the password. Checks that
# the session started on the seat from GDM.
#
#   scripts/vm/debian.sh up
#   packaging/deb/build.sh                    (prints the package's path)
#   scripts/vm/display-manager.sh DEB [CHOOSER.png [SESSION.png]]
#
# CHOOSER.png is GDM's session list, SESSION.png the session once started.
# The session's log: scripts/vm/debian.sh ssh 'journalctl --user -t hocuspanda -b'
set -eu

[ $# -ge 1 ] || { sed -n '2,14p' "$0"; exit 2; }
deb=$1
chooser=${2:-}
shot=${3:-}
vm="$(dirname "$0")/debian.sh"

"$vm" ssh 'sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
    gdm3 gnome-session > /dev/null 2>&1
# GDM greets on tty1, where the VM logs alex in otherwise.
sudo rm -f /etc/systemd/system/getty@tty1.service.d/autologin.conf
sudo systemctl daemon-reload
sudo systemctl stop getty@tty1'
"$vm" install "$deb" > /dev/null
# GDM reads the sessions as its greeter starts.
"$vm" ssh 'sudo systemctl restart gdm3'
echo "GDM restarted"
sleep 10

# The keys are a US keyboard's: with another layout in the VM's
# /etc/default/keyboard, GDM reads the password differently.
# The user, then the chooser (the gear, after the password field), its
# second entry (GNOME comes first), back to the password.
"$vm" keys ret
sleep 1.5
"$vm" keys tab
sleep 0.5
"$vm" keys spc
sleep 1
"$vm" keys down
sleep 0.3
"$vm" keys down
[ -z "$chooser" ] || { sleep 0.5; "$vm" screen "$chooser"; }
"$vm" keys ret
sleep 0.5
"$vm" keys shift-tab
sleep 0.3
"$vm" keys p a n d a ret

# Started by GDM on the seat, as alex, with the compositor running.
started() {
    "$vm" ssh sh -s 2> /dev/null << 'EOF'
s=$(loginctl list-sessions --no-legend | awk '$3 == "alex" && $4 == "seat0" { print $1 }')
[ -n "$s" ] &&
[ "$(loginctl show-session "$s" -p Service --value)" = gdm-password ] &&
[ "$(loginctl show-session "$s" -p Type --value)" = wayland ] &&
[ "$(loginctl show-session "$s" -p Active --value)" = yes ] &&
pgrep -u alex -x hocuspanda > /dev/null
EOF
}
i=0
until started; do
    i=$((i + 1))
    if [ "$i" -ge 60 ]; then
        echo "no session after a minute; its log:"
        "$vm" ssh 'journalctl --user -t hocuspanda -b --no-pager | tail -n 20' || true
        exit 1
    fi
    sleep 1
done
"$vm" ssh 'loginctl list-sessions --no-legend | grep seat0; pgrep -a -u alex -x hocuspanda'
echo "the session started from GDM"
if [ -n "$shot" ]; then
    sleep 2
    "$vm" screen "$shot"
fi
