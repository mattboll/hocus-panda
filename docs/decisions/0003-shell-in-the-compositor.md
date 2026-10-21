# 0003 — The shell is drawn by the compositor, in one process

Date: 2026-10-21 · Status: accepted

## Context

A shell made of separate client windows (a full-screen panel window, menus
and notifications as separate windows positioned over IPC, state shared
through polled files) misplaces popups, restarts on output changes and
repaints whole windows for small changes (see [robustness](../robustness.md)).

Alternatives: the shell as layer-shell clients (COSMIC's approach), or drawn
inside the compositor.

## Decision

The shell (widget column, dock, launcher, notifications, quick settings,
menus, lock screen, pandas) is drawn by the compositor in its own scene, in
the same process and on the same event loop.

## Consequences

- The shell knows its surfaces by identity, output and layer; placement bugs
  between processes disappear.
- Shell animations share the compositor's frame clock and damage tracking.
- A bug in the shell can take the compositor down: the shell code must keep
  the same standards (no panics on the loop, no blocking calls).
- Third-party bars and notification daemons remain possible through
  layer-shell, for people who want them.
- We need our own UI toolkit (`hocus-ui`).
