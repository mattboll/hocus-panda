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

## Souris et pavé tactile (6.12)

Built 2026-11-05 (issue #446), variant A of
[the mockup](https://claude.ai/artifact/8huWi8Vafj8QZgLrCUT9aF), from the
spec in the issue's last comment. The maintainer decided: two groups,
the touchpad then the mouse, then the gestures, then a test area; the
gestures as switches on this page; "Coupé quand une souris est branchée"
kept, off by default; tap to click on by default.

- `src/mouse.rs` holds the rows, `src/app/mouse.rs` lays the page out
  and keeps the test area's state, `src/devices.rs` follows the devices.
- Under the title, a line links to Apparence et langue for the cursor's
  theme and size (6.2's keys; nothing is duplicated here).
- **PAVÉ TACTILE**, `[touchpad]` (every device that taps): Vitesse du
  pointeur `speed`, Toucher pour cliquer `tap-to-click`, Toucher puis
  glisser `tap-and-drag` (at 45 % while tap is off), Clic droit en
  appuyant `right-click` (`"two-fingers"` or `"corner"`; without the key,
  the touchpad's own, which the row shows), Désactivé pendant la frappe
  `disable-while-typing`, Défilement naturel `natural-scroll`, Défiler
  `scroll` (`"two-fingers"` or `"edge"`), Coupé quand une souris est
  branchée `off-with-mouse`.
- **SOURIS**, `[pointer]` (every other pointer): Vitesse du pointeur
  `speed`, Accélération `acceleration` (`"adaptive"`, `"flat"`),
  Défilement naturel `natural-scroll`, Gaucher `left-handed`.
- **GESTES DU PAVÉ TACTILE** (hidden without a touchpad): Trois doigts de
  côté `[touchpad] swipe-desktops` (windows.md §12: off, three fingers go
  to applications), Quatre doigts vers le haut `[touchpad] swipe-launcher`
  (written and read, but the four-finger launcher is 6.13's, #447: not
  built yet).
- The speeds are a bipolar slider "Lente … Rapide" (172 wide, a tick at
  the middle, the fill from the middle to the knob, no number), written
  as decimals from -1.0 to 1.0 450 ms after it stops; the toast and the
  search say très lente, lente, moyenne, rapide, très rapide. `←`/`→`
  move it a tenth, `Page Up`/`Page Down` a half, `Home`/`End` to its ends.
- The toast adds the device: "Défilement naturel, pavé tactile : oui".
- **The devices** are the compositor's: the window follows `hocus msg
  input-devices follow` on `$HOCUS_SOCKET` on a thread of its own. Each
  group's label line names its device by kind and bus, never its
  vendor's name: "Intégré au portable", "Souris USB", "Souris Bluetooth",
  "2 souris". With no mouse the group is one line, "Aucune souris
  branchée" and that its settings stay in the file; with no touchpad, the
  same, "Aucun pavé tactile", and no gestures. With `off-with-mouse` on
  and a mouse plugged, the touchpad's line says "Coupé : une souris est
  branchée" in amber. A row no device plugged can take (edge scrolling on
  a pad without it, a choice of right click on a pad with one way) is at
  45 %, "Pas sur ce pavé tactile". Started outside Hocuspanda, the window
  knows no device and shows every row.
- **ESSAYER ICI**: a list of 16 lines to scroll and a target to click.
  The label line says what was recognised: "Reconnu : pavé tactile" for
  a finger's pixels, "Reconnu : molette de la souris" for a wheel's
  notches (Wayland's axis source, as winit gives it); under the list, the
  direction in force for that device; under the target, the last press
  ("Clic droit", with ": boutons inversés pour un gaucher" when
  left-handed is on); a double click (the toolkit's time) lights its dot
  for 1.4 s. Nothing done there is written.

**The compositor's side.** `Config` gains `pointer` and `touchpad`
(kebab-case; a key not known refuses the file as elsewhere, a value that
does not read is logged and its default applies). The tty backend gives
them to each libinput device as it is plugged in (`input_devices.rs`;
tap with the left-right-middle map), skipping and logging what a device
cannot take, and again to every device when the file changes: a thread
of the compositor's follows `config.toml` with inotify and sends the loop
what it now reads (`reload.rs`; a file that does not read changes
nothing). `hocus msg input-devices` says each device, what it can and
what it was given; `--follow` stays and says each one plugged, changed or
unplugged (`hocus_link::input`). Headless and winit have no libinput:
tests plug devices of their own, which keep what they would be given.

Choices made here (to confirm on the machine): the dashed rims of the
empty groups and of the target are drawn solid (iced draws no dashes);
a USB mouse is "Souris USB" (a wireless receiver is not knowable); the
list's lines read "Ligne N de la liste".

## Clavier et raccourcis (6.8)

Built 2026-11-05 (issue #408), variant A of
[the mockup](https://claude.ai/artifact/K49KfhL6QjVzEfBo1kc5oB), from the
spec in the issue's last comment. The maintainer decided: the shortcuts
on a sub-page with their own search; while a shortcut is recorded the
Settings ask the compositor to suspend the desktop's own shortcuts
(keyboard-shortcuts-inhibit); "Rétablir" takes the line out of the file,
so that the default applies and follows future defaults; a family of
nine is one folded row that unfolds.

- `src/keyboard.rs` holds the rows and the 64 actions of `shortcuts.rs`
  (each a setting of its own: the search, the undo and the file's
  watching are 6.1's), `src/keys.rs` the chords as written, shown and
  recorded, `src/xkb.rs` XKB's registry (`evdev.xml`) and its French
  catalog, read on a thread; `src/app/keyboard_page.rs` lays out both
  pages.
- **The page**: DISPOSITIONS (each layout of `[keyboard] layouts`, its
  chip, its name, ↑, ↓, ✕; the system's when the file names none, the
  first change writing the list; "+ Ajouter une disposition" opens the
  registry's search, accents and case aside, `Enter` adding the first,
  `Esc` closing; "Changer de disposition", `grp:` of `[keyboard] options`,
  at 45 % with one layout), RÉPÉTITION DES TOUCHES (`repeat_delay` 100 to
  1000 ms by 50, `repeat_rate` 5 to 60 /s, and a field to try them),
  TOUCHES SPÉCIALES (Verr. Maj., `caps:`; Touche Compose, `compose:`,
  Verr. Maj. at 45 % while Compose takes it; Verr. Num. à l'ouverture,
  `[keyboard] numlock`), RACCOURCIS (one row, "N modifiés", that opens
  the sub-page). The options a menu does not own stay in the line, in
  their place.
- **The sub-page** ("Réglages › Clavier et raccourcis › Raccourcis",
  "‹ Clavier et raccourcis" back): its search (names in both languages,
  nearby words, the keys as shown and as XKB names them, the file's
  key), "64 raccourcis · N modifiés", "Tout rétablir"; FENÊTRES, BUREAUX,
  BARRE ET LANCEUR, CAPTURES, SESSION, TOUCHES MULTIMÉDIA, then FIXES.
  Each row: its keycaps, "+", and once changed a green dot, "Par défaut :
  …" and "↺ Rétablir". Desktops 1 to 9, send to and take to are one row
  each ("Super + 1 … 9", or "Propres à chaque bureau" once they differ),
  "Les 9" / "Replier"; a search shows the members.
- **Recording**: a click on a combination (or "+") records the next one
  pressed, the modifiers shown as they are held. The window holds the
  keyboard-shortcuts-inhibit protocol meanwhile (`src/activate.rs`, on
  the window's own connection, on a thread), so `Super+L` reaches it
  instead of locking. `Esc` cancels, `Backspace` removes the combination.
  Refused, in the row: a key that types without Super, Ctrl or Alt,
  `Ctrl+Alt+F1`…`F12`, one the action has, `Super+Space` while it
  switches the layouts. A combination another action holds opens the
  amber box under the row: "Super + L sert déjà à « Verrouiller la
  session ». Le remplacer le lui enlève.", "Remplacer" (both lines
  written, one toast, its "Annuler" taking both back) or "Annuler".
- Chords are written as the compositor reads them: Super, Ctrl, Alt,
  Shift, then the XKB name (`Super+Shift+E`, `Super+period`, `Print`);
  the top row's digits whatever the layout.

**The compositor's side.** `reload.rs` follows `[shortcuts]` and
`[keyboard]` too: the new shortcuts at once; new layouts or options make
a keymap compiled on the configuration's thread (XKB reads its files
there), which the loop takes in and the shell is told again (a keymap XKB
refuses is logged, the old one stays); the repeat is given to clients
again. `[keyboard] numlock = true` locks Num Lock in the keymap's state as
the session opens. The keyboard-shortcuts-inhibit global is served: an
inhibitor is granted at once, and while the surface holding the keyboard
has one, no shortcut runs (the virtual terminals' switch stays).

Choices made here (to confirm on the machine): layouts move with ↑ and
↓, not by dragging; the add popover opens in the card, under its row,
rather than floating; the Super+Space choice of the switch menu stays
choosable (refused when recording, not greyed in the menu); with no file
options the menus show "none", not the system's options; the keycaps'
2 px bottom is a shadow; a layout's line says its XKB code, not
"AZERTY"; Num Lock's LED on a physical keyboard follows on the first
key; the window's keyboard reaches combinations by the mouse only (not
`Tab`); on the CPU at scale 2 the sub-page takes a few seconds to draw
its 64 rows the first time.

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
  taken out when the file did not hold it, and a table the change made
  with it when it is left empty and says nothing (the file then is byte
  for byte what it was). "Annulé : … revient à 5 min." for 3.5 s, the row lit.
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

- What the desktop does with the rest of `[power]` (6.10). The
  compositor follows the file live for `[pointer]` and `[touchpad]`
  (6.12), `[keyboard]` and `[shortcuts]` (6.8); each other setting's
  ticket adds its own.
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

`crates/hocus-compositor/tests/settings_mouse.rs` (6.12), the same bed
with the harness's touchpad and mouse:

- `the_page_lists_the_touchpad_and_the_mouse`
- `unplugging_the_mouse_leaves_one_line_in_its_group`
- `natural_scrolling_on_the_touchpad_writes_touchpad_natural_scroll`
- `the_test_area_says_which_device_scrolled`
- `tap_and_drag_is_inert_while_tap_is_off`
- `annuler_takes_the_touchpad_setting_back`
- `searching_tap_finds_tap_to_click`
- `a_touchpad_gets_tap_to_click_by_default`
- `a_hand_edit_of_the_file_reaches_every_device`
- `swipe_desktops_off_keeps_three_fingers_off_the_desktops`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

`crates/hocus-compositor/tests/settings_keyboard.rs` (6.8), the same bed
with an XKB registry of the test's own:

- `the_shortcuts_are_on_a_sub_page_with_their_own_search`
- `desktops_one_to_nine_are_one_folded_row`
- `recording_a_shortcut_suspends_the_desktops_own_and_a_conflict_shows`
- `a_rebinding_takes_effect_live`
- `retablir_takes_the_line_out_of_the_file`
- `numlock_at_login_writes_keyboard_numlock`
- `a_layout_added_writes_the_list`
- `numlock_is_on_as_the_session_opens`
- `the_keyboards_layouts_and_repeat_follow_the_file`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

Unit tests: the sections and groups, the delays' and the idle delay's
codecs, the search's order, accents and keys, the store's edits (only the
line, its comment kept, tables made, a value written back as it was), the
power source from the kernel's supplies, the recent changes kept, the
second launch's line, the ids' names; `hocus-config`'s edit through a link
with its mode; the user popover's rows; the launcher reading the
package's entry as "Réglages".
