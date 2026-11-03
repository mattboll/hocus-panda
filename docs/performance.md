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
  beside them missed their frames. The tests wait on a virtual clock, so
  one thread costs them no time;
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
