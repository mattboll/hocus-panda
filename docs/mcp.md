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
| `capture` | a PNG of a `window` (or an `output`), and how its pixels map to logical coordinates; a window on a desktop no screen shows, or minimized, as its application draws it: woken first (see below) |
| `click` | `window` at `x`, `y` (from its top-left corner, logical pixels; its centre when left out), or `element` by label; `button`, `count`, `intent` |
| `scroll` | the same aim, `dy` in logical pixels (15 per notch, down positive); `intent` |
| `type` | `text` into `window`, with the keyboard layout in effect; `intent` |
| `press_keys` | a combination into `window`: `Ctrl+A`, `Return`, `Shift+Tab`; `intent` |
| `wait_for` | until a window (`app_id`, a part of its `title`) or an element (`label`) is shown, or with `gone` until none is; `timeout_ms` |
| `journal` | what this agent did this session, oldest first, one line each (see below); `limit` (the last so many, 50 by default), `agent` |

`intent` is what the action is for, in one short line the user reads before
it happens ("Sends the message"); without it, the desktop says it plainly
("Clicks in Shopping list"). Keys an agent types go to that window alone,
never to the desktop's shortcuts. Refusals and failures come back as the
tool's result, marked as an error, for the agent to read.

## A seat of its own

An agent that names itself gets a seat of its own (a `wl_seat` named
`agent-NAME`), with its own pointer and keyboard, and its own focus: it
clicks and types in its windows while the user works in theirs, and the
user's pointer, keyboard focus and stacking stay as they are. Nothing comes
forward for an agent:

- A click or a scroll lands on the window at the point asked, even under
  another window: the seat is the agent's alone.
- Clicks, scrolls and typing go wherever the window is: on a desktop no
  screen shows too (see below). The agent's keyboard follows its clicks,
  as the user's does theirs.
- A minimized window the agent acts on comes back on its own desktop,
  where it was in its stack, without the user's keyboard and without
  switching desktops: shown if a screen shows that desktop, unseen
  otherwise.
- A click on the shell's own interface (`element`, with `shell = true`)
  still goes through the session's pointer, put back where it was after:
  the shell has one pointer.

The seat goes when the connection does. Its pointer is drawn in the
agent's colour (one of the theme's, the same for its name every time) with
its name beside it, while it acts and for ten seconds after.

Limits:

- An application gets the agent's input only if it takes every seat, as
  GTK, Qt and most toolkits do. One that takes the first seat alone never
  does: after two seconds the action is refused, saying so.
- X11 windows (XWayland) have the session's pointer and keyboard alone:
  agents act on Wayland windows.
- The agent's seat has its own clipboard and selection: what it copies
  does not reach the user's, nor the clipboard history, and it pastes
  only what it copied.
- A window that the agent's click would activate (xdg-activation, opening
  a link) asks for attention instead of taking the user's keyboard, and
  an agent's click never starts moving or resizing a window.

## A desktop it works on, unseen

An agent may work on a desktop no screen shows, desktop 2 say, while the
user stays on desktop 1: its windows there keep living as if shown. They
keep their size and state; while the agent drives one (it acted on it in
the last ten seconds, captured it, or an action waits), the compositor
sends its application a frame callback ten times a second, so that it
draws what the agent does, and the agent's `capture` of that window shows
it. A capture there, even before any action, first sends the window a
frame callback when its application waits for one, and waits for it to
draw (200 ms at most): it shows what the application has to show now. Nothing
is drawn on a screen for them, and nothing at all once no agent drives a
window there: the cost is an application drawing at most ten times a
second while an agent works in it, and one wake of the compositor each
time, no rendering.

The agent's pointer, ring and intent show where a screen shows the window
it points at: going to desktop 2, the user sees it work. A card that waits
for the user shows wherever they are, alone, saying where the window is
(`In a password field · Desktop 2, not shown`). Take over brings the
window forward on its desktop, as before.

## It shows before acting

Before a click, a ring shows at the point with the agent's intent beside
its pointer, `[agents] show_before_click_ms` (600 by default, 0 for none);
the click lands then, after the compositor checked again that the agent
may. The tool answers once it is done.

Some actions wait for the user to confirm them on a card beside the point
(Allow, Refuse, Take over); past `[agents] confirm_timeout_s` (60 seconds)
they are refused, and nothing is done. Either way the journal keeps them
(`; confirmed by the user`, or the refusal). They are:

- clicking, typing or pressing keys in a window of an application of
  `[agents] confirm_apps` (globs of app ids): the settings and the usual
  password managers by default (KeePassXC, Bitwarden, 1Password, Seahorse,
  Secrets, KWallet); a bank's application can be added;
- typing or pressing keys where the window's application says, through
  text-input-v3, that the field is for a password or a PIN. The agent's
  keyboard tells the application's text inputs when it comes into the
  window, and typing waits a moment for the answer; an application that
  says nothing, or says it on the user's seat alone, is not caught.

Scrolling never waits. The keys an agent types never reach the desktop's
shortcuts (locking, the stop key), so those are not on the list. The card
takes the user's pointer; it takes their keyboard only once they clicked
it (Allow shows it; the card says `Enter · Esc`): then Enter presses Allow (Tab moves to the other
buttons), Escape refuses, and a press elsewhere gives the keyboard back.
An Enter typed in another window before that never answers it.

```toml
[agents]
show_before_click_ms = 600
confirm_timeout_s = 60
confirm_apps = ["hocus-settings", "org.keepassxc.KeePassXC", "my.bank.App"]
```

The settings' AI and agents page sets the three, applied at once.

## Take over

While an agent is connected, the column's journal shows its window, small
and live, with where it is (`helper · in Shopping list`) and Take over: the
window comes forward with the user's keyboard, and the agent's actions are
refused (`refused: the user took over…`) until Hand back. The card of a
sensitive action offers the same. The thumbnail shows the window wherever
it is, on a desktop no screen shows too: the desktops are the agent's
screens, there is no output of its own.

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
anyway. A question the user hands a coding agent from an AI's answer is
kept too, as that agent's (`refacto: prompted with 170 characters`, the
text only with `journal_text`).

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
kept as such (`helper: connect; refused: …`). An action that waited for
the user is kept when it is allowed (`; confirmed by the user`), refused,
or not confirmed in time. The journal lives in memory
only, the last 500 entries: nothing is written to disk, and entries never
reach the compositor's log above debug level.

Once an agent connected, the column shows the last three, newest first
(agent · action · window · time), and a click on them opens the last dozen
with the reasons of refusals.

## Stop them all

`stop-agents`, Super+Shift+Escape by default, or Stop in the column's
journal, stops every agent at once: from then on every request of an agent's
connection is refused, reading included (`refused: the user stopped every
agent…`), so is what an agent waits on (shown before a click, or for
the user's confirmation), a new agent cannot name itself (the attempt is kept in the
journal), and the events they subscribed to stop reaching them. The column says so, and the same key, or Allow, lets
them go on. The key works even while a window holds the shortcuts back or a
panel is open; agents cannot press it, since the keys they type never reach
the desktop's shortcuts. It lasts until allowed again or the session ends.
A server started without `--agent` is no agent: it reads, as any program of
the user's may.
