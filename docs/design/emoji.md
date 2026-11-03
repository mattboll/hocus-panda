# The emoji picker — specification (5.7)

Status: approved 2026-11-02 (issue #397), variant A of the 5.7 mockup:
at the caret. Built since 2026-11-03: see [as built](#as-built). Sizes
are the mockup's logical pixels; the picker is drawn ×1.5 of them, the
launcher's zoom (launcher.md, 5.1b), so that a 2560 × 1600 screen at
scale 1 reads it well.

## 1. Opening

- `Super+.` (`[shortcuts] emoji`) opens the picker; again, it closes.
- **Where**: just under the focused application's text caret, when the
  application reports it (text-input-v3's cursor rectangle). Otherwise
  centred on the focused window, and with no window, centred on the
  screen. It stays on that screen and flips above the caret when there is
  no room under it.
- **Keyboard**: the picker holds the keyboard while it is open. `Escape`
  or a click outside closes it.
- **Size**: 336 wide, padding 12, radius 14, the popovers' glass (bar.md
  §9).

## 2. Layout

- **Top line**: the search field (36 tall, green border, "Chercher un
  emoji" / "Search emoji"), and a 32 px round button with the skin tone's
  colour.
- **Skin tone**: the button opens a row of six tones (yellow by default,
  then the five Fitzpatrick tones). The choice is kept and applies to
  every emoji that has tones.
- **Tabs**: one per group, as an emoji: recent, smileys, gestures,
  hearts, animals, food, objects, symbols, flags. A tab jumps to its
  group, and the tab of the group in view is lit.
- **The grid**: 8 columns of 38 px cells, emoji at 22 px in the colour
  emoji font. "RÉCENTS" / "RECENT" (the last 16 used) comes first, then
  each group under its heading. The wheel and the arrows scroll; the
  selected cell has the green border and tint.
- **Footer**: the selected emoji, its name, and "Entrée insère" / "Enter
  inserts".

## 3. Search

- It matches the emoji's names and keywords from CLDR's annotations, in
  the user's language and in English. Accents and case are ignored, and
  words match from their start (or inside, for 3 letters or more).
- It shows a flat grid; no result shows "Aucun emoji pour « … »".

## 4. Inserting

- `Enter` or a click inserts the emoji into the application and closes
  the picker. `Shift+Enter` or `Shift+click` inserts it and keeps the
  picker open.
- **How**: through the application's text input (text-input-v3's
  `commit_string`) when it has one active, else by pasting (the clipboard
  is given back afterwards).
- The emoji goes to the top of the recent ones, which are kept in the
  state directory.

## 5. In the launcher

A query starting with `:` (`:chat`) shows an emoji row first: up to 8,
the first selected; `Enter` inserts it into the window that had the
keyboard before the launcher opened (launcher.md, 5.7).

## 6. Data

The emoji list and annotations come from Unicode's and CLDR's data files,
bundled at build time (Unicode License v3, permissive), for French and
English, Unicode 15.1 at least. The font is the system's colour emoji
font (Noto Color Emoji); the .deb recommends `fonts-noto-color-emoji`.

## As built

### 5.7, the emoji picker (2026-11-03)

- **Who does what.** The compositor takes `Super+.` and tells the shell
  `emoji OUTPUT SCALE WIDTH HEIGHT caret X Y W H` (or `window X Y W H`, or
  `screen`): the focused window's screen (else the pointer's), its size,
  and the caret's rectangle on it, or the window's. The shell opens the
  picker there, or closes it when it is open. The picker is a layer
  surface of the shell's (`hocus-emoji`), in the overlay layer, as large
  as its panel, placed by its margins from the screen's top left corner
  (its exclusive zone `-1`: other surfaces' zones do not move it), holding
  the keyboard (`exclusive`). The compositor draws it as a popover's
  (bar.md §9): the screen behind blurred, in a pane of its own, the
  mockup's shadow (`0 18px 50px rgba(0,0,0,0.5)`, zoomed), and its motion,
  the mockup's `pop`: in over 160 ms (`ease-out`) coming down 9 px
  (`translateY(-6px)` zoomed), out over 120 ms going back up, from the
  surface's first frame and once the shell says `closing hocus-emoji`;
  with reduced motion or a shell on the CPU, at once. The mockup's scale
  from 0.98 is left out. A press on none of the shell's surfaces closes
  it (`outside`) and goes no further, as a menu's.
- **Where.** Under the caret, its left edge on the caret's, 6 px (4 ×
  1.5) under it; without room under it, as far above it; centred on the
  window or the screen otherwise; in every case 8 px inside the screen's
  edges at least. The caret is the rectangle the application's enabled
  text input last committed (`set_cursor_rectangle`), from its surface's
  origin (the window's geometry less what of it is outside, a client-side
  shadow).
- **text-input-v3** (`crates/hocus-compositor/src/text_input.rs`): the
  compositor keeps each text input's state itself and is the input
  method. smithay's own text-input hands everything to an input method
  client (`zwp_input_method_v2`) and drops every request while there is
  none, the caret's rectangle included, and only that client could
  commit text; making the shell an input method client would have taken
  that place from real input methods and still not told the compositor
  where the caret is on screen. A text input enters the surface holding
  the keyboard when it is its client's and leaves it when the keyboard
  goes, as the protocol says; its requests take effect on `commit`,
  `enable` resetting what came before; `done` carries the number of
  commits received. Real input methods (fcitx5, IBus) have no place yet:
  they would come with smithay's relay, beside the picker.
- **Inserting** (`emoji insert TEXT`, `emoji keep TEXT`, the compositor's
  `crates/hocus-compositor/src/emoji.rs`): the shell lets the keyboard go
  first, so that the window has it back. When the focused window's
  surface had a text input enabled as the keyboard left it, the
  compositor waits for the application to enable it again (toolkits
  disable it as they lose the keyboard), then commits the emoji there
  (`commit_string`, `done`). Otherwise, or when no text input comes back
  within a second, it pastes: what the clipboard holds is read first, off
  the loop (its text, else its image, the types the clipboard's history
  reads, 1 s at most; the compositor's own selection as it is), the emoji
  is offered instead (to X11 clients too), the window gets `Ctrl+V`
  (`Ctrl+Shift+V` in a terminal, as 5.6 tells them), and once the window
  has read it (or a second after, when it does not) the clipboard is
  given back what it held. A selection that could not be read in time is
  not given back: the emoji stays on the clipboard. The other types the
  previous selection offered are not given back either, only its text or
  its image. One insertion at a time: a later one replaces it.
- **Kept open** (`Shift+Enter`, `Shift+click`): the picker lets the
  keyboard go, the emoji goes in as above, the compositor says
  `emoji-inserted`, and the picker takes the keyboard back; its search
  and its scroll stay. The caret does not show while the keyboard is
  away, and a second emoji waits for the first.
- **The table** (`crates/hocus-shell/data/emoji/emoji.tsv`, embedded in
  the shell, read once on a thread as it starts) is made by
  `scripts/emoji/generate.py` from Unicode 15.1's `emoji-test.txt` and
  CLDR 44's `annotations` and `annotationsDerived` for French and
  English, Unicode License v3 beside it (`LICENSE-UNICODE`): 1,898 emoji,
  every fully-qualified one a keyboard offers but the skin tones and hair
  components, with their names (CLDR's `tts`) and keywords; 36 with no
  CLDR name keep `emoji-test.txt`'s English one in both languages. An
  emoji with tones keeps its form with the light tone on each of its
  people (`🧑🏻‍🤝‍🧑🏻`); a tone swaps that modifier for its own, which
  the generator checks exists for every tone. Unicode 15.1, not 16: what
  the colour font of the distributions in use draws.
- **The groups** are Unicode's, gathered into the spec's tabs: smileys
  (Smileys & Emotion but its hearts), gestures (People & Body: the hands
  first, then the people), hearts (the `heart` subgroup), animals (Animals
  & Nature), food (Food & Drink), objects (Travel & Places, Activities,
  Objects), symbols, flags; Unicode's order within each.
- **The search** folds as the launcher's (case and accents, `œ` as `oe`,
  apostrophes as spaces); every word of the query must match a word of an
  emoji's names or keywords, in either language, from its start, or
  inside it from three letters. The best first: the emoji whose whole
  name the query is (`chat`: 🐈), then a whole word of its name, a word's
  start in its name, a keyword's start, a word's inside; Unicode's order
  within each.
- **Kept on disk**: `$XDG_STATE_HOME/hocuspanda/emoji` (`tone N`, then the
  16 last emoji used, newest first, without their tone), read on a thread
  as the shell starts and written on it after each change.
- **Drawing**: the shell's canvas, nothing through iced's messages, as
  the launcher; the emoji in `Noto Color Emoji`. The cells and the tabs
  show whole rows of the grid from the first one scrolled to (the
  headings 33 px, the cells 57 px, in 378 px).
- **The shortcut's key** is the keysym `period`. On a layout that types
  `.` with Shift (AZERTY), a chord without Shift now matches with Shift
  held when the key, shifted, types the chord's key: `Super+Shift+;` is
  `Super+.` on AZERTY, as the spec's `Super+.` means.
- **Choices made here** (to confirm on the machine): Shift+click needs the
  picker to hold the keyboard (the shell sees Shift through it); the tones
  have no keyboard access (the spec gives none); a tab clicked during a
  search clears it; the tab of the recent ones is lit when they are in
  view, and goes to the top when there are none; the empty grid's message
  is the mockup's "Aucun emoji pour « … »" / "No emoji for “…”".
- **Measured headless** (`crates/hocus-compositor/tests/emoji.rs`), the
  real shell on the CPU, in French, test windows with and without a text
  input: one scenario per point of the spec (under the caret, else
  centred on the window; the search in French, in English and without
  accents, nothing found; `Enter` committing through the text input and
  closing; `Shift+Enter` committing and keeping it open, its search
  taking keys again; a window without a text input getting it by
  `Ctrl+V`, then the clipboard holding what it held; a tone applying, and
  kept on disk; the recent emoji first, kept on disk; the launcher's
  `:chat` row inserting into the window). Unit tests for the table, the
  search, the tones, the grid and its arrows, the placement, the sizes,
  the recent emoji's file and the link's lines. With foot as the
  application, on the Intel GPU, the screenshots of the pull request:
  foot reports its caret through text-input-v3, and takes the emoji
  committed.
- **Not built**: names for screen readers (the Accessibility milestone);
  real input methods beside the picker; the emoji's own variants beyond
  the skin tones (two-person tones).
