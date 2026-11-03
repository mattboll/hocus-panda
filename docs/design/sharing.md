# Screen sharing — specification (5.11)

Status: approved 2026-11-03 (issue #443), variant A of the 5.11 mockup: a
chooser window. Built since 2026-11-03: see [as built](#as-built). The
chooser's sizes are logical pixels, the mockup's, drawn × 1.5
(`hocus_link::LAUNCHER_ZOOM`) as the launcher's and the capture's are; the
bar's sign and its popover keep the bar's own sizes. 5.12 (issue #445)
groups the sign with the microphone's and the camera's into one sign of
what is in use.

## The approved spec

**The portal**

- The desktop provides `org.freedesktop.impl.portal.ScreenCast` (and the
  `Screenshot` it needs), with a portals config
  (`hocuspanda-portals.conf`) that picks it for this desktop. It streams
  over PipeWire, from the compositor's own rendering, off the loop.
- **Sources**: monitors and windows; virtual sources are out of scope.
- **Cursor modes**: embedded and hidden. Metadata if cheap.
- Browsers offer their own tab sharing once this works; nothing is needed
  for tabs.
- The .deb depends on `xdg-desktop-portal` and Recommends `pipewire`.

**The chooser (variant A)**

- A panel centred on the screen, over a 0.55 darkening, at the launcher's
  zoom (`LAUNCHER_ZOOM`); 560 wide in mockup units, the popovers' glass.
- **Header**: the asking app's icon (from its app id), "<App> veut voir ton
  écran" / "<App> wants to see your screen", and under it the page or app
  detail when the portal gives one (the parent window's title), then
  "choisis ce que tu partages" / "choose what to share".
- **Tabs**: **Écrans** / **Screens** and **Fenêtres** / **Windows**; the
  default follows the request's types.
- **The grid**: 3 columns of live thumbnails (refreshed at most twice a
  second while the chooser is open). Each has a name and a line: the
  screen's size, or the window's desktop ("ce bureau" / "this desktop",
  "bureau 2" / "desktop 2"). Windows on every desktop are listed,
  minimised ones too.
- **Choosing**: a click selects, a double click shares at once. The
  selected thumbnail has the blue ring and tint (`#5B8DEF`).
- **Options**: "Montrer le pointeur" / "Show the pointer", on by default.
- **Buttons**: **Annuler** / **Cancel** and **Partager** / **Share** (blue).
- **Keyboard**: arrows and Enter; Escape cancels.

**While sharing**

- **The shared thing** keeps a 2 px blue ring with a soft glow that
  breathes (2.4 s). It is never in the stream. For a whole screen, the
  ring runs around the screen.
- **The bar**: under the clock, a 48-wide blue item (radius 12,
  `rgba(91,141,239,0.18)`, 1 px `rgba(91,141,239,0.6)` ring) with a
  screen-share icon and "Partage" / "Sharing".
  - Hovering it opens a small popover: "<App> voit ton écran" / "<App>
    sees your screen", "Tu partages la fenêtre « … »" / "You share the
    window "…"", how many notifications are held, and **Arrêter le
    partage** / **Stop sharing**.
  - Stopping ends the portal session.
  - The full form shows the same item under the clock.
- **Notifications** are held as with do not disturb (critical ones still
  show). When sharing ends, one bubble sums them up: "4 notifications
  pendant le partage" / "4 notifications while sharing", with the apps and
  counts, and a link to the history.
- **Several shares** at once each get their own row in the popover.

**Rules**

- Encoding and streaming happen off the loop.
- The lock screen stops all shares.
- A shared window that closes ends its share.

## As built

### 5.11, a whole screen or a window (2026-11-03)

- **Who does what.** The compositor serves the portal and streams; the
  shell shows the chooser, the bar's sign and its popover, and holds the
  notifications. The lines between them are `hocus-link`'s
  (`crates/hocus-link/src/share.rs`): `share-choose` (a request, its
  screen and what it offers), `share-choose-end`, `shares` (what is shared
  and by whom; sent again as the shell connects); from the shell, `share
  thumbs` (where the chooser's thumbnails go), `share chosen`, `share
  cancel` and `share stop`.
- **The portal, in the compositor's process**, on a thread of its own with
  its own runtime (`crates/hocus-compositor/src/share/portal.rs`, zbus),
  on the session bus the shell serves the notifications on: the bus given
  with `--session-bus`, else the session's own on a virtual terminal;
  never a live session's beside a nested or headless desktop. Why there
  and not in a binary of its own: the frames are the compositor's
  rendering, which would otherwise cross processes (screencopy with a
  privileged client, or buffers over a socket), and a share outlives a
  restart of the shell. It owns
  `org.freedesktop.impl.portal.desktop.hocuspanda`, whose object serves
  `ScreenCast` version 5 (`AvailableSourceTypes` monitors and windows,
  `AvailableCursorModes` hidden and embedded), each session's
  `org.freedesktop.impl.portal.Session` (`Close`, `Closed`) and each
  `Start`'s `org.freedesktop.impl.portal.Request` (`Close` cancels the
  chooser). `CreateSession` and `SelectSources` are answered on the
  thread; `Start` goes to the loop, owned, and waits there (not on the
  loop) for the chooser and the stream. `persist_mode` and restore data
  are not kept (every share asks). The bus name is taken as the session
  starts: xdg-desktop-portal, which cannot start it, finds it there.
- **Its files** (`packaging/`): `hocuspanda.portal` (its D-Bus name, the
  `ScreenCast` interface, `UseIn=Hocuspanda`) in
  `/usr/share/xdg-desktop-portal/portals/`, and `hocuspanda-portals.conf`
  in `/usr/share/xdg-desktop-portal/` (ours for `ScreenCast`, GTK's for the
  rest), which xdg-desktop-portal reads for `XDG_CURRENT_DESKTOP` =
  `Hocuspanda`, what `hocuspanda-session` sets and gives systemd's and
  D-Bus's activation environment. The .deb depends on
  `xdg-desktop-portal`, recommends `pipewire` and `xdg-desktop-portal-gtk`
  (the file chooser, which saving a capture uses), and links
  `libpipewire-0.3` (building needs `libpipewire-0.3-dev`).
- **No `Screenshot` portal**: xdg-desktop-portal does not need one for the
  screen cast (checked in its sources and by running it, below), so none
  is served for now; `Screenshot` falls to the default backend.
- **Who asks**: the portal's app id when it gives one (an application
  started from a sandbox or a systemd scope), its window found by that id
  (the focused one first) for its title; an X11 parent window
  (`x11:XID`) by its XWayland window; with no app id, the focused window,
  which the user just asked to share. The shell names it from its desktop
  entry (by its id or its id's last part, `org.mozilla.firefox` or
  `firefox`), with its icon; else the id's last part with a capital;
  else "Une application".
- **The chooser** (`crates/hocus-shell/src/share/`): a layer surface of
  its own (`hocus-share-chooser`), the panel alone, in the overlay layer,
  with no anchor (centred on the screen under the pointer), holding the
  keyboard. The shell draws its panel; the compositor darkens the screen
  under it (`rgba(6,8,14,0.55)`), draws its shadow (`0 22px 60px
  rgba(0,0,0,0.5)`, zoomed), pops it in over 180 ms from 0.97 of its size
  (`cubic-bezier(.2,.8,.2,1)`; at once with reduced motion or on the CPU)
  and keeps the pointer's presses off what it darkens. Sizes: the mockup's
  × 1.5 (840 wide, 24 px in, the header's 48 px icon, the title 22.5 px
  bold, the cards 9 px in, their thumbnails' boxes 129 px tall). The tabs
  are those the request's `types` allow, Windows first when both are. The
  windows are listed most recently used first, every desktop's and the
  minimised ones; a screen is "Écran du portable" (a built-in panel), its
  model's name, else "Écran externe", with its size in pixels. As many
  rows as fit the screen show, three at most; the wheel scrolls by rows.
- **The thumbnails** are the compositor's, drawn over the shell's surface
  in the boxes the shell says (`share thumbs`), every 500 ms while the
  chooser shows (a timer of the loop, removed with it): each screen or
  window drawn again at its own resolution (as the capture draws them,
  5.8) and read back, then averaged down to fit its box, its proportions
  kept and its corners rounded, on a thread of its own; back on the loop,
  a texture over the chooser.
- **The stream** (`crates/hocus-compositor/src/share/stream.rs`): a
  PipeWire output stream per share, on a thread of its own with
  PipeWire's loop (the `pipewire` crate, MIT); a driver, raw video `BGRx`
  (or `BGRA`), its size the screen's pixels or the window's (offered again
  when the window's size changes), any pace up to 60 frames a second
  (`maxFramerate`), its buffers in shared memory PipeWire allocates
  (`MemFd`; no DMA-BUF yet). The loop draws a share's frame
  (`share/frames.rs`) only while a consumer streams, when something
  changed (its own damage, in a buffer kept from frame to frame), and no
  sooner than its pace, and at least once a second while a consumer
  streams (so that one which joined, or lost its first frames while
  being linked, has a picture of a still screen): as the screen shows it
  but without the chooser, the capture's drawing, the toasts and the ring,
  a password manager's windows black, the pointer in it with "Montrer le
  pointeur"; a window alone (its surfaces and popups), the pointer when it
  is over it. **The loop never copies a frame** (16 MB at 2560 × 1600):
  on GLES it asks for the frame's pixels into a pixel buffer of its own
  (`glReadPixels` into a PBO, then an EGL fence and `glFlush`) and goes
  on; a thread of its own (`share/glread.rs`), with a context shared with
  the renderer's and current there only, waits for the fence (a second at
  most: else the buffer is dropped, never mapped), maps the buffer (where
  Mesa copies, about 1.3 ms a frame on the Intel GPU, measured before
  this was moved off the loop), lends its memory to the streams' thread
  and unmaps it once given back (`share/lease.rs`); three buffers a share
  at most, a frame owed when none is free. On the CPU renderer (pixman)
  the share's own buffer is lent, and not drawn into again until given
  back. The streams' thread copies the pixels into a free PipeWire buffer,
  if one is (else the frame is owed and drawn again at the next pace). A
  shared window on another desktop, or minimised, is sent its frame
  callbacks so that it keeps drawing.
- **The ring** (`share/ring.rs`): on GLES, a pixel shader of the mockup's
  `box-shadow`s (a 2 px ring outside the window's edge and its corners, the
  glow its box blurred, outside only), from `0.9, 18px, 0.45` to `0.6, 8px,
  0.2` and back over 2.4 s; in 20 steps a second (a timer of the loop
  while something is shared), each step its own element, so that the ring
  costs a few small redraws a second; none with reduced motion. A window's
  is right over it (windows in front cover it); a screen's on a box 2 px
  in from its edges, radius 6, over everything. On the CPU renderer, the
  ring alone, steady. Never in a stream nor a capture.
- **The bar's sign** (`crates/hocus-shell/src/in_use/`): a surface of its
  own on each bar (`hocus-bar-in-use`), opened as the first share starts
  and removed as the last ends, where the agents' group was (its top 100
  px under the bar's in the full form, 68 in the mini), centred; the
  agents' group and the mini form's agents' item move under it (its height
  and 14 px). `in_use::Uses` is the model of what is in use, by kind (the
  screen's shares for now): 5.12 adds the microphone and the camera to it
  and draws its tiles in the same sign. The sign is lit under the pointer
  and while its popover shows; a 150 ms hover or a click opens its
  popover (`popover/sharing.rs`, 250 wide), as the bar's other items: for
  each share, "Firefox voit ton écran", "Tu partages la fenêtre « … »"
  (or "l'écran du portable", "l'écran « … »") and **Arrêter le partage**,
  which ends its session (`share stop`: the compositor emits the session's
  `Closed`, which xdg-desktop-portal passes on to the application); under
  the last one, how many notifications wait ("3 notifications en
  attente", or "Les notifications attendent la fin").
- **Notifications** (`notifications/stack.rs`): while something is shared,
  a bubble of normal or low urgency is held as with do not disturb (to the
  history, not seen, counted by the bell), a critical one shows. When the
  last share ends, one bubble sums up what was held: "3 notifications
  pendant le partage", "Slack (2), Signal · dans l'historique"; its card
  opens the history (beside the footer's bell); it is the desktop's own
  (no signal goes to the bus for it). Nothing held: no bubble.
- **A share ends** when the user stops it from the bar, when its window
  closes, when its screen is unplugged, when the application (or
  xdg-desktop-portal) closes the session, or when its stream fails
  (PipeWire gone).
- **Choices made here** (to confirm on the machine): with both kinds
  asked, the Windows tab shows first (the mockup's); the pointer is shown
  by default unless the application asked for it hidden; `Tab` switches
  the tabs and `Space` the pointer's option; a click outside the chooser
  does nothing (only Escape or Annuler cancel); the chooser pops in but
  goes at once; the sign has no `rise` (it shows at once); the summary's
  first line says "Hocuspanda · maintenant" as every bubble's first line
  does; the popover's line is cut to one line ("…"); `persist_mode` is
  not honoured (each share asks again).
- **Measured headless** (`crates/hocus-compositor/tests/sharing.rs`): the
  real shell on the CPU, the test calling the portal as xdg-desktop-portal
  does, on a D-Bus daemon of its own, a PipeWire of its own in a short
  runtime directory of its own, each stopped by its process id: a request for screens shows the chooser (its words, its
  Screens tab, the screen selected, the pointer's box ticked) and `Enter`
  returns the screen's stream (its node, its type, its size and place);
  one for windows returns the window's, the pointer off as asked, and the
  window has its ring on the screen; `Escape` returns the cancel
  response; a shared screen has its ring at its edges and a consumer
  linked to its node gets frames without it (the test links it, as
  `pw-link` does: no session manager, whose policy differs between
  versions); the sign shows under the
  clock, its popover says who sees what, and its button ends the share and
  closes the session; three notifications held, then one bubble summing
  them up; a shared window closing ends its share; frames of 2560 × 1600
  reach a consumer with none of their bytes copied on the loop (pixman,
  its buffer lent; counted: the lent pixels read on the loop's thread,
  0; how long a hand-over took is printed, never asserted); on GLES (Mesa's software device, never a GPU: the harness's
  `RendererChoice::GlesSoftware`), the read-back thread's frames come
  whole, upright and in the screen's colours. Unit tests for the
  lines, the chooser's words and its arrows, the cards' sizes, the
  popover's rows, the sign's place, the summary's words and the stack's
  holding, the ring's breath, the pop, the thumbnails' shrinking and the
  stream's formats. By hand, xdg-desktop-portal itself on a private bus,
  the packaged `hocuspanda.portal` and `hocuspanda-portals.conf` found
  ("Using hocuspanda.portal for org.freedesktop.impl.portal.ScreenCast
  (interface specific config)"), an application's `CreateSession`,
  `SelectSources`, `Start` and `OpenPipeWireRemote` through the real
  portal, and GStreamer's `pipewiresrc` taking 1920 × 1080 frames on the
  remote it handed over. On the Intel GPU, the screenshots of the pull
  request.
- **Not built**: the lock screen stopping the shares (the desktop locks
  with the display manager's until it has its own lock screen; then it
  ends them); the cursor as metadata; DMA-BUF buffers (shared memory is
  copied, off the loop, a frame of 2560 × 1600 being 16 MB); the
  chooser's thumbnails off the loop (drawn and read back there twice a
  second while it shows, as the capture's screens are); virtual screens;
  restoring a share (`persist_mode`); the `Screenshot` and `RemoteDesktop`
  portals; xdg-foreign's parent windows (a Wayland application's window,
  which the portal names by its exported handle, is not looked up).

### 5.11b, a portal left by a previous session (2026-11-03)

Issue #538. The user's service manager outlives a graphical session
(lingering, a terminal multiplexer, another login): after logging out of
a previous session and into this one, `xdg-desktop-portal` can still be
the process that session started, with its `XDG_CURRENT_DESKTOP` and
`WAYLAND_DISPLAY`. It reads that session's `*-portals.conf` and sends the
screen cast to a backend that is not ours, and never reads the variables
again: an application's screen share failed while our backend was on the
bus.

- **When**: as the session starts (`--session`), on the thread that gives
  the session's variables (`crates/hocus-compositor/src/session.rs`), right
  after systemd's user manager has them; never on the loop, as it waits
  for `systemctl`. Without systemd (its `import-environment` failing),
  nothing more is done.
- **Which units** (`session/portals.rs`): `xdg-desktop-portal.service`,
  and its backends that run as units, whatever desktop they are for: the
  running units matching `*xdg-desktop-portal-*.service`
  (`xdg-desktop-portal-gtk.service`, …). Not the document portal nor the
  permission store, which do not depend on the desktop.
- **The rule**, for each of them that runs (`ActiveState=active`, a main
  process): its process's environment (`/proc/PID/environ`, what it was
  started with and reads its configuration by) is compared with the
  session's, `XDG_CURRENT_DESKTOP` (`Hocuspanda`) and `WAYLAND_DISPLAY`
  (the socket's name). Either different or unset: restarted. Both the
  same: left, it serves this session. Its environment unreadable:
  restarted if it became active (`ActiveEnterTimestampMonotonic`) before
  the variables reached systemd, left otherwise. The environment comes
  first, rather than when it started, because it is what the portal acts
  on: one activated by an application started with the session, before
  the variables reached systemd, is newer than the session but has the
  previous one's variables; and one started by this desktop on the same
  display (the session started again) serves it as it is.
- **How**: `systemctl --user restart`, the backends first, then the
  frontend, which, started again, finds them serving this session. Each
  restart is logged (`restarted a portal left by a previous session`, the
  unit, its old process and what it had), as a failure to. A unit not
  running is left to D-Bus activation, which starts it with the variables
  systemd now has.
- **Measured**: unit tests for the rule (a previous session's portal, this
  session's left however old, a newer one with the old variables, one
  without them, a value read whole, an unreadable environment by its
  start, a unit not running) and for reading `systemctl`'s output. In the
  Debian VM (`scripts/vm/debian.sh`, its user manager lingering, never a
  live session's): a previous session's compositor left running (headless,
  its own runtime directory), and `xdg-desktop-portal` and
  `xdg-desktop-portal-gtk` started by systemd with
  `XDG_CURRENT_DESKTOP=Previous` and that compositor's display; then the
  session on the virtual terminal (`hocuspanda --session`). Both were
  restarted, the backend first, a second after the variables were given,
  and ran with `XDG_CURRENT_DESKTOP=Hocuspanda` and `WAYLAND_DISPLAY` the
  session's. An application's `CreateSession`, `SelectSources` and `Start`
  through xdg-desktop-portal then reached our backend: the chooser
  showed, Enter returned the screen's stream (1280 × 800 at 0, 0) and
  `OpenPipeWireRemote` its remote. The session stopped and started again,
  the portal already its own: nothing restarted (the same process), and a
  screen cast went through it again.
