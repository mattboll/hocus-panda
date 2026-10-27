# Performance

Fast is a goal, not a hope ([vision](vision.md)): what the desktop costs is
measured, and budgets keep it from drifting (#88). This page says what is
measured, what gates a build, where the reference numbers come from and how
to reproduce them.

## What is measured

- **The control API's `stats`** (`hocus msg stats`): for each output, the
  frames drawn and presented, the scenes built (`renders`: a pass that
  builds the scene and compares it with the last frame, whether anything
  is drawn or not), the frame-time histogram and the slowest frame; for
  the loop, its wakeups (turns that handled something; one that a test
  harness turns counts none), how many times each loop handler ran by
  name (what woke the loop: `watchdog`, `wayland clients`,
  `control request`, a worker's result such as `wallpaper`, a moment on
  the clock such as `column clock`, `calendar`, `session save`), the slow
  handlers and the stalls.
- **Allocations on the loop**: test programs install a counting allocator
  (`hocus_compositor::allocations::Counting`), and the harness counts what
  the compositor's loop allocates while it works: allocations, bytes asked
  for, bytes held. Test clients run in threads of their own and are not
  counted.
- **Wall-clock timings**: startup, a turn of the loop at rest, input to
  photon, opening the launcher (and again right after a search) and the
  settings, frame times (`hocus-test`'s `timings`).

## Budgets that gate a build

Counts, not times: they are the same on every machine however loaded, so a
regression fails the build and nothing flakes. Each budget is the measured
count with a margin of about 10 % (allocations are the same from run to run
within a handful out of 180,000); a change that moves a count on purpose
updates the budget in the same pull request, with the numbers.

| Budget | Where | Measured | Budget |
|---|---|---|---|
| At rest, two screens and the shell, 60 s on a virtual clock | `budgets` | 1 frame (the column's clock turning a minute) and 1 scene per screen, no handler run, 209,500 allocations (29 a turn of the loop), 4.1 MB fewer held, or 316 kB more when a buffer of the start went before the minute | frames `[1, 0]`, scenes `[1, 1]`, no handler, ≤ 230,000 allocations, ≤ 512 kB more held |
| At rest, the same, 60 s of real time | `budgets` | the watchdog 60 times, the column's clock once, its frame, the session saved once after the start | anything for the watchdog; at most 2 for the column's clock, 2 for the wallpaper's light (every ten minutes), 4 frames, 2 session saves; nothing else |
| Twenty windows open on two screens | `budget-twenty-windows` | frames `[160, 150]`, scenes `[320, 320]`, 180,600 allocations, 376 MB asked for, 9.1 MB held | `[176, 165]`, `[352, 352]`, 199,000, 414 MB, 10.5 MB |
| Their desktops switch, three times on each screen | `budget-twenty-windows` | frames `[23, 21]`, scenes `[102, 102]`, 59,950 allocations, 91 MB, 1.8 MB (the switches' panels' shadows kept, #281) | `[26, 24]`, `[113, 113]`, 66,000, 101 MB, 2.1 MB |
| A minute at rest with the twenty windows | `budget-twenty-windows` | frames `[1, 0]`, scenes `[1, 1]`, 274,700 allocations (38 a turn), 8.5 MB, 0.1 MB | `[1, 0]`, `[1, 1]`, 302,000, 10 MB, 0.25 MB |
| The launcher on 40 applications opened, searched in, closed, and opened again (#242) | `budgets` | the second opening 288 allocations, the first 5,091 | the second at most a tenth of the first |

At rest the loop wakes for the watchdog (once a second) and for what the
clock shows: the column's minute, and the wallpaper's light every ten
minutes. The session is saved thirty seconds after something happened,
not every thirty seconds; calendars and updates wake the loop only when
there is a calendar or something to ask.

## Timings, reported only

Wall-clock times measure the machine as much as the code: in a shared CI
runner they flaked (#235). CI's `timings` job runs them alone, in a release
build, publishes the table in the run's summary and warns, without failing,
where a target is passed: a frame's budget (16 ms, the one the
architecture sets, opening the launcher from the keyboard included,
#242), 1 s for startup, 100 ms for opening the settings.
Headless outputs are drawn by pixman, on the CPU: frame times there are an
upper bound of what a GPU draws. The same goes for the scenarios'
`expect_budget` steps (#295): a frame slower than their limit, or a
handler that held the loop, is printed as a warning with its time and
never fails the scenario.

Reference numbers, a debug build (the test profile) on a development
machine, three runs:

| What | Time |
|---|---|
| Startup: two screens and the shell drawn | 220–232 ms |
| A turn of the loop at rest | 4.4–5.4 µs |
| A turn of the loop at rest, twenty windows | 9.7–10.1 µs |
| Opening the launcher on 60 applications, the grid of all of them shown (#254), to its first frame, drawn as it opens while closed (#242) | 6.7–7.0 ms (22.0–24.3 ms before) |
| The same right after a search, before the closed card was drawn again | 17.2–19.8 ms |
| Opening the settings, to their first frame | 23–28 ms |
| Input to photon (pointer motion to the frame drawn), median / 95th percentile / worst | 0.4–0.5 / 0.6–0.7 / 1.9–2.4 ms |
| The twenty windows' scenario: frames over 16 ms, laptop panel at scale 2 | 82–91 of 185, slowest 32–35 ms |
| The same, wide screen | 0 of 172, slowest 9–10 ms |

## The pass of #88, before and after

Measured on the same development machine, each count with the
instrumentation alone (before) and with the changes (after).

| Measure | Before | After |
|---|---|---|
| A turn of the loop at rest, two screens and the shell | 49–55 µs, 242 allocations, 77 KB asked for | 4.4–5.4 µs, 29 allocations, 0.7 KB |
| The same, twenty windows | 78–88 µs, 560 allocations | 9.7–10.1 µs, 38 allocations |
| A minute at rest, virtual clock | 1,743,100 allocations, 552 MB | 209,500, 5.3 MB |
| A minute at rest, real time, headless: what woke the loop besides the watchdog | calendar 2, session save 2, updates 1, column's clock 1, frames 4 (4 scenes on each screen) | column's clock 1, its frame 1, session save 1 (after the start) |
| Twenty windows opening: allocations | 502,400 | 180,600 |
| Six desktop switches: scenes built per screen, allocations | 312, 482,200 | 102, 59,900 |
| A minute at rest with twenty windows: allocations | 4,026,900 | 274,700 |
| Input to photon over the settings window, 95th percentile / worst | 29–40 / 30–45 ms | 0.6–0.9 / 2–2.5 ms |
| The session in the Debian VM (DRM, Mesa's software GL, a debug build), a steady minute at rest: wakeups | 73–76 (calendar 2, session save 2, frames 2–3) | 68 (the watchdog 60, the column's clock and its frame, one session save and the stats asked for twice) |
| The same, the compositor's CPU time per mouse motion (20 s of motion) | 264 µs (1,491 ms for 5,647) | 223 µs (1,316 ms for 5,905) |

What changed:

- **The built-in themes are read once.** `Theme::builtin` parsed its TOML
  file at every call, and the theme in effect was asked for four times
  after every turn of the loop: most of a turn's cost. The files are parsed
  once (`OnceLock`), and the theme in effect is worked out again only when
  what it is made of changes (the configuration, the hour, glass, the
  screens and their desktops, the wallpapers' colours).
- **After a turn, the shell works out only what changed.** The wallpapers
  and the theme, and the dock's apps, are worked out again only when what
  they read changes (a hash of it, made without allocating); the leave
  card, quick settings and the lock screen read nothing while they do not
  show; a widget tree walks its children without copying their list.
- **Nothing ticks at rest.** The calendar's timer woke the loop every 30 s
  and redrew every screen, accounts or not; the updates' every ten
  minutes; the session's every 30 s. Each is now an alarm at the moment
  something falls due (`alarm.rs`): reminders and syncs while there is a
  calendar, the next look for updates, the session thirty seconds after
  something happened.
- **A lingering desktop switcher is not redrawn.** It stays 600 ms once
  the strip stopped; every frame meanwhile built both screens' scenes for
  nothing. Now one frame shows where the strip stopped, and an alarm
  takes the switcher away.
- **The settings app tells what it drew again.** It damaged its whole
  window on every pointer motion over it; the compositor redrew 2.7
  million pixels each time.

## The launcher's card, #242

Opening the launcher drew its whole card on the loop: 940 × 564 logical
pixels, 1880 × 1128 on the laptop panel at scale 2. Measured in a release
build, the opening was the card laid out (1 ms: every tile made again and
its name shaped), drawn (13 ms, two thirds of it the card's shadow:
vello_cpu evaluates its blurred rectangle under the whole card), copied
into a new buffer (2 ms), then the first frame (5–7 ms, pixman). The same
machine, three runs of `timings` each (a debug build costs about what a
release build does here: the drawing is in dependencies built optimised):

| Measure | Before | After |
|---|---|---|
| Opening the launcher on 60 applications, to its first frame, debug | 22.0–24.3 ms | 6.7–7.0 ms |
| The same, release | 20.4–23.2 ms | 6.4–6.8 ms |
| Opening it again right after a search, debug | 17.1–19.2 ms | 17.2–19.8 ms |
| The same, release | 16.8–18.4 ms | 16.6–18.7 ms |
| A second opening after a search, allocations (`budgets`), of the first's | 4,189 of 5,065 | 288 of 5,091 |

What changed:

- **The grid is kept.** Its caption, rows and tiles are made once; while
  something is typed it is hidden (`Tree::set_hidden`: no room taken,
  neither drawn, hit nor read, its text still laid out), and it shows
  again as it was. Scrolling changes only the names and icons of the
  tiles that show another application. Opening again on the grid as it
  was left changes nothing, and draws nothing.
- **The closed card is drawn as it opens.** A second after what it shows
  changed (it closed, the applications, their icons, the theme, the
  language or the system's fonts), the card is put back on its empty
  field and drawn, off screen, into its buffer, on the screen under the
  pointer. No alarm of its own wakes the loop for it: the first turn after
  that second does it (the watchdog's, each second in a session): nothing
  wakes a desktop at rest for it. Opening there then draws nothing
  new: its first frame is the frame alone.
- **A shadow hidden under an opaque box is left out** when only the
  inside of the box is drawn again (a result typed, a tile chosen): the
  box would cover it whole. The pixels are the same.

## Panels' shadows, #281

Panels cast windows' shadow, drawn by the compositor under their surface
from the textures windows' shadows use (a Gaussian drawn once per scale,
corner radius and light, and per size for a panel too short for the shared
one), no longer by vello_cpu in the panel's own buffer. A panel drawn whole
draws no blurred rectangle any more; the compositor draws a few more pieces
on each frame it shows. The shadow's texture is written in place: 0.5 ms
for the launcher's at scale 2 instead of 4.8 ms, for windows' too, and the
closed launcher card's is made as the card is drawn ahead of time (#242).
The same machine, loaded (other builds running), the shadows drawn in the
surface or by the compositor alternately, release, twelve openings each,
three runs of each, the least of each run:

| Measure | In the surface | By the compositor |
|---|---|---|
| The launcher's card drawn whole by the toolkit (scale 2, 60 applications) | 9.9–13.8 ms | 2.7–3.7 ms |
| Opening the launcher, to its first frame (pixman) | 5.7–5.9 ms | 7.0–7.7 ms |
| Opening it again right after a search | 19.6–23.3 ms | 13.9–14.5 ms |

The first opening, drawn ahead of time, pays for the pieces pixman
composites on its first frame (a GPU stretches and mirrors them for
nothing); every other opening, where the card is drawn on the loop, is
drawn without its shadow. The twenty windows opening hold 10.02 MB as
before, with 194,748 allocations instead of 194,755. Their desktops
switching hold 0.85 MB more: the shadows of the panels the switches show
are three textures, kept for the next switch as windows' are.

## Reproducing

```sh
# the counted budgets: at rest (60 s on a virtual clock, then 60 s of real
# time), and the twenty windows' scenario
cargo nextest run -p hocus-test --test budgets --no-capture
cargo nextest run -p hocus-test --test scenarios --no-capture budget-twenty-windows
# the timings, as CI's job runs them
cargo nextest run --release -p hocus-test --test timings --run-ignored only --no-capture
# a running compositor's counters
target/debug/hocus msg --socket <control socket> stats
```

The scenario steps `measure` and `expect_cost` count what the compositor
does between two steps; `expect_cost` prints what it counted, which is
what a budget is set from ([scenarios](../tests/scenarios/README.md)).

Profiles are taken in the Debian VM only, never on a developer's machine
(perf needs `kernel.perf_event_paranoid` lowered):

```sh
cargo build --workspace --bins
scripts/vm/debian.sh up && scripts/vm/debian.sh copy
scripts/vm/debian.sh ssh 'sudo apt-get install -y linux-perf binutils && sudo sysctl kernel.perf_event_paranoid=1'
scripts/vm/debian.sh ssh '~/hocus-vm session'
# a minute at rest: what woke the loop (two `stats` a minute apart), its CPU time
scripts/vm/debian.sh ssh '~/hocus-vm msg stats; perf stat -e task-clock,context-switches -p $(cat ~/hocus.pid) -- sleep 60; ~/hocus-vm msg stats'
# a profile while the mouse moves (`scripts/vm/debian.sh mouse 6 0` in a loop meanwhile)
scripts/vm/debian.sh ssh 'perf record -g -p $(cat ~/hocus.pid) -o /tmp/perf.data -- sleep 15'
scripts/vm/debian.sh ssh 'perf report -i /tmp/perf.data --no-children --no-inline -g none --sort sym --stdio | head -40'
scripts/vm/debian.sh down
```

The VM runs alone: nothing is built while it is up.

The profile of the session in the VM, the mouse moving for 15 s after
these changes, is flat: no function of the compositor's takes more than
2 % of its samples; a quarter is the kernel (scheduling the threads), a
tenth Mesa's software rasterizer.

## Not yet

- **A redraw is asked for every screen.** `needs_redraw` is one flag:
  a change on one screen builds the scene of every screen (when twenty
  windows open, 320 scenes on each screen for 160 and 150 frames; the
  column's minute builds the second screen's too).
- **The column's state is worked out after every turn** (15 of the 29
  allocations at rest), then compared: an event-driven dirtiness would
  make a turn at rest allocate nothing.
- **The dock's picture is made again for each new app**, buffers
  included: most of the 376 MB asked for while twenty windows open.
- **Opening the launcher where it was not drawn as it opens draws its
  whole card on the loop**: within a second of closing it, on another
  screen than the pointer was on, or in another mode (the history, emoji,
  the palette, the notes). Its shadow is no longer drawn with it (#281),
  about two thirds of it; the rest is the card itself.
- **Night light ticks every 30 s while on**, and asks for a redraw each
  time; the annotation window still damages all of itself on each draw.
- **smithay's `Output::name()` copies the name** at every call, and
  outputs are found by name on paths taken after every turn.
