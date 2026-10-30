//! The agents' states as the desktop follows them (docs/design/bar.md §4
//! group 3, §10; docs/agents.md): what an agent is doing, told by its
//! **sources**, each a small adapter that runs off the compositor's loop.
//!
//! - Claude Code's and Codex's hooks run `hocus agent-hook TOOL`, which
//!   reads the hook's JSON and maps it to a state ([`hooks`]), then writes
//!   one [`Report`] line to the compositor's control socket.
//! - Any other tool runs `hocus agent NAME STATE`: the same line.
//! - herdr's socket, when herdr runs, is followed by a thread of the
//!   compositor's ([`herdr`]).
//!
//! Plugging a tool in (writing its hooks into its settings, keeping a copy
//! of the original) and finding the tools on the machine are [`tools`]'
//! and [`watch`]'s. The agent's process, and through its ancestry its
//! window, are [`process`]'.

pub mod herdr;
pub mod hooks;
pub mod process;
pub mod tools;
pub mod watch;

use std::{fmt, str::FromStr};

/// The environment variable naming the compositor's control socket, set
/// for the programs it starts.
pub const SOCKET_ENV: &str = "HOCUS_SOCKET";

/// The control socket's path beside the Wayland socket's.
pub fn socket_path(wayland: &std::path::Path) -> std::path::PathBuf {
    let mut path = wayland.as_os_str().to_owned();
    path.push(".control");
    std::path::PathBuf::from(path)
}

/// What an agent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Working,
    /// It waits for the user: a permission, a question.
    Waiting,
    /// It finished its turn, and nobody looked yet.
    Done,
    /// Its turn ended on an error.
    Failed,
    /// Ready, nothing to look at.
    Idle,
}

impl State {
    pub const ALL: [Self; 5] = [
        Self::Working,
        Self::Waiting,
        Self::Done,
        Self::Failed,
        Self::Idle,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Idle => "idle",
        }
    }
}

impl FromStr for State {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.name() == s)
            .ok_or_else(|| {
                BadLine(format!(
                    "no state {s:?}: working, waiting, done, failed, idle or ended"
                ))
            })
    }
}

/// Where an agent's state comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    ClaudeCode,
    Codex,
    Herdr,
    /// `hocus agent NAME STATE`, from any other tool.
    Command,
}

impl Source {
    pub const ALL: [Self; 4] = [Self::ClaudeCode, Self::Codex, Self::Herdr, Self::Command];

    pub const fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Herdr => "herdr",
            Self::Command => "command",
        }
    }
}

impl FromStr for Source {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|source| source.name() == s)
            .ok_or_else(|| BadLine(format!("no source {s:?}")))
    }
}

/// An agent's state as one source tells it: `None` when its session
/// ended, and it is forgotten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub source: Source,
    /// The source's own name for it: a session's id, herdr's pane, the
    /// command's name. One word.
    pub key: String,
    pub state: Option<State>,
    /// The agent's process, when known: its window is found above it, and
    /// it is forgotten when it ends.
    pub pid: Option<u32>,
    /// The name it shows under (its project's directory, or the name the
    /// command gave).
    pub name: String,
}

/// What a line's key may hold: no spaces, no control characters.
pub fn key_word(key: &str) -> String {
    let word: String = key
        .chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(200)
        .collect();
    if word.is_empty() {
        "_".to_owned()
    } else {
        word
    }
}

/// A name as a line carries it: on one line, without tabs, 80 characters
/// at most, trimmed.
pub fn name_text(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(80)
        .collect();
    name.trim().to_owned()
}

/// The line on the control socket: `agent SOURCE KEY STATE PID NAME`, with
/// `ended` for a state that ended and `-` for an unknown process; the name
/// last, as it may hold spaces.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.map_or("ended", State::name);
        let pid = self.pid.map_or_else(|| "-".to_owned(), |p| p.to_string());
        write!(
            f,
            "agent {} {} {state} {pid} {}",
            self.source.name(),
            key_word(&self.key),
            name_text(&self.name)
        )
    }
}

impl FromStr for Report {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let line = line.trim_end_matches(['\n', '\r']);
        let mut words = line.splitn(6, ' ');
        if words.next() != Some("agent") {
            return Err(BadLine(format!("not an agent's line: {line:?}")));
        }
        let mut word = |what: &str| {
            words
                .next()
                .filter(|w| !w.is_empty())
                .ok_or_else(|| BadLine(format!("no {what}: {line:?}")))
        };
        let source = word("source")?.parse()?;
        let key = word("key")?.to_owned();
        let state = match word("state")? {
            "ended" => None,
            state => Some(state.parse()?),
        };
        let pid = match word("process")? {
            "-" => None,
            pid => Some(
                pid.parse()
                    .map_err(|e| BadLine(format!("a process: {e}")))?,
            ),
        };
        let name = words.next().map(name_text).unwrap_or_default();
        let name = if name.is_empty() { key.clone() } else { name };
        Ok(Self {
            source,
            key,
            state,
            pid,
            name,
        })
    }
}

/// A line that does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadLine(pub String);

impl fmt::Display for BadLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadLine {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_reads_back_as_written() {
        let reports = [
            Report {
                source: Source::ClaudeCode,
                key: "7c1f-22".into(),
                state: Some(State::Waiting),
                pid: Some(4242),
                name: "hocus panda".into(),
            },
            Report {
                source: Source::Command,
                key: "build".into(),
                state: None,
                pid: None,
                name: "build".into(),
            },
        ];
        for report in reports {
            assert_eq!(report.to_string().parse::<Report>(), Ok(report));
        }
        // A key's spaces, a name's tabs and line breaks stay on the line.
        let odd = Report {
            source: Source::Command,
            key: "my agent".into(),
            state: Some(State::Idle),
            pid: None,
            name: "a\tb\nc".into(),
        };
        assert_eq!(odd.to_string(), "agent command my_agent idle - a b c");
        for bad in [
            "",
            "agent",
            "agent claude-code k working",
            "agent nobody k working 1 x",
            "agent command k sleeping 1 x",
            "agent command k working pid x",
            "hello command k working 1 x",
        ] {
            assert!(bad.parse::<Report>().is_err(), "{bad:?}");
        }
        // No name: the key.
        let bare: Report = "agent command build done -".parse().expect("a line");
        assert_eq!(bare.name, "build");
    }
}
