//! What a tool's hook says, as an agent's state: one place for every
//! tool's mapping (docs/agents.md has the table).
//!
//! Claude Code and Codex run a hook with one JSON object on its standard
//! input, `hook_event_name` naming the event; their events share names.
//! Codex's older `notify` program gets one JSON argument instead, whose
//! `type` is `agent-turn-complete`.
//!
//! - `SessionStart`: idle. `UserPromptSubmit`, `PostToolUse`,
//!   `PostToolUseFailure`: working (the user answered, the agent goes on).
//! - `PermissionRequest`, and `PreToolUse` of the tools that ask the user
//!   (`AskUserQuestion`, `ExitPlanMode`): waiting. Claude Code's
//!   `Notification` of a permission or of an MCP server's form: waiting
//!   (it comes 6 s late, but also covers the sandbox's network prompts,
//!   which have no `PermissionRequest`).
//! - `Stop`, Codex's `notify`, Claude Code's `idle_prompt` (a minute after
//!   a turn ended, also when the user interrupted it, which runs no
//!   `Stop`): done. `StopFailure` (an API error): failed. Codex's
//!   `Interrupt`: idle.
//! - `SessionEnd`: the agent is forgotten.

use serde_json::Value;

use crate::State;

/// The tools whose hooks call `hocus agent-hook`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    ClaudeCode,
    Codex,
}

impl Tool {
    pub fn source(self) -> crate::Source {
        match self {
            Self::ClaudeCode => crate::Source::ClaudeCode,
            Self::Codex => crate::Source::Codex,
        }
    }

    /// Its name on the command line, as its hooks write it.
    pub const fn word(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        [Self::ClaudeCode, Self::Codex]
            .into_iter()
            .find(|t| t.word() == word)
    }

    /// The name people know it by.
    pub const fn title(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }
}

/// What a hook says of its agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// Its session.
    pub key: String,
    /// Its project's directory's name (its `cwd`'s last part), when the
    /// event carries one.
    pub name: Option<String>,
    /// Its state; `None`: its session ended.
    pub state: Option<State>,
}

/// What `payload`, from a tool's hook, says; nothing for an event that
/// changes nothing (a subagent stopping, a compaction). The tools share
/// their events' names and fields.
pub fn said(_tool: Tool, payload: &Value) -> Option<Said> {
    let text = |key: &str| payload.get(key).and_then(Value::as_str);
    // Codex's `notify`: dashes, and its own type.
    if let Some(kind) = text("type") {
        if kind != "agent-turn-complete" {
            return None;
        }
        return Some(Said {
            key: text("thread-id").unwrap_or_default().to_owned(),
            name: text("cwd").and_then(crate::folder),
            state: Some(State::Done),
        });
    }
    let event = text("hook_event_name")?;
    let state = match event {
        "SessionStart" => Some(State::Idle),
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" => Some(State::Working),
        "PermissionRequest" => Some(State::Waiting),
        "PreToolUse" => match text("tool_name") {
            Some("AskUserQuestion" | "ExitPlanMode") => Some(State::Waiting),
            _ => Some(State::Working),
        },
        "Notification" => match text("notification_type")? {
            "permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog" => {
                Some(State::Waiting)
            }
            "idle_prompt" => Some(State::Done),
            _ => return None,
        },
        "Stop" => Some(State::Done),
        "StopFailure" => Some(State::Failed),
        "Interrupt" => Some(State::Idle),
        "SessionEnd" => None,
        _ => return None,
    };
    Some(Said {
        key: text("session_id").unwrap_or_default().to_owned(),
        name: text("cwd").and_then(crate::folder),
        state,
    })
}

/// What the hook prints on its standard output for `event`: nothing,
/// except for Codex's `Stop`, which expects JSON (an empty object changes
/// nothing). Claude Code adds a plain output of some events to the
/// agent's context, so nothing is ever printed for it.
pub fn answer(tool: Tool, payload: &Value) -> &'static str {
    let event = payload.get("hook_event_name").and_then(Value::as_str);
    match (tool, event) {
        (Tool::Codex, Some("Stop" | "SubagentStop")) => "{}",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn state(tool: Tool, payload: Value) -> Option<Option<State>> {
        said(tool, &payload).map(|s| s.state)
    }

    #[test]
    fn claude_codes_events_as_its_documentation_shows_them() {
        // The payloads of code.claude.com/docs/en/hooks.
        let start = json!({ "session_id": "abc123",
            "transcript_path": "/Users/.../transcript.jsonl", "cwd": "/home/alex/hocus-panda",
            "hook_event_name": "SessionStart", "source": "startup", "model": "claude-opus-5" });
        assert_eq!(
            said(Tool::ClaudeCode, &start),
            Some(Said {
                key: "abc123".into(),
                name: Some("hocus-panda".into()),
                state: Some(State::Idle),
            })
        );
        let prompt = json!({ "session_id": "abc123", "cwd": "/w",
            "permission_mode": "default", "hook_event_name": "UserPromptSubmit",
            "prompt": "Write a function to calculate the factorial of a number" });
        assert_eq!(state(Tool::ClaudeCode, prompt), Some(Some(State::Working)));
        let permission = json!({ "session_id": "abc123", "cwd": "/w",
            "hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": { "command": "rm -rf node_modules" },
            "permission_suggestions": [] });
        assert_eq!(
            state(Tool::ClaudeCode, permission),
            Some(Some(State::Waiting))
        );
        let question = json!({ "session_id": "abc123", "hook_event_name": "PreToolUse",
            "tool_name": "AskUserQuestion", "tool_input": { "questions": [] },
            "tool_use_id": "toolu_01" });
        assert_eq!(
            state(Tool::ClaudeCode, question),
            Some(Some(State::Waiting))
        );
        let answered = json!({ "session_id": "abc123", "hook_event_name": "PostToolUse",
            "tool_name": "Write", "tool_input": {}, "tool_response": {}, "tool_use_id": "t" });
        assert_eq!(
            state(Tool::ClaudeCode, answered),
            Some(Some(State::Working))
        );
        let notified = |kind: &str| {
            json!({ "session_id": "abc123", "cwd": "/w", "hook_event_name": "Notification",
                "message": "Claude needs your permission", "title": "Permission needed",
                "notification_type": kind })
        };
        assert_eq!(
            state(Tool::ClaudeCode, notified("permission_prompt")),
            Some(Some(State::Waiting))
        );
        assert_eq!(
            state(Tool::ClaudeCode, notified("idle_prompt")),
            Some(Some(State::Done))
        );
        assert_eq!(state(Tool::ClaudeCode, notified("auth_success")), None);
        let stop = json!({ "session_id": "abc123", "hook_event_name": "Stop",
            "stop_hook_active": true, "last_assistant_message": "I've completed the refactoring.",
            "background_tasks": [], "session_crons": [] });
        assert_eq!(state(Tool::ClaudeCode, stop), Some(Some(State::Done)));
        let failed = json!({ "session_id": "abc123", "hook_event_name": "StopFailure",
            "error": "rate_limit", "error_details": "429 Too Many Requests",
            "last_assistant_message": "API Error: Rate limit reached" });
        assert_eq!(state(Tool::ClaudeCode, failed), Some(Some(State::Failed)));
        let subagent = json!({ "session_id": "abc123", "hook_event_name": "SubagentStop",
            "agent_id": "def456", "agent_type": "Explore" });
        assert_eq!(state(Tool::ClaudeCode, subagent), None);
        let end = json!({ "session_id": "abc123", "cwd": "/w",
            "hook_event_name": "SessionEnd", "reason": "other" });
        assert_eq!(state(Tool::ClaudeCode, end), Some(None));
        assert_eq!(state(Tool::ClaudeCode, json!({ "nothing": 1 })), None);
    }

    #[test]
    fn claude_codes_events_name_their_project_never_the_tool() {
        // Every event carries the common fields, `cwd` among them
        // (code.claude.com/docs/en/hooks, "Common input fields").
        let common = |event: &str| {
            json!({ "session_id": "abc123", "prompt_id": "p1",
                "transcript_path": "/home/alex/.claude/projects/x/abc123.jsonl",
                "cwd": "/home/alex/hocus-panda", "permission_mode": "default",
                "hook_event_name": event, "notification_type": "permission_prompt" })
        };
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "Notification",
            "Stop",
            "StopFailure",
            "SessionEnd",
        ] {
            let said = said(Tool::ClaudeCode, &common(event)).expect(event);
            assert_eq!(said.name.as_deref(), Some("hocus-panda"), "{event}");
        }
        let mut bare = common("Stop");
        bare.as_object_mut().map(|o| o.remove("cwd"));
        assert_eq!(said(Tool::ClaudeCode, &bare).map(|s| s.name), Some(None));
    }

    #[test]
    fn codexs_hooks_and_its_notify_program() {
        // The payloads of developers.openai.com/codex/hooks and of the
        // advanced configuration's `notify`.
        let end = json!({ "session_id": "thr_123", "transcript_path": "/workspace/.codex/rollout.jsonl",
            "cwd": "/workspace", "hook_event_name": "SessionEnd", "reason": "other" });
        assert_eq!(state(Tool::Codex, end), Some(None));
        let permission = json!({ "session_id": "thr_123", "cwd": "/workspace",
            "hook_event_name": "PermissionRequest", "model": "gpt-5", "turn_id": "t1",
            "tool_name": "Bash", "tool_input": { "command": "cargo publish", "description": null } });
        assert_eq!(state(Tool::Codex, permission), Some(Some(State::Waiting)));
        let interrupted = json!({ "session_id": "thr_123", "hook_event_name": "Interrupt",
            "turn_id": "t1", "permission_mode": "default" });
        assert_eq!(state(Tool::Codex, interrupted), Some(Some(State::Idle)));
        let stop = json!({ "session_id": "thr_123", "hook_event_name": "Stop",
            "turn_id": "t1", "stop_hook_active": false, "last_assistant_message": null });
        assert_eq!(answer(Tool::Codex, &stop), "{}");
        assert_eq!(answer(Tool::ClaudeCode, &stop), "");
        let notify = json!({ "type": "agent-turn-complete", "thread-id": "thr_123",
            "turn-id": "t1", "cwd": "/home/alex/infra", "input-messages": ["deploy"],
            "last-assistant-message": "Done." });
        assert_eq!(
            said(Tool::Codex, &notify),
            Some(Said {
                key: "thr_123".into(),
                name: Some("infra".into()),
                state: Some(State::Done),
            })
        );
        assert_eq!(
            state(Tool::Codex, json!({ "type": "approval-requested" })),
            None
        );
        // No directory: no name, never the tool's (the agent's process's
        // directory names it, `hocus agent-hook`).
        let bare = json!({ "session_id": "s", "hook_event_name": "Stop" });
        assert_eq!(said(Tool::Codex, &bare).map(|s| s.name), Some(None));
    }
}
