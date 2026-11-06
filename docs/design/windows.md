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
- Windows go **over the bar**: the bar sits under normal windows, and a
  window the user moves or resizes onto it covers it and takes the
  pointer there (bar.md §2, #519). Its popovers, its halo, the bubbles
  and the launcher stay above the windows; the pointer held 300 ms
  against the screen's right edge, over the bar, brings the bar in front
  until it leaves the bar.

## 3. A window's look

- Corner radius 8 px. Client-side decorated windows (GTK, libadwaita,
  Firefox, Slack) keep their own header; server-side decorated ones (foot,
  kitty when it asks, Qt applications) get no title bar (section 7 for
  moving them).
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
  in the accent so it is noticed, as a window gone to from an agent's row
  does (bar.md §8; issue #495): in 150 ms a 2 px rim at 95 % and a 4 px
  ring at 55 %, then the ring grows to 14 px as both fade, by 600 ms.
  With reduced motion, or a compositor on the CPU, the 2 px rim lights
  and fades without its ring. A window asking for the focus without a
  token pulses the same way. Its pulse ends when it gets the focus. A
  click on an application's tray icon is the user's too: its application
  may focus one window within 2 s, asking or opening it (bar.md, as
  built, 5.14b).
- **Dialogs** (#595): a window that is another's dialog opens just
  above its parent and stays above it when the parent is raised. Its
  parent is its xdg parent (`xdg_toplevel.set_parent`, from its own
  client) or one set through xdg-foreign (`zxdg_importer_v2`), as the
  desktop portal's backend makes its file chooser the child of the
  window that asked for it (`parent_window`, `wayland:HANDLE`). It takes
  the focus when its parent has it, and asking for the focus then
  (xdg-activation, with a token of its own made from no input, as the
  portal's backend does) brings it forward; a dialog of a window
  without the focus opens above its parent, under the focused window,
  and pulses. The shell's surfaces are layer surfaces, which the
  protocol cannot export: the shell asks over its link for a handle of
  one (`export ASK NAMESPACE`), and a dialog of that handle (the
  capture's editor's "Enregistrer sous…") comes in front with the
  keyboard, whatever window had the focus. A handle made for the shell
  lasts a minute; one that names nothing is said `destroyed` at once.
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
- **Rules per application** (`[[windows.rules]]`, 6.14; the Settings'
  "Bureaux et fenêtres"), in the file's order, the first of an
  application's applying (its `app-id`, whatever the case): a window of
  it opens on `desktop` (from 1; on the screen it opens on, without the
  focus when that desktop does not show, its window out of sight until
  it comes; none or one past the count: the desktop shown), at `size`
  (`"last"`, its last size, by default; `"maximized"`, `"left"`,
  `"right"`, `"fullscreen"`: given in its first configure, as a window
  asking to open maximised is, so that its first frame fits; a window
  asking for a size of its own keeps it), and with `above = true` is kept
  above the others, its dialogs with it, even without the focus; a
  fullscreen window in front covers it until another is raised. The rule
  applies to its first window and the next ones; a dialog follows its
  parent, not the rule. The desktop and the size apply as a window opens;
  `above` follows the file as it changes, for the windows open too.

## 6. Moving, and the wobbly windows

- Moving: drag a client-side header, or `Alt`+drag anywhere in the
  window, or the grip (section 7). `Alt`+drag took the place of
  `Super`+drag with issue #564, `Super`+drag drawing on the screen since
  ([ink.md](ink.md)); nested too, `Alt`+drag moves.
- While a window moves, it **wobbles**: a soft spring mesh (4×4 control
  points) where the grabbed point follows the pointer exactly and the far
  side lags, then settles in about 400 ms after the pointer stops.
  Stiffness and damping are tuned once on the user's machine and fixed,
  with the user's current desktop's wobble as the reference to match or
  beat (the prototype's was found a little less pleasant).
- The shadow deepens while moving (blur 48 px, 50 %). No transparency.
- With reduced motion, or `[windows] wobble = false` (6.14): no wobble;
  the window moves rigidly.
- The key held to move from anywhere is `[windows] move-modifier`
  (6.14): `"alt"` (the default), `"super"` (nested `Alt+Super`; the ink
  then no longer starts, ink.md) or `"none"` (title bars and the grip
  only). The key held to resize with the right button is
  `[windows] resize-modifier`: `"super"` (the default, nested
  `Alt+Super`), `"alt"` or `"none"` (the frame only). Both followed as the
  file changes.

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
past the 12 px frees it, with no modifier. `[windows] magnet = false`
(6.14) turns it off, for moves and resizes, followed as the file
changes. Where the pointer is in a snap
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

The pill stands out from what lies under it (#719): light (`#EEF1F6` at
75 %) over a dark header, dark (`#141926` at 75 %, the light theme's
text) over a light one, at least 3:1 against it either way. Which one is
decided from the window's own pixels around the pill (2 px around it):
their mean colour, and the tone whose pill has the higher contrast
against it; the other tone takes over only once it does 15 % better, so
a mid-grey header does not make it flicker. Before anything is read, and
over a mostly transparent spot, the pill is light. The pixels are read
only while the grip shows, once per commit of the window's surface: the
renderer copies those few pixels after a frame is drawn and they are read
at the next frame, so the loop never waits on the GPU
(`crates/hocus-compositor/src/grip_tone.rs`). The seam's handle (section
8) stays light: under it is the gap between two windows, not a header.

Which windows have no header of their own (#716):

- A Wayland window that leaves its decorations to the desktop, through
  xdg-decoration (foot, Qt applications) or KDE's server decoration
  protocol (GTK 3 windows without a header bar). Both default to the
  desktop's decorations; a client asking for its own keeps its header (GTK
  3 with a header bar asks so).
- Not a Wayland window that asks through neither protocol: GTK 4 and
  libadwaita applications ask through neither and draw their own header,
  as KDE's protocol says to assume of a window that does not ask.
- An X11 window, unless its Motif hints ask for no decorations (GTK 3's
  client-side decorated windows on X11 do).

## 8. Resizing

- Resize zones on all four edges and all four corners (the prototype's
  first version had only the bottom and right ones, and that was felt at
  once): an 8 px band outside each edge, 16×16 px squares at the corners,
  with the matching resize cursors (`ew-resize`, `ns-resize`,
  `nwse-resize`, `nesw-resize`).
- Cursors come from the system's cursor theme, as applications' do
  (`[appearance] cursor-theme` and `cursor-size`, else `XCURSOR_THEME`
  and `XCURSOR_SIZE`, else the `default` theme and what it inherits),
  each screen showing the image nearest its scale, pixel for pixel; the
  programs the desktop starts are given the same theme and size. Without
  a theme, or for a shape the theme lacks, the compositor draws its own
  plain ones.
- `Super`+right-drag resizes from the nearest corner: once the pointer
  has moved 6 px with the button down, from where it was pressed. A
  `Super`+right click that does not move opens the window's radial menu
  (§10b).
- Live resize, respecting the application's minimum and maximum sizes,
  and the size it takes: a floating terminal sized by whole cells may
  take a little less than asked, and keeps it once the resize is over,
  its far edges where they were.
- The edge or edges being dragged stick as a moving window's do (section
  6, magnetic edges): within 12 px of another window's edge, at the gap
  or in line, or of the work area's, flush. Dragging on frees them. The
  windows that follow a seam's handle do not pull.
- Snapped and maximised windows resize too, and resized alone they
  float: a maximised window, or a half or a quarter by any of its edges
  (an outer one, or one with no snapped window across it), leaves its
  zone once its size changes. Only a seam's handle (below) resizes
  snapped windows and keeps them snapped.
- A **pair**: two windows shown on the same screen and desktop, side by
  side or one above the other, whose facing edges are exactly the 8 px
  gap apart and overlap along the seam between them, whether snapped
  (halves, quarters) or floating (brought together by hand, held there
  by the magnet), or one of each. Maximised and fullscreen windows pair
  with none; windows 9 px apart do not pair.
  - Dragging their shared edge resizes **only the window grabbed**, like
    any edge: in the gap, the window whose edge is nearer. A snapped one
    leaves its zone and floats once its size changes; the other stays
    where it is.
  - A **handle** on the seam moves the boundary and **both windows
    follow**; snapped ones stay snapped, floating ones stay floating. The
    windows that follow keep the gap to the one dragged wherever its
    client's size puts its edge (a terminal's, by whole cells), so the two
    stay a pair; their far edges may move by what is left of a cell. It is
    the grip's pill (section 7) along the seam: 4×44 px upright between
    windows side by side, 44×4 px between windows one above the other,
    centred in the 8 px gap at the middle of the span the two windows
    share. It shows when the pointer is within 12 px of the seam's centre
    line (the gap and a little of each window) where the seam itself
    shows, and fades in and out as the grip does (120 ms in; it stays 300
    ms after the pointer leaves, then fades in 120 ms). A press within 4
    px around the pill takes it (a 12×52 px target), with `ew-resize` or
    `ns-resize`. It sits in the stacking order right above the front one
    of the two windows: a window in front of both that covers the seam
    covers the handle too, and takes the presses there; a window behind
    them, seen through the gap, does not. Where more than two windows line
    up along a seam (a half facing two quarters), each pair has its handle
    at the middle of what the two share, and it moves the whole edge of
    the window whose side spans the seam: every window facing that edge
    follows.
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
- `[windows] snap-by-drag = false` (6.14): no zone while dragging, and
  the magnet pulls at the edges too; the keys below still snap, and a
  snapped window dragged away still takes back its size.
- The zones, the magnet and tearing a window off a zone are the crate
  `hocus-geometry`'s, without smithay: the Settings' trial area
  ("Bureaux et fenêtres", 6.14) moves its drawn windows with the same
  code.

Keyboard:

| Keys | Action |
|---|---|
| `Super+←` / `Super+→` | left / right half; again: move to the other side |
| `Super+↑` | maximise |
| `Super+↓` | restore; again: minimise |
| `Super+Shift+←/→/↑/↓` | quarters, by the corner the arrows point to |

- A snap is of the screen it is made on: the one under the pointer for a
  drag (the preview's, even with most of the window still on another
  screen), the window's for the keys. The window keeps to that screen as
  its work area changes, through fullscreen and back, and as its pair is
  resized; if that screen goes, it takes the same zone on a screen still
  there (§12, "A screen unplugged").

## 10. Maximise, fullscreen, minimise, close

- **Maximise** fills the work area (8 px gaps, bar's space kept). Animated
  like a snap.
- **Fullscreen** (the application asks, or `F11` in it) covers the whole
  output, the bar's space included. When the bar has something to say
  (slice 2's halo), a 4 px line in the halo's colour shows on the
  output's right edge.
- A window that asks to be maximised or fullscreen before its first
  frame (`foot --fullscreen`, an application reopening as it was) opens
  so, its first frame already that size. Restored, it floats centred at
  its application's last size, or two thirds of the work area each way
  (#351).
- **Minimise** (`Super+H` or `Super+↓` twice): the window shrinks toward
  the bar's area in 220 ms and fades. It comes back with Alt+Tab or the
  desktop's list.
- **Close**: `Super+Q` or `Alt+F4`, or the application's own button.
  Closing never kills the application: it asks it to close.
- **Always on top** (a window's radial menu, the bar's menu of a window,
  or `[[windows.rules]] above`): the window stays above the others, its
  dialogs with it, until toggled; a fullscreen window brought in front
  covers it while it stays there.
- **On all desktops** (the same menus): the window shows on every
  desktop of its screen, staying where it is while the strip slides (it
  is on the desktop shown, whichever that is); the bar's tiles sketch it
  in each. Sent to a desktop (`Super+Shift+N`, a menu), it is on that
  one only again.

## 10b. A window's radial menu (#723)

Its actions around the pointer, each direction meaning something: up and
down as `Super+↑` and `Super+H`, left and right as the desktops' strip.

| Direction | Action | Same as |
|---|---|---|
| up | Maximise / Restore (the label follows the state) | `Super+↑` / `Super+↓` |
| up-right | Always on top (a switch) | `[[windows.rules]] above` |
| right | Next desktop (sends the window; the screen stays) | `Super+Shift+<N+1>` |
| down-right | Other screen (the next by position, onto the desktop it shows) | — |
| down | Minimise | `Super+H` |
| down-left | Close (never by a gesture) | `Super+Q` |
| left | Previous desktop (sends the window; the screen stays) | `Super+Shift+<N−1>` |
| up-left | On all desktops (a switch) | — |

- **Opening**: `Super` and a right click over a window, released within
  6 px of the press: the menu opens centred on the pointer and stays
  open. A right drag of 6 px or more resizes, as before (§8). The key is
  `[windows] resize-modifier`'s (`"none"`: the grip only). A right click
  on the grip (§7) opens it too, without a key. Over the bar or another
  surface of the shell's, nothing opens.
- **Its look** (the mockup's, real size): a hub of radius 44, the ring to
  116 with a 6 px rim (`rgba(14,17,28,0.92)`, a 1 px white rim at 10 %,
  a drop shadow), eight sectors 41.8° wide, their icons 22 px at mid-ring
  and their labels outside it (12.5 px Manrope 600, `text.dim`, a dark
  outline; two lines at most), a soft dark halo behind it all so that
  the labels read over any window. Near a screen's edge it is pushed in
  to stay whole, labels included (196 px from its centre each way): its
  centre then is not the pointer.
- **Picking**: moving into a sector lights it (the accent at 24 %, a
  1.4 px accent rim, its label bold), and the hub says its shortcut, the
  desktop it sends to, or the other screen's name. **Crossing a sector's
  outer rim on the way out picks it**, without a click (a quick flick
  picks the sector it crossed); a click in a sector picks it too. The
  picked sector flashes 80 ms and the menu fades in 100 ms; its action
  runs at once.
- **Close** lights in orange (`rgba(242,148,91,0.22)`); crossing its rim
  does nothing. Resting on it 300 ms fills an arc along its rim, the hub
  then says "clic"; a click then asks the window to close. A click
  before is ignored.
- Inert sectors (no desktop on that side, one screen only) at 25 %:
  they neither light nor pick. A switch that is on shows its icon in
  `#7EE0A0` with a 3 px dot.
- A window hosting an agent that waits: the hub shows its panda (64 px,
  bobbing); otherwise its application's letter tile.
- **Closing it**: `Esc`, a click in the hub or outside the menu, the
  window going, its screen going, the session locking.
- **Keyboard** while open: the arrows step the lit sector that way (from
  up, → lights up-right), `Tab` and `Shift+Tab` go round, `Enter` or
  `Space` picks the lit one, Close too, without the rest.
- While it is open no client sees the pointer or the keys.
- **Motion**: it opens in 140 ms (scale 0.85 → 1 from its centre, and a
  fade), closes in 100 ms; with reduced motion it only fades. Nothing
  draws at rest: only the arc filling and the panda bobbing draw at
  every frame, each on a small picture of its own.
- The same actions from the bar: a right click on a window's line
  (bar.md, "The windows in the bar").
- **As built** (`crates/hocus-compositor/src/radial.rs`, its drawing in
  `radial_draw.rs`): the compositor draws it on the CPU with tiny-skia,
  its words in the shell's own fonts, once for each look it takes.
  Measured headless (`crates/hocus-compositor/tests/radial.rs`): it opens
  on the pointer and a drag still resizes; up past the rim maximises,
  right past it sends to desktop 2 (the screen staying); Close's rim does
  nothing, a click after the rest asks the window; on desktop 1 previous
  is inert, with one screen other screen too, with two it sends the
  window to the second; opened 20 px from the corner it stays whole; the
  keys; the grip's right click; nothing over a layer surface; always on
  top stays above a window focused after; on all desktops shows on
  desktop 2 and, again, only there; `Super+Shift+2` takes it off all
  desktops. With the real shell (`tests/bar_windows.rs`): nothing over
  the bar, the ring over a window (`HOCUS_SHOTS`: its screenshots).

## 11. Alt+Tab

- `Alt+Tab` held: after 100 ms, a panel appears centred on the output
  under the pointer, with a live thumbnail per window (150 px high,
  aspect kept), the window's title below; no application icon, the
  thumbnail already shows the window. A quick
  `Alt+Tab` (released before 100 ms) switches to the previous window
  without showing anything.
- Order: most recently used first, on the current desktop; then a
  separator and the other desktops' windows (dimmed, with the desktop's
  number). Minimised windows are in the list.
- `Tab` / `Shift+Tab` move the selection; releasing `Alt` switches to it
  (and to its desktop); `Esc` cancels; a click on a thumbnail switches.
  Held, `Tab` (or `Shift+Tab`) steps on after the keyboard's repeat delay,
  then at its rate (`[keyboard] repeat_delay`, `repeat_rate`), as a held
  key repeats in an application; the desktop times it, since keyboards
  send no repeats of their own. No other shortcut repeats when held.
- The panel appears in 150 ms (fade and 96 % → 100 % scale) and vanishes
  in 100 ms. The selection's ring moves in 120 ms.
- As built: every thumbnail is 150 px high, its width following the
  window's aspect, so that no window looks smaller than the others (a
  wide one gets a wider thumbnail, not a shorter one; #341); a window
  wider than 2:1 gets one 300 px wide and a little shorter. It is
  centred in its item, which is at least 160 px wide inside its 8 px
  padding, for the title; it shows the window as
  its application sets it (its xdg geometry), without the shadow a
  client-side decorated application draws around it. The items are 14 px apart on
  one line, their titles lined up; the panel has 16 px of padding, an
  18 px radius, a dark fill at 86 % with a 1 px white rim at 12 % and a
  deep shadow, and shrinks as a whole when the windows do not fit the
  screen. The selection is the accent at 18 % with a 2 px accent ring.
  Titles are the application's, ellipsed; another desktop's number
  follows in grey (`Firefox · 2`). Other desktops' and minimised windows
  show at 60 %. `←`/`→` move the selection too, `Return` switches.
- `[windows] alt-tab = "desktop"` (6.14) lists this desktop's windows
  only (minimised ones included); `"all"`, the default, as above.
- Nested, `Alt+Tab` is `Alt+Super+Tab`, so that the host keeps its own.
- Reduced motion: the panel fades without scaling, the ring jumps.

## 12. Virtual desktops

- **Per screen**: each output has its own desktops and switches alone
  (first build's decision, kept). 4 desktops by default (1 to 9).
- Switching: `Super+1…4`, `Super+Ctrl+←/→`, a three-finger horizontal
  swipe on a touchpad (following the fingers; `[touchpad] swipe-desktops
  = false`, the Settings' "Trois doigts de côté", leaves it to
  applications, settings.md), and the bar's tiles (a
  click, the wheel; bar.md §4.4). Keys and swipes act on the screen under
  the pointer, the tiles on their bar's screen.
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
  desktop the strip barely follows. A three-finger swipe that starts
  vertical is left to applications for its whole length (6.13: the
  first 12 units of travel decide, the compositor keeping the events
  until then); four fingers sideways too, while four fingers up and down
  open and close the launcher (launcher.md, 6.13). The direction is the
  fingers', whatever natural scrolling says.
- **Which desktop shows**: while the strip moves (keys, swipe, a window
  sent), dots at the top centre of the screen show the desktops: 8 px
  high, 6 px apart, 18 px from the top, the current one 22 px wide in the
  accent, the others 8 px in white at 35 %; the wide dot follows the
  strip as it slides. They fade in and out in 300 ms around the motion
  (the prototype's). The bar's tiles show it at all times (bar.md §4.4).
- Reduced motion: the desktops cross-fade in 120 ms instead of sliding; a
  swipe does not follow the fingers, the release decides.
- `Super+Shift+1…4` sends the focused window to that desktop, without
  switching: it leaves the screen toward its desktop's side, a screen's
  width in 320 ms easing in, as leaving windows do (it fades with reduced
  motion), and the dots show. With `Ctrl`
  too, it follows it: the window stays in place as the strip slides
  under it. `Super+Ctrl+Shift+←/→` takes it the same way to the desktop
  on that side of its screen; past the first or the last, it stays (no
  wrap), and the dots show, as with `Super+Ctrl+←/→`. A window being
  dragged while desktops switch goes along.
- Empty desktops stay; their order is fixed (1 to 4). `[desktops] count`
  sets how many, from 1 to 9, on every screen, followed as the file
  changes (6.14): more are added after the last, empty; fewer take out the
  last ones, their windows going to the last that stays. Nothing slides:
  a screen showing a desktop taken out shows the one its windows went to.
- **Names** (`[desktops] names`, by position, the same on every screen;
  6.14): the bar's tiles say them (a tile's name for screen readers, its
  list's title) and Alt+Tab's other desktops' suffix (`Firefox ·
  Courrier`); one missing or empty is "Bureau N" / "Desktop N".
- **A desktop taken out by the Settings** (settings.md, "Bureaux et
  fenêtres"), wherever it is: on the Settings' line (`msg screens`),
  `desktop-remove N` (from 1) gives its windows to the desktop before it
  (the first's to the next one) on every screen, the desktops after it
  moving down one with their names; the answer
  (`{"desktop-removed": {"desktop": N, "to": M, "windows": [ids]}}`)
  names the windows that moved, and `desktop-restore N ID…` (the toast's
  "Annuler") puts a desktop back at `N` with them. The Settings ask first
  and write the file once answered, so that the count the file then says
  is already the desktops', and nothing moves twice. The line also says
  how many windows each desktop has, every screen's together, as they
  change: `{"desktops": {"count": 4, "windows": [3, 2, 0, 0]}}`.

### A screen unplugged

- No window is ever lost. When a screen goes, every window on it, on any
  of its desktops, floating, snapped, fullscreen or minimised, moves to
  the screen still there nearest to it, onto the desktop of the same
  number (the last one if that screen has fewer), in the same state:
  - a floating window keeps its place across the screen: its centre at
    the same share of the work area's width and height, its size kept
    but no larger than the new work area, and wholly inside it; where it
    goes back when restored is carried the same way;
  - a snapped window takes the same zone, a fullscreen one covers the new
    screen, a minimised one stays minimised.
  The windows move at once, without motion: there is nothing to see them
  leave from. Those whose desktop is not the one the new screen shows go
  out of sight there, as on any other desktop.
- What was going on ends: a drag or a resize under way, wherever it is
  (without snapping where its preview was); Alt+Tab shown on that screen
  (cancelled), a swipe of its desktops; a desktop sliding there stops on
  the desktop it was going to. The pointer, if it was there,
  goes to the middle of the new screen; the focus stays on its window if
  that one still shows, otherwise it goes to the window on top that does.
- When a screen of the same identity comes back (below: its EDID's make,
  model and serial, on any connector; #417), the windows that came from
  it go back to it: on their desktop, at their place (the same one on a
  screen of the same size, the same share of it otherwise), in their
  state; the screen shows the desktop it showed. A window no longer where
  it was put (the user moved or resized it, changed it between floating,
  snapped and fullscreen, or sent it to another desktop) stays where it
  is; minimising it or bringing it back does not count. Each window
  remembers only the screen it left, its desktop and its place there,
  for the session's length (nothing is written about windows); closed
  meanwhile, it is forgotten.
- **They fly back** (#417): each window shown before and after flies
  from where it is to its place in **320 ms**, `cubic-bezier(0.22, 0.9,
  0.3, 1)` (the desktops' curve), with no wobble and no transparency;
  windows on desktops not shown are placed at once. Its place is its own
  from the start (the drawing only travels), so a click or a key on a
  flying window acts on it where it goes. Reduced motion: it fades in at
  its place in 120 ms.
- **The lid** closed with another screen plugged and on takes the
  laptop's screen away exactly as an unplug, whatever the lid then does
  (`[power] lid`, settings.md): its windows go to the other screen, the
  bar too, and it is out of the set of screens (below); the lid opened
  brings it back, its windows with it. With no other screen, the lid only
  turns it off (7.2).
- **Screens swapped while the machine slept** are seen at resume, in one
  turn (the real screens' scan): a screen whose EDID changed on its
  connector went, and another came; one gone and back with the same
  identity is no change at all.
- With every screen gone, the windows wait where they are; a screen
  that comes then shows those on it where they were, and takes those on
  none in the middle of its work area.
- The real screens (tty) and the headless backend's outputs (in tests)
  come and go, through the same code; the nested window is one output
  that never goes.

#### A screen's identity

As built (#417; the maintainer's question 5): a screen is its EDID's
make (the three letters), model (its name descriptor) and serial number
(its serial descriptor, else the number in its header): `DEL U3423WE
7XY1234`. A different monitor on the same cable is another screen; the
same one on another cable is the same. Without a serial, its connector
tells it apart, `DEL U2723QE (DP-2)`: two of one model without serials
swap when their cables do. Without an EDID (a virtual machine's screens,
the headless outputs) it is its connector, `HEADLESS-2`. Two screens of
the same identity plugged at once: the second's connector is added. The
journal says each screen's identity as it lights ("a screen, known as");
the Settings get it on their line (`key`). The headless harness plugs a
screen with an identity of its choice (`Headless::plug_screen`), on a
connector unplugged before or a new one.

#### Sets of screens

- **A set** is the identities of the screens plugged and on: a screen
  turned off by the file (`enabled = false`) or under the closed lid does
  not count. The laptop alone, the laptop and the office's screen, the
  laptop and the home's: three sets.
- **Each set keeps its own arrangement** (question 6), in the
  configuration file: `[[screen-sets]]`, its `screens`, its `primary`
  (an identity) and one table per screen, by identity, with the keys of
  `[outputs.NAME]` that arrange it (`position`, `mode`, `scale`, `vrr`).
  What a set's table says wins over `[outputs.NAME]` and `[panel] output`,
  which stay the defaults any set starts from: a set seen for the first
  time takes them (then left to right, tops aligned). Plugging a set back
  restores its arrangement in the same turn as its screens light (the
  real screens light in its mode and scale at once).

  ```toml
  [[screen-sets]]
  screens = ["BOE 0x0BCA (eDP-1)", "DEL U3423WE 7XY1234"]
  primary = "DEL U3423WE 7XY1234"
  [screen-sets.outputs."DEL U3423WE 7XY1234"]
  position = [0, 0]
  [screen-sets.outputs."BOE 0x0BCA (eDP-1)"]
  position = [440, 1440]
  ```
- **The Settings write it** (settings.md, Écrans): with two screens or
  more, what the user changes there (a place, a mode, a scale, VRR, the
  primary screen) goes into the set in use's table, made when needed and
  taken out when nothing but its screens is left; with one screen, into
  `[outputs.NAME]`. Turning a screen off and its wallpaper stay
  `[outputs.NAME]`'s. The compositor only reads the file.
- **The sets seen** are kept in `$XDG_STATE_HOME/hocuspanda/screen-sets`
  (each set's screens, their sizes, when it was last in use), written on a
  thread, so that the Settings list them with the file's; "Oublier cet
  ensemble" takes a set's table out of the file and its line out of that
  list (`forget-set` on the Settings' line, the set in use never).
- The bar stays on the primary screen (8.2, several bars, comes later).

#### Saying it

As built (#417; question 2, option C): when at least one window moved
with a screen gone or back, the **winking panda** (its hat raised, never
an agent state's pose) comes out of the bar's edge as an agent's panda
does (bar.md §7 C: 170 px, its motion, 18 px from the bar, at the
secretary's height), with a **blue bubble** (`rgba(16,24,44,0.95)`, a
1 px border `#8FB3FF` at 45 %; under the pointer
`rgba(22,32,58,0.97)`), two lines of 13 px, the first in bold
`#B9CEFF`:

| | Français | English |
|---|---|---|
| Gone | **L'écran externe 34 pouces est parti.** Firefox et foot sont ici, sur le bureau 1. | **The 34-inch screen left.** Firefox and foot are here, on desktop 1. |
| Back | **Re-bonjour, écran externe 34 pouces !** Firefox et foot y retournent. | **Welcome back, 34-inch screen!** Firefox and foot go back to it. |

The screen is "écran du portable" / "laptop screen" for the built-in
one (the lid), "écran externe" / "external screen" when its size is not
known. The windows are their applications' names, the focused one
first: one ("Firefox est ici"), two ("Firefox et foot"), more ("Firefox,
foot et 3 autres"), several of one application ("2 fenêtres foot"); "sur
le bureau N" only when they are all on one desktop. A line longer than
340 px is cut with an ellipsis.

- It stays **4 s** (not the agents' 8 s), then goes back as a peek does.
  A click on it sends it back at once (`peek-close`).
- An agent's peek arriving meanwhile replaces it; it never replaces an
  agent's (that one goes on; nothing is said of the screens). The
  secretary's briefing open, nothing is said.
- **Not while the session is locked**: the windows move, nothing is
  said.
- The compositor tells the shell `screens-moved gone|back
  built-in|external INCHES DESKTOP`, then a tab and each window's
  application id; the shell names them from their desktop entries.
- Reduced motion, or a shell on the CPU: it fades in and out in place.

#### Measured headless

`tests/screens_return.rs`: two windows halved on a screen known as `DEL
U3423WE 7XY1234` go to the other screen's halves, desktop 1, the focus
kept; another screen on the same connector takes none of them and is a
set of its own, right of the first; their screen back on another
connector, they go back to it in their halves. Back, a window is drawn
where it was, between its two places at 160 ms (past half way), at its
place at 320 ms on the held clock; with reduced motion at its place at
once. A file arranging two sets: each takes its places, scale and
primary screen as its screens are plugged, a set not in the file left
to right. The Settings forget a set seen (the set in use refused), the
state file following. The lid closed with another screen on: the
laptop's window on the other screen, the set without the laptop; opened,
the window back at its place. With the real shell: no window, nothing
said; two windows: the panda out beside the bar on the screen still
there, saying "L’écran externe 27 pouces est parti. Foot et Firefox
sont ici, sur le bureau 1.", still out at 3.9 s, back from 4 s, its
surface gone; back, "Re-bonjour, écran externe 27 pouces ! Foot et
Firefox y retournent."; locked, nothing said.

#### What clients are told

As built (#536), in this order, in the same turn of the loop:

1. every surface on the screen, windows (mapped or not, with their
   subsurfaces and popups) and layer surfaces, gets `wl_surface.leave`
   for its `wl_output`;
2. its layer surfaces (bars, wallpapers, the shell's overlays) get
   `zwlr_layer_surface_v1.closed`; the shell is told its bar there went;
3. each window gets a configure for the screen it moves to, its bounds
   that screen's work area;
4. the screen's `wl_output` global is disabled: clients get
   `wl_registry.global_remove`. It is removed 5 s later, so that a client
   binding it meanwhile is not killed for it;
5. the windows then enter the screen they moved to, and are told its
   scale.

Nothing names the screen's `wl_output` after its `global_remove`: a
client that drops it then (kitty does) gets such an event's output as
null, and kitty crashed on it. A layer surface asked for on that output
meanwhile is closed at once. `tests/unplug_clients.rs` checks the order
with a client bound to every output, and runs kitty and foot when they
are installed.

A screen plugged back gets its bar on it, wherever the pointer is
(#540). The shell's toolkit finds a screen by its xdg-output name, which
reaches it a few exchanges after the screen's global: asked for before,
a surface would be on no screen, and the compositor would put it under
the pointer. So the shell keeps a bar it is told to open until it knows
that name itself, on a queue of its own on the same connection that
takes the toolkit's steps (bind the screen, ask for its xdg-output, read
its name), each request made after the toolkit's for the same step: the
name reaches the shell after it reached the toolkit, with no round trip
counted (#552). The screen's global is the one step the shell does not
ask for: the compositor announces it to the shell's registry first, and
the toolkit's can come in a later write. So the shell binds the screen
only once a `wl_display.sync` it asked for on seeing the global is
answered, which comes after the toolkit's announcement (#559). It never
asks for a bar on no screen. The compositor defends too: one of the
bar's own surfaces (the bar, its clock, levels, connections, desktops,
agents and footer) asked for on no screen goes to a screen given a bar
that lacks that surface, and is closed if none does, rather than making
a second bar under the pointer. `tests/unplug_clients.rs` plugs a screen
back with the pointer on the other one and checks that the bar is on the
screen that came back, the shell having asked for none of its surfaces
on no screen; and checks the compositor's choice with a client that asks
for them so.

### Screens of different scales

A screen at 1 beside one at 2 (or 1.25 … 1.75), side by side (#419; the
s8a mockup, questions 12 to 15):

- **The same size**: a window keeps its logical size on every screen;
  each screen draws it at its own scale. A 1000×640 window is 1000×640
  pixels on a screen at 1 and 2000×1280 on one at 2.
- **Sharp** (question 12, A): the scale a window is asked to draw at,
  `wp_fractional_scale_v1.preferred_scale`, and `wl_surface`'s
  `preferred_buffer_scale` (rounded up) for clients without it, is the
  **largest** scale of the screens its geometry overlaps (its own
  shadow does not count); once wholly on one screen, that screen's. It
  changes when the screens it overlaps change, during a drag too, and is
  sent at most once a frame, only when it changes. The screen at the
  smaller scale draws the larger buffer reduced (linear filtering). The
  compositor offers `wl_compositor` version 6 for that. A window on no
  screen keeps the scale of the screen it last showed most on.
- **No gap, no jump**: until the client commits a buffer at the new
  scale, the screens show the last one at the same logical size; nothing
  goes blank or changes size.
- **Too big for its screen** (question 13, C): a floating window dragged
  onto another screen (its centre there when dropped) whose work area is
  smaller than it shrinks to fit that work area, wholly inside it; it
  remembers its size from before and takes it back (kept inside the work
  area) when dragged onto a screen where it fits, unless the user resized
  it meanwhile. It is never made smaller than its client's least size.
  Dragged within one screen, nothing changes; a window dropped in a snap
  zone takes the zone. A screen unplugged carries its windows as above
  (no larger than the new work area).
- **The pointer** (question 14, B): moving past an edge of its screen
  where no screen touches, toward a screen that lies beyond that edge's
  line (screens of different heights side by side), it goes to the
  nearest point of that screen instead of stopping. Elsewhere the
  layout's outer edges still stop it. This is a mouse's or a touchpad's
  motion; an absolute one (a tablet, a virtual machine's pointer) goes
  where it points.
- **X11 applications** (question 15, A): XWayland's windows draw at 1;
  on a screen at 2 they are shown at their logical size, enlarged, a
  little soft. Their geometry is the same logical size on every screen.
- **Cursors** keep their logical size; a client's own cursor surface is
  told the scale of the screen under the pointer.
- **Layer surfaces** (the bar, wallpapers, the shell's overlays) are each
  on one screen, and told its scale.
- **The bar** stays on the primary screen (8.2, several bars, comes
  later).
- **Moving the focused window to another screen with the keyboard** is
  question 16, its own lot.

Measured headless (`tests/scales.rs`): a screen at 2 (2560×1600, 1280×800
logical) left of one at 1 (1920×1080); a test client drawing at the
buffer scale it is told, with a band of one-pixel stripes. A 1000×640
window on the screen at 1 is told 1 (120ths: 120); held across the seam,
2 (240) at once, its size unchanged; on the screen at 2 its stripes are
one pixel each, on the one at 1 two buffer pixels make one grey pixel;
wholly on the screen at 2: 2; back wholly on the one at 1: 1, each change
said once. A 1500×1000 window dropped on the screen at 2 takes its work
area's size, inside it; dragged back, 1500×1000; shrunk again, resized by
its right edge, dragged back: the size it was given. The pointer at
(1290, 1000) moved 30 px left lands at (1260, 799); at (1290, 400), at
(1260, 400); the outer edges stop it. An X11 window of 400×300 opened on
the screen at 2 is 400×300 logical, its blue over 800×600 pixels. Its
screenshots (by hand, `HOCUS_SHOTS=DIR`, the ignored tests
`straddling_shots` with foot and `x11_shots` with xterm): each screen, the
two as arranged, and the seam's two sides at the same size.

## 13. Shortcuts

Most use `Super`, as in the user's current desktop; `Alt+Tab` and
`Alt+F4` are there for the habit. They are listed in one place in the
configuration and can be changed. In nested mode (the desktop running in
a window of another session) the modifier is `Alt+Super`, so the host's
shortcuts don't take them; `Alt+Tab` takes `Super` too. Other shortcuts
without `Super`, as `Alt+F4`, stay as they are: the host sees them first
and keeps its own (`Alt+F4` closes the desktop's window there), and only
those it lets through reach the desktop.

The configuration's `[shortcuts]` section holds them, by action: one
shortcut, or a list of them (`close = ["Super+Q", "Alt+F4"]`); an empty
string or list turns an action's off. The package ships every setting with its default
(`/usr/share/doc/hocuspanda/config.toml`, from `packaging/config.toml`).
The compositor reads the section again as the file changes, without a
restart (6.8). The Settings' "Clavier et raccourcis › Raccourcis" lists
every action below and records new combinations (settings.md); while it
records one, it holds the keyboard-shortcuts-inhibit protocol, and the
desktop runs none of these (media keys included) for the window that
holds the keyboard; the virtual terminals' switch stays.

| Action (`[shortcuts]`) | Default | What it does |
|---|---|---|
| `terminal` | `Super+Return` | Opens a terminal: `[programs] terminal`, or the first of kitty, foot, alacritty, xterm found in `PATH` |
| `log-out` | `Super+Shift+E` | The session menu's log-out confirmation (bar.md, 7.4): 10 s, cancellable, then the session ends and the display manager comes back; without a shell, at once |
| `lock` | `Super+L` | Locks the session (lock.md) |
| `close` | `Super+Q`, `Alt+F4` | Asks the focused window to close (§10) |
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
| `take-to-desktop-left`, `take-to-desktop-right` | `Super+Ctrl+Shift+←`, `Super+Ctrl+Shift+→` | Takes the focused window to the desktop on that side of its screen, going with it; it stays past the first or the last (§12) |
| `toggle-bar` | `Super+B` | The bar's other form, on the screen under the pointer ([bar.md](bar.md)) |
| `launcher` | `Super+Space` | Opens the launcher on the screen under the pointer; again, closes it ([launcher.md](launcher.md)) |
| `clipboard` | `Super+V` | The clipboard's history; again, closes it ([bar.md](bar.md), 5.6) |
| `emoji` | `Super+period` | The emoji picker, under the focused application's caret ([emoji.md](emoji.md)) |
| `join` | `Super+J` | Joins the meeting whose reminder shows, else the one starting next within 10 min; nothing without one ([bar.md](bar.md), Day.6) |
| `capture`, `capture-screen`, `capture-window` | `Print`, `Shift+Print`, `Alt+Print` | The screen capture; the screen under the pointer or the focused window at once ([screenshots.md](screenshots.md)) |
| `record` | `Super+Shift+R` | Records the screen with the last settings; while recording, stops ([screenshots.md](screenshots.md), 5.10) |
| `volume-up`, `volume-down`, `mute` | the keyboard's sound keys | 5 % a press, stepping on while held; to the shell's gauges |
| `brightness-up`, `brightness-down` | the keyboard's brightness keys | The same for the screen's brightness, down to 5 % |

Other settings of this slice in the same file: `[keyboard]` (layouts;
without them, the system's `/etc/default/keyboard`, then XKB's `us`;
options, the keys' repeat and `numlock`, Num Lock on as the session
opens; followed live as the file changes, the keymap compiled off the
loop) and
`[outputs.NAME] scale` per screen by connector name (`eDP-1`; the
journal names each screen as it lights, with its density), from 0.5 to
4. Otherwise a screen takes 1, or 2 when it is denser than 192 dpi and
stays at least 1280×800 logical at 2; never a fraction by itself, which
would blur the applications that cannot draw at one (X11 ones). A screen
without a believable size (none, a projector's, an impossible density)
takes 1.

The same section takes, since 6.9 (the Settings' Écrans page,
settings.md): `mode = "2560x1600@60"` (width, height and refresh rate,
whole or not; without one, its preferred size at its highest rate; one it
does not offer is the same), `position = [x, y]` (its top-left corner,
logical; used when every screen on has one and they neither overlap nor
leave one apart, else they go left to right in the connectors' order,
tops aligned), `enabled = false` (off, plugged: its windows go to another
as for a screen unplugged; the last screen on stays on) and `vrr = true`.
The compositor follows the file as it changes (a thread waits in inotify
on its folder, then the loop applies `[outputs]` and `[panel] output`):
windows go along with their screen, at the same share of its work area
(a window the desktop moved so is still the one a screen turned on again,
or plugged back, takes back).
A mode can also be tried by the Settings without the file: the
compositor takes it back by itself after 15 s, or when the Settings go,
unless it is kept.

`[appearance]` holds `reduced-motion` (windows fade instead of moving,
and do not wobble; desktops cross-fade) and `high-contrast` (the focused
rim 2 px wide), both off by default. `[desktops] count` is the number of
desktops on each screen (4), `names` their names (§12); both followed as
the file changes.

`[windows]` (6.14) holds `move-modifier` and `resize-modifier` (§6),
`magnet` (§6), `snap-by-drag` (§9), `wobble` (§6), `alt-tab` (§11) and
the rules per application, `[[windows.rules]]` (§5);
`[ink] enabled` turns drawing on the screen off; all followed as the
file changes.

Not configurable yet: `Super`+drag draws on the screen,
`Super`+`Shift`+drag draws an arrow and `Super`+`Alt` held shows the
laser (`Alt+Super` and `Alt+Super+Shift` drag nested; [ink.md](ink.md));
`Ctrl+Alt+F1…F12` switch virtual terminals. The other sections' shortcuts join the table as
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
  launcher takes them back from systemd when the session ends. A desktop
  portal a previous session left running is restarted then (see
  [sharing](sharing.md#511b-a-portal-left-by-a-previous-session-2026-11-03)).

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
2. moves them with the header, `Alt`+drag and the grip: they wobble, stay
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
8. A screen unplugged moves its windows to the nearest screen left, on
   the same-numbered desktop, floating ones at the same place across it;
   they go back when that screen returns unless the user placed them
   meanwhile (#347).
9. A half or a quarter resized from any edge leaves its zone and floats,
   as a maximised window does; only a seam's handle resizes snapped
   windows and keeps them snapped (#352).
10. Any two windows at the gap form a pair with a handle on their seam,
    floating ones brought together by hand as snapped ones: the handle
    resizes both and leaves each floating or snapped as it was (#356).
11. The seam's handle is drawn right above the front one of its two
    windows, not above all windows: a window over the seam hides it and
    takes the presses there (#354).
12. A window placed over the bar goes in front of it; the desktop's
    placements still keep off the bar, and the pointer pushed against the
    screen's edge brings the bar back in front (#519).
13. A window's radial menu has eight actions, each direction meaning
    something; Close sits in the ring, down-left, never picked by a
    gesture; it opens with `Super` and a right click that does not move,
    and from the grip; the bar's window lines have the same actions on a
    right click; "always on top" and "on all desktops" are new window
    states, marked after the title in the bar (#723).
14. Screens plugged and unplugged (#417, the s8a mockup, questions 1 to
    7): windows go as in 8; the winking panda says it with a blue bubble
    for 4 s; back, windows fly to their places in 320 ms; a window placed
    meanwhile stays; a screen is known by its EDID's make, model and
    serial, its connector only as a fallback; each set of screens keeps
    its arrangement in `[[screen-sets]]`, `[outputs.NAME]` staying the
    defaults; Écrans lists the sets known and forgets one. The peek's
    panda is 170 px (was 96).
15. Screens of different scales (#419, the s8a mockup, questions 12 to
    15): a window straddling two scales is asked to draw at the larger,
    reduced on the other screen; a window too big for the screen it is
    dragged onto shrinks to fit and takes its size back where it fits;
    the pointer at a step between screens slides onto the screen beyond;
    X11 applications are shown at their size on a screen at 2, enlarged
    and a little soft.
