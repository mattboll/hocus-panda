# CLAUDE.md

Guidance for AI agents (Claude Code and others) working in this repository.

## Project

Hocuspanda v2: a Wayland desktop for developers who work with AI agents. A
compositor on smithay, and a shell drawn with iced as a separate client.
Linux first. MIT. The first build is kept under the tag `v0-first-build`
as a technical reference only.

Read, in this order: [docs/vision.md](docs/vision.md),
[docs/design/tech-choices.md](docs/design/tech-choices.md), the
[decisions](docs/decisions/), and the spec of whatever you work on
([docs/design/bar.md](docs/design/bar.md) for the bar).

## How we work

- **Nothing is coded before its mockup and its spec are approved** by the
  maintainer. The design canvas and the spec are the reference; when they
  disagree, the approved spec wins.
- Work goes in vertical slices, in the vision's order. A slice is done
  when the maintainer accepted it on their own machine, against the
  mockup, not when the tests pass.
- Reusing code from `v0-first-build`: reread it, port it only if it holds
  up, and say so in the pull request.
- Test what you can headless, to keep what works from breaking; never use
  references you recorded yourself as proof that something looks right.

## Hard rules

- **Event loop.** Compositor state is owned by the calloop loop. Never
  share it across threads; never block on the loop (no blocking I/O, D-Bus
  calls, waiting on processes); never queue work per animation tick.
  Workers send owned results back through channels.
- **Time.** Monotonic time for everything except what the user reads.
- **No panics on the loop.** Handle errors; log them with context.
- **Licenses.** Only permissive dependencies (decision 0002); `cargo deny`
  must pass. Never copy or translate code from GPL projects (niri,
  cosmic-comp, KWin, Mutter). smithay, anvil and iced (MIT) are fine.
- **Do not touch the developer's live session.** Test compositors run with
  a fresh, short `XDG_RUNTIME_DIR`, `HOME`, `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME`, and no session D-Bus. Never write into the user's real
  configuration. Stop processes by exact PID, never with `pkill -f` or
  pattern matching. Never signal processes you did not start.
- **Deleting files**: name every path explicitly; no globs or variables in
  `rm`.

## Workflow

- One issue per branch, `<type>/<issue>-<short-name>`, based on `v2`. A
  pull request says `Closes #N`, how it was tested, and shows screenshots
  for anything visible.
- Commit messages in English, `area: what changes`. **No attribution
  trailers** (`Co-Authored-By`, `Claude-Session`, "Generated with…") in
  commits or pull requests.
- Code, comments, docs, issues and PRs in English; user-facing strings
  translatable, shipped in English and French.
- Ask when a product or design choice is unclear; do not guess.

## Style

- Small crates with clear boundaries; the shell does not depend on smithay.
- Comments explain why, not what.
- Prefer explicit state machines (enums) over flags.
- `unsafe` only at FFI boundaries, each block with a `// SAFETY:` comment.

## Commands

```sh
cargo build                                   # build everything
cargo nextest run --workspace                 # tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo deny check                              # licences and advisories
```

CI runs the same on every pull request to `v2`. Update this section as
commands land.
