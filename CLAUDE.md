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

The compositor (`crates/hocus-compositor`, binary `hocuspanda`): nested in
a window when started from a graphical session, the session itself on a
virtual terminal otherwise; `--backend winit|tty|headless` forces one.
Always with a fresh, short runtime directory next to a live session:

```sh
# nested; Alt+Super+drag moves a window (Super+drag on a real session)
target/debug/hocuspanda --runtime-dir "$(mktemp -d /tmp/hp-XXXX)" --spawn foot
# nested without touching the live session's screen: in an Xvfb display
Xvfb :87 -screen 0 1600x1000x24 -nolisten tcp &
env -i HOME="$(mktemp -d)" PATH=/usr/bin:/bin DISPLAY=:87 LIBGL_ALWAYS_SOFTWARE=1 \
    XDG_RUNTIME_DIR="$(mktemp -d /tmp/hp-XXXX)" target/debug/hocuspanda \
    --runtime-dir <that same directory> --spawn foot   # drive with xdotool
# headless: offscreen outputs, a private runtime directory by default
target/debug/hocuspanda --backend headless --output 2560x1600@2 --output 1920x1080 \
    --renderer pixman --no-xwayland --spawn foot
RUST_LOG=hocus_compositor=debug                       # more of what it does
```

Its configuration is `$XDG_CONFIG_HOME/hocuspanda/config.toml` (or
`--config FILE`); so far it reads `[keyboard]` (`layouts = ["fr",
"fr(bepo)"]`, `options`, `repeat_delay`, `repeat_rate`). Headless tests
start it in-process (`hocus_compositor::Headless`, see
`crates/hocus-compositor/tests/`) with sctk clients on threads.

The real-screen path (DRM/KMS, libinput, logind) in a Debian testing VM
(QEMU with KVM; the base image is cached in
`$XDG_CACHE_HOME/hocuspanda-vm`):

```sh
scripts/vm/debian.sh up        # boot it, user alex / panda
scripts/vm/debian.sh copy      # target/debug's hocuspanda into it
scripts/vm/debian.sh ssh '~/hocus-vm session'    # the session on its tty1
scripts/vm/debian.sh keys e c h o ret            # its keyboard (QEMU key names)
scripts/vm/debian.sh mouse 60 40; scripts/vm/debian.sh click
scripts/vm/debian.sh screen shot.png             # what its screen shows
scripts/vm/debian.sh ssh '~/hocus-vm log'        # what the session said
scripts/vm/debian.sh down
# a second VM beside the maintainer's: its own name and ports
HOCUS_VM_NAME=other HOCUS_VM_SSH_PORT=2532 HOCUS_VM_HTTP_PORT=2590 scripts/vm/debian.sh up
# to try it by hand: its screen in a window, with its keyboard and mouse
HOCUS_VM_DISPLAY=gtk scripts/vm/debian.sh up
```
