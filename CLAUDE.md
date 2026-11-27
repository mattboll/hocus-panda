# CLAUDE.md

Guidance for AI agents (Claude Code and others) working in this repository.

## Project

Hocuspanda v2: a Wayland desktop for developers who work with AI agents. A
compositor on smithay, and a shell drawn with iced as a separate client.
Linux first. MIT. The first build is kept under the tag `v0-first-build`
as a technical reference only.

Read, in this order: [docs/vision.md](docs/vision.md),
[docs/architecture.md](docs/architecture.md) (the processes, their
sockets, the crates),
[docs/design/tech-choices.md](docs/design/tech-choices.md), the
[decisions](docs/decisions/), and the spec of whatever you work on
([docs/design/bar.md](docs/design/bar.md) for the bar,
[docs/design/notifications.md](docs/design/notifications.md) for the
notifications, [docs/design/sharing.md](docs/design/sharing.md) for screen
sharing).

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
  The Settings and other application windows test their logic (what they
  write, the services they ask); what a page shows and where is validated
  by the maintainer on screenshots, not by new headless tests.

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
  One exception, and only one: when the maintainer asks to see a change
  of the shell live, `scripts/dev/live.sh shell` reloads that program
  alone in their session
  ([docs/workflow.md](docs/workflow.md), "Development loop"); the session
  takes it only with `[developer] live-reload = true` in its
  configuration, off by default, which the maintainer turns on, never an
  agent. Only when
  asked, only through that script: nothing else of the session is touched,
  and the change is still developed and tested headless first. A
  compositor change is not seen this way: it needs the package and a new
  login.
- **Deleting files**: name every path explicitly; no globs or variables in
  `rm`.

## Workflow

- Each sprint has a branch, `sprint/<N>`, based on `main`. An agent takes
  a lot of related issues on one branch, `<type>/<short-name>`, based on
  the sprint branch; it runs the tests of what it changes, and the lot is
  merged into the sprint branch without a pull request. Every push to a
  sprint branch runs the whole suite in CI, and the package the maintainer
  tries is built from it. At the sprint's end, one pull request merges it
  into `main`: it says `Closes #N` for each issue, how they were tested,
  and shows screenshots for anything visible.
- A fix outside a sprint: one branch based on `main`, one pull request.
- Stress runs are not part of a lot: once a week, a timer runs
  `~/github/hocus-panda-tools/weekly-stress hocus-panda` (every test file
  changed in the last 7 days, 20 times in a row under load; its report in
  `~/.local/state/hocus-panda/weekly-stress/`). Each failure there, like
  each test CI reports FLAKY, gets an issue and is quarantined
  the same day (docs/workflow.md, "A test that fails by chance").
- Commit messages in English, `area: what changes`. **No attribution
  trailers** (`Co-Authored-By`, `Claude-Session`, "Generated with…") in
  commits or pull requests.
- Code, comments, docs, issues and PRs in English; user-facing strings
  in the catalogue (`crates/hocus-words/words/*.toml`, one file a
  language, docs/design/settings.md "Words"), shipped in English and
  French; one language type, `hocus_words::Lang`.
- Ask when a product or design choice is unclear; do not guess.

## Style

- Small crates with clear boundaries; the shell does not depend on smithay.
- A shell surface is iced's stock widgets and the shell's kit
  (`crates/hocus-shell/src/ui`, decision 0005; docs/design/bar.md, "The
  shell's widget kit"), a canvas only where the look draws.
- Comments explain why, not what.
- Prefer explicit state machines (enums) over flags.
- `unsafe` only at FFI boundaries, each block with a `// SAFETY:` comment.

## Commands

Building needs, besides Rust, the system's headers the CI installs
(`.github/workflows/ci.yml`), `libpipewire-0.3-dev` among them (screen
sharing's streams), `cmake`, a C++ compiler, `libclang`, the Vulkan
headers (`libvulkan-dev`) and `glslc` (whisper.cpp and its Vulkan backend, built
from their sources for `hocus-transcribe`), and sherpa-onnx's prebuilt libraries,
which `scripts/prebuilt.sh` fetches once and checks (vendor/README.md):
run it before the first build.

```sh
scripts/prebuilt.sh                           # once: sherpa-onnx's libraries, checked
cargo build                                   # build everything
cargo programs                                # the programs the tests run (an alias)
cargo nextest run --workspace                 # tests, on the CPU
cargo nextest run -p hocus-compositor settings_sound::   # one test file's
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo deny check                              # licences and advisories
cargo machete                                 # dependencies no source uses
scripts/vendor.sh --check                     # vendor/ is upstream plus its patch
scripts/ci/docs-names.sh                      # the docs name no crate nor `hocus msg` that is gone
scripts/dev/stress.sh 'test(=today::a_test)'  # by hand only: reproduce a failure, or show its fix holds
```

The Rust version is the one `rust-toolchain.toml` pins; dependencies'
versions are declared once, in the root `Cargo.toml`'s
`[workspace.dependencies]`. What CI runs when, and why a pull request runs
only the tests of what it changes: [docs/workflow.md](docs/workflow.md).
The vendored crate and its patch: [vendor/README.md](vendor/README.md).

A crate's integration tests are one program, `tests/main.rs`, and each
test file a module of it (#765: one link a crate, what the files share
built once): a test is named `FILE::TEST`, and a new test file is declared
in `main.rs` (a test fails while one is not). Select by name
(`settings_sound::`, or `-E 'test(/^bar::/)'` when another file's name
ends the same), never with `--test FILE`. Run them with nextest, each test
in a process of its own: `cargo test` runs a binary's tests as threads of
one process, which these do not support (environment variables, children,
a process's working directory). The compositor's tests run the shell, the
Settings and `hocus` from beside their own program: build
them first with `cargo programs` (CI does, and its archive takes them
along). A test that needs one that is missing, or older than its sources,
fails and names it; none passes without testing.

On a pull request, CI runs the tests of what it changes
(`scripts/ci/pr-tests.sh`): the test files it touches, the unit tests of
the crates whose sources it touches, and those its description names on a
line `Tests: settings_sound, appearance` (test files' names). Name there
the tests of a behaviour the change can affect but whose files it does not
touch. A change to a test harness, `Cargo.lock` or the label `full-tests`
runs everything, as main does: built once, then four jobs. So does
every push to a sprint branch. After each merge, main runs the whole suite; a failure
there is fixed at once; a test that fails by chance is quarantined from
CI (`[profile.ci]` in `.config/nextest.toml`) until its issue's fix
takes it out (docs/workflow.md, "A test that fails by chance": one
retry, reported FLAKY; no fail-fast). A test fails, never skips, when a system program
it runs is missing (`common::missing`); tests and the programs they start
run with `TZ=UTC` and `LANG=C.UTF-8` (`.cargo/config.toml`). Update this
section as commands land.

Tests never draw on a GPU unless `HOCUS_TEST_GPU` names its render node
(docs/performance.md): the compositor draws with pixman, or with GLES on
Mesa's software device for its shaders' tests; test shells draw with
tiny-skia; the shell's motion tests are `#[ignore]`d (they need the
GPU, and fail without it). A test fails if it, or a
process it started, holds a render node open without the variable. Run
the GPU tests on purpose only: alone, one process at a time, on a machine
or VM whose session does not depend on that GPU:

```sh
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor glass:: -j1
# the shell's motion, which only a GPU shows (ignored without one)
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor -j1 --run-ignored only \
    -E 'test(a_minute_change_moves_once) | test(a_change_slides_and_hops) | test(the_secretary_radiates_once)'
```

How each part works and is tested (the compositor, the shell, the
Settings, sharing, the camera, the microphone, the lock screen, the
agents, the phone, the VM, the package…) is in
[docs/dev/reference.md](docs/dev/reference.md), one `##` section per
part: `grep -n '^## ' docs/dev/reference.md`, then read that section only.

## Budget: context and agents

Every turn of an agent reads its whole conversation again: what it reads
early is paid at every later turn. The maintainer's weekly quota is the
limit, so:

- **Read narrowly.** Find with `grep -n`, then read about 40 lines around
  the hit (`sed -n A,Bp`, or Read with offset and limit). Never print a
  whole file over 300 lines. The design docs are long (`docs/design/bar.md`
  is 5,500 lines): read the sections the issue cites. Do not read again
  what this session already read and did not change.
- **Short outputs.** Pipe long command output through `tail -n 40` or
  `grep`; nextest prints its failures at the end.
- **One agent, one ticket.** A brief names the issue, the spec sections and
  the files to start from; it does not paste the spec. An agent stops when
  its ticket is merged; the next ticket gets a fresh agent. When a ticket
  runs long (a hundred commands or more), the agent writes where it stands
  in the issue and stops; a fresh agent continues from that note.

Keep this file short: what every agent needs at every start. The rest goes
in docs/dev/reference.md.
