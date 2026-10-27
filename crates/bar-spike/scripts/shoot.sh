#!/bin/sh
# Headless Chrome, no display, fresh profile: $1 html, $2 png, $3 WxH.
P=$(mktemp -d /tmp/hp-chrome-XXXX)
env -i PATH=/usr/bin:/bin HOME="$P" google-chrome --headless=new --no-first-run --disable-gpu \
  --user-data-dir="$P/profile" --allow-file-access-from-files --hide-scrollbars \
  --force-device-scale-factor=${SCALE:-1} --window-size="$3" --screenshot="$2" "file://$1" >/dev/null 2>&1
echo "$P"
