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
//!
//! The compositor's control socket (`<socket>.control`, [`SOCKET_ENV`])
//! takes the lines written and read here: a [`Report`], a [`Question`]
//! (`hocus msg idle`, `lock`, `outputs`, `input-devices`, `a11y`), a
//! [`ProgramAsk`] (`hocus msg shell status|restart`); an agent's MCP line
//! ([`mcp`], `mcp NAME`); an agent's good news ([`Celebration`], `hocus
//! celebrate`); and the Settings' own, `msg screens` (the compositor's
//! `displays.rs`).

pub mod herdr;
pub mod hooks;
pub mod mcp;
pub mod process;
pub mod runs;
pub mod tools;
pub mod watch;

use std::{fmt, str::FromStr};

pub use hocus_link::{Note, Supervised};

/// The environment variable naming the compositor's control socket, set
/// for the programs it starts.
pub const SOCKET_ENV: &str = "HOCUS_SOCKET";

/// The control socket's path beside the Wayland socket's.
pub fn socket_path(wayland: &std::path::Path) -> std::path::PathBuf {
    let mut path = wayland.as_os_str().to_owned();
    path.push(".control");
    std::path::PathBuf::from(path)
}

/// A question to the compositor on its control socket (`hocus msg`): a
/// line `msg WHAT`, answered with lines of text before the compositor
/// closes the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// The session's idle state, its time left and what holds it off
    /// (docs/design/idle.md).
    Idle,
    /// Lock the session now (docs/design/lock.md), as `Super+L` will;
    /// answered once it is locked.
    Lock,
    /// The screens: their modes, places, scales and settings, as JSON
    /// (docs/design/settings.md, "Écrans").
    Outputs,
    /// The mice and touchpads plugged in, with what they were given of
    /// `[pointer]` and `[touchpad]` (6.12, [`hocus_link::input`]).
    /// Followed, the connection stays open and hears each device plugged,
    /// changed or unplugged.
    InputDevices { follow: bool },
    /// What the shell shows, for screen readers: each window's tree, a
    /// node a line (docs/design/screen-reader.md).
    A11y,
}

impl Question {
    /// Its line, without the line break.
    pub fn line(self) -> &'static str {
        match self {
            Self::Idle => "msg idle",
            Self::Lock => "msg lock",
            Self::Outputs => "msg outputs",
            Self::InputDevices { follow: false } => "msg input-devices",
            Self::InputDevices { follow: true } => "msg input-devices follow",
            Self::A11y => "msg a11y",
        }
    }

    /// The question a line asks, if it is one.
    pub fn of(line: &str) -> Option<Self> {
        match line.trim_end_matches(['\n', '\r']) {
            "msg idle" => Some(Self::Idle),
            "msg lock" => Some(Self::Lock),
            "msg outputs" => Some(Self::Outputs),
            "msg input-devices" => Some(Self::InputDevices { follow: false }),
            "msg input-devices follow" => Some(Self::InputDevices { follow: true }),
            "msg a11y" => Some(Self::A11y),
            _ => None,
        }
    }
}

/// Which program a restart runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restart {
    /// The one that ran: the installed one, or the one given before.
    Same,
    /// The installed one again: a program given before is let go of.
    Installed,
    /// This one, for this session only: an absolute path. Nothing is
    /// written to the configuration; the next session runs the installed
    /// one.
    Given(std::path::PathBuf),
}

/// What is asked of a supervised program (`hocus msg shell status`, `hocus
/// msg shell restart [--program PATH]`): a line `msg shell status` or `msg
/// shell restart [installed|program PATH]`, answered with lines of text. A
/// refusal's answer starts with [`REFUSED`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramAsk {
    /// Which program runs, since when, and whether it was given.
    Status(Supervised),
    /// End it and start it again at once.
    Restart(Supervised, Restart),
}

/// How a refusal's answer starts: `hocus msg` then fails.
pub const REFUSED: &str = "refused: ";

impl ProgramAsk {
    /// Its line, without the line break.
    pub fn line(&self) -> String {
        match self {
            Self::Status(which) => format!("msg {} status", which.word()),
            Self::Restart(which, Restart::Same) => format!("msg {} restart", which.word()),
            Self::Restart(which, Restart::Installed) => {
                format!("msg {} restart installed", which.word())
            }
            Self::Restart(which, Restart::Given(program)) => {
                format!("msg {} restart program {}", which.word(), program.display())
            }
        }
    }

    /// What a line asks, if it asks this. A program's path is the rest of
    /// its line, spaces and all.
    pub fn of(line: &str) -> Option<Self> {
        let rest = line.trim_end_matches(['\n', '\r']).strip_prefix("msg ")?;
        let (which, rest) = rest.split_once(' ')?;
        let which = Supervised::of(which)?;
        match rest {
            "status" => Some(Self::Status(which)),
            "restart" => Some(Self::Restart(which, Restart::Same)),
            "restart installed" => Some(Self::Restart(which, Restart::Installed)),
            _ => {
                let program = rest.strip_prefix("restart program ")?;
                (!program.is_empty()).then(|| Self::Restart(which, Restart::Given(program.into())))
            }
        }
    }
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
    /// How the tests it just ran went (#810), when it ran some.
    pub tests: Option<runs::TestRun>,
    /// Its conversation, as a hook says it (docs/design/projects.md
    /// §8.1): never logged.
    pub conversation: Conversation,
}

/// What a hook says of its agent's conversation (docs/design/projects.md
/// §8.1): its folder (`cwd`), its transcript's path, and the prompt the
/// user just gave (`UserPromptSubmit`'s, cut at [`PROMPT_MOST`]
/// characters). Its `Debug` says none of them: they are never logged.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Conversation {
    pub cwd: Option<String>,
    pub transcript: Option<String>,
    pub prompt: Option<String>,
}

impl fmt::Debug for Conversation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Conversation")
            .field("cwd", &self.cwd.is_some())
            .field("transcript", &self.transcript.is_some())
            .field("prompt", &self.prompt.is_some())
            .finish()
    }
}

/// A prompt's characters kept at most, as a project's card shows it (§4).
pub const PROMPT_MOST: usize = 60;

impl Conversation {
    /// A path as a line carries it: absolute, without tabs nor line
    /// breaks (none otherwise).
    pub fn path(path: &str) -> Option<String> {
        (path.starts_with('/') && !path.chars().any(char::is_control)).then(|| path.to_owned())
    }

    /// A prompt as a line carries it: on one line, cut at [`PROMPT_MOST`]
    /// characters ("…" when cut), trimmed; none when empty.
    pub fn prompt_text(prompt: &str) -> Option<String> {
        let flat: String = prompt
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let words: Vec<&str> = flat.split_whitespace().collect();
        let flat = words.join(" ");
        if flat.is_empty() {
            return None;
        }
        Some(if flat.chars().count() > PROMPT_MOST {
            let cut: String = flat.chars().take(PROMPT_MOST - 1).collect();
            format!("{}…", cut.trim_end())
        } else {
            flat
        })
    }

    fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(cwd) = self.cwd.as_deref().and_then(Self::path) {
            write!(f, "\tcwd {cwd}")?;
        }
        if let Some(path) = self.transcript.as_deref().and_then(Self::path) {
            write!(f, "\ttranscript {path}")?;
        }
        if let Some(prompt) = self.prompt.as_deref().and_then(Self::prompt_text) {
            write!(f, "\tprompt {prompt}")?;
        }
        Ok(())
    }

    /// Reads a field of a line, if it is one of these.
    fn read(&mut self, field: &str) -> bool {
        if let Some(cwd) = field.strip_prefix("cwd ") {
            self.cwd = Self::path(cwd);
        } else if let Some(path) = field.strip_prefix("transcript ") {
            self.transcript = Self::path(path);
        } else if let Some(prompt) = field.strip_prefix("prompt ") {
            self.prompt = Self::prompt_text(prompt);
        } else {
            return false;
        }
        true
    }
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
/// [`Note`] (`allow TOOL WHAT`, `says TEXT`); then, when it ran tests, a
/// tab and how they went (`tests passed 2040`, `tests failed`); then, each
/// after a tab, its conversation's `cwd PATH`, `transcript PATH` and
/// `prompt TEXT` when the hook says them.
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
        if let Some(note) = &self.note {
            write!(f, "\t{note}")?;
        }
        if let Some(tests) = &self.tests {
            write!(f, "\t{tests}")?;
        }
        self.conversation.write(f)
    }
}

impl FromStr for Report {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let line = line.trim_end_matches(['\n', '\r']);
        // The note and the tests apart: what is said of a line that does
        // not read never holds them. One that does not read is left out.
        let mut fields = line.split('\t');
        let line = fields.next().unwrap_or_default();
        let (mut note, mut tests) = (None, None);
        let mut conversation = Conversation::default();
        for field in fields {
            if conversation.read(field) {
                continue;
            }
            match field.parse::<runs::TestRun>() {
                Ok(run) => tests = Some(run),
                Err(_) => note = note.or_else(|| field.parse::<Note>().ok()),
            }
        }
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
            tests,
            conversation,
        })
    }
}

/// An agent's good news (#809, `hocus celebrate WHY`): the line `celebrate
/// PID NAME`, then a tab and why. `PID` the process that has it (`-`
/// unknown), whose agent it is; `NAME` the name it shows under when no
/// agent is known for that process (its folder), maybe empty.
#[derive(Clone, PartialEq, Eq)]
pub struct Celebration {
    pub pid: Option<u32>,
    pub name: String,
    /// What the news is, one short line ([`hocus_link::note_text`]); never
    /// logged but its length.
    pub why: String,
}

impl fmt::Debug for Celebration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Celebration")
            .field("pid", &self.pid)
            .field("name", &self.name)
            .field(
                "why",
                &format_args!("{} characters", self.why.chars().count()),
            )
            .finish()
    }
}

impl Celebration {
    /// News of `why` (cut as a note's text); none when it says nothing.
    pub fn new(pid: Option<u32>, name: &str, why: &str) -> Option<Self> {
        let why = hocus_link::note_text(why);
        (!why.is_empty()).then(|| Self {
            pid,
            name: name_text(name),
            why,
        })
    }
}

impl fmt::Display for Celebration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pid = self.pid.map_or_else(|| "-".to_owned(), |p| p.to_string());
        write!(
            f,
            "celebrate {pid} {}\t{}",
            name_text(&self.name),
            hocus_link::note_text(&self.why)
        )
    }
}

impl FromStr for Celebration {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let line = line.trim_end_matches(['\n', '\r']);
        let rest = line
            .strip_prefix("celebrate ")
            .ok_or_else(|| BadLine("not a celebration".to_owned()))?;
        let (head, why) = rest
            .split_once('\t')
            .ok_or_else(|| BadLine("a celebration without its news".to_owned()))?;
        let (pid, name) = head.split_once(' ').unwrap_or((head, ""));
        let pid = match pid {
            "-" => None,
            pid => Some(
                pid.parse()
                    .map_err(|e| BadLine(format!("a process: {e}")))?,
            ),
        };
        Self::new(pid, name, why).ok_or_else(|| BadLine("news that says nothing".to_owned()))
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
/// its sources know: herdr's (its tab's own name, else its workspace's
/// label); else its project's folder (a hook's `cwd`, or the command's
/// name); else its process's working directory's. Never its tool's name: herdr's agent kind
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

    /// A question is never an agent's report, nor a report a question.
    #[test]
    fn a_question_reads_back_and_is_no_report() {
        assert_eq!(Question::of(Question::Idle.line()), Some(Question::Idle));
        assert_eq!(Question::of("msg idle\n"), Some(Question::Idle));
        assert!(Question::Idle.line().parse::<Report>().is_err());
        assert_eq!(Question::of("agent command k idle - k"), None);
        assert_eq!(Question::of("msg"), None);
        assert_eq!(Question::of(Question::Lock.line()), Some(Question::Lock));
        assert_eq!(Question::of(Question::A11y.line()), Some(Question::A11y));
        for follow in [false, true] {
            let question = Question::InputDevices { follow };
            assert_eq!(Question::of(question.line()), Some(question));
        }
    }

    /// What is asked of a supervised program reads back as written, a
    /// path with spaces too, and is neither a question nor a report.
    #[test]
    fn a_programs_ask_reads_back() {
        let asks = [
            ProgramAsk::Status(Supervised::Shell),
            ProgramAsk::Restart(Supervised::Shell, Restart::Same),
            ProgramAsk::Restart(Supervised::Shell, Restart::Installed),
            ProgramAsk::Restart(
                Supervised::Shell,
                Restart::Given("/home/alex/my builds/hocuspanda-shell".into()),
            ),
        ];
        for ask in asks {
            assert_eq!(ProgramAsk::of(&ask.line()), Some(ask.clone()));
            assert_eq!(
                ProgramAsk::of(&format!("{}\n", ask.line())),
                Some(ask.clone())
            );
            assert_eq!(Question::of(&ask.line()), None);
            assert!(ask.line().parse::<Report>().is_err());
        }
        assert_eq!(ProgramAsk::of("msg shell"), None);
        assert_eq!(ProgramAsk::of("msg shell restart program "), None);
        assert_eq!(ProgramAsk::of("msg bar restart"), None);
        assert_eq!(ProgramAsk::of("msg idle"), None);
    }

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
                tests: None,
                conversation: Conversation::default(),
            },
            Report {
                source: Source::Command,
                key: "build".into(),
                state: None,
                pid: None,
                name: "build".into(),
                note: None,
                tests: None,
                conversation: Conversation::default(),
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
            tests: None,
            conversation: Conversation::default(),
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
    fn a_report_carries_its_conversation_apart_never_said() {
        let report = Report {
            source: Source::ClaudeCode,
            key: "3f6c2a1e-9b0d".into(),
            state: Some(State::Working),
            pid: Some(7),
            name: "hocus-site".into(),
            note: None,
            tests: Some(runs::TestRun::Failed),
            conversation: Conversation {
                cwd: Some("/home/alex/github/hocus site".into()),
                transcript: Some("/home/alex/.claude/projects/x/3f6c2a1e-9b0d.jsonl".into()),
                prompt: Some("refais la section\tdu haut".into()),
            },
        };
        let line = report.to_string();
        assert_eq!(
            line,
            "agent claude-code 3f6c2a1e-9b0d working 7 hocus-site\ttests failed\t\
             cwd /home/alex/github/hocus site\ttranscript /home/alex/.claude/projects/x/3f6c2a1e-9b0d.jsonl\t\
             prompt refais la section du haut"
        );
        let read: Report = line.parse().expect("a line");
        assert_eq!(
            read.conversation.prompt.as_deref(),
            Some("refais la section du haut")
        );
        assert_eq!(read.conversation.cwd, report.conversation.cwd);
        assert_eq!(read.tests, report.tests);
        let debug = format!("{report:?}");
        assert!(
            !debug.contains("alex") && !debug.contains("refais"),
            "{debug}"
        );
        // A relative path is not one; a long prompt is cut.
        assert_eq!(Conversation::path("x/y"), None);
        let long = "a".repeat(80);
        let cut = Conversation::prompt_text(&long).expect("a prompt");
        assert_eq!(cut.chars().count(), PROMPT_MOST);
        assert!(cut.ends_with('…'));
        assert_eq!(Conversation::prompt_text(" \n "), None);
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
            tests: None,
            conversation: Conversation::default(),
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
    fn a_report_carries_its_tests_apart_with_or_without_a_note() {
        let green = Report {
            source: Source::Command,
            key: "build".into(),
            state: Some(State::Done),
            pid: None,
            name: "build".into(),
            note: None,
            tests: Some(runs::TestRun::Passed(Some(2040))),
            conversation: Conversation::default(),
        };
        let line = green.to_string();
        assert_eq!(line, "agent command build done - build\ttests passed 2040");
        assert_eq!(line.parse::<Report>(), Ok(green.clone()));
        let both = Report {
            note: Note::says("all good"),
            tests: Some(runs::TestRun::Failed),
            ..green
        };
        assert_eq!(both.to_string().parse::<Report>(), Ok(both));
    }

    #[test]
    fn a_celebration_reads_back_and_is_no_report() {
        let news = Celebration::new(Some(42), "site web", "#791\tmerged\non main").expect("news");
        assert_eq!(news.why, "#791 merged on main");
        let line = news.to_string();
        assert_eq!(line, "celebrate 42 site web\t#791 merged on main");
        assert_eq!(line.parse::<Celebration>(), Ok(news));
        assert!(line.parse::<Report>().is_err());
        let unnamed: Celebration = "celebrate - \tdeployed".parse().expect("a line");
        assert_eq!((unnamed.pid, unnamed.name.as_str()), (None, ""));
        for bad in [
            "celebrate 1 x",
            "celebrate 1 x\t  ",
            "celebrate x y\tz",
            "agent",
        ] {
            assert!(bad.parse::<Celebration>().is_err(), "{bad}");
        }
        assert!(Celebration::new(None, "x", " \n").is_none());
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
            agent_name(Some("refacto"), Some("hocus-panda"), Some("x")),
            Some("refacto".into())
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
