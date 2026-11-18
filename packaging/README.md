# Packaging

What the package of Hocuspanda holds, and how it is built.

## What is installed

| File | Where | Why |
|---|---|---|
| `hocuspanda` | `/usr/bin` | The compositor |
| `hocuspanda-shell` | `/usr/bin` | The shell (the bar), which the compositor starts and restarts |
| `hocus-settings` | `/usr/bin` | The Settings (6.1, [docs/design/settings.md](../docs/design/settings.md)), an application window that writes `~/.config/hocuspanda/config.toml`; one window at a time |
| [`hocus-settings.desktop`](hocus-settings.desktop) | `/usr/share/applications` | "Réglages" / "Settings" in the launcher (shown in the Hocuspanda session only) |
| `panda-1.png` … `panda-8.png` | `/usr/share/hocuspanda/avatars` | The pandas the Settings' Account page offers as avatars |
| `hocus-transcribe` | `/usr/bin` | Makes a meeting's text from its recorded sound, at a low priority, which the compositor starts from beside itself ([docs/design/meetings.md](../docs/design/meetings.md)); it downloads its models on first use into `~/.local/share/hocuspanda/models` |
| `hocus-camera` | `/usr/bin` | The Hocuspanda camera: a PipeWire video source with the camera's effects, reading the real camera only while an application reads it, which the compositor starts from beside itself ([docs/design/camera.md](../docs/design/camera.md)) |
| `hocus` | `/usr/bin` | What agents' hooks and scripts run to tell the bar their state ([docs/agents.md](../docs/agents.md)) |
| [`hocuspanda-session`](hocuspanda-session) | `/usr/bin` | What display managers run: once through the user's login shell, then `hocuspanda --session`; it ends when the compositor does, however it ends, so the display manager comes back |
| [`hocuspanda.desktop`](hocuspanda.desktop) | `/usr/share/wayland-sessions` | Display managers (GDM, SDDM, greetd's greeters) offer the "Hocuspanda" session |
| [`hocuspanda.portal`](hocuspanda.portal) | `/usr/share/xdg-desktop-portal/portals` | Screen sharing and the appearance: the desktop's portal backend, `org.freedesktop.impl.portal.ScreenCast` ([docs/design/sharing.md](../docs/design/sharing.md)) and `org.freedesktop.impl.portal.Settings` (`org.freedesktop.appearance`: the theme, the accent, the contrast; [docs/design/settings.md](../docs/design/settings.md)), which the compositor serves on the session bus |
| [`hocuspanda-portals.conf`](hocuspanda-portals.conf) | `/usr/share/xdg-desktop-portal` | Makes xdg-desktop-portal use it in this session (`XDG_CURRENT_DESKTOP=Hocuspanda`), then GTK's for the other settings, GTK's backend for the other portals |
| [`modprobe-hocuspanda-camera.conf`](modprobe-hocuspanda-camera.conf) | `/etc/modprobe.d/hocuspanda-camera.conf` | v4l2loopback's options: one device, number 42, labelled "Hocuspanda" (which `hocus-camera` finds it by, never by its number), `exclusive_caps=1` (browsers list it once written); a conffile |
| [`modules-load-hocuspanda-camera.conf`](modules-load-hocuspanda-camera.conf) | `/etc/modules-load.d/hocuspanda-camera.conf` | Loads `v4l2loopback` at boot, for the Hocuspanda camera's device ([docs/design/camera.md](../docs/design/camera.md)); a conffile |
| [`pam-hocuspanda`](pam-hocuspanda) | `/etc/pam.d/hocuspanda` | The lock screen's PAM service: the system's `common-auth` and `common-account` ([docs/design/lock.md](../docs/design/lock.md)); a conffile |
| [`config.toml`](config.toml) | `/usr/share/doc/hocuspanda` | Every setting with its default, to copy to `~/.config/hocuspanda/` |
| `LICENSE` | `/usr/share/doc/hocuspanda/copyright` | MIT, and the SIL Open Font License of the fonts the shell and the Settings carry in their binaries (Martian Mono, Manrope: `crates/hocus-shell/fonts`; the compositor carries Martian Mono too, for the capture's sizes and countdown); what `hocus-transcribe` links statically: whisper.cpp (MIT), sherpa-onnx (Apache-2.0) and onnxruntime (MIT) |

The binary keeps its symbols, for backtraces.

## .deb

On Debian or Ubuntu, with `dpkg-dev` and the build dependencies of the CI:

```sh
scripts/prebuilt.sh                     # sherpa-onnx's libraries, checked (hocus-transcribe)
cargo build --release --locked -p hocus-compositor -p hocus-shell -p hocus-cli -p hocus-settings -p hocus-transcribe -p hocus-camera
packaging/deb/build.sh                  # target/packages/hocuspanda_<version>-1_<arch>.deb
packaging/deb/check.sh target/packages/hocuspanda_<version>-1_<arch>.deb   # it holds the files above, no others
sudo apt install ./target/packages/hocuspanda_<version>-1_<arch>.deb
```

CI does the same on every push to `main` and to a sprint branch (the
`package` job), and keeps the .deb as the run's artifact. A file added to
or taken out of the package changes [`install.sh`](install.sh), the table
above and [`deb/check.sh`](deb/check.sh) together.

Library dependencies are the ones `dpkg-shlibdeps` finds on the build
system, so a package built on a release installs on that release and later
ones (`libpipewire-0.3` among them, for screen sharing: building needs
`libpipewire-0.3-dev`). EGL is opened at run time and is listed by hand;
so is `xdg-desktop-portal`, which applications ask to share the screen.
Screen recordings are encoded by `ffmpeg`, run as a program of its own
(never linked: its codecs' licences stay its own, and an encoder or a GPU
driver that fails ends the recording, not the session): the package
recommends it, and a VA-API driver for it to encode on the GPU.
"Your day"'s Google calendars come from GNOME Online Accounts
(`gnome-online-accounts`, recommended; [docs/design/day.md](../docs/design/day.md)).
The shell is the session's polkit authentication agent: the package
recommends `polkitd`, whose helper (`polkit-agent-helper-1`, through its
socket `/run/polkit/agent-helper.socket` from polkit 126, else the setuid
program) checks the password; without polkit, no system action asks for
one ([docs/design/polkit.md](../docs/design/polkit.md)).
The Hocuspanda camera writes its frames to a v4l2loopback device too, for
the applications that read only `/dev/video*` (Slack, Teams, Discord, and
Firefox and Chrome without their PipeWire settings): the package
recommends `v4l2loopback-dkms`, which builds the module (GPL, never
linked, a kernel module of its own), and ships its options and its loading
at boot; it never loads or builds the module itself. After installing
`v4l2loopback-dkms`, restart, or `sudo modprobe v4l2loopback`; without the
module, the camera is on PipeWire only. A system that loads v4l2loopback
with other options (another program's virtual camera) has no device
labelled "Hocuspanda": PipeWire only there too.
The session starts the start-up applications itself (6.15): once the bar
shows, once per session, the XDG autostart entries of
`~/.config/autostart` and `/etc/xdg/autostart` (the user's file of a name
winning) that are not hidden, are for this desktop (`OnlyShowIn`,
`NotShowIn`) and whose `TryExec` is there, with
`XDG_CURRENT_DESKTOP=Hocuspanda`. No systemd unit or target is needed, so
it is the same on FreeBSD; systemd's XDG autostart generator is left to
the desktops that start its target.

GDM reads the sessions when its login screen starts: after installing,
log out (or restart GDM) and the gear under the password field lists
"Hocuspanda". GDM shows the gear only when more than one session is
installed.

To check the package against GDM in the Debian VM, logging in by the
keyboard as a person would:

```sh
scripts/vm/debian.sh up
scripts/vm/display-manager.sh target/packages/hocuspanda_<version>-1_<arch>.deb chooser.png session.png
scripts/vm/debian.sh down
```

## When the session does not start

The display manager comes back. Why is in the journal:

```sh
journalctl --user -t hocuspanda -b      # this boot's sessions; -b -1 for the previous boot
```

## Without a package

[`install.sh`](install.sh) installs the same files from a release build,
into `/usr/local` by default (xdg-desktop-portal reads portal files from
`/usr/share` only: with `PREFIX=/usr/local`, screen sharing needs
`XDG_DESKTOP_PORTAL_DIR=/usr/local/share/xdg-desktop-portal/portals` in its
environment):

```sh
sudo packaging/install.sh /
```
