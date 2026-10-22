# Identity

## Name

**Hocuspanda** — the magician panda. Chosen on 2026-10-21 over Pandawitch,
Magicpanda, Pandamonium, Zouyu and Pandacadabra: free on GitHub, on the main
domains and package registries, easy to say in French and English.

Naming in code and on the system:
- repository, crates, config directory: `hocuspanda` (`~/.config/hocuspanda`)
- short command: `hocus` (`hocus msg`, `hocus scenario`…)
- avoid the `hp-` prefix (HPLIP tools) and `pw-` (PipeWire)
- D-Bus and app IDs: `org.hocuspanda.*` (to confirm once the domain is registered)

## Mascot

Two characters, one family (`hocus-shell/src/panda.rs`):

- **The apprentice magician**: mischievous, a little overtaken by his own
  magic. A patched top hat that does everything (held out to ask, thrown
  when done, over his eyes to sleep, a popcorn bucket's neighbour), a
  sparkle pinned in its band, a stage bow tie, a wand, a wink.
- **The watcher**: calm and round, keeps the machine at night. Welding
  goggles pulled down to work, a scarf, a lantern raised to call you.

Agents get them in turn, so two agents side by side are told apart. What
ties them: one ear **touched by magic**, in the accent colour, that says
the state: lit while the panda acts, radiating when something waits for
you, orange and drooping when stuck, dark asleep; and the sparkle. Dark
parts are slate (`#23262E`), never pure black, with a cream rim so they
read on a dark background; no pink cheeks.

They **move for a moment when their state changes** (juggling, the hat
thrown, sparks, smoke, a lantern swaying), then hold still; a panda that
is a page's subject (the welcome) may move all the time. With reduced
motion or on battery saver they hold still.

**Logo**: the apprentice's head, winking, with the magic ear and a sparkle
("the magic ear"). Kept in reserve: a panda coming out of the top hat
instead of a rabbit, and a top hat with two panda ears. The exploration
board has all three.

They **show the real state of the desktop**:

| State | Panda |
|---|---|
| A coding agent works | One panda per agent, at its laptop or welding |
| An agent waits for your answer | Holds out his hat or raises his lantern; the ear radiates |
| An agent finished | Arms up, sparkles, the hat thrown |
| An agent is stuck or failed | The spell smoked; the ear turns orange |
| Nothing is happening | Juggles |
| A video plays fullscreen on the other screen | Eats popcorn, looking at it |
| Before locking, locked | Yawns, then sleeps on the lock screen |
| Boot, session restore | Waves, sets the desktop up |
| Logo | The apprentice's head, winking, the magic ear, a sparkle |

**Not Clippy.** The pandas:
- never interrupt and only talk when asked;
- can be turned off with one setting (quick settings: "Pandas");
- move only a moment when something changes: no cost at rest, none on battery saver;
- are original drawings: no resemblance to the WWF logo or Kung Fu Panda.

Flat vector, drawn from shapes by the toolkit at any size. Artwork
may get its own license (CC BY or CC0) next to the MIT code.

## Colour

The panda's black and white is the natural base of the dark and light themes,
plus one accent colour.

| Theme | Base | Surface | Accent | Feel |
|---|---|---|---|---|
| Dark | `#0E0F12` | `#1A1C21` | `#7BC47F` bamboo | ink, frosted glass, bamboo |
| Light | `#E9E6DF` | `#FFFFFF` | `#287046` | white fur, soft shadows |
| Matrix | `#02040A` → `#050B1C` | `#040A16` | `#00F060` | green rain on black and night blue, all monospace |
| HUD | `#050A14` | `#0A1A2E` | `#38D9F5` | head-up display, thin lines, grid |
| Paper | `#F1E9DA` | `#FFFBF3` | `#56702A` | warm, ink and bamboo |

Accent options: bamboo `#7BC47F`, amber `#F2A65A`, blue `#8AB4F8`, pink
`#F28DB2` (darker variants on light themes for contrast; each theme file in
`crates/hocus-ui/themes/` holds its own). Themes can be set
per desktop.

Agents each get their own colour (cursor, label, halo); violet `#B48CFF` in
the mockups for an agent acting on screen. Waiting = amber, error = coral.

Text contrast: 4.5:1 minimum everywhere, including small grey labels; a test
checks every text colour on every background of every theme, with every
accent.

## Typography

- **Fraunces** — the name and large titles (clock, greetings)
- **Manrope** — the whole interface
- **JetBrains Mono** — terminals, code, the Matrix theme
- **Space Grotesk** — the HUD theme

All under the SIL Open Font License, bundled with the desktop.

## Wallpaper

A soft glow placed by the time of day (sun position in the morning, low and
orange in the evening, night blue at night) over the base colour, with bamboo
silhouettes in the bamboo themes, a grid in HUD, rain in Matrix. Night light
is a separate setting, not baked into the themes.
