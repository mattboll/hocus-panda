# Idle, held by applications and full screen (7.3), and locking on idle and before sleep (7.2)

Status: 7.3 is a technical ticket (issue #414), built 2026-11-04: the
core that says when the session is idle. Video players and calls keep the
screen on, and the countdown starts again when they let go. 7.2 (issue
#413, approved 2026-11-04, variant A, mockup
<https://claude.ai/artifact/KPGJCSEhwRTPLq4rQ1Vo4A>) is what being idle
does: the curtain, the lock, the screens off; and the lock before the
machine sleeps. Built 2026-11-04; the approved spec is the issue's last
comment, this page says how it is built (below, "Locking on idle and
before sleep").

## What it does

- **A countdown since the user's last input.** Any input from a device
  counts: keys, buttons, scrolling, touchpad gestures, and pointer motion
  of 4 px or more all told (a mouse on a shaking desk moves less). Its
  length on AC is `[idle] delay` in the configuration, in seconds, from 1
  to 86400, or `"off"`; five minutes by default (7.2). On battery it is
  `[power.battery] idle` (below), three minutes by default:

  ```toml
  [idle]
  delay = 300
  ```

- **Idle is reached** when the delay passes with no input and nothing
  holding it off: `Idle::Reached` (below). The next input is
  `Idle::Resumed`, and the countdown starts again.
- **What holds idleness off**, each from the moment it starts:
  - **an idle inhibitor** (`zwp_idle_inhibit_manager_v1`) on a surface
    that is shown. A window is shown when it is not minimised and is on
    its screen's current desktop. A subsurface or popup counts as its
    window, and a layer surface counts as shown while it is on a screen.
    mpv, and Firefox for a video, use this;
  - **an `org.freedesktop.ScreenSaver` `Inhibit(application, reason)`**
    on the session bus, held until the matching `UnInhibit(cookie)` from
    the same caller, or until the caller leaves the bus (it quit or
    crashed). Chromium (videos, WebRTC calls), Firefox and VLC use this.
    It is served at `/org/freedesktop/ScreenSaver` and at `/ScreenSaver`.
    `SimulateUserActivity` counts as activity;
  - **the focused window in full screen**, while it shows.
- **When the last hold lets go, the countdown starts again from zero**,
  however long ago the last input was.
- **A hold keeps the session from becoming idle, but does not wake it once
  it is idle.** Only the user does that. A video that starts by itself
  after the session went idle does not undo the lock that 7.2 will add.
- **Clients' idle notifications** (`ext_idle_notifier_v1`, version 2):
  each has its own timeout. One made with `get_idle_notification` is not
  made idle while something holds idleness off. One made with
  `get_input_idle_notification` ignores holds and counts input alone. The
  countdown starts when the notification is made, or at the last activity
  if that is later. `idled` and `resumed` alternate as the protocol asks.
  Notifications work whether the session's delay is set or off.

## Asking it: `hocus msg idle`

`hocus msg idle` sends the line `msg idle` to the control socket
(`<socket>.control`, `HOCUS_SOCKET` for the programs the compositor
starts). The compositor answers with one `key value` per line, then closes
the connection:

```
state counting            # off, counting, held, idle
delay 300 s               # or: delay off
left 240.0 s              # while counting
held-by nothing           # or a line per holder:
held-by inhibitor org.mpv.mpv
held-by screensaver Firefox: video-playing
held-by fullscreen org.mozilla.firefox
```

An inhibitor names its window's app id, else its layer's namespace, else
its process. The tests read the same state through `Headless::idle()`.

`hocus msg idle` also says, after those lines, where 7.2 is:

```
away here                 # curtain locking, curtain, dark (screens off, unlocked)
screens on                # or: screens off eDP-1 HDMI-A-1
power ac                  # or: power battery (UPower's OnBattery)
sleep-inhibitor held      # or: none (logind's delay inhibitor)
```

## Where 7.2 hooks

`State::idle_event(Idle)` in `crates/hocus-compositor/src/idle/mod.rs` is
called on the loop when idle is reached (`Idle::Reached`), which brings
the curtain (`crate::away`), and when the user comes back after it
(`Idle::Resumed`), which is only logged: the user's input lifts the
curtain as it comes, before it reaches anything (`State::felt`).

## How it is built

- **One timer, on the loop, never a busy wait.** A single calloop timer
  is set for the nearest deadline: the session's, or a notification's.
  Input only records its time; it touches no timer, allocates nothing and
  wakes nothing. When the timer fires before a deadline that input has
  pushed back, it sets itself again for the new deadline
  (`TimeoutAction::ToDuration`, the same source). A deadline moved earlier
  (a hold letting go, a new notification) replaces the timer. With nothing
  to wait for (idle reached, held off, delay off and no notification), no
  timer is armed at all. At rest, the only wakeup is that timer firing at
  the deadline.
- **Holds are judged once per turn of the loop** (`State::flush_idle`,
  after dispatch, like the other flushes), without allocating: an
  `Inhibit` held, the focused window's mode, the inhibitors' surfaces.
  Only a change is acted on.
- **The bus off the loop.** `org.freedesktop.ScreenSaver` is served on a
  thread of its own with its own runtime, on the session bus where the
  notifications and the screen cast portal are served: the bus given with
  `--session-bus`, or the session's own bus on a virtual terminal. Nested
  or headless without `--session-bus`, it is not served, so it never
  touches a live session's bus. The thread answers calls itself, hands the
  loop the list of holds (owned) whenever it changes, and watches
  `NameOwnerChanged` to drop the holds of callers that leave.
- **Time is the motions' clock** (`State::now`), so a test that holds the
  clock moves the countdown with `Headless::step`: minutes pass in no
  time, the same on every machine.
- **ext-idle-notify is the compositor's own**, not smithay's. smithay's
  removes and re-adds a timer per notification on every input event; here
  notifications share the session's timer.

## Not done here

- **logind's `idle` inhibitors** (`systemd-inhibit --what=idle`): the
  compositor reads logind's sleeps over D-Bus since 7.2 (below), not its
  inhibitors' list yet.
- **The desktop portal's `Inhibit`** (`org.freedesktop.portal.Inhibit`,
  used by Flatpak applications through xdg-desktop-portal) needs the
  portal backend's `org.freedesktop.impl.portal.Inhibit`. Not served yet.
- **The screen saver's other methods** (`GetActive`, `Lock`,
  `GetSessionIdleTime`): not served yet.

## Tests

`crates/hocus-compositor/tests/idle.rs`, headless, on the held clock,
with test clients and a private `dbus-daemon`:

- `idle_is_reached_after_the_delay`
- `input_starts_the_countdown_again`
- `five_minutes_by_default_and_never_when_off`
- `a_shown_surfaces_inhibitor_holds_it_and_a_hidden_ones_does_not`
- `the_countdown_starts_from_zero_when_the_last_hold_goes`
- `the_focused_fullscreen_window_holds_it`
- `a_screensaver_inhibit_holds_it_until_uninhibit`
- `a_screensaver_inhibit_ends_when_its_caller_leaves_the_bus`
- `hocus_msg_idle_says_who_holds_it`
- `idle_notifications_honour_holds_except_input_ones`
- `at_rest_one_timer_and_no_wakeup_beyond_it`

## Locking on idle and before sleep (7.2), as built

The crate's `src/away/` module (`mod.rs` the timeline, `draw.rs` the
curtain on screen, `system.rs` the system bus); the shell's
`src/curtain/` (the pill).

### The timeline

- **Idle** (the countdown above: 5 min on AC, 3 min on battery) brings the
  **curtain**: every screen dims over 10 s (`hocus_link::CURTAIN`), the
  shell's pill on the screen under the pointer.
- **Its end locks** the session (`State::lock_now`), the lock fading in
  over the curtain's last look: the picture the lock takes of the desktop
  as it locks (7.1) is drawn with the curtain at its end, the dim and the
  pill, so the screen never brightens between them.
- **The screens go off** the power source's `screen-off-after` after the
  lock with no input (30 s on AC, 10 s on battery, 6.10), whatever locked
  it (`Super+L`, `hocus msg lock`, an ext-session-lock client): an input
  turns them on, still locked (a key goes to the lock's field), and they
  go off that long after it again, but never sooner than 10 s
  (`away::SCREEN_OFF_AFTER_INPUT`: a password is typed with the screen
  on).
- **What holds idle off** (an inhibitor on a shown surface, a screen
  saver `Inhibit`, the focused window in full screen) holds all of it: no
  curtain, so no lock and no screen off.

### The Settings' keys

The Énergie page's rows (docs/design/settings.md, 6.10), followed live
(`reload.rs`): a changed delay moves only the end of the countdown under
way, from the same activity.

| Key | Means | Default |
|---|---|---|
| `[idle] delay` | the dim on AC, seconds or `"off"` | `300` |
| `[power.battery] idle` | the dim on battery, `"3m"`… or `"never"` | `"3m"` |
| `[power.ac] screen-off-after`, `[power.battery] screen-off-after` | the screens off after the lock (after the dim, locking off) | `"30s"`, `"10s"` |
| `[power.ac] suspend-after`, `[power.battery] suspend-after` | the machine asleep after this long without activity | `"never"`, `"15m"` |
| `[power] lock-on-idle` | the session locks as the dim ends | `true` |
| `[power] lock-before-sleep` | the session locks before the machine sleeps | `true` |

With `lock-on-idle = false` the curtain has no pill (it would say
"Verrouillage dans 7 s"), stays dimmed once it ended, and the screens go
off `screen-off-after` later, unlocked (`away dark`). A value that does
not read takes its default, the rest of the file kept; a key not known
refuses the file. 6.1's keys still read: `[power.battery] screen-off` is
the battery's `idle` when that is absent; `[power.*] lock-after` says
`lock-on-idle` when that is absent (`"never"` on every source it names:
off; anything else: on, the session locking as the dim ends: 6.1's delay
after the screens went off is gone). Both are logged as the file is read
and never written again. The power source is UPower's `OnBattery`, read
by the compositor itself (below), not through the shell: on AC until it
says otherwise, and on AC without UPower. A countdown under way when it
changes keeps the time already spent (4 min of AC's 5 spent, unplugged:
the battery's 3 are over, the dim comes at once).

### The curtain (variant A)

- **The dim**, drawn by the compositor over everything but the cursor
  (windows, the bar, its popovers, every screen): `rgba(4,6,10,0)` to
  `rgba(4,6,10,0.74)` along `p^1.15`. It draws a frame a tenth of a second
  (`CURTAIN_FRAME`, through the motions' wake-up, no timer of its own),
  and with reduced motion a frame a second, the dim in steps.
- **The pill**, drawn by the shell on a layer surface of its own
  connection only (`hocus-curtain`, 300 × 64, anchored to the top of the
  screen the compositor names, its top at 64 % of the screen's height,
  centred across; it takes neither the pointer nor the keys): the glass
  (`rgba(22,27,42,0.86)` to `rgba(14,17,28,0.92)`, opaque on the CPU), a
  1 px rim, the panda (42 px) falling asleep, "Verrouillage dans 7 s" over
  "Bouge la souris pour rester", and the countdown's line. The shell
  draws it at most ten times a second (`request_redraw_at`), and nothing
  once it is gone. The compositor draws its shadow (the popovers'), fades
  and rises it 10 px in over 250 ms from its first frame (at once with
  reduced motion or a shell on the CPU, whose buffers are cut to its
  corners).
- **The panda falling asleep**: the lock's panda (its face of the
  mockup's 30-unit box), its eyes open for the first third, half closed
  for the second, closed with a "z" for the last; its head tilting from
  0° to 16° (none with reduced motion).
- **Lifted**: the dim and the pill fade out over 150 ms (`ease-in`), then
  the shell is told `curtain-end` and removes the pill.
- **Never in a capture**, a share or a recording.
- The line: `curtain ELAPSED LENGTH TOP motion|still OUTPUT`, and
  `curtain-end` (`hocus_link::CurtainShown`); told again as the shell
  connects while the curtain is up.

### What lifts it, swallowed

`State::felt` sees every input from a device (and the tests' own) before
it goes anywhere. While the curtain is up, or the screens are off with the
session unlocked, the input that lifts it goes no further: a press of a
key, a button or a gesture is kept from every client, its release too; a
motion moves nothing; a wheel scrolls nothing. The next input goes as
usual. A motion of less than 4 px all told since the last input that
counted is nobody's: it neither counts nor lifts. Locked, input goes to
the lock as usual (the screens come on with it).

### Before sleep

- **The system bus**, on a thread of its own with its runtime (zbus),
  never on the loop: the machine's on a virtual terminal, or the one
  given (`--system-bus ADDRESS`); nested or headless without one, none,
  so that a test never reaches the machine's logind. On it:
  - logind's `Inhibit("sleep", "Hocuspanda", …, "delay")`, taken as it
    starts (once `PrepareForSleep` is listened to) and again after each
    sleep: its file descriptor is held on the loop;
  - `PrepareForSleep(true|false)`, handed to the loop;
  - UPower's `OnBattery` and its display device's `Percentage` (when
    `IsPresent`), power-profiles-daemon's `ActiveProfile`, read as it
    starts, again on a `PropertiesChanged` under `/org/freedesktop/UPower`
    and when either comes back; only what changed goes to the loop;
  - logind's `Inhibit("handle-lid-switch:handle-power-key", …,
    "block")`, held for the session (6.10, below).
- **`PrepareForSleep(true)`**: the curtain goes, the session locks at
  once (`State::lock_at_once`: no curtain, no fade, no picture of the
  desktop), and the inhibitor is closed once every screen that is on has
  presented a frame of the lock itself (`State::lock_presented`: the
  shell's surface fully in, or black without a shell; a screen off counts
  as shown, nothing of the session being on it), or 2 s after at most
  (`away::SLEEP_WAIT`, logged). With `[power] lock-before-sleep = false`
  the inhibitor is closed at once, unlocked: the screen wakes on the
  desktop.
- **`PrepareForSleep(false)`**: a new inhibitor; the lock's field holds
  the keyboard; the waking counts as the user's input (the screens on, the
  30 s from then).
- **The lid**: libinput's lid switch, what `[power] lid` says (6.10,
  below); opened, the laptop's own screens (eDP, LVDS, DSI connectors)
  on again, as the user's input.

### The Énergie page's actions (6.10)

`src/away/power.rs`, on the loop; logind's calls from the session menu's
thread (`leave/logind.rs`), the rest from the system bus's (`system.rs`).

- **Sleeping after a while** (`suspend-after`): counted from the same
  activity as the dim, held off by the same holds (an inhibitor, an
  `Inhibit`, a focused full-screen window). Reached, logind's
  `ListInhibitors` is read: a `block` inhibitor of `idle` (a player's)
  puts it off, the countdown starting again from then; else
  `Suspend(false)`. Asked once until the next activity. The lock comes
  from `PrepareForSleep`, as for any sleep.
- **The lid**, once logind gave the inhibitor: `suspend` (logind's
  `Suspend(false)`, the panel off), `lock` (locked, the panel off),
  `screen-off` (the panel off, the session going on), `nothing` (the panel
  stays on). Docked (`lid-ignored-when-docked`, on AC, a screen that is
  not the laptop's on), only the panel goes off, whatever `lid` says.
  Without the inhibitor (no logind, or refused) logind decides, and the
  panel goes off as before.
- **The power button** (`KEY_POWER`), once logind gave the inhibitor: a
  press is taken (its release too) and does `[power] power-button`:
  `ask` opens the session menu on the bar of the focused window's screen
  (`session-menu OUTPUT` to the shell; nothing while locked), `suspend`
  is the menu's "Mettre en veille" (locked first, then logind's
  `Suspend(true)`), `power-off` asks every window to close and then calls
  logind's `PowerOff` as the menu does after its countdown (the password
  and the countdown first when polkit wants it), `nothing`. Without the
  inhibitor the key goes on as any other.
- **The automatic power saver** (`auto-saver`, `auto-saver-below`): on
  battery, the level at or under the threshold and the profile not the
  power saver, power-profiles-daemon's `ActiveProfile` is set to
  `power-saver`, the profile it replaced remembered, and one notification
  goes through the session bus's `org.freedesktop.Notifications`
  ("Économie d'énergie activée : batterie à 20 %."). Once per discharge:
  another profile chosen meanwhile is kept, and forgets the one replaced;
  back on AC the replaced profile comes back if the power saver is still
  on, and it may come on again on the next discharge.
- `hocus msg idle` adds:

  ```
  lid open desktop          # or: closed; system (logind handles it)
  level 82                  # the battery in percent, or: none
  profile balanced          # power-profiles-daemon's, or: none
  saver ready               # or: on PROFILE-REPLACED, spent
  ```

### The screens off

- On a virtual terminal, DPMS through DRM: the CRTC cleared
  (`DrmCompositor::clear`), nothing drawn; on again, its state reset and a
  frame drawn afresh, which lights it. Back from another terminal, a
  screen that was off is turned off again.
- Headless and nested, a screen off draws black (no element, not even
  the cursor); `State::output_powered` and `Headless::screens_on` say it.
- `wlr-output-power-management` is not served: nothing to tell.

### Choices made here (to confirm on the machine)

- A curtain that does not end in the lock shows the dim without the pill,
  held at its end until the screens go off (6.10).
- The lid is libinput's switch, not logind's `LidClosed`: the same
  switch, read where the compositor reads its input already.
- `lid = "nothing"` leaves the laptop's panel on.
- After an input while locked the screens stay on 10 s at least, whatever
  `screen-off-after` says ("Aussitôt" would turn them off at each key).
- On the lid closed, windows stay on the laptop's screen (the several
  screens' slice moves them).

### Tests

`crates/hocus-compositor/tests/away.rs`, headless, the clock held, a
private `dbus-daemon` as the system bus with a fake logind (its
inhibitors' other ends kept, to see them closed) and a fake UPower, the
real shell on the CPU where the pill is drawn:

| Test | Checks |
|---|---|
| `idle_brings_the_curtain_then_the_lock_then_the_screens_off` | The curtain at 5 min, its dim at 5 s, locked at 10 s, the screens off 30 s after; a key: on, still locked, typed in the field; off again; the password opens |
| `what_lifts_the_curtain_is_swallowed` | A click, a move, a key lift it and reach no window; the next click does; less than 4 px does not lift it |
| `on_battery_its_own_delay` | UPower on battery: 3 min; back on AC: 5 min |
| `an_inhibitor_or_a_focused_fullscreen_window_holds_it_all_off` | 20 min with either: no curtain, no lock, the screens on |
| `prepare_for_sleep_locks_and_lets_go_once_the_lock_shows` | The inhibitor held; `PrepareForSleep(true)`: locked without a curtain, the inhibitor closed once shown; the wake-up takes a new one |
| `before_sleep_the_shells_lock_is_waited_for_two_seconds_at_most` | The shell's lock on screen first; a held shell: let go at 2 s, not before |
| `the_lid_closed_docked_on_ac_does_not_lock` | The laptop's screen off, the other on, nothing locked; opened, both on |
| `at_rest_no_wakeup_beyond_the_idle_timer` | Nothing armed at rest but the idle countdown; a frame a tenth of a second while it dims; locked and off, nothing armed, nothing drawn |
| `the_shell_draws_the_pill_on_the_screen_under_the_pointer` | The pill on the pointer's screen, 300 × 64, centred, at 64 %; gone once lifted |

6.10's, `crates/hocus-compositor/tests/power_actions.rs` (a fake logind,
UPower and power-profiles-daemon on a private system bus, a fake
notifications server on a private session bus): the delays of each
source, plugging in keeping the time spent, locking off, sleeping after a
while (and held by a video or logind's `idle` inhibitor), the lid, the
power button, the automatic power saver, a delay changed on disk.

The screenshots of the pull request: `tests/away_shots.rs`
(`HOCUS_SHOTS=DIR cargo nextest run -p hocus-compositor --test away_shots
--run-ignored only`).
