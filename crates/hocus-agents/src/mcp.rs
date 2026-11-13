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
    /// Its entry's `StartupWMClass`: the id its windows may go by.
    #[serde(default)]
    pub wm_class: Option<String>,
    /// The desktop its first window opens on, from 1; the shown one
    /// without it. What the user sees never changes.
    #[serde(default)]
    pub desktop: Option<u32>,
    /// The screen of that desktop; the pointer's by default.
    #[serde(default)]
    pub output: Option<String>,
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

/// Where a click or a scroll lands in a window: a point from its top left
/// (its centre when left out), or an element of its accessible tree.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Click {
    pub window: u64,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    /// An element's label, exactly, else the one that holds it.
    #[serde(default)]
    pub label: Option<String>,
    /// Its role, among several of one label (`push button`).
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub button: Option<Button>,
    /// 1 to 3.
    #[serde(default)]
    pub count: Option<u8>,
    #[serde(default)]
    pub intent: Option<String>,
}

/// A mouse button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

impl Button {
    /// Its evdev code.
    pub fn code(self) -> u32 {
        match self {
            Self::Left => 0x110,
            Self::Right => 0x111,
            Self::Middle => 0x112,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scroll {
    pub window: u64,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    /// Logical pixels, down positive: 15 a notch.
    pub dy: f64,
    #[serde(default)]
    pub intent: Option<String>,
}

/// The characters a call types at most.
pub const MAX_TYPED: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Type {
    pub window: u64,
    pub text: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressKeys {
    pub window: u64,
    /// As the shortcuts write them: `Ctrl+S`, `Return`, `Shift+Tab`.
    pub keys: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseWindow {
    pub window: u64,
    #[serde(default)]
    pub intent: Option<String>,
}

/// How long `wait_for` waits at most.
pub const MAX_WAIT_MS: u32 = 30_000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitFor {
    /// A window of this app id…
    #[serde(default)]
    pub app_id: Option<String>,
    /// …whose title holds this.
    #[serde(default)]
    pub title: Option<String>,
    /// Or an element of this window's tree, by its label.
    #[serde(default)]
    pub window: Option<u64>,
    #[serde(default)]
    pub label: Option<String>,
    /// Until none is.
    #[serde(default)]
    pub gone: bool,
    /// 5000 by default.
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

/// Good news (#809): the bar says who and why, the confetti fly. Any
/// agent of `[[agents.mcp]]` may, `read` too: it touches nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Celebrate {
    /// What the news is, one short line.
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfWindow {
    pub window: u64,
}

/// One of an acting agent's rights, a check box of the Settings' page
/// each (`[[agents.mcp]]`' `windows`, `desktops`, `settings`, `input`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Right {
    /// Arrange, move, bring forward and close windows.
    Windows,
    /// Switch desktops, send a window to another, launch applications.
    Desktops,
    /// Change settings.
    Settings,
    /// Click, scroll, type and press keys in the applications of its
    /// `apps`.
    Input,
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
    Click(Click),
    Scroll(Scroll),
    Type(Type),
    PressKeys(PressKeys),
    CloseWindow(CloseWindow),
    WaitFor(WaitFor),
    Capture(OfWindow),
    AccessibilityTree(OfWindow),
    Celebrate(Celebrate),
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
                | Self::Click(_)
                | Self::Scroll(_)
                | Self::Type(_)
                | Self::PressKeys(_)
                | Self::CloseWindow(_)
        )
    }

    /// Whether it goes through the agent's own seat into an application's
    /// window: only into the applications of its `apps`.
    pub fn inputs(&self) -> bool {
        matches!(
            self,
            Self::Click(_) | Self::Scroll(_) | Self::Type(_) | Self::PressKeys(_)
        )
    }

    /// What of an agent's rights it needs, beyond `access = "act"`
    /// (docs/design/mcp.md, "Rights"): none for a reading tool.
    pub fn needs(&self) -> Vec<Right> {
        match self {
            Self::FocusWindow(_) | Self::CloseWindow(_) => vec![Right::Windows],
            Self::MoveWindow(m) => {
                let placed = [m.x, m.y, m.width, m.height].iter().any(Option::is_some);
                match (placed, m.desktop.is_some()) {
                    (true, true) => vec![Right::Windows, Right::Desktops],
                    (false, true) => vec![Right::Desktops],
                    _ => vec![Right::Windows],
                }
            }
            Self::SwitchDesktop(_) | Self::Launch(_) => vec![Right::Desktops],
            Self::SetSetting(_) => vec![Right::Settings],
            Self::Click(_) | Self::Scroll(_) | Self::Type(_) | Self::PressKeys(_) => {
                vec![Right::Input]
            }
            _ => Vec::new(),
        }
    }

    /// The window it aims at, if it aims at one.
    pub fn window(&self) -> Option<u64> {
        match self {
            Self::FocusWindow(c) => Some(c.window),
            Self::MoveWindow(c) => Some(c.window),
            Self::Click(c) => Some(c.window),
            Self::Scroll(c) => Some(c.window),
            Self::Type(c) => Some(c.window),
            Self::PressKeys(c) => Some(c.window),
            Self::CloseWindow(c) => Some(c.window),
            Self::Capture(c) | Self::AccessibilityTree(c) => Some(c.window),
            Self::WaitFor(c) => c.window,
            _ => None,
        }
    }

    /// What the agent says it does, as it said it.
    pub fn intent(&self) -> Option<&str> {
        match self {
            Self::FocusWindow(c) => c.intent.as_deref(),
            Self::MoveWindow(c) => c.intent.as_deref(),
            Self::SwitchDesktop(c) => c.intent.as_deref(),
            Self::Launch(c) => c.intent.as_deref(),
            Self::SetSetting(c) => c.intent.as_deref(),
            Self::Click(c) => c.intent.as_deref(),
            Self::Scroll(c) => c.intent.as_deref(),
            Self::Type(c) => c.intent.as_deref(),
            Self::PressKeys(c) => c.intent.as_deref(),
            Self::CloseWindow(c) => c.intent.as_deref(),
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
            Self::Click(_) => "click",
            Self::Scroll(_) => "scroll",
            Self::Type(_) => "type",
            Self::PressKeys(_) => "press_keys",
            Self::CloseWindow(_) => "close_window",
            Self::WaitFor(_) => "wait_for",
            Self::Capture(_) => "capture",
            Self::AccessibilityTree(_) => "accessibility_tree",
            Self::Celebrate(_) => "celebrate",
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
        assert!(Call::of("drag", &json!({ "window": 3 })).is_err());
        let typed = Call::of("type", &json!({ "window": 3, "text": "ab", "intent": "x" }));
        assert!(typed.as_ref().is_ok_and(|t| t.acts() && t.inputs()));
        let closed = Call::of("close_window", &json!({ "window": 3, "intent": "x" }));
        assert!(closed.as_ref().is_ok_and(|c| c.acts() && !c.inputs()));
        let news = Call::of("celebrate", &json!({ "why": "#791 merged" }));
        assert!(news.is_ok_and(|n| !n.acts() && n.needs().is_empty() && n.window().is_none()));
        assert!(Call::of("celebrate", &Value::Null).is_err());
        let tree = Call::of("accessibility_tree", &json!({ "window": 3 }));
        assert!(tree.is_ok_and(|t| !t.acts() && t.window() == Some(3)));
        let click = Call::of(
            "click",
            &json!({ "window": 3, "button": "right", "intent": "x" }),
        );
        assert!(click.is_ok_and(|c| matches!(
            c,
            Call::Click(Click {
                button: Some(Button::Right),
                ..
            })
        )));
    }

    #[test]
    fn each_acting_call_needs_its_check_box() {
        let call = |tool: &str, args: Value| Call::of(tool, &args).map(|c| c.needs());
        assert_eq!(call("list_windows", Value::Null), Ok(vec![]));
        assert_eq!(
            call("close_window", json!({ "window": 1 })),
            Ok(vec![Right::Windows])
        );
        assert_eq!(
            call("move_window", json!({ "window": 1, "desktop": 2 })),
            Ok(vec![Right::Desktops])
        );
        assert_eq!(
            call("move_window", json!({ "window": 1, "x": 0, "desktop": 2 })),
            Ok(vec![Right::Windows, Right::Desktops])
        );
        assert_eq!(
            call(
                "launch",
                json!({ "id": "foot", "name": "Foot", "argv": ["foot"] })
            ),
            Ok(vec![Right::Desktops])
        );
        assert_eq!(
            call("type", json!({ "window": 1, "text": "a" })),
            Ok(vec![Right::Input])
        );
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
