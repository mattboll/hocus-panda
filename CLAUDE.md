# CLAUDE.md

Guidance for AI agents (Claude Code and others) working in this repository.
Humans are welcome to read it too.

## Project

Hocuspanda: a Wayland compositor and its whole shell in one Rust process, on
smithay. Linux first, FreeBSD as a target. MIT. Read, in this order:
[docs/vision.md](docs/vision.md), [docs/architecture.md](docs/architecture.md),
[docs/workflow.md](docs/workflow.md), and the [decisions](docs/decisions/).
The feature list is [docs/features.md](docs/features.md); motion is
[docs/design/motion.md](docs/design/motion.md).

## Commands

The workspace is being set up (milestone M0). Until then:

```sh
cargo build                                   # build everything
cargo nextest run                             # tests (or: cargo test)
cargo clippy --all-targets -- -D warnings     # lints
cargo fmt --check                             # formatting
cargo deny check                              # licenses and advisories
```

Running the compositor, always with a fresh, short runtime directory next to
a live session:

```sh
# nested, in a window of the current session
target/debug/hocuspanda --runtime-dir "$(mktemp -d /tmp/hp-XXXX)" --spawn foot
# headless: offscreen outputs, a private runtime directory by default
target/debug/hocuspanda --backend headless --output 2560x1600@2 --output 3440x1440 \
    --api-permission test --spawn foot
# talk to it: the socket is logged at start ("control=…")
target/debug/hocus msg --socket <control socket> windows   # also: outputs, scene,
    # events, pointer-move X Y, click, key CODE, screenshot --output NAME -o FILE
```

Tests drive a headless compositor in-process on a virtual clock
(`hocus_compositor::Headless`, see `crates/hocus-test/tests/`).

Update this section as commands land (scenarios, VM scripts).

## Hard rules

- **Event loop.** All compositor and shell state is owned by the calloop
  loop. Never share it across threads; never block on the loop (no blocking
  I/O, D-Bus calls, waiting on processes); never queue work per animation
  tick. Worker threads send owned results back through channels.
- **Time.** Monotonic time for everything except what the user reads.
- **No panics on the loop.** Handle errors; log them with context.
- **Licenses.** Only permissive dependencies (see decision 0002); `cargo deny`
  must pass. Never copy or translate code from GPL projects (niri,
  cosmic-comp, KWin, Mutter). smithay and its `anvil` example (MIT) are fine.
- **Plugins** are Rust crates compiled in. No scripting runtime, no WASM, no
  dynamic loading.
- **Testable without a screen.** Every feature comes with scenarios that run
  headless; write them first, from the issue's acceptance criteria.
- **Do not touch the developer's live session.** Test compositors run with a
  fresh, short `XDG_RUNTIME_DIR`, `HOME`, `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME`, and no session D-Bus. Never write into the user's real
  configuration. Stop processes by exact PID, never with `pkill -f` or
  pattern matching. Never signal processes you did not start.
- **Deleting files**: name every path explicitly; no globs or variables in
  `rm`.

## Workflow

- One issue per branch: `<type>/<issue>-<short-name>`. Open a pull request
  that says `Closes #N` and how it was tested. The maintainer merges.
- Commit messages in English, `area: what changes` (behaviour, present
  tense). **No attribution trailers** (`Co-Authored-By`, `Claude-Session`,
  "Generated with…") in commits or pull requests.
- Code, comments, docs, issues and PRs in English; user-facing strings
  translatable, shipped in English and French.
- Ask in the issue when a product or design choice is unclear; do not guess.
- Keep docs and code consistent in the same pull request.

## Style

- Small crates with clear boundaries (`hocus-core` and `hocus-ui` do not
  depend on smithay).
- Comments explain why, not what. Match the surrounding code.
- Prefer explicit state machines (enums) over flags.
- `unsafe` only at FFI boundaries, each block with a `// SAFETY:` comment.
