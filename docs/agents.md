# Telling the desktop what an agent is doing

Hocuspanda shows each agent's state in its bar: working, waiting for you,
done, failed, idle, in the background (docs/design/bar.md §4 group 3). Claude Code and Codex
are followed through their hooks, once plugged in from the bar; herdr is
read while it runs. Any other tool can say the same with one command.

## The command

```sh
hocus agent NAME STATE [--pid PID] [--tests-passed [N]]
```

- `NAME`: the name the agent shows under (a project, a task). The same
  name again is the same agent.
- `STATE`: `working`, `waiting` (it waits for the user: a permission, a
  question), `done` (its turn ended, nobody looked yet: the bar says "à
  voir" until its window gets the keyboard or its row is clicked, then
  "repos"), `failed`, `idle`, `background` (its turn ended and something
  it started still runs: "à voir" until looked at, then "en fond", until
  it says another state); or `ended`, to forget it.
- `--pid PID`: the agent's process. By default, the process that ran
  `hocus` (above the shells it ran through). The agent's window is the
  first window found above that process (its terminal, its editor); for a
  process in one of herdr's panes, which runs under herdr's server, the
  window of the terminal running herdr's client. The agent is forgotten
  when that process ends.

- `--tests-passed [N]`: its tests all passed (N of them, when known). An
  agent that finishes (`done`) with all its tests passing sets the
  fireworks off (`[effects] fireworks`, [design/effects.md](design/effects.md),
  "The moments"), and its row says "au vert" while it is to see.

It writes one line to the desktop's control socket and returns at once;
outside a Hocuspanda session it fails with a message and exit status 1, a
state that is not one with status 2. The socket is `$HOCUS_SOCKET` (set
for the programs the desktop starts), else `<the Wayland socket>.control`
in `$XDG_RUNTIME_DIR`, or `--socket PATH`.

For example, a build script:

```sh
hocus agent build working
make && hocus agent build done || hocus agent build failed
```

## Good news

```sh
hocus celebrate WHY [--name NAME] [--pid PID]
```

An agent's good news, in one short line: a pull request merged, a
deployment done, a bug finally fixed (`hocus celebrate "#791 merged on
main"`). The row of the agent whose process ran it (above its shells,
or `--pid`) says why on a violet card for a minute, or until its row is
clicked; when no agent is known for that process, a row of its own does,
named `NAME` (else the current folder's name). The confetti fly when
`[effects] confetti` is on ([design/effects.md](design/effects.md), "The
moments"). The line is `celebrate PID NAME`, a tab, then why. Agents
acting through `hocus mcp` have the same as their `celebrate` tool
([design/mcp.md](design/mcp.md)).

## Tests that pass

The hooks also say how a shell command's tests went: in `PostToolUse` and
`PostToolUseFailure`, the command's output is read for a test runner's
summary line (cargo test, cargo-nextest, pytest, Jest, Vitest, go test):
all passed, with their count, or one failed; a test command (`cargo
test`, `nextest`, `pytest`, `npm test`, `jest`, `vitest`, `go test`, `make
test`) that failed without a summary failed. The last run of a turn
counts; the turn over, it is forgotten. A turn that ends (`Stop`) with
its last run green is a finish with all tests passing. On the line, after
a tab: `tests passed [N]` or `tests failed`.

## How an agent is named

The bar names each agent by the best of what its sources know, never by
its tool ("claude", "Codex"):

1. **Its Claude Code session's own name**, given with `/rename`: the last
   `custom-title` line of the transcript its hooks name, read as each hook
   is heard, so a rename shows at the session's next hook (#886).
2. **herdr's name**, for an agent herdr sees (its own, or a Claude Code or
   Codex running in one of its panes): its tab's own name when the tab
   was given one ("refacto"), else its workspace's label ("hocus-panda";
   a tab herdr labels with its place, "1", "2", has no name of its own).
   Renaming the workspace or the tab in herdr renames the row at once.
3. **Its project's folder**: the last part of the hook's `cwd` (the first
   event's, kept when later events have none or a directory the agent went
   down into), or the command's `NAME`.
4. **Its process's working directory**, when no event of its session
   carries a `cwd`: the last part of `/proc/PID/cwd`.

## The line, for tools that write it themselves

The control socket's lines, their answers, its version and the events a
program can subscribe to (an agent's state among them) are documented in
[api.md](api.md); this section details the agents' own.

One line of text per report on the Unix socket, words separated by
spaces:

```
agent SOURCE KEY STATE PID NAME
```

`SOURCE` is `command` for any tool (`claude-code` and `codex` are the
hooks'); `KEY` the agent's own name, one word; `STATE` as above;
`PID` a process id or `-`; `NAME` the rest of the line (empty for a
hook's agent whose folder is not known yet; a command's is then its
`KEY`). The connection may carry several lines; nothing is answered.

The line may end with a tab and what the agent asks for, or why it
failed, which the secretary's briefing shows (bar.md §7 B): `allow TOOL
WHAT` (it asks to use `TOOL`, one word, on `WHAT`) or `says TEXT`. One
line, cut to 120 characters. It may hold a command or a path: the desktop
never logs it.

A hook's line may also carry, each after a tab, its conversation
(docs/design/projects.md §8.1): `cwd PATH` (the hook's `cwd`),
`transcript PATH` (its `transcript_path`), both absolute paths, and
`prompt TEXT` (`UserPromptSubmit`'s prompt, on one line, cut at 60
characters). The desktop keeps the first `cwd`, the last transcript and
the first prompt of the agent's session, never logs them, saves them
with a project the agent's window is on, and resumes the conversation
there when the project reopens (`claude --resume ID`, `codex resume
ID`). The first `cwd` of an agent is also written to
`$XDG_STATE_HOME/hocuspanda/folders`, which the project's form and the
launcher propose.

```
agent claude-code 3f6c2a1e working 4242 hocus-site\tcwd /home/alex/github/hocus-site\ttranscript /home/alex/.claude/projects/-home-alex-github-hocus-site/3f6c2a1e.jsonl\tprompt refais la section du haut
```

## What the hooks say

`hocus agent-hook TOOL` is what Claude Code's and Codex's hooks run once
plugged in. It reads the hook's JSON on its standard input (Codex's
`notify` gives it as an argument), prints nothing the tool would take as
an answer, and always succeeds.

| Event | Claude Code | Codex | State |
|---|---|---|---|
| `SessionStart` | yes | yes | idle |
| `UserPromptSubmit`, `PostToolUse`, `PostToolUseFailure` | yes | yes (not the last) | working |
| `PermissionRequest` | yes | yes | waiting |
| `PreToolUse` of `AskUserQuestion`, `ExitPlanMode` | yes | | waiting |
| `Notification`: `permission_prompt`, `elicitation_dialog`, `elicitation_url_dialog` | yes | | waiting |
| `Notification`: `idle_prompt` | yes | | done |
| `Stop`; Codex's `notify` (`agent-turn-complete`) | yes | yes | done; Claude Code's: and its tasks in flight ([In the background](#in-the-background)) |
| `StopFailure` | yes | | failed |
| `Interrupt` | | yes | idle |
| `SessionEnd` | yes | yes | forgotten |
| `SubagentStart`, `SubagentStop` | yes | | none: a sub-agent running, ended, and `SubagentStop`'s tasks in flight ([In the background](#in-the-background)) |

The hooks also say what a waiting agent asks and why a failed one
stopped: a `PermissionRequest`'s tool and its input in one line (the
command it runs, the file it writes, by its name), the question of
`AskUserQuestion`, a waiting `Notification`'s `message`, a
`StopFailure`'s error message.

What runs on is said in lines `ran SOURCE KEY PID WHAT`, `WHAT` being
`subagent-start ID`, `subagent-stop ID` or `tasks N` (how many
`background_tasks` the event lists), after the event's report when it
has one.

Plugging in writes these hooks into `~/.claude/settings.json` (or
`~/.codex/config.toml`, with `notify` when there is none), after the
user's click in the bar, and keeps the file as it was beside it
(`….before-hocuspanda`); unplugging puts it back, or takes only these
entries out if the file changed since. Codex runs a new hook only once
the user trusted it with `/hooks`. Claude Code plugged in before the sub-agents'
hooks came (#831) has none of them: unplugging it and plugging it in
again, from Sources, adds them.

## In the background

An agent whose turn ended may have left something running that will
bring it back by itself: a command in the background, a sub-agent, a
monitor, a workflow. Once the user looked, its row then says "en
fond" (a blue dot) rather than "repos"; not looked at
yet, "à voir" wins (docs/design/bar.md §4 group 3). It is quiet: no halo,
no panda peeking out, no notification, nothing in the mini form's
badge. When what ran ends and the agent takes a turn by itself, it works
again; when it does not come back, it rests (or stays "à voir").

For the agents of Claude Code's and Codex's hooks the desktop tells it by
itself:

- **Claude Code** says it. Its `Stop` lists the session's tasks still in
  flight (`background_tasks`: commands, background sub-agents, monitors,
  workflows…); any there is "en fond". `SubagentStop` lists them again
  as a sub-agent ends, and `SubagentStart` and `SubagentStop` count the
  sub-agents still running, for a Claude Code whose events carry no list.
  What it listed holds until its next turn starts (a task that ends
  brings it back: its next `Stop` lists what still runs), a
  `SubagentStop` lists them again, or its session ends. Its scheduled
  wake-ups (`session_crons`: `CronCreate`, `ScheduleWakeup`, `/loop`) do
  not count: nothing of them runs until they fire, which may be days
  away. Its processes are never read: a language server, an MCP
  server's relay, a shell left over are not work it waits for.
- **Codex**: the agent's process (the one above its hooks) has
  children still running. They are read in `/proc` off the desktop's
  loop, only for agents whose turn ended (`Stop`,
  `agent-turn-complete`): once then, and every 2 s while one runs. Left
  out: its MCP servers (the children started before its first turn, and
  those whose command line is one of its configured servers':
  `[mcp_servers]` in its `config.toml`), its hooks' own `hocus`, the
  zombies, and a child younger than a second (counted at the next
  reading, if it still runs).

Any other tool says it with `hocus agent NAME background`.

## Their notifications

A followed tool's desktop notification that only says its agent waits or
has finished ("Claude is waiting for your input", "Claude needs your
permission to use Bash") makes no bubble: the agent's row, its panda and
the halo say it already. It goes to the notifications' history only. Its
other notifications are bubbles. How one is recognised:
[notifications.md](design/notifications.md#agents-notifications).
