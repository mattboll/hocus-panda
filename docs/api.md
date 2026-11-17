# The control socket

The desktop's public interface for programs of their own: scripts, the
agents' tools, and later add-ons (#442). One Unix socket, lines of text
in, lines of text or JSON out. What is written here is the interface: a
line, its answer or an event that changes is a change of its version
(below), and the tests hold each example of this page to what the
desktop answers (`crates/hocus-compositor/tests/api.rs`).

`hocus` (crate `hocus-cli`) writes these lines for people and scripts
(`hocus msg version`, `hocus agent …`); any program may write them
itself.

## Where

`$HOCUS_SOCKET`, set for every program the desktop starts; else
`<the Wayland socket>.control`, beside `$WAYLAND_DISPLAY` in
`$XDG_RUNTIME_DIR` (`wayland-1.control`).

## How a connection goes

A client connects, writes a line ending with `\n`, and:

- **a question** (`msg …`) is answered with lines of text or one line of
  JSON, then the desktop closes the connection. One question a
  connection. An answer that starts with `refused: ` is a refusal, the
  rest of the line saying why.
- **a report** (`agent …`, `celebrate …`, `ran …`) is answered with
  nothing. A connection may carry up to 64 of them, one a line; it is
  read for a second at most.
- **a subscription** (`msg subscribe`, `msg input-devices follow`) keeps
  the connection open: the desktop writes on it as things happen, until
  the client closes it.

A line the desktop does not know is ignored, unanswered: a desktop older
than this interface answers nothing to `msg version` either.

## Versions

`msg version` says three things:

- `api`, the interface's version, an integer. Every change to a line
  documented here, to its answer or to an event raises it, additions
  included (a new line, a new event, a new field in a JSON object).
- `oldest`, the oldest `api` whose clients this desktop still serves.
  Additions leave it as it was: a client reads the fields and events it
  knows and skips the others. A change a client written for an older
  version would notice (a line or an event gone, an answer reshaped, a
  field's meaning changed) raises it to `api`.
- `hocuspanda`, the desktop's own version (its package's).

A client written for version `N` works with a desktop whose
`oldest <= N <= api`. Version 1 is this page as first written.

```control
> msg version
< {"api":1,"oldest":1,"hocuspanda":"…"}
```

## The lines

| Line | Answer | Since |
|---|---|---|
| `msg version` | one line of JSON: `api`, `oldest`, `hocuspanda` | 1 |
| `msg subscribe [KINDS]` | the events, one line of JSON each ([Events](#events)) | 1 |
| `msg idle` | the session's idle state, its time left, what holds it off (design/idle.md) | 1 |
| `msg lock` | `locked`, once the session is (design/lock.md) | 1 |
| `msg outputs` | the screens and their sets, as one line of JSON (design/settings.md, "Écrans") | 1 |
| `msg input-devices` | the mice and touchpads, a line each, or `no mouse or touchpad` | 1 |
| `msg input-devices follow` | the same, then `listed`, then each device `added`, `changed` or `removed` | 1 |
| `msg a11y` | what the shell shows to screen readers, a node a line, or `no shell` (design/screen-reader.md) | 1 |
| `msg shell status` | the program the desktop runs as its shell, since when (docs/workflow.md) | 1 |
| `msg shell restart [installed \| program PATH]` | `restarting` and the program, or a refusal | 1 |
| `agent SOURCE KEY STATE PID NAME` | nothing: an agent's state (docs/agents.md) | 1 |
| `celebrate PID NAME` `\t` `WHY` | nothing: an agent's good news (docs/agents.md) | 1 |
| `ran SOURCE KEY PID WHAT` | nothing: what an agent left running after its turn | 1 |

### Questions

The session's idleness: its state, its delay, the time left while it
counts, and what holds it off; then whether the screens are away.

```control
> msg idle
< state …
< delay …
< …
```

Locks the session now, as `Super+L` does; answered once it is locked.

```control
> msg lock
< locked
```

The screens (`outputs`: each one's modes, place, scale and settings,
whether it is on, plugged and primary) and the sets of screens the
desktop knows, one JSON object.

```control
> msg outputs
< {"outputs":[{"name":"HEADLESS-1",…}],…}
```

The mice and touchpads; followed, the connection stays open and says
each one plugged, changed or unplugged.

```control
> msg input-devices
< no mouse or touchpad
```

```control
> msg input-devices follow
< listed
< …
```

What the bar and the shell's other surfaces show, as screen readers
read it (a node a line), or `no shell` while none runs.

```control
> msg a11y
< …
```

The shell's program: its state (`running`, `starting`, `waiting`,
`ending`, `off`), which program, whether it was given for this session,
its process and since when, how many screens show their bar. A restart
ends it and starts it again at once (the windows stay); a program given
(`program PATH`, an absolute path) is taken only in developer mode
(`[developer] live-reload`), else refused.

```control
> msg shell status
< state …
< program …
< …
```

```control
> msg shell restart program /bin/sh
< refused: developer mode is off, …
```

### Reports

An agent's state, from any tool: `SOURCE` is `command` (`claude-code`
and `codex` are the hooks'), `KEY` its name in one word, `STATE` one of
`working`, `waiting`, `done`, `failed`, `idle`, `background`, or `ended`
to forget it, `PID` its process or `-`, `NAME` the rest of the line.
Fields after tabs follow (a note, tests, the conversation): docs/agents.md,
"The line, for tools that write it themselves".

```control
> agent command build working - build
```

Good news: `PID` the process that has it (`-` unknown), `NAME` the name
its row takes when no agent is known for that process, then a tab and
why.

```control
> celebrate - build	the build is green
```

What an agent left running after its turn, as Claude Code's and Codex's
hooks say it: `WHAT` is `subagent-start ID`, `subagent-stop ID` or
`tasks N`.

```control
> ran claude-code 3f6c2a1e - tasks 1
```

## Events

`msg subscribe KINDS` subscribes to events of `KINDS`, comma-separated:
`window`, `desktop`, `screen`, `agent`; every kind when none is said. A
kind that is not one is refused. The connection stays open, and the
desktop writes one JSON object a line: first that the subscription is
taken, then each event as it happens. Each object's `event` says which;
fields may be added in later versions, and events of new names too:
skip what you do not know.

```control
> msg subscribe window,agent
< {"event":"subscribed","api":1,"kinds":["window","agent"]}
< …
```

```control
> msg subscribe window,mouse
< refused: no events "mouse": window, desktop, screen, agent
```

`hocus msg subscribe [KINDS]` prints them as they come.

The events, and what each says (the shapes below are what the tests
check every event against):

A window opened, closed, or got the keyboard (`window` is `null` when
no window has it). `id` is the window's for as long as it is open, the
same as `hocus mcp`'s (design/mcp.md); `app_id` is its application's
(empty when it gives none), `title` its title.

```event
{"event":"window-opened","window":{"id":3,"app_id":"foot","title":"~"}}
{"event":"window-closed","window":{"id":3,"app_id":"foot","title":"~"}}
{"event":"window-focused","window":{"id":3,"app_id":"foot","title":"~"}}
```

A screen shows another of its desktops: the screen, and the
desktop's number (from 1) and name (its project's, or `[desktops]
names`'; empty when none).

```event
{"event":"desktop-switched","screen":{"name":"eDP-1"},"desktop":{"number":2,"name":"hocus-site"}}
```

A screen plugged or unplugged (or turned on or off in the Settings): its
name. `msg outputs` says the rest.

```event
{"event":"screen-plugged","screen":{"name":"HDMI-A-1"}}
{"event":"screen-unplugged","screen":{"name":"HDMI-A-1"}}
```

An agent's state, as the bar shows it: `working`, `waiting`, `done`
(finished, not looked at yet), `seen` (finished and looked at),
`failed`, `idle`, `background` (looked at, something it started still
runs); `ended` once it is forgotten. `id` is the agent's for the
session, `source` who tells it (`claude-code`, `codex`, `herdr`,
`command`, joined with `+` when several do), `window` the id of its
window, or `null` when none is known.

```event
{"event":"agent-state","agent":{"id":1,"name":"build","state":"working","source":"command","window":null}}
```

The first line, and the loss of events:

```event
{"event":"subscribed","api":1,"kinds":["window","desktop","screen","agent"]}
{"event":"lost","count":12}
```

Events are told after the turn of the desktop's loop they happened in;
what changes and changes back within one turn is not told. Within a
turn: screens plugged, windows opened, windows closed, desktops
switched, the focus, screens unplugged, in that order; agents' states
come before the rest.

**A slow reader.** The desktop never waits for a subscriber. It keeps
at most 256 events for one that does not read (and the few its socket
holds); past that, events are lost, and once there is room again the
subscriber is told how many with `{"event":"lost","count":N}`, in their
place. A subscriber that closes its connection is dropped.

## Not part of the interface

Some lines are the desktop's own programs', and may change with them
without a change of version:

- `msg screens`, the Settings' line to the screens (design/settings.md).
- `perf settings PID PAGE`, the Settings saying their page, for the
  performance record (docs/performance.md).
- `mcp NAME`, `hocus mcp`'s line for an agent (design/mcp.md); the
  interface there is the MCP server's tools.

## How the examples are read

The tests run each block of this page fenced as `control` against a
desktop of their own, each on a connection of its own, in the page's
order; and check every event they see against the block fenced as
`event` that has its name. In a `control` block:

- `> LINE` is the line written (a tab is a tab);
- `< TEXT` a line of the answer, `…` standing for any text;
- `< …` alone, last, stands for any further lines (the connection may
  stay open);
- a block without `<` lines is answered with nothing.
