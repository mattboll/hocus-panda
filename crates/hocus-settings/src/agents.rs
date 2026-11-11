//! Agents / Agents (6.16, issue #450, variant A): where the agents' states
//! come from, which tools are followed, and how the bar calls the user —
//! the halo and the pandas.
//!
//! The tools are not in the file: the compositor's watcher finds them,
//! plugs them in and out (4.0, `hocus-agents`), and says them on the
//! Settings' line (`agent-tools`), which the page asks `agent-plug`,
//! `agent-unplug`, `agent-follow herdr` and `agent-unfollow herdr`. The
//! halo, its reminder, the peek and the rows' pandas are `[agents]`'s
//! keys, which the compositor follows as the file changes.

use hocus_words::settings::agents as words;
use std::sync::OnceLock;

use hocus_link::ToolState;

use crate::{
    lang::{Lang, Words},
    model::{Choice, Codec, Kind, Outside, Places, RowGroup, SectionId, Setting, Value, one},
};

pub const TOOLS: RowGroup = RowGroup {
    label: words::FOLLOWED_TOOLS,
    per_source: false,
};
pub const HALO_GROUP: RowGroup = RowGroup {
    label: words::THE_HALO,
    per_source: false,
};
pub const PANDAS_GROUP: RowGroup = RowGroup {
    label: words::THE_PANDAS,
    per_source: false,
};

pub const HALO: &str = "agents.halo";
pub const REMIND: &str = "agents.remind";
pub const PEEK: &str = "agents.peek";
pub const PANDAS: &str = "agents.pandas";
/// The page's own rows, for the search and the lit row.
pub const TOOLS_ROW: &str = "agents.tools";
pub const PREVIEW: &str = "agents.preview";
pub const REST: &str = "agents.rest";
pub const ANOTHER: &str = "agents.another";

/// What any program can run to say where its agent is.
pub const COMMAND: &str = "hocus agent mon-agent waiting";

const HALO_WORDS: &[&str] = &["all", "waiting", "off"];
const REMIND_WORDS: &[&str] = &["2m", "5m", "10m", "never"];
const PEEK_WORDS: &[&str] = &["waiting-and-done", "waiting", "off"];

/// A tool the page lists, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    ClaudeCode,
    Codex,
    Herdr,
}

impl Tool {
    pub const ALL: [Self; 3] = [Self::ClaudeCode, Self::Codex, Self::Herdr];

    /// The compositor's word for it.
    pub const fn word(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Herdr => "herdr",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.word() == word)
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Herdr => "herdr",
        }
    }

    /// Its mark: a letter on its colour (4.0's).
    pub const fn mark(self) -> (&'static str, u32) {
        match self {
            Self::ClaudeCode => ("C", 0xE8A87C),
            Self::Codex => ("X", 0xA9B2C2),
            Self::Herdr => ("H", 0x8FB3FF),
        }
    }

    /// The hooks' tool, for the files' checks; herdr has none.
    pub const fn hooks(self) -> Option<hocus_agents::tools::Tool> {
        match self {
            Self::ClaudeCode => Some(hocus_agents::tools::Tool::ClaudeCode),
            Self::Codex => Some(hocus_agents::tools::Tool::Codex),
            Self::Herdr => None,
        }
    }
}

/// One tool as the compositor says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub tool: Tool,
    pub state: ToolState,
    /// The file plugging it in changes, as the user knows it.
    pub file: String,
}

/// The tools as the compositor's line says them (`agent-tools`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tools {
    pub list: Vec<Said>,
    /// The compositor looks for tools at all (a session; not a test
    /// without a home of its own).
    pub watched: bool,
    pub herdr_found: bool,
    pub herdr_declined: bool,
    pub codex_approved: bool,
    /// How many agents run.
    pub running: usize,
}

impl Tools {
    /// One JSON line's `agent-tools`.
    pub fn read(value: &serde_json::Value) -> Option<Self> {
        let flag = |key: &str| value.get(key).and_then(serde_json::Value::as_bool);
        let list = value
            .get("tools")?
            .as_array()?
            .iter()
            .filter_map(|t| {
                Some(Said {
                    tool: Tool::of(t.get("tool")?.as_str()?)?,
                    state: t.get("state")?.as_str()?.parse().ok()?,
                    file: t
                        .get("file")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect();
        Some(Self {
            list,
            watched: flag("watched").unwrap_or(true),
            herdr_found: flag("herdr_found").unwrap_or(false),
            herdr_declined: flag("herdr_declined").unwrap_or(false),
            codex_approved: flag("codex_approved").unwrap_or(false),
            running: value
                .get("running")
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(0),
        })
    }

    pub fn of(&self, tool: Tool) -> Option<&Said> {
        self.list.iter().find(|s| s.tool == tool)
    }

    pub fn state(&self, tool: Tool) -> ToolState {
        self.of(tool).map_or(ToolState::Absent, |s| s.state)
    }

    /// What the page shows of `tool`.
    pub fn row(&self, tool: Tool) -> Row {
        match tool {
            Tool::Herdr => match self.state(tool) {
                ToolState::Running => Row::Running,
                ToolState::Ignored => Row::RunningIgnored,
                _ if self.herdr_found => Row::Stopped,
                _ => Row::Absent,
            },
            Tool::ClaudeCode | Tool::Codex => match self.state(tool) {
                ToolState::Plugged if tool == Tool::Codex && !self.codex_approved => Row::ToApprove,
                ToolState::Plugged => Row::Plugged,
                ToolState::Found => Row::Found,
                ToolState::Declined => Row::Declined,
                _ => Row::Absent,
            },
        }
    }

    /// How many tools are followed.
    pub fn followed(&self) -> usize {
        Tool::ALL
            .into_iter()
            .filter(|t| {
                matches!(
                    self.row(*t),
                    Row::Plugged | Row::ToApprove | Row::Running | Row::Stopped
                ) && !(*t == Tool::Herdr && self.herdr_declined)
            })
            .count()
    }

    /// No tool on this computer at all.
    pub fn none(&self) -> bool {
        Tool::ALL.into_iter().all(|t| self.row(t) == Row::Absent)
    }
}

/// A tool's row: its state, and what its action is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Plugged,
    /// Codex plugged in, its hooks not approved yet.
    ToApprove,
    Found,
    /// Declined with "Ne rien brancher".
    Declined,
    Absent,
    /// herdr runs and is followed.
    Running,
    /// herdr runs and is not followed.
    RunningIgnored,
    /// herdr is here and does not run.
    Stopped,
}

impl Row {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Plugged => "plugged",
            Self::ToApprove => "to-approve",
            Self::Found => "found",
            Self::Declined => "declined",
            Self::Absent => "absent",
            Self::Running => "running",
            Self::RunningIgnored => "running-ignored",
            Self::Stopped => "stopped",
        }
    }

    /// Its state's words and colour.
    pub fn state(self) -> (Words, u32) {
        match self {
            Self::Plugged => (words::PLUGGED_IN, 0x7EE0A0),
            Self::ToApprove => (words::PLUGGED_IN_TO_APPROVE, 0xF5C46E),
            Self::Found => (words::FOUND_NOT_PLUGGED_IN, 0x8C96A8),
            Self::Declined => (words::DECLINED, 0x8C96A8),
            Self::Absent => (words::NOT_ON_THIS_COMPUTER, 0x8C96A8),
            Self::Running => (words::RUNNING, 0x7EE0A0),
            Self::RunningIgnored => (words::RUNNING_NOT_FOLLOWED, 0x8C96A8),
            Self::Stopped => (words::NOT_RUNNING, 0x8C96A8),
        }
    }
}

/// What a tool's line says after its state; `file` as the user knows it,
/// `declined` the day it was declined ("30 octobre").
pub fn line(row: Row, tool: Tool, file: &str, declined: Option<&str>, lang: Lang) -> String {
    let dir = match tool {
        Tool::ClaudeCode => "~/.claude",
        Tool::Codex => "~/.codex",
        Tool::Herdr => "",
    };
    match row {
        Row::Plugged => words::hooks_in(lang, file),
        Row::ToApprove => words::TO_APPROVE.get(lang).to_owned(),
        Row::Found => words::found_in(lang, dir),
        Row::Declined => match declined {
            Some(day) => words::declined_on(lang, words::DeclinedOn { day, dir }),
            None => words::declined_found_in(lang, dir),
        },
        Row::Running => words::HERDR_RUNNING.get(lang).to_owned(),
        Row::RunningIgnored => words::HERDR_IGNORED.get(lang).to_owned(),
        Row::Stopped => words::HERDR_STOPPED.get(lang).to_owned(),
        Row::Absent => String::new(),
    }
}

/// "~/.claude/settings.json ne se lit pas : il n'est pas touché."
pub fn broken(file: &str, lang: Lang) -> String {
    words::does_not_read_it_is_left(lang, file)
}

/// "2 suivis · 3 agents en cours".
pub fn count_said(followed: usize, running: usize, lang: Lang) -> String {
    let running = match running {
        0 => words::NO_AGENT_RUNNING.get(lang).to_owned(),
        1 => words::N_1_AGENT_RUNNING.get(lang).to_owned(),
        n => words::agents_running(lang, n),
    };
    let followed = words::followed(lang, followed);
    format!("{followed} · {running}")
}

/// The day of `at` (seconds since the epoch) in the user's zone, as a
/// line says it: "30 octobre", "30 October".
pub fn day_said(at: i64, lang: Lang) -> Option<String> {
    let zoned = jiff::Timestamp::from_second(at)
        .ok()?
        .to_zoned(jiff::tz::TimeZone::system());
    let month = usize::try_from(zoned.month() - 1).ok()?;
    let month = hocus_words::date::MONTHS.get(lang).get(month)?;
    Some(words::day_month(
        lang,
        words::DayMonth {
            day: zoned.day(),
            month,
        },
    ))
}

/// A state the preview plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Played {
    Waiting,
    Stuck,
    Done,
}

impl Played {
    pub const ALL: [Self; 3] = [Self::Waiting, Self::Stuck, Self::Done];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Stuck => "stuck",
            Self::Done => "done",
        }
    }

    pub fn label(self) -> Words {
        match self {
            Self::Waiting => words::WAITING,
            Self::Stuck => words::STUCK,
            Self::Done => words::DONE,
        }
    }

    /// The halo's colour for it (bar.md §6).
    pub const fn colour(self) -> u32 {
        match self {
            Self::Waiting => 0xF5B44A,
            Self::Stuck => 0xF2945B,
            Self::Done => 0x6CC287,
        }
    }

    /// What the preview's line says of it.
    pub fn said(self) -> Words {
        match self {
            Self::Waiting => words::AN_AGENT_WAITS_FOR_YOU_A,
            Self::Stuck => words::AN_AGENT_IS_STUCK_THE_ORANGE,
            Self::Done => words::AN_AGENT_IS_DONE_A_SOFT,
        }
    }

    /// Whether `halo` (the key's word) shows it.
    pub fn shown_by(self, halo: &str) -> bool {
        match halo {
            "off" => false,
            "waiting" => self == Self::Waiting,
            _ => true,
        }
    }
}

/// The page's rows the model does not hold, for the search.
pub fn outside() -> &'static [Outside] {
    const ALL: [Outside; 4] = [
        Outside {
            id: TOOLS_ROW,
            section: SectionId::Agents,
            group: TOOLS.label,
            name: words::FOLLOWED_TOOLS_2,
            keywords: (
                &[
                    "outils",
                    "claude",
                    "claude code",
                    "codex",
                    "herdr",
                    "brancher",
                    "débrancher",
                    "crochets",
                    "ne rien brancher",
                    "refusé",
                    "suivre",
                    "agent",
                ],
                &[
                    "tools",
                    "claude",
                    "claude code",
                    "codex",
                    "herdr",
                    "plug",
                    "unplug",
                    "hooks",
                    "plug nothing in",
                    "declined",
                    "follow",
                    "agent",
                ],
            ),
        },
        Outside {
            id: ANOTHER,
            section: SectionId::Agents,
            group: TOOLS.label,
            name: words::ANOTHER_TOOL,
            keywords: (
                &["commande", "hocus agent", "programme", "état"],
                &["command", "hocus agent", "program", "state"],
            ),
        },
        Outside {
            id: PREVIEW,
            section: SectionId::Agents,
            group: HALO_GROUP.label,
            name: words::PREVIEW,
            keywords: (
                &["halo", "lueur", "vague", "aperçu"],
                &["halo", "glow", "wave", "preview"],
            ),
        },
        Outside {
            id: REST,
            section: SectionId::Agents,
            group: PANDAS_GROUP.label,
            name: words::NEVER_AT_REST,
            keywords: (
                &["panda", "pandas", "mouvement", "repos", "économie"],
                &["panda", "pandas", "motion", "rest", "power saving"],
            ),
        },
    ];
    &ALL
}

fn word(text: hocus_words::Text, word: &'static str) -> Choice {
    Choice {
        label: text,
        value: Value::Word(word),
    }
}

fn setting(
    id: &'static str,
    group: RowGroup,
    name: Words,
    line: Words,
    keywords: (&'static [&'static str], &'static [&'static str]),
    kind: Kind,
    places: Places,
) -> Setting {
    Setting {
        id,
        section: SectionId::Agents,
        group,
        name,
        line: Some(line),
        keywords,
        kind,
        places,
        needs: None,
    }
}

/// What a menu's button says, shorter than its choice when it must be.
pub fn button_said(id: &str, value: &Value, lang: Lang) -> Option<&'static str> {
    Some(match (id, value) {
        (HALO, Value::Word("waiting")) | (PEEK, Value::Word("waiting")) => {
            words::WHEN_IT_WAITS_FOR_YOU.get(lang)
        }
        (PEEK, Value::Word("waiting-and-done")) => words::WAITS_OR_IS_DONE.get(lang),
        _ => return None,
    })
}

/// What a choice says beside it in its menu.
pub fn aside(id: &str, value: &Value, lang: Lang) -> Option<&'static str> {
    match (id, value) {
        (HALO, Value::Word("all")) => Some(words::WAITS_STUCK_DONE.get(lang)),
        _ => None,
    }
}

/// The page's settings, in its order.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        vec![
            setting(
                HALO,
                HALO_GROUP,
                words::THE_HALO_2,
                words::THE_BAR_LIGHTS_UP_AMBER_WHEN,
                (
                    &["halo", "lueur", "vague", "barre", "illumine"],
                    &["halo", "glow", "wave", "bar", "lights"],
                ),
                Kind::Menu(vec![
                    word(words::FOR_EVERYTHING, "all"),
                    word(words::WHEN_AN_AGENT_WAITS_FOR_YOU, "waiting"),
                    word(words::NEVER, "off"),
                ]),
                one(&["agents", "halo"], Codec::Word(HALO_WORDS)),
            ),
            setting(
                REMIND,
                HALO_GROUP,
                words::REMIND_ME_OF_A_WAITING_AGENT,
                words::ONE_MORE_WAVE_ONCE_IF_IT,
                (
                    &["rappel", "rappeler", "vague", "attend"],
                    &["reminder", "remind", "wave", "waits"],
                ),
                Kind::Menu(vec![
                    word(words::AFTER_2_MIN, "2m"),
                    word(words::AFTER_5_MIN, "5m"),
                    word(words::AFTER_10_MIN, "10m"),
                    word(words::NEVER, "never"),
                ]),
                one(&["agents", "remind"], Codec::Word(REMIND_WORDS)),
            ),
            setting(
                PEEK,
                PANDAS_GROUP,
                words::THE_PANDA_PEEKING_OUT_OF_THE,
                words::IT_COMES_OUT_FOR_8_S,
                (
                    &["panda", "peek", "bulle", "sort", "secrétaire"],
                    &["panda", "peek", "bubble", "peeking", "secretary"],
                ),
                Kind::Menu(vec![
                    word(words::WHEN_AN_AGENT_WAITS_FOR_YOU_2, "waiting-and-done"),
                    word(words::WHEN_AN_AGENT_WAITS_FOR_YOU, "waiting"),
                    word(words::NEVER, "off"),
                ]),
                one(&["agents", "peek"], Codec::Word(PEEK_WORDS)),
            ),
            setting(
                PANDAS,
                PANDAS_GROUP,
                words::PANDAS_IN_THE_AGENTS_ROWS,
                words::A_PANDA_PER_AGENT_SHOWING_ITS,
                (
                    &["panda", "pandas", "rond", "couleur", "lignes"],
                    &["panda", "pandas", "dot", "colour", "rows"],
                ),
                Kind::Switch,
                one(&["agents", "pandas"], Codec::Bool),
            ),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_reads_and_says_each_row() {
        let said = serde_json::json!({ "tools": [
            { "tool": "claude-code", "state": "plugged", "file": "~/.claude/settings.json" },
            { "tool": "codex", "state": "declined", "file": "~/.codex/config.toml" },
            { "tool": "herdr", "state": "stopped", "file": "" },
        ], "herdr_found": true, "herdr_declined": false, "codex_approved": false, "running": 3 });
        let tools = Tools::read(&said).expect("reads");
        assert_eq!(tools.row(Tool::ClaudeCode), Row::Plugged);
        assert_eq!(tools.row(Tool::Codex), Row::Declined);
        assert_eq!(tools.row(Tool::Herdr), Row::Stopped);
        assert_eq!(tools.followed(), 2);
        assert_eq!(
            count_said(tools.followed(), tools.running, Lang::French),
            "2 suivis · 3 agents en cours"
        );
        assert_eq!(
            count_said(0, 0, Lang::English),
            "0 followed · no agent running"
        );
        assert!(!tools.none());
        let mut codex = tools.clone();
        codex.list[1].state = ToolState::Plugged;
        assert_eq!(codex.row(Tool::Codex), Row::ToApprove);
        codex.codex_approved = true;
        assert_eq!(codex.row(Tool::Codex), Row::Plugged);
        let none = Tools::default();
        assert!(none.none());
        assert_eq!(
            line(
                Row::Declined,
                Tool::Codex,
                "",
                Some("30 octobre"),
                Lang::French
            ),
            "avec « Ne rien brancher » le 30 octobre · trouvé dans ~/.codex"
        );
    }

    #[test]
    fn searching_crochets_halo_and_the_keys_finds_the_rows() {
        let names = |query| -> Vec<&str> {
            crate::search::find(query, Lang::French)
                .iter()
                .map(|f| f.name)
                .collect()
        };
        assert!(names("crochets").contains(&"Les outils suivis"));
        assert!(names("lueur").contains(&"Le halo"));
        assert!(names("agents.pandas").contains(&"Les pandas dans les lignes des agents"));
        assert!(names("hocus agent").contains(&"Un autre outil"));
    }

    #[test]
    fn the_preview_shows_what_the_halo_shows() {
        assert!(Played::ALL.iter().all(|p| p.shown_by("all")));
        assert!(Played::Waiting.shown_by("waiting") && !Played::Done.shown_by("waiting"));
        assert!(Played::ALL.iter().all(|p| !p.shown_by("off")));
    }
}
