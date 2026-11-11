//! The line `hocus mcp` holds with the compositor (docs/design/mcp.md):
//! opened on the control socket with `mcp NAME`, the agent named as the
//! configuration's `[[agents.mcp]]` names it, then one JSON object a line
//! each way. `hocus mcp` asks `{"id":1,"tool":"list_windows","args":{}}`
//! ([`Asked`]); the compositor answers `{"id":1,"ok":…}` or
//! `{"id":1,"error":"…"}` ([`Answer`]), the error being what the agent
//! reads. The compositor does every tool ([`Call`]): the rights, what
//! shows, the journal and undoing are kept in one place.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How the line's first line starts.
const OPENING: &str = "mcp ";

/// An intent is one short line: longer is cut.
pub const MAX_INTENT: usize = 120;

/// The line that opens an agent's connection.
pub fn opening(agent: &str) -> String {
    format!("{OPENING}{}", crate::name_text(agent))
}

/// The agent a line opens a connection for, if it opens one.
pub fn agent_of(line: &str) -> Option<String> {
    let name = line.trim_end_matches(['\n', '\r']).strip_prefix(OPENING)?;
    let name = crate::name_text(name);
    (!name.is_empty()).then_some(name)
}

/// A tool call, as `hocus mcp` hands it on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asked {
    pub id: u64,
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

/// The compositor's answer to an [`Asked`] of the same id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Answer {
    pub fn ok(id: u64, value: Value) -> Self {
        Self {
            id,
            ok: Some(value),
            error: None,
        }
    }

    pub fn error(id: u64, why: impl Into<String>) -> Self {
        Self {
            id,
            ok: None,
            error: Some(why.into()),
        }
    }

    /// Its line, without the line break.
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| format!("{{\"id\":{}}}", self.id))
    }
}

/// No arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nothing {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetSettings {
    /// A section's name (`appearance`), or every section.
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    /// The last so many (50 by default).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusWindow {
    pub window: u64,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveWindow {
    pub window: u64,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    /// From 1, on the window's screen.
    #[serde(default)]
    pub desktop: Option<u32>,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchDesktop {
    /// From 1.
    pub desktop: u32,
    /// The screen; the focused window's by default.
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
}

/// An application to start: the desktop entry `hocus mcp` found for what
/// the agent named, never a command the agent wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub id: String,
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSetting {
    /// Its section and name, dotted: `appearance.theme`.
    pub key: String,
    pub value: Value,
    #[serde(default)]
    pub intent: Option<String>,
}

/// What a tool asks, read from its name and arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", content = "args", rename_all = "snake_case")]
pub enum Call {
    ListWindows(Nothing),
    ListDesktops(Nothing),
    ListOutputs(Nothing),
    GetSettings(GetSettings),
    Journal(Journal),
    FocusWindow(FocusWindow),
    MoveWindow(MoveWindow),
    SwitchDesktop(SwitchDesktop),
    Launch(Launch),
    SetSetting(SetSetting),
}

impl Call {
    /// The call `tool` makes with `args`, or why it does not read: the
    /// agent reads it.
    pub fn of(tool: &str, args: &Value) -> Result<Self, String> {
        let args = match args {
            Value::Null => Value::Object(serde_json::Map::new()),
            args => args.clone(),
        };
        serde_json::from_value(serde_json::json!({ "tool": tool, "args": args }))
            .map_err(|err| format!("{tool}: {err}"))
    }

    /// Whether it changes the desktop (and so needs `access = "act"`, an
    /// intent, and is shown first).
    pub fn acts(&self) -> bool {
        matches!(
            self,
            Self::FocusWindow(_)
                | Self::MoveWindow(_)
                | Self::SwitchDesktop(_)
                | Self::Launch(_)
                | Self::SetSetting(_)
        )
    }

    /// What the agent says it does, as it said it.
    pub fn intent(&self) -> Option<&str> {
        match self {
            Self::FocusWindow(c) => c.intent.as_deref(),
            Self::MoveWindow(c) => c.intent.as_deref(),
            Self::SwitchDesktop(c) => c.intent.as_deref(),
            Self::Launch(c) => c.intent.as_deref(),
            Self::SetSetting(c) => c.intent.as_deref(),
            _ => None,
        }
    }

    /// The tool's name.
    pub fn tool(&self) -> &'static str {
        match self {
            Self::ListWindows(_) => "list_windows",
            Self::ListDesktops(_) => "list_desktops",
            Self::ListOutputs(_) => "list_outputs",
            Self::GetSettings(_) => "get_settings",
            Self::Journal(_) => "journal",
            Self::FocusWindow(_) => "focus_window",
            Self::MoveWindow(_) => "move_window",
            Self::SwitchDesktop(_) => "switch_desktop",
            Self::Launch(_) => "launch",
            Self::SetSetting(_) => "set_setting",
        }
    }
}

/// An intent as it is shown: on one line, its runs of spaces one,
/// [`MAX_INTENT`] characters at most; none when it says nothing.
pub fn intent_text(intent: &str) -> Option<String> {
    let line: Vec<&str> = intent
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect();
    let line = line.join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= MAX_INTENT {
        return Some(line);
    }
    let mut cut: String = line.chars().take(MAX_INTENT - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    Some(cut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_opening_names_the_agent_on_one_line() {
        assert_eq!(opening("Claude"), "mcp Claude");
        assert_eq!(agent_of("mcp Claude\n").as_deref(), Some("Claude"));
        assert_eq!(agent_of("mcp a\tb").as_deref(), Some("a b"));
        assert_eq!(agent_of("mcp "), None);
        assert_eq!(agent_of("msg idle"), None);
    }

    #[test]
    fn a_call_reads_its_arguments_and_says_what_does_not() {
        assert_eq!(
            Call::of("list_windows", &Value::Null),
            Ok(Call::ListWindows(Nothing {}))
        );
        let moved = Call::of(
            "move_window",
            &json!({ "window": 3, "x": 0, "desktop": 2, "intent": "à gauche" }),
        )
        .expect("a move");
        assert!(moved.acts());
        assert_eq!(moved.intent(), Some("à gauche"));
        assert!(!Call::ListWindows(Nothing {}).acts());
        let odd = Call::of("move_window", &json!({ "window": 3, "colour": 1 }));
        assert!(odd.is_err_and(|e| e.contains("colour")));
        assert!(Call::of("close_window", &json!({ "window": 3 })).is_err());
    }

    #[test]
    fn an_intent_is_one_short_line() {
        assert_eq!(intent_text("  a\n  b "), Some("a b".into()));
        assert_eq!(intent_text(" \n"), None);
        let long = intent_text(&"x ".repeat(200)).expect("cut");
        assert!(long.chars().count() <= MAX_INTENT);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn an_answer_is_one_line() {
        assert_eq!(Answer::ok(2, json!([1])).line(), r#"{"id":2,"ok":[1]}"#);
        assert_eq!(Answer::error(2, "no").line(), r#"{"id":2,"error":"no"}"#);
    }
}
