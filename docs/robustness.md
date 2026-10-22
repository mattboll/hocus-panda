# Robustness by design

Desktops freeze, stutter and misplace things for a small number of recurring
reasons. Hocuspanda's architecture is built to make each of them impossible,
not just rarer.

## 1. The main loop blocked, or its state touched from elsewhere

A compositor's state is not thread-safe. If any thread can reach it, one day
one will; if anything slow runs on the loop, the screen freezes.

*Here:* one event loop (calloop) owns all compositor and shell state; other
threads cannot reach it — the compiler refuses. Blocking work (D-Bus, files,
network, AI) runs elsewhere and sends owned results back through channels.
Every loop handler is timed against a budget.

## 2. Frames lost, frame health measured wrong

A GPU driver can lose a page-flip event (typically after resume) and leave an
output nearly frozen. A watchdog that measures the *start* of rendering sees
nothing wrong.

*Here:* each output's frame is followed from the moment it is queued to its
vblank. An output with a frame out is looked at once a second; when three
looks in a row find the same frame still out, the lost flip counts as done,
the CRTC is committed afresh with a full redraw, and `output_recovered` is
said in the log and the event stream. Headless outputs can be stalled on
purpose, which is how this path is tested (`output-stuck.toml`); the DRM
backend itself runs in a VM (`scripts/vm/debian.sh`).

## 3. A UI toolkit doing a shell's job

Application toolkits make poor shells: a UI thread that runs one queued call
per frame floods when animations queue work on every tick; caches timed with
the wall clock break after suspend; a full-screen transparent window
repainted for small animations costs CPU all day.

*Here:* the shell is drawn by the compositor in its scene, with damage
tracking ([decision 0003](decisions/0003-shell-in-the-compositor.md));
animations are paced by the output's frame clock and never queue work per
tick; time is monotonic everywhere.

## 4. Pieces talking badly

A shell split into separate processes and windows (a panel window, menus and
notifications as separate windows positioned over IPC, state shared through
polled files) misplaces popups on the wrong output, restarts on output
changes, tears its shared files, and depends on whatever protocol the panel
uses.

*Here:* one process, one scene: the shell knows its own surfaces and their
outputs. External tools use a typed control API with an event stream, not
files.

## 5. The watcher that bites

A watchdog that compares "time since the loop last ran" with a threshold
takes a quiet screen (a fullscreen video, nothing to redraw) for a stuck one
and restarts the session.

*Here:* the watchdog checks that the loop answered *its own* wakeup, on the
monotonic clock, and dumps every thread's backtrace before acting. A quiet
screen is a healthy screen. The goal is not to restart nicely: it is not to
crash.

## Other rules

- **Fullscreen means awake.** A fullscreen window, the Wayland idle-inhibit
  protocol and D-Bus inhibitors all hold idle actions; the countdown restarts
  when the hold ends.
- **A crash never unlocks the session**, and a crashing lock never leaves a
  black screen with no way out.
- **Test compositors are isolated** from the live session (fresh `HOME`, short
  `XDG_RUNTIME_DIR`, no session D-Bus); test processes are stopped by exact
  PID, never by pattern.
- **Premultiplied alpha done right**: textures built on the CPU must be
  premultiplied correctly, or anti-aliased edges get bright halos.
- **Decorations differ by toolkit**: GTK/libadwaita draw their own; Qt, SDL
  and mpv can negotiate server-side ones; X11 windows rely on the compositor.
  Shadows and rounded corners must work for all.
- **GPU drivers misbehave around suspend.** Survive what can be survived, and
  log enough to tell a driver failure from ours.
- **Search never reads secrets.** Document indexing never reads `~/.ssh`,
  keyrings, password stores or dot-directories with secrets, whatever the
  configuration says.
- **Launched applications get a clean environment** (no `LD_PRELOAD` and the
  like inherited from the desktop).
