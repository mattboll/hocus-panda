#!/bin/sh
# Starts the spike's bar as a client of the compositor in run dir $1.
# Extra environment (BAR_TRACE, WAYLAND_DEBUG, BAR_HALO…) passes through
# as arguments 2.. (NAME=VALUE). PID in $1/bar.pid, log in $1/bar.log.
D=$1; shift
SOCK=$(ls "$D/run" | grep -E '^(wayland|hocus)-[0-9]+$' | head -1)
BIN=${CARGO_TARGET_DIR:-target}/debug/bar-spike
env -i PATH=/usr/bin:/bin HOME="$D/home" XDG_RUNTIME_DIR="$D/run" \
  XDG_CONFIG_HOME="$D/config" XDG_CACHE_HOME="$D/cache" WAYLAND_DISPLAY="$SOCK" "$@" \
  setsid -f sh -c "echo \$\$ > '$D/bar.pid'; exec '${BAR_BIN:-$BIN}' > '$D/bar.log' 2>&1"
