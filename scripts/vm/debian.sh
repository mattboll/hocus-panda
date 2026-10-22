#!/bin/sh
# A Debian testing VM (QEMU, KVM) to try hocuspanda against a real system:
# logind, PAM, UPower, NetworkManager, power profiles, PipeWire, autostart.
# Nothing of the host's session is used: the VM has its own user, buses and
# services.
#
#   scripts/vm/debian.sh up            boot it (downloads the image once)
#   scripts/vm/debian.sh copy          copy target/debug's programs into it
#   scripts/vm/debian.sh ssh [cmd…]    a shell, or a command, as the test user
#   scripts/vm/debian.sh down          stop it
#
# The base image is cached in $XDG_CACHE_HOME/hocuspanda-vm; each `up`
# starts from a fresh overlay. The test user is `alex`, password `panda`.
set -eu

CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/hocuspanda-vm"
WORK="$CACHE/debian"
IMAGE_URL="https://cloud.debian.org/images/cloud/forky/daily/latest/debian-14-genericcloud-amd64-daily.qcow2"
BASE="$CACHE/debian-14-genericcloud-amd64.qcow2"
SSH_PORT="${HOCUS_VM_SSH_PORT:-2522}"
HTTP_PORT="${HOCUS_VM_HTTP_PORT:-2580}"
MEMORY="${HOCUS_VM_MEMORY:-4G}"
CPUS="${HOCUS_VM_CPUS:-4}"

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
runcmd:
  - [loginctl, enable-linger, alex]
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
    # In a scope of its own, capped: it never takes the host's memory.
    systemd-run --user --scope --quiet --collect -p MemoryMax=6G -- \
        qemu-system-x86_64 -enable-kvm -cpu host -smp "$CPUS" -m "$MEMORY" \
        -name hocuspanda-vm \
        -drive file="$WORK/disk.qcow2",if=virtio \
        -nic user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:"$SSH_PORT"-:22 \
        -smbios type=1,serial="ds=nocloud;s=http://10.0.2.2:$HTTP_PORT/" \
        -device virtio-vga -display none \
        -global PIIX4_PM.disable_s3=0 \
        -monitor unix:"$WORK/monitor",server,nowait \
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
    for program in hocuspanda hocus hocus-settings hocus-test-client; do
        scp -q -i "$WORK/key" -P "$SSH_PORT" \
            -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
            "target/debug/$program" alex@127.0.0.1:bin/
    done
    scp -q -i "$WORK/key" -P "$SSH_PORT" \
        -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        scripts/vm/in-vm.sh alex@127.0.0.1:hocus-vm
    echo "copied to ~/bin, and ~/hocus-vm to drive it"
}

down() {
    if [ -f "$WORK/qemu.pid" ]; then
        kill "$(cat "$WORK/qemu.pid")" 2>/dev/null || true
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
    ssh) shift; ssh_vm "$@" ;;
    down) down ;;
    monitor) shift; printf '%s\n' "$*" | socat - UNIX-CONNECT:"$WORK/monitor" ;;
    *) sed -n '2,15p' "$0"; exit 2 ;;
esac
