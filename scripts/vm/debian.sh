#!/bin/sh
# A Debian testing VM (QEMU, KVM) to try hocuspanda against a real system:
# the session on a virtual terminal (DRM/KMS on virtio-gpu, drawn by Mesa's
# software GL; libinput; logind), PAM, UPower, NetworkManager, power
# profiles, PipeWire, autostart. Nothing of the host's session is used: the
# VM has its own user, buses and services.
#
#   scripts/vm/debian.sh up            boot it (downloads the image once)
#   scripts/vm/debian.sh copy          copy target/debug's programs into it
#   scripts/vm/debian.sh install DEB   install a .deb (packaging/deb/build.sh)
#   scripts/vm/debian.sh ssh [cmd…]    a shell, or a command, as the test user
#   scripts/vm/debian.sh screen FILE [N]
#                                      save what the VM's screen shows (PNG);
#                                      N=1: the second GPU's (HOCUS_VM_GPUS=2)
#   scripts/vm/debian.sh keys K…       press keys on its keyboard (QEMU names:
#                                      ctrl-alt-f2, a, ret…)
#   scripts/vm/debian.sh mouse DX DY   move its mouse by so many pixels
#   scripts/vm/debian.sh click         click its mouse's left button
#   scripts/vm/debian.sh monitor CMD   a QEMU monitor command
#   scripts/vm/debian.sh down          stop it
#
# The base image is cached in $XDG_CACHE_HOME/hocuspanda-vm; each `up`
# starts from a fresh overlay. The test user is `alex`, password `panda`.
# HOCUS_VM_GPUS=2 boots it with a second GPU (a virtio-gpu of its own, with
# its own screen): the first has no EDID then, so that the EDID of a VR
# headset given to the second's connector (`~/hocus-vm headset`) is not
# taken for the first's too.
# HOCUS_VM_DISPLAY=gtk shows its screens in a window of the current
# session, its keyboard and mouse for a person to try it by hand (any QEMU
# display backend; `none` by default).
set -eu

CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/hocuspanda-vm"
WORK="$CACHE/debian"
IMAGE_URL="https://cloud.debian.org/images/cloud/forky/daily/latest/debian-14-generic-amd64-daily.qcow2"
BASE="$CACHE/debian-14-generic-amd64.qcow2"
SSH_PORT="${HOCUS_VM_SSH_PORT:-2522}"
HTTP_PORT="${HOCUS_VM_HTTP_PORT:-2580}"
MEMORY="${HOCUS_VM_MEMORY:-4G}"
CPUS="${HOCUS_VM_CPUS:-4}"
GPUS="${HOCUS_VM_GPUS:-1}"
DISPLAY_BACKEND="${HOCUS_VM_DISPLAY:-none}"

# Input events to the VM's devices, as a person would make them (QMP: the
# monitor's mouse_move reaches no device without a display).
qmp() {
    printf '{"execute":"qmp_capabilities"}\n{"execute":"input-send-event","arguments":{"events":[%s]}}\n' "$1" \
        | socat -t 0.2 - UNIX-CONNECT:"$WORK/qmp" >/dev/null
}

# A QMP command, its answer on stdout.
qmp_command() {
    printf '{"execute":"qmp_capabilities"}\n%s\n' "$1" \
        | socat -t 2 - UNIX-CONNECT:"$WORK/qmp"
}

ssh_vm() {
    ssh -q -i "$WORK/key" -p "$SSH_PORT" \
        -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -o ConnectTimeout=5 alex@127.0.0.1 "$@"
}

seed() {
    mkdir -p "$WORK/seed"
    [ -f "$WORK/key" ] || ssh-keygen -q -t ed25519 -N '' -C hocuspanda-vm -f "$WORK/key"
    printf 'instance-id: hocuspanda-%s\nlocal-hostname: hocuspanda-vm\n' "$(date +%s)" \
        > "$WORK/seed/meta-data"
    : > "$WORK/seed/vendor-data"
    cat > "$WORK/seed/user-data" <<EOF
#cloud-config
users:
  - name: alex
    gecos: Alex
    shell: /bin/bash
    groups: [sudo, video, input, render, audio]
    sudo: ALL=(ALL) NOPASSWD:ALL
    lock_passwd: false
    ssh_authorized_keys:
      - $(cat "$WORK/key.pub")
chpasswd:
  expire: false
  users:
    - {name: alex, password: panda, type: text}
package_update: true
packages:
  - dbus-user-session
  - upower
  - network-manager
  - power-profiles-daemon
  - pipewire
  - pipewire-pulse
  - wireplumber
  - foot
  - libseat1
  - libinput10
  - libxkbcommon0
  - libpixman-1-0
  - libgbm1
  - libegl1
  - libgles2
  - fontconfig
  - fonts-noto-color-emoji
  - libgl1-mesa-dri
  - libegl-mesa0
  - kbd
write_files:
  # alex logs in on tty1; the session starts there when asked (hocus-vm).
  - path: /etc/systemd/system/getty@tty1.service.d/autologin.conf
    content: |
      [Service]
      ExecStart=
      ExecStart=-/sbin/agetty --autologin alex --noclear %I \$TERM
runcmd:
  - [loginctl, enable-linger, alex]
  - [sh, -c, "printf '[ \"\$(tty)\" = /dev/tty1 ] && [ -f ~/.hocus-session ] && exec ~/hocus-vm run\n' >> /home/alex/.bash_profile && chown alex:alex /home/alex/.bash_profile"]
  - [systemctl, daemon-reload]
  - [systemctl, restart, getty@tty1]
  - [touch, /var/lib/cloud/instance/hocuspanda-ready]
EOF
}

up() {
    mkdir -p "$WORK"
    if [ -f "$WORK/qemu.pid" ] && kill -0 "$(cat "$WORK/qemu.pid")" 2>/dev/null; then
        echo "already running (pid $(cat "$WORK/qemu.pid"))"
        return
    fi
    [ -f "$BASE" ] || curl -fL -o "$BASE.part" "$IMAGE_URL"
    [ -f "$BASE.part" ] && mv "$BASE.part" "$BASE"
    seed
    qemu-img create -q -f qcow2 -F qcow2 -b "$BASE" "$WORK/disk.qcow2" 12G
    # cloud-init fetches its seed from here, on the host's loopback.
    python3 -m http.server "$HTTP_PORT" --bind 127.0.0.1 --directory "$WORK/seed" \
        > "$WORK/http.log" 2>&1 &
    echo $! > "$WORK/http.pid"
    case "$GPUS" in
        1) gpus="-device virtio-vga,id=gpu0" ;;
        2) gpus="-device virtio-vga,id=gpu0,edid=off -device virtio-gpu-pci,id=gpu1" ;;
        *) echo "HOCUS_VM_GPUS is 1 or 2"; exit 2 ;;
    esac
    # In a scope of its own, capped: it never takes the host's memory.
    # shellcheck disable=SC2086 # $gpus is several words.
    systemd-run --user --scope --quiet --collect -p MemoryMax=6G -- \
        qemu-system-x86_64 -enable-kvm -cpu host -smp "$CPUS" -m "$MEMORY" \
        -name hocuspanda-vm -machine vmport=off \
        -drive file="$WORK/disk.qcow2",if=virtio \
        -nic user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:"$SSH_PORT"-:22 \
        -smbios type=1,serial="ds=nocloud;s=http://10.0.2.2:$HTTP_PORT/" \
        $gpus -display "$DISPLAY_BACKEND" \
        -global PIIX4_PM.disable_s3=0 \
        -monitor unix:"$WORK/monitor",server,nowait \
        -qmp unix:"$WORK/qmp",server,nowait \
        -serial file:"$WORK/serial.log" \
        -pidfile "$WORK/qemu.pid" -daemonize
    echo "booting; installing packages takes a few minutes"
    i=0
    until ssh_vm test -f /var/lib/cloud/instance/hocuspanda-ready 2>/dev/null; do
        i=$((i + 1))
        [ "$i" -lt 120 ] || { echo "the VM never got ready: $WORK/serial.log"; exit 1; }
        sleep 5
    done
    kill "$(cat "$WORK/http.pid")" 2>/dev/null || true
    rm -f "$WORK/http.pid"
    echo "ready: scripts/vm/debian.sh ssh"
}

copy() {
    ssh_vm mkdir -p bin
    for program in hocuspanda hocus hocus-settings hocus-test-client hocus-lease; do
        scp -q -i "$WORK/key" -P "$SSH_PORT" \
            -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
            "${CARGO_TARGET_DIR:-target}/debug/$program" alex@127.0.0.1:bin/
    done
    scp -q -i "$WORK/key" -P "$SSH_PORT" \
        -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        scripts/vm/in-vm.sh alex@127.0.0.1:hocus-vm
    echo "copied to ~/bin, and ~/hocus-vm to drive it"
}

# A package built by packaging/deb/build.sh, with what it depends on.
install_deb() {
    scp -q -i "$WORK/key" -P "$SSH_PORT" \
        -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        "$1" alex@127.0.0.1:/tmp/
    ssh_vm sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q \
        /tmp/"$(basename "$1")"
}

down() {
    if [ -f "$WORK/qemu.pid" ]; then
        pid=$(cat "$WORK/qemu.pid")
        kill "$pid" 2>/dev/null || true
        # Gone before its ports are taken again.
        i=0
        while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 50 ]; do
            i=$((i + 1)); sleep 0.2
        done
        rm -f "$WORK/qemu.pid"
    fi
    if [ -f "$WORK/http.pid" ]; then
        kill "$(cat "$WORK/http.pid")" 2>/dev/null || true
        rm -f "$WORK/http.pid"
    fi
    rm -f "$WORK/disk.qcow2"
    echo "stopped"
}

case "${1:-}" in
    up) up ;;
    copy) copy ;;
    install) install_deb "$2" ;;
    ssh) shift; ssh_vm "$@" ;;
    down) down ;;
    monitor) shift; printf '%s\n' "$*" | socat - UNIX-CONNECT:"$WORK/monitor" ;;
    screen)
        # QEMU writes the file itself: an absolute path.
        file=$(realpath -m "$2")
        qmp_command "{\"execute\":\"screendump\",\"arguments\":{\"filename\":\"$file\",\"device\":\"gpu${3:-0}\",\"format\":\"png\"}}" >/dev/null
        sleep 0.5
        echo "$file" ;;
    mouse) qmp "{\"type\":\"rel\",\"data\":{\"axis\":\"x\",\"value\":$2}},{\"type\":\"rel\",\"data\":{\"axis\":\"y\",\"value\":$3}}" ;;
    click)
        qmp '{"type":"btn","data":{"down":true,"button":"left"}}'
        qmp '{"type":"btn","data":{"down":false,"button":"left"}}' ;;
    keys)
        shift
        for key in "$@"; do
            printf 'sendkey %s\n' "$key" | socat - UNIX-CONNECT:"$WORK/monitor" >/dev/null
            sleep 0.05
        done ;;
    *) sed -n '2,28p' "$0"; exit 2 ;;
esac
