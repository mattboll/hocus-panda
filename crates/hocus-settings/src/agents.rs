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

use std::sync::OnceLock;

use hocus_link::ToolState;

use crate::{
    lang::{Lang, Words, w},
    model::{Choice, Codec, Kind, Outside, Place, Places, RowGroup, SectionId, Setting, Value},
};

pub const TOOLS: RowGroup = RowGroup {
    label: w("LES OUTILS SUIVIS", "FOLLOWED TOOLS"),
    per_source: false,
};
pub const HALO_GROUP: RowGroup = RowGroup {
    label: w("LE HALO", "THE HALO"),
    per_source: false,
};
pub const PANDAS_GROUP: RowGroup = RowGroup {
    label: w("LES PANDAS", "THE PANDAS"),
    per_source: false,
};
pub const SECRETARY_GROUP: RowGroup = RowGroup {
    label: w("LA SECRÉTAIRE", "THE SECRETARY"),
    per_source: false,
};

pub const HALO: &str = "agents.halo";
pub const REMIND: &str = "agents.remind";
pub const PEEK: &str = "agents.peek";
pub const PANDAS: &str = "agents.pandas";
/// `[secretary] mode` (docs/design/secretary.md §21).
pub const SECRETARY: &str = "secretary.mode";
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
            Self::Plugged => (w("Branché", "Plugged in"), 0x7EE0A0),
            Self::ToApprove => (
                w("Branché · à approuver", "Plugged in · to approve"),
                0xF5C46E,
            ),
            Self::Found => (w("Trouvé, pas branché", "Found, not plugged in"), 0x8C96A8),
            Self::Declined => (w("Refusé", "Declined"), 0x8C96A8),
            Self::Absent => (
                w("Pas sur cet ordinateur", "Not on this computer"),
                0x8C96A8,
            ),
            Self::Running => (w("En marche", "Running"), 0x7EE0A0),
            Self::RunningIgnored => (
                w("En marche · pas suivi", "Running · not followed"),
                0x8C96A8,
            ),
            Self::Stopped => (w("Ne tourne pas", "Not running"), 0x8C96A8),
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
    match (row, lang) {
        (Row::Plugged, Lang::French) => format!("ses crochets sont dans {file}"),
        (Row::Plugged, Lang::English) => format!("its hooks are in {file}"),
        (Row::ToApprove, Lang::French) => {
            "dans Codex avec /hooks ; d’ici là, seule la fin de chaque tour arrive.".to_owned()
        }
        (Row::ToApprove, Lang::English) => {
            "in Codex with /hooks; until then only the end of each turn arrives.".to_owned()
        }
        (Row::Found, Lang::French) => format!("trouvé dans {dir}"),
        (Row::Found, Lang::English) => format!("found in {dir}"),
        (Row::Declined, Lang::French) => match declined {
            Some(day) => format!("avec « Ne rien brancher » le {day} · trouvé dans {dir}"),
            None => format!("avec « Ne rien brancher » · trouvé dans {dir}"),
        },
        (Row::Declined, Lang::English) => match declined {
            Some(day) => format!("with “Plug nothing in” on {day} · found in {dir}"),
            None => format!("with “Plug nothing in” · found in {dir}"),
        },
        (Row::Running, Lang::French) => {
            "lu tout seul, rien à brancher ; ses noms d’onglets nomment les agents.".to_owned()
        }
        (Row::Running, Lang::English) => {
            "read by itself, nothing to plug in; its tab names name the agents.".to_owned()
        }
        (Row::RunningIgnored, Lang::French) => {
            "ses agents ne sont pas suivis ; « Suivre herdr » les reprend.".to_owned()
        }
        (Row::RunningIgnored, Lang::English) => {
            "its agents are not followed; “Follow herdr” brings them back.".to_owned()
        }
        (Row::Stopped, Lang::French) => "suivi dès qu’il démarre".to_owned(),
        (Row::Stopped, Lang::English) => "followed as soon as it starts".to_owned(),
        (Row::Absent, _) => String::new(),
    }
}

/// "~/.claude/settings.json ne se lit pas : il n'est pas touché."
pub fn broken(file: &str, lang: Lang) -> String {
    match lang {
        Lang::French => format!("{file} ne se lit pas : il n’est pas touché."),
        Lang::English => format!("{file} does not read: it is left alone."),
    }
}

/// "2 suivis · 3 agents en cours".
pub fn count_said(followed: usize, running: usize, lang: Lang) -> String {
    let running = match (lang, running) {
        (Lang::French, 0) => "aucun agent en cours".to_owned(),
        (Lang::French, 1) => "1 agent en cours".to_owned(),
        (Lang::French, n) => format!("{n} agents en cours"),
        (Lang::English, 0) => "no agent running".to_owned(),
        (Lang::English, 1) => "1 agent running".to_owned(),
        (Lang::English, n) => format!("{n} agents running"),
    };
    let followed = match (lang, followed) {
        (Lang::French, n) if n <= 1 => format!("{n} suivi"),
        (Lang::French, n) => format!("{n} suivis"),
        (Lang::English, n) => format!("{n} followed"),
    };
    format!("{followed} · {running}")
}

/// The day of `at` (seconds since the epoch) in the user's zone, as a
/// line says it: "30 octobre", "30 October".
pub fn day_said(at: i64, lang: Lang) -> Option<String> {
    let zoned = jiff::Timestamp::from_second(at)
        .ok()?
        .to_zoned(jiff::tz::TimeZone::system());
    let months_fr = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    let months_en = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month = usize::try_from(zoned.month() - 1).ok()?;
    Some(match lang {
        Lang::French => format!("{} {}", zoned.day(), months_fr.get(month)?),
        Lang::English => format!("{} {}", zoned.day(), months_en.get(month)?),
    })
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
            Self::Waiting => w("Attend", "Waiting"),
            Self::Stuck => w("Bloqué", "Stuck"),
            Self::Done => w("Fini", "Done"),
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
            Self::Waiting => w(
                "Un agent t’attend : une vague, puis la lueur reste jusqu’à ce que tu y ailles.",
                "An agent waits for you: a wave, then the glow stays until you go to it.",
            ),
            Self::Stuck => w(
                "Un agent est bloqué : la lueur orange, sans vague.",
                "An agent is stuck: the orange glow, without a wave.",
            ),
            Self::Done => w(
                "Un agent a fini : une lueur douce, sans vague, jusqu’à ce que tu le voies.",
                "An agent is done: a soft glow, without a wave, until you see it.",
            ),
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
            name: w("Les outils suivis", "Followed tools"),
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
            name: w("Un autre outil", "Another tool"),
            keywords: (
                &["commande", "hocus agent", "programme", "état"],
                &["command", "hocus agent", "program", "state"],
            ),
        },
        Outside {
            id: PREVIEW,
            section: SectionId::Agents,
            group: HALO_GROUP.label,
            name: w("Aperçu", "Preview"),
            keywords: (
                &["halo", "lueur", "vague", "aperçu"],
                &["halo", "glow", "wave", "preview"],
            ),
        },
        Outside {
            id: REST,
            section: SectionId::Agents,
            group: PANDAS_GROUP.label,
            name: w("Jamais au repos", "Never at rest"),
            keywords: (
                &["panda", "pandas", "mouvement", "repos", "économie"],
                &["panda", "pandas", "motion", "rest", "power saving"],
            ),
        },
    ];
    &ALL
}

fn one(key: &'static [&'static str], codec: Codec, default: Value) -> Places {
    Places::One(Place {
        key,
        codec,
        default,
    })
}

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: w(fr, en),
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
            w("Quand il t’attend", "When it waits for you").get(lang)
        }
        (PEEK, Value::Word("waiting-and-done")) => {
            w("Attend ou a fini", "Waits or is done").get(lang)
        }
        _ => return None,
    })
}

/// What a choice says beside it in its menu.
pub fn aside(id: &str, value: &Value, lang: Lang) -> Option<&'static str> {
    match (id, value) {
        (HALO, Value::Word("all")) => {
            Some(w("attend, bloqué, fini", "waits, stuck, done").get(lang))
        }
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
                w("Le halo", "The halo"),
                w(
                    "La barre s’illumine : ambre quand un agent t’attend, orange s’il est bloqué, vert quand il a fini.",
                    "The bar lights up: amber when an agent waits for you, orange when it is stuck, green when it is done.",
                ),
                (
                    &["halo", "lueur", "vague", "barre", "illumine"],
                    &["halo", "glow", "wave", "bar", "lights"],
                ),
                Kind::Menu(vec![
                    word("Pour tout", "For everything", "all"),
                    word(
                        "Quand un agent t’attend",
                        "When an agent waits for you",
                        "waiting",
                    ),
                    word("Jamais", "Never", "off"),
                ]),
                one(
                    &["agents", "halo"],
                    Codec::Word(HALO_WORDS),
                    Value::Word("all"),
                ),
            ),
            setting(
                REMIND,
                HALO_GROUP,
                w(
                    "Rappeler un agent qui t’attend",
                    "Remind me of a waiting agent",
                ),
                w(
                    "Une vague de plus, une seule fois, s’il attend encore.",
                    "One more wave, once, if it still waits.",
                ),
                (
                    &["rappel", "rappeler", "vague", "attend"],
                    &["reminder", "remind", "wave", "waits"],
                ),
                Kind::Menu(vec![
                    word("Après 2 min", "After 2 min", "2m"),
                    word("Après 5 min", "After 5 min", "5m"),
                    word("Après 10 min", "After 10 min", "10m"),
                    word("Jamais", "Never", "never"),
                ]),
                one(
                    &["agents", "remind"],
                    Codec::Word(REMIND_WORDS),
                    Value::Word("5m"),
                ),
            ),
            setting(
                PEEK,
                PANDAS_GROUP,
                w(
                    "Le panda qui sort de la barre",
                    "The panda peeking out of the bar",
                ),
                w(
                    "Il sort 8 s avec une bulle, puis rentre ; un clic va à l’agent.",
                    "It comes out for 8 s with a bubble, then goes back; a click goes to the agent.",
                ),
                (
                    &["panda", "peek", "bulle", "sort", "secrétaire"],
                    &["panda", "peek", "bubble", "peeking", "secretary"],
                ),
                Kind::Menu(vec![
                    word(
                        "Quand un agent t’attend ou a fini",
                        "When an agent waits for you or is done",
                        "waiting-and-done",
                    ),
                    word(
                        "Quand un agent t’attend",
                        "When an agent waits for you",
                        "waiting",
                    ),
                    word("Jamais", "Never", "off"),
                ]),
                one(
                    &["agents", "peek"],
                    Codec::Word(PEEK_WORDS),
                    Value::Word("waiting-and-done"),
                ),
            ),
            setting(
                PANDAS,
                PANDAS_GROUP,
                w(
                    "Les pandas dans les lignes des agents",
                    "Pandas in the agents’ rows",
                ),
                w(
                    "Un panda par agent, qui montre son état ; coupé, un rond de sa couleur.",
                    "A panda per agent, showing its state; off, a dot of its colour.",
                ),
                (
                    &["panda", "pandas", "rond", "couleur", "lignes"],
                    &["panda", "pandas", "dot", "colour", "rows"],
                ),
                Kind::Switch,
                one(&["agents", "pandas"], Codec::Bool, Value::Bool(true)),
            ),
            setting(
                SECRETARY,
                SECRETARY_GROUP,
                w("La secrétaire assistante", "The assistant secretary"),
                w(
                    "Elle connaît ton équipe et tes projets, lit ton agenda et tes messages sans rien marquer lu, et propose ; tu décides. Coupée, elle ne parle que des agents.",
                    "She knows your team and projects, reads your calendar and messages without marking anything read, and proposes; you decide. Off, she only speaks of the agents.",
                ),
                (
                    &["secrétaire", "assistante", "mémoire", "classique", "agenda"],
                    &["secretary", "assistant", "memory", "classic", "calendar"],
                ),
                Kind::Switch,
                one(
                    &["secretary", "mode"],
                    Codec::Flag {
                        off: "classic",
                        on: "assistant",
                    },
                    Value::Bool(false),
                ),
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
    fn the_secretary_s_switch_writes_classic_or_assistant() {
        let codec = Codec::Flag {
            off: "classic",
            on: "assistant",
        };
        assert_eq!(codec.read(&"assistant".into()), Some(Value::Bool(true)));
        assert_eq!(codec.read(&"classic".into()), Some(Value::Bool(false)));
        assert_eq!(codec.read(&"on".into()), None);
        assert_eq!(
            codec
                .write(&Value::Bool(true))
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("assistant".to_owned())
        );
        let names: Vec<&str> = crate::search::find("secrétaire", Lang::French)
            .iter()
            .map(|f| f.name)
            .collect();
        assert!(names.contains(&"La secrétaire assistante"), "{names:?}");
    }

    #[test]
    fn the_preview_shows_what_the_halo_shows() {
        assert!(Played::ALL.iter().all(|p| p.shown_by("all")));
        assert!(Played::Waiting.shown_by("waiting") && !Played::Done.shown_by("waiting"));
        assert!(Played::ALL.iter().all(|p| !p.shown_by("off")));
    }
}
