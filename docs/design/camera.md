# The camera (#619)

What the desktop does while an application uses the camera. The issue
plans five lots (its revised spec, 2026-11-17); this page says what is
built. Mockup: the design canvas « #619 — effets webcam », its boards
"Lumière d'écran" (lot 1), "Caméra et micro dans la barre", "La lumière
qui s'ajuste" and "Dans l'appel" (lot 2) (sources in the maintainer's
`hocus-panda-tools/mockups/s23-webcam`).

## The screen light (lot 1, as built)

While an application uses the camera, the edges of the screen of the call
glow softly and light the user's face. It works with every application:
the light is the screen's, the video is not touched.

- **When**: as long as the in-use sign of the bar (bar.md 5.12) counts a
  camera's user, without its 3 s linger, after 3 s of continuous use: a
  `Stream/Input/Video` node linked from a `Video/Source` in PipeWire's
  graph, or a process holding a camera's device open. A use that ends
  before, as applications open a camera for a moment to list or probe it,
  lights nothing (one back while its tile lingers starts its 3 s again);
  the sign shows it all the same. The shell tells the compositor who they
  are as that changes (in the journal: "the compositor is told who uses
  the camera", and the compositor's "the screen light", with their
  names), and again as it connects (`ToCompositor::Camera`: each
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
  (`"auto"` since lot 2, below; `"soft"`, `"medium"`, `"strong"`). The
  camera's window in the bar holds its switch.

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

## The Hocuspanda camera (lot 2, as built)

A camera named "Hocuspanda" that calls choose instead of the laptop's:
the same picture, with the user's effects. It reads the real camera only
while something reads it, so the camera's light is never on for nothing.

### The program, `hocus-camera`

- **A program of its own** (crate `hocus-camera`), started by the
  compositor from beside its binary on a virtual terminal (nested or
  headless with `--camera`, as its source would appear on the PipeWire
  around; tests give `Config::camera_program`), started again when it
  ends (2 s, then twice as long, up to a minute). A process, not a
  thread of the compositor: its frames' work (decoding, the GPU, the
  CPU's path) stays away from the loop, and a fault in it ends it alone.
  It ends with its input: when the compositor goes.
- **Its lines**: the compositor writes how its frames look on its
  standard input, one JSON object a line (`hocus_link::CameraLook`:
  Lumière, the banner's name and project line), at once and as that
  changes (looked at once a turn of the loop); it says on its standard
  output what happens to it (`hocus_link::FromCamera`: its node, read or
  not, the camera it reads, the light on the face, where it makes its
  frames). Its log goes to the compositor's.
- **Its source**: a PipeWire output stream, `media.class = Video/Source`,
  `media.role = Camera` (what the camera portal's restricted connections
  show), `node.name = hocuspanda-camera`, `node.description =
  Hocuspanda`; a driver, 1280 × 720 at 30 frames a second, in B, G, R, x,
  or I420 or YUY2 for readers that take only YUV (the browsers' WebRTC).
- **Read only while read**: its stream streams while a reader is linked
  to it. Then, and only then, it reads the real camera: a stream of its
  own (`hocuspanda-camera-input`, not autoconnected), which it links
  itself to the camera's node (a session manager must never link it to
  its own source). The camera: PipeWire's video sources but its own and
  the screen casts (`media.role = Screen`), an infrared one last. It
  takes MJPEG first (webcams send 720p at 30 a second in it), else YUY2,
  NV12, I420 or RGB, any size, scaled to cover 1280 × 720 (its middle
  kept). The last reader gone, the camera's stream and the GPU's work
  go. Without a camera, a dark frame twice a second (with the banner).
- **On the GPU** (wgpu, Vulkan or GL, an integrated GPU first): each
  frame written into a texture, Lumière, its denoise and the banner in
  one fragment shader's pass, read back; a CPU's adapter (lavapipe) is
  no faster than the CPU's own path, which is taken then, and without an
  adapter, and for a frame the GPU fails (`--device auto|cpu|any`).

### Lumière and Bandeau (the Image tab)

- **Lumière** (`[camera] lighting`, on by default: the mockup's state),
  from the frame's statistics (every eighth pixel each way), no model:
  grey-world white balance at 70 %, each gain within ¾ and 4⁄3; a warmth
  of 2 % (red up, blue down), up to 6 % when the picture was cold; the
  exposure as a curve `e·x / (1 + (e − 1)·x)` that keeps black and white,
  its `e` (0.6 to 3) taking the balanced mean to 0.46; the correction
  following the statistics a seventh of the way each frame (no flicker);
  a denoise over time, a still pixel 40 % the new frame, a moving one
  all of it (`crates/hocus-camera/src/look.rs`, the shader in `gpu.rs`,
  the same arithmetic).
- **Bandeau** (`[camera] banner`, off): the mockup's banner scaled to the
  frame (its preview's 32 px of 176): a band across the bottom,
  `rgba(10,14,24,0.85)` to `0.35`, the name in Manrope bold `#EEF1F6`,
  under it "projet NOM" in Manrope `#A9B2C2` when the focused screen's
  current desktop is a project (docs/design/projects.md) and
  `banner-project` is on; the name `banner-name`, else the account's
  (its GECOS field's first part, else its login). In the frames, so
  every reader sees it, unmirrored.

### The light that adjusts

While the Hocuspanda camera is read, it measures the light on the face:
the mean brightness of the picture's centre (35 % to 65 % of its width,
20 % to 60 % of its height; no face is looked for), averaged over about a
second, said every half second when it moved. With
`screen-light-strength = "auto"` (the default), the screen light's glow
goes between soft (28 px, 0.55) and strong (70 px, 0.92) in 8 steps: a
step stronger while the face is under 0.36, softer while over 0.50, a
step at most every 600 ms (soft to strong in about 5 s); between the two
it holds, the light it adds being in what is measured. Each step draws
the strips again (`screen_light.rs`), the edges alone. Nothing read,
nothing measured: soft, as lot 1. Fixed strengths stay.

### The window « CAMÉRA ET MICRO »

A click on the camera's tile of the in-use sign (bar.md 5.12) opens it,
as the bar's popovers (`crates/hocus-shell/src/popover/camera.rs`), 340
wide, as the mockup draws it: the label and who reads the camera and the
microphone ("Utilisée par Firefox"); the preview, 312 × 176, what the others
see, read from the Hocuspanda source while the window shows (so the
camera is on while it shows), 30 frames a second; the tabs Image · Fond ·
Visage · Voix · Réactions, a violet dot on a tab with an effect on; the
tab's grid of tiles from one list of effects
(`crates/hocus-shell/src/camera/effects.rs`: id, tab, on-or-off or one of
the tab's, thumbnail, words, made or not), the chosen ringed blue; the
dashed "+ add-ons" tile, inert (#442); the hint line; the screen light's
switch ("auto, sur ton visage", or its fixed strength) and the link to
the help (the "Dans l'appel" board: choosing Hocuspanda in the call's
camera settings, where it works). A tile written into `[camera]` at once,
shown before the compositor says it back. The violet dot on the camera's
tile while an effect is on and the Hocuspanda camera is read.

**Choices made here** (to confirm on the machine):

- All five tabs show, as the mockup; the tiles this lot does not make
  (Cadrage, Fond's, Visage's, Voix's, Réactions') are dimmed, take no
  click, say "Bientôt" to screen readers, and a tab with none made says
  "Bientôt dans Hocuspanda" under its grid; Fond's "Ta pièce", Visage's
  "Aucun" and Voix's "Telle quelle", what is true today, are ringed.
- The voice's level meter on the preview is left for lot 5 (no
  Hocuspanda microphone yet); the microphone's tile opens nothing new.
- Hovering the camera's tile no longer opens the in-use popover (its
  click is the window's); the rest of the sign does.
- The switch is the kit's (40 × 22, the accent), not the mockup's amber
  38 × 22.
- Lumière is on by default (the mockup's state); `auto` is the strength's
  default, soft while nothing measures, so lot 1 alone looks as it did.
- The help's table says "pas encore" for Slack, Teams and Discord (no
  v4l2loopback in this lot) and drops the screen light's column for a
  line ("La lumière d'écran marche avec toutes les applis."), the window
  being 340 wide.
- The banner shows the project's line only with `banner-project` (on by
  default), a Settings row.

### The camera portal (finding, not built)

Browsers reach PipeWire's cameras through xdg-desktop-portal's Camera
portal (`org.freedesktop.portal.Camera`: `IsCameraPresent`,
`AccessCamera`, `OpenPipeWireRemote`), which xdg-desktop-portal itself
serves, and only when an `org.freedesktop.impl.portal.Access` backend
is there ("The camera portal requires an access impl"): it asks it before
it gives a camera, to sandboxed applications and unsandboxed ones alike
("Allow Apps to Use the Camera?"). Its restricted PipeWire connection
shows the nodes whose `media.role` is `Camera`: the Hocuspanda source is
one. The desktop serves no `Access` today (`hocuspanda.portal`:
`ScreenCast` and `Settings`); `hocuspanda-portals.conf` sends the rest to
GTK's backend, so with `xdg-desktop-portal-gtk` installed the camera
portal works and GTK's own dialog asks; without it there is no camera
portal at all. Firefox and Chromium read cameras through it only behind
a setting (`media.webrtc.camera.allow-pipewire`, Chromium's "PipeWire
Camera support"); otherwise they open `/dev/video*` and see only the
real camera. The mockup's « Firefox veut utiliser la caméra » card means
serving `org.freedesktop.impl.portal.Access` (`AccessDialog`) on the
compositor's bus with a card of the shell's, and keeping the answers (the
permission store) for Settings › Caméra to take back: left to the
maintainer. v4l2loopback (for applications that read only V4L2) is not
in this lot.

**Measured headless** (`crates/hocus-compositor/tests/camera.rs`, a
PipeWire of the test's own, a fake camera's node of the test's, frames on
the CPU): a reader of the source gets the camera's frames, and the fake
camera streams only while the source is read, again with the next
reader; Lumière takes a dark, blue picture's mean to its target, its
cast halved; the screen light's `auto` climbs step by step to strong on
a dark face, falls to soft in daylight, holds between, and is soft again
once unread; with the real shell, the camera's tile opens the window,
its Bandeau tile writes `banner = true` and the call's frames get the
band, its Lumière tile writes `lighting = false`. On the GPU (lavapipe
alone shown to Vulkan): `hocus-camera`'s `gpu::tests` make what the CPU
makes within two levels. The Settings' rows:
`settings_appearance::the_camera_page_writes_the_screen_light_s_keys`.
The look: `camera::screenshots_of_the_window` (ignored, `HOCUS_SHOTS`).
