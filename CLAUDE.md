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
target/debug/hocuspanda --backend winit --runtime-dir "$(mktemp -d /tmp/hp-XXXX)" --spawn foot
# notifications: only on a bus given explicitly, never the live session's
#   --session-bus "$(dbus-daemon --session --print-address --fork)"
# headless: offscreen outputs, a private runtime directory by default
target/debug/hocuspanda --backend headless --output 2560x1600@2 --output 3440x1440 \
    --api-permission test --spawn foot
# talk to it: the socket is logged at start ("control=…")
target/debug/hocus msg --socket <control socket> windows   # also: outputs, scene,
    # events, pointer-move X Y, click, key CODE, screenshot --output NAME -o FILE,
    # ui (the shell's buttons and texts with their places)
```

Tests drive a headless compositor in-process on a virtual clock
(`hocus_compositor::Headless`, see `crates/hocus-test/tests/`). Acceptance
scenarios are TOML files in `tests/scenarios/` (format in its README):

```sh
cargo nextest run -p hocus-test --test scenarios         # all scenarios
HOCUS_BLESS=1 cargo nextest run -p hocus-test --test scenarios  # record references
target/debug/hocus scenario run tests/scenarios/click-to-focus.toml
```

The shell's toolkit runs alone, in a window of any Wayland session (or of a
nested or headless hocuspanda); its reference images are recorded the same
way:

```sh
cargo run -p hocus-ui --example gallery
HOCUS_BLESS=1 cargo test -p hocus-ui --test spring
```

Against a real system (the session on a TTY with DRM/KMS and libinput,
logind, PAM, UPower, NetworkManager, power profiles, PipeWire, autostart,
suspend), in a Debian testing VM (QEMU with KVM; the base image is cached in
`$XDG_CACHE_HOME/hocuspanda-vm`):

```sh
scripts/vm/debian.sh up        # boot it, user alex / panda
scripts/vm/debian.sh copy      # target/debug's programs into it
scripts/vm/debian.sh ssh '~/hocus-vm session'    # the session on its tty1
scripts/vm/debian.sh keys e c h o ret            # its keyboard (QEMU key names)
scripts/vm/debian.sh mouse 60 40; scripts/vm/debian.sh click
scripts/vm/debian.sh screen shot.png             # what its screen shows
scripts/vm/debian.sh ssh '~/hocus-vm msg ui'     # its control socket
scripts/vm/debian.sh ssh '~/hocus-vm start'      # or headless, with --session-services
scripts/vm/debian.sh monitor system_wakeup       # after `systemctl suspend` in it
scripts/vm/debian.sh down
```

`--session-services` makes a headless hocuspanda use the machine's services
as the session would: only in a VM, never next to a live session.

Packages (see [packaging/](packaging/README.md)), from a release build:

```sh
cargo build --release --locked -p hocuspanda -p hocus -p hocus-settings
packaging/deb/build.sh                 # target/packages/*.deb (dpkg-dev)
packaging/rpm/build.sh                 # target/packages/*.rpm (rpm-build)
# the .deb in the VM, logging in from a display manager (greetd)
scripts/vm/display-manager.sh target/packages/hocuspanda_*.deb shot.png
```

Update this section as commands land.

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
