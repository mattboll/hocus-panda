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

Several pandas that **show the real state of the desktop**:

| State | Panda |
|---|---|
| A coding agent works | One panda per agent, at its laptop |
| An agent waits for your answer | Raised paw, question bubble |
| An agent finished | Arms up, sparkles |
| An agent is stuck or failed | Paw on the head, question mark |
| Nothing is happening | Plays with a ball |
| A video plays fullscreen on the other screen | Eats popcorn, looking at it |
| Before locking, locked | Yawns, then sleeps on the lock screen |
| Boot, session restore | Waves, sets the desktop up |
| Logo | Top hat with an accent band, sparkles |

**Not Clippy.** The pandas:
- never interrupt and only talk when asked;
- can be turned off with one setting (quick settings: "Pandas");
- do not move when nothing changes: no cost at rest, none on battery saver;
- are original drawings: no resemblance to the WWF logo or Kung Fu Panda.

Styles to explore: flat vector (current mockups), soft 3D, pixel art. Artwork
may get its own license (CC BY or CC0) next to the MIT code.

## Colour

The panda's black and white is the natural base of the dark and light themes,
plus one accent colour.

| Theme | Base | Surface | Accent | Feel |
|---|---|---|---|---|
| Dark | `#0E0F12` | `#1A1C21` | `#7BC47F` bamboo | ink, frosted glass, bamboo |
| Light | `#E9E6DF` | `#FFFFFF` | `#2E7D4F` | white fur, soft shadows |
| Matrix | `#02040A` → `#050B1C` | `#040A16` | `#00F060` | green rain on black and night blue, all monospace |
| HUD | `#050A14` | `#0A1A2E` | `#38D9F5` | head-up display, thin lines, grid |
| Paper | `#F1E9DA` | `#FFFBF3` | `#56702A` | warm, ink and bamboo |

Accent options: bamboo `#7BC47F`, amber `#F2A65A`, blue `#8AB4F8`, pink
`#F28DB2` (darker variants on light themes for contrast). Themes can be set
per desktop.

Agents each get their own colour (cursor, label, halo); violet `#B48CFF` in
the mockups for an agent acting on screen. Waiting = amber, error = coral.

Text contrast: 4.5:1 minimum everywhere, including small grey labels.

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
