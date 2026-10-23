//! Circle to ask (#46): a loop drawn around anything on screen, in the
//! accent, then the actions offered next to it (explain, search,
//! translate, copy the text, note, or a question typed) and, on the side,
//! the card an AI's answer streams into, the command field's own
//! ([`crate::answer`]). One sheet the size of the screen holds the three;
//! the compositor reads the circled text and runs what is chosen. What an
//! answer proposes (#197) comes first under it, confirmed before it runs.

use std::time::Duration;

use hocus_ui::{
    AccessNode, ButtonKind, Color, Direction, Event, Input, Insets, Key, Kind, Length, Look,
    NodeId, Paint, PixelRect, Point, Property, Rect, Size, Style, Surface, TextField, Theme, Tree,
    canvas::Canvas, tree::Draw,
};

use crate::{
    answer::{self, Answer, Block, Card, Source},
    column::View,
    i18n::Lang,
    proposals::{Offer, Run},
};

/// The loop's width, logical, as the board draws it.
const WIDTH: f32 = 5.0;
/// A drag smaller than this both ways circles nothing: a click.
const SMALLEST: f32 = 24.0;
/// Kept from the screen's edges.
const MARGIN: f32 = 24.0;
/// The menu's width, its padding included.
const MENU: f32 = 424.0;
/// Between the loop and the menu.
const MENU_GAP: f32 = 12.0;
/// The card's width.
const CARD: f32 = 464.0;
/// Between the loop (or the menu) and the card.
const CARD_GAP: f32 = 40.0;
/// Room in the card for all but the answer's text: the head, the buttons,
/// the padding and the gaps.
const CARD_CHROME: f32 = 220.0;

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// What the menu offers for the circled text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Explain,
    Search,
    Translate,
    Copy,
    Note,
}

impl Act {
    /// In the menu's order, the AI's left out when it is off.
    pub fn offered(ai: bool) -> Vec<Act> {
        [
            Self::Explain,
            Self::Search,
            Self::Translate,
            Self::Copy,
            Self::Note,
        ]
        .into_iter()
        .filter(|a| ai || !a.asks_ai())
        .collect()
    }

    pub fn asks_ai(self) -> bool {
        matches!(self, Self::Explain | Self::Translate)
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Explain => lang.pick("Explain", "Expliquer"),
            Self::Search => lang.pick("Search", "Chercher"),
            Self::Translate => lang.pick("Translate", "Traduire"),
            Self::Copy => lang.copy_text(),
            Self::Note => lang.pick("Note", "Noter"),
        }
    }
}

/// What an answer on the card answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    Explain,
    Translate,
    /// A question typed in the menu's field.
    Question(String),
}

impl Asked {
    /// What was asked, in words, for an agent the answer goes to (#197).
    pub fn question(&self, lang: Lang) -> String {
        match self {
            Self::Explain => lang
                .pick("Explain the circled text.", "Explique le texte entouré.")
                .into(),
            Self::Translate => lang
                .pick("Translate the circled text.", "Traduis le texte entouré.")
                .into(),
            Self::Question(q) => q.clone(),
        }
    }

    /// Whether its answer may propose actions: a translation is only that.
    pub fn proposes(&self) -> bool {
        !matches!(self, Self::Translate)
    }

    /// "Explain", as the card's caption starts.
    fn caption(&self, lang: Lang) -> &'static str {
        match self {
            Self::Explain => lang.pick("Explain", "Expliquer"),
            Self::Translate => lang.pick("Translate", "Traduire"),
            Self::Question(_) => lang.pick("Answer", "Réponse"),
        }
    }

    /// How the model is asked: briefly, in the user's language. Not shown.
    pub fn instructions(&self, lang: Lang) -> String {
        let language = lang.pick("English", "French");
        let read = "Someone circled part of their Linux desktop's screen; the text read \
                    in it by OCR (it may have small errors) follows";
        let brief = "briefly: one sentence first, then at most a few short list items or \
                     a short code block when they help. No headings, no tables.";
        match self {
            Self::Explain => format!(
                "{read}. Explain it in {language}, {brief} When it is an error, say what \
                 went wrong and how to fix it."
            ),
            Self::Translate => format!(
                "{read}. Translate it into {language}; when it is already in {language}, \
                 into English. Answer with the translation only."
            ),
            Self::Question(_) => format!(
                "{read} their question. Answer the question about that text in {language}, \
                 {brief}"
            ),
        }
    }

    /// What is sent with the instructions: the circled text, and the
    /// question typed.
    pub fn text(&self, circled: &str) -> String {
        match self {
            Self::Question(q) => format!("{q}\n\n---\n{circled}"),
            _ => circled.to_owned(),
        }
    }
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircleEvent {
    Act(Act),
    /// A question typed about the circled text.
    Ask(String),
    /// The card's buttons: the answer copied, or noted.
    CopyAnswer(String),
    NoteAnswer(String),
    StopAnswer,
    /// The answer asked again.
    Retry(Asked),
    /// An action the answer proposed, confirmed (#197).
    Run(Run),
    /// Escape, or a click elsewhere.
    Close,
}

/// A piece of the loop, from `a` to `b`, from its node's corner.
struct Piece {
    a: Point,
    b: Point,
    color: Color,
}

impl Draw for Piece {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, _theme: &Theme, _now: Duration) {
        let at = |p: Point| Point::new(rect.x + p.x, rect.y + p.y);
        canvas.polyline(&[at(self.a), at(self.b)], WIDTH, self.color);
    }

    fn size(&self) -> Size {
        Size::default()
    }
}

/// The menu, once the loop is closed.
struct Menu {
    node: NodeId,
    buttons: Vec<(NodeId, Act)>,
    field: Option<NodeId>,
    /// "Reading the text…", "No text found": said under the buttons.
    note: Option<NodeId>,
}

/// The card, once something was asked.
struct CardShown {
    node: NodeId,
    asked: Asked,
    answer: Answer,
    buttons: Vec<(NodeId, Press)>,
    /// What the answer proposes, once all there (#197).
    offers: Vec<Offer>,
    /// The proposal whose confirmation shows.
    confirming: Option<usize>,
    /// The answer's box and its text, to scroll a long one.
    text: Option<(NodeId, NodeId)>,
    tall: bool,
    scroll: f32,
}

/// What a card's button does.
#[derive(Debug, Clone)]
enum Press {
    Event(CircleEvent),
    /// The proposal at this index: its confirmation shows.
    Offer(usize),
    /// Back to the answer, nothing done.
    Back,
}

/// One screen's circle.
pub struct Circle {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    ai: bool,
    color: Color,
    pieces: Vec<NodeId>,
    /// Where the loop has been, from the screen's corner.
    bounds: Option<Rect>,
    last: Option<Point>,
    menu: Option<Menu>,
    card: Option<CardShown>,
    /// The card changed since it was laid out.
    card_changed: bool,
    /// The menu or the card moved or grew: placed again once laid out.
    place: bool,
}

impl std::fmt::Debug for Circle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Circle")
            .field("pieces", &self.pieces.len())
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

impl Circle {
    /// A clear sheet the size of a screen; `ai` when questions may be
    /// asked.
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, ai: bool) -> Self {
        let color = theme.colors.accent;
        let mut tree = Tree::new(theme, size, scale);
        let root = tree.root();
        tree.set_look(root, Look::default());
        let mut surface = Surface::new(size, scale);
        let damage = tree.update(Duration::ZERO);
        surface.draw(&mut tree, &damage);
        Self {
            tree,
            surface,
            lang,
            ai,
            color,
            pieces: Vec::new(),
            bounds: None,
            last: None,
            menu: None,
            card: None,
            card_changed: false,
            place: false,
        }
    }

    /// The loop goes on to `to`, from the screen's corner.
    pub fn line_to(&mut self, to: Point) {
        let from = self.last.unwrap_or(to);
        self.last = Some(to);
        // By hand: a union leaves out empty rectangles, and a point is one.
        self.bounds = Some(match self.bounds {
            None => Rect::new(to.x, to.y, 0.0, 0.0),
            Some(b) => {
                let (x, y) = (b.x.min(to.x), b.y.min(to.y));
                Rect::new(x, y, b.right().max(to.x) - x, b.bottom().max(to.y) - y)
            }
        });
        let pad = WIDTH;
        let x = from.x.min(to.x) - pad;
        let y = from.y.min(to.y) - pad;
        let w = (from.x - to.x).abs() + 2.0 * pad;
        let h = (from.y - to.y).abs() + 2.0 * pad;
        let piece = Piece {
            a: Point::new(from.x - x, from.y - y),
            b: Point::new(to.x - x, to.y - y),
            color: self.color,
        };
        let root = self.tree.root();
        if let Some(id) = self.tree.add(
            root,
            Style {
                at: Some(Point::new(x, y)),
                width: Length::Px(w),
                height: Length::Px(h),
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(piece),
                label: None,
            },
        ) {
            self.pieces.push(id);
        }
    }

    /// What the loop goes around, from the screen's corner; `None` for a
    /// drag too small to circle anything.
    pub fn circled(&self) -> Option<Rect> {
        let b = self.bounds?;
        let size = self.tree.size();
        let x = b.x.max(0.0);
        let y = b.y.max(0.0);
        let r = Rect::new(x, y, b.right().min(size.w) - x, b.bottom().min(size.h) - y);
        (r.w >= SMALLEST || r.h >= SMALLEST).then_some(r)
    }

    /// The loop is closed: the menu shows next to it.
    pub fn open_menu(&mut self) {
        if self.menu.is_some() {
            return;
        }
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let root = self.tree.root();
        let node = self
            .tree
            .add(
                root,
                Style {
                    at: Some(Point::default()),
                    width: Length::Px(MENU),
                    padding: Insets::all(12.0),
                    gap: 8.0,
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Glass),
                    radius: 18.0,
                    border: Some(Paint::Border),
                    shadow: 20.0,
                }),
            )
            .unwrap_or_else(MISSING);
        // Rows as the board wraps them: three, then the rest.
        let acts = Act::offered(self.ai);
        let mut buttons = Vec::new();
        for (i, row_acts) in acts.chunks(3).enumerate() {
            let row = self
                .tree
                .add(
                    node,
                    Style {
                        direction: Direction::Row,
                        gap: 8.0,
                        grow: 0.0,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(MISSING);
            for (j, act) in row_acts.iter().enumerate() {
                let kind = if i == 0 && j == 0 {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Plain
                };
                let id = self
                    .tree
                    .add(
                        row,
                        Style {
                            height: Length::Px(44.0),
                            padding: Insets::symmetric(0.0, 14.0),
                            grow: 0.0,
                            ..Style::default()
                        },
                        Kind::button(act.label(lang), kind, &theme),
                    )
                    .unwrap_or_else(MISSING);
                buttons.push((id, *act));
            }
        }
        let field = self.ai.then(|| {
            let field_box = self
                .tree
                .add(
                    node,
                    Style {
                        height: Length::Px(44.0),
                        padding: Insets::symmetric(0.0, 12.0),
                        justify: hocus_ui::Justify::Center,
                        grow: 0.0,
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Background),
                        radius: 12.0,
                        border: Some(Paint::Border),
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            self.tree
                .add(
                    field_box,
                    Style {
                        width: Length::Fill,
                        height: Length::Px(22.0),
                        grow: 0.0,
                        ..Style::default()
                    },
                    Kind::TextField(Box::new(TextField::new(
                        lang.pick(
                            "Ask something else about the circled area",
                            "Demander autre chose sur la zone entourée",
                        ),
                        lang.pick("Or ask something else…", "Ou demander autre chose…"),
                        theme.text.body.with_weight(500),
                    ))),
                )
                .unwrap_or_else(MISSING)
        });
        self.tree.set_focus(field);
        self.menu = Some(Menu {
            node,
            buttons,
            field,
            note: None,
        });
        self.place = true;
    }

    /// Says something under the menu's buttons (the text being read, none
    /// found), or nothing.
    pub fn set_note(&mut self, note: Option<&str>) {
        let style = self.tree.theme().text.label.with_color(Paint::Muted);
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        match (menu.note, note) {
            (Some(id), Some(note)) => self.tree.set_text(id, note),
            (None, Some(note)) => {
                menu.note = self.tree.add(
                    menu.node,
                    Style {
                        padding: Insets::symmetric(2.0, 4.0),
                        grow: 0.0,
                        ..Style::default()
                    },
                    Kind::text(note, style),
                );
            }
            (Some(id), None) => {
                self.tree.remove(id);
                menu.note = None;
            }
            (None, None) => {}
        }
        self.place = true;
    }

    /// An answer starts on the card, from `source`.
    pub fn ask(&mut self, asked: Asked, source: Source) {
        let question = match &asked {
            Asked::Question(q) => q.clone(),
            _ => String::new(),
        };
        let answer = Answer::new(question, source);
        if let Some(card) = self.card.take() {
            self.tree.remove(card.node);
        }
        let root = self.tree.root();
        let node = self
            .tree
            .add(
                root,
                Style {
                    at: Some(Point::default()),
                    width: Length::Px(CARD),
                    padding: Insets::all(24.0),
                    gap: 16.0,
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Glass),
                    radius: 20.0,
                    border: Some(Paint::Rgba(self.color.with_alpha(102))),
                    shadow: 20.0,
                }),
            )
            .unwrap_or_else(MISSING);
        self.card = Some(CardShown {
            node,
            asked,
            answer,
            buttons: Vec::new(),
            offers: Vec::new(),
            confirming: None,
            text: None,
            tall: false,
            scroll: 0.0,
        });
        self.card_changed = true;
        self.place = true;
    }

    /// More of the answer, or its end.
    pub fn answer_news(&mut self, news: answer::News) {
        if let Some(card) = self.card.as_mut() {
            card.answer.apply(news);
            self.card_changed = true;
        }
    }

    /// What the answer proposes, once all there, as the desktop can
    /// offer it.
    pub fn set_offers(&mut self, offers: Vec<Offer>) {
        if let Some(card) = self.card.as_mut() {
            card.offers = offers;
            card.confirming = None;
            self.card_changed = true;
        }
    }

    /// The answer on the card, and what it answers.
    pub fn answer(&self) -> Option<(&Asked, &Answer)> {
        self.card.as_ref().map(|c| (&c.asked, &c.answer))
    }

    /// Whether an answer streams in.
    pub fn streaming(&self) -> bool {
        self.card
            .as_ref()
            .is_some_and(|c| c.answer.state == answer::State::Streaming)
    }

    /// The card's content again, as the answer stands.
    fn render_card(&mut self) {
        let lang = self.lang;
        let Some(card) = self.card.as_mut() else {
            return;
        };
        for child in self.tree.children(card.node) {
            self.tree.remove(child);
        }
        let a = &card.answer;
        let confirming = card.confirming.and_then(|i| card.offers.get(i));
        let (shown, presses) = match confirming {
            Some(offer) => confirmation(offer, lang),
            None => {
                let mut blocks = answer::blocks(a.shown());
                if blocks.is_empty() && a.state == answer::State::Streaming {
                    blocks.push(Block::Paragraph(lang.thinking().into()));
                }
                let plain = a.plain();
                let (error, buttons, presses) = match &a.state {
                    answer::State::Streaming => (
                        None,
                        vec![lang.stop().to_owned()],
                        vec![Press::Event(CircleEvent::StopAnswer)],
                    ),
                    // What it proposes first, the main one first (#197),
                    // then the note and Copy, as board D05 orders them.
                    answer::State::Done(_) => (
                        None,
                        card.offers
                            .iter()
                            .map(|o| o.label.clone())
                            .chain([lang.add_to_a_note().to_owned(), lang.copy().to_owned()])
                            .collect(),
                        (0..card.offers.len())
                            .map(Press::Offer)
                            .chain([
                                Press::Event(CircleEvent::NoteAnswer(plain.clone())),
                                Press::Event(CircleEvent::CopyAnswer(plain)),
                            ])
                            .collect(),
                    ),
                    answer::State::Failed(why) => (
                        Some(why.clone()),
                        vec![lang.ask_again().to_owned()],
                        vec![Press::Event(CircleEvent::Retry(card.asked.clone()))],
                    ),
                };
                let card = Card {
                    mood: a.mood(),
                    caption: format!("{} · {}", card.asked.caption(lang), a.source.label(lang)),
                    blocks,
                    chips: Vec::new(),
                    error,
                    buttons,
                    width: CARD - 48.0,
                    compact: true,
                };
                (card, presses)
            }
        };
        let filled = answer::fill(&mut self.tree, card.node, &shown);
        card.buttons = filled.buttons.into_iter().zip(presses).collect();
        card.text = Some((filled.text, filled.content));
        if card.tall {
            let room = self.text_room();
            if let Some(card) = self.card.as_mut() {
                self.tree.set_style(filled.text, answer::text_box(room));
                self.tree
                    .jump(filled.content, Property::OffsetY, -card.scroll);
            }
        }
        self.place = true;
    }

    /// How high the answer's text may be for the card to fit the screen.
    fn text_room(&self) -> f32 {
        (self.tree.size().h - 2.0 * MARGIN - CARD_CHROME).max(80.0)
    }

    /// The menu under the loop, its right edge on the loop's (above it
    /// when there is no room below); the card beside them both.
    fn place(&mut self) {
        let Some(circled) = self.bounds else {
            return;
        };
        let screen = self.tree.size();
        let clamp = |v: f32, size: f32, room: f32| v.min(room - MARGIN - size).max(MARGIN);
        let mut menu_rect = None;
        if let Some(menu) = &self.menu
            && let Some(r) = self.tree.rect(menu.node)
        {
            let x = clamp(circled.right() - MENU, MENU, screen.w);
            let below = circled.bottom() + MENU_GAP;
            let above = circled.y - MENU_GAP - r.h;
            let y = if below + r.h <= screen.h - MARGIN {
                below
            } else if above >= MARGIN {
                above
            } else {
                clamp(below, r.h, screen.h)
            };
            self.tree.set_style(menu.node, menu_style(Point::new(x, y)));
            menu_rect = Some(Rect::new(x, y, MENU, r.h));
        }
        if let Some(card) = &self.card
            && let Some(r) = self.tree.rect(card.node)
        {
            let around = menu_rect.map_or(circled, |m| m.union(&circled));
            let x = if around.right() + CARD_GAP + CARD <= screen.w - MARGIN {
                around.right() + CARD_GAP
            } else if around.x - CARD_GAP - CARD >= MARGIN {
                around.x - CARD_GAP - CARD
            } else {
                screen.w - MARGIN - CARD
            };
            let y = clamp(circled.y, r.h, screen.h);
            self.tree.set_style(card.node, card_style(Point::new(x, y)));
        }
    }

    /// Typed text goes to the field.
    pub fn text(&mut self, text: &str) {
        if let Some(field) = self.menu.as_ref().and_then(|m| m.field) {
            self.tree.set_focus(Some(field));
            self.tree.text_input(text);
        }
    }

    pub fn key(&mut self, key: Key) -> Option<CircleEvent> {
        match key {
            Key::Escape => Some(CircleEvent::Close),
            key => {
                let events = self.tree.input(Input::Key(key));
                self.handle(events)
            }
        }
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.tree.input(Input::PointerMove(at));
    }

    /// The primary button: on the menu or the card, what it hits; anywhere
    /// else, a press closes it all.
    pub fn pointer_button(&mut self, at: Point, pressed: bool) -> Option<CircleEvent> {
        if pressed && !self.hit(at) {
            return Some(CircleEvent::Close);
        }
        let events = self.tree.input(Input::PointerButton { pressed });
        let chosen = self.handle(events);
        // The keyboard stays in the field.
        if let Some(field) = self.menu.as_ref().and_then(|m| m.field) {
            self.tree.set_focus(Some(field));
        }
        chosen
    }

    /// Over the menu or the card.
    pub fn hit(&self, at: Point) -> bool {
        let menu = self.menu.as_ref().map(|m| m.node);
        let card = self.card.as_ref().map(|c| c.node);
        [menu, card]
            .into_iter()
            .flatten()
            .filter_map(|n| self.tree.rect(n))
            .any(|r| r.contains(at))
    }

    /// The wheel over the card scrolls a long answer.
    pub fn wheel(&mut self, at: Point, dy: f32) -> bool {
        let room = self.text_room();
        let Some(card) = self.card.as_mut() else {
            return false;
        };
        let Some((_, content)) = card.text.filter(|_| card.tall) else {
            return false;
        };
        if !self.tree.rect(card.node).is_some_and(|r| r.contains(at)) {
            return false;
        }
        let Some(inner) = self.tree.rect(content) else {
            return false;
        };
        let scroll = (card.scroll + dy).clamp(0.0, (inner.h - room).max(0.0));
        if scroll != card.scroll {
            card.scroll = scroll;
            self.tree.jump(content, Property::OffsetY, -scroll);
        }
        true
    }

    fn handle(&mut self, events: Vec<Event>) -> Option<CircleEvent> {
        for event in events {
            let Event::Activated(id) = event else {
                continue;
            };
            if let Some(menu) = &self.menu {
                if let Some((_, act)) = menu.buttons.iter().find(|(b, _)| *b == id) {
                    return Some(CircleEvent::Act(*act));
                }
                if menu.field == Some(id) {
                    let typed = self.tree.field_value(id).unwrap_or_default().trim();
                    if typed.is_empty() {
                        return None;
                    }
                    let typed = typed.to_owned();
                    self.tree.set_field_value(id, "");
                    return Some(CircleEvent::Ask(typed));
                }
            }
            if let Some(card) = self.card.as_mut()
                && let Some((_, press)) = card.buttons.iter().find(|(b, _)| *b == id)
            {
                match press.clone() {
                    Press::Event(event) => return Some(event),
                    Press::Offer(i) => card.confirming = Some(i),
                    Press::Back => card.confirming = None,
                }
                card.tall = false;
                card.scroll = 0.0;
                self.card_changed = true;
                return None;
            }
        }
        None
    }

    /// What changed since the last time, in pixels.
    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        if self.card_changed {
            self.card_changed = false;
            self.render_card();
        }
        let mut damage = self.tree.update(now);
        let room = self.text_room();
        if let Some(card) = self.card.as_mut()
            && let Some((text, content)) = card.text
            && !card.tall
            && self.tree.rect(content).is_some_and(|r| r.h > room)
        {
            // Too long for the screen: it scrolls in a box of its own.
            card.tall = true;
            self.tree.set_style(text, answer::text_box(room));
            self.place = true;
            damage.extend(self.tree.update(now));
        }
        if std::mem::take(&mut self.place) {
            self.place();
            damage.extend(self.tree.update(now));
        }
        self.surface.draw(&mut self.tree, &damage)
    }

    /// Its see-through panels, from its corner, for the compositor to blur
    /// what is behind them (#37).
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
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

/// What a proposal's confirmation shows (#197): what it will do, the
/// diff or the text it sends, then its buttons and Cancel, which goes back
/// to the answer.
fn confirmation(offer: &Offer, lang: Lang) -> (Card, Vec<Press>) {
    let mut blocks = vec![Block::Paragraph(offer.title.clone())];
    blocks.extend(offer.detail.clone().map(Block::Code));
    let buttons = offer
        .confirms
        .iter()
        .map(|(label, _)| label.clone())
        .chain([lang.cancel().to_owned()])
        .collect();
    let presses = offer
        .confirms
        .iter()
        .map(|(_, run)| Press::Event(CircleEvent::Run(run.clone())))
        .chain([Press::Back])
        .collect();
    let card = Card {
        mood: crate::panda::Mood::Wait,
        caption: lang.intent_caption().into(),
        blocks,
        chips: offer.chips.clone(),
        error: None,
        buttons,
        width: CARD - 48.0,
        compact: true,
    };
    (card, presses)
}

fn menu_style(at: Point) -> Style {
    Style {
        at: Some(at),
        width: Length::Px(MENU),
        padding: Insets::all(12.0),
        gap: 8.0,
        grow: 0.0,
        ..Style::default()
    }
}

fn card_style(at: Point) -> Style {
    Style {
        at: Some(at),
        width: Length::Px(CARD),
        padding: Insets::all(24.0),
        gap: 16.0,
        grow: 0.0,
        ..Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(ai: bool) -> Circle {
        drawn_in(Lang::En, ai)
    }

    fn drawn_in(lang: Lang, ai: bool) -> Circle {
        let mut circle = Circle::new(Theme::default(), lang, Size::new(1280.0, 800.0), 1.0, ai);
        for (x, y) in [
            (120.0, 260.0),
            (400.0, 190.0),
            (640.0, 250.0),
            (380.0, 340.0),
            (130.0, 250.0),
        ] {
            circle.line_to(Point::new(x, y));
        }
        circle.open_menu();
        circle.draw(Duration::ZERO);
        circle
    }

    fn rect_of(circle: &Circle, label: &str) -> Option<Rect> {
        fn find(node: &AccessNode, label: &str) -> Option<Rect> {
            if node.label.as_deref() == Some(label) {
                return Some(node.rect);
            }
            node.children.iter().find_map(|c| find(c, label))
        }
        find(&circle.access(), label)
    }

    #[test]
    fn a_click_circles_nothing() {
        let mut circle = Circle::new(
            Theme::default(),
            Lang::En,
            Size::new(800.0, 600.0),
            1.0,
            false,
        );
        circle.line_to(Point::new(100.0, 100.0));
        circle.line_to(Point::new(104.0, 103.0));
        assert_eq!(circle.circled(), None);
        circle.line_to(Point::new(160.0, 130.0));
        assert_eq!(circle.circled(), Some(Rect::new(100.0, 100.0, 60.0, 30.0)));
    }

    #[test]
    fn the_menu_sits_under_the_loop_and_offers_the_ai_only_when_on() {
        let circle = drawn(true);
        let dump = circle.access().dump();
        for label in ["Explain", "Search", "Translate", "Copy the text", "Note"] {
            assert!(dump.contains(label), "{label} in {dump}");
        }
        let explain = rect_of(&circle, "Explain").unwrap_or_default();
        assert!(explain.y >= 340.0 + MENU_GAP, "{explain:?}");
        let note = rect_of(&circle, "Note").unwrap_or_default();
        assert!(note.right() <= 640.0, "right edge on the loop's: {note:?}");

        let circle = drawn(false);
        let dump = circle.access().dump();
        assert!(
            !dump.contains("Explain") && !dump.contains("Translate"),
            "{dump}"
        );
        assert!(!dump.contains("Or ask something else"), "{dump}");
        assert!(dump.contains("Copy the text"), "{dump}");
    }

    #[test]
    fn it_speaks_french_too() {
        let mut circle = drawn_in(Lang::Fr, true);
        circle.set_note(Some(Lang::Fr.reading_text()));
        circle.ask(Asked::Translate, Source::Local);
        circle.draw(Duration::ZERO);
        let dump = circle.access().dump();
        for label in [
            "Expliquer",
            "Chercher",
            "Traduire",
            "Copier le texte",
            "Noter",
            "Demander autre chose sur la zone entourée",
            "Lecture du texte…",
            "TRADUIRE · IA LOCALE",
            "Réflexion…",
        ] {
            assert!(dump.contains(label), "{label} in {dump}");
        }
        assert!(
            Asked::Translate
                .instructions(Lang::Fr)
                .contains("into French")
        );
    }

    #[test]
    fn escape_or_a_click_elsewhere_closes_and_a_button_acts() {
        let mut circle = drawn(true);
        assert_eq!(circle.key(Key::Escape), Some(CircleEvent::Close));
        assert_eq!(
            circle.pointer_button(Point::new(20.0, 20.0), true),
            Some(CircleEvent::Close)
        );
        let copy = rect_of(&circle, "Copy the text").unwrap_or_default();
        let at = Point::new(copy.x + 4.0, copy.y + 4.0);
        circle.pointer_move(at);
        assert_eq!(circle.pointer_button(at, true), None);
        assert_eq!(
            circle.pointer_button(at, false),
            Some(CircleEvent::Act(Act::Copy))
        );
        circle.text("what is it?");
        assert_eq!(
            circle.key(Key::Enter),
            Some(CircleEvent::Ask("what is it?".into()))
        );
    }

    #[test]
    fn the_answer_streams_into_a_card_beside_the_loop() {
        let mut circle = drawn(true);
        circle.ask(Asked::Explain, Source::Local);
        circle.answer_news(answer::News::Text("It is a **test**.".into()));
        circle.draw(Duration::ZERO);
        let dump = circle.access().dump();
        assert!(dump.contains("EXPLAIN · LOCAL AI"), "{dump}");
        assert!(dump.contains("It is a test."), "{dump}");
        let text = rect_of(&circle, "It is a test.").unwrap_or_default();
        let menu = rect_of(&circle, "Note").unwrap_or_default();
        assert!(text.x > menu.right(), "beside: {text:?} {menu:?}");
        circle.answer_news(answer::News::Done(Duration::from_secs(1)));
        circle.draw(Duration::ZERO);
        let copy = rect_of(&circle, "Copy").unwrap_or_default();
        let at = Point::new(copy.x + 4.0, copy.y + 4.0);
        circle.pointer_move(at);
        circle.pointer_button(at, true);
        assert_eq!(
            circle.pointer_button(at, false),
            Some(CircleEvent::CopyAnswer("It is a test.".into()))
        );
    }
}
