# The pandas

The agents' pandas (docs/design/bar.md §7 A), one per state of an agent's
row in the bar (4.1): `working` at its laptop, `waiting` holding out its
hat, `done` with its arms up, `stuck` with smoking ears, `resting` asleep.

They are Hocuspanda's own artwork, the apprentice magician family drawn
for the project and validated on the design canvas in October 2026, under
the repository's licence (MIT). Each file is one mood of that drawing,
still: the parts the other moods show are taken out, and the view box is
the design report's crop (126 × 120.5 units round the 120-unit drawing),
so that they draw sharp at any size and scale.

The parts of a mood that move on the pandas canvas are each a group with
an id (#649): `spark` (the hat's star, every mood), `swing` (the hat held
out, or the wink's raised arm and hat), `toss` (the hat's contents),
`ring1` and `ring2` (the ear's rings), `ask` (the "?"), `c1` to `c5` (the
confetti), `p1` to `p3` (the smoke), `body`, `z1` and `z2` (the resting
panda's breath and its "z"), `eye` (the wink's open eye), `lid`, `paw-l`
and `paw-r` (the laptop's star, the typing paws). The groups change
nothing drawn; `hocus-pandas` transforms them to draw a frame of a mood's
motion. Keep them when a drawing changes.

`wink.svg` is the same drawing's "wave" mood, winking, its hat raised in
its left paw, its arms at rest: never an agent's state, it peeks out to
say that windows moved with a screen (docs/design/windows.md §12,
"Saying it"), drawn at 200 px.

The secretary (§7 B, 4.6), who heads the agents' group and opens her
briefing, comes from the same family: round glasses, a clipboard, a pencil
behind her ear. Her validated drawing (the 4.6 design canvas, October
2026) is one 120-unit view box; `secretary-calm.svg`,
`secretary-waiting.svg` and `secretary-stuck.svg` are it with her magic
ear, and the dot on her clipboard, in the moment's colour: green, amber,
orange. The rings her ear sends out and her hops are motion, drawn by the
shell over the drawing, so they are not in the files. The shell draws her
at 56 px in the group, 46 px in the mini item and 34 px in her briefing.
