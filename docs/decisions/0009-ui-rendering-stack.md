# 0009 — hocus-ui draws on the CPU with vello_cpu and parley

Date: 2026-10-22 · Status: accepted

## Context

The shell is drawn by the compositor ([0003](0003-shell-in-the-compositor.md)):
`hocus-ui` rasterises widgets into textures that smithay's GLES renderer
composites, and redraws a widget only when its content changes. Its renderer
is the main technical risk, so it is chosen on measurements.

Three permissive candidates drew the same column
([prototypes/ui-render](../../prototypes/ui-render/)): 320 logical pixels
wide, seven widgets (clock, agenda, weather, system, media, an agent card
changing every frame, notes), French text with accents, ligatures, guillemets
and narrow no-break spaces, in the identity's variable fonts (Manrope,
Fraunces).

- **tiny-skia + cosmic-text**: CPU; glyphs from swash blended by hand.
- **vello_cpu + parley**: CPU, sparse strips (linebender).
- **vello + parley**: GPU compute through wgpu.

Release builds, one thread, warm caches, median of 200 frames (full column)
and 1 000 frames (one widget), on a recent x86-64 desktop with a discrete GPU.
Absolute numbers vary by machine; the ratios are what matter.

| | tiny-skia + cosmic-text | vello_cpu + parley | vello GPU + parley |
|---|---|---|---|
| Full column, scale 1 | 0.70 ms | 0.74 ms | 1.42 ms |
| Full column, scale 2 (640×1640) | 2.65 ms | **1.50 ms** | 1.66 ms |
| One widget, scale 1 | 0.11 ms | **0.06 ms** | 0.95 ms |
| One widget, scale 2 | 0.36 ms | **0.13 ms** | 0.95 ms |
| Column without text, scale 2 | 2.23 ms | 0.47 ms | 1.23 ms |
| Back to memory, one widget, scale 2 | — (already there) | — | 1.13 ms |
| Init | < 1 ms | 0.4 ms | 124 ms warm, 2 s cold |
| Peak memory (whole process) | 15 MB | 17 MB | 260 MB |
| Stripped binary (whole prototype) | 3.3 MB | 3.8 MB | 9.0 MB |

GPU times are until the GPU is done; encoding the scene takes microseconds,
so the GPU path is dominated by a fixed cost per submission.

Text quality is the same in all three at scale 1 and 2: grayscale
antialiasing, correct shaping of French (accents, `œ`, `fi`/`ffi` ligatures),
line breaking that respects narrow no-break spaces (`23 %`, `« … »`,
`1 234,56 €`), variable weights applied. None of them falls back to another
font by itself when a glyph is missing (`ᵉ` is not in Manrope): a fallback
font has to be bundled whatever the choice.

## Decision

`hocus-ui` draws with **vello_cpu**, and lays out text with **parley**, on the
CPU, into pixmaps uploaded as textures.

- It is the fastest for what the shell does most: redrawing one widget
  (0.13 ms at scale 2, a fraction of a 16 ms frame), and still the fastest
  for a whole column at scale 2.
- Pixels are the same on every machine and need no GPU: the headless
  compositor and its reference screenshots on CI (pixman, no GPU) see what
  users see ([0008](0008-ai-testable-by-design.md)).
- It integrates like the compositor's existing textures (shadows, focus ring):
  a pixmap uploaded through smithay's memory buffers. The GPU path would need
  either a copy back to memory (1.1 ms per widget, more than drawing it on the
  CPU) or sharing buffers between wgpu and smithay's GLES context, and costs
  260 MB and a GPU driver in the compositor.
- It brings what a shell needs: a stack of layers with clip, opacity and blend
  modes, and blurred rounded rectangles for shadows (tiny-skia has clip masks
  and per-paint blending, but no layers and no blur).
- All permissive: vello_cpu, parley, fontique, skrifa and harfrust are MIT or
  Apache-2.0.

## Consequences

- `hocus-ui` puts the renderer behind its own small painting interface
  (rounded rectangles, paths, strokes, text, images, clip, opacity layers), so
  that changing renderer stays local to it.
- The linebender crates are young (vello_cpu 0.3) and their versions move
  together: parley 0.12 and vello 0.11 already disagreed on the type of
  variable font coordinates. Upgrades go together, behind that interface.
- vello_cpu's glyph atlas is experimental (5 ms spikes, 80 MB in the
  prototype); it stays off. Laid out text is cached by `hocus-ui`, so only
  changed text is shaped again.
- Drawing happens on the event loop, widget by widget, only when content
  changes; pixmaps are plain owned data, so drawing can move to a worker
  thread later without changing the model.
- A fallback font for missing glyphs (emoji, symbols, other scripts) is
  bundled with the fonts (#22).

## When to revisit

- vello_cpu stops being maintained, or a parley/vello_cpu upgrade breaks
  without a path forward: tiny-skia + cosmic-text is the measured fallback
  (slower at scale 2, same text quality).
- The real shell misses its frame budget on the weakest supported machine
  with drawing on the CPU (#88).
- A GPU renderer can draw straight into smithay's GLES context without a copy
  (vello_hybrid or similar), with a fixed cost per submission well under a
  millisecond.
