# 0008 — Every feature is testable by an AI, without a screen

Date: 2026-10-21 · Status: accepted

## Context

The project is developed largely by AI agents, in the open, through issues and
pull requests. Agents cannot look at a screen the way a person does, and
should not need the developer's session to check their work.

## Decision

From the first milestone:
- the compositor runs headless on a virtual clock;
- a control API exposes the scene (windows, positions, focus, effects) and
  injects input;
- scenarios (steps and expectations) and reference screenshots run in CI;
- every issue carries acceptance criteria written as scenarios, and a pull
  request is done when they pass.

## Consequences

- Features are designed with an observable state, not only pixels.
- Webcam or microphone features ship with simulated inputs.
- The same API that tests the desktop lets agents use it (agent API, MCP).
