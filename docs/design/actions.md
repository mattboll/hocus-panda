# The actions palette — specification (#736)

Status: approved 2026-11-07 (issue #736): the palette is its own
dropdown, not a mode of the launcher; the empty field shows the recent
actions, then every action by category; an action asking for a target
turns the list into its targets; an action impossible now stays listed at
45 % with its reason; names are searched in French and in English; no
result offers the launcher's and the Settings' search; only actions that
already exist. Built since 2026-11-07: see [as built](#as-built). Sizes
are logical pixels.

## 1. What it is

`Super+P` (`[shortcuts] actions`, "Actions" in Keyboard and shortcuts,
group BAR AND LAUNCHER) opens a dropdown listing every action the desktop
can run, filtered as the user types, each with its shortcut. `Enter` runs
the selected action on the **target**: the window that had the keyboard
when it opened, or that window's screen; with no focused window, the
screen under the pointer. `Super+P` again closes it.

It shares the launcher's search ([launcher.md](launcher.md) §4) and its
rows' style, not its surface: the launcher opens things (applications,
calculation, the web), the palette acts on what is open.

## 2. Where and how it shows

- On the target's screen, centred across its work area (the space the
  bar leaves), its top 64 px below the screen's top. 720 wide (on a
  narrower work area: its width less 32), radius 16, the popovers' glass
  (`rgba(26,31,48,.97)` → `rgba(16,19,30,.98)`), a 1 px
  `rgba(255,255,255,.12)` rim, the popovers' shadow, 10 px in.
- **No blur.** The rest of the screen is dimmed by `rgba(6,8,14,.25)`;
  the **target window** is drawn above the dim with a 2.5 px `#7EE0A0`
  ring and a 4 px `rgba(126,224,160,.25)` glow while the palette shows.
- **The field**: 56 tall, radius 12, a green border
  (`rgba(108,194,135,.45)`) on `rgba(255,255,255,.05)`; a prompt glyph
  (19 px), the text in Manrope 20 px, the placeholder "Une action :
  fenêtre, bureau, réglage, session…" / "An action: window, desktop,
  setting, session…" (`#7C8698`, 18 px); on its right the **target chip**
  (a window glyph and the target's title, 12.5 px 600, on
  `rgba(108,194,135,.12)` in a `rgba(108,194,135,.35)` rim) and "Échap" /
  "Esc".
- **The list**, 8 px under the field: headings (11 px 800 capitals spaced
  0.09 em, `#7C8698`, 24 px rows) and rows 44 px tall, radius 10: a 30 px
  tile (radius 8, `rgba(255,255,255,.06)`) with the action's glyph
  (17 px), 12 px, the name (Manrope 600 15.5 px, cut with "…"; matched
  letters `#7EE0A0` 800), a `›` after an action that asks for a target, a
  keyword that found it ("mot-clé : bureaux"), its category (12.5 px,
  `#7C8698`), then on the right either its shortcut as keycaps (Martian
  Mono 11 px; "Super", "Maj"/"Shift", "Ctrl", "Alt", arrows, "1…9" for a
  family), or for a switch its state ("activé" in green / "désactivé"),
  or for an inert action its reason (italic, 12.5 px).
- **The selected row**: the launcher's green border and tint
  (`rgba(108,194,135,.6)` / `.14`). The first row that can run is
  selected as the list opens and as the query changes, past inert ones;
  the arrows still go over inert rows (`Enter` on one does nothing).
- Nine rows' room at most; the list scrolls past that (a 3 px scroll
  mark). A footer line (12 px, `#7C8698`) under a rule says how many
  actions match and the keys.
- **Motion**: in over 140 ms, coming down 8 px and fading (`ease-out`);
  out over 100 ms (`ease-in`). With reduced motion or a shell on the CPU:
  a fade only. Nothing is drawn at rest.

## 3. The empty field

- "RÉCENTES" / "RECENT": the last five actions run from the palette, the
  latest first, kept in `$XDG_STATE_HOME/hocuspanda/actions` (written off
  the loop). Absent before the first one.
- Then every action by category, in this order: Fenêtre / Window, Bureaux
  / Desktops, Écrans / Screens, Agents, Barre / Bar, Captures /
  Screenshots, Accessibilité / Accessibility, Session, Réglages /
  Settings.

## 4. Searching

- The launcher's fuzzy match (word starts and letters that follow each
  other count more, accents and case ignored), on the action's name **in
  both languages**, its keywords (at 0.6) and its category (at 0.5). A
  match found by a keyword only shows the keyword after the name.
- Equal scores go to the most recently run, then the catalogue's order.
- A trailing digit fills a family's target: "envoyer 3" lists "Envoyer au
  bureau 3 · Notes" first (the desktop's name when it has one).

## 5. Actions with a target (steps)

- An action whose name ends in "…" (send to desktop…, take to desktop…,
  go to desktop…, go to an agent…) opens a second step in the same list
  on `Enter`, `Tab` or `→`: the field shows the action as a green chip
  ("Envoyer au bureau ›") and is emptied; the list shows the targets:
  - the desktops of the target's screen: a 44 × 28 sketch of their
    windows, "Bureau N · name", how many windows; the window's own (or,
    to go to one, the screen's current one) inert, "actuel"; the screen's
    current desktop ringed;
  - the agents: their panda (26 px) and state, in the order of the bar's
    rows.
- Digits `1`–`9` pick a desktop at once. `Backspace` on the empty field,
  or `Esc`, goes back to the actions, the query as it was; `Esc` again
  closes.

## 6. Running

- `Enter` or a click runs the selected action and closes the palette: its
  fade first, then the action. The action is the same code as its
  shortcut, its menu or its row in the bar; session actions show their
  usual confirmation (log out, restart, power off).
- **Inert actions** (another screen with one screen, a window action with
  no focused window, the previous desktop on the first one, the
  magnifier's when it is off, what logind does not allow…) are listed at
  45 % with their reason; `Enter` on them does nothing.
- **No result**: "Aucune action pour « … »." / "No action for "…"."
  (14 px, `#8C96A8`, centred), then two rows: "Chercher « … » dans le
  lanceur" (opens the launcher with the query, its shortcut shown) and
  "Chercher « … » dans les Réglages" (opens the Settings' search with it).
- `Esc`, a click outside, the session locking or the target window
  closing close the palette; the keyboard goes back to the target.

## 7. The catalogue

One list, `hocus_link::actions`, which the compositor, the shell and the
Settings share: id, names (French, English), keywords, category, whether
it acts on a window, whether it is a switch, the target it asks for, its
`[shortcuts]` key (or a family's prefix), its glyph, and who runs it. The
shortcut shown is read from the live `[shortcuts]` configuration, so a
rebound key shows rebound and an unbound action shows none.

- The shortcuts' actions, the families of nine folded into one action
  with a target.
- The radial menu's: always on top and on all desktops (switches), the
  other screen, the previous and the next desktop.
- The bar's: the secretary's briefing, the agents' sources, the
  clipboard's history, the notifications, do not disturb (a switch), the
  calendar, adding a task, the sound, the Wi-Fi, the volume keys, the
  bar's form.
- The agents: go to the agent that waits; go to an agent… (a step).
- Accessibility: the magnifier's three, the screen reader, high contrast
  and reduced motion (switches), the keyboard into the bar.
- The session: lock, suspend, log out, restart, power off.
- The Settings: each page ("Réglages › Son" / "Settings › Sound").

## 8. Keyboard, screen readers, themes

- The arrows (and Page Up / Page Down) move the selection past the
  headings, the wheel scrolls, hover selects, a click runs.
- For screen readers the palette is a window "Actions" holding a dialog
  (described by its target), its text field, and a list whose items are
  its rows, the selected one focused, an inert one disabled with its
  reason, each with its category and its shortcut or state; a screen
  reader's click runs a row.
- Every colour is a theme's token: the light theme and high contrast
  read them by their rule (accessibility.md, A.3).

## As built

### #736, 2026-11-07

- **Who does what.** The compositor takes `Super+P` and tells the shell
  `actions OUTPUT SCALE WINDOW X WIDTH\tON\tINERT\tSHORTCUTS\tTITLE`
  (`hocus_link::ActionsShown`): the target's screen, its work area across
  (the palette centred in it), the target window, the switches on (always
  on top, on all desktops, the screen reader, high contrast, reduced
  motion), the actions that cannot run and why, every `[shortcuts]` key's
  chords as the configuration binds them now, and the target's title (its
  screen's name without a window). The shell opens its layer surface
  (`hocus-actions`, overlay layer, anchored to the screen's top left by
  its margins, holding the keyboard), lists and draws; it says `action ID
  ARG WINDOW` once its fade is over for the compositor's actions, and
  carries out its own (the bar's popovers, do not disturb, the agents,
  the Settings) then. `actions-end` closes it when its target window
  goes.
- **The compositor's actions** run as their shortcuts do (the same
  `Action`), the radial menu's as its menu does, focusing the target
  first when it lost the focus; the screen's own (desktop left and right,
  go to desktop N, the bar's form, the launcher) on the target's screen,
  not the pointer's. Restart and power off ask logind's leave
  confirmation on that screen's bar; suspend locks first, as the session
  menu's row. High contrast and reduced motion are written to
  `[appearance]` (`high-contrast`, `reduced-motion`) off the loop, as the
  Settings' switches write them; the desktop follows the file.
- **Drawing.** The compositor moves the surface as a popover of its own
  kind (in over 140 ms coming down 8 px, out over 100 ms, a fade only
  with reduced motion or on the CPU), draws the popovers' shadow round it,
  no glass under it (it is nearly opaque), the dim over the whole screen
  under it and the target window's surfaces, ring and glow above the dim
  (`render.rs`, the target taken out of the windows' order for that
  frame). On the CPU its corners are cut. The dim's and the ring's pieces
  keep their ids: at rest no frame is drawn.
- **The glyphs** are the mockup's 24-unit paths drawn as small SVGs,
  tinted by the theme; the agents' rows draw the bar's pandas.
- **"Chercher dans les Réglages"** starts `hocus-settings --search QUERY`
  (a running window is told it on its socket), which shows the search's
  results, its field holding the keyboard.
- **Choices made here** (to confirm on the machine): the step's heading
  is "kitty part sur…" for send and take, the action's name for the
  others; the screen's current desktop is ringed in the step's sketches;
  the recent list keeps twenty ids on disk, five shown; airplane mode is
  not listed (the bar only reads it; it is the Settings' switch); there is
  no "new desktop" target (the desktops' count is the configuration's);
  switching windows from the palette goes to the previous window at once
  (`Alt+Tab` released).
- **Measured headless** (`crates/hocus-compositor/tests/actions.rs`, the
  real shell in French): `Super+P` on the focused window's screen with
  the pointer on the other, 64 px down, centred in the work area, the
  window ringed above the dim, nothing on the other screen; the empty
  field's categories; the shortcuts shown, a rebound one followed live;
  screen readers' dialog and rows; no frame at rest; `Esc` giving the
  keyboard back; "bur" (the desktops' actions, marks, a keyword), the
  English names, running "Moitié droite" and "Agrandir" and the recent
  list after them (and on disk); a switch's state and `Enter`; the
  desktops' step, `Backspace` back, a digit, "envoyer 2"; an inert
  action; "pizza" and the launcher searching it; log out asking first;
  reduced motion; the arrows; the target closing. Unit tests: the
  catalogue (each `[shortcuts]` key and each Settings page in it, both
  languages), the lines, the keycaps, the search's ranking and marks, the
  steps, the recent list, the motion, the ring.
