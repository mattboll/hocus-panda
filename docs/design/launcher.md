# The launcher — specification (5.1, 5.2)

Status: 5.1 approved 2026-11-02 (issue #391), variant C of the 5.1
mockup: full screen; 5.2 approved 2026-11-02 (issue #392), variant A of
the 5.2 mockup: an answer card above the grid (§6). Built since
2026-11-02: see [as built](#as-built). Sizes are logical pixels; they
follow each screen's scale. Later in the same field: files and AI; room
is left for other kinds of results.

## 1. Opening and closing

- `Super+Space` (`[shortcuts] launcher`) opens it on the screen under the
  pointer, and closes it again. `Escape` closes it, and so does a click
  outside the field and the tiles.
- The desktop darkens (about 62 % of `#06080E`) and blurs behind it. The
  bar is covered too.
- It fades in over 160 ms with a scale from 0.97 to 1, and fades out over
  120 ms. With reduced motion, it only fades.
- It holds the keyboard while it is open.

## 2. The field

- Centred near the top: 460 wide, 54 tall, radius 12, a green border
  (`rgba(108,194,135,0.45)`) on `rgba(255,255,255,0.05)`.
- Inside, 14 px in from the left and 12 from the right: a magnifier
  (18 px, `text.dim`), 10 px, the text in Manrope 19 px (`text`), the
  placeholder "Chercher une application" / "Search for an application"
  (`#7C8698`), and an "Échap" / "Esc" hint at the right (11 px 600,
  `#7C8698`, a 1 px `rgba(255,255,255,0.14)` frame, radius 6, 6 px in).
- The field has focus as soon as it opens: the caret shows just before the
  placeholder, then after what is typed.

## 3. The grid

- The panel is 760 wide, centred across the screen, its top 54 px from
  the screen's: the field, 22 px, then the grid's area, down to 98 px from
  the screen's bottom (the mockup's laptop screen: 480 px).
- 8 columns of tiles, each 92 × 106 with radius 12, the rows 6 px apart.
  A tile holds a 48 px icon 9 px from its top and the name 7 px under it,
  in Manrope 11.5 px with 15 px lines, `#C9D0DC`, 4 px in from its sides.
- A name takes **at most two whole lines**. Beyond that it ends with "…",
  and the full name shows as a tooltip on hover.
- **Empty field**: a "FRÉQUENTES" / "FREQUENT" row (the 8 most launched
  applications), then "TOUTES LES APPLICATIONS" / "ALL APPLICATIONS" from
  A to Z. The headings: 24 px rows, 11 px 700 capitals spaced by 0.08 em,
  `#7C8698`, 4 px in.
- **Typing** filters the grid in place, best match first, and the first
  tile is selected.
- **The selected tile** has a green border and tint
  (`rgba(108,194,135,0.6)` / `0.16`).
- **Hover** selects a tile. The arrows move the selection in two
  dimensions. The wheel scrolls by rows. A thin scroll mark (3 px,
  `rgba(255,255,255,0.22)`, 14 px right of the area) shows when the grid
  overflows.
- `Enter`, or a click, launches the application and closes the launcher.
  No results shows "Aucune application pour « … »." / "No application for
  "…"." (13.5 px, `text.faint`, centred, 26 px under the field's gap).

## 4. Search

- Fuzzy matching on the name, the generic name and the keywords of the
  desktop entry. Word starts and consecutive letters count more.
- Ties go to the most launched application. Launch counts are kept on
  disk in the user's state directory.
- Accents and case are ignored.

## 5. Applications

- They are the desktop entries from the XDG data directories, with the
  usual precedence. `NoDisplay`, `Hidden` and `OnlyShowIn` / `NotShowIn`
  are honoured. The locale's name is used.
- **Icons** come from the user's icon theme, falling back to hicolor, SVG
  or PNG at the right size. With no icon, a neutral rounded square with
  the first letter.
- **Launching** works like the compositor's other spawns: off the loop, as
  a session child, with field codes stripped. The application opens on the
  current desktop.
- Directories are watched: an application installed while the session
  runs shows up.

## 6. Calculation, conversions, web search (5.2, variant A)

**What the field understands**

- **Calculation**: `+ − × ÷ ^ %`, parentheses, square root (`sqrt`,
  `√`), comma or point as the decimal mark, `x`/`×`/`*` and `÷`/`/`. A
  leading `=` forces calculation. Only an input that parses as an
  expression with an operator counts; a bare number does not.
- **Conversion**: `<number> <unit> en|to|vers|-> <unit>`, with French and
  English unit names and symbols, accents and case ignored. Units:
  length, mass, time, volume, temperature, data (decimal Ko/Mo/Go/To and
  binary Kio/Mio/Gio; KB/MB/GB as aliases), currencies. Currency rates
  come from the European Central Bank's daily reference rates, fetched
  once a day off the loop and kept on disk. Offline, the last known rates
  are used, and the card says their date.
- **Address**: `scheme://…`, `localhost[:port][/…]`, or
  `name.tld[:port][/…]` with no spaces. It opens in the default browser
  (`xdg-open`).
- **Web search**: always offered last when the field is not empty.
  DuckDuckGo by default; the setting comes with the settings sprint.
  `Ctrl+Enter` searches the web whatever is selected.

**Layout**

- **The answer card**: a calculation's or a conversion's result shows
  under the field, 736 wide, 68 tall, radius 14, selected first: a 40 px
  green badge with "=", the result in Martian Mono 26 px, and a line
  under it: the expression, or "10 km = 6,214 mi · Longueur" / "Length".
  "Entrée copie" / "Enter copies" shows at the right while it is
  selected.
- **An address**: a 54 px card, with a link badge, "Ouvrir github.com"
  and its full URL.
- **The applications**: the 5.1 grid follows, as before. When the input
  is an answer or an address, no application is listed.
- **The web row**: last, 50 px tall, with a globe badge, "Chercher « … »
  sur le web" and "DuckDuckGo · Ctrl+Entrée…".
- **Selection**: the arrows move between the cards, the grid rows and the
  web row. The first item is selected.

**Actions**

- `Enter` on a result copies it to the clipboard, formatted for the
  locale (`6,214`), and closes the launcher. A toast "Copié : 6,214" /
  "Copied: 6.214" shows at the bottom for about 1.8 s.
- `Enter` on an address or the web row opens the browser and closes the
  launcher.
- Results are formatted for the user's locale: up to 6 decimals, with
  the thousands separator.

## As built

### 5.1, find and open an application (2026-11-02)

- **Who does what.** The compositor takes `Super+Space` and tells the
  shell `launcher OUTPUT SCALE` (the screen under the pointer and its
  scale); the shell opens the launcher there, or closes it when it is
  open. The launcher is a layer surface of the shell's
  (`hocus-launcher`), in the overlay layer, anchored to the screen's four
  edges, holding the keyboard (`exclusive`). The shell draws the field and
  the grid, transparent around them; the compositor draws, under that
  surface, the dim (`#06080E` at 62 %) over the whole screen and, with
  glass, the screen behind blurred (CSS's `blur(14px)`: 28 px of reach,
  no saturation, in a pane of its own), the bar and its popovers included.
  It fades it all in over 160 ms (`ease-out`) from the surface's first
  frame, the surface growing from 0.97 about the panel's middle
  (`cubic-bezier(0.2, 0.8, 0.2, 1)`, the mockup's), and out over 120 ms
  (`ease-in`) once the shell says `closing hocus-launcher`; with reduced
  motion it only fades. The shell gives the keyboard back as it closes,
  and removes the surface 120 ms later. A press on another screen while it
  is open closes it (`outside`) and goes no further, as a menu's; the
  bar's edge does not switch the bar's form under it.
- **Launching.** The shell sends `launch` with the entry's id, whether it
  runs in a terminal, its working directory (`Path`) and its arguments,
  one per field, the field codes taken out (`%f %F %u %U` and the
  deprecated ones gone, `%i` the icon, `%c` the name, `%k` the file). The
  compositor starts it as the terminal of `Super+Return` is started: on a
  thread of its own, without a shell, as its child, with an activation
  token so that its first window takes the focus on the current desktop.
  An entry with `Terminal=true` runs in the terminal (`[programs]
  terminal`, else the first found), after `-e`.
- **The applications** are read by the shell on a thread of its own as it
  starts: each `applications` directory of `XDG_DATA_HOME` then
  `XDG_DATA_DIRS`, the first file of an id winning (a hidden one hides
  those after it), in the messages' locale (`LC_ALL`, `LC_MESSAGES`,
  `LANG`; `Name[fr_FR]`, then `Name[fr]`, then `Name`), for the desktops of
  `XDG_CURRENT_DESKTOP` (`Hocuspanda`, set by the compositor); `TryExec`
  is honoured too. The directories (and the one above a directory not
  there yet) are watched with inotify; after 200 ms without a change, they
  are read again, and an open launcher shows what changed. Our own reading
  of the specification: `freedesktop-desktop-entry` is MPL-2.0, outside
  decision 0002.
- **The launch counts** are in `$XDG_STATE_HOME/hocuspanda/launches` (one
  line each: the count, a tab, the id), read as the shell starts and
  written on a thread of its own after each launch.
- **The search** folds names to lower case without accents (Unicode's
  decomposition, `unicode-normalization`) and scores as the mockup does:
  each letter of the query in order, a word's start or the letter right
  after the last one found counting more, longer names a little less; the
  name's score whole, the generic name's at 0.75, the keywords' at 0.6.
  Equal scores go to the most launched, then by name.
- **The icons** are looked for on a thread of their own as the launcher
  opens, in the grid's order, at 48 px on the screen's scale: in the
  user's theme (`gtk-icon-theme-name` of `gtk-4.0/settings.ini`, then
  `gtk-3.0`'s; else KDE's `kdeglobals`; else Adwaita, GTK's default, when
  it is there), the themes it inherits, `hicolor`, then `pixmaps`, under
  `~/.icons` and each data directory's `icons`, as the Icon Theme
  Specification looks them up (a directory of the size first, else the
  closest); a theme without `index.theme` is read from its directories'
  names. SVGs are drawn with resvg (iced's own) at the icon's pixel size,
  PNGs scaled to it, so that the thread that draws only shows them. An
  icon not read yet is left blank for that moment; none: the neutral
  square (`#5C6576 → #9AA3B4`, a 1 px light rim) and the name's first
  letter in white, Manrope 700 19 px. When the applications change, the
  icons not found are looked for again.
- **Drawing.** As for the popovers, nothing goes through iced's messages:
  the launcher's state is the shell's, keys and the pointer change it and
  draw its surface alone; the applications' and the icons' threads wake
  it through the compositor (`wake hocus-launcher`). At rest it draws
  nothing.
- **Choices made here** (to confirm on the machine): the frequent row
  shows only applications launched at least once, and is left out before
  any launch; the tooltip shows after 500 ms on a tile whose name is cut,
  under it (12 px, a dark frame); the field keeps the end of a long query
  in view ("…" before it); `Ctrl+Backspace` deletes a word; the caret does
  not blink. With a shell on the CPU (whose surfaces have no
  transparency), the launcher fills the screen with `#0A0E18` instead of
  showing the dimmed desktop.
- **Measured headless** (`crates/hocus-compositor/tests/launcher.rs`), the
  real shell with applications, icons and launch counts of the test's own:
  one scenario per point above. On the Intel GPU, the screenshots of the
  pull request.
- **Not built**: names for screen readers (§11 of the bar's spec, the
  Accessibility milestone); D-Bus activation (`DBusActivatable`) and the
  entries' actions; pasting into the field; input methods' preedit.

### 5.2, calculation, conversions, web search (2026-11-02)

- **What counts** (`crates/hocus-shell/src/launcher/answers.rs`, our own
  reading of the mockup's logic): a calculation first, then a
  conversion, then an address; anything else lists the applications.
  `%` is a hundredth of what it follows (`200 × 15%` is 30), `^` goes to
  the right. A conversion's units are one word each, a quantity's
  thousands may be spaced (`1 000 g en kg`); `in` is the inch, so the
  words between the units are `en`, `to`, `vers` and `->` only. Time
  goes from milliseconds to weeks (no months nor years: their length
  varies); the gallon is the US one; `b` is the byte (bits are not
  converted). The currencies are the bank's (about thirty), by their
  code, `€ $ £ ¥`, `euro`, `dollar` and `yen`; `$` is the US dollar. Two
  units of the same kind and different only.
- **Numbers** follow `LC_NUMERIC` (then `LC_ALL`, `LANG`): French writes
  `1 234,5` (a narrow no-break space between thousands), English
  `1,234.5`. The value keeps its twelve first significant digits (`0.1 +
  0.2` is `0.3`), then up to six decimals: "6,213712 mi" for 10 km, the
  spec's "6,214" being shortened. What `Enter` copies is the value so
  written, without its unit. The units show as the card writes them:
  `°C`, `€`, `Ko` in French and `kB` in English, `j`/`d`, `sem`/`wk`.
- **The rates** (`rates.rs`) are the bank's file as it is, in
  `$XDG_STATE_HOME/hocuspanda/ecb-rates.xml`, read on a thread as the
  shell starts. When the launcher opens and the file is 24 h old or more
  (by its time on disk) or missing, a thread fetches it again, with a
  30 s timeout, and an open launcher shows the new rates; after a failure
  it is not tried again for an hour. Nothing is fetched before the
  launcher opens. The client is `ureq` over `rustls` (ring) with the
  system's certificates; the file is read with `roxmltree`. A currency
  conversion's line ends with the rates' day: "Monnaie, taux du
  30/10/2026" / "Currency, rates of 2026-10-30", always, not only
  offline. With no rates at all, a currency is not a unit and the
  applications are searched instead.
- **The cards** are rows of the grid of their own (the 5.2 mockup's row
  heights: the card and 8 px under it), so that the arrows, the wheel and
  the selection kept in view treat them as the tiles' rows; a card's
  column is the first. Hover selects a card, a click acts on it. The
  hint at the right ("Entrée copie", "Entrée ouvre", "Entrée cherche")
  keeps its room unselected, as the mockup's. A line too long for the
  card ends with "…". The badges are drawn: "=" in Martian Mono 20 px on
  `#6CC287`; the mockup's link on `#2E7D6B → #5CC0A6`, its globe on
  `#3B5BA9 → #6E8FE0`, each in a light rim.
- **Copying** goes through the compositor's selection, as the Sources
  popover's command does (`copy TEXT`): the shell's surfaces have no
  serial to set it themselves.
- **The toast** is a layer surface of the shell's of its own
  (`hocus-launcher-toast`), in the overlay layer, anchored to the bottom
  of the launcher's screen 40 px up, taking neither the keyboard nor the
  pointer: 13 px 600 text, 10 px by 16 px in, a 1 px green rim
  (`rgba(108,194,135,0.45)`) on `rgba(14,17,28,0.94)`, radius 12. The
  shell draws it once and removes it after 1.8 s (and 100 ms); the
  compositor plays the mockup's `toast-in` from its first frame: in over
  216 ms rising 8 px, out over the last 270 ms, `ease-out` each, nothing
  drawn in between but the frame its fade out starts in. With reduced
  motion it only fades. On the CPU it is opaque and the compositor cuts
  its corners.
- **Opening**: the shell sends `launch` with `xdg-open` (looked for in
  its own `PATH`, sent with its full path) and the address, or
  `https://duckduckgo.com/?q=…` (the query form-encoded); the compositor
  starts it as it starts the applications, off its loop, with an
  activation token.
- **The search** (5.1, fixed here): a match is left out when one of its
  letters is neither at a word's start nor right after the one before
  it, unless the query is in the name whole; and the best score must be
  over 2, the 5.2 mockup's threshold. "fire" no longer lists
  LibreOffice Impress or Writer; "office" still does.
- **Choices made here** (to confirm on the machine): with the web row
  always there once something is typed, the 5.1 "Aucune application pour
  « … »" line is gone (the 5.2 mockup has none); the placeholder is the
  5.2 mockup's, "Chercher une application, calculer…" / "Search for an
  application, calculate…"; the toast is centred across the whole
  screen, not the space left of the bar (the bar reserves none); it has
  no shadow.
- **Measured headless** (`crates/hocus-compositor/tests/launcher.rs`),
  the real shell, rates from a file of the test's, the browser a program
  of the test's (the shell's `PATH` is the test's directory, so the
  machine's `xdg-open` is never found): one scenario per point of the
  spec (the calculation's card, `Enter` copying it and the toast; a
  conversion, and a currency's at the file's rates with their date; an
  address opening; the web row last, reached by the arrows, and
  `Ctrl+Enter` from a tile; no application with an answer or an
  address). The parser, the units, the numbers, the bank's file and the
  scattered matches have unit tests. On the Intel GPU, the screenshots
  of the pull request.
