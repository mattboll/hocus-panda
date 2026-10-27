# 0001 — Rust and smithay

Date: 2026-10-21 · Status: accepted

## Context

We want code that stays clean, robust and evolvable for years, written
largely by AI agents and reviewed by a human. A compositor's worst bugs come
from state touched by several threads, a blocked main loop, and object
lifetimes across a foreign-function boundary .

## Decision

Rust, on smithay.

## Consequences

- One event loop (calloop) owns the state; data races and cross-thread access
  are compile errors, which rules out the first failure mode by construction.
- No garbage collector: stable frame times.
- The compiler and type system catch many mistakes before running, which
  matters for AI-written code and for refactoring over the years.
- smithay is MIT, maintained, used by niri and COSMIC, and runs on FreeBSD.
- Slower builds: split the workspace into crates, use the mold linker, keep
  UI crates independent of smithay, develop in nested mode.
- The shell is a separate client with its own toolkit: see
  [0003](0003-shell-as-a-client.md). (The first build drew its shell with
  its own toolkit inside the compositor; that choice is withdrawn.)
