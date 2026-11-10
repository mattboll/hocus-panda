# The lock screen (7.1)

Status: approved 2026-11-04 (issue #412, variant A, mockup
<https://claude.ai/artifact/JFALZsj1Fp9hfYXdWsNCvc>); built 2026-11-04. The
approved spec is the issue's last comment; this page says how it is built
and what was decided on the way.

## What locks, and what it guarantees

**The entry point** is `State::lock_now()` (crate `hocus-compositor`,
`src/lock/mod.rs`). 7.2's idle (as the curtain ends, docs/design/idle.md)
and 7.4 (the session menu's "Verrouiller", `Super+L`) call it; the shell
can ask it too (`lock now` on its line, `ToCompositor::Lock(LockAsk::Now)`),
and so can `hocus msg lock` on the control socket (any program of the
session; a lock is no privilege). Locked already, it changes nothing.
`State::lock_at_once()` locks without the fade nor the picture of the
desktop: before the machine sleeps (7.2).

Its state, `State::lock_state()`:

| State | Means |
|---|---|
| `Unlocked` | The desktop as usual |
| `Locked` | From the `lock_now()` call on: no client gets input; from the next frame on, none is drawn |
| `Opening` | The right password (or an external locker's unlock): the session is open again, input back where it was, the lock's halves sliding apart over it (400 ms) |

`State::locked()` is true in `Locked` only. `State::lock_drawn()` says
every screen has drawn the lock, its fade over: nothing of the session
shows any more. `State::lock_presented()` says more, what 7.2 waits for
before letting the machine sleep: every screen that is on has presented
(its page flip done, `State::presented`) a frame of the lock itself, the
shell's surface fully in (or black when no shell is linked). A screen off
(7.2: idle, or the lid closed over it) is not waited for by either.

**As built in 7.2** (docs/design/idle.md, "Locking on idle and before
sleep"):

- Idle, the curtain dims every screen for 10 s, then the lock fades in
  over the curtain's last look: the picture of the desktop taken as it
  locks is drawn with the curtain at its end, so the screen never
  brightens between them.
- Locked, the screens go off 30 s after the lock or the last input, by
  whatever locked; input turns them on, still locked, and a key goes to
  the field.
- Before sleep, logind's delay inhibitor is held for the whole session;
  `PrepareForSleep(true)` locks at once (no curtain, no fade), and the
  inhibitor goes once `lock_presented()`, or 2 s after at most. Waking
  (`PrepareForSleep(false)`), the field holds the keyboard.

While locked:

- **Nothing of the session is drawn.** Every output's elements are the
  lock's alone (`State::elements_for` returns them first, whatever it draws
  for): the screen, a capture (`Print` taken as it locks), a share and a
  recording alike, which therefore show the lock or black, never the
  desktop. A window shared or recorded alone shows black. The lock's
  surface fades in over the desktop's last frame, a picture the compositor
  takes as it locks (the windows as they were, never live), on the screen
  only; a capture never has it.
- **No client gets input.** The keyboard goes to nobody: the compositor
  keeps every key and types the password into its own memory. The pointer
  goes to the lock's surface under it only (the shell's, whose clicks on the
  arrow, the field and the layout's button it reports); a press, a wheel,
  a gesture reach nothing else. Grabs, menus, Alt+Tab, the capture, the
  strokes end as it locks. A window that maps or asks for the focus while
  locked gets it only after.
- **The shortcuts** are the spec's only: the sound's and the brightness's
  keys (`[shortcuts]`' level keys), the keyboard layout (XKB's group
  switch, and the lock's own button), the virtual terminals
  (`Ctrl+Alt+Fn`). The suspend key and the lid are logind's, which the
  compositor does not take (its lid switch only turns the laptop's
  screen off, 7.2). Nothing else runs: no `Super` shortcut, no
  Alt+Tab, launcher, capture, recording, nor an application's key. There is
  no microphone mute key in `[shortcuts]` yet: nothing to let through.
- **The shell's line** carries only what draws: the lock's state, how many
  characters were typed (never them), Caps Lock, where `Tab` is, how many
  agents wait. What the shell asks that would show or move a window, start
  or copy anything is refused (`State::lock_lets`). The control socket
  takes agents' reports only, which show nothing but the count.
- **Frames** go to the lock's surfaces only: the windows and the bar wait,
  and the compositor draws nothing at rest.

## The password

- Typed into the compositor (`LockKey`): a character a key (`key_char` of
  the layout's keysym), dead keys through XKB's compose table of the
  user's locale (`ê` on AZERTY). Chords with Super, Ctrl or Alt type
  nothing. `Enter` and the arrow check; `Esc` empties the field; `Tab` goes
  from the field to the button, then to the layout's button (with a
  choice of layouts), where `Enter` or `Space` changes the layout. A letter
  goes to the field whatever `Tab` had. At most 512 bytes.
- It lives in a `Zeroizing<String>` whose buffer is reserved whole as it
  locks, so it is never moved by a reallocation and left behind; it is
  moved to the check's thread, zeroed there once checked (the C string PAM
  reads too); `Esc` and a check zero the field's.
- **PAM**, on a thread of its own, off the loop (`lock/pam.rs`): libpam is
  opened at run time (`libpam.so.0`, no headers needed to build);
  `pam_start` on the service, `pam_authenticate`, `pam_acct_mgmt`,
  `pam_setcred(PAM_REFRESH_CRED)`, `pam_end`. The conversation answers the
  first password prompt and nothing else; the modules' messages are taken
  (debug log), never shown. The user is the compositor's own account
  (`getpwuid_r`), never `$USER`. Without libpam, or a broken service, the
  check fails as a wrong password: the session stays locked.
- **The service** is `hocuspanda`, the package's `/etc/pam.d/hocuspanda`
  (`packaging/pam-hocuspanda`, a conffile): the system's `common-auth` and
  `common-account`, so that its own modules (LDAP, systemd-homed) work as
  at login. Without that file (a build run by hand), the system's `login`.
- **Too many attempts** is PAM's to say, not a count of ours: PAM refusing
  before it asked for the password (as pam_faillock's `preauth` does), or
  `PAM_MAXTRIES`. The field then waits 30 s, counting down each second,
  taking nothing, then takes a password again (and if PAM still refuses,
  another 30 s).
- **What PAM can count here** (tried in the Debian VM, 2026-11-04): the
  compositor runs as the user, not root, and pam_faillock keeps its tally
  in `/run/faillock` as root: run by the user it cannot open it ("Error
  opening the tally file"), so it counts nothing and never refuses. With
  Debian's stock stack the wait therefore never comes; a wrong password
  costs pam_unix's own delay (about 2 s, "Vérification…" meanwhile), each
  time. A count of the lock's own, if wanted, is a choice to make (see
  the pull request).

### The test authenticator

The compositor's tests never use PAM nor anyone's password:
`Authenticator::test("panda")` (`lock/auth.rs`) takes that password and,
after three wrong ones in a row, refuses the next attempt without asking
for it, as pam_faillock's `preauth` does where it can count. It exists
only when `hocus-compositor` is built with its `test-auth` feature, which only the crate's own tests turn on (a dev-dependency of the
crate on itself, `crates/hocus-compositor/Cargo.toml`); a build of the
program (`cargo build --release -p hocus-compositor`, the package's) has
none of it: `Config::lock_auth` is then `Pam` only. `HOCUS_TEST_AUTH=…`
names a test password for a compositor run by hand (screenshots) in such a
build only, and `packaging/deb/build.sh` refuses a binary that holds that
name.

## The shell's surfaces, and why not ext-session-lock for them

The shell draws the lock on layer surfaces (namespace `hocus-lock`, one
per screen, the overlay layer, all of the screen, never the keyboard).
The compositor shows those of the shell's own Wayland connection only:
it makes that connection itself as it starts the shell (a socket pair,
`WAYLAND_SOCKET`), so no other client can claim to be the shell, and
another client's `hocus-lock` surface is never drawn, locked or not.

Why not ext-session-lock-v1 for the shell's own lock:

- the unlock is the compositor's decision, from PAM's answer on its own
  thread: a shell (or any process that reaches its line) can never open it,
  only report clicks; with ext-session-lock the locker client decides;
- the password never leaves the compositor's process: the shell, its
  toolkit's text field and its line never hold it;
- a shell that crashes leaves the lock as it is (the protocol would leave
  the session locked with no locker), and the restarted shell draws it
  again as it connects;
- iced_layershell draws layer surfaces; a session-lock surface is another
  toolkit's event loop.

**Another locker** (swaylock, gtklock) may lock through ext-session-lock-v1
(`lock/external.rs`), with the same guarantees: its surfaces alone shown
(black on a screen without one), the keyboard to its surface under the
pointer, the allowed shortcuts kept by the compositor, `locked` sent once
every screen drew the lock. It cannot be gone round: locked already (by
the desktop or another locker), a new lock is refused (`finished`); only
the holder's own `unlock_and_destroy` opens it (smithay calls the unlock
for any lock object's, a refused one's too, without saying whose: the
compositor takes it after the turn, only when the holder's lock object is
gone while its client is still connected); the locker crashing leaves the
session locked under the desktop's own lock, whose password opens it.

## A shell crash

The shell's lock surfaces go with it: those screens show black, the
session locked, nothing reaching anyone. The compositor starts the shell
again (its usual back-off, from 500 ms) and tells it the lock as it
connects; its lock fades in from black.

## What the shell draws (variant A)

Sizes in logical pixels (`crates/hocus-shell/src/lock/draw.rs`):

- **The background**: each screen's wallpaper (6.3, `[wallpaper] lock`:
  `"dimmed"` by default, `"blurred"`, or `"picture"`, `lock-picture`),
  drawn with the compositor's function (`hocus_link::wallpaper::render`)
  on a thread of the shell's (`lock/wallpaper.rs`), `rgba(5,7,12,0.45)`
  over it. "Blurred" is a box blur made once on a small copy, never a
  live blur of the screen: the lock never shows windows.
  It is a canvas of its own under the rest: iced draws a canvas' images
  over its shapes.
- **The clock**: `HH:MM`, Martian Mono 400, 112 px, `text`, in cells as
  wide as a `0` (the colon 0.55 of it); the date 16 px under, Manrope
  22 px, `text.dim`; its top 330 px above the middle, 90 px on the other
  screens.
- **The form** on the bar's screen (the primary, bar.md §2), 320 px wide,
  its top 110 px above the middle: the avatar (88 px, the accent's
  gradient, a 2 px `rgba(255,255,255,0.18)` ring, a soft shadow drawn as
  fading rings, the panda's face of 66 px; or the account's photo), the
  name 14 px under (Manrope 700, 20 px; GECOS, else the login), the field
  20 px under (320 × 44, radius 12, `rgba(255,255,255,0.08)`, its 1 px
  border green with the keyboard, orange when wrong, faint while checking,
  the field at 55 % while waiting), a dot a character (3.5 px, 10.6 px
  apart; the last ones when they do not fit), the Caps Lock sign, the
  34 px button with its arrow (or its spinner while checking), the line
  12 px under (13.5 px on 1.45, 40 px kept).
- **The agents waiting**: a count, no name, C's pill (13 px, 600, `#F5C46E`
  on `rgba(245,180,74,0.10)`, a 34 px row; the waiting panda, 34 px, 6 px
  in, in place of its dot since #649, playing its mood as the lock
  shows), centred 12 px
  under the line's room: "2 agents t'attendent" / "2 agents are waiting
  for you". Without one, nothing.
- **The status row**, bottom right (32 px in, its centre 44 px up, 18 px
  apart): the layout's code in its 44 × 32 button (with a choice of
  layouts), the battery's sign and percentage in its band's colour (with
  a battery), the Wi-Fi's sign (with NetworkManager).
- **Two pandas asleep** in the bottom left corner: the validated resting
  drawing (#649), 132 and 108 px, at 100 %, the second 12 px into the
  first's box and turned 14° back; their "z" are the drawing's. As the
  lock shows they breathe twice, their "z" rising (8 s), then lie still:
  the lock draws once a minute again. Still with reduced motion, in power
  saver or on the CPU.
- **Motion**: the fade in, 250 ms out-cubic (120 ms with reduced motion or
  on the CPU), by the compositor over its frozen frame. The shake, 360 ms
  (10, 9, 6, 4 px), by the shell, on a GPU without reduced motion only. The
  spinner turns on a GPU only. The opening, 400 ms in-out, the halves
  sliding apart, by the compositor (its two halves of the shell's surface),
  at once with reduced motion or a shell on the CPU.
- **At rest** a lock surface draws once a minute, as the minute changes;
  the bar's clock draws nothing while locked and starts again as it
  unlocks; new notification bubbles wait for the unlock.
- **Several screens**: one surface each, the form on the primary's, the
  clock alone on the others; a screen plugged in while locked gets its
  surface, and the primary unplugged moves the form to where the bar goes.

### The avatar's photo

The account's photo from AccountsService (`org.freedesktop.Accounts` on
the system bus, `FindUserById`, the user's `IconFile`), asked once as the
shell starts, on a thread with a 5 s limit; PNG only (the codec the shell
has; GNOME's Settings saves PNG), cut square from its middle, 176 px.
Without it, the panda.

## Choices made here (to confirm on the machine)

- The agents' pill in variant A: under the form's line, centred.
- Typing a character after `Tab` moved to a button puts the keyboard back
  on the field.
- On the CPU (tiny-skia), a lock surface is a whole screen drawn again at
  each key: a key's dot shows a moment later than on a GPU.
- The status row's Wi-Fi is the bar's sign (its bars, slashed when off);
  no sign without NetworkManager.

## Tests

`crates/hocus-compositor/tests/lock.rs` and `lock_external.rs`, one per
point of the spec, headless with pixman, the real shell on the CPU where
it draws:

| Test | Checks |
|---|---|
| `locked_no_window_is_drawn_and_no_input_reaches_a_client` | Black but the cursor, no window nor a fake `hocus-lock` surface; no key, motion, click to a client; Alt+Tab, Super+Space, Print, Super+Return do nothing; after the unlock the window's first key is the next one |
| `a_capture_while_locked_shows_no_desktop` | A capture (screen, window) asked as it locks is black; `Print` takes none while locked |
| `a_wrong_password_then_the_right_one` | Wrong: emptied, shaken once, the shell draws it; the right one opens it; no key reached the window |
| `caps_lock_shows_as_it_locks_and_from_the_first_key` | On as it locks, off and on at once as keys come |
| `too_many_attempts_wait_30_seconds_then_the_field_again` | PAM's refusal after three wrong: 30 s counting down on the compositor's clock, nothing typed, then the field |
| `a_second_screen_shows_the_clock_alone` | The form on the bar's screen, the clock alone on the other |
| `a_shell_crash_keeps_the_session_locked` | The shell killed: locked, black, no key to a window; back, it draws the lock, which opens |
| `unlocking_gives_the_desktop_back` | The shell told, its surfaces gone, the window drawn and holding the keyboard, the bar's clock drawing again |
| `at_rest_while_locked_nothing_wakes_but_the_minute` | No frame, no commit of the lock or the bar at rest; the minute draws the lock once |
| `hocus_msg_lock_locks` | `hocus msg lock` locks, answering once locked |
| `tab_goes_round_and_the_layout_switches_while_locked` | `Tab`, `Shift+Tab`, `Enter` on the layout's button |
| `a_locker_locks_with_its_surfaces_and_its_unlock_opens` | ext-session-lock: its surfaces alone, `locked`, its unlock |
| `nothing_goes_round_a_lockers_lock` | A second locker refused, its unlock opening nothing; the locker's crash leaving the desktop's lock, which the password opens |

The screenshots of the pull request: `tests/lock_shots.rs`
(`HOCUS_SHOTS=DIR cargo nextest run -p hocus-compositor lock_shots::
--run-ignored only`).
