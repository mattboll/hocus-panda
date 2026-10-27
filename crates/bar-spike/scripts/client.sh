#!/bin/sh
# Starts any client ($2..) against run dir $1; PID in $1/<basename>.pid.
D=$1; shift
SOCK=$(ls "$D/run" | grep -E '^(wayland|hocus)-[0-9]+$' | head -1)
N=$(basename "$1")
env -i PATH=/usr/bin:/bin HOME="$D/home" XDG_RUNTIME_DIR="$D/run" \
  XDG_CONFIG_HOME="$D/config" XDG_CACHE_HOME="$D/cache" WAYLAND_DISPLAY="$SOCK" \
  setsid -f sh -c "echo \$\$ > '$D/$N.pid'; exec \"\$@\" > '$D/$N.log' 2>&1" sh "$@"
