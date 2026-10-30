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

pub use hocus_link::Note;

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
    /// The name it shows under: its project's directory (else its
    /// process's working directory's, empty when neither is known), or the
    /// name the command gave.
    pub name: String,
    /// What it asks for, or why it failed, as its tool says it (never
    /// logged: its [`fmt::Debug`] says its length only).
    pub note: Option<Note>,
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
/// last, as it may hold spaces; then, when there is one, a tab and the
/// [`Note`] (`allow TOOL WHAT`, `says TEXT`).
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
        )?;
        match &self.note {
            Some(note) => write!(f, "\t{note}"),
            None => Ok(()),
        }
    }
}

impl FromStr for Report {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let line = line.trim_end_matches(['\n', '\r']);
        // The note apart: what is said of a line that does not read never
        // holds it. One that does not read is left out.
        let (line, note) = match line.split_once('\t') {
            Some((line, note)) => (line, note.parse::<Note>().ok()),
            None => (line, None),
        };
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
        // A command's agent is its name; a hook's without one is named
        // later, never by its key (a session's id).
        let name = if name.is_empty() && source == Source::Command {
            key.clone()
        } else {
            name
        };
        Ok(Self {
            source,
            key,
            state,
            pid,
            name,
            note,
        })
    }
}

/// An agent's note once a report of `said`, in `state`, comes: a state
/// that changed takes the report's note (none: none). In the same state, a
/// tool's request replaces what was kept (a new question), and a text
/// only fills a note missing: Claude Code's notification of a permission
/// comes after its request, and says less ("Claude needs your
/// permission").
pub fn next_note(kept: Option<Note>, changed: bool, said: Option<Note>) -> Option<Note> {
    match (changed, said) {
        (true, said) => said,
        (false, Some(said @ Note::Allow { .. })) => Some(said),
        (false, said) => kept.or(said),
    }
}

/// A directory's name as an agent's: its path's last part (none for `/`
/// or an empty path).
pub fn folder(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .file_name()
        .map(|n| name_text(&n.to_string_lossy()))
        .filter(|n| !n.is_empty())
}

/// The name an agent shows under (docs/agents.md), from the best of what
/// its sources know: herdr's, as its sidebar shows it; else its project's
/// folder (a hook's `cwd`, or the command's name); else its process's
/// working directory's. Never its tool's name: herdr's agent kind
/// ("claude") named every agent alike (#497).
pub fn agent_name(
    herdr: Option<&str>,
    project: Option<&str>,
    process: Option<&str>,
) -> Option<String> {
    [herdr, project, process]
        .into_iter()
        .flatten()
        .map(name_text)
        .find(|n| !n.is_empty())
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
                note: Some(Note::allow("Bash", "npm install")),
            },
            Report {
                source: Source::Command,
                key: "build".into(),
                state: None,
                pid: None,
                name: "build".into(),
                note: None,
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
            note: None,
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
        // No name: a command's is its key; a hook's stays unknown.
        let bare: Report = "agent command build done -".parse().expect("a line");
        assert_eq!(bare.name, "build");
        let hook: Report = "agent claude-code 7c1f-22 working 42 "
            .parse()
            .expect("a line");
        assert_eq!(hook.name, "");
        let unnamed = Report {
            name: String::new(),
            ..hook.clone()
        };
        assert_eq!(unnamed.to_string().parse::<Report>(), Ok(hook));
    }

    #[test]
    fn a_report_carries_its_note_apart() {
        let failed = Report {
            source: Source::ClaudeCode,
            key: "s".into(),
            state: Some(State::Failed),
            pid: Some(1),
            name: "site web".into(),
            note: Note::says("API Error:\nRate limit reached"),
        };
        let line = failed.to_string();
        assert_eq!(
            line,
            "agent claude-code s failed 1 site web\tsays API Error: Rate limit reached"
        );
        assert_eq!(line.parse::<Report>(), Ok(failed));
        // A note that does not read is left out, the report kept; a line
        // that does not read never says its note.
        let odd: Report = "agent command k idle - k\tnope".parse().expect("a line");
        assert_eq!(odd.note, None);
        let bad = "agent command k sleeping - k\tsays secret"
            .parse::<Report>()
            .expect_err("no such state");
        assert!(!bad.to_string().contains("secret"), "{bad}");
    }

    #[test]
    fn a_request_replaces_the_note_and_a_text_only_fills_one_missing() {
        let asked = || Some(Note::allow("Bash", "npm install"));
        let notified = Note::says("Claude needs your permission");
        // The permission's notification after its request: the request's.
        assert_eq!(next_note(asked(), false, notified.clone()), asked());
        // Alone, the notification's.
        assert_eq!(next_note(None, false, notified.clone()), notified);
        // Another request while it still waits: the new one.
        let other = Some(Note::allow("Write", "schema.sql"));
        assert_eq!(next_note(asked(), false, other.clone()), other);
        // A report without one keeps it; a new state takes the report's.
        assert_eq!(next_note(asked(), false, None), asked());
        assert_eq!(next_note(asked(), true, None), None);
        assert_eq!(next_note(None, true, notified.clone()), notified);
    }

    #[test]
    fn an_agent_is_named_by_herdr_else_its_folder_else_its_process_never_its_tool() {
        assert_eq!(
            agent_name(
                Some("hocus-panda · refacto"),
                Some("hocus-panda"),
                Some("x")
            ),
            Some("hocus-panda · refacto".into())
        );
        assert_eq!(
            agent_name(None, Some("infra"), Some("home")),
            Some("infra".into())
        );
        assert_eq!(
            agent_name(Some(" "), Some(""), Some("site")),
            Some("site".into())
        );
        assert_eq!(agent_name(None, None, None), None);
        assert_eq!(folder("/home/alex/hocus-panda"), Some("hocus-panda".into()));
        assert_eq!(folder("/home/alex/infra/"), Some("infra".into()));
        assert_eq!(folder("/"), None);
        assert_eq!(folder(""), None);
    }
}
