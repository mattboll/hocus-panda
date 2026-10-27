# The bar — specification (v2, draft for review)

Status: draft, 2026-10-27, to be approved before any code. It describes
what the prototype on the design canvas shows, in numbers. Where it and
the prototype disagree, this document wins once approved.

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
- `Super+B` toggles the form on the output under the pointer.
- The bar's width animates over **240 ms**,
  `cubic-bezier(0.2, 0.9, 0.25, 1.08)` (a slight overshoot). Windows that
  touch the bar's side resize in the same frames. The contents cross-fade
  in 120 ms, starting at 60 ms.
- With reduced motion: no animation; the form changes on the next frame.

## 4. Full form, top to bottom

Padding 14 px top, 12 px sides, 12 px bottom; 14 px between groups.

| # | Group | Contents | Size |
|---|---|---|---|
| 1 | Clock | `HH:MM` in Unbounded 400, 38 px; seconds in Unbounded 300, 15 px, grey; date in Manrope 13 px | 60 px |
| 2 | Next meeting | Card: 6 px colour bar, title (bold, 1 line, ellipsis), `HH:MM · dans N min`. Hidden when nothing remains today | 44 px |
| 3 | Agents | Header `AGENTS` and total. Three tiles: working, waiting, resting, each with its panda (48 px) and its count. Below, up to 3 lines: agents waiting (oldest first), then stuck ones | 80 px + 18 px a line |
| 4 | Today | The next 2 events after the meeting card: time, title | 18 px header + 20 px a line |
| 5 | To do | Up to 3 open tasks with checkboxes; `+` adds one | 18 px + 22 px a line |
| 6 | Spacer | Takes what is left | flexible |
| 7 | Secretary | Section 7. Panda 62 px, title, one line | 74 px |
| 8 | Desktops | 4 tiles, 30 px high, with a sketch of their windows; the current one outlined in the accent | 30 px |
| 9 | Connections | 2×2 tiles of 34 px: Wi-Fi (network name), Bluetooth (device), phone (name, battery), keyboard layout (short name, one click cycles) | 73 px |
| 10 | Levels | Volume, brightness and battery as 6 px gauges with a gradient and a value; then the battery's time left and the power profile | 74 px |
| 11 | Footer | Clipboard, notifications (count badge), menu, user | 34 px |

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
clock on two lines (Unbounded 400, 17 px), next meeting mark (30×4 px,
blue, glowing when under 10 min), waiting tile (44 px, panda 40 px, count
badge), working count, spacer, secretary (46 px), battery value, layout,
notifications count.

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
| Sound | PipeWire | |
| Battery, power profile | UPower, power-profiles-daemon | |
| Brightness | backlight through logind | |
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
| fonts | Unbounded (clock, counts), Manrope (text) |

The light and neon themes give their own values for the same tokens.

## 14. Open questions

1. The secretary's pencil drawing (section 7, B).
2. Whether the peek's 8 s before it retracts is right (the prototype keeps
   it until the agent is handled).
3. The colour and animation of the clock when the minute changes (later).
4. What the bar shows when no agent tool is running at all (hide the
   agents group, or show "aucun agent").
