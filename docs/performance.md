# Performance, and the GPU in tests

How the tests that need a GPU run, and why only on purpose (issue #534).

## Tests draw on the CPU

On a developer's machine, the first render node (`/dev/dri/renderD128`)
is usually the GPU the live session draws with. Many test compositors
drawing there in parallel, each at 2560 × 1600 with its shell on wgpu,
once froze a machine whole: no keyboard, no pointer, a hard reboot. The
GPU is part of the live session, which tests must not touch (CLAUDE.md).

So, unless `HOCUS_TEST_GPU` names a render node:

- the test compositor (`hocus_compositor::Headless`) draws with pixman.
  The tests of its shaders (the streak, the halo, the glass, the rim's
  ring) draw with GLES on Mesa's software device (llvmpipe,
  `RendererChoice::GlesSoftware`, through
  `hocus_compositor::test_gpu::gles()`). Without that device, as on CI,
  they draw with pixman and skip what only GLES draws. llvmpipe draws on
  one thread (`LP_NUM_THREADS=1`, in `.cargo/config.toml`). With a thread
  per core, each of those tests took every core, and the tests running
  beside them missed their frames. The tests that measure a motion
  step the compositor's clock held (below), so one thread costs them no
  time;
- the shells they start draw with tiny-skia (`ICED_BACKEND=tiny-skia`),
  since the compositor has no GPU to give them;
- the tests of the shell's motion on wgpu (the clock's minute, the agents'
  slide and hop, the secretary's ear) are skipped, and say why.

The harness enforces it. It refuses a compositor asked to draw with GLES
on a GPU without the variable, or on another node than the one it names,
and it draws `RendererChoice::Auto` with pixman. When a harness is dropped
without the variable, it looks for a render node held open by the test's
process or any process it started, down to their children (the shell, the
programs it runs), in `/proc/PID/fd`. It fails the test if it finds one.
Whatever draws on a GPU holds its node open while it does: Mesa's EGL
displays, wgpu's devices. So a test, or a script it gives the shell, that
reaches a GPU by itself fails.

## Running the GPU tests on purpose

The motion tests measure what only a GPU shows: the shell's frames on
wgpu, the compositor's shaders at a real GPU's pace. Run them:

- **alone**: one test binary, and one test process at a time (`-j1`);
- **on a machine or VM whose session does not depend on that GPU**: a
  second GPU the session does not draw on, a machine with no session
  running, or a VM with a GPU of its own.

```sh
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor --test glass -j1
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor --test agents -j1
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor --test bar -j1
```

With the variable set, the compositor of a test of its shaders draws on
that node, and the shell it starts draws there with wgpu. Every other test
still draws with pixman, and its shell with tiny-skia.

## Tests on a loaded machine wait on conditions, never on time

A test acts and reads on the machine it runs on, which may be busy:
CI draws on its CPU, and a developer's machine runs other builds and tests
beside it (issue #533). To find these races as CI's runner shows them,
without loading the rest of the machine, the suite runs on four cores
with more test threads than that, and passes three full runs in a row:

```sh
taskset -c 0-3 env NEXTEST_TEST_THREADS=8 cargo nextest run --workspace
```

A test waits for what it needs, with a generous timeout that only fails
when that never comes. It does not sleep a guessed time:

- **A shell surface takes a click once it is drawn.** A layer surface has
  its place from its first commit, before any buffer. Until the shell
  draws it at that size, it shows nothing and takes no input, and a press
  goes to what is under it. `common::click` and `common::point` wait
  until every layer surface placed under the pointer is drawn
  (`Headless::undrawn_at`). `common::over` also waits until the surface
  of the namespace asked for takes the pointer there, for a surface that
  the shell moves after another one. A surface's place is read from
  `Headless::drawn_layers`, so a 0 × 0 place before the first buffer is
  never read.
- **Pixels are read once the screen is at rest.** A frame is drawn only
  after the loop turns, so on a loaded machine it can come long after a
  motion's end. `common::at_rest` waits until no motion is under way as
  of the last frame drawn, no frame is asked for since, and none is due
  by now from a timer that has not run yet, as Alt+Tab's panel's
  (`Headless::at_rest`). A loop held up past a second after a motion's
  last frame draws one more frame as of then, so that the screen shows
  the motion's end and comes to rest.
- **A motion is measured on the held clock.** `Headless::hold_clock`
  stops the motions' time. `Headless::step` moves it on by one refresh
  and draws a frame at that time. `common::sample` reads each step's
  frame. A streak, a halo or a pulse is then seen at the same moments
  however late the frames come. What falls due on the motions' clock
  (the idle countdown, the halo's reminder, a peeking panda's 8 s) comes
  at the step it falls in, not when the real clock's timer runs. Frame
  callbacks and presentation times stay on the real clock. An agent's
  start is on the machine's clock, which counts from boot: a test moves
  the motions' clock on rather than backdating an agent past the
  machine's start.
- **A private daemon is used once it listens.** Its socket's file comes
  as the daemon binds it, a moment before it listens, and a connection
  then is refused. `common::private_bus` waits for `dbus-daemon` to print
  its address (`--print-address`). `common::wait_accepts` waits for
  PipeWire's socket to take a connection.
- **What the shell does off its loop is waited for by what it says.** An
  icon looked up or a paste read from the clipboard lands later: the test
  waits for the line the shell logs, or for the pixels, before it acts on
  them. A log is read by its whole lines (`common::logged`,
  `common::last_json`): the line being written is read once it ends, not
  cut short. What a wait found is what it returns, not a read after it.
- **A count is not a time window around something else.** The clock
  draws on a surface of its own, counted apart: a test that counts what a
  change draws counts the surfaces it means, and does not retry while the
  clock ticks.

## The wallpaper's counts (6.3)

The backgrounds are counted, not timed. Each screen's is drawn once per
change (of the file's `[wallpaper]`, or of the screen's size or scale),
on the compositor's wallpaper thread, and none at rest
(`Backgrounds::draws`, `settings_wallpaper.rs`'s
`at_rest_the_wallpaper_page_commits_nothing`). A picture of one's own is
decoded once per change (`Backgrounds::decodes`) and its source dropped
once scaled. Memory: one RGBA buffer per screen at its size for a picture
(16.4 MB for 2560 × 1600, 19.8 MB for 3440 × 1440); a built-in or the
plain colour a sixteenth of that (a quarter of the pixels each way).

## What moves for show: the window rain and the moving wallpapers (#432, #433)

Counts gate, times are reported. Each motion is counted on the held
clock, on GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU`
names: without either, the tests that only GLES draws are skipped, as
the glass's) and, where it draws there, on pixman.

**A window's rain** (`tests/rain.rs`, 60 Hz):

| Budget | Gate | Measured (2026-11-07, llvmpipe and pixman) |
|---|---|---|
| Frames to open, on its screen | ≤ ⌈0.36 × refresh⌉ + 1 (23) | 22 |
| Frames to close, on its screen | ≤ ⌈0.30 × refresh⌉ + 1 (19) | 19 |
| On the other screen | 0 | 0 |
| 100 ms after its end | 0 frames | 0 |
| Allocations on the loop | a whole rain ≤ the standard opening's + 50; none growing frame after frame | 71 a frame against the standard's 80 (its picture is drawn again only when the window commits) |

**A moving wallpaper** (`tests/moving_wallpaper.rs`):

| Budget | Gate | Measured |
|---|---|---|
| Visible 60 s: frames on its screen | rain ≤ 902, stars ≤ 1 202; on battery ≤ 482 / 602 | 900, 1 200; 450, 600 |
| Covered (≥ 90 %) 60 s | 0 frames, 0 of its own, 0 wakes | 0, 0, 0 |
| Another screen covered | 0 frames there | 0 |
| The bar's glass over it, 60 s | 0 blurs after the first | 0 |
| Allocations a frame, over a turn of the loop at rest | ≤ 30, none growing over the minute | 29 to 30 (most are smithay's drawing of a frame and the loop's timer) |
| Reduced motion, power saver, `animate = false`, battery "still", locked, fullscreen, 2 min without input | no frame of its own | none |
| Pixman, 60 s | its still picture drawn once, then 0 frames | 0 |
| Memory | the atlas ≤ 256 KB a scale; no screen-sized buffer more | 16 KB for every scale; on GLES the wallpaper thread's still picture is not even drawn (the shader draws into the screen's frame) |

Reported, not gated: `cargo nextest run -p hocus-compositor --test
moving_wallpaper --run-ignored only timings` (a frame at 3440 × 1440, beside
a still one), and the shots for the pull request
(`HOCUS_SHOTS=DIR … --run-ignored only shots`).

The glass never blurs a moving wallpaper again: what is behind a pane is
drawn with a second element of the wallpaper, still at the moment it
started, whose commit never changes; the screen shows the moving one,
which replaces it once the panes are done.
