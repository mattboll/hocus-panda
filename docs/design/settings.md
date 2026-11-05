# The Settings (6.1)

Status: built 2026-11-04 (issue #401), from the approved mockup
([variant A, the sidebar](https://claude.ai/artifact/Xy4WWrPSZyzYJ9QyjqcqSo))
and the spec in the issue's last comment. This page says what was built
and how; the spec stays the reference for what is not built yet.

The maintainer decided on 2026-11-04:

- variant A, the sidebar;
- each change is saved at once, with an "Annuler" toast and `Ctrl+Z`;
- an unreadable `config.toml` turns the Settings read-only and offers to
  take back the last version that read; nothing is overwritten without the
  user;
- the search finds French and English words and the file's keys;
- 17 sections in 5 groups (6.12 and 6.14 to 6.17 have their own tickets).

6.1 is the app's frame: the window, the sidebar, the home, the search, the
toast with its undo, the file's reading, writing and watching, the model
the sections' tickets fill, and the Énergie page as the first filled-in
example. The other sixteen pages show their title, their line and "À
venir".

## The program

- `hocus-settings [SECTION]` (crate `crates/hocus-settings`), an
  application window of its own, a Wayland client drawn with iced as the
  shell is (tech-choices, decision 2), with the bar's tokens (bar.md §13)
  and its fonts (Manrope, Martian Mono, carried in the binary). App id
  `hocus-settings`. 1100 × 760, 860 × 560 at least, placed by the
  compositor (the centre of the screen under the pointer). It asks for the
  compositor's decorations (xdg-decoration, #603): under Hocuspanda that is
  no title bar but the grip at its top edge (windows.md §7: a drag moves
  the window, a double click maximises or restores it); its own header
  moves it too. Its ✕ closes it, as do `Esc` and `Ctrl+W` (see
  "Closing").
- It draws on the GPU with wgpu, preferring the integrated one
  (`WGPU_POWER_PREF=low`), or on the CPU with tiny-skia when wgpu offers
  only a software adapter, as the shell chooses (its `renderer.rs`).
- **Opened from** the launcher ("Réglages" / "Settings",
  `packaging/hocus-settings.desktop`, shown in the Hocuspanda session), from
  the bar's user popover ("Réglages", above "Se déconnecter"), and with
  `hocus settings [SECTION]`. Sections by their names on the command line:
  `appearance`, `wallpaper`, `bar`, `agenda`, `screens`, `sound`,
  `keyboard`, `mouse`, `network`, `bluetooth`, `desktops`, `applications`,
  `agents`, `captures`, `power`, `account`, `date-and-time`.
- **One window at a time.** The first listens on
  `$XDG_RUNTIME_DIR/hocus-settings-<WAYLAND_DISPLAY>`. A second launch
  hands it its section and its activation token (`XDG_ACTIVATION_TOKEN`,
  which the compositor gives the programs it starts for the user), then
  ends. The first opens that section and asks for the focus with the token
  over xdg-activation, on the connection winit drew the window on (winit
  has no focus request on Wayland: `src/activate.rs`, the crate's FFI
  boundary).
- **Its file** is the one the compositor read: the compositor sets
  `HOCUS_CONFIG` for every program it starts (empty: no file, and the
  window says "Aucun fichier"); started from elsewhere,
  `$XDG_CONFIG_HOME/hocuspanda/config.toml`.

## The window

As the spec's "The window (variant A)": the sidebar (240, its header, the
search field with "Ctrl F" or its ✕, "Accueil", the five groups' labels
and their sections, the current one green), the content's header (where we
are, "Enregistré" / "Lecture seule" with its dot, ✕), and the page under
it, 720 wide at most, padded 6 40 72 32. All 17 sections fit 760 px
without scrolling.

**The home**: the initial on the accent's gradient (52 px), "Bonjour
Alex", the line; six cards (Écrans, Son, Réseau, Bluetooth, Énergie,
Apparence), each opening its section; "CHANGÉS RÉCEMMENT" (the last three
changes: the setting, its value, "à l'instant", "il y a 5 min", "à 13:12",
"hier", "à la main" for one read from the file); "LE FICHIER", its path,
the line, and "Ouvrir le fichier" (`xdg-open`). The user's name is
`HOCUS_USER_NAME`, else their account's full name, else their login.

**A section's page**: its title (22, 700), its line, then its groups: the
label (11.5, 700) on a 28 px line, a segmented control on its right when
the group's rows hold one value per power source, and a card of rows (54
px at least, the name 14/600, the line under it, the control on the
right; 1 px dividers 16 px in). Controls: the switch (40 × 22, its knob
sliding in 160 ms), the menu (32 high, 158 wide at least; its list 38 px
under the button's top, its right on the button's, a check on the current
choice, 140 ms fade and 4 px slide; it closes on a choice, `Esc` or a
click elsewhere), the slider (220 wide, the green track, its value in
Martian Mono; written 450 ms after it stops moving), the segmented
control. A control that does not apply now (the slider while its switch is
off; everything while the file does not read) is drawn at 45 % and inert.

## Énergie

The mockup's page, its rows and choices. The keys:

| Row | On AC | On battery |
|---|---|---|
| Éteindre l'écran après | `[idle] delay` (seconds, or `"off"`; 7.3) | `[power.battery] screen-off` |
| Verrouiller la session | `[power.ac] lock-after` | `[power.battery] lock-after` |
| Mettre en veille après | `[power.ac] suspend-after` | `[power.battery] suspend-after` |
| Couvercle fermé | `[power] lid` (`suspend`, `lock`, `screen-off`, `nothing`) | |
| Rester allumé avec un écran branché | `[power] lid-ignored-when-docked` | |
| Bouton d'alimentation | `[power] power-button` (`ask`, `suspend`, `power-off`, `nothing`) | |
| Économie d'énergie automatique | `[power] auto-saver` | |
| Sous ce niveau de batterie | `[power] auto-saver-below` (5 to 50) | |

- **Coordination with 7.3 and 7.2** (docs/design/idle.md): the screen's
  delay is the first countdown without input, which is what `[idle] delay`
  is, after which the curtain dims the screens, then the session locks.
  `[idle]` takes no other key (the compositor refuses unknown keys there),
  so the AC side writes `[idle] delay` and the battery side
  `[power.battery] screen-off`; the compositor takes the one of the power
  source UPower says (7.2). "Verrouiller la session" is `[power.*]
  lock-after`, which the compositor reads too: "Aussitôt" (`"0s"`) locks
  as the curtain ends, a delay that long after the screens went off,
  "Jamais" never. The other keys are the mockup's proposal for 6.10:
  nothing reads them yet. The compositor reads them as it starts.
- The delays of `[power.*]` are written `"0s"`, `"5m"`, `"1h"`, `"never"`.
- The segmented control starts on the power source in use (the kernel's
  supplies, `/sys/class/power_supply`; `HOCUS_POWER_SUPPLY_DIR` in tests),
  on AC without a battery.
- What applies without a key is shown as the value: the screen's delay
  is the compositor's, 5 min on AC and 3 min on battery (7.2; the menu
  has a "3 min"); the others are the mockup's.

## Écrans (6.9)

Built 2026-11-05 (issue #409), variant A of the
[mockup](https://claude.ai/artifact/NpviTjpi82usEsSWkM9XFA): the
arrangement on top. The maintainer decided: variant A; rotation drawn but
"Plus tard", with no effect; the scale written at once with "Annuler",
the resolution and the refresh rate through the 15 s "Garder ?"; VRR an
on/off switch, off by default. `src/screens.rs` (the compositor's line,
the arrangement's geometry, the page's settings for the search) and
`src/app/screens.rs` (the page, its drawing, its menus, the dialog): the
page draws itself, its keys holding a screen's name.

**The compositor is the source.** The window opens a line of the
compositor's control socket (`HOCUS_SOCKET`, `msg screens`) on threads of
its own; the compositor writes the screens on it as one JSON line each
time they change (a screen plugged or unplugged, the file read again, a
trial, the lid, the primary screen), then a trial's seconds left.
`hocus msg outputs` prints the same JSON once. Each screen: its connector,
whether it is built in, its size in mm, its modes (width, height, mHz,
native), its mode, its scale and whether the file sets it, the scale it
takes by itself, its logical place and size, VRR offered and on, on, plugged,
lid closed, primary, the file's mode and whether the screen offers it; a
screen unplugged this session is listed with its place and when. Nothing
comes from the session bus.

**The page**: "Écrans", its line; DISPOSITION with "Identifier", the
drawing (196 high, a dot every 14 px; each screen at its logical size,
0.1 px per logical px at most, centred; its number's disc, its name, its
mode, its scale when the file sets one; the bar's strip on the primary
screen, 196 or 56 logical wide; dashed for a screen off, unplugged or
behind its closed lid) and its line; the notes of a screen missing (blue);
the chosen screen's group (its number, its name in capitals, "Principal",
its connector, size and density), its rows "Utiliser cet écran",
"Résolution" (native first), "Fréquence" (inert with one rate), "Échelle"
("Automatique (100 %)", 100 to 200 %; "Comme 2048 × 1280", its density,
the bar's mini form under 1796 logical), "Fréquence variable (VRR)"
(inert when not offered), "Rotation" ("Plus tard", inert); TOUS LES
ÉCRANS with "Écran principal" (Automatique or a screen) and "Recopie";
ÉCLAIRAGE NOCTURNE, "Plus tard", inert. Screens are numbered 1 for the
built-in panel, then by connector, and named "Écran du portable", "Écran
externe 34 pouces" (its EDID's size), or "Écran 2" when its size is not
known.

**Dragging** a screen (4 px before it counts) shows where it lands, a
dashed green outline: on the side of the other nearest its centre (the
centres' offset over half their summed sizes, the larger axis wins),
flush, at least 200 logical px shared, caught by aligned edges or
middles within 120 logical px. Let go, it slides there in 200 ms (none
with reduced motion) and both screens' places are written, normalised
so that the smallest x and y are 0; dropped where it was, nothing is
("Lâché à sa place : rien ne change."). A click chooses a screen; `Tab`
reaches each, `Enter` or `Space` chooses it, the arrows put it on that
side of the other, tops or lefts aligned. A size changed (mode, scale)
while the file places the screens writes the places too, keeping the
side and the alignment.

**Tried first**: a resolution or a refresh rate (a new size keeps the
rate when it has it, else takes its highest) is asked of the compositor
(`try NAME mode=WxH@HZ`), which applies it without the file and counts 15 s
on its own clock: the dialog over the window shows the seconds it is told
(a ring, "Garder cette résolution ?", what the screen shows now and what
it goes back to, "Revenir", "Garder" with the focus; `Tab` between them,
`Esc` reverts), and the header says "À confirmer" with an amber dot.
Nothing pressed, `Esc`, "Revenir", or the window gone (its line closes):
the compositor takes it back and nothing is written. "Garder" keeps it
(`keep ID`) and writes `mode`, with the toast "Gardé : 2560 × 1080 à 60 Hz
sur l’écran externe 34 pouces" and its "Annuler", which goes straight
back.

**Written at once**, with the toast's "Annuler" and `Ctrl+Z`: a place,
the scale, the primary screen, VRR, a screen turned off or on. "Annuler"
takes each key back as the file had it: a key the file did not hold is
taken out, with its table when that one is left empty and has no comment
(the file then is byte for byte what it was). These keys go through the
file's worker together (`Ask::Keys`), as 6.1's do one by one.

| Row | Key |
|---|---|
| Disposition | `[outputs.NAME] position = [x, y]`, every screen plugged |
| Utiliser cet écran | `[outputs.NAME] enabled = false` (on: taken out) |
| Résolution, Fréquence | `[outputs.NAME] mode = "2560x1600@60"` |
| Échelle | `[outputs.NAME] scale` (Automatique: taken out) |
| Fréquence variable (VRR) | `[outputs.NAME] vrr = true` (off: taken out) |
| Écran principal | `[panel] output` (Automatique: taken out) |

**The compositor follows the file** (`crates/hocus-compositor/src/
displays.rs`): a thread waits in inotify on its folder, reads and parses
it, and the loop takes `[outputs]` and `[panel] output` from it at once
(windows.md §13). A file that does not read changes nothing.

**A screen missing**: unplugged, its dashed ghost stays at its place,
"Débranché · sa place est gardée", not chosen, with a note ("Écran
externe 34 pouces débranché à 14:02 — Sa place (…) et ses réglages sont
gardés…"); off by the file, dashed "Éteint", still chosen and dragged,
with its note; the lid closed over the built-in panel with another
screen on, dashed "Couvercle fermé", its rows inert, with its note. The
last screen on cannot be turned off (its switch is inert), and the
compositor keeps one on whatever the file says. A file whose places
overlap or leave a screen apart: the compositor puts them left to right
and the page says so.

**The search** finds the page's rows by their words and keys
("Disposition des écrans", "Identifier les écrans", "Utiliser cet écran",
"Résolution", "Fréquence", "Échelle" by « hidpi », "Fréquence variable
(VRR)", "Rotation", "Écran principal", "Recopie" by « miroir »,
"Éclairage nocturne"); they are model settings whose `Places::Page` say
their keys (`outputs.NAME.scale`) but are not slots of the file.

Not built here (to confirm): **mirroring** (no `mirror` key yet: the
compositor does not draw one screen's picture on another; the row is
"Plus tard", inert); **Identifier**'s overlay on the screens themselves
(the drawing shows the numbers large for 3 s, the compositor draws
nothing yet); a drop that would split three screens or more (only two are
drawn against each other); the night light.

## Saving

- **At once**, with no "Apply": a switch, a menu's choice, a segmented
  choice when it changes; a slider 450 ms after it stops moving.
- **Off the window's thread**: the file's worker (`src/worker.rs`) reads
  and writes the file; the window only shows what it last read.
- **Only the changed line**: the file is edited as TOML that keeps its
  comments, order, blank lines and unknown keys (`toml_edit`, through the
  new crate `hocus-config`, which Day.1's "Choisir…" now shares); the
  line's comment stays; a table is made only when the key needs one
  (`[power.battery]`, not `[power]` as well). A key at its default is still
  written. The file is written beside itself then renamed over (through a
  link to where it really is), its mode kept; what is written is read back
  first and refused unless it says the value asked.
- **The toast**, 18 px over the window's bottom, centred on the content:
  "Mettre en veille après, sur batterie : 30 min", "Annuler", ✕. It rises
  8 px and fades in over 160 ms (with reduced motion it only appears), and
  goes after 6 s or when the next replaces it.
- **Undo** writes the previous value back: the line as it was, the key
  taken out when the file did not hold it (the file then is byte for byte
  what it was). "Annulé : … revient à 5 min." for 3.5 s, the row lit.
  `Ctrl+Z` undoes the changes one by one while the window is open,
  `Ctrl+Shift+Z` redoes.

## A hand-edited file

- **Watched** with inotify on its folder (editors rename a new file over
  the old), and on its link's target's. The window shows what it holds
  now, on every page; nothing is written over it. "config.toml a changé :
  relu.", with "Voir" when one value of the page that is open changed:
  that row lights. The worker tells its own writes from others' by their
  text.
- **A value not understood** (`suspend-after = "3 heures"`): the default
  applies; its row says "Dans le fichier : « 3 heures », non compris ;
  Jamais s'applique." in `#F7B98A`. Choosing a value writes over that one
  line.
- **A file that does not read**: the window is read-only. The banner
  ("config.toml ne se lit plus, ligne 12", the parser's message, the
  line about the desktop keeping the last readable configuration), "Ouvrir
  le fichier" and "Reprendre la version de 13:12". The rows show the last
  version that read, at 45 %, inert; the search still works. After each
  read that reads, the worker keeps a copy in
  `$XDG_STATE_HOME/hocuspanda/config.last-good.toml`; taking it back keeps
  the broken file as `config.toml.broken` beside it. Fixed on disk, the
  banner goes by itself.

## The search

`src/search.rs`. It looks in the sections' names, the settings' names, a
few nearby words for each in French and in English (whatever the window's
language), the other language's names, and each setting's keys
(`suspend-after`, `idle.delay`). Accents and case are ignored, `’` is `'`;
several words must all be found. The order: a section's name, then
settings whose name starts a word with the first word typed, then names
holding it, then settings found by a nearby word, then the settings of a
section found by its name; 8 at most. The list replaces the sections:
"3 résultats · ↑ ↓ puis Entrée", each row its section's icon, its name
with the typed part lit, and "Énergie › Écran et veille", " · par
« veille »" when found by a nearby word. Opening one (a click, `Enter`)
opens its page, scrolls its row to the middle and lights it
(`rgba(108,194,135,0.20)`, a 1.5 px ring, after 250 ms, held 1 s, gone by
2.4 s; with reduced motion it only fades). The list stays; `Esc` or ✕
empties it.

"veille" finds "Mettre en veille après" (in its name), then "Couvercle
fermé" and "Bouton d'alimentation" (by « veille »).

## The keyboard

`Ctrl+F` searches (a letter typed while nothing has the keyboard starts the
search too); `Tab` goes from the search field through the sidebar's rows,
then the page's controls (`Shift+Tab` back); `↑`/`↓` move through the
sidebar's rows or the results, `Enter` opens; `Space` toggles a switch;
`Space`, `Enter` or `Alt+↓` open a menu, `↑`/`↓` and `Enter` choose in it;
`←`/`→` move a slider by a step, `Page Up`/`Page Down` by a quarter;
`←`/`→` choose on the segmented control; `Esc` closes a menu, then empties
the search, then leaves the search field, then closes the window;
`Ctrl+Z` / `Ctrl+Shift+Z`; `Ctrl+W` closes.

## Closing

✕, `Esc` (when no menu is open and the search field neither holds text
nor has the keyboard), `Ctrl+W` and a close the compositor asks (its
shortcut, for one) all close the window the same way, never
waiting on its thread: a slider still moving is written at once instead
of 450 ms later, and the window goes once the file's worker has written
every change asked (after 5 s at most, with a warning in the log, should
the disk hang). The program ends with the window ("the Settings closed"
in its log).

## The model, for the sections' tickets

`src/model.rs`. A section's ticket writes a module like `src/power.rs`
returning its `&'static [Setting]`, and names it in `model::settings_of`.
The window does the rest: its page, its rows, the search, the saving, the
undo, the watching, the warnings.

```rust
pub struct Setting {
    pub id: &'static str,                     // "power.suspend-after"
    pub section: SectionId,
    pub group: RowGroup,                      // its group's label; per_source: one value on battery, one on AC
    pub name: Words,                          // w("Mettre en veille après", "Suspend after")
    pub line: Option<Words>,                  // the line under the name
    pub keywords: (&'static [&'static str], &'static [&'static str]), // nearby words, French then English
    pub kind: Kind,                           // Switch | Menu(Vec<Choice>) | Slider { min, max, step, unit } | Text
    pub places: Places,                       // One(Place) | PerSource { battery, ac }
    pub needs: Option<&'static str>,          // the switch it depends on
}
pub struct Place {
    pub key: &'static [&'static str],         // ["power", "battery", "suspend-after"]
    pub codec: Codec,                         // Bool | Int { min, max } | IdleSeconds | Delay | Word(&[..]) | Text
    pub default: Value,                       // what applies without the key
}
pub enum Value { Bool(bool), Int(i64), Seconds(Option<u64>), Word(&'static str), Text(String) }
```

`store::Slot` is one value of a setting (its id, and its power source when
it has one each way); `store::read` says what a text holds of every slot
(`Reading::Absent`, `Value`, `NotUnderstood`), `store::written` changes one
slot in a text. Settings that live outside the file (Wi-Fi networks,
Bluetooth pairings, the password, the time zone) are their tickets' to
model: they go to their services, off the window's thread, never into the
file.

## How it is built

- The window's state is iced's (`src/app`): every change of the file goes
  to the worker over a channel and comes back as what the file now holds;
  second launches come in the same way.
- **At rest it draws nothing**: no timer, no tick. Motions (the toast
  rising, a row lit, a switch's knob, a menu opening) ask for frames only
  while they move; toasts go and sliders are written on one-shot timers.
  The caret of the search field blinks while the field has the keyboard
  (iced's), and draws then.
- The worker's threads: one owns the file (reads, writes, the last
  version that read, the recent changes in
  `$XDG_STATE_HOME/hocuspanda/settings-recent`), one waits in inotify's
  read.
- **What it shows is said in its log** (target `hocus_settings::shows`,
  JSON: the page, the sections, the query and its results, the lit row,
  the toast, the file's state, the banner, each row's value and warning)
  and **where its pieces are** (`hocus_settings::places`, at debug: each
  id'd piece's place in the window, scrolling counted), as the shell says
  what it draws. Tests read both; names for screen readers come with the
  accessibility slice.

## Choices made here (to confirm on the machine)

- The battery's screen delay in `[power.battery] screen-off` (above).
- The avatar's initial is drawn in Manrope Bold: Unbounded is not carried.
  No picture yet (AccountsService, 6.11).
- The home's cards say "À venir" for Écrans, Son, Réseau and Bluetooth
  until their tickets give their state; Énergie's says the battery (or "Sur
  secteur") and the suspend delay; Apparence's "Sombre · français".
- The keyboard's ring is drawn on the piece's own edge, not 2 px outside
  it; the labels' 0.1 em letter spacing is not drawn (iced has none).
- A found row is scrolled to at once, without a smooth scroll; then it
  lights as the spec says.
- "Ouvrir le fichier" opens the file with `xdg-open`, not at the line.
- The window has no resize edges of its own yet (its 1100 × 760 fits a
  laptop's screen); it moves by the compositor's grip or by its header.
- Under a compositor without xdg-decoration, winit draws its own frame
  around the window: the Settings are made for Hocuspanda's.

## Not built here

- What the desktop does with the rest of `[power]` (6.10), and the
  compositor following the file's changes live (each setting's ticket;
  the screens' and the primary screen's are followed since 6.9).
- The other sixteen pages (6.2 to 6.17, Day.1).
- The popovers' "… settings" links straight to a section (each popover's
  ticket; `hocus settings SECTION` is there for them).

## Tests

`crates/hocus-compositor/tests/settings.rs`: a headless compositor, the
real `hocus-settings` drawing on the CPU, a configuration file, state,
home and runtime directories of the test's own, no bus, no power supply;
waits on what the window's log says, never on a time, except to check
that nothing draws at rest.

- `the_window_opens_with_its_sidebars_17_sections`
- `searching_veille_finds_the_suspend_delay_the_lid_and_the_button`
- `changing_the_delay_writes_idle_delay_at_once_keeping_the_rest`
- `annuler_and_ctrl_z_take_the_value_back`
- `a_hand_edit_of_the_file_shows_in_the_window`
- `an_unreadable_file_turns_it_read_only_and_the_last_readable_one_comes_back`
- `a_value_not_understood_warns_on_its_row`
- `a_second_launch_brings_the_first_forward`
- `at_rest_it_commits_nothing`
- `the_grip_shows_on_hover_at_the_top_edge_and_a_drag_on_it_moves_the_window`
- `the_close_button_esc_and_ctrl_w_close_the_window`
- `a_change_made_just_before_closing_is_on_disk`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

`crates/hocus-compositor/tests/settings_screens.rs` (6.9), on a compositor
of two screens (2560 × 1600 at 1, 3440 × 1440):

- `the_arrangement_is_drawn_from_the_compositors_screens`
- `dragging_the_external_above_the_laptop_writes_both_places`
- `the_scale_is_written_at_once_and_annuler_puts_the_file_back`
- `a_resolution_kept_with_garder_is_written`
- `a_resolution_not_kept_goes_back_after_15_s_on_the_held_clock`
- `the_compositor_follows_the_files_screens`
- `searching_hidpi_and_miroir_finds_the_scale_and_mirroring`
- `at_rest_the_screens_page_commits_nothing`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

Unit tests: the sections and groups, the delays' and the idle delay's
codecs, the search's order, accents and keys, the store's edits (only the
line, its comment kept, tables made, a value written back as it was), the
power source from the kernel's supplies, the recent changes kept, the
second launch's line, the ids' names; `hocus-config`'s edit through a link
with its mode; the user popover's rows; the launcher reading the
package's entry as "Réglages".
