# The bar — specification (v2, draft for review)

Status: draft, 2026-10-27, to be approved before any code. It describes
what the prototype on the design canvas shows, in numbers. Where it and
the prototype disagree, this document wins once approved. Sections 2 and
3 (the empty column, its glass and its two forms) are built since
2026-10-28 (3.1, issue #358), the clock of section 4 group 1 and of
section 5 (3.2, issue #359), and the levels of section 4 group 10 and of
section 5 (3.3, issue #360): see [as built](#as-built).

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
| 1 | Clock | `HH:MM` in Martian Mono 400, 34 px, `text`, every character in a fixed cell (a `0`'s width, the colon 0.55 of it), centred; seconds in Martian Mono 300, 15 px, `text.faint`, on the same baseline, 4 px after; date 6 px below in Manrope 13 px, `text.dim` ("mercredi 28 octobre", "Wednesday 28 October"); on a minute change, the odometer (§4.1) | 60 px |
| 2 | Next meeting | Card: 6 px colour bar, title (bold, 1 line, ellipsis), `HH:MM · dans N min`. Hidden when nothing remains today | 44 px |
| 3 | Agents | Header `AGENTS` and total. Three tiles: working, waiting, resting, each with its panda (48 px) and its count. Below, up to 3 lines: agents waiting (oldest first), then stuck ones | 80 px + 18 px a line |
| 4 | Today | The next 2 events after the meeting card: time, title | 18 px header + 20 px a line |
| 5 | To do | Up to 3 open tasks with checkboxes; `+` adds one | 18 px + 22 px a line |
| 6 | Spacer | Takes what is left | flexible |
| 7 | Secretary | Section 7. Panda 62 px, title, one line | 74 px |
| 8 | Desktops | 4 tiles, 30 px high, with a sketch of their windows; the current one outlined in the accent | 30 px |
| 9 | Connections | 2×2 tiles of 34 px: Wi-Fi (network name), Bluetooth (device), phone (name, battery), keyboard layout (short name, one click cycles) | 73 px |
| 10 | Levels | Volume, brightness and battery rows (§4.2): a 14 px icon, a 6 px gauge with its gradient, the value in tabular figures; then the status line, the battery's time left and the power profile | 83 px |
| 11 | Footer | Clipboard, notifications (count badge), menu, user | 34 px |

### 4.1 The clock (approved 2026-10-28, issues #359 and #332)

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

### 4.2 The levels (approved 2026-10-29, issue #360)

- **Rows**, 7 px apart, Manrope 11.5 px `text.dim`: the icon (sound,
  sun, battery; `text`), 8 px, the gauge (6 px high, radius 3, track
  `rgba(255,255,255,0.10)`, fill with its gradient from the left), 8 px,
  the value right-aligned in 30 px. The sound's gauge is `gauge.sound`,
  the brightness's `gauge.light`, the battery's `gauge.battery`, amber
  (`#E8A13A → #F5C46E`, value and status `#F5C46E`) at 20 % or less, orange
  (`#E5793F → #F7B98A`, `#F7B98A`) at 10 % or less, both only off the
  charger. Muted: the speaker crossed in `text.faint`, the fill
  `rgba(255,255,255,0.22)` at the volume's length, the value "muet" /
  "muted" in `text.faint`. Charging or full: a bolt `#7EE0A0` in the
  battery.
- **The status line**, Manrope 11 px, right-aligned, `text.faint` (the
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
  only here; their popovers are §9's.

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
15 px, line height 1.1, in cells as in §4.1, with the same motion), next meeting mark (30×4 px,
blue, glowing when under 10 min), waiting tile (44 px, panda 40 px, count
badge), working count, spacer, secretary (46 px), the sound's gauge
(vertical, 6 × 40 px, filled from the bottom, grey when muted), the
battery's percentage (Manrope 700, 11 px, `#7EE0A0`, amber or orange as in
§4.2, a bolt before it when charging), layout, notifications count. The
levels are 12 px apart, as the mockup has them.

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

- Width 280 px (briefing 320 px), radius 14 px, same glass as the bar,
  8 px from it, aligned with the item that opened them.
- Open on click, or after 150 ms of hover in the mini form.
- Stay open while the pointer moves toward them (a cone from the pointer
  to the popover's near corners), and close 300 ms after it has really
  left. `Esc` closes; focus returns to the item.
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
| Network | NetworkManager | |
| Bluetooth | BlueZ | |
| Sound | PipeWire | the default sink's volume and mute, as `pw-dump --monitor` streams the graph's changes; set with `wpctl` |
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
  top, inside its rim: 12 px below the bar's top, 194 × 62 px in the full
  form and 54 × 32 px in the mini, the full size while the form switches.
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
  rectangle (194 × 62 or 54 × 32 px), measured headless on an Intel GPU
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
    doubling up to a minute: it announces nothing when it starts.
  - A service missing hides its row, says so once in the log, and once
    again when it is back.
- **Its own surface, woken without a message.** The levels draw on a
  layer surface of their own (`hocus-bar-levels`), inside the bar's rim,
  41 px above its bottom: 194 × 120 px in the full form (the group's
  bottom 61 px above the bar's, above the footer's place), 54 × 68 px in
  the mini (40 px above, above the next item's place), the full size while
  the form switches. iced draws every surface after any message of the
  shell's, so a change of level is not a message: the shell asks the
  compositor (`wake hocus-bar-levels`), which sends that surface a
  configure as it is, and iced draws it alone. A level key reaches its
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
