# Agents on the desktop, through `hocus mcp` (#440)

Status: lots A, B and C of #440, mockup approved 2026-11-12 (tools'
`mockups/s14-mcp`, scenes 1 to 5;
<https://claude.ai/artifact/Cd6Ba69odbhqgaFB5ux8pg>). Lot B (below, "In
applications") brings the agent's own seat, applications' accessible
trees and captures, the confirmation card and taking over; lot C the
Settings' page of rights (scene 5, "Agir sur le bureau",
docs/design/settings.md), which writes the file below.

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

Settings › Agir sur le bureau › « Brancher un autre agent… » plugs
Claude Code in, with the user's click:

```sh
claude mcp add --scope user hocuspanda -- hocus mcp --agent Claude
```

## Out of sight (#790)

An agent works without moving what the user sees. `launch` with a
`desktop` opens the application's first window on that desktop (its id,
the id's last part or its entry's `StartupWMClass`, within 30 s), without
the keyboard; the desktop the user sees stays. `switch_desktop`, and
`focus_window` on a window of another desktop, change what the user sees:
their descriptions tell the client to call them only when the user asked
(the desktop cannot check).

## Rights

```toml
[[agents.mcp]]
name = "Claude"        # what it shows under, and --agent's NAME
access = "act"         # "read" (default): only the reading tools
windows = true         # arrange, move, bring forward, close (default true)
desktops = true        # switch desktops, send a window, launch (default true)
settings = true        # may change settings (default false)
input = true           # click and type in its apps (default true)
client = "claude-code" # what the Settings plugged it into, and the day
since = "2026-11-12"
```

Each right is a check box of the Settings' page; an `act` agent's call
needing one it does not hold is refused, saying which (`move_window` with
a place needs `windows`, with a desktop `desktops`).

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
| `launch` | `app` (a desktop entry's id, else its name, ignoring case), `desktop`, `output` (default the pointer's screen), `intent` | started, as the launcher starts it; with `desktop`, its first window opens there |
| `set_setting` | `key` (`appearance.theme`), `value`, `intent` | written into the configuration file in place (hocus-config), `before` and `after`. Only keys `packaging/config.toml` lists, with a value of their default's type |
| `celebrate` | `why` (120 characters at most) | good news (#809): the agent's row says why until its next request, the confetti fly when `[effects] confetti` is on; `celebrated`, and whether the `confetti` flew. Any agent the file names, a `read` one too (it touches nothing); refused while locked or stopped |

There is no `arrange`: an agent composes moves. Lot B's tools are under
"In applications".

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

## In applications (lot B, scenes 1 and 3)

Ported from v0-first-build's `docs/mcp.md` ("A seat of its own",
"Applications' trees", "It shows before acting", "Take over", "A desktop
it works on, unseen"), as it held up.

```toml
[[agents.mcp]]
name = "Claude"
access = "act"
apps = ["kitty", "org.mozilla.*"]   # app ids, globs; none by default

[agents]
# Each click, typing, keys or closing there waits for the user's card.
confirm-apps = ["hocus-settings", "org.keepassxc.KeePassXC", "com.bitwarden.desktop",
                "1password", "org.gnome.seahorse.Application", "org.gnome.World.Secrets",
                "org.kde.kwalletmanager5"]
```

`apps` names the applications whose windows an `act` agent may click,
scroll, type and press keys in, and whose trees any agent reads; another
is refused ("pas dans ses applications"). Capturing a window and closing
it are not limited by `apps` (a password manager's window comes black in
a capture, as in the user's own).

| Tool | Arguments | Result |
|---|---|---|
| `click` | `window`, and `x`, `y` (from its top left, logical; its centre when left out) or `label` (and `role`) of its tree; `button` (`left`, `right`, `middle`), `count` (1 to 3), `intent` | where it landed |
| `scroll` | the same aim, `dy` (logical pixels, 15 a notch, down positive), `intent` | where |
| `type` | `window`, `text` (500 characters at most, with the layout in effect), `intent` | how many characters |
| `press_keys` | `window`, `keys` (`Ctrl+S`, `Return`, `Shift+Tab`), `intent` | the keys |
| `close_window` | `window`, `intent` | `closed`, or why it stays: its own dialog (its title), or it did not go in 2 s |
| `wait_for` | `app_id` and/or `title` (a part of it), or `window` and `label`; `gone`; `timeout_ms` (5000, at most 30000) | whether it came (or went) |
| `capture` | `window` | a PNG of it, its logical size and scale |
| `accessibility_tree` | `window` | its elements, as below |

**A seat of its own.** Each connection of an agent the file names gets a
`wl_seat` of its own, `agent-NAME`, with a pointer and a keyboard (the
user's layouts, the one in use): its clicks and keys reach the window
asked, even under another, while the user's pointer, keyboard focus and
stacking stay as they are; nothing comes forward for it. Its keyboard
follows its clicks. The keys it types go to that window alone, never to
the desktop's shortcuts (the stop key among them). It works on a desktop
no screen shows too: while it acted on a window there in the last 10 s
the window gets a frame callback 10 times a second, and a capture of it
wakes it first and waits for it to draw (200 ms at most). The seat rests
when the connection ends (no focus, no pointer drawn) and the agent's next
connection takes it again; it is never taken away, since applications
still ask its pointer's cursor shape after, which ended the session.
Limits: an application must take every seat (GTK
and Qt do): one that has not taken the agent's 2 s after it came is
refused, saying so; X11 windows are refused, saying so; an agent's click
never starts a move or a resize, nor grabs a menu; its seat's selection
and cursor shapes are its own (never the clipboard's history).

**Trees** (`accessibility_tree`, and `click`/`scroll`/`wait_for` by
`label`): what the window's application tells screen readers over
AT-SPI, on the accessibility bus of the session's bus (`org.a11y.Bus`;
none nested without `--session-bus`, and the tool says so). Each element
depth first: `role` (`push button`, `entry`, `password text`), `label`,
`rect` from the window's top left (as the application says it), `depth`,
`focused`, `disabled`, `selected`, `checked`, and what a field holds
(its first 200 characters); a password field's content is never read
(its role, or a label saying password, passphrase, passcode, PIN, mot de
passe). Read breadth first within 800 elements, 40 levels and 3 s
(`partial` says what was left out); what is not showing is left out. The
window is its application's by the process that drew it, else its app
id, and its top-level element by its title (and size). A label matches
exactly, else the one label holding it (ignoring case), `role` choosing
among several; several left are refused with them listed.

**What shows** (scene 1). The agent's pointer is a violet arrow with its
name beside it, where it acts and for 10 s after, where a screen shows
that point. Before an action on a window shown, the pointer and a ring
(34 px, 2 px of violet and a 6 px halo) go to the point (a click's, else
the window's top, its middle) with the intent under the name, for
`show-before-ms`, the window ringed as in lot A; the rights are checked
again, then it acts. On a window no screen shows, lot A's tag at the top
of the screen.

**The confirmation card** (scene 3) beside the point, the agent's name
and what it wants ("Claude veut taper dans « Notes »"), its intent
quoted, why it asks, and Autoriser (or « Fermer quand même »), Refuser,
« Je prends la main »; « Sans réponse dans 60 s, c'est refusé. » It
comes for typing or keys where the window's application says through
text-input-v3 (on the agent's seat) that the field is for a password or
a PIN, and for clicks, typing, keys and closing in an application of
`confirm-apps`. Scrolling never waits. It takes the pointer, and the
keyboard only once clicked: then Enter allows, Escape refuses. Past 60 s
it is refused. While it waits the agent's row says "attend ta réponse".

**Taking over**: the card's « Je prends la main » refuses that action,
brings its window forward with the user's keyboard, and refuses the
agent's actions ("tu as pris la main") until « Rendre la main » in the
journal's popover; the agent's row says "tu as la main".

**Closing** asks the application (`xdg_toplevel.close`, or X11's), as
the window's ✕ does. The compositor cannot know whether the application
holds unsaved work (no protocol says it), so it never claims to: the card
asks every time an agent closes a window of a `confirm-apps` application,
and never for another. The application's own "save changes?" dialog
stays the application's: the tool says whether the window went, stays,
or shows a dialog, and the agent decides what to do in it.

**The journal and the toast** count what cannot be undone ("cliqué 2
fois", "tapé 12 caractères", "fermé 1 fenêtre"): the toast says « Ne
s'annule pas » for them. What is typed is never kept, its characters
counted; keys are kept (`Ctrl+S`).

## The journal

In the compositor's memory only, never on disk nor in its log above
debug: the last 500 actions and refusals of the session, each with its
time (as the clock says it), the agent, the tool, what it touched (the
window's app id and title then, the desktop, the setting's key; a
setting's value only for keys that are not text), its intent, and the
refusal's reason. What an agent types is counted, never kept. `journal`
gives an agent its own entries.

## Stop

`Super+Shift+Escape` (`[shortcuts] stop-agents`) or the journal's "Tout
arrêter" stops every agent: from then on each call is refused (reading
too), an action shown and not done yet is not done, and the bar's
agents' row says "arrêté". The same key, or the journal's "Reprendre",
lets them go on. It lasts until then or the session's end. An agent's
typing never reaches the desktop's shortcuts, so it cannot press it. A
card waiting is refused too.

## Limits

One action at a time per agent, the next waiting for the one shown; an
agent whose connection ends has what it waited for dropped; its row
and its journal stay for the session. An agent the configuration does
not name has no row. Undo applies
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
  bubbles'; the window's ring is the compositor's, 3 px of violet 2 px
  outside the window and a 6 px violet halo around it, on the screen
  only (never in a capture or a share). A desktop's tile takes the same
  violet (a soft fill, 3 px inside, a 3 px halo) only while an action on
  it shows.
- **The toast** says the request's first intent quoted under its
  sentence (no "il y a 4 s": nothing draws for a clock), "Ne s'annule
  pas" when nothing of it can be undone. A setting is said with the
  Settings' own words, its row's name and its choice's label ("mis
  « Thème » sur « Sombre »", "activé « Mouvements réduits »"), from a
  table of the settings agents most often change (the shell does not
  link the Settings' model); another is "changé un réglage". The journal
  says it the same way.
- **The journal's popover** lists requests (one line each, as the toast
  sums them up) and refusals, the last 8; the journal itself (the
  `journal` tool) keeps every call. Its "Annuler" works while the
  request is among the last 20.
- **The agents' group**: a row per MCP agent seen this session, its mark
  (its initial on the violet) and what it does on two lines; a click
  opens the journal. An agent acting counts as at work in the group's
  head.
- **Tests** (`crates/hocus-compositor/tests/mcp.rs`), each with a real
  `hocus mcp`: initialize and the tools, `list_windows`, a read agent and
  an unknown one refused, a move shown before it is done and its toast,
  a setting put back by the toast's Annuler (the real shell, clicked),
  the stop key. Screenshots of scenes 1, 2 and 4: `HOCUS_SHOTS=DIR cargo
  nextest run -p hocus-compositor mcp::shots --run-ignored only`.

## As built (2026-11-12, lot B)

- **Where**: the compositor's `src/mcp/` (`seat.rs` the agent's seat and
  its input, `apps.rs` the card, take over, closing, `wait_for` and the
  trees asked, `atspi.rs` the reader on its thread, `capture.rs`,
  `draw.rs` the pointer and its ring), `text_input.rs` (each text input
  its seat's, a password's purpose), the shell's `src/mcp/card.rs`.
- **The pointer** is the mockup's arrow and ring, drawn by the compositor
  on the CPU once per scale (the shell's surfaces have no alpha on the
  CPU); its name, and its intent before it acts, are the shell's tag
  beside it ([`McpAt::Pointer`], `McpShown::pointers`), never in a
  capture or a share.
- **Closing**: the compositor cannot know about unsaved work, so the
  card's text says why it asks (`confirm-apps`), never "will be lost".
- **The settings' words** in the toast: lot A's table until lot C (see
  there).
- **Not ported from v0**: an agent clicking the shell's own elements
  (`shell = true`), turning the session's accessibility on while agents
  are there (`org.a11y.Status`), the column's live thumbnail of the
  agent's window.
- **Tests** (`tests/mcp.rs`, a real `hocus mcp` each): a click through
  the agent's seat with the user's focus kept, typing and the stop key
  into a window alone, a click by label on a fake tree, a password
  field's card refused, taking over, a capture on a desktop no screen
  shows. Scenes 1 and 3: `HOCUS_SHOTS=DIR cargo nextest run -p
  hocus-compositor mcp::shots_in_apps --run-ignored only`.

## As built (2026-11-12, lot C)

- **The page**: Settings › Agir sur le bureau (docs/design/settings.md,
  "Agir sur le bureau"), a section of its own after Agents, as the
  mockup's sidebar shows it. The rights the mockup draws as four check
  boxes are four keys of the agent's table (`windows`, `desktops`,
  `settings`, `input`); `windows`, `desktops` and `input` default to true
  so that lot A's and B's files keep what they allowed, `settings` to
  false. The compositor checks them at each call (`Call::needs`,
  `hocus-agents`; refusals `no-windows`, `no-desktops`, `no-input`).
- **Plugging in**: Claude Code's MCP server is added in its user's scope
  (`claude mcp add --scope user`), so that "dans tous tes projets" holds;
  the agent is written here only once `claude` took it, read-only, with
  `client` and `since`. Without `claude` on the PATH the sheet shows the
  line to paste and « Brancher » adds the agent here. Another client gets
  the JSON of the server and an agent named "Agent" (no field to name it
  in this lot).
- **« Voir son journal »** asks the compositor (`mcp-journal` on the
  Settings' line), which tells the shell (`ToShell::Journal`): the
  journal's popover opens on the focused screen's bar.
- **The settings' words**: every key a row of the Settings writes is
  named by `hocus_config::labels` (its row's name, its choices' labels, on
  or off for a switch, a number with its unit), generated from the
  Settings' model and checked against it by its test; the shell's table
  of nine went. Keys several rows share (`[keyboard] options`) and the
  shortcuts (which no agent changes) are left out: "changé un réglage".
- **Tests**: `tests/settings_acting.rs` (the page, with a fake `claude`),
  `hocus-agents`' `each_acting_call_needs_its_check_box`, the shell's
  `a_request_is_one_sentence` (a key lot A's table did not have, named by
  its row). Scene 5: `HOCUS_SHOTS=DIR cargo nextest run -p
  hocus-compositor settings_acting::shots --run-ignored only`.
