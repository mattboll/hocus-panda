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
  `hocus_compositor::test_gpu::gles()`), on CI too. Without that device
  they fail, saying so (`common::missing`). llvmpipe draws on
  one thread (`LP_NUM_THREADS=1`, in `.cargo/config.toml`). With a thread
  per core, each of those tests took every core, and the tests running
  beside them missed their frames. The tests that measure a motion
  step the compositor's clock held (below), so one thread costs them no
  time;
- the shells they start draw with tiny-skia (`ICED_BACKEND=tiny-skia`),
  since the compositor has no GPU to give them;
- the tests of the shell's motion on wgpu (the clock's minute, the agents'
  slide and hop, the secretary's ear) are `#[ignore]`d with that reason:
  asked for without a GPU, they fail and say why.

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

- **alone**: one test file's, and one test process at a time (`-j1`);
- **on a machine or VM whose session does not depend on that GPU**: a
  second GPU the session does not draw on, a machine with no session
  running, or a VM with a GPU of its own.

```sh
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor glass:: -j1
# with the tests that only a GPU passes (`--run-ignored all`)
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor -E 'test(/^agents::/)' -j1 --run-ignored all
HOCUS_TEST_GPU=/dev/dri/renderD128 cargo nextest run -p hocus-compositor -E 'test(/^bar::/)' -j1 --run-ignored all
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

## A day's view (#655)

Counts gate CI (frames, scenes, wakeups, allocations: the same on any
machine); times, CPU and watts depend on the machine and are only read,
over a real day. The compositor records that day: is the desktop light
over hours of work, and when is it not, and why?

### What is recorded

A thread of the compositor (`perf`, `hocus_perf::record`), never the
loop, wakes every 10 s and appends one sample to the day's file:

- the compositor's, the shell's and, while they run, the Settings' CPU
  time (user and system, `/proc/PID/stat`) and resident memory
  (`/proc/PID/statm`);
- what the loop counted since the last sample, in atomics it only
  increments (`hocus_perf::Counters`): the frames drawn (all screens, and
  each screen's when there are several), the scenes built, the loop's
  turns (its wakeups), the user's input events, the frames drawn while
  one of the desktop's motions ran, the applications' window commits;
  the longest turn and the longest frame; allocations, when the program
  counts them (none does yet);
- on battery, the machine's power (`power_now`, else `current_now` ×
  `voltage_now`, in `/sys/class/power_supply`); nothing on AC;
- what happened since the last sample, which explains a peak: a window
  opened or closed (its application), a screen plugged or unplugged, the
  lock and the unlock, the machine going to sleep and waking, the
  configuration changing (its sections: `[appearance]`, `[wallpaper]`…),
  the power source, a page of the Settings opened (they say it on the
  control socket, `perf settings PID NAME`, which the socket's own thread
  notes: the loop is not told), a moving wallpaper started or stopped.
  The loop pushes them into a ring of 128, under a lock it takes only to
  push one.

The file is `$XDG_STATE_HOME/hocuspanda/perf/YYYY-MM-DD` (the local
date), one JSON line per sample, appended, so that a day reads after a
crash up to its last whole line. Fields at zero are left out: a sample
at rest is about 80 bytes. A `start` line marks each session, a
`notified` line each signal notified. 14 days are kept: older files are
removed as the recorder starts and as a day begins. `[perf] record =
false` (the Settings' "Mesurer la journée"): no thread, no file; changed
in the file, the recorder starts or stops at once.

### Signals

What counts as a problem, judged sample after sample by
`hocus_perf::Watch` (a few minutes of samples in hand at most), the same
way while recording and when a day is read:

| Signal | When |
|---|---|
| sustained CPU | the compositor and the shell above 10 % of one core on average over 5 min |
| frames at rest | more than 1 frame a second for 1 min, with no input, no application's window drawing, beside the desktop's own motions |
| wakeups | more than 5 loop wakeups a second on average over 10 min with no input |
| memory only growing | the compositor's or the shell's, rising for more than 3 h at more than 2 MB an hour, each of its last three hours higher than the one before, never more than 5 % back |
| slow moment | a frame over 50 ms, or a loop turn over 16 ms (not in the first 30 s, while the session starts) |

A gap (the machine asleep, the session off) ends what was running. A
signal raised sends a notification of low urgency (to the history, no
bubble), once a day per kind (even across sessions: the `notified`
lines), "Le bureau a dessiné 9 images par seconde sans rien qui bouge
(depuis 15:10)."; opened from the history, it opens the Settings'
Performances page. The notification is sent and waited on by a thread of
its own; only its opening comes back to the loop.

### A day read back

`hocus_perf::report` reads a day: its totals (CPU, memory at the start,
the end and its peak, frames, scenes, wakeups at rest, the frames drawn
while locked, the power on battery, a moving wallpaper's time), its
signals, and its peaks: runs above twice the day's median CPU and above
2 % of one core (runs a minute apart or less are one), and every
signal's span but a memory's, the eight that cost the most CPU kept,
numbered by time, each with the events from 30 s before it to 30 s
after. The Settings' Performances page (docs/design/settings.md) and

```sh
hocus perf day [--date YYYY-MM-DD] [--json]
```

show the same report: the command as text in the messages' language, or
as JSON for a script or an agent. A day not recorded says so and fails.
`hocus_perf::fixture` is an invented working day, for the tests and the
screenshots.

### Its own budgets

- The loop: **0** wakeups and frames more at rest with recording on
  (`tests/perf.rs`, `recording_wakes_the_loop_no_more_at_rest`: 2 s at
  rest, the recorder sampling every 50 ms, the loop turning and drawing
  as with it off: once, nothing).
- The thread: one wakeup per sample; a stack of 256 KB; the ring of
  events and a few minutes of samples in hand.
- The disk: under 1 MB a day (`a_working_day_is_under_a_megabyte`: 24 h,
  10 of them at work, an event a minute); a day's file is cut at 1 MB in
  any case.
- Reading a day (the page, the command): on the Settings' own thread as
  the page shows, nothing at rest.

## What moves for show: the window rain and the moving wallpapers (#432, #433)

Counts gate, times are reported. Each motion is counted on the held
clock, on GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU`
names: without either, the tests that only GLES draws fail, as the
glass's) and, where it draws there, on pixman.

**A window's rain** (`tests/rain.rs`, 60 Hz):

| Budget | Gate | Measured (2026-11-07, llvmpipe and pixman) |
|---|---|---|
| Frames to open, on its screen | ≤ ⌈0.36 × refresh⌉ + 1 (23) | 22 |
| Frames to close, on its screen | ≤ ⌈0.30 × refresh⌉ + 1 (19) | 19 |
| On the other screen | 0 | 0 |
| 100 ms after its end | 0 frames | 0 |
| Allocations on the loop | its first two frames (which make its picture) ≤ the standard opening's + 100, each after ≤ the standard's most + 10 (frame by frame: either opening's frames take 7 more or fewer as the client's commits fall in them, too much for totals); none growing frame after frame | 318 to 334 against 254 to 270, then 63 a frame (75 the last) against 58 to 71, flat (its picture is drawn again only when the window commits) |

The thawing pane (#740) is held to the same budgets
(`the_thawing_pane_costs_the_rains_frames`): 22 frames to open, 19 to
close, none elsewhere, none 100 ms after.

**A moving wallpaper** (`tests/moving_wallpaper.rs`):

| Budget | Gate | Measured |
|---|---|---|
| Visible 60 s: frames on its screen | rain ≤ 902, stars ≤ 1 202, aurora ≤ 602; on battery ≤ 482 / 602 / 302 | 900, 1 200, 600; 450, 600, 300 |
| Covered (≥ 90 %) 60 s | 0 frames, 0 of its own, 0 wakes | 0, 0, 0 |
| Another screen covered | 0 frames there | 0 |
| The bar's glass over it | ≤ 1 blur in 60 s, ≤ 5 in 10 min (its still copy taken again every 2 min), at least 1 in 10 min | 0 in 60 s, 3 in 10 min |
| Allocations a frame, over a turn of the loop at rest | ≤ 30 on every frame of the minute; growth from its start to its end is not gated (a step takes a loop turn more or fewer as the real clock goes under load, some ten allocations each way: early and late counts read the load), what grows with the frames fails the cap | 24 to 25 at most, 13 to 15 on average, flat (one timer for the wallpapers, set again by itself, never inserted per frame) |
| Reduced motion, power saver, `animate = false`, battery "still", locked, fullscreen, 2 min without input | no frame of its own | none |
| Pixman, 60 s | its still picture drawn once, then 0 frames | 0 |
| Memory | the atlas ≤ 256 KB a scale; no screen-sized buffer more | 16 KB for every scale; on GLES the wallpaper thread's still picture is not even drawn (the shader draws into the screen's frame) |

Reported, not gated: `cargo nextest run -p hocus-compositor --run-ignored
only -E 'test(/^moving_wallpaper::.*timings/)'` (a frame at 3440 × 1440, beside
a still one), and the shots for the pull request
(`HOCUS_SHOTS=DIR … --run-ignored only shots`).

The glass never blurs a moving wallpaper's frames: what is behind a pane
is drawn with a second element of the wallpaper, a still copy whose
commit changes only when it is taken again (as a surface with glass
appears, and every 2 minutes while it moves); the screen shows the moving
one, which replaces it once the panes are done. Between its frames it is
drawn again only in the places that changed over it, each apart (eight
to a draw call, nothing allocated), never their bounds: the damage
tracker draws again what is over it only where it is damaged.

## The effects' cost (#815)

The effects that play on their own (docs/design/effects.md) are measured
where they cost: on the live session. With

```toml
[developer]
effects-timing = true
```

(followed live; off by default) the compositor writes, every 5 s, at info
level under `hocus_compositor::effects` (`journalctl --user -t hocuspanda`
on a session), for example:

```text
an effect's cost effect="snow" steps_a_second="60.0" us_a_step=412 tick=95 build=210 draw=107
the frames' cost frames_a_second="61.2" us_a_frame=690
```

- one line per effect that moved in those 5 s: its steps a second (60 at
  most, `effects::Pace`), and the loop's CPU time a step, in µs: its
  `tick` (its simulation), the `build` of its elements (its meshes, what
  it draws into its textures) and the `draw` calls it submitted. CPU time
  of the loop's thread (`CLOCK_THREAD_CPUTIME_ID`): what it waits for, the
  GPU or a fence, is not counted; what the GPU does, neither;
- one line for the frames, whatever drew them: how many a second, and
  the CPU time each took, all of it (the elements, the damage, the draws,
  the effects' within). An effect's own line can be small while it costs
  through the frames it asks for: compare `us_a_frame` with the effect
  on and off.

The same, headless, on Mesa's software device (no GPU touched), with the
real shell, a terminal maximised and two floating, at the maintainer's
screen's size:

```sh
env -i HOME=$(mktemp -d) PATH=/usr/bin:/bin XDG_RUNTIME_DIR=$(mktemp -d) \
    LP_NUM_THREADS=3 ICED_BACKEND=tiny-skia RUST_LOG=hocus_compositor::effects=info \
    taskset -c 0-3 target/release/hocuspanda --backend headless --renderer gles-software \
    --output 2560x1600@1.25 --no-xwayland --config FILE --spawn foot
```

Read it for what the compositor's own code does; not for a GPU's frame:
llvmpipe draws on the CPU (its threads, `llvmpipe-N`, apart from the
loop's), and the loop's thread does part of its work (vertices, clears,
uploads) that a GPU does itself, so `draw`, and the `build` of an effect
that draws into a texture, are larger there than on a GPU.

Measured so (2026-11-13, 2560 × 1600 at 1.25, a terminal maximised and
two floating, or the two floating alone; `LP_NUM_THREADS=3`, four cores),
the loop's thread's CPU, before and after #815's second pass, each effect
alone; in µs a step, its tick / build / draw (llvmpipe holds the frame
rate down, so the steps a second differ; compare the milliseconds a step):

| Effect | Windows | Before | After | Loop's ms a step |
|---|---|---|---|---|
| snow | maximised | 32.7 % at 35/s; 83 / 2 131 / 190 | 14.9 % at 42/s; 71 / 2 010 / 374 | 9.5 → 3.6 |
| snow | floating | 24.5 % at 34/s; 121 / 1 222 / 1 017 | 7.9 % at 24/s; 178 / 1 219 / 337 | 7.3 → 3.3 |
| koi | maximised | 10.4 % at 24/s | waits: 0.1 % | 4.3 → 0 |
| koi | floating | 6.7 % at 14/s; 1 116 / 1 733 / 778 | 5.5 % at 15/s; 345 / 1 463 / 450 | 4.7 → 3.6 |
| rain on the glass | maximised | 14.1 % at 46/s | waits: 0.2 % | 3.1 → 0 |
| rain on the glass | floating | 13.1 % at 47/s; 27 / 1 549 / 209 | 10.0 % at 48/s; 23 / 1 359 / 150 | 2.8 → 2.1 |
| fireflies (evening) | maximised | 44.5 % at 27/s; 38 / 463 / 4 719 | waits: 0.2 % | 16.5 → 0 |
| fireflies (evening) | floating | 90.5 % at 5/s; 107 / 552 / 170 264 | 17.2 % at 39/s; 40 / 65 / 3 028 | 177 → 4.4 |
| garland | floating | 12.2 % at 38/s; 42 / 387 / 365 | 4.9 % at 43/s; 38 / 374 / 60 | 3.2 → 1.2 |
| creatures | floating | 4.2 % at 58/s | 4.4 % at 58/s | 0.7 → 0.8 |
| snow, garland, creatures | floating | 30.8 % at 25/s | 8.6 % at 22/s | 12.5 → 4.0 |

What it cost, and what changed:

- **The damage tracker's rectangles.** The snow's flakes were told in
  some 3 000 cells of 16 px a frame; smithay's damage tracker took 1.7
  ms a frame on them before drawing anything, and as much again drawing.
  Changes many and spread are now told by rows of cells of 32 to 128 px,
  a few hundred rectangles at most, all of the element once they cover
  half of it (`effects/kept.rs`, `cells`).
- **What is behind a window.** Nothing an effect changes under an opaque
  element in front of it (a window that says it is opaque) is told: the
  windows over it are not drawn again (`render/mod.rs`, `cover_effects`).
  The snow's flakes are drawn only where the wallpaper shows
  (`kept::Seen`).
- **The wallpaper under a maximised window.** With windows and the bar
  covering 90 % of a screen or more, the rain on the glass, the koi, the
  fireflies and the aurora wait, their clocks going on, as a wallpaper
  that moves stops (`State::wall_unseen`).
- **The fireflies' glows** were drawn whole for each damaged cell, a
  few thousand times a step (170 ms a step on llvmpipe, the live
  session's 59 %). Shapes drawn straight now take, in each place, those
  that reach it alone, and the glows, which cover most of the screen,
  are told as all of it, in one.
- **The koi's water**: its waves stepped row by row (vectorised), its
  heights' texels written in place, not allocated each step.
- A cap's glints, which twinkle at each step, are drawn straight, not
  into a texture first.

On a GPU, `draw` and the textures' parts of `build` (the snow's flakes,
the koi under their water, the drops' map) cost the loop far less than
here, where llvmpipe's vertices, clears and uploads run on it.
