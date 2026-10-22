//! Coding agents (#41) in the column: the active ones with their state and
//! a panda each, the others summed up; the ones waiting for an answer offer
//! it. [`Board`] follows what an agent manager (herdr) says, holding back
//! rests shorter than [`SETTLE`] so that an agent flickering between work
//! and rest does not make the column flicker.

use std::time::Duration;

use hocus_ui::{
    Align, Button, ButtonKind, Color, Direction, Insets, Justify, Kind, Length, Look, NodeId,
    Paint, Style, TextStyle, Theme, Tree,
};

use crate::{
    column::Action,
    i18n::Lang,
    panda::{Character, Mood, Panda},
};

/// A rest shorter than this is not shown.
pub const SETTLE: Duration = Duration::from_secs(2);
/// Agents shown in full; the others are summed up.
const ROWS: usize = 3;
/// A panda's side in a row, from the design.
const PANDA: f32 = 64.0;

/// What an agent is doing, as the column shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Rest,
    Work,
    /// Waits for the user's answer.
    Wait,
    Done,
    /// Failed or stuck.
    Stuck,
}

impl AgentState {
    fn mood(self) -> Mood {
        match self {
            Self::Rest => Mood::Rest,
            Self::Work => Mood::Work,
            Self::Wait => Mood::Wait,
            Self::Done => Mood::Done,
            Self::Stuck => Mood::Error,
        }
    }

    /// Its colour: the panda's magic ear, or the dot without pandas.
    fn paint(self) -> Paint {
        match self {
            Self::Wait => Paint::Waiting,
            Self::Stuck => Paint::Error,
            Self::Rest => Paint::Muted,
            Self::Work | Self::Done => Paint::Accent,
        }
    }

    /// Which come first in the column.
    fn rank(self) -> u8 {
        match self {
            Self::Wait => 0,
            Self::Stuck => 1,
            Self::Done => 2,
            Self::Work => 3,
            Self::Rest => 4,
        }
    }
}

/// What the user asks of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentAsk {
    /// Shows it in its manager, and its terminal.
    Show,
    /// Accepts what it proposes.
    Yes,
    /// Refuses it.
    No,
}

/// An agent waits for an answer, or no longer does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Waits {
        key: String,
        name: String,
        question: Option<String>,
    },
    Answered(String),
}

#[derive(Debug, Clone)]
struct Tracked {
    key: String,
    name: String,
    /// Its place among the agents seen: its panda's character.
    nth: usize,
    shown: AgentState,
    /// Since when it is in that state.
    since: Duration,
    /// When it came to that state while followed: its panda moves a
    /// moment from then. `None` for how it was found.
    moved_at: Option<Duration>,
    /// A rest not shown yet, since then.
    resting: Option<Duration>,
    question: Option<String>,
}

impl Tracked {
    /// Shows `state` from `now`.
    fn show(&mut self, state: AgentState, now: Duration, changes: &mut Vec<Change>) {
        self.resting = None;
        if state == self.shown {
            return;
        }
        if self.shown == AgentState::Wait {
            changes.push(Change::Answered(self.key.clone()));
        }
        self.shown = state;
        self.since = now;
        self.moved_at = Some(now);
        if state == AgentState::Wait {
            changes.push(Change::Waits {
                key: self.key.clone(),
                name: self.name.clone(),
                question: self.question.clone(),
            });
        }
    }
}

/// The agents an agent manager runs, as the column shows them.
#[derive(Debug, Clone, Default)]
pub struct Board {
    agents: Vec<Tracked>,
    next_nth: usize,
}

impl Board {
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Every agent, in the manager's order, with its state and its
    /// question: new ones show as they are, the others change as
    /// [`Board::set`] does; those missing are gone.
    pub fn replace(
        &mut self,
        list: Vec<(String, String, AgentState, Option<String>)>,
        now: Duration,
    ) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut old = std::mem::take(&mut self.agents);
        for (key, name, state, question) in list {
            if let Some(i) = old.iter().position(|t| t.key == key) {
                let mut t = old.swap_remove(i);
                t.name = name;
                self.agents.push(t);
                changes.extend(self.set(&key, state, question, now));
            } else {
                if state == AgentState::Wait {
                    changes.push(Change::Waits {
                        key: key.clone(),
                        name: name.clone(),
                        question: question.clone(),
                    });
                }
                self.agents.push(Tracked {
                    key,
                    name,
                    nth: self.next_nth,
                    shown: state,
                    since: now,
                    moved_at: None,
                    resting: None,
                    question,
                });
                self.next_nth += 1;
            }
        }
        changes.extend(
            old.into_iter()
                .filter(|t| t.shown == AgentState::Wait)
                .map(|t| Change::Answered(t.key)),
        );
        changes
    }

    /// The manager went away: no agents.
    pub fn clear(&mut self) -> Vec<Change> {
        self.replace(Vec::new(), Duration::ZERO)
    }

    /// An agent's state changed; a rest shows only once it lasted
    /// [`SETTLE`].
    pub fn set(
        &mut self,
        key: &str,
        state: AgentState,
        question: Option<String>,
        now: Duration,
    ) -> Vec<Change> {
        let mut changes = Vec::new();
        let Some(t) = self.agents.iter_mut().find(|t| t.key == key) else {
            return changes;
        };
        if state == AgentState::Wait {
            t.question = question;
        }
        if state == AgentState::Rest && t.shown != AgentState::Rest {
            t.resting.get_or_insert(now);
        } else {
            t.show(state, now, &mut changes);
        }
        changes
    }

    /// Rests that lasted show.
    pub fn settle(&mut self, now: Duration) -> Vec<Change> {
        let mut changes = Vec::new();
        for t in &mut self.agents {
            if t.resting.is_some_and(|since| now >= since + SETTLE) {
                t.show(AgentState::Rest, now, &mut changes);
            }
        }
        changes
    }

    /// When the next rest falls due.
    pub fn deadline(&self) -> Option<Duration> {
        self.agents
            .iter()
            .filter_map(|t| t.resting)
            .min()
            .map(|since| since + SETTLE)
    }

    /// The agent's name.
    pub fn name(&self, key: &str) -> Option<&str> {
        self.agents
            .iter()
            .find(|t| t.key == key)
            .map(|t| t.name.as_str())
    }

    /// Whether it waits for an answer.
    pub fn waits(&self, key: &str) -> bool {
        self.agents
            .iter()
            .any(|t| t.key == key && t.shown == AgentState::Wait)
    }

    /// What the column shows at `now`; nothing without agents.
    pub fn view(&self, now: Duration, pandas: bool, still: bool) -> Option<AgentsView> {
        if self.agents.is_empty() {
            return None;
        }
        let count = |s: AgentState| self.agents.iter().filter(|t| t.shown == s).count();
        let mut active: Vec<&Tracked> = self
            .agents
            .iter()
            .filter(|t| t.shown != AgentState::Rest)
            .collect();
        active.sort_by_key(|t| (t.shown.rank(), t.nth));
        let rows = active
            .iter()
            .take(ROWS)
            .map(|t| AgentRow {
                key: t.key.clone(),
                name: t.name.clone(),
                state: t.shown,
                minutes: now.saturating_sub(t.since).as_secs() / 60,
                question: t.question.clone().filter(|_| t.shown == AgentState::Wait),
                character: Character::nth(t.nth),
                moved_at: t.moved_at,
            })
            .collect();
        Some(AgentsView {
            rows,
            more: active.len().saturating_sub(ROWS),
            resting: count(AgentState::Rest),
            waiting: count(AgentState::Wait),
            working: count(AgentState::Work),
            done: count(AgentState::Done),
            stuck: count(AgentState::Stuck),
            pandas,
            still,
            // All at rest: the juggler moves from the last one's rest.
            rested_at: active
                .is_empty()
                .then(|| self.agents.iter().filter_map(|t| t.moved_at).max())
                .flatten(),
        })
    }
}

/// An agent shown in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub key: String,
    pub name: String,
    pub state: AgentState,
    /// In that state for so many minutes.
    pub minutes: u64,
    /// While it waits: what it asks, when known.
    pub question: Option<String>,
    pub character: Character,
    /// It moves a moment from then.
    pub moved_at: Option<Duration>,
}

/// The column's agents section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsView {
    pub rows: Vec<AgentRow>,
    /// Active agents left out for room.
    pub more: usize,
    pub resting: usize,
    pub waiting: usize,
    pub working: usize,
    pub done: usize,
    pub stuck: usize,
    /// Pandas shown; otherwise a dot of each agent's colour.
    pub pandas: bool,
    /// They hold still (battery saver).
    pub still: bool,
    pub rested_at: Option<Duration>,
}

impl AgentsView {
    fn total(&self) -> usize {
        self.resting + self.waiting + self.working + self.done + self.stuck
    }
}

impl Lang {
    pub fn agents_caption(self) -> &'static str {
        self.pick("HERDR AGENTS", "AGENTS HERDR")
    }

    /// The header's summary: who waits, else what the others do.
    pub fn agents_summary(self, v: &AgentsView) -> String {
        let fr = self == Self::Fr;
        if v.waiting > 0 {
            return match (fr, v.waiting) {
                (false, 1) => "1 waits for an answer".into(),
                (false, n) => format!("{n} wait for an answer"),
                (true, 1) => "1 attend une réponse".into(),
                (true, n) => format!("{n} attendent une réponse"),
            };
        }
        let plural = |n: usize| if fr && n > 1 { "s" } else { "" };
        let parts: Vec<String> = [
            (v.working, self.pick("at work", "au travail"), ""),
            (v.done, self.pick("done", "fini"), plural(v.done)),
            (v.stuck, self.pick("stuck", "bloqué"), plural(v.stuck)),
        ]
        .into_iter()
        .filter(|(n, _, _)| *n > 0)
        .map(|(n, what, s)| format!("{n} {what}{s}"))
        .collect();
        if parts.is_empty() {
            format!("{} {}", v.resting, self.pick("at rest", "au repos"))
        } else {
            parts.join(", ")
        }
    }

    /// What an agent shown in full is doing.
    pub fn agent_status(self, row: &AgentRow) -> String {
        let fr = self == Self::Fr;
        let lasted = |m: u64| match m {
            0..60 => format!("{m} min"),
            _ if m.is_multiple_of(60) => format!("{} h", m / 60),
            _ => format!("{} h {} min", m / 60, m % 60),
        };
        match row.state {
            AgentState::Work if row.minutes > 0 => format!(
                "{} · {}",
                self.pick("Works", "Travaille"),
                lasted(row.minutes)
            ),
            AgentState::Work => self.pick("Works", "Travaille").into(),
            AgentState::Wait => match (&row.question, fr) {
                (Some(q), false) => format!("Waits for an answer: “{q}”"),
                (Some(q), true) => format!("Attend une réponse\u{a0}: «\u{a0}{q}\u{a0}»"),
                (None, _) => self.agent_waiting().into(),
            },
            AgentState::Done if row.minutes > 0 => match fr {
                false => format!("Done · {} ago", lasted(row.minutes)),
                true => format!("Fini · il y a {}", lasted(row.minutes)),
            },
            AgentState::Done => self.pick("Done", "Fini").into(),
            AgentState::Stuck => self.pick("Stuck", "Bloqué").into(),
            AgentState::Rest => self.pick("At rest", "Au repos").into(),
        }
    }

    pub fn agent_waiting(self) -> &'static str {
        self.pick("Waits for an answer", "Attend une réponse")
    }

    /// A waiting agent's toast, without its question.
    pub fn agent_waits(self, name: &str) -> String {
        match self {
            Self::En => format!("{name} waits for an answer"),
            Self::Fr => format!("{name} attend une réponse"),
        }
    }

    pub fn agent_yes(self) -> &'static str {
        self.pick("Yes", "Oui")
    }

    pub fn agent_no(self) -> &'static str {
        self.pick("No", "Non")
    }

    pub fn agent_show(self) -> &'static str {
        self.pick("Show", "Voir")
    }

    pub fn agent_open(self) -> &'static str {
        self.pick("Open in herdr", "Ouvrir dans herdr")
    }

    /// A button's name for screen readers: whom it answers.
    pub fn agent_button(self, ask: AgentAsk, name: &str) -> String {
        match (self, ask) {
            (Self::En, AgentAsk::Yes) => format!("Yes to {name}"),
            (Self::En, AgentAsk::No) => format!("No to {name}"),
            (Self::En, AgentAsk::Show) => format!("Show {name}"),
            (Self::Fr, AgentAsk::Yes) => format!("Oui à {name}"),
            (Self::Fr, AgentAsk::No) => format!("Non à {name}"),
            (Self::Fr, AgentAsk::Show) => format!("Voir {name}"),
        }
    }

    /// Under the rows: the agents not shown.
    pub fn agents_rest(self, more: usize, resting: usize) -> Option<String> {
        Some(match (self, more, resting) {
            (_, 0, 0) => return None,
            (Self::En, 0, 1) => "1 other at rest in herdr".into(),
            (Self::En, 0, r) => format!("{r} others at rest in herdr"),
            (Self::En, m, 0) => format!("{m} more in herdr"),
            (Self::En, m, r) => format!("{m} more, {r} at rest in herdr"),
            (Self::Fr, 0, 1) => "1 autre au repos dans herdr".into(),
            (Self::Fr, 0, r) => format!("{r} autres au repos dans herdr"),
            (Self::Fr, m, 0) => format!("{m} de plus dans herdr"),
            (Self::Fr, m, r) => format!("{m} de plus, {r} au repos dans herdr"),
        })
    }

    /// Every agent at rest.
    pub fn agents_all_resting(self, count: usize, pandas: bool) -> String {
        let first = match (self, count) {
            (Self::En, 1) => "The agent is at rest.".to_owned(),
            (Self::En, n) => format!("The {n} agents are at rest."),
            (Self::Fr, 1) => "L’agent est au repos.".to_owned(),
            (Self::Fr, n) => format!("Les {n} agents sont au repos."),
        };
        if pandas {
            format!(
                "{first} {}",
                self.pick(
                    "The pandas make the most of it.",
                    "Les pandas en profitent."
                )
            )
        } else {
            first
        }
    }
}

/// The section's room in the column: some, or none.
pub(crate) fn room(shown: bool) -> Style {
    Style {
        gap: 12.0,
        padding: Insets {
            bottom: if shown { 18.0 } else { 0.0 },
            ..Insets::default()
        },
        height: if shown { Length::Auto } else { Length::Px(0.0) },
        clip: true,
        ..Style::default()
    }
}

/// A panda in `state`'s colour, moving a moment from `moved_at`.
fn panda(
    theme: &Theme,
    character: Character,
    mood: Mood,
    accent: Color,
    moved_at: Option<Duration>,
) -> Panda {
    let mut coloured = theme.clone();
    coloured.colors.accent = accent;
    let panda = Panda::themed(character, mood, &coloured);
    match moved_at {
        Some(at) => panda.moving_from(at),
        None => panda,
    }
}

/// Fills the section as the design draws it: a header with the summary,
/// the agents shown in full, then the others in a line. The rows' and the
/// buttons' actions come back.
pub(crate) fn fill(
    tree: &mut Tree,
    room: NodeId,
    theme: &Theme,
    lang: Lang,
    view: &AgentsView,
) -> Vec<(NodeId, Action)> {
    let missing = || unreachable!("adding to a node of this tree");
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };
    let theme = &theme
        .clone()
        .with_reduced_motion(theme.reduced_motion || view.still);
    let small = TextStyle {
        size: 13.0,
        weight: 500,
        line_height: 1.35,
        ..theme.text.label
    };
    let row = |gap: f32| Style {
        direction: Direction::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    };
    let card = |background| Look {
        background: Some(background),
        radius: 14.0,
        ..Look::default()
    };
    let mut targets = Vec::new();

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
        Kind::text(lang.agents_caption(), theme.text.caption),
    );
    add(
        tree,
        header,
        Style::default(),
        Kind::text(lang.agents_summary(view), small.with_color(Paint::Muted)),
    );

    if view.rows.is_empty() {
        let line = add(
            tree,
            room,
            Style {
                padding: Insets::symmetric(10.0, 12.0),
                ..row(12.0)
            },
            Kind::Box(card(Paint::SurfaceHover)),
        );
        if view.pandas {
            let juggler = panda(
                theme,
                Character::Apprentice,
                Mood::Play,
                theme.colors.accent,
                view.rested_at,
            );
            add(
                tree,
                line,
                square(PANDA),
                Kind::Custom {
                    draw: Box::new(juggler),
                    label: Some(Mood::Play.label(lang).into()),
                },
            );
        }
        add(
            tree,
            line,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text(
                lang.agents_all_resting(view.total(), view.pandas),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
        return targets;
    }

    for agent in &view.rows {
        let waits = agent.state == AgentState::Wait;
        let colour = agent.state.paint();
        let look = if waits {
            Look {
                background: Some(Paint::Rgba(theme.colors.waiting.with_alpha(31))),
                border: Some(Paint::Waiting),
                ..card(Paint::SurfaceHover)
            }
        } else {
            card(Paint::SurfaceHover)
        };
        let box_ = add(
            tree,
            room,
            Style {
                padding: Insets::symmetric(10.0, 12.0),
                gap: 10.0,
                ..Style::default()
            },
            Kind::Box(look),
        );
        let status = lang.agent_status(agent);
        let line = add(tree, box_, row(12.0), Kind::Box(Look::default()));
        tree.set_clickable(line, &format!("{} · {status}", agent.name));
        targets.push((line, action(agent, AgentAsk::Show)));
        if view.pandas {
            let mood = agent.state.mood();
            let drawn = panda(
                theme,
                agent.character,
                mood,
                theme.colors.get(colour),
                agent.moved_at,
            );
            add(
                tree,
                line,
                square(PANDA),
                Kind::Custom {
                    draw: Box::new(drawn),
                    label: Some(mood.label(lang).into()),
                },
            );
        } else {
            add(
                tree,
                line,
                square(12.0),
                Kind::Box(Look {
                    background: Some(colour),
                    radius: 6.0,
                    ..Look::default()
                }),
            );
        }
        let words = add(
            tree,
            line,
            Style {
                width: Length::Fill,
                gap: 2.0,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        add(
            tree,
            words,
            Style::default(),
            Kind::text_line(
                agent.name.clone(),
                TextStyle {
                    size: 15.0,
                    weight: 700,
                    ..theme.text.title
                },
            ),
        );
        let status_paint = match agent.state {
            AgentState::Wait | AgentState::Stuck => colour,
            _ => Paint::Muted,
        };
        add(
            tree,
            words,
            Style::default(),
            Kind::text(status, small.with_color(status_paint)),
        );
        if waits {
            let buttons = add(tree, box_, row(8.0), Kind::Box(Look::default()));
            for (ask, label, kind) in [
                (AgentAsk::Yes, lang.agent_yes(), ButtonKind::Primary),
                (AgentAsk::No, lang.agent_no(), ButtonKind::Plain),
                (AgentAsk::Show, lang.agent_show(), ButtonKind::Plain),
            ] {
                let button = add(
                    tree,
                    buttons,
                    Style {
                        width: Length::Fill,
                        height: Length::Px(44.0),
                        ..Style::default()
                    },
                    Kind::Button(
                        Button::new(label, kind, theme)
                            .with_name(lang.agent_button(ask, &agent.name)),
                    ),
                );
                targets.push((button, action(agent, ask)));
            }
        }
    }
    if let Some(line) = lang.agents_rest(view.more, view.resting) {
        add(
            tree,
            room,
            Style {
                padding: Insets {
                    left: 4.0,
                    ..Insets::default()
                },
                ..Style::default()
            },
            Kind::text(line, small.with_color(Paint::Muted)),
        );
    }
    targets
}

fn square(side: f32) -> Style {
    Style {
        width: Length::Px(side),
        height: Length::Px(side),
        ..Style::default()
    }
}

fn action(agent: &AgentRow, ask: AgentAsk) -> Action {
    Action::Agent {
        key: agent.key.clone(),
        ask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    fn board(states: &[(&str, AgentState)]) -> Board {
        let mut board = Board::default();
        board.replace(
            states
                .iter()
                .map(|(k, s)| ((*k).to_owned(), (*k).to_owned(), *s, None))
                .collect(),
            Duration::ZERO,
        );
        board
    }

    #[test]
    fn a_rest_shorter_than_two_seconds_never_shows() {
        let mut b = board(&[("refacto", AgentState::Work)]);
        let before = b.view(10 * S, true, false);
        assert!(b.set("refacto", AgentState::Rest, None, 10 * S).is_empty());
        assert_eq!(b.deadline(), Some(12 * S));
        assert_eq!(b.view(11 * S, true, false), before);
        b.set("refacto", AgentState::Work, None, 11 * S);
        assert!(b.settle(13 * S).is_empty());
        assert_eq!(b.deadline(), None);
        // It never stopped working: nothing moved, nothing changed.
        assert_eq!(b.view(11 * S, true, false), before);
    }

    #[test]
    fn a_rest_that_lasts_shows_and_the_juggler_moves() {
        let mut b = board(&[("refacto", AgentState::Work)]);
        b.set("refacto", AgentState::Rest, None, 10 * S);
        b.settle(11 * S);
        assert_eq!(b.view(11 * S, true, false).expect("agents").rows.len(), 1);
        b.settle(12 * S);
        let view = b.view(12 * S, true, false).expect("agents");
        assert!(view.rows.is_empty());
        assert_eq!(view.resting, 1);
        assert_eq!(view.rested_at, Some(12 * S));
    }

    #[test]
    fn waiting_shows_at_once_and_says_so_once() {
        let mut b = board(&[("refacto", AgentState::Work), ("tests", AgentState::Work)]);
        let changes = b.set("refacto", AgentState::Wait, Some("Push?".into()), 5 * S);
        assert_eq!(
            changes,
            vec![Change::Waits {
                key: "refacto".into(),
                name: "refacto".into(),
                question: Some("Push?".into())
            }]
        );
        assert!(
            b.set("refacto", AgentState::Wait, Some("Push?".into()), 6 * S)
                .is_empty()
        );
        let view = b.view(6 * S, true, false).expect("agents");
        assert_eq!(view.rows[0].key, "refacto");
        assert_eq!(view.rows[0].state, AgentState::Wait);
        assert_eq!(view.rows[0].moved_at, Some(5 * S));
        assert_eq!(
            b.set("refacto", AgentState::Work, None, 7 * S),
            vec![Change::Answered("refacto".into())]
        );
    }

    #[test]
    fn agents_keep_their_character_and_the_gone_are_answered() {
        let mut b = board(&[("a", AgentState::Wait), ("b", AgentState::Work)]);
        let changes = b.replace(vec![("b".into(), "b".into(), AgentState::Work, None)], S);
        assert_eq!(changes, vec![Change::Answered("a".into())]);
        let view = b.view(S, true, false).expect("agents");
        assert_eq!(view.rows[0].character, Character::nth(1));
        assert_eq!(b.clear(), Vec::new());
        assert_eq!(b.view(S, true, false), None);
    }

    #[test]
    fn three_rows_at_most_the_others_summed_up() {
        let b = board(&[
            ("a", AgentState::Work),
            ("b", AgentState::Done),
            ("c", AgentState::Wait),
            ("d", AgentState::Work),
            ("e", AgentState::Rest),
        ]);
        let view = b
            .view(Duration::from_secs(400), true, false)
            .expect("agents");
        let keys: Vec<_> = view.rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["c", "b", "a"]);
        assert_eq!((view.more, view.resting), (1, 1));
        assert_eq!(view.rows[2].minutes, 6);
        assert_eq!(Lang::En.agents_summary(&view), "1 waits for an answer");
        assert_eq!(
            Lang::En.agents_rest(view.more, view.resting).as_deref(),
            Some("1 more, 1 at rest in herdr")
        );
    }

    #[test]
    fn the_summary_says_what_they_do() {
        let b = board(&[("a", AgentState::Done), ("b", AgentState::Stuck)]);
        let view = b.view(S, true, false).expect("agents");
        assert_eq!(Lang::En.agents_summary(&view), "1 done, 1 stuck");
        assert_eq!(Lang::Fr.agents_summary(&view), "1 fini, 1 bloqué");
        let b = board(&[("a", AgentState::Rest), ("b", AgentState::Rest)]);
        let view = b.view(S, true, false).expect("agents");
        assert_eq!(Lang::Fr.agents_summary(&view), "2 au repos");
        assert_eq!(
            Lang::Fr.agents_all_resting(2, true),
            "Les 2 agents sont au repos. Les pandas en profitent."
        );
    }

    #[test]
    fn a_status_says_for_how_long() {
        let row = AgentRow {
            key: "a".into(),
            name: "refacto".into(),
            state: AgentState::Work,
            minutes: 6,
            question: None,
            character: Character::Apprentice,
            moved_at: None,
        };
        assert_eq!(Lang::En.agent_status(&row), "Works · 6 min");
        let done = AgentRow {
            state: AgentState::Done,
            minutes: 125,
            ..row.clone()
        };
        assert_eq!(Lang::Fr.agent_status(&done), "Fini · il y a 2 h 5 min");
        let waits = AgentRow {
            state: AgentState::Wait,
            question: Some("Push?".into()),
            ..row
        };
        assert_eq!(
            Lang::En.agent_status(&waits),
            "Waits for an answer: “Push?”"
        );
    }
}
