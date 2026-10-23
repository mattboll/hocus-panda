# Agents over MCP

`hocus mcp` is a [Model Context Protocol](https://modelcontextprotocol.io)
server on stdin and stdout (JSON-RPC 2.0, one message per line; revisions
2024-11-05 to 2025-11-25). An AI agent uses the desktop through it: the
windows, the shell's interface, captures of a window, and clicks, typing and
scrolls aimed at a window or at a labelled element, rather than guessing on
pixels of the whole screen.

## Give the agent its rights

An agent is named in the configuration, with what it may do:

```toml
[[agents.mcp]]
name = "assistant"
access = "act"                    # "read" by default
apps = ["foot", "org.mozilla.*"]  # app ids it may act on, globs; ["*"] for all
shell = false                     # true: it may click the shell's own elements too
```

The server names itself with `--agent NAME`, and the compositor holds that
connection to the entry: a `read` agent that tries to click is refused, an
`act` agent only acts on the windows of its apps, and taking a right away
in the file applies at the next action. No agent acts while the session is
locked or a password dialog is open. A name that is not listed is refused;
without `--agent`, the server only reads.

## Plug a client

Any client that starts stdio servers runs it as a command, in the session
(it finds the compositor through `$WAYLAND_DISPLAY` or `$HOCUS_SOCKET`; or
pass `--socket PATH`). The usual JSON form:

```json
{
  "mcpServers": {
    "hocuspanda": { "command": "hocus", "args": ["mcp", "--agent", "assistant"] }
  }
}
```

With Claude Code: `claude mcp add hocuspanda -- hocus mcp --agent assistant`.

## Tools

| Tool | Does |
|---|---|
| `list_windows` | the screens, and every window: id, title, app id, output, geometry (global logical pixels), focus, visibility, state, desktop |
| `accessibility_tree` | the shell's interface (role, label, rectangle, depth, focused, disabled, selected); with `window`, that window alone: applications' own trees (AT-SPI) are not read yet |
| `capture` | a PNG of a `window` (or an `output`), and how its pixels map to logical coordinates |
| `click` | `window` at `x`, `y` (from its top-left corner, logical pixels; its centre when left out), or `element` by label; `button`, `count` |
| `scroll` | the same aim, `dy` in logical pixels (15 per notch, down positive) |
| `type` | `text` into `window`, with the keyboard layout in effect |
| `press_keys` | a combination into `window`: `Ctrl+A`, `Return`, `Shift+Tab` |
| `wait_for` | until a window (`app_id`, a part of its `title`) or an element (`label`) is shown, or with `gone` until none is; `timeout_ms` |
| `journal` | what this agent did this session, oldest first, one line each (see below); `limit` (the last so many, 50 by default), `agent` |

Acting on a window brings it forward first (its desktop shown, restored if
minimized) and gives it the keyboard; a click that something else would
catch (another window, a bar, the shell) is refused rather than sent. Keys
an agent types go to that window alone, never to the desktop's shortcuts.
Refusals and failures come back as the tool's result, marked as an error,
for the agent to read.

Until agents get a seat of their own, acting moves the session's pointer and
keyboard focus, as the user would.

## The journal

The compositor keeps what each agent asked for, in order, the last 500
actions of the session: when (the local time, as the clock says it), which
agent, which action (click, scroll, type, keys), on what (the window's app
id and title as they were then, or the shell element's label), and, when
it was refused, why. What an agent types is not kept: the entry says how
many characters, unless the configuration asks for the text:

```toml
[agents]
journal_text = true
```

Key combinations are kept (`Ctrl+S`), but neither keys nor text while the
session is locked or a password is asked for, when agents are refused
anyway.

```
$ hocus msg agents-journal
2026-10-21 14:37:00 helper: click on window 2 notes "Shopping list"
2026-10-21 14:38:01 helper: type 4 characters on window 2 notes "Shopping list"
2026-10-21 14:38:01 helper: keys Ctrl+S on window 2 notes "Shopping list"
2026-10-21 14:38:01 helper: click on window 1 other "Passwords"; refused: agent "helper" may not act on windows of "other": its apps in [[agents.mcp]] do not match it
```

`--json` gives the entries as data; the control API's `agents_journal`
request (open to any client of the session) gives the same. An agent reads
its own entries alone, through the `journal` tool or the request: the user
(`hocus msg`, the column) and a server started without `--agent` read them
all. While agents are stopped, an agent that names itself is refused and
kept as such (`helper: connect; refused: …`). The journal lives in memory
only, the last 500 entries: nothing is written to disk, and entries never
reach the compositor's log above debug level.

Once an agent connected, the column shows the last three, newest first
(agent · action · window · time), and a click on them opens the last dozen
with the reasons of refusals.

## Stop them all

`stop-agents`, Super+Shift+Escape by default, or Stop in the column's
journal, stops every agent at once: from then on every request of an agent's
connection is refused, reading included (`refused: the user stopped every
agent…`), a new agent cannot name itself (the attempt is kept in the
journal), and the events they subscribed to stop reaching them. The column says so, and the same key, or Allow, lets
them go on. The key works even while a window holds the shortcuts back or a
panel is open; agents cannot press it, since the keys they type never reach
the desktop's shortcuts. It lasts until allowed again or the session ends.
A server started without `--agent` is no agent: it reads, as any program of
the user's may.
