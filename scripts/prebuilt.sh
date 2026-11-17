#!/bin/sh
# The one library a build links prebuilt: sherpa-onnx's static libraries
# (Apache-2.0), with onnxruntime's (MIT) inside, which `hocus-transcribe`
# separates a meeting's voices with (docs/design/meetings.md). One exact
# version, checked against its SHA-256 before it is unpacked into
# prebuilt/sherpa-onnx/lib, where `.cargo/config.toml` points
# sherpa-onnx-sys's build (`SHERPA_ONNX_LIB_DIR`). That build (vendored,
# vendor/README.md) never downloads anything, and stops, saying to run
# this, until it ran.
#
#   scripts/prebuilt.sh            fetch and check it (once: kept)
#   scripts/prebuilt.sh --check    check the archive there, fetch nothing
#
# CI runs it before every Rust build, as packaging/deb/build.sh's caller
# does. The version is the `sherpa-onnx` crate's, pinned exactly in the
# root Cargo.toml: both change together.
set -eu

version=2.0.1
case "$(uname -m)" in
x86_64)
    archive=sherpa-onnx-v$version-linux-x64-static-lib.tar.bz2
    sha256=55cfadf65ba50d0afc1a802b7a337942819e51c94a5b5a384def1978f7c70871
    ;;
aarch64)
    archive=sherpa-onnx-v$version-linux-aarch64-static-lib.tar.bz2
    sha256=b6c6090e40fd18d6812f04fe21260602cd1223d7b5e83ad39dab87ad7c1c031d
    ;;
*)
    echo "prebuilt.sh: no sherpa-onnx archive for $(uname -m)" >&2
    exit 1
    ;;
esac
url=https://github.com/k2-fsa/sherpa-onnx/releases/download/v$version/$archive

top=$(cd "$(dirname "$0")/.." && pwd)
dir=$top/prebuilt
file=$dir/$archive
libs=$dir/sherpa-onnx

# The checked archive's libraries, unpacked whole or not at all.
unpack() {
    if [ -f "$libs/lib/libsherpa-onnx-c-api.a" ] && [ "$libs/lib" -nt "$file" ]; then
        return
    fi
    work=$(mktemp -d "$dir/unpack.XXXXXX")
    tar -xjf "$file" -C "$work"
    rm -rf "$libs"
    mv "$work/${archive%.tar.bz2}" "$libs"
    rmdir "$work"
    touch "$libs/lib"
    echo "$libs/lib: unpacked"
}

check() {
    echo "$sha256  $1" | sha256sum --check --status
}

if [ "${1:-}" = --check ]; then
    if [ -f "$file" ] && check "$file"; then
        echo "$file: checked"
        exit 0
    fi
    echo "$file: missing or not the archive expected; run scripts/prebuilt.sh" >&2
    exit 1
fi

if [ -f "$file" ] && check "$file"; then
    echo "$file: checked"
    unpack
    exit 0
fi
mkdir -p "$dir"
part=$file.part
curl --fail --location --silent --show-error --retry 3 --output "$part" "$url"
if ! check "$part"; then
    echo "$url: not the archive expected (SHA-256)" >&2
    rm -f "$part"
    exit 1
fi
mv "$part" "$file"
echo "$file: fetched and checked"
unpack
