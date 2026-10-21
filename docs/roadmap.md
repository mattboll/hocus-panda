# Roadmap

Milestones in order. Each is a usable step; the detailed tasks are GitHub
issues in the matching milestone. The order front-loads what everything
else depends on (the test harness, the window model) and the main technical
risk (the UI toolkit).

## M0 — Foundations

*It runs nested, and CI can drive it.*

Workspace, CI (Linux and FreeBSD build), license checks; compositor skeleton
in a window of the current session; headless backend on a virtual clock;
control API v0 (windows, outputs, input injection, scene dump, screenshot);
scenario runner with a test client and reference screenshots; frame timing
and a loop watchdog from day one.

## M1 — Window model

*Windowing good enough to live in, nested.*

Pure core model (outputs own desktops own windows); placement on the screen
under the pointer; move and resize; decorations without title bars (rounded
corners, shadow, focus accent); snapping with preview, magnetic edges, gaps,
keyboard snapping; maximize, fullscreen, minimize; several outputs with hot
plug; per-screen desktops; free shortcuts with scopes; keyboard layouts;
configuration file applied live; XWayland.

## M2 — UI toolkit and the column

*The shell appears.*

Rendering stack decision with measurements; `hocus-ui` (layout, text, theme
tokens, springs, hit testing, damage) runnable alone; themes and bundled
fonts; the widget column on the right (clock, meeting, active window
controls, desktops line, status row); notification server and toasts; the
launcher and command field v0; quick settings (Super+A).

## M3 — Real session

*Log in to it.*

DRM, libinput and libseat backend, tested in QEMU first; output management
(modes, scale, VRR); session lock and lock screen; idle and inhibitors
(fullscreen keeps awake); Linux platform services (power, battery, backlight,
network, audio, power profiles); session save and restore, autostart;
protocol batch (layer-shell, screencopy and portal, xdg-activation,
foreign-toplevel, idle-notify…).

## M4 — Motion and effects

*It moves like it should.*

Animation framework on the frame clock; open, close, genie minimize and
restore, wobbly windows, maximize morph, desktop slide and swipe; blur and
shadows; discreet dock; Alt+Tab and Super+Tab; overview; radial menu.

## M5 — Agents and AI

*Agents are first-class users.*

Agents widget (agent managers such as herdr) and notifications with replies; agent API over MCP;
agent seat (own pointer and keyboard) and "show before acting"; AI answers in
the command field and intents; document search with its security rules;
circle to ask.

## M6 — Identity

*Hocuspanda looks like itself.*

The pandas and their states; boot, curtain and lock moments; themes per
desktop; the complete Matrix theme; French translation reviewed.

## M7 — FreeBSD and packages

FreeBSD platform services and a real FreeBSD session; .deb, .rpm and FreeBSD
packages.

## Later

Ideas kept or to explore from [ideas.md](ideas.md): contexts, live window
clips, tabs across apps, summon a window, sound per window, hint mode,
depth of field, X-ray, glowing edges, world light, the screen you look at,
meeting copilot, notification triage, voice.
