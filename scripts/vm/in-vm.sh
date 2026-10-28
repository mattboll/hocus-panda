#!/bin/sh
# Inside the VM (copied there as ~/hocus-vm): hocuspanda as the session on
# tty1.
#
#   hocus-vm session    start the session on tty1 (the screen), with foot
#   hocus-vm log        what it said (its stderr)
#   hocus-vm stop       stop it
set -eu
export XDG_RUNTIME_DIR=/run/user/"$(id -u)"

case "${1:-}" in
    session)
        touch ~/.hocus-session
        # The autologin on tty1 runs `hocus-vm run` (~/.bash_profile).
        sudo systemctl restart getty@tty1
        # Its Wayland socket: newer than the flag, not one a session before
        # left.
        i=0
        until f=$(ls -t "$XDG_RUNTIME_DIR"/hocus-[0-9]* 2>/dev/null | grep -v lock | head -n 1) \
            && [ -S "$f" ] && [ "$f" -nt ~/.hocus-session ]; do
            i=$((i + 1)); [ "$i" -lt 100 ] || { tail ~/hocus.log; exit 1; }
            sleep 0.2
        done
        echo "session started on tty1 (pid $(cat ~/hocus.pid), socket $f)" ;;
    run)
        # On tty1, from the login shell: the session itself. What it says
        # on stderr is kept, with backtraces, should it end.
        echo $$ > ~/hocus.pid
        systemctl --user start dbus.socket
        RUST_BACKTRACE=1 exec ~/bin/hocuspanda --backend tty --spawn foot \
            > /dev/null 2> ~/hocus.log ;;
    log) exec cat ~/hocus.log ;;
    stop)
        # The session would start again on tty1 without this.
        rm -f ~/.hocus-session
        kill "$(cat ~/hocus.pid)" && echo stopped ;;
    *) sed -n '2,8p' "$0"; exit 2 ;;
esac
