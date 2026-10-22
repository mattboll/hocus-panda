#!/bin/sh
# Inside the VM (copied there as ~/hocus-vm): hocuspanda as the session on
# tty1, or headless with the VM's own services, driven over its control
# socket.
#
#   hocus-vm session                 start the session on tty1 (the screen)
#   hocus-vm start [extra options]   start it headless (logs in ~/hocus.log)
#   hocus-vm msg <request…>          `hocus msg` to the one running
#   hocus-vm stop                    stop it
set -eu
export XDG_RUNTIME_DIR=/run/user/"$(id -u)"
export DBUS_SESSION_BUS_ADDRESS=unix:path="$XDG_RUNTIME_DIR"/bus

wait_socket() {
    i=0
    until [ -S "$(cat ~/hocus.socket 2>/dev/null)" ]; do
        i=$((i + 1)); [ "$i" -lt 100 ] || { tail ~/hocus.log; exit 1; }
        sleep 0.2
    done
}

case "${1:-}" in
    session)
        rm -f ~/hocus.socket
        touch ~/.hocus-session
        # The autologin on tty1 runs `hocus-vm run` (~/.bash_profile).
        sudo systemctl restart getty@tty1
        # Its control socket, next to the Wayland one it picked.
        i=0
        until f=$(ls -t "$XDG_RUNTIME_DIR"/*.api 2>/dev/null | head -n 1) && [ -S "$f" ]; do
            i=$((i + 1)); [ "$i" -lt 100 ] || { tail ~/hocus.log; exit 1; }
            sleep 0.2
        done
        echo "$f" > ~/hocus.socket
        echo "session started on tty1 (pid $(cat ~/hocus.pid))" ;;
    run)
        # On tty1, from the login shell: the session itself.
        echo $$ > ~/hocus.pid
        systemctl --user start dbus.socket
        exec ~/bin/hocuspanda --backend tty --api-permission test \
            --log-file ~/hocus.log --spawn foot > /dev/null 2>&1 ;;
    start)
        shift
        # The user's bus, installed after the user's manager started.
        systemctl --user start dbus.socket
        dir=$(mktemp -d /tmp/hp-XXXX)
        nohup ~/bin/hocuspanda --backend headless --output 1280x800 --renderer pixman \
            --runtime-dir "$dir" --api-permission test --session-services \
            --session-bus "$DBUS_SESSION_BUS_ADDRESS" --no-xwayland --no-log-file "$@" \
            > ~/hocus.log 2>&1 &
        echo $! > ~/hocus.pid
        echo "$dir/hocus-0.api" > ~/hocus.socket
        wait_socket
        echo "started (pid $(cat ~/hocus.pid))" ;;
    msg) shift; exec ~/bin/hocus msg --socket "$(cat ~/hocus.socket)" "$@" ;;
    stop)
        # The session would start again on tty1 without this.
        rm -f ~/.hocus-session
        kill "$(cat ~/hocus.pid)" && echo stopped ;;
    *) sed -n '2,10p' "$0"; exit 2 ;;
esac
