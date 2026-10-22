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

Acting on a window brings it forward first (its desktop shown, restored if
minimized) and gives it the keyboard; a click that something else would
catch (another window, a bar, the shell) is refused rather than sent. Keys
an agent types go to that window alone, never to the desktop's shortcuts.
Refusals and failures come back as the tool's result, marked as an error,
for the agent to read.

Until agents get a seat of their own, acting moves the session's pointer and
keyboard focus, as the user would.
