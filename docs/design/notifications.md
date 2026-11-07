# Notifications — the server, its bubbles, the history, do not disturb

Status: the server and its bubbles approved by the maintainer on
2026-11-02 (issue #393, variant A: at the top, against the bar), built in
5.3; the history, "clear all" and do not disturb approved on 2026-11-02
(issue #394, variant B: by time), built in 5.4 ([below](#54-the-history-clear-all-do-not-disturb)).
The footer's four buttons, the bell among them, are 5.5 (built:
[bar.md](bar.md), as built, 5.5).

All sizes are logical pixels.

## The approved spec (5.3)

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
  5.5). *The bell's badge replaced the dot in 5.4.*
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
- **The icon**, found as the launcher finds its icons (5.1: the user's
  icon theme and those it inherits, then `hicolor`, then `pixmaps`, drawn
  at the card's 32 px on the screens' scale), on the server's thread:
  `app_icon` (an icon's name, a path or a `file://` address), else the
  image (`image-path`, then `image-data`), else the `Icon` of the desktop
  entry the `desktop-entry` hint names among the launcher's applications
  (else the icon named as that entry), else the icon of the application
  whose name is the notification's application's. None: the launcher's
  letter tile, the application's first letter in white on its neutral
  gradient (`#5C6576 → #9AA3B4`, a 1 px `rgba(255,255,255,0.14)` rim),
  radius 9, the letter 13 px bold.

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
  have one each. (5.4: the dot is now the bell, `hocus-bar-bell`, below.) They take no keyboard.
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
  closes (1), `CloseNotification` closes (3), a waiting one too. *5.4: the
  time up closes nothing any more, the notification staying held in the
  history (below).*
- **Low urgency** goes to the history (kept for 5.4, 200 at most), and the
  dot counts it (5.4 replaced the dot with the bell's badge): on each bar, a 14 px `#8FB3FF` pill with its count in
  9 px bold `#0B1020`, where the notifications button's badge goes. Until
  5.5 draws the button, its place is assumed: in the mini form, a 24 px
  bell centred in the notifications' place under the keyboard layout's
  button (bar.md 3.8); in the full form, the footer's second item of four
  (§4 group 11). The dot's right is 4 px past the bell's, its top 3 px over
  it.
- **A replacement** that is of low urgency, or an agent's, takes the
  bubble it replaces away (without a signal) and goes to the history.

### What the journal says (#710)

So that a bubble that did not show can be told apart from one that was
held, each notification's bubble is said at `INFO`, by its application
and id, never by its title or its text (the journal is read by others):

- the shell: `a notification's bubble shows` (its serial and screen);
  `held: do not disturb`; `held: the screen is shared`; `waits for the
  unlock`; `waits its turn` (`behind` the bubbles shown, its `place` in
  the queue); `a notification without a bubble: to the history only`
  (low, or an agents' tool's);
- the compositor: `a bubble is drawn` at its surface's first frame, by
  its namespace; and a `WARN`, `a bubble's surface went without ever
  being drawn`, for one the shell opened that never showed.

### Not built

- The history, "clear all", do not disturb (built in 5.4, below); the
  footer's buttons (5.5).
- The keyboard: the bubbles take none (`Tab` does not reach them), and
  names for screen readers (bar.md §11).
- Sounds (`sound-file`, `sound-name`), `action-icons`, the
  `ActivationToken` signal, markup shown as styled text.

### Measured headless

The real shell on a D-Bus daemon of the test's own
(`crates/hocus-compositor/tests/notifications.rs`), on the CPU as in CI:

- a bubble shows 340 px wide, its top 8 px from the screen's, its right
  8 px from the full bar, and goes after its time (closed, 1; since 5.4,
  nothing said: still held);
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
  "Claude Code" makes no bubble and no dot; "Login successful" makes one;
- the journal says each bubble shown (and drawn), waiting its turn,
  waiting for the unlock, or none for a low one, without its text; a
  bubble's surface that goes undrawn is said as a warning.

Unit tests for the stack (the rules above), the card's layout, the
surfaces' places, the markup, the timeouts, the actions, the agents'
rule, and the compositor's motion curves. On the Intel GPU, the
screenshots of the pull request.

## 5.4: the history, clear all, do not disturb

### The approved spec

Approved by the maintainer on 2026-11-02 (issue #394), variant B: by time.

**The history popover**

- It opens from the bell, the notifications place 5.3 put in the bar; 5.5
  turns that place into the footer's bell button. Like the other popovers:
  a hover of 150 ms or a click, 360 wide, at most 600 tall, anchored at
  the bar's bottom.
- **Header**: "Notifications" (15 px bold) and "Tout effacer" / "Clear
  all" (a ghost button, shown when there is something to clear).
- **Do not disturb card**: under the header.
  - A moon icon, "Ne pas déranger" / "Do not disturb", and its subtitle:
    "Les bulles s'affichent" / "Bubbles show" when off; when on, "Jusqu'à
    15 h 32" / "Until 3:32 PM", "Jusqu'à demain, 8 h" / "Until tomorrow,
    8 AM" or "Jusqu'à ce que tu l'arrêtes" / "Until you turn it off", then
    "· seules les urgences passent" / "· only urgent ones come through".
  - A switch on the right.
  - When on, the card turns amber and offers three chips: "Pendant 1 h" /
    "For 1 hour" (the default), "Jusqu'à demain matin" / "Until tomorrow
    morning" (8:00) and "Sans limite" / "No limit".
- **The list (variant B)**: everything in order of arrival, newest first,
  under section headers "RÉCENTES" (under an hour) / "RECENT", "PLUS TÔT
  AUJOURD'HUI" / "EARLIER TODAY", "HIER" / "YESTERDAY", then "PLUS
  ANCIENNES" / "OLDER".
- **Each item**: the app's 28 px icon (5.3's lookup); "App · il y a 9 min";
  a blue dot when it has not been seen; the bold title on one line; the
  body clamped to 2 lines; ✕ on hover.
  - A click invokes its default action (if the app still holds it) and
    raises the app's window, then removes the item.
  - ✕ removes it.
  - Actions show only while the sender still holds the notification.
- **Clear all**: the items slide out right one after another, 35 ms apart,
  in 180 ms. The empty state is the resting panda, "Rien de neuf" /
  "Nothing new" and "Tout est lu." / "All read.".
- **Scrolling**: the wheel, and the keyboard as the other popovers.

**The count**

- The bell's badge counts what the user has not seen: bubbles that left
  without the pointer resting on them or a click, low urgency, and
  everything held during do not disturb.
- Opening the history resets the count to 0 and marks the items seen when
  it closes.
- This replaces 5.3's dot.

**Do not disturb**

- While on, bubbles are held: they go straight to the history, except
  critical ones, which still show.
- The bell shows a slash in amber (`#F5C46E`).
- It ends by itself at the chosen time: an hour on the monotonic clock
  (an hour awake), the morning at the wall clock's 8:00, even after a
  suspend.
- It survives a restart of the shell, but not a new session.

**Keeping**

- The history keeps 7 days, written on disk in the user's state directory
  (`$XDG_STATE_HOME/hocuspanda/notifications`), off the loop.
- Agents' notifications that 5.3 sends straight to the history are there
  too.

### As built (5.4, 2026-11-02)

#### The bell

- **Its place, until 5.5** (since 5.5 the bell is the footer's second
  button in the full form, and over the user's avatar in the mini form,
  on the footer's surface, `hocus-bar-footer`, and a 150 ms hover opens
  the history in both forms: [bar.md](bar.md), as built, 5.5). A layer
  surface of its own over each bar
  (`hocus-bar-bell`, 52 × 44, anchored to the screen's bottom right),
  opened with the bar's other surfaces and after them, so that it is
  above them. Its button is 40 × 32, radius 10, centred where 5.3's dot
  assumed the bell: in the mini form on the bar's middle, its centre 22 px
  over the bar's bottom (under the keyboard layout's button); in the full
  form the footer's second place of four, its centre 29 px over the bar's
  bottom. The surface takes the pointer on the button only. The bell is
  drawn by a function of its own (`notifications::bell::draw`), for 5.5's
  footer to take with its place.
- **Its look**, the mockup's: the bell's 24 px path drawn at 20 px,
  `#C9D0DC`, 1.8 px; while the user is not to be disturbed, amber
  (`#F5C46E`) with the slash. The button is lit `rgba(255,255,255,0.08)`
  under the pointer, `0.10` while the history shows, and wears the
  keyboard's ring when it holds the keyboard.
- **The badge**: what was not seen, none at 0 and none while the history
  shows; 15 px high (wider for its figures, 3 px in), `#8FB3FF`, its count
  in 9.5 px bold `#0B1020`, a 2 px `rgba(14,17,28,0.95)` ring; its right
  5 px past the bell's 24 px box and its top 4 px over it.
- **It opens the history** as the other items open their popovers: a
  click (in both forms), a hover of 150 ms in the mini form; `Esc` gives
  it the keyboard, `Tab` and the arrows reach it from the bar's other
  items, `Enter` or `Space` opens the history again.

#### The history

- **The popover**: 360 wide, as tall as what it shows and 600 at most,
  its bottom on the bar's, 8 px from the bar; the bubbles' tint
  (`rgba(22,27,42,0.95) → rgba(14,17,28,0.97)`, opaque without glass), a
  1 px rim, radius 14.
- **The header**: 14 px in; "Notifications" 15 px bold; "Tout effacer" a
  28 px ghost button, 10 px in, 12.5 px 600 `text.dim` (`text` and
  `rgba(255,255,255,0.08)` under the pointer); 10 px under it, the card.
- **The card**: 14 px from the sides, 10 px and 12 px in, radius 12, a
  1 px rim; off `rgba(255,255,255,0.04)` and `0.08`, on
  `rgba(245,196,110,0.08)` and `rgba(245,196,110,0.35)`. The moon (18 px,
  `#A9B2C2`, amber while on), 10 px before "Ne pas déranger" (13.5 px
  600) over what it does (11.5 px `text.faint`, wrapped when long), then
  the switch: 38 × 22, `rgba(255,255,255,0.18)` off and `#F5B44A` on, a
  16 px white knob 3 px in, sliding over 160 ms (at once without motion).
  On, the chips 8 px under: 26 px, 10 px in, radius 13, 12 px 600, 6 px
  apart, on another row when they do not fit (the second and the third,
  in French); the chosen one `rgba(245,196,110,0.18)`, a
  `rgba(245,196,110,0.6)` rim and `#F5C46E`, the others
  `rgba(255,255,255,0.05)`, `rgba(255,255,255,0.14)` and `#C9D0DC`.
- **The list**: 8 px in, from 8 px under the card to 10 px over the
  bottom. A section's label 8 px in, 11 px bold capitals spaced 0.08 em,
  `#7C8698`; everything 2 px apart. An item: 8 px in (30 px on its right),
  radius 10, lit `rgba(255,255,255,0.05)` under the pointer; its icon
  28 px (the launcher's letter tile without one); 10 px after it, "App"
  (11 px bold `text.dim`, cut to half the column), "·" and how long ago
  (11 px `#7C8698`) 6 px apart, and the blue dot (6 px `#8FB3FF`) when not
  seen; the title 13 px bold on a line, cut; the body 12 px `text.dim` on
  1.4 lines, two at most, the last cut with an ellipsis. Its actions,
  while its sender holds it: 26 px buttons, 6 px under the body, 10 px
  in, radius 8, 12 px 600. Its ✕ under the pointer: 22 px, radius 6, 6 px
  from its right and 8 px from its top, two strokes in `text.faint`
  (`text` on `rgba(255,255,255,0.08)` under the pointer).
- **Sections and ages**, on the wall clock (the system's time zone; the
  clock's own time with `HOCUS_CLOCK_AT`): under an hour, "RÉCENTES",
  "maintenant" under a minute, then "il y a 9 min" / "9 min ago"; the
  same calendar day, "PLUS TÔT AUJOURD'HUI", "il y a 2 h" / "2 h ago";
  the day before, "HIER", "hier" / "yesterday"; earlier, "PLUS
  ANCIENNES", "il y a 3 jours" / "3 days ago".
- **A click** on an item invokes its `default` if its sender still holds
  it and has one, asks the compositor for its application's window (5.3's
  `raise`), then takes it out of the history (its sender hears it closed
  by the user, 2, if it holds it) and the history closes. ✕ takes it out
  (closed, 2, if held). An action invokes its key and takes it out
  (unless `resident`).
- **Clear all** takes everything out (each one still held closed, 2):
  with motion, the items slide 40 px right and fade, each over 180 ms
  (`ease-in`), 35 ms after the one above, from the first in view; then
  they go, and the empty state comes in (8 px up and fading in over
  260 ms). Without motion, or on the CPU, at once. ✕ slides its item out
  the same way. The empty state: the resting panda (140 px, at 100 %,
  #649), "Rien de neuf" (14 px 600 `#C9D0DC`) and "Tout est lu." (12 px
  `#7C8698`), 8 px apart, in a 264 px list. As the history opens empty,
  or empties, the panda breathes once as two "z" rise (4 s), then lies
  still; not with reduced motion, in power saver or on the CPU.
- **Scrolling**: past 600 px the list scrolls, the wheel 40 px a notch,
  a 6 px thumb on the right. `Tab` goes through "Tout effacer", the
  switch, the chips, then each item, its ✕ and its actions; the arrows go
  through the items; the list scrolls to what holds the keyboard;
  `Enter` or `Space` acts; `Esc` closes and gives the keyboard to the
  bell.
- **The panel draws** when it opens, when the history or do not disturb
  changes (resized then), and on the minute's change only if something
  else draws it: the ages do not tick by themselves.

#### What the history keeps, and the count

- **Everything** goes to the history as it comes, newest first: a bubble
  as it shows (or waits its turn), a low one, an agents' tool's (5.3's
  rule: not counted, shown seen), one held while the user is not to be
  disturbed. A replacement of a notification its sender holds updates it
  in place and comes first again. `CloseNotification` takes it out of the
  history too: its application took it back. 200 at most.
- **Held by its sender** (the server's `persistence`): from its `Notify`
  until it is closed. A bubble whose time is up closes nothing: its
  notification stays held, its actions and its default working from the
  history. It is closed (`NotificationClosed`, with its reason) when the
  user takes it out of the history, its ✕ or "clear all" (2), clicks it
  or one of its actions (`ActionInvoked`, then 2), or the bubble's ✕, card
  or actions (2); when its application calls `CloseNotification` (3);
  when it leaves the history after 7 days, or past the 200 kept (1); or
  when the session ends (the shell's connection to the bus goes). A
  notification read back after a restart of the shell is held by no one.
- **Not seen**, counted by the badge: a bubble whose time ran out without
  the pointer having rested on it (a hover pauses it, 5.3), a low one,
  one held while not to be disturbed. A bubble the pointer rested on, or
  clicked, closed with its ✕ or acted on, is seen. Opening the history
  sets the badge to 0; as it closes, what it listed is seen.
- **On disk**: `$XDG_STATE_HOME/hocuspanda/notifications` (else
  `~/.local/state`'s), one JSON object a line (the application, its desktop
  entry, what its icon was found by — `app_icon` and `image-path` —, the
  title, the body, the process, whether it is an agent's, when, whether it
  was seen), the newest first, readable by the user alone (0600), written
  whole (a new file renamed over the old). A thread reads it as the shell
  starts, leaves out what is older than 7 days and finds the icons again
  (once the launcher's applications and the screens' scales are known, a
  few seconds at most; an image sent as pixels is not kept: the letter
  tile then); the stack writes nothing until it is read back. Then the
  same thread writes it after each change, the latest of those sent
  meanwhile. The shell ends (`SIGTERM`, as a session that ends sends it,
  or its compositor gone) only once what it was told before is handled and
  written: the history closed just before has what it listed seen on disk
  too (#557). What is older than 7 days also goes as notifications come
  and as the history opens. A notification read back is held by no one:
  no actions, no default.
- **Only where notifications are served**: a shell given no bus (a nested
  or headless desktop without `--session-bus`) neither reads nor writes
  the history, nor do not disturb.

#### Do not disturb

- **The switch** turns it on for an hour; a chip chooses again from now:
  an hour, the next 8:00 ("Jusqu'à demain, 8 h", "Jusqu'à 8 h" before
  8:00), or no limit. An hour's end is on the monotonic clock: the wall
  clock read once for how long until the target. The morning's is the
  wall clock's 8:00 (#711): the monotonic clock stops while the machine
  sleeps, so its wait is read again from the wall clock as the machine
  resumes (logind's `PrepareForSleep`), when the clock is set, and on a
  `CLOCK_REALTIME` timer set to 8:00, which rings on resume past it. The
  stack's deadline wakes it at its end; it turns off by itself, the bell
  losing its slash.
- **While on**, a bubble of normal or low urgency is held: no bubble, to
  the history, not seen; a critical one shows. A bubble already on screen
  stays its time; a replacement of one on screen updates it.
- **Kept for the session**: written in the session's runtime directory
  (`$XDG_RUNTIME_DIR/hocuspanda/dnd`) with the compositor's process id
  (the shell's parent, which starts it again) and its end on
  `CLOCK_MONOTONIC`; read back by a shell started again by the same
  compositor, left off by another. Turned off, the file goes.

#### Measured headless

The real shell on a D-Bus daemon of the test's own, on the CPU as in CI,
its wall clock reading 2 November 2026, 14:32:07 UTC
(`crates/hocus-compositor/tests/notifications.rs`):

- the history lists, newest first, a low one just come (not seen) and
  three read back from its file under "RÉCENTES", "PLUS TÔT AUJOURD'HUI"
  and "HIER"; 360 wide, 8 px from the full bar, its bottom on the bar's;
- a click on a held item invokes its default, the sender hearing it
  closed (2), the history closes and no longer lists it;
- its ✕ takes one out; "clear all" empties it, the empty state's words
  and no "clear all" left, its file written empty;
- a bubble gone after its time closes nothing, and its action, invoked
  from the history, says `ActionInvoked` then closed (2);
- a bubble gone after its time counts 1, one the pointer rested on does
  not, a low one counts; opening sets 0, two not seen listed; opened
  again after closing, none;
- do not disturb from its switch: on for an hour, the bell slashed; a
  normal one held (no bubble, counted), a critical one shows;
- the history read back after the shell is ended and started again by
  the compositor, without what was older than 7 days, do not disturb
  still on.

Unit tests for the history (order, replacement, seen, file, 7 days,
sections and ages), do not disturb (its ends, a restart, another
session), the stack (the count, a click, clear all, do not disturb
ending at its time on the clock it is given, nothing written before what
was kept is read back), the panel's layout and words, the bell's place.
On the Intel GPU, the screenshots of the pull request.

#### Not built

- The footer's other buttons (5.5); the bell moves there with it. (Built
  in 5.5.)
- Hovering the bell in the full form opens nothing (only a click, as the
  full form's other items). (In 5.5, a 150 ms hover opens it in both
  forms, as the mockup has it.)
- Names for screen readers (bar.md §11); grouping by application
  (variant A).
- Images sent as pixels (`image-data`) are not kept on disk: after a
  restart such a notification shows the letter tile.

## 5.11: held while the screen is shared

While something is shared ([sharing.md](sharing.md)), bubbles are held as
with do not disturb (critical ones still show); when the last share ends,
one bubble sums them up ("3 notifications pendant le partage", "Slack (2),
Signal · dans l'historique"), its card opening the history. The sharing
popover says how many wait meanwhile. See sharing.md, as built.

