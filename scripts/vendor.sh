#!/usr/bin/env bash
# vendor/<crate> is the crate as published on crates.io plus one patch,
# vendor/patches/<crate>-<version>.patch (vendor/README.md says why).
#
#   scripts/vendor.sh            # re-creates vendor/<crate> from the tarball and the patch
#   scripts/vendor.sh --check    # fails when vendor/<crate> is anything else (CI)
#   scripts/vendor.sh --diff     # writes the patch from vendor/<crate> as it is
#
# The tarball is the one cargo already downloaded
# (~/.cargo/registry/cache), else it comes from crates.io; either way its
# SHA-256 must be the one below. Needs curl, tar, patch, git and sha256sum.
set -euo pipefail

# name version sha256-of-the-.crate
crates=(
    "iced_tiny_skia 0.14.1 c267596d742714b1853cc10c3983a367762816fc4836bd3b79f76ce76787d6f8"
)
# What `cargo publish` adds to a tarball, and the changelog: not vendored.
left_out=(Cargo.lock Cargo.toml.orig .cargo_vcs_info.json CHANGELOG.md)

mode=${1:-create}
case $mode in
    create | --check | --diff) ;;
    *) sed -n '2,11p' "$0"; exit 2 ;;
esac

top=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The published crate, unpacked into $work/NAME/a, without what is left out.
upstream() {
    local name=$1 version=$2 sum=$3 tarball="" cached
    for cached in "${CARGO_HOME:-$HOME/.cargo}"/registry/cache/*/"$name-$version.crate"; do
        [ -e "$cached" ] && tarball=$cached
    done
    if [ -z "$tarball" ]; then
        tarball=$work/$name-$version.crate
        curl --fail --silent --show-error --location --retry 3 -o "$tarball" \
            "https://static.crates.io/crates/$name/$name-$version.crate"
    fi
    echo "$sum  $tarball" | sha256sum --check --quiet
    mkdir -p "$work/$name"
    tar -xzf "$tarball" -C "$work/$name"
    mv "$work/$name/$name-$version" "$work/$name/a"
    local file
    for file in "${left_out[@]}"; do
        rm -f "$work/$name/a/$file"
    done
}

status=0
for crate in "${crates[@]}"; do
    read -r name version sum <<< "$crate"
    patch=$top/vendor/patches/$name-$version.patch
    upstream "$name" "$version" "$sum"

    if [ "$mode" = --diff ]; then
        cp -r "$top/vendor/$name" "$work/$name/b"
        # git's diff: no timestamps, so the same change is the same file.
        # It exits 1 when the two differ, which they do.
        (cd "$work/$name" && git diff --no-index --no-color --no-ext-diff --no-renames --no-prefix a b) \
            > "$patch" || [ $? -eq 1 ]
        echo "vendor/patches/$name-$version.patch written"
        continue
    fi

    patch --directory="$work/$name/a" --strip=1 --batch --silent --no-backup-if-mismatch --fuzz=0 < "$patch"
    if [ "$mode" = --check ]; then
        if diff -r "$work/$name/a" "$top/vendor/$name"; then
            echo "vendor/$name is $name $version and its patch"
        else
            echo "::error::vendor/$name is not $name $version plus vendor/patches/$name-$version.patch (scripts/vendor.sh re-creates it, --diff rewrites the patch)"
            status=1
        fi
    else
        rm -rf "$top/vendor/$name"
        mv "$work/$name/a" "$top/vendor/$name"
        echo "vendor/$name re-created"
    fi
done
exit $status
