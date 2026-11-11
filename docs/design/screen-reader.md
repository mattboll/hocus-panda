# Screen readers (A.1, #421)

What a screen reader (Orca) reads of the shell: the bar and its popovers,
the notifications' bubbles, the lock screen and the authentication dialog.
The Settings and the launcher are separate issues.

Decisions (sprint A's questions, A.1): the shell builds its own AccessKit
tree (1: our own tree); the reader announces on its own only an agent that
starts waiting or gets stuck (2: A); the scope is the bar, its popovers,
the bubbles, the lock and the password dialog (3: B).

## The model

- iced exposes no accessibility tree (spike-iced.md, 6). The shell
  describes what it shows itself, from the model it draws, as AT-SPI
  windows of nodes: role, name, description, value (as words and as a
  number), states (checked, pressed, selected, expanded, disabled), place
  among its kind ("2 sur 4"), bounds and actions. Crate `hocus-a11y` turns
  them into AccessKit trees and hands them to AT-SPI (`accesskit_unix`,
  MIT OR Apache-2.0), one top-level window per surface, all under one
  application, which `accesskit_unix` names after the program:
  "hocuspanda-shell".
- **The bar's items are the keyboard's.** A bar's nodes are the items its
  surfaces register with the popovers' controller for the keyboard
  (`Controller::items_of`), in their order from the top and at their
  places: what the keyboard steps through is what a reader reads, and
  nothing else. Their names come from the same model as what is drawn.
- **Names are the words drawn, whole**: "62 %", not "62"; times as
  sentences ("14 h 32", "depuis 2 minutes"); in the bar's language, French
  or English.
- **Only when asked.** Nothing is built while no assistive technology
  listens: AccessKit's adapter asks for a tree only once the session's
  accessibility bus says one is enabled (`org.a11y.Status.IsEnabled`).
  Until then the shell's thread for it sleeps; there is no timer, and the
  counted budgets at rest do not change. `hocus_a11y::built()` counts the
  trees built.
- **Off the loop.** A thread of the shell owns the bridge; AccessKit's
  adapter runs its own D-Bus connection on a thread of its own. While a
  reader listens, the thread reads the model a few times a second (every
  250 ms, and at once on a reader's event), builds each window, and hands
  over only the windows that changed.
- **Only on a bus the compositor names**, as the notifications: the shell
  registers on the session's accessibility bus only when the compositor
  names a session bus (`--session-bus`, or the session's own on a virtual
  terminal) and that bus is the one in `DBUS_SESSION_BUS_ADDRESS`. A
  nested shell next to a live session never appears in that session's
  reader.
- **While the session is locked, the lock alone** is described: nothing
  behind it can be read.
- Bounds are in each surface's logical pixels (Wayland gives no window
  position).

## Windows, roles and names

| Window | Role | Name (fr) | States, value | Actions |
|---|---|---|---|---|
| A bar | frame | "Barre — Écran du portable" (its screen as the Settings name it) | active while an item holds the keyboard | — |
| Clock | button | "14 h 32, jeudi 6 novembre" | expanded while the calendar shows | click: the calendar |
| Next meeting | button | the card's name: "Prochaine réunion : Point produit, 15:00, dans 28 minutes, visio" | — | click |
| Its join button | button | "Rejoindre la visio" | — | click |
| In use | button | "Micro utilisé par Firefox, caméra utilisée par Zoom" | — | click |
| Today | list | "Aujourd'hui, 3 évènements" | items: buttons "15:00, Point produit, visio" | click |
| To do | list | "À faire, 5 tâches" | items: check boxes named by the task, checked when ticked; "Ajouter une tâche"; "4 autres tâches" | click |
| Agents | list | "Agents" | — | — |
| Its header | button | "Agents : 3 t'attendent, 10 au travail" | — | click: the briefing |
| An agent's row | button | "site-web, t'attend depuis 2 minutes" (au travail depuis, bloqué depuis, a fini, à voir, au repos) | — | click: goes to the agent |
| "+ N autres" | label | "5 autres agents au repos" | — | — |
| Windows | list | "Fenêtres, 3" | items "Slack — #produit, active", selected when focused; child button "Fermer Slack — #produit" | click |
| Desktops | radio group | "Bureaux" | radio buttons "Bureau 2, 1 fenêtre : Firefox"; the current one checked, "Bureau 1, actuel, 2 fenêtres : …"; place "2 sur 4"; another screen's prefixed by it | click: switches |
| Wi-Fi | button | "Wi-Fi, connecté à Livebox-A3F2, signal fort" | — | click |
| Bluetooth | button | "Bluetooth, Casque, batterie 80 %" | — | click |
| Keyboard layout | button | "Disposition du clavier, Français" | — | click: next layout |
| Volume | slider | "Volume" | "62 %" or "muet"; 0–100 by 5 | increment, decrement (5 %), click: its popover |
| Brightness | slider | "Luminosité" | "45 %" | increment, decrement, click |
| Battery | progress bar | "Batterie" | "82 %, 3 h 10 restantes, équilibré" | click |
| Footer | toolbar | "Pied de la barre" | buttons "Presse-papiers", "Notifications, 2 non vues", "Menu : toutes les applications", "Matthieu, menu de session" | click |
| Tray | toolbar | "Icônes des applications" | buttons named by the item's title | click, menu |
| A popover | frame, dialog | its title ("Son"), described by its subtitle ("Haut-parleurs du portable") | active and focused while it holds the keyboard | — |
| The sound's rows | toggle button "Couper le son" (pressed when muted), slider "Volume", radio buttons for the outputs (checked: the default, "1 sur 2"), the microphone's, "Couper le son de Firefox", "Réglages du son…" | | focus follows the keyboard's ring | |
| Every popover's rows | by the kit's kinds: switches (Wi-Fi, Bluetooth, do not disturb, "Au premier plan") as toggle buttons; sliders (brightness, the keyboard's backlight); choices as radio buttons (power modes, outputs, a window's desktops, a calendar's colours, the keyboard's levels); lines (a window, a notification, a clipboard item, an agent, an event) as list items or buttons with their details as description; fields (a task, the search, a Wi-Fi password and Planka's token as password text, never their value); section labels and notes as labels | | focus follows each popover's ring (the clipboard's: its selection) | as the keys do |
| A bubble | frame, notification | "Slack : Léa Martin", ", urgent" when critical; described by its body; a progress bar when it has one | said as it comes | click: its default action; its actions as buttons |
| The lock (its screen with the form) | frame | "Session verrouillée" | label: the clock; password text "Mot de passe", described by what the form says (wrong, Caps Lock, wait); button "Déverrouiller"; button "Disposition du clavier : Français. Changer"; label "2 agents t'attendent" | the field focused as the compositor says |
| The lock (other screens) | frame | "Session verrouillée" | the clock alone | — |
| The password dialog | frame, dialog | "Autorisation nécessaire : Mises à jour", described by polkit's message | "Changer" (several accounts), the accounts as radio buttons while listed, password text "Mot de passe d'Alex" described by the line under it, toggle "Détails", "Annuler", "Autoriser" | focus as the dialog's |

- The secretary and the pandas are decoration: no node of their own; what
  they mean is in the rows' names.
- A name from a tool (an agent's) is said as drawn, its control characters
  left out.
- Never a password: the fields have no value.

## Announcements

- When an agent starts waiting or gets stuck, a polite announcement with
  the peek's words: "site-web t'attend.", "infra est bloqué.". Nothing
  for working, finished, resting.
- Several within 2 s make one line: "site-web et docs t'attendent. infra
  est bloqué." (said 2 s after the first). One that stops waiting before
  is left out. With do not disturb on, none. What already waits when a
  reader starts is not announced (it is in the bar).
- It is a polite status node at the end of the first bar's window, new for
  each announcement (AT-SPI's `object:announcement`), and goes after 10 s.
- A bubble is announced as it comes (its notification node, polite).
- The clock, the levels and the counts change their nodes' values without
  announcing.

## A screen reader's actions

Done as the pointer or the keys would: a click on a bar's item is the
popovers' controller's press on it (opens its popover, or acts); the
volume's and the brightness' increment and decrement are the level keys;
a bubble's action, its action; the lock's and the dialog's buttons, a
press and a release on them. A popover's row: the keys that reach it and
act, as the popover takes them (`Tab` to it, then `Enter`, or the arrows
for a slider's step; the clipboard's selection by the arrows). A row the
keys cannot reach (the clipboard's pin) has no action.

Each popover says what it shows as it draws (`crate::a11y::rows`): its
rows, the order of its ring and where the ring is. It costs nothing at
rest: a popover draws only while it is open and changes. The popovers
built with the shell's widget kit (the Wi-Fi's, Bluetooth's, the sound's
and the music player's, [bar.md, "The shell's widget kit"](bar.md))
write no rows by hand: each control declares its role, name and state
where it is built, and the kit reads them from the laid-out tree, with
their bounds, their order and the ring; their keys are the controls' ids
(`sound:Mute`).

## Orca with the session

- `[accessibility] screen-reader = true` starts Orca (`orca --replace`) as
  the session's program as the session starts; `Super+Alt+S`
  (`[shortcuts] screen-reader`) starts or stops it, on the lock screen and
  in the password dialog too. Only when the compositor is the session (a
  virtual terminal): nested, nothing starts (it would read the session it
  runs in). Changing the key in the file starts or stops it.
- Stopped by its own process id: `SIGTERM`, then `SIGKILL` if it is still
  there 2 s later.
- The Settings' Accessibilité page has its row, in a group of its own,
  LECTEUR D'ÉCRAN: "Lecteur d'écran", a switch on `[accessibility]
  screen-reader` ([settings.md](settings.md)). The Keyboard page lists the
  shortcut in a group of its own, "LECTEUR D'ÉCRAN".

## `hocus msg a11y`

`hocus msg a11y` prints what the shell shows for screen readers, each
window's tree a node a line (role, name, value, range, description,
states, place among its kind, bounds, actions; the window's focus and
whether it is active), whether a reader listens or not: the compositor
asks the shell (`a11y`), which says it a line at a time (`a11y-line`,
`a11y-end`); "no shell" without one. The shell also logs each window's
tree as a reader gets it (`RUST_LOG=hocus_a11y=debug`, "the accessibility
tree").

## Testing

- `hocus-a11y`'s tests build trees and check them; `tests/atspi.rs` runs a
  session bus and an accessibility bus of its own (`at-spi-bus-launcher`,
  `at-spi2-registryd`), shows a bar, a popover, a lock and a bubble, reads
  them back with AT-SPI's own library and presses a button. Skipped
  without them.
- The shell's tests (`a11y::*`) check the names, roles, states and
  actions above in French and in English, the announcement rule, the
  lock, the password dialog and a bubble; `ui::keys`, the ring's order.
- `crates/hocus-compositor/tests/screen_reader.rs`: the real shell's tree
  through `hocus msg a11y` (the bar's sound a slider; its popover a
  dialog with its rows, holding the keyboard; `Tab` moving the focus;
  `Esc` giving the focus back to the bar's sound), and `Super+Alt+S`
  starting a fake screen reader, as on a real session, and stopping it.
- By hand, never on the live session's buses: a headless compositor with
  `--session-bus` on a dbus-daemon of its own, `at-spi-bus-launcher
  --launch-immediately` on it with `IsEnabled` set, then a reader (Orca,
  or AT-SPI's Python binding) with the same `DBUS_SESSION_BUS_ADDRESS`
  and `XDG_RUNTIME_DIR`.

## Not built yet

- A text field's caret and selection (iced does not expose them): fields
  are read by name (and a non-secret one by its text) only.
- The session does not set `GTK_A11Y` or `QT_LINUX_ACCESSIBILITY_ALWAYS_ON`
  while a reader runs: applications keep their own defaults.
