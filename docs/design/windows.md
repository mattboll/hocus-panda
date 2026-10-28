# Windows and desktops — specification (slice 1, draft for review)

Status: approved 2026-10-28, after the prototype on the design canvas. Slice 1 of the
[vision](../vision.md)'s order. The bar is slice 2 and has its own
[spec](bar.md); this slice leaves its space reserved (196 px on the right
when the output's default form is full, 56 px otherwise) so that nothing
moves when it arrives.

## 1. Scope

In this slice: windows (open, close, focus, move, resize, snap, maximize,
fullscreen, minimize), virtual desktops per screen, Alt+Tab, and the
session from GDM on one screen.

Not in this slice: the bar, launcher, notifications, lock screen (the
session locks with the display manager's lock until slice 5), several
screens plugged and unplugged (slice 6; windows stay on the screen they
open on), agents' badges on windows (slice 3).

## 2. Units and the work area

- All sizes are logical pixels and follow the output's scale.
- The **work area** is the output minus the bar's reserved width and an
  8 px gap on every side. Windows placed by the desktop (snapped,
  maximised, new) keep 8 px from the work area's edges and from each
  other.

## 3. A window's look

- Corner radius 8 px. Client-side decorated windows (GTK, libadwaita,
  Firefox, Slack) keep their own header; server-side decorated ones (foot,
  kitty when it asks) get no title bar (section 7 for moving them).
- A 1 px rim: `rgba(255,255,255,0.18)` unfocused, the accent (`#6CC287`)
  focused. The rim is drawn outside the window's content, so a terminal
  with no padding keeps its first column.
- A shadow: offset 0, 12 px down; blur 36 px; black at 35 % unfocused,
  45 % focused.
- No transparency at any time, moving included.

## 4. Focus

- Click to focus. A click on an unfocused window focuses it and is passed
  to the application (no click is swallowed).
- A new window takes the focus when the user started it (an activation
  token from the user's last key or click). Otherwise it does not steal
  the focus: it opens behind the focused window, and its rim pulses once
  in the accent (600 ms) so it is noticed.
- Focus moves with Alt+Tab (section 11) and the pointer. Focusing the
  window on a side is an action of `[shortcuts]` (`focus-left`,
  `focus-right`, `focus-up`, `focus-down`) with no default key:
  `Super+Ctrl+←/→` switch desktops (section 12; the maintainer's decision
  on #323).
- The focused window's rim changes in 120 ms.

## 5. Opening and closing

- **Placement:** the centre of the work area of the output under the
  pointer. When a window of the same application is already there, offset
  by 32 px down and right. An application's last size is remembered per
  app id; its position is not (it opens where the user is).
- **Opening:** from 94 % scale and 0 opacity to 100 % and 1, in 220 ms,
  spring (`cubic-bezier(0.2, 0.9, 0.25, 1.08)`), scaling from the window's
  centre.
- **Closing:** to 97 % and 0 opacity in 160 ms, ease-in. The window's last
  frame is what fades; the application is not waited for.
- Reduced motion: opening and closing fade in 120 ms, without scaling.

## 6. Moving, and the wobbly windows

- Moving: drag a client-side header, or `Super`+drag anywhere in the
  window, or the grip (section 7).
- While a window moves, it **wobbles**: a soft spring mesh (4×4 control
  points) where the grabbed point follows the pointer exactly and the far
  side lags, then settles in about 400 ms after the pointer stops.
  Stiffness and damping are tuned once on the user's machine and fixed,
  with the user's current desktop's wobble as the reference to match or
  beat (the prototype's was found a little less pleasant).
- The shadow deepens while moving (blur 48 px, 50 %). No transparency.
- With reduced motion: no wobble; the window moves rigidly.

### Magnetic edges

While a window moves, each of its edges **sticks** to a nearby edge when
it comes within **12 px** of it:

- an edge of another window shown on the same screen and desktop (not
  minimised, not maximised or fullscreen), at the **8 px gap** snapped
  windows keep: a window's left edge sticks 8 px right of the other's
  right edge, its top 8 px below the other's bottom, and so on;
- an edge of another such window on the same side, in line with it: left
  to left, top to top;
- an edge of the work area, flush with it, as snapped windows are.

The nearest wins, across and down on their own: a window can stick to one
window's side and line up with another's top. A window pulls only where
the two overlap the other way, or come within the gap and the 12 px of
it, so a window far above does not pull sideways. The magnet applies to
where the pointer would put the window, nothing accumulates: dragging on
past the 12 px frees it, with no modifier. Where the pointer is in a snap
zone (section 9), snapping keeps priority and nothing sticks. The wobble
follows the pointer's motion, not the magnet's jumps: a window catching
or coming free jumps rigidly, and one held still by the magnet does not
bend. Reduced motion changes nothing here.

## 7. Server-side decorated windows: the grip

A window with no header of its own still needs a mouse handle. Proposal:
a **grip**, a pill 44×4 px centred 4 px inside the top edge, that appears
in 120 ms when the pointer is in the window's top 12 px, and disappears
300 ms after it leaves. Dragging it moves the window; double-clicking it
maximises or restores. Chosen over a thin title bar after trying both in
the prototype. The top resize band is the window's first 4 px; the grip
sits just below it.

## 8. Resizing

- Resize zones on all four edges and all four corners (the prototype's
  first version had only the bottom and right ones, and that was felt at
  once): an 8 px band outside each edge, 16×16 px squares at the corners,
  with the matching resize cursors (`ew-resize`, `ns-resize`,
  `nwse-resize`, `nesw-resize`).
- `Super`+right-drag resizes from the nearest corner.
- Live resize, respecting the application's minimum and maximum sizes.
- The edge or edges being dragged stick as a moving window's do (section
  6, magnetic edges): within 12 px of another window's edge, at the gap
  or in line, or of the work area's, flush. Dragging on frees them. The
  windows that follow a seam's handle do not pull.
- Snapped and maximised windows resize too. A maximised window resized
  floats.
- A **snapped pair** (two snapped windows side by side, or one above the
  other, facing each other across the gap):
  - Dragging their shared edge resizes **only the window grabbed**, like
    any edge: in the gap, the window whose edge is nearer. Once its size
    changes it leaves its zone and floats; the other stays snapped where
    it is.
  - A **handle** on the seam moves the boundary and **both windows
    follow**. It is the grip's pill (section 7) along the seam: 4×44 px
    upright between windows side by side, 44×4 px between windows one
    above the other, centred in the 8 px gap at the middle of the span
    the two windows share. It shows when the pointer is within 12 px of
    the seam's centre line (the gap and a little of each window) and
    fades in and out as the grip does (120 ms in; it stays 300 ms after
    the pointer leaves, then fades in 120 ms). A press within 4 px around
    the pill takes it (a 12×52 px target), with `ew-resize` or
    `ns-resize`. Where a half faces two quarters, its handles move the
    half's whole edge and both quarters follow.
  - With reduced motion and high contrast, the handle is as the grip.

## 9. Snapping

While dragging a window:

| Pointer at | Preview | Result |
|---|---|---|
| Left or right edge of the work area | that half | half width, full height |
| Top edge | the whole work area | maximised |
| A corner (within 48 px of it) | that quarter | quarter |

- The preview is a rounded rectangle (radius 8) filled with the accent at
  12 % and a 1 px accent rim. It appears in 120 ms after the pointer has
  stayed 80 ms in the zone, so passing across the edge does not trigger it.
- Releasing animates the window into place in 200 ms (spring); leaving the
  zone removes the preview in 120 ms.
- Dragging a snapped or maximised window away restores its previous size
  under the pointer, at the same relative grab point.

Keyboard:

| Keys | Action |
|---|---|
| `Super+←` / `Super+→` | left / right half; again: move to the other side |
| `Super+↑` | maximise |
| `Super+↓` | restore; again: minimise |
| `Super+Shift+←/→/↑/↓` | quarters, by the corner the arrows point to |

## 10. Maximise, fullscreen, minimise, close

- **Maximise** fills the work area (8 px gaps, bar's space kept). Animated
  like a snap.
- **Fullscreen** (the application asks, or `F11` in it) covers the whole
  output, the bar's space included. When the bar has something to say
  (slice 2's halo), a 4 px line in the halo's colour shows on the
  output's right edge.
- **Minimise** (`Super+H` or `Super+↓` twice): the window shrinks toward
  the bar's area in 220 ms and fades. It comes back with Alt+Tab or the
  desktop's list.
- **Close**: `Super+Q`, or the application's own button. Closing never
  kills the application: it asks it to close.

## 11. Alt+Tab

- `Alt+Tab` held: after 100 ms, a panel appears centred on the output
  under the pointer, with a live thumbnail per window (up to 220 px wide,
  aspect kept), the window's title below; no application icon, the
  thumbnail already shows the window. A quick
  `Alt+Tab` (released before 100 ms) switches to the previous window
  without showing anything.
- Order: most recently used first, on the current desktop; then a
  separator and the other desktops' windows (dimmed, with the desktop's
  number). Minimised windows are in the list.
- `Tab` / `Shift+Tab` move the selection; releasing `Alt` switches to it
  (and to its desktop); `Esc` cancels; a click on a thumbnail switches.
- The panel appears in 150 ms (fade and 96 % → 100 % scale) and vanishes
  in 100 ms. The selection's ring moves in 120 ms.
- As built: a thumbnail fits in 220 × 150 px, its aspect kept, centred in
  its item (236 px wide with its 8 px padding), the items 14 px apart on
  one line, their titles lined up; the panel has 16 px of padding, an
  18 px radius, a dark fill at 86 % with a 1 px white rim at 12 % and a
  deep shadow, and shrinks as a whole when the windows do not fit the
  screen. The selection is the accent at 18 % with a 2 px accent ring.
  Titles are the application's, ellipsed; another desktop's number
  follows in grey (`Firefox · 2`). Other desktops' and minimised windows
  show at 60 %. `←`/`→` move the selection too, `Return` switches.
- Nested, `Alt+Tab` is `Alt+Super+Tab`, so that the host keeps its own.
- Reduced motion: the panel fades without scaling, the ring jumps.

## 12. Virtual desktops

- **Per screen**: each output has its own desktops and switches alone
  (first build's decision, kept). 4 desktops by default (1 to 9).
- Switching: `Super+1…4`, `Super+Ctrl+←/→`, a three-finger horizontal
  swipe on a touchpad (following the fingers), and later the bar's tiles.
  Keys and swipes act on the screen under the pointer.
- **Animation**: the desktops sit side by side, in their order, like one
  strip. Switching slides the strip: the outgoing desktop's windows move
  out on one side while the incoming one's move in from the other, in
  320 ms (`cubic-bezier(0.22, 0.9, 0.3, 1)`), with no fade or dimming: the
  windows never disappear, they travel. Going from 1 to 3 slides past 2.
  A swipe follows the fingers, then completes or springs back on release.
  (Changed after the maintainer compared it with their current desktop.)
- As built: 24 px between desktops on the strip; a desktop's width of
  strip per 360 units of finger travel (libinput's, a few millimetres
  each); released past a quarter of a desktop, or on a flick (1.2
  desktops a second), the swipe goes on to the next desktop, otherwise it
  springs back, in the same 320 ms and curve; past the first and the last
  desktop the strip barely follows. Three fingers going down, and four,
  are left to applications.
- **Which desktop shows**: while the strip moves (keys, swipe, a window
  sent), dots at the top centre of the screen show the desktops: 8 px
  high, 6 px apart, 18 px from the top, the current one 22 px wide in the
  accent, the others 8 px in white at 35 %; the wide dot follows the
  strip as it slides. They fade in and out in 300 ms around the motion
  (the prototype's). The bar's tiles will show it at all times.
- Reduced motion: the desktops cross-fade in 120 ms instead of sliding; a
  swipe does not follow the fingers, the release decides.
- `Super+Shift+1…4` sends the focused window to that desktop, without
  switching: it leaves the screen toward its desktop's side, a screen's
  width in 320 ms easing in, as leaving windows do (it fades with reduced
  motion), and the dots show. With `Ctrl`
  too, it follows it: the window stays in place as the strip slides
  under it. A window being dragged while desktops switch goes along.
- Empty desktops stay; their order is fixed (1 to 4). `[desktops] count`
  sets how many, from 1 to 9, on every screen.

## 13. Shortcuts

All use `Super`, as in the user's current desktop. They are listed in one
place in the configuration and can be changed. In nested mode (the
desktop running in a window of another session) the modifier is
`Alt+Super`, so the host's shortcuts don't take them.

The configuration's `[shortcuts]` section holds them, by action; an empty
string turns one off. The package ships every setting with its default
(`/usr/share/doc/hocuspanda/config.toml`, from `packaging/config.toml`).

| Action (`[shortcuts]`) | Default | What it does |
|---|---|---|
| `terminal` | `Super+Return` | Opens a terminal: `[programs] terminal`, or the first of kitty, foot, alacritty, xterm found in `PATH` |
| `log-out` | `Super+Shift+E` | Ends the session at once; the display manager comes back |
| `close` | `Super+Q` | Asks the focused window to close (§10) |
| `minimize` | `Super+H` | Minimises the focused window (§10) |
| `snap-left`, `snap-right` | `Super+←`, `Super+→` | Left or right half; again, the other side (§9) |
| `maximize` | `Super+↑` | Maximises (§9) |
| `restore` | `Super+↓` | Restores; on a floating window, minimises (§9) |
| `quarter-up`, `quarter-down`, `quarter-left`, `quarter-right` | `Super+Shift+arrows` | The quarter on that side, keeping the window's other side (§9) |
| `focus-left`, `focus-right`, `focus-up`, `focus-down` | none | Focuses the window on that side (§4) |
| `switch-windows`, `switch-windows-backward` | `Alt+Tab`, `Alt+Shift+Tab` | The windows, most recently used first, while `Alt` is held (§11); nested, with `Super` too |
| `desktop-left`, `desktop-right` | `Super+Ctrl+←`, `Super+Ctrl+→` | The desktop on that side (§12) |
| `desktop-1` … `desktop-9` | `Super+1` … `Super+9` | That desktop (§12); the top row's keys, whatever the layout |
| `send-to-desktop-1` … `send-to-desktop-9` | `Super+Shift+1` … | Sends the focused window there, staying here (§12) |
| `take-to-desktop-1` … `take-to-desktop-9` | `Super+Ctrl+Shift+1` … | Takes the focused window there, going with it (§12) |

Other settings of this slice in the same file: `[keyboard]` (layouts;
without them, the system's `/etc/default/keyboard`, then XKB's `us`) and
`[outputs.NAME] scale` per screen by connector name (`eDP-1`; the
journal names each screen as it lights), the density's scale otherwise.

`[appearance]` holds `reduced-motion` (windows fade instead of moving,
and do not wobble; desktops cross-fade) and `high-contrast` (the focused
rim 2 px wide), both off by default. `[desktops] count` is the number of
desktops on each screen (4).

Not configurable yet: `Super`+drag moves a window, `Super`+right-drag
resizes it; `Ctrl+Alt+F1…F12` switch virtual terminals. The other sections' shortcuts join the table as
they are built.

## 14. Session

- A "Hocuspanda" session in the display manager (GDM, SDDM, greetd), from
  the `.deb`.
- It starts the compositor, then the shell client (slice 2 onwards). If
  the compositor fails to start, the display manager comes back with an
  error in the journal; it never leaves a black screen.
- How: the session entry runs `hocuspanda-session`, which goes once
  through the user's login shell (so that their profile's `PATH` is the
  session's), then runs `hocuspanda --session` and ends when it does,
  however it ends. What the compositor says, and the status it ended
  with, are in the journal: `journalctl --user -t hocuspanda -b`.
- Once ready, the compositor gives `WAYLAND_DISPLAY`, `DISPLAY`,
  `XDG_CURRENT_DESKTOP=Hocuspanda` and `XDG_SESSION_TYPE=wayland` to the
  session bus's activation environment and to systemd's user manager, so
  that portals and programs started by D-Bus show in the session; the
  launcher takes them back from systemd when the session ends.

## 15. Performance

- At rest: no frame drawn on an output where nothing changes.
- Every animation in this slice holds the output's refresh rate (60 to
  165 Hz) on the user's laptop, at scale 1 and 2: no frame over budget
  while moving, wobbling, snapping or switching desktops with five windows
  open.
- Input to screen: a window follows the pointer with no more than one
  frame of lag.

## 16. Accessibility

- Everything here works from the keyboard.
- Reduced motion: as stated per section.
- The focus rim never relies on colour alone: it is also thicker
  (2 px focused, 1 px otherwise) when "high contrast" is set.

## 17. Acceptance on the user's machine

Done when the maintainer, in a Hocuspanda session from GDM on the laptop:

1. opens Firefox, kitty, Slack and foot: each opens in place, animated,
   focused;
2. moves them with the header, `Super`+drag and the grip: they wobble, stay
   opaque, settle;
3. snaps halves, quarters and maximise by dragging and with the keys, and
   resizes one window of a snapped pair by its shared edge, and both with
   the handle on their seam;
4. resizes from every edge and corner, with the right cursors;
5. switches desktops with keys and the touchpad, sends windows to other
   desktops;
6. uses Alt+Tab quickly and slowly, across desktops, with minimised
   windows;
7. goes fullscreen in Firefox (F11) and back;
8. and finds nothing stutters, freezes or lags.

## 18. Decisions taken with the maintainer

1. Windows without a header get the grip, not a title bar.
2. Snapping is by halves and quarters only, ultrawide screens included.
3. A new window that did not take the focus pulses its rim once.
4. Four desktops on every screen by default.
5. A snapped pair's shared edge resizes the window grabbed alone; a handle
   on their seam resizes both (#335).
6. Minimised windows come back through Alt+Tab only, with no shortcut of
   their own; Alt+Tab shows no application icon (#338).
7. Magnetic edges, 12 px, at the gap: a moved window's edges, and a
   resized window's dragged edges, stick to other windows' at the 8 px
   gap or in line with them, and flush to the work area's; dragging on
   frees them (#343).
