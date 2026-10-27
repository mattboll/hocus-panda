# Design brief

The brief given for the first visual explorations (2026-10-21), updated with
the feedback they produced. The mockups themselves live on the maintainer's
design canvas; this document keeps what they decided.

## 1. Vision

Beautiful, fast, modern, "wow". References: Compiz, Matrix, Minority Report,
Her, Iron Man, in service of daily work rather than demos. Every effect helps
you understand what is happening, or is genuinely beautiful. Quiet while you
work, alive when you look.

## 2. Situations to design for

- One laptop screen; a laptop plus an external monitor of another shape and
  scale (for example 16:10 at scale 2 and an ultrawide 21:9 at scale 1).
- A window maximized on one screen, two windows side by side on another.
- A chat app open all day, a browser, terminals running AI coding agents.
- Meetings with screen sharing; videos in fullscreen.
- Mockups show several screens together, at real size: per-screen desktops
  are central.

## 3. Decided

- **No title bars** by default. Close, minimize, maximize: radial menu at the
  pointer, widget panel, gestures, shortcuts.
- **Discreet dock**: hidden while working, shown on demand or when the
  desktop is empty (after a restart).
- **Widget panel as a slim strip on the right**, opening into the whole
  column over the windows; the column always there, a top bar and a hidden
  panel are options ([decision 0011](../decisions/0011-panel-strip-by-default.md),
  which superseded the column of [0007](../decisions/0007-widget-column-right.md)).
- **Per-screen desktops**; actions aim at the screen under the pointer. No
  permanent desktop indicator taking room: a compact line in the panel, and
  the switcher appears when switching.
- **Alt+Tab** for the active screen, **Super+Tab** for all screens.
- **Command field** ("Search, act, ask…"): AI answers appear inside it.
- Frosted glass, blur, soft shadows, rounded corners, spring animations
  ([motion](motion.md)).
- Notifications top right, sliding in; held during screen sharing.

## 4. Variations explored

- **Themes**: dark, light, Matrix (black and night blue), HUD, paper.
- **Open windows**: empty desk after restart (dock shown, pandas welcome,
  "put everything back"), a code day, fullscreen video, a meeting with screen
  sharing.
- **Agents**: none, working, one waiting for an answer, one acting on screen
  with its own pointer, finished or stuck.
- **Tasks**: a few, one late, all done.
- **Time of day**: morning, afternoon, evening, night (glow, shadow direction,
  clock).

## 5. Screens mocked

Each answered a question:

1. Desktop on two screens: Hocuspanda without title bars, dock hidden, column on the right.
2. Radial menu at the pointer vs a pill on the top edge on hover.
3. Per-screen desktop switcher, and what happens when the external screen is unplugged.
4. Alt+Tab (active screen) vs Super+Tab (all screens).
5. Agent seat and agent desktop.
6. Circle to ask.
7. Command field with an AI answer; notification triage.
8. Compiz-like effects: depth of field, X-ray, glowing edges, world light, living wallpaper, physical windows.
9. Productivity: tabs across apps, live window clip, contexts, summon a window, sound per window, hint mode.
10. Infinite map desktop.
11. The screen you look at: three levels (notifications only, shortcuts too, pointer follows).
12. Boot, curtain, lock screen.
13. Four panel forms in the same situation.
14. The full column and its menus: power profile, keyboard layout, account, desktops.
15. Quick settings (Super+A).

## 6. Still to draw

Calendar month, Wi-Fi and Bluetooth lists, volume and brightness OSD,
recording in progress, notification-area menus, clipboard history, emoji,
notes preview, camera/microphone/capture indicators, "waiting for you",
phone and updates details, developer widgets.

## 7. Limits of mockups

Mockups show look, layout and simple motion. Real smoothness, blur on real
windows and frame cost are judged on the compositor itself.
