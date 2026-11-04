# Ink: drawing on the screen — specification

Status: approved 2026-11-04 (issue #564), variant C of the mockup (the
keys). Built since 2026-11-04: see [as built](#as-built). Sizes are
logical pixels. This first version has the stroke, the arrow and the
laser, all of them seen in shares and recordings. Kept for later: strokes
that stay, other shapes, the highlighter, colours, the spotlight, undo.

The point: to show something for a moment, while sharing the screen,
recording it or showing a colleague, over everything, without a tool to
pick first and without anything to clean up afterwards.

## The approved spec

**Gestures**

- **`Super` + drag** (left button): a stroke that follows the pointer.
- **`Super` + `Shift` + drag**: an arrow, straight from the press to where
  the pointer is, its head at that end, redrawn as the pointer moves.
- **`Super` + `Alt` held, without clicking**: the laser. A red dot follows
  the pointer, with a trail that fades in 450 ms. Releasing either key
  ends it. Clicks and motion still reach the windows as usual, so you can
  point and click.
- **`Esc`**, while strokes show: they all go at once. Otherwise `Esc` goes
  to the focused window as usual.
- **Moving a window from anywhere in it is now `Alt` + drag**, which takes
  the place of `Super` + drag ([windows.md](windows.md) §6 changes with
  this ticket). Its header and its grip still work.

**While drawing**

- From the press to the release, the pointer is grabbed by the
  compositor. The windows under it get no motion, button or enter event,
  as during a move.
- The pointer shows a crosshair.
- A drag starting over the bar or another shell surface draws too.
- The stroke stays on the screen where it started. It is clipped at that
  screen's edges, even if the pointer crosses onto another one.

**The look**

- Strokes and arrows: `#F2545B`, 4 px, round caps and joins, with a soft
  glow `rgba(242,84,91,0.55)` 5 px and a 1 px dark shadow, so that they
  read on any background.
- The arrow's head: two 22 px strokes at about 31° each side, shorter for
  an arrow shorter than 55 px (40 % of its length).
- The laser: a 12 px dot `rgba(255,90,96,1)` with a 14 px glow, and its
  trail of fading dots from 10 px down to 3 px.
- Drawn above every window and the shell's surfaces, below the lock
  screen. Nothing draws on a locked screen, and locking clears the
  strokes.
- The points are smoothed only as the pointer gives them; nothing is
  invented.

**Fading**

- The strokes stay while you draw. Then all of them go together, 3 s
  after the last release, fading over 600 ms (ease-in). A drawing made of
  several strokes goes as one.
- Starting a new stroke during the 3 s or the fade brings everything back
  to full opacity and starts the 3 s again.
- With reduced motion, they go without a fade at 3 s.

**Seen by others**

- The strokes, the arrow and the laser are in the compositor's scene. So
  they show in screen shares (5.11), recordings (5.10) and screenshots
  (5.8) of that screen, as on the screen itself.
- The bar's in-use sign stays out of them, as before.

**Cost**

- Only the damaged areas are redrawn: each new segment's bounds, then the
  whole drawing's bounds while it fades.
- Nothing is drawn and nothing wakes once the strokes are gone and the
  laser is off.
- A stroke keeps at most 4096 points. Past that, it stops growing until
  the release.

**Measured headless**

- `Super` + drag draws red pixels along the path, and the window under it
  gets no event.
- `Super` + `Shift` + drag draws a straight arrow with its head at the
  end.
- The strokes are still there 2.9 s after the release, and gone by 3.6 s
  (virtual clock).
- A second stroke before 3 s keeps the first.
- `Esc` clears them.
- `Super` + `Alt` shows the laser's dot under the pointer, and its trail
  is gone 450 ms after it stops; a click meanwhile reaches the window.
- The strokes are in a screenshot and in a share's frames.
- `Alt` + drag moves a window; `Super` + drag no longer does.
- At rest, after the fade: no frame and no wakeup.

## As built

### Ink, 2026-11-04 (issue #564)

**Where it lives.** `crates/hocus-compositor/src/ink.rs`: the strokes, the
arrows and the laser are the compositor's own, an element of each
screen's scene (`render::OutputElements::Ink`), not a shell surface. The
press is a pointer grab of its own (`grabs::InkGrab`): the pointer's
focus is cleared at the press, as a move's is, so the window under it
gets a leave and nothing else (no motion, relative motion, button or
enter) until the release gives it back. The cursor is `crosshair` from
the press to the release: the theme's, else a thin cross the compositor
draws (`cursor/drawn.rs`).

**Which press draws.** The left button with `Super`, without `Ctrl` or
`Alt`; with `Shift`, an arrow. This is checked before anything else under
the pointer takes the press (a menu, the bar, a layer surface, a window's
frame or grip), so a drag over the bar draws too. Only the capture (5.8),
the share's chooser (5.11) and Alt+Tab's panel, which keep the pointer
while they are open, come first. `Super` + right-drag still resizes a
window from its nearest corner, and `Alt` + drag (without `Super`) moves
one.

**The points.** A stroke takes the pointer's points as they come, each one
more than 2 px from the last (the mockup's step), up to 4096; they are
joined by straight segments with round joins: nothing is smoothed or
invented. An arrow keeps its press and the pointer's last point; its head
is the mockup's: 22 px back from the tip and 0.6 of that across (31°),
40 % of the arrow's length when it is shorter than 55 px. A press released
without motion draws nothing; it still brings the drawing back to full
opacity and starts the 3 s again, as in the mockup. The points are the
screen's where the press was; a stroke carried onto another screen is
clipped at its own screen's edge.

**How it is drawn.** On the CPU, into tiles of 120 logical pixels (a whole
number of pixels at every scale the fractional-scale protocol gives)
made as the ink reaches them and dropped once no pixel of theirs is
inked. A pixel's colour is a function of its distance to the nearest
segment, so that strokes crossing and joining read as one line, never
darker where they overlap: the 4 px line, anti-aliased; under it the glow,
a band blurred by 2.5 px (the deviation of CSS's 5 px blur), at 0.55; and
under both the shadow, the same shape 1 px lower blurred by 0.5 px, black
at 0.6 (the mockup's `drop-shadow(0 0 5px rgba(242,84,91,0.55))
drop-shadow(0 1px 1px rgba(0,0,0,0.6))`). A new segment repaints only the
pixels its glow and shadow reach (about 11 px around it), from the
segments near them; an arrow redrawn repaints where it was and where it
is. The painting happens when a frame is built, once for every frame
however many motions came; the screen, a share and a recording of it
then read the same tiles.

**The laser.** Shown while `Super` and `Alt` are held (without `Ctrl`),
no press drawing. Its dot, 12 px with its 14 px glow (a deviation of
7 px), is drawn at the pointer, over the windows, the cursor over it. A
dot is added to its trail each 2 px of motion (at most 96 kept): it shows
at 10 px and 75 %, shrinking to 3 px and fading out in 450 ms, with an
8 px glow. The two dots are images drawn once per scale and placed,
scaled and faded at each frame. The trail is gone 450 ms after the
pointer stops; the dot stays under the pointer as long as the keys are
held. A key other than a modifier pressed with them is a shortcut's: the
laser goes until they are let go, so that a shortcut with `Super` and
`Alt` leaves no dot behind.

**Fading.** Held: everything at full opacity while a press draws, and for
3 s after the last release; then the drawing fades as one in 600 ms,
CSS's ease-in, by its tiles' opacity; with reduced motion it goes at 3 s.
`Esc` (with any modifier still held from drawing) clears everything at
once while strokes show, and is kept from the focused window; with none,
it goes to the window. A screen unplugged takes its strokes with it.

**Seen by others.** The ink is drawn over the windows, the bar, the
launcher and every shell surface, under what the compositor shows to
share, record or capture (the share's chooser and its ring, the recorded
area's frame, the capture's darkening and its toolbar) and under the
cursor. It is part of the elements a capture, a share's frame and a
recording's frame of that screen are drawn from, so it is in them as on
the screen; a window shared alone is drawn without it.

**Cost.** The ink is one element per screen, whatever it shows: where an
element is among the others is part of what smithay's damage tracker
compares, so a dot or a tile coming and going as an element of its own
would move every element under it, and the whole screen would be drawn
again at each frame. Its damage is what changed: the pixels painted
again, and each tile or dot that came, went, moved or faded, where it was
and where it is. A tile is known by a serial never given twice: one made
again where another was, after `Esc` or the fade, is a new piece whose
whole place is damaged, so nothing of the old one stays on the screen. So a new segment redraws its bounds, the trail redraws
the trail, and the fade redraws the drawing's tiles. The screen is drawn
whole once when the ink first shows on it and once when it has gone (the
element comes and goes). In between, no frame is asked for during the
3 s hold: one wakeup at its end starts the fade, and none comes once
everything is gone; the laser held still asks for nothing once its trail
has faded.

**Tests.** `crates/hocus-compositor/tests/ink.rs`, one per line of
"Measured headless", pixman, the motions' clock held for the 3 s, the
fade and the trail:

- `super_drag_draws_red_along_the_path_and_the_window_under_gets_no_event`
  (a client that reports its pointer events: a leave at the press, no
  motion or button until the release; the crosshair while drawing);
- `super_shift_drag_draws_a_straight_arrow_its_head_at_the_end`;
- `the_strokes_are_still_there_at_2_9_s_and_gone_by_3_6_s` (and fading at
  3.3 s);
- `a_second_stroke_before_3_s_keeps_the_first`;
- `escape_clears_the_strokes_and_otherwise_goes_to_the_window`;
- `super_alt_shows_the_laser_its_trail_gone_in_450_ms_and_a_click_reaches_the_window`
  (and a frame of the trail redraws the trail only; no frame with the
  laser held still);
- `the_strokes_are_in_a_screenshot` (`Shift+Print`'s PNG), and in
  `tests/sharing.rs`, `the_strokes_are_in_the_shared_screens_frames`
  (PipeWire's frames of a screen shared);
- `alt_drag_moves_a_window_and_super_drag_no_longer_does`;
- `at_rest_after_the_fade_no_frame_and_no_wakeup` (on the real clock);
- `a_segment_damages_its_bounds_and_the_fade_the_drawings` (the damage of
  one more segment, and of a frame of the fade);
- `a_stroke_cleared_by_escape_leaves_no_ink_when_another_is_drawn` and
  `a_stroke_gone_by_its_fade_leaves_no_ink_when_another_is_drawn`: a
  stroke drawn in the tiles of one gone leaves nothing of the first, on
  the screen, on the next frame and in a capture.

The tests that moved windows with `Super` + drag move them with `Alt` +
drag. The pull request's screenshots: `screenshots_for_the_pull_request`
(ignored) in `ink.rs`, `screenshot_of_a_shared_stroke_for_the_pull_request`
in `sharing.rs`, with `HOCUS_SHOTS=dir`.

**Choices the spec did not settle** (to confirm on the machine):

- **Nested** (in a window of another session), `Super` gestures take
  `Alt` as the shortcuts do (windows.md §13): `Alt+Super` + drag draws,
  `Alt+Super+Shift` + drag draws an arrow. `Alt+Super` held shows the
  laser there too, but a press then draws rather than clicking through.
  `Alt` + drag moves a window, nested or not.
- **`Super` + right-drag still resizes**: the spec moved only the move to
  `Alt`.
- **The laser's dot stays** under the pointer while the keys are held,
  even with the pointer still (in the mockup it faded with the trail once
  the pointer stopped). The cursor stays shown over it.
- **A shortcut with `Super` and `Alt`** hides the laser until the keys are
  let go.
- **No lock screen yet**: the session uses the display manager's lock
  until slice 5. "Below the lock screen, nothing on a locked screen,
  locking clears" waits for it.
- **A window shared alone** does not show the ink drawn over it: the spec
  speaks of shares of the screen.
