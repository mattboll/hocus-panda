# How each part works and is tested

Moved out of CLAUDE.md (2026-11-27) so that agents load it on demand, not at
every start. One `##` section per part: list them with
`grep -n '^## ' docs/dev/reference.md` and read only the one you need
(`sed -n A,Bp`). Update the section of what you change as commands land.

## The compositor (hocus-compositor): nested, headless, the session

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

## The compositor's log

Its log is written on stderr by a thread of its own, lines dropped rather
than the loop held. Nested or headless, the programs it starts write
where it does; on a virtual terminal, each into a journal stream of its
own: the desktop's under `hocuspanda`, an application under its id or
program (docs/architecture.md, "Where logs go").

A window asked to come forward is one line, `activation asked`: its
application, process and window, its token (valid, stale, not from
input, unknown), what allowed it (`token`, `dialog`, `tray click`,
`nothing`) and what was done (came forward, its desktop shown; pulsed…);
`journalctl --user -t hocuspanda -g 'activation asked'` collects the
cases of a link that opened out of sight (#966). Its test file is
`activation`.

## The compositor's configuration

Its configuration is `$XDG_CONFIG_HOME/hocuspanda/config.toml` (or
`--config FILE`), followed live for most sections (`src/reload.rs`);
`packaging/config.toml` lists every section and setting with its default,
the one place defaults are defined (`hocus_config::defaults`): the
compositor's and hocus-day's tests check their `Default`s against it, and
the Settings' rows take theirs from it. Without `[keyboard] layouts`, the
compositor reads `/etc/default/keyboard`; nested and headless, screens
take `--output`, not `[outputs.NAME]`. The shell reads `[day]` (the
day's accounts, docs/design/day.md) from the same file (`HOCUS_CONFIG`,
which the compositor sets); the rest it is told on its line. `--session` is what display managers
run (through `packaging/hocuspanda-session`): it tells the user's systemd
and D-Bus where the session is, so never use it next to a live session. Headless tests
start it in-process (`hocus_compositor::Headless`, see
`crates/hocus-compositor/tests/`) with sctk clients on threads.

## The shell (hocus-shell)

The shell (`crates/hocus-shell`, binary `hocuspanda-shell`, iced) draws the
bar; the compositor starts it from beside its own binary (or `--shell
PATH`), restarts it when it ends, and talks to it over a private socket
(`crates/hocus-link`). `--no-shell` keeps it off (the bar's space is kept
all the same); the in-process harness starts none unless asked
(`Headless::start_with_shell`). The compositor's tests that ask run the
shell built beside them, which a test binary does not build: build it
first (`cargo programs`, as CI does). They fail without it, saying so,
and when it is older than its sources. The bar's base:
docs/design/bar.md, "As built".

## The clipboard's history

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
The weather (bar.md "Weather", crate `hocus-weather`): the shell asks
Open-Meteo for the forecast of `[weather] place` every 30 minutes and keeps
it in `$XDG_CACHE_HOME/hocuspanda/weather.json`; next to a live session,
give the nested compositor a fresh `HOME` (or `XDG_CACHE_HOME`).
`HOCUS_WEATHER_FIXTURE=FILE` reads a saved answer instead of the network
(`crates/hocus-weather/fixtures/forecast.json`, 19 November 2026 in Paris),
and `HOCUS_WEATHER_PLACES=FILE` the Settings' place search; tests and
screenshots never reach the network.
The emoji picker (docs/design/emoji.md) keeps its recent emoji and skin
tone in `$XDG_STATE_HOME/hocuspanda/emoji`; its table is generated by
`scripts/emoji/generate.py` from Unicode's and CLDR's files.

## The Settings (hocus-settings)

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
`HOCUS_POWER_SUPPLY_DIR` stands for `/sys/class/power_supply`. Their
Calendar and tasks page reads Online Accounts and keeps a Planka board's
token on the bus `DBUS_SESSION_BUS_ADDRESS` names: next to a live
session, start them without it or with a bus of your own (its tests:
`tests/settings_agenda.rs`, the day's fakes). They say
what they show in their log (`RUST_LOG=hocus_settings=debug`: "shows" and
"places" lines, JSON), which their tests
(`crates/hocus-compositor/tests/settings.rs`) read; those run the program
built beside them (`cargo programs`, as CI does).

## Screen sharing

Screen sharing (docs/design/sharing.md): the compositor serves the
desktop portal's `org.freedesktop.impl.portal.ScreenCast` on the same bus
as the notifications (`--session-bus`, or the session's on a virtual
terminal; none otherwise), and streams to PipeWire's default socket
(`PIPEWIRE_REMOTE`, else `pipewire-0` in the runtime directory): next to a
live session, give the nested compositor a PipeWire of its own, or its
streams appear on your real PipeWire. Its tests
(`crates/hocus-compositor/tests/sharing.rs`) start a dbus-daemon and a
PipeWire of their own (`Config::pipewire_remote`), link their consumer
themselves (no WirePlumber), and fail without them.

## The camera (hocus-camera)

The Hocuspanda camera (docs/design/camera.md, crate `hocus-camera`): the
compositor starts `hocus-camera` from beside itself on a virtual terminal
only (nested or headless with `--camera`, else none, or
`Config::camera_program`); its "Hocuspanda" source appears on PipeWire's
default socket (`PIPEWIRE_REMOTE`, or the compositor's `pipewire_remote`):
next to a live session, give it a PipeWire of its own, or the source
appears on your real PipeWire, and a reader of it reads your real camera.
Its tests (`crates/hocus-compositor/tests/camera.rs`) start a PipeWire of
their own with a fake camera node of the test's, its frames on the CPU
(`Config::camera_device = "cpu"`); its GPU path is tested on lavapipe
alone (`cargo nextest run -p hocus-camera gpu::`, the test showing
Vulkan's loader that driver only). Its face's and hands' tracking is held
to its time budget only with `HOCUS_TEST_BUDGETS=1`, on a real machine
(docs/performance.md; CI's runners are slower). The window's screenshots:

```sh
HOCUS_SHOTS=/tmp/shots cargo nextest run -p hocus-compositor camera::screenshots --run-ignored only
```

## The microphone (hocus-voice)

The Hocuspanda microphone (docs/design/voice.md, crate `hocus-voice`):
the compositor starts `hocus-voice` as it starts the camera (a virtual
terminal, else `--voice`, or `Config::voice_program`); its "Hocuspanda"
source appears on PipeWire's default socket, and a reader of it reads the
default microphone of that PipeWire: next to a live session, give it a
PipeWire of its own. Its tests (`crates/hocus-compositor/tests/voice.rs`)
start a PipeWire of their own with a fake microphone of the test's; the
voices' own tests are `hocus-voice`'s. The listening samples:

```sh
HOCUS_VOICE_SAMPLES=/tmp/voices cargo nextest run -p hocus-voice --run-ignored only listening_samples
```

## The authentication dialog (polkit)

The system's authentication dialog (docs/design/polkit.md): the shell is
the session's polkit agent on the system bus only with `HOCUS_AUTH_AGENT`
set, which the compositor sets on a virtual terminal, never nested. Its
tests (`crates/hocus-compositor/tests/polkit.rs`) run a dbus-daemon of
their own with a fake polkit authority, a fake helper
(`HOCUS_POLKIT_HELPER`; with it set, polkit's socket is never used) and
accounts of their own (`HOCUS_AUTH_ETC`).
The shell is NetworkManager's secret agent under the same gate (#930,
polkit.md "NetworkManager's secrets"), keeping agent-owned secrets in the
keyring of the bus the compositor names; its tests
(`crates/hocus-compositor/tests/nm_secrets.rs`) run a fake NetworkManager
and a fake keyring on dbus-daemons of their own, never the machine's.

## Project desktops

Project desktops (docs/design/projects.md) are kept in
`$XDG_STATE_HOME/hocuspanda/projects` by the compositor as soon as one is
made: next to a live session, give the nested compositor a fresh
`XDG_STATE_HOME` (or `HOME`). Tests make them on the Settings' line
(`project-make SCREEN DESKTOP NAME FOLDER`).

## Screen captures

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
(`Config::videos`), decode the files with ffmpeg and ffprobe, and fail
without them. The capture's editor (5.9) asks the desktop
portal's file chooser only on the bus given with `--session-bus` (or the
session's own on a virtual terminal), else saves beside the capture.

## The lock screen

The lock screen (docs/design/lock.md): `State::lock_now()` locks (the
tests' `Headless::lock`, `hocus msg lock`); the password is checked by PAM (the package's
`/etc/pam.d/hocuspanda`, else `login`), so a compositor run by hand
checks your real password. Tests use a password of their own
(`Authenticator::test`, the crate's `test-auth` feature, which only its
tests turn on); a build with that feature takes `HOCUS_TEST_AUTH=PASSWORD`
instead of PAM, for screenshots, and is never packaged. The lock's
screenshots:

```sh
HOCUS_SHOTS=/tmp/shots cargo nextest run -p hocus-compositor lock_shots:: --run-ignored only
```

## Screen readers (hocus-a11y)

Screen readers (docs/design/screen-reader.md, crate `hocus-a11y`): the
shell describes what it shows on AT-SPI only on the session bus the
compositor names, and only while an assistive technology listens. Try it
on a dbus-daemon of your own with `at-spi-bus-launcher
--launch-immediately` on it (`IsEnabled` set), never on the live
session's buses; `RUST_LOG=hocus_a11y=debug` logs each window's tree,
and `hocus msg a11y` prints it whether a reader listens or not.
`[accessibility] screen-reader` and `Super+Alt+S` start Orca only when the
compositor is the session (tests: `Headless::allow_screen_reader`).

## Agents' states (hocus agent, hooks)

Agents tell the desktop their state through `hocus` (crate `hocus-cli`,
docs/agents.md): `hocus agent NAME STATE`, and `hocus agent-hook TOOL`
which Claude Code's and Codex's hooks run once plugged in from the bar;
their good news with `hocus celebrate WHY` (the moments,
docs/design/effects.md). It writes to the compositor's control socket, `<socket>.control`
(`HOCUS_SOCKET` for the programs it starts), a public interface whose
lines, version and events are docs/api.md (its examples are tests:
`api::`); `--socket PATH` names one:

```sh
target/debug/hocus --socket <runtime dir>/hocus-0.control agent build working
target/debug/hocus --socket <runtime dir>/hocus-0.control msg idle   # idle state, what holds it
target/debug/hocus --socket <runtime dir>/hocus-0.control msg lock   # lock the session now
target/debug/hocus --socket <runtime dir>/hocus-0.control msg a11y   # what screen readers read
```

## A meeting's transcription

A meeting's transcription (docs/design/meetings.md, crates
`hocus-meetings` and `hocus-transcribe`): the card's Transcrire records
the meeting's two tracks into `$XDG_STATE_HOME/hocuspanda/meetings`, from
PipeWire's default socket (`PIPEWIRE_REMOTE`), then runs
`hocus-transcribe` from beside the compositor (or `Config::transcriber`),
which hears on a GPU through Vulkan (a discrete one first; `[meetings]
device`) else on the CPU, downloads its models (610 MB) on first use into
`$XDG_DATA_HOME/hocuspanda/models` and writes the text in
`$XDG_DOCUMENTS_DIR`'s "Réunions" (else `user-dirs.dirs`', else
`~/Documents`); its notifications go to the bus given with
`--session-bus`. Next to a live session, give the nested compositor a
fresh `HOME` (or `XDG_STATE_HOME`, `XDG_DATA_HOME` and
`XDG_DOCUMENTS_DIR`), a PipeWire of its own (else it records your real
microphone and calls) and a bus of its own. Its tests
(`tests/transcribe.rs`) run the fake engine (`HOCUS_TRANSCRIBE_FAKE=FILE`,
fixed lines, no model nor device); the real one's test is ignored, and
runs on the CPU, or on Mesa's CPU Vulkan (lavapipe), never a real GPU next
to a live session (a mount namespace with an empty /dev/dri keeps the GPUs
out of reach):

```sh
HOCUS_TEST_MODELS=/tmp/models cargo nextest run -p hocus-transcribe --run-ignored only real::
# the Vulkan path, on lavapipe alone
HOCUS_TRANSCRIBE_TEST_GPU=1 GGML_VK_VISIBLE_DEVICES=0 \
    VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json HOCUS_TEST_MODELS=/tmp/models \
    unshare -r -m sh -c 'mount -t tmpfs none /dev/dri && exec "$@"' sh \
    cargo nextest run -p hocus-transcribe --run-ignored only real::
```

## Idle

Idle (docs/design/idle.md): the compositor serves
`org.freedesktop.ScreenSaver` (applications' `Inhibit`) on the same bus as
the notifications, so only on a bus of your own next to a live session;
its tests (`crates/hocus-compositor/tests/idle.rs`) start a dbus-daemon of
their own.

## The text size

The text size (`[appearance] text-scale`, docs/design/settings.md 6.2):
besides the settings portal, the session on a virtual terminal writes
GNOME's `org.gnome.desktop.interface text-scaling-factor` into the
user's own dconf for GTK applications (`gsettings`,
`Config::gnome_settings`); a nested or headless compositor never does, so
it never touches the live session's dconf. Its tests
(`tests/appearance.rs`) give a fake `gsettings` of their own; never run
`gsettings set` or `dconf write` next to a live session.

## Removable drives

Removable drives (docs/design/bar.md "Removable drives", crate
`hocus-removable`): the shell follows UDisks2 on the system bus, and
mounts, unmounts and powers off what the user asks only: a nested shell
reads the machine's UDisks2, so never click "Ouvrir" or ⏏ there on a
drive of yours you did not mean to. Its tests
(`crates/hocus-compositor/tests/removable.rs`) serve a fake UDisks2
(`tests/levels/udisks.rs`: the computer's disk, a loop device, a stick
plugged in when the test says) on the fakes' private bus, which the shell
takes as its system bus (`DBUS_SYSTEM_BUS_ADDRESS`), with an `xdg-open`
of their own; `HOCUS_DRIVES_FREE` (bytes) stands for statvfs.

## The phone (hocus-phone)

The phone (docs/design/bar.md "The phone", crate `hocus-phone`): the
shell follows KDE Connect's daemon on the session bus the compositor names
(none nested without `--session-bus`), never starting it, and serves KDE's
job view there (`org.kde.kuiserver`, `org.kde.JobViewServer`): next to a
live session, give the nested compositor a bus of its own, or it reads
your real phone, and "Faire sonner" or "Oublier" act on it. Its tests
(`crates/hocus-compositor/tests/phone.rs`) serve a fake KDE Connect
(`tests/levels/kdeconnect.rs`: paired phones and tablets, their plugins,
the phone's notifications sent to the shell as `kdeconnectd` sends them,
a pairing, a transfer reported to the job view) and a fake portal file
chooser on the fakes' private bus, which the shell takes as both its
system bus and its session bus, with an `xdg-open` of their own; a
`services` folder beside a private bus's socket makes its names
activatable (`common::private_bus`).

## The MCP server (hocus mcp) and agents' seats

`hocus mcp --agent NAME` (docs/design/mcp.md) is an MCP server on stdio
for an agent of the configuration's `[[agents.mcp]]`, on the same control
socket; its tests (`tests/mcp.rs`) run it beside them (`cargo programs`).
Each agent gets a `wl_seat` of its own (`agent-NAME`), kept for the
session and taken again by its next connection; it
reads applications' trees on the accessibility bus the session's bus
names (none nested without `--session-bus`; tests give
`Config::a11y_bus` a private bus with a fake registry,
`tests/common/atspi.rs`), never the live session's. The agents' rights
are set in Settings › Act on the desktop (`hocus settings act-on-desktop`),
whose « Brancher » runs `claude mcp add --scope user` from the `PATH` and
`HOME` the Settings were given: next to a live session, start them with a
`HOME` and a `PATH` of their own, or it writes into your real
`~/.claude.json`. Their tests (`tests/settings_acting.rs`) put a fake
`claude` on theirs. `hocus_config::labels` (every key's name, as the
Settings' rows say it, for the agents' toast) is generated from the
Settings' model: `HOCUS_LABELS_WRITE=1 cargo test -p hocus-settings
the_labels_are_the_rows` after changing a row.

## Tools looked for in the compositor's HOME

The tools are looked for, and plugged in, in the compositor's `HOME`
(their settings), `PATH` (their programs) and `XDG_CONFIG_HOME` (herdr):
next to a live session, give the nested compositor a fresh `HOME`, or
plugging in writes into your real `~/.claude/settings.json`. Tests give
`Config::agent_places` a home of their own (`tests/agents.rs`).

## The real-screen path in a Debian VM

The real-screen path (DRM/KMS, libinput, logind) in a Debian testing VM
(QEMU with KVM; the base image is cached in
`$XDG_CACHE_HOME/hocuspanda-vm`):

```sh
scripts/vm/debian.sh up        # boot it, user alex / panda
scripts/vm/debian.sh copy      # target/debug's programs into it (says what libraries it lacks)
scripts/vm/debian.sh ssh '~/hocus-vm session'    # the session on its tty1
scripts/vm/debian.sh ssh 'echo --renderer pixman > ~/.hocus-args'  # its next session on the CPU
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

## The package, and the session from GDM in the VM

The package, from a release build, and the session from GDM in the VM:

```sh
cargo build --release --locked -p hocus-compositor -p hocus-shell -p hocus-cli -p hocus-settings -p hocus-transcribe -p hocus-camera -p hocus-voice -p hocus-charge
packaging/deb/build.sh                 # prints target/packages/hocuspanda_<version>-1_<arch>.deb
scripts/vm/debian.sh up
scripts/vm/display-manager.sh <that .deb> chooser.png session.png   # GDM, logs in as alex
scripts/vm/debian.sh keys meta_l-ret             # Super+Return: a terminal
scripts/vm/debian.sh keys meta_l-shift-e         # Super+Shift+E: back to GDM
scripts/vm/debian.sh ssh 'journalctl --user -t hocuspanda -b'   # the session's log
scripts/vm/debian.sh ssh 'journalctl --user -t foot -b'         # an application's (its id or program)
scripts/vm/debian.sh down
```
