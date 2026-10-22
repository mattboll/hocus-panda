#!/bin/sh
# The .deb against a display manager, in the Debian VM (scripts/vm/debian.sh):
# installs the package and greetd with its text greeter (tuigreet) on tty7,
# then logs in as a person would, choosing the "Hocuspanda" session from the
# greeter's list (what the package's session file put there), and checks
# that the session started on the seat.
#
#   scripts/vm/debian.sh up
#   packaging/deb/build.sh
#   scripts/vm/display-manager.sh target/packages/hocuspanda_*.deb [SHOT.png]
#
# On a VM fresh from `up`. SHOT.png is what its screen shows once the
# session runs.
set -eu

[ $# -ge 1 ] || { sed -n '2,13p' "$0"; exit 2; }
deb=$1
shot=${2:-}
vm="$(dirname "$0")/debian.sh"

"$vm" install "$deb" > /dev/null
"$vm" ssh 'sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q greetd tuigreet > /dev/null
sudo tee /etc/greetd/config.toml > /dev/null <<EOF
[terminal]
vt = 7

[default_session]
command = "tuigreet --time --asterisks --sessions /usr/share/wayland-sessions"
user = "_greetd"
EOF
sudo systemctl restart greetd'
echo "greeter on tty7"
sleep 3

# The sessions menu, its first entry (the only one: no other desktop is
# installed), then the user and the password.
"$vm" keys f3
sleep 0.5
"$vm" keys ret
sleep 0.5
"$vm" keys a l e x ret
sleep 1
"$vm" keys p a n d a ret

# Started by greetd on the seat, as alex, with its control socket answering.
started() {
    "$vm" ssh sh -s 2>/dev/null <<'EOF'
s=$(loginctl list-sessions --no-legend | awk '/ tty7 / { print $1 }')
[ -n "$s" ] &&
[ "$(loginctl show-session "$s" -p Name --value)" = alex ] &&
[ "$(loginctl show-session "$s" -p Service --value)" = greetd ] &&
[ "$(loginctl show-session "$s" -p Type --value)" = wayland ] &&
[ "$(loginctl show-session "$s" -p Active --value)" = yes ] &&
hocus msg --socket "$(ls "$XDG_RUNTIME_DIR"/hocus-*.api | head -n 1)" outputs > /dev/null
EOF
}
i=0
until started; do
    i=$((i + 1))
    if [ "$i" -ge 60 ]; then
        echo "no session after a minute; its log:"
        "$vm" ssh 'tail -n 20 ~/.local/state/hocuspanda/hocuspanda.log' || true
        exit 1
    fi
    sleep 1
done
"$vm" ssh 'loginctl list-sessions --no-legend | grep " tty7 "; pgrep -a -u alex -x hocuspanda'
echo "the session started from the display manager"
if [ -n "$shot" ]; then
    # The shell's first frames.
    sleep 2
    "$vm" screen "$shot"
fi
