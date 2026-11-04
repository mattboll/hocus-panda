# Idle, held by applications and full screen (7.3)

Status: a technical ticket (issue #414), built 2026-11-04. It is the core
that the lock screen (7.1, #412) and locking on idle (7.2, #413) will use;
it shows nothing by itself. Video players and calls keep the screen on, and
the countdown starts again when they let go.

## What it does

- **A countdown since the user's last input.** Any input from a device
  counts: keys, pointer motion, buttons, scrolling, touchpad gestures.
  Its length is `[idle] delay` in the configuration, in seconds, from 1 to
  86400, or `"off"`. It is off by default until 7.2 picks a delay:

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

## For 7.2: where to hook

`State::idle_event(Idle)` in `crates/hocus-compositor/src/idle/mod.rs` is
called on the loop when idle is reached (`Idle::Reached`) and when the
user comes back after it (`Idle::Resumed`). For now it only logs. The lock
goes there, and the delay 7.2 chooses becomes `[idle] delay`'s default
(`packaging/config.toml` lists it).

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
  compositor does not read logind over D-Bus (it takes its seat through
  libseat). Reading them is left for when it does, read only.
- **The desktop portal's `Inhibit`** (`org.freedesktop.portal.Inhibit`,
  used by Flatpak applications through xdg-desktop-portal) needs the
  portal backend's `org.freedesktop.impl.portal.Inhibit`. Not served yet.
- **The screen saver's other methods** (`GetActive`, `Lock`,
  `GetSessionIdleTime`): nothing locks yet.
- **What being idle does** (blanking the screens, locking): 7.1 and 7.2.

## Tests

`crates/hocus-compositor/tests/idle.rs`, headless, on the held clock,
with test clients and a private `dbus-daemon`:

- `idle_is_reached_after_the_delay`
- `input_starts_the_countdown_again`
- `off_by_default`
- `a_shown_surfaces_inhibitor_holds_it_and_a_hidden_ones_does_not`
- `the_countdown_starts_from_zero_when_the_last_hold_goes`
- `the_focused_fullscreen_window_holds_it`
- `a_screensaver_inhibit_holds_it_until_uninhibit`
- `a_screensaver_inhibit_ends_when_its_caller_leaves_the_bus`
- `hocus_msg_idle_says_who_holds_it`
- `idle_notifications_honour_holds_except_input_ones`
- `at_rest_one_timer_and_no_wakeup_beyond_it`
