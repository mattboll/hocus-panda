# The desktop's effects (#434)

Effects for the fun of it, fourteen of them, prototyped and approved on
the effects canvas ("Effets du bureau"; sources in the maintainer's
`hocus-panda-tools/mockups/s15-fx`). Each is off by default and has its
own switch, `[effects]` in the configuration and its row in Settings ›
Appearance › Effects ([settings.md](settings.md), "Apparence et
langue").

| Group | Effect | Key | Ticket |
|---|---|---|---|
| The wallpaper | Neige qui tient | `snow` | #795 |
| | Aurore en musique | `aurora` | #801 |
| | Pluie sur la vitre | `rain-on-glass` | #802 |
| | Bassin de koïs | `koi` | #803 |
| | Lucioles du soir | `fireflies` | #804 |
| | La lumière du jour | `daylight` | #805 |
| | Bureaux en profondeur | `depth` | #806 |
| The windows | Fermer en flammes | `flames` | #794 (built) |
| | Éclats de verre | `shatter` | #798 |
| | Fenêtres en gelée | `jelly` | #797 |
| | Guirlande sous la fenêtre active | `garland` | #799 |
| The moments | Tests au vert : feu d'artifice | `fireworks` | #810 |
| | Bonne nouvelle d'un agent : confettis | `confetti` | #811 |
| The agents | Les agents en créatures | `creatures` | #796 |

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
- once an effect is over, nothing more is drawn for it, and no frame is
  asked for.

An effect is decided as it starts: a switch turned off, or a rule that
starts holding, leaves one under way to end.

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
  it (`ONE, ONE_MINUS_SRC_COLOR`).

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
  window, fading out in the last 18 % of the burn;
- the window's soft shadow fades out in the first 22 %;
- seven embers a step along the front and a puff of smoke seven times in
  ten, while it burns; the embers left are swept away 2.8 s after it
  starts, the smoke drifts out on its own (all gone by 5 s);
- a warm light (orange to red, 620 px around the window's middle at the
  front) as bright as the fire, which rises then falls over the burn,
  flickering.

The window leaves the desktop as it starts (its picture alone burns):
input never reaches it, and the next window takes the focus at once. Its
sparks, its picture and its light are drawn front first over the windows,
as the windows leaving are.

Tests: `tests/effects.rs` (a window burning on GLES on Mesa's software
device, its flames over and above it, gone within 5 s and nothing drawn
after; on pixman, with reduced motion or the switch off, the standard
close); its screenshots, `HOCUS_SHOTS=DIR cargo nextest run -p
hocus-compositor --run-ignored only effects::shots`.

## Not built yet

The animated layer under the windows (over the wallpaper, on the
screens where it shows) comes with the snow (#795); the decorations
around a window's edges with the garland (#799); the others with their
tickets.
