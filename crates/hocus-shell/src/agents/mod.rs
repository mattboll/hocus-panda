//! The agents' group in the bar (bar.md §4 group 3; 4.0, 4.1, 4.6): at its
//! head the secretary (§7 B), her ear in the moment's colour, "AGENTS" and
//! how many run over her line; a click on her opens her briefing, where
//! "Sources" is. Under her one row per agent (4.1): its panda, its name,
//! and what it is doing, in the order of what concerns the user first, at
//! most five and a line for the rest. Before any tool is plugged in, the card that proposes to plug in
//! the tools found; with the tools followed and no agent, "Aucun agent en
//! cours.". With no tool followed and none to propose, it does not show at
//! all (variant B). In the mini form, an item under the clock holds her
//! head, a badge with how many need the user and how many work (4.7).
//!
//! The compositor owns the agents and the tools: it says which tools are
//! there and what each is (`agent-tools`), and which agents run
//! (`agents`), as the shell connects and when they change. The link keeps
//! them here without a message, lays the group out (its text measured
//! once), resizes its surface if its height changed and wakes it
//! ([`draw`]); an open briefing, Sources or consent popover draws again
//! for them. A working agent's minutes, and the briefing's, go on with the
//! clock's minute ([`minute`]): nothing draws for them in between.

pub mod draw;
pub mod peek;

use std::sync::{Mutex, PoisonError};

use hocus_link::{AGENTS_NAMESPACE, AgentShown, AgentTool, Note, ToolState};
use iced::{Color, Font, Point, Rectangle, Size};

use crate::{
    clock::time::Lang,
    levels::draw::{LINE, MANROPE, MANROPE_BOLD, rgb},
    popover::{self, Item, kit::MANROPE_SEMIBOLD},
};

/// A tool that hooks plug in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    ClaudeCode,
    Codex,
}

impl Tool {
    pub const ALL: [Self; 2] = [Self::ClaudeCode, Self::Codex];

    /// The compositor's word for it.
    pub const fn word(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    /// Its mark: a letter on its colour (the mockup's).
    pub const fn mark(self) -> (&'static str, Color) {
        match self {
            Self::ClaudeCode => ("C", rgb(0xE8A87C)),
            Self::Codex => ("X", rgb(0xA9B2C2)),
        }
    }
}

/// herdr's mark.
pub const HERDR_MARK: (&str, Color) = ("H", rgb(0x8FB3FF));
/// A mark's letter.
pub const MARK_INK: Color = rgb(0x0B1020);

/// The tools and the agents as last said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Said {
    pub tools: Vec<AgentTool>,
    pub agents: Vec<AgentShown>,
}

impl Said {
    /// `tool`'s state, absent when not said.
    pub fn state(&self, tool: &str) -> ToolState {
        self.tools
            .iter()
            .find(|t| t.tool == tool)
            .map_or(ToolState::Absent, |t| t.state)
    }

    /// The file plugging `tool` in changes, as the user knows it.
    pub fn file(&self, tool: Tool) -> String {
        self.tools
            .iter()
            .find(|t| t.tool == tool.word())
            .map(|t| t.file.clone())
            .unwrap_or_default()
    }

    /// What the group shows at `now` (microseconds of `CLOCK_MONOTONIC`,
    /// the agents' `since`'s clock).
    pub fn shows(&self, now: u64) -> Shows {
        let to_plug: Vec<Tool> = Tool::ALL
            .into_iter()
            .filter(|t| self.state(t.word()) == ToolState::Found)
            .collect();
        let mut followed: Vec<&'static str> = Tool::ALL
            .into_iter()
            .filter(|t| self.state(t.word()).followed())
            .map(Tool::title)
            .collect();
        if self.state("herdr").followed() {
            followed.push("herdr");
        }
        match (
            to_plug.is_empty(),
            followed.is_empty(),
            self.agents.is_empty(),
        ) {
            (false, _, _) => Shows::Card(to_plug),
            (true, true, true) => Shows::Hidden,
            (true, false, true) => Shows::Idle(followed),
            (true, _, false) => Shows::Lines(Lines::of(&self.agents, now)),
        }
    }
}

/// What an agent's row says (4.1), from its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Look {
    /// It waits for the user: "t'attend", amber.
    Waiting,
    /// It failed: "bloqué", orange.
    Stuck,
    /// It finished and the user has not seen it yet: "à voir", green.
    ToSee,
    /// It works: for how long.
    Working,
    /// Idle, or finished and seen: "repos", its panda asleep.
    Resting,
}

impl Look {
    /// The compositor's word for the state (bar.md §10, `hocus-link`).
    pub fn of(state: &str) -> Self {
        match state {
            "waiting" => Self::Waiting,
            "failed" => Self::Stuck,
            "done" => Self::ToSee,
            "working" => Self::Working,
            _ => Self::Resting,
        }
    }
}

/// The rows shown, at most.
pub const MOST_ROWS: usize = 5;

/// The agents' rows, as the group shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lines {
    /// How many agents run, in the header.
    pub total: usize,
    /// The rows shown, from the top.
    pub rows: Vec<Line>,
    /// How many are not shown, and whether they all rest.
    pub hidden: usize,
    pub hidden_rest: bool,
}

/// An agent's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub id: u64,
    pub look: Look,
    pub name: String,
    /// How long it has worked, whole minutes (working only).
    pub minutes: u64,
}

impl Lines {
    /// `agents` in their rows' order at `now`: waiting (the longest first),
    /// stuck, to see, working, resting, in the order they came otherwise;
    /// the first five shown.
    pub fn of(agents: &[AgentShown], now: u64) -> Self {
        let mut ordered: Vec<(Look, &AgentShown)> =
            agents.iter().map(|a| (Look::of(&a.state), a)).collect();
        ordered.sort_by_key(|(look, a)| {
            let waited = if *look == Look::Waiting { a.since } else { 0 };
            (*look, waited, a.id)
        });
        let hidden = &ordered[ordered.len().min(MOST_ROWS)..];
        Self {
            total: agents.len(),
            hidden: hidden.len(),
            hidden_rest: hidden.iter().all(|(look, _)| *look == Look::Resting),
            rows: ordered
                .iter()
                .take(MOST_ROWS)
                .map(|(look, a)| Line {
                    id: a.id,
                    look: *look,
                    name: a.name.clone(),
                    minutes: match look {
                        Look::Working => now.saturating_sub(a.since) / 60_000_000,
                        _ => 0,
                    },
                })
                .collect(),
        }
    }
}

/// The secretary's mood (§7 B): what her ear and her line say. Someone
/// waiting comes first, as in the halo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mood {
    /// Nothing waits, nothing is stuck: green.
    Calm,
    /// An agent waits for the user: amber.
    Waiting,
    /// An agent failed: orange.
    Stuck,
}

/// What the secretary knows (4.6): who waits (the longest first), who is
/// stuck, who finished and is not seen yet, each with since when and what
/// its tool said; who works and who rests (the mini form's briefing lists
/// them, 4.7; the full form's counts them).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Brief {
    pub waiting: Vec<Entry>,
    pub stuck: Vec<Entry>,
    pub to_see: Vec<Entry>,
    pub working: Vec<Entry>,
    pub resting: Vec<Entry>,
}

/// An agent in the briefing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub name: String,
    /// Since when, microseconds of `CLOCK_MONOTONIC`.
    pub since: u64,
    pub note: Option<Note>,
}

impl Brief {
    /// `agents` as the secretary sees them, in their rows' order.
    pub fn of(agents: &[AgentShown]) -> Self {
        let mut ordered: Vec<(Look, &AgentShown)> =
            agents.iter().map(|a| (Look::of(&a.state), a)).collect();
        ordered.sort_by_key(|(look, a)| {
            let waited = if *look == Look::Waiting { a.since } else { 0 };
            (*look, waited, a.id)
        });
        let mut brief = Self::default();
        for (look, a) in ordered {
            let entry = || Entry {
                id: a.id,
                name: a.name.clone(),
                since: a.since,
                note: a.note.clone(),
            };
            match look {
                Look::Waiting => brief.waiting.push(entry()),
                Look::Stuck => brief.stuck.push(entry()),
                Look::ToSee => brief.to_see.push(entry()),
                Look::Working => brief.working.push(entry()),
                Look::Resting => brief.resting.push(entry()),
            }
        }
        brief
    }

    pub fn mood(&self) -> Mood {
        match (self.waiting.is_empty(), self.stuck.is_empty()) {
            (false, _) => Mood::Waiting,
            (true, false) => Mood::Stuck,
            (true, true) => Mood::Calm,
        }
    }

    /// How many agents need the user (waiting, stuck, to see): the mini
    /// form's badge (4.7).
    pub fn needs(&self) -> usize {
        self.waiting.len() + self.stuck.len() + self.to_see.len()
    }
}

/// Now, in microseconds of `CLOCK_MONOTONIC`: the clock of the agents'
/// `since`.
pub fn now_us() -> u64 {
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    (clock.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(clock.tv_nsec as u64 / 1_000)
}

/// What the agents' group shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shows {
    /// No tool followed, none to propose (variant B).
    Hidden,
    /// The card, for these tools found and not plugged in.
    Card(Vec<Tool>),
    /// No agent running: the tools followed.
    Idle(Vec<&'static str>),
    /// Agents run: a row for each (4.1).
    Lines(Lines),
}

static SAID: Mutex<Option<Said>> = Mutex::new(None);

/// The tools and agents as last said.
pub fn said() -> Said {
    SAID.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

fn change(with: impl FnOnce(&mut Said)) {
    let words = Words::new(Lang::of_messages());
    let (before, after) = {
        let mut said = SAID.lock().unwrap_or_else(PoisonError::into_inner);
        let said = said.get_or_insert_with(Said::default);
        let before = said.clone();
        with(said);
        (before, said.clone())
    };
    if before == after {
        return;
    }
    let shows = after.shows(now_us());
    let brief = Brief::of(&after.agents);
    tracing::debug!(
        shows = %words.said(&shows),
        secretary = %words.line(&brief),
        mood = ?brief.mood(),
        mini = %words.mini(&shows, &brief),
        agents = after.agents.len(),
        "the agents group shows it"
    );
    draw::stale();
    popover::send(vec![popover::Effect::Wake(AGENTS_NAMESPACE)]);
    popover::changed(Item::Sources);
    popover::changed(Item::Secretary);
    for tool in Tool::ALL {
        popover::changed(Item::Plug(tool));
    }
}

/// The group laid out for what was last said, its text measured with the
/// shell's fonts (as it draws: they are loaded by then).
pub fn lay_out() -> Layout {
    let said = said();
    let shows = said.shows(now_us());
    let words = Words::new(Lang::of_messages());
    layout(
        &words,
        &shows,
        &Brief::of(&said.agents),
        crate::popover::kit::measure,
    )
}

/// The clock's minute changed: the working agents' minutes, if one shows
/// and they changed, are drawn again (only then: nothing draws for them in
/// between).
pub fn minute() {
    // The briefing's "since when", if it is open.
    popover::changed(Item::Secretary);
    let Some(laid) = draw::layout() else {
        return;
    };
    let Shows::Lines(before) = &laid.shows else {
        return;
    };
    if !before.rows.iter().any(|r| r.look == Look::Working) {
        return;
    }
    let now = said().shows(now_us());
    // Once for every bar's clock.
    static WOKEN: Mutex<Option<Shows>> = Mutex::new(None);
    let mut woken = WOKEN.lock().unwrap_or_else(PoisonError::into_inner);
    if now != laid.shows && woken.as_ref() != Some(&now) {
        let words = Words::new(Lang::of_messages());
        tracing::debug!(shows = %words.said(&now), "the agents' minutes go on");
        *woken = Some(now);
        draw::stale();
        popover::send(vec![popover::Effect::Wake(AGENTS_NAMESPACE)]);
    }
}

/// The compositor says the tools.
pub fn set_tools(tools: Vec<AgentTool>) {
    change(|said| said.tools = tools);
}

/// The compositor says the agents.
pub fn set_agents(agents: Vec<AgentShown>) {
    change(|said| said.agents = agents);
}

// --- Words ----------------------------------------------------------------------

/// The group's and its popovers' words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    pub fn header(self) -> &'static str {
        "AGENTS"
    }

    pub fn sources(self) -> &'static str {
        "Sources"
    }

    /// The secretary's line (§7 B, 4.6): the names of who waits (the
    /// longest first), else of who is stuck, her mood's colour saying
    /// which (#503); else how many work, and how many are to see.
    pub fn line(self, brief: &Brief) -> String {
        let names = |entries: &[Entry]| {
            entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match brief.mood() {
            Mood::Waiting => names(&brief.waiting),
            Mood::Stuck => names(&brief.stuck),
            Mood::Calm => {
                let working = self.working(brief.working.len());
                match brief.to_see.len() {
                    0 => working,
                    n if self.fr => format!("{working} · {n} à voir"),
                    n => format!("{working} · {n} to see"),
                }
            }
        }
    }

    /// "3 au travail".
    pub fn working(self, n: usize) -> String {
        if self.fr {
            format!("{n} au travail")
        } else {
            format!("{n} working")
        }
    }

    /// The briefing's footer: "3 au travail · 1 au repos".
    pub fn footer(self, brief: &Brief) -> String {
        let resting = brief.resting.len();
        let resting = if self.fr {
            format!("{resting} au repos")
        } else {
            format!("{resting} resting")
        };
        format!("{} · {resting}", self.working(brief.working.len()))
    }

    /// The briefing's title.
    pub fn briefing(self) -> &'static str {
        self.pick("Le point", "Briefing")
    }

    /// Its sections' labels; the last two in the mini form's (4.7).
    pub fn section(self, look: Look) -> &'static str {
        match look {
            Look::Waiting => self.pick("T'ATTEND", "WAITING"),
            Look::Stuck => self.pick("BLOQUÉ", "STUCK"),
            Look::ToSee => self.pick("À VOIR", "TO SEE"),
            Look::Working => self.pick("AU TRAVAIL", "AT WORK"),
            Look::Resting => self.pick("AU REPOS", "RESTING"),
        }
    }

    /// The mini form's item, under how many work (4.7).
    pub fn at_work(self) -> &'static str {
        self.pick("au travail", "at work")
    }

    /// Since when, after a name: "· 2 min" (waiting, stuck), "· il y a
    /// 4 min" for a finished one (`ago`); "· à l'instant" within the
    /// minute; "1 h 05" from an hour.
    pub fn since(self, minutes: u64, ago: bool) -> String {
        if minutes == 0 {
            return format!("· {}", self.pick("à l'instant", "just now"));
        }
        let time = if minutes >= 60 {
            format!("{}\u{a0}h\u{a0}{:02}", minutes / 60, minutes % 60)
        } else {
            format!("{minutes}\u{a0}min")
        };
        match (ago, self.fr) {
            (false, _) => format!("· {time}"),
            (true, true) => format!("· il y a {time}"),
            (true, false) => format!("· {time} ago"),
        }
    }

    /// What the peek's bubble says after the agent's name (4.5).
    pub fn peek(self, kind: hocus_link::PeekKind) -> &'static str {
        match kind {
            hocus_link::PeekKind::Waiting => self.pick("t'attend", "is waiting"),
            hocus_link::PeekKind::Done => self.pick("a fini", "is done"),
        }
    }

    /// What a waiting or stuck agent's tool said, as the briefing words
    /// it: a request "Autoriser Bash : npm install ?", a text as it is.
    pub fn note(self, note: &Note) -> String {
        match (note, self.fr) {
            (Note::Says(text), _) => text.clone(),
            (Note::Allow { tool, what }, true) if what.is_empty() => {
                format!("Autoriser {tool}\u{a0}?")
            }
            (Note::Allow { tool, what }, true) => {
                format!("Autoriser {tool}\u{a0}: {what}\u{a0}?")
            }
            (Note::Allow { tool, what }, false) if what.is_empty() => format!("Allow {tool}?"),
            (Note::Allow { tool, what }, false) => format!("Allow {tool}: {what}?"),
        }
    }

    pub fn intro(self) -> &'static str {
        self.pick(
            "Pour suivre tes agents, branche ton outil\u{a0}:",
            "To follow your agents, plug in your tool:",
        )
    }

    pub fn plug(self) -> &'static str {
        self.pick("Brancher", "Plug in")
    }

    pub fn another(self) -> &'static str {
        self.pick("Un autre outil\u{a0}?", "Another tool?")
    }

    pub fn decline(self) -> &'static str {
        self.pick("Ne rien brancher", "Plug nothing in")
    }

    pub fn no_agent(self) -> &'static str {
        self.pick("Aucun agent en cours.", "No agent running.")
    }

    pub fn followed(self, names: &[&str]) -> String {
        let list = names.join(", ");
        if self.fr {
            format!("Suivis\u{a0}: {list}.")
        } else {
            format!("Following: {list}.")
        }
    }

    /// What the mini form's item says (4.7), for the log: its badge and
    /// its colour, and how many work.
    pub fn mini(self, shows: &Shows, brief: &Brief) -> String {
        match shows {
            Shows::Hidden => "nothing".to_owned(),
            Shows::Card(_) => "badge + green".to_owned(),
            Shows::Idle(_) | Shows::Lines(_) => {
                let working = format!("{} {}", brief.working.len(), self.at_work());
                match brief.needs() {
                    0 => working,
                    n => format!("badge {n} {} · {working}", badge_colour(brief).1),
                }
            }
        }
    }

    /// What the group says, for the log (and screen readers, once iced
    /// has their tree).
    pub fn said(self, shows: &Shows) -> String {
        match shows {
            Shows::Hidden => "nothing".to_owned(),
            Shows::Card(tools) => {
                let names: Vec<&str> = tools.iter().map(|t| t.title()).collect();
                format!("{} {}", self.intro(), names.join(", "))
            }
            Shows::Idle(names) => format!("{} {}", self.no_agent(), self.followed(names)),
            Shows::Lines(lines) => {
                let mut said = vec![format!("{} {}", self.header(), lines.total)];
                said.extend(
                    lines
                        .rows
                        .iter()
                        .map(|line| format!("{} {}", line.name, self.word(line))),
                );
                if lines.hidden > 0 {
                    said.push(self.more(lines.hidden, lines.hidden_rest));
                }
                said.join(" · ")
            }
        }
    }

    /// What an agent's row says on its right.
    pub fn word(self, line: &Line) -> String {
        match line.look {
            Look::Waiting => self.pick("t'attend", "waiting").to_owned(),
            Look::Stuck => self.pick("bloqué", "stuck").to_owned(),
            Look::ToSee => self.pick("à voir", "to see").to_owned(),
            Look::Resting => self.pick("repos", "resting").to_owned(),
            // "1 h 05" from an hour.
            Look::Working if line.minutes >= 60 => {
                format!("{}\u{a0}h\u{a0}{:02}", line.minutes / 60, line.minutes % 60)
            }
            Look::Working => format!("{}\u{a0}min", line.minutes),
        }
    }

    /// The line for the agents not shown: "+ 2 au repos" when they all
    /// rest, else "+ 2 autres".
    pub fn more(self, hidden: usize, rest: bool) -> String {
        let what = match rest {
            true => self.pick("au repos", "resting"),
            false => self.pick("autres", "more"),
        };
        format!("+\u{a0}{hidden} {what}")
    }

    pub fn plug_title(self, tool: Tool) -> String {
        if self.fr {
            format!("Brancher {}", tool.title())
        } else {
            format!("Plug in {}", tool.title())
        }
    }

    pub fn what(self, tool: Tool) -> &'static str {
        match tool {
            Tool::ClaudeCode => self.pick(
                "Hocuspanda ajoute ses crochets aux réglages de Claude Code\u{a0}: il saura quand un agent travaille, t'attend (une permission, une question) ou a fini.",
                "Hocuspanda adds its hooks to Claude Code's settings: it will know when an agent works, waits for you (a permission, a question) or is done.",
            ),
            Tool::Codex => self.pick(
                "Hocuspanda ajoute ses crochets et sa notification aux réglages de Codex\u{a0}: il saura quand un agent travaille, t'attend (une permission) ou a fini son tour. Codex te demande de les approuver avec /hooks.",
                "Hocuspanda adds its hooks and its notification to Codex's settings: it will know when an agent works, waits for you (a permission) or has finished its turn. Codex asks you to approve them with /hooks.",
            ),
        }
    }

    pub fn file_changed(self) -> &'static str {
        self.pick("Fichier modifié", "File changed")
    }

    pub fn copy_kept(self) -> &'static str {
        self.pick(
            "Une copie de l'original est gardée. «\u{a0}Débrancher\u{a0}», dans Sources, remet tout comme avant.",
            "A copy of the original is kept. “Unplug”, in Sources, puts everything back as it was.",
        )
    }

    pub fn cancel(self) -> &'static str {
        self.pick("Annuler", "Cancel")
    }

    pub fn sources_title(self) -> &'static str {
        self.pick("Sources des agents", "Agents' sources")
    }

    /// A tool's state in Sources, and whether it shows in green.
    pub fn tool_state(self, state: ToolState) -> (&'static str, bool) {
        match state {
            ToolState::Plugged => (self.pick("Branché", "Plugged in"), true),
            ToolState::Found | ToolState::Declined => (
                self.pick("Trouvé, pas branché", "Found, not plugged in"),
                false,
            ),
            ToolState::Absent => (
                self.pick("Pas sur cet ordinateur", "Not on this computer"),
                false,
            ),
            ToolState::Running => (
                self.pick(
                    "En marche · suivi tout seul",
                    "Running · followed by itself",
                ),
                true,
            ),
            ToolState::Ignored => (
                self.pick("En marche · pas suivi", "Running · not followed"),
                false,
            ),
            ToolState::Stopped => (self.pick("Ne tourne pas", "Not running"), false),
        }
    }

    /// Sources' button for a herdr declined.
    pub fn follow(self) -> &'static str {
        self.pick("Suivre", "Follow")
    }

    pub fn unplug(self) -> &'static str {
        self.pick("Débrancher", "Unplug")
    }

    pub fn another_label(self) -> &'static str {
        self.pick("UN AUTRE OUTIL", "ANOTHER TOOL")
    }

    pub fn any_tool(self) -> &'static str {
        self.pick(
            "N'importe quel outil peut dire où en est son agent avec une commande\u{a0}:",
            "Any tool can say how its agent is doing with a command:",
        )
    }

    pub fn command(self) -> &'static str {
        "hocus agent build waiting"
    }

    pub fn copy(self, copied: bool) -> &'static str {
        match copied {
            false => self.pick("Copier", "Copy"),
            true => self.pick("Copié", "Copied"),
        }
    }

    pub fn states_note(self) -> &'static str {
        self.pick(
            "États\u{a0}: working, waiting, done, failed, idle. La fenêtre de l'agent est trouvée d'elle-même.",
            "States: working, waiting, done, failed, idle. The agent's window is found by itself.",
        )
    }
}

// --- The group's layout, at ×1 (the mockup's) -----------------------------------

/// The group's width, inside the bar's padding.
pub const WIDTH: f32 = 172.0;
/// The header, the secretary (4.6, variant B): a row as wide as the
/// agents' (6 px past the group's sides), 2 px above and under her 40 px
/// head, radius 9; 8 px after her head, "AGENTS" 11.5 px 700 `text.dim`
/// and the total, over her line (12 px), 1 px apart.
pub const HEADER: f32 = 44.0;
pub const HEAD: f32 = 40.0;
pub const HEAD_TOP: f32 = 2.0;
pub const HEAD_GAP: f32 = 8.0;
pub const HEADER_RADIUS: f32 = 9.0;
pub const LABEL: f32 = 11.5;
pub const LINK: f32 = 12.0;
/// Her line, 1 px under "AGENTS".
pub const SAYS: f32 = 12.0;
const SAYS_GAP: f32 = 1.0;
/// Under the header.
const GAP: f32 = 6.0;
/// The card: 10 px in, radius 10, its parts 6 px apart.
const CARD_PAD: f32 = 10.0;
pub const CARD_RADIUS: f32 = 10.0;
const CARD_GAP: f32 = 6.0;
pub const INTRO: f32 = 13.0;
pub const INTRO_LINE: f32 = 1.4;
/// A tool's row: 28 px; its 18 px mark, 8 px before its name (14 px); its
/// button 24 px high, 9 px in, radius 12, 12.5 px 700.
pub const ROW: f32 = 28.0;
pub const MARK: f32 = 18.0;
pub const MARK_RADIUS: f32 = 5.0;
pub const MARK_LETTER: f32 = 11.0;
pub const MARK_GAP: f32 = 8.0;
pub const NAME: f32 = 14.0;
pub const BUTTON: f32 = 24.0;
pub const BUTTON_PAD: f32 = 9.0;
pub const BUTTON_TEXT: f32 = 12.5;
/// The card's last line, 2 px more above.
const FOOT_PAD: f32 = 2.0;
/// "Aucun agent en cours." 13.5 px, 4 px above "Suivis : …" 12 px.
pub const IDLE: f32 = 13.5;
const IDLE_GAP: f32 = 4.0;
pub const FOLLOWED: f32 = 12.0;
/// The total, 3 px after "AGENTS" (spaced 0.08 em), Manrope 600.
const TOTAL_GAP: f32 = 3.0;
const LABEL_SPACING: f32 = 0.08 * LABEL;
/// An agent's row (4.1, at real size since #497: the mockups were shown
/// at ×1.5): 34 px, as wide as the column (6 px past the group's sides),
/// radius 7, 6 px in; its panda 30 px, 7 px before its name (Manrope 600,
/// 13.5 px), which ends 7 px before the word on its right (12.5 px).
pub const AGENT_ROW: f32 = 34.0;
pub const ROW_OUT: f32 = 6.0;
pub const ROW_RADIUS: f32 = 7.0;
pub const ROW_PAD: f32 = 6.0;
pub const PANDA: f32 = 30.0;
pub const PANDA_GAP: f32 = 7.0;
pub const AGENT_NAME: f32 = 13.5;
pub const AGENT_WORD: f32 = 12.5;
/// The line for the rest, 2 px under the rows (12.5 px, `text.faint`); the
/// group 20 px taller with it.
const MORE_TOP: f32 = 2.0;
const MORE: f32 = 20.0;
pub const MORE_TEXT: f32 = 12.5;

/// The group, laid out from its top left: what is where.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub shows: Shows,
    /// Its height; 0 when hidden.
    pub height: f32,
    /// The secretary's row, at its head; her mood, and her line cut to
    /// its room. "AGENTS"'s middle, and her line's.
    pub secretary: Rectangle,
    pub mood: Mood,
    pub line: String,
    pub label_middle: f32,
    pub line_middle: f32,
    pub card: Option<Card>,
    /// The idle lines' middles, and the second's text.
    pub idle: Option<(f32, f32, String)>,
    /// The total in the header, and where it starts.
    pub total: Option<(String, f32)>,
    /// The agents' rows (4.1), from the top.
    pub rows: Vec<Row>,
    /// The line for the rest, and its middle.
    pub more: Option<(String, f32)>,
    /// The mini form's item (4.7): none when the group is hidden.
    pub mini: Option<Mini>,
}

/// An agent's row, laid out: its name cut to its room, the word on its
/// right, where it is in the group (6 px past its sides).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: u64,
    pub look: Look,
    pub name: String,
    pub word: String,
    pub rect: Rectangle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub rect: Rectangle,
    /// The intro's lines, and the first's middle.
    pub intro: Vec<String>,
    pub intro_top: f32,
    /// Each tool's row: its top and height, its name's lines, its button.
    pub rows: Vec<CardRow>,
    pub another: Rectangle,
    pub decline: Rectangle,
}

/// A tool's row in the card: 28 px, taller when its name takes two lines
/// beside its button (as a browser lays the mockup out: "Claude Code"
/// and "Brancher" do not fit side by side in its 152 px).
#[derive(Debug, Clone, PartialEq)]
pub struct CardRow {
    pub tool: Tool,
    pub top: f32,
    pub height: f32,
    pub name: Vec<String>,
    pub button: Rectangle,
}

/// The group for `shows`, `measure` giving a text's width at a size in a
/// font.
pub fn layout(
    words: &Words,
    shows: &Shows,
    brief: &Brief,
    mut measure: impl FnMut(&str, f32, Font) -> f32,
) -> Layout {
    let link_height = LINK * LINE;
    let secretary = Rectangle::new(
        Point::new(-ROW_OUT, 0.0),
        Size::new(WIDTH + 2.0 * ROW_OUT, HEADER),
    );
    let column = LABEL * LINE + SAYS_GAP + SAYS * LINE;
    let label_middle = (HEADER - column) / 2.0 + LABEL * LINE / 2.0;
    let line_middle = label_middle + LABEL * LINE / 2.0 + SAYS_GAP + SAYS * LINE / 2.0;
    let line = crate::popover::kit::elide(&words.line(brief), WIDTH - HEAD - HEAD_GAP, |t| {
        measure(t, SAYS, MANROPE)
    });
    let top = HEADER + GAP;
    let mut out = Layout {
        shows: shows.clone(),
        height: 0.0,
        secretary,
        mood: brief.mood(),
        line,
        label_middle,
        line_middle,
        card: None,
        idle: None,
        total: None,
        rows: Vec::new(),
        more: None,
        mini: match shows {
            Shows::Hidden => None,
            Shows::Card(_) => Some(Mini::card(&mut measure)),
            Shows::Idle(_) | Shows::Lines(_) => Some(Mini::of(brief, &mut measure)),
        },
    };
    match shows {
        Shows::Hidden => {}
        Shows::Lines(lines) => {
            let agents: f32 = words
                .header()
                .chars()
                .map(|ch| measure(&ch.to_string(), LABEL, MANROPE_BOLD) + LABEL_SPACING)
                .sum();
            out.total = Some((
                lines.total.to_string(),
                HEAD + HEAD_GAP + agents + TOTAL_GAP,
            ));
            let width = WIDTH + 2.0 * ROW_OUT;
            out.rows = lines
                .rows
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let word = words.word(line);
                    let room = width
                        - 2.0 * ROW_PAD
                        - PANDA
                        - 2.0 * PANDA_GAP
                        - measure(&word, AGENT_WORD, MANROPE);
                    Row {
                        id: line.id,
                        look: line.look,
                        name: crate::popover::kit::elide(&line.name, room, |t| {
                            measure(t, AGENT_NAME, MANROPE_SEMIBOLD)
                        }),
                        word,
                        rect: Rectangle::new(
                            Point::new(-ROW_OUT, top + i as f32 * AGENT_ROW),
                            Size::new(width, AGENT_ROW),
                        ),
                    }
                })
                .collect();
            let end = top + lines.rows.len() as f32 * AGENT_ROW;
            out.height = end;
            if lines.hidden > 0 {
                let middle = end + MORE_TOP + MORE_TEXT * LINE / 2.0;
                out.more = Some((words.more(lines.hidden, lines.hidden_rest), middle));
                out.height = end + MORE;
            }
        }
        Shows::Idle(names) => {
            let first = top + IDLE * LINE / 2.0;
            let second = top + IDLE * LINE + IDLE_GAP + FOLLOWED * LINE / 2.0;
            out.idle = Some((first, second, words.followed(names)));
            out.height = top + IDLE * LINE + IDLE_GAP + FOLLOWED * LINE;
        }
        Shows::Card(tools) => {
            let inner = WIDTH - 2.0 * CARD_PAD;
            let intro =
                crate::popover::kit::wrap(words.intro(), inner, |t| measure(t, INTRO, MANROPE));
            let intro_top = top + CARD_PAD;
            let mut y = intro_top + intro.len() as f32 * INTRO * INTRO_LINE + CARD_GAP;
            let button_width = measure(words.plug(), BUTTON_TEXT, MANROPE_BOLD) + 2.0 * BUTTON_PAD;
            let name_room = inner - MARK - MARK_GAP - MARK_GAP - button_width;
            let rows = tools
                .iter()
                .map(|tool| {
                    let name = crate::popover::kit::wrap(tool.title(), name_room, |t| {
                        measure(t, NAME, MANROPE)
                    });
                    let height = ROW.max(name.len() as f32 * NAME * LINE);
                    let row = CardRow {
                        tool: *tool,
                        top: y,
                        height,
                        name,
                        button: Rectangle::new(
                            Point::new(
                                WIDTH - CARD_PAD - button_width,
                                y + (height - BUTTON) / 2.0,
                            ),
                            Size::new(button_width, BUTTON),
                        ),
                    };
                    y += height + CARD_GAP;
                    row
                })
                .collect();
            // Its two links on its last line, or one under the other when
            // they do not fit side by side.
            let foot = y + FOOT_PAD;
            let another_width = measure(words.another(), LINK, MANROPE);
            let decline_width = measure(words.decline(), LINK, MANROPE);
            let apart = another_width + MARK_GAP + decline_width <= inner;
            let another = Rectangle::new(
                Point::new(CARD_PAD, foot),
                Size::new(another_width, link_height),
            );
            let foot = if apart { foot } else { foot + link_height };
            let decline = Rectangle::new(
                Point::new(WIDTH - CARD_PAD - decline_width, foot),
                Size::new(decline_width, link_height),
            );
            let bottom = foot + link_height + CARD_PAD;
            out.card = Some(Card {
                rect: Rectangle::new(Point::new(0.0, top), Size::new(WIDTH, bottom - top)),
                intro,
                intro_top,
                rows,
                another,
                decline,
            });
            out.height = bottom;
        }
    }
    out
}

// --- The mini form's item (4.7, variant A: the secretary), at ×1 ---------------

/// Its top under the bar's: 12 px of padding, the clock's two lines of
/// 19 px at 1.1 (41.8 px), and the 14 px between the mini form's items.
pub const MINI_TOP: f32 = 68.0;
/// A button 48 px wide, radius 10, 4 px in above and 6 px under: her 40 px
/// head, 4 px over how many work.
pub const MINI_ITEM: f32 = 48.0;
pub const MINI_RADIUS: f32 = 10.0;
pub const MINI_PAD_TOP: f32 = 4.0;
const MINI_PAD_BOTTOM: f32 = 6.0;
const MINI_GAP: f32 = 4.0;
/// "3" over "au travail": Manrope 600, 11 px, line height 1.25.
pub const MINI_TEXT: f32 = 11.0;
const MINI_LINE: f32 = 1.25;
/// The badge, at her head's bottom right (6 px past its right, 3 px under
/// its bottom): 16 px high and at least as wide, 4 px in, radius 8; its
/// number Manrope 700, 10 px; a 2 px ring in the bar's colour.
pub const BADGE: f32 = 16.0;
const BADGE_OUT: (f32, f32) = (6.0, 3.0);
const BADGE_PAD: f32 = 4.0;
pub const BADGE_TEXT: f32 = 10.0;
/// Its colours: amber when one waits, else orange when one is stuck, else
/// green (to see, and the card's "+").
const BADGE_WAITING: Color = rgb(0xF5B44A);
const BADGE_STUCK: Color = rgb(0xF2945B);
const BADGE_GREEN: Color = rgb(0x7EE0A0);

/// The badge's colour for `brief`, and its name for the log.
fn badge_colour(brief: &Brief) -> (Color, &'static str) {
    match brief.mood() {
        Mood::Waiting => (BADGE_WAITING, "amber"),
        Mood::Stuck => (BADGE_STUCK, "orange"),
        Mood::Calm => (BADGE_GREEN, "green"),
    }
}

/// The mini form's item, laid out from its top left (its button's, 48 px
/// wide).
#[derive(Debug, Clone, PartialEq)]
pub struct Mini {
    pub height: f32,
    /// Her head.
    pub head: Rectangle,
    pub badge: Option<Badge>,
    /// How many work, and its two lines' middles; none on the card, where
    /// she is alone.
    pub working: Option<(String, f32, f32)>,
}

/// The mini item's badge: what it says, its colour, where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Badge {
    pub text: String,
    pub colour: Color,
    pub rect: Rectangle,
}

impl Badge {
    fn new(
        text: String,
        colour: Color,
        head: Rectangle,
        measure: &mut impl FnMut(&str, f32, Font) -> f32,
    ) -> Self {
        let width = (measure(&text, BADGE_TEXT, MANROPE_BOLD) + 2.0 * BADGE_PAD).max(BADGE);
        let right = head.x + head.width + BADGE_OUT.0;
        let bottom = head.y + head.height + BADGE_OUT.1;
        Self {
            text,
            colour,
            rect: Rectangle::new(
                Point::new(right - width, bottom - BADGE),
                Size::new(width, BADGE),
            ),
        }
    }
}

impl Mini {
    fn head() -> Rectangle {
        Rectangle::new(
            Point::new((MINI_ITEM - HEAD) / 2.0, MINI_PAD_TOP),
            Size::new(HEAD, HEAD),
        )
    }

    /// Agents followed: her head, the badge with how many need the user
    /// (none at 0), and how many work.
    fn of(brief: &Brief, measure: &mut impl FnMut(&str, f32, Font) -> f32) -> Self {
        let head = Self::head();
        let needs = brief.needs();
        let (colour, _) = badge_colour(brief);
        let line = MINI_TEXT * MINI_LINE;
        let first = head.y + head.height + MINI_GAP;
        Self {
            height: first + 2.0 * line + MINI_PAD_BOTTOM,
            head,
            badge: (needs > 0).then(|| Badge::new(needs.to_string(), colour, head, measure)),
            working: Some((
                brief.working.len().to_string(),
                first + line / 2.0,
                first + line * 1.5,
            )),
        }
    }

    /// Tools to plug in (the card's place): she is alone, with a "+".
    fn card(measure: &mut impl FnMut(&str, f32, Font) -> f32) -> Self {
        let head = Self::head();
        Self {
            height: head.y + head.height + MINI_PAD_BOTTOM,
            head,
            badge: Some(Badge::new("+".to_owned(), BADGE_GREEN, head, measure)),
            working: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(tool: &str, state: ToolState) -> AgentTool {
        AgentTool {
            tool: tool.into(),
            state,
            file: String::new(),
        }
    }

    fn agent() -> AgentShown {
        shown(1, "working", 0, "build")
    }

    fn shown(id: u64, state: &str, since: u64, name: &str) -> AgentShown {
        AgentShown {
            id,
            state: state.into(),
            since,
            source: "command".into(),
            window: 0,
            name: name.into(),
            note: None,
        }
    }

    #[test]
    fn the_group_proposes_follows_or_hides() {
        let said = |tools: Vec<AgentTool>, agents: Vec<AgentShown>| Said { tools, agents }.shows(0);
        // Both found: the card for both.
        assert_eq!(
            said(
                vec![
                    tool("claude-code", ToolState::Found),
                    tool("codex", ToolState::Found)
                ],
                vec![]
            ),
            Shows::Card(vec![Tool::ClaudeCode, Tool::Codex])
        );
        // One plugged, the other declined, herdr running: who is followed.
        assert_eq!(
            said(
                vec![
                    tool("claude-code", ToolState::Plugged),
                    tool("codex", ToolState::Declined),
                    tool("herdr", ToolState::Running),
                ],
                vec![]
            ),
            Shows::Idle(vec!["Claude Code", "herdr"])
        );
        // Declined, herdr's following too: no group (variant B).
        assert_eq!(
            said(
                vec![
                    tool("claude-code", ToolState::Declined),
                    tool("herdr", ToolState::Ignored),
                ],
                vec![]
            ),
            Shows::Hidden
        );
        assert_eq!(said(vec![], vec![]), Shows::Hidden);
        // Agents run: their rows.
        assert!(matches!(
            said(vec![tool("claude-code", ToolState::Plugged)], vec![agent()]),
            Shows::Lines(lines) if lines.total == 1
        ));
    }

    #[test]
    fn the_card_is_as_tall_as_the_mockups() {
        // A narrow font: each row and the links on one line.
        let measure = |t: &str, size: f32, _font: Font| t.chars().count() as f32 * size * 0.35;
        let words = Words::new(Lang::French);
        let two = layout(
            &words,
            &Shows::Card(vec![Tool::ClaudeCode, Tool::Codex]),
            &Brief::default(),
            measure,
        );
        let card = two.card.expect("a card");
        // The intro on two lines in its 152 px.
        assert_eq!(card.intro.len(), 2);
        // 10 + 2 × 18.2 + 6 + 2 × (28 + 6) + 2 + 16.4 + 10 under the header.
        let expected = 10.0 + 2.0 * 13.0 * 1.4 + 6.0 + 2.0 * 34.0 + 2.0 + 12.0 * LINE + 10.0;
        assert!(
            (card.rect.height - expected).abs() < 0.01,
            "{}",
            card.rect.height
        );
        // Under the secretary's 44 px and 6 px.
        assert_eq!(card.rect.y, 50.0);
        assert!((two.height - (50.0 + expected)).abs() < 0.01);
        // The buttons on the card's right, 10 px in; the links on its last line.
        let row = &card.rows[1];
        assert_eq!(row.button.x + row.button.width, WIDTH - 10.0);
        assert_eq!(row.button.y, row.top + 2.0);
        assert_eq!(card.decline.x + card.decline.width, WIDTH - 10.0);
        assert_eq!(card.another.x, 10.0);
        // Manrope's widths: "Claude Code" on two lines beside its button,
        // the links one under the other, as a browser lays the mockup out.
        let wide = |t: &str, size: f32, _font: Font| t.chars().count() as f32 * size * 0.55;
        let card = layout(
            &words,
            &Shows::Card(vec![Tool::ClaudeCode]),
            &Brief::default(),
            wide,
        )
        .card
        .expect("a card");
        assert_eq!(card.rows[0].name, ["Claude", "Code"]);
        assert!((card.rows[0].height - 2.0 * 14.0 * LINE).abs() < 0.01);
        assert!((card.decline.y - card.another.y - 12.0 * LINE).abs() < 0.01);
        let idle = layout(
            &words,
            &Shows::Idle(vec!["Claude Code"]),
            &Brief::default(),
            measure,
        );
        assert_eq!(
            idle.idle.map(|(_, _, t)| t),
            Some("Suivis\u{a0}: Claude Code.".into())
        );
        assert_eq!(
            layout(&words, &Shows::Hidden, &Brief::default(), measure).height,
            0.0
        );
    }

    #[test]
    fn the_rows_are_at_real_size() {
        // #497: 34 px rows, the "+ N" line 20 px, the width unchanged;
        // under the secretary (4.6), 44 px and 6 px.
        let measure = |t: &str, size: f32, _font: Font| t.chars().count() as f32 * size * 0.55;
        let words = Words::new(Lang::French);
        let agents: Vec<AgentShown> = (1..=7)
            .map(|i| shown(i, "working", 0, "refacto-du-panneau-des-agents"))
            .collect();
        let laid = layout(
            &words,
            &Shows::Lines(Lines::of(&agents, 0)),
            &Brief::of(&[shown(9, "waiting", 0, "refacto-du-panneau-des-agents")]),
            measure,
        );
        assert_eq!(laid.rows.len(), 5);
        for (i, row) in laid.rows.iter().enumerate() {
            assert_eq!(row.rect.y, 50.0 + i as f32 * 34.0);
            assert_eq!(row.rect.height, 34.0);
            assert_eq!(row.rect.width, WIDTH + 12.0);
        }
        assert_eq!(laid.height, 50.0 + 5.0 * 34.0 + 20.0);
        // Her line cut to what her head leaves.
        assert_eq!(
            laid.secretary,
            Rectangle::new(Point::new(-6.0, 0.0), Size::new(184.0, 44.0))
        );
        assert!(laid.line.ends_with('…'), "{}", laid.line);
        assert!(measure(&laid.line, 12.0, MANROPE) <= WIDTH - 48.0 + 0.01);
        // The name at 13.5 px gives way to the word, at 12.5 px, and the
        // 30 px panda.
        let row = &laid.rows[0];
        let room = WIDTH + 12.0 - 12.0 - 30.0 - 14.0 - measure(&row.word, 12.5, MANROPE);
        assert!(
            row.name.starts_with("refacto-") && row.name.ends_with('…'),
            "{}",
            row.name
        );
        assert!(measure(&row.name, 13.5, MANROPE_SEMIBOLD) <= room + 0.01);
    }

    #[test]
    fn the_rows_put_what_concerns_the_user_first() {
        const MIN: u64 = 60_000_000;
        let agents = vec![
            shown(1, "working", 100 * MIN - 12 * MIN, "hocus-panda"),
            shown(2, "idle", 0, "notes"),
            shown(3, "waiting", 99 * MIN, "infra"),
            shown(4, "seen", 0, "site-web"),
            shown(5, "done", 0, "api"),
            shown(6, "failed", 0, "migration"),
            // Waiting longer: above infra.
            shown(7, "waiting", 90 * MIN, "docs"),
        ];
        let lines = Lines::of(&agents, 100 * MIN);
        let rows: Vec<(u64, Look)> = lines.rows.iter().map(|l| (l.id, l.look)).collect();
        assert_eq!(
            rows,
            [
                (7, Look::Waiting),
                (3, Look::Waiting),
                (6, Look::Stuck),
                (5, Look::ToSee),
                (1, Look::Working),
            ]
        );
        assert_eq!(lines.rows[4].minutes, 12);
        // The two resting ones, the lowest, are not shown.
        assert_eq!((lines.total, lines.hidden, lines.hidden_rest), (7, 2, true));
    }

    #[test]
    fn the_secretary_says_who_waits_else_who_is_stuck_else_how_many_work() {
        const MIN: u64 = 60_000_000;
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        let mut agents = vec![
            shown(1, "working", 0, "build"),
            shown(2, "working", 0, "docs"),
            shown(3, "done", 0, "api"),
            shown(4, "idle", 0, "notes"),
        ];
        let calm = Brief::of(&agents);
        assert_eq!(calm.mood(), Mood::Calm);
        assert_eq!(fr.line(&calm), "2 au travail · 1 à voir");
        assert_eq!(en.line(&calm), "2 working · 1 to see");
        assert_eq!(fr.footer(&calm), "2 au travail · 1 au repos");
        // Nothing to see: how many work alone.
        assert_eq!(fr.line(&Brief::of(&agents[..2])), "2 au travail");
        // Stuck: orange, the stuck ones' names only.
        agents.push(shown(5, "failed", 0, "migration"));
        let stuck = Brief::of(&agents);
        assert_eq!(stuck.mood(), Mood::Stuck);
        assert_eq!(fr.line(&stuck), "migration");
        agents.push(shown(6, "failed", 0, "infra"));
        assert_eq!(en.line(&Brief::of(&agents)), "migration, infra");
        // Someone waiting comes first, amber, the waiting ones' names
        // only, the longest waiting first.
        agents.push(shown(7, "waiting", 9 * MIN, "site-web"));
        let waiting = Brief::of(&agents);
        assert_eq!(waiting.mood(), Mood::Waiting);
        assert_eq!(fr.line(&waiting), "site-web");
        agents.push(shown(8, "waiting", 2 * MIN, "infra-2"));
        let two = Brief::of(&agents);
        assert_eq!(fr.line(&two), "infra-2, site-web");
        assert_eq!(en.line(&two), "infra-2, site-web");
        assert_eq!(two.stuck.len(), 2);
        assert_eq!(two.to_see.len(), 1);
    }

    #[test]
    fn the_briefing_says_since_when_and_what_is_asked() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.since(0, false), "· à l'instant");
        assert_eq!(fr.since(2, false), "· 2\u{a0}min");
        assert_eq!(fr.since(4, true), "· il y a 4\u{a0}min");
        assert_eq!(en.since(65, true), "· 1\u{a0}h\u{a0}05 ago");
        let asks = Note::allow("Bash", "npm install");
        assert_eq!(fr.note(&asks), "Autoriser Bash\u{a0}: npm install\u{a0}?");
        assert_eq!(en.note(&asks), "Allow Bash: npm install?");
        assert_eq!(
            fr.note(&Note::allow("apply_patch", "")),
            "Autoriser apply_patch\u{a0}?"
        );
    }

    #[test]
    fn the_rest_is_said_resting_only_when_it_all_rests() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.more(3, true), "+\u{a0}3 au repos");
        assert_eq!(fr.more(2, false), "+\u{a0}2 autres");
        assert_eq!(en.more(3, true), "+\u{a0}3 resting");
        assert_eq!(en.more(2, false), "+\u{a0}2 more");
        // Six working, one resting: what is hidden does not all rest.
        let mut agents: Vec<AgentShown> = (1..=6).map(|i| shown(i, "working", 0, "a")).collect();
        agents.push(shown(7, "idle", 0, "b"));
        let lines = Lines::of(&agents, 0);
        assert_eq!((lines.hidden, lines.hidden_rest), (2, false));
        let working = |minutes| Line {
            id: 1,
            look: Look::Working,
            name: String::new(),
            minutes,
        };
        assert_eq!(fr.word(&working(12)), "12\u{a0}min");
        assert_eq!(fr.word(&working(65)), "1\u{a0}h\u{a0}05");
    }

    #[test]
    fn the_mini_item_badges_who_needs_the_user_in_the_first_concerns_colour() {
        let measure = |t: &str, size: f32, _font: Font| t.chars().count() as f32 * size * 0.6;
        let words = Words::new(Lang::French);
        let mini = |agents: &[AgentShown]| {
            layout(
                &words,
                &Shows::Lines(Lines::of(agents, 0)),
                &Brief::of(agents),
                measure,
            )
            .mini
            .expect("an item")
        };
        let badge = |agents: &[AgentShown]| mini(agents).badge.map(|b| (b.text, b.colour));
        let mut agents = vec![shown(1, "working", 0, "a"), shown(2, "idle", 0, "b")];
        // Nobody needs the user: no badge; one works.
        let calm = mini(&agents);
        assert_eq!(calm.badge, None);
        assert_eq!(calm.working.as_ref().map(|w| w.0.as_str()), Some("1"));
        // 4 px, her 40 px head, 4 px, two lines of 11 px at 1.25, 6 px.
        assert!((calm.height - (4.0 + 40.0 + 4.0 + 2.0 * 13.75 + 6.0)).abs() < 0.01);
        assert_eq!(
            calm.head,
            Rectangle::new(Point::new(4.0, 4.0), Size::new(40.0, 40.0))
        );
        // To see: green; stuck: orange; waiting: amber, counting all three.
        agents.push(shown(3, "done", 0, "c"));
        assert_eq!(badge(&agents), Some(("1".into(), BADGE_GREEN)));
        agents.push(shown(4, "failed", 0, "d"));
        assert_eq!(badge(&agents), Some(("2".into(), BADGE_STUCK)));
        agents.push(shown(5, "waiting", 0, "e"));
        let three = mini(&agents);
        let b = three.badge.expect("a badge");
        assert_eq!((b.text.as_str(), b.colour), ("3", BADGE_WAITING));
        // 16 px, its right 6 px past her head's, its bottom 3 px under it.
        assert_eq!(
            b.rect,
            Rectangle::new(Point::new(34.0, 31.0), Size::new(16.0, 16.0))
        );
        // Wider with two figures, toward her head.
        agents.extend((6..=15).map(|i| shown(i, "waiting", 0, "f")));
        let wide = mini(&agents).badge.expect("a badge").rect;
        assert_eq!((wide.x + wide.width, wide.width), (50.0, 2.0 * 6.0 + 8.0));
        // The card: she is alone, with a green "+".
        let card = layout(
            &words,
            &Shows::Card(vec![Tool::Codex]),
            &Brief::default(),
            measure,
        )
        .mini
        .expect("an item");
        assert_eq!(card.working, None);
        assert_eq!(
            card.badge.map(|b| (b.text, b.colour)),
            Some(("+".into(), BADGE_GREEN))
        );
        assert_eq!(card.height, 50.0);
        // Hidden: no item.
        assert_eq!(
            layout(&words, &Shows::Hidden, &Brief::default(), measure).mini,
            None
        );
    }
}
