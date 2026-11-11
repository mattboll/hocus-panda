# 0005 — The shell's surfaces are iced's widgets, and a canvas only where the look draws

Date: 2026-11-11 · Status: accepted (#770), piloted on the Wi-Fi's,
Bluetooth's and the sound's popovers

## Context

Since 0003 the shell drew each surface on one iced canvas: its own
layout, its own hit-testing (an `enum Target` per surface, the pointer
matched against rectangles), its own focus and ring, its own buttons,
lists and wheel scrolling, and, beside the drawing, a list of rows for
screen readers written again by hand. The November 2026 audit counted 24
`enum Target`, 9 hit-testing implementations, 7 button drawings, 3 focus
rings and 5 wheel scrollers; tests asserted on `Debug` names
(`focus=Some(Force(0))`). The same control was described three or four
times, and the descriptions drifted: the text field and focus bugs of
sprints 11 and 12 (#763, #771) came from there, and the screen readers'
tree missed whole surfaces.

Two experiments rebuilt a popover from iced's stock widgets: the Wi-Fi's
(the spike of #770) and the music player's block and popover (sprint 12).
Both kept the mockups' look.

## Decision

- A shell surface is built from iced's stock widgets (`container`, `row`,
  `column`, `button`, `scrollable`, `text`, `mouse_area`, `stack`) and the
  shell's widget kit (`hocus-shell/src/ui`): the surface around a
  popover's widgets (`Around`), the declared control (`control`: its id,
  role, name, state, what the keyboard does on it, the ring drawn outside
  it), the shared text field (`field::Field` over `field::Entry`), a
  volume's slider, and the popovers' shared parts.
- From the one tree a surface builds, the kit derives what each surface
  wrote by hand: hit-testing and hover (the stock widgets'), the
  keyboard's order and keys (`Tab`, the arrows, `Enter`, `Space`,
  `Escape`), the ring, the screen readers' rows, and the tests' dump, by
  id.
- A canvas only where the look needs drawing: the pandas, the halo, the
  gauges, glyphs and icons, a switch's knob, a slider's track and knob.
  A canvas is a widget like another in the tree.
- New surfaces are built this way; the others move when they are touched
  (#772).

## Consequences

- Measured on the three popovers piloted, headless on the CPU, dark, ×1,
  in their main state: 0.50 % (Wi-Fi), 0.81 % (Bluetooth), 0.91 % (sound)
  of pixels off the canvas version, none by more than 24/255
  (anti-aliasing of rounded corners and dividers). Their tests pass
  unchanged; the screen reader's finds controls by their ids.
- The three popovers' hand-written hit-testing, focus, ring and rows are
  gone; the shell lost about 1,500 lines with them.
- iced 0.14 gives the keyboard's focus to text fields only, has no
  letter-spacing nor ellipsis, and its stock slider and toggler lack the
  mockups' shadows: the kit carries these (its traversal, spaced labels,
  `kit::elide`, its own slider and switch).
- A popover's view is built again on each change, its text measured
  through a cache; at rest nothing draws, as before.
