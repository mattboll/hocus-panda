# The camera (#619)

What the desktop does while an application uses the camera. The issue
plans five lots (its revised spec, 2026-11-17); this page says what is
built. Mockup: the design canvas « #619 — effets webcam », its board
"Lumière d'écran" (sources in the maintainer's
`hocus-panda-tools/mockups/s23-webcam`).

## The screen light (lot 1, as built)

While an application uses the camera, the edges of the screen of the call
glow softly and light the user's face. It works with every application:
the light is the screen's, the video is not touched.

- **When**: as long as the in-use sign of the bar (bar.md 5.12) counts a
  camera's user, without its 3 s linger: a `Stream/Input/Video` node
  linked from a `Video/Source` in PipeWire's graph, or a process holding a
  camera's device open. The shell tells the compositor who they are as
  that changes, and again as it connects (`ToCompositor::Camera`: each
  application's name and the names its windows' ids may go by); the
  compositor forgets them when the shell goes.
- **Where**: on the screen of the window of an application using the
  camera, matched as the in-use popover matches it (a window whose
  application id, lower case, is one of the application's ids, whole or
  by its last part), the focused window first, then from the front,
  minimised ones left out; with none, the focused screen (the focused
  window's, else the pointer's). It follows the window to another screen,
  fading out on one as it fades in on the other.
- **The look**: the mockup's two inset `box-shadow`s in the warmth's
  colour, at the strength's alpha: a frame a quarter of the glow's width
  wide, and the glow blurred over its width, spread over half of it.
  Warmth: cool `rgb(230,240,255)`, neutral `rgb(255,236,214)`, warm
  `rgb(255,214,170)`; strength: soft, medium, strong, the glow 28, 46 or
  70 px wide at scale 1, at 0.55, 0.75 or 0.92.
- **Over everything but the pointer**, the lock screen too; on the screen
  only: never in a capture, a share's frames or a recording (drawn for
  `Purpose::Screen` alone, as the share's ring is).
- **Its motion**: it fades in and out over 600 ms (CSS's `ease`, the
  mockup's transition), at once with reduced motion; turned back midway,
  it goes from where it is in the part of the fade left.
- **Its cost** (`crates/hocus-compositor/src/screen_light.rs`): four
  strips along the edges, computed on the CPU once for the screen's size,
  scale, warmth and strength into textures of their own (the inset
  shadow's blur is a Gaussian's, separable: the opacity is a product of
  two error functions); drawn as textures, so on the CPU (pixman) as on
  the GPU. While it fades, only its screen draws again, and only the
  strips' alpha changes: the damage is the edges alone. Steady, it asks
  for no frame and damages nothing; gone, its strips are let go.
- **Its settings** (`[camera]`, followed live; Settings › Caméra):
  `screen-light` (on by default, the maintainer's choice of 2026-11-17),
  `screen-light-warmth` (`"neutral"`), `screen-light-strength`
  (`"soft"`). The camera's window in the bar (lot 2) will hold the same
  switch and warmth.

**Choices made here** (to confirm on the machine): the light shows over
the lock screen too (the spec's "over everything but the pointer"); with
the magnifier on, the magnified picture shows it, magnified; a
minimised call window does not hold the light (the focused screen takes
it); the desktop's own screen cast (a `Video/Source` whose `media.role`
is `Screen`) is no camera, so that sharing a screen lights nothing (the
in-use sign counted what read it as a camera's user until then); the
strips reach in twice the glow's width (past it the light is
under half a level), at most half the screen; the `[camera]` section lets
keys it does not know through, for the Hocuspanda camera's own (lots
2 to 5).

**Measured headless** (`crates/hocus-compositor/tests/screen_light.rs`):
the real shell against a PipeWire of the test's own, a camera's node and
an application's stream linked to it as a session manager would: the
light on the second screen, where the application's window is, its edge
lit and the other screen's not, and gone once the stream is; a `Print`
capture of the lit screen without it; `screen-light = false`, none, and
turned back on live, the light comes; with reduced motion at once. The
share's frames without it: `sharing::the_screen_light_is_never_in_a_share`.
The rows writing their keys, and the compositor following them:
`settings_appearance::the_camera_page_writes_the_screen_light_s_keys`. Unit tests: the
glow's fall-off, the strips edge to edge, a fade turned back
(`screen_light::tests`), the keys' words (`config::camera::tests`).
