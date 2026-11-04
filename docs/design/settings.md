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
  compositor (the centre of the screen under the pointer); no title bar of
  the compositor's: its own header moves it, its ✕ closes it.
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

- **Coordination with 7.3** (docs/design/idle.md): the screen's delay is
  the first countdown without input, which is what `[idle] delay` is. It
  has one value, whatever the power source, and `[idle]` takes no other
  key (the compositor refuses unknown keys there). So the AC side writes
  `[idle] delay`, and the battery side keeps its value in
  `[power.battery] screen-off` until 6.10 has the compositor choose by the
  power source. The other keys are the mockup's proposal for 6.10: nothing
  reads them yet.
- The delays of `[power.*]` are written `"0s"`, `"5m"`, `"1h"`, `"never"`.
- The segmented control starts on the power source in use (the kernel's
  supplies, `/sys/class/power_supply`; `HOCUS_POWER_SUPPLY_DIR` in tests),
  on AC without a battery.
- What applies without a key is shown as the value: `[idle] delay`'s
  default is off ("Jamais"), the compositor's; the others are the
  mockup's.

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
the search; `Ctrl+Z` / `Ctrl+Shift+Z`; `Ctrl+W` closes.

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
  laptop's screen); it moves by its header.

## Not built here

- What the desktop does with `[power]` (6.10), and the compositor
  following the file's changes live (each setting's ticket).
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
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

Unit tests: the sections and groups, the delays' and the idle delay's
codecs, the search's order, accents and keys, the store's edits (only the
line, its comment kept, tables made, a value written back as it was), the
power source from the kernel's supplies, the recent changes kept, the
second launch's line, the ids' names; `hocus-config`'s edit through a link
with its mode; the user popover's rows; the launcher reading the
package's entry as "Réglages".
