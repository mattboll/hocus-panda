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

Building needs, besides Rust, the system's headers the CI installs
(`.github/workflows/ci.yml`), `libpipewire-0.3-dev` among them (screen
sharing's streams).

```sh
cargo build                                   # build everything
cargo nextest run --workspace                 # tests, on the CPU
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo deny check                              # licences and advisories
```

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
takes it out. Update this section as commands land.

Tests never draw on a GPU unless `HOCUS_TEST_GPU` names its render node
(docs/performance.md): the compositor draws with pixman, or with GLES on
Mesa's software device for its shaders' tests; test shells draw with
tiny-skia; the shell's motion tests are skipped. A test fails if it, or a
process it started, holds a render node open without the variable. Run
the GPU tests on purpose only: alone, one process at a time, on a machine
or VM whose session does not depend on that GPU:

```sh
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor --test glass -j1
```

The compositor (`crates/hocus-compositor`, binary `hocuspanda`): nested in
a window when started from a graphical session, the session itself on a
virtual terminal otherwise; `--backend winit|tty|headless` forces one.
Always with a fresh, short runtime directory next to a live session:

```sh
# nested; Alt+drag moves a window, Alt+Super+drag draws on the screen
# (Super+drag on a real session, docs/design/ink.md)
target/debug/hocuspanda --runtime-dir "$(mktemp -d /tmp/hp-XXXX)" --spawn foot
# notifications (docs/design/notifications.md): only on a bus of your own,
# never the live session's; the programs it starts use it (notify-send)
#   --session-bus "$(dbus-daemon --session --print-address --fork)"
# (stop that dbus-daemon by its own PID afterwards)
# nested without touching the live session's screen: in an Xvfb display
Xvfb :87 -screen 0 1600x1000x24 -nolisten tcp &
env -i HOME="$(mktemp -d)" PATH=/usr/bin:/bin DISPLAY=:87 LIBGL_ALWAYS_SOFTWARE=1 \
    XDG_RUNTIME_DIR="$(mktemp -d /tmp/hp-XXXX)" target/debug/hocuspanda \
    --runtime-dir <that same directory> --spawn foot   # drive with xdotool
# headless: offscreen outputs, a private runtime directory by default
target/debug/hocuspanda --backend headless --output 2560x1600@2 --output 1920x1080 \
    --renderer pixman --no-xwayland --spawn foot
# headless on a GPU: GLES on that render node (default: the first GPU)
target/debug/hocuspanda --backend headless --renderer gles --render-node /dev/dri/renderD128
RUST_LOG=hocus_compositor=debug                       # more of what it does
```

Its configuration is `$XDG_CONFIG_HOME/hocuspanda/config.toml` (or
`--config FILE`); so far it reads `[keyboard]` (`layouts = ["fr",
"fr(bepo)"]`, `options`, `repeat_delay`, `repeat_rate`; without layouts,
the program reads `/etc/default/keyboard`), `[outputs.NAME]` (`scale`, on
the real screen; nested and headless take `--output`; `panel-form`),
`[[screen-sets]]` (each set of screens' arrangement, docs/design/windows.md
§12; the sets seen are kept in `$XDG_STATE_HOME/hocuspanda/screen-sets`),
`[shortcuts]` (`terminal = "Super+Return"`, `log-out = "Super+Shift+E"`,
`toggle-bar = "Super+B"`, `clipboard = "Super+V"`, `emoji = "Super+period"`, `capture = "Print"`, `record = "Super+Shift+R"`, the volume and brightness keys, and the
windows' keys of docs/design/windows.md §13), `[programs]` (`terminal`), `[panel]`
(`screens`, `output`, `form`, `windows`), `[recording]` (`format`), `[idle]` (`delay`,
docs/design/idle.md), `[magnifier]` (docs/design/accessibility.md) and
`[appearance]` (`reduced-motion`, `high-contrast`, `reduce-transparency`), `[accessibility]` (`screen-reader`); the shell reads `[day]` from the same file
(`HOCUS_CONFIG`, which the compositor sets), the day's accounts (docs/design/day.md). `packaging/config.toml` lists every setting
with its default (a test checks it). `--session` is what display managers
run (through `packaging/hocuspanda-session`): it tells the user's systemd
and D-Bus where the session is, so never use it next to a live session. Headless tests
start it in-process (`hocus_compositor::Headless`, see
`crates/hocus-compositor/tests/`) with sctk clients on threads.

The shell (`crates/hocus-shell`, binary `hocuspanda-shell`, iced) draws the
bar; the compositor starts it from beside its own binary (or `--shell
PATH`), restarts it when it ends, and talks to it over a private socket
(`crates/hocus-link`). `--no-shell` keeps it off (the bar's space is kept
all the same); the in-process harness starts none unless asked
(`Headless::start_with_shell`). The compositor's tests that ask run the
shell built beside them, which a test binary does not build: build it
first (`cargo build -p hocus-shell -p hocus-settings`, as CI does). They skip without it,
and fail when it is older than its sources. The bar's base:
docs/design/bar.md, "As built".

The shell keeps the clipboard's history in `$XDG_STATE_HOME/hocuspanda/clipboard`
(bar.md, 5.6), its key in the keyring of the bus given with
`--session-bus` only, else in `clipboard.key` beside it: next to a live
session, give the nested compositor a fresh `HOME` (or `XDG_STATE_HOME`).
"Your day" (docs/design/day.md, crate `hocus-day`) keeps the local task
list in `$XDG_STATE_HOME/hocuspanda/tasks.json` and its accounts' caches in
`$XDG_CACHE_HOME/hocuspanda/day`; a `kind = "fixture"` account reads a day
from a file, its times relative to `HOCUS_CLOCK_AT`, for tests and
screenshots. Next to a live session, give the nested compositor a fresh
`HOME`. The bar's "Choisir…" (Day.1) writes the chosen accounts into the
configuration file the compositor read (`--config FILE`, else
`$XDG_CONFIG_HOME/hocuspanda/config.toml`) and keeps "Plus tard" in
`$XDG_STATE_HOME/hocuspanda/day-sources`: next to a live session, give
the nested compositor a configuration file of its own.
The emoji picker (docs/design/emoji.md) keeps its recent emoji and skin
tone in `$XDG_STATE_HOME/hocuspanda/emoji`; its table is generated by
`scripts/emoji/generate.py` from Unicode's and CLDR's files.

The Settings (docs/design/settings.md, crate `hocus-settings`, binary
`hocus-settings [SECTION]`, also `hocus settings [SECTION]`) are an iced
application window of their own: one at a time (a second launch brings
the first forward, by a socket in `XDG_RUNTIME_DIR`). They write the
configuration file the compositor names (`HOCUS_CONFIG`, which it sets for
every program it starts; else `$XDG_CONFIG_HOME/hocuspanda/config.toml`)
at once, in place (crate `hocus-config`, which Day.1's "Choisir…" shares),
and keep the last version that read in
`$XDG_STATE_HOME/hocuspanda/config.last-good.toml` and the last changes in
`settings-recent` beside it: next to a live session, start them with a
configuration file, a `HOME` and an `XDG_STATE_HOME` of their own.
`HOCUS_POWER_SUPPLY_DIR` stands for `/sys/class/power_supply`. They say
what they show in their log (`RUST_LOG=hocus_settings=debug`: "shows" and
"places" lines, JSON), which their tests
(`crates/hocus-compositor/tests/settings.rs`) read; those run the program
built beside them (`cargo build -p hocus-settings`, as CI does).

Screen sharing (docs/design/sharing.md): the compositor serves the
desktop portal's `org.freedesktop.impl.portal.ScreenCast` on the same bus
as the notifications (`--session-bus`, or the session's on a virtual
terminal; none otherwise), and streams to PipeWire's default socket
(`PIPEWIRE_REMOTE`, else `pipewire-0` in the runtime directory): next to a
live session, give the nested compositor a PipeWire of its own, or its
streams appear on your real PipeWire. Its tests
(`crates/hocus-compositor/tests/sharing.rs`) start a dbus-daemon and a
PipeWire of their own (`Config::pipewire_remote`), link their consumer
themselves (no WirePlumber), and skip without them.

The system's authentication dialog (docs/design/polkit.md): the shell is
the session's polkit agent on the system bus only with `HOCUS_AUTH_AGENT`
set, which the compositor sets on a virtual terminal, never nested. Its
tests (`crates/hocus-compositor/tests/polkit.rs`) run a dbus-daemon of
their own with a fake polkit authority, a fake helper
(`HOCUS_POLKIT_HELPER`; with it set, polkit's socket is never used) and
accounts of their own (`HOCUS_AUTH_ETC`).

Screen captures (`Print`, docs/design/screenshots.md) are saved in
`$XDG_PICTURES_DIR` (else `user-dirs.dirs`', else `~/Pictures`): next to a
live session, give the nested compositor a fresh `HOME` (or
`XDG_PICTURES_DIR`). Its tests save in folders of their own
(`Config::pictures`). Screen recordings (`Super+Shift+R`, or the capture's Vidéo; same doc,
5.10) are encoded by `ffmpeg` (a program of its own, which must be
installed) and saved in `$XDG_VIDEOS_DIR` (else `user-dirs.dirs`', else
`~/Videos`), their sound from PipeWire's default socket: next to a live
session, give the nested compositor a fresh `HOME` (or `XDG_VIDEOS_DIR`)
and a PipeWire of its own. Its tests save in folders of their own
(`Config::videos`), decode the files with ffmpeg and ffprobe, and skip
without them. The capture's editor (5.9) asks the desktop
portal's file chooser only on the bus given with `--session-bus` (or the
session's own on a virtual terminal), else saves beside the capture.

The lock screen (docs/design/lock.md): `State::lock_now()` locks (the
tests' `Headless::lock`, `hocus msg lock`); the password is checked by PAM (the package's
`/etc/pam.d/hocuspanda`, else `login`), so a compositor run by hand
checks your real password. Tests use a password of their own
(`Authenticator::test`, the crate's `test-auth` feature, which only its
tests turn on); a build with that feature takes `HOCUS_TEST_AUTH=PASSWORD`
instead of PAM, for screenshots, and is never packaged. The lock's
screenshots:

```sh
HOCUS_SHOTS=/tmp/shots cargo nextest run -p hocus-compositor --test lock_shots --run-ignored only
```

Screen readers (docs/design/screen-reader.md, crate `hocus-a11y`): the
shell describes what it shows on AT-SPI only on the session bus the
compositor names, and only while an assistive technology listens. Try it
on a dbus-daemon of your own with `at-spi-bus-launcher
--launch-immediately` on it (`IsEnabled` set), never on the live
session's buses; `RUST_LOG=hocus_a11y=debug` logs each window's tree,
and `hocus msg a11y` prints it whether a reader listens or not.
`[accessibility] screen-reader` and `Super+Alt+S` start Orca only when the
compositor is the session (tests: `Headless::allow_screen_reader`).

Agents tell the desktop their state through `hocus` (crate `hocus-cli`,
docs/agents.md): `hocus agent NAME STATE`, and `hocus agent-hook TOOL`
which Claude Code's and Codex's hooks run once plugged in from the bar.
It writes to the compositor's control socket, `<socket>.control`
(`HOCUS_SOCKET` for the programs it starts); `--socket PATH` names one:

```sh
target/debug/hocus --socket <runtime dir>/hocus-0.control agent build working
target/debug/hocus --socket <runtime dir>/hocus-0.control msg idle   # idle state, what holds it
target/debug/hocus --socket <runtime dir>/hocus-0.control msg lock   # lock the session now
target/debug/hocus --socket <runtime dir>/hocus-0.control msg a11y   # what screen readers read
```

Idle (docs/design/idle.md): the compositor serves
`org.freedesktop.ScreenSaver` (applications' `Inhibit`) on the same bus as
the notifications, so only on a bus of your own next to a live session;
its tests (`crates/hocus-compositor/tests/idle.rs`) start a dbus-daemon of
their own.

The tools are looked for, and plugged in, in the compositor's `HOME`
(their settings), `PATH` (their programs) and `XDG_CONFIG_HOME` (herdr):
next to a live session, give the nested compositor a fresh `HOME`, or
plugging in writes into your real `~/.claude/settings.json`. Tests give
`Config::agent_places` a home of their own (`tests/agents.rs`).

The real-screen path (DRM/KMS, libinput, logind) in a Debian testing VM
(QEMU with KVM; the base image is cached in
`$XDG_CACHE_HOME/hocuspanda-vm`):

```sh
scripts/vm/debian.sh up        # boot it, user alex / panda
scripts/vm/debian.sh copy      # target/debug's programs into it (says what libraries it lacks)
scripts/vm/debian.sh ssh '~/hocus-vm session'    # the session on its tty1
scripts/vm/debian.sh keys e c h o ret            # its keyboard (QEMU key names)
scripts/vm/debian.sh mouse 60 40; scripts/vm/debian.sh click
scripts/vm/debian.sh screen shot.png             # what its screen shows
scripts/vm/debian.sh ssh '~/hocus-vm log'        # what the session said
scripts/vm/debian.sh down
# two screens, both lit: Virtual-1 1280x800 at scale 1, Virtual-2 2560x1440
# at 1.5 (HOCUS_VM_HEAD1_SIZE, HOCUS_VM_HEAD1_SCALE)
HOCUS_VM_HEADS=2 scripts/vm/debian.sh up
scripts/vm/debian.sh screen shot.png 1           # what the second screen shows
# a second VM beside the maintainer's: its own name and ports
HOCUS_VM_NAME=other HOCUS_VM_SSH_PORT=2532 HOCUS_VM_HTTP_PORT=2590 scripts/vm/debian.sh up
# to try it by hand: its screen in a window, with its keyboard and mouse
HOCUS_VM_DISPLAY=gtk scripts/vm/debian.sh up
```

The package, from a release build, and the session from GDM in the VM:

```sh
cargo build --release --locked -p hocus-compositor -p hocus-shell -p hocus-cli -p hocus-settings
packaging/deb/build.sh                 # prints target/packages/hocuspanda_<version>-1_<arch>.deb
scripts/vm/debian.sh up
scripts/vm/display-manager.sh <that .deb> chooser.png session.png   # GDM, logs in as alex
scripts/vm/debian.sh keys meta_l-ret             # Super+Return: a terminal
scripts/vm/debian.sh keys meta_l-shift-e         # Super+Shift+E: back to GDM
scripts/vm/debian.sh ssh 'journalctl --user -t hocuspanda -b'   # the session's log
scripts/vm/debian.sh down
```
