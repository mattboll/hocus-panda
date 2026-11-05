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
  of the last frame drawn, and no frame is asked for since
  (`Headless::at_rest`).
- **A motion is measured on the held clock.** `Headless::hold_clock`
  stops the motions' time. `Headless::step` moves it on by one refresh
  and draws a frame at that time. `common::sample` reads each step's
  frame. A streak, a halo or a pulse is then seen at the same moments
  however late the frames come. Frame callbacks and presentation times
  stay on the real clock.
- **A private daemon is used once it listens.** Its socket's file comes
  as the daemon binds it, a moment before it listens, and a connection
  then is refused. `common::private_bus` waits for `dbus-daemon` to print
  its address (`--print-address`). `common::wait_accepts` waits for
  PipeWire's socket to take a connection.
- **What the shell does off its loop is waited for by what it says.** An
  icon looked up or a paste read from the clipboard lands later: the test
  waits for the line the shell logs, or for the pixels, before it acts on
  them.

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
