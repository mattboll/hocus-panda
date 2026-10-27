# 0003 — The shell is a client, drawn with iced

Date: 2026-10-27 · Status: accepted, to be confirmed by the spike in
[tech-choices](../design/tech-choices.md)

## Context

The first build drew its shell with its own toolkit inside the compositor's
process. Most defects the user hit came from that toolkit (menus, text
fields, hover), and every try meant logging out, since the shell could not
restart without the session.

## Decision

- The compositor is one process: window management, rendering, input,
  protocols, effects on windows.
- The shell (bar, popovers, launcher, notifications, the lock screen's
  interface, settings) is a separate client drawing layer-shell surfaces,
  started and supervised by the compositor, restartable on its own.
- The shell is drawn with iced (MIT) and iced_layershell (MIT), with our
  theme and a few widgets of our own.

## Consequences

- The shell restarts in seconds without losing a window: development in
  the real session, and a shell crash no longer ends the session.
- A protocol between the two (Wayland protocols plus the control socket)
  to keep stable and tested.
- Effects that span both (blur behind the bar, a streak across windows)
  need compositor support for layer surfaces.
- Supersedes the first build's "shell in the compositor" decision.
