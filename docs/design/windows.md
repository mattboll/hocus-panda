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
  in the accent so it is noticed, as a window gone to from an agent's row
  does (bar.md §8; issue #495): in 150 ms a 2 px rim at 95 % and a 4 px
  ring at 55 %, then the ring grows to 14 px as both fade, by 600 ms.
  With reduced motion, or a compositor on the CPU, the 2 px rim lights
  and fades without its ring. A window asking for the focus without a
  token pulses the same way. Its pulse ends when it gets the focus.
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
- Cursors come from the system's cursor theme, as applications' do
  (`[appearance] cursor-theme` and `cursor-size`, else `XCURSOR_THEME`
  and `XCURSOR_SIZE`, else the `default` theme and what it inherits),
  each screen showing the image nearest its scale, pixel for pixel; the
  programs the desktop starts are given the same theme and size. Without
  a theme, or for a shape the theme lacks, the compositor draws its own
  plain ones.
- `Super`+right-drag resizes from the nearest corner.
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
- Nested, `Alt+Tab` is `Alt+Super+Tab`, so that the host keeps its own.
- Reduced motion: the panel fades without scaling, the ring jumps.

## 12. Virtual desktops

- **Per screen**: each output has its own desktops and switches alone
  (first build's decision, kept). 4 desktops by default (1 to 9).
- Switching: `Super+1…4`, `Super+Ctrl+←/→`, a three-finger horizontal
  swipe on a touchpad (following the fingers), and the bar's tiles (a
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
  desktop the strip barely follows. Three fingers going down, and four,
  are left to applications.
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
  sets how many, from 1 to 9, on every screen.

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
- When a screen of the same name comes back (the same connector), the
  windows that came from it go back to it: on their desktop, at their
  place (the same one on a screen of the same size, the same share of it
  otherwise), in their state; the screen shows the desktop it showed.
  A window no longer where it was put (the user moved or resized it,
  changed it between floating, snapped and fullscreen, or sent it to
  another desktop) stays where it is; minimising it or bringing it back
  does not count. Each window remembers only the screen it left, its
  desktop and its place there.
- With every screen gone, the windows wait where they are; a screen
  that comes then shows those on it where they were, and takes those on
  none in the middle of its work area.
- The real screens (tty) and the headless backend's outputs (in tests)
  come and go; the nested window is one output that never goes.

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

| Action (`[shortcuts]`) | Default | What it does |
|---|---|---|
| `terminal` | `Super+Return` | Opens a terminal: `[programs] terminal`, or the first of kitty, foot, alacritty, xterm found in `PATH` |
| `log-out` | `Super+Shift+E` | Ends the session at once; the display manager comes back |
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

Other settings of this slice in the same file: `[keyboard]` (layouts;
without them, the system's `/etc/default/keyboard`, then XKB's `us`) and
`[outputs.NAME] scale` per screen by connector name (`eDP-1`; the
journal names each screen as it lights, with its density), from 0.5 to
4. Otherwise a screen takes 1, or 2 when it is denser than 192 dpi and
stays at least 1280×800 logical at 2; never a fraction by itself, which
would blur the applications that cannot draw at one (X11 ones). A screen
without a believable size (none, a projector's, an impossible density)
takes 1.

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
