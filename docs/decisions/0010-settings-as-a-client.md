# 0010 — The settings app is a Wayland client drawn with hocus-ui

Date: 2026-10-22 · Status: accepted

## Context

The settings are a window like any other: they sit on a desktop, take the
focus, tile, snap, minimize and go to another screen. The shell's surfaces
(column, launcher, toasts, quick settings, menus, lock screen) are drawn by
the compositor ([0003](0003-shell-in-the-compositor.md)) because they must be
placed exactly and share the frame clock; a window has neither need.

Alternatives:

- **A window drawn by the compositor**: every part of the window model
  (desktops, stacking, focus, tiling, screenshots, foreign toplevels, title
  bars) would learn a second kind of window besides Wayland and X11 ones,
  and a bug in a settings page could take the session down.
- **A separate client with another toolkit** (GTK, Qt, iced): a second look
  to keep in step with the shell, and dependencies we do not want.
- **A separate client drawn with `hocus-ui`**: the same widgets, theme and
  fonts as the shell, in a process of its own.

## Decision

`hocus-settings` is a Wayland client of its own, drawn with `hocus-ui` into
shared-memory buffers. It reads what is in effect through the control API
(`hocus-api`), follows changes through its event stream, and changes
settings by editing the configuration file, keeping the user's comments and
layout; the compositor applies the file live as for any edit (#18). The
compositor starts it (Super+Comma, the command field, quick settings) or
brings its window forward, and tells a running one which page to show
through the event stream.

## Consequences

- Settings windows behave like every window, with no special case in the
  compositor.
- A crash in the settings app loses a window, not the session.
- The configuration file stays the single source of truth: a change made in
  the app, by hand or by another tool shows up everywhere.
- `hocus-ui` gains nothing compositor-specific; the app converts its pixels
  for `wl_shm` itself.
- Tests run the app in-process as a client of the headless compositor, and
  read its accessibility tree directly.
