# Hocuspanda

A fast, beautiful Wayland desktop for Linux and FreeBSD, with a few pandas that
show what your computer is doing.

> **Status: early design and foundations.** Nothing runs yet. The documents
> below describe what we are building and how.

## What it is

- A Wayland compositor and its whole shell (widget column, launcher,
  notifications, quick settings, lock screen) in a single Rust process, built
  on [smithay](https://github.com/Smithay/smithay).
- Floating windows with snapping, per-screen virtual desktops, no title bars
  by default, a discreet dock.
- AI-native: coding agents (Claude Code and others) are first-class users of
  the desktop, with their own pointer and keyboard, and a control API that
  both agents and the test suite use.
- Portable: Linux first, FreeBSD as a target from the start.
- MIT licensed, with permissive dependencies only.

## Documents

| Document | What it covers |
|---|---|
| [Vision](docs/vision.md) | Why this project exists and what "good" means |
| [Features](docs/features.md) | Exhaustive, implementation-neutral feature list |
| [Ideas](docs/ideas.md) | Brainstormed features beyond the basics, and their triage |
| [Identity](docs/design/identity.md) | Name, mascot, themes, typography |
| [Motion](docs/design/motion.md) | Animations: windows, desktops, panel, notifications, menus |
| [Design brief](docs/design/brief.md) | What the mockups explore and the decisions they led to |
| [Roadmap](docs/roadmap.md) | Milestones, in order |
| [Architecture](docs/architecture.md) | Process model, crates, rendering, testing |
| [Robustness](docs/robustness.md) | The failure modes the architecture rules out |
| [Decisions](docs/decisions/) | Architecture decision records |
| [Workflow](docs/workflow.md) | Issues, pull requests, testing, tools, working with AI agents |

## Building

Not yet. See [Workflow](docs/workflow.md#tools) for the tools to install.

## License

[MIT](LICENSE).
