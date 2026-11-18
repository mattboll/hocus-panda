# 0002 — MIT license, permissive dependencies only

Date: 2026-10-21 · Status: accepted

## Context

We want a license freer than the GPL (BSD or MIT), for the desktop
and for anyone who builds on it.

## Decision

The code is MIT. Dependencies must be permissive: MIT, Apache-2.0,
BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode, CC0-1.0; OFL-1.1 for fonts;
the Unlicense (public-domain-like; added 2026-11-17 by the maintainer for
`whisper-rs`, `hocus-transcribe`'s binding to whisper.cpp, which is MIT);
CDLA-Permissive-2.0 and bzip2-1.0.6 (added the same day for the build
dependencies of `sherpa-onnx-sys`: `webpki-roots` through `ureq`, and
`libbz2-rs-sys` through `bzip2` and `zip`; none of them is in a program).
Anything else (MPL, LGPL, GPL) needs a new decision record.

No code is copied or translated from GPL projects, including smithay-based
ones (niri, cosmic-comp). Reading their public design discussions is fine;
reading their code to reproduce it is not.

## Consequences

- `cargo deny` checks licenses in CI and fails on anything outside the list.
- Some convenient crates or system libraries may be off-limits; we write our
  own or pick another.
- Artwork (pandas) may use CC BY or CC0.
