# Vision

## In one sentence

A desktop that is beautiful, fast and modern enough to make people say "wow",
and solid enough to never freeze, stutter or lose a window.

## Who it is for

People who spend their day at the computer and want it to be a pleasure:
developers working alongside several AI agents, people with several screens
of different shapes and scales, anyone who wants a desktop that gets out of
the way while they work and is beautiful when they stop.

## What "good" means

1. **Never freezes, never stutters.** A slow widget, service or application
   never slows down the screen. Every frame is measured; a drift is visible
   before anyone feels it.
2. **Beautiful on purpose.** Glass, blur, soft shadows, springs, light that
   follows the time of day. Effects that help you understand what is
   happening (where the focus is, who is waiting, what is running), or that
   are genuinely beautiful. Inspirations: Compiz, Matrix, Minority Report,
   Her, Iron Man, serving daily work rather than demos.
3. **Quiet while you work, alive when you look.** No title bars by default, a
   dock that hides, notifications that know when to wait.
4. **AI-native.** Coding agents get their own pointer, keyboard and virtual
   screen; they show what they are about to do before doing it; you can watch
   them, take over and hand back. The same control API lets agents use the
   desktop and lets the test suite drive it.
5. **Pleasant to develop.** Test without restarting your session (nested
   mode, hot reload, VMs). Every feature comes with a scenario that runs
   without a screen.
6. **Portable and free.** Linux and FreeBSD. MIT license, permissive
   dependencies only. Plugins are written in Rust and compiled with the
   desktop: no JavaScript, no Lua, no WebAssembly.

## What it is not

- Not a tiling window manager first. The model is floating windows with
  snapping and per-desktop tiling as an option.
- Not an X11 desktop. X11 applications run through XWayland as plain
  compatibility; no part of the desktop depends on it.
- Not Clippy. The pandas show state; they never interrupt.
