//! An MCP agent's seat on the screen (#43), as board D04 draws it: its
//! pointer in its own colour with its name beside it, and, before a click,
//! a ring at the point and what it means to do in one line; a sensitive
//! action waits on a card the user answers, with the pointer, or with the
//! keyboard once clicked (#229). The compositor places the view so that its
//! anchor (the pointer's tip) is where the agent points; where no screen
//! shows the agent's window, the card alone shows.

use std::time::Duration;

use hocus_ui::{
    AccessNode, ButtonKind, Color, Direction, Event, Input, Insets, Key, Kind, Length, Look,
    NodeId, Paint, PixelRect, Point, Rect, Shape, Size, Style, Surface, Theme, Tree,
    canvas::Canvas, tree::Draw,
};

use crate::{column::View, i18n::Lang};

/// The ring around the point, its radius and the glow outside it.
const RING: f32 = 22.0;
const GLOW: f32 = 8.0;
/// The pointer's box, and its tip in it.
const POINTER: f32 = 34.0;
const TIP: f32 = 1.0;
/// The label, from the pointer's tip.
const LABEL: Point = Point::new(30.0, 33.0);
/// The card's width, its gap under the label, and its distance from the
/// screen's edges.
const CARD: f32 = 440.0;
const CARD_GAP: f32 = 64.0;
const MARGIN: f32 = 16.0;

/// The arrow of the boards' pointers, in a 28×28 box.
const ARROW: &str = "M1 1 L1 22 L7 16 L11 26 L15 24 L11 15 L19 15 Z";

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// An agent's colour, the same for its name every time: one of the
/// theme's agent colour and accents, read on its background.
pub fn agent_color(theme: &Theme, name: &str) -> Color {
    let a = &theme.accents;
    let choices = [theme.colors.agent, a.bamboo, a.amber, a.blue, a.pink];
    // FNV-1a: stable across runs and builds, unlike the standard hasher.
    let hash = name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    choices[hash as usize % choices.len()]
}

/// A theme whose accent is the agent's colour, as readable as an accent.
pub fn agent_theme(theme: &Theme, name: &str) -> Theme {
    theme.clone().with_custom_accent(agent_color(theme, name))
}

/// What the agent asks the user to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    /// "helper will click · sensitive action".
    pub title: String,
    /// What it means to do.
    pub intent: String,
    /// Why it asks: "In a password field".
    pub why: String,
    /// The user clicked it: it has the keyboard, Enter allows, Escape
    /// refuses.
    pub keys: bool,
}

/// What the seat shows now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Showing {
    /// Its pointer and label are drawn: a screen shows where it points.
    pub pointer: bool,
    /// Its name, or its name and what it does.
    pub label: String,
    /// The ring at the point, before a click.
    pub ring: bool,
    pub confirm: Option<Confirm>,
}

/// What the user answered on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Allow,
    Refuse,
    TakeOver,
}

impl Lang {
    pub fn agent_will(self, agent: &str, typing: bool) -> String {
        match (self, typing) {
            (Self::En, false) => format!("{agent} will click · sensitive action"),
            (Self::En, true) => format!("{agent} will type · sensitive action"),
            (Self::Fr, false) => format!("{agent} va cliquer · action sensible"),
            (Self::Fr, true) => format!("{agent} va saisir · action sensible"),
        }
    }

    pub fn sensitive_password(self) -> &'static str {
        self.pick("In a password field", "Dans un champ de mot de passe")
    }

    pub fn sensitive_app(self, app: &str) -> String {
        match self {
            Self::En => format!("In {app}, which asks you first"),
            Self::Fr => format!("Dans {app}, qui vous demande d’abord"),
        }
    }

    /// Why it asks, and the desktop no screen shows where it acts.
    pub fn sensitive_unseen(self, why: &str, desktop: &str) -> String {
        match self {
            Self::En => format!("{why} · {desktop}, not shown"),
            Self::Fr => format!("{why} · {desktop}, non affiché"),
        }
    }

    pub fn allow(self) -> &'static str {
        self.pick("Allow", "Valider")
    }

    pub fn refuse(self) -> &'static str {
        self.pick("Refuse", "Refuser")
    }

    pub fn take_over(self) -> &'static str {
        self.pick("Take over", "Prendre la main")
    }

    pub fn hand_back(self) -> &'static str {
        self.pick("Hand back", "Rendre la main")
    }

    /// What an agent does when it gave no intent of its own.
    pub fn clicks_in(self, what: &str) -> String {
        match self {
            Self::En => format!("Clicks in {what}"),
            Self::Fr => format!("Clique dans {what}"),
        }
    }

    pub fn clicks_on(self, label: &str) -> String {
        match self {
            Self::En => format!("Clicks “{label}”"),
            Self::Fr => format!("Clique sur «\u{a0}{label}\u{a0}»"),
        }
    }

    pub fn scrolls_in(self, what: &str) -> String {
        match self {
            Self::En => format!("Scrolls in {what}"),
            Self::Fr => format!("Fait défiler {what}"),
        }
    }

    pub fn types_in(self, what: &str) -> String {
        match self {
            Self::En => format!("Types in {what}"),
            Self::Fr => format!("Saisit dans {what}"),
        }
    }

    pub fn presses_in(self, keys: &str, what: &str) -> String {
        match self {
            Self::En => format!("Presses {keys} in {what}"),
            Self::Fr => format!("Appuie sur {keys} dans {what}"),
        }
    }
}

/// The ring: dashed, in the agent's colour, a glow around it.
struct Ring;

impl Draw for Ring {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let color = theme.colors.accent;
        let centre = Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        canvas.arc(
            centre,
            RING + GLOW / 2.0,
            0.0,
            std::f32::consts::TAU,
            GLOW,
            color.with_alpha(56),
        );
        let dashes = 14;
        let step = std::f32::consts::TAU / dashes as f32;
        for i in 0..dashes {
            canvas.arc(centre, RING, i as f32 * step, step * 0.55, 3.0, color);
        }
        canvas.fill_rect(
            Rect::new(centre.x - 2.5, centre.y - 2.5, 5.0, 5.0),
            2.5,
            color,
        );
    }

    fn size(&self) -> Size {
        let d = 2.0 * (RING + GLOW);
        Size::new(d, d)
    }
}

/// The pointer: the boards' arrow in the agent's colour, outlined.
struct Arrow(Option<Shape>);

impl Draw for Arrow {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let Some(shape) = &self.0 else {
            return;
        };
        canvas.fill_shape(shape, 28.0, rect, theme.colors.accent);
        canvas.stroke_shape(shape, 28.0, rect, 1.6, Color::rgba(17, 17, 17, 255));
    }

    fn size(&self) -> Size {
        Size::new(POINTER, POINTER)
    }
}

/// One agent's pointer, label, ring and card.
pub struct SeatView {
    tree: Tree,
    surface: Surface,
    /// Where the view's corner is, from the screen's.
    origin: Point,
    card: Option<NodeId>,
    allow: Option<NodeId>,
    refuse: Option<NodeId>,
    take_over: Option<NodeId>,
}

impl std::fmt::Debug for SeatView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeatView")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

impl SeatView {
    /// The view of an agent pointing at `at` on a screen of `room`
    /// (logical, from the screen's corner); `theme` is the agent's
    /// ([`agent_theme`]).
    pub fn new(
        theme: Theme,
        lang: Lang,
        scale: f32,
        room: Size,
        at: Point,
        showing: &Showing,
    ) -> Self {
        let mut tree = Tree::new(theme.clone(), room, scale);
        let root = tree.root();
        // Over whatever is on the screen: only its parts are drawn.
        tree.set_look(root, Look::default());
        let mut parts = Vec::new();
        let mut add = |tree: &mut Tree, style: Style, kind: Kind| {
            let id = tree.add(root, style, kind).unwrap_or_else(MISSING);
            parts.push((id, style));
            id
        };
        if showing.pointer && showing.ring {
            add(
                &mut tree,
                Style {
                    at: Some(Point::new(at.x - RING - GLOW, at.y - RING - GLOW)),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Ring),
                    label: None,
                },
            );
        }
        if showing.pointer {
            add(
                &mut tree,
                Style {
                    at: Some(Point::new(at.x - TIP, at.y - TIP)),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Arrow(Shape::svg(ARROW))),
                    label: None,
                },
            );
            let label = add(
                &mut tree,
                Style {
                    at: Some(Point::new(at.x + LABEL.x, at.y + LABEL.y)),
                    padding: Insets::symmetric(4.0, 10.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Accent),
                    radius: 8.0,
                    ..Look::default()
                }),
            );
            tree.add(
                label,
                Style::default(),
                Kind::text_line(
                    &showing.label,
                    theme.text.caption.with_color(Paint::OnAccent),
                ),
            )
            .unwrap_or_else(MISSING);
        }
        let mut view = Self {
            tree,
            surface: Surface::new(Size::new(1.0, 1.0), scale),
            origin: Point::default(),
            card: None,
            allow: None,
            refuse: None,
            take_over: None,
        };
        if let Some(confirm) = &showing.confirm {
            parts.extend(view.add_card(&theme, lang, room, at, confirm));
        }
        view.fit(room, scale, &parts);
        // Clicked, the card has the keyboard: Allow shows it, as Tab
        // would.
        if showing.confirm.as_ref().is_some_and(|c| c.keys) {
            view.tree.input(Input::Key(Key::Tab));
        }
        view
    }

    fn add_card(
        &mut self,
        theme: &Theme,
        lang: Lang,
        room: Size,
        at: Point,
        confirm: &Confirm,
    ) -> Option<(NodeId, Style)> {
        let t = &mut self.tree;
        let root = t.root();
        let card = t
            .add(
                root,
                Style {
                    at: Some(Point::default()),
                    width: Length::Px(CARD),
                    padding: Insets::symmetric(20.0, 22.0),
                    gap: 14.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    radius: 16.0,
                    border: Some(Paint::Accent),
                    shadow: 30.0,
                }),
            )
            .unwrap_or_else(MISSING);
        t.add(
            card,
            Style::default(),
            Kind::text(
                confirm.title.to_uppercase(),
                theme.text.caption.with_color(Paint::Accent),
            ),
        )
        .unwrap_or_else(MISSING);
        t.add(
            card,
            Style::default(),
            Kind::text(
                &confirm.intent,
                hocus_ui::TextStyle {
                    size: 18.0,
                    line_height: 1.4,
                    ..theme.text.title
                },
            ),
        )
        .unwrap_or_else(MISSING);
        t.add(
            card,
            Style::default(),
            Kind::text(&confirm.why, theme.text.body.with_color(Paint::Muted)),
        )
        .unwrap_or_else(MISSING);
        let row = t
            .add(
                card,
                Style {
                    direction: Direction::Row,
                    gap: 10.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        let button = |t: &mut Tree, words: &str, kind| {
            t.add(
                row,
                Style {
                    height: Length::Px(44.0),
                    padding: Insets::symmetric(0.0, 20.0),
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::button(words, kind, theme),
            )
            .unwrap_or_else(MISSING)
        };
        self.allow = Some(button(t, lang.allow(), ButtonKind::Primary));
        self.refuse = Some(button(t, lang.refuse(), ButtonKind::Plain));
        self.take_over = Some(button(t, lang.take_over(), ButtonKind::Plain));
        // Under the label, or above the ring when the screen ends first;
        // within the screen's edges.
        t.layout();
        let h = t.rect(card).map_or(200.0, |r| r.h);
        let below = at.y + CARD_GAP;
        let y = if below + h + MARGIN <= room.h {
            below
        } else {
            (at.y - RING - GLOW - 12.0 - h).max(MARGIN)
        };
        let x = (at.x - RING - GLOW).min(room.w - CARD - MARGIN).max(MARGIN);
        let style = Style {
            at: Some(Point::new(x, y)),
            width: Length::Px(CARD),
            padding: Insets::symmetric(20.0, 22.0),
            gap: 14.0,
            ..Style::default()
        };
        t.set_style(card, style);
        self.card = Some(card);
        Some((card, style))
    }

    /// The view shrinks to its parts, within the screen.
    fn fit(&mut self, room: Size, scale: f32, parts: &[(NodeId, Style)]) {
        self.tree.layout();
        let all = parts
            .iter()
            .filter_map(|(p, _)| self.tree.rect(*p))
            // The card's shadow, and the pill's, around them.
            .map(|r| Rect::new(r.x - 24.0, r.y - 24.0, r.w + 48.0, r.h + 48.0))
            .reduce(|a, b| a.union(&b))
            .unwrap_or_default();
        let x0 = all.x.max(0.0).floor();
        let y0 = all.y.max(0.0).floor();
        let x1 = all.right().min(room.w).ceil();
        let y1 = (all.y + all.h).min(room.h).ceil();
        let size = Size::new((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        for (id, style) in parts {
            let at = style.at.unwrap_or_default();
            self.tree.set_style(
                *id,
                Style {
                    at: Some(Point::new(at.x - x0, at.y - y0)),
                    ..*style
                },
            );
        }
        self.tree.resize(size, scale);
        self.surface = Surface::new(size, scale);
        self.origin = Point::new(x0, y0);
    }

    /// Where the view's corner goes, from the screen's corner.
    pub fn origin(&self) -> Point {
        self.origin
    }

    /// The card, from the view's corner, while one asks.
    pub fn card_rect(&self) -> Option<Rect> {
        self.tree.rect(self.card?)
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.tree.input(Input::PointerMove(at));
    }

    pub fn pointer_button(&mut self, pressed: bool) -> Option<Answer> {
        let events = self.tree.input(Input::PointerButton { pressed });
        events.into_iter().find_map(|event| match event {
            Event::Activated(id) if Some(id) == self.allow => Some(Answer::Allow),
            Event::Activated(id) if Some(id) == self.refuse => Some(Answer::Refuse),
            Event::Activated(id) if Some(id) == self.take_over => Some(Answer::TakeOver),
            _ => None,
        })
    }

    /// A key while the card has the keyboard: Escape refuses, Enter and
    /// Space press the button with the keyboard (Allow first), Tab moves
    /// it.
    pub fn key(&mut self, key: Key) -> Option<Answer> {
        if key == Key::Escape {
            return self.card.map(|_| Answer::Refuse);
        }
        let events = self.tree.input(Input::Key(key));
        events.into_iter().find_map(|event| match event {
            Event::Activated(id) if Some(id) == self.allow => Some(Answer::Allow),
            Event::Activated(id) if Some(id) == self.refuse => Some(Answer::Refuse),
            Event::Activated(id) if Some(id) == self.take_over => Some(Answer::TakeOver),
            _ => None,
        })
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn showing(confirm: bool) -> Showing {
        Showing {
            pointer: true,
            label: "helper · Clicks in Notes".into(),
            ring: true,
            confirm: confirm.then(|| Confirm {
                title: Lang::En.agent_will("helper", false),
                intent: "Deletes every note".into(),
                why: Lang::En.sensitive_app("Passwords"),
                keys: false,
            }),
        }
    }

    #[test]
    fn an_agent_keeps_its_colour() {
        let theme = Theme::default();
        assert_eq!(agent_color(&theme, "helper"), agent_color(&theme, "helper"));
        let all: std::collections::HashSet<_> = ["helper", "tests-e2e", "refacto-ipc", "docs"]
            .iter()
            .map(|n| {
                let c = agent_color(&theme, n);
                (c.r, c.g, c.b)
            })
            .collect();
        assert!(all.len() > 1, "{all:?}");
    }

    #[test]
    fn the_view_fits_its_parts_within_the_screen() {
        let room = Size::new(1280.0, 800.0);
        let mut view = SeatView::new(
            agent_theme(&Theme::default(), "helper"),
            Lang::En,
            1.0,
            room,
            Point::new(400.0, 300.0),
            &showing(false),
        );
        view.draw(Duration::ZERO);
        let o = view.origin();
        assert!(o.x < 400.0 - RING && o.y < 300.0 - RING, "{o:?}");
        assert!(view.card_rect().is_none());
        assert!(view.access().dump().contains("helper · Clicks in Notes"));
    }

    #[test]
    fn the_card_answers_and_stays_on_the_screen() {
        let room = Size::new(1280.0, 800.0);
        // Near the bottom right: the card goes above, inside the screen.
        let mut view = SeatView::new(
            agent_theme(&Theme::default(), "helper"),
            Lang::En,
            1.0,
            room,
            Point::new(1250.0, 760.0),
            &showing(true),
        );
        view.draw(Duration::ZERO);
        let card = view.card_rect().expect("a card");
        let o = view.origin();
        assert!(o.x + card.right() <= room.w, "{o:?} {card:?}");
        assert!(o.y + card.y + card.h <= 760.0, "{o:?} {card:?}");
        let dump = view.access().dump();
        assert!(
            dump.contains("HELPER WILL CLICK · SENSITIVE ACTION"),
            "{dump}"
        );
        let line = dump
            .lines()
            .find(|l| l.contains("\"Allow\""))
            .unwrap_or_else(|| panic!("{dump}"));
        let xy: Vec<f32> = line
            .split_whitespace()
            .find(|w| w.contains(',') && w.chars().next().is_some_and(|c| c.is_ascii_digit()))
            .map(|w| w.split(',').filter_map(|n| n.parse().ok()).collect())
            .unwrap_or_default();
        view.pointer_move(Point::new(xy[0] + 8.0, xy[1] + 8.0));
        view.pointer_button(true);
        assert_eq!(view.pointer_button(false), Some(Answer::Allow));
    }

    #[test]
    fn the_clicked_card_answers_the_keyboard() {
        let room = Size::new(1280.0, 800.0);
        let mut showing = showing(true);
        let make = |showing: &Showing| {
            SeatView::new(
                agent_theme(&Theme::default(), "helper"),
                Lang::En,
                1.0,
                room,
                Point::new(400.0, 300.0),
                showing,
            )
        };
        // Not clicked: Enter presses nothing.
        let mut view = make(&showing);
        assert_eq!(view.key(Key::Enter), None);
        // Clicked: Enter allows, Tab then Enter refuses, Escape refuses.
        if let Some(c) = showing.confirm.as_mut() {
            c.keys = true;
        }
        assert_eq!(make(&showing).key(Key::Enter), Some(Answer::Allow));
        let mut view = make(&showing);
        assert_eq!(view.key(Key::Tab), None);
        assert_eq!(view.key(Key::Enter), Some(Answer::Refuse));
        assert_eq!(make(&showing).key(Key::Escape), Some(Answer::Refuse));
    }

    #[test]
    fn unseen_the_card_shows_alone() {
        let room = Size::new(1280.0, 800.0);
        let mut showing = showing(true);
        showing.pointer = false;
        let mut view = SeatView::new(
            agent_theme(&Theme::default(), "helper"),
            Lang::En,
            1.0,
            room,
            Point::new(400.0, 300.0),
            &showing,
        );
        view.draw(Duration::ZERO);
        let dump = view.access().dump();
        assert!(!dump.contains("helper · Clicks in Notes"), "{dump}");
        assert!(view.card_rect().is_some());
        assert_eq!(
            Lang::Fr.sensitive_unseen("Dans un champ de mot de passe", "Bureau 2"),
            "Dans un champ de mot de passe · Bureau 2, non affiché"
        );
    }
}
