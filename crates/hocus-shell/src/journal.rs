//! The agents' journal in the column (#176): once an MCP agent connected,
//! the last few things agents did (agent · action · window · time), the
//! whole of what is kept in a popover, and the button that stops every
//! agent at once, or allows them again.

use hocus_ui::{
    Align, Button, ButtonKind, Direction, Insets, Justify, Kind, Length, Look, NodeId, Paint,
    Style, TextStyle, Theme, Tree,
};

use crate::i18n::Lang;

/// Entries shown in the column; the popover shows them all.
pub const ROWS: usize = 3;

/// Which button a click pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    Left,
    Right,
    Middle,
}

/// What an agent did, as the column says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Did {
    Click {
        button: Pressed,
        count: u32,
    },
    Scroll {
        down: bool,
    },
    /// The text, when the configuration keeps it.
    Type {
        characters: usize,
        text: Option<String>,
    },
    /// The combination, when it is kept.
    Keys(Option<String>),
    /// It named itself (kept when refused).
    Connect,
    /// The user handed it a question and an AI's answer (#197); the text
    /// when the configuration keeps it.
    Prompt {
        characters: usize,
        text: Option<String>,
    },
    /// It added a task (#255); the text when the configuration keeps it.
    AddTask {
        characters: usize,
        text: Option<String>,
    },
}

/// One entry, newest first in [`JournalView::lines`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalLine {
    pub agent: String,
    pub did: Did,
    /// The window's title (or app id), or the shell element's label;
    /// empty for a connection.
    pub target: String,
    /// The time of day, as the clock shows it.
    pub time: String,
    /// Why it was refused, if it was.
    pub refused: Option<String>,
}

/// An MCP agent connected now, with a seat of its own (#43).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeatLine {
    pub agent: String,
    /// The window it acts in, by its title (or app id), when it has one:
    /// the compositor shows it live in the slot.
    pub window: Option<String>,
    /// The user took over from it: its actions are refused until they
    /// hand back.
    pub taken: bool,
}

/// The column's journal section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JournalView {
    /// Every agent is stopped: their requests are refused.
    pub stopped: bool,
    /// The agents connected now, each with its window.
    pub seats: Vec<SeatLine>,
    /// The latest entries, newest first, as many as the popover shows.
    pub lines: Vec<JournalLine>,
    /// Entries kept in all.
    pub total: usize,
}

impl Lang {
    pub fn journal_caption(self) -> &'static str {
        self.pick("AGENTS’ JOURNAL", "JOURNAL DES AGENTS")
    }

    /// What opens the popover, for screen readers and tests.
    pub fn journal(self) -> &'static str {
        self.pick("Agents' journal", "Journal des agents")
    }

    pub fn journal_empty(self) -> &'static str {
        self.pick("No action yet", "Aucune action pour l’instant")
    }

    pub fn agents_stopped(self) -> &'static str {
        self.pick(
            "Stopped: their requests are refused",
            "Arrêtés : leurs demandes sont refusées",
        )
    }

    /// The button's words, and its name: stop, or allow again.
    pub fn stop_agents(self, stopped: bool) -> (&'static str, &'static str) {
        match stopped {
            false => (
                self.pick("Stop", "Arrêter"),
                self.pick("Stop every agent", "Arrêter tous les agents"),
            ),
            true => (
                self.pick("Allow", "Autoriser"),
                self.pick("Allow agents again", "Autoriser à nouveau les agents"),
            ),
        }
    }

    /// The toast when the key or the button stops or allows them.
    pub fn agents_toast(self, stopped: bool) -> (&'static str, &'static str) {
        match stopped {
            true => (
                self.pick("Agents stopped", "Agents arrêtés"),
                self.pick(
                    "Their requests are refused until you allow them again.",
                    "Leurs demandes sont refusées jusqu’à ce que vous les autorisiez à nouveau.",
                ),
            ),
            false => (
                self.pick("Agents allowed again", "Agents de nouveau autorisés"),
                "",
            ),
        }
    }

    /// The slot an agent's window shows in, for screen readers and tests.
    pub fn window_of(self, agent: &str) -> String {
        match self {
            Self::En => format!("Window of {agent}"),
            Self::Fr => format!("Fenêtre de {agent}"),
        }
    }

    /// Where an agent is, in the column.
    pub fn seat_line(self, seat: &SeatLine) -> String {
        match (self, &seat.window, seat.taken) {
            (Self::En, _, true) => format!("{} · you took over", seat.agent),
            (Self::Fr, _, true) => format!("{} · vous avez la main", seat.agent),
            (Self::En, Some(w), false) => format!("{} · in {w}", seat.agent),
            (Self::Fr, Some(w), false) => format!("{} · dans {w}", seat.agent),
            (Self::En, None, false) => format!("{} · connected", seat.agent),
            (Self::Fr, None, false) => format!("{} · connecté", seat.agent),
        }
    }

    /// The popover's subtitle: how many it shows of how many.
    pub fn journal_count(self, shown: usize, total: usize) -> String {
        match (self, shown < total, total) {
            (Self::En, true, _) => format!("The last {shown} of {total}"),
            (Self::Fr, true, _) => format!("Les {shown} dernières sur {total}"),
            (Self::En, false, 1) => "1 action this session".into(),
            (Self::En, false, n) => format!("{n} actions this session"),
            (Self::Fr, false, 1) => "1 action cette session".into(),
            (Self::Fr, false, n) => format!("{n} actions cette session"),
        }
    }

    /// What an agent did, or tried to.
    pub fn did(self, did: &Did, refused: bool) -> String {
        let fr = self == Self::Fr;
        if refused {
            return match did {
                Did::Click { .. } => self.pick("Click refused", "Clic refusé"),
                Did::Scroll { .. } => self.pick("Scroll refused", "Défilement refusé"),
                Did::Type { .. } => self.pick("Typing refused", "Saisie refusée"),
                Did::Keys(_) => self.pick("Keys refused", "Touches refusées"),
                Did::Connect => self.pick("Connection refused", "Connexion refusée"),
                Did::Prompt { .. } => self.pick("Question refused", "Question refusée"),
                Did::AddTask { .. } => self.pick("Task refused", "Tâche refusée"),
            }
            .into();
        }
        match did {
            Did::Click { button, count } => match (fr, button, *count) {
                (false, Pressed::Left, 0 | 1) => "Clicked".into(),
                (false, Pressed::Left, 2) => "Double-clicked".into(),
                (false, Pressed::Left, _) => "Triple-clicked".into(),
                (false, Pressed::Right, _) => "Right-clicked".into(),
                (false, Pressed::Middle, _) => "Middle-clicked".into(),
                (true, Pressed::Left, 0 | 1) => "A cliqué".into(),
                (true, Pressed::Left, 2) => "A double-cliqué".into(),
                (true, Pressed::Left, _) => "A triple-cliqué".into(),
                (true, Pressed::Right, _) => "A fait un clic droit".into(),
                (true, Pressed::Middle, _) => "A fait un clic milieu".into(),
            },
            Did::Scroll { down } => match (fr, down) {
                (false, true) => "Scrolled down".into(),
                (false, false) => "Scrolled up".into(),
                (true, true) => "A défilé vers le bas".into(),
                (true, false) => "A défilé vers le haut".into(),
            },
            Did::Type {
                text: Some(text), ..
            } => match fr {
                false => format!("Typed “{text}”"),
                true => format!("A tapé «\u{a0}{text}\u{a0}»"),
            },
            Did::Type { characters, .. } => match (fr, characters) {
                (false, 1) => "Typed 1 character".into(),
                (false, n) => format!("Typed {n} characters"),
                (true, 1) => "A tapé 1 caractère".into(),
                (true, n) => format!("A tapé {n} caractères"),
            },
            Did::Keys(Some(keys)) => match fr {
                false => format!("Pressed {keys}"),
                true => format!("A appuyé sur {keys}"),
            },
            Did::Keys(None) => self.pick("Pressed keys", "A appuyé sur des touches").into(),
            Did::Connect => self.pick("Connected", "S’est connecté").into(),
            Did::Prompt {
                text: Some(text), ..
            } => match fr {
                false => format!("Was handed “{text}”"),
                true => format!("A reçu «\u{a0}{text}\u{a0}»"),
            },
            Did::Prompt { characters, .. } => match fr {
                false => format!("Was handed a question, {characters} characters"),
                true => format!("A reçu une question, {characters} caractères"),
            },
            Did::AddTask {
                text: Some(text), ..
            } => match fr {
                false => format!("Added the task “{text}”"),
                true => format!("A ajouté la tâche «\u{a0}{text}\u{a0}»"),
            },
            Did::AddTask { .. } => self.pick("Added a task", "A ajouté une tâche").into(),
        }
    }

    /// An entry in one line: agent · what · where.
    pub fn journal_line(self, line: &JournalLine) -> String {
        let did = self.did(&line.did, line.refused.is_some());
        if line.target.is_empty() {
            format!("{} · {did}", line.agent)
        } else {
            format!("{} · {did} · {}", line.agent, line.target)
        }
    }
}

/// The section's room in the column: some, or none.
pub(crate) fn room(shown: bool) -> Style {
    Style {
        gap: 10.0,
        padding: Insets {
            bottom: if shown { 18.0 } else { 0.0 },
            ..Insets::default()
        },
        height: if shown { Length::Auto } else { Length::Px(0.0) },
        clip: true,
        ..Style::default()
    }
}

fn small(theme: &Theme) -> TextStyle {
    TextStyle {
        size: 13.0,
        weight: 500,
        line_height: 1.35,
        ..theme.text.label
    }
}

fn row(gap: f32) -> Style {
    Style {
        direction: Direction::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    }
}

/// A dot of the entry's colour: carried out, or refused.
fn dot(refused: bool) -> (Style, Kind) {
    (
        Style {
            width: Length::Px(8.0),
            height: Length::Px(8.0),
            ..Style::default()
        },
        Kind::Box(Look {
            background: Some(if refused { Paint::Error } else { Paint::Agent }),
            radius: 4.0,
            ..Look::default()
        }),
    )
}

/// What the section holds that the column answers to.
#[derive(Debug, Clone, Default)]
pub(crate) struct Filled {
    pub button: Option<NodeId>,
    /// The entries, the popover's anchor.
    pub entries: Option<NodeId>,
    /// Each agent's Take over or Hand back: the agent, and whether it is
    /// taken over.
    pub seats: Vec<(NodeId, String, bool)>,
    /// Each agent's window slot.
    pub slots: Vec<(String, NodeId)>,
}

/// The empty box an agent's window shows in, live: the compositor draws it.
struct Slot;

impl hocus_ui::tree::Draw for Slot {
    fn draw(
        &self,
        canvas: &mut hocus_ui::Canvas,
        rect: hocus_ui::Rect,
        theme: &Theme,
        _now: std::time::Duration,
    ) {
        canvas.fill_rect(rect, 10.0, theme.colors.background);
        canvas.stroke_rect(rect, 10.0, 1.0, theme.colors.border);
    }

    fn size(&self) -> hocus_ui::Size {
        hocus_ui::Size::new(0.0, SLOT)
    }
}

/// The window slot's height.
const SLOT: f32 = 120.0;

/// Fills the section: its caption and the stop button, the stopped line,
/// each agent connected with its window and Take over, then the latest
/// `rows` entries (one at least), which open the popover.
pub(crate) fn fill(
    tree: &mut Tree,
    room: NodeId,
    theme: &Theme,
    lang: Lang,
    view: &JournalView,
    rows: usize,
) -> Filled {
    let missing = || unreachable!("adding to a node of this tree");
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };
    let small = small(theme);

    let header = add(
        tree,
        room,
        Style {
            justify: Justify::SpaceBetween,
            ..row(8.0)
        },
        Kind::Box(Look::default()),
    );
    add(
        tree,
        header,
        Style::default(),
        Kind::text(lang.journal_caption(), theme.text.caption),
    );
    let (words, name) = lang.stop_agents(view.stopped);
    let kind = if view.stopped {
        ButtonKind::Primary
    } else {
        ButtonKind::Plain
    };
    let button = add(
        tree,
        header,
        Style {
            height: Length::Px(32.0),
            padding: Insets::symmetric(0.0, 12.0),
            ..Style::default()
        },
        Kind::Button(Button::new(words, kind, theme).with_name(name)),
    );

    if view.stopped {
        let line = add(
            tree,
            room,
            Style {
                padding: Insets::symmetric(8.0, 12.0),
                ..row(10.0)
            },
            Kind::Box(Look {
                background: Some(Paint::Rgba(theme.colors.waiting.with_alpha(31))),
                border: Some(Paint::Waiting),
                radius: 12.0,
                ..Look::default()
            }),
        );
        add(
            tree,
            line,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text(lang.agents_stopped(), small.with_color(Paint::Waiting)),
        );
    }

    let mut filled = Filled {
        button: Some(button),
        ..Filled::default()
    };
    for seat in &view.seats {
        let card = add(
            tree,
            room,
            Style {
                gap: 8.0,
                padding: Insets::all(10.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 12.0,
                ..Look::default()
            }),
        );
        if seat.window.is_some() {
            let slot = add(
                tree,
                card,
                Style {
                    height: Length::Px(SLOT),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Slot),
                    label: Some(lang.window_of(&seat.agent)),
                },
            );
            filled.slots.push((seat.agent.clone(), slot));
        }
        let line = add(tree, card, row(8.0), Kind::Box(Look::default()));
        add(
            tree,
            line,
            Style {
                width: Length::Px(8.0),
                height: Length::Px(8.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Rgba(crate::seat::agent_color(theme, &seat.agent))),
                radius: 4.0,
                ..Look::default()
            }),
        );
        add(
            tree,
            line,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(lang.seat_line(seat), small),
        );
        let (words, kind) = if seat.taken {
            (lang.hand_back(), ButtonKind::Primary)
        } else {
            (lang.take_over(), ButtonKind::Plain)
        };
        let button = add(
            tree,
            line,
            Style {
                height: Length::Px(32.0),
                padding: Insets::symmetric(0.0, 12.0),
                grow: 0.0,
                ..Style::default()
            },
            Kind::Button(Button::new(words, kind, theme)),
        );
        filled.seats.push((button, seat.agent.clone(), seat.taken));
    }

    let entries = add(
        tree,
        room,
        Style {
            gap: 6.0,
            padding: Insets::symmetric(4.0, 4.0),
            ..Style::default()
        },
        Kind::Box(Look {
            radius: theme.radii.small,
            ..Look::default()
        }),
    );
    tree.set_clickable(entries, lang.journal());
    if view.lines.is_empty() {
        add(
            tree,
            entries,
            Style::default(),
            Kind::text_line(
                lang.journal_empty(),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
    }
    for line in view.lines.iter().take(rows.max(1)) {
        let r = add(tree, entries, row(10.0), Kind::Box(Look::default()));
        let (style, kind) = dot(line.refused.is_some());
        add(tree, r, style, kind);
        add(
            tree,
            r,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(lang.journal_line(line), theme.text.body),
        );
        add(
            tree,
            r,
            Style::default(),
            Kind::text_line(line.time.clone(), small.with_color(Paint::Muted)),
        );
    }
    filled.entries = Some(entries);
    filled
}

/// The popover's height for `entries`: the head, then two lines each.
pub(crate) fn popover_height(entries: usize) -> f32 {
    16.0 * 2.0 + 46.0 + entries.max(1) as f32 * 48.0
}

/// Fills the popover's card: every entry the view holds, with its time
/// and, when refused, why.
pub(crate) fn fill_popover(
    tree: &mut Tree,
    card: NodeId,
    theme: &Theme,
    lang: Lang,
    view: &JournalView,
) {
    let missing = || unreachable!("adding to a node of this tree");
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };
    let small = small(theme);
    tree.set_style(
        card,
        Style {
            height: Length::Fill,
            padding: Insets::all(16.0),
            gap: 8.0,
            ..Style::default()
        },
    );
    let head = add(
        tree,
        card,
        Style {
            gap: 2.0,
            padding: Insets {
                bottom: 6.0,
                ..Insets::default()
            },
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    add(
        tree,
        head,
        Style::default(),
        Kind::text_line(lang.journal(), theme.text.title.with_weight(700)),
    );
    add(
        tree,
        head,
        Style::default(),
        Kind::text_line(
            lang.journal_count(view.lines.len(), view.total),
            small.with_color(Paint::Muted),
        ),
    );
    if view.lines.is_empty() {
        add(
            tree,
            card,
            Style::default(),
            Kind::text_line(
                lang.journal_empty(),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
    }
    for line in &view.lines {
        let entry = add(
            tree,
            card,
            Style {
                gap: 2.0,
                height: Length::Px(40.0),
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        let first = add(tree, entry, row(10.0), Kind::Box(Look::default()));
        let (style, kind) = dot(line.refused.is_some());
        add(tree, first, style, kind);
        add(
            tree,
            first,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(lang.journal_line(line), theme.text.body.with_weight(600)),
        );
        let second = add(
            tree,
            entry,
            Style {
                padding: Insets {
                    left: 18.0,
                    ..Insets::default()
                },
                ..row(8.0)
            },
            Kind::Box(Look::default()),
        );
        add(
            tree,
            second,
            Style::default(),
            Kind::text_line(line.time.clone(), small.with_color(Paint::Muted)),
        );
        if let Some(why) = &line.refused {
            add(
                tree,
                second,
                Style {
                    width: Length::Fill,
                    ..Style::default()
                },
                Kind::text_line(capitalized(why), small.with_color(Paint::Error)),
            );
        }
    }
}

/// A reason as a sentence starts.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(did: Did, refused: Option<&str>) -> JournalLine {
        JournalLine {
            agent: "helper".into(),
            did,
            target: "Shopping list".into(),
            time: "14:37".into(),
            refused: refused.map(str::to_owned),
        }
    }

    #[test]
    fn an_entry_says_who_did_what_where_in_both_languages() {
        let typed = line(
            Did::Type {
                characters: 4,
                text: None,
            },
            None,
        );
        assert_eq!(
            Lang::En.journal_line(&typed),
            "helper · Typed 4 characters · Shopping list"
        );
        assert_eq!(
            Lang::Fr.journal_line(&typed),
            "helper · A tapé 4 caractères · Shopping list"
        );
        let refused = line(
            Did::Click {
                button: Pressed::Left,
                count: 1,
            },
            Some("no"),
        );
        assert_eq!(
            Lang::En.journal_line(&refused),
            "helper · Click refused · Shopping list"
        );
        assert_eq!(Lang::Fr.did(&refused.did, true), "Clic refusé");
        let connect = JournalLine {
            target: String::new(),
            ..line(Did::Connect, Some("stopped"))
        };
        assert_eq!(
            Lang::En.journal_line(&connect),
            "helper · Connection refused"
        );
        assert_eq!(
            Lang::Fr.journal_line(&connect),
            "helper · Connexion refusée"
        );
        let keys = Did::Keys(Some("Ctrl+S".into()));
        assert_eq!(Lang::Fr.did(&keys, false), "A appuyé sur Ctrl+S");
        let kept = Did::Type {
            characters: 4,
            text: Some("Eggs".into()),
        };
        assert_eq!(Lang::En.did(&kept, false), "Typed “Eggs”");
        assert_eq!(Lang::En.journal_count(12, 40), "The last 12 of 40");
        assert_eq!(Lang::Fr.journal_count(3, 3), "3 actions cette session");
        assert_eq!(capitalized("agent \"x\" may not"), "Agent \"x\" may not");
    }
}
