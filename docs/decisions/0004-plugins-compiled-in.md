# 0004 — Plugins are Rust crates compiled in; no scripting

Date: 2026-10-21 · Status: accepted

## Context

We want to extend the desktop easily (widgets in the column, launcher
providers, how Alt+Tab behaves, effects) without "a nest of messy things".
JavaScript, Lua and WebAssembly were considered and rejected: they add a
second runtime, a second API surface to keep stable, and performance and
debugging costs.

## Decision

A plugin is a Rust crate implementing traits exposed by `hocus-shell` and
`hocus-compositor` (status module, widget, launcher provider, action, effect).
Plugins are compiled with the desktop, enabled by Cargo features and turned
on or off in the configuration. No dynamic loading, no scripting language.

## Consequences

- Plugins get the same type checking, performance and tests as the core.
- Adding a third-party plugin means rebuilding; distributions ship a set of
  plugins enabled by default.
- The plugin traits are the public extension API and must be designed and
  versioned with care.
