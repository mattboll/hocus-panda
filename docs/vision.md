# Vision (v2)

## In one sentence

The desktop of a developer who works with AI agents: they see at a glance
what their agents are doing, and everything else is beautiful, fluid and
never in the way.

## Who it is for

A developer running several agents (Claude Code and others) on one or more
screens. Not the general public in v1. It must work on any common screen,
from 1366×768 up, not only on the author's.

## What sets it apart: following the agents

- Each agent's state is always visible without looking for it: working,
  waiting for you, done, failed.
- A clear signal when an agent waits for you, and one click to its window.
- The pandas embody that state, from the first version.

## Daily use, which must be flawless

1. Floating windows with snapping (halves, quarters), virtual desktops,
   Alt+Tab. Wobbly windows are part of the charm; no transparency while
   moving a window.
2. Sound, Wi-Fi, Bluetooth, battery and brightness, set in two clicks.
3. Everything at a glance without taking room: the bar (see
   [the bar's spec](design/bar.md)).
4. A launcher and notifications that can be cleared.
5. Locking, and the session started from a display manager.
6. Several screens of different scales, plugged and unplugged at will.

## Style

Spectacular yet careful and rather sober: striking animations at the key
moments (opening, closing, switching desktops, an agent calling you),
quick, never in the way at work. Futuristic through light and material,
a little kawaii through the pandas.

## Quality

Each step is accepted against the mockup the user approved, on the user's
machine (the VM when the machine cannot run it yet), with no freeze or
stutter.

## Order of delivery

1. Windows and desktops.
2. The bar, with sound, Wi-Fi, Bluetooth, battery.
3. Following the agents, and the pandas.
4. Launcher and notifications.
5. Lock and session.
6. Several screens.

## Not in v1

Tiling; agents driving the desktop with their own pointer; the
natural-language command; the Matrix theme; FreeBSD; plugins.
