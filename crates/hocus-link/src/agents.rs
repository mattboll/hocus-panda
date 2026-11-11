//! The agents and their tools as the bar shows them (bar.md §4 group 3,
//! §7).

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{BadLine, text::tidy};

/// An agents' tool, as the bar shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTool {
    /// `claude-code`, `codex` or `herdr`.
    pub tool: String,
    pub state: ToolState,
    /// The file plugging it in changes, as the user knows it.
    pub file: String,
}

/// What an agents' tool is to the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolState {
    /// Not on this computer.
    Absent,
    /// There, not plugged in: the card proposes it.
    Found,
    /// There, not plugged in, and the user said to plug nothing in.
    Declined,
    /// Plugged in: its agents are followed.
    Plugged,
    /// herdr runs, and is followed.
    Running,
    /// herdr runs, and the user said to plug nothing in.
    Ignored,
    /// herdr does not run.
    Stopped,
}

impl ToolState {
    pub const ALL: [Self; 7] = [
        Self::Absent,
        Self::Found,
        Self::Declined,
        Self::Plugged,
        Self::Running,
        Self::Ignored,
        Self::Stopped,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Found => "found",
            Self::Declined => "declined",
            Self::Plugged => "plugged",
            Self::Running => "running",
            Self::Ignored => "ignored",
            Self::Stopped => "stopped",
        }
    }

    /// Whether its agents are followed.
    pub const fn followed(self) -> bool {
        matches!(self, Self::Plugged | Self::Running)
    }
}

/// An agent as the bar shows it. Then a tab and its [`Note`], empty when it has
/// none.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentShown {
    pub id: u64,
    pub state: String,
    pub since: u64,
    pub source: String,
    pub window: u64,
    #[serde(serialize_with = "tidy::name")]
    pub name: String,
    /// What it asks for, or why it failed, as its tool said it (4.6).
    pub note: Option<Note>,
}

// The note may hold code or a secret (a command, a file's path): its
// length only, as everywhere it is logged.
impl fmt::Debug for AgentShown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentShown")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("since", &self.since)
            .field("source", &self.source)
            .field("window", &self.window)
            .field("name", &self.name)
            .field("note", &self.note)
            .finish()
    }
}

/// What an agent waits for, or why it failed, as its tool says it: what the
/// secretary's briefing shows (bar.md §7 B, 4.6). One short line, cut to
/// [`NOTE_MAX`] characters.
///
/// It may hold code or a secret (a command, a file's path): it is never
/// logged, only its length ([`fmt::Debug`]).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Note {
    /// It asks to use `tool` (one word) on `what` (its input in one line,
    /// maybe empty): the shell words it ("Autoriser Bash : npm install ?").
    Allow { tool: String, what: String },
    /// What its tool says: a notification, a question, an error.
    Says(String),
}

/// The characters a [`Note`]'s text keeps at most: the briefing's line is
/// cut to its width long before that.
pub const NOTE_MAX: usize = 120;

/// A note's text as it is kept: on one line, its spaces runs made one,
/// trimmed, [`NOTE_MAX`] characters at most (the last an ellipsis when it
/// was longer).
pub fn note_text(text: &str) -> String {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect();
    let line = words.join(" ");
    if line.chars().count() <= NOTE_MAX {
        return line;
    }
    let mut cut: String = line.chars().take(NOTE_MAX - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

impl Note {
    /// A tool's request, its words kept as a note's.
    pub fn allow(tool: &str, what: &str) -> Self {
        let tool: String = tool
            .chars()
            .map(|c| {
                if c.is_whitespace() || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .take(64)
            .collect();
        Self::Allow {
            tool,
            what: note_text(what),
        }
    }

    /// A tool's text, kept as a note's; none when it says nothing.
    pub fn says(text: &str) -> Option<Self> {
        Some(Self::Says(note_text(text))).filter(|n| *n != Self::Says(String::new()))
    }

    /// Its length, in characters: what is logged of it.
    pub fn len(&self) -> usize {
        match self {
            Self::Allow { tool, what } => tool.chars().count() + what.chars().count(),
            Self::Says(text) => text.chars().count(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Allow { .. } => "allow",
            Self::Says(_) => "says",
        };
        write!(f, "{kind} ({} characters)", self.len())
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow { tool, what } if tool.is_empty() => {
                write!(f, "allow _ {}", note_text(what))
            }
            Self::Allow { tool, what } => write!(f, "allow {tool} {}", note_text(what)),
            Self::Says(text) => write!(f, "says {}", note_text(text)),
        }
    }
}

impl FromStr for Note {
    type Err = BadLine;

    // The error never holds the note's text.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, rest) = s.split_once(' ').unwrap_or((s, ""));
        match kind {
            "allow" => {
                let (tool, what) = rest.split_once(' ').unwrap_or((rest, ""));
                if tool.is_empty() {
                    return Err(BadLine("a note's request without its tool".to_owned()));
                }
                Ok(Self::allow(tool, what))
            }
            "says" => {
                Self::says(rest).ok_or_else(|| BadLine("a note that says nothing".to_owned()))
            }
            _ => Err(BadLine("a note of no kind".to_owned())),
        }
    }
}
