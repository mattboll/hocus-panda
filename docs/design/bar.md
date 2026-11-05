# The bar — specification (v2, draft for review)

Status: draft, 2026-10-27, to be approved before any code. It describes
what the prototype on the design canvas shows, in numbers. Where it and
the prototype disagree, this document wins once approved. Sections 2 and
3 (the empty column, its glass and its two forms) are built since
2026-10-28 (3.1, issue #358), the clock of section 4 group 1 and of
section 5 (3.2, issue #359), the levels of section 4 group 10 and of
section 5 (3.3, issue #360), the popovers of section 9 with the
sound's (3.4a, issue #361), the Wi-Fi's row and popover (section 4
group 9, section 5, section 9; 3.5, issue #362), Bluetooth's (3.6,
issue #363), the brightness's and the battery's popovers (3.7,
issue #364), the keyboard layout's row (3.8, issue #365), and the
desktops' tiles of section 4 group 8 and section 5 (3.9, issue #366), and
the agents' group's header, its sources and the tools plugged in (section 4
group 3, section 10; 4.0, issue #383), and its rows, one per agent, with
their pandas (section 4 group 3, section 7 A, section 8 in part; 4.1,
issue #384), and going to an agent from its row, its streak, its rim's
pulse and herdr's pane (section 8; 4.3, issue #386), herdr's terminal
brought for its agents (4.3c, issue #500), and the agents' halo
(section 6; 4.4, issue #387), the panda peeking out of the bar's edge
(section 7 C; 4.5, issue #388), the secretary at the agents' group's
head and her briefing (section 7 B; 4.6, issue #389), the mini form's
agents' item (section 5; 4.7, issue #390), the footer (section 4
group 11, section 5; 5.5, issue #395), and the bar under the windows,
brought in front by the screen's edge (sections 2 and 3; 5.13, issue
#519), and the tray, the applications' icons over the footer (section 4
group 11, section 5, section 9; 5.14, issue #531), and the sign under the
clock of what uses the microphone, the camera or the screen, and its
popover (5.12, issue #445), and the next meeting's card under the clock,
its mark in the mini form and the halo's meeting states (section 4 group
2, section 5, section 6; Day.2, issue #426), and Today, the list of
today's events after it (section 4 group 4; Day.3, issue #427), and the
column's order, the agents' group under the to-do list in both forms
(sections 4 and 5; issue #599), and the windows of the screen's current
desktop over its desktops, each with its ✕, and the mini form's item
listing them (sections 4 and 5; issue #604): see [as built](#as-built).

References: the design report (slides) and the design canvas, sections
"Proposition révisée" and "Prototype animé". The research behind the
choices is in the report: interruptions and breakpoints (Iqbal & Bailey,
CHI 2008), motion in the periphery (Bartram, Ware & Calvert, 2003), calm
technology (Weiser & Brown; Case), cute imagery and focus (Nittono et al.,
2012), edge targets (Fitts).

## 1. Purpose

The bar is the desktop's one permanent surface. It answers, at a glance:

- does an agent need me, which one, since when;
- what is next in my day (meeting, events, tasks);
- the state of the machine (network, sound, battery, keyboard layout,
  phone).

It never interrupts: what concerns the user is signalled in the bar (the
halo) and acted on when the user chooses. It does not duplicate the
agents' tools (Claude Code, Codex, herdr): it signals and takes the user
there.

## 2. Units and placement

- All sizes are logical pixels; they follow each output's scale.
- The bar sits against the right edge of its output, 8 px from the top,
  right and bottom edges. Windows keep 8 px from the bar, as from the
  screen edges.
- **Under the windows** (decided 2026-11-02, #519): the bar sits under
  normal windows. The windows the desktop places (new, maximised,
  snapped) still keep off it: its space stays reserved. A window the user
  moves or resizes onto it covers it, and takes the pointer there. The
  bar's popovers, the halo, the peeking panda, the bubbles and the
  launcher stay above the windows. A fullscreen window covers the bar and
  all of them, as before. Pushing the pointer against the screen's right
  edge, over the bar, brings it in front after **300 ms** at rest there,
  until the pointer leaves the bar (§3).
- Corner radius 14 px. Windows: radius 8 px and a 1 px light rim
  (`rgba(255,255,255,0.22)` on dark themes); the focused window's rim is
  the accent.
- One bar per output where the panel shows (setting `[panel] screens =
  "primary" | "all"`, default `primary`). Every visible bar shows the same
  halo.
- The primary screen: the one `[panel] output` names when it is
  connected, else the built-in panel (eDP, LVDS, DSI) when it is connected,
  else the first screen. When that screen is unplugged, the bar goes to a
  screen still there, and comes back with it.

## 3. Two forms

| Form | Width | Shows without a click |
|---|---|---|
| Full | 196 px | everything in section 4 |
| Mini | 56 px | clock, next meeting mark, the to-do item (Day.4), the agents' item under it (the secretary, how many need the user, how many work; 4.7), battery, keyboard layout, the bell with its count, the user's avatar (5.5) |

Default form per output: **full** when the output's usable width (logical,
without the bar) is at least 1600 px, **mini** otherwise. With these
defaults:

| Output | Default |
|---|---|
| 1366×768 at scale 1 | mini |
| 1920×1080 at scale 1 | full |
| 2560×1600 at scale 2 (1280×800 logical) | mini |
| 2560×1600 at scale 1 | full |
| 3440×1440 at scale 1 | full |

The user's choice overrides the default and is remembered per output
(`[panel] form = "auto" | "full" | "mini"`, per output in `[[outputs]]`).

### Switching

- A click on the bar's outer edge zone toggles the form: the 8 px strip
  between the bar and the screen edge plus the bar's first 6 px. The
  screen edge stops the pointer, so the target needs no aiming (Fitts).
  The compositor takes that click itself and no client sees it: it knows
  where the pointer is even when it reached the edge in one fast move,
  with no motion over the bar before the press.
- `Super+B` toggles the form on the output under the pointer.
- Held against the screen's edge over the bar for 300 ms, the pointer
  brings the bar in front of the windows over it (§2), until it leaves the
  bar's column; a click there still toggles the form.
- The bar's width animates over **240 ms**,
  `cubic-bezier(0.2, 0.9, 0.25, 1.08)` (a slight overshoot). Windows that
  touch the bar's side resize in the same frames. The contents cross-fade
  in 120 ms, starting at 60 ms.
- With reduced motion, or a shell drawing on the CPU: no animation; the
  form changes on the next frame, the windows with it.

## 4. Full form, top to bottom

Padding 14 px top, 12 px sides, 12 px bottom; 14 px between groups.

Top to bottom (asked by the maintainer on 2026-11-04, issue #599): the
clock (group 1), the next meeting's card (2), the sign of what is in use
(5.12), Today (4), To do (5), the agents' group (3: its header, its rows
and "+ N autres"), the spacer (6), the desktops (8), the connections (9),
the levels (10), the footer (11); the windows (#604) sit in the spacer,
14 px over the desktops. The groups keep their numbers, which
the rest of this document and the code use; the table lists them in that
order. A group that does not show takes no room, nor its 14 px. On a
short bar the groups over the spacer give way in this order: Today first
(its lines, then the group, #572), then To do's lines (into its "+ N",
its header staying), then the agents' rows (into "+ N"). The peek (§7 C),
the halo (§6) and the secretary's briefing (§7 B) stay with the agents'
group where it is.

| # | Group | Contents | Size |
|---|---|---|---|
| 1 | Clock | `HH:MM` in Martian Mono 400, 42 px, `text`, every character in a fixed cell (a `0`'s width, the colon 0.55 of it), centred; seconds in Martian Mono 300, 18 px, `text.faint`, on the same baseline, 4 px after; date 6 px below in Manrope 15.5 px, `text.dim` ("mercredi 28 octobre", "Wednesday 28 October"); on a minute change, the odometer (§4.1) | 72 px |
| 2 | Next meeting | Card (Day.2, variant B): 6 px colour bar, title (bold, 1 line, ellipsis), `HH:MM · dans N min`; a click opens the meeting, a button joins its call. Hidden when nothing remains today (as built, Day.2) | 44 px |
| — | In use | The sign of what uses the microphone, the camera or the screen, under the card (5.12, as built): a tile a kind in use; none when nothing is | 36 px |
| 4 | Today | The events after the meeting card (Day.3, variant A): time, colour dot, title, as many as fit, then "+ N autres aujourd'hui" (as built, Day.3) | 18 px header + 20 px a line |
| 5 | To do | Approved 2026-11-04 (issue #428, variant C; as built, Day.4): the open tasks of the board's tasks' column chosen in Day.1 (else the local list), in the column's order; an 18 px header, "À FAIRE" / "TO DO" (Manrope 700, 11.5 px, spaced 0.1 em, `text.dim`) and on its right the `+` (22 × 18, radius 6, a 14 px plus) that opens the field to add a task as the first line; up to 3 tasks on 22 px lines (fewer on a short bar once Today gave way, #599), each a 14 px box (radius 4, a 1.5 px `rgba(255,255,255,0.38)` border) in a 22 × 22 button and its title (13 px, cut, whole in a tooltip after 600 ms); a click ticks it (`#6CC287`, a dark check, the title struck through and `text.faint`), the line goes 1.5 s later, a second click within them unticks; a change refused says "Pas enregistré" for 3 s; past 3, "+ N autres" opens the tasks' popover (§9). No board and no local task: hidden; a board with nothing open: "Rien à faire." | 18 px + 22 px a line (+ 20 px for "+ N", + 30 px for the field) |
| 3 | Agents | Header: the secretary (§7 B, 4.6, variant B), her 40 px head, `AGENTS` and the total over her line; "Sources" in her briefing. One 34 px row per agent (4.1, track 4 "the lines"): its panda (30 px, §7 A), its name (as herdr names it, else its project's folder), and on its right what it does: how long it has worked ("12 min"), "t'attend", "bloqué", "à voir" (finished, not seen yet) or "repos"; waiting, stuck and to-see rows tinted. Waiting first (the longest first), then stuck, to see, working, resting; at most 5 rows (fewer on a short bar, as many as fit: they give way last, #599), then "+ N au repos" (or "+ N autres"). A click on a row goes to the agent (§8). Before any tool is plugged in, a card proposes those found (4.0); with none followed and none to propose, the group does not show (variant B) | 26 px + 34 px a row (+ 20 px for the rest) |
| 6 | Spacer | Takes what is left | flexible |
| — | Windows | The windows of the screen's current desktop (#604, as built): the focused one first, then by most recent focus, at most 3 on 30 px lines (its icon, its title, a ✕ that asks it to close), the focused one lit; past 3, "+ N autres" opens the desktop's list; none without a window; in the spacer's room only, 14 px over the desktops; `[panel] windows = false` hides it | 30 px a line (+ 20 px for "+ N") |
| 7 | Secretary | Moved to the head of group 3, the agents' (4.6, variant B) | — |
| 8 | Desktops | A tile per desktop of the bar's screen (§4.4), 4 to a row, 30 px high, with a sketch of its windows and their applications' icons; the current one ringed in the accent; resting on one lists its windows | 30 px a row, 6 px apart |
| 9 | Connections | One 30 px row per connection, like a level's (§4.3): Wi-Fi (network name), Bluetooth (device), phone (name, battery), keyboard layout (its name and code; a click goes to the next) | 30 px a row, 2 px apart |
| 10 | Levels | Volume, brightness and battery rows (§4.2): an 18 px icon, an 8 px gauge with its gradient, the value in tabular figures; then the status line, the battery's time left and the power profile | 108 px |
| 11 | Footer | Four 40 × 34 buttons spread evenly across the bar's inside (5.5, variant A): the clipboard, the notifications' bell (its badge counting what was not seen, 5.4), the menu (the launcher, on a click only) and the user's avatar (their initial on the accent's gradient); a 150 ms hover or a click opens the clipboard's, the bell's and the user's popovers (as built, 5.5). Just above it, the tray (5.14, variant A, always visible): the applications' icons (StatusNotifierItem), a row of 34 × 34 cells 4 px apart under a 1 px `rgba(255,255,255,0.08)` line, wrapping past four; none without an icon (as built, 5.14) | 34 px (+ 17 px and 38 px a row of icons) |

### 4.1 The clock (approved 2026-10-28, issues #359 and #332; sizes of #378, 2026-10-29)

- **Cells.** Each character sits in a cell as wide as the font's `0` (the
  colon 0.55 of it) and 1.05 em high, centred in it, so that nothing
  shifts as digits change; the row (`HH:MM` and the seconds) is centred in
  the bar.
- **The odometer.** On a minute change, each changed digit moves inside
  its cell, which clips it: the old one leaves upward (105 % of its line)
  fading and blurring to 3 px in 360 ms,
  `cubic-bezier(0.4, 0, 0.8, 0.4)`; the new one comes from 105 % below,
  blurred 3 px, to 8 % above at 70 % of 520 ms, then home, each stretch
  `cubic-bezier(0.2, 0.9, 0.25, 1.08)`. The seconds change without motion.
  With reduced motion, or a shell on the CPU: no motion.
- **Settings**, `[panel.clock]`, for users who like other things: `font`
  (the bundled Martian Mono, or any installed family; one not installed
  falls back to Martian Mono), `color` (`#EEF1F6`), `transition`
  (`odometer`, or `roll`: the new digit rises 60 % and fades in, 340 ms;
  `flip`: the old folds away about its bottom in 220 ms, the new unfolds
  from its top with a slight overshoot in 480 ms; `neon`: it flickers on
  in 700 ms; `none`), `seconds` (`true`).
- **Time.** The wall clock in the system's time zone (summer time, a zone
  changed, a clock set or a resume show within a second); the next look
  is on the monotonic clock, just past the next wall-clock second.

### 4.2 The levels (approved 2026-10-29, issue #360; sizes of #378)

- **Rows**, 10 px apart, Manrope 14.5 px `text.dim`: the icon (sound,
  sun, battery; `text`; 18 px), 8 px, the gauge (8 px high, radius 4,
  track `rgba(255,255,255,0.10)`, fill with its gradient from the left),
  8 px, the value right-aligned in 36 px. The sound's gauge is `gauge.sound`,
  the brightness's `gauge.light`, the battery's `gauge.battery`, amber
  (`#E8A13A → #F5C46E`, value and status `#F5C46E`) at 20 % or less, orange
  (`#E5793F → #F7B98A`, `#F7B98A`) at 10 % or less, both only off the
  charger. Muted: the speaker crossed in `text.faint`, the fill
  `rgba(255,255,255,0.22)` at the volume's length, the value "muet" /
  "muted" in `text.faint`. On AC (charging, full, or held by a charge
  limit; never on battery): a bolt `#7EE0A0` in the battery (3.7).
- **The status line**, Manrope 13.5 px, right-aligned, `text.faint` (the
  battery's band colour when low or critical): "3 h 40 restantes ·
  équilibré" / "3 h 40 left · balanced"; "plein dans 1 h 10" / "full in
  1 h 10"; "branché, plein" / "plugged in, full"; "branché" / "plugged in"
  (a charge limit); without an estimate yet, "sur batterie" / "on battery"
  and "en charge" / "charging"; at 10 % or less, "· branche le chargeur" /
  "· plug in the charger" after the time. The profile: "économie",
  "équilibré", "performance" / "power saver", "balanced", "performance".
  Without a battery, the profile alone; without either, no line. Too long
  for the bar, it breaks between its parts.
- **Missing**: a row whose service or device is not there (no PipeWire,
  no backlight, no battery, no UPower) is hidden, and the group closes up
  from the bottom.
- **Motion**: a gauge's length moves to a new value in 160 ms, `ease-out`;
  none with reduced motion or a shell on the CPU. The numbers change at
  once.
- **Keys**: the keyboard's volume up and down (5 %, up to 100 %), mute,
  brightness up and down (5 %, never below 5 %) work in the session; a
  volume key unmutes. Their only feedback is the gauge: no popup, no sound.
  No sound and no window for a low battery either. The gauges are read
  only here; their popovers are §9's (the sound's since 3.4a, the
  brightness's and the battery's since 3.7).

### 4.3 The connections (approved 2026-10-29, issue #362, variant B)

- **Rows**, not the first draft's 2×2 tiles, so that a network's name
  reads in full: one 30 px row per connection (6 px side padding inside a
  −6 px margin, radius 8), 14 px above the levels. An 18 px icon, 8 px,
  the name in Manrope 14.5 px, cut with an ellipsis. Lit
  `rgba(255,255,255,0.06)` under the pointer, `0.08` while its popover is
  open, as the sound's row. The Wi-Fi's, then Bluetooth's under it
  (3.6), then the keyboard layout's (3.8); the phone will add its own. A
  row whose service is missing is not drawn, and the others close up from
  the bottom.
- **The Wi-Fi's row**: connected, the Wi-Fi icon with its dot and three
  arcs, 1 to 4 lit in `text` by the signal (the others
  `rgba(255,255,255,0.22)`), and the network's name in `text`; on its way,
  every arc in `text.dim` and "Connexion…" / "Connecting…" in `text.dim`;
  Wi-Fi on without a network, the icon slashed and "Hors ligne" /
  "Offline" in `text.faint`; Wi-Fi off, slashed, "Wi-Fi coupé" / "Wi-Fi
  off"; a cable connected (first, whatever the Wi-Fi does), a cable's icon
  and "Câble" / "Wired". A 7 px `#F2945B` dot (ringed with the bar's
  colour) at the icon's top right when a sign-in page waits.
- **Bluetooth's row** (approved 2026-10-29, issue #363, variant A), 2 px
  under the Wi-Fi's, the Bluetooth sign as its icon: one device connected,
  its name (`text`), and on the right its battery ("82 %", 13 px,
  `text.dim`, the digits in fixed cells) when it reports one; several,
  the one connected last, then a "+N" chip (12 px, `text.dim` on
  `rgba(255,255,255,0.08)`, radius 6, 5 px each side) before the battery;
  the name cut to leave them room. A 7 px `#F2945B` dot (ringed with the
  bar's colour) at the sign's top right while a connected device's battery
  is at 15 % or less. Connecting with nothing connected: the sign and
  "Connexion…" / "Connecting…" in `text.dim`. On, nothing connected: "Aucun
  appareil" / "No device", sign and words in `text.faint`. Off (or blocked
  by rfkill): the sign slashed, "Bluetooth coupé" / "Bluetooth off", in
  `text.faint`. No adapter, or no BlueZ: no row.
- **The keyboard layout's row** (approved 2026-10-29, issue #365, variant
  A), 2 px under Bluetooth's: the keyboard icon, the active layout's full
  name in `text` ("Français", "Anglais (US)" / "French", "English (US)"),
  cut with an ellipsis, and on the right its code in a chip (12 px bold,
  `text.dim` on `rgba(255,255,255,0.08)`, radius 6, 6 px each side). A
  click goes to the next layout, round the list: no popover, no popup, no
  sound; the row changes when the compositor says the layout changed,
  whatever changed it (the row, a group-switch option of the user's XKB
  options). No default shortcut: `Super+Space` stays the launcher's. Only
  with two layouts or more; one, no row. **The code**: the layout's XKB
  name in capitals ("FR", "US", "RU"); when another configured layout
  shares it, the variant's, spelled as the layout's name writes it when it
  holds it ("BÉPO" for `fr(bepo)`, "Français (BÉPO)"; "BEPO" in English,
  "French (BEPO)"), else as XKB names it ("INTL" for `us(intl)`). Lit
  `rgba(255,255,255,0.06)` under the pointer, ringed while it holds the
  keyboard.
- A click opens its popover (§9); the keyboard layout's goes to the next
  layout.

### 4.4 The desktops (approved 2026-10-29, issue #366, variant A)

- **Tiles**, 14 px above the connections: one per desktop of the bar's
  screen, 4 to a row, 6 px apart (rows 6 px apart: 5 to 9 desktops take a
  second or a third row, filled from the left), 30 px high, radius 6,
  `rgba(255,255,255,0.05)` (`0.10` under the pointer). In each, a sketch of
  its windows at their places and sizes on the screen, in stacking order:
  4 px in, radius 1.5, `rgba(236,239,244,0.20)`, the focused window of the
  current desktop `0.40` (3.9b; `0.30` and `0.62` before the icons). Each
  window's rectangle carries its application's icon (from the icon theme,
  as the sound's applications find theirs, §9), 10 × 10 px, centred, with a
  1 px `rgba(14,17,28,0.6)` ring, when the rectangle is 12 × 12 px at least
  (else the rectangle alone). The current desktop ringed 2 px inside in the
  accent; the others a 1 px `rgba(255,255,255,0.10)` ring. An empty desktop
  shows its number (11 px, 600, `text.faint`).
- **A click** on a tile goes to that desktop, the strip sliding as with the
  shortcuts (windows.md §12). **The wheel** over the tiles goes to the next
  or the previous desktop, a notch a desktop, no further than the last or
  the first.
- **The list** (approved 2026-10-30, issue #473, variant A + B): resting
  150 ms on a tile (or a mini form's dot) opens a 240 px popover beside the
  bar (§9's hover mechanics: it closes 300 ms after the pointer leaves both,
  and stays while it moves toward it), its top on the tiles': "Bureau N"
  (14 px, 700) and "M fenêtres" / "M windows" on the right (12 px,
  `text.faint`); one 32 px row per window, front to back: its icon (16 px),
  its title (13.5 px, cut with an ellipsis). An empty desktop: "Aucune
  fenêtre." / "No window.". A click on a row goes to that desktop and
  focuses that window. A click on a tile opens no list (it goes to the
  desktop). The list follows windows opening, closing and retitling while
  it shows. Past 8 windows, it scrolls, as the Wi-Fi's list (the wheel, a
  touchpad, the row holding the keyboard kept in view); minimised windows
  come after the others, greyed (`text.faint`, their icon at 50 %), and a
  click on one brings it back on its desktop and goes there (3.9c,
  answered 2026-10-30, issue #479). `Enter` on a tile holding the keyboard
  goes to its desktop; it opens no list.
- **Motion**: the sketch follows the windows as they open, close, move and
  resize, and nothing draws at rest; nothing moves in the tiles but the
  ring, from the desktop left to the one shown (none with reduced motion).
- **Several screens**: a bar shows its own screen's desktops (sprint 8
  decides the rest).
- **Names** of desktops (6.14) come later; for now a tile's name for
  screen readers is "Bureau N, M fenêtres" / "Desktop N, M windows".

When the output is too short (1366×768 gives 752 px of bar), groups give
way in this order: Today, then To do, then the agent lines beyond the
first, then Levels collapse to battery only. Clock, meeting, agent tiles,
secretary, desktops and footer always stay.

Clicks:

- A tile or line of an agent: go to it (section 8).
- The meeting card: open the meeting (Day.5's calendar on today, the
  meeting chosen); its button joins the call (Day.2, variant B).
- The date and the clock: the calendar (Day.5, variant A), by a click
  only, in both forms.
- A connection tile or a gauge: opens its popover (section 9).
- A desktop tile: switch to it (§4.4); the wheel over the tiles, to the
  next or the previous one. A task's box: toggles it.

## 5. Mini form

Width 56 px, padding 12 px top, 10 px bottom, items centred, 14 px apart:
clock on two lines, hours over minutes, no seconds (Martian Mono 400,
19 px, line height 1.1, in cells as in §4.1, with the same motion), next meeting mark (30×4 px,
blue, glowing when under 10 min), the to-do item (approved 2026-11-04, issue #428, variant C; as built,
Day.4), 14 px under the meeting's mark (or the sign of what is in use,
or the clock), over the agents' item (#599): 44 × 34, radius 10, a 16 px box
(`text.dim`) and how many tasks are open (Martian Mono 11 px; no number
at 0), lit `0.06` under the pointer and `0.08` while its popover shows; a
150 ms hover or a click opens the tasks' popover (§9); no board and no
local task, no item;
then, 14 px under it, the agents' item (approved 2026-10-30,
issue #390, variant A: the secretary; it replaces the waiting tile, the
working count and the secretary further down): a button 48 px wide,
radius 10, 4 px in above and 6 px under, holding her 40 px head (her ear
in her mood, §7 B, her motion only on a change) and, at its bottom right
(6 px past it, 3 px under it), a 16 px badge (Manrope 700, 10 px,
`#1C1406`, 4 px in, a 2 px ring in the bar's colour) with how many agents
need the user (waiting, stuck and to see): amber `#F5B44A` when one
waits, else orange `#F2945B` when one is stuck, else green `#7EE0A0`; no
badge at 0. Under her, 4 px, how many work: "3" over "au travail" / "at
work" (Manrope 600, 11 px, line height 1.25; `text.dim`, the second line
`#7C8698`). Lit `0.06` under the pointer, `0.08` while her briefing is
open. A hover of 150 ms, or a click, opens her briefing (§7 B) with two
more sections at its end: "AU TRAVAIL" / "AT WORK" (each name and since
when) and "AU REPOS" / "RESTING" (each name); each line goes to its
agent. The group's rules hold: no item when the group is hidden (4.0); in
the card's place, she is alone with a green "+" badge, and opens Sources.
The halo and the peek work as in the full form, the peek at her head;
spacer, the desktops (3.9, variant
A): a dot per desktop (18 × 13, radius 4), two to a row, 4 px apart, 14 px
above the connections' buttons: the current one filled with the accent,
those with windows `rgba(255,255,255,0.30)`, the empty ones
`rgba(255,255,255,0.10)` with a 1 px `rgba(255,255,255,0.12)` ring; a click
goes to that desktop, the wheel as in the full form; resting 150 ms on a
dot opens its desktop's list (§4.4, 3.9b; the dots are too small for
icons); 14 px over the dots, the windows' item (#604, as built): the
first listed window's icon in a 44 × 34 button, which a 150 ms hover or
a click opens the windows' popover from (their lines with their ✕, and
"+ N"); the Wi-Fi's button (44 × 38 px, radius 10, a 20 px
icon in the row's states, the sign-in page's dot 9 px from its right and
7 px from its top), the sound's gauge
(vertical, 8 × 48 px, filled from the bottom, grey when muted), Bluetooth's
button under the Wi-Fi's (2 px apart, the same 44 × 38 px, a 20 px sign in
the row's states, the low battery's dot 10 px from its right and 7 px from
its top; 3.6), the
battery (3.7): a 26 × 15 battery, its outline `text`, filled to its level
in its band's colour, a bolt (`text`, edged `#0B1020`) while on AC, 4 px
above its percentage (Manrope 700, 14 px, `#7EE0A0`, amber or orange as in
§4.2); the whole a 44 × 46 item 14 px under the sound's gauge, which a
hover opens; the keyboard layout (3.8): its code alone (Manrope 700, 13 px,
`text`) in a 44 × 30 button, radius 8, 14 px under the battery's
percentage, which a click (not a hover) takes to the next layout, with two
layouts or more; the tray (5.14): a column of the applications' icons in
40 × 32 cells 2 px apart, under its line, 10 px over the bell, the items
above going up by it; at the bottom, the bell (44 × 34, its badge) and, 2 px
under it, last of all, the user's avatar (44 × 34, 24 px round), which
opens the same popover as the full form's (5.5); the clipboard and the
menu have their shortcuts instead (`Super+V`, 5.6; `Super+Space`).

A hover of 150 ms on an item opens its popover beside the bar; it floats
over the windows and closes 300 ms after the pointer leaves, unless the
pointer moves toward it (section 9).

## 6. The halo

Approved 2026-10-30 (issue #387), with variant B for finished agents.

The whole bar lights up when something concerns the user. One state shows
at a time, by priority:

| Priority | State | Colour | Wave | Then | Reminder |
|---|---|---|---|---|---|
| 1 | Meeting in 2 min or less | blue `#8FB3FF` | strong (`0.42`) | strong glow | — |
| 2 | An agent waits for the user | amber `#F5B44A` | yes (`0.38`) | glow | one wave after 5 min |
| 3 | Meeting in 10 min or less | blue `#8FB3FF` | softer (`0.22`) | glow | wave again at 2 min (priority 1) |
| 4 | An agent is stuck (failed) | orange `#F2945B` | no | glow | — |
| 5 | A finished agent not seen yet | green `#6CC287` | no | soft glow | — |
| 6 | Nothing | none | — | — | — |

- **Wave**: a band 40 % of the bar's height, the state's colour at the
  table's alpha in its middle (`linear-gradient` transparent, colour,
  transparent), runs from top to bottom once in **600 ms**,
  `cubic-bezier(0.3, 0.6, 0.3, 1)`.
- **Glow**: border in the colour at 85 %, outer glow 34 px at 45 %, inner
  glow 22 px at 18 %. The strong glow: outer 44 px at 60 %. The soft glow:
  border at 45 %, outer 20 px at 25 %, inner 14 px at 10 %. It fades in
  over 600 ms (`ease-out`) and out over 500 ms (`ease-in`).
- Lower states still show where they belong (the row, the meeting card):
  only the halo is unique.
- A reminder is one more wave, never a loop. The user acting ends the
  state at once: going to the agent (§8), answering it, seeing the finished
  one, opening the meeting.
- With reduced motion: no wave; the glow fades in (and out) over 200 ms.
- The meetings come with the calendar ("Your day"); 4.4 builds the
  agents' states and the priorities, ready for them.
- It costs frames only during the wave and the fades; nothing is drawn at
  rest.
- It stays above the windows: over one covering the bar, it shows on it
  (#519).

## 7. Pandas

The pandas are the validated characters (the apprentice magician family).
They move only when something changes, then hold still; never at rest,
never on battery saver, never with reduced motion.

- **A, a panda per state** (in the agents' rows, 30 px; 4.1): working (at
  its laptop), waiting (holds out its hat, ear radiating), finished (arms
  up), stuck (smoking ears), resting (asleep, at 55 %). On a change, the
  changed agent's panda hops once (650 ms: 4 px up, down, 2 px up, down).
- **B, the secretary** (approved 2026-10-30, issue #389, variant B: she
  heads the agents' group): one panda with round glasses, a clipboard, and
  a pencil behind her ear (the validated drawing, `Secretaire.dc.html`).
  The group's header is her 40 px head, then "AGENTS N" (11.5 px 700) over
  her line (12 px, ellipsis); "Sources" moves into her briefing. Her magic
  ear follows the moment: green when calm; amber when someone waits (it
  radiates twice, two small hops, 1.2 s in all, only at the change);
  orange when an agent is stuck (no motion). Her line: "N au travail · M
  à voir" when calm (`text.dim`); only the waiting agents' names when
  someone waits ("site-web, docs", amber); only the stuck agents' names
  when one is stuck (orange); cut with an ellipsis (#503). The header row
  is lit amber or orange at 8 % / 7 % in those states.
  A click on her opens her **briefing** beside the bar (320 px; §9's
  mechanics), in this order: T'ATTEND, each waiting agent, since when,
  and what it asks (the text of the tool's notification, "Autoriser Bash
  : npm install ?"); BLOQUÉ, each stuck agent, since when, and why, from
  the failure's message; À VOIR, each finished agent not seen yet, and
  since when; PROCHAINE RÉUNION, with the calendar, later; NOTIFICATIONS,
  with sprint 5, later (nothing shows of either before it exists); a
  footer "N au travail · M au repos", and "Sources" (the Sources
  popover). Each line acts like its row: it goes to the agent (§8). She
  never opens by herself. In the mini form she is the agents' item (§5,
  4.7), whose hover or click opens her briefing, with who works and who
  rests at its end.
- **C, the peek** (approved 2026-10-30, issue #388: 8 s, and also when an
  agent finishes):
  - **When**: an agent starts waiting (its panda holding out its hat), or
    an agent finishes (its panda with arms up). Once per change, and once
    more with the 5-minute reminder of a waiting agent (§6). Never for a
    stuck agent: the orange line and glow show it.
  - **Where**: the 96 px panda comes out of the bar's edge at the agent's
    row's height, 18 px from the bar, from under the bar's surface. Beside
    it, 8 px before it, a bubble: Manrope 13 px, 8 px and 12 px in,
    `rgba(30,26,18,0.92)` (`rgba(40,32,18,0.96)` under the pointer), radius
    12, a 1 px border in the state's colour at 45 % (amber waiting, green
    finished), `box-shadow: 0 10px 30px rgba(0,0,0,0.35)`; the panda's
    `drop-shadow(0 8px 16px rgba(0,0,0,0.4))`. "**site-web** t'attend · à
    l'instant" / "**site-web** is waiting · just now" (the word `#F5C46E`,
    since when `text.faint`), "**api-paiements** a fini" / "**api-paiements**
    is done" (`#7EE0A0`).
  - **Motion**: out in 320 ms, `cubic-bezier(0.2, 0.9, 0.25, 1.15)`, from
    150 px under the bar, fading in over 260 ms (`ease-out`); still while
    out; back into the bar after **8 s**, in 260 ms (`cubic-bezier(0.5, 0,
    0.75, 0.3)`), fading out over 220 ms (`ease-in`). With reduced motion,
    or a shell on the CPU: it fades in and out in place, without sliding.
  - **A click** on it (panda or bubble) goes to the agent (§8), the streak
    from the panda, and it goes back at once.
  - Several at once: the latest replaces the one out; it never stacks.
  - Nothing is drawn at rest: it is a surface of the shell's above the
    windows, beside each bar, taking the pointer only where it draws, and
    gone once back.

## 8. Going to an agent

A click on anything that names an agent (approved for its row in 4.3,
2026-10-30, variant A):

1. When the agent's window is on another desktop, the strip slides there
   first, as the shortcuts do (320 ms, §12 of windows.md).
2. A streak runs from the row's left edge to the centre of the agent's
   window in **300 ms**, `cubic-bezier(0.3, 0.7, 0.2, 1)`, then fades out
   in 220 ms (`ease-in`): a 4 px line with round ends, transparent at the
   row and full at its head, and a 12 px glow of its colour; the glow
   fades in with the line, so the streak starts from nothing at the row
   (no dark core near its tail; issue #495). **Its colour
   is the row's state**: waiting `#F5B44A`, stuck `#F2945B`, to see
   `#7EE0A0`, working `#6CC287`, resting `#A9B2C2`. It is drawn by the
   compositor over every window and under the bar, and costs frames only
   while it runs.
3. The agent's window has the keyboard (on its output); as the streak
   lands, its rim pulses once in the accent (600 ms): a 2 px rim, and a
   ring growing from 4 px to 14 px while both fade. A window that opens
   without the keyboard pulses the same way (windows.md §4).
4. herdr selects the agent's pane (herdr's API).
5. The halo and the peek end; the secretary returns to calm unless
   another agent waits.

With reduced motion, or a compositor on the CPU: no slide and no streak;
the rim lights and fades out.

## 9. Popovers

Approved 2026-10-29 (issue #361) for the mechanics and the sound's.

- Width 280 px (briefing 320 px), radius 14 px, same glass as the bar
  (`bar.bg` a little denser: `rgba(22,27,42,0.72) → rgba(14,17,28,0.82)`),
  its rim, a soft shadow (`0 14px 40px rgba(0,0,0,0.38)`); 8 px from the
  bar, aligned with the item that opened them, pushed up to stay 8 px from
  the screen's bottom. One at a time.
- **Opened by a click** on its item (in the full form, the whole row of a
  level, lit `rgba(255,255,255,0.06)` under the pointer and `0.08` while
  its popover is open; the pointer a hand over it, as over the
  connections' rows and the desktops' tiles, set again each time the
  pointer comes back onto one of the shell's surfaces, whatever another
  window made of it meanwhile (#553)): it stays open until a click outside it, `Esc`, or
  another click on the item; it does not close when the pointer leaves.
- **Opened by hovering** an item of the mini form for 150 ms: it closes
  300 ms after the pointer has really left the item and the popover, and
  stays open while the pointer moves toward it (a cone from the pointer to
  the popover's near corners). A click on the item opens it as in the full
  form.
- `Esc` closes; focus returns to the item, where `Enter` or `Space` opens
  it again and `Esc` lets the keyboard go. `Tab` goes through what is in
  the popover; `Enter` or `Space` acts.
- It fades in and slides 8 px away from the bar in 160 ms, `ease-out`;
  out in 120 ms, `ease-in`; neither with reduced motion or a shell on the
  CPU.
- **The wheel** over the sound's row (or the mini form's gauge) changes
  the volume by 5 % a notch, up to 100 %, unmuting as the keys do, without
  opening the popover.
- **The sound's** (3.4a): the title "Son" / "Sound" and the default
  output's name; the master volume (a mute button, a slider, the value or
  "muet" / "muted"); "SORTIE" / "OUTPUT" and one row per output, its icon
  (speaker, headphones, screen), its name, a check on the default one. A
  row makes its output the default, which keeps its own volume. No
  "Sound settings…" link until the Settings sprint.
- **Its microphone and applications** (3.4b, issue #380), under the
  outputs with the same divider and labels: "MICRO" / "MICROPHONE", a
  36 px row (a 32 px mute button, the mic icon crossed and `#F7B98A` on
  `rgba(229,121,63,0.18)` when muted; the slider as the master's, grey
  when muted; the value or "coupé" / "off"), then, only with several
  microphones, one 36 px row each, indented 48 px, the default checked:
  a row makes it the default. No microphone: no section. Then
  "APPLICATIONS": one 46 px row per application playing, its icon (or
  its letter on a colour of its own) in a 32 px tile, its name over a
  slimmer slider (track 6 px, knob 14 px), its value; a click on the
  tile mutes it (tile at 45 %, name and value `text.faint`, "muet" /
  "muted"); none: "Aucune application ne joue de son." / "No application
  is playing sound.". Moving a slider unmutes what it sets. The popover
  grows to fit, its bottom on the bar's.
- **The Wi-Fi's** (3.5, issue #362), its bottom on the bar's whatever its
  height: "Wi-Fi" (Manrope 700, 16 px) in a 32 px header with a switch
  (40 × 22, radius 11, `#6CC287` on, `rgba(255,255,255,0.18)` off, an
  18 px white knob sliding in 160 ms, `ease-out`; at once without motion);
  no switch without a Wi-Fi adapter, and "Aucune carte Wi-Fi." / "No Wi-Fi
  adapter." instead. A cable connected: a 32 px line, its icon and
  "Connecté par câble" / "Connected by cable" (13.5 px, `text.dim`). Off:
  "Le Wi-Fi est coupé." / "Wi-Fi is off." and nothing else. The network
  connected: a block of 48 px at least on `rgba(108,194,135,0.10)`,
  radius 8, its icon in `#6CC287`, its name (14 px, 600, cut), under it
  "Connecté" / "Connected" (12.5 px, `#6CC287`) or, behind a sign-in page,
  "Page de connexion à ouvrir" / "Sign-in page to open" (`#F7B98A`, on two
  lines beside the button, the block growing), and "Déconnecter" /
  "Disconnect" (28 px, radius 7, `rgba(255,255,255,0.06)`, 12.5 px
  `text.dim`). Behind a sign-in page, a 34 px button
  (`rgba(229,121,63,0.18)`, `#F7B98A` 13.5 px 600) "Ouvrir la page de
  connexion" / "Open the sign-in page" opens NetworkManager's check address
  in the browser, where the page shows itself. Then the divider,
  "RÉSEAUX" / "NETWORKS" and, while the scan asked as it opened runs,
  "Recherche…" / "Searching…" with a 12 px spinner (none without motion);
  one 36 px row per network but the connected one: its signal, its name
  (14 px, cut), "enregistré" / "saved" (12 px, `text.faint`) when a
  connection is saved for it, "dans les Réglages" / "in Settings" for those
  it cannot join (802.1X, WEP: their rows greyed and inert), a 14 px lock
  for protected ones; strongest first; past six rows the list scrolls
  (236 px at most: six rows and a peek of the seventh, a 6 px thumb). None:
  "Aucun réseau trouvé." / "No network found." ("Recherche des réseaux…" /
  "Searching for networks…" while the first scan runs). A saved or open
  network joins with a click, its row saying "Connexion…" with a spinner
  until NetworkManager says it is done, the others inert meanwhile. A
  protected one unfolds under its row (the row's block lit
  `rgba(255,255,255,0.05)`) a 34 px field ("Mot de passe" / "Password",
  dots, an eye to show it), "Annuler" / "Cancel" and "Se connecter" /
  "Connect" (green from 8 characters). `Ctrl+V` or `Shift+Insert` in the
  field pastes the clipboard's text at its end (3.5b, issue #453): its
  line breaks and other control characters dropped, whole if it fits in
  the field (64 characters at most) or not at all, clearing the error as
  typing does; nothing from a clipboard without text. It is kept as a
  typed one is. `Enter` tries it, `Esc` folds it (a second `Esc` closes
  the popover). Refused: "Mot de passe incorrect.
  Vérifie-le et réessaie." / "Wrong password. Check it and try again."
  (12.5 px, `#F7B98A`, wrapped) and the field's border `#F2945B`, the
  field keeping what was typed and the keyboard. A saved network whose
  password is refused unfolds its field the same way. The keyboard: `Tab`
  goes through the switch, Disconnect, the sign-in page's button, each
  network that can be joined, then in an unfolded row its field, its eye,
  Cancel and Connect; the arrows go through the networks.
- **Bluetooth's** (3.6, issue #363, variant A), 280 px, its bottom on the
  bar's as the Wi-Fi's: "Bluetooth" (16 px, 700) and its switch (the
  Wi-Fi's; the adapter's `Powered`). Off: "Le Bluetooth est coupé." /
  "Bluetooth is off." and nothing else. Each device connected, the last
  first, in a block of 48 px at least on `rgba(108,194,135,0.10)`, radius
  8: its kind's icon in `#6CC287` (headphones, speaker, mouse, keyboard,
  phone, else the Bluetooth sign), its name (14 px, 600, cut), "Connecté" /
  "Connected" (12.5 px, `#6CC287`) and its battery, " · batterie 82 %" /
  " · battery 82 %" (`#F7B98A` at 15 % or less), which goes under it,
  whole, when it does not fit beside "Déconnecter" / "Disconnect" (the
  Wi-Fi's button), the block growing. Then the divider, "MES APPAREILS" /
  "MY DEVICES" and the paired devices not connected, by name, one 36 px
  row each (their icon, their name): a click connects one, its row saying
  "Connexion…" with a 12 px spinner, the others inert meanwhile; refused
  or given up: "Il ne répond pas. Vérifie qu'il est allumé et à portée." /
  "It does not answer. Check that it is on and in range." (12.5 px on
  1.4 lines, `#F7B98A`, 34 px in) under its row. None: "Tous tes appareils
  sont connectés." / "All your devices are connected.". Then the divider,
  "AUTOUR" / "NEARBY" and, while the search's first pass runs,
  "Recherche…" with a spinner; the devices found around, not paired, named
  (one that never said its name is left out), the strongest first, one row
  each; past four the list scrolls (152 px, a 6 px thumb). A click pairs
  one, trusts it and connects it ("Association…", then "Connexion…" as it
  joins "my devices", then its block); failed: "L'association a échoué.
  Mets l'appareil en mode association et réessaie." / "Pairing failed. Put
  the device in pairing mode and try again." under its row. One that asks
  for a code (a phone or a computer, or one that asked the agent for one)
  is greyed, inert, "dans les Réglages" / "in Settings" (6.7 pairs those).
  None: "Aucun nouvel appareil trouvé." / "No new device found."
  ("Recherche des appareils…" / "Searching for devices…" while the first
  pass runs). The rows around keep their order while the pointer is over
  them, one holds the keyboard, or something runs. The keyboard: `Tab`
  goes through the switch, each Disconnect, then the rows that can be
  acted on; the arrows through the rows; `Enter` or `Space` acts; `Esc`
  closes, as the Wi-Fi's. The search runs while the popover is open, and
  stops as it closes.
- **The brightness's** (3.7, issue #364, variant B), opened by a click on
  the brightness's row (the full form only), its bottom on the bar's:
  "Luminosité" / "Brightness" (16 px, 700) and on the right "Écran du
  portable" / "Built-in screen" (13 px, `text.faint`). A 36 px row: the
  sun (18 px in a 32 px slot, not a button), a slider as the sound's (8 px
  track in `gauge.light`, 16 px knob) from 5 % to 100 % (never below, as
  the keys), the value in 36 px. The keyboard's backlight, when UPower has
  one: a 36 px row, the keyboard's icon, then its levels in a strip
  (`rgba(255,255,255,0.06)`, radius 9, 3 px inside, 26 px buttons 4 px
  apart, radius 7, 12.5 px 600; the chosen one `rgba(255,255,255,0.14)`
  and `text`, the others `text.dim`): "Éteint" / "Off", "Faible" / "Low",
  "Fort" / "High" for three levels; "Éteint", "Allumé" / "On" for two;
  "Éteint", "Faible", "Moyen" / "Medium", "Fort" for four; past four, a
  slider and its percentage. Then one line per other screen plugged in,
  "L'écran <name> se règle avec ses propres boutons." / "The <name> screen
  is set with its own buttons." (12.5 px on 1.4 lines, `text.faint`, 2 px
  more above, wrapped): the panel sets only the laptop's screen and its
  keyboard. The wheel over the brightness's row (the bar's or the
  popover's) moves it by 5 %; the keyboard: `Tab` through the slider and
  the levels (or the keyboard's slider), the arrows move a slider (5 %, a
  level) and choose the next level, `Enter` or `Space` chooses.
- **The battery's** (3.7), opened by a click on the battery's row and the
  status line under it (full form), or in the mini form by a 150 ms hover
  or a click on the battery; its bottom on the bar's: "Batterie" /
  "Battery" and on the right "Sur batterie" / "On battery" or "Branché" /
  "Plugged in". A 42 × 24 battery filled to its level in its band's
  colour, a bolt on AC; 12 px after it the level (Manrope 700, 26 px on
  1.1 lines) over its line (13.5 px, 1 px apart): "3 h 40 restantes" /
  "3 h 40 left", "En charge · plein dans 1 h 10" / "Charging · full in 1 h
  10", "Branché, plein" / "Plugged in, full", "Branché, ne charge pas" /
  "Plugged in, not charging" (a charge limit), "Sur batterie" / "On
  battery" and "En charge" / "Charging" without an estimate; at 10 % or
  less "· branche le chargeur" / "· plug in the charger" after it, the
  level and the line in `#F7B98A`, at 20 % or less in `#F5C46E`; too long
  for its 196 px, it breaks between its parts. The divider, "MODE
  D'ÉNERGIE" / "POWER MODE", three 58 px tiles 6 px apart (radius 10, a
  1 px border, an 18 px icon over its 12.5 px 600 name, 4 px apart):
  "Économie" / "Power saver" (a leaf), "Équilibré" / "Balanced" (scales),
  "Performance" (a bolt); the active one `rgba(108,194,135,0.16)` with a
  `#6CC287` border and `text`, the others `rgba(255,255,255,0.04)` with a
  `rgba(255,255,255,0.08)` border and `text.dim` (`0.08` under the
  pointer); one the machine does not have greyed (`rgba(255,255,255,0.28)`)
  and inert, with "Le mode performance n'existe pas sur cet ordinateur." /
  "This computer has no performance mode." under them (12.5 px on 1.4
  lines, `text.faint`). While performance is chosen and
  power-profiles-daemon holds it back, the reason the same way:
  "Performance réduite : l'ordinateur est sur tes genoux." / "Performance
  reduced: the computer is on a lap.", "… est trop chaud." / "… is too
  hot.", else "Performance réduite par le système." / "Performance reduced
  by the system.". Last, "État de la batterie : 91 % de sa capacité
  d'origine" / "Battery health: 91 % of its original capacity" (12.5 px,
  `text.faint`), when known. Gaps of 8 px. Without a battery but with
  power profiles, the status line (the profile alone) opens it with the
  modes only, under "Énergie" / "Power" (no divider, no label). Without
  power profiles, the battery alone. The keyboard: `Tab` through the tiles
  the machine has, the arrows choose the next, `Enter` or `Space` chooses.
- **The tasks'** (Day.4, issue #428, variant C), from the mini form's
  to-do item and from the full form's "+ N autres", its top on what
  opened it: "À faire" / "To do" (14 px, bold), the count of open tasks
  (12 px, `text.dim`) and a `+` as the group's; the field to add a task as
  its first line when open (30 px, radius 8); up to 6 tasks on 30 px
  lines, which tick as the group's; past 6 the list scrolls and "+ N
  autres" under it says how many are past the six; none: "Rien à faire."
- Contents: Wi-Fi (networks, connect), Bluetooth (devices, connect, their
  battery), sound (outputs, inputs, volume per app), brightness, battery
  and power profile, phone (KDE Connect: battery, notifications, send a
  file, ring), clipboard history (5.6, as built),
  notifications (clear all), menu (apps,
  settings, session), a tray icon's menu (its application's, 5.14, as
  built). The keyboard layouts have none: their row goes to the
  next layout (§4.3, 3.8).

## 10. Data sources

| Data | Source | Notes |
|---|---|---|
| Agents and their states | their sources, each its own adapter off the loop: Claude Code's hooks, Codex's hooks and `notify`, herdr's socket when it runs, and `hocus agent NAME STATE` for any other tool ([agents.md](../agents.md)) | working, waiting (a permission or a question), done, failed, idle; the same agent seen by two sources (its process) counted once; its window found above its process (as built, 4.0) |
| Meetings, events | calendar accounts behind `CalendarSource` adapters, each read by a worker of its own off the loop ([day.md](day.md)): Google Calendar through GNOME Online Accounts first (Day.1), a fixture's file for tests and screenshots; CalDAV and `.ics` later | the window of 2 months back to 12 ahead, cached in `$XDG_CACHE_HOME/hocuspanda/day`; the shell keeps the latest day and wakes a view only when it changes |
| Tasks | a kanban board behind a `KanbanSource` adapter (Planka first), else the local list in `$XDG_STATE_HOME/hocuspanda/tasks.json` ([day.md](day.md)) | the tasks' column's cards in its order; ticks, unticks and additions queued per account and tried again when the source answers |
| Network | NetworkManager | on the system bus: `WirelessEnabled`, `Connectivity` (2: a sign-in page) and `ConnectivityCheckUri`, the devices (the Wi-Fi one, a wired one, their state), the access points (SSID, `Strength`, `Flags`, `WpaFlags`, `RsnFlags`), the active connections and the saved ones (their SSID); followed by their signals; set with `RequestScan`, `WirelessEnabled`, `ActivateConnection`, `AddAndActivateConnection`, `Update`, `Disconnect` (as built, 3.5) |
| Bluetooth | BlueZ | on the system bus: the object manager's objects (`GetManagedObjects`, then `InterfacesAdded`, `InterfacesRemoved`), `PropertiesChanged` under `/org/bluez`; the first adapter's `Powered`, its devices' `Name`, `Alias`, `Icon` (else `Class`, else `Appearance`), `Paired`, `Connected`, `RSSI`, and `Battery1`'s `Percentage`; set with `Powered`, `StartDiscovery` and `StopDiscovery`, `Connect`, `Disconnect`, `Pair` (through the shell's `NoInputNoOutput` agent), `Trusted`, `CancelPairing` (as built, 3.6) |
| Sound | PipeWire | the sinks (their names, what they are, their own volume and mute) and the default one, as `pw-dump --monitor` streams the graph's changes; set with `wpctl` (`set-volume`, `set-mute`, `set-default`) |
| Battery, power profile | UPower, power-profiles-daemon | UPower's display device; `OnBattery` (on AC: the bolt); the first battery powering the machine among `EnumerateDevices` (`Type` 2, `PowerSupply`), its `Capacity` (the health; 0 or none: unknown); followed by `PropertiesChanged` under `/org/freedesktop/UPower`, `DeviceAdded` and `DeviceRemoved`. `org.freedesktop.UPower.PowerProfiles`: `ActiveProfile` (set by the popover), `Profiles` (those there), `PerformanceDegraded` (as built, 3.7) |
| Brightness | backlight through logind | read in sysfs, its changes from udev; set with logind's `SetBrightness` on the session |
| Keyboard's backlight | UPower's `org.freedesktop.UPower.KbdBacklight` | `GetMaxBrightness`, `GetBrightness`, `SetBrightness`; followed by `BrightnessChanged` and `BrightnessChangedWithSource`; none: no row (as built, 3.7) |
| Other screens | the compositor | their EDID's model, else their connector's name, on the `screens` line of `hocus-link` (as built, 3.7) |
| Phone | KDE Connect (D-Bus) | |
| Desktops | the compositor | each bar's screen's desktops: their count, the current one, and each window's desktop, place and size in thousandths of the screen, bottom first, whether it is the focused one or minimised, its id and its application's id, on the `desktops` line of `hocus-link`; the windows' titles on the `titles` line; a desktop asked with `desktop`, a window with `bring` (as built, 3.9, 3.9b, 3.9c) |
| Keyboard layouts | the compositor's XKB keymap | its layouts in its groups' order: their XKB codes (from `[keyboard] layouts`, else `/etc/default/keyboard`, else `XKB_DEFAULT_LAYOUT`) and the keymap's names (`xkb_keymap_layout_get_name`: the symbols files' group names, which xkeyboard-config writes as the registry's descriptions), and the active group, on the `layouts` and `layout` lines of `hocus-link`; the names in French from xkeyboard-config's gettext catalog (`/usr/share/locale/fr/LC_MESSAGES/xkeyboard-config-2.mo`, else `xkeyboard-config.mo`), read by the shell; the next layout asked with `next-layout` (as built, 3.8) |
| Notifications | the desktop's own server, `org.freedesktop.Notifications` on the session bus, in the shell ([notifications.md](notifications.md), 5.3); their history kept 7 days in `$XDG_STATE_HOME/hocuspanda/notifications` (5.4) | the bell's badge counts what was not seen; do not disturb holds the bubbles |
| Applications' icons (the tray) | the desktop's own `org.kde.StatusNotifierWatcher` (and `org.freedesktop.StatusNotifierWatcher`), its host, on the notifications' session bus, in the shell; each item's properties and signals, its `com.canonical.dbusmenu` menu (as built, 5.14) | `Passive` items hidden; the attention icon while one `NeedsAttention` |
| Microphone, camera, screen in use | PipeWire's graph, from the sound's `pw-dump --monitor`: `Stream/Input/Audio` nodes linked to a source, `Stream/Input/Video` nodes linked to a `Video/Source`, their `application.*` and `media.name` props; the processes holding `/dev/video*` open, from `/proc`, read on inotify's word that one was opened or closed, at most once a second and only while a camera's node exists; the compositor's `shares` and `recording` lines (5.11, 5.10) | the default source's mute greys the microphone; muted with `wpctl set-mute` as the sound's button (as built, 5.12) |
| Clipboard | the compositor, which sees every selection | what clients copy (their text, else their PNG image), read off its loop, never what is marked secret, on the `copied` line of `hocus-link`; kept by the shell, sealed, in `$XDG_STATE_HOME/hocuspanda/clipboard` (as built, 5.6) |

Every source is read off the event loop and pushes changes; nothing is
polled from the loop.

## 11. Accessibility

- Text holds 4.5:1 on the bar's darkest background (3:1 for 24 px and
  up). Colour never says a state alone: counts, names and words come with
  it.
- Every item is reachable with the keyboard (`Super+B` then arrows, Tab
  through items, Enter acts, Esc closes) and has a name for screen
  readers.
- Reduced motion: as stated in sections 3, 6 and 7.
- The glass falls back to an opaque background when "reduce
  transparency" is set, or behind a maximised window.

## 12. Performance

- At rest the bar draws once a second, for the seconds, and only the
  clock's region. Nothing else draws until something changes.
- An animation (form switch, wave, peek, streak) lasts at most 600 ms and
  draws only its region.
- No work on the loop per animation frame beyond drawing; data arrives
  from workers.

## 13. Theme tokens

| Token | Dark theme value |
|---|---|
| `bar.bg` | `linear(rgba(22,27,42,0.70) → rgba(14,17,28,0.78))`, blur 26 px, saturation 1.35 |
| `bar.border` | `rgba(255,255,255,0.10)` |
| `text` / `text.dim` / `text.faint` | `#EEF1F6` / `#A9B2C2` / `#8C96A8` |
| `accent` | `#6CC287` (the pandas' green) |
| `state.wait` / `state.meeting` / `state.stuck` | `#F5B44A` / `#8FB3FF` / `#F2945B` |
| `gauge.sound` / `gauge.light` / `gauge.battery` | `#5B8DEF→#8FB3FF` / `#E8B44E→#FFE08A` / `#3FA66B→#7EE0A0` |
| fonts | Martian Mono (clock), Unbounded (counts), Manrope (text) |

The light and neon themes give their own values for the same tokens.

## 14. Open questions

1. ~~The secretary's pencil drawing (section 7, B).~~ Decided on the 4.6
   mockup: the pencil rests behind her ear.
2. ~~Whether the peek's 8 s before it retracts is right.~~ Decided on the
   4.5 mockup: 8 s, and it also comes out when an agent finishes.
3. ~~The colour and animation of the clock when the minute changes.~~
   Decided on the 3.2 mockup: white, the odometer (§4.1).
4. ~~What the bar shows when no agent tool is running at all.~~ Decided on
   the 4.0 mockup (variant B): the group does not show, nor after "Ne rien
   brancher"; "Aucun agent en cours." once a tool is followed.

## As built

### 3.1, the empty column (2026-10-28)

What the approved mockup of 3.1 shows: the column of §2 with its glass
(§13) and its rim, nothing inside, its two forms and the switch (§3),
reduced transparency (§11) and reduced motion. The mockup's dashed slots
are later slices'.

- **Who does what.** The compositor decides where the bars are and in
  which form, and keeps their space whether the shell runs or not, so that
  nothing moves while the shell restarts. The shell (`hocuspanda-shell`,
  iced 0.14 and iced_layershell) draws one layer surface per bar, anchored
  right, top and bottom with 8 px layer-shell margins and an exclusive
  zone of 0: the bar itself, nothing transparent around it. It is as wide
  as the form, and as the full form while a switch is under way, so that
  the bar has room to move. The compositor handles the click on the edge
  zone (§3) in its own pointer code, before any client: the strip at the
  screen's edge belongs to no surface. They talk over a private socket
  (crate `hocus-link`, which documents the lines). The compositor starts the shell once ready,
  and again when it ends, after 0.5 s doubling up to 30 s while it keeps
  ending within 30 s.
- **The space kept** is the bar's final width as soon as a switch starts:
  the work area is the screen less 8 px around, less the bar and 8 px on
  the right. Windows snapped against it take their new zone at once and
  are drawn moving to it along the bar's motion; a floating window whose
  right edge touches the work area's keeps touching it.
- **The switch in step.** The compositor and the shell compute the bar's
  width from the same start on the same clock (`CLOCK_MONOTONIC`, sent
  once with the new form) and the same curve: the compositor moves the
  windows and the glass in its frames, the shell draws the bar in its own,
  about one refresh ahead (what it draws shows a frame later). Nothing is
  sent per frame, and the loop does no work per frame beyond drawing. A
  switch reversed midway starts from where the bar is.
- **The form** on a screen: `[outputs.NAME] panel-form`, else `[panel]
  form`; on `auto` (the default), the form last chosen there with a click
  or `Super+B`, else full from 1600 px left beside the full bar. A click's
  or `Super+B`'s choice is remembered in
  `$XDG_STATE_HOME/hocuspanda/panel.toml` when the screen's form is `auto`,
  never in `config.toml`; with `full` or `mini` set there, a switch lasts
  the session.
- **The glass.** On GLES the compositor draws what is behind the bar (the
  lower layers and the background; the windows too while the bar is in
  front of them, since 5.13) into a texture of its own
  through a damage tracker, over the full bar's place and the blur's reach
  (52 px, twice blur(26px)'s deviation) around it, and blurs it again only
  when that changed: the first build's dual Kawase blur, its last pass
  saturating by 1.35. It draws it under the shell's surface, cut to the bar's
  rounded rectangle as the bar is at that frame. The shell draws `bar.bg`
  (70 % to 78 %) and `bar.border` over it. With pixman, `[appearance]
  reduce-transparency`, or a shell on the CPU, there is no blur and the
  shell draws the opaque `#161B2A → #0E111C`. What is behind is drawn into
  the texture moved by the region's corner, so every element draws where
  it is told rather than where it is on the screen: a window wobbling
  while dragged once drew at its screen place there, and the glass showed
  nothing of it under the bar, and some of it before it got there (#517).
- **The renderer** (3.0): the shell is started with `WGPU_POWER_PREF=low`,
  `VK_LOADER_DRIVERS_SELECT` naming the compositor's GPU's Vulkan driver,
  Mesa's EGL vendor, and `vk_wsi_force_bgra8_unorm_first=true`, Mesa's
  option that lists `Bgra8Unorm` first: iced then takes an 8-bit-alpha
  surface without a patch to `iced_wgpu`. When wgpu's adapter is a CPU, or
  the compositor has no GPU, the shell draws with tiny-skia, with the
  opaque look and no motion: the form changes at once, and the compositor
  moves the space kept and the windows at once too. tiny-skia's buffers
  have no alpha on Wayland (softbuffer presents `Xrgb8888`), so the
  compositor cuts that surface to the bar's rounded corners itself; with
  no transparent strip left, no patched softbuffer is needed.
- **At rest** neither the shell nor the compositor draws: no clock yet
  (since 3.2, the clock draws once a second, alone: see below).
- **The background.** Until wallpapers, the compositor draws the mockup's
  night-blue gradient and its glows under everything, so that the glass
  has something to show.
- **Not built yet:** the glass's opaque fallback behind a maximised window
  (§11), the bar's keyboard access (§11), the contents' cross-fade (§3,
  nothing to fade yet), hover feedback on the edge strip (the mockup's
  green tint is the mockup's own aid), and the blur's corners, which are
  cut to whole pixels under the shell's antialiased rim.

### 3.2, the clock (2026-10-28)

What the approved mockup of 3.2 shows (§4 group 1, §4.1, §5); the
mockup's control panel is not part of the product.

- **Its own surface.** iced presents a surface whole (wgpu has no partial
  present, and softbuffer's present damages the whole buffer), so a clock
  drawn in the bar's surface would draw the whole bar each second. The
  clock has a layer surface of its own (`hocus-bar-clock`), over the bar's
  top, inside its rim: 12 px below the bar's top, 194 × 74 px in the full
  form and 54 × 40 px in the mini, the full size while the form switches.
  Pointer events go through it to the bar. Opened after the bar's, it is
  in front of it. On glass it is transparent; without glass it fills
  itself with the bar's opaque colour at its height, and with a shell on
  the CPU the compositor cuts it to the bar, as the two change size in
  their own commits.
- **It wakes itself.** The clock is a canvas that, each time it is drawn,
  asks iced to draw it again just past the next wall-clock second (on the
  monotonic clock), or at the next frame while its digits move. iced then
  draws only that surface: no message goes through the shell, which would
  draw every surface. **At rest** the shell commits the clock's surface
  once a second and nothing else, and the compositor draws only its
  rectangle (194 × 74 or 54 × 40 px), measured headless on an Intel GPU
  and with pixman. Without seconds it still draws once a second, so that
  a clock set or a resume shows within a second: iced wakes a widget only
  by drawing it.
- **Time.** The time is read afresh at each look, in the system's zone,
  read by the shell from `TZ` or `/etc/localtime` (jiff); a thread
  watches `/etc/localtime` with inotify and hands a changed zone over.
  A minute change starts one motion, whatever the jump (a resume, a clock
  set back): never one per minute missed. The date's language follows
  `LC_ALL`, `LC_TIME`, then `LANG`: French or English.
- **The motions** follow the mockup's keyframes, each stretch eased as CSS
  eases it. The blur is drawn as nine copies spread vertically and
  weighed as a Gaussian; the flip's turn as a vertical squash about its
  pivot, without perspective.
- **Fonts.** Martian Mono 1.1.0 (Regular and Light, its standard width)
  and Manrope Regular, SIL Open Font License 1.1, are in the shell's
  binary; their licences are in `crates/hocus-shell/fonts` and the
  package's copyright file. A family named in `[panel.clock] font` is
  looked for among the installed fonts.
- **Sizes** (#378): `HH:MM` and its seconds take 163 px at most, inside
  the padding; the longest English dates ("Wednesday 30 September",
  187 px) reach into the padding, inside the rim.
- **Cross-fade.** While the form switches, the old form's clock fades out
  and the new one's in, over 120 ms from 60 ms (§3), each centred in the
  bar as wide as it is at that frame and cut to it.
- **For recordings and tests**, `HOCUS_CLOCK_AT` (`09:59:57`, or a date
  and time) makes the shell's clock read that time as it starts; the
  system's clock is never touched.
- **Not built:** the clock's keyboard access and name for screen readers
  (§11).

### 3.3, the levels (2026-10-29)

What the approved mockup of 3.3 shows (§4 group 10, §4.2, §5); its
control panel and its dashed "Connexions" and "Pied" slots are not part of
the product.

- **Who owns what.** The shell owns the services: only it shows them, and
  the compositor stays small. The compositor takes the level keys as
  `[shortcuts]` actions (`volume-up`, `volume-down`, `mute`,
  `brightness-up`, `brightness-down`, on `XF86AudioRaiseVolume`,
  `XF86AudioLowerVolume`, `XF86AudioMute`, `XF86MonBrightnessUp`,
  `XF86MonBrightnessDown`), repeats the up and down ones while held at the
  keyboard's delay and rate as it does Alt+Tab's Tab, and hands each step
  to the shell as a `level` line. While the shell is away (restarting),
  it keeps the presses, 16 at most, and hands those under 5 s old to the
  shell when it is back: a key pressed during a restart works, one pressed
  long before does not surprise.
- **Read off the loop, pushed.** Each source runs on a thread of its own
  and writes into one snapshot; nothing is polled where the service
  signals.
  - Battery and profile: the system bus (`DBUS_SYSTEM_BUS_ADDRESS`, else
    the system's), with zbus on a small runtime of its own: UPower's
    display device and power-profiles-daemon's `ActiveProfile`, read
    again on their `PropertiesChanged` and when their owner changes (a
    service that starts later is picked up). Without the bus, it is tried
    again after 2 s, doubling up to a minute.
  - Brightness: the panel's backlight in `/sys/class/backlight` (the
    firmware's, else the platform's, else a raw one), read again on each
    udev `backlight` event; set through logind's `SetBrightness` on
    `/org/freedesktop/login1/session/auto`, without root or a helper.
    `HOCUS_BACKLIGHT_DIR` names another directory, for tests (watched with
    inotify, which sysfs does not feed).
  - Sound: `pw-dump --monitor` (PipeWire's tool) streams the graph's
    changes as JSON; the `default` metadata's `default.audio.sink` names
    the sink, whose node's `Props` give its volume (the cube root of the
    channels' mean, as `wpctl` shows it) and mute. A new default sink is
    followed as a new volume is. The keys run `wpctl set-volume --limit 1.0
    ID 5%+` (or `5%-`, or `set-mute`) on that node; steps pressed while
    one runs go together in one command. The `pipewire` crate would need
    libpipewire's headers and bindgen to build, for the same events. A
    PipeWire not there (no socket, no `pw-dump`) is tried again after 2 s,
    doubling up to a minute: it announces nothing when it starts. With
    PipeWire 1.0's `pw-dump` (Ubuntu 24.04), a default sink changed alone
    shows with the graph's next change; PipeWire 1.6's shows it at once.
  - A service missing hides its row, says so once in the log, and once
    again when it is back.
- **Its own surface, woken without a message.** The levels draw on a
  layer surface of their own (`hocus-bar-levels`), inside the bar's rim,
  41 px above its bottom: 194 × 165 px in the full form (175 px since
  3.4a, and 97 px in the mini, for the sound's item; the group's
  bottom 61 px above the bar's, above the footer's place; room for the
  status line on three lines, a part a line), 54 × 84 px in the mini (40
  px above, above the next item's place: 12 px under the battery, as the
  3.3 mockup has it), the full size while the form switches. iced draws
  every surface after any message of the shell's, so a change of level is
  not a message: the shell asks the compositor (`wake hocus-bar-levels`),
  which sends that surface a configure as it is, and iced draws it alone. A level key reaches its
  service straight from the link, without a message either. **At rest**
  the levels draw nothing; a change draws their rectangle only, measured
  headless with the real shell. A shell alone, without a compositor, draws
  every surface on a change.
- **Fonts**: Manrope Bold (wght 700, an instance of the variable font of
  the same version as the Regular, SIL OFL 1.1) for the mini form's
  battery.
- **Not built**: the levels' popovers and clicks (§9, slices 3.4 and
  3.7, since built); their keyboard access and names for screen readers
  (§11); collapsing to the
  battery alone on short screens (§4); hiding the brightness when the
  laptop's panel is off (lid closed, an external screen alone) while its
  backlight device is still there.

### 3.4a, the popovers and the sound's (2026-10-29)

What the approved mockup of 3.4 shows for 3.4a (§9; issue #361): the
popovers' mechanics, the sound's popover's header, master volume and
outputs. Its "Micro" and "Applications" sections are 3.4b's (#380); its
control panel is not part of the product.

- **A layer surface, not an xdg_popup.** A popover is a layer surface of
  the shell's (`hocus-bar-popover`), anchored right and bottom, its
  margins putting it 8 px left of the bar and its bottom where §9 says.
  iced_layershell's popups take their grab serial only in the dispatch of
  a press and cannot be moved by an offset; a hover must not grab; and
  the compositor's popup grabs serve windows' menus. The layer surface
  takes the keyboard when a click opened it (`exclusive`), and when
  clicked when a hover did (`on-demand`).
- **The compositor draws around it.** It blurs what is behind it in a
  pane of its own, as behind the bar, cut to its corners; draws its shadow;
  cuts it to its corners for a shell on the CPU; and moves it in and out
  (§9), from its first frame and from the shell's `closing` line, so that
  the shell draws it once for that motion. The shell removes it 120 ms
  after `closing` (at once without motion).
- **Clicks outside.** The compositor tells the shell about a press on
  none of the shell's surfaces while a popover shows, or while a surface
  of the shell's holds the keyboard (`outside`). A popover holding the
  keyboard is a menu: that press goes no further, as a press outside a
  window's menu does not. One opened by hovering does not hold it: the
  press goes on to what is under it. The shell sees the presses on its own
  surfaces itself (the bar's surface reports its presses and where the
  pointer goes, for the cone, and never draws for it), which keeps a
  second click on the item a toggle.
- **Nothing through iced's messages.** iced draws every surface after any
  message: the surfaces report what the pointer and the keys do to one
  controller, which decides (a state machine, tested on its own clock), and
  its effects are layer-shell actions (open, remove, keyboard), which iced
  runs without drawing anything else, and `wake` lines. The levels'
  surface takes the pointer since 3.4a; it grew to 194 × 175 (full) and
  54 × 97 (mini), upward, for the item's light and its keyboard ring (3 px
  of the accent, 2 px out); in the mini form without a battery, the gauge
  sits 13 px higher than in 3.3, for them to stay on that surface. The bars' heights come from their surfaces as
  they open, not as messages. **Measured** headless with the real shell:
  opening a popover commits no bar surface; open or closed, at rest
  nothing commits but the clock; a hover in the popover damages exactly
  its rectangle.
- **The sound.** `pw-dump --monitor` gives each sink's description
  (`node.description`, else `node.nick`), what it is (`device.form-factor`
  on the node or its device, `device.bus`, an `hdmi` in its name: headphones,
  a screen, else a speaker) and its own volume and mute. The slider sends
  each volume as it moves; the sound's thread runs `wpctl set-volume ID
  0.42` with only the last one asked each time `wpctl` is ready, and shows
  it until PipeWire says it (a second at most); a volume set unmutes, as in
  the mockup. A row runs `wpctl set-default ID`: WirePlumber then makes
  that sink the default, whose own volume the slider shows. A mute or a
  step asked for one output is never carried over to the next.
- **The wheel**: a mouse's notch is one step, a fast spin's several in one
  event each (#487; its pixels after it count for nothing); a touchpad's
  scrolling makes a step each 15 logical px, at any scale (#488).
- **Not built:** the microphone and the volume per application (3.4b, below); the
  other popovers (3.5–3.7, since built); names for screen readers (iced 0.14 has no
  accessibility tree: the Accessibility milestone); the bar's keyboard
  access beyond the item a popover came from (§11); following the pointer
  over the 8 px between the bar and a hovered popover, where no surface of
  the shell's is (the 300 ms cover it).

### 3.4b, the microphone and the applications (2026-10-29)

What the approved mockup of 3.4 shows under the outputs (§9; issue #380).

- **The sound's service** follows the sources and the applications'
  streams in the same `pw-dump --monitor`: a source is a node of class
  `Audio/Source` or `Audio/Source/Virtual` that is not a sink's monitor;
  the default one is the `default` metadata's `default.audio.source`
  (else the first). A stream is a `Stream/Output/Audio` node; it plays
  while its node is `running` and not corked (`pulse.corked`). Streams
  are grouped by application: a sandbox's app id
  (`pipewire.access.portal.app_id`), else `application.name`, else the
  program. An application's volume is its oldest stream's; it is muted
  when all its streams are; a change sets all its streams. `wpctl` sets
  them (`set-volume`, `set-mute` on the node ids, `set-default` for a
  source), the last of each asked while `wpctl` ran taken together, never
  waited on by the loop that draws.
- **Which applications are listed.** On opening: those playing now. A
  browser's paused tab leaves its stream idle and corked for minutes
  (Firefox), or running silence for seconds (Chrome): neither is listed.
  While the popover is open, an application that starts to play joins
  the list (the popover grows), and one leaves it only when its last
  stream goes: a pause does not move the rows under the pointer. At most
  five rows, then "+N autres applications" / "+N more applications"
  (no scrolling).
- **Icons**: `application.icon-name`, else the desktop file named after
  the application's ids or program, whose `Icon` is looked for in the
  `hicolor` theme (where applications install theirs; scalable first,
  then 64, 48, 128, 32… px) and `pixmaps`, in `XDG_DATA_HOME` and
  `XDG_DATA_DIRS`. Read and decoded on the sound's thread, once per
  application. None: the name's first letter on one of six colours,
  chosen by the name. Icons need iced's `image` (PNG only) and `svg`
  features.
- **The popover resizes.** When what it lists changes while it is open,
  the controller gives its surface the new height (and new margins if
  its bottom moves), kept beside its item and inside the bar; nothing
  else is drawn.
- **The keyboard**: `Tab` goes on from the outputs to the microphone's
  mute button and slider, the microphones, then each application's tile
  and slider; the arrows move a slider by 5 % (and go through the
  microphones); `Enter` or `Space` mutes, or chooses.
- **Volume per browser tab** (studied on #380): Firefox gives each
  playing tab a stream named after the tab's title; Chrome's streams per
  tab are all named "Playback". One row per application for now; tabs
  would need a browser extension (a ticket of its own).
- **Not built:** a hover look on the applications' tiles (the pointer
  changes); scrolling past five applications; per-tab rows; input
  streams (applications recording).

### 3.5, the Wi-Fi (2026-10-29)

What the approved mockup of 3.5 shows, variant B (§4.3, §5, §9; issue
#362); its control panel and its dashed Bluetooth, phone and keyboard
slots are not part of the product.

- **Its own surface.** The connections draw on a layer surface of their
  own (`hocus-bar-connections`), inside the bar's rim: 194 × 40 px in the
  full form (the row and room for its ring), 54 × 48 px in the mini (the
  button and its ring), both while the form switches. It sits 14 px above
  the levels' group and follows it: the levels say where their group's top
  is (a row coming, the status line on two lines) and the surface moves by
  its margins, which draws only it. So a network's change draws only that
  surface, woken through the compositor (`wake hocus-bar-connections`)
  and only when the row's state changes (a strength that lights the same
  arcs draws nothing), and a level's change never draws it; the levels and
  the clock never draw for the network. On the CPU the compositor cuts it
  to the bar, as the levels'. Measured headless with the real shell: at
  rest, popover closed or open, nothing commits but the clock; a network's
  change damages only the connections' rectangle.
- **NetworkManager, off the loop.** A thread of its own, with zbus on a
  small runtime, keeps NetworkManager's objects as last said: read whole
  when NetworkManager has an owner on the system bus, then followed by
  every signal under its path (`PropertiesChanged` with the values, and
  the older per-interface one, `AccessPointAdded` and `…Removed`, devices
  and connections added and removed, `StateChanged` with its reason) and
  by its owner's changes: nothing is polled. A burst of signals (a scan's
  strengths) is shown once. What it asks: `RequestScan` as the popover
  opens (NetworkManager refuses one too soon after another: then what it
  has is fresh; "Recherche…" lasts until `LastScan` changes, 15 s at
  most); `WirelessEnabled` from the switch; `ActivateConnection` for a
  saved network; `AddAndActivateConnection` with only the SSID for an
  open one, and with `802-11-wireless-security` (`key-mgmt` `wpa-psk`, or
  `sae` for a WPA3-only network, and `psk`) for a protected one, as nmcli
  does; for a saved network whose password is refused, its connection
  `Update`d with the new one, then activated; `Disconnect` on the Wi-Fi
  device, which keeps it from connecting again by itself. A connection it
  added that fails is deleted: a wrong password is not saved.
  NetworkManager keeps a password in the connection's profile, in
  `/etc/NetworkManager/system-connections/` (readable by root only) with
  `psk-flags=0`.
- **Networks**: access points of the same SSID are one network (the
  strongest's strength, security and path); hidden ones (an empty SSID)
  are left out; an SSID that is not UTF-8 shows with replacement marks.
  Strength to arcs: 70 and up four, 50 three, 30 two, below the dot alone.
  Security from the flags: 802.1X (or Suite B) enterprise; PSK, or SAE
  alone (WPA3); WEP (privacy without WPA): left to the Settings with
  802.1X; anything else open (OWE too). The rows hold their order while the
  pointer is over the list, a row is unfolded or focused, or a connection
  runs (newcomers join at the end); otherwise strongest first.
- **Failures**: an active connection that ends with reason 9 (no secrets)
  or 10 (login failed), or a Wi-Fi device that failed with reason 7 (no
  secrets: NetworkManager found no agent to ask again after the network
  refused the key), is a wrong password: the field says so. Anything else
  is "La connexion a échoué. Réessaie." / "Could not connect. Try again."
  under the field, or "échec" / "failed" (`#F7B98A`) on a row joined
  with a click.
- **Missing**: without NetworkManager on the bus, no row (as a level whose
  service is missing), and it is picked up when it comes. Without a Wi-Fi
  adapter: the row says the cable or "Hors ligne"; the popover, "Aucune
  carte Wi-Fi.", without a switch.
- **The password** comes from the keys as xkbcommon types them in the
  shell with the keymap and the modifiers the compositor sends (the user's
  layouts, Shift, AltGr, dead keys composed): the popover's surface holds
  the keyboard, and the shell's surfaces that receive its keys hand them
  over with their text. It lives in one buffer that never grows, zeroed
  when its row folds, when the connection succeeds, when the popover
  closes and when dropped; its `Debug` says its length; it leaves the shell
  only in NetworkManager's D-Bus call. It is shown as dots unless the eye
  is pressed. The compositor logs no key, not even at trace.
  Ctrl+Backspace clears the field. Paste came after (3.5b).
- **Popover**: its bottom on the bar's whatever its height (the mockup's),
  where the sound's top is on its item; it grows and shrinks as rows come,
  unfold and go. A hovered popover in the mini form whose row unfolds
  takes the keyboard and stays, as if clicked. The list and the field's
  text are drawn in frames cut to their bounds; on the CPU, whose renderer
  does not cut text to a frame, a list's text that does not show whole is
  not drawn. The caret does not blink (a blink would draw at rest).
- **Fonts**: Manrope SemiBold (wght 600, an instance of the same variable
  font, SIL OFL 1.1) for the network's name and the sign-in page's button.
- **For tests**, `HOCUS_OPENER` names the program that opens the sign-in
  page (`xdg-open` otherwise).
- **Not built**: the connecting row's pulse in the bar (the mockup's: an
  animation as long as a connection, against §12); a secret agent
  (passwords NetworkManager asks for later, for connections whose secrets
  are kept by an agent: such a network unfolds its field when refused);
  hidden networks, forgetting a network, enterprise and WEP networks,
  airplane mode and VPN (the Settings, Sprint 6); names for screen readers
  (§11).

### 3.5b, paste into the password field (2026-10-29)

What issue #453 adds to 3.5 (§9); nothing changes on screen but the field
filling, so there is no mockup.

- **The keys**: `Ctrl+V` (with Shift too, as terminals take it) and
  `Shift+Insert`, in the field holding the keyboard; elsewhere in the
  popover they do nothing. `V` is the key the layout types as v, its
  modifiers aside (AZERTY and bépo alike).
- **The clipboard**, not the primary selection. The compositor gives the
  selection to the client holding the keyboard, layer surfaces included,
  so the popover's surface gets it as a window would. The shell reads it
  itself, on the Wayland connection iced draws on (made by the shell and
  handed to iced): a data device on an event queue of its own, whose
  events are read along with iced's and handled only when a paste asks or
  a popover closes, so that nothing wakes for them. iced's own clipboard
  is off: it reads on the loop and waits there as long as the client that
  copied takes to write, forever if that client hangs, and it keeps a
  thread awake for every key and every move of the pointer.
- **Off the loop**: the shell asks for `text/plain;charset=utf-8` (else
  `UTF8_STRING`, else `text/plain`), and a thread of its own reads the
  pipe for 2 s at most and 4 KiB at most; the field takes the text when it
  comes, if it still holds the keyboard and its row is still unfolded
  (folding or closing drops a paste on its way). One paste at a time. Text
  that is not UTF-8, too long, late, or empty once cleaned pastes nothing.
- **A secret as a typed password**: read into a buffer of fixed size,
  cleaned into another, both overwritten with zeros once the field's
  `Secret` has taken it; no `String` holds it. Nothing logs it nor its
  length (the shell logs, at debug, that it was pasted or did not fit).
  What the client that copied keeps, and the kernel's pipe, are theirs.
- **Found on the way**: a click on the field's eye went to the field,
  which holds it (the eye works by its button now; `Tab` then `Enter`
  already did); and cosmic-text traces the text it shapes, a password
  shown with the eye too: the shell turns `cosmic_text::shape` off
  whatever `RUST_LOG` says.
- **For later fields**: `paste::asks` (the keys) and `paste::request` (the
  text, once, off the loop) are what any text field of the shell uses.
- **Measured headless**, against a test client that copies: the password
  pasted with its line break reaches NetworkManager exactly, by `Ctrl+V`
  and by `Shift+Insert`; out of the field, from an empty clipboard, one
  holding only an image, or a client that never writes, nothing is
  pasted, and the field takes keys while the paste waits; the eye shows
  the pasted password and hides it, and the shell's log, every crate at
  trace, never holds it.

### 3.6, Bluetooth (2026-10-29)

What the approved mockup of 3.6 shows, variant A: new devices paired from
the popover (§4.3, §5, §9; issue #363); its control panel and its dashed
phone and keyboard slots are not part of the product.

- **The connections' surface holds both rows.** It is as tall as the rows
  it draws (the Wi-Fi's on top, Bluetooth's under it, 2 px apart; one
  button above the other in the mini form): 194 × 72 px with both, 194 ×
  40 with one, 54 × 88 and 54 × 48 in the mini form. A service that comes
  or goes resizes it (layer-shell actions, nothing else drawn). With no row
  at all it keeps one row's size and takes no pointer (an empty input
  region), so that a click there reaches the bar's surfaces under it; it
  takes the pointer again when a row comes (#455's leftover, for the
  Wi-Fi too). A Bluetooth change draws only that surface, woken through
  the compositor when the row's words change; the levels and the clock
  never draw for it, nor it for a level. Measured headless with the real
  shell: at rest, popover closed or open (the search running), nothing
  commits but the clock; a battery's change damages only the connections'
  rectangle.
- **BlueZ, off the loop.** A thread of its own, with zbus on a small
  runtime, keeps BlueZ's objects as last said: read whole with
  `GetManagedObjects` when BlueZ has an owner on the system bus, then
  followed by `InterfacesAdded` and `…Removed` (from its object manager at
  `/`, for paths under `/org/bluez`), `PropertiesChanged` under
  `/org/bluez`, and its owner's changes: nothing is polled. The first
  adapter by path is used. What it publishes holds the order the devices
  are listed in, not their strength: a strength that moves no row draws
  nothing. Calls that wait on a device (`Connect`, `Pair`) run aside and
  say how they ended; nothing on the loop that draws waits on D-Bus.
- **The order**: connected devices, the one seen connecting last first
  (those connected as the shell starts, by path); paired ones by name;
  those around by `RSSI`, the strongest first, those not heard in this
  search last.
- **Names**: `Alias` (what the user may have renamed), shown for a device
  that said its `Name`, or is paired or connected. A device around
  without `Name` (BlueZ's alias is then its address) is not listed.
- **The search**: `StartDiscovery` as the popover opens, if the adapter is
  on (and again if it is turned on while open), `StopDiscovery` as it
  closes and as the shell ends (BlueZ also ends a client's search when its
  connection goes). "Recherche…" and its spinner show for its first 10 s
  only: the search goes on while the popover is open, and what it finds
  still comes, but a spinner turning all along would draw at every frame.
- **Connecting**: `Device1.Connect`, given up after 20 s (BlueZ pages a
  device that is off for about 5 s and fails sooner) with `Disconnect`,
  which cancels it. A refusal or the time out: "Il ne répond pas…" under
  its row. `Disconnect` for a connected one. One at a time: the rows are
  inert while one runs.
- **Pairing**: the shell serves an `org.bluez.Agent1` and registers it
  with `RegisterAgent` as `NoInputNoOutput`; BlueZ uses the agent of the
  client that asked for the pairing, so the shell does not ask to be the
  default agent. `Pair` (30 s at most, then `CancelPairing`), then
  `Trusted` set (so that the device comes back by itself), then
  `Connect`. A device that pairs without a code ("Just Works") does.
  **Telling a code before the click**: BlueZ says nothing of the pairing
  method ahead, so a phone or a computer (its `Icon`, else its class or
  appearance) is taken to ask for one and shows "dans les Réglages",
  inert. Any other that asks the agent for a PIN, a passkey, or a
  confirmation is refused (`org.bluez.Error.Rejected`): its pairing fails,
  and its row turns "dans les Réglages" for the rest of the session,
  without a failure message. The agent authorizes only the device the
  popover pairs.
- **Missing**: no BlueZ on the bus, or BlueZ without an adapter: no row
  (as a level whose service is missing), said once in the log, and picked
  up when it comes. An adapter blocked by rfkill is off (`Powered` false):
  the switch asks, BlueZ refuses, and it stays off.
- **The battery** is `Battery1`'s, which BlueZ gives while a device is
  connected.
- **For tests**, `HOCUS_BLUETOOTH_TIMEOUT` (seconds) sets the time a
  connection and a pairing get, and the search's first pass.
- **Measured headless**, the real shell against a fake BlueZ on the
  fakes' private bus (`tests/levels/bluez.rs`): the row's states in both
  forms, the dot, no row and the pointer let through without an adapter;
  the popover by a click and by a hover; the search started and stopped; a
  Just Works pairing reaching the connected blocks; a refusal and a time
  out said; Disconnect; the switch, and a blocked adapter; a passkey
  refused by the agent; the list scrolling; the keyboard; nothing drawn at
  rest. The Wi-Fi's tests run with both rows.
- **Not built**: pairing devices that ask for a code, forgetting a device,
  renaming one, several adapters (the first is used), audio profiles (the
  Settings, 6.7); scrolling "my devices" (it grows with them); names for
  screen readers (§11).

### 3.7, the brightness and the battery (2026-10-29)

What the approved mockup of 3.7 shows, variant B for the other screens
(§4.2, §5, §9; issue #364); its control panel and its dashed slots are
not part of the product.

- **Two more items on the levels' surface.** The brightness's row and the
  battery's (its row and the status line under it, 4 px past it; the
  status line alone, for the modes, without a battery) open their
  popovers as the sound's does, lit under the pointer and while open,
  ringed while they hold the keyboard. The wheel over the brightness's row
  steps it as its keys do. In the mini form the battery is the 3.7
  mockup's sign over its percentage, a 44 × 46 item 14 px under the
  sound's gauge, its percentage's bottom 51 px above the bar's (the
  layout's place under it); the levels' surface grew to 54 × 128 for it.
  Both popovers sit on the bar's bottom, and are drawn by one canvas each:
  a change draws that surface alone.
- **The system bus, off the loop.** The levels' thread for the system bus
  follows UPower and power-profiles-daemon by their signals under
  `/org/freedesktop/UPower` (one match rule) and their owners' changes:
  the display device on its `PropertiesChanged`; `OnBattery` and the
  battery's device on UPower's own, `DeviceAdded` and `DeviceRemoved`; the
  health again only when the battery's device says its `Capacity` changed;
  the keyboard's level from `BrightnessChanged` (or `…WithSource`); the
  profiles on their `PropertiesChanged`. A burst is shown once. Nothing is
  polled; a service that comes is read, one that goes is hidden.
- **On AC.** UPower's `OnBattery` decides, when it says: on AC, charging
  and full stay as the display device says, anything else (a charge limit
  holding the battery, which some firmware calls discharging or pending
  charge) is plugged in, not charging; on battery, it discharges. The bolt
  shows whenever on AC, in the row, the mini form's sign and the popover.
- **Asking.** The slider, the wheel, the arrows, the keyboard's levels and
  the tiles go to that thread through a channel; what comes while a call
  runs waits for it, then only the last of each is asked (the keys' steps
  taken together): logind's `SetBrightness` (the slider's percentage made
  a raw value, never below 5 % nor 1), UPower's `SetBrightness` on
  `KbdBacklight`, power-profiles-daemon's `ActiveProfile`. A slider shows
  what it asked until it is said (a second at most), a tile its profile
  (3 s: the daemon may refuse). Nothing on the loop that draws waits on
  D-Bus or sysfs.
- **The other screens** are the compositor's: it knows them, so a line of
  `hocus-link` carries them (`screens`, then each name after a tab), sent
  as the shell connects and when they change, rather than the shell
  binding the outputs itself on its Wayland connection (where only a
  connector name and a free description are said). The screen the
  backlight lights is the built-in panel (eDP, LVDS, DSI), else the first
  screen; the others are named by their EDID's model (read already for the
  outputs), else their connector's name (headless and nested outputs have
  no EDID). The line reaches the shell without a message: only an open
  brightness's popover draws (and resizes) for it.
- **Decisions.** Four levels named "Éteint", "Faible", "Moyen", "Fort";
  the popover's line for a charge limit says "Branché, ne charge pas"
  (the bar's status line keeps "branché"); the state line breaks between
  its parts, not inside one; the modes alone without the divider and the
  label the title repeats; `PerformanceDegraded` shown, while performance
  is chosen, as a line under the tiles (its two known reasons named); the
  numbers kept to their sign with non-breaking spaces in French.
- **Measured headless**, the real shell against the fakes (UPower with its
  root, its battery's device and its keyboard's backlight, a settable
  power-profiles-daemon, a slow logind on demand: `tests/power.rs`): both
  popovers by a click, the battery's by a hover in the mini form; the
  slider's calls coalesced against a slow logind, never below 5 %, the
  wheel and the arrows by 5 %; the keyboard's backlight with two, three
  and six levels, and none; another screen's line, gone when unplugged
  while open and back when plugged; the battery's states, their words,
  the bolt on AC (a charge limit too), low and critical colours; the
  health shown and hidden; the profiles switched, a missing one inert with
  its line, the reason performance is held back; the modes alone without a
  battery; the mini sign's bolt and colours; the keyboard; nothing drawn
  at rest, popovers closed or open; a hover drawing the popover's
  rectangle alone, a battery's change the levels and the popover only.
- **Not built**: other screens' brightness (DDC/CI, variant A); hiding the
  brightness when the laptop's panel is off (lid closed, an external
  screen alone); a link to the Settings; names for screen readers (§11).

### 3.8, the keyboard layout (2026-10-29)

What the approved mockup of 3.8 shows, variant A: a click goes to the
next layout (§4.3, §5, §9; issue #365); its control panel, its variant B
(a popover listing the layouts) and its "Super + Espace" button are not
part of the product: there is no default shortcut.

- **Where it draws.** The full form's row is the connections' surface's
  lowest, which grows by one row for it (194 × 104 with the Wi-Fi's and
  Bluetooth's, 194 × 40 alone). The mini form's button is on the levels'
  surface, under the battery, as §5 orders them (the battery's percentage,
  then the layout): the connections' surface sits above the levels, so it
  could not hold it there. With the button the levels' mini surface is
  54 × 168: the button's bottom is 47 px over the bar's, where the
  battery's item's was, and the battery's percentage 14 px over its top,
  so the battery, the sound's gauge and the connections above move up by
  40 px. One layout: the surfaces as they were.
- **The compositor owns XKB.** As the shell connects, it sends the
  keymap's layouts (`layouts`, then a tab, the XKB code, a tab, the name,
  for each) and the active one (`layout N`), and the active one again
  whenever it changes: after every key the compositor handles (a
  group-switch option such as `grp:alt_shift_toggle`), and after a switch
  it made. The shell asks with `next-layout`; the compositor moves the
  seat's keyboard to the next group, round the list, which sends the
  client holding the keyboard its modifiers with the new group (the others
  get them as the keyboard enters them). The row changes when `layout N`
  comes, not before. Nothing is polled: the check after a key reads the
  keyboard's state, already at hand.
- **The names.** XKB's own: the keymap's group names
  (`xkb_keymap_layout_get_name`), which xkeyboard-config writes as the
  registry's descriptions ("French", "English (US)", "French (BEPO)"),
  already compiled into the keymap, so the compositor reads nothing and
  needs no registry library. In French, the shell translates them with
  xkeyboard-config's own gettext catalog, the one every desktop listing
  layouts uses (`xkeyboard-config-2.mo`, or its older name
  `xkeyboard-config.mo`, in `/usr/share/locale/fr/LC_MESSAGES`; shipped
  by Debian's `xkb-data`), read once as the layouts come, on a blocking
  thread of tokio's, never on the loop that draws; its `.mo` format is
  read in a few lines, every offset checked, rather than through a gettext
  library and the C library's locale. The bar's language decides, as for
  its other words (`LC_ALL`, `LC_MESSAGES`, `LANG`). A name the catalog
  lacks, or no catalog, stays in English. So "English (US)" is "Anglais
  (US)", the catalog's, where the mockup wrote "Anglais (États-Unis)".
- **The codes** (§4.3): the layout's XKB name in capitals; when two
  configured layouts share it, the variant's, spelled as the name in the
  bar's language writes it (a word of the name with the variant's letters,
  accents allowed: "BÉPO" in French, "BEPO" in English), else the
  variant's XKB name in capitals. A layout XKB gave no code for (its own
  default) shows its name's first two letters.
- **The keyboard.** Since the row opens no popover, it is reached from
  another item holding the keyboard (`Esc` in its popover): `Tab` and
  `Shift+Tab`, or the arrows up and down, now go through the items of that
  item's surface (the Wi-Fi's, Bluetooth's and the layout's rows in the
  full form; the sound's gauge, the battery and the layout's button in the
  mini), round its ends. On the layout's, `Enter` or `Space` goes to the
  next layout and keeps the keyboard; `Esc` lets it go. A click never takes
  the keyboard from the window that has it: the next layout is typed there
  at once.
- **Drawing.** A layout change wakes the connections' surface where a bar
  is in the full form and the levels' where one is in the mini, nothing
  else. The layouts, which come once as the shell connects, are a message
  (every surface draws once) that sizes the surfaces for the row. Measured
  headless with the real shell: at rest nothing commits; a group switch
  from the keyboard damages only the connections' rectangle (full form)
  or the levels' (mini form).
- **For tests**, `HOCUS_LOCALE_DIR` names where the catalogs are
  (`/usr/share/locale` otherwise).
- **Measured headless**, the real shell against the fakes, the
  compositor's keymap set by `[keyboard]` (`tests/layouts.rs`): two
  layouts, the first's name and code, a click switching the compositor's
  group (a test window holding the keyboard is given it in its modifiers)
  and the row following, round the list; three with two French ones,
  "BÉPO"; English names; one layout, no row, the pointer let through, no
  switch, the mini surface as it was; `grp:alt_shift_toggle` moving the
  row and drawing it alone, nothing at rest; the mini button, its place,
  its click, no popover on a hover, a switch drawing the levels alone; the
  keyboard from the Wi-Fi's row to the layout's, `Enter` and `Space`.
- **Not built**: the letter spacing of the code (0.03 em, a third of a
  pixel: iced's canvas has none); choosing a layout from a list (variant
  B); per-window layouts; names for screen readers (§11).

### The popovers' kit (2026-10-29)

What issue #465 changes under the five popovers, nothing on screen.

- **One kit** (`hocus-shell/src/popover/kit/`) holds what they shared by
  copy: the frame (its padding, its glass and rim, its height from where
  the content ends), the header (the title, the word on its right, the
  switch and its slide), the sections' divider and spaced label, notes and
  wrapped lines, values in tabular figures, sliders and the wheel's
  notches, list rows (lit, what they say on their right with a spinner,
  the room left for their name), lists that scroll (their rows stacked,
  the viewport, the scrollbar, scrolling to the row holding the keyboard,
  text cut to the list on the CPU), the blocks of what is connected and
  their button; text measured, cut with an ellipsis and broken into lines;
  the pointer (coming in and leaving, what it is over and pressed), the
  keys sent to another surface, `Tab`'s order and the arrows round a list,
  and what was asked shown until it is said. Plain functions and small
  types: each popover keeps its own layout, state and drawing order, and
  its own sizes where the mockups differ (the sound's and the battery's
  label lines, the Wi-Fi's rows ending 2 px short of an unscrolled list,
  the brightness's 8 px between its slot, its slider and its value).
- **The wheel's notches** count each, a fast spin's several in one event
  too (#472): in the desktops' tiles, the brightness's slider and the
  lists. iced_layershell hands them multiplied by the surface's scale, so
  the compositor tells each bar's screen's scale with the bar (`bar OUTPUT
  FORM SINCE SCALE`, in 120ths), which all of a bar's surfaces share, and
  the shell divides by it.
- **A touchpad's pixels** are multiplied by the scale too; the kit's
  `Wheel` divides them as it does the notches, so a list scrolls by the
  logical pixels the fingers moved, and the touchpad's notches are 15
  logical px, at any scale (#488). Every list and level counts through it:
  the bar's sound and brightness rows (#487, which counted a fast spin as
  one notch) and the calendar's list among them.
- **Bluetooth's keys** lay the popover out from what it shows now, as the
  Wi-Fi's do since #455, not as last drawn: an arrow in the frame where
  `Enter` cleared a row's failure scrolls with the rows' new heights
  (#462).
- **`Tab` stays in a popover** even when nothing in it can hold the
  keyboard, in each of them as in the Wi-Fi's (the battery's and
  Bluetooth's let it go); and a row the keyboard unfolds in the Wi-Fi's
  list scrolls by what the list, grown with it, needs (#470).
- **Measured** before and after, headless, on the Intel GPU and on the
  CPU, at ×1 and ×2: every popover in its main states (open, hovered, the
  keyboard's ring, muted, a row unfolded, a password refused, a failure, a
  list scrolled), pixel for pixel the same.

### 3.9, the desktops (2026-10-30)

What the approved mockup of 3.9 shows, variant A for the mini form (§4
group 8, §4.4, §5; issue #366); its control panel, its screen and its
dashed slots are not part of the product.

- **Its own surface.** The tiles draw on a layer surface of their own
  (`hocus-bar-desktops`): a desktop's change draws only it, a connection's
  or a level's never does, and the clock goes on alone. It is 194 px wide
  and as tall as its rows and the keyboard's ring's room (5 px around): 194
  × 40 with up to four desktops, 194 × 76 with five to eight, 194 × 112
  with nine; in the mini form 54 × (13 a row of dots, 4 between, and the
  room), both while the form switches. It sits 14 px above the
  connections' group (the levels' when no connection shows), on a whole
  pixel, and follows it as the connections follow the levels: by its
  margins, drawing nothing else. In the mini form the dots sit above the
  connections' buttons, as the tiles do above their rows (the mockup's
  mini form had no buttons: there, right above the sound's gauge). With no
  desktop said yet it takes no pointer. On the CPU the compositor cuts it
  to the bar, as the others.
- **The compositor tells**, for each bar's screen, a `desktops` line: the
  count, the current desktop, and each window on the screen, bottom
  first, minimised ones aside: its desktop, its place and size in
  thousandths of the screen (cut to it; where it comes back to while its
  desktop is out of sight), and whether it is the focused window of the
  current desktop. As the shell connects, then after any frame of that
  screen in which it changed: at most once a frame, and never at rest,
  where nothing is drawn; a thousandth of the screen is a tenth of a pixel
  in a tile, so smaller moves tell nothing. **A drag or a resize** is told
  where the window lands, once the grab ends: following it would draw the
  tiles at every frame of every drag for a move of a pixel or two in them.
  **The strip's slide** changes nothing but the current desktop, told as
  it starts, so that the ring moves with the strip. The shell keeps it
  without a message and wakes the desktops' surface; a count that changes
  the rows resizes it (layer-shell actions).
- **Going there.** A click, `Enter` or `Space` sends `desktop SCREEN N`,
  the wheel `desktop SCREEN next` or `previous` a notch (a touchpad's
  15 px); the compositor switches that screen as its shortcuts do, sliding,
  no further than the last or the first (the dots at the top showing where
  the strip is, as with the keys there). The tiles change when the
  compositor tells, not before.
- **The ring's move**: from the desktop left to the one shown, in the
  strip's 320 ms and curve (windows.md §12); in the mini form the accent
  dot slides the same way. None with reduced motion or on the CPU.
- **The sketch**: each window half a pixel in from its place, so that
  windows side by side on the screen (8 px apart, a tenth of a pixel in a
  tile) show apart, as the mockup's do.
- **The keyboard**: `Tab` and `Shift+Tab` (or the arrows up and down) now
  go through the bar's items from its top to its bottom, round its ends,
  from one surface to the next (3.8 stayed on one surface): the tiles,
  first, are reached from the rows under them (`Shift+Tab` from the Wi-Fi's
  row or the sound's, once it holds the keyboard after `Esc` in its
  popover). On a tile, `Enter` or `Space` goes to its desktop and keeps the
  keyboard; `Esc` lets it go.
- **Names for screen readers** ("Bureau 1, 2 fenêtres" / "Desktop 1,
  2 windows", "aucune fenêtre" / "no windows" for an empty one) are made,
  and logged at debug as the tiles change; iced 0.14 has no accessibility
  tree to give them to (§11).
- **Measured headless**, the real shell against the fakes, the
  compositor's desktops from `[desktops] count` (`tests/bar_desktops.rs`):
  four tiles in one row, an empty desktop's number; a window opened
  sketched where it is, light while focused; dragged with Super, nothing
  drawn on the way and its sketch where it landed; closed, the number
  back; the ring on the first, then on the second after Super+2; a click
  on the third sliding the strip past the second; the wheel a desktop a
  notch, stopping at both ends; six desktops on two rows (194 × 76), the
  sixth clicked; the mini form's dots, the current one in the accent, one
  with a window light, the empty ones faint with a lighter rim, a click;
  `Shift+Tab` from the sound's row to the last tile, `Enter` and `Space`;
  a desktop's change committing the desktops' surface alone; nothing
  committing at rest. On the Intel GPU, the ring's move.
- **Not built**: names of desktops (6.14); a window dragged onto a tile;
  previews of a desktop under the pointer; other screens' desktops in a
  bar (sprint 8); names for screen readers (§11).

### 3.9b, desktops that say what is on them (2026-10-30)

What the approved mockup of 3.9b shows, variant A + B (§4.4, §5; issue
#473); its coloured squares stand for the applications' icons.

- **What the compositor tells.** The `desktops` line gives each window its
  id and its application's id (xdg-shell's `app_id`, X11's class); the
  titles go on a line of their own, `titles`, each window's (its
  application's id when it has none, 200 characters at most), compared and
  sent apart, at most once a frame as the desktops are. A terminal
  retitles at every command: on the `desktops` line, each retitle would
  draw the tiles, which show no title, and send every window's place
  again; on its own line it draws an open list and nothing else (the shell
  keeps it without a message, and wakes the popover only when the rows it
  shows changed). A client may retitle without drawing, its window out of
  sight: a change of title or of application id (xdg-shell's
  `title_changed`, `app_id_changed`; X11's `WM_NAME`, `WM_CLASS`) asks for
  a look 16 ms later, whatever is drawn, which takes the changes of that
  frame together; nothing runs at rest.
- **Icons.** Found from the application's id by the sound's lookup
  (`levels::icons`, 3.4b), by the id, its lower case and its last part
  (`org.gnome.Nautilus`, `nautilus`), on a thread of the desktops' own,
  once per id, kept for the session; ids found together wake the tiles
  once. In a tile, none for an application without an icon (the
  rectangle alone); in the list, its letter on a colour of its own, as the
  sound's applications (the tile is shared, `kit::app_tile`). Each icon on
  whole pixels at ×1.
- **The list** is a popover of the controller's (§9, 3.4a), opened by
  hovering only, in both forms: 240 px wide, its top on the tiles' first
  row (the dots' in the mini form) whichever tile opened it, pushed up to
  stay inside the bar. While it shows, resting on another tile shows that
  desktop's at once, on the same surface (no 150 ms again, no fade). A
  click on a tile closes it and goes there; opening or closing it draws
  nothing of the tiles (a tile is lit by the pointer, not by its list).
- **A row** sends `bring ID`: the compositor slides to the window's
  desktop and focuses it, as the switcher does; the list closes. Clicked
  into, the list takes the keyboard (`on-demand`): `Tab`, `Shift+Tab` and
  the arrows go through the rows, `Enter` or `Space` brings the window,
  `Esc` closes it and gives the keyboard to the tile.
- **Measured headless**, the real shell against the fakes, icons from a
  theme of the test's own (`tests/bar_desktops.rs`): an icon in a 16 ×
  14 px sketch, none in a 5 × 4 px one; resting on a tile, nothing at
  100 ms, then the list 240 px wide beside the bar, its top on the tiles',
  its rows front to back; gone once the pointer leaves; no list after a
  click on a tile; an empty desktop's line and 80 px; a row bringing its
  window from the second desktop, focused; a retitle without drawing,
  told and cut with an ellipsis in the open list, the tiles' surface not
  committing; the mini form's dot; nothing committing at rest while the
  list shows. On the Intel GPU, the screenshots of the pull request.
- **Not built**: a list longer than the bar (a desktop with more than about
  25 windows; the list did not scroll: it does since 3.9c); previews of a
  desktop under the pointer; dragging a window onto a tile or a row.

### 3.9c, the list's long desktops and minimised windows (2026-10-30)

The maintainer's answers to 3.9b's questions (issue #479):

- **Past 8 windows the list scrolls**: 8 rows show (324 px), the others
  under them, as the Wi-Fi's list does (the kit's `List`, its scrollbar
  beside the rows, which then end 2 px short of the content's right): the
  wheel two rows a notch, a touchpad by its pixels, and the row holding
  the keyboard scrolled into view (`kit::reveal`). On the CPU, a title
  shows only whole in the viewport. Another desktop's list starts at its
  top.
- **Minimised windows** are on the `desktops` line too: each window's
  look is `0`, `1` (the focused one of the current desktop) or `2`
  (minimised). The tiles and the dots leave them out, as before; the list
  shows them after the others, front to back, their title in
  `text.faint` and their icon at 50 %, and counts them in "M fenêtres". A
  click (or `Enter`, `Space`) sends `bring ID`, which already slid to the
  window's desktop, unminimised it and focused it.
- **`Enter` on a tile** holding the keyboard goes to its desktop, as in
  3.9; it opens no list.
- **An application without an icon** shows its letter on a colour in the
  list, as built in 3.9b (`kit::app_tile`).
- **Measured headless** (`tests/bar_desktops.rs`): ten windows, 8 rows
  shown; a notch scrolls to the end (the scrollbar's thumb at the bottom);
  clicked into, `Tab` scrolls back to the first row and `Up`, round to
  the last, to it; `Enter` brings that window. A minimised window listed
  last and counted, its icon no longer at full colour and its title
  fainter; an application without an icon, its letter tile; a click
  bringing the minimised window back, focused. On the Intel GPU, the
  screenshots of the pull request.

### 4.0, the agents' sources (2026-10-30)

What the approved 4.0 mockup shows, variant B, and the maintainer's
clarification of "Ne rien brancher" (§4 group 3, §10; issue #383); its
control panel and its dashed slots are not part of the product. 4.1's
tiles come under the header later: until then, with agents running and
nothing to propose, the group is its header alone.

- **The model is the compositor's**, on its loop: each agent's name,
  state, since when (`CLOCK_MONOTONIC`), sources, processes and window.
  Its sources run off the loop and hand it what they learn through a
  channel:
  - **the control socket**, `<socket>.control` (`HOCUS_SOCKET` for the
    programs the compositor starts), where `hocus agent NAME STATE` and
    the tools' hooks (`hocus agent-hook TOOL`) write one line each
    ([agents.md](../agents.md)); a thread takes the connections and reads
    the agent's process's ancestry in `/proc`;
  - **the tools' watcher** (`hocus-agents::watch`): for each place that
    says a tool is there (`~/.claude`, `$CODEX_HOME` or `~/.codex`, their
    settings, `claude`, `codex` and `herdr` in each of `PATH`'s
    directories, herdr's directory), the nearest directory that exists is
    watched with inotify for that name, so a tool installed later or a
    settings file changed by hand is seen as it happens; nothing is
    polled. Plugging in, unplugging and declining, which write files, run
    there too;
  - **herdr** (`hocus-agents::herdr`, ported from the first build without
    its polling): its socket waited for with inotify, `agent.list`, the
    workspaces' and tabs' labels (`workspace.list`, `tab.list`), each
    pane's `pane.process_info`, then `events.subscribe` on a blocking
    read, the renames with it (`workspace.renamed`, `workspace.updated`,
    `tab.renamed`, `tab.created`, `tab.closed`, `tab.moved`).
- **One agent for two sources**: a report joins the agent that has its
  key, else one of another source that has its process (a Claude Code in
  a herdr pane is the pane's foreground process); one source's keys stay
  its own agents. The state that changed last wins; herdr's name wins (the
  user names things there, #497).
- **Its name** (#497, #503): herdr's, its tab's own name when the user
  gave it one ("refacto"), else its workspace's label (a tab herdr labels
  with its place has none), a rename in herdr showing at once; else its
  project's folder, the last part of its
  hook's first `cwd` (kept when a later event has none, or a directory
  the agent went down into), or the command's `NAME`; else its process's
  working directory's (`/proc/PID/cwd`). Never its tool's name: herdr's
  agent kind ("claude") had named every agent alike, and won over the
  hook's folder when both saw one.
- **Forgotten** when its session ends (`SessionEnd`, `hocus agent NAME
  ended`), when its process ends (a pidfd on the loop), or when herdr
  goes for its own.
- **Its window**: the topmost window whose client (a Wayland client's
  credentials, an X11 window's `_NET_WM_PID`) is the nearest process
  above the agent: the hook's parent above its shells is the agent, its
  terminal above it. A terminal that serves all its windows from one
  process (foot's server, gnome-terminal) gives its topmost window, not
  necessarily the agent's. An agent herdr sees whose processes reach no
  window (they run under herdr's server) has herdr's terminal's (4.3c).
- **To the shell**, two lines (`hocus-link`): `agent-tools` (each tool,
  its state, the file plugging it in changes) and `agents` (each agent:
  id, state, since, sources, window, name), sent after the turn of the
  loop in which they changed: a burst of reports is one line, a report
  that changes nothing sends none.
- **Plugging in** (after the consent popover's "Brancher" only): Claude
  Code's `~/.claude/settings.json` gets one matcher group of ours per
  event (`SessionStart`, `UserPromptSubmit`, `PreToolUse` of
  `AskUserQuestion|ExitPlanMode`, `PermissionRequest`, `PostToolUse`,
  `PostToolUseFailure`, `Notification` of the prompts, `Stop`,
  `StopFailure`, `SessionEnd`), running `<hocus> agent-hook claude-code`
  with a 5 s timeout, the user's keys kept in their order; Codex's
  `config.toml` gets inline `[[hooks.EVENT]]` tables (`SessionStart`,
  `UserPromptSubmit`, `PermissionRequest`, `PostToolUse`, `Stop`,
  `Interrupt`, `SessionEnd`) and `notify` when it has none, edited with
  `toml_edit` so the user's comments and layout stay. The file as it was
  goes beside it (`….before-hocuspanda`), and what was written is
  remembered (its hash) in `$XDG_STATE_HOME/hocuspanda/agents.toml`. A
  link is written through; a file that does not read is never written.
  **Unplugging** puts the copy back when the file is still as written
  (removes it when there was none), else takes only our entries out.
- **"Ne rien brancher"** declines every tool there and not plugged in,
  herdr's following too (its agents are then not followed: Sources says
  "En marche · pas suivi" / "Running · not followed", with "Suivre" /
  "Follow", in "Brancher"'s style, to follow it again), remembered in the
  same file; the group goes
  (variant B). A tool found later that was never declined brings the
  card, and the group, back for itself. Plugging a declined tool in from
  Sources clears its refusal.
- **The group's surface** (`hocus-bar-agents`), under the clock: its top
  100 px under the bar's (14 px of padding, the clock's 72 px, 14 px;
  since #599 the column's last, under the to-do list: see "The column's
  order" below),
  194 px wide and as tall as what it shows plus the keyboard ring's room;
  one pixel high, taking no pointer, when it shows nothing or in the mini
  form. Its text is measured as it draws, and a change of height resizes
  it (layer-shell actions): a change draws only it. The card: "Pour
  suivre tes agents, branche ton outil :" (13 px on 1.4 lines), a row per
  tool found (its 18 px mark, its name 14 px, "Brancher" 24 px), "Un
  autre outil ?" and "Ne rien brancher" (12 px). Manrope's widths do not
  let "Claude Code" and "Brancher" sit side by side in the card's 152 px
  (nor the two links): as a browser lays the mockup out, the name goes on
  two lines (its row 38 px) and the links one under the other. Plugged in
  with nothing running: "Aucun agent en cours." and "Suivis : …".
- **The popovers** (§9's mechanics, the kit): 300 px wide as the
  mockup's, their top 62 px under the bar's (the mockup's), whichever item
  opened them. Sources: each tool's mark (at 40 % when not there), name,
  state ("Branché" green, "Trouvé, pas branché", "Pas sur cet
  ordinateur"; herdr "En marche · suivi tout seul", "En marche · pas
  suivi", "Ne tourne pas") and "Brancher" or "Débrancher"; the divider,
  "UN AUTRE OUTIL", the command in Martian Mono with "Copier" (the
  compositor offers the text as the clipboard's selection, "Copié"
  after), the states. Its "Brancher" gives way to the consent popover.
  Consent: the mark and "Brancher <outil>", what it lets the desktop know
  (for Codex, that it asks to approve the hooks with `/hooks`), "Fichier
  modifié" and the file in Martian Mono, the copy kept, "Annuler" and
  "Brancher". `Tab` goes through their buttons, `Enter` or `Space` acts,
  `Esc` closes.
- **Measured headless**, the real shell away from the machine's services,
  in a home of the test's own (`tests/agents.rs`): the card with two
  tools and Sources' states; "Brancher" writing nothing before the
  consent's click, then the hooks and the copy; "Débrancher" putting the
  file back byte for byte; "Ne rien brancher" hiding the group, the
  surface one pixel high, remembered in a new session; the same with
  herdr running (a fake herdr), its agent no longer followed; Codex
  installed later bringing the card back for it alone; no tool, no group,
  the pointer let through; `hocus agent` reaching the model and the shell
  (the header alone), `ended` forgetting it; Claude Code's documented
  payloads through `hocus agent-hook`, and Codex's `notify`; the agent's
  window found above its process; nothing committed at rest, nor for a
  report that changes nothing. On the Intel GPU, the screenshots of the
  pull request.
- **Not built**: 4.1's tiles and pandas, 4.2–4.7 (lines, going to an
  agent, the halo); the mini form's agents; Settings → Agents (6.16,
  #450), where a declined tool is turned back on; `CLAUDE_CONFIG_DIR`
  (Claude Code's other settings directory); writing Codex's hooks into a
  `hooks.json` the user already has (Codex warns when a layer has both);
  telling the agent apart from its terminal's other windows when one
  process serves them all; names for screen readers (§11).

### 4.1, the agents' rows (2026-10-30)

What the approved mockup of track 4, "the lines", shows, with the
maintainer's spec (§4 group 3, §7 A; issue #384, which covers the former
4.2, who waits and who is stuck). The other tracks (the tribe, the stage,
the heads) are display options for later (#491).

- **One row per agent**, under the header as 4.0 built it, which now says
  how many run ("AGENTS 7", the total in `text.faint`, Manrope 600, 4 px
  after the spaced word). A row is 34 px high and as wide as the column
  (6 px past the group's sides, 184 px), radius 7, 6 px in: the agent's
  panda (30 px), 7 px, its name (Manrope 600, 13.5 px, `text`, cut with an
  ellipsis to what the word on its right leaves), 7 px, that word
  (12.5 px). These are the real sizes (#497): the mockups were shown
  zoomed ×1.5, and their 28 px rows looked small at ×1; the bar's width
  does not change:
  - working: how long it has worked, `text.faint` ("12 min"; "1 h 05"
    from an hour);
  - waiting: "t'attend" / "waiting", `#F5C46E`, the row on
    `rgba(245,180,74,0.12)`;
  - stuck (`failed`): "bloqué" / "stuck", `#F7A77A`, on
    `rgba(242,148,91,0.10)`;
  - finished, not seen yet: "à voir" / "to see", `#7EE0A0`, on
    `rgba(108,194,135,0.10)`;
  - resting (idle, or finished and seen): "repos" / "resting", `#6A7384`,
    its panda asleep at 55 %.
  Under the pointer a row is lit `rgba(255,255,255,0.06)` and the pointer
  is a hand, as the bar's other rows (the mockup's tints, inline, hide its
  own `:hover`). The name is herdr's for an agent herdr sees (its tab's
  own name, else its workspace's label: #503), else the project's
  folder (a hook's `cwd`) or the command's `NAME`, else the agent's
  process's directory (4.0 as built, "Its name").
- **Order**: waiting (the longest first), stuck, to see, working,
  resting; in the order they came otherwise. **At most five rows**; the
  rest, the lowest first, make one line 2 px under them (12.5 px,
  `text.faint`): "+ N au repos" / "+ N resting" when they all rest, else
  "+ N autres" / "+ N more". The group is 26 px (the header and the 8 px
  under it), 34 px a row and 20 px for that line; its surface follows its
  height, never its width.
- **The pandas** are the validated drawings, one still mood each, as SVG
  (`crates/hocus-shell/assets/pandas/`, with a note of their origin),
  drawn by iced's `svg` (resvg) at the screen's scale: sharp at ×1 and ×2.
- **Motion**, only when something changes: the rows slide to their new
  places in 450 ms, `cubic-bezier(0.2, 0.9, 0.25, 1)`; a row going under
  the "+ N" line slides there and fades out, one coming from it fades in
  (300 ms); a row's tint changes over 300 ms; the panda of an agent whose
  state changed, or that came, hops once (650 ms). Then nothing draws.
  With reduced motion, or a shell on the CPU, the rows change at once.
- **"Finished, not seen yet"** is the compositor's: a finished agent is
  told to the shell as `done` until the user has seen it, then `seen`
  (`hocus-link`), until it finishes again. Seen means its window got the
  keyboard after it finished, or had it as it finished (as herdr's green
  dot: the user is looking), or its row was clicked. A report of `done`
  again (Claude Code's `idle_prompt`, 60 s later) changes nothing. herdr's
  agents are also seen as herdr says (it turns `done` into `idle` once the
  pane is looked at); their window is herdr's terminal, shared by every
  pane, whose getting the keyboard sees only the pane herdr shows (4.3c).
- **"12 min"** goes on with the clock: at each of its minute changes, the
  shell lays the rows out again if a working agent shows and what they say
  changed, and wakes the group's surface (once for all the bars). Nothing
  draws the rows in between: no timer of their own, none per agent.
- **Going to an agent** (§8, the window and its keyboard, without the
  streak and the pulse): a click on a row, or `Enter` on it with the bar's
  keyboard (`Tab` and the arrows reach the rows from "Sources", as 3.8 and
  3.9 go through the bar's items), asks the compositor for the agent
  (`go-to-agent ID`): its window, when it has one, takes the keyboard, its
  desktop shown first if it is another (the desktops' list's `bring`); a
  finished agent is then seen.
- **Measured headless**, the real shell (`tests/agents.rs`): the five
  states in their order with their words, 34 px apart, their tints read
  from the screen; more than five agents folding into "+ 2 au repos", the
  group 226 px high (its surface, with the ring's 10 px), then "+ 2 autres" once a hidden one works; a finished
  agent staying "à voir" while its window is on another desktop, seen once
  its window has the keyboard, and seen at once when it finishes with it;
  a row's click switching to the window's desktop and giving it the
  keyboard; "0 min" becoming "1 min" at the clock's minute with the
  group's surface drawn once while the clock drew every second; on a GPU,
  a change moving for 0.5 to 0.9 s then resting, and not moving with
  reduced motion; nothing committed at rest. On the Intel GPU, the
  screenshots of the pull request.
- **Not built**: the streak and the rim's pulse (4.3), the halo (4.4), the
  peek (4.5), the secretary (4.6); the mini form's agents (4.7); herdr
  selecting the agent's pane (§8); names for screen readers (§11).

### 4.3, going to an agent (2026-10-30)

What the approved 4.3 mockup shows, variant A, with the maintainer's spec
(§8; issue #386).

- **The line**: the shell's `go-to-agent ID` (4.1) carries the row:
  `go-to-agent ID OUTPUT X Y W H`, its rectangle in the bar's frame (from
  the bar's right edge, its top). The shell knows where the row is in its
  bar, which moves as the rows slide; the compositor knows where the bar
  is on its screen. Without a row (a line of 4.1's form), the window is
  gone to without a streak, its rim pulsing.
- **The streak** is a GLES pixel shader of the compositor's
  (`going.rs`), drawn each frame from closed-form curves on the frame
  clock: its head at `cubic-bezier(0.3, 0.7, 0.2, 1)` of 300 ms toward the
  window's centre as drawn, then its alpha down along `ease-in` over
  220 ms. The shader draws what the mockup's CSS does, as Chrome renders
  it: the 4 px box with round ends, its `linear-gradient` from transparent
  at the row to full at the head, and its `box-shadow: 0 0 12px` (the box
  blurred, a Gaussian of deviation 6 written with the error function)
  outside the box only, but no brighter than the line beside it (4.3b,
  issue #495): CSS's shadow ignores the gradient, and near the row it
  ringed a line still transparent with its glow, a dark core. Where the
  line's alpha is below the glow's at its edge (a quarter, over the
  line's first quarter), the glow is scaled down to it, so the streak
  starts from nothing at its row; further on, and at its head, the glow
  is the shadow's. It is drawn over the windows and under the bar's
  surfaces, over the bar's glass rather than under it: under it, the
  glass would be blurred again at each of its frames, for a line that is
  transparent where it starts, on its row. Where the strip slides first,
  the streak starts once it has (320 ms). A new element each frame: the
  damage is where it was and where it is. With two screens, each draws its
  part.
- **The pulse**: the mockup's keyframes, each step easing out: in 150 ms
  the 2 px rim at 95 % and a ring of 4 px at 55 % (the accent), then the
  ring grows to 14 px as both fade, by 600 ms. Both are the window's
  rims, outside its content as its other rims are, in whole pixels (a
  texture per width, not per frame). Its start is set with the go-to, at
  the streak's landing: nothing is queued. It is the window's one pulse
  (4.3b, issue #495): a window that opens without the keyboard, or asks
  for it without a token, pulses the same way at once (windows.md §4).
- **A second click** on another row while a go-to runs replaces it at
  once: its streak and its pulse (the maintainer's answer on #495).
- **The keyboard goes to the window at the click**, as 4.1 built it, not
  as the streak lands: keys typed during the 300 to 620 ms of the motion
  would otherwise reach the window that had the keyboard, another agent's
  prompt maybe. Only the pulse waits for the landing.
- **Reduced motion, or a compositor on the CPU** (Pixman: no shaders): no
  slide (the desktops cross-fade as with reduced motion) and no streak;
  the 2 px rim lights and fades out over the same 600 ms, without its
  ring.
- **herdr**: an agent of herdr's (its own, or a hook's agent in one of its
  panes) has its pane selected, `pane.focus` with its `pane_id` (the API
  schema herdr bundles, `herdr api schema`), on a thread, at the click.
- **The colour** is the state the shell is told, before the click marks a
  finished agent seen: "to see" stays green for the streak that goes to
  it.
- **Measured headless** (`tests/agents.rs`): on a GPU, a row's click on
  the same desktop runs the streak from the row's left edge to the
  window's centre in the working accent for 420 to 650 ms, the rim
  lighting 250 ms or more after it starts with the ring growing past
  8 px, the keyboard on the window, nothing moving after 1.1 s and only
  the clock's seconds drawn after; on another desktop, the strip slides
  and the streak runs only once it is still; the streak's colour read from
  the screen, amber to a waiting agent and the accent to a working one;
  with reduced motion on a GPU, and on Pixman, no slide, no streak, the
  rim lit without its ring; herdr (a fake one) asked `pane.focus` for its
  agent's pane. On the Intel GPU, the screenshots of the pull request.
  4.3b: on a GPU, the screen against what it showed before the click: the
  streak's first pixels next to its row, and behind it, change nothing
  (3 levels at most), and along its start the line is no darker than its
  glow beside it; a second row clicked while a streak runs leaves only
  its own streak, from its first frame. A window opened in the background
  (`tests/windows.rs`, another program's window having the keyboard)
  stays behind without the keyboard and pulses once, its ring growing
  past 8 px on a GPU, without its ring with reduced motion and on Pixman,
  over within 700 ms, and then nothing is drawn.
- **Not built**: going to an agent from anything else that names it (the
  peek, the secretary's briefing: 4.5, 4.6); the halo ending (4.4).

### 4.3c, herdr's terminal (2026-10-30)

What the maintainer found on the laptop (issue #500): with herdr's
terminal not in front, a herdr agent's row did not bring it.

- **The cause**: herdr's server is a daemon (`herdr server`, its parent
  init), its agents run under it and under no terminal; the terminal shows
  herdr through a client (`herdr` under the terminal's shell). The
  compositor looked for herdr's clients in `/proc` only as herdr's agents
  were listed, when it connected to herdr and when panes came or went.
  herdr's server had run for days; the compositor started, listed its
  agents, and only then the terminal opened and herdr's client attached
  in it: no client was ever found, the agent had no window, and its row
  only selected its pane. A Claude Code agent in a herdr pane looked for
  its window above its own processes only, which reach herdr's server and
  no terminal. Reproduced headless with that tree in a `/proc` of the
  test's own, the client written after the listing: the keyboard never
  reached the terminal.
- **Its window** is the terminal's that runs herdr's client: for each of
  herdr's clients (a process named `herdr` that is not its server, known
  by its command line's `server`), the nearest process above it that has
  windows. herdr's clients are looked for again, on a thread (it reads
  every process), as a row is clicked, the go-to going on once they are
  found; and as the keyboard goes to another window while herdr has
  agents. An agent herdr sees whose own processes reach no window (a
  hook's or a command's agent in one of its panes) has the same window.
- **A terminal process with several windows** (kitty's): the one whose
  title's first word is `herdr`, as the shell titles a terminal with the
  command it runs ("herdr", "herdr session attach work"; not "vim
  herdr.rs"); else the most recently focused of them; never focused, the
  topmost. herdr's API does not say its client's window title: its
  `client_window_title` is only the answer to `client.window_title.set`
  and `.clear`, which change it, and its panes' `terminal_title` is what
  each pane's program sets (Claude Code's "✳ Claude Code", in a terminal
  of its own as well), not the window's.
- **Seen**: herdr's terminal getting the keyboard shows only the pane
  herdr shows: an agent's pane focused in its tab, its tab and its
  workspace the focused ones (`agent.list`, `tab.list`,
  `workspace.list`), then `pane.focused` events; another tab or
  workspace focused lists the agents again. The halo counts it the same
  way.
- **The journal** says each go-to at the `info` level: the agent's id and
  sources, and its window and how it was found ("above its process",
  "herdr's terminal, its window titled herdr", "… its only window", "…
  its most recently focused window"), or why there is none ("no herdr
  client found", "no window above herdr's clients"); never the agent's
  name or a window's title.
- **Tests**: herdr's clients are read in a `/proc` given
  (`Places::processes`, `/proc` in a session), so the tests never see the
  machine's own herdr (#494). Headless (`tests/agents.rs`), the
  maintainer's tree: the terminal on another desktop, its client attached
  after the listing, the row's click brings it with the keyboard and herdr
  focuses the pane; `hocus agent --pid` of the pane's process has that
  window too. Two windows of one terminal process side by side, the other
  focused last: herdr's window getting the keyboard sees the agent of the
  pane herdr shows, not the other's; the other's row then goes to the
  window titled herdr.

### 4.4, the halo (2026-10-30)

What the approved 4.4 mockup shows, variant B (a soft green glow for a
finished agent not seen yet), with the maintainer's spec (§6; issue
#387). The meetings' states came with the next meeting's card (Day.2).

- **The compositor draws it**, around and over the bar, as it draws the
  bar's glass under it: the outer glow is outside the shell's surfaces,
  which are the bar itself, and a halo drawn by the shell would redraw its
  bar's surface at every frame of a wave or a fade. Here the shell draws
  nothing for it.
- **Two GLES pixel shaders' elements** (`halo.rs`), one program: what the
  mockup's CSS draws, as Chrome renders it (its pixels beside the bar
  match a headless Chrome's to two levels). Outside the bar, its
  `box-shadow: 0 0 34px` (the bar's rounded box blurred by a Gaussian of
  deviation 17, written with the error function, exact along the sides and
  summed over 16 rows near the corners), drawn outside the box only; the
  1 px border in the colour; inside it, the `inset` shadow (what is outside
  the padding box, blurred, inside it); over it, the wave's band, cut to
  the inside of the border. The glow's element is kept while it does not
  change (same id, same commit): at rest it leaves no damage, and when the
  clock draws its second, only the clock's region is drawn again, the
  halo with it. The wave is a new element each frame while it runs, as
  tall as its band: its damage is where the band was and where it is.
- **Where**: over every bar's surfaces of the screens that show one, under
  the popovers beside the bar (the layer surfaces before the bar's), over
  the glass and outside what the glass blurs: the glass is not blurred
  again for it. A fullscreen window on top covers it as it covers the bar.
  During a form switch it follows the bar's width at each frame.
- **Over the bar's content**: the mockup draws its wave and inner glow
  under the bar's text; the compositor draws them over the shell's
  surfaces, which hold the text and the background together. Where the
  band passes over text (white), the text takes some of its colour for
  the 600 ms of the wave; the inner glow, strongest at the border, is
  down to 3 % at the text 12 px in.
- **The states** come from the agents' model as the shell is told them
  (4.0, 4.1): waiting, stuck (`failed`), finished not seen yet (`done`).
  Each agent in one of them is an episode, from the start of its state.
  The halo shows the most important episode the user has not acted on,
  and the meetings' slots (`State::meeting_near`: in 10 min or less, in
  2 min or less), as the shell says them (`meeting ten|two|none`, Day.2).
- **It ends when the user acts**: the agent answered (it works again),
  going to it from its row (`go-to-agent`, 4.3), or its own window getting
  the keyboard (as a finished agent is seen, 4.1; herdr's terminal counts
  only for the pane herdr shows, 4.3c); a finished one seen. An
  agent that starts waiting while its window has the keyboard (the user is
  looking at it) shows no halo, as a finished one is seen at once. Going
  to a waiting agent ends its halo while it still waits (§8 step 5).
- **The waves**: an agent that starts waiting runs one (each agent that
  starts waiting, as the mockup's "Un agent t'attend" does), its reminder
  runs one, and a meeting that comes nearer runs its own; a wave runs only
  when its state is the one shown (no agent's wave under a meeting in
  2 min).
- **The reminder** of an agent still waiting at its fifth minute is a
  timer of the loop, set for the earliest due and moved when that changes:
  nothing polls. Once per waiting: answered and waiting again, it is a new
  one.
- **Fades** from what shows at that moment to the new state: in over
  600 ms `ease-out`, out over 500 ms `ease-in`; from one state to another,
  the colour and the glows go from one to the other over 600 ms. As CSS's
  transition from `0 0 0 transparent` does, the blurs grow with the alpha.
- **Reduced motion**: no wave, the glow in and out over 200 ms (the
  mockup's `.reduced` class sets both). **On the CPU** (Pixman, no shaders)
  there is no halo: the rows' tints still show the states.
- **Frames**: only during a wave or a fade (the frame clock's closed-form
  curves; nothing is queued per frame); none at rest.
- **Measured headless**, the real shell (`tests/agents.rs`): on a GPU, an
  agent waiting runs one amber wave down the bar for 450 to 650 ms as the
  glow fades in, the glow then full and amber read beside the bar, and
  only the clock's seconds drawn after; backdated to 4 min 59 s, its
  reminder's wave a second later, then no other an hour on; a stuck agent
  orange without a wave; a finished one not seen soft green without a
  wave; answering, and going to it from its row (still waiting), fading it
  out in 350 to 650 ms; with reduced motion, no wave and the glow full
  within 260 ms. On Pixman, the priorities: to see, then stuck, then
  waiting, and back as each is dealt with, nothing once the finished one
  is seen from its row, nothing drawn. Unit tests for the priorities, the
  looks, the waves, the reminder, the fades. On the Intel GPU, the
  screenshots of the pull request.
- **Not built**: the in the halo's colour (4.6); the peek (4.5); the halo on
  the CPU; the mini form's agents (4.7).

### 4.5, the panda peeking out (2026-10-30)

What the approved 4.5 mockup shows, with the maintainer's spec (§7 C;
issue #388): it stays out 8 s, and also comes out when an agent finishes.

- **What brings it out** is the halo's model (4.4): an agent that starts
  waiting, or finishes, unless the user is looking at it (its own window
  has the keyboard: no halo, no peek), and a waiting agent's reminder at
  its fifth minute, the halo's timer of the loop: nothing polls. A stuck
  agent does not. Of several in one turn of the loop, the latest.
- **The line**: the compositor tells the shell `peek ID waiting|done SINCE
  NAME`, after the `agents` line, and `no-peek` once the panda is back in.
- **The shell draws it once**, on a layer surface of its own
  (`hocus-bar-peek`) beside each bar, anchored to the screen's top right:
  the panda's box 18 px from the bar, its middle on the agent's row's
  middle as the group lays the rows out, where the bar's column puts the
  group (#599: it follows the group as it moves; the secretary's middle when the
  agent has no row shown: under "+ N"; in the mini form, which has no
  rows, her head's in the mini item since 4.7). On a GPU the surface is transparent and holds the shadows:
  the bubble's `box-shadow`, a blur outside its box only (as CSS draws
  it), and the panda's `drop-shadow` round its drawing, are SVG filters
  that resvg draws as it draws the pandas; their room is 45 px left of the
  bubble, 14 px above, 24 px right of the panda, 30 px below. The surface
  takes the pointer on the bubble and on the panda's box only (its input
  region), the pointer a hand there; the bubble is lit under it. A newer
  peek resizes and moves the surfaces of the one out: there is never a
  second one.
- **The compositor moves it**, as it does the popovers: from the surface's
  first frame, closed-form curves on the frame clock. It slides out from
  150 px under the bar, cut at the bar's edge as drawn (it is drawn under
  the bar's surfaces and its halo, over the windows and the streak), and
  back the same way. Two timers of the loop: 8 s after its first frame it
  goes back; 260 ms later the shell is told `no-peek` and removes its
  surfaces. Frames only while it moves: none while it is out and still,
  none at rest.
- **It goes back at once** when the user goes to its agent: a click on it
  (panda or bubble) asks `go-to-agent ID OUTPUT X Y W H` with the panda's
  box, so the streak starts at the panda's left edge; its row or the
  briefing's line does too (§8 step 5). Clicked on its way out, it goes
  back from where it is. It also goes back at once when its agent is dealt
  with otherwise (answered, working again; a finished one seen; its window
  getting the keyboard), as the halo ends: the bubble would say something
  no longer true; and when the secretary's briefing opens (4.7b).
- **The latest replaces the one out**: while that one is out and still,
  the new one takes its place there (its words, its row) for 8 s more;
  while it goes back, the new one comes out afresh.
- **Reduced motion, or a shell on the CPU**: no slide; it fades in over
  260 ms (`ease-out`) and out over 220 ms (`ease-in`) in place. On the CPU
  the shell's buffers have no alpha: the bubble's card holds the panda
  too (96 px high, the bubble's colour, made opaque, and its border round
  both, 8 px after the panda), without shadows, and the compositor cuts
  its rounded corners.
- **A long name** is cut with an ellipsis at 280 px, so that the bubble
  stays beside the bar.
- **Measured headless**, the real shell (`tests/agents.rs`): an agent
  starting to wait brings its panda out at its row, 18 px from the bar,
  saying "build t'attend · à l'instant"; on a GPU it slides out from
  under the bar, past its place a little, still within 250 to 450 ms (on
  the CPU it fades in place); still out at 7.5 s, back between 7.9 and
  8.5 s, its surfaces gone, nothing drawn after. A finished agent's comes
  out ("a fini"), a stuck agent's does not. The reminder brings it out once
  more ("· 5 min"), and no more an hour later. A click on the panda goes to
  the agent's window on another desktop, the streak from the panda's left
  edge on a GPU, the halo ending, the panda going back at once; on the
  bubble, a finished agent is seen. A newer one replaces it: one surface,
  at the new agent's row. With reduced motion on a GPU, and on the CPU, it
  fades in and out without moving. It takes the pointer on its bubble and
  its panda, not in the room round them nor between them, and nothing once
  back. Unit tests for the motion's curves, the placing, the words, the
  panda's shadow and the halo's news. On the Intel GPU, the screenshots of
  the pull request.
- **Not built**: the keyboard (the bar's `Tab` does not reach the peek)
  and names for screen readers (§11); the mini form's agents (4.7), whose
  rows the peek will come out beside.

### 4.6, the secretary and her briefing (2026-10-30)

What the approved 4.6 mockup shows, variant B (the secretary heads the
agents' group), with the maintainer's spec (§7 B; issue #389). The next
meeting and the notifications wait for the calendar and sprint 5.

- **Her drawing** is the validated one, as SVG
  (`crates/hocus-shell/assets/pandas/secretary-*.svg`, with the pandas'
  note of their origin): one file per mood, her ear and the dot on her
  clipboard in its colour (green `#7BC47F`, amber `#F5B44A`, orange
  `#F2945B`), drawn by iced's `svg` at the screen's scale, sharp at ×1 and
  ×2; 40 px in the group, 34 px in her briefing.
- **The header** is a row as wide as the agents' (6 px past the group's
  sides), 44 px (2 px around her head), radius 9: her head, 8 px, then
  "AGENTS" (11.5 px 700, spaced, `text.dim`) and the total (Manrope 600,
  `text.faint`, 3 px after) over her line (12 px), 1 px apart, centred on
  her head. The rows start 6 px under it: the group is 50 px, 34 px a
  row and 20 px for the "+ N" line. Lit amber at 8 % when someone waits,
  orange at 7 % when an agent is stuck; `0.06` under the pointer, `0.08`
  while her briefing is open; the pointer a hand. She heads the group in
  each of its forms: the card (4.0), "Aucun agent en cours." and the rows;
  the group shows and hides as 4.0 built it.
- **Her mood and her line** come from the agents as the shell is told
  them (4.1): someone waiting first, as in the halo, then someone stuck,
  else calm. Waiting: only the waiting agents' names, "site-web",
  "infra, site-web" (the longest waiting first), `#F5C46E`. Stuck: only
  the stuck agents' names, "migration", `#F7A77A` (#503). Calm:
  "3 au travail", and " · 1 à voir" when a finished agent is not seen yet,
  `text.dim`. Cut with an ellipsis to the 124 px her head leaves.
- **Her motion**: when an agent starts waiting (one that was not waiting
  as the group last drew; not as the shell starts), her ear sends out two
  rings twice and she hops twice, the drawing's keyframes in 1.2 s
  instead of its 3.2 s, each scaled alike (#503): the rings (arcs of 15
  and 20 units round the ear, the outer at 60 %) from 0.9 to 1.25 times
  their size and from 90 % to nothing in 525 ms `ease-out`, twice; the
  hops 3 px up and down in 600 ms `ease-in-out`, twice. 1.2 s, once,
  then she holds still; the same report again does not start her
  again, another agent starting to wait while she moves starts her over.
  None with reduced motion, nor on the CPU. The rings are
  drawn by the shell over her drawing; the group's surface draws only
  while she moves.
- **What a tool said** (the hooks' payloads, docs/agents.md): a
  permission request (`PermissionRequest`), its tool and its input in one
  line (what it runs, the file it writes by its name, the address it
  reads, else its first text); the question `AskUserQuestion` asks;
  Claude Code's `Notification` text for the waiting ones; a
  `StopFailure`'s `last_assistant_message` (the API error as the
  conversation shows it), else its `error_details`, else its `error`.
  One line, its runs of spaces made one, 120 characters at most
  (`hocus_link::Note`). It goes on the report's line after a tab, and on
  the shell's `agents` line after each agent's fields, a tab apart
  (`allow TOOL WHAT`, `says TEXT`). The compositor keeps it with the
  agent's state: a new state takes the report's (none: none); in the same
  state a request replaces it and a text only fills one missing, since
  Claude Code's notification of a permission comes after the request and
  says less ("Claude needs your permission"). A state herdr gave more
  recently than the hook has no note. It may hold code or a secret: it is
  never logged, only its kind and its length (`allow (24 characters)`),
  and a line that does not read never says it.
- **Her briefing** (`popover/briefing.rs`, §9's mechanics and the kit):
  320 px, its top on her row's (100 px under the bar's), 12, 14 and 14 px
  in, its parts 8 px apart. Her head (34 px) and "Le point" / "Briefing"
  (16 px 700); no time, the clock is just above (#503).
  Then, each only when an agent is in it, "T'ATTEND" / "WAITING"
  (`#F5C46E`), "BLOQUÉ" / "STUCK" (`#F7A77A`), "À VOIR" / "TO SEE"
  (`#7EE0A0`): 11 px 700, spaced, 4 px more above; a line per agent, 2 px
  apart, 6 px past the sides, 5 px in, radius 7, lit `0.06` under the
  pointer: its name (13 px 700) and since when (`text.faint`, 6 px after:
  "· 2 min", "· il y a 4 min" for a finished one, "· à l'instant" within
  the minute, "1 h 05" from an hour), over what its tool said (12.5 px,
  `text.dim`, 1 px under: "Autoriser Bash : npm install ?" / "Allow Bash:
  npm install?", a text as it is), cut with an ellipsis. An agent whose
  tool said nothing (herdr's, a command's) shows its name and since when
  only; a finished one, since when only. The divider (2 px from what is
  around it), "N au travail · M au repos" (12.5 px, `text.faint`) and, on
  its right, "Sources", which opens the Sources popover in the briefing's
  place (4.0's, its top 62 px under the bar's). It is laid out again when
  the agents change, and at the clock's minute while it is open (its
  "since when").
- **A line goes to its agent** as its row does (4.3): the briefing closes
  and the compositor is asked `go-to-agent ID OUTPUT X Y W H` with the
  line's rectangle in the bar's frame, so the streak runs from the line's
  left edge. `Tab` and the arrows go through the lines and "Sources",
  `Enter` or `Space` acts, `Esc` closes and gives the keyboard to her
  row, which the bar's `Tab` reaches as its other items.
- **Measured headless**, the real shell (`tests/agents.rs`): her line and
  mood calm, waiting and stuck (the names alone), the row's amber and
  orange read from the screen, "Sources" no longer beside her; on a GPU,
  an agent starting to wait makes her move 1.0 to 1.5 s, once, then
  nothing draws, and the same report again nothing; with reduced motion,
  nothing moves. The briefing, 320 px, its top on her row: who waits with
  what it asks (the request kept over its later notification), who is
  stuck with the failure's message, who is to see, the footer, nothing
  drawn right of its title; the shell's log holding neither the command
  nor the error. A line going to its agent's window
  on another desktop, the briefing closed; Sources opening from it in
  4.0's place, the card under her; nothing committed at rest with the
  briefing open. Unit tests for her line's words, her motion's
  keyframes, since when, the
  request's words, the briefing's sections, the note's capping, its
  reading back and its log, the notes the hooks' documented payloads
  give, and which note is kept. On the Intel GPU, the screenshots of the
  pull request.
- **Not built**: the next meeting and the notifications in the briefing
  (the calendar, sprint 5); her line about a meeting ("Réunion bientôt");
  the peek (4.5); the mini form's agents and her place there (4.7); names
  for screen readers (§11).

### 4.7, the mini form's agents (2026-10-30)

What the approved 4.7 mockup shows, variant A (the secretary), with the
maintainer's spec (§5; issue #390). The meeting's mark above her came
with Day.2.

- **The item** is the agents' group's surface's, in the mini form: 68 px
  under the bar's top (12 px of padding, the clock's two lines of 19 px at
  1.1, 14 px; since #599 under the meeting's mark and the to-do item when
  they show), a button 48 px wide centred in the bar, radius 10: her 40 px
  head 4 px in, then 4 px under her how many work, "3" over "au travail"
  / "at work" (Manrope 600, 11 px, line height 1.25; `text.dim`, then
  `#7C8698`), 6 px under it: 81.5 px. The surface is 54 px wide and as
  tall as the item and the keyboard ring's room; while the form switches
  it is the full form's width, from the item's top to the group's bottom,
  each form drawn where it is and cross-faded as the clock's. It takes the
  pointer when a form shows alone. "au travail"
  is some 46 px wide at 11 px: it fits the button.
- **Her head** is the 4.6 drawing in her mood (green calm, amber when one
  waits, orange when one is stuck), with her rings and hops when an agent
  starts waiting (1.2 s, once; none with reduced motion nor on the CPU):
  the same motion, drawn by the same surface.
- **The badge**: 16 px high, its right 6 px past her head's, its bottom
  3 px under it; as wide as its number and 4 px each side, at least 16 px,
  radius 8; the number in Manrope 700, 10 px, `#1C1406`; a 2 px ring
  `rgba(14,17,28,0.95)` round it (its outer pixel on the right is cut at
  the surface's edge, inside the bar's rim). It says how many are waiting,
  stuck or to see; amber `#F5B44A` if one waits, else orange `#F2945B` if
  one is stuck, else green `#7EE0A0`; none at 0. It hops with her.
- **Lit** `rgba(255,255,255,0.06)` under the pointer, `0.08` while her
  briefing (or Sources, on the card) is open; the pointer a hand.
- **Her briefing**: a 150 ms hover opens it (the mini form's other items'
  mechanics, §9: it closes 300 ms after the pointer has left the item and
  the briefing, and a click keeps it), a click opens it and it stays.
  Beside the mini bar, 8 px from it, its top on the item's (§9, aligned
  with its item; the mockup has it 26 px higher). As 4.6 builds it, with,
  only in the mini form, after "À VOIR": "AU TRAVAIL" / "AT WORK"
  (`text.dim`), each working agent and since when ("· 8 min", as a
  waiting one's), and "AU REPOS" / "RESTING" (`#6A7384`), each resting
  agent's name alone; neither says what its tool said. Each line goes to
  its agent (4.3, the streak from the line); the footer and "Sources" as
  in 4.6. The full form's briefing does not change.
- **The group's rules**: hidden (no tool followed, none to propose), no
  item and the surface one pixel, the pointer going through; tools to plug
  in (the card's place), she is alone, calm, with a green "+" badge and no
  count (50 px), and her hover or click opens Sources (4.0's popover, its
  top 62 px under the bar's), where "Brancher" is; plugged in with no
  agent, she and "0 au travail".
- **"Ne rien brancher" from Sources** (4.7b, issue #508): the mini form
  has no card, so Sources opened from her "+" (in the mini form, while the
  group would show the card) has the card's "Ne rien brancher" / "Plug
  nothing in", under the tools' rows on the right (12 px, `text.faint`,
  `text` under the pointer, 6 px under the last row), one of its buttons
  for `Tab`. A click does what the card's does (4.0): every tool there
  and not plugged in declined, herdr's following too, remembered in the
  same file; Sources closes and the group goes, in both forms; a tool
  found later that was never declined brings it back. Sources opened
  otherwise (the full form's briefing, the card's "Un autre outil ?")
  does not have it: the card under her does.
- **The halo** is the compositor's, around whichever form the bar has
  (4.4): nothing changes. **The peek** comes out at her head's middle
  (92 px under the bar's top), 18 px from the mini bar.
- **The peek and her briefing** (4.7b, issue #508), in both forms: when
  her briefing opens while a panda is out, it goes back at once, with its
  usual going-back motion (4.5), so that it never shows through the
  briefing; while the briefing is open, none comes out. An agent that
  starts waiting or finishes meanwhile does not peek out, then or once the
  briefing closes (nothing is kept for later): its row, the briefing's
  line and the halo show it, and a waiting one's reminder at its fifth
  minute still brings its panda out if the briefing is closed by then.
  The shell tells the compositor `briefing open` and `briefing closed` as
  the briefing opens and closes (Sources taking its place closes it).
- **Her briefing's top** stays on her item's (4.7b, as built in 4.7).
- **The keyboard**: the item is one of the bar's items for `Tab`; `Enter`
  or `Space` opens her briefing, whose `Esc` gives the keyboard back to
  it (its ring at radius 10).
- **Measured headless**, the real shell in the mini form
  (`tests/agents.rs`): her item under the clock in the bar's middle, the
  surface 54 px wide; her ear and the badge read from the screen, green
  "1" for one to see, orange "2" once one is stuck, amber "3" once one
  waits; no badge at 0, how many work following them; a 150 ms hover
  opening the briefing beside the mini bar on the item's top with "AU
  TRAVAIL" and "AU REPOS" after who needs the user, leaving closing it, a
  click keeping it; a working agent's line going to its window on another
  desktop; no tool, no item and the pointer going through; a tool to plug
  in, the "+" and Sources from it; the halo's concern and the peek beside
  the mini bar at her head; nothing committed at rest with her briefing
  open. Since 4.7b: from the "+", Sources' "Ne rien brancher" declining
  both tools, Sources closing and the item going, the settings untouched,
  and the group still hidden in a new session in the full form; Sources
  from the full form's briefing without it; a panda out going back as her
  briefing opens (sliding under the bar on a GPU, fading in place on the
  CPU), gone long before its 8 s, and an agent finishing while it is open
  bringing none out, nor after it closes. Unit tests for the badge's
  number, colour and place, the card's "+", the briefing's mini sections,
  Sources' "Ne rien brancher" (where it is, and when it shows), and the
  `briefing` line read back. On the Intel GPU, the screenshots of the
  pull request.
- **Not built**: names for screen readers (§11).

### 5.5, the footer (2026-11-02)

What the approved 5.5 mockup shows, variant A (four equal buttons), with
the maintainer's spec (§4 group 11, §5; issue #395).

- **Its surface** is the bell's of 5.4 (`hocus-bar-footer` now), over the
  bar's bottom, opened after the bar's other surfaces so that it is above
  them, taking the pointer on its buttons only; a change of the badge
  draws it alone. Full form: 194 × 46 (the bar inside its rim), its
  bottom 7 px over the bar's. Mini form: 54 × 82, its bottom 5 px over
  the bar's. While the form switches, the full form's width and both
  forms' height, the two forms cross-faded as the other groups'.
- **The full form**: a 34 px row, its bottom 12 px over the bar's inside
  bottom, four 40 × 34 buttons across the bar's 172 px inside, 4 px
  apart: the clipboard, the bell, the menu (a 2 × 2 grid) and the user.
  The levels keep 14 px over it, as before.
- **The mini form**: the bell (44 × 34), and 2 px under it (as the
  Wi-Fi's and Bluetooth's buttons are), last of all, the user's avatar
  (44 × 34), its bottom 10 px over the bar's inside bottom. The column's
  items over them moved up 48 px: the keyboard layout's button (or the
  battery, or the sound's gauge, whichever is lowest) keeps 14 px over the
  bell.
- **The buttons**: radius 9, lit `rgba(255,255,255,0.08)` under the
  pointer, `0.10` while their popover shows, the keyboard's ring while
  they hold it; the pointer a hand over them. The glyphs are the
  mockup's 24 px paths drawn at 20 px, 1.8 px strokes in `#C9D0DC`, the
  bell's colour (the spec says `text.dim`; the mockup and 5.4's bell use
  this one). The avatar: 24 px round, the name's first letter (Manrope
  700, 12 px, `#0B1020`) on the accent's gradient (`#6CC287` to
  `#3E8FBF`, top left to bottom right); since 7.4 the account's photo,
  else the panda on that gradient (below). The badge is 5.4's, its right
  2 px inside the button's and its top 1 px under it (the mockup's).
- **Opening**: the clipboard, the bell and the user open their popovers
  by a 150 ms hover or a click, in both forms (the mockup's; the bar's
  other items in the full form open on a click only), closing 300 ms
  after the pointer leaves unless a click opened them (§9). Their bottom
  is on the bar's. The menu opens the launcher (5.1) on the bar's screen
  on a click, or `Enter`, only: never by hovering.
- **The clipboard's popover**: its history since 5.6 (below). Until
  then, a 280 wide popover showed what the clipboard held, read by the
  shell as a client.
- **The user's popover**: until 7.4, 280 wide, the user's name as the
  title and one "Se déconnecter" row that took a second click within 4 s
  to log out, `Super+Shift+E` ending the session at once. The session
  menu (7.4, below) replaced it: lock, suspend, log out, restart and shut
  down, the last three confirmed with a 10 s countdown.
- **"Réglages" / "Settings"** (6.1, docs/design/settings.md): a row of the
  user's popover above "Se déconnecter", its sliders glyph in the same
  tile; a click, or `Enter` or `Space` once `Tab` gave it the keyboard,
  starts `hocus-settings` (in `PATH`, else beside the shell) through the
  compositor, with an activation token, and closes the popover; an open
  Settings window comes forward. Since 7.4 it is the menu's first row.
- **The user's name**: their account entry's (`getent passwd UID`, else
  `/etc/passwd`), its GECOS field's first part, else their login; looked
  up on a thread as the shell starts, the session's `USER` standing in
  meanwhile. `HOCUS_USER_NAME` names the user instead, for recordings and
  tests.
- **Names**: "Presse-papiers", "Notifications, 3 non vues",
  "Menu : toutes les applications", "Alex, menu de session" (and in
  English), said in the shell's log as the footer draws them; `Tab` and
  the arrows reach the buttons from the bar's other items, `Enter` or
  `Space` acts.
- **Measured headless** (`crates/hocus-compositor/tests/footer.rs`), the
  real shell on the CPU: the full form's footer at its place, its four
  buttons drawn in a row, their names in order; the clipboard's history
  saying "nothing", then the text a window copied; the bell opening the
  history, the menu the launcher over the whole screen, the user's
  popover with "Alex" and its rows (7.4's tests below); the
  menu opening nothing after 600 ms of hovering, the bell beside it
  opening its history, and a click opening the launcher with the
  keyboard; the mini form's bell over the avatar at the bar's bottom, the
  avatar's hover opening the user's popover beside the mini bar. The
  badge drawn on the footer's bell (`tests/notifications.rs`). Unit tests
  for the places, the names, the user's name from an entry and its
  initial. On the Intel GPU, the screenshots of the pull request.
- **Not built**: names for screen readers (§11: the names are said in
  the log only). The session's menu and the account's picture came with
  7.4.

### 5.13, the bar under the windows (2026-11-02)

What the maintainer decided on 2026-11-02 (§2, §3; issue #519): a window
placed over the bar goes in front of it.

- **The order**, front first: the launcher and its toast, the overlay
  layer, a fullscreen window in front, the other surfaces of the top layer
  (the bar's popovers and their shadows, the bubbles and their chip,
  third-party bars), the halo, the peek, the streak, the snap preview, the
  windows leaving, the windows; then the bar's own surfaces (its tint, the
  clock, the levels, the connections, the desktops, the agents and the
  footer: `hocus-bar*` but the popovers and the peek), its glass, the
  bottom and background layers. The bar in front: its surfaces and its
  glass go back above the windows, under the popovers, as before 5.13. The
  pointer finds surfaces in the same order (`Stand` in
  `handlers/layer_shell.rs`): over a window over the bar, the window's
  are found first, and it gets the presses, the cursor and the resize
  bands there.
- **The glass** shows what is under the bar: the lower layers and the
  background, blurred, where no window covers it; a window over it hides
  it there. In front, it blurs the windows under it again (#517's glass).
- **What still keeps off it**: the work area (§2) is unchanged, so new
  windows, maximised, snapped and the bar's switch place windows as
  before. Dragged by the pointer to the work area's right edge, a window
  still snaps to the right half; held by its left part, it goes over the
  bar. A window's shadow falls on the bar under it, as on the desktop.
- **The edge** (`crate::front`): the pointer against the screen's right
  edge (its last column), level with the bar, starts a 300 ms timer of
  the loop; leaving the edge before cancels it; when it fires, the bar
  comes in front and the pointer's surface is found again where the
  pointer is. It stays in front while the pointer is in the bar's column
  (from the bar's left edge to the screen's right, top to bottom), and goes
  back under as the pointer leaves it, at the next motion. Not while a
  button is held on a client or a window is dragged, nor over a
  fullscreen window or under the launcher. The click on the edge zone
  still toggles the form, raised or not. One frame as the bar comes in
  front and one as it goes back; none at rest.
- **Measured headless** (`crates/hocus-compositor/tests/bar_front.rs`),
  plain layer surfaces of their own colours standing for the shell's: a
  window dragged half over the bar covers that half (the screen shows it)
  and takes the pointer and a click there, the other half the bar's; the
  pointer against the edge 150 ms, or leaving it before 300 ms, brings
  nothing; resting there brings the bar in front, over the window and
  taking the pointer where it covered it; out of the bar, the window
  covers it again; the edge's click switches the form; nothing drawn at
  rest in either state. A new window and a maximised one stop 8 px from
  the bar. A popover and a bubble over a window covering the bar show and
  take the pointer. On the GPU, under the real shell, the halo shows over
  the window covering the bar, and goes. A fullscreen window covers the
  bar, pushed or not. On the Intel GPU, the screenshots of the pull
  request.

### 5.6, the clipboard's history (2026-11-02)

What the approved 5.6 mockup shows, variant A (from the bar), with the
maintainer's spec (issue #396).

- **Opening**: `Super+V` (`[shortcuts] clipboard`) opens it on the bar of
  the focused window's screen (else the pointer's; a screen without a bar:
  the first bar's), its search holding the keyboard at once; again, it
  closes, and opened by hovering it takes the keyboard. The footer's
  clipboard opens it too: by a 150 ms hover without the keyboard, by a
  click with it, and a click in a hovered history gives it the keyboard
  (§9's `Hold`). Opened by `Super+V`, `Esc` gives the keyboard back to the
  window that had it, not to the footer's button.
- **The popover**: 360 wide, 556 tall whatever it lists (its bottom on the
  bar's, so that typing never moves the field), 12 px inside, radius 14,
  its tint the mockup's (`rgba(22,27,42,0.96)` to `rgba(14,17,28,0.98)`,
  opaque without glass), its parts 8 px apart. The header (26 px):
  "Presse-papiers" / "Clipboard", Manrope 700, 15 px, and "Tout effacer" /
  "Clear all", a ghost button (12 px 600 `#A9B2C2`, 9 px in, radius 8,
  `rgba(255,255,255,0.10)` under the pointer). The search: 36 px, radius
  10, `rgba(255,255,255,0.05)` in its green border
  (`rgba(108,194,135,0.45)`), a 15 px magnifier 10 px in, 8 px before
  what is typed (Manrope 13.5 px) or "Chercher dans ce que tu as copié" /
  "Search what you copied" (`#7C8698`); the caret while it holds the
  keyboard; a query too long for it shows its end. The list: 430 px; the
  keys under it: "Entrée colle · Maj+Entrée copie · Suppr retire" /
  "Enter pastes · Shift+Enter copies · Delete removes" (10.5 px,
  `#6A7384`).
- **The list**: "ÉPINGLÉS" / "PINNED" when there are pinned items, then
  "RÉCENTS" / "RECENT" (22 px labels, 10.5 px bold spaced 0.08 em,
  `#7C8698`, 6 px in), each newest first, everything 2 px apart. A row:
  44 px, radius 10; its 30 px tile (`rgba(255,255,255,0.07)`, radius 8)
  9 px in: `>_` for a command or code, `↗` for a link, `Aa` for text, `/`
  for a path (11 px bold, `#A9B2C2`), a colour's swatch or an image's
  thumbnail 3 px inside it (radius 6); 10 px after it its content on one
  line, its white space made spaces, cut with an ellipsis (Martian Mono
  11.5 px for code, paths and colours, else Manrope 600 13 px), over "App
  · il y a 4 min" (10.5 px, `#7C8698`; the application's name from its
  desktop entry, else its id; "maintenant", "il y a 2 h", "hier", "il y a
  6 jours", as the notifications' history says them). An image's line is
  "Image · 1280 × 800". The selected row (the first one as it opens, as
  the query changes and when the one selected goes): the green border
  (`rgba(108,194,135,0.55)`) and tint (`0.13`), and on its right, 5 px
  in, its pin and ✕ (24 px, 2 px apart, radius 6, lit `0.10` under the
  pointer), the pin `#8C96A8`, amber (`#F5C46E`) and filled when pinned.
  The pointer selects the row it is over. The wheel scrolls a row a notch
  (a 6 px thumb on the right); the arrows move the selection, the list
  following it. Nothing listed: "Rien de copié pour l’instant." / "Nothing
  copied yet.", or "Rien pour « kitty »" / "Nothing for “kitty”" (13 px,
  `#8C96A8`, 30 px down), from the mockup.
- **Kinds**, told from the text alone: a colour (`#6CC287`, `#fff`,
  `rgb(…)`, `rgba(…)`; `#123`, more likely an issue's number, is text), a
  link (`http://`, `https://`, `ftp://`, `mailto:`, one word), a path
  (`/`, `~/`, `./`, `../`, `file://`, one line without a command's
  flags), code (a line starting with a usual command, `git`, `cargo`,
  `ssh`, `sudo`…, or `$ `, or holding a pipe, `&&`, `--`, `$(`, or
  source's marks), else text.
- **The search**: a substring of an item's text and its application's
  name, accents and case ignored (the launcher's folding).
- **Actions**: `Enter`, or a click on a row, makes the item the clipboard
  (`clip paste`: the compositor offers it to Wayland and X11 clients),
  closes the history and lets the keyboard go (its surface asks for none
  first, so that the window has it back at once), and the compositor
  sends the window that had the keyboard `Ctrl+V`, or `Ctrl+Shift+V` when
  its desktop entry (by its application's id, as is or in lower case, in
  the data directories' `applications`) says `TerminalEmulator`: the keys
  of the active layout (the one typing `v`), as the keyboard's own, once
  the window holds the keyboard again, 1 s at most. `Shift+Enter` only
  copies (`clip copy`), and closes. Either way the item moves to the top.
  `Delete` or ✕ removes it (the next row selected), the pin pins it or
  not, "Tout effacer" removes all but the pinned, `Esc` closes, `Tab`
  stays in the field.
- **The source**: when a client takes the clipboard (a Wayland client's
  selection, or an X11 client's through XWayland), the compositor reads
  its `text/plain;charset=utf-8` offer (else `UTF8_STRING`, else
  `text/plain`) or, offering no text, its `image/png`, through a pipe on a
  thread, 8 MB and 3 s at most, and tells the shell (`copied`: the
  application of the window focused as it was copied, and the bytes, in
  base64). Copies the history itself makes are not read again. Screen
  captures come in too (5.8b, [screenshots.md](screenshots.md)): their
  PNG, from "Capture d'écran" / "Screenshot" (`capture-copied`). Never
  read, never told: a selection offering `x-kde-passwordManagerHint` that
  says `secret`, or `CLIPBOARD_STATE` that says `sensitive` (either unread
  in time counting as a secret), or copied from a window whose
  application's id names Bitwarden, KeePassXC or 1Password. The same text
  (or image) copied again is the same item, moved to the top. The link
  writes what its socket does not take at once later, as it takes more,
  never waiting (64 MB at most ahead of the shell).
- **What is kept**: the pinned items for good; the others, the last 200,
  30 days at most (pruned as copies come and as the history opens). On
  disk, `$XDG_STATE_HOME/hocuspanda/clipboard/` (`0700`): an item's
  details and its text, or its image's 72 px thumbnail, in `<key>.clip`,
  an image's PNG in `<key>.png`, each sealed with XChaCha20-Poly1305 (a
  random nonce each time, the file's name authenticated with it), written
  whole then renamed, `0600`, on the history's own thread. The key: the
  session's keyring's (the Secret Service on the session bus the
  compositor names, as the notifications' bus: never one a nested or
  headless desktop was not given; an item `application=hocuspanda`,
  `hocuspanda=clipboard` in the default collection, asked with a 15 s
  timeout, an unlock prompt included), else a key file beside the
  history, `clipboard.key` (`0600`), made once and used from then on. A
  keyring that holds the key and does not answer leaves the history
  alone: what is copied is then kept for the session only.
- **Measured headless** (`crates/hocus-compositor/tests/clipboard.rs`),
  the real shell on the CPU, no session bus (the key in its file), the
  test's own desktop entries: a copy listed with its application's name;
  the search by text without accents, by application, and nothing found;
  `Enter` pasting into an editor (`Ctrl+V`) and into a terminal
  (`Ctrl+Shift+V`), and a click on a row of a hovered history too;
  `Shift+Enter` only copying, the item first afterwards; `Delete`
  removing it and its file; a pin kept through "Clear all"; a password
  marked secret and a KeePassXC window's copy never kept; the history
  read back after a restart, nothing on disk holding the text copied, the
  files `0600`, the directory `0700`; an image's thumbnail drawn in its
  tile, and pasted back as the PNG copied. Unit tests for the kinds, the
  thumbnail, the history's limits and order, the sealed files and the
  key file, the keyring not answering, the lines, the secret marks, the
  terminal's desktop entry and `Super+V`'s keyboard. On the Intel GPU,
  the screenshots of the pull request.
- **Not built**: the mockup's "1 mot de passe copié, non gardé" in the
  footer (the spec keeps no trace of a secret); the shell's own copies
  (the launcher's results, the Sources popover's command) in the history;
  the primary selection.

### 5.11, the sign of what is shared (2026-11-03)

While the screen is shared, a sign shows under the clock (48 wide, in
both forms, under the meeting's card or mark; the column under it moves
down, #599) and
its popover stops the share: [sharing.md](sharing.md), as built. 5.12
groups it with the microphone and the camera (issue #445), below.

### 5.12, what uses the microphone, the camera or the screen (2026-11-03)

What the approved 5.12 mockup shows, variant A (issue #445): one sign
under the clock, which 5.11's sharing sign joins as its screen and
5.10's recording item as a tile of its own (5.12b, issue #561, chosen on
the machine).

- **The sign** (`crates/hocus-shell/src/in_use/draw.rs`): a surface of
  its own on each bar (`hocus-bar-in-use`, never in a capture, a share or
  a recording), under the next meeting's card or mark (Day.2), else right
  under the clock, centred; the column under it (Today, To do, the
  agents' group or their items, #599) moves under it and 14 px. 48 wide,
  radius 12, `rgba(255,255,255,0.05)` (`0.10` under the pointer and while
  its popover shows), 6 px in above and under its tiles: a 30 × 24 tile a
  kind in use (radius 7, 4 px apart), its 16 px icon (the mockup's
  drawings, stroked 2 px at 24) in its colour on `#RRGGBB29` of it: the
  microphone `#F5A35B` (muted: `#A9B2C2` on `rgba(255,255,255,0.06)`),
  the camera `#6CC287`, then the recording's (below), then the screen
  `#8FB3FF` for the shares only, in that order. Stacked in the mini form,
  each centred across it; in a row in the full one, 9 px in at its sides,
  as tall as its tallest tile, the others centred on it (one tile is the
  same 48 × 36 sign in both forms). It pops in over 200 ms as it appears
  (`ease-out`, from 0.9 of its size and transparent; a fade alone with
  reduced motion and on the CPU, which draws no motion), then draws only
  when what it shows changes. A tile coming or going resizes its surface;
  the agents follow.
- **The recording's tile** (5.12b, issue #561; 5.10's item, its drawing
  in `in_use/draw.rs`): 36 × 42 (the sign's 48 less its 6 px sides; 6 px
  in above its 10 px dot, 4 px over its time's 16 px line, 6 px under
  it), radius 8, `rgba(242,84,91,0.16)` in a 1 px `rgba(242,84,91,0.55)`
  ring; the dot `#F2545B`, the time in Martian Mono 12 px, `#FFD9DB`:
  "0:12", "12:04", "1:02:03", smaller when it would not fit inside the
  ring (12 px holds four characters). The dot beats from 1 to 0.35 of its
  opacity and back over 1.4 s (ease-in-out); still with reduced motion
  and on the CPU. Paused, the dot amber `#F5B44A` and still, the time
  "pause" / "paused". Alone, the sign is 48 × 54 in the mini form, 54 ×
  54 in the full one; with the others, the mini sign grows by 46 and the
  full one by 40. The time is counted by the shell from the compositor's
  `recording ID running|paused ELAPSED` lines (on the monotonic clock,
  from when the line came); the sign draws again only for it: as its
  second changes when still, every 50 ms while the dot beats; nothing
  wakes while it is paused, nor once no recording runs. Ended, it says
  the time it ended at, still, while it lingers. No buttons: they stay
  in the popover.
- **When** (`in_use/mod.rs`): each use shows from the moment its source
  says it, and lingers 3 s after it ends (on the monotonic clock: a
  thread of its own sleeps until the next use to go, and without one
  until told one); a use back while it lingers is the same use, its
  "depuis" kept. The sign goes when the last use has lingered.
- **The microphone**: the sound's `pw-dump --monitor`
  (`levels/sound.rs`) keeps the graph's links too: a `Stream/Input/Audio`
  node linked from a source (`Audio/Source`, `Audio/Source/Virtual`; not a
  sink's monitor: a level meter on an output is not the microphone) is a
  use, its streams taken together by application (its sandbox's app id,
  else `application.name`, else its program); named by
  `application.name`, else `application.process.binary`, else the
  program of `application.process.id` (`/proc/PID/comm`); its icon as the
  sound's popover finds it (its icon's name, its desktop file's).
  Grey while the default source (else the first) is muted.
- **The camera**: a `Stream/Input/Video` node linked from a
  `Video/Source`, the same way; and the processes holding a camera's
  device open (`in_use/cameras.rs`), for programs that open
  `/dev/video0` themselves: a thread of its own, started with the first
  camera's node, watches the devices (`/dev/video*`, or
  `HOCUS_VIDEO_DEVICES`'s) with inotify (opened, closed) while a camera's
  node exists, and reads `/proc/*/fd` when one was touched, at most once a
  second (a touch during the second after a reading is read at its end);
  without a node it sleeps, its watches dropped. A device it cannot
  watch is read each second while a node exists. PipeWire's and
  WirePlumber's own processes are left out (their opens are a stream's,
  which the graph shows with its application); a program is named by its
  desktop entry (by its program's name), else by its program.
- **The screen**: the compositor's `shares` (5.11), the screen's tile;
  and its `recording` (5.10), the recording's tile. No longer items of
  their own: their lines and buttons are in the popover.
- **The popover** (`popover/in_use.rs`), 290 wide, as the bar's others: a
  150 ms hover or a click opens it, the pointer leaving closes it 300 ms
  later, `Tab` and the arrows go through its buttons, `Enter` acts, `Esc`
  closes. "En ce moment" / "Right now" (14 px bold), then a line per use
  (the microphone's, the camera's, the recording, the shares): the
  application's 28 px icon (its letter on its colour without one; the
  recording's red dot, amber while paused, on 5.10's tint) ringed 2 px in
  its kind's colour, its name (13 px semibold) and "Micro ·
  meet.google.com · depuis 12 min" / "Microphone · … · for 12 min" (11.5
  px, the kind's colour; "Micro coupé" / "Microphone muted" while muted).
  The detail is the title of the application's window (a window whose
  application id is one of the stream's ids, whole or by its last part,
  the focused one first), else the stream's `media.name`; a share's,
  "partage la fenêtre « … »" / "sharing the window "…"" or the screen; the
  recording's, "enregistrement" or "en pause" and its time ("0:12"). The
  time: "depuis 12 s" under a minute, "depuis 12 min", "depuis 1 h 05"
  ("for 1 h 5 min"); drawn again each second while it shows. A line too
  long has its detail cut ("…"), what is used and since when kept. While
  sharing, how many notifications wait for its end (5.11's line). Then
  its buttons, 30 tall, radius 9, 8 px apart: **Couper le micro** /
  **Mute the microphone** (`rgba(245,163,91,0.14)`, its 1 px ring
  `0.6`), or **Rétablir le micro** / **Unmute** (no fill, a
  `rgba(255,255,255,0.14)` ring), which mutes the default source as the
  sound's popover's button does (`wpctl set-mute`); **Arrêter le
  partage** / **Stop sharing** for each share (5.11's `#5B8DEF`; with
  several, "Arrêter le partage avec Firefox"), which ends its session; the
  recording's pause (or go on, 36 wide, 5.10's) and **Arrêter
  l'enregistrement** / **Stop recording** (`rgba(242,84,91,0.85)`). A use
  that ended keeps its line while it lingers, without its button.
- **Choices made here** (to confirm on the machine): 5.12 had the
  screen's tile blue for a recording too and the recording's time in its
  popover's line only; the maintainer chose on the machine to bring 5.10's
  red and time back (5.12b, issue #561): the recording is its own red
  tile with its time, the screen's blue tile for shares only. Then, 5.12b's
  own: the recording's tile is 42 tall, 5.10's item's 6 px
  in kept above its dot and under its time, and in the full form's row
  the shorter tiles are centred on its height; its time shrinks to fit
  inside its ring when longer than four characters (Martian Mono's 12 px
  advance is 8.4 px: "12:04" is 9.7 px, "1:02:03" 6.9 px, "pause" 9.7
  px), where 5.10's 48 px item let it overflow; without a beat (reduced
  motion, the CPU) it draws as its second changes, not every 250 ms as
  5.10's did; the sign pops in as it appears, a new kind's tile
  then shows at once; under a minute the line says "depuis 12 s"; a
  share's and the recording's lines come after the microphone's and the
  camera's, the recording first; a lingering use keeps its line;
  "Unmute" in English as the spec says; with several shares, each button
  names its application; the cameras' devices are watched with inotify,
  so that `/proc` is read only when one is opened or closed (the spec's
  bound, once a second while a camera's node exists, holds: it is the
  most).
- **Measured headless** (`crates/hocus-compositor/tests/in_use.rs`): the
  real shell on the CPU, against a PipeWire of the test's own (its source
  a null node, the applications' streams null nodes of their classes,
  linked by the test as a session manager would), a D-Bus daemon of its
  own for the screen cast portal, the cameras' devices files of the
  test's own: the microphone's sign as a stream takes the source (a
  stream not linked: none), orange; a use gone, the sign gone no sooner
  than 3 s after (the shell's clock says how long it lingered); the
  camera's through PipeWire and through a program holding its device;
  the screen's as it is shared; the microphone grey while the source is
  muted, orange again; the three side by side (the surface 92 + 10 tall);
  the full form's row; the popover's lines, the window's title or the
  media name, "depuis"; its button muting the default source and back
  (as `wpctl` reads it); its buttons stopping the share and the recording
  (pause, go on, stop); at rest no commit of the sign and no reading of
  `/proc` without a camera's node, one with a node nobody opens; twenty
  opens in two seconds, a reading a second at most, none once the node is
  gone. 5.11's and 5.10's tests go through the popover. 5.12b's (issue
  #561), recording the screen with ffmpeg: the recording's red tile (its
  dot's pixel), 54 + 10 tall, its time "0:0N" (as the shell says it
  reads it) moving on, the sign committed at most once a second on the
  CPU; paused, amber and "pause"; going on, red; stopped, gone no sooner
  than 3 s after; with a share, the red tile then the blue one (the
  surface 82 + 10 tall); in the full form after the microphone's, in a
  194 × 64 surface; paused, and once the recording's tile has gone with
  a share still on, no commit of the sign. Unit tests for
  the linger and a use back, the kinds' order, the words in both
  languages, the sizes in both forms (the recording's tile's too), the
  pop, the colours, the time and the beat, when the recording draws
  again, the time's size, the graph's
  links, the popover's layout, the `/proc` reading's bound and a holder
  found by its program.
- **Not built**: the camera's light as a privacy indicator of its own
  (none is read: the device's use stands for it); a portal's camera
  sessions told apart from other streams (they are streams).

### 5.14, the tray (2026-11-03)

What the approved 5.14 mockup shows, variant A (always visible; the amber
ring left out), with the maintainer's spec (issue #531).

- **Its place**: just above the footer, under a 1 px
  `rgba(255,255,255,0.08)` line across the bar's inside, 6 px over the
  cells; the cells' bottom 10 px over the footer's top (the mockup's gap).
  The bar's groups above (the levels, and the connections and the
  desktops over them) keep their 14 px over the line: they go up by the
  cells' height and 17 px. Full form: a row of 34 × 34 cells 4 px apart
  from the bar's inside left, four to a row, wrapping under. Mini form: a
  column of 40 × 32 cells 2 px apart, centred. No icon: no surface, no
  line, and the groups where they were.
- **Its surface** (`hocus-bar-tray`, under the windows with the bar's
  others, 5.13), opened after them while an icon shows, taking the pointer
  on its cells only; an icon's change draws it alone, an icon coming or
  going places the bar's groups again. While the bar switches its form, it
  holds both forms' places, the two cross-fading.
- **The icons**: 22 px, from `IconPixmap` (the smallest at least as large
  as the icon's pixels, else the largest, scaled to them; ARGB32 in
  network byte order), else `IconName`, looked for in the item's
  `IconThemePath` (its files, then its `hicolor`), then as the launcher
  looks for its icons; decoded on the tray's thread at the bars' screens'
  largest scale, again when it changes. An item with neither shows its
  name's letter on a colour of its own. The overlay (`OverlayIcon…`) is
  drawn over the icon's bottom right, half its size. They pop in over
  200 ms (from 80 %, fading in, `ease-out`), at once without motion or on
  the CPU. A cell is lit `0.08` under the pointer and `0.10` while its menu
  shows (the footer's buttons' radius 9 and colours), the keyboard's ring
  while it holds it.
- **What shows**: the icon as the application sends it, read again on
  `NewIcon`, `NewAttentionIcon`, `NewOverlayIcon`, `NewStatus`, `NewTitle`
  and `NewToolTip` (Slack's dots are its own pixels). `Passive` items are
  left out; `NeedsAttention` shows the attention icon when there is one.
- **Actions**: a left click sends `Activate(0, 0)`; an item that says
  `ItemIsMenu`, or one whose `Activate` fails, opens its menu instead. A
  middle click sends `SecondaryActivate(0, 0)`, the wheel `Scroll` (120 a
  notch, up positive, `vertical` or `horizontal`). Wayland tells a client
  no place on the screen, so the coordinates are 0.
- **Its application comes forward** (5.14b, issue #537): the click is the
  user's input, but on the bar's surface, so the application never had
  the keyboard and the token it makes for itself would only pulse its
  rim (§4). Before `Activate` and `SecondaryActivate`, the tray's thread
  asks the item's process of the bus (`GetConnectionUnixProcessID`) and
  tells the compositor (`tray-click ASK PID`); the compositor answers at
  once with an activation token made for the click (`tray-token ASK
  TOKEN`, waited for 500 ms at most, on the tray's thread only), which
  the item is given with `ProvideXdgActivationToken(token)`, its answer
  waited for (250 ms at most) before `Activate`, since an item may serve
  its calls side by side; an item without it says so at once. Qt's and KDE's
  applications activate with it, a token that focuses as one the
  desktop gives a program it starts. For those that ignore it
  (Electron's, Slack), the click also lets the item's application focus
  one window within 2 s on the compositor's clock: a window asking with
  any token, or the first it maps (an application hidden in the tray that
  opens its window on `Activate`). The token used, or the one focus
  given, spends the click. The application is the Wayland client's
  process when it is the item's, one the item's started (Chromium's
  helpers, a launcher script's program) or one that started it (a tray
  helper), or, sandboxed, one of the same application unit
  (`app-….scope`, `app-….service`, `snap.….scope`: Flatpak's D-Bus goes
  through a proxy beside the application); never by process group, nor
  through the compositor or its parents, nor by the compositor's own
  unit: what it starts shares its group and its unit, and everything
  descends from it.
- **Its menu**: a right click (or `Enter` while the icon holds the
  keyboard) reads the item's `com.canonical.dbusmenu` (`AboutToShow(0)`,
  then `GetLayout(0, -1, [])`) and opens it as a popover, its bottom on the
  icon's, 8 px from the bar as the others: 220 wide, 6 px inside; the
  application's name over the rows (11 px bold capitals spaced 0.08 em,
  `#7C8698`); 30 px rows (12.5 px, 8 px in, radius 8, lit `0.07`), 9 px
  separators, entries that are off in `#6A7384` and inert, a check or a
  radio's dot (`#6CC287`, a ring when off) in a column of their own, 16 px
  icons (`icon-name`, `icon-data`), the labels' mnemonics' underscores
  left out, hidden entries too. A submenu (`›`) opens in its place under
  "‹ ITS NAME", which goes back. A click on a row sends
  `Event(id, "clicked", 0, 0)`, then it closes. `LayoutUpdated` and
  `ItemsPropertiesUpdated` while it shows read it again. The arrows and
  `Tab` go through the rows that act, `Enter` or `Space` acts, `Esc` goes
  back, then closes. An item without a menu is sent `ContextMenu(0, 0)`.
  A second right click on the icon closes it.
- **The tooltip** (`hocus-bar-tip`): after 400 ms on an icon, "Name ·
  what it says" (12 px, the name bold), beside the bar 8 px from it,
  centred on the icon, 7 px by 10 inside, radius 9,
  `rgba(14,17,28,0.96)` in a 1 px `rgba(255,255,255,0.12)` rim; the name
  is the item's `Title` (else its `Id`), what it says its `ToolTip`'s
  title (else its text), left out when empty or the same as the name. It
  takes no pointer, and goes as the pointer leaves the icon or presses it.
- **The protocol**: the shell serves `org.kde.StatusNotifierWatcher` and
  `org.freedesktop.StatusNotifierWatcher` at `/StatusNotifierWatcher`
  (`RegisterStatusNotifierItem`, `RegisterStatusNotifierHost`,
  `RegisteredStatusNotifierItems`, `IsStatusNotifierHostRegistered`,
  `ProtocolVersion`, and their signals), owns both names and
  `org.kde.StatusNotifierHost-PID`, on the bus the notifications use
  (`--session-bus`, or the session's own on a virtual terminal; none,
  no tray). An item registers by its bus name (its object at
  `/StatusNotifierItem`) or its object's path (on the bus it calls from);
  `org.kde.StatusNotifierItem` is read, else
  `org.freedesktop.StatusNotifierItem`. It goes when its name's owner
  does (`NameOwnerChanged`). Items keep the order in which they (by their
  `Id`) first registered this session, a restarted application its
  place. Everything is on the tray's thread; what the bar asks runs
  aside, so that an item that does not answer holds nothing up.
- **Measured headless** (`crates/hocus-compositor/tests/tray.rs`), the
  real shell on the CPU, a D-Bus daemon of the test's own and an item of
  the test's own (a StatusNotifierItem and its dbusmenu, in zbus): an item
  registering shows its icon above the footer, the levels going up, in the
  full form then the mini; `NewIcon` draws it again, the red dot of its
  new pixmap with it; `Passive` hides it, `NeedsAttention` shows its
  attention icon; a left click is `Activate`, a middle click
  `SecondaryActivate`, the wheel `Scroll 120 vertical`; with the item's
  application's window behind another program's, which has the keyboard
  (5.14b): an item that takes the token is given it before `Activate`,
  and the window comes forward with it even past the 2 s; an item without
  `ProvideXdgActivationToken` whose window asks with a token of its own
  comes forward; without a click it only pulses; 2 s after the click it
  only pulses, and the click lets it come forward once, a second request
  pulsing; an application with no window that opens one on `Activate`
  has it focused, in front; a right click
  opens its menu beside the bar, its bottom on the icon's, its rows read
  ("Ouvrir Slack" without its mnemonic, the separator, the check, the one
  off), and a click on "Quitter Slack" sends `Event(5, "clicked")` and
  closes it; the item's connection ending takes its icon away; resting on
  it shows "Slack · Vous avez 2 notifications" after 400 ms, gone as the
  pointer leaves. Unit tests for the places in both forms and while
  switching, the pop-in, the pixmaps (the best size, ARGB, the square),
  the rule matching the item's process to the window's (its own, a child,
  a parent, a Flatpak's proxy; not a sibling, the compositor, its
  parents),
  the item's own theme path, a layout's entries and mnemonics, the
  registrations' places, the tooltip's words and its delay, the menu's
  rows. On the CPU, the screenshots of the pull request.
- **Not built**: the amber ring (left out of the approved spec); the
  items' `AttentionMovieName`; names for screen readers (§11).

### Day.2, the next meeting (2026-11-04)

What the approved Day.2 mockup shows, variant B (the card opens the
meeting, a button joins it), with the maintainer's spec (§4 group 2, §5,
§6; issue #426). Its data is the day's (docs/design/day.md).

- **Which meeting** (`crates/hocus-shell/src/meeting/mod.rs`): the first
  of today's events of the featured calendars (`Day::featured`, Day.7)
  that take time (timed, neither cancelled, declined nor free:
  `Event::takes_time`) and has not ended.
  One in progress keeps the card until its end, overlapping or not; then
  the next. Nothing left today: no card.
- **When it changes**: the shell asks the day as it changes
  (`day::on_change`, on the day's thread) and at the clock's minute change
  (each bar's clock says it, as for the agents' minutes): the card's
  minutes, its line and its colours change then, and only then. The times
  are whole minutes: the meeting's start and end to the nearest minute,
  the clock's minute as it shows it, so that "dans 1 min" lasts until the
  start ("dans 25 min" is the start's minute less the clock's). A change
  of the day that the card does not draw (the event after it, which only
  the mini popover shows) does not draw it.
- **The card** (`meeting/draw.rs`): a surface of its own on each bar
  (`hocus-bar-meeting`), opened while a meeting is left today and removed
  after, right under the clock (100 px under the bar's top in the full
  form, 68 in the mini), opened after the bar's own surfaces so that it
  is in front of the bar's. The sign of what is in use (5.12) and the
  column under it (Today, To do, the agents' group, #599) move under it:
  58 px lower (the card and its 14 px) in the full form, 30 (the mark's
  button and its 14 px) in the mini. 172 ×
  44, radius 10, its fill and 1 px border by how near the meeting is:
  `rgba(255,255,255,0.05)` / `0.06`; 10 min or less `rgba(143,179,255,0.12)`
  / `0.30`; 2 min or less `0.18` / `0.55`; in progress `0.09` / `0.22`. Its
  stripe (6 × 30, radius 3, the calendar's colour) 7 px in (its border and
  6 px), its title 8 px after (Manrope 700, 13.5 px, cut with "…"), over
  its time line 1 px under (12 px `text.dim`: the start in Martian Mono 11
  px, " · ", then "dans 25 min", "dans 1 h 05", in `#8FB3FF` 600 from 10
  min, 700 from 2; in progress "fin dans 18 min" alone, blue, 600). In
  progress, a 2 px `#8FB3FF` line along its bottom, inside its border and
  cut round at its corners, to the share gone. With a video link, the join
  button: 30 × 30, radius 8, 6 px from the card's right (inside its
  border), the mockup's 16 px camera; `rgba(255,255,255,0.09)` and the
  camera in `text` more than 10 min ahead, `#5B8DEF` and white after.
  Under the pointer the card is lit `rgba(255,255,255,0.06)` (the button
  `0.14`, or `#6E9BF2`) and the pointer is a hand. Its colours change over
  300 ms, at once with reduced motion or on the CPU. It draws only when
  what it shows changes, under the pointer, and while its colours change.
- **The mark** (mini form): 30 × 4, radius 2, centred in a 48 × 16 button
  (lit `0.06` under the pointer, `0.08` while its popover shows):
  `rgba(143,179,255,0.60)`; 10 min or less `#8FB3FF` with an 8 px glow;
  in progress `#8FB3FF` from the left to the share gone over
  `rgba(143,179,255,0.25)`.
- **Its popover** (`popover/meeting.rs`), as the bar's others (§9): a
  150 ms hover or a click on the mark, 280 wide, its top on the mark's,
  14 px in, its parts 10 px apart: "PROCHAINE RÉUNION" / "NEXT MEETING"
  (or "EN COURS" / "NOW"; 11 px, 700, spaced 0.1 em, `text.faint`); the
  stripe beside the title (15 px, 700, two lines at most), "14:00 – 14:30
  · dans 25 min" (12.5 px, the times in Martian Mono 11.5 px), the link's
  address without its scheme with a 13 px camera, else the place with a
  pin (12.5 px, `text.faint`); **Rejoindre** / **Join** (34 px, radius 9,
  `#5B8DEF`, the camera and the word white 13.5 px 700) with a link;
  **Voir dans le calendrier** / **Show in the calendar** (30 px, radius 8,
  `rgba(255,255,255,0.06)`, 12.5 px 600 `text.dim`), which opens the
  calendar (Day.5) on today with the meeting chosen; a 1 px
  divider, then "ENSUITE" / "NEXT", the next event's start and its title
  on one line, when one is left. `Tab` goes through its buttons, `Enter`
  acts, `Esc` closes; a button acts and closes it.
- **Clicks**: the card (or `Enter` on it) opens the calendar (Day.5) on
  today with its meeting chosen (as built, Day.5). The join button (and
  Rejoindre) opens the video link (`video`: the event's conference data,
  else the first meeting address of its place or description, as its
  source finds it, day.md), through the launcher's `xdg-open`, which
  the compositor starts. `Tab` reaches the card, then its button, among
  the bar's items.
- **The halo** (§6): the shell tells the compositor `meeting ten` (10 min
  or less, priority 3), `meeting two` (2 min or less, priority 1) or
  `meeting none` as it changes, and again once linked; the compositor's
  halo shows it as built in 4.4 (its waves and glows), on every bar, the
  mini form's too. None once the meeting has started, and none for a
  meeting the user opened or joined from the card, its button or its
  popover (§6, "the user acting ends the state"). The compositor forgets
  it when the shell goes.
- **Names for screen readers** (§11), said in the log until iced has
  them: "Prochaine réunion : Point produit, 14:00, dans 25 minutes, visio"
  / "Next meeting: Point produit, 14:00, in 25 minutes, video call" ("Réunion
  en cours : … fin dans 18 minutes" in progress; the place instead of
  "visio" without a link); the button's "Rejoindre la visio" / "Join the
  video call".
- **Choices made here** (to confirm on the machine): the time line runs
  to 2 px from the join button, where the title stops 6 px before it:
  "14:25 · dans 25 min" is 112 px in these fonts and the title's room 108
  (the spec's "about 110"), and it would be cut; without a button the
  room is 144 (the spec's "about 140"). In progress the line is "fin dans
  18 min" alone, as the mockup draws it. The sign of what is in use goes
  under the card, the card staying right under the clock. Opening the
  meeting from the card (in the calendar) ends its halo as joining does.
  The popover's "Voir dans le calendrier" always shows (it showed only
  with a page until Day.5), its "ENSUITE" only when an event is left
  after it. The card's
  minutes count from the clock's minute, the meeting's start and end
  rounded to the nearest minute.
- **Measured headless** (`crates/hocus-compositor/tests/meeting.rs`): the
  real shell on the CPU, in French, its day a fixture's file and its
  clock `HOCUS_CLOCK_AT`, its `xdg-open` the test's own: the first
  meeting left takes the card ("14:25 · dans 25 min", its name and its
  button's), all-day, declined, free, cancelled and ended events skipped;
  its surface 194 × 54 under the clock, the stripe in the calendar's
  colour, the card's and the button's fills read on the screen, the
  agents' group 58 px lower (since #599 under Today, which is 14 px
  under the card); "dans 1 h 05"; within 10 min the card blue
  (its fill on the screen), the button `#5B8DEF` and the halo
  `MeetingSoon`, at the minute's change within 2 min, its fill stronger
  and the halo `MeetingNow`; in progress "fin dans 18 min", the line blue
  to 40 % of the card and not past it, no halo; at a meeting's end the
  next one at the minute's change, drawn once, no button without a link;
  the last one's end: no card, its surface gone, the agents' group 58 px
  higher; a click on the card opens the calendar on its meeting and ends
  the halo, on its button the call; in the mini form, the mark's glow and its 54 × 26
  surface, the agents' item 30 px lower, a hover opening the 280 px
  popover with what it says, Rejoindre opening the call and ending the
  halo; at rest nothing drawn for 3 s, the minute's change drawn once, a
  change of the day the card does not show drawing nothing. Unit tests
  for which meeting, the overlaps, the whole minutes, the looks and the
  halo's states, the halo ended once acted on, the words in both
  languages, the places, the colours, the button and the title's room,
  the popover's layout, and `Tab` from the card to its button. On the
  CPU, the screenshots of the pull request
  (`screenshots_for_the_pull_request`, `HOCUS_SHOTS=dir`).
- **Not built**: the secretary's meeting line in her briefing (4.6);
  names for screen readers (§11).

### Day.3, Today (2026-11-04)

What the approved Day.3 mockup shows, variant A (plain lines), with the
maintainer's spec (§4 group 4; issue #427). Its data is the day's
(docs/design/day.md), the same list as the next meeting's card (Day.2).

- **What it lists** (`crates/hocus-shell/src/today/mod.rs`): today's
  events that take time (`Event::takes_time`) and have not ended, in order
  of start (`meeting::today`), the card's own left out (`meeting::ahead`'s
  first). All-day events stay in Day.5's calendar; every calendar shown is
  in it, featured or not (Day.7), and a featured one's events look the
  same as the others'. Nothing after the card: no group. The mini form: nothing.
- **Where**: a surface of its own on each bar (`hocus-bar-today`),
  opened while it lists something and removed after, 14 px under the
  card, or the sign of what is in use (since #599 the first of the day's
  groups, To do and the agents' group under it; it was under the agents'
  group). Its room is what the spacer would have: down to 14 px over the
  desktops' group, less what To do and the agents' group take. When the lines do
  not fit, the last line that fits says how many more: "+ 5 autres
  aujourd'hui" / "+ 5 more today", "+ 1 autre aujourd'hui" / "+ 1 more
  today"; with no room for even one line (18 + 20 px), no group: Today
  gives way first (§4). It follows To do's and the agents' group's
  heights, the card and the sign coming or going, the desktops' rows and
  the bar's height.
- **The group** (`today/draw.rs`): its 18 px header "AUJOURD'HUI" /
  "TODAY" (Manrope 700, 11.5 px, spaced 0.1 em, `text.dim`), then 20 px
  lines, 6 px past the group's sides (radius 6, 6 px in): the start in
  Martian Mono 11 px `text.dim` in a 38 px cell, 7 px, a 7 px dot in the
  calendar's colour, 7 px, the title (Manrope 13 px, `text`, cut with
  "…"); "+ N" in 12.5 px `text.faint`. Under the pointer a line is lit
  `rgba(255,255,255,0.06)` and the pointer is a hand; resting 600 ms on an
  event shows "15:00–15:45 · Entretien — poste back-end" in the tray's
  tooltip (5.14), the times in bold, beside the bar on the line.
- **Time passing**: at each minute change the shell asks whether the
  card's meeting ended; if so the first line becomes the card: it fades
  out (200 ms) where it was and the lines under it move up 20 px (240 ms,
  `cubic-bezier(0.2, 0.9, 0.25, 1)`), a line counted in "+ N" fading in at
  the bottom; at once with reduced motion or on the CPU. Its lines say no
  minutes: a minute's change that changes no line draws nothing.
- **Clicks**: a line with a video link (found by its source, as for the
  card) opens it in the browser; else the calendar (Day.5) on today, the
  event chosen. "+ N" opens the calendar on today. `Tab` goes through the
  lines and "+ N" among the bar's items, `Enter` or `Space` acts.
- **Names for screen readers** (§11), said in the log: "15:00, Entretien —
  poste back-end, visio" / "15:00, Entretien — poste back-end, video
  call".
- **Choices made here** (to confirm on the machine): one 14 px gap over
  the desktops (the spacer then having nothing, its own gap not counted
  twice); with room for one line only and several events, that line is
  "+ N" alone, as the rule says; the tooltip is the tray's
  (bold times, then the title) rather than a new look.
- **Measured headless** (`crates/hocus-compositor/tests/today.rs`): the
  real shell on the CPU, in French, a fixture's day, `HOCUS_CLOCK_AT`, its
  `xdg-open` the test's own: the three events after the card in order,
  their names, all-day, declined, free, cancelled and ended skipped, its
  194 × 88 surface 14 px under the card (#599), each dot in
  its calendar's colour on the screen; nothing after the card, no group;
  40 events: the first ones, then "+ N" counting the rest, its bottom 14
  px or more over the desktops' group and one more line not fitting; on a
  1920 × 320 screen, no group; at the card's end the first line becomes
  the card at the minute's change, the group a line shorter; a click on a
  line with a link opens it, on one without the calendar on its event, on
  "+ N" the calendar on today; resting on a line, its tooltip
  ("15:00–15:45", "Entretien — poste back-end") beside the bar on it, the
  line lit, gone as the pointer leaves; the mini form, no group; at rest
  and through a minute's change that changes no line, no commit. Unit
  tests for what fits and "+ N", the words in both languages, the
  surface's size, the lines' motion, `Tab` through the lines and "+ N".
  On the CPU, the screenshots of the pull request
  (`screenshots_for_the_pull_request`, `HOCUS_SHOTS=dir`).
- **Not built**: names for screen readers (§11).

### Day.4, the to-do list (2026-11-04)

What the approved Day.4 mockup shows, variant C (A's group in the full
form, an item and its popover in the mini form), with the maintainer's
spec (issue #428).

- **Its place**: the full form's group 14 px under Today (Day.3) when it
  lists something, else where Today would be (under the card, the sign of
  what is in use, or the clock); the agents' group 14 px under it (#599;
  it was over it); Today keeps the room of the groups under it as it fits
  its lines; the mini form's item 14 px under the meeting's mark (or the
  sign, or the clock), the agents' item under it (#599). Its surface (`hocus-bar-tasks`, under the windows with the
  bar's others, 5.13) is the bar inside its rim, 5 px around what it
  draws for the keyboard's ring, as tall as the group (18 px, 22 px a
  line, 20 px for "+ N", 30 px for the field) or the item (34 px), one
  pixel taking no pointer when nothing shows. Today laid out again (or
  the meeting card, the sign of what is in use coming) moves it, and the
  agents' group under it. While the bar
  switches its form, it holds both forms, the two cross-fading.
- **What shows**: the day's tasks' account (`Day::tasks()`, docs/design/
  day.md): the cards of the board's tasks' column chosen in Day.1, else
  the local list, in the column's order. No board set and no local task:
  no group and no item. A board with nothing open: the header and "Rien
  à faire." (13 px, `text.faint`). The group shows 3 lines; "+ 4 autres"
  / "+ 1 autre" (12.5 px, `text.faint`, a button lit under the pointer
  6 px past its text) opens the tasks' popover. A title too long is cut
  with an ellipsis; resting 600 ms on it shows it whole in the bar's
  tooltip (5.14, as Today's lines), beside the bar.
- **Ticking**: a click on the box (or `Space`, `Enter` while it holds the
  keyboard) ticks it at once: `#6CC287`, a dark check (`#0b1020`), the
  title `text.faint` and struck through. The source is told at once
  (`day::change`, its account's worker off the loop). The line stays
  1.5 s, as the shell measures it; a second click within them unticks it
  and tells the source again (back where it was in its column). Then the
  line goes: with motion it fades and closes over 300 ms (`ease-in`), the
  lines under it moving up, the next task fading in at the bottom; with
  reduced motion or on the CPU, at once. A task the source no longer
  lists (ticked, its card moved to the done column) stays in its place
  for its 1.5 s; one unticked shows in its place until the source lists
  it again.
- **Refused**: a change the source refuses (offline, a conflict:
  `Pending::refused`) unticks the box (an untick refused ticks it) and
  says "Pas enregistré" / "Not saved" (12 px, `#F7A77A`) in the title's
  place for 3 s; the change stays queued, tried again when the source
  answers again (the workers' own retries).
- **Adding**: `+` (or `Enter` while it holds the keyboard) opens the field
  as the first line, 26 px with 2 px above and under, radius 7,
  `rgba(255,255,255,0.06)`, a 1 px `rgba(108,194,135,0.65)` border, 13 px,
  "Nouvelle tâche…" / "New task…" while empty, a caret (it does not
  blink: nothing draws at rest). The group's surface takes the keyboard
  (exclusively, while the field holds it). `Enter` adds the task at the top
  of the board's add column (`Change::Add`, no due date) and closes the
  field; empty, it adds nothing. `Esc` closes it; a click elsewhere closes
  it when empty, else keeps it and its text and lets the keyboard go; a
  click in it takes the keyboard again. `Ctrl+V` and `Shift+Insert` paste
  the clipboard's text at its end on one line (Day.4b, #574): each line
  break or run of white space one space, its ends trimmed, its other
  control characters dropped; whole or not at all (200 characters at
  most). A task added shows at the top, its box inert, until the source
  made its card; with 3 tasks shown, the third goes into "+ N".
- **The mini item**: 44 × 34, radius 10, the mockup's 16 px box and check
  (`text.dim`), 5 px before how many tasks are open (Martian Mono 11 px,
  `text`; no number at 0); lit `0.06` under the pointer, `0.08` while the
  popover shows. A 150 ms hover or a click opens the tasks' popover (§9's
  mechanics: the pointer leaving closes it 300 ms later, `Esc` closes).
- **The tasks' popover** (§9): 280 wide, the popovers' glass, 8 px left of
  the bar, its top on the item or the line that opened it. "À faire" (14 px
  bold), the count (12 px, `text.dim`), the `+`; the field as its first
  line (30 px, radius 8, 13.5 px); up to 6 tasks on 30 px lines (the box in
  a 26 × 26 button, the title 13.5 px), ticking as the group's; past 6 the
  wheel scrolls the list (a 6 px thumb) and "+ N autres" under it says how
  many are past the six. Opened by hovering, it takes the keyboard when
  its field opens. `Tab` goes through the `+` and each box, `Space` or
  `Enter` acts, `Esc` closes the field, then the popover.
- **The keyboard in the bar**: the `+`, each box and "+ N autres" are the
  bar's items: `Tab` and `Shift+Tab` (or the arrows) go through them with
  the bar's others, from the `+` down; `Space` or `Enter` on a box ticks
  it, on the `+` opens the field, which takes the keys (its ring stays on
  the `+`, which has the keyboard again as the field closes).
- **Names** (said in the shell's log, for screen readers to come, §11):
  "Ajouter une tâche" / "Add a task"; a box "Cocher « … »" / "Décocher
  « … »" ("Tick "…"" / "Untick "…""); the mini item "À faire : 7 tâches"
  / "To do: 7 tasks".
- **Waking**: the day's changes wake the group's surfaces and the open
  popover only when the tasks' account's tasks or pending changes
  changed (`day::on_change`); the
  1.5 s, the 300 ms and the 3 s are deadlines of the popovers' effects
  stream. At rest nothing wakes or draws.
- **Measured headless** (`crates/hocus-compositor/tests/tasks.rs`), the
  real shell on the CPU, a fixture's board of the test's own, in French:
  three tasks in the column's order and "+ 4 autres", the surface 18 + 3 ×
  22 + 20 px (and 5 px around) right under the clock (no meeting, no
  event, no agent; the agents' group under it since #599), a cut
  title whole in the tooltip, "+ 4 autres" opening the popover (6 rows,
  "+ 1 autre", 280 wide, its top on the line)
  (`the_group_shows_three_tasks_in_the_columns_order_and_the_rest_in_its_popover`);
  a tick reaching the fixture at once, the line going 1.5 s after as the
  shell measured it, the next task coming in
  (`a_tick_reaches_the_source_at_once_and_its_line_goes_after_1_5_s`); a
  second click within 1.5 s unticking, the card back first, the line
  staying (`a_second_click_within_1_5_s_unticks_and_tells_the_source_again`);
  a fixture refusing changes: the box unticked, "Pas enregistré" for 3 s,
  then the title, the change still pending
  (`a_refused_change_unticks_and_says_not_saved_for_3_s`); the field
  taking the keyboard, `Enter` adding at the top and the third going into
  "+ 5 autres", `Esc`, a click elsewhere on an empty field and on one
  with text, a click back in it
  (`enter_adds_the_task_at_the_top_and_esc_or_a_click_elsewhere_closes_the_empty_field`);
  `Ctrl+V` from a window's clipboard, "Appeler\nInès\n" pasted as
  "Appeler Inès" (`ctrl_v_pastes_the_clipboards_text_into_the_field_on_one_line`);
  the mini item 44 × 34 counting 7, its popover after a 150 ms hover as
  the shell measured it, a box ticked there counting 6, the pointer
  leaving closing it
  (`the_mini_item_counts_the_open_tasks_and_opens_the_popover_after_150_ms`);
  `Esc` in the popover giving the keyboard to "+ 4 autres", `Shift+Tab`
  up through the boxes to the `+`, `Tab`, `Space` ticking, `Enter` on the
  `+` opening the field
  (`tab_goes_through_the_add_and_each_box_space_ticks_and_enter_opens_the_field`);
  with a meeting card and two events, the group 14 px under Today
  (`the_group_goes_14_px_under_today_when_today_lists_something`);
  no board, no group (a one-pixel surface), an empty board "Rien à
  faire." (`no_board_no_group_and_an_empty_board_says_nothing_to_do`); at
  rest no commit, an event's change none either, a task's change one
  draw (`at_rest_the_list_draws_nothing_and_another_change_of_the_day_neither`).
  Unit tests for the group's and the popover's sizes, the `+`'s and the
  boxes' places, the words in both languages. On the CPU, the
  screenshots of the pull request.
- **Choices the spec did not settle**: the group shows when a board is
  set, or the local list has a task (hidden with only an empty local
  list, as no task source is chosen); a task added shows at once, its
  box inert, until its card is made; past 6 in the popover, "+ N autres"
  counts those past the first six and the wheel scrolls; the tooltip
  shows only for a title that is cut; an untick refused ticks the box
  again with "Pas enregistré"; the field takes 200 characters at most; a
  pasted line break is a space, not dropped as in the Wi-Fi's password
  (Day.4b, #574).
- **Not built**: names for screen readers (§11); the motion of the
  line's close is drawn only on a GPU, so the tests check its CPU path.

### Day.5, the calendar (2026-11-04)

Built in #429, variant A of the mockup (the month, then the day's list),
as the issue's last comment specifies it, with its decisions: no
calendars' colours in the month, no line for the time nor free gaps
(variant B), an event without a page of its own unfolding its details in
its row, the mini form's clock opening it on a click only.

- **Opening** (`crates/hocus-shell/src/clock/item.rs`,
  `popover::Item::Calendar`): a click on the clock's group, the date and
  the clock (172 × 77 in the full form, from the digits' cells to 6 px
  under the date; 48 × 46 in the mini, the two lines and 6 px), opens the
  calendar; a second click, a click outside or `Esc` closes it (§9).
  Resting on it opens nothing, in either form. The group is lit
  `rgba(255,255,255,0.06)` under the pointer, which turns into a hand,
  and `0.08` while the calendar is open, radius 10; ringed (2 px, the
  accent, inside it) while it holds the keyboard after `Esc`, where
  `Enter` or `Space` opens it again and `Tab` goes on to the next item of
  the bar. The clock's surface grew to hold the group (194 × 80, 54 × 46)
  and still lets the pointer through: the bar's own surface hears it for
  the group (`popover::BarPointer`), the clock telling the popovers where
  the group is as it draws. It always opens on today.
- **The popover** (`popover/calendar/`: `layout.rs`, `draw.rs`,
  `words.rs`): 320 wide, the popovers' glass, rim and shadow, padding 14,
  16 and 12 px, its parts 10 px apart; 8 px left of the bar, its top on
  the group's (22 px from the screen's top in the full form, 20 in the
  mini). Its height follows the day: six weeks always, the list 236 px at
  most (573 px with a full list). It moves in and out as the other
  popovers do, the compositor's (none with reduced motion or on the CPU).
- **The header** (28 px): the month and year (Manrope 700, 16 px, cut
  if it must be); "Aujourd'hui" / "Today" (26 px high, 10 px in, radius 7,
  `rgba(255,255,255,0.06)`, `0.10` under the pointer, 12 px 600,
  `text.faint` with today's month and today shown, else `text`); the two
  26 × 26 arrows (16 px chevrons in `text.dim`, lit `0.10`), 6 px apart.
  The arrows turn the month, not the day chosen; "Aujourd'hui" brings
  back today's month and today.
- **The month**: a 28 px column of ISO weeks (Martian Mono 10.5 px,
  `text.faint`), then seven equal columns, Monday first, under an 18 px
  row of "sem." / "wk" (10 px 600) and "L M M J V S D" / "M T W T F S S"
  (11 px 700, `text.faint`); six rows of 32 px, 2 px apart. A day is a
  34 × 30 button, radius 9, its number in Martian Mono 12.5 px (`text`,
  `text.dim` on weekends, `text.faint` for the months around), its 4 px
  dot 4 px under it when it has events (`rgba(238,241,246,0.6)`, around:
  `rgba(140,150,168,0.6)`), lit `0.06` under the pointer; today ringed
  1.5 px `#6CC287` inside; the day chosen filled `rgba(255,255,255,0.16)`
  (today's `rgba(108,194,135,0.24)`, its ring kept), its number white;
  the keyboard's day outlined 2 px `#EEF1F6`, 1 px outside, once a key
  moved it.
- **The day's agenda**: a 1 px `rgba(255,255,255,0.08)` divider, then the
  day's name (13.5 px 700, with its year when it is not this year's),
  "aujourd'hui" / "demain" / "hier" (12.5 px, `text.dim`) 8 px after it,
  the count on its right ("6 événements", "1 événement", 12 px,
  `text.faint`; nothing on an empty day), all on one baseline; the line
  may reach 14 px into the right padding before the name is cut, as the
  mockup's does ("Mercredi 4 novembre", "aujourd'hui" and "6 événements"
  are 299 px in 286). The all-day events first: 28 px rows, 4 px more
  under them, radius 8, tinted with their calendar's colour at 14 %, an
  8 px dot, the title (12.5 px 600, cut), "toute la journée" / "all day"
  or a span of days ("7 – 8 nov.", "30 nov. – 2 déc.") on the right
  (11.5 px, `text.dim`). Then the timed ones by start: rows of 44 px at
  least (49 with a line under the title), radius 9, 6 and 8 px in, the
  start (Martian Mono 12 px) 3 px over the end (10.5 px, `text.faint`) in
  a 42 px column (wider for 12 hours' times), a 3 px bar in the
  calendar's colour, the title (13.5 px 600, cut), under it the video
  link's host and path with a 13 px camera in `#A9C4FF`, else the place
  with a pin in `text.dim`; lit `0.06` under the pointer; today's events
  over at half their opacity. Times as the system's clock writes them:
  24 hours, but 12 ("9:30 AM") in `en_US`, `en_CA`, `en_AU`, `en_NZ`,
  `en_PH`, `en_IN`. Past 236 px the list scrolls (the wheel, a notch a
  44 px row; the keyboard as it goes to an event), with the popovers'
  6 px thumb 3 px past the content's right. An empty day: "Rien de prévu
  ce jour-là." / "Nothing planned that day." (13.5 px, `text.dim`,
  centred, 14 px above, 12 px under). No calendar account: the month,
  and "Aucun agenda branché." / "No calendar connected.".
- **Clicks**: a day chooses it, its agenda replacing the one shown; one
  of the months around also turns to its month. An event with a video
  link opens it (`xdg-open`, the launcher's), else its own page (Google's
  `htmlLink`), and the calendar closes; one with neither unfolds its
  details under its title, its row lit `0.05`: its time, its place (an
  all-day event's: a timed one shows it under its title already), its
  calendar's name and its description on four lines at most (12 px,
  `text.dim`, its HTML read as text); a second click folds it.
- **The keyboard**: as it opens, on today. On the days, `←` `→` a day,
  `↑` `↓` a week, `Page Up` `Page Down` a month (the same day, or the
  month's last), `Home` `End` the week's Monday and Sunday, the month
  turning past it; `Enter` or `Space` chooses the day. `Tab` goes
  through the arrows, "Aujourd'hui", the days (one stop) and each event,
  round; on an event `Enter` or `Space` acts as a click and `↑` `↓` go to
  the one before or after. `Esc` closes, the keyboard back on the clock's
  group. Names for screen readers, said in the log: "mercredi 4 novembre,
  6 événements, aujourd'hui" / "Wednesday 4 November, 6 events, today"
  ("…, rien de prévu" without events), the arrows' "Mois précédent" /
  "Previous month", "Mois suivant" / "Next month".
- **Data**: the day's (`day::snapshot()`), its window kept by the workers
  off the loop (2 months back to 12 ahead); the calendars hidden left out
  by `Day::events`, the cancelled events too (Google keeps them with
  their times). A change of the day draws the popover again while it is
  open (`day::on_change`), nothing while it is closed.
- **Months outside the window** (Day.5b, #578): as the popover opens on a
  month or turns to it, the months its six weeks reach that the window
  does not hold whole are asked of the calendars' accounts, once per
  month shown (`day::months`, `Workers::months`; never blocking: a
  message to each worker). Each worker reads those it has not read yet,
  off the loop, in one range per run of months (`CalendarSource::events`:
  Google's `timeMin`/`timeMax`, the fixture's file), keeps them with its
  cache and sends them in its next snapshot (`Snapshot::months`,
  day.md). Until every account has answered, a day of them reads
  "Chargement…" / "Loading…" (the note's place and size), and their
  days have no dots; then their dots and the day's agenda show. A month
  an account did not give (offline) reads "Ce jour n'est pas chargé." /
  "This day is not loaded.", and is asked again the next time the
  popover turns to it. Nothing is asked while it is closed, nor at rest.
- **For the other views** (`popover::calendar::open_on(bar, day,
  event)`): the calendar opens beside `bar`'s clock (its screen's name;
  else the first bar) on `day`, the event of that id chosen: it holds the
  keyboard and is scrolled to, unfolded when a click would unfold it;
  what was open closes, and the calendar open already turns to it. From
  within the popovers' controller (an item's click), as
  `popover::Effect::OpenCalendar`, carried out after it. The next
  meeting's card and its mini popover's "Voir dans le calendrier" open it
  on today with the meeting chosen (Day.2); Today's lines without a video
  link on their event, "+ N" on today (Day.3); a reminder's Voir on its
  meeting's day with the meeting chosen (Day.6). For tests and tools, the
  compositor's `day calendar DATE [EVENT]` line asks the same.
- **At rest** it draws nothing; while open, again as one of today's
  events ends (it greys out) and at midnight (today moves), nothing
  between.
- **Choices made here** (to confirm on the machine): an event's own page
  closes the calendar as its video call does; the list's 236 px hold four
  rows with a line under their title and a part of the fifth (the
  spec's "five rows" counts 44 px ones); the day's line reaching into the
  padding as the mockup's does; the year after a day's name of another
  year; `↑` `↓` between events; the 12 hours' locales; (Day.5b) the
  months of the six weeks asked for, not the month shown alone, so that
  the days around have their dots; "Chargement…" in the empty day's
  note; a month read kept until the window is listed whole (once a day),
  then read again only when the popover turns to it, its events shown
  meanwhile; at most 24 such months kept per account.
- **Measured headless** (`crates/hocus-compositor/tests/calendar.rs`):
  the real shell on the CPU, in French, a fixture's day and
  `HOCUS_CLOCK_AT` (Wednesday 4 November 2026, 09:47), its `xdg-open` the
  test's own: a click on the date opens it 320 wide at 8 px from the bar,
  22 px from the top, the group lit while it is open, a second click on
  the clock closes it and another opens it again; in the mini form
  resting on the clock opens nothing and a click opens it beside the 56
  px bar; today chosen with its rows in order, the hidden calendar's and
  the cancelled event left out, the one over greyed, the dots of the six
  weeks, today's green ring and a dot on the screen; another day, the
  day after ("demain"), an empty day, a day of the next month turning
  it; the arrows (their places), "Aujourd'hui"; a busy day's 236 px list
  and the wheel; an event's call, another's page, both closing it, one
  unfolding and folding its details; `Esc`, the keyboard on the group
  ringed, `Enter` opening it again, a click outside; the keyboard by day,
  week, month, the week's ends, `Enter`, `Tab` through the arrows,
  "Aujourd'hui", the days and an event, `Enter` on it; the entry point on
  a busy day's last event, scrolled to, then on another, unfolded; a
  month 14 months ahead (Day.5b,
  `a_month_outside_the_window_is_asked_for_as_it_turns_to_it_and_shows_once_loaded`,
  the fixture's listings outside the window taking 1 s): nothing read
  while closed, "Chargement…" and no dots, then its dots and its agenda,
  its six weeks' three months read at once, the month before reading
  only the one it adds, back to it shown at once and nothing read again,
  nor at rest; no calendar; at rest no commit for 3 s, a change of
  the day drawing it once while open and nothing while closed. Unit
  tests for the grid and ISO weeks, the rows' order and sizes, the list's
  scroll and what it reveals, the details, the notes, the keyboard's
  moves, the words in both languages and the 12 hours, the controller's
  item (a click only, `Esc`, `Enter`). The next meeting's and Today's
  tests open it from the card and the lines. On the CPU, the screenshots
  of the pull request (`screenshots_for_the_pull_request`,
  `screenshot_of_a_month_outside_the_window`, `HOCUS_SHOTS=dir`).
- **Not built**: names for screen readers given to them (§11, logged
  until iced has them).

### Day.6, a meeting's reminder (2026-11-04)

What the approved Day.6 mockup shows, variant B (a tab against the bar),
with the maintainer's spec (issue #430) and its decisions: reminders get
through do not disturb; on a shared screen the tab hides its title;
`Super+J` joins; 5 min before by default, and the events' own alarms.

- **When** (`crates/hocus-shell/src/reminder/mod.rs`): for each event that
  takes a slot of the day (`Event::takes_time`: timed, neither cancelled,
  declined nor free) of the featured calendars (`Day::featured`, Day.7),
  at its own earliest alarm
  (`Event::alarms`: Google's popup reminders, a fixture's `alarms`), else
  `[calendar] remind` minutes before its start (default 5; `0` turns the
  default off and keeps the events' own; `hocus_day::reminders`). The
  times are whole minutes, as the meeting card's: the start to the
  nearest, the clock's minute as it shows it, so that the tab and the card
  say the same minutes.
- **Off the loop**: a thread of its own (`reminders`) looks at the day's
  snapshot (`day::snapshot`, its window 24 h back to 400 days ahead) when
  the day changes (`day::on_change`), and waits for the next reminder (or
  a shown tab's next minute, or a meeting's end for the history) on the
  monotonic clock: the wall clock is read once as the next one is known,
  and again after a resume (logind, through `day::resumed`), a clock set
  (a `CLOCK_REALTIME` timer that the kernel cancels when the clock is set,
  on a thread of its own) or a time zone changed. Nothing wakes the
  shell's loop until a tab comes, changes or goes: the thread sends the
  layer-shell actions on a stream of its own, and the tab's `wake`.
- **A missed one**: a reminder whose time passed (asleep, or the shell
  started late) shows when the thread looks again, if the meeting has not
  been running for 10 min ("a commencé il y a 2 min").
- **The tab** (`reminder/draw.rs`): a surface of its own on each bar
  (`hocus-reminder-N`, one per tab; on the overlay layer, so that a
  fullscreen window does not cover it), 8 px left of the bar, its top on
  the meeting card's (108 px from the screen's top in the full form);
  in the mini form centred on the next meeting's mark, 8 px left of the
  mini bar. 340 × 44, radius 12, the bubbles' tint
  (`rgba(22,27,42,0.96) → rgba(14,17,28,0.97)`, opaque without glass), a
  1 px `rgba(143,179,255,0.55)` rim; on a GPU its shadow `0 10px 30px
  rgba(0,0,0,0.40)` and glow `0 0 20px rgba(143,179,255,0.22)` drawn in
  the room around it (its surface takes the pointer on the tab only), on
  the CPU the tab alone, its corners cut by the compositor. 12 px in on
  the left, 6 on the right: the title (Manrope 700, 13 px, cut with "…")
  over the line (11.5 px `#A9C4FF`): "14:30 · dans 5 min · visio" / "in
  5 min · video", "dans 1 min", "maintenant" / "now", "a commencé il y a
  4 min" / "started 4 min ago", the place instead of "visio" without a
  link. Then its 30 px buttons, radius 8, 6 px apart: **Rejoindre** /
  **Join** (a 14 px camera, 12.5 px 700 white on `#3F6FD8`, `#4A7BE6`
  under the pointer) with a link, else **Voir** / **Open**
  (`rgba(255,255,255,0.06)`, `0.12` under the pointer, in a 1 px
  `rgba(255,255,255,0.14)` rim); **Later**, a 30 × 30 clock
  (`rgba(255,255,255,0.06)`, its icon `#C9D0DC`), "Me le redire 1 min
  avant" / "Remind me 1 min before" until 1 min is left, then "Me le
  redire à l'heure" / "Remind me when it starts", gone once the meeting
  started; **✕** "Ignorer ce rappel" / "Dismiss this reminder", 30 × 30,
  `text.faint`. The icon buttons are lit `0.10` under the pointer and say
  their names after 500 ms in the tray's tooltip (5.14), under the tab.
- **Motion**: the compositor moves each tab as a bubble
  (`hocus_link::slides_in`): 26 px from the bar's side and fading in over
  220 ms (`cubic-bezier(.2,.8,.2,1)`), out over 170 ms (`ease-in`, the
  shell saying `closing`, then removing it); a fade only with reduced
  motion or a shell on the CPU. It draws when what it shows changes (at
  most once a minute while it shows, for "dans N min") and under the
  pointer; nothing at rest.
- **Clicks**: Rejoindre opens the link with the default handler (the
  launcher's `xdg-open`), the tab goes and the halo's meeting state ends
  (`meeting::acted_on`); Voir, or a click on the text, opens the calendar
  (Day.5) on the meeting's day with the meeting chosen, the tab goes and
  the halo ends; Later takes it away until 1 min before, then until the
  start, the halo going on; ✕ takes it away for this meeting and ends its
  halo, so that the 2-min wave does not run. The card stays in each case.
  Untouched, it stays until 10 min after the start, then goes to the
  notifications' history, seen, as "Point produit · a commencé à 14:30" /
  "… · started at 14:30" from "Agenda" / "Calendar", its link its action
  (and its click) until the meeting ends, then none.
- **`Super+J`** (`[shortcuts] join`): the compositor tells the shell
  (`join`), which joins the meeting whose tab shows (the first with a
  link), else the one with a link starting next within 10 min, its tab
  not coming after, of the featured calendars too; with none, nothing
  (said in the log).
- **Do not disturb** holds the bubbles, never a tab. **A screen shared**
  (5.11): the compositor says which screens are, by connector
  (`shared-screens`, with `shares`); on them the title reads "Réunion" /
  "Meeting". **Full screen**: the compositor says which screens' front
  window is full screen (`fullscreen`, when it changes); on them the tab
  shows 8 px from the screen's edge, at the same height.
- **Several**: a tab each, 6 px apart, the earliest on top. An event
  changed while its tab shows updates it in place; cancelled, declined or
  removed, its tab goes; moved, its reminder comes anew.
- **Names for screen readers** (§11), said once in the log as a tab comes:
  "Point produit, dans 5 minutes, visio" / "…, in 5 minutes, video call".
- **Choices made here** (to confirm on the machine): free events get no
  reminder (as day.md's `takes_time` says, like the card); the tab shows
  on every bar, as the card does; in the mini form it is centred on the
  mark; on a shared screen only the title is replaced, its line kept;
  `Super+J` joins the next meeting again if it was already joined; what
  was dismissed is kept in memory
  only (a restart of the shell shows a due tab again).
- **Measured headless** (`crates/hocus-compositor/tests/reminders.rs`):
  the real shell on the CPU, in French, a fixture's day, `HOCUS_CLOCK_AT`,
  its `xdg-open` the test's own: the tab at 14:25 for 14:30, its 340 × 44
  surface 8 px left of the bar at the card's top, Rejoindre's blue on the
  screen, its name announced, the halo the card's; at the event's own
  alarm (15 min); none for a declined, a cancelled or an all-day event;
  Join opening the link, the tab gone, the halo ended, the card kept;
  `Super+J` with the tab shown, with none and a meeting within 10 min, and
  nothing 15 min ahead; resting on ✕, its name under the tab, centred on
  it, gone with the press; ✕ ending the halo; Later until 1 min before, the
  halo on; a 32 min sleep (`day slept`) showing the meeting started 2 min
  ago and not the one that ran 12; do not disturb on (a private bus); the
  title "Réunion" on a shared screen and back; at the screen's edge with
  full screen and back; to the history 10 min after the start, its link
  kept; the mini form's place; at rest, nothing looked at nor drawn
  until the reminder is due, then one look, and nothing drawn after
  within its minute. Unit tests for when it comes, stays and goes, a
  missed one, Later, joined or dismissed, two at once, a change and a
  cancel, `Super+J`, the words in both languages, the places, the
  buttons' layout. On the CPU, the screenshots of the pull request
  (`screenshots_for_the_pull_request`, `HOCUS_SHOTS=dir`).
- **Not built**: the tab's keyboard (`Super+B` reaching the bar, then
  `Tab` through its buttons): the bar has no keyboard entry yet
  (`Super+B` switches its form); calendars "without reminders" that still
  take the card (Day.1's settings; since Day.7 a calendar not featured
  has neither); the halo on the CPU.

### Day.1, choosing the day's sources (2026-11-04)

What the approved Day.1 spec says, variant B (step by step, from the
bar), with its two amendments (Google through GNOME Online Accounts, a
Planka board through a pasted token; issue #425). The data under it is
docs/design/day.md's; the Settings page (sprint 6), CalDAV and `.ics`
are not part of it.

- **The first-run card** (`crates/hocus-shell/src/setup/card.rs`): while
  `[day]` sets no `[[day.calendars]]` nor `[[day.tasks]]` and the user has
  not said "Plus tard", in the full form, where the day's groups go
  (Today and To do give it their place: it is drawn on the to-do list's
  surface, `hocus-bar-tasks`, where Today would start; the agents' group
  under it, #599). 172 px, radius 10,
  `rgba(255,255,255,0.05)`, a 1 px `0.08` rim, padding 10, 8 px gaps: the
  18 px calendar in `#8FB3FF` and "Ta journée ici" (13.5 px, 700); "Branche
  ton agenda et tes tâches : la barre montrera tes réunions et ce qu'il
  reste à faire." (12 px, `text.dim`, 1.45 lines); "Choisir…" (30 px, radius
  8, `#6CC287`, `#0b1020` 12.5 px 700, lit `#7DCF97` under the pointer and
  while its popover shows); "Plus tard" (24 px, transparent, `text.faint`,
  lit `0.06`). Both are the bar's items: `Tab` reaches them, `Enter` acts.
  Not before both are known (the configuration read, "Plus tard" read),
  so that it never shows for a moment. The mini form shows none.
- **"Plus tard"** removes it for good: `$XDG_STATE_HOME/hocuspanda/day-sources`
  (written off the loop) keeps it, read as the shell starts. Day.5's
  calendar still offers the popover: with no source, "Choisir des
  agendas…" (30 px, radius 7, 12 px 600, under "Aucun agenda branché.";
  `Tab` reaches it) opens step 1 in the calendar's place.
- **The popover** (`crates/hocus-shell/src/popover/setup.rs`): 320 wide,
  the popovers' glass, padding 12 px at the top, 14 px on the sides and
  the bottom, its parts 12 px apart, 8 px left of the bar, its top on the
  card's (on the clock's from the calendar), pushed up to stay 8 px from
  the screen's bottom; it grows and shrinks with its step. Its 28 px
  header: the back arrow (26 × 26, "Retour") from step 2 on, the step's
  title (15 px, 700), ✕ ("Fermer").
  - **Step 1, "Ajouter un compte"**: rows of 54 px at least, radius 10,
    `rgba(255,255,255,0.03)` (`0.06` under the pointer), 8 px apart: a 32
    px tile (radius 8), the kind's letter in its tint on 16 % of it (G
    `#8FB3FF`, P `#6CC287`), the name (13.5 px, 600), its line (12 px,
    `text.dim`, on 16 px lines, wrapped: the row grows), a chevron. Google's
    line is "Tes réunions et rendez-vous." and under it, in the same row, one
    34 px line per Google account of Online Accounts whose calendar is on
    (its e-mail, 13 px, a chevron), each going straight to step 3. None:
    "Ajoute un compte Google dans les comptes en ligne", the row running
    `gnome-control-center online-accounts` (through the compositor, as the
    applications are) when it is in `PATH`, else inert; Online Accounts is
    followed while the popover is open, so that an account added there
    shows by itself. Without Online Accounts on the bus (or no bus):
    "Comptes en ligne de GNOME absents", the row at 50 % and inert. Then
    "Tableau Planka" ("Tes tâches : les cartes d'une colonne d'un
    tableau."), and under the rows "Tes tâches restent sur cet ordinateur
    tant qu'aucun tableau n'en fournit." (12 px, `text.faint`).
  - **Step 2, "Tableau Planka"**: "Adresse du tableau" and "Jeton d'accès"
    (12 px, 600, `text.dim`, 6 px over their field), two 34 px fields
    (radius 8, `rgba(255,255,255,0.05)`, a 1 px `0.14` rim, green while it
    holds the keyboard, `#F2945B` when what went wrong is about it; 13 px,
    10 px in, the end of a long text in view). The token is a `Secret`
    of 4096 characters at most, shown as dots, zeroed as the popover
    closes; `Ctrl+V` and `Shift+Insert` paste the clipboard's text in
    either field (3.5b: the address on one line, the token without its
    line breaks). The help line: "Dans Planka, une fois connecté : outils du
    navigateur (F12) › Stockage › Cookies : copie accessToken et
    httpOnlyToken." Then "Annuler" (back to step 1) and "Se connecter"
    (green, inert until both fields hold something; "Connexion…" while it
    checks), 32 px, side by side. What went wrong, in 12.5 px `#F7B98A`,
    the fields keeping what was typed: "Jeton refusé ou expiré : colle un
    nouveau jeton." (`401`); "Aucun serveur Planka ne répond à cette
    adresse." (no answer, a `5xx`, or not Planka's API); "Ce tableau
    n'existe pas ou ne t'est pas partagé." (`404`, `403`); "Colle l'adresse
    d'un tableau : https://…/boards/…" (not an address).
  - **The check**, off the loop (`hocus_day::planka::find`): `GET
    /api/users/me` with the token, then `GET /api/boards/{id}` (its columns
    and how many cards each holds). A server's address alone lists the
    user's boards (`GET /api/projects`), and step 3 then has a "Tableau"
    menu first, a board chosen being read the same way.
  - **Step 3, "Quoi montrer ?"**: the account's name and address (12.5 px,
    `text.dim`: "alex@example.org · Google Agenda", "Alex ·
    planka.example.org"). For Google, "AGENDAS" and one 32 px row per
    calendar, as Online Accounts' token reads `calendarList`: a 16 px box
    (`#6CC287` and a dark check when ticked), a 16 px swatch, a 16 px star
    10 px after it ("Mettre en avant" / "Feature", Day.7: filled the eight's
    yellow `#F5C46E` when featured, else a 1.3 px `rgba(255,255,255,0.38)`
    outline; at half strength on a calendar unticked, where it counts for
    nothing; a click, or `Space` on it, lights it or puts it out), the name
    (13 px, `text`, `text.dim` when unticked), "lecture seule" (11.5 px,
    `text.faint`) on the right. Ticked at first: those the account marks
    selected; every star lit. The swatch unfolds under its row the eight colours (20 px, 6
    px apart; the chosen one ringed `text`), the account's own matched to
    the nearest; a click chooses and folds. For a board, "TÂCHES", its name
    (13.5 px, 600), then "Colonne des tâches" (TODO by name, else the
    first), "Colonne des terminées" (DONE, else the last), "Ajout dans" (the
    tasks' column, following it until chosen apart), each a 32 px row
    whose menu unfolds the board's columns (28 px, the chosen one with a
    green check), and how many cards the tasks' column holds ("2 cartes").
    "Terminé" (34 px, green) ends; ✕ here keeps the account as ticked.
  - **The keyboard**: `Tab` through the rows, the fields and the buttons
    (✕ last), `↑` `↓` through a list's rows, `Space` ticks, `Enter`
    chooses (in a field, signs in), `Esc` folds what is unfolded, goes back
    a step, then closes (the keyboard on "Choisir…").
- **"Terminé"**, off the loop (`day: sources`): the choice written into
  the configuration file the compositor names (`HOCUS_CONFIG`) with
  `toml_edit` (`hocus_day::edit`: its other sections, comments and order
  kept; through a symbolic link, with its mode; beside it, then renamed
  over it); a Google account's `[[day.calendars]]` (`account`, `shown`,
  `featured`, the ticked calendars whose star is lit, `colors`, replacing
  the entry that names it), a board's `[[day.tasks]]`
  (its `address`, `tasks`, `done`, `add`, replacing the first). The token
  is kept by `hocus_day::secrets` under the board's account (the keyring
  on the bus given, else its 0600 file), never in the file. Then the day's
  workers start again from the file (`Workers::reconfigure`), the card
  goes, and the bar shows the day (the meeting card, Today, To do).
- **Names for screen readers** (§11), logged: the header's "Retour" and
  "Fermer"; the card's buttons are the bar's items.
- **At rest** nothing draws: no spinner turns on the CPU or with reduced
  motion, the caret does not blink, Online Accounts is followed on its
  signals only.
- **Choices the spec did not settle** (to confirm on the machine): the
  card on the to-do list's surface, in the full form only; the Google
  row's accounts listed inside it (34 px lines), its header inert then;
  no account and no `gnome-control-center`: the row inert; the tiles'
  tints; Planka's user name and password fields left out (the token
  only: Planka's SSO cannot be told apart on 2.2, the brief asked the
  token); the address error's words; a calendar list that fails to read:
  "Les agendas de ce compte ne se lisent pas. Réessaie."; "Ajout dans"
  following the tasks' column; every calendar's colour written in
  `colors` (the eight), by name; a press outside, or `Esc`, in step 3
  keeps nothing (only ✕ and "Terminé" do); the popover's lists do not
  scroll (it grows with them); no spinner beside "Lecture des agendas…".
- **Measured headless** (`crates/hocus-compositor/tests/day_sources.rs`),
  the real shell on the CPU, in French, its configuration file and
  folders the test's own, Online Accounts on a private bus, a Calendar API
  and a Planka server on 127.0.0.1, `gnome-control-center` a script:
  `the_first_run_card_shows_only_without_a_source` (and not with a source,
  after "Plus tard", in the mini form);
  `later_removes_the_card_for_good` (the file, a restart of the shell);
  `choose_opens_step_1_beside_the_card_without_online_accounts_google_is_disabled`
  (320 wide, its top on the card's);
  `the_google_row_lists_online_accounts_and_step_3_its_calendars_and_colours`
  (the nearest colours, read-only, a colour chosen, a box unticked, the file
  written with its other sections, the account's events in the day);
  `step_3s_stars_feature_calendars_and_done_writes_featured_keeping_the_files_other_lines`
  (Day.7: every star lit at first, one put out and lit again, `featured`
  written beside `shown`, the rest of the file kept);
  `without_an_account_the_google_row_opens_online_accounts_and_comes_back_with_one`;
  `the_planka_step_takes_a_pasted_token_and_says_a_refused_one_no_server_or_a_board_not_shared`
  (`Ctrl+V`, the token never logged nor in the file, the columns' menus,
  the board's tasks in the bar); `a_servers_address_alone_lists_its_boards`;
  `tab_goes_through_the_rows_and_esc_goes_back_a_step_then_closes`;
  `at_rest_the_card_and_the_popover_draw_nothing`. In `calendar.rs`,
  `with_no_calendar_the_agenda_says_none_is_connected_and_offers_day_1s_popover`.
  Unit tests: the card's layout, the steps' layouts (step 3's star after
  each swatch, `step_3_has_a_star_after_each_calendars_swatch`), the nearest colour,
  the first columns, the refusals, the words in both languages
  (`hocus-shell`); the configuration edited and written
  (`hocus_day::edit`), the workers started again (`hocus-day`).
- **Not built**: the Settings page (sprint 6); Planka's own sign-in by
  user name and password; CalDAV and `.ics`; names for screen readers
  given to them (§11).

### Day.7, featured calendars (2026-11-05)

Asked by the maintainer (issue #607): following 5 or 6 calendars, only 2
matter for what is about to start; a colleague's leave took the meeting
card.

- **Featured or not**: each shown calendar is featured or not
  (`[[day.calendars]] featured = ["name", …]`, by name or id as `shown`
  is; without it, every shown calendar is, as before; docs/design/day.md,
  "The configuration"). The rule is in one place, `hocus_day::Day::featured`
  (`Shown::featured`).
- **What follows it**: the meeting card (Day.2) and the mini form's mark
  and popover, the halo's meeting states (`meeting ten|two|none`, §6), the
  reminders (Day.6) and `Super+J`. Today (Day.3) and the calendar (Day.5)
  show every shown calendar's events, featured or not, alike; an event of
  a calendar not featured that comes before the card's meeting is one of
  Today's lines.
- **Day.1's step 3** has a star per calendar after its swatch ("Mettre en
  avant" / "Feature"), which "Terminé" writes as `featured` (Day.1 above).
- **Measured headless** (`crates/hocus-compositor/tests/featured.rs`), the
  real shell on the CPU, in French, a fixture's day of two calendars
  ("Travail", featured, and a colleague's leave, "Congés", whose event
  starts first, in 3 min): `an_event_of_a_calendar_not_featured_takes_no_card_reminder_nor_halo_and_still_shows_in_today_and_the_calendar`
  (the card the featured meeting's, 35 min ahead, no halo, the next
  reminder the featured meeting's, `Super+J` joins nothing, the leave in
  Today and in the calendar);
  `without_featured_every_shown_calendar_feeds_the_card_the_reminders_and_the_halo`
  (the leave takes the card, the halo, a reminder and `Super+J`). Day.1's
  in `day_sources.rs` (above).
- **Choices made here** (to confirm on the machine): every star lit as
  step 3 opens (as `featured` unset is); `featured` always written, as
  `shown` is; a star on a calendar unticked drawn at half strength and
  not written; the mini popover's "ENSUITE" (the event after the card's)
  of the featured calendars too.

### The column's order, the agents under To do (2026-11-04)

Asked by the maintainer (issue #599): in the bar, everything about the
agents goes under the to-do list, in both forms.

- **The order** (`crates/hocus-shell/src/column.rs`): one list, top to
  bottom, of the bar's surfaces under the clock: in the full form the
  next meeting's card (Day.2), the sign of what is in use (5.12), Today
  (Day.3), To do (Day.4), the agents' group (4.0 to 4.6); in the mini
  form the meeting's mark, the sign, the to-do item, the agents' item
  (4.7). Every surface's top is the column's start (100 px under the
  bar's top in the full form, 68 in the mini) and what the surfaces
  above it in that list take, each its height and 14 px, none for one
  that does not show; no surface places itself from the one above it.
  When one of them comes, goes or changes height, or the bar changes
  height or form, the column puts every one of them in its place again,
  in an order where each reads what it depends on (`column::moved`).
- **Room on a short bar** (`column::room`): the column ends 14 px over
  the desktops' group. The groups give way in the list `GIVES_WAY`:
  Today first (its lines, then the group, as in Day.3), then To do's
  lines (into its "+ N autres", its header staying: at the least its
  header and "+ N", or "Rien à faire."), then the agents' rows (into
  "+ N autres", at the least the header and "+ N"). Today's room is what
  To do and the agents' group leave; To do's ignores Today and counts the
  agents' group as it is; the agents' ignores Today and counts To do at
  its least.
- **What follows the group**: the peek's panda (4.5) comes out beside
  its agent's row where the column puts the group, and moves with it
  while out; the secretary's briefing (4.6, 4.7) opens with its top on
  her row (or her mini item), as before; Sources and the consent popover
  keep their place beside the group, as high above its top as the
  mockup had them (62 px under the bar's top with the group at 100).
- **On the CPU** the agents' and the to-do list's surfaces fill with the
  bar's colour where the column has them as they draw, and draw again as
  they move (they used to keep the colour of where they first drew).
- **Choices the issue did not settle** (to confirm on the machine): the
  sign of what is in use stays right under the card, over Today (the
  issue's order); the agents' group is laid out once for every bar, so
  that on several screens the shortest bar in the full form sets how many
  rows show; the group numbers of §4 stay as they were (the code and the
  other documents name them), the table now in the column's order;
  without a calendar or a task source, Day.1's first-run card, in the
  day's groups' place, has the agents' group under it too.
- **Measured headless** (`crates/hocus-compositor/tests/bar_order.rs`),
  the real shell on the CPU, in French, a fixture's day and tasks,
  `HOCUS_CLOCK_AT`, its agents said with `hocus agent`, in a home of the
  test's own: the card, Today, To do and the agents' group in that order,
  14 px apart, each at its height, the desktops further down
  (`the_full_form_goes_card_today_to_do_then_the_agents_14_px_apart`);
  without a meeting, To do and the agents under the clock
  (`without_a_meeting_to_do_and_the_agents_start_under_the_clock`); an
  empty Today (`with_an_empty_today_the_agents_come_under_to_do_under_the_card`);
  no task source (`with_no_task_source_the_agents_come_under_today`); the
  mini form's mark, to-do item and agents' item, 14 px apart, with and
  without a meeting and with no task source
  (`the_mini_form_goes_mark_to_do_item_then_the_agents_item`); a waiting
  agent's row heading the group and the peek's panda on its middle,
  beside the bar (`the_peek_comes_out_beside_its_agents_row_under_to_do`);
  her briefing's top on her row
  (`her_briefing_opens_beside_the_agents_group_under_to_do`); on a 1920 ×
  590 screen no Today, To do's three lines and the agents' three rows, on
  a 1920 × 470 one To do's header and "+ 5 autres" and the agents' rows
  as many as fit over "+ N", 14 px or more over the desktops
  (`on_a_short_screen_today_gives_way_then_to_dos_lines_then_the_agents_rows`).
  Unit tests: the order, the tops and the room shared (`column`), the
  agents' rows and To do's lines that fit. On the CPU, the screenshots of
  the pull request (`HOCUS_SHOTS=dir`).

### 7.4, the session menu (2026-11-04)

The approved spec is issue #415's last comment (variant A, mockup
<https://claude.ai/artifact/YYrHQz2rMPhjogPJMqPSZw>), with its decisions:
`Super+Shift+E` opens the log-out confirmation; a window that does not
close within 5 s is listed as holding it back, with "Revenir à …" and
"Fermer quand même"; restart and shut down that need the password open
7.5's dialog as they are chosen; the avatar is 7.1's.

- **Who does what.** The compositor decides and keeps the time
  (`crates/hocus-compositor/src/leave/`): the countdown, the windows asked
  to close and the 5 s they are given, on the motions' clock
  (`State::now`, which tests hold); logind on a thread of its own with its
  runtime (`leave/logind.rs`), never on the loop. The shell draws what it
  is told in the user's popover (`crates/hocus-shell/src/popover/session/`)
  and says what is chosen. The lines are `hocus_link::leave`'s:
  `leave-can`, `leave OUTPUT ACTION STEP…`, `leave-end`; from the shell
  `leave open`, `leave ask ACTION OUTPUT`, `leave suspend`, `leave now`,
  `leave cancel`, `leave force WINDOW`.
- **logind's bus** is the one given with `--system-bus ADDRESS`, else the
  system's on a virtual terminal only. Nested or headless without it,
  there is no logind: suspend is hidden, restart and shut down greyed, and
  the developer's machine is never asked to sleep or power off.
- **The menu**: 280 wide, the header 40 px (the avatar 32 px: the
  account's photo from AccountsService's `IconFile` as 7.1 reads it, else
  the panda on the accent's gradient; the name, Manrope 700, 16 px), then
  6 px and the rows, 44 px each, their 30 px tile (radius 8, `0.06`), its
  17 px glyph (1.9 px strokes, `#C9D0DC`), the label 14 px 600, "Super+L"
  on Lock's right (Martian Mono 11 px, `text.faint`), lit `0.06` under the
  pointer: "Réglages" (6.1), a divider, "Verrouiller", "Mettre en
  veille", a divider, "Se déconnecter", "Redémarrer", "Éteindre". 364 px
  tall with every row (the spec's 305, "Réglages" and its divider, the
  rim). `CanSuspend` read again as it opens hides suspend when it says no;
  `CanReboot` and `CanPowerOff` saying no grey their rows
  (`rgba(255,255,255,0.28)`, inert); `challenge` leaves them as they are.
  The footer's avatar is the photo or the panda too (no longer the
  initial).
- **Lock** (and `Super+L`, a new `[shortcuts] lock`): `lock now`, the
  popover closes. **Suspend**: the session locks, and logind's
  `Suspend(true)` is called once every screen shows the lock
  (`State::lock_drawn`), 2 s at most.
- **The confirmation**, in place of the rows, the header kept: the amber
  tile and the title (15 px), the sentence ("L’ordinateur s’éteint dans
  10 s.", the number each second), the 4 px bar (draining smoothly on a
  GPU with motion, a step a second on the CPU or with reduced motion),
  "Éteindre maintenant" (`#F5C46E`) and "Annuler", 238 px tall, growing
  upward from the bar's bottom. The keyboard is on "Annuler"; `Tab` to "…
  maintenant"; `Esc` cancels, `Esc` again closes; a click outside, or the
  popover closing, cancels. Chosen from a hover, the popover is held as a
  click holds it: the pointer leaving does not close it. `Super+Shift+E`
  opens it on the log-out confirmation, on the focused window's screen
  (else the pointer's); without a shell, it logs out at once as before.
- **The password first**: when logind says `challenge`, the compositor
  asks polkit (`CheckAuthorization` with `AllowUserInteraction`, the
  action logind checks: `…reboot` or `…power-off`, their
  `-multiple-sessions` forms when another user has a session) before the
  countdown; the session's agent (7.5) shows its dialog. Refused, the menu
  shows its rows again. The call itself is `interactive` too, so polkit
  asks again if the first answer is not kept (`auth_admin`).
- **What holds it back**: logind's inhibitors in `block` mode holding
  `shutdown` (`ListInhibitors`, read as the confirmation opens and every
  second while it shows: logind says nothing when one comes or goes),
  their `Who` and `Why`. `delay` ones are not listed: logind waits for
  them itself. While one holds, the list shows ("Firefox retient
  l’arrêt", the spec's sentence, a block each: the icon of its desktop
  entry or its letter, its name, its reason, "Revenir à Firefox", which
  brings the window of its process and closes the menu), "Éteindre quand
  même" and "Annuler", and nothing counts; when the last lets go, the
  countdown starts from 10 s. Logging out has no logind inhibitor; the
  desktop portal's `Inhibit` is not served yet (docs/design/idle.md).
- **Ending**: at 0, or "… maintenant", every window (Wayland's
  `xdg_toplevel.close`, X11's `WM_DELETE_WINDOW`) is asked to close, and
  given 5 s ("Fermeture des applications…"). Once none is left it
  happens at once. Those still open are then listed as holding it back
  (its title in its reason: "« Sans titre » ne s’est pas fermée.", or
  "Sa fenêtre ne s’est pas fermée." without one), with
  "Revenir à …" and "Fermer quand même", which cuts that window's client
  off (an X11 window's process is killed); "Se déconnecter quand même"
  goes on without them. Log out ends the compositor (`Ending::LoggedOut`)
  and the session script after it; restart and shut down call logind's
  `Reboot(true)` or `PowerOff(true)` and then end the compositor
  (`Ending::Restarted`, `Ending::PoweredOff`). logind refusing (polkit
  said no, an inhibitor) logs it and shows the rows again.
- **The confirmation goes** when the session locks (by the idle, a
  locker), or when the shell goes: nobody could cancel it any more.
- **Choices made here** (to confirm on the machine): "Réglages" first,
  over its own divider; the "Fermer quand même" button beside "Revenir à
  …" in a window's block (and the list's "… quand même" button for them
  all); "Ton mot de passe est demandé." while polkit asks, "Fermeture des
  applications…" while the windows close, "Extinction…" (the mockup's)
  once logind has it; the amber tile beside the list's title too; the
  inhibitors read every second while the confirmation shows rather than
  on logind's property changes.
- **Measured headless** (`crates/hocus-compositor/tests/session_menu.rs`),
  the real shell on the CPU, in French, its directories the test's own,
  the compositor's system bus a private `dbus-daemon` with a fake logind
  and polkit that record what they are asked:
  `the_popover_shows_the_menus_rows_under_the_avatar_and_the_name` (and
  suspend hidden, restart and shut down greyed as logind says);
  `verrouiller_locks`, `super_l_locks`;
  `mettre_en_veille_locks_then_suspends` (once the lock shows);
  `eteindre_counts_ten_seconds_down_then_powers_off` (the held clock, the
  sentence's seconds, the bar's pixels draining, `PowerOff(true)` at 10 s
  and not before); `annuler_stops_the_countdown`;
  `eteindre_maintenant_acts_at_once`; `redemarrer_maintenant_reboots`;
  `an_inhibitors_reason_shows_and_holds_the_countdown` (a `delay` one not
  listed, 12 s without a call, the countdown from 10 s once it goes);
  `a_window_that_does_not_close_is_listed_and_fermer_quand_meme_closes_it`;
  `super_shift_e_opens_the_log_out_confirmation`;
  `logging_out_ends_the_compositor_once_the_windows_close`;
  `the_keyboard_lands_on_annuler` (a stray `Enter` ends nothing);
  `a_restart_that_needs_the_password_asks_polkit_first`;
  `at_rest_the_menu_draws_nothing`. In `tests/session.rs`,
  `super_shift_e_logs_out` without a shell. Unit tests: the lines read
  back (`hocus-link`); the inhibitors kept and polkit's actions
  (`leave/logind.rs`); the rows, the confirmation's and the list's
  layouts, the holders' names, the words (`hocus-shell`).

### The windows in the bar (2026-11-05)

Asked by the maintainer (issue #604): the windows of the screen in the
bar, with a ✕ each. The approved spec is the issue's last comment:
variant B (over the desktops) of the draft, changed by the maintainer
from the focused window's controls into a list of the screen's windows.
The draft's mockup: https://claude.ai/artifact/PaF3yATgWkqw93aJYYSnmF.

- **What it lists** (`crates/hocus-shell/src/windows`): the windows of
  the bar's screen's current desktop, the focused one first, then by
  most recent focus (the compositor's own order, Alt+Tab's), at most 3;
  minimised ones too, greyed and their icon at half, as their desktop's
  list has them (3.9c). Past 3, a 20 px "+ N autres" / "+ N more"
  (12.5 px, `text.faint`) opens the current desktop's list (3.9b, every
  window of the desktop) beside it, until the overview comes (8.5). No
  window on the desktop: no group.
- **A line**: 30 px, 6 px past the group's sides, radius 8: the
  application's icon (18 px, from the icon theme as the tiles find
  theirs, else its letter on a colour), 8 px, its title (Manrope 13 px,
  `text`, one line, cut with "…", whole in a tooltip after 600 ms when
  cut), and on the right, 4 px in, a 22 × 22 ✕ (radius 6, a 10 px cross
  of 1.5 px strokes, `text.dim`; under the pointer
  `rgba(242,148,91,0.20)` and the cross `#F7B98A`, the draft's close).
  The focused window's line is lit `rgba(255,255,255,0.06)`, a line under
  the pointer too (`0.10` for the focused one's); the pointer is a hand.
- **Clicks**: a line focuses and raises its window, back from minimised,
  on its desktop (`bring`, as the desktop's list does; the lines are the
  current desktop's, so no switch is needed from them); the ✕ asks the
  window to close as `Super+Q` does (`close ID`, new on the link:
  `xdg_toplevel.close`, X11's `WM_DELETE_WINDOW`), the application
  deciding; nothing waits for it. Both name the window by its id, not
  "the focused one". `Tab` reaches the lines and their ✕ from the bar's
  other items; `Enter` acts.
- **Its surface** (#617): opened with the bar and kept as long as it,
  drawing nothing and taking no pointer while it lists nothing (10 px in
  the gap over the desktops: tiny-skia refuses a buffer of one pixel at a
  fractional scale), and placed from the bar's bottom only once the bar
  knows its height (a place off a short screen ended the shell). iced_layershell hands
  the keys to the surface made last: one made as a window opens, while
  the emoji picker or the launcher holds the keyboard, took their typing.
  Its lines are placed from the surface's bottom, as the compositor has
  it: a press read while it still has the surface where it was finds
  the line it was on there.
- **Its place**: a surface of its own (`hocus-bar-windows`) 14 px over
  the desktops' tiles (over the dots in the mini form), placed from
  them, in the room the column leaves free: the windows are not in the
  column's order and nothing in it moves for them. In `GIVES_WAY` they
  come after Today and before To do: Today, which fills the spacer, ends
  14 px over them; To do's lines and the agents' rows never give way to
  them; on a short bar their lines fold into "+ N", and without room for
  one line and "+ N" the group does not show. A change of focus only
  reorders the lines: its height stays, nothing moves.
- **The mini form**: the first listed window's icon (20 px) in a 44 × 34
  button (radius 10) 14 px over the dots, lit `0.06` under the pointer,
  `0.08` while its popover shows; a 150 ms hover or a click opens the
  popover: 240 px, the popovers' glass, its bottom on the item's, the
  same lines (up to 3, with their ✕) and "+ N", which gives way to the
  desktop's list. A line brings its window and closes the popover; a ✕
  asks its window to close, the popover staying and following the
  windows as they go. `Tab` and the arrows go through the lines, their
  ✕ and "+ N"; `Esc` closes it.
- **Several screens**: each bar lists its own screen's windows (the
  focused window's line lit on the screen that has it only).
- **The link** (`hocus-link`): `recent OUTPUT ID…` gives each bar's
  screen's windows, the most recently focused first, told as the shell
  connects and after a frame where it changed, at most once a frame;
  `bar-windows shown|hidden` says `[panel] windows`; `close ID` asks a
  window to close. The windows' titles and icons are the desktops'
  (`desktops`, `titles`).
- **Setting**: `[panel] windows = true` (default), followed live: `false`
  hides the group in both forms. Settings › La barre (6.4) is not built
  yet; its switch comes with it.
- **At rest nothing draws**: the group draws when what it lists changes
  (a window opening, closing, retitled, focused, minimised, the desktop
  switched), never for a window moving or resizing.
- **Choices the issue did not settle** (to confirm on the machine):
  minimised windows are listed (greyed, in their focus order), so the
  bar brings them back; the mini item shows the first listed window's
  icon (the focused one when the focus is on this screen); the mini
  popover has no header, "the same list" as the full form's; "+ N" opens
  the existing desktop's list rather than a list of its own with ✕;
  Today gives way to the windows (it fills the spacer the windows sit
  in).
- **Measured headless** (`crates/hocus-compositor/tests/bar_windows.rs`),
  the real shell on the CPU, in French, test clients for windows, in a
  home of the test's own: two lines, then three, the focused one first
  and lit, 14 px over the tiles
  (`up_to_three_lines_the_focused_first_and_lit`); a click focuses, a
  minimised window greyed and brought back
  (`a_click_on_a_line_focuses_its_window_even_minimised`); the ✕ asks
  that window alone to close, which stays until it closes
  (`the_close_button_asks_its_window_to_close`); five windows, three
  lines and "+ 2 autres", which opens the desktop's list
  (`past_three_windows_plus_n_opens_the_desktops_list`); none without a
  window, on an empty desktop, once the last closes
  (`no_window_no_group`); `[panel] windows = false`, then `true` written
  in the file (`panel_windows_false_hides_them_and_true_brings_them_back_live`);
  with the card, Today and To do, each window focused in turn from its
  line and no surface moving
  (`only_free_room_is_used_nothing_else_moves_when_the_focus_changes`);
  the mini form's item 14 px over the dots and its popover, its ✕ and a
  line (`the_mini_form_lists_them_in_a_popover`); two screens, each bar
  its own windows (`each_screens_bar_lists_its_own_screens_windows`); a
  window snapped draws the tiles and not the windows, a retitle draws
  them (`nothing_draws_at_rest_and_a_move_draws_nothing`). Unit tests:
  the lines read back (`hocus-link`); the order, the fitting and the
  words, the surface's placement, the lines' parts, the popover's height
  (`hocus-shell`); the room shared (`column`). With `HOCUS_SHOTS=dir`,
  the screenshots of the pull request.
