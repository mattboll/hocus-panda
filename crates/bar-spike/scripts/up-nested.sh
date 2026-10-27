#!/bin/sh
# The first build's compositor nested in a window of the live session (for a
# GPU-rendered look only): fresh runtime dir, HOME and XDG dirs, no session
# bus; only the host's Wayland socket is shared, by absolute path.
V1=${V1_BIN:?set V1_BIN to the directory of the first build (hocuspanda and hocus)}
D=$(mktemp -d /tmp/hp-XXXX) || exit 1
mkdir -p "$D/run" "$D/home" "$D/config" "$D/cache" "$D/state"
chmod 700 "$D/run"
env -i PATH=/usr/bin:/bin HOME="$D/home" XDG_RUNTIME_DIR="$D/run" \
  XDG_CONFIG_HOME="$D/config" XDG_CACHE_HOME="$D/cache" XDG_STATE_HOME="$D/state" \
  WAYLAND_DISPLAY=${HOST_WAYLAND:?the host socket, by absolute path} RUST_LOG=${RUST_LOG:-info} \
  setsid -f sh -c "echo \$\$ > '$D/hp.pid'; exec '$V1/hocuspanda' --backend winit --runtime-dir '$D/run' --api-permission test --no-shell --no-xwayland --no-log-file > '$D/hp.log' 2>&1"
for i in $(seq 1 50); do
  grep -q "control=" "$D/hp.log" 2>/dev/null && break
  sleep 0.1
done
echo "$D"
