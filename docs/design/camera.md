# The camera (#619)

What the desktop does while an application uses the camera. The issue
plans five lots (its revised spec, 2026-11-17); this page says what is
built. Mockup: the design canvas « #619 — effets webcam », its boards
"Lumière d'écran" (lot 1), "Caméra et micro dans la barre", "La lumière
qui s'ajuste" and "Dans l'appel" (lot 2), the window's Fond tab, « Je
reviens » and "Présentateur" (lot 3), the window's Visage and Réactions
tabs and its preview's reactions (lot 4) (sources in the maintainer's
`hocus-panda-tools/mockups/s23-webcam`).

## The screen light (lot 1, as built)

While an application uses the camera, the screen of the call lights the
user's face. It works with every application: the light is the screen's,
the video is not touched.

**Wider, and a second shape (2026-11-18).** Tried on the maintainer's
laptop (its backlight at its maximum), lot 1's frame (28 to 70 px at
0.55 to 0.92) made no visible difference on the face: a laptop's screen
is a weak light, it takes far more white surface. The maintainer chose
both of these, as a choice (`screen-light-shape`): a wide frame (the
default), and the whole screen around the call's window.

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
- **The look, a wide frame** (`screen-light-shape = "frame"`, the
  default): along the edges of the screen, in the warmth's colour, nearly
  opaque at the edge and glowing inward: an inset `box-shadow` spread
  over the frame's width and blurred over two thirds of it (a Gaussian
  of a third of the width), so the light is the strength's alpha at the
  edge, over 90 % of it half way in, half of it at the width, and gone at
  twice the width. The width is a part of the screen's shorter side:
  soft a sixteenth, medium an eleventh, strong an eighth (on a 1080 p
  screen, 68, 98 and 135 px, the light reaching about 200 px in at
  strong); the alpha 0.85, 0.92 and 0.97. Warmth: cool
  `rgb(230,240,255)`, neutral `rgb(255,236,214)`, warm `rgb(255,214,170)`.
- **The look, all around the call** (`"around-call"`): everything on the
  call's screen but the call's window is the light, the warmth's colour
  at the strength's alpha (0.97 at strong): the wallpaper and the other
  windows. It is drawn right behind the call's window, which stays as it
  is with its shadow, over it (a window brought in front of the call
  stays in front of the light too). Around the window, the light rises
  from a little over half of itself (0.55) at its edge to full 48 px out
  (a smoothstep, round at the corners), so that it does not look cut out:
  the window's own shadow and a soft dimming around it, not a dark ring
  of the wallpaper (from nothing, what is behind showed as one, another
  window's edge with it); under the window, the light at that half, which
  its round corners show. It follows the window as
  it moves or is resized. When the call has no window on that screen (the
  light then on the focused screen), while locked and in the overview,
  it is the wide frame.
- **The bar stays as it is**: neither shape covers the bar's column (the
  bar, the gap beside it, to the screen's edge, as its form settles); the
  frame glows along that column's inner edge as along the screen's.
- **Over the windows, under the shell's surfaces**: the bar's popovers,
  the notifications, the launcher and the compositor's own overlays stay
  over the light (lot 1's light was over everything but the pointer; a
  light this wide would hide them); a fullscreen window on top has the
  frame over it; locked, the frame shows over the lock screen. On the
  screen only: never in a capture, a share's frames or a recording (drawn
  for `Purpose::Screen` alone, as the share's ring is).
- **Its motion**: it fades in and out over 600 ms (CSS's `ease`, the
  mockup's transition), at once with reduced motion; turned back midway,
  it goes from where it is in the part of the fade left.
- **Its cost** (`crates/hocus-compositor/src/screen_light.rs`): the
  frame is four strips along the edges of the part lit, computed on the
  CPU once for its size, the frame's width, the warmth and the alpha into
  textures of their own (the inset shadow's blur is a Gaussian's,
  separable: the opacity is a product of two error functions); drawn as
  textures, so on the CPU (pixman) as on the GPU. Around the call: flat
  fills (solid colour elements) on a fixed grid of 256-pixel cells, each
  cell the window does not touch kept as the same element, and the soft
  edge's nine pieces cut from one small picture (its corners as they
  are, its sides stretched along the window's), made once for the
  scale, warmth and alpha: as the window moves or is resized, only the
  cells along its edge and the soft edge change, and only they are
  damaged. While it fades, only its screen draws again, and only the
  pieces' alpha changes. Steady, it asks for no frame and damages
  nothing; gone, its pictures are let go.
- **Its settings** (`[camera]`, followed live; Settings › Caméra):
  `screen-light` (on by default, the maintainer's choice of 2026-11-17),
  `screen-light-shape` (`"frame"`, « Cadre »; `"around-call"`, « Tout
  autour de l’appel »; the row « Forme »), `screen-light-warmth`
  (`"neutral"`), `screen-light-strength`
  (`"auto"` since lot 2, below; `"soft"`, `"medium"`, `"strong"`). The
  camera's window in the bar holds its switch.

**Choices made here** (to confirm on the machine): the light shows over
the lock screen too (the spec's "over everything but the pointer"); with
the magnifier on, the magnified picture shows it, magnified; a
minimised call window does not hold the light (the focused screen takes
it); the desktop's own screen cast (a `Video/Source` whose `media.role`
is `Screen`) is no camera, so that sharing a screen lights nothing (the
in-use sign counted what read it as a camera's user until then); the
strips reach in twice the frame's width (past it the light is under
half a level), at most half the part lit; around the call, the light
rises over 48 px from half of itself at the window's edge, and a window
in front of the call stays in front of the light; neither shape covers
the bar's column, and both go under the shell's surfaces (the light was
over everything but the pointer until 2026-11-18); the `[camera]`
section lets keys it does not know through, for the Hocuspanda camera's
own (lots 2 to 5).

**Measured headless** (`crates/hocus-compositor/tests/screen_light.rs`):
the real shell against a PipeWire of the test's own, a camera's node and
an application's stream linked to it as a session manager would: the
light on the second screen, where the application's window is, its edge
lit and the other screen's not, and gone once the stream is; a `Print`
capture of the lit screen without it; `screen-light = false`, none, and
turned back on live, the light comes; with reduced motion at once;
around the call, at strong, another window and the wallpaper of the
call's screen are the light while the call's window and the bar's column
are untouched, the light rises from the call's edge, a `Print` capture
has none of it, and it follows the window moved over the other one. The
share's frames without it: `sharing::the_screen_light_is_never_in_a_share`.
The rows writing their keys, and the compositor following them:
`settings_appearance::the_camera_page_writes_the_screen_light_s_keys`. Unit tests: the
frame's width and fall-off, the strips edge to edge with the bar's
column left out, the light around the call tiling the screen but the
window (a cell away from it the same as it moves), the soft edge, a fade
turned back (`screen_light::tests`), the keys' words
(`config::camera::tests`). The maintainer's screenshots:
`HOCUS_SHOTS=DIR cargo nextest run -p hocus-compositor --run-ignored only
screen_light::screenshots_of_the_wide_light`.

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
  to it; the v4l2loopback device (below) has readers of its own. While
  either is read, and only then, it reads the real camera: a stream of its
  own (`hocuspanda-camera-input`, not autoconnected), which it links
  itself to the camera's node (a session manager must never link it to
  its own source). The camera: PipeWire's video sources but its own and
  the screen casts (`media.role = Screen`), an infrared one last. It
  takes MJPEG first (webcams send 720p at 30 a second in it), else YUY2,
  NV12, I420 or RGB, any size, scaled to cover 1280 × 720 (its middle
  kept). The last reader gone, the camera's stream and the GPU's work
  go. Without a camera, a dark frame twice a second (with the banner).
  PipeWire's node of the v4l2loopback device (WirePlumber makes one once
  it is written) is never the camera it reads: its label
  (`api.v4l2.cap.card`) or its description is "Hocuspanda".
- **On the GPU** (wgpu, Vulkan or GL, an integrated GPU first): each
  frame written into a texture, Lumière, its denoise and the banner in
  one fragment shader's pass, read back; a CPU's adapter (lavapipe) is
  no faster than the CPU's own path, which is taken then, and without an
  adapter, and for a frame the GPU fails (`--device auto|cpu|any`).

### On V4L2 too: the v4l2loopback device

Firefox and Chrome read cameras from `/dev/video*` unless a setting says
PipeWire (`media.webrtc.camera.allow-pipewire`, Chrome's "PipeWire Camera
support"); Slack, Teams and Discord (Electron) only from `/dev/video*`.
So `hocus-camera` writes the same frames to a v4l2loopback device too
(`crates/hocus-camera/src/loopback.rs`), and every application lists
"Hocuspanda" with no setting.

- **The package** recommends `v4l2loopback-dkms` and ships the module's
  options, `/etc/modprobe.d/hocuspanda-camera.conf` (`devices=1
  video_nr=42 card_label="Hocuspanda" exclusive_caps=1`), and its loading
  at boot, `/etc/modules-load.d/hocuspanda-camera.conf`; it never loads or
  builds the module itself. After installing the module: a restart, or
  `sudo modprobe v4l2loopback`.
- **Found by its label**, never by its number: the `name` of each entry of
  `/sys/class/video4linux` (`HOCUS_VIDEO4LINUX` stands for it, which
  tests point at a tree of their own: `Config::camera_video4linux`), its
  node by its `uevent`'s `DEVNAME` in `/dev` (`HOCUS_VIDEO_DEVICES`).
  Only that node is opened, and kept only if `VIDIOC_QUERYCAP` says its
  driver is `v4l2 loopback` and its card "Hocuspanda": no other
  `/dev/video*` is ever opened. Without it, PipeWire only, said once in
  the log at info; it is looked for again every ten seconds (sysfs
  alone), so a `modprobe` needs no new session.
- **Its format**: YUYV 4:2:2 (BT.601, limited range), 1280 × 720, 30 a
  second: what every UVC webcam sends, so what every reader takes
  (libwebrtc's V4L2 capture in Firefox and Chrome, Electron's), and the
  source's YUY2 already; NV12 is not taken by every one. Set with
  `VIDIOC_S_FMT` and `VIDIOC_S_PARM` on the output, then each frame
  written with `write()`, the V4L2 output interface.
- **Written on a thread of its own**, converted there from the frame the
  PipeWire source gets; one frame waits at most, the next dropped while
  it does. With nothing new for half a second, the last frame again:
  while nothing reads, the still (the dark frame with the banner, made
  again when the banner changes), so that a reader starting gets a
  picture at once, and with `exclusive_caps` the device shows as a camera
  only once written.
- **Its readers**: v4l2loopback's client-usage event
  (`V4L2_EVENT_PRI_CLIENT_USAGE`, from 0.12.5: the readers streaming from
  the device), dequeued after each frame written, so within half a second
  while idle; on an older v4l2loopback, the processes holding the device
  open (`/proc/*/fd`, at most once a second), but `hocus-camera` itself
  and PipeWire's (whose node of the device reads it for a stream the
  graph shows). The device read is the camera read: the real camera
  comes on, the screen light's `auto` measures, the violet dot shows; the
  last reader gone, it goes, as with PipeWire.
- **Who uses the camera**: the shell's in-use sign (bar.md 5.12) leaves
  `hocus-camera`'s holding of the device out, as PipeWire's; the
  application reading it holds it too, and is the one shown ("Utilisée
  par Firefox"), whose window the screen light lights. Its reading of the
  real camera is a PipeWire stream of its own, left out already.

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
goes between soft (a sixteenth of the shorter side, 0.85) and strong (an
eighth, 0.97) in 8 steps: a
step stronger while the face is under 0.36, softer while over 0.50, a
step at most every 600 ms (soft to strong in about 5 s); between the two
it holds, the light it adds being in what is measured. Each step draws
the strips again (`screen_light.rs`), the edges alone (around the call,
the fills' colour and the soft edge). Nothing read,
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
- The voice's level meter on the preview, and the microphone's tile
  opening the window on Voix: lot 5 ([voice.md](voice.md)).
- Hovering the camera's tile no longer opens the in-use popover (its
  click is the window's); the rest of the sign does.
- The switch is the kit's (40 × 22, the accent), not the mockup's amber
  38 × 22.
- Lumière is on by default (the mockup's state); `auto` is the strength's
  default, soft while nothing measures, so lot 1 alone looks as it did.
- The help's table « OÙ ÇA MARCHE » follows the v4l2loopback device,
  looked for in sysfs as the page opens: with it, "oui" for Firefox,
  Chrome and Slack, Teams, Discord; without it, "réglage à activer" for
  Firefox, "réglage expérimental" for Chrome, "pas sans ce paquet" for
  the applications, then two lines: install `v4l2loopback-dkms` and
  restart (or `sudo modprobe v4l2loopback`), and Firefox's
  `about:config` setting. It drops the screen light's column for a line
  ("La lumière d'écran marche avec toutes les applis."), the window
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
maintainer. For the applications that read only V4L2, the v4l2loopback
device above.

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
The v4l2loopback device, with no module (`hocus-camera`'s
`loopback::tests`, `convert::tests`): found by its label in a sysfs tree
of the test's own, never another; a B, G, R, x frame written as YUYV, its
values checked; the client-usage event's count, and `/proc`'s holders
but `hocus-camera` and PipeWire's; the ioctls' numbers on x86-64. The
shell leaving `hocus-camera` out (`in_use::cameras::tests`) and the
help's words with the device and without (`popover::camera::tests`).
Not measured headless: a real v4l2loopback (the module is the
machine's): on the maintainer's.

## What is behind you (lot 3, as built)

The person is found in the picture by a model; what is behind them can
be blurred or be one of the desktop's living wallpapers; when they are
away, the call sees a card instead of an empty chair; while the desktop
shares a screen, they can stand on it like a weather presenter.

### The person (`crates/hocus-camera/src/segment.rs`)

- **The model**: MediaPipe's selfie segmentation, landscape (256 ×
  144), Apache-2.0, run with tract (pure Rust) on the CPU. tract reads no
  TensorFlow Lite holding MediaPipe's own operator
  (`Convolution2DTransposeBias`), so its graph is written again as ONNX
  by `scripts/camera/selfie-to-onnx.py` from the file MediaPipe publishes
  (its URL and SHA-256 in the script), operator by operator, its weights
  unchanged: `crates/hocus-camera/models/`, with its licence
  (`LICENSE-MEDIAPIPE`). 432 KB, in the program itself (no download; the
  package's copyright names it).
- **When**: only while the camera is read. Every frame while Fond or the
  presenter needs the person (every other one while the model takes over
  20 ms), three times a second while only « Je reviens » does, never
  otherwise. Measured: some 9 ms a frame on one core of a laptop's
  processor, the frame scaled to 256 × 144 included.
- **The mask**: each pixel follows the model a part of the way (most of
  it where it changed much, a third where it hesitates), so that an edge
  does not flicker and a move is followed. At the frame's size: its
  probabilities smoothed a little (a 3 × 3 binomial), scaled up by a
  cubic (Catmull-Rom) each way, then a soft ramp at each of the frame's
  pixels (`smoothstep` from 0.5 to 0.82): a smooth contour some three
  pixels soft, no steps of the model's pixels, and what the model is
  unsure of (a shape of the room it half takes for the person) left to
  the room. On the CPU, the GPU's path and the CPU's alike (the frames'
  effects are drawn over the made frame).

### Fond (one at a time; `backdrop.rs`, `scenes.rs`)

- **Ta pièce** (`[camera] background = "room"`, the default): the room
  as it is; nothing is looked for.
- **Flou** (`"blur"`): the room blurred at a quarter of its size (a
  Gaussian of some 26 pixels of the frame: the mockup's `blur(7px)` on its
  312 px preview), the person left out of what is blurred so that no
  halo of them stays around them.
- **Aurore** and **Koï** (`"aurora"`, `"koi"`): the desktop's own living
  wallpapers, drawn by the compositor's own code: their shaders
  (`hocus_scenes::shaders`, GLSL ES as the compositor compiles them with
  GLES) adapted for wgpu at load (`hocus-camera`'s `glsl`: the uniforms
  into a block, textures and samplers apart, the stages' locations), the
  koi pond's simulation and its shapes (`hocus_scenes::koi`, `mesh`, the
  compositor's own, moved out of it); on a thread of their own, about 30
  times a second while one is wanted, on a GPU (else lavapipe, at a
  quarter of the pixels; else none, and the room blurred stands in). The
  aurora moves as it does with the music, its swell without the beat's
  ripple (no music plays in a call); the pond is a screen of the frame's
  size (seven koi, six pads), its koi swimming, its drops falling. Until
  a wallpaper's first picture comes, the room blurred.
- The banner is drawn over what is behind; Lumière corrects the room,
  not the wallpaper.

### « Je reviens » (`away.rs`; the board « Je reviens »)

- **When** (`[camera] away`, on by default): nobody in the picture (the
  person under 2 % of it) for 10 s, or the screen locked while the
  Hocuspanda camera is read (the compositor tells it, `CameraLook.locked`).
  Someone over 5 % of the picture again, and the screen unlocked, it
  goes (the gap between the two: a hand in the picture does not bring
  it back).
- **The picture**: the living wallpaper (Fond's when it is one, else the
  desktop's own pond when `[effects] koi` is on, else the aurora), its
  card over it as the board draws it, scaled from its 560 × 315 picture:
  320 wide, 16 round, `rgba(14,18,30,0.72)`, a 1 px border; the board's
  panda asleep (64 px) and its "z" rising every 2.6 s; "De retour dans
  un instant" (Manrope bold 17 px) and how long (12.5 px): "absent depuis
  moins d'une minute", "absent depuis 2 min", "absent depuis 1 h 05",
  from when the person left (or the screen locked). Without a GPU nor
  lavapipe, the board's own night (its gradient and its two glows) under
  the card.
- **The microphone**: while the screen is locked during a call (the
  Hocuspanda camera read, « Je reviens » on), the compositor tells the
  shell to hush (`CameraShown.hush`): the shell mutes the session's
  default source if it was not muted, and unmutes it as that ends if it
  muted it; a microphone muted by its user stays muted.

### Présentateur (`presenter.rs` in both; the board "Présentateur")

- **When** (`[camera] presenter`, off by default): on, while the desktop
  shares a screen or a window (5.11). The camera is read for it then
  even when no call reads the Hocuspanda source (the board: "ta vignette
  de caméra, elle, peut se couper"); its stream is then named
  `hocuspanda-presenter`, its application "Présentateur", so that the
  bar's in-use sign shows the camera on, for the presenter (its stream is
  made again when a call starts or stops reading the source).
- **The cut-out** (`hocus-camera`): the person's box in the frame, the
  person premultiplied by their alpha, the board's `drop-shadow(0 6px
  18px rgba(0,0,0,0.35))` under them (its lengths the board's for its
  262 px tall silhouette, scaled to the person); sent to the compositor
  after a line of its own on the camera's output, raw (B, G, R, A), every
  frame (a cut-out not yet taken when the next comes is dropped).
- **Where** (the compositor): drawn into the share's frames alone, never
  on the screen, under the pointer: on the bottom edge, across where
  windows stand (beside the bar: the bar stays readable), 24 px of the
  board's 860 in from the right (`presenter-side = "right"`) or the left,
  as tall as the board's 196, 262 or 340 px of its 520
  (`presenter-size`: `small`, `medium`, `large`), as wide as the cut-out
  for that height. While the pointer is under it, it stands in the other
  corner, sliding there over 300 ms (at once with reduced motion).
- **Switched on** from the share's chooser (a "Présentateur" box beside
  "Montrer le pointeur", written as the share starts) or the camera's
  window (Image's "Présentateur" tile); its corner and size in
  Settings › Caméra.

### The window and the settings

Fond's tiles (Ta pièce, Flou, Aurore, Koï, the mockup's) are made, one
of them ringed, a violet dot on the tab with any but the room; Image
holds "Présentateur" and Réactions "Je reviens" (the board's sleeping
panda), each its words under the grid. `[camera]`: `background`, `away`,
`presenter`, `presenter-side`, `presenter-size`, with their rows in
Settings › Caméra.

**Choices made here** (to confirm on the machine):

- The desktop's other wallpapers (the daylight's sky, the landscape in
  depth; the fireflies, the rain and the snow are drawn over a
  wallpaper, not one) are not in Fond: the mockup's four tiles fill its
  row; the daylight's and the depth's shaders are in `hocus-scenes`
  already, a tile each would bring them.
- « Je reviens » is a tile of Réactions, on by default (the spec says
  nothing of turning it off); the presenter a tile of Image.
- The presenter's corner and size are Settings rows only (the board's
  "Où" and "Taille" were its trial panel).
- The person's colours are the camera's; the cut-out is the frame's own
  pixels, unscaled (a "large" presenter on a large screen is scaled up).

**Measured headless**: `hocus-camera`'s unit tests: the model on the test
picture (`crates/hocus-camera/data/person.jpg`, NASA's portrait of
Jeffrey Hoffman, public domain) finds the face and the collar, not the
wall, within 16 ms of its thread's processor time a frame; Flou and a
wallpaper change the room's pixels and not the person's; the aurora and
the koi pond drawn on lavapipe, moving (`scenes::tests`); « Je reviens »'s
timing and words; the cut-out and its shadow. The compositor's tests
(`tests/camera.rs`): an empty picture for 10 s, the card (a dark picture
where the room was bright), someone back, the room again; the screen
locked during a call (the real shell, a microphone of the fakes'), the
card and the microphone muted, unlocked both gone, and a microphone muted
by hand staying muted. `sharing::the_presenter_stands_in_the_shared_stream_never_on_the_screen`:
the person's suit in the stream's lower right and not on the screen; the
pointer under it, it moves to the left.
`hocus-sound`'s `graph::tests::the_presenter_reading_the_camera_is_a_use_of_it`:
the presenter's stream on a camera is a use of it, named so.

## Your face and your hands (lot 4, as built)

The face and the hands are found by models; an accessory follows the
head, or a panda takes the person's place; the picture can follow the
face, a thumb up or a raised hand shows in the picture, and the agents'
moments can rain confetti on it. Mockups: the window's Visage and
Réactions tabs, and the panda's live mockup (`hocus-panda-tools/mockups/
s23-webcam/panda-avatar`, approved with its three looks).

### The models (`track.rs`, `face.rs`, `hands.rs`)

- **Which**: MediaPipe's face landmarker (its face detector, BlazeFace
  short range at 128 × 128; its mesh, 478 points at 256 × 256; its 52
  expressions, the blendshapes, from 146 of the points) and hand
  landmarker (its palm detector at 192 × 192; its 21 points at 224 ×
  224), Apache-2.0, run with tract on the CPU as lot 3's.
- **How they ship**: tract reads ONNX, MediaPipe publishes TensorFlow
  Lite bundles (`.task`). `scripts/camera/landmarks-to-onnx.py` writes
  each graph again as ONNX from the two bundles MediaPipe publishes (their
  URLs and SHA-256 in the script), operator by operator, the weights the
  same 16-bit floats (cast to 32 bits once, as tract loads them), every
  output checked against TensorFlow Lite's own runtime within 10⁻³ of
  their range. Two operators are written as what tract runs fastest:
  PReLU as `max(x, 0) + slope · min(x, 0)`, the slope a 1 × 1 convolution
  of a channel each (tract runs ONNX's `PRelu` as a comparison, a choice
  and a broadcast product, the mesh three times slower). 12 MB in all
  (`crates/hocus-camera/models/`, their licence beside them): in the
  program, as lot 3's, no download (it works offline, from the first
  call; the package's copyright names them).
- **The face**: the detector only while no face is followed (the frame
  letterboxed); then the mesh in a square round the face (its box 1.5
  times, turned so that the eyes are level), the next frame's square from
  this frame's points, as MediaPipe follows a face; lost when the mesh
  says no face is there. The points are kept still between frames by a
  One Euro filter each (still within a pixel or two at rest, a move
  followed at once); the head's pose read from them: the eyes' middle,
  their span (in depth too, so that it barely changes as the head turns),
  their line's angle (roll), the forehead's top, the face's width
  (temple to temple) and height. The expressions run only when asked
  (the panda's blinks and mouth, to come).
- **The hands**: the palm's detector three times a second while no hand
  is followed, once a second while one is (a second one coming), at most
  two hands; each hand's points in a square round its palm turned so that
  its fingers point up, followed from frame to frame.
- **When**: only while the camera is read, an effect needs them, and
  « Je reviens » does not show. On threads of their own (`sight.rs`),
  which take the latest frame when they are free (frames meanwhile are
  not copied), so that the frames' loop never waits: the face every frame
  for an accessory, every other one for Cadrage alone; the hands ten times
  a second. Their threads end when no effect needs them.
- **Their cost** (one core of a laptop's Core Ultra 7 165H, the thread's
  own time): the face followed, some 16 to 20 ms a frame (the mesh), the
  detector 4 ms when it runs; the hands followed, some 27 ms a look, the
  palm's detector 35 to 40 ms when it runs. A face's accessory at 30
  frames a second is about half a core; the gestures, while no hand is
  in the picture, about an eighth of one.

### Visage (one at a time; `wear.rs`, `rig.rs`)

- **Oreilles, Bonnet, Couronne, Lunettes** (`[camera] face = "ears"`,
  `"beanie"`, `"crown"`, `"glasses"`; `"none"`, the default): shapes in
  three dimensions on the head (`rig.rs`, the panda mockup's rig: the
  head's half width 30 units, its middle between the temples), turned by
  the head's yaw, pitch and roll and seen in perspective. The pose comes
  from the mesh: its axes across (temple to temple) and down (forehead to
  chin), the roll from the eyes' line, the pitch at rest measured on the
  test pictures; each point the accessory sits on (the eyes, the
  forehead's top) is placed exactly on the face's.
- **On the hair**: the top of the head is where the person's mask (lot
  3's segmentation, five times a second while a hat or the ears are on)
  ends above the forehead along the face's axis, else a typical head's.
  The beanie's ribbed fold sits on the forehead's top, its dome (an
  ellipsoid's cap, its outline the hull of its points seen) covers the
  hair to its top, its pompom on it; the crown, five points round a ring,
  its inner side seen between them, three jewels and gold balls on its
  front, sits a little into the hair; the ears, the panda's ink
  (`#23262E`, a soft highlight, a darker inner ear), stand on it, tilted
  outwards, the far one first.
- **On the face**: the glasses' lenses in front of the eyes, curving
  with the face (the mockup's proportions: of the eyes' half span), thick
  frames, a light edge on their top, a reflection across the lenses;
  their arms go back towards the ears, fading into the hair, the one on
  the side turned away hidden behind the head.
- **Their light**: a soft gradient (lit from above on the left), a light
  rim on the edge, a soft contact shadow on the head under each (the
  beanie's fold, the crown's ring, the ears' base, the glasses' frames).
- Drawn anti-aliased (tiny-skia), apart in the head's box, over the made
  frame: the same on the GPU's path and the CPU's, after what is behind
  the person, before Cadrage's crop. A face lost for half a second, the
  accessory goes; it comes back with the face.

### Panda (Visage; `panda.rs`)

- **The approved mockup, ported** (`[camera] face = "panda"`): the bar's
  panda on its rig, each feature of the face on a sphere (the cheeks and
  the muzzle a bump on it) turned with yaw, pitch and roll in
  perspective; the ears and the magician's hat on springs of their own
  (driven by their anchors' accelerations, the hat kicked by a start);
  blinks, the wink drawing's happy arch, wide eyes, brows lifting the
  patches, the mouth from the jaw, its round and the smile (its tongue
  showing), blush, the breath and its bob, the paws up when laughing.
- **What drives it**: the face landmarker's pose and its expressions
  (the blendshapes, asked only for the panda): `eyeBlink`, `jawOpen`,
  `mouthFunnel`/`mouthPucker`, `mouthSmile`/`mouthFrown`, `eyeSquint`
  (happy eyes when laughing), `browInnerUp`/`browOuterUp`/`browDown`,
  `eyeWide`, `eyeLook*` (where the eyes look); the picture is not
  mirrored (its left is the person's right). Followed through the
  mockup's springs at fixed steps of 1/120 s; the lids a little smoothed.
  Without a face for 2 s it idles: the mockup's sway, glances, breath,
  and a slow blink every 4.3 s.
- **Its look** (« Allure », `[camera] panda-look`, `"magician"` by
  default, the mockup's): `"classic"` (both ears ink), `"magician"` (the
  green magic ear and its glow, the top hat with its band, patch and
  star, the bow tie), `"secretary"` (gold glasses, the pencil behind the
  ear, the clipboard, the bow tie); its shapes, colours and frames (where
  it stands, its size) the mockup's.
- **In the person's place**: behind it, Fond's wallpaper (moving), else
  the room without the person (the mask's room pulled in where they were
  from ever coarser pictures of it, then blurred, `backdrop::emptied`);
  the background's glow behind the panda, its soft shadow, the scene's
  tints on it (the aurora's or the room's, the mockup's `SCENES`).
  Cadrage does not apply; the reactions and the banner do.
- **Its cost**: drawn with tiny-skia at 0.8 of the frame's size and laid
  over it between its pixels in one pass (its glow, shadow, tints):
  measured 22 to 28 ms a frame on one core (the secretary the most), on
  the camera's loop: 30 frames a second hold.

### Cadrage (Image; `framing.rs`)

- `[camera] framing` (off by default): the picture cropped round the
  face, 16:9 kept: the face's middle at the middle across and at 45 % of
  the height, zoomed 1.32 times (the mockup's `scale(1.32)`), less for a
  face near the camera (the crop at least 2.2 times the face's height,
  forehead to chin: its head and the shoulders' top stay in), never out
  of the frame (a face at its edge stays nearer it).
- **No jitter**: a face moving less than 4 % of the crop moves nothing;
  further, the crop goes two thirds of the way in 450 ms (no jump).
  Without a face for 1.5 s, the whole picture comes back, as slowly.
- Over the accessory and what is behind the person, under the reactions
  and the banner, which stay where they are in the picture (the mockup's
  transform holds the room and the person only).

### Réactions (`react.rs`)

- **Gestes** (`[camera] gestures`, on: the mockup's state): a thumb up
  (the thumb's tip above the rest of the hand, pointing up within 40°,
  the four fingers folded) held 0.2 s sends one rising reaction: the
  mockup's amber disc (`rgba(245,180,74,0.95)`, 42 px of its 176 tall
  preview, its thumb `#1B1408`) 28 px from the right and 54 from the top,
  popping in, rising 26 px and fading over 1.8 s (its `pop`, CSS's
  `ease-out` between its keyframes); another, once the thumb was down a
  second. A hand raised (its four fingers open, pointing up within 40°,
  its middle finger's tip in the picture's top 60 %) for 0.3 s shows
  « main levée » (« hand raised »; 11 px bold `#1B1408` on the same
  amber, 8 round, 8 px from the top right corner) while it stays up; down
  0.6 s, it goes.
- **Fêtes** (`[camera] celebrations`, off by default): on each of the
  agents' moments (an agent's tests green, `hocus celebrate`, the MCP's
  `celebrate`: the moments of docs/design/effects.md, counted by the
  compositor, `CameraLook.moments`, under the desktop's rules that are
  the user's: none with reduced motion, nor in the power saver),
  while the camera is read, 48 of the mockup's pieces (6 × 9 px of its
  preview, its four colours) fall through the picture turning 260°, 2.2
  to 3.1 s each, started over 1.5 s; another moment adds a shower.
- None shows while « Je reviens » does.

### The window and the settings

Visage's tiles (Aucun, Oreilles, Bonnet, Couronne, Lunettes, Panda) are
made, one of them ringed, a violet dot on the tab with one; with the
panda, « Allure » under the hint: its three looks, each drawn (96 px
square, the mockup's still), its name under it, the chosen ringed.
Image's Cadrage and Réactions' Gestes and Fêtes are made, each its words
under the grid. `[camera]`: `face`, `panda-look`, `framing`, `gestures`,
`celebrations`, with their rows in Settings › Caméra.

**Choices made here** (to confirm on the machine):

- Gestes is on by default (the mockup's state; the spec says only that
  Fêtes is off): the hands' models then run in every call, some an eighth
  of a core while no hand shows.
- The hats' width is a head's with its hair (the beanie's 37 units of
  the head's 30): a big head of hair still shows round a beanie.
- The panda stands where the mockup puts it, in the middle, whatever
  where the person sits; « Allure »'s tiles are drawn on the aurora's
  still colours (the mockup's), whatever Fond is; the mockup's own
  « Arrière-plan » choice is Fond's tab.
- Cadrage's zoom is the mockup's 1.32, less for a face near the camera
  (no head cut), never more for a face far away.
- Fêtes follows reduced motion and the power saver; a share or the
  CPU's drawing (which hold the desktop's own effects) do not stop it,
  the picture being the call's; a moment while the camera is not read
  rains nothing later.

**Measured headless**: `hocus-camera`'s unit tests: the face's points on
the portrait (`data/person.jpg`) where its eyes, nose and chin are,
level, smiling with its eyes open, followed within 30 ms of the thread's
time a frame; no face in an empty picture; a thumb up on `data/thumb_up.jpg`
(NASA's portrait of Koichi Wakata, public domain), its thumb's tip where
it is, followed within 50 ms a look; a raised hand on
`data/raised_hand.jpg` (NASA's photograph of Chris Cassidy, public
domain); no gesture on the portrait; each accessory changing the pixels
round the head (above the eyes, on them for the glasses) and none
elsewhere; the panda: a blink closing its eyes, `jawOpen` opening its
mouth, idle and blinking without a face, each look drawn within 33 ms
a frame; Cadrage bringing the face to the middle wherever it is in the
picture (as far as the crop reaches), holding still for a few pixels'
move, and zooming in on `data/far.jpg` (NASA's photograph of Jonny Kim,
public domain); the gestures' timing (once, again after a second down;
« main levée » after 0.3 s, gone 0.6 s down); the confetti falling and
gone. They run one at a time (`.config/nextest.toml`'s `camera-models`
group), so that their budgets do not share a core. The settings:
`settings_appearance::the_camera_page_writes_the_screen_light_s_keys`; the
window's tiles (`camera::effects::tests`). The look: the screenshots,
`sight::tests::screenshots_of_the_camera_s_pictures_lot_4` and
`panda::shots::screenshots_of_the_panda` (the frames)
and the compositor's `camera::screenshots_of_the_window_s_lot_4` (the
window, the real program's frames in its preview), both ignored,
`HOCUS_SHOTS`.
