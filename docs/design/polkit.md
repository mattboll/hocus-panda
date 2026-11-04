# The system's authentication dialog — specification (7.5)

Status: approved 2026-11-04 (issue #416, its last comment), variant A of
the 7.5 mockup: centred, every screen dimmed. Built since 2026-11-04: see
[as built](#as-built). Sizes are logical pixels (the mockup is drawn
×1.5).

## 1. What it is

- The desktop's own dialog when a system action needs a password
  (installing updates, formatting a disk, mounting a backup disk).
- The shell is the session's polkit authentication agent: nothing else
  in the session asks for an administrator's password.
- It shows who asks, why, for which account, the password, and the
  details folded.

## 2. Where

- On the screen the request comes from: the screen of the asking window,
  else the screen under the pointer.
- Every screen dims under it: `rgba(5,7,12,0.58)` over everything, the bar
  included, fading in over 160 ms (`ease-out`).
- The dialog holds the keyboard until it is answered: no shortcut but the
  sound, brightness and layout keys reaches anything else; a click
  outside it does nothing.
- Centred horizontally; its top at the screen's middle − 220 px, so that
  it grows downward when Details unfolds or the account chooser opens,
  and never jumps.

## 3. The dialog

- 380 px wide, radius 16, padding 20 / 20 / 18; glass denser than the
  popovers': `rgba(22,27,42,0.96) → rgba(14,17,28,0.97)`, rim
  `rgba(255,255,255,0.10)`, shadow `0 24px 60px rgba(0,0,0,0.50)`.
- **The header**: the asking application's icon in a 44 × 44 tile
  (radius 12), a 20 px badge at its bottom right (a `#6CC287` padlock on
  `#0b1020`, ringed 2 px in the dialog's colour: the desktop's sign that
  the system asks); 12 px after it, the application's name (Manrope 600,
  13 px, `text.dim`, cut) over "Autorisation nécessaire" (Manrope 700,
  17 px). The application: the polkit subject's process, matched to its
  window; its desktop entry's name and icon. No window: the program's
  desktop entry, else its name, and a generic gear.
- **Why**: polkit's message as sent (14.5 px on 1.5 lines, wrapped), 14 px
  under the header.
- **The account**, 16 px under: a 32 px round avatar (the user's panda on
  the accent's gradient `#6CC287 → #3E8FBF`), the name (14.5 px, 700) over
  "administrateur" (12.5 px, `text.faint`). With several identities, a
  "Changer" button on the right (30 px, radius 8); it unfolds a list
  under the row (radius 10, `rgba(255,255,255,0.04)`, a 1 px
  `rgba(255,255,255,0.08)` rim), one 34 px row per identity, a green check
  on the chosen one. Choosing clears the field. The default is the
  session's user when polkit offers them, else the first.
- **The field**, 14 px under: 7.1's, 44 px high, radius 12,
  `rgba(255,255,255,0.08)`, its 1 px border `rgba(108,194,135,0.60)` while
  it has the keyboard, `#F2945B` after a wrong password,
  `rgba(255,255,255,0.16)` while checking; "Mot de passe" as placeholder;
  Caps Lock's sign (22 × 22, radius 6, `rgba(245,196,110,0.16)`, a 14 px
  arrow in `#F5C46E`).
- **The line under the field** (13 px on 1.45 lines, 19 px reserved, 6 px
  under it): "Majuscules verrouillées" (`#F5C46E`), "Vérification…"
  (`text.faint`), "Mot de passe incorrect. Réessaie." (`#F7B98A`, with "
  Les majuscules sont verrouillées." when they are), and PAM's own
  messages as they come.
- **Details**: a 28 px "Détails" button, its chevron turning 90° in
  150 ms, folded by default (for everyone). Unfolded: a box (radius 8,
  `rgba(255,255,255,0.04)`, padding 10 / 12), its labels in a 74 px column
  (12 px), its values in Martian Mono 11 px `#C9D0DC`, broken anywhere:
  Action, Programme, Processus ("48213, lancé par Alex"), Fenêtre (or
  "aucune").
- **The footer**, 16 px under: "1 autre demande en attente" on the left
  when one waits; "Annuler" and "Autoriser" (38 px, radius 10) on the
  right: Autoriser `#6CC287` on `#0b1020` once something is typed,
  `rgba(108,194,135,0.18)` and `text.faint` while the field is empty,
  `rgba(255,255,255,0.10)` with a 14 px spinner while checking.

## 4. Answering

- `Enter` in the field, or Autoriser, sends the password; the field, the
  buttons and the chooser are inert until polkit answers.
- **Granted**: the dialog fades out (150 ms), the dim with it.
- **Wrong**: the field shakes (360 ms, 10 px, as 7.1's), empties and keeps
  the keyboard; polkit's helper is started again. After 3 wrong tries in
  a row the dialog closes and the request is refused; the application may
  ask again.
- **Cancel**: Annuler or `Esc` (which folds the chooser first) cancels;
  the application learns it was refused.
- The application cancels: the dialog closes at once, without a message.
- **Queued**: one dialog at a time; a request that comes while one shows
  waits, counted in the footer, and opens 300 ms after the first closes.

## 5. Motion

- In: the dim over 160 ms; the dialog fades and scales from 96 % over
  250 ms (out-cubic). Out: 150 ms fade (in-cubic).
- With reduced motion, or a shell on the CPU: no scale, no shake; the dim
  and the dialog are there at once; a wrong password only turns the
  border and the line.

## 6. Keyboard

- The field has the keyboard as the dialog opens. `Tab` goes through
  Changer (when shown), the identities (when unfolded), the field,
  Détails, Annuler, Autoriser; `Shift+Tab` back. `Enter` in the field
  authorizes, on a button acts; `Space` acts on buttons.

## 7. Text

| FR | EN |
|---|---|
| Autorisation nécessaire | Authorization required |
| administrateur | administrator |
| Changer | Change |
| Mot de passe | Password |
| Mot de passe d'Alex | Alex's password |
| Majuscules verrouillées | Caps Lock is on |
| Vérification… | Checking… |
| Mot de passe incorrect. Réessaie. | Wrong password. Try again. |
| Les majuscules sont verrouillées. | Caps Lock is on. |
| Détails | Details |
| Action · Programme · Processus · Fenêtre | Action · Program · Process · Window |
| lancé par Alex | started by Alex |
| aucune | none |
| 1 autre demande en attente · 2 autres demandes en attente | 1 more request waiting · 2 more requests waiting |
| Annuler | Cancel |
| Autoriser | Authorize |
| Autorisation nécessaire : Mises à jour | Authorization required: Updates |

## As built

### 7.5, the system's authentication dialog (2026-11-04)

- **Who does what.** The shell is the agent (`crates/hocus-shell/src/auth/`):
  on a thread of its own, on the system bus, it serves
  `org.freedesktop.PolicyKit1.AuthenticationAgent` at
  `/org/hocuspanda/PolicyKit1/AuthenticationAgent` and registers it with
  `RegisterAuthenticationAgent` for the session's subject (`unix-session`,
  `XDG_SESSION_ID`, else logind's `session/auto`'s `Id`), in the messages'
  locale (`LC_ALL`, `LC_MESSAGES`, `LANG`), so that polkit translates its
  message with the action's policy. It registers again whenever polkit's
  name gets a new owner (polkitd restarted). Only polkit's calls are
  taken: one whose sender is not `org.freedesktop.PolicyKit1`'s owner is
  refused (`NotAuthorized`). The shell is the agent only when the
  compositor is the session (on a virtual terminal it sets
  `HOCUS_AUTH_AGENT=1` for it): a nested shell leaves the host session's
  agent alone. Our own reading of polkit's documented interfaces; no code
  of polkit's (LGPL) is used, and no crate (none permissive and kept up
  was found): zbus, as for the other system services.
- **A request.** `BeginAuthentication` is answered once the dialog is:
  nothing when granted, `org.freedesktop.PolicyKit1.Error.Cancelled` when
  cancelled (by the user or by polkit), `…Error.Failed` after three wrong
  passwords. What the dialog needs is read on the agent's thread: the
  accounts (`unix-user` by its id; `unix-group` by its members in
  `/etc/group` and the accounts whose primary group it is; an id without
  an entry in `/etc/passwd` is left out), the subject's process
  (`polkit.subject-pid`, else `polkit.caller-pid`) and its parents from
  `/proc`, its program (polkit's `program` detail, else its executable),
  and who started it (its real user's name). `CancelAuthentication`
  closes that cookie's dialog at once, or takes its request out of the
  queue.
- **The helper.** The shell never checks a password. Where
  `/run/polkit/agent-helper.socket` is (polkit 126 and later, Debian's
  127 among them, whose `polkit-agent-helper-1` is no longer setuid), the
  shell connects to it and writes the login, then the cookie, a line each;
  else it starts the setuid `polkit-agent-helper-1` (in
  `/usr/lib/polkit-1`, `/usr/libexec` or `/usr/libexec/polkit-1`) with the
  login as its argument and writes the cookie on its input (never in its
  arguments, which any process can read). The conversation runs on a
  thread of its own: `PAM_PROMPT_ECHO_OFF` / `…_ECHO_ON` (the first is
  answered with the password typed, a later one, a second factor, shows
  its prompt under the field and takes the next answer), `PAM_ERROR_MSG`
  and `PAM_TEXT_INFO` under the field, C escapes undone, then `SUCCESS`
  (the helper has told polkit itself) or `FAILURE`. A conversation starts
  as the dialog opens (so that PAM's first messages, a fingerprint's,
  show before anything is typed), again after a wrong password and for
  another account; ending one closes the helper's input or the socket and
  stops the helper by its own handle (a setuid one ends by itself once its
  input closes), and its thread reaps it.
- **The password** is typed into a [`Secret`](../../crates/hocus-shell/src/network/secret.rs)
  (one buffer that never grows, zeroed as it goes), never logged (the
  dialog's log says how many characters), written to the helper as it is
  and wiped once sent.
- **Where.** The shell says `auth open ID PID…` (the asking process, then
  its parents, read by the shell so that the compositor reads nothing
  from `/proc` on its loop); the compositor finds the topmost window of
  the nearest of them (as a bubble finds its sender's) and answers
  `auth-at ID OUTPUT SCALE HEIGHT`, the window's application and title:
  its screen, else the one under the pointer. The dialog is a layer
  surface of the shell's (`hocus-auth`), in the overlay layer, anchored to
  the screen's top, centred across it (exclusive zone `-1`), its top
  `HEIGHT / 2 − 220` (8 px at least), as tall as its content and resized
  as it grows, holding the keyboard (`exclusive`). Only a surface of that
  namespace on the screen asked, while the shell asks, is taken for the
  dialog: a client naming its surface so at any other time gets no dim
  and no keyboard of the dialog's.
- **The dim and the input** (`crates/hocus-compositor/src/auth.rs`): from
  the surface's first frame the compositor draws, over everything but the
  cursor, the dialog and its shadow on its screen and the dim on every
  screen, never in a capture, a recording or a share. While it shows, the
  dialog holds the keyboard before any other exclusive surface; the
  compositor's shortcuts are off but the levels' (volume, brightness) and
  the virtual terminals' (XKB's own keys switch the layout, as ever); the
  capture's, Alt+Tab's and the strokes' keys are off too. Presses away
  from the dialog go nowhere, and nothing under the dim gets the pointer's
  motion, the wheel or a gesture: the window under a still pointer loses
  it as the dialog shows. `auth closing ID` lets them go at once, as the
  dialog fades out.
- **Caps Lock** is the compositor's to know: it tells the shell `caps on`
  or `caps off` as it changes and as the shell connects; iced's keys do
  not carry it.
- **Who asks**: the window's application by its desktop entry (its id, or
  its id's last part), else the program's file name's entry, else the
  window's id's last part or the program's name; its icon from the
  launcher's icon lookup, else the gear on the neutral gradient
  (`#5C6576 → #9AA3B4`). The avatar is the panda for the session's own
  account, another account's initial on `#C792EA → #6F9BFF` (the
  mockup's).
- **Queue.** Requests wait in arrival order; the next opens 300 ms after
  the one shown closed (a deadline of the shell's effects, no timer per
  frame). The bar's popover, the launcher and the emoji picker close as a
  dialog opens.
- **Drawing.** As for the share's chooser, nothing goes through iced's
  messages: the dialog's state is the shell's, keys, the pointer and the
  helper's thread change it and draw its surface alone (`wake hocus-auth`
  from the threads). At rest it draws nothing: the caret does not blink,
  and without motion the spinner does not turn. The shake (its
  keyframes 10, 9, 6, 4 px) and the chevron's turn ask for frames only
  while they last, with motion.
- **Choices made here** (to confirm on the machine): the field's border
  stays green while the dialog types (as the mockup's) even when `Tab` is
  on a button, whose focus shows as a 2 px accent ring; typing on a button
  types into the field; the shake's curve is the keyframes, linear between
  them; Annuler and `Esc` stay live while a password is checked (they end
  the helper); a helper that fails before anything is sent counts as no
  try, the next `Enter` starts another; the dots are 7.1's as its spec
  says them (a dot per character), to be matched to 7.1's code once it
  lands.
- **Measured headless** (`crates/hocus-compositor/tests/polkit.rs`): the
  real shell on the CPU against a fake polkit authority on a D-Bus daemon
  of the test's own (as the system bus), which records the registration
  and calls `BeginAuthentication` / `CancelAuthentication` from polkit's
  name, a fake helper (`HOCUS_POLKIT_HELPER`, a script speaking the
  helper's protocol, which takes "panda"), a socket of the test's
  (`HOCUS_POLKIT_SOCKET`) for polkit 126's way, and accounts of the test's
  (`HOCUS_AUTH_ETC`): one scenario per point. The machine's polkit, its
  helper and its system bus are never used.
- **Not built**: names for screen readers (the Accessibility milestone;
  the dialog's and the field's names are in its words and its log);
  pasting into the field; an account's photo (AccountsService); the
  application's own icon name polkit sends (the desktop entry's is used).
