# 0001 — Rust and smithay

Date: 2026-10-21 · Status: accepted

## Context

We want code that stays clean, robust and evolvable for years, written
largely by AI agents and reviewed by a human. A compositor's worst bugs come
from state touched by several threads, a blocked main loop, and object
lifetimes across a foreign-function boundary (see [robustness](../robustness.md)).

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
- No ready-made toolkit to draw a shell inside a compositor: we write a small
  one (`hocus-ui`). It is the main technical risk and comes early in the plan.
