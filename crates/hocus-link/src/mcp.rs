//! What the agents acting through `hocus mcp` show (docs/design/mcp.md):
//! the compositor keeps them and tells the shell all of it in one message,
//! [`McpShown`], as it changes: their rows in the bar, the intent shown
//! before an action, the toast once a request settled, the journal's
//! lines. The shell words it; it answers with [`crate::ToCompositor::McpUndo`],
//! [`crate::ToCompositor::McpStop`].

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::text::tidy;

/// The tag over what an agent is about to touch, its name over its intent:
/// a layer surface of the shell's, at the place [`McpShow`] says.
pub const MCP_TAG_NAMESPACE: &str = "hocus-agent-tag";
/// The toast that sums a request up, with "Annuler" and "Voir".
pub const MCP_TOAST_NAMESPACE: &str = "hocus-agent-toast";
/// Their corners, logical pixels (the mockup's 7 and 12).
pub const MCP_TAG_RADIUS: f64 = 7.0;
pub const MCP_TOAST_RADIUS: f64 = 12.0;
/// How long the toast shows.
pub const MCP_TOAST: Duration = Duration::from_secs(8);
/// The card that waits for the user's answer (scene 3).
pub const MCP_CARD_NAMESPACE: &str = "hocus-agent-card";
/// Its corners (the mockup's 14).
pub const MCP_CARD_RADIUS: f64 = 14.0;
/// How long it waits before the action is refused.
pub const MCP_CARD_WAIT: Duration = Duration::from_secs(60);
/// The journal's lines the shell is told, newest first.
pub const MCP_JOURNAL_SHOWN: usize = 40;

/// Everything the agents show now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpShown {
    /// Every agent stopped (`Super+Shift+Escape`, "Tout arrêter").
    pub stopped: bool,
    pub agents: Vec<McpRow>,
    pub show: Option<McpShow>,
    pub toast: Option<McpToast>,
    /// Newest first, [`MCP_JOURNAL_SHOWN`] at most.
    pub journal: Vec<McpLine>,
    /// The agents' pointers resting where they acted, their names beside
    /// them (the one of the action shown is [`McpShow`]'s).
    pub pointers: Vec<McpPointer>,
    /// The action waiting for the user's answer.
    pub card: Option<McpCard>,
    /// The agents the user took over from, until "Rendre la main".
    pub taken: Vec<String>,
}

/// An agent's pointer, where a screen shows it: its tip at `x`, `y` (its
/// screen's logical pixels), its name beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPointer {
    #[serde(serialize_with = "tidy::name")]
    pub agent: String,
    pub output: String,
    pub x: i32,
    pub y: i32,
}

/// What an agent waits for the user to allow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpCard {
    /// Its own, which [`crate::ToCompositor::McpReply`] names.
    pub id: u64,
    #[serde(serialize_with = "tidy::name")]
    pub agent: String,
    #[serde(serialize_with = "tidy::name")]
    pub intent: String,
    pub ask: McpAsk,
    pub why: McpWhy,
    /// The point it aims at, on its screen (logical): the card beside it.
    pub output: String,
    pub x: i32,
    pub y: i32,
}

/// What the agent wants to do, in the window titled so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpAsk {
    Click {
        #[serde(serialize_with = "tidy::title")]
        title: String,
    },
    Type {
        #[serde(serialize_with = "tidy::title")]
        title: String,
    },
    Keys {
        #[serde(serialize_with = "tidy::title")]
        title: String,
        keys: String,
    },
    Close {
        #[serde(serialize_with = "tidy::title")]
        title: String,
    },
}

/// Why it waits for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpWhy {
    /// The application says the field is for a password or a PIN.
    Password,
    /// An application of `[agents] confirm-apps`, by its name.
    App {
        #[serde(serialize_with = "tidy::name")]
        name: String,
    },
}

/// The user's answer on a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpReply {
    Allow,
    Refuse,
    /// « Je prends la main »: refused, and the agent's actions after it.
    TakeOver,
}

/// An agent's row in the bar's agents' group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpRow {
    pub id: u64,
    #[serde(serialize_with = "tidy::name")]
    pub name: String,
    pub doing: McpDoing,
}

/// What an agent's row says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpDoing {
    /// Connected, nothing done yet.
    Ready,
    /// A request under way, of that kind.
    Acting(McpKind),
    /// An action waits for the user's answer on its card.
    Asking,
    /// The user took over from it.
    Taken,
    /// Its last request, settled at `at` (the clock's `10:43`).
    Did { summary: McpSummary, at: String },
}

/// What an action is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpKind {
    Windows,
    Desktop,
    Launch,
    Setting,
    /// Clicks, scrolls, typing or keys in an application.
    Input,
}

/// What a request did, counted, for one sentence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpSummary {
    /// Windows moved, sized or sent to another desktop.
    pub moved: u32,
    /// Windows brought forward.
    pub focused: u32,
    /// The desktop shown last, from 1.
    pub switched: Option<u32>,
    /// The applications started, by name.
    pub launched: Vec<String>,
    /// The settings changed, and what they were set to.
    pub settings: Vec<McpSet>,
    /// In applications: what cannot be undone.
    #[serde(default)]
    pub clicked: u32,
    #[serde(default)]
    pub scrolled: u32,
    /// Characters typed, counted, never kept.
    #[serde(default)]
    pub typed: u32,
    /// Key combinations pressed.
    #[serde(default)]
    pub pressed: u32,
    #[serde(default)]
    pub closed: u32,
}

/// A setting an agent changed: its key (`appearance.theme`) and its new
/// value as text (`dark`, `true`, `600`), which the shell words with the
/// Settings' own labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpSet {
    pub key: String,
    #[serde(serialize_with = "tidy::name")]
    pub value: String,
}

impl McpSummary {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// An action about to happen: who, why, where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpShow {
    #[serde(serialize_with = "tidy::name")]
    pub agent: String,
    #[serde(serialize_with = "tidy::name")]
    pub intent: String,
    /// The screen it shows on.
    pub output: String,
    pub at: McpAt,
}

/// Where an action's tag goes, in its screen's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpAt {
    /// A window, ringed by the compositor: the tag at its top left.
    Window { x: i32, y: i32 },
    /// A desktop of the screen (from 0), its tile ringed in the bar; the
    /// tag at the top of the screen, centred on `x`.
    Desktop { index: u8, x: i32, y: i32 },
    /// The screen: the tag at its top, centred on `x`.
    Screen { x: i32, y: i32 },
    /// The agent's own pointer, its tip at `x`, `y`, ringed by the
    /// compositor: the tag beside it.
    Pointer { x: i32, y: i32 },
}

/// The toast once a request settled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToast {
    /// The request's, which [`crate::ToCompositor::McpUndo`] names.
    pub request: u64,
    #[serde(serialize_with = "tidy::name")]
    pub agent: String,
    pub summary: McpSummary,
    /// The request's first intent.
    #[serde(serialize_with = "tidy::name")]
    pub intent: String,
    /// Whether something of it can be undone.
    pub undo: bool,
    /// The screen, and its bottom left corner (the work area's, logical).
    pub output: String,
    pub left: i32,
    pub bottom: i32,
    /// Since when, microseconds of `CLOCK_MONOTONIC`.
    pub since: u64,
}

/// A line of the journal's popover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpLine {
    #[serde(serialize_with = "tidy::name")]
    pub agent: String,
    /// The clock's time (`10:43`).
    pub at: String,
    pub said: McpSaid,
}

/// What a journal's line says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpSaid {
    /// A request: what it did, on which desktop (from 1) or in the
    /// settings, and its id while it can be undone.
    Did {
        summary: McpSummary,
        desktop: Option<u32>,
        undo: Option<u64>,
    },
    /// A request undone.
    Undone { summary: McpSummary },
    /// A call refused, the tool's name, and why.
    Refused { tool: String, why: McpRefusal },
}

/// Why a call was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpRefusal {
    /// `[[agents.mcp]]` does not name it.
    Unknown,
    /// Its access is `read`.
    ReadOnly,
    /// It may not change settings.
    NoSettings,
    /// The session is locked.
    Locked,
    /// A password is asked for.
    Asking,
    /// Every agent is stopped.
    Stopped,
    /// A setting it may never change (`[agents]`, `[shortcuts]`).
    Forbidden,
    /// Not an application of its `apps`.
    NotItsApp,
    /// An X11 window: agents act on Wayland windows.
    X11,
    /// The user took over from it.
    TakenOver,
    /// The user refused it on its card.
    UserRefused,
    /// Nobody answered its card in time.
    NoAnswer,
}
