//! The agents' group in the bar (bar.md §4 group 3; 4.0): its header
//! ("AGENTS", and "Sources" on its right), and under it the card that
//! proposes to plug in the tools found, or "Aucun agent en cours." with the
//! tools followed. 4.1's tiles come under the header later; until then,
//! with agents running, the group is its header alone. With no tool
//! followed and none to propose, it does not show at all (variant B).
//!
//! The compositor owns the agents and the tools: it says which tools are
//! there and what each is (`agent-tools`), and which agents run
//! (`agents`), as the shell connects and when they change. The link keeps
//! them here without a message, lays the group out (its text measured
//! once), resizes its surface if its height changed and wakes it
//! ([`draw`]); an open Sources or consent popover draws again for them.

pub mod draw;

use std::sync::{Mutex, PoisonError};

use hocus_link::{AGENTS_NAMESPACE, AgentShown, AgentTool, ToolState};
use iced::{Color, Point, Rectangle, Size};

use crate::{
    clock::time::Lang,
    levels::draw::{LINE, rgb},
    popover::{self, Item},
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

    /// What the group shows.
    pub fn shows(&self) -> Shows {
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
            (true, _, false) => Shows::Header,
        }
    }
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
    /// Agents run: the header, until 4.1's tiles.
    Header,
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
    let shows = after.shows();
    tracing::debug!(
        shows = %words.said(&shows),
        agents = after.agents.len(),
        "the agents group shows it"
    );
    draw::stale();
    popover::send(vec![popover::Effect::Wake(AGENTS_NAMESPACE)]);
    popover::changed(Item::Sources);
    for tool in Tool::ALL {
        popover::changed(Item::Plug(tool));
    }
}

/// The group laid out for what was last said, its text measured with the
/// shell's fonts (as it draws: they are loaded by then).
pub fn lay_out() -> Layout {
    let shows = said().shows();
    let words = Words::new(Lang::of_messages());
    layout(&words, &shows, |text, size, bold| {
        crate::popover::kit::measure(
            text,
            size,
            if bold {
                crate::levels::draw::MANROPE_BOLD
            } else {
                crate::levels::draw::MANROPE
            },
        )
    })
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
            Shows::Header => self.header().to_owned(),
        }
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
/// The header: "AGENTS" 11.5 px 700 `text.dim`, "Sources" 12 px `text.faint`.
pub const HEADER: f32 = 18.0;
pub const LABEL: f32 = 11.5;
pub const LINK: f32 = 12.0;
/// Under the header.
const GAP: f32 = 8.0;
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

/// The group, laid out from its top left: what is where.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub shows: Shows,
    /// Its height; 0 when hidden.
    pub height: f32,
    /// "Sources", in the header.
    pub sources: Rectangle,
    pub card: Option<Card>,
    /// The idle lines' middles, and the second's text.
    pub idle: Option<(f32, f32, String)>,
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

/// The group for `shows`, `measure` giving a text's width at a size (bold
/// or not).
pub fn layout(
    words: &Words,
    shows: &Shows,
    mut measure: impl FnMut(&str, f32, bool) -> f32,
) -> Layout {
    let sources_width = measure(words.sources(), LINK, false);
    let link_height = LINK * LINE;
    let sources = Rectangle::new(
        Point::new(WIDTH - sources_width, (HEADER - link_height) / 2.0),
        Size::new(sources_width, link_height),
    );
    let top = HEADER + GAP;
    let mut out = Layout {
        shows: shows.clone(),
        height: 0.0,
        sources,
        card: None,
        idle: None,
    };
    match shows {
        Shows::Hidden => {}
        Shows::Header => out.height = HEADER,
        Shows::Idle(names) => {
            let first = top + IDLE * LINE / 2.0;
            let second = top + IDLE * LINE + IDLE_GAP + FOLLOWED * LINE / 2.0;
            out.idle = Some((first, second, words.followed(names)));
            out.height = top + IDLE * LINE + IDLE_GAP + FOLLOWED * LINE;
        }
        Shows::Card(tools) => {
            let inner = WIDTH - 2.0 * CARD_PAD;
            let intro =
                crate::popover::kit::wrap(words.intro(), inner, |t| measure(t, INTRO, false));
            let intro_top = top + CARD_PAD;
            let mut y = intro_top + intro.len() as f32 * INTRO * INTRO_LINE + CARD_GAP;
            let button_width = measure(words.plug(), BUTTON_TEXT, true) + 2.0 * BUTTON_PAD;
            let name_room = inner - MARK - MARK_GAP - MARK_GAP - button_width;
            let rows = tools
                .iter()
                .map(|tool| {
                    let name = crate::popover::kit::wrap(tool.title(), name_room, |t| {
                        measure(t, NAME, false)
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
            let another_width = measure(words.another(), LINK, false);
            let decline_width = measure(words.decline(), LINK, false);
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
        AgentShown {
            id: 1,
            state: "working".into(),
            since: 0,
            source: "command".into(),
            window: 0,
            name: "build".into(),
        }
    }

    #[test]
    fn the_group_proposes_follows_or_hides() {
        let said = |tools: Vec<AgentTool>, agents: Vec<AgentShown>| Said { tools, agents }.shows();
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
        // Agents run: the header, until the tiles.
        assert_eq!(
            said(vec![tool("claude-code", ToolState::Plugged)], vec![agent()]),
            Shows::Header
        );
    }

    #[test]
    fn the_card_is_as_tall_as_the_mockups() {
        // A narrow font: each row and the links on one line.
        let measure = |t: &str, size: f32, _bold: bool| t.chars().count() as f32 * size * 0.35;
        let words = Words::new(Lang::French);
        let two = layout(
            &words,
            &Shows::Card(vec![Tool::ClaudeCode, Tool::Codex]),
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
        assert_eq!(card.rect.y, 26.0);
        assert!((two.height - (26.0 + expected)).abs() < 0.01);
        // The buttons on the card's right, 10 px in; the links on its last line.
        let row = &card.rows[1];
        assert_eq!(row.button.x + row.button.width, WIDTH - 10.0);
        assert_eq!(row.button.y, row.top + 2.0);
        assert_eq!(card.decline.x + card.decline.width, WIDTH - 10.0);
        assert_eq!(card.another.x, 10.0);
        // Manrope's widths: "Claude Code" on two lines beside its button,
        // the links one under the other, as a browser lays the mockup out.
        let wide = |t: &str, size: f32, _bold: bool| t.chars().count() as f32 * size * 0.55;
        let card = layout(&words, &Shows::Card(vec![Tool::ClaudeCode]), wide)
            .card
            .expect("a card");
        assert_eq!(card.rows[0].name, ["Claude", "Code"]);
        assert!((card.rows[0].height - 2.0 * 14.0 * LINE).abs() < 0.01);
        assert!((card.decline.y - card.another.y - 12.0 * LINE).abs() < 0.01);
        let idle = layout(&words, &Shows::Idle(vec!["Claude Code"]), measure);
        assert_eq!(
            idle.idle.map(|(_, _, t)| t),
            Some("Suivis\u{a0}: Claude Code.".into())
        );
        assert_eq!(layout(&words, &Shows::Hidden, measure).height, 0.0);
        assert_eq!(layout(&words, &Shows::Header, measure).height, HEADER);
    }
}
