#!/bin/sh
# Inside the VM (copied there as ~/hocus-vm): a headless hocuspanda using the
# VM's own services, driven over its control socket.
#
#   hocus-vm start [extra options]   start it (logs in ~/hocus.log)
#   hocus-vm msg <request…>          `hocus msg` to it
#   hocus-vm stop                    stop it
set -eu
export XDG_RUNTIME_DIR=/run/user/"$(id -u)"
export DBUS_SESSION_BUS_ADDRESS=unix:path="$XDG_RUNTIME_DIR"/bus
case "${1:-}" in
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
        i=0
        until [ -S "$dir/hocus-0.api" ]; do
            i=$((i + 1)); [ "$i" -lt 50 ] || { tail ~/hocus.log; exit 1; }
            sleep 0.2
        done
        echo "started (pid $(cat ~/hocus.pid))" ;;
    msg) shift; exec ~/bin/hocus msg --socket "$(cat ~/hocus.socket)" "$@" ;;
    stop) kill "$(cat ~/hocus.pid)" && echo stopped ;;
    *) sed -n '2,8p' "$0"; exit 2 ;;
esac
