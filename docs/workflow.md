# Workflow

How we work: issues, pull requests, tests, tools, and AI agents. Most of the
code is written by AI agents (Claude Code) and reviewed by the maintainer;
these rules make that safe and pleasant.

## Issues and milestones

- **Milestones** follow the [roadmap](roadmap.md), in order. A milestone is a
  usable step, not a bag of tasks.
- **One issue = one deliverable** that fits in one pull request (a few days
  at most). Bigger things are split.
- Every issue has:
  - the *why* in two or three sentences, with links to the docs;
  - **acceptance criteria written as scenarios** ("given two outputs, when
    the pointer is on the external one and Super+2 is pressed, then only the
    external output shows desktop 2");
  - labels: `area:*` (core, compositor, ui, shell, platform, api, test, docs,
    ci), `type:*` (feature, bug, chore, design), `size:S|M|L`.
- Design questions get a `type:design` issue and, when settled, a decision
  record in [decisions/](decisions/).

## Branches, commits, pull requests

- Branch per issue: `<type>/<issue>-<short-name>`, e.g. `feat/7-nested-compositor`.
- Commit messages in English, `area: what changes`, describing behaviour in
  the present tense, e.g. `core: a window snapped to a half keeps its half when
  the gaps change`. Clean, meaningful commits; no "fix typo" chains.
- No AI attribution trailers or "generated with" lines in commits or PRs.
- A pull request:
  - says `Closes #N`;
  - explains what changed and why, and **how it was tested** (scenarios,
    screenshots, manual check in nested mode);
  - is green in CI;
  - is reviewed and merged by the maintainer (rebase merge).
- `main` is always releasable: it builds, its tests pass.

## Definition of done

- Acceptance scenarios pass headless in CI.
- Unit tests for the logic; no test that depends on the developer's session.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo deny check` pass.
- Strings shown to users are translatable (English and French).
- Docs updated when behaviour or decisions change.
- No panic, no blocking call and no unbounded queue on the event loop.

## Tools

On Debian (adapt for other systems):

```sh
sudo apt install rustup mold clang qemu-system-x86 qemu-utils ovmf \
    foot wayland-utils wev \
    libseat-dev libinput-dev libudev-dev libgbm-dev libdrm-dev \
    libxkbcommon-dev libwayland-dev libegl-dev libgles-dev libpixman-1-dev xwayland
rustup default stable
rustup component add rustfmt clippy rust-src
cargo install cargo-nextest cargo-deny
```

- `mold` + `clang`: fast linking (configured in `.cargo/config.toml`).
- `foot`: a light Wayland terminal, the default test client.
- `wayland-info`, `wev`: inspect the compositor and input events.
- QEMU with KVM: the real DRM session, suspend, several outputs, FreeBSD.
  Preferred over VirtualBox: virtio-gpu with GL, scriptable from the command
  line, no kernel module conflicts with KVM.

## Development loop

- **Nested**: run the compositor in a window of your session; start clients
  inside it. No session restart.
- **Headless**: the same compositor without a screen, driven by scenarios.
- **VM**: the real session on a virtual GPU, for DRM, input devices, suspend
  and FreeBSD.
- **UI crates alone**: `hocus-ui` and `hocus-shell` can be developed in a
  plain window, without the compositor.

Exact commands live in [CLAUDE.md](../CLAUDE.md) and the README as they land.

## Testing safely next to a live session

The developer usually runs their real desktop while testing. A test
compositor must never interfere with it:
- run it with a fresh `XDG_RUNTIME_DIR` (short path: socket paths are limited
  to 108 bytes), `HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, and no session
  D-Bus, unless the test is about D-Bus and starts its own bus;
- never write into the user's real configuration;
- stop test processes by exact PID, never by pattern (`pkill -f` has killed
  the developer's own shell before);
- never kill or signal processes of the live session.

## Working with AI agents

- Agents follow [CLAUDE.md](../CLAUDE.md).
- An agent works on one issue at a time, in its own branch (or worktree),
  writes the scenarios first, then the code, and opens a pull request.
- Agents may push branches and open pull requests; merging, releasing,
  changing repository settings and anything outside the repository is the
  maintainer's call.
- When an agent is unsure about a product or design choice, it asks in the
  issue rather than guessing.
- Agents keep the docs honest: if the code diverges from a document, fix one
  of them in the same pull request.

## Languages

English for code, comments, docs, issues, commits and pull requests. The
interface ships in English and French from the start.
