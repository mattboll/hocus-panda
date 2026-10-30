# Telling the desktop what an agent is doing

Hocuspanda shows each agent's state in its bar: working, waiting for you,
done, failed, idle (docs/design/bar.md §4 group 3). Claude Code and Codex
are followed through their hooks, once plugged in from the bar; herdr is
read while it runs. Any other tool can say the same with one command.

## The command

```sh
hocus agent NAME STATE [--pid PID]
```

- `NAME`: the name the agent shows under (a project, a task). The same
  name again is the same agent.
- `STATE`: `working`, `waiting` (it waits for the user: a permission, a
  question), `done` (its turn ended, nobody looked yet), `failed`, `idle`;
  or `ended`, to forget it.
- `--pid PID`: the agent's process. By default, the process that ran
  `hocus` (above the shells it ran through). The agent's window is the
  first window found above that process (its terminal, its editor), and
  the agent is forgotten when that process ends.

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

## The line, for tools that write it themselves

One line of text per report on the Unix socket, words separated by
spaces:

```
agent SOURCE KEY STATE PID NAME
```

`SOURCE` is `command` for any tool (`claude-code` and `codex` are the
hooks'); `KEY` the agent's own name, one word; `STATE` as above;
`PID` a process id or `-`; `NAME` the rest of the line. The connection may
carry several lines; nothing is answered.

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
| `Stop`; Codex's `notify` (`agent-turn-complete`) | yes | yes | done |
| `StopFailure` | yes | | failed |
| `Interrupt` | | yes | idle |
| `SessionEnd` | yes | yes | forgotten |

Plugging in writes these hooks into `~/.claude/settings.json` (or
`~/.codex/config.toml`, with `notify` when there is none), after the
user's click in the bar, and keeps the file as it was beside it
(`….before-hocuspanda`); unplugging puts it back, or takes only these
entries out if the file changed since. Codex runs a new hook only once
the user trusted it with `/hooks`.
