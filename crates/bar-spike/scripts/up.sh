#!/bin/sh
# Starts the first build's compositor headless, in a fresh environment:
# short runtime dir, fresh HOME and XDG dirs, no session bus, no display.
# Prints the run directory; the compositor's PID is in $D/hp.pid.
V1=${V1_BIN:?set V1_BIN to the directory of the first build (hocuspanda and hocus)}
D=$(mktemp -d /tmp/hp-XXXX) || exit 1
mkdir -p "$D/run" "$D/home" "$D/config" "$D/cache" "$D/state"
chmod 700 "$D/run"
# No curtain and no lock: a test session nobody types into for minutes.
mkdir -p "$D/config/hocuspanda"
printf '[idle]\ncurtain_after = 0\nlock = false\n' > "$D/config/hocuspanda/config.toml"
OUT=${OUT:-1920x1080}
env -i PATH=/usr/bin:/bin HOME="$D/home" XDG_RUNTIME_DIR="$D/run" \
  XDG_CONFIG_HOME="$D/config" XDG_CACHE_HOME="$D/cache" XDG_STATE_HOME="$D/state" \
  RUST_LOG=${RUST_LOG:-info} \
  setsid -f sh -c "echo \$\$ > '$D/hp.pid'; exec '$V1/hocuspanda' --backend headless --runtime-dir '$D/run' --output $OUT --api-permission test --no-shell --no-xwayland --no-log-file $EXTRA > '$D/hp.log' 2>&1"
for i in $(seq 1 50); do
  grep -q "control=" "$D/hp.log" 2>/dev/null && break
  sleep 0.1
done
echo "$D"
