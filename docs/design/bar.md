# The bar — specification (v2, draft for review)

Status: draft, 2026-10-27, to be approved before any code. It describes
what the prototype on the design canvas shows, in numbers. Where it and
the prototype disagree, this document wins once approved. Sections 2 and
3 (the empty column, its glass and its two forms) are built since
2026-10-28 (3.1, issue #358), the clock of section 4 group 1 and of
section 5 (3.2, issue #359), the levels of section 4 group 10 and of
section 5 (3.3, issue #360), the popovers of section 9 with the
sound's (3.4a, issue #361), the Wi-Fi's row and popover (section 4
group 9, section 5, section 9; 3.5, issue #362), and Bluetooth's (3.6,
issue #363): see [as built](#as-built).

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
agents' tool (herdr): it signals and takes the user there.

## 2. Units and placement

- All sizes are logical pixels; they follow each output's scale.
- The bar sits against the right edge of its output, 8 px from the top,
  right and bottom edges. Windows keep 8 px from the bar, as from the
  screen edges.
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
| Mini | 56 px | clock, next meeting mark, agents waiting (panda and count), agents working (count), secretary, battery, keyboard layout, notifications count |

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
- The bar's width animates over **240 ms**,
  `cubic-bezier(0.2, 0.9, 0.25, 1.08)` (a slight overshoot). Windows that
  touch the bar's side resize in the same frames. The contents cross-fade
  in 120 ms, starting at 60 ms.
- With reduced motion, or a shell drawing on the CPU: no animation; the
  form changes on the next frame, the windows with it.

## 4. Full form, top to bottom

Padding 14 px top, 12 px sides, 12 px bottom; 14 px between groups.

| # | Group | Contents | Size |
|---|---|---|---|
| 1 | Clock | `HH:MM` in Martian Mono 400, 42 px, `text`, every character in a fixed cell (a `0`'s width, the colon 0.55 of it), centred; seconds in Martian Mono 300, 18 px, `text.faint`, on the same baseline, 4 px after; date 6 px below in Manrope 15.5 px, `text.dim` ("mercredi 28 octobre", "Wednesday 28 October"); on a minute change, the odometer (§4.1) | 72 px |
| 2 | Next meeting | Card: 6 px colour bar, title (bold, 1 line, ellipsis), `HH:MM · dans N min`. Hidden when nothing remains today | 44 px |
| 3 | Agents | Header `AGENTS` and total. Three tiles: working, waiting, resting, each with its panda (48 px) and its count. Below, up to 3 lines: agents waiting (oldest first), then stuck ones | 80 px + 18 px a line |
| 4 | Today | The next 2 events after the meeting card: time, title | 18 px header + 20 px a line |
| 5 | To do | Up to 3 open tasks with checkboxes; `+` adds one | 18 px + 22 px a line |
| 6 | Spacer | Takes what is left | flexible |
| 7 | Secretary | Section 7. Panda 62 px, title, one line | 74 px |
| 8 | Desktops | 4 tiles, 30 px high, with a sketch of their windows; the current one outlined in the accent | 30 px |
| 9 | Connections | One 30 px row per connection, like a level's (§4.3): Wi-Fi (network name), Bluetooth (device), phone (name, battery), keyboard layout (short name, one click cycles) | 30 px a row, 2 px apart |
| 10 | Levels | Volume, brightness and battery rows (§4.2): an 18 px icon, an 8 px gauge with its gradient, the value in tabular figures; then the status line, the battery's time left and the power profile | 108 px |
| 11 | Footer | Clipboard, notifications (count badge), menu, user | 34 px |

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
  "muted" in `text.faint`. Charging or full: a bolt `#7EE0A0` in the
  battery.
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
  only here; their popovers are §9's (the sound's since 3.4a).

### 4.3 The connections (approved 2026-10-29, issue #362, variant B)

- **Rows**, not the first draft's 2×2 tiles, so that a network's name
  reads in full: one 30 px row per connection (6 px side padding inside a
  −6 px margin, radius 8), 14 px above the levels. An 18 px icon, 8 px,
  the name in Manrope 14.5 px, cut with an ellipsis. Lit
  `rgba(255,255,255,0.06)` under the pointer, `0.08` while its popover is
  open, as the sound's row. The Wi-Fi's, then Bluetooth's under it
  (3.6); the phone and the keyboard layout (3.8) will add theirs. A row
  whose service is missing is not drawn, and the others close up from the
  bottom.
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
- A click opens its popover (§9).

When the output is too short (1366×768 gives 752 px of bar), groups give
way in this order: Today, then To do, then the agent lines beyond the
first, then Levels collapse to battery only. Clock, meeting, agent tiles,
secretary, desktops and footer always stay.

Clicks:

- A tile or line of an agent: go to it (section 8).
- The meeting card: open the meeting's link, or the calendar on that day.
- A connection tile or a gauge: opens its popover (section 9).
- A desktop tile: switch to it. A task's box: toggles it.

## 5. Mini form

Width 56 px, padding 12 px top, 10 px bottom, items centred, 14 px apart:
clock on two lines, hours over minutes, no seconds (Martian Mono 400,
19 px, line height 1.1, in cells as in §4.1, with the same motion), next meeting mark (30×4 px,
blue, glowing when under 10 min), waiting tile (44 px, panda 40 px, count
badge), working count, spacer, secretary (46 px), the Wi-Fi's button (44 × 38 px, radius 10, a 20 px
icon in the row's states, the sign-in page's dot 9 px from its right and
7 px from its top), the sound's gauge
(vertical, 8 × 48 px, filled from the bottom, grey when muted), Bluetooth's
button under the Wi-Fi's (2 px apart, the same 44 × 38 px, a 20 px sign in
the row's states, the low battery's dot 10 px from its right and 7 px from
its top; 3.6), the
battery's percentage (Manrope 700, 14 px, `#7EE0A0`, amber or orange as in
§4.2, a bolt before it when charging), layout, notifications count. The
gauge and the percentage are 16 px apart (#378).

A hover of 150 ms on an item opens its popover beside the bar; it floats
over the windows and closes 300 ms after the pointer leaves, unless the
pointer moves toward it (section 9).

## 6. The halo

The whole bar lights up when something concerns the user. One state shows
at a time, by priority:

| Priority | State | Colour | Wave | Then | Reminder |
|---|---|---|---|---|---|
| 1 | Meeting in 2 min or less | blue `#8FB3FF` | yes | strong glow | — |
| 2 | An agent waits for the user | amber `#F5B44A` | yes | glow | one wave after 5 min |
| 3 | Meeting in 10 min or less | blue `#8FB3FF` | yes, softer | glow | wave again at 2 min |
| 4 | An agent is stuck (failed) | orange `#F2945B` | no | glow | — |
| 5 | Nothing | none | — | — | — |

- **Wave**: a band 40 % of the bar's height, the state's colour at 38 %,
  runs from top to bottom once in **600 ms**,
  `cubic-bezier(0.3, 0.6, 0.3, 1)`.
- **Glow**: border in the colour at 85 %, outer glow 34 px at 45 %, inner
  glow 22 px at 18 %. It fades in over 600 ms and out over 500 ms.
- Lower states still show where they belong (the tile, the line, the
  meeting card): only the halo is unique.
- A reminder is one more wave, never a loop. The user acting (going to the
  agent, opening the meeting) ends the state at once.
- With reduced motion: no wave; the glow fades in over 200 ms.

## 7. Pandas

The pandas are the validated characters (the apprentice magician family).
They move only when something changes, then hold still; never at rest,
never on battery saver, never with reduced motion.

- **A, a panda per state** (in the agents tiles): working (at its laptop),
  waiting (holds out its hat, ear radiating), resting (asleep). A stuck
  agent shows as an orange line, not a tile.
- **B, the secretary**: one panda with round glasses, a clipboard, and a
  pencil behind its ear (drawing to fix: the pencil must rest behind the
  ear, not into the head). Its magic ear takes the colour of the bar's
  state: green calm, amber waiting (radiates twice, two small hops), orange
  stuck (still). Its line says the essential in five words: "Tout va
  bien · 6 au travail", "2 t'attendent · hocus-panda, infra", "Réunion
  bientôt · Point équipe 15:00".
  A click opens its **briefing** beside the bar (320 px), in this order:
  who waits and for what (the tool or the question, and since when); who
  is stuck and why (one line); the next meeting; the notifications not
  read (senders); how many work and rest. Each line acts like its source
  (going to an agent, opening the meeting). It never opens by itself.
- **C, the peek**: when an agent starts waiting, its panda (96 px) slides
  out from the bar's edge, 18 px from the bar, with a label "**name**
  t'attend · 1 min". In 320 ms, `cubic-bezier(0.2, 0.9, 0.25, 1.15)`,
  fading in over 260 ms. It retracts after **8 s** into its tile, and peeks
  again with the 5-minute reminder. A click on it goes to the agent.

## 8. Going to an agent

A click on anything that names an agent:

1. A 4 px light streak, amber, runs from the bar to the agent's window in
   **300 ms**, then fades.
2. The agent's window takes the focus (on its output, switching desktop if
   needed); its rim pulses once in the accent (600 ms).
3. herdr selects the agent's pane (herdr's API).
4. The halo and the peek end; the secretary returns to calm unless
   another agent waits.

## 9. Popovers

Approved 2026-10-29 (issue #361) for the mechanics and the sound's.

- Width 280 px (briefing 320 px), radius 14 px, same glass as the bar
  (`bar.bg` a little denser: `rgba(22,27,42,0.72) → rgba(14,17,28,0.82)`),
  its rim, a soft shadow (`0 14px 40px rgba(0,0,0,0.38)`); 8 px from the
  bar, aligned with the item that opened them, pushed up to stay 8 px from
  the screen's bottom. One at a time.
- **Opened by a click** on its item (in the full form, the whole row of a
  level, lit `rgba(255,255,255,0.06)` under the pointer and `0.08` while
  its popover is open): it stays open until a click outside it, `Esc`, or
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
- Contents: Wi-Fi (networks, connect), Bluetooth (devices, connect, their
  battery), sound (outputs, inputs, volume per app), brightness, battery
  and power profile, phone (KDE Connect: battery, notifications, send a
  file, ring), keyboard layouts, clipboard history, notifications (clear
  all), menu (apps, settings, session).

## 10. Data sources

| Data | Source | Notes |
|---|---|---|
| Agents and their states | herdr's socket | working, waiting (permission or input), done, failed, idle |
| Fallback for agents | Claude Code hooks `permission_prompt`, `idle_prompt` | when herdr is not running |
| Meetings, events | calendar accounts (CalDAV, Google), local `.ics` | |
| Tasks | local list, CalDAV VTODO, Google Tasks | |
| Network | NetworkManager | on the system bus: `WirelessEnabled`, `Connectivity` (2: a sign-in page) and `ConnectivityCheckUri`, the devices (the Wi-Fi one, a wired one, their state), the access points (SSID, `Strength`, `Flags`, `WpaFlags`, `RsnFlags`), the active connections and the saved ones (their SSID); followed by their signals; set with `RequestScan`, `WirelessEnabled`, `ActivateConnection`, `AddAndActivateConnection`, `Update`, `Disconnect` (as built, 3.5) |
| Bluetooth | BlueZ | on the system bus: the object manager's objects (`GetManagedObjects`, then `InterfacesAdded`, `InterfacesRemoved`), `PropertiesChanged` under `/org/bluez`; the first adapter's `Powered`, its devices' `Name`, `Alias`, `Icon` (else `Class`, else `Appearance`), `Paired`, `Connected`, `RSSI`, and `Battery1`'s `Percentage`; set with `Powered`, `StartDiscovery` and `StopDiscovery`, `Connect`, `Disconnect`, `Pair` (through the shell's `NoInputNoOutput` agent), `Trusted`, `CancelPairing` (as built, 3.6) |
| Sound | PipeWire | the sinks (their names, what they are, their own volume and mute) and the default one, as `pw-dump --monitor` streams the graph's changes; set with `wpctl` (`set-volume`, `set-mute`, `set-default`) |
| Battery, power profile | UPower, power-profiles-daemon | UPower's display device; `org.freedesktop.UPower.PowerProfiles` |
| Brightness | backlight through logind | read in sysfs, its changes from udev; set with logind's `SetBrightness` on the session |
| Phone | KDE Connect (D-Bus) | |
| Keyboard layouts | the compositor's XKB configuration | |
| Notifications, clipboard | the desktop's own servers | |

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

1. The secretary's pencil drawing (section 7, B).
2. Whether the peek's 8 s before it retracts is right (the prototype keeps
   it until the agent is handled).
3. ~~The colour and animation of the clock when the minute changes.~~
   Decided on the 3.2 mockup: white, the odometer (§4.1).
4. What the bar shows when no agent tool is running at all (hide the
   agents group, or show "aucun agent").

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
  windows, the lower layers, the background) into a texture of its own
  through a damage tracker, over the full bar's place and the blur's reach
  (52 px, twice blur(26px)'s deviation) around it, and blurs it again only
  when that changed: the first build's dual Kawase blur, its last pass
  saturating by 1.35. It draws it under the shell's surface, cut to the bar's
  rounded rectangle as the bar is at that frame. The shell draws `bar.bg`
  (70 % to 78 %) and `bar.border` over it. With pixman, `[appearance]
  reduce-transparency`, or a shell on the CPU, there is no blur and the
  shell draws the opaque `#161B2A → #0E111C`.
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
- **Not built**: the levels' popovers and clicks (§9, slice 3.4); their
  keyboard access and names for screen readers (§11); collapsing to the
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
- **The wheel**: a mouse's notch is one step (its pixels after it count for
  nothing); a touchpad's scrolling makes a step each 15 px.
- **Not built:** the microphone and the volume per application (3.4b, below); the
  other popovers (3.5–3.7: the brightness and battery rows are not
  clickable yet); names for screen readers (iced 0.14 has no
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
