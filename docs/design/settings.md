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

## Son (6.5)

Built 2026-11-05 (issue #405), from the approved mockup
([variant A](https://claude.ai/artifact/LBfcAFNRETnVMi6Kj4HThi)) and the
spec in the issue's last comment. The maintainer decided on 2026-11-05:

- variant A: two groups of ticked rows, the output's then the input's,
  then the applications;
- no `[sound]` table in `config.toml`: PipeWire remembers everything;
- the microphone's meter listens while the input group is on screen, and
  the bar's in-use sign shows "Réglages" meanwhile;
- the alert sound and volume above 100 % stay out until their ticket (no
  "PLUS TARD" group).

**The page**, under its title and its line ("Les sorties, les entrées et
leurs profils ; le volume de chaque application."):

- **SORTIE**: the outputs' card, one row each in PipeWire's order (the
  sink's node id), the row one button: a 32 px tile (blue for the one in
  use) with the popover's icon (speaker, headphones, screen), the name
  (`node.description`, as the popover says it), a line of what it is
  ("Intégrés au portable" for a built-in or PCI device, "Bluetooth" with
  " · mode appel" in call mode, "Écran externe", "USB", else its device's
  description), its own volume ("45 %", "muet"), and the green check on
  the one in use. A click makes it the default (`wpctl set-default`),
  which keeps its own volume. Then the chosen output's card: its caption,
  "Volume" (mute button, 220 px slider of the sound's gauge, the value;
  0–100 %, applied as it moves, moving it unmutes), "Profil" (a menu when
  its device has more than one profile, its "Off" never offered; else the
  profile as text, "Le seul que propose cet appareil."; no row for a sink
  of no device), "Balance" ("G" … "D", −100 to +100 by tens, its fill
  from the centre; inert for a mono output) with "Tester" on its row.
- **ENTRÉE**: the same for the sources, "coupé" for a muted one; a
  Bluetooth headset in high quality whose microphone is not there yet is
  listed as "Micro du casque" ("… · le passe en mode appel (HFP)"): it
  switches the headset to its call mode's profile, then makes its source
  the default once it comes; choosing another input then takes the
  headset back to the profile it was in, if the page switched it. The
  input in use says " · Visioconférence l'utilise" (in `#F5A35B`) while an
  application records it. Its card: "Volume", and "Niveau": 24 segments
  of 7 px, 2 px apart, blue to `#8FB3FF`, the top tenth `#F5B44A`, empty
  while muted. The note under it.
- **APPLICATIONS**, "2 en cours" on the label's right: one row per
  application playing or recording now (grouped as 3.4b; a new one joins
  at the bottom, one goes with its last stream; none of the meter's own),
  its letter tile in the bar's colours, "Joue sur …" (+ " · enregistre
  …"), a menu ("Suit la sortie", with the output in use's name, then each
  output; `pw-metadata … target.object SERIAL`, `-d` to follow the default
  again), its mute button and a 140 px slider. None: "Aucune application
  ne joue de son." and its line. The note under it.

**What lives where.** Nothing of the page is in `config.toml`; the header
keeps "Enregistré". Choices (an output, an input, a profile, an
application's output) toast with "Annuler" and join `Ctrl+Z` with the
file's changes ("Sortie : Casque", "Musique joue sur Casque", "Annulé : la
sortie revient à Haut-parleurs.", the row lit); volumes, mutes and the
balance are heard at once, with no toast and no undo. Changes from
elsewhere (the popover, the keys) show as PipeWire says them. An output of
a device that goes while in use toasts "… déconnecté : le son passe par
…"; one that comes toasts that the sound goes back to it (with "Garder
…") or stays where it is (with "Passer à …").

**How it reads and writes.** The crate `hocus-sound` (no smithay, no
iced), shared with the bar: its follower runs `pw-dump --monitor` on a
thread of the Settings' own and sends the window what the page lists
when it changes; its setter's thread runs `wpctl` (`set-default`,
`set-volume`, `set-mute`, `set-profile`), `pw-cli set-param ID Props '{
channelVolumes: [ … ] }'` (the balance: the louder side at the volume;
and a volume on an output whose balance is not centred, which `wpctl`
would centre) and `pw-metadata` (an application's output), what was asked
meanwhile taken together. `pw-dump` says a metadata key taken away as an
update listing nothing: the metadata is then read again whole with
`pw-metadata -n default`. Before PipeWire 1.0.9 (Ubuntu 24.04 has 1.0.5),
`pw-dump` writes a metadata change out only at the graph's next change;
after writing an application's output, the setter runs `pw-metadata -n
default` once more, a client connecting after the write, so that the
page shows the output chosen at once.

**The meter** is a `pw-record` of the input in use (`target.object` its
serial, 16 kHz, one channel), its node `hocus-settings-meter` and its
application "Réglages" (given in `PIPEWIRE_PROPS`: PipeWire 1.0's
`pw-record` ignores `-P`), started and read on a thread of its own (raw
where `pw-record` has `--raw`, else past its container's header): the peak of each 33 ms on a
scale of decibels from −60, held and falling over 300 ms, sent only when
it changes (silence sends nothing). It runs while the page shows the
input group within the page's viewport and the microphone is not muted;
it stops when the page changes, the microphone is muted or the window
closes. Its capture stream, linked from the source, is what the bar's
in-use sign lists (5.12).

**"Tester"** plays, with `pw-play --target`, the freedesktop sound
theme's `audio-channel-front-left` then `-right` when installed, else a
0.4 s tone of 660 Hz on one channel then the other; each side 0.9 s, its
dot lit and its words ("À gauche…", "À droite…"); once in the middle on a
mono output.

**The search** finds the page's settings (`model::Outside`, settings that
live outside the file: their page draws their row `row:{id}`): "Sortie",
"Volume de la sortie", "Profil", "Balance", "Tester les haut-parleurs",
"Entrée", "Volume du micro", "Niveau du micro", "Volume des
applications", with the spec's nearby words; "casque" finds Sortie and
Entrée by « casque ».

**The keyboard**: the outputs' group is one `Tab` stop (`↑`/`↓` move the
ring, `Space`/`Enter` choose), then the mute button, the slider (`←`/`→`
5 %, `Page Up`/`Page Down` 25 %, `Home`/`End`), the profile's menu, the
balance (`←`/`→` 10), "Tester"; the input the same way; then each
application's menu, mute and slider.

**The home's card** says the output in use and its volume
("Haut-parleurs · 62 %").

## Réseau (6.6)

Approved 2026-11-05 (issue #406), from the mockup
([variant A](https://claude.ai/artifact/TJuioNUbbBZY96NtB2m5Lx#reseau)) and
the spec in the issue's last comment. The maintainer decided:

- variant A: the saved networks on a sub-page, the forms in a sheet over
  the page;
- airplane mode turns off Wi-Fi **and** Bluetooth (and mobile broadband),
  then turns back on what was on before, not everything;
- the networks around are listed on the page too;
- VPN in v1: a WireGuard or OpenVPN file imported, no form.

Built in two parts. **The first** (this one): airplane mode, the Wi-Fi,
the network in use, the saved networks, the networks around and the
sheet of a WPA or WPA3 network. **The second**: the hidden network, the
802.1X and WEP sheets, the cable, the VPNs, the bar's popover's "dans les
Réglages" rows and "Réglages du Wi-Fi…", the bar's airplane mode.

**The page**, under its title and its line ("Le Wi-Fi et ses réseaux
enregistrés, les réseaux masqués et d'entreprise, le câble, les VPN et le
mode avion."); rows with a 32 px tile (green for what is on or connected,
blue for airplane mode on):

- **Mode avion**, a card of its own: "Coupe le Wi-Fi et le Bluetooth ; un
  câble reste branché." and its switch; on, the line turns `#F5C46E`
  ("Activé : le Wi-Fi et le Bluetooth sont coupés.") and the tile blue.
  Without `/dev/rfkill`, the row is inert and says so.
- **WI-FI**: the switch (`WirelessEnabled`; "Aucune carte Wi-Fi." without
  an adapter; "Bloqué par l'interrupteur sans fil." and inert when a
  switch blocks the radio; in airplane mode "Coupé par le mode avion.
  L'allumer ici le rallume seul.", and turning it on unblocks the Wi-Fi's
  radio alone); the network in use (its signal, "Connecté · WPA2 · 5 GHz ·
  signal fort", "Adresse … · par la box …", the sign-in page's amber line
  behind one), "Déconnecter" and "Détails" (the sub-page, this network
  unfolded); "Réseaux enregistrés" with their count and a chevron.
- **RÉSEAUX DISPONIBLES** while the Wi-Fi is on: "Recherche…" with a
  12 px spinner while a scan runs; one 46 px row per network around but
  the one in use, strongest first (3.5's list), the order held while the
  pointer is over the list or the keyboard is in it; its lock, its line
  ("WPA2", "WPA3", "Ouvert · une page de connexion peut s'afficher",
  "Entreprise (802.1X) · …", "WEP · …" in `#F7B98A`, " · enregistré"),
  "Se connecter" (open or saved: joined at once, the row says
  "Connexion…" with a spinner, the others inert) or "Se connecter…" (the
  sheet). 802.1X and WEP networks' "Se connecter…" is inert until the
  second part. The note under it.
- **PLUS TARD**: "Partage de connexion Wi-Fi", inert, amber chip.

**The saved networks' sub-page**: "‹ Réseau", its title and line, one row
per saved Wi-Fi connection, the one in use first, then by
`connection.timestamp` ("Connecté maintenant · WPA2", "Il y a 2 h ·
WPA2", "Hier", "Il y a 12 jours · ouvert", "Jamais utilisé"). A click
unfolds one: "Se connecter seul" (`connection.autoconnect`), "Mot de
passe" (dots; the eye reads it with `GetSecrets`, never before; "Changer…"
makes the field editable, `Enter` saves it, `Esc` gives up), the 802.1X
identity, the details of the one in use (address, gateway, DNS, MAC
address, band and speed), "Oublier ce réseau". Forgetting deletes the
connection at once (left first if in use), toasts "« Café Lomi » oublié."
with "Annuler", and joins `Ctrl+Z`: the undo gives the connection back
with its settings, its UUID and the secrets that could be read
(`AddConnection`), and joins it again if it was in use ("… revient, sans
son mot de passe." when a secret did not read).

**The sheet** (a WPA or WPA3 network around): a 460 px panel over a
`rgba(7,9,15,0.62)` scrim, "Se connecter à « … »", the password field
(dots, its eye; the keyboard starts in it, what is typed before it takes
the keyboard goes in it), "Annuler" and "Se connecter" (green once 8
characters are typed; `Enter` connects). While it connects: "Connexion…"
and a spinner, the field inert; refused: "Mot de passe incorrect.
Vérifie-le et réessaie." under the field, its rim `#F2945B`, the sheet
kept; another failure: "La connexion a échoué. Réessaie."; joined: the
sheet closes. A connection added that fails is deleted (3.5's).

**What lives where.** Nothing in `config.toml`; the header keeps
"Enregistré". The connections live in NetworkManager; airplane mode in
rfkill, what was on before it in `$XDG_STATE_HOME/hocuspanda/airplane`.
Airplane mode, the Wi-Fi's switch and a network forgotten toast with
"Annuler" and join `Ctrl+Z`; connecting and disconnecting say themselves
on their rows. Changes from elsewhere (the popover, nmcli) show without a
toast. A change NetworkManager refuses toasts "Le système a refusé :
<its message>" when polkit refused it, else "NetworkManager n'a rien
changé : …"; the page keeps what NetworkManager says.

**How it reads and writes.** The crate `hocus-network` (no smithay, no
iced), the bar's NetworkManager follower moved out of the shell: a thread
of the Settings' own reads NetworkManager whole, then follows its signals,
and keeps the devices' IP configurations and the saved connections'
settings (never their secrets); the page gets its details
(`View::details`) when they change. Its asks: `RequestScan` each time the
page shows; `WirelessEnabled`; 3.5's joins; `Disconnect`; `Update2` (to
disk) for `connection.autoconnect` and a password, NetworkManager keeping
the secrets an update leaves out; `GetSecrets` for the eye and before a
`Delete`; `AddConnection` to give one back. **The radios**:
`hocus_network::rfkill` reads `/dev/rfkill` (the session's user has it
through logind's seat access) on a thread of its own, the radios said as
it opens then each change (a hardware key's too); airplane mode is every
Wi-Fi, Bluetooth and mobile broadband radio blocked; turning it on writes
which of those kinds were on, then blocks each kind
(`RFKILL_OP_CHANGE_ALL`); off unblocks those kinds only (all of them when
the file names none). NetworkManager and BlueZ follow rfkill by
themselves; the Bluetooth page's code is not touched.

**Without NetworkManager** on the system bus: 6.1's amber banner
"NetworkManager ne répond pas", the rows "Inconnu" at 45 %, inert; the
page fills in when NetworkManager gets an owner.

**The search** finds "Mode avion", "Wi-Fi", "Réseaux enregistrés" and
"Adresse IP" (the network in use's row) by the spec's words.

**The keyboard**: the switches, "Déconnecter", "Détails", the saved
networks' row, then the networks around as one `Tab` stop (`↑`/`↓`,
`Enter` or `Space` connects); on the sub-page "‹ Réseau", the saved
networks as one stop (`↑`/`↓`, `Enter` unfolds, `Delete` forgets), then
the unfolded one's switch, eye, "Changer…" and "Oublier". The sheet keeps
`Tab` among its field and its buttons; `Esc` closes it.

**The home's card** says the connection in use: "Wi-Fi · Maison",
"Câble", "Hors ligne", "Mode avion".

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
displays.rs`): the one watcher of the file (`reload.rs`, 6.12's) hands
the loop what it says, and the loop takes `[outputs]` and `[panel]
output` from it at once (windows.md §13). A file that does not read changes nothing.

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

## Apparence et langue (6.2)

Issue #402, variant A, as the issue's last comment specifies it;
decided 2026-11-05: dark and light themes in v1, the whole desktop
following; the 12/24 h and date formats here, once for the desktop; an
accent among six chosen hues and the wallpaper's. Section `appearance`
(`hocus settings appearance`), PERSONNALISATION's first row. Every change
applies at once to the whole desktop, the Settings window first: the
window is the preview.

| Group | Row | Control | Key |
|---|---|---|---|
| THÈME ET COULEUR | Thème | segmented "Sombre \| Clair" | `[appearance] theme = "dark" \| "light"` |
| | Couleur d'accent (its line starts with the colour's name) | six 24 px round swatches, the chosen one ringed (2 px of the window, 2 px in its colour; the accent's ring while it holds the keyboard) | `[appearance] accent = "green" \| "blue" \| "violet" \| "rose" \| "teal" \| "wallpaper"` |
| TEXTE | Taille du texte | slider, 80 % to 150 % by 10 | `[appearance] text-scale = 1.0` (0.8 to 1.5) |
| CURSEUR | Thème du curseur | menu: "Celui du système", then each Xcursor theme installed | `[appearance] cursor-theme` (`""`: the system's) |
| | Taille du curseur | a 72 × 72 preview of the arrow at that size, then segmented "24 \| 32 \| 48 \| 64" (a value off the four shows as a fifth segment) | `[appearance] cursor-size` |
| MOUVEMENT ET LISIBILITÉ | Mouvements réduits, Transparence réduite, Contraste élevé ("En partie") | switches | `[appearance] reduced-motion`, `reduce-transparency`, `high-contrast` |
| LANGUE ET FORMATS | Langue | menu "Celle du système (français)", "Français", "English" | `[locale] language = "system" \| "fr" \| "en"` |
| | Date | menu, each choice with today's date beside it: "En toutes lettres" (jeudi 5 novembre), "Abrégée" (jeu. 5 nov.), "En chiffres" (05/11/2026), "Année d'abord" (2026-11-05) | `[locale] date = "long" \| "short" \| "numeric" \| "iso"` |
| | Heure | segmented "14:05 \| 2:05 PM" | `[locale] time = "24h" \| "12h"` (none: the session's locale decides, 12 hours in the English of the countries that write it so) |
| | L'horloge de la barre | link "La barre ›" | (6.4's `[panel.clock]`) |

- **The keyboard**: 6.1's; the segmented controls and the swatches move
  and choose with `←` and `→`; the link opens with `Enter`.
- **The search** finds the rows by the spec's nearby words and their keys
  (`theme`, `accent`, `text-scale`, `cursor-size`…).
- **The language** (`src/lang.rs`'s words hold both): the window and the
  shell switch at once; `[locale] language` wins over `LC_ALL`,
  `LC_MESSAGES` and `LANG` for the desktop's own words, and is given as
  `LANGUAGE` to programs started afterwards.
- **The formats**: the compositor gives the shell `locale LANGUAGE DATE
  TIME` as the file changes; the bar's clock (over 12 hours "AM"/"PM" in
  the seconds' place: "2:25 PM"), its date line, the calendar's times, the
  next meeting's card, Today, the reminders and the lock's clock follow.
- **Text size** draws the Settings window that much larger (iced's scale
  factor); the bar keeps its sizes.
- **Choices made here (to confirm on the machine)**: the spec's "Police"
  and "Thème d'icônes" rows are not built (nothing in the desktop reads
  them yet: a ticket of their own); text size draws the Settings window
  larger, not yet the shell's popovers, launcher and notifications; the
  cursor's preview is a drawn arrow, not the theme's image; a menu near
  the page's bottom opens over its button; the compositor's own rim
  around the focused window keeps the pandas' green.

### The themes and the accent

Issue #402, decided 2026-11-05: dark and light themes in v1, the whole
desktop following; an accent among six (bar.md §13 gives every value).

- **The keys**: `[appearance] theme = "dark" | "light"` and `accent =
  "green" | "blue" | "violet" | "rose" | "teal" | "wallpaper"`; a word not
  understood refuses the file, as any key's.
- **Live**: `[appearance]` is followed as the file changes, through the
  compositor's one watcher (`reload.rs`, `appearance/mod.rs`): the theme
  and the accent go to the shell (`theme SCHEME ACCENT` on the line),
  which draws every surface again in them (the bar, its popovers, the
  launcher, the notifications, the lock screen, the authentication
  dialog, the capture's and the sharing's panels); reduced motion and
  reduced transparency to the bars and to what the compositor moves; high
  contrast to the focused window's rim; the cursor's theme and size to
  the cursor (its theme's thread started again) and to the programs
  started afterwards.
- **The Settings window** reads the same keys with the rest of the file
  and draws in them as soon as the file changes: it is its own preview.
  Its toast stays dark in both themes.
- **Applications** are told through the desktop's portal backend, which
  now serves `org.freedesktop.impl.portal.Settings` (version 2) beside
  the screen cast, on the same name (`hocuspanda.portal`;
  `hocuspanda-portals.conf` sends Settings to it, then to GTK's for the
  other namespaces): `org.freedesktop.appearance` `color-scheme` (1 dark,
  2 light), `accent-color` (the base, red, green, blue from 0 to 1) and
  `contrast` (1 with high contrast), each signalled with
  `SettingChanged` when it changes. GTK 4, libadwaita, Qt 6.5 and browsers
  read it; an application already open follows if it listens.
- **How the programs draw it**: their colours are `Ink`s (the shell's
  `theme.rs`, the Settings' `theme.rs`), constants that give their colour
  in the theme of the moment: a token, the accent, or a colour drawn for
  the dark theme that light reads by rule (bar.md §13). At rest nothing
  draws: a theme change is one message, each surface drawn once.
- **Choices made here**: the wallpaper's accent is the primary screen's
  wallpaper's (6.3, below; Nuit's `#7A96E0` by default); the windows' rims and grips, drawn over the wallpaper, keep
  their colours; the screen around a capture stays dimmed dark.

## Fond d'écran (6.3)

Built 2026-11-05 (issue #403), variant A of the mockup, the gallery, from
the spec in the issue's last comment. The maintainer decided: Nuit (the
desktop's background so far) plus Brume, Aube and Forêt, the same recipe
in other colours; a picture chosen in the file chooser is copied into
`~/.local/share/hocuspanda/wallpapers`; a background per screen or for
all; the lock screen shows the desktop's wallpaper darkened by default,
as before; the plain colour is six swatches; pictures are decoded on a
thread, never on the compositor's loop. Section `wallpaper` (`hocus
settings wallpaper`), PERSONNALISATION's second row. `src/wallpaper.rs`
(the keys, the page's settings for the search, its thread),
`src/portal.rs` (the file chooser), `src/app/wallpaper_page.rs` (the
page).

| Group | What | Key |
|---|---|---|
| APERÇU | segmented "Tous les écrans \| Chaque écran"; the screens side by side, bottoms aligned, each with its background and the bar's strip on the primary, "Écran du portable · Nuit" under it; with "Chaque écran" a click chooses the screen the gallery applies to (green ring) | `[wallpaper] per-screen = false` |
| FONDS | "Pour tous les écrans" / "Pour l’écran du portable"; tiles: Nuit, Brume, Aube, Forêt ("Nouveau"), Couleur unie, one's own pictures (✕ removes), "Choisir une image…" | `[wallpaper] picture = "night" \| "mist" \| "dawn" \| "forest" \| "plain" \| PATH`, or `[outputs.NAME] wallpaper` with per-screen |
| | Remplissage (a picture of one's own): "Couvrir l’écran", "L’image entière", "Centrer, sans agrandir" | `[wallpaper] fit = "cover" \| "contain" \| "center"` |
| | Couleur unie (the plain background, or around a picture not covering): six 24 px swatches, Nuit, Ardoise (default), Bleu profond, Sapin, Terre, Graphite | `[wallpaper] color = "#1b2233"` |
| ÉCRAN DE VERROUILLAGE | the lock's drawing (its source, the veil, the clock), a radio list: "Le fond du bureau, assombri" (default), "… flouté", "Une autre image"; then "Image du verrouillage", a list of the same backgrounds and "Choisir une image…" | `[wallpaper] lock = "dimmed" \| "blurred" \| "picture"`, `lock-picture` |

- **Writing**: a tile, the scope and the lock's picture are keys the page
  writes itself (one toast, "Annuler", `Ctrl+Z`: "Fond d’écran : Brume",
  "Fond de l’écran externe 34 pouces : Forêt", "Annulé : le fond revient
  à Nuit."); the fit, the colour and the lock are the window's rows.
  "Chaque écran" the first time writes each plugged screen's `wallpaper`
  from `picture`; "Tous les écrans" keeps them in the file for later.
- **Choisir une image…**: `org.freedesktop.portal.FileChooser.OpenFile`
  on the session bus (`DBUS_SESSION_BUS_ADDRESS`), from the page's
  thread: titled "Choisir une image pour le fond" (or "… pour le
  verrouillage"), modal, a filter "Images (JPEG, PNG, WebP)" on their
  three types, the pictures' folder (`XDG_PICTURES_DIR`, else
  `~/Pictures`) as `current_folder`. Chosen, the file is checked (its
  size read from its header), copied into
  `$XDG_DATA_HOME/hocuspanda/wallpapers` under its name ("NAME (2).jpg"
  when another file has it; the same file chosen again is not copied
  twice), and its copy written with `~/`; the green line says "Copié dans
  ~/.local/share/hocuspanda/wallpapers/Montagne.jpg". Not a picture:
  "Montagne.jpg ne s’ouvre pas comme une image.", nothing written.
  Cancelled: nothing. No portal: a toast says so.
- **One's own pictures** are the folder's JPEG, PNG and WebP files, in the
  order added. ✕ (or `Delete` on its tile) is refused while it is in use
  ("Montagne.jpg sert encore : choisis d’abord un autre fond."); else the
  copy is deleted, with "Annuler" (its bytes are kept until the window
  closes).
- **A hand-edited value**: a path that does not read draws Nuit (the
  compositor's journal says why) and the page says, in `#F7B98A` above
  the gallery, "Dans le fichier : « ~/Images/Ancienne photo.jpg »,
  introuvable ; Nuit s’affiche. Choisis un fond pour remplacer cette
  ligne."; no tile is chosen. A path outside the folder is used where it
  is. An unknown word, or a colour not `#RRGGBB`, refuses the file in the
  compositor and is "non compris" on the page, as 6.1's.
- **The keyboard**: the scope (`←`/`→`), each screen with "Chaque écran"
  (`Space`), the gallery as one stop (arrows move the ring, row by row;
  `Space`/`Enter` choose; `Delete` removes one's own), the fit's menu, the
  swatches (`←`/`→`), the lock's choices (arrows), the lock's picture
  (`Space` opens its list).
- **The search** finds "Fond du bureau", "Fonds fournis", "Choisir une
  image", "Un fond par écran", "Remplissage", "Couleur unie", "Écran de
  verrouillage" and "Image du verrouillage" by the spec's nearby words
  and their keys.
- **The accent "wallpaper"** is the primary screen's wallpaper's: Nuit's
  `#7A96E0`, Brume's, Aube's and Forêt's main glow's hue, the plain
  colour's, a picture's strongest hue, each lifted to the accents'
  lightness until its text reads at 4.5:1 (`AccentColours::lifted`). The
  compositor finds it as the primary's background is drawn and tells the
  shell (`theme SCHEME ACCENT WALLPAPER`, the three colours) and the
  portal; the window finds it from the file or its own thumbnail.

**How it is drawn.** `hocus_link::backdrop` takes a palette
(`Backdrop::Night`, `Mist`, `Dawn`, `Forest`: the same stops' places and
glows, other colours) and `hocus_link::wallpaper::render` draws any
background for a size: a built-in or the plain colour at a quarter of the
pixels, drawn stretched; a picture (the `image` crate's JPEG, PNG and
WebP, EXIF orientation applied) scaled and cropped to the screen's pixels
for its fit, on the colour for "contain" and "center". The compositor
(`src/wallpaper.rs`) asks it of a thread of its own for each screen whose
background, size or scale changed, as the screen is drawn next, and only
swaps the screen's buffer when it comes back: the previous stays
meanwhile (a screen's first frame draws a built-in at once, as before).
The shell draws the lock's on a thread of its own from `wallpaper` on the
line (each screen's picture, the lock's, the fit and the colour), the
window its thumbnails on the page's thread. At rest nothing is drawn.

**The compositor follows the file**: `[wallpaper]` and `[outputs.NAME]
wallpaper` through `reload.rs`; a screen's `wallpaper` is not part of its
layout (a change redraws its background, never sets its mode). `~/` in a
path is the session's home.

Choices made here (to confirm on the machine): the file chooser gets no
parent window (the Settings have no xdg-foreign handle yet), so it is not
attached to the window; the lock's picture's list opens in the card under
its row rather than floating; the screens' and tiles' rounded corners are
their rims' (iced does not clip pictures to them); a tile is not lit under
the pointer; a picture removed is put back by "Annuler" but not by
`Ctrl+Shift+Z`; the preview's dashed outline of a screen unplugged is
solid (as 6.9's).

## La barre (6.4)

Issue #404, variant A (the rows, a live preview of the bar beside them),
as the issue's spec and its "Decided (2026-11-05)" specify it: the 12/24
h format is not here (it is 6.2's `[locale] time`, the preview follows
it); one form row per screen; the order of Today, To do and the agents is
a setting with ↑ and ↓; the windows' switch is here; every `[panel]` key
is followed live (bar.md "6.4, the bar followed live"). Section `bar`
(`hocus settings bar`), PERSONNALISATION's third row.

| Group | Row | Control | Key |
|---|---|---|---|
| EMPLACEMENT | Côté de l'écran ("Nouveau": 10.5/700, `#8FB3FF` on its 16 %, radius 5) | segmented "Droite \| Gauche" | `[panel] side` |
| | Sur quels écrans (its line: one screen plugged, the primary's name, or a bar per screen; link "L'écran principal se choisit dans Écrans ›") | menu "L'écran principal \| Tous les écrans" | `[panel] screens` |
| FORME ("Pleine 196 px · mini 56 px") | one row per screen known (plugged, unplugged with a form remembered, or with `panel-form` in the file), named as Écrans names it; its line says what applies now | segmented "Auto \| Pleine \| Mini" | `[outputs.NAME] panel-form` |
| | Changer de forme à la main (the shortcut as bound now; link to Clavier et raccourcis' row) | the shortcut | `[shortcuts] toggle-bar` (read only) |
| CE QU'ELLE MONTRE, DE HAUT EN BAS ("Un groupe vide ne prend pas de place") | L'horloge et la date, Micro, caméra ou écran en cours, Bureaux, connexions, niveaux, boutons du bas | "Toujours" | |
| | La prochaine réunion | switch | `[panel] meeting` |
| | Aujourd'hui, À faire, Les agents, in the file's order | ↑, ↓ (28 × 28, at 30 % and inert at an end), switch | `[panel] order`, `today`, `tasks`, `agents` |
| | Les fenêtres | switch | `[panel] windows` |
| L'HORLOGE | Les secondes | switch | `[panel.clock] seconds` |
| | Quand la minute change | menu: Compteur, Rouleau, Volet, Néon, Sans mouvement | `[panel.clock] transition` |
| | Police de l'horloge | menu: "Martian Mono (fournie)", then the installed monospaced families (`fc-list :spacing=mono`, on a thread as the program starts) | `[panel.clock] font` |
| | Couleur de l'heure | a 12 px swatch, menu: Blanc doux, Vert, Bleu, Ambre (each with its swatch); a colour of the file's own shows "Personnalisée · #3DFFA2" and stays until another is chosen | `[panel.clock] color` |

- **Under the title**: "La transparence de la barre et les mouvements
  réduits se règlent dans Apparence et langue ›", which opens 6.2 with
  its row lit.
- **A screen's form**: written at once as that screen's `panel-form`
  ("Forme de la barre, écran du portable : Mini", "Annuler"); the
  compositor forgets the form chosen there with a click. In automatic,
  the line says the width ("Automatique : pleine, l'écran fait 2560 px de
  large (pleine dès 1796).") or the click's choice ("Automatique : mini,
  choisie d'un clic sur son bord.") with "Revenir à la forme de sa
  largeur", which asks the compositor (`bar-auto NAME` on the screens'
  line) to forget it: nothing is written in `config.toml`. A screen with
  no bar now adds "La barre n'y vient que si l'écran principal
  s'éteint."; one unplugged is at 45 % and inert. The compositor's
  screens' line now says each screen's bar (`bar`) and the form chosen
  with a click (`bar_chosen`).
- **The order**: ↑ or ↓ writes `[panel] order` whole ("Ordre de la barre
  : À faire, Aujourd'hui, Les agents", "Annuler"). `Space` or `Enter`
  on a button moves its group; the keyboard stays on that button as it
  follows its row, and goes to the other one at an end. `Alt+↑` /
  `Alt+↓` on a group's switch moves it too.
- **Nothing to show**: without a calendar (`[[day.calendars]]`) the
  meeting's and Today's rows, without a task board (`[[day.tasks]]`) To
  do's, say in amber "Rien à montrer : aucun agenda ni tableau de tâches
  branché." with "Agenda et tâches ›"; their switches stay.
- **The preview** (214 wide, 20 px right of the rows; drawn over the
  page at its place and held at the page's top as it scrolls): "APERÇU"
  and the laptop's screen's form ("Portable · pleine"); the screens as
  Écrans places them, at 1/30, each with its bar's strip on the chosen side and
  its caption ("Écran 2 · sans barre", "· débranché" dashed); then the
  laptop's bar drawn at its logical sizes (960 high) at 55 %, 8 px from a
  line standing for the screen's edge, on the chosen side (the line on the
  bar's outer side), with the mockup's sample content
  in the column's order, the groups shown or not, the windows over the
  desktops, the connections, the levels and the footer. Its clock is the
  real time with the chosen seconds, font, colour and the 6.2 time
  format, ticking while the page shows (each second with the seconds,
  else each minute); a transition chosen plays once on its minutes. A
  form change moves its width along the bar's switch (none with reduced
  motion). A click on one of its groups lights that group's row. Under
  it: "La barre de l'écran du portable, à 55 %. Un clic sur un groupe
  allume sa ligne."
- **The search** finds the rows by the spec's words and their keys;
  "Forme de la barre", "Changer de forme à la main" (super+b, toggle-bar,
  clic, bord) and "Ordre des groupes" (ordre, monter, descendre,
  ranger; it lights Today's row) are the page's own rows.
- **Choices made here (to confirm on the machine)**: the order's toast
  says "Ordre de la barre : …" as the spec asks while its search result
  is named "Ordre des groupes"; the colour's swatch sits beside the menu
  button, not inside it; the preview's bar is a drawing of the bar, not
  the shell's own surfaces; the sample agents are drawn as the mockup's
  heads, not the pandas' pictures.

## Compte (6.11)

Issue #411, variant A of the mockup (`s611`), as the issue's last
comment, approved 2026-11-05, specifies it, the pandas made larger than
the mockup's at the maintainer's word. Section `account` (`hocus settings
account`), SYSTÈME's second row. Nothing is written in `config.toml`: the
name and the avatar are AccountsService's, the password the system's
(`passwd`, PAM). The header keeps "Enregistré".

| Group | Row | Control | Where |
|---|---|---|---|
| PROFIL | the avatar's row: the avatar (88 px circle: the picture, or the initial on the accent's gradient), the name (18/700), "Sur l'écran de verrouillage, à l'ouverture de session et dans la barre." | "Changer l'avatar…" (image icon), opening the popover | `IconFile`; `SetIconFile` |
| | Nom affiché: "Ton nom complet, tel que le bureau te salue." | a 240 px field with a pencil; `Enter` or the keyboard leaving it saves (if changed and not empty), `Esc` puts it back | `RealName`; `SetRealName` |
| | Identifiant: "Le nom de connexion ; il ne se change pas ici." | Martian Mono 12.5, read only | `UserName` |
| | Type de compte: "Peut installer des logiciels et changer les réglages du système." / "Les réglages du système demandent le mot de passe d'un administrateur." | "Administrateur" / "Standard", read only | `AccountType` |
| SÉCURITÉ | Mot de passe: "Il ouvre la session, la déverrouille et confirme les actions d'administrateur." (without `passwd`: "Le mot de passe se change avec les outils du système.", the button inert) | "Changer…" (key icon), opening the sheet | `passwd` |
| | Verrouillage automatique: "Quand la session se verrouille d'elle-même." | link "Énergie ›", opening 6.10 with "Verrouiller après" lit | |
| PLUS TARD | Comptes en ligne (globe, amber chip) | inert | |

- **The popover** (400 px, under its button, its right on the button's):
  "LES PANDAS", the eight pandas at 76 px in a 4 × 2 grid, the current one
  ringed 2 px with the accent; a divider; "Choisir une image…" and
  "Retirer"; "Une image est recadrée en carré au centre ; « Retirer »
  remet l'initiale." A panda applies at once and closes it; toast
  "Avatar changé." with "Annuler" (`Ctrl+Z`), which sets the previous
  picture's bytes back. "Choisir une image…" opens the portal's file
  chooser (PNG, JPEG, WebP); the picture is cut to its centred square,
  made 512 px at most, written as PNG in `$XDG_RUNTIME_DIR` and set
  (AccountsService copies it; the file is removed). `←` `→` `↑` `↓` move
  in the grid, `Enter` chooses, `Esc` or a click elsewhere closes.
- **The pandas**: eight 512 px PNGs, the mockup's panda on eight
  backgrounds (`crates/hocus-settings/data/avatars`, drawn by
  `scripts/avatars/generate.py`), carried in the program and installed in
  `share/hocuspanda/avatars/` by `packaging/install.sh`; an installed
  panda is set by its path, else its bytes are written as a picture is.
  The current one is known by its bytes (AccountsService keeps a copy).
- **The sheet** (420 px): "Changer le mot de passe", its line, "Mot de
  passe actuel", "Nouveau mot de passe", "Encore une fois" (dots, an eye),
  under the last "Les deux sont identiques." or "Les deux ne sont pas
  identiques." as it is typed; the hint "Le système peut refuser un mot de
  passe trop court ou trop simple : sa raison s'affiche ici."; "Annuler"
  and "Changer" (green once the current is filled and both new ones
  match; "Changement…" while it runs, the fields inert). The current one
  refused: "Ce n'est pas le mot de passe actuel. Vérifie-le et réessaie."
  under it, its border `#F2945B`, the keyboard back in it, all of it
  chosen; the new one refused: PAM's reason under it ("BAD PASSWORD:"
  left out); anything else: "Le mot de passe n'a pas changé : …"; no
  answer for 10 s: "Le système ne répond pas." Success closes it; toast
  "Mot de passe changé." without "Annuler". Nothing of it joins `Ctrl+Z`.
- **`passwd`** (`passwd.rs`) runs on a thread of its own, on a
  pseudo-terminal of its own (its own session, the terminal its
  controlling one, echo off), its prompts answered in turn: a prompt is
  what it writes without ending its line, so the session's language does
  not matter; what it says between the new password and the next prompt
  is PAM's refusal; ending in an error before the new one is asked is the
  current one refused. The three values are `Secret`s (`secret.rs`: one
  buffer that never grows, zeroed when set again, cleared or dropped, its
  `Debug` saying nothing, not even its length), copied once for the
  thread, never logged; the window's log says only whether each field is
  filled.
- **AccountsService** (`accounts.rs`) is followed on a thread of its own,
  started the first time the page shows: `FindUserById` of the process's
  uid, its properties read again on its `Changed` (or
  `PropertiesChanged`), its picture read and made round there. Only a
  change reaches the window; at rest nothing draws. Without it, the rows
  are empty and inert. A refusal (polkit) toasts "Le système a refusé."
- **The search** finds "Avatar" (photo, image, panda, profil), "Nom
  affiché" (nom, prénom), "Identifiant" (login, utilisateur), "Mot de
  passe" (changer, sécurité, passwd), "Type de compte" (administrateur,
  sudo), and lights their rows.
- **Choices made here (to confirm on the machine)**: the avatar is 88 px
  and the pandas 76 px (the mockup's 72 and 52), for the pandas to show;
  the sheet's fields have an eye but no paste button (`Ctrl+V` pastes);
  "Changement…" has no spinner; `Tab` in the sheet goes from field to
  field; a picture removed toasts "Avatar retiré."; the home's greeting,
  the bar's popover and the lock still draw what they drew (the lock
  already reads AccountsService's picture).

## Date et heure (6.11)

Issue #411, variant A of the mockup (`s611`), as the issue's last
comment, approved 2026-11-05, specifies it. Section `date-and-time` (`hocus settings
date-and-time`), SYSTÈME's last row. Nothing is written in
`config.toml`: the time, the zone and the automatic time are systemd's
timedated's; their format is 6.2's `[locale] time` and `[locale] date`,
only read here. The header keeps "Enregistré".

| Group | Row | Control | Where |
|---|---|---|---|
| (none) | the clock's card (88 px): the time in Martian Mono 36 in the desktop's format, the date in words (15/700, "jeudi 5 novembre 2026"), "Europe/Paris · UTC+01:00, heure d'hiver jusqu'au 28 mars" (the next offset change from the zone's rules; "pas de changement d'heure" without one) | read only | timedated's `Timezone`, tzdata |
| HEURE | Mettre à l'heure automatiquement: green "Par Internet (NTP) · à l'heure", amber "… pas encore à l'heure", "Coupé : l'heure ne se corrige plus seule.", or "Aucun service de mise à l'heure sur cet ordinateur." (the switch inert, off) | switch | `NTP`, `NTPSynchronized`, `CanNTP`; `SetNTP` |
| | Date et heure: the date (118 px, "05/11/2026", or year first when `[locale] date = "iso"`) and the time (72 px, 24 or 12 h) in Martian Mono 12.5, at 45 % and inert while automatic; off, editable, "Tape la date et l'heure, puis « Régler »." and "Régler" (green, 28 px) | two fields and a button | `SetTime` |
| FUSEAU HORAIRE | Fuseau horaire: "Paris, France · UTC+01:00" | a 200 px button with the zone's id and "›", opening the picker | `SetTimezone` |
| FORMAT | Format de l'heure et de la date: "24 h · « jeudi 5 novembre ». Une fois pour tout le bureau : la barre, l'agenda, l'écran verrouillé." | link "Apparence et langue ›", opening 6.2 with "Heure" lit | `[locale] time`, `date` (read) |
| PLUS TARD | Fuseau horaire automatique (a switch, off), Formats régionaux | inert, amber chip | |

- **timedated** is followed on a thread of its own (`timedate.rs`, zbus on
  the system bus), started the first time the page shows: its properties
  read again on each `PropertiesChanged`, and `NTPSynchronized` (which it
  never says) every 5 s only while the automatic time is on and not right
  yet. Only a change reaches the window. Without timedated the rows are
  inert and the switch's line says "Le service de l'heure du système
  (timedated) ne répond pas."; the thread tries again, slower and slower,
  silently. The calls are interactive (polkit may ask, 7.5) and answered
  aside; a refusal toasts "Le système a refusé." and the page shows what
  timedated still holds.
- **The time set by hand**: "Régler" (or `Enter` in a field) sets the
  date and time typed, read in the zone shown; toast "Heure réglée :
  14:45." without "Annuler". `↑` / `↓` in a field step its day or its
  minute. The fields follow the clock until something is typed in them.
- **The picker** (a sheet, 440 px): "Fuseau horaire" with a globe, a
  search field holding the keyboard, up to 8 rows (44 px): the city and
  its country (the typed part marked as the search's results), "America/Toronto
  · UTC−05:00", the time there in Martian Mono. Nothing typed: the
  current zone, its country's, then the best known cities. Typed: the
  cities whose name (French or English) starts with it first, then a word
  of it, its country, the zone's id, an abbreviation now or in six months
  ("CET", "PST"), an offset ("UTC+1", "+05:30"); accents and case
  ignored; among equals the best known first. `↑` / `↓` move, `Enter`
  chooses, `Esc` or a click outside closes. "Choisir Montréal" sets the
  zone; toast "Fuseau horaire : Montréal (UTC−05:00)." with "Annuler",
  which joins `Ctrl+Z` and `Ctrl+Shift+Z`.
- **The zones**: tzdata's `zone.tab` (one line per country and zone, so
  Monaco is found by its name), the cities' and countries' names in
  French and English from CLDR (Unicode License v3), and a few large
  cities living in another's zone (Montréal, Mumbai, Genève…), in
  `crates/hocus-settings/data/zones/zones.tsv`, made by
  `scripts/zones/generate.py`; their offsets and changes computed with
  jiff from the system's tzdata.
- **At rest** the page draws once a minute: its clock, from a one-shot
  timer to the next minute while it shows.
- **The search** finds "Mettre à l'heure automatiquement" (ntp,
  synchroniser, internet), "Date et heure" (régler, changer l'heure),
  "Fuseau horaire" (pays, ville, utc, gmt, décalage) and "Format de
  l'heure" (12 h, 24 h, am, pm), and lights their rows.
- **Choices made here (to confirm on the machine)**: the clock is in
  Martian Mono Regular (the Light weight is not carried); `↑` / `↓` in a
  field step its day or its minute, not the part under the caret (iced
  does not say where the caret is); the date and time fields are reached
  with the pointer, `Tab` stops on the switch, "Régler", the zone's
  button and the link; a row of the picker shows its city's own name
  ("Montréal"), the zone's row the zone's ("Toronto"); the switch shows
  the value asked until timedated answers.

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
- **Whole frames where pixels could stay behind.** On the CPU, iced
  (tiny-skia) draws only what it finds changed between frames, and
  misses what is drawn past a piece's bounds: a shadow (the toast's), an
  anti-aliased stroke, a canvas's text moved with the page (6.4's
  preview). Those pixels stayed on screen after a scroll. For 150 ms
  after a scroll, a toast coming or going, and any motion, each frame's
  background is a 1024th of blue apart from the last one's
  (`app::background_of`), which makes iced draw the whole window; at
  rest nothing changes and nothing draws.
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
  No picture yet on the home (the Compte page sets it in AccountsService,
  6.11).
- The home's cards say "À venir" for Écrans, Réseau and Bluetooth until
  their tickets give their state (Son's says its output, 6.5); Énergie's says the battery (or "Sur
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
- Son (6.5): the meter's stream is not `node.passive` as the spec draft
  said: a passive stream never wakes the microphone, so the meter would
  stay empty unless another application recorded. It records only while
  the input group shows, which is what the sign says.
- Son: the applications' tiles are their letters on the bar's colours;
  their icons are not read yet. A Bluetooth headset's battery is not on
  its line yet (BlueZ's `Battery1` is read by the bar, not the Settings),
  nor an external screen's name and size from the compositor's outputs:
  the line says "Bluetooth", "Écran externe".
- Son: a menu at the page's bottom opens over its button when the window
  ends under it.

## Not built here

- What the desktop does with the rest of `[power]` (6.10). The
  compositor follows the file live for `[pointer]` and `[touchpad]`
  (6.12), `[outputs]` and `[panel] output` (6.9), `[keyboard]` and
  `[shortcuts]` (6.8), `[appearance]` and `[locale]` (6.2), `[wallpaper]`
  (6.3), `[panel]` and `[panel.clock]` (6.4, bar.md), through one watcher
  (`reload.rs`); each other setting's ticket adds its own.
- The pages whose tickets are not built yet (6.7, 6.10, 6.13 to 6.17, Day.1).
- Réseau's second part (#406): the hidden network, the 802.1X and WEP
  sheets, the cable, the VPNs, the popover's links, the bar's airplane
  mode.
- The popovers' "… settings" links straight to a section (each popover's
  ticket; `hocus settings SECTION` is there for them), but the sound's
  (6.5).
- Son: the alert sound and volume above 100 % (their ticket); the device
  events' toasts are not tested headless (null sinks have no device), nor
  the headset's call mode (null nodes have no profiles): the graph's and
  the page's unit tests cover them.

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

`crates/hocus-compositor/tests/appearance.rs` (6.2, the themes): the
real shell and Settings, the portal on a D-Bus daemon of the test's own:

- `switching_to_light_redraws_the_bar_and_the_calendar_in_light_live`
- `the_accent_changes_the_bars_accent_colour`
- `the_settings_window_draws_in_the_theme_and_follows_the_file`
- `the_portal_answers_color_scheme_and_signals_its_change`
- `once_the_switch_is_drawn_nothing_commits_at_rest`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

`crates/hocus-compositor/tests/settings_appearance.rs` (6.2, the page):

- `the_page_shows_its_five_groups_and_choosing_light_turns_the_desktop_light`
- `a_swatch_writes_the_accent_and_the_row_says_its_name`
- `twelve_hours_shows_2_25_pm_in_the_bar_the_calendar_and_the_lock`
- `the_language_switch_turns_the_shell_and_the_window_to_english_at_once`
- `at_rest_the_appearance_page_and_the_bar_commit_nothing`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

`crates/hocus-compositor/tests/settings_bar.rs` (6.4), on a compositor
of two screens (a laptop's 2560 × 1600 at 1, 3440 × 1440), the
Settings alone:

- `the_page_shows_its_groups_a_row_per_screen_and_the_preview`
- `gauche_writes_panel_side_and_the_bar_and_the_preview_move_to_the_left`
- `a_screens_form_is_written_at_once_and_annuler_takes_it_back`
- `back_to_the_form_of_its_width_forgets_the_click`
- `moving_to_do_up_writes_the_order_and_the_keyboard_follows_its_button`
- `hiding_today_writes_panel_today_and_a_group_with_nothing_to_show_says_so`
- `the_clock_rows_write_panel_clock_and_a_colour_of_the_files_own_stays`
- `searching_super_b_ordre_and_secondes_finds_their_rows` (and "gauche")
- `the_links_open_their_pages_with_their_row_lit`
- `at_rest_the_bar_page_draws_only_its_clock`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

Unit tests of 6.4: a group moved up or down, the file's forms and order,
`fc-list`'s families (`hocus-settings`). What the bar does with the keys
is `bar_live.rs`'s (bar.md).

`crates/hocus-compositor/tests/settings_wallpaper.rs` (6.3), on a
compositor of two screens (1920 × 1080, 1280 × 800), the real shell and
Settings, the file chooser a fake portal on a D-Bus daemon of the test's
own:

- `choosing_brume_redraws_the_desktops_background`
- `a_picture_chosen_in_the_file_chooser_is_copied_and_drawn`
- `each_screen_takes_its_own_background`
- `the_lock_shows_the_desktops_wallpaper_dimmed`
- `the_lock_takes_a_picture_of_its_own`
- `a_swatch_colours_the_plain_background`
- `the_wallpaper_accent_takes_the_wallpapers_colour`
- `at_rest_the_wallpaper_page_commits_nothing`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

Unit tests of 6.3: the palettes, the words, the line, a picture covering,
contained or centred and its accent, a file that is no picture, the blur
(`hocus-link`); the file's screens' pictures, the copy's name
(`hocus-settings`); the accents lifted reading on both themes.

`crates/hocus-compositor/tests/settings_sound.rs` (6.5): the same window
against a PipeWire of the test's own (null sinks and a null source;
WirePlumber's part a `pw-metadata` of the test's; the meter's stream
linked by the test as a session manager would; never WirePlumber nor the
session's PipeWire), the shell too where the bar is part of it:

- `the_sound_page_lists_the_sinks_and_sources_with_the_default_checked`
- `choosing_an_output_sets_the_default_and_annuler_takes_it_back`
- `a_volume_and_a_mute_moved_on_the_page_reach_pipewire`
- `an_application_playing_joins_the_list_and_its_device_can_change`
- `no_application_says_aucune_application_ne_joue_de_son`
- `the_meter_listens_only_while_the_input_shows`
- `the_popovers_link_opens_settings_on_son`
- `at_rest_the_page_commits_nothing`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

Unit tests of 6.5: the graph's devices and profiles (never "Off"), a
stream's target and the chosen defaults, a metadata key taken away, the
channels' volumes, the setter's commands (balance, profile, target, a
volume keeping the balance), the meter's scale, the tone's side
(`hocus-sound`); the balance read and written, a headset's microphone
offered by its call mode, the page's settings found by their words
(`hocus-settings`); the sound popover's height and its link's place
(`hocus-shell`).

`crates/hocus-compositor/tests/settings_network.rs` (6.6): the real
Settings against a NetworkManager of the test's own on a private system
bus (`levels/nm.rs`) and radios of their own (`levels/rfkill.rs`, a Unix
socket given as `HOCUS_RFKILL`):

- `the_network_page_shows_the_connected_network_and_the_ones_around`
- `an_open_network_joins_from_its_row_and_disconnect_leaves`
- `the_wifi_switch_toasts_and_annuler_turns_it_back_on`
- `a_saved_networks_password_shows_with_the_eye_and_changes_with_enter`
- `forgetting_a_network_deletes_it_and_annuler_brings_it_back_with_its_password`
- `a_change_polkit_refuses_is_said_and_nothing_changes`
- `a_wrong_password_in_the_sheet_says_so_and_keeps_the_field`
- `airplane_mode_blocks_wifi_and_bluetooth_and_restores_what_was_on`
- `without_networkmanager_the_page_is_read_only`
- `at_rest_the_page_draws_nothing`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

Unit tests of 6.6: the radios' airplane mode and what was on kept once
(`hocus-network`); the rows' words, the saved networks' order and age,
the home's card, the page's settings found by their words
(`hocus-settings`).

`crates/hocus-compositor/tests/settings_screens.rs` (6.9), on a compositor
of two screens (2560 × 1600 at 1, 3440 × 1440):

- `the_arrangement_is_drawn_from_the_compositors_screens`
- `dragging_the_external_above_the_laptop_writes_both_places`
- `the_scale_is_written_at_once_and_annuler_puts_the_file_back`
- `a_resolution_kept_with_garder_is_written`
- `a_resolution_not_kept_goes_back_after_15_s_on_the_held_clock`
- `the_compositor_follows_the_files_screens`
- `windows_go_along_with_their_screen`
- `a_window_goes_back_to_its_screen_turned_on_again`
- `searching_hidpi_and_miroir_finds_the_scale_and_mirroring`
- `at_rest_the_screens_page_commits_nothing`
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

`crates/hocus-compositor/tests/settings_account.rs` (6.11, Compte), the
same bed with AccountsService faked on a D-Bus daemon of the test's own
(`levels/accounts.rs`), the portal's file chooser on it, and a fake
`passwd` first on the `PATH` (never the system's: without it, the `PATH`
holds no system folder):

- `the_account_page_shows_the_name_login_type_and_avatar`
- `choosing_a_panda_sets_the_icon_file_and_annuler_takes_it_back`
- `a_picture_is_cropped_square_before_it_is_set`
- `the_display_name_is_saved_on_enter`
- `a_wrong_current_password_is_said_on_its_field`
- `a_new_password_refused_by_the_system_shows_its_reason`
- `the_password_is_changed_and_never_reaches_the_log`
- `a_refusal_by_polkit_leaves_the_values_unchanged`
- `without_passwd_the_button_is_inert`
- `the_lock_link_and_the_search_light_their_rows`
- `at_rest_the_page_commits_nothing`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

`crates/hocus-compositor/tests/settings_time.rs` (6.11, Date et heure),
the same bed with timedated faked on a D-Bus daemon of the test's own
(`levels/timedate.rs`):

- `the_page_shows_the_clock_the_zone_and_the_automatic_time`
- `not_on_time_yet_and_no_time_service_are_said`
- `automatic_time_off_lets_the_time_be_set`
- `the_time_zone_picker_finds_montreal_by_mon_and_sets_it`
- `the_picker_moves_with_the_arrows_and_closes_with_esc`
- `a_refusal_by_polkit_leaves_the_values_unchanged`
- `a_zone_changed_elsewhere_shows_at_once`
- `without_timedated_the_page_says_so`
- `the_format_link_and_the_search_light_their_rows`
- `at_rest_the_page_draws_once_a_minute_only`
- `screenshots_for_the_pull_request` (ignored; `HOCUS_SHOTS=DIR`)

Unit tests: the sections and groups, the delays' and the idle delay's
codecs, the search's order, accents and keys, the store's edits (only the
line, its comment kept, tables made, a value written back as it was), the
power source from the kernel's supplies, the recent changes kept, the
second launch's line, the ids' names; the zones' offsets, next changes
and the picker's search, the dates and times typed; a picture cut
square, the pandas known by their bytes, `passwd`'s reasons, the
`Secret` never said; `hocus-config`'s edit
through a link with its mode; the user popover's rows; the launcher
reading the package's entry as "Réglages".
