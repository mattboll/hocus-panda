//! The panel as a thin strip (#263, decision 0011), as board D12 draws its
//! form B: 72 px on the right of the primary screen (or of each, #282),
//! from top to bottom
//! the time, stacked, the next meeting's ring, a panda for each active
//! agent and how many others there are, the tasks and the notifications
//! counted, then the screen's desktops, the battery and the account. It
//! only shows: over it, or clicked, the compositor slides the whole column
//! out over the windows from the right edge.
//!
//! Its contents are a [`Glance`] at the column's state: the strip is built
//! again only when what it shows changes, which is rare (the minute, an
//! agent, a task), never per frame.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Canvas, Color, Direction, Draw, Icon, Insets, Justify, Kind, Length, Look,
    NodeId, Paint, PixelRect, Point, Rect, Size, Style, Surface, TextStyle, Theme, Tree,
};

use crate::{
    agents::{AgentRow, AgentState},
    column::{ActiveWindow, ColumnState, Meeting, Ring, ScreenDesktops, View, WallTime},
    i18n::Lang,
    icons,
};

/// The strip's width, from board D12.
pub const WIDTH: f32 = 72.0;
/// Active agents shown with their panda; the others are counted.
const AGENTS: usize = 3;
/// A panda's side in the strip, from the board.
const PANDA: f32 = 52.0;
/// The most a counter shows; more is said "99+".
const MOST: usize = 99;

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// The screen a strip or a bar is on (#282): its desktops and its active
/// window, out of the column's state.
#[derive(Debug, Clone, Copy, Default)]
pub struct Here<'a> {
    pub screen: Option<&'a ScreenDesktops>,
    pub active: Option<&'a ActiveWindow>,
}

impl<'a> Here<'a> {
    /// The column's screen and the focused window, as `state` has them.
    pub fn of(state: &'a ColumnState) -> Self {
        Self {
            screen: state.screens.first(),
            active: state.active.as_ref(),
        }
    }
}

/// What the strip and the top bar show of the column's state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Glance {
    pub time: WallTime,
    pub meeting: Option<Meeting>,
    /// The active agents shown, those of the desktop shown first.
    pub agents: Vec<AgentRow>,
    /// Every agent followed, shown or not; `None` without agents.
    pub agents_total: Option<usize>,
    /// Pandas shown; otherwise a dot of each agent's colour.
    pub pandas: bool,
    /// They hold still (battery saver).
    pub still: bool,
    /// Tasks not done (#255), and those of them late.
    pub todos: usize,
    pub late: usize,
    pub notifications: usize,
    /// This screen's desktops, and the one shown (from 1).
    pub desktops: (usize, usize),
    pub badges: Vec<crate::activity::Badge>,
    pub battery: Option<u8>,
    pub user: String,
    /// The active window: the top bar's alone.
    pub active: Option<ActiveWindow>,
}

impl Glance {
    /// What `state` shows at a glance, on the column's screen.
    pub fn of(state: &ColumnState) -> Self {
        Self::here(state, Here::of(state))
    }

    /// What `state` shows at a glance on the screen `here`.
    pub fn here(state: &ColumnState, here: Here<'_>) -> Self {
        let (agents, agents_total, pandas, still) = match &state.agents {
            Some(view) => {
                let agents = view
                    .rows
                    .iter()
                    .filter(|r| r.state != AgentState::Rest)
                    .take(AGENTS)
                    .map(|r| AgentRow {
                        // Not shown: their changes draw nothing again.
                        minutes: 0,
                        question: None,
                        ..r.clone()
                    })
                    .collect();
                (agents, Some(view.totals().total()), view.pandas, view.still)
            }
            None => (Vec::new(), None, false, false),
        };
        let screen = here.screen;
        Self {
            time: state.time,
            meeting: state.meeting.clone(),
            agents,
            agents_total,
            pandas,
            still,
            todos: state.tasks.as_ref().map_or(0, |t| t.open),
            late: state.tasks.as_ref().map_or(0, |t| t.late),
            notifications: state.notification_count,
            desktops: screen.map_or((1, 1), |s| (s.count.max(1), s.shown.max(1))),
            badges: screen.map(|s| s.badges.clone()).unwrap_or_default(),
            battery: state.status.battery,
            user: state.user.clone(),
            active: here.active.cloned(),
        }
    }

    /// Whether it is what `state` shows at a glance on the screen `here`
    /// (its active window too, `with_active`), found without copying
    /// anything: the state is given after every turn of the loop.
    pub fn matches(&self, state: &ColumnState, here: Here<'_>, with_active: bool) -> bool {
        let screen = here.screen;
        let agents_match = match &state.agents {
            Some(view) => {
                let mut shown = view
                    .rows
                    .iter()
                    .filter(|r| r.state != AgentState::Rest)
                    .take(AGENTS);
                self.agents_total == Some(view.totals().total())
                    && self.pandas == view.pandas
                    && self.still == view.still
                    && self.agents.iter().all(|a| {
                        shown.next().is_some_and(|r| {
                            r.key == a.key
                                && r.name == a.name
                                && r.state == a.state
                                && r.character == a.character
                                && r.moved_at == a.moved_at
                        })
                    })
                    && shown.next().is_none()
            }
            None => self.agents_total.is_none(),
        };
        agents_match
            && self.time == state.time
            && self.meeting == state.meeting
            && self.todos == state.tasks.as_ref().map_or(0, |t| t.open)
            && self.late == state.tasks.as_ref().map_or(0, |t| t.late)
            && self.notifications == state.notification_count
            && self.desktops == screen.map_or((1, 1), |s| (s.count.max(1), s.shown.max(1)))
            && self.badges.as_slice() == screen.map_or(&[][..], |s| s.badges.as_slice())
            && self.battery == state.status.battery
            && self.user == state.user
            && (!with_active || self.active.as_ref() == here.active)
    }

    /// What is said of the agents not shown: `+11` beside the pandas, or
    /// how many there are when none is shown; `None` without any.
    pub fn others(&self) -> Option<Others> {
        let total = self.agents_total?;
        let shown = self.agents.len();
        match (shown, total.saturating_sub(shown)) {
            (_, 0) if shown > 0 => None,
            (0, 0) => None,
            (0, all) => Some(Others::AllResting(all)),
            (_, more) => Some(Others::More(more)),
        }
    }
}

/// The agents a glance does not show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Others {
    /// So many more beside the ones shown.
    More(usize),
    /// None shown: so many, at rest or busy elsewhere.
    AllResting(usize),
}

impl Lang {
    /// What the strip's clock says to screen readers.
    pub fn strip_time(self, time: &str) -> String {
        match self {
            Self::En => format!("Time: {time}"),
            Self::Fr => format!("Heure\u{a0}: {time}"),
        }
    }

    /// The tasks' counter.
    pub fn strip_tasks(self, open: usize, late: usize) -> String {
        match (self, open, late) {
            (Self::En, 0, _) => "Tasks: all done".into(),
            (Self::Fr, 0, _) => "Tâches\u{a0}: tout est fait".into(),
            (Self::En, n, 0) => format!("Tasks: {n} to do"),
            (Self::Fr, n, 0) => format!("Tâches\u{a0}: {n} à faire"),
            (Self::En, n, late) => format!("Tasks: {n} to do, {late} late"),
            (Self::Fr, n, late) => format!("Tâches\u{a0}: {n} à faire, {late} en retard"),
        }
    }

    /// The notifications' counter.
    pub fn strip_notifications(self, n: usize) -> String {
        match self {
            Self::En => format!("Notifications: {n}"),
            Self::Fr => format!("Notifications\u{a0}: {n}"),
        }
    }

    /// The agents not shown, as the strip counts them.
    pub fn strip_others(self, others: Others) -> String {
        match (self, others) {
            (Self::En, Others::More(n)) => format!("{n} more agents"),
            (Self::Fr, Others::More(n)) => format!("{n} autres agents"),
            (Self::En, Others::AllResting(1)) => "1 agent, none active here".into(),
            (Self::Fr, Others::AllResting(1)) => "1 agent, aucun actif ici".into(),
            (Self::En, Others::AllResting(n)) => format!("{n} agents, none active here"),
            (Self::Fr, Others::AllResting(n)) => format!("{n} agents, aucun actif ici"),
        }
    }

    /// The battery, short: `64%`.
    pub fn battery_short(self, percent: u8) -> String {
        match self {
            Self::En => format!("{percent}%"),
            Self::Fr => format!("{percent}\u{202f}%"),
        }
    }

    /// The meeting, said: `Weekly sync, in 12 min`.
    pub fn meeting_said(self, meeting: &Meeting) -> String {
        format!("{}, {}", meeting.title, lower_first(&meeting.when))
    }

    /// The top bar's line beside its clock: the date, and the meeting.
    pub fn bar_day(self, time: &WallTime, meeting: Option<&Meeting>) -> String {
        let date = self.date(time.weekday, time.day.max(1), time.month.max(1));
        match meeting {
            Some(m) => format!("{date} · {} {}", m.title, lower_first(&m.when)),
            None => date,
        }
    }

    /// The top bar's agents when none is shown.
    pub fn bar_agents(self, n: usize) -> String {
        match (self, n) {
            (Self::En, 1) => "1 agent".into(),
            (Self::En, n) => format!("{n} agents"),
            (Self::Fr, 1) => "1 agent".into(),
            (Self::Fr, n) => format!("{n} agents"),
        }
    }

    /// The top bar's tasks.
    pub fn bar_tasks(self, open: usize) -> String {
        match (self, open) {
            (Self::En, 0) => "Tasks ✓".into(),
            (Self::Fr, 0) => "Tâches ✓".into(),
            (Self::En, n) => format!("Tasks {n}"),
            (Self::Fr, n) => format!("Tâches {n}"),
        }
    }

    /// The top bar's notifications.
    pub fn bar_notifications(self, n: usize) -> String {
        match self {
            Self::En => format!("Notif. {n}"),
            Self::Fr => format!("Notif. {n}"),
        }
    }
}

/// `In 12 min` as `in 12 min`, in the middle of a line.
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The time as the strip stacks it: the hour over the minutes, and the
/// half of the day under them on a 12-hour clock.
fn stacked(time: &WallTime) -> (String, String, Option<&'static str>) {
    use hocus_core::config::ClockFormat;
    let minutes = format!("{:02}", time.minute);
    match time.format {
        ClockFormat::TwentyFour => (format!("{:02}", time.hour), minutes, None),
        ClockFormat::Twelve => {
            let hour = match time.hour % 12 {
                0 => 12,
                h => h,
            };
            let half = if time.hour < 12 { "AM" } else { "PM" };
            (hour.to_string(), minutes, Some(half))
        }
    }
}

/// A screen's desktops as dots, one under another, the shown one in the
/// accent; one where something goes on carries its badge.
struct Column {
    count: usize,
    shown: usize,
    badges: Vec<crate::activity::Badge>,
}

const DOT: f32 = 8.0;
const DOT_GAP: f32 = 6.0;

impl Draw for Column {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let x = rect.x + (rect.w - DOT) / 2.0;
        for i in 0..self.count {
            let dot = Rect::new(x, rect.y + i as f32 * (DOT + DOT_GAP), DOT, DOT);
            let badge = self.badges.get(i).copied().unwrap_or_default();
            if i + 1 != self.shown && badge.is_some() {
                badge.draw_dot(canvas, dot, DOT, theme);
                continue;
            }
            let color = if i + 1 == self.shown {
                theme.colors.accent
            } else {
                theme.colors.muted.with_alpha(0x66)
            };
            canvas.fill_rect(dot, DOT / 2.0, color);
        }
    }

    fn size(&self) -> Size {
        let n = self.count.max(1) as f32;
        Size::new(DOT, n * DOT + (n - 1.0) * DOT_GAP)
    }
}

fn square(side: f32) -> Style {
    Style {
        width: Length::Px(side),
        height: Length::Px(side),
        align: Align::Center,
        justify: Justify::Center,
        ..Style::default()
    }
}

/// The colour of an agent's state, as its row in the column has it.
pub(crate) fn agent_color(theme: &Theme, state: AgentState) -> Color {
    theme.color(state.paint())
}

/// An agent's panda, or its dot without pandas, with its state's dot at
/// its corner as the board draws it.
pub(crate) fn agent_mark(
    tree: &mut Tree,
    parent: NodeId,
    theme: &Theme,
    lang: Lang,
    agent: &AgentRow,
    pandas: bool,
    side: f32,
) -> NodeId {
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(MISSING)
    };
    let colour = agent_color(theme, agent.state);
    let mood = agent.state.mood();
    let label = format!("{} · {}", agent.name, mood.label(lang));
    let mark = add(tree, parent, square(side), Kind::Box(Look::default()));
    if pandas {
        let drawn = crate::agents::panda(theme, agent.character, mood, colour, agent.moved_at);
        add(
            tree,
            mark,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(drawn),
                label: Some(label),
            },
        );
        // Its state's dot, ringed with the panel's surface.
        let ring = add(
            tree,
            mark,
            Style {
                at: Some(Point::new(side - 14.0, 0.0)),
                ..square(14.0)
            },
            Kind::Box(Look {
                background: Some(Paint::Surface),
                radius: 7.0,
                ..Look::default()
            }),
        );
        add(
            tree,
            ring,
            square(10.0),
            Kind::Box(Look {
                background: Some(Paint::Rgba(colour)),
                radius: 5.0,
                ..Look::default()
            }),
        );
    } else {
        let dot = add(
            tree,
            mark,
            square(14.0),
            Kind::Box(Look {
                background: Some(Paint::Rgba(colour)),
                radius: 7.0,
                ..Look::default()
            }),
        );
        tree.set_clickable(dot, &label);
    }
    mark
}

/// The strip.
pub struct Strip {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    shown: Glance,
    /// The time's box, and its hour and minutes.
    clock: Option<(NodeId, NodeId, NodeId)>,
}

impl std::fmt::Debug for Strip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Strip")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl Strip {
    /// A strip `height` logical pixels tall, drawn at `scale`.
    pub fn new(theme: Theme, lang: Lang, height: f32, scale: f32) -> Self {
        let size = Size::new(WIDTH, height);
        let mut strip = Self {
            lang,
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            shown: Glance::default(),
            clock: None,
        };
        strip.build();
        strip
    }

    /// Shows what `state` shows at a glance; built again only when that
    /// changes, and only its clock's text when the minute turns.
    pub fn set_state(&mut self, state: &ColumnState) {
        self.set_state_here(state, Here::of(state));
    }

    /// The same, on the screen `here` (#282).
    pub fn set_state_here(&mut self, state: &ColumnState, here: Here<'_>) {
        if self.shown.matches(state, here, false) {
            return;
        }
        let glance = Glance {
            // The top bar's alone.
            active: None,
            ..Glance::here(
                state,
                Here {
                    active: None,
                    ..here
                },
            )
        };
        let mut old = std::mem::take(&mut self.shown);
        // The same lines: the same clock, the same half of the day.
        let lines = (old.time.format, old.time.hour < 12);
        old.time = glance.time;
        let minute = old == glance && lines == (glance.time.format, glance.time.hour < 12);
        self.shown = glance;
        match self.clock {
            Some(clock) if minute => self.tell_time(clock),
            _ => self.build(),
        }
    }

    /// The clock's lines say the time shown.
    fn tell_time(&mut self, (time, hour, minutes): (NodeId, NodeId, NodeId)) {
        let (h, m, _) = stacked(&self.shown.time);
        self.tree.set_text(hour, &h);
        self.tree.set_text(minutes, &m);
        let said = self.lang.strip_time(&self.shown.time.text());
        self.tree.set_name(time, &said);
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if *self.tree.theme() != theme {
            self.tree.set_theme(theme);
            self.build();
        }
    }

    pub fn set_lang(&mut self, lang: Lang) {
        if lang != self.lang {
            self.lang = lang;
            self.build();
        }
    }

    pub fn resize(&mut self, height: f32, scale: f32) {
        let size = Size::new(WIDTH, height);
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.build();
        }
    }

    pub fn size(&self) -> Size {
        self.tree.size()
    }

    /// Whether `at`, from its corner, is on it.
    pub fn hit(&self, at: Point) -> bool {
        let s = self.tree.size();
        Rect::new(0.0, 0.0, s.w, s.h).contains(at)
    }

    /// Draws what changed at `now`.
    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn is_animating(&self, now: Duration) -> bool {
        self.tree.is_animating(now)
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    /// Its see-through panel, for the compositor to blur what is behind.
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
    }

    /// Its panels' shadows, from its corner, for the compositor to draw
    /// under it (#281).
    pub fn shadows(&self) -> Vec<hocus_ui::Cast> {
        self.tree.shadows()
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }

    fn build(&mut self) {
        let tree = &mut self.tree;
        let root = tree.root();
        for child in tree.children(root) {
            tree.remove(child);
        }
        let theme = tree.theme().clone();
        let theme = &theme
            .clone()
            .with_reduced_motion(theme.reduced_motion || self.shown.still);
        let lang = self.lang;
        let g = &self.shown;
        let add = |tree: &mut Tree, parent, style, kind| {
            tree.add(parent, style, kind).unwrap_or_else(MISSING)
        };
        tree.set_look(root, Look::default());
        let card = add(
            tree,
            root,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                padding: Insets::symmetric(20.0, 0.0),
                gap: 22.0,
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Glass),
                radius: 22.0,
                border: Some(Paint::Border),
                shadow: 0.0,
            }),
        );

        // The time, stacked.
        let (hour, minutes, half) = stacked(&g.time);
        let time = add(
            tree,
            card,
            Style {
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        tree.set_clickable(time, &lang.strip_time(&g.time.text()));
        let digits = TextStyle {
            size: 26.0,
            weight: 600,
            line_height: 1.05,
            ..theme.text.display
        };
        let hour = add(tree, time, Style::default(), Kind::text_line(hour, digits));
        let minutes = add(
            tree,
            time,
            Style::default(),
            Kind::text_line(minutes, digits),
        );
        self.clock = Some((time, hour, minutes));
        if let Some(half) = half {
            add(
                tree,
                time,
                Style::default(),
                Kind::text_line(half, theme.text.caption.with_color(Paint::Muted)),
            );
        }

        // The next meeting's ring.
        if let Some(meeting) = &g.meeting {
            add(
                tree,
                card,
                square(40.0),
                Kind::Custom {
                    draw: Box::new(Ring {
                        progress: f32::from(meeting.progress.min(100)) / 100.0,
                    }),
                    label: Some(lang.meeting_said(meeting)),
                },
            );
        }

        let rule = |tree: &mut Tree| {
            add(
                tree,
                card,
                Style {
                    width: Length::Px(40.0),
                    height: Length::Px(1.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Border),
                    ..Look::default()
                }),
            );
        };

        // A panda for each active agent, then how many others.
        if g.agents_total.is_some() {
            rule(tree);
            for agent in &g.agents {
                agent_mark(tree, card, theme, lang, agent, g.pandas, PANDA);
            }
            if let Some(others) = g.others() {
                let line = add(
                    tree,
                    card,
                    Style {
                        direction: Direction::Row,
                        gap: 4.0,
                        align: Align::Center,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                );
                tree.set_clickable(line, &lang.strip_others(others));
                let small = TextStyle {
                    size: 13.0,
                    weight: 700,
                    ..theme.text.label
                }
                .with_color(Paint::Muted);
                let text = match others {
                    Others::More(n) => format!("+{n}"),
                    Others::AllResting(n) => n.to_string(),
                };
                add(tree, line, Style::default(), Kind::text_line(text, small));
                if matches!(others, Others::AllResting(_)) {
                    // The board's ○: at rest.
                    add(
                        tree,
                        line,
                        square(9.0),
                        Kind::Box(Look {
                            radius: 4.5,
                            border: Some(Paint::Muted),
                            ..Look::default()
                        }),
                    );
                }
            }
        }
        rule(tree);

        // The tasks and the notifications, counted beside their icons;
        // with none, the icon alone.
        let count = TextStyle {
            size: 15.0,
            weight: 700,
            ..theme.text.label
        };
        let counter =
            |tree: &mut Tree, name: &str, icon: &str, n: usize, (none, some): (Paint, Paint)| {
                let chip = add(
                    tree,
                    card,
                    Style {
                        direction: Direction::Row,
                        gap: 4.0,
                        align: Align::Center,
                        justify: Justify::Center,
                        height: Length::Px(36.0),
                        padding: Insets::symmetric(0.0, 9.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::SurfaceHover),
                        radius: 10.0,
                        ..Look::default()
                    }),
                );
                tree.set_clickable(chip, name);
                let (side, ink) = if n == 0 { (18.0, none) } else { (16.0, some) };
                add(
                    tree,
                    chip,
                    Style::default(),
                    Kind::Icon {
                        icon: Icon::stroked(icon, side, 2.2)
                            .unwrap_or_else(|| unreachable!("a constant icon")),
                        color: ink,
                        label: None,
                    },
                );
                if n > 0 {
                    let text = if n > MOST {
                        format!("{MOST}+")
                    } else {
                        n.to_string()
                    };
                    add(
                        tree,
                        chip,
                        Style::default(),
                        Kind::text_line(text, count.with_color(some)),
                    );
                }
            };
        counter(
            tree,
            &lang.strip_tasks(g.todos, g.late),
            icons::CHECK,
            g.todos,
            // Late ones count in the danger colour, as board Bureau has it.
            (
                Paint::Text,
                if g.late > 0 {
                    Paint::Error
                } else {
                    Paint::Text
                },
            ),
        );
        counter(
            tree,
            &lang.strip_notifications(g.notifications),
            icons::BELL,
            g.notifications,
            (Paint::Muted, Paint::Text),
        );

        // What is left, then the desktops, the battery and the account.
        add(
            tree,
            card,
            Style {
                height: Length::Fill,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        let foot = add(
            tree,
            card,
            Style {
                align: Align::Center,
                gap: 10.0,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        let (count_desktops, shown) = g.desktops;
        let desktops = lang.desktop_of(shown, count_desktops);
        let dots = add(
            tree,
            foot,
            Style::default(),
            Kind::Custom {
                draw: Box::new(Column {
                    count: count_desktops,
                    shown,
                    badges: g.badges.clone(),
                }),
                label: Some(desktops),
            },
        );
        let _ = dots;
        if let Some(battery) = g.battery {
            let low = battery <= 15;
            add(
                tree,
                foot,
                Style::default(),
                Kind::text_line(
                    lang.battery_short(battery),
                    TextStyle {
                        size: 13.0,
                        weight: 500,
                        ..theme.text.label
                    }
                    .with_color(if low {
                        Paint::Waiting
                    } else {
                        Paint::Muted
                    }),
                ),
            );
        }
        let avatar = add(
            tree,
            foot,
            Style {
                width: Length::Px(36.0),
                height: Length::Px(36.0),
                align: Align::Center,
                justify: Justify::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Accent),
                radius: 18.0,
                ..Look::default()
            }),
        );
        tree.set_clickable(avatar, &lang.account(&g.user));
        let initial: String = g
            .user
            .chars()
            .take(1)
            .flat_map(char::to_uppercase)
            .collect();
        add(
            tree,
            avatar,
            Style::default(),
            Kind::text_line(
                initial,
                theme
                    .text
                    .title
                    .with_weight(700)
                    .with_color(Paint::OnAccent),
            ),
        );
        tree.mark(root);
    }
}

/// The hint where the hidden panel waits (form D): a thin bar at the
/// middle of the right edge, in the accent, faint.
pub struct EdgeHint {
    surface: Surface,
}

impl std::fmt::Debug for EdgeHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeHint").finish_non_exhaustive()
    }
}

impl EdgeHint {
    pub const SIZE: Size = Size::new(5.0, 320.0);
    /// From the screen's right edge, as the board has it.
    pub const RIGHT: f32 = 3.0;

    pub fn new(theme: Theme, scale: f32) -> Self {
        let mut tree = Tree::new(theme.clone(), Self::SIZE, scale);
        let root = tree.root();
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Rgba(theme.colors.accent.with_alpha(102))),
                radius: 2.5,
                ..Look::default()
            },
        );
        let mut surface = Surface::new(Self::SIZE, scale);
        let damage = tree.update(Duration::ZERO);
        surface.draw(&mut tree, &damage);
        Self { surface }
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agents::AgentsView, panda::Character};

    fn row(name: &str, state: AgentState) -> AgentRow {
        AgentRow {
            key: name.into(),
            name: name.into(),
            state,
            minutes: 4,
            question: None,
            character: Character::nth(0),
            moved_at: None,
        }
    }

    fn with_agents(rows: Vec<AgentRow>, resting: usize) -> ColumnState {
        let mut counts = crate::agents::Counts::default();
        for r in &rows {
            match r.state {
                AgentState::Rest => counts.resting += 1,
                AgentState::Wait => counts.waiting += 1,
                AgentState::Work => counts.working += 1,
                AgentState::Done => counts.done += 1,
                AgentState::Stuck => counts.stuck += 1,
            }
        }
        counts.resting += resting;
        ColumnState {
            agents: Some(AgentsView {
                rows,
                more: 0,
                counts,
                others: Vec::new(),
                pandas: true,
                still: false,
                rested_at: None,
            }),
            ..ColumnState::default()
        }
    }

    #[test]
    fn active_agents_show_and_the_others_are_counted() {
        let state = with_agents(
            vec![
                row("refacto-ipc", AgentState::Wait),
                row("tests-e2e", AgentState::Work),
                row("idle", AgentState::Rest),
            ],
            10,
        );
        let glance = Glance::of(&state);
        assert_eq!(glance.agents.len(), 2);
        assert_eq!(glance.others(), Some(Others::More(11)));
        // Their minutes change nothing it shows.
        assert_eq!(glance.agents[0].minutes, 0);
        let resting = Glance::of(&with_agents(vec![row("idle", AgentState::Rest)], 12));
        assert_eq!(resting.others(), Some(Others::AllResting(13)));
        assert_eq!(Glance::of(&ColumnState::default()).others(), None);
    }

    #[test]
    fn the_time_stacks_on_both_clocks() {
        let mut time = WallTime {
            hour: 14,
            minute: 7,
            ..WallTime::default()
        };
        assert_eq!(stacked(&time), ("14".into(), "07".into(), None));
        time.format = hocus_core::config::ClockFormat::Twelve;
        assert_eq!(stacked(&time), ("2".into(), "07".into(), Some("PM")));
    }

    #[test]
    fn the_strip_is_built_again_only_when_what_it_shows_changes() {
        let mut strip = Strip::new(Theme::default(), Lang::En, 760.0, 1.0);
        strip.draw(Duration::ZERO);
        let mut state = ColumnState {
            user: "Alex".into(),
            ..ColumnState::default()
        };
        strip.set_state(&state);
        assert!(!strip.draw(Duration::from_millis(16)).is_empty());
        // The tray is the column's alone.
        state.tray.push(crate::column::TrayApp {
            key: "k".into(),
            title: "Chat".into(),
            attention: false,
            icon: None,
        });
        strip.set_state(&state);
        assert!(strip.draw(Duration::from_millis(32)).is_empty());
        let labels = |node: &AccessNode, out: &mut Vec<String>| {
            fn walk(node: &AccessNode, out: &mut Vec<String>) {
                out.extend(node.label.clone());
                for c in &node.children {
                    walk(c, out);
                }
            }
            walk(node, out);
        };
        let mut said = Vec::new();
        labels(&strip.access(), &mut said);
        assert!(said.iter().any(|l| l == "Account: Alex"), "{said:?}");
        assert!(said.iter().any(|l| l == "Tasks: all done"), "{said:?}");
    }

    #[test]
    fn the_minute_turning_changes_only_the_clock() {
        let mut strip = Strip::new(Theme::default(), Lang::En, 760.0, 1.0);
        let mut state = ColumnState {
            user: "Alex".into(),
            ..ColumnState::default()
        };
        state.time.hour = 14;
        state.time.minute = 37;
        strip.set_state(&state);
        strip.draw(Duration::ZERO);
        let built = strip.clock;
        state.time.minute = 38;
        strip.set_state(&state);
        assert_eq!(strip.clock, built, "built again");
        let drawn = strip.draw(Duration::from_millis(16));
        assert!(!drawn.is_empty());
        // Only around the clock, at the top.
        assert!(drawn.iter().all(|r| r.y < 120), "{drawn:?}");
        let mut said = Vec::new();
        fn walk(node: &AccessNode, out: &mut Vec<String>) {
            out.extend(node.label.clone());
            for c in &node.children {
                walk(c, out);
            }
        }
        walk(&strip.access(), &mut said);
        assert!(said.iter().any(|l| l == "Time: 14:38"), "{said:?}");
        assert!(said.iter().any(|l| l == "38"), "{said:?}");
    }

    #[test]
    fn meetings_are_said_in_the_middle_of_a_line() {
        let meeting = Meeting {
            title: "Weekly sync".into(),
            when: "In 12 min".into(),
            progress: 80,
            join: true,
        };
        assert_eq!(Lang::En.meeting_said(&meeting), "Weekly sync, in 12 min");
        let time = WallTime {
            weekday: 2,
            day: 21,
            month: 10,
            ..WallTime::default()
        };
        assert!(
            Lang::En
                .bar_day(&time, Some(&meeting))
                .ends_with(" · Weekly sync in 12 min")
        );
    }
}
