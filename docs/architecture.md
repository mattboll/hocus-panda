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
- **udev/DRM + libinput through libseat**: the real session on a TTY, in a VM
  first (QEMU virtio-gpu), then on hardware.

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

## Testing

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
