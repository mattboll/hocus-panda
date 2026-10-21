# 0006 — No title bars by default

Date: 2026-10-21 · Status: accepted

## Context

Close, minimize and maximize buttons are rarely used and title bars take room
on every window. Firefox already does without.

## Decision

Server-side decorations draw no title bar by default: windows get rounded
corners, a shadow and a focus accent. Window actions live in the widget
column (active window controls), the radial menu at the pointer, gestures
and shortcuts. Moving uses Alt+drag (or the configured modifier) and the
top edge of the window. Title bars remain an option.

## Consequences

- Client-side decorated apps (GTK/libadwaita) keep their own header bars.
- The radial menu and the active-window controls must exist before this is
  usable day to day.
