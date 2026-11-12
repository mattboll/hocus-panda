# The desktop's effects (#434)

Effects for the fun of it, fourteen of them, prototyped and approved on
the effects canvas ("Effets du bureau"; sources in the maintainer's
`hocus-panda-tools/mockups/s15-fx`). Each is off by default and has its
own switch, `[effects]` in the configuration and its row in Settings ›
Appearance › Effects ([settings.md](settings.md), "Apparence et
langue").

| Group | Effect | Key | Ticket |
|---|---|---|---|
| The wallpaper | Neige qui tient | `snow` | #795 (built) |
| | Aurore en musique | `aurora` | #801 |
| | Pluie sur la vitre | `rain-on-glass` | #802 |
| | Bassin de koïs | `koi` | #803 |
| | Lucioles du soir | `fireflies` | #804 |
| | La lumière du jour | `daylight` | #805 |
| | Bureaux en profondeur | `depth` | #806 |
| The windows | Fermer en flammes | `flames` | #794 (built) |
| | Éclats de verre | `shatter` | #798 |
| | Fenêtres en gelée | `jelly` | #797 (built) |
| | Guirlande sous la fenêtre active | `garland` | #799 |
| The moments | Tests au vert : feu d'artifice | `fireworks` | #810 |
| | Bonne nouvelle d'un agent : confettis | `confetti` | #811 |
| The agents | Les agents en créatures | `creatures` | #796 (built) |

## When they stop

Every effect follows the same rules (`State::effects_still`, in
`crates/hocus-compositor/src/effects/mod.rs`):

- none plays with reduced motion, nor in the power saver;
- none over a fullscreen window: the window closing is fullscreen, or a
  fullscreen one is in front on its screen;
- none while a screen or a window is shared or the screen recorded, and
  none ever appears in a capture, a share's or a recording's frames (they
  are drawn for `Purpose::Screen` only);
- none on the CPU: pixman draws no shader, and what would have played
  plays as it does without the effect (a closing window closes as
  standard);
- none on a screen showing a fullscreen window, nor on a screen off:
  what plays there waits;
- never over what the user reads: around a window effect, nothing is
  drawn over the other windows nor the bar (each effect's shader is told
  their rectangles, the nearest eight, and draws nothing there), and a
  light falls on the wallpaper alone, drawn under the windows and the
  bar;
- once an effect is over, nothing more is drawn for it, and no frame is
  asked for.

An effect is decided as it starts: a switch turned off, or a rule that
starts holding, leaves one under way to end. An effect that lasts (the
snow) follows its switch at once, and the rules as they change: with
reduced motion it is drawn still (what lies stays, nothing falls), and
in the power saver or while a screen is shared or recorded it is not
drawn, and waits.

## How a window effect is drawn

`crates/hocus-compositor/src/effects/`:

- **The window pass** (`draw.rs`, the `Fire` trait): the window's last
  picture (the textures its surfaces had as it went, `snapshot.rs`) is
  drawn once into a texture of its own, its corners rounded, and drawn
  every frame through a GLES program of the effect's over the window and
  a margin around it (60 px, 132 above for the flames). The programs are
  made once a GL context, with the glyphs' helpers (`matrix.rs`); a
  program that does not compile leaves the window fading over the
  effect's time.
- **The particles** (`particles.rs`): one CPU simulation stepped at a
  fixed 60 steps a second from the effect's start, with a seeded
  generator, so that a moment of an effect is the same whatever the
  frames' pace (and in tests on the held clock); drawn as one batch of
  quads a blend: smoke over what is under it, embers added to it.
- **A light** (`draw.rs`): a radial gradient screened over what is under
  it (`ONE, ONE_MINUS_SRC_COLOR`), in the place of the layers under the
  windows: on the wallpaper only.
- **Where they may draw** (`draw.rs`, `Clip`): a rectangle to stay
  within, and the other windows' and the bar's to stay out of.
- **Shapes drawn in one call** (`mesh.rs`, the `SHAPES` program): indexed
  triangles whose vertices say what each pixel is: a fill of their own
  colours (smoothed by a fringe of transparent vertices), a soft disc, a
  clump lit from its upper left, a box blurred by a Gaussian. Drawn over
  what is under them, then added to it; cut to the frame's damage (a
  scissor each rectangle), never blended twice over what the frame kept.
- **The layer under the windows** (`render/snow.rs`): drawn over the
  wallpaper and the layers under the windows, under the windows and the
  bar, on each screen where an effect on it shows (not one showing a
  fullscreen window); in the frames of the screen alone. The glass of the
  bar and of the popovers is blurred from what is behind it with a blank
  in the layer's place, put back once the glass is done: the glass is not
  blurred again at each of the layer's frames. Nothing is drawn while
  nothing on it moves (its picture changes only as its effect steps).
- **Decorations on a window's edge** (`render/snow.rs`): each drawn right
  in front of what holds it (its window among the windows, the bar right
  in front of the bar), where it is drawn this frame (sliding with its
  desktop, fading with it): a window in front covers it, and it never
  covers another. It shows above the edge, and of the window itself on
  its top 8 px alone (`REACH_BELOW`, below which the shader draws
  nothing): never on a title's text, nor on what the window shows.
- **The windows' motion** (`motion.rs`): each window's velocity and
  acceleration from where it is drawn frame to frame (a frame counting
  for 1/30 s at most; its velocity smoothed over 25 ms: a window dragged
  slowly moves by whole pixels, one frame in three, which is no jolt).

The fragment shaders compute in high precision: Mesa's software device
takes `mediump` as 16 bits, whose squared distances overflow past 256 px.

## Closing in flames (#794)

With `flames = true`, an application's window that closes (not a dialog,
not minimised) burns from the bottom up in 1.7 s in place of the closing
animation, as the board "Fermer en flammes" does; its shader is the
board's `FLAME_FS` ported to GLSL ES 1.00, its lengths in logical pixels:

- a burn field of fbm noise rising from the bottom against the front
  (eased: smoothstep's share 35 %, linear's 65 %);
- ahead of the front the paper scorches, wavers in the heat and glows on
  a flickering rim; behind it, charred flakes with sparks;
- flames rise over the paper still there and up to 120 px above the
  window, fading out in the last 18 % of the burn; around the window, and
  where its paper is gone, never over another window or the bar;
- the window's soft shadow fades out in the first 22 %;
- seven embers a step along the front and a puff of smoke seven times in
  ten, while it burns, shown within the window's place and above it
  (fading out over 16 px at its sides) and never over another window or
  the bar; the embers left are swept away 2.8 s after it starts, the
  smoke drifts out on its own (all gone by 5 s);
- a warm light (orange to red, 620 px around the window's middle at the
  front) as bright as the fire, which rises then falls over the burn,
  flickering, on the wallpaper alone.

The board drew the light, the embers and the smoke over everything; on
the desktop they would wash out a pale window behind into a white haze
(#794's review), so they keep off the windows and the bar.

The window leaves the desktop as it starts (its picture alone burns):
input never reaches it, and the next window takes the focus at once. Its
sparks and its picture are drawn over the windows, as the windows
leaving are; its light under them.

Tests: `tests/effects.rs` (a window burning on GLES on Mesa's software
device, its flames over and above it, the page of text beside it
untouched, gone within 5 s and nothing drawn after; on pixman, with reduced motion or the switch off, the standard
close); its screenshots, `HOCUS_SHOTS=DIR cargo nextest run -p
hocus-compositor --run-ignored only effects::shots`.

## Windows in jelly (#797)

With `jelly = true`, a window dragged deforms like jelly, in place of the
standard wobble ([windows.md](windows.md) §6, `[windows] wobble`), as the
board "Fenêtres en gelée" does: its spring mesh ported
(`effects/jelly.rs`), drawn by the wobble's bent mesh (`wobble.rs`), its
lengths in logical pixels.

- 22 × 16 points over the window, each pulled to its place on the rigid
  window by a damped spring: held, stiffest near the hand (`22 + 1100
  e^(-d/55)`, damped `3.2 + 12 e^(-d/90)`), the points within 6 px of it
  pinned; let go, all at 120 damped 5.5, a few swings before it lies
  still. Neighbours along a row are kept near their spacing (8 % of the
  gap a step). Stepped 180 times a second (the board's three a frame).
- The grid's columns and rows are cut so that one of each passes under
  the hand: the point held is pinned, and follows the pointer exactly
  (the board's grid seldom has a point within 6 px of the hand).
- Once every point is back at its place and still (0.5 px, 6 px/s),
  held or let go, the window is drawn as it is, not through the mesh: its
  text straight, pixel for pixel; and no frame is asked for while it is
  held still.
- Its content is the window itself, deformed: no other window nor the bar
  is drawn over. On the screen only: a capture, a share or a recording
  shows the window straight. Decided as the drag starts: with reduced
  motion, the power saver, a share or a recording, on the CPU, or with a
  fullscreen window in front on its screen, the drag is as without it
  (the standard wobble if it is on).

Tests: `tests/window_effects.rs` (on GLES on Mesa's software device: a
window dragged bends, and once settled is as it was before, pixel for
pixel, nothing drawn after; on pixman, with reduced motion or the switch
off, a drag is rigid); its screenshots, `HOCUS_SHOTS=DIR cargo nextest
run -p hocus-compositor --run-ignored only window_effects::shots`.

## Snow that settles (#795)

With `snow = true`, snow falls behind the windows and settles on them,
as the board "Neige qui tient" does: its JS ported, its lengths in
logical pixels (`effects/snow.rs` the simulation, `snow_look.rs` its
drawing, `snowing.rs` where it falls).

- **Falling**: as many flakes as the board's 520 over 1200 × 720, each at
  a depth from far to near (its size, its speed and its alpha: 16 to
  86 px/s, 25 to 90 %), drifting with the wind's gusts and its own sway;
  far and middle ones small soft discs, near ones larger and softer.
  Drawn on the layer under the windows.
- **Settling**: the flakes of the middle plane land on the top edges of
  the windows (front first) and of the bar (inset 6 px), else on the
  ground at the bottom of the screen. Each is a heightfield: a column
  every 4 px on an edge (24 px at most), every 6 px on the ground (46 px
  at most, rolling as it starts); its slopes slide to their angle of
  repose a step at a time, an edge's two ends thinned.
- **Its caps**: smoothed by a 9-tap blur, along quadratic curves, white
  at their peak to blue at their base (2 px below the edge), a white
  highlight along their top, a soft shadow on the edge (5 px, blurred
  3 px), and a glint now and then where the snow lies 3 px deep or more.
  The ground: white 50 px up to blue at the bottom.
- **Shedding**: a window whose acceleration passes 2600 px/s² sheds its
  snow, all of it beyond 11 600: in clumps thrown up and along with its
  speed, which fall with gravity (behind the windows), break in four on
  the ground when big and grow it.
- **Starting**: as it is turned on (and on a screen that comes), its piles
  are grown as if it had snowed 50 s: the board's warm-up, each landing
  on the highest edge under it.
- **Stepped** at 60 steps a second from its start with a seeded generator
  (on the tests' held clock too), on the screens that are on and show no
  fullscreen window; a frame is drawn as it steps.
- A cap follows its window as it moves and slides with its desktop; a
  window sent to another desktop takes its cap along, kept while out of
  sight; a minimised or closed window's goes. A window opening starts
  bare. With reduced motion the piles are drawn still: no flakes, no
  clumps, no glints, nothing drawn frame after frame.

Tests: `tests/snow.rs` (on GLES on Mesa's software device: a cap grows
and the window's pixels below its top 8 px stay as they were; a window
shaken sheds its snow in clumps, and moved slowly does not; off, on
pixman or still, nothing drawn frame after frame); its screenshots,
`HOCUS_SHOTS=DIR cargo nextest run -p hocus-compositor --run-ignored only
snow::shots`.

## The agents as creatures (#796)

With `creatures = true`, each agent's panda sits on the top edge of its
agent's window and plays its state, as the board "Les agents en
créatures" does: the bar's validated drawings (`hocus-pandas`), moving
on and on with the board's motion (`hocus_pandas::creature`: working
types, its laptop's star pulsing, bobbing; waiting sways its held-out
hat, its "?" hops, its rings go out; resting breathes as two "z" rise;
done hops, squashing as it lands, waving its hat among its confetti;
stuck puffs its smoke as the pandas canvas does). Each agent its own
phase.

- **Where**: one agent in a window, large (92 px), on the top edge's
  right corner, 12 px in. herdr: one panda (72 px) over each pane of the
  tab herdr shows that an agent runs in (`pane.layout` and its
  `layout.updated` events give each pane's columns; the terminal's are
  the panes' area's reach, herdr's sidebar on its left); herdr's other
  tabs' agents stay in the bar. Several agents in one window (a
  terminal's tabs: the desktop knows the window, not the tab): the shown
  tab's on the left corner (84 px, 18 px in), the others small (52 px,
  60 % opaque) beside it, each standing on its name tag ("blog · autre
  onglet"), as many as fit. The shown one is the one the window's title
  names, else the one that started working last (the user just wrote to
  it), else the one whose state changed last.
- **Never over what the window shows**: a panda's feet rest 3 px into
  the window's top edge, a tag's bottom 4 px: within the top 8 px a
  decoration may cover. Drawn right in front of its window (the snow's
  caps' place): a window in front covers it.
- **No free edge, no panda**: none on a maximised or fullscreen window,
  nor on one without room above it on its screen; the pandas stay in
  the bar.
- **How it is drawn** (`effects/rig.rs`): a mood's drawing is cut where
  its moving parts begin and end, each piece drawn once with resvg into
  one sheet (a texture) per mood and size; each frame lays each piece on
  a quad of its own, moved by its parts' poses (`FireElement::sprites`,
  one draw call a panda, cut to the frame's damage). Nothing is drawn on
  the CPU while it moves. At rest the pieces are the drawing, pixel for
  pixel (each piece falls on whole pixels, in its sheet as on the
  screen).
- **When**: moving where one shows (its window on the desktop shown, on
  a screen that is on with no fullscreen window), a frame each refresh;
  with reduced motion still, nothing drawn per frame; none in the power
  saver, while a screen is shared or recorded, on the CPU, nor in a
  capture.

Tests: `tests/creatures.rs` (on GLES on Mesa's software device: a panda
on an agent's window's right corner, its feet on the edge, the window's
pixels below its top 8 untouched, moving, gone with its agent; none on a
maximised window; none with the switch off, still with reduced motion);
its screenshots, `HOCUS_SHOTS=DIR cargo nextest run -p hocus-compositor
--run-ignored only creatures::shots` (foot terminals, each a process of
its own; a fake herdr).

## Not built yet

The garland (#799) hangs under the active window as an edge's
decoration; the other effects come with their tickets.
