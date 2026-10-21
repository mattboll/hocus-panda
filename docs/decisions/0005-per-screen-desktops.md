# 0005 — Virtual desktops are per screen

Date: 2026-10-21 · Status: accepted

## Context

With global desktops, two screens switch together and previews show both
screens squashed side by side. A common use is one screen that stays on a
chat app all day while another switches by project.

## Decision

Each screen has its own list of desktops and switches alone. The number of
desktops is set per screen; a screen with one desktop never switches (fixed
screen). Shortcuts, gestures and the switcher act on the screen under the
pointer. Each desktop can have its own theme.

When a screen is unplugged, its desktops join a remaining screen and return
when it is plugged again. This is to be tried; if it proves annoying, the
fallback is to gather its windows on a single desktop.

## Consequences

- The core model is "outputs own desktops own windows", with stable desktop
  identities across hot plug.
- Sending a window to the other screen targets its shown desktop or a chosen one.
- External tools (foreign-toplevel, ext-workspace) see one workspace group per output.
