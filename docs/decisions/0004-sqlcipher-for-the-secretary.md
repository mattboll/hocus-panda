# 0004 — SQLCipher for the secretary's memory

Date: 2026-11-08 · Status: withdrawn (the secretary was removed, 2026-11-10)

## Context

The secretary keeps a temporal knowledge graph of the user's work and
personal life in one SQLite database, with FTS5 indexes
(docs/design/secretary.md §7.1), "encrypted at rest with a key from the
keyring". Two ways were weighed:

- **SQLCipher**, through rusqlite's `bundled-sqlcipher` feature: SQLite
  with every page encrypted (AES-256, HMAC-SHA512 per page), the indexes,
  the write-ahead log and temporary files included. SQLCipher Community
  Edition is BSD-3-Clause; the SQLite it is built on is in the public
  domain. On Linux and FreeBSD it links the system's libcrypto (OpenSSL 3,
  Apache-2.0). The crate that bundles its sources, `sqlcipher-src`,
  declares `MIT AND BSD-3-Clause AND blessing AND WTFPL`: `blessing` is
  SQLite's public-domain dedication, `WTFPL` covers libtomcrypt's sources,
  which it compiles only for targets without a system crypto library
  (WebAssembly), never ours.
- **Encryption in the application**, with plain bundled SQLite (MIT at the
  crate level): either each text column sealed with ChaCha20-Poly1305 (as
  the clipboard's history is), which leaves FTS5 with nothing to index but
  blind tokens (no prefix, no ranking on words, aliases folded only by
  exact match) and the graph's structure (edge types, dates, who is linked
  to whom) in the clear; or the whole database kept in memory and written
  as one sealed snapshot, which loses the last changes on a crash and
  rewrites megabytes for each episode seen.

## Decision

SQLCipher, built from source with the system's libcrypto. The key is 32
random bytes, given raw (no key derivation), kept in the Secret Service on
the bus the compositor names, else in a `0600` file beside the database
(the clipboard's rule).

`blessing` and `WTFPL` are allowed for the crate `sqlcipher-src` only
(`deny.toml`, `[[licenses.exceptions]]`): both are more permissive than
MIT, and neither reaches another dependency.

## Consequences

- Building needs libcrypto's headers (`libssl-dev`), which CI installs; the
  package depends on `libssl3`.
- Everything she knows is sealed on disk, the full-text indexes included,
  and every SQLite feature works as is.
- Should this be refused, the fallback is the in-memory database sealed as
  a whole, behind the same `Store`: the rest of the secretary is unchanged.
