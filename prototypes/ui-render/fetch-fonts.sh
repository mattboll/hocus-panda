#!/bin/sh
# Downloads the identity's fonts (SIL Open Font License) from google/fonts.
set -eu
dir="$(dirname "$0")/fonts"
mkdir -p "$dir"
base=https://raw.githubusercontent.com/google/fonts/main/ofl
curl -fsSL -o "$dir/Manrope.ttf" "$base/manrope/Manrope%5Bwght%5D.ttf"
curl -fsSL -o "$dir/Fraunces.ttf" "$base/fraunces/Fraunces%5BSOFT,WONK,opsz,wght%5D.ttf"
