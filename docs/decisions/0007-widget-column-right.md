# 0007 — The widget panel is a column on the right

Date: 2026-10-21 · Status: superseded by [0011](0011-panel-strip-by-default.md)

## Context

Four forms were mocked in the same situation (an agent waiting for an
answer, two half-screen windows): a 320 px column on the right, a 72 px strip
that expands on hover, a top bar, and a hidden panel revealed from the edge.

## Decision

A column on the right of the screen, always visible: clock and next meeting,
active window controls, compact desktops line, command field, agents, tasks,
notifications, status row, account. Quick settings (Super+A) slide over it
from the right edge.

## Consequences

- The work area of that screen loses about 320 px; snapping halves account for it.
- The other forms stay possible later as options; the panel content is built
  as independent widgets so it can be rearranged.
