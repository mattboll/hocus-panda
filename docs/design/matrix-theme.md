# Matrix theme animations

> The complete set of Matrix-themed animations. Background: black and night
> blue, never brownish (see [motion](motion.md)).

---

## Design Principles

- **Color palette**: Primary green `#00CC66`, bright head `#CCFFCC`, dark trail
  `#003300`, phosphor glow `#00FF88` at 30% opacity, background black `#02040A` to night blue `#050B1C`.
- **Typography**: Monospace (DejaVu Sans Mono / Liberation Mono), katakana-like
  glyphs (half-width katakana and digits) for particle effects.
- **CRT feel**: Scanline overlay (every other row darkened 15%), subtle vignette
  on fullscreen overlays, 2px green phosphor bloom on bright elements.
- **Reduce motion**: every animation honours reduced motion and jumps to its
  final state.
- **Performance budget**: effects run as GPU shaders in the compositor, under
  2 ms per frame at 1920×1080, and nothing is drawn when nothing moves.
- **As built**: the window effects (§1 to §8) and the shell's (§9 to §16)
  are drawn with the elements every renderer has (the window's picture
  stretched, parts of the shell's pictures, plain colours, rows of glyphs
  from a sheet), so that pixman and the scenarios show them too;
  the green shift is a tint. On the GPU (GLES), shaders add what needs
  one (#169): the closing glitch takes red and blue apart, bright lines
  bloom, and fullscreen overlays get scanlines and a vignette; without a
  GPU, and with reduced motion, the plain effects stay, the phosphor glow
  a faint wider line. Measured at 1920×1080 (a debug build, waiting for
  the GPU): the glitch of a window as large as the screen 0.13 ms, the
  scanlines and vignette over it 0.12 ms, the blooms of eight traces
  1.1 ms. The test that measures them runs alone, on request (see
  docs/architecture.md, Testing). Sounds are not played yet.
- **Window border**: every decorated window has a 1 px hairline inside its
  edge, `rgba(0,240,96,0.22)` here (the boards' window border).

---

## 1. Window Open (Map)

### Visual Effect
The window materializes from a column of cascading matrix glyphs. A vertical
rain column appears at the window's center X position, 200ms before the window
surface is shown. The glyphs then "crystallize" outward horizontally -- scanlines
of green characters rush left and right from the center column, filling the
window rectangle. As they reach the edges, the characters dissolve into the
actual window content with a brief green-tinted overlay that fades out. The
window's titlebar draws character-by-character (typewriter effect) with the
app name appearing in green monospace before transitioning to the normal title
font.

### Timing
- **Total duration**: 400ms
- **Phase 1 -- Rain column** (0-100ms): Single column of matrix rain drops from
  top of window rect to bottom.
- **Phase 2 -- Horizontal fill** (100-300ms): Scanlines of characters expand
  from center to edges, row by row (top to bottom, staggered 2ms per row).
- **Phase 3 -- Content reveal** (250-400ms): Green character overlay fades to
  transparent, revealing actual window surface beneath. Green tint decays from
  `alpha=180` to `alpha=0`.

### Easing
- Phase 1: Linear (constant rain speed, matching the wallpaper rain speed)
- Phase 2: `easeOutCubic` for horizontal expansion
- Phase 3: `easeInQuad` for green tint fadeout (slow start, fast finish)

With title bars on (decision 0006: off by default), the title is typed in
from Phase 3, up to 8 ms a character, a `_` cursor at its end, done within
the effect; the theme's faces are all monospace already.

### Particle Effects
- 8-12 matrix glyph particles scatter outward from window edges at Phase 2
  completion. Each particle is a single glyph that drifts outward with gravity
  (downward acceleration), fading over 200ms.
- Phosphor bloom: 2px Gaussian-approximated green glow around window border
  during Phase 2, fading with the overlay.

### Sound Effects
- Soft digital "materialize" whoosh (100ms, 2kHz-8kHz band, filtered white noise
  with downward pitch sweep). Optional, gated by sound setting.

---

## 2. Window Close (Unmap)

### Visual Effect
The glitch close effect is enhanced with
Matrix-specific decay. Instead of generic pixelation, the window content
"decomposes" into matrix characters:

- **Phase 1 -- Glitch + green shift** (0-120ms): RGB channel separation
  with a green color bias -- the R and B channels
  decay faster than G, causing the image to shift toward monochrome green.
- **Phase 2 -- Character dissolve** (120-250ms): Blocks of pixels are replaced
  by matrix glyphs. Starting from random seed points, rectangular regions
  (8x14px, matching glyph size) flip from window content to green-on-black
  matrix characters. Coverage grows from 10% to 90%.
- **Phase 3 -- Rain collapse** (250-350ms): The remaining glyphs collapse
  downward as rain columns, accelerating with gravity. Alpha fades from 255
  to 0. The window "drains" off the bottom edge.

### Timing
- **Total duration**: 350ms (long enough for
  the character dissolve phase)

### Easing
- Phase 1: Linear
- Phase 2: `easeInQuad` (accelerating dissolution)
- Phase 3: `easeInCubic` (gravity-like acceleration for falling characters)

### Particle Effects
- Small green "sparks" (1-2px bright green dots) emit from dissolving block
  edges, 20-30 particles total, lifetime 150ms each.

As built: the glitch shifts bands of the picture sideways, a new glitch
every 30 ms, stronger as it goes, under a green tint. On the GPU each band
also takes its red from one side and its blue from the other, 2 to 6 px
apart, so that the colours part at every edge; it is painted on each
frame of its 120 ms and damages only the window.

### Sound Effects
- Brief digital "derez" sound (80ms, descending tone cluster 4kHz to 800Hz).

---

## 3. Window Minimize

### Visual Effect
The window compresses vertically into a single bright green horizontal line at
its bottom edge (like a CRT monitor switching off). The line then shoots
horizontally toward the taskbar icon position, leaving a brief phosphor trail.

- **Phase 1 -- Vertical crush** (0-200ms): Window height compresses to 2px,
  maintaining full width. Content scanlines merge together, brightening. A
  green tint intensifies as height decreases.
- **Phase 2 -- Horizontal dash** (200-350ms): The 2px-tall line contracts
  horizontally toward the taskbar button, leaving a phosphor afterimage trail
  (green line that decays over 100ms).
- **Phase 3 -- Flash** (350-400ms): Brief bright green flash at the taskbar
  icon position (circular, 20px radius, fading).

### Timing
- **Total duration**: 400ms

### Easing
- Phase 1: `easeInCubic` (slow start, accelerating crush -- mimics CRT off)
- Phase 2: `easeOutQuad` (fast launch, decelerating arrival)
- Phase 3: Linear fade

### Particle Effects
- Phosphor persistence trail: The horizontal dash leaves a green afterglow
  along its path, 1px tall, decaying with an exponential decay table over 3-4 frames.

The line and the flash are drawn over the dock, so that the flash shows on
the icon; the crushed window is drawn with the windows. On the GPU the line
blooms (see §5); elsewhere it has a faint wider glow.

### Sound Effects
- CRT "zzzip" (60ms, descending sweep 6kHz to 200Hz, sawtooth wave).

---

## 4. Window Restore from Minimize

### Visual Effect
Reverse of minimize. A bright green dot expands from the taskbar icon position
into a horizontal line that races to the window's target position, then expands
vertically to reveal the window content.

- **Phase 1 -- Dot to line** (0-150ms): Green dot at taskbar position expands
  into a horizontal line traveling to the window's X position.
- **Phase 2 -- Vertical expand** (150-350ms): The line expands vertically from
  center outward (top and bottom simultaneously), revealing window content
  scanline-by-scanline with a brief green-to-normal color transition.
- **Phase 3 -- Phosphor settle** (350-400ms): Subtle green tint overlay fades
  out completely. Window is fully restored.

### Timing
- **Total duration**: 400ms

### Easing
- Phase 1: `easeOutCubic`
- Phase 2: `dampedSpring` (slight vertical overshoot then settle)
- Phase 3: Linear fade

### Particle Effects
- Scanline "power on" sweep: A bright horizontal scanline sweeps top-to-bottom
  once during Phase 2 (like a CRT warming up), 2px tall, white-green.

### Sound Effects
- CRT "power on" hum (100ms, ascending tone 200Hz to 2kHz, sine wave with
  slight distortion).

---

## 5. Window Maximize

### Visual Effect
The window edges emit bright green "circuit traces" that race outward to the
screen edges. The traces are 1px bright green lines that follow the path from
each window corner to the corresponding screen corner. Once traces reach
the edges, the window snaps to fullscreen with a brief bright flash along
all four borders.

- **Phase 1 -- Trace race** (0-200ms): Four corner traces expand simultaneously.
  Each trace is a right-angle path (horizontal then vertical, or vice versa).
- **Phase 2 -- Window expand** (100-300ms): Window geometry interpolates to
  fullscreen using `dampedSpring`. Content scales smoothly.
- **Phase 3 -- Border flash** (250-350ms): All four screen edges flash bright
  green (4px wide border), fading to the normal border color.

### Timing
- **Total duration**: 350ms

### Easing
- Phase 1: `easeOutQuad` (traces decelerate as they reach edges)
- Phase 2: `dampedSpring` (slight overshoot)
- Phase 3: `easeOutCubic` for flash fade

### Particle Effects
- At each corner where a trace reaches the screen edge, a small burst of
  4-6 green sparks (2px dots, 100ms lifetime).

As built: on the GPU the traces bloom: a gaussian glow one pixel wide
(sigma) around each, at 30 % (the phosphor glow), showing over two pixels
on each side, drawn in nine parts from a profile painted once. The grid
lines of a snap (§6) and the guides of a drag (§7) bloom the same; where
there is no GPU they have a faint wider line instead.

### Sound Effects
- None (maximize is too frequent to warrant sound).

---

## 6. Window Snap (Left/Right/Quadrant)

### Visual Effect
A bright green "grid line" appears at the snap boundary (screen center for
left/right, screen quadrant lines for quarters). The line pulses once with
phosphor glow, then the window slides to its snapped position. The grid line
fades after the window arrives.

- **Phase 1 -- Grid reveal** (0-80ms): Bright green dashed line appears at
  snap boundary. Line style: 4px dash, 4px gap, 1px wide, color `#00FF88`.
- **Phase 2 -- Window slide** (80-300ms): Window animates to snapped geometry
  using `dampedSpring`.
- **Phase 3 -- Grid fade** (250-380ms): The guide line fades out with
  `easeOutCubic`.

### Timing
- **Total duration**: 380ms

### Easing
- Phase 2: `dampedSpring`

### Particle Effects
- None (keep it clean for a frequent operation).

### Sound Effects
- Subtle "tick" (20ms, 1.5kHz sine pulse). Very quiet.

---

## 7. Window Move (Drag)

### Visual Effect
While dragging, the window leaves a faint green phosphor "ghost" trail -- a
semi-transparent afterimage at each position that decays over 3 frames. This
creates a CRT-like persistence-of-vision effect. The trail color is the
window's averaged hue shifted toward green.

Additionally, when the window crosses snap zones (screen edges, center), thin
green guide lines flash momentarily (60ms) at the snap boundaries.

The window stays rigid while dragged in this theme (no wobbly jelly), so
that the trail reads as a CRT's persistence; thrown, it still glides. Its
trail is its own picture under a green tint, at the places it was drawn
17, 33 and 50 ms ago. The guide lines flash in the bright head green, to
read over the zone's outline.

### Timing
- **Trail persistence**: 3 frames (approximately 50ms at 60fps)
- **Guide line flash**: 60ms per appearance

### Easing
- Trail decay: Exponential (using an exponential decay table)
- Guide lines: Instant on, `easeOutCubic` fade

### Particle Effects
- None (performance-sensitive during drag).

### Sound Effects
- None.

---

## 8. Window Resize

### Visual Effect
During interactive resize, the window border glows bright green (`#00FF88`,
2px width). A small monospace text readout appears near the cursor showing
the current window dimensions in green-on-black terminal style:
`[1280x720]`. The text updates in real-time as the user drags.

When resize completes (button release), the green border flashes brightly
once (50ms) then returns to normal.

The glow takes the place of the focus ring; the readout shows the window's
size in logical pixels, as its client has it.

### Timing
- **Border glow**: Continuous during resize
- **Completion flash**: 50ms

### Easing
- Flash: `easeOutCubic`

### Particle Effects
- None.

### Sound Effects
- None.

---

## 9. Desktop Switch (Virtual Desktops)

### Visual Effect
The current desktop dissolves into a rain of matrix characters that sweep
in the switch direction, revealing the new desktop beneath. This replaces
the regular slide with a thematically unified effect.

- **Phase 1 -- Character breakdown** (0-150ms): The old desktop (captured as
  a snapshot) breaks into a grid of character-sized blocks (7x14px). Each
  block is replaced by a matrix glyph in the block's average color, shifted
  toward green. Blocks convert from outside-in (edges first, center last).
- **Phase 2 -- Directional rain** (100-300ms): The character blocks slide in
  the switch direction (left for "next desktop", right for "previous").
  Blocks fall with slight downward drift and random horizontal jitter,
  simulating the chaotic motion of matrix rain. Speed varies per column.
- **Phase 3 -- Clear** (250-350ms): Remaining characters fade out with
  an exponential decay table. New desktop is fully visible.

### Timing
- **Total duration**: 350ms

### Easing
- Phase 1: `easeInQuad` (accelerating breakdown)
- Phase 2: `easeOutCubic` with per-column random speed variation (+/-20%)
- Phase 3: Exponential decay (LUT-based)

### Particle Effects
- Stray glyphs: 10-15 matrix characters continue drifting after the main
  transition, fading over an additional 150ms. These are the "stragglers"
  that didn't make it off-screen with the main wave.

### Sound Effects
- Digital "whoosh" with character cascade undertone (200ms, band-limited
  noise with rhythmic modulation suggesting rapid character rendering).

As built: the windows of the desktop left behind turn, cell by cell from
the screen's edges in, to glyphs on black over their live picture
(0-150 ms), then their columns rain off (150-300 ms) and fade (to
350 ms); 10 to 15 stragglers drift 150 ms longer. The glyphs are the
rain's green, not each block's colour (that needs the picture's pixels,
a shader's work), and cells are the sheet's, 8×14. The switcher of the
screen's desktops shows as with the slide. A three-finger swipe keeps
the strip, which follows the fingers.

---

## 10. Overview / Expose Mode Enter/Exit

### Visual Effect

**Enter**: All windows simultaneously "decode" -- their content converts
to matrix rain for a split second (100ms), then the rain columns compress
and rearrange into the overview grid layout. Each window thumbnail has a
thin green border and its title rendered in green monospace below.

The background darkens to pure black with sparse matrix rain columns falling
slowly behind the window thumbnails (reusing the rain renderer
at 50% column density).

**Exit**: The selected window's thumbnail expands while emitting a bright
green pulse. Other thumbnails dissolve into downward-falling matrix
characters. The selected window's green border flashes and fades as it
returns to normal size and position.

### Timing
- **Enter duration**: 400ms
- **Exit duration**: 350ms

### Easing
- Enter window shrink: `dampedSpring` (for grid arrangement)
- Exit selected expand: `dampedSpring`
- Exit others dissolve: `easeInCubic`

### Particle Effects
- Background: Slow matrix rain (reuse the rain renderer with reduced column count)
- Exit: Character dissolution particles from non-selected windows

### Sound Effects
- Enter: Soft ambient "digital hum" onset (150ms, low 120Hz drone + high
  harmonics)
- Exit: Quick "focus" click (30ms, 2kHz pulse)

As built: the overview is the switcher of every screen (Super alone, a
hot corner, a pinch). Entering, glyphs fall over each window (100 ms),
then a block of them springs from the window's place to its card,
giving way to the live preview; the cards themselves fade in. Its titles
are the theme's monospace, each card with the theme's thin green border.
Picked, the window springs back from its card to its place, its border
glowing and fading; the other cards fall as rain. Escape lets them all
fall. The rain behind falls while it is open, as the live previews
already ask for frames.

---

## 11. Alt-Tab Switcher

### Visual Effect
The switcher overlay renders on a translucent black background (`alpha=200`)
with matrix rain falling slowly behind the window thumbnails. The currently
selected window thumbnail has a bright green glowing border (2px, with 4px
phosphor bloom). As the user cycles through windows, the selection highlight
slides to the next thumbnail with a brief green "trace" connecting the old
and new positions (a horizontal line that appears and fades in 100ms).

Window titles below thumbnails render character-by-character (typewriter
effect, 3ms per character) in green monospace.

The entire overlay has a subtle CRT scanline effect (alternating rows at 92%
and 100% brightness).

### Timing
- **Appear**: 150ms fade-in
- **Selection move**: 100ms slide
- **Dismiss**: 200ms fade-out with selected window "popping" to front

### Easing
- Appear/dismiss: `easeOutCubic`
- Selection slide: `dampedSpring` (snappy with micro-overshoot)

### Particle Effects
- Background: Sparse matrix rain (30% column density, slow speed)
- Selection change: Brief green spark burst at new selection (6 particles,
  80ms lifetime)

### Sound Effects
- Each tab press: Subtle "tick" (15ms, 1.8kHz sine)
- Confirm (modifier release): "Lock" sound (30ms, 2.2kHz double-pulse)

As built: the glow follows the card's ring as it glides (150 ms,
out-cubic, as in every theme); the trace runs between the cards'
middles. On the GPU (#169), every other row of the whole overlay is
8 % darker and a vignette darkens its corners (45 % at most), both
fading with it; the overview has them too, its rows 15 % darker.

---

## 12. Lock Screen Appear/Dismiss

### Visual Effect

**Appear**: The desktop freezes, then dense matrix rain cascades over it
from top to bottom, progressively obscuring the content. After the rain
covers the entire screen, it continues falling on a black background. A
centered lock icon (rendered in matrix glyphs forming a padlock shape)
fades in. Below it, a password field appears with a green blinking cursor.
The entire lock screen has CRT scanlines and slow vignette pulse.

**Dismiss** (successful auth): The matrix rain accelerates dramatically
(3x speed), then all characters flash white simultaneously for one frame
(50ms), simulating a CRT "flash". The screen clears to reveal the desktop
with a brief green-to-normal color correction (100ms).

**Failed auth**: The lock icon and password field shake horizontally
(3 oscillations, 8px amplitude, 200ms) with brief red-tinted scanlines.
The matrix rain momentarily shifts to red-green for 300ms before returning
to pure green.

### Timing
- **Appear**: 800ms (rain cascade: 600ms, icon fade-in: 200ms)
- **Dismiss**: 300ms (rain accelerate: 150ms, flash: 50ms, fade: 100ms)
- **Failed shake**: 200ms

### Easing
- Rain cascade: Linear (constant downward sweep)
- Icon fade: `easeOutCubic`
- Dismiss flash: Instant on, `easeOutQuad` off
- Failed shake: `dampedSpring` (natural oscillation decay)

### Particle Effects
- Dense matrix rain (100% column density, all columns active)
- Lock icon composed of 12x16 matrix glyph grid forming padlock silhouette

### Sound Effects
- Appear: Low digital drone rising in pitch (400ms, 80Hz to 200Hz)
- Dismiss: Sharp "access granted" chime (100ms, ascending C-E-G triad in
  square waves)
- Failed: "Access denied" buzz (150ms, 90Hz square wave, 2 pulses)

As built: the lock screen keeps its clock, date and field; the padlock
(12×16 glyphs) takes the place of the initial. The rain cascades down
in 600 ms, the desktop showing under its front, then the lock fades in
(to 800 ms) while the rain slows to a stop by 1000 ms and dims: it stays
still behind the lock, so that a locked screen draws nothing. The
field's caret is the field's own. Refused, the padlock and the field
shake three times within 200 ms under red scanlines, the rain red for
300 ms. Unlocked: the rain speeds up to three times over 150 ms, a
flash fades in 50 ms (out-quad), and the desktop's green in 100 ms. A
lock client's own screen is left as it draws it. On the GPU (#169), the
lock has scanlines (every other row 15 % darker) as the rain covers the
desktop, and a vignette that swells once, slowly, as the lock comes in
(one second), then rests: a pulse that went on would repaint a still
locked screen. Unlocked, they fade in 200 ms. The boot screen has them
too, gone with its flash.

---

## 13. Notification Toast Appear/Dismiss

### Visual Effect

**Appear**: The notification materializes from a horizontal line of green
characters at the notification's Y position. Characters "type" left to right
across the notification width (typewriter effect), then the filled rectangle
transitions from green-on-black terminal style to the normal notification
appearance. A thin green left border (2px) persists as a subtle theme accent.

**Dismiss**: The notification content converts back to green characters
briefly (50ms), then the characters scatter -- each character drifts in a
random direction with slight downward gravity, fading out. This gives the
impression of the notification "dispersing into the matrix."

### Timing
- **Appear**: 350ms (character typing: 200ms, color transition: 150ms)
- **Dismiss**: 300ms (character conversion: 50ms, scatter: 250ms)

### Easing
- Appear typing: Linear (constant character rate)
- Appear color transition: `easeOutCubic`
- Dismiss scatter: `easeInQuad` (accelerating outward drift)
- Dismiss fade: Linear

### Particle Effects
- Dismiss: 20-30 glyph particles scattering from notification bounds,
  each a single matrix character, lifetime 200ms.

### Sound Effects
- Appear: Brief "incoming data" blip (40ms, 3kHz sine pulse)
- Dismiss: None

As built: the toast comes where it goes (no slide); its card is left out
where it is not typed yet. The green left border is the theme's green
border all round, as the boards draw it.

---

## 14. Sidebar (Raven) Open/Close

### Visual Effect

**Open**: A vertical curtain of matrix rain sweeps from the right edge
leftward to the sidebar's full width. The rain is dense and fast (150ms).
Behind the rain, the sidebar content fades in. Once the rain curtain
reaches the left edge of the sidebar, it dissolves and the sidebar
content is fully visible. A thin green vertical line marks the sidebar's
left border permanently while open.

**Close**: The sidebar content fades to green-tinted (50ms), then a
sweep of matrix rain moves from left to right across the sidebar,
erasing it. As the rain passes each column, the sidebar disappears
behind it. The rain continues off the right screen edge.

### Timing
- **Open**: 350ms (rain sweep: 250ms, content fade-in: 200ms overlapping)
- **Close**: 300ms (green tint: 50ms, rain sweep: 250ms)

### Easing
- Open rain sweep: `easeOutCubic` (fast start, gentle stop)
- Open content fade: `easeOutQuad`
- Close rain sweep: `easeInCubic` (accelerating exit)

### Particle Effects
- Rain curtain: Uses the rain renderer but constrained to the
  sidebar rectangle, with 100% column density and 2x normal speed.

### Sound Effects
- Open: Soft "terminal open" whoosh (80ms, rising noise sweep)
- Close: Soft "terminal close" whoosh (80ms, falling noise sweep)

As built: the sidebar is the column, which stays open. It opens so when
it first shows as the session starts, when it comes to another screen
(its screen unplugged, another made primary) and when a fullscreen
window over it goes; it closes so when one comes over it. The band of
rain is 40 px wide. Its permanent left line
is the theme's green border.

---

## 15. App Launcher Open/Close

### Visual Effect

**Open**: The launcher appears with a "jack in" sequence. A single bright
green cursor blinks at the center of the screen (2 blinks, 100ms on/off).
Then the search field "types" itself into existence -- a green rectangle
expands from the cursor position horizontally, with the text cursor
blinking at the right edge. Below the search field, suggestion rows
cascade in from top to bottom with a 30ms stagger, each row appearing
via a left-to-right character reveal (typewriter). The background dims
to 60% black with sparse matrix rain.

**Close**: All text in the launcher converts to green characters that fall
downward as matrix rain (each character drops independently with random
speed and slight horizontal drift). The search field collapses horizontally
to its center point. The background rain clears.

### Timing
- **Open**: 450ms (cursor blinks: 200ms, field expand: 100ms, row cascade:
  150ms with stagger)
- **Close**: 300ms (character rain: 250ms, field collapse: 200ms overlapping)

### Easing
- Field expand/collapse: `dampedSpring`
- Row cascade: `easeOutCubic` per row
- Close character rain: `easeInQuad` (gravity acceleration)

### Particle Effects
- Background: Sparse matrix rain (20% density, slow)
- Close: Each character of the search query and suggestions becomes an
  individual falling glyph particle.

### Sound Effects
- Open: "Login" sequence -- two short beeps (50ms each, 1kHz and 1.5kHz)
  followed by a soft hum
- Close: Quick "logout" descending tone (60ms, 1.5kHz to 800Hz)

As built: the cursor blinks where the field will be; the rest of the
card fades in with the results. The rain behind stands still once it is
open (nothing is drawn at rest); with reduced motion the dim stays,
without rain.

---

## 16. Boot Sequence

### Visual Effect
The boot sequence in the Matrix theme (green monospace text on
black, progress bar, scanline sweep):

- **Phase 0 -- Darkness** (0-300ms): Pure black screen. A single green cursor
  blinks once in the center.
- **Phase 1 -- Wake up** (300-800ms): Matrix rain begins falling across the
  entire screen at maximum density. The rain is bright and fast, establishing
  the theme immediately.
- **Phase 2 -- Terminal** (800-2800ms): Diagnostic messages
  appear, but now over a background of slow matrix rain. Each "> " prompt
  line has its "OK" status flash bright white-green when it appears. The
  progress bar is replaced by a horizontal matrix rain strip -- characters
  cascade left to right to fill the bar.
- **Phase 3 -- "SYSTEM READY"** (2800-3200ms): The "SYSTEM READY" text
  renders in large block characters composed of matrix glyphs (each letter
  is a 5x7 grid of matrix characters). A bright flash ripples outward from
  the text.
- **Phase 4 -- Dissolve** (3200-3800ms): All matrix rain accelerates to 3x
  speed and brightens. The text fades. A white-green flash fills the screen
  for 1 frame, then rapidly fades to reveal the desktop with the wallpaper
  matrix rain effect already running seamlessly.

### Timing
- **Total duration**: 3800ms (extended from 3500ms to accommodate the
  enhanced phases)

### Easing
- Rain acceleration in Phase 4: `easeInQuad`
- Text flash ripple: Radial `easeOutCubic`
- Final fade: `easeOutQuad`

### Particle Effects
- Full-density matrix rain throughout (reuse the rain renderer)
- Phase 3 ripple: Circular wavefront of brightness that expands from center,
  radius growing at 2000px/s.

### Sound Effects
- Phase 0: Silence
- Phase 1: Low digital hum fading in (200ms, 60Hz + 120Hz harmonics)
- Phase 2: Soft keyboard-like clicks synced with each diagnostic line
  (20ms per line, 2kHz)
- Phase 3: Ascending chord (C major in square waves, 200ms)
- Phase 4: Brief white noise burst (50ms) then silence

As built: the boot screen shows only while a saved session is put back
and goes as soon as everything is back (at most 5 s), so the phases run
on its own clock and are cut short when it is quick: darkness and a
cursor (to 300 ms), the rain waking from the top (to 800 ms), then the
logo, the name and a terminal listing the windows put back, `OK` bright
as each comes, over slow rain, the progress bar a strip of glyphs. Once
done: SYSTEM READY (`SYSTÈME PRÊT` in French; accents dropped in the
glyph letters) with its ripple (400 ms), the dissolve (to 850 ms), the
flash (to 1000 ms). The pandas at work stay in their corner.

---

## Reduce Motion Behavior

When reduced motion is on, ALL Matrix theme animations MUST:
1. Skip particle effects entirely
2. Skip rain/character effects
3. Use instant state transitions (0ms)
4. Apply only color changes (green accent borders) without animation

As built, windows then move as in every theme with reduced motion: short
fades where they are (opening, closing, minimizing, restoring, maximizing),
a snap at once, no trail, no flashes. The resize glow and readout stay:
they are colour and information, not motion. The shell keeps its colours
without rain: the lock is black with its padlock, Alt+Tab and the
overview over black, the launcher over its dim, the boot's strip filling;
everything comes and goes at once, as in every theme.

