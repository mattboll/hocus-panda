# Accessibility — specification

Sizes are logical pixels. Each part is its own issue; this file gathers
what the desktop does for whoever needs to see larger, read without
seeing, work without a mouse, or see clearer.

## Screen readers (#421)

What a screen reader reads of the bar, its popovers, the bubbles, the
lock and the password dialog, and what it hears on its own: in
[screen-reader.md](screen-reader.md).

The Accessibilité page's row for it (#734), in its own group LECTEUR
D'ÉCRAN: "Lecteur d'écran" / "Screen reader", a switch on
`[accessibility] screen-reader` (Orca starts with the session; off by
default), under it "Super+Alt+S le lance ou l'arrête, même sur l'écran
de verrouillage." / "Super+Alt+S starts or stops it, on the lock screen
too.". Turned on in the Settings, Orca starts at once (the compositor
follows the file).

## The magnifier (#424)

Status: approved 2026-11 with the mockup's recommended options (full
screen; `Super+=`, `Super+-`, `Super+Escape`, `Super` and the wheel; the
view pushed at its edge; the keyboard's focus and the caret followed).
Built since: see [as built](#as-built).

### Behaviour

- **Full screen.** The compositor draws the screen under the pointer
  magnified, from **1× to 16×**, by steps 1, 1.5, 2, 3, 4, 6, 8, 12, 16;
  the wheel by 10 %. The other screens stay at 1×.
- **The view follows the pointer at its edge**: it moves only when the
  pointer pushes its edge, and by as much. The Settings offer "centre"
  (the pointer stays in the view's middle) and "proportional" (the view
  sits where the pointer sits on the screen).
- **It follows the keyboard**: when the keyboard's focus moves (a window
  focused, a popover of the bar), the view jumps so that the focused thing
  shows, centred if it fits, else from its top left corner. An
  application's text caret is kept 10 % in from the view's edges, the
  view moving as little as it can. A pointer move afterwards takes over
  again.
- **The pointer grows** with the zoom: drawn at its size times the zoom,
  from its theme's image nearest that size.
- **Input keeps its real coordinates**: the pointer moves on the desktop as
  it always does, and what it points at is drawn under it; a click reaches
  what is drawn under the pointer, at its real coordinates.
- **Smoothing** is on by default (linear filtering); off, hard pixels.
- **Not magnified**: screenshots, recordings and shared screens get the
  real screen.
- **The pill**: at each change of zoom, a pill at the top centre for 2 s:
  the magnifying glass, "Loupe" ("Magnifier"), the zoom (`×8`, `×1,5`),
  − and + buttons that take clicks, and "Super+Échap quitte" ("Super+Esc to
  exit"). **The minimap** (64 px wide, at the bottom right) shows the view
  on the screen while it moves, and 1.5 s after it stops.
- **Motion**: the view glides 120 ms (`ease-out`) to the focus and between
  zoom steps; with reduced motion, it jumps. A push of the pointer moves
  it at once; the wheel's steps too. The pill and the minimap fade out in
  120 ms, or go at once with reduced motion.

### Keys

| Key | What it does | `[shortcuts]` |
|---|---|---|
| `Super+=` (also `Super+plus`, the keypad's `+`) | a step in; off, turns it on at its last zoom | `magnifier-in` |
| `Super+-` (also the keypad's `-`) | a step out; at 1×, off | `magnifier-out` |
| `Super+Escape` | off | `magnifier-off` |
| `Super` + wheel, or two fingers | 10 % a notch, up zooming in | — |

- They work on the lock screen and in the authentication dialog, and over
  the radial menu, the overview, Alt+Tab and the capture.
- On layouts where `-` shares its key with a digit (AZERTY: the `6` key),
  `Super` and that key zooms out: shortcuts match the key's symbol at its
  first level before the digit of its place, so it does not go to desktop
  6 there. Desktop 6 stays on `Super+Ctrl+→` and the overview; the
  keypad's `-` zooms out on every layout.
- Nested (in a window of another session), `Super` is `Alt+Super`, as for
  every shortcut.

### Settings

`[magnifier]` in the configuration, followed live:

- `enabled` (false): on from the start, or turned on and off by the
  Settings;
- `zoom` (2.0, from 1 to 16): the zoom `enabled` turns it on at, and its
  zoom when the Settings change it while it is on;
- `follow-pointer = "edge" | "centre" | "proportional"` (`"edge"`);
- `follow-keyboard` (true): the focus and the caret;
- `smooth` (true).

The Settings show them in their Accessibility page's Loupe group: on,
the zoom (×1 to ×16), how the view follows the pointer, following the
keyboard, smoothing.

### Edge cases

- A screen at another scale: the zoom is in logical pixels; the drawn
  pixels follow the screen's scale.
- The pointer crossing to another screen: the magnifier goes with it, at
  the same zoom, the view where the pointer enters; the screen it left is
  back at 1×.
- 16× on 3440 × 1440: the view is 215 × 90; it never leaves the screen.
- A fullscreen game or video under the magnifier: direct scanout is off
  while it is magnified.
- The lock screen: the magnifier stays, and its keys work.
- An absolute pointer (a tablet, a virtual machine's pointer, the nested
  window's) cannot push an edge: the view follows it proportionally,
  whatever the setting.

### Performance

At 1× (off), nothing changes: the screen is drawn as before. On, what the
screen shows is drawn into a picture of the screen's size, kept between
frames with damage tracking of its own, then that picture's view is drawn
stretched over the screen: one more pass. A frame is drawn only when
something in the view changes or the view moves; with the magnifier on and
nothing moving, none (counted by the tests).

### Acceptance criteria

1. Headless: `Super+=` twice gives ×1.5 then ×2 (`hocus msg outputs` shows
   the zoom and the view); `Super+-` back; `Super+Escape` off.
2. A screenshot of the screen at ×2 and ×8 is the 1× screen's view
   rectangle magnified (hard pixels), the cursor larger; screenshots,
   recordings and shares stay at 1×.
3. Pointer moves inside the view do not move it (edge); pushing its right
   edge by 50 px moves it by 50 px; centre and proportional checked the
   same way.
4. The keyboard's focus moving elsewhere: the view jumps to it; a text
   caret near the view's edge is kept 10 % in.
5. A click at a magnified point reaches the window there.
6. At rest with the magnifier on: no frame over 5 s.
7. AZERTY: `Super+minus` zooms out and does not go to desktop 6.
8. In the VM: the magnifier on the lock screen, the pill, a text editor's
   caret followed (manual).

### As built

- `crates/hocus-compositor/src/magnifier/`: the state (zooms, the view's
  rules, the pill's clicks, `[magnifier]`), and the drawing (`draw.rs`:
  the screen's picture with pixman or GLES, the pill, the minimap). Every
  backend draws its screens through it (`magnifier::draw::screen`).
- The caret followed is the one applications give through text-input-v3
  (GTK 4, Qt, Firefox and the terminals that support it, while one of
  their text fields has the keyboard). The caret through AT-SPI (the
  accessibility bus's `object:text-caret-moved`), for applications without
  text-input, is not built: the compositor has no AT-SPI client yet.
- The keyboard's ring in the bar (`Super+B`, bar.md §11) is followed:
  each time it lands or moves, the shell tells the compositor where it is
  (`ring OUTPUT X Y W H`, in the bar's frame) and the view shows it, as a
  focus (`bar_keys.rs`, `the_magnifiers_view_follows_the_ring`); the view
  jumps to a popover as it takes the keyboard.
- Nested, the host draws the cursor: it is not magnified there.
- The zoom the keys leave is the session's: the next session starts from
  `[magnifier] zoom` when `enabled`, else off.
- The pill's words follow `[locale] language`, as the radial menu's.

Tests: `crates/hocus-compositor/tests/magnifier.rs` (the keys, the wheel,
the pixels at 2× and 8× with pixman and with GLES on Mesa's software
device, the edge, centre and proportional, the focus and the caret, a
click, AZERTY, the file, no frame at rest), and the unit tests of
`magnifier` (the view's rules) and `shortcuts` (the default keys).

## The whole bar by the keyboard (#422)

Status: approved 2026-11 with the mockup's recommended options (`Super+B`
enters the bar, the form's switch moving to `Super+Shift+B`; the ring on
the waiting agent, else the stuck one, else the clock; a second `Super+B`
to the next screen's bar, then back to the window). The behaviour, the
keys, the ring and what is built are in [bar.md §11](bar.md#11-accessibility).

## High contrast and reduced transparency (#423)

Status: approved 2026-11 with the mockup's recommended options (a
reinforcement of the theme chosen, keeping the accent; it implies reduced
transparency; 7:1 for text, 3:1 for borders and controls).

- **High contrast** (`[appearance] high-contrast`): the tokens, the rules
  for the other colours, the ring and what is built are in
  [bar.md §13, "High contrast"](bar.md#high-contrast-423). The bar, its
  popovers, the bubbles, the lock screen, the authentication dialog and
  the Settings draw it; so do the radial menu, the overview and the
  magnifier's pill, which the compositor draws.
- **Reduced transparency** (`[appearance] reduce-transparency`, or high
  contrast): no blur, and the opaque colours of the bar, its popovers, the
  bubbles, the reminder, the capture's and the sharing's panels. Behind a
  maximised window, the bar is opaque too, whatever the setting
  ([bar.md §11](bar.md#11-accessibility)). The launcher's and the
  overview's scrims stay: they dim, they are not glass.

## The Settings' page

Accessibilité gathers what these issues set (settings.md, "Accessibilité"):
LOUPE (the magnifier's rows), VUE (high contrast, reduced transparency,
reduced motion, a link to the text's and the cursor's size), CLAVIER (the
way into the bar). Appearance keeps a link to it. While high contrast is
on, reduced transparency's switch shows on and inert ("Comprise dans le
contraste élevé"); the file keeps its own value. LECTEUR D'ÉCRAN holds the
screen reader's switch (#734).
