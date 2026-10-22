#!/bin/sh
# Builds the .rpm from release binaries already built, with rpmbuild.
#
#   packaging/rpm/build.sh [OUTDIR]     (default: CARGO_TARGET_DIR/packages)
#
# Needs rpm-build; the binaries are read from CARGO_TARGET_DIR/release.
set -eu

top=$(cd "$(dirname "$0")/../.." && pwd)
target=$(realpath -m "${CARGO_TARGET_DIR:-$top/target}")
out=$(realpath -m "${1:-$target/packages}")
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$top/Cargo.toml" | head -n 1)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

rpmbuild -bb --quiet \
    --define "_topdir $work" \
    --define "_rpmdir $work/rpms" \
    --define "hp_top $top" \
    --define "hp_target $target" \
    --define "hp_version $version" \
    "$top/packaging/rpm/hocuspanda.spec"

mkdir -p "$out"
for rpm in "$work"/rpms/*/hocuspanda-*.rpm; do
    cp "$rpm" "$out/"
    echo "$out/$(basename "$rpm")"
done
