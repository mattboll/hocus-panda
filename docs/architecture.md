# Architecture

Status: initial design, to be refined issue by issue. Decisions with lasting
consequences get a record in [decisions/](decisions/).

## One process, one loop

Hocuspanda is a single Rust process: the Wayland compositor **and** the whole
shell (widget column, dock, launcher, notifications, quick settings, menus,
lock screen). It is built on [smithay](https://github.com/Smithay/smithay)
(MIT).

```
            ┌─────────────────────────── hocuspanda ───────────────────────────┐
 Wayland  ──┤  calloop event loop — owns ALL state (no locks, no sharing)      │
 clients    │   ├─ smithay: protocols, input, outputs, XWayland                │
            │   ├─ core model: windows, desktops per screen, layout, focus     │
            │   ├─ shell: widgets drawn into the scene, animations             │
            │   └─ control API socket (+ event stream)                         │
            │                ▲ channels (results only)                         │
            │  worker threads: D-Bus, network, audio, files, AI, search        │
            └──────────────────────────────────────────────────────────────────┘
```

Rules:
- **The loop owns the state.** Nothing outside the loop holds a reference to
  it. Worker threads receive owned requests and send owned results back
  through calloop channels. The type system enforces it.
- **Nothing slow on the loop.** No blocking I/O, no D-Bus calls, no process
  spawning that waits. Every loop handler is timed against a frame (16 ms);
  one over budget is logged with its name and duration, and kept in the
  `stats` of the control API with the frame-time histogram of each output.
- **A watchdog that does not bite a quiet screen.** A thread wakes the loop
  every second and checks it answered; after 10 s without an answer it
  writes every thread's state and backtrace (gdb when installed) to
  `$XDG_STATE_HOME/hocuspanda/stall-*.txt`, then logs the stall. It does not
  kill the session (yet): what to do with a stuck loop on a real seat is
  decided with the DRM backend.
- **Logs** go to the terminal and, for a session, to
  `$XDG_STATE_HOME/hocuspanda/hocuspanda.log` (the previous one kept as
  `.old`), written by a worker thread through a bounded queue.
- **Monotonic time everywhere.** Animations, timeouts, idle, caches. Wall
  clock only for what the user reads (the clock, the calendar).
  Compositor time is read from one clock, which tests replace with a
  virtual one that moves only when they advance it.
- **The shell is part of the scene.** Its surfaces are known by identity,
  output and layer; no title matching, no positioning over IPC.

## Crates

A Cargo workspace; names may move as the code lands.

| Crate | Role | Depends on smithay |
|---|---|---|
| `hocus-core` | Pure model: windows, desktops per screen, outputs layout, focus, snapping, placement, rules. Deterministic and unit-tested | no |
| `hocus-compositor` | smithay glue: protocols, backends, input, rendering, XWayland | yes |
| `hocus-ui` | Small retained widget toolkit drawn into textures: layout, text, theme tokens, springs, hit testing, accessibility tree | no |
| `hocus-shell` | The desktop's own UI built on `hocus-ui`: widget column, dock, launcher, notifications, quick settings, menus, lock screen, pandas | no |
| `hocus-settings` | The settings app: a Wayland client of its own, drawn with `hocus-ui`, changing the configuration file ([decision 0010](decisions/0010-settings-as-a-client.md)) | no |
| `hocus-platform` | Services behind traits: power, battery, backlight, network, audio, session, notifications (D-Bus); Linux and FreeBSD implementations | no |
| `hocus-api` | Control protocol types, socket server, event stream; MCP bridge | no |
| `hocus-test` | Scenario runner, test clients, screenshot comparison | yes |
| `hocuspanda` | The binary | yes |
| `hocus` | Command-line tool: `hocus msg`, `hocus scenario`, `hocus mcp`… | no |

Keeping `hocus-core`, `hocus-ui` and `hocus-shell` free of smithay keeps them
fast to compile and test, and lets the UI be developed in a plain window.

## Backends

- **winit (nested)**: the compositor runs in a window of the current session.
  Default for development: no session restart, no VT switch.
- **headless**: no screen; outputs are offscreen buffers (GLES on a render
  node, or the pixman software renderer on CI). Used by every scenario test.
- **tty**: the session itself on a virtual terminal (`--backend tty`, the
  default when started outside a graphical session). libseat (logind or
  seatd) gives the seat, its devices and the switches to other terminals
  (Ctrl+Alt+F1…F12, which pause the session and draw everything again on
  return). The primary GPU's display engine is driven through DRM/KMS
  (smithay's `DrmOutputManager`), drawn with GLES on GBM (Mesa's software GL
  in a VM without 3D); screens plugged and unplugged are followed through
  udev; linux-dmabuf is offered for clients that draw on the GPU. libinput
  reads keyboards and mice. Screens of other GPUs stay dark for now (#123).
  Tried in a QEMU VM on virtio-gpu, then on hardware.

Screens are set the same way on every backend (`outputs.rs`): the backend
lists its screens, lit or off, with their modes, and carries a setting out
on the hardware (a mode on the CRTC, adaptive sync, a buffer size headless);
what follows on the desktop is common — the output's state, the model's
geometry, windows kept in place, relayout, events. Wanted settings come from
`[[outputs]]` and, for where screens are, the `[[layouts]]` entry for the set
connected, or, for the session, from wlr-output-management; they are
settled after the turn of the loop where something changed, and
configurations are answered then. Output globals are retired in two steps
(hidden, destroyed seconds later) so that a client binding one meanwhile is
not cut off.

## Rendering

- Compositing: smithay's GLES renderer with damage tracking.
- Effects (blur, shadows, rounded corners, wobbly mesh, genie, depth of
  field, Matrix): GLES shaders in the compositor, reloadable at runtime in
  development builds.
- Shell UI: widgets rasterised on the CPU into textures and cached, redrawn
  only when their content changes: vello_cpu for drawing, parley for text,
  behind `hocus-ui`'s own painting interface
  ([decision 0009](decisions/0009-ui-rendering-stack.md)).
- Frame pacing per output, driven by presentation feedback. Animations
  advance on that clock and never schedule work per tick.

## Protocols

Besides the core (compositor, xdg-shell, xdg-decoration, data device,
XWayland), the compositor serves the protocols applications and tools
expect: xdg-activation (a recent token brings its window forward),
idle-inhibit, cursor-shape, keyboard-shortcuts-inhibit (granted while the
window has the keyboard), xdg-foreign, presentation-time, fractional-scale,
viewporter, single-pixel-buffer, alpha-modifier, content-type, pointer
constraints and relative pointer, pointer gestures, the virtual keyboard,
the primary selection, ext-data-control for clipboard managers,
ext-foreign-toplevel-list, security-context, and wlr-layer-shell for
third-party bars, launchers and wallpaper tools: background and bottom
surfaces under the windows, top above them (under a fullscreen window),
overlay above everything but the cursor; exclusive zones come off the work
area, and a surface may take the keyboard always or when clicked. Clients
started through a security context (Flatpak and other sandboxes) do not see
layer-shell, the virtual keyboard, data control, the window list nor
security contexts.

Screenshot tools and screen sharing read the screen through
ext-image-copy-capture (output and window sources) and wlr-screencopy
(whole outputs and regions): requests are kept on the loop and served
after each dispatch, where the renderer is, by drawing the output or
window again offscreen into the client's shared-memory buffer; a copy
that waits for damage is served once the output drew a new frame.
xdg-desktop-portal-wlr works on them; the shell's own portal backend and
picker come with #116. While the screen is shared, toasts are held and
one summarises them afterwards. Idle notification (#29) is served;
drm-lease, for VR headsets, waits for an issue of its own.

## Session

What is open (each window's application, output, desktop, place and
state) is saved every thirty seconds when it changed and when quitting, in
`$XDG_STATE_HOME/hocuspanda/session.toml`, written on a worker thread. At
the next start a card offers to put everything back, to choose which
applications, or to start fresh (`[session] restore`); the applications
start again and their windows go back where they were as they come; one
that cannot start, or whose window never comes, is reported in a toast.
XDG autostart entries start too. Only the session itself saves, restores
and autostarts: nested or headless, the developer's session is never
touched.

## Idle and lock

Without input, the curtain dims every screen with a countdown, then the
session locks. Idle inhibitors (shown windows) and fullscreen windows hold
it off; the countdown starts again when they stop. ext-idle-notify tells
third-party tools. Locked, the screens show nothing but the lock: a lock
client's surfaces (ext-session-lock), or our own lock screen, whose
password goes to PAM on a worker thread. A lock client that goes away
without unlocking leaves our lock screen: a crash never unlocks.

## Control API

- UNIX socket next to the Wayland one (`hocus-0.api` beside `hocus-0`),
  JSON lines, versioned, typed in `hocus-api`. Requests (windows, desktops,
  outputs, focus, input injection, screenshots, scene dump) and an event
  stream (subscriptions). Commands the compositor starts get its path in
  `HOCUS_SOCKET`.
- Served on the loop with non-blocking sockets; a client that stops reading
  is dropped past a bound. Slow work (PNG encoding) runs on a worker thread
  and the reply follows when it is done.
- The same API serves the `hocus` tool, the MCP bridge for AI agents, and the
  test harness. Input injection and scene dumps require an explicit test or
  agent permission (`--api-permission test`; agents come with their seat).
- Agents get their own seat (pointer and keyboard) through the API: see the
  agent seat idea in [ideas](ideas.md).
- The settings app ([decision 0010](decisions/0010-settings-as-a-client.md))
  reads what is in effect through it (`config`, `outputs`), follows the event
  stream, and is told which page to show by `open_settings`
  (`hocus settings keyboard`).

## Configuration

`~/.config/hocuspanda/config.toml` (or `--config FILE`), applied live on
change. Defaults live in code (`hocus_core::Config`); the file only holds
what the user changed. A thread looks at the file every half second, reads
and checks it, and hands the loop a configuration ready to apply; a file that
does not read is reported with its line and key (log, `config_error` event,
`hocus msg config`) and the previous configuration stays. Tests use a
configuration file of their own. Every key and the shortcut actions are in
[configuration](configuration.md). Themes in their own files.

## Plugins

Rust crates implementing traits from `hocus-shell` (status module, widget,
launcher provider, action, effect), compiled with the desktop and enabled by
Cargo features and configuration. No scripting language, no dynamic loading.
See [decision 0004](decisions/0004-plugins-compiled-in.md).

## Platform layer

Each service is a trait with a Linux and a FreeBSD implementation:

| Service | Linux | FreeBSD |
|---|---|---|
| Seat, session | libseat (logind or seatd) | libseat (seatd) |
| Device hot plug | udev | devd |
| Power, suspend | logind | acpiconf, `zzz` |
| Battery | UPower / sysfs | `hw.acpi.battery` sysctl |
| Backlight | logind / sysfs | backlight(8) |
| Network | NetworkManager, iwd | ifconfig, wpa_supplicant |
| Audio | PipeWire (wpctl) | OSS mixer(8), PipeWire when present |
| Notifications, portals, screensaver | D-Bus (zbus) | D-Bus (zbus) |

CI builds on Linux and FreeBSD from the start, even when FreeBSD services are
still stubs.

On Linux, quick settings and the status row read UPower (battery),
power-profiles-daemon, NetworkManager and BlueZ on the system bus, following
their property changes, and PipeWire through `wpctl` and the backlight
through sysfs every two seconds; changes go back the same ways (the
backlight through logind). logind asks to lock and unlock, and a delay
inhibitor holds sleep until the lock shows. The ScreenSaver service lets
applications keep the screen awake and ask to lock. Leaving the session
(#72) asks logind what the machine can do (hibernating is offered only
where it works) and which applications block shutting down: their reasons
stop the ten-second countdown until the user decides; the session is then
saved, windows are asked to close, and logind restarts or powers off. The
session is polkit's authentication agent (#83): a request shows a dialog
drawn by the compositor over every window, with the keyboard and the
system's label, so that no client can cover it or imitate it; the password
goes to polkit's own helper (its socket, or the setuid binary of older
versions), which tells polkit. Each source runs on a
thread of its own; a missing one leaves its part of the state as it was.
Only the session itself uses the machine's services: nested or headless,
quick settings show a stub and change nothing on the machine, and tests give
private buses with services of their own.

## Testing

A Debian testing VM (`scripts/vm/`) runs hocuspanda as the session on its
tty1 (virtio-gpu, Mesa's software GL), with keys and mouse motion sent
through QEMU and its screen read back, against real logind, PAM, UPower,
NetworkManager, power-profiles-daemon and PipeWire; it switches terminals,
suspends and wakes.

- **Unit tests** on `hocus-core` and `hocus-ui`: layout, snapping, desktops,
  springs, text layout.
- **Scenarios**: files that describe steps (spawn a test client, move the
  pointer, press keys, plug an output, advance virtual time) and
  expectations (scene state, focus, positions, screenshot within tolerance).
  They run headless on a virtual clock, so animations are deterministic.
  Each runs on its own compositor in-process, through the control API;
  the format is described in `tests/scenarios/README.md`.
- **Test clients**: a small in-repo client that draws known colours and
  reports what it receives, plus real ones (foot, Firefox) for smoke tests.
- **Reference screenshots** reviewed in pull requests.
- **Performance**: frame-time histograms per output and slow handlers,
  exposed as `stats` over the API (`hocus msg stats`); scenarios can assert
  budgets.
- **VMs**: QEMU scripts for the DRM session, suspend/resume, several outputs,
  and FreeBSD.

## Licenses

MIT for the code. Dependencies must be permissive (MIT, Apache-2.0, BSD,
ISC, Zlib, Unicode, CC0; OFL for fonts), checked by `cargo deny` in CI. No
code copied or translated from GPL projects: niri and cosmic-comp (both
smithay-based) are GPL-3.0, as are KWin and Mutter. Read their documentation
and design discussions, not their code. smithay and its `anvil` example are
MIT and can be used freely; sway and wlroots (MIT) and Hyprland (BSD-3) are
permissive but written in other languages.
