# Notifications — the server and its bubbles (5.3)

Status: approved by the maintainer on 2026-11-02 (issue #393, variant A:
at the top, against the bar); built in 5.3. The history, "clear all" and
do not disturb are 5.4; the bar's notifications button is 5.5.

All sizes are logical pixels.

## The approved spec

**The server**

- The desktop serves `org.freedesktop.Notifications` on the session bus:
  `Notify`, `CloseNotification`, `GetCapabilities` (body, actions,
  body-markup if cheap, persistence), `GetServerInformation`, and the
  `NotificationClosed` and `ActionInvoked` signals.
- It runs off the loop and sends owned events to it.
- Tests and nested runs use only an explicitly given bus (`--session-bus`),
  never the live session's.

**Bubbles**

- **Place**: right of the screen against the bar, top aligned, 340 wide.
  They stack downward with an 8 px gap, on the screen of the focused
  window.
- **Card**: radius 14, glass like the popovers, with a 32 px app icon (from
  the hint or the app's desktop entry, else a neutral square).
  - First line: the app's name, then "· maintenant" / "· now".
  - Then the title in bold 13.5 px, and the body at 12.5 px clamped to two
    lines.
  - A ✕ at top right.
  - The actions as small buttons under the body; `default` is the click on
    the card.
- **Progress**: the `value` hint shows as a progress bar.
- **Countdown**: normal urgency stays 6 s, or the app's `expire_timeout` if
  it gives one. A 2 px line at the bottom counts down. Hovering any bubble
  pauses all countdowns.
- **Critical urgency**: an orange border and glow, and an "Urgent" mark. It
  stays until closed or acted on.
- **Low urgency**: no bubble. It goes straight to the history (5.4), and a
  dot with the count shows on the bar's notifications place (its button is
  5.5).
- **Replacement** (`replaces_id`): the bubble updates in place with no new
  animation, and its countdown restarts.
- **Many at once**: at most 3 bubbles. The others wait their turn, shown as
  a "+N en attente" / "+N waiting" chip under the stack.
- **Motion**: in 220 ms sliding from the bar's edge, out in 170 ms. With
  reduced motion, a fade only.
- **Clicks**: a click on the card invokes `default` (if any) and raises the
  app's window; ✕ closes the bubble (reason 2); an action invokes it and
  closes.

**Agents' notifications**

- A notification from an agent tool that the agents group follows (4.0's
  sources) and that only says the agent waits or has finished does not make
  a bubble: the agent's row, its panda and the halo already show it. It
  goes to the history only.
- Its other notifications are normal bubbles.

## As built (5.3, 2026-11-02)

### The server

- **In the shell** (decision 0003: notifications are the shell's), on a
  thread of its own with its own runtime (`notifications::server`), as the
  levels' and the network's D-Bus clients are. A notification is read
  there (its hints, its icon, the process that sent it, whether it is an
  agent tool's) and handed, owned, to the bubbles' stack; the stack's
  answers (closed, acted on) go back to that thread, which emits the
  signals. Nothing waits for the other.
- **Which bus**: the compositor tells the shell in
  `HOCUS_NOTIFICATIONS_BUS`. `--session-bus ADDRESS` names one (any
  backend); the compositor also gives that address to the programs it
  starts (`DBUS_SESSION_BUS_ADDRESS`), so that a terminal started with
  `--spawn` sends its notifications there. Without it, only the session on
  a virtual terminal (`--backend tty`, `--session`) serves them, on the
  session's own bus; a nested or headless compositor serves none, and
  never takes the live session's.
- **The name**: requested with `REPLACE_EXISTING` (not `DO_NOT_QUEUE`):
  the desktop takes it from a server that lets it go, else waits in the
  queue for it.
- **`GetCapabilities`**: `body`, `body-markup`, `actions`, `persistence`,
  `icon-static`. The markup is taken out (`<b>`, `<i>`, `<u>`, `<a>`; an
  `<img>`'s `alt`), entities read: the bubbles show plain text.
  `GetServerInformation`: `Hocuspanda`, `Hocuspanda`, the version, `1.2`.
- **Ids**: from 1, one per notification; `replaces_id` keeps its id while
  that notification is not closed, else a new one is given. A shell
  restarted starts again from 1.
- **Timeouts**: `expire_timeout` -1 is 6 s, 0 never expires (no countdown
  line), a positive one is the application's. Critical urgency stays
  whatever it asks.
- **Hints read**: `urgency` (any integer width), `desktop-entry`,
  `image-data` (also `image_data`, `icon_data`; 8-bit RGB or RGBA, up to
  1024 px), `image-path` (`image_path`), `value`, `resident` (an action
  does not close it). `CloseNotification` always answers; an id not known
  is ignored.
- **The icon**: the image's data, else the image's path, else
  `app_icon` (a path, a `file://` address or an icon's name), else the
  desktop entry's `Icon`, else a desktop file named as the application;
  icons looked for as the sound's applications' are (`hicolor`, then
  `pixmaps`). None: a neutral square (radius 9, `rgba(255,255,255,0.10)`, a
  1 px `rgba(255,255,255,0.14)` rim).

### Agents' notifications

A notification makes no bubble, goes to the history and is not counted
on the bar when both are true:

- **It is a followed tool's**: Claude Code, Codex or herdr, followed by the
  agents' group (plugged in, or herdr running and followed), and the
  notification names it — its application's name, its desktop entry or its
  title is the tool's (`Claude Code`, `claude`, `claude-code`; `Codex`,
  `codex`; `herdr`: Claude Code titles the notifications its terminal shows
  "Claude Code") — or it was sent by the tool's process or one below it (a
  hook's `notify-send`, herdr's).
- **It only says the agent waits or finished**, as the tools word it:
  "waiting for your input", "is waiting", "needs your permission", "needs
  permission", "needs your input", "needs your attention", "needs
  attention", "approval requested", "wants to edit"; "agent turn complete",
  "turn complete", "has finished", "is done", "finished". One that says it
  failed, or an error, is a bubble.

Codex's terminal notifications carry no name of Codex's: they are bubbles.

### Bubbles

- **Surfaces**: one layer surface per bubble (namespace
  `hocus-bubble-N`), on the top layer, anchored to the screen's top right;
  the chip (`hocus-bubbles-waiting`) and the bar's dot (`hocus-bar-unread`)
  have one each. They take no keyboard.
- **Place**: the stack's screen is the focused window's (else the
  pointer's), as the compositor says it (`focus OUTPUT`), taken when the
  first bubble of a stack shows; the next ones join it there until it is
  empty. Its cards' right is 8 px from the bar (or from the screen's edge
  on a screen without a bar), and moves when the bar changes form.
- **Card**: as the mockup: the tint `rgba(22,27,42,0.95) →
  rgba(14,17,28,0.97)` (opaque without glass), a 1 px
  `rgba(255,255,255,0.10)` rim, radius 14; padding 12 px, 34 px on the
  right for the ✕; the 32 px icon, 11 px before the text's column (249 px
  wide). First line 11.5 px: the name bold in `text.dim` (cut to fit),
  "· maintenant" in `text.faint` 6 px after it, a critical one's
  "Urgent" bold `#F7A77A` 10 px after that. The title bold 13.5 px on 1.3
  lines, two at most (cut with an ellipsis); the body 12.5 px `#C9D0DC` on
  1.4 lines, two at most, cut with an ellipsis; 3 px between them. The
  progress 4 px under the body, 5 px, `rgba(255,255,255,0.10)` and
  `#8FB3FF`. The ✕: a 24 px circle 8 px in, two strokes, `text.faint`
  (`text` on `rgba(255,255,255,0.10)` under the pointer). The actions:
  28 px buttons from the text's left, 12 px in, 6 px apart, radius 8, a 1 px
  `rgba(255,255,255,0.14)` rim on `rgba(255,255,255,0.06)` (`0.12` under
  the pointer), 12 px semibold; on another row when one does not fit. The
  card is lit `rgba(255,255,255,0.03)` under the pointer, the pointer a
  hand.
- **Shadow and glow**: on a GPU the surface reaches 60 px left of the card
  and 74 px under it (8 px above and on the right: the screen's top and
  the bar's gap) for the mockup's `0 14px 40px rgba(0,0,0,0.42)`, and a
  critical one's `0 0 24px rgba(242,148,91,0.25)`, drawn as SVG filters
  outside the card only, as the peek's; the surface takes the pointer on
  the card only. The compositor does not blur behind it: at 95 % its tint
  hides what a blur would show. On the CPU the surface is the card alone,
  opaque, without shadow, and the compositor cuts its corners.
- **The countdown**: a 2 px line 14 px from the sides on the card's
  bottom, `rgba(108,194,135,0.7)` (`rgba(255,255,255,0.35)` while paused).
  The bubble draws itself again as it shortens, a pixel at a time and 30
  times a second at most; nothing draws while the countdowns are paused or
  without one. The pointer on any bubble pauses them all; they go on from
  where they were when it leaves.
- **Motion**: the compositor moves each bubble from its first frame: in,
  sliding 26 px from the bar's side and fading in over 220 ms
  (`cubic-bezier(.2, .8, .2, 1)`); out over 170 ms (`ease-in`) once the
  shell says `closing hocus-bubble-N`, after which the shell removes it.
  With reduced motion, or a shell on the CPU, it fades only. A bubble
  updated in place commits again and does not move. The ones under a
  bubble that went move up once it is gone, and the next waiting one comes
  in.
- **The chip**: under the stack, 8 px under the last card, right-aligned
  with the cards: "+N en attente" / "+N waiting", 11.5 px semibold
  `text.dim`, 4 px and 10 px in, radius 10, `rgba(14,17,28,0.9)` and a 1 px
  `rgba(255,255,255,0.12)` rim.
- **Clicks**: the card asks the compositor for its application's window
  (`raise PID APP`: the window whose application's id is the desktop
  entry's, else the application's name, else the window of the process
  that sent it or of its nearest parent with one, such as the terminal
  that ran `notify-send`; to its desktop, focused), invokes `default` if
  it has one, then closes (2). An action invokes its key, then closes (2).
  A `resident` notification stays after either. ✕ closes (2), the time up
  closes (1), `CloseNotification` closes (3), a waiting one too.
- **Low urgency** goes to the history (kept for 5.4, 200 at most), and the
  dot counts it: on each bar, a 14 px `#8FB3FF` pill with its count in
  9 px bold `#0B1020`, where the notifications button's badge goes. Until
  5.5 draws the button, its place is assumed: in the mini form, a 24 px
  bell centred in the notifications' place under the keyboard layout's
  button (bar.md 3.8); in the full form, the footer's second item of four
  (§4 group 11). The dot's right is 4 px past the bell's, its top 3 px over
  it.
- **A replacement** that is of low urgency, or an agent's, takes the
  bubble it replaces away (without a signal) and goes to the history.

### Not built

- The history, "clear all", do not disturb (5.4); the bar's button (5.5).
- The keyboard: the bubbles take none (`Tab` does not reach them), and
  names for screen readers (bar.md §11).
- Sounds (`sound-file`, `sound-name`), `action-icons`, the
  `ActivationToken` signal, markup shown as styled text.

### Measured headless

The real shell on a D-Bus daemon of the test's own
(`crates/hocus-compositor/tests/notifications.rs`), on the CPU as in CI:

- a bubble shows 340 px wide, its top 8 px from the screen's, its right
  8 px from the full bar, and goes after its time (closed, 1);
- the pointer on it pauses it past its time; away, it goes after what was
  left;
- a critical one stays past its time, and its ✕ closes it (2);
- a low one makes no bubble, and the dot shows on the bar;
- a replacement keeps its id and its surface, its countdown starting
  again;
- five at once: three bubbles 8 px apart, the chip under them saying 2;
  one closed, the next shows; one waiting closed by its application (3),
  the chip goes;
- an action invokes its key and closes; the card invokes `default` and
  closes; the ✕ closes without an action; `CloseNotification` closes (3);
- Claude Code plugged in: "Claude is waiting for your input" titled
  "Claude Code" makes no bubble and no dot; "Login successful" makes one.

Unit tests for the stack (the rules above), the card's layout, the
surfaces' places, the markup, the timeouts, the actions, the agents'
rule, and the compositor's motion curves. On the Intel GPU, the
screenshots of the pull request.
