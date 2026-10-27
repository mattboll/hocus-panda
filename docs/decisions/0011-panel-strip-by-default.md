# 0011 — The widget panel is a thin strip by default, opening into the column

Date: 2026-10-27 · Status: accepted · Supersedes [0007](0007-widget-column-right.md)

## Context

Decision 0007 kept the 320 px column always on the right of the primary
screen. Used day to day, it takes too much of the screen: a maximized
window loses a fifth of a laptop's width for widgets that are mostly
glanced at. Board D12 had mocked four forms in the same situation (an agent
waiting for an answer, two half-screen windows): A, the column; B, a 72 px
strip that opens into the column on hover; C, a bar at the top; D, hidden,
the panel coming out of the right edge.

## Decision

The strip (form B) is the default; the column, the top bar and hidden are
options, `[panel] form = "strip" | "column" | "top" | "hidden"`, chosen on
the settings' Appearance page and applied at once.

- **Strip**: 72 px on the right, from top to bottom the time (stacked), the
  next meeting's ring, a panda for each active agent and how many others
  there are, the to-dos and notifications counted, then the screen's
  desktops, the battery and the account. Resting the pointer on it, a
  click on it, or Super+C slides the whole column over the windows from
  the right edge; leaving it, Escape, Super+C again or a click elsewhere
  slides it back. Windows leave room for the strip only.
- **Column**: as decision 0007 had it.
- **Top bar**: one line across the top, with the screen's desktops, the
  active window and its buttons, the time and the date with the next
  meeting, the active agents, the tasks and the notifications. A click on
  it away from its buttons, or Super+C, brings the column out.
- **Hidden**: windows get the whole screen; a faint hint marks the right
  edge, and resting there (or Super+C) brings the column out. Toasts, an
  agent's question among them, still show.

The column out is the same column, drawn over the windows and their toasts;
it slides as docs/design/motion.md says, and not at all with reduced motion.

## Consequences

- The work area loses the strip (72 px and its margin), the bar (64 px at
  the top) or nothing, instead of the column; snapping, maximizing and the
  dock follow the work area as before.
- What the column says stays reachable everywhere; the strip and the bar
  show a glance of the same state, built again only when what they show
  changes.
- Scenarios about the column itself say `[panel] form = "column"`; the other
  forms have scenarios of their own.
- Quick settings (Super+A) still slide over whatever is on the right.
