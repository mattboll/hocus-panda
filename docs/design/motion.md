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
| Move | **Wobbly windows**: the window bends like jelly, lagging behind the pointer the farther from where it is held, settles with a small bounce (within 600 ms); a 4×4 grid of springs, drawn in strips of its picture as the genie is (its shadow and ring wait until it is still); the Matrix theme keeps it rigid and leaves a phosphor trail instead |
| Flick | Let go faster than 1100 px/s, a window keeps its momentum, glides (it travels its speed × 0.22 s, over 0.6 s) and lands inside the work area of the screen its middle is aimed at, on the desktop that screen shows |
| Minimize | **Magic lamp** (genie) towards its dock icon, or the panel when the dock is hidden; a short fade where it is with reduced motion |
| Restore | Reverse genie from where it went; a short fade with reduced motion |
| Maximize, fullscreen | Snapshot morph from the old to the new geometry, content crossfades; with reduced motion the window is in its new place at once, its old picture fading shortly where it was |
| Snap | Preview zone fades in under the pointer; the window is in place at once (it springs into place in the Matrix theme) |
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

- Boot: while a saved session is put back, the boot screen shows the logo,
  the name, how many windows are back and two pandas at work; it goes as soon
  as they are all back (at most after 5 s, an application slow to come is
  reported), fading out over `STANDARD`
- Curtain: the screen dims progressively before locking, with a panda
  falling asleep over the countdown; any input lifts it
- Lock and unlock: our lock screen fades in over the desktop it hides
  (`STANDARD`; over a blurred picture once there is blur, #37), two pandas
  asleep in its corner; unlocked, it parts from the centre, its two halves
  sliding apart over `LARGE`. A lock client's own screen is left as it draws it.

## Matrix theme

A complete set of themed animations (window open, close, minimize, restore,
maximize, snap, move, resize, desktop switch, overview, Alt+Tab, lock screen,
notifications, panel, launcher, boot), specified in
[matrix-theme.md](matrix-theme.md). Palette: green rain on black and night
blue, never brownish. The window motions are drawn with plain elements, so
that every renderer shows them and scenarios sample them; with reduced
motion they give way to the short fades of every theme. Every decorated
window has a hairline just inside its edge in the theme's tint (green in
this theme, faint white in the dark one), as the boards draw it. The
shell's moments are drawn the same way; with reduced motion they keep the
theme's colours (the black behind the lock and Alt+Tab, the padlock, the
launcher's dim) and come and go as in every theme.

## The pandas

Static unless something changes: a panda changes pose when its state changes
(agent starts working, waits, finishes), with a short transition. No idle
loops beyond an occasional blink, and none at all on battery saver.
