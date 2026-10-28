# Windows and desktops — specification (slice 1, draft for review)

Status: draft, 2026-10-28, to be approved before any code. Slice 1 of the
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
  the focus: it opens behind the focused window.
- Focus moves with Alt+Tab (section 11) and `Super+arrows` with `Ctrl`
  (to the window on that side).
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
  Stiffness and damping are tuned once on the user's machine and fixed.
- The shadow deepens while moving (blur 48 px, 50 %). No transparency.
- With reduced motion: no wobble; the window moves rigidly.

## 7. Server-side decorated windows: the grip

A window with no header of its own still needs a mouse handle. Proposal:
a **grip**, a pill 44×4 px centred 4 px inside the top edge, that appears
in 120 ms when the pointer is in the window's top 12 px, and disappears
300 ms after it leaves. Dragging it moves the window; double-clicking it
maximises or restores. Open question 1 offers a thin title bar instead.

## 8. Resizing

- Resize zones: an 8 px band outside each edge, 16×16 px squares at the
  corners, with the matching resize cursors (`ew-resize`, `ns-resize`,
  `nwse-resize`, `nesw-resize`).
- `Super`+right-drag resizes from the nearest corner.
- Live resize, respecting the application's minimum and maximum sizes.
- Snapped and maximised windows resize too; a snapped pair resizes
  together along their shared edge.

## 9. Snapping

While dragging a window:

| Pointer at | Preview | Result |
|---|---|---|
| Left or right edge of the work area | that half | half width, full height |
| Top edge | the whole work area | maximised |
| A corner (within 48 px of it) | that quarter | quarter |
| On a screen wider than 21:9 (3440×1440), left or right third of the top edge | that third | a third |

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
  aspect kept), the application's icon and title below. A quick
  `Alt+Tab` (released before 100 ms) switches to the previous window
  without showing anything.
- Order: most recently used first, on the current desktop; then a
  separator and the other desktops' windows (dimmed, with the desktop's
  number). Minimised windows are in the list.
- `Tab` / `Shift+Tab` move the selection; releasing `Alt` switches to it
  (and to its desktop); `Esc` cancels; a click on a thumbnail switches.
- The panel appears in 150 ms (fade and 96 % → 100 % scale) and vanishes
  in 100 ms. The selection's ring moves in 120 ms.

## 12. Virtual desktops

- **Per screen**: each output has its own desktops and switches alone
  (first build's decision, kept). 4 desktops by default (1 to 9).
- Switching: `Super+1…4`, `Super+Ctrl+←/→`, a three-finger horizontal
  swipe on a touchpad (following the fingers), and later the bar's tiles.
- **Animation**: the outgoing desktop recedes (scale 96 %, dims 25 %)
  while the incoming one slides in from the side, in 280 ms (spring). A
  swipe follows the fingers, then completes or springs back on release.
- `Super+Shift+1…4` sends the focused window to that desktop, without
  switching; with `Ctrl` too, it follows it.
- Empty desktops stay; their order is fixed (1 to 4).

## 13. Shortcuts

All use `Super`, as in the user's current desktop. They are listed in one
place in the configuration and can be changed. In nested mode (the
desktop running in a window of another session) the modifier is
`Alt+Super`, so the host's shortcuts don't take them.

## 14. Session

- A "Hocuspanda" session in the display manager (GDM, SDDM, greetd), from
  the `.deb`.
- It starts the compositor, then the shell client (slice 2 onwards). If
  the compositor fails to start, the display manager comes back with an
  error in the journal; it never leaves a black screen.

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
   resizes a snapped pair together;
4. resizes from every edge and corner, with the right cursors;
5. switches desktops with keys and the touchpad, sends windows to other
   desktops;
6. uses Alt+Tab quickly and slowly, across desktops, with minimised
   windows;
7. goes fullscreen in Firefox (F11) and back;
8. and finds nothing stutters, freezes or lags.

## 18. Open questions

1. **Server-side decorated windows**: the grip (section 7), or a thin title
   bar (24 px: title, and close/maximise/minimise on hover)?
2. Thirds on ultrawide screens (section 9): wanted, or halves only?
3. Should a new window that did not take the focus get a short pulse of its
   rim, so it is noticed?
4. The number of desktops: 4 everywhere by default, or 4 on the laptop and
   1 on a secondary screen that stays on a chat app?
