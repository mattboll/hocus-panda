# Agents on the desktop, through `hocus mcp` (#440)

Status: lot A of #440, mockup approved 2026-11-12 (tools'
`mockups/s14-mcp`, scenes 1, 2 and 4;
<https://claude.ai/artifact/Cd6Ba69odbhqgaFB5ux8pg>). Lot B brings the
agent's own seat (clicking and typing in applications, their accessible
trees, captures), the confirmation cards and taking over; lot C the
Settings' page of rights (scene 5). Until then the rights are in the
configuration file.

An agent (Claude Code, Codex, any MCP client) uses the desktop through
`hocus mcp`: the natural language is the client's, the desktop offers
tools and shows what the agent does with them. Every agent has the same
colour, violet (the theme's `agent` token, `#B69CFF`), and its name
everywhere.

## The server

`hocus mcp --agent NAME` is a [Model Context Protocol](https://modelcontextprotocol.io)
server on standard input and output: JSON-RPC 2.0, one message per line.
It speaks the current revision, `2026-07-28` (each request says its
version in `_meta`; `server/discover`), and the earlier ones that open
with `initialize` (`2025-11-25`, `2025-06-18`, `2025-03-26`,
`2024-11-05`); a version it does not speak is answered with
`UnsupportedProtocolVersionError` (-32022) listing those it does. It holds
one connection to the compositor's control socket (`--socket`, else
`$HOCUS_SOCKET`, else beside `$WAYLAND_DISPLAY`), opened with the line
`mcp NAME`, answered by a hello (`{"id":0,"ok":"hocuspanda"}`), then one
JSON object a line each way: `{"id":1,"tool":"list_windows","args":{}}`,
answered `{"id":1,"ok":…}` or `{"id":1,"error":"…"}`. The compositor does every tool: the rights, what
shows, the journal and undoing are in one place.

```sh
claude mcp add hocuspanda -- hocus mcp --agent Claude
```

## Rights

```toml
[[agents.mcp]]
name = "Claude"        # what it shows under, and --agent's NAME
access = "act"         # "read" (default): only the reading tools
settings = true        # may change settings (default false)
```

Read at every call, followed live: a right taken away applies to the
next call. A name the file does not list is refused, every call. A `read`
agent's acting tools are refused. No action while the session is locked
or the authentication dialog shows; reading too is refused while locked.
`set_setting` never writes `[agents]` (an agent does not give itself
rights or hide what it does), `[shortcuts]` nor `[developer]`.

## Tools

Ids are the compositor's (`list_windows`), desktops count from 1 on
their screen, geometry is global logical pixels. Every acting tool takes
`intent`: one short line in the agent's words (120 characters at most),
shown before it acts; without one it is refused. A refusal or a failure
is the tool's result, `isError: true`, its text saying why.

| Tool | Arguments | Result |
|---|---|---|
| `list_windows` | — | each window: `id`, `title`, `app_id`, `output`, `desktop`, `geometry` {x, y, width, height}, `focused`, `minimized`, `state` (floating, maximized, snapped, fullscreen) |
| `list_desktops` | — | each screen: `output`, `current`, its `desktops` (`number`, `name`, `windows`) |
| `list_outputs` | — | each screen: `name`, `geometry`, `work_area`, `scale` |
| `get_settings` | `section` (`appearance`) | the settings' values now (the file's, else the default), by key (`appearance.theme`) |
| `journal` | `limit` (50) | this agent's entries, oldest first |
| `focus_window` | `window`, `intent` | brought forward with the keyboard; a minimised one comes back |
| `move_window` | `window`, `x`, `y`, `width`, `height` (any of them), `desktop`, `intent` | moved and sized (a snapped or maximised window floats first), and/or sent to that desktop of its screen; its new `geometry` and `desktop` |
| `switch_desktop` | `desktop`, `output` (default the focused window's screen), `intent` | that screen shows it |
| `launch` | `app` (a desktop entry's id, else its name, ignoring case), `intent` | started, as the launcher starts it |
| `set_setting` | `key` (`appearance.theme`), `value`, `intent` | written into the configuration file in place (hocus-config), `before` and `after`. Only keys `packaging/config.toml` lists, with a value of their default's type |

Closing a window is lot B's: whether it would lose unsaved work needs
its confirmation card. There is no `arrange`: an agent composes moves.

## What shows

1. **Before an action** (scene 1): for `[agents] show-before-ms` (600 by
   default, 0 for none), a violet ring 2 px outside the window it will
   touch, and a tag at the window's top left, the agent's name over its
   intent; a desktop it switches to or sends a window to rings its tile
   in the bar; a setting or a launch puts the tag at the top of the
   screen. The rights are checked again as the time ends, then it acts.
   No pointer of its own in this lot.
2. **While it acts**: its row in the bar's agents' group, its mark (its
   initial on violet) and its line: "range tes fenêtres…", "change un
   réglage…", "change de bureau…", "lance une application…".
3. **After a request** (scene 2): once 1.5 s pass without a new action,
   a toast at the bottom left of the screen sums its actions up in one
   sentence ("Claude a rangé 3 fenêtres et changé appearance.theme"), its
   first intent quoted under it, with "Annuler" when something of it can
   be undone and "Voir", for 8 s. Its row says "a rangé 3 fenêtres ·
   10:43". "Annuler" puts the windows back where they were (place, size,
   state, desktop), the desktops each screen showed, and the settings as
   they were (a key the file did not have is removed again); launching an
   application is not undone, and the toast says so when that is all.
4. **The journal** (scene 4): a click on its row opens "Ce que font les
   agents": the day's entries, newest first, a dot (red when refused),
   "Claude · rangé 3 fenêtres", where (bureau 1, Réglages) or why it was
   refused, the time, "Annuler" on a request that can still be undone;
   "Tout arrêter" at its top.

## The journal

In the compositor's memory only, never on disk nor in its log above
debug: the last 500 actions and refusals of the session, each with its
time (as the clock says it), the agent, the tool, what it touched (the
window's app id and title then, the desktop, the setting's key; a
setting's value only for keys that are not text), its intent, and the
refusal's reason. What an agent types will be counted, never kept (lot
B). `journal` gives an agent its own entries.

## Stop

`Super+Shift+Escape` (`[shortcuts] stop-agents`) or the journal's "Tout
arrêter" stops every agent: from then on each call is refused (reading
too), an action shown and not done yet is not done, and the bar's
agents' row says "arrêté". The same key, or the journal's "Reprendre",
lets them go on. It lasts until then or the session's end. An agent has
no keys in this lot; lot B's typing never reaches the desktop's
shortcuts, so it cannot press it.

## Limits

One action at a time per agent, the next waiting for the one shown; an
agent whose connection ends is forgotten, its journal kept. Undo applies
to a request's actions as they were left: a window moved since by the
user goes back too, a window closed since is skipped. Settings' undo
writes the file as the Settings would; the Settings' recent changes do
not list an agent's.

## As built (2026-11-12, lot A)

- **Where**: `crates/hocus-cli/src/mcp.rs` (the MCP server, its tools'
  schemas, `launch`'s desktop entry found with hocus-xdg), the line's
  types in `hocus-agents` (`mcp`), the compositor's `src/mcp.rs` (rights,
  queue, show timer, requests, undo, journal, stop), what the shell is
  told in `hocus-link` (`McpShown`), the shell's `src/mcp/` (tag, toast,
  words) and `popover/journal.rs`.
- **The keys**: `[agents] show-before-ms` (the file's kebab case) and
  `[[agents.mcp]]`; `[shortcuts] stop-agents` (also in the actions
  palette, "Arrêter tous les agents").
- **The tag** is one card, the name's violet band over the intent, so
  that the compositor rounds its corners on the CPU as it does the
  bubbles'; the window's ring is the compositor's, 2 px at 55 % violet, 2
  px outside the window, on the screen only (never in a capture or a
  share). A desktop's tile is ringed only while an action on it shows.
- **The toast** says the request's first intent quoted under its
  sentence (no "il y a 4 s": nothing draws for a clock), "Ne s'annule
  pas" when nothing of it can be undone. A setting is named by its key
  (`changé appearance.theme`): the Settings' row names are theirs, not
  the shell's.
- **The journal's popover** lists requests (one line each, as the toast
  sums them up) and refusals, the last 8; the journal itself (the
  `journal` tool) keeps every call. Its "Annuler" works while the
  request is among the last 20.
- **The agents' group**: a row per MCP agent seen this session, its mark
  (its initial on the violet) and what it does on two lines; a click
  opens the journal. Its rows count in the group's total, not in the
  secretary's line.
- **Tests** (`crates/hocus-compositor/tests/mcp.rs`), each with a real
  `hocus mcp`: initialize and the tools, `list_windows`, a read agent and
  an unknown one refused, a move shown before it is done and its toast,
  a setting put back by the toast's Annuler (the real shell, clicked),
  the stop key. Screenshots of scenes 1, 2 and 4: `HOCUS_SHOTS=DIR cargo
  nextest run -p hocus-compositor mcp::shots --run-ignored only`.
