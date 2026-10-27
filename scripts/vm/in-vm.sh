#!/bin/sh
# Inside the VM (copied there as ~/hocus-vm): hocuspanda as the session on
# tty1, or headless with the VM's own services, driven over its control
# socket.
#
#   hocus-vm session                 start the session on tty1 (the screen)
#   hocus-vm start [extra options]   start it headless (logs in ~/hocus.log)
#   hocus-vm msg <request…>          `hocus msg` to the one running
#   hocus-vm lease <command…>        `hocus-lease` (drm-lease) against it
#   hocus-vm headset                 the second GPU's screen turns into a VR
#                                    headset (HOCUS_VM_GPUS=2): its connector
#                                    gets a headset's EDID, the GPU goes and
#                                    comes back; `desktop` undoes it
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

# A VR headset's EDID: maker HVR, product 0xaa01, which the kernel's quirk
# list says is no desktop; one 1024x768@60 timing.
HEADSET_EDID=00ffffffffffff0022d201aa01000000011e0103800a06780aee91a3544c99260f5054000000010101010101010101010101010101016419004041002630188836002c1a00000018000000fc00486561647365740a202020202000000010000000000000000000000000000000000010000000000000000000000000000000e1

# The second GPU's virtio device (virtio1…): the one that does not boot.
second_gpu() {
    for card in /sys/class/drm/card[0-9]*; do
        case "$card" in *-*) continue ;; esac
        pci=$(readlink -f "$card/device")
        [ "$(cat "$pci/boot_vga" 2>/dev/null || echo 0)" = 1 ] && continue
        for virtio in "$pci"/virtio*; do
            basename "$virtio"
            return
        done
    done
    echo "no second GPU (HOCUS_VM_GPUS=2)" >&2
    exit 1
}

# The second GPU goes and comes back, reading its screen's EDID again.
replug_second_gpu() {
    gpu=$(second_gpu)
    echo "$gpu" | sudo tee /sys/bus/virtio/drivers/virtio_gpu/unbind > /dev/null
    sleep 2
    echo "$gpu" | sudo tee /sys/bus/virtio/drivers/virtio_gpu/bind > /dev/null
}

case "${1:-}" in
    session)
        rm -f ~/hocus.socket
        touch ~/.hocus-session
        # The autologin on tty1 runs `hocus-vm run` (~/.bash_profile).
        sudo systemctl restart getty@tty1
        # Its control socket, next to the Wayland one it picked: newer than
        # the flag, not one a session before left.
        i=0
        until f=$(ls -t "$XDG_RUNTIME_DIR"/*.api 2>/dev/null | head -n 1) && [ -S "$f" ] \
            && [ "$f" -nt ~/.hocus-session ]; do
            i=$((i + 1)); [ "$i" -lt 100 ] || { tail ~/hocus.log; exit 1; }
            sleep 0.2
        done
        echo "$f" > ~/hocus.socket
        echo "session started on tty1 (pid $(cat ~/hocus.pid))" ;;
    run)
        # On tty1, from the login shell: the session itself. What it says
        # on stderr is kept, with backtraces, should it end (#260).
        echo $$ > ~/hocus.pid
        systemctl --user start dbus.socket
        RUST_BACKTRACE=1 exec ~/bin/hocuspanda --backend tty --api-permission test \
            --log-file ~/hocus.log --spawn foot > /dev/null 2> ~/hocus.stderr ;;
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
    lease)
        shift
        # The Wayland socket the control socket is named after.
        WAYLAND_DISPLAY=$(basename "$(cat ~/hocus.socket)" .api) exec ~/bin/hocus-lease "$@" ;;
    headset)
        sudo mkdir -p /lib/firmware/edid
        python3 -c 'import sys; sys.stdout.buffer.write(bytes.fromhex(sys.argv[1]))' "$HEADSET_EDID" \
            | sudo tee /lib/firmware/edid/headset.bin > /dev/null
        # Every connector that reads an EDID: the first GPU reads none.
        echo edid/headset.bin | sudo tee /sys/module/drm/parameters/edid_firmware > /dev/null
        replug_second_gpu
        echo "the second GPU's screen is a headset now" ;;
    desktop)
        printf '\n' | sudo tee /sys/module/drm/parameters/edid_firmware > /dev/null
        replug_second_gpu
        echo "the second GPU's screen is a screen again" ;;
    stop)
        # The session would start again on tty1 without this.
        rm -f ~/.hocus-session
        kill "$(cat ~/hocus.pid)" && echo stopped ;;
    *) sed -n '2,15p' "$0"; exit 2 ;;
esac
