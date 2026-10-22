# Motion

Motion is a first-class part of Hocuspanda, not decoration added at the end.

## Principles

- **Motion carries information or is genuinely pleasant.** Where did the
  window go, what is waiting, what changed. Nothing loops just to look busy.
- **No cost at rest.** When nothing changes, nothing is redrawn. A looping
  effect (breathing halo, battery bubbles) runs only while it carries
  information, and is capped.
- **Paced by the compositor.** Animations advance on the frame clock of the
  output they are on, using monotonic time. They never queue work per tick.
- **Shared vocabulary.** Durations: 150 ms (small, feedback), 250 ms (standard),
  400 ms (large, spatial). Easings: out-cubic to enter, in-cubic to leave,
  springs for anything the user drags or flicks. Springs are critically damped
  by default, with a little bounce only where it means "settled".
- **Interruptible.** A new action retargets a running animation from its
  current state; nothing waits for an animation to finish.
- **Reduced motion.** One setting turns movement into short fades or instant
  changes; every effect honours it, looping ones included.
- **Testable.** The scene can be sampled at a given animation time, so a
  scenario can capture "halfway through the minimize" deterministically.

## Windows

| Moment | Motion |
|---|---|
| Open | Fade + scale from 96 %, out-cubic 250 ms; themed variants (Matrix rain) |
| Close | Dissolve; themed variants (Matrix glitch and rain collapse) |
| Move | **Wobbly windows**: the window bends like jelly, lagging behind the pointer the farther from where it is held, settles with a small bounce; drawn on a spring mesh |
| Flick | A thrown window keeps its momentum, glides and snaps (on the other screen if aimed there) |
| Minimize | **Magic lamp** (genie) towards its dock icon, or the panel when the dock is hidden |
| Restore | Reverse genie from where it went |
| Maximize, fullscreen | Snapshot morph from the old to the new geometry, content crossfades |
| Snap | Preview zone fades in under the pointer; the window springs into place |
| Resize | Live, no stretching of stale content |
| Focus change | Dim and depth-of-field fade on the windows behind; accent fade on the focused one |
| Attention | Breathing amber halo; dock icon wiggle; edge glow in the app colour |
| Dialogs | Appear from their parent with a short scale; the parent dims behind modal ones |
| Send to desktop | The window flies towards the desktop indicator of its screen |

Closing, minimizing and maximizing draw the window's last picture while its
client moves on: its surfaces' textures, kept by handle as the window
changes (nothing is copied), drawn stretched or bent. A client that quits
takes its surface with it, so the picture is taken as the surface goes.
A maximizing window heads for the place it was given, not for its size
of the moment: the client follows at its own pace.

## Desktops, overview, switching

| Moment | Motion |
|---|---|
| Switch desktop | Slide on the screen that switches only; windows follow the fingers during a three-finger swipe and spring to the nearest desktop |
| Desktop switcher | Appears while switching, previews in the screen's proportions, fades out after |
| Overview (expo) | Windows glide from their places to the grid and back; pinch can scrub it |
| Alt+Tab, Super+Tab | Fade + scale in, gliding selection, live previews |
| Theme per desktop | The colours crossfade with the slide |

## Panel, widgets, dock

- Liquid battery: level visible while charging, bubbles, bolt, heartbeat under 10 %
- Volume arcs; OSD with a rubber-band overshoot and close fade
- Brightness sun whose rays grow with the level
- Wi-Fi: connecting sweep, strength
- Keyboard layout odometer (the label rolls to the next layout)
- Meeting ring that fills as the meeting approaches
- Bell swing on a new notification; clipboard copy pulse
- Dock: spring magnification, launch bounce and dot, attention wiggle and breathing
- Panels and popovers: open/close fade + scale from where they come from
- Launcher: results cascade in, the selection glides
- Screenshot: flash, then the picture flies into its notification
- Recorder: breathing dot while recording
- Desktop indicator glides as soon as a switch starts

## Notifications

- Slide in from the screen edge, newest on top; the stack reflows with springs
- A thin countdown line; hovering pauses it
- Dismiss by flick, with momentum
- Held while sharing the screen or watching fullscreen video; a single summary appears afterwards
- On the screen you are working on (or looking at, see the gaze idea)

## Menus and popovers

- Open from their origin (the icon, the pointer) with fade + scale, close faster than they open
- The radial menu unfolds around the pointer; a quick flick in a direction triggers the action without waiting for the menu to show
- Submenus slide in the reading direction

## Session moments

- Boot: the pandas set the desktop up while windows are restored
- Curtain: the screen dims progressively before locking; any input lifts it
- Lock and unlock: the lock screen fades in over a blurred snapshot; unlock reveals the desktop from the centre

## Matrix theme

A complete set of themed animations (window open, close, minimize, restore,
maximize, snap, move, resize, desktop switch, overview, Alt+Tab, lock screen,
notifications, panel, launcher, boot), specified in
[matrix-theme.md](matrix-theme.md). Palette: green rain on black and night
blue, never brownish.

## The pandas

Static unless something changes: a panda changes pose when its state changes
(agent starts working, waits, finishes), with a short transition. No idle
loops beyond an occasional blink, and none at all on battery saver.
