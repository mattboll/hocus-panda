# Vendored crates

One crate of crates.io is built from here with a small change, through
`[patch.crates-io]` in the workspace's `Cargo.toml`. Its directory is the
published crate plus one patch file, nothing else: `scripts/vendor.sh
--check` rebuilds it from the crates.io tarball and the patch and fails on
any difference, in CI (`vendor` job).

| Crate | Version | Patch | Issue | Upstream |
|---|---|---|---|---|
| `iced_tiny_skia` | 0.14.1 | [`patches/iced_tiny_skia-0.14.1.patch`](patches/iced_tiny_skia-0.14.1.patch) | #662 | not proposed |

## Why

- **`iced_tiny_skia`: `draw_whole_frames(bool)`** (#662). iced's CPU
  renderer draws only the regions it finds changed since the buffer's last
  frame, and misses what a primitive draws past its bounds (a shadow, an
  anti-aliased stroke, text moved with its layer, a gradient drawn in
  pieces, a closed overlay): those pixels stay on screen. The Settings ask
  for whole frames; they draw only when something changes
  ([docs/design/settings.md](../docs/design/settings.md)). 18 lines in
  `src/window/compositor.rs` and `src/lib.rs`.

The patch also replaces the header of the published `Cargo.toml` with a
note of the change, and adds the crate's licence text (the tarball carries
none; MIT asks that it goes with copies).

`accesskit_unix` was vendored too, to name the shell "Hocuspanda" on AT-SPI
(#734); that patch is gone, and the shell is named after its executable as
the published crate does.

## What a directory holds

The tarball's files, without `Cargo.lock`, `Cargo.toml.orig`,
`.cargo_vcs_info.json` (what `cargo publish` adds) and any `CHANGELOG.md`,
with the patch applied. The tarball's SHA-256 is in `scripts/vendor.sh` and is
checked whether it comes from cargo's cache or from crates.io.

## Changing a patch

Edit the files in `vendor/<crate>`, then:

```sh
scripts/vendor.sh --diff     # rewrites vendor/patches/*.patch from vendor/
scripts/vendor.sh --check    # what CI runs
```

## Moving to a new version

1. In `scripts/vendor.sh`, set the crate's new version and the SHA-256 of
   its tarball (`sha256sum ~/.cargo/registry/cache/*/<crate>-<version>.crate`,
   or the `cksum` crates.io's index gives).
2. `git mv vendor/patches/<crate>-<old>.patch vendor/patches/<crate>-<new>.patch`,
   then `scripts/vendor.sh`: it re-creates the directory from the new
   tarball and applies the patch, and stops if it no longer applies; then
   make the change by hand in `vendor/<crate>` and run
   `scripts/vendor.sh --diff`.
3. The version in `[workspace.dependencies]` (`iced` and `iced_tiny_skia`
   are held at exact versions there: both move together), `cargo update -p
   <crate>`, and this table.

When upstream offers the change, the directory, its patch, its line in
`[patch.crates-io]` and in `scripts/vendor.sh` all go.
