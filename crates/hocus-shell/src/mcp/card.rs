//! The card of an MCP agent's action waiting for the user
//! (docs/design/mcp.md, scene 3): beside the point it aims at, its head
//! ("Claude veut fermer « Courses »", "À confirmer" on its right), the
//! agent's intent quoted on the violet, why it asks, its buttons
//! (Autoriser or « Fermer quand même » on the amber, Refuser, « Je prends
//! la main »), and « Sans réponse dans 60 s, c'est refusé. » A layer
//! surface of its own ([`hocus_link::MCP_CARD_NAMESPACE`]) built with the
//! shell's kit: it takes the pointer, and the keyboard only once clicked
//! (on demand), then Enter allows and Escape refuses.
//!
//! The mockup's `.card`: 300 px wide, 12 px in, 9 px between its parts,
//! radius 14, the bar's ink rimmed in amber at 45 %; its head 13.5 px bold,
//! "À confirmer" 11 px bold amber; the quote 12.5 px, 7 × 9 px in, its left
//! edge 3 px of violet; the text 12.5 px; the buttons `.btn` 12.5 px bold,
//! 6 × 10 px in, 6 px apart, wrapping; the last line 11 px.

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use hocus_link::{McpCard, McpReply, ToCompositor};
use iced::{
    Background, Border, Color, Element, Length, Padding,
    alignment::Vertical,
    keyboard::{Key, key::Named},
    widget::{Space, button, column, container, row, space},
    window,
};

use super::{Words, tag::AGENT};
use crate::{
    Message,
    levels::draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT},
    popover::{self, Effect, kit},
    theme::{DIM, Ink},
    ui::{Around, control, style},
};

type El<'a> = Element<'a, Msg>;

const WIDTH: f32 = 300.0;
const PAD: f32 = 12.0;
const GAP: f32 = 9.0;
const RADIUS: f32 = 14.0;
const HEAD: f32 = 13.5;
const MARK: f32 = 11.0;
const SAID: f32 = 12.5;
const NOTE: f32 = 11.0;
const QUOTE_PAD: Padding = Padding {
    top: 7.0,
    right: 9.0,
    bottom: 7.0,
    left: 9.0,
};
const QUOTE_EDGE: f32 = 3.0;
const BUTTON: f32 = 12.5;
const BUTTON_PAD: Padding = Padding {
    top: 6.0,
    right: 10.0,
    bottom: 6.0,
    left: 10.0,
};
const BUTTON_GAP: f32 = 6.0;
const BUTTON_RADIUS: f32 = 8.0;
/// Where it sits from the point (the mockup's card from its cursor).
const BESIDE: (i32, i32) = (20, 29);
const FILL: Ink = Ink::Dark(Color::from_rgb8(0x16, 0x1B, 0x2A));
const AMBER: Ink = Ink::Dark(Color::from_rgb8(0xF5, 0xC4, 0x6E));
const AMBER_LIT: Ink = Ink::Dark(Color::from_rgb8(0xF8, 0xD3, 0x8E));
const ON_AMBER: Ink = Ink::Dark(Color::from_rgb8(0x2A, 0x1D, 0x05));
const RIM: Ink = Ink::Dark(Color::from_rgba8(245, 196, 110, 0.45));
const ROW: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const BUTTON_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.09));
const BUTTON_LIT: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.15));
const BUTTON_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.07));

/// Its buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Allow,
    Refuse,
    TakeOver,
}

impl Target {
    fn id(self) -> String {
        format!("mcp-card:{self:?}")
    }

    fn reply(self) -> McpReply {
        match self {
            Self::Allow => McpReply::Allow,
            Self::Refuse => McpReply::Refuse,
            Self::TakeOver => McpReply::TakeOver,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Act(Target),
    Focus(Target),
    /// Clicked: it has the keyboard, Allow its ring.
    Pressed,
    Stale,
}

/// The card's keyboard ring, and which card it is on.
static FOCUS: Mutex<Option<(u64, Target)>> = Mutex::new(None);

fn focus(card: u64) -> Option<Target> {
    FOCUS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .filter(|(c, _)| *c == card)
        .map(|(_, t)| t)
}

fn set_focus(card: u64, target: Target) {
    *FOCUS.lock().unwrap_or_else(PoisonError::into_inner) = Some((card, target));
}

fn button_width(word: &str) -> f32 {
    kit::measure(word, BUTTON, MANROPE_BOLD) + BUTTON_PAD.left + BUTTON_PAD.right + 2.0
}

/// The room inside it.
fn room() -> f32 {
    WIDTH - 2.0 * (PAD + 1.0)
}

/// What it lays out: its head cut to its room, the quote's and the why's
/// lines, its buttons in rows.
struct Laid {
    head: String,
    quote: Vec<String>,
    why: Vec<String>,
    rows: Vec<Vec<Target>>,
}

fn lay(words: Words, card: &McpCard) -> Laid {
    let mark = kit::measure(words.to_confirm(), MARK, MANROPE_BOLD) + 8.0;
    let head = kit::elide(&words.wants(card), room() - mark, |t| {
        kit::measure(t, HEAD, MANROPE_BOLD)
    });
    let quote = kit::wrap(
        &card.intent,
        room() - QUOTE_PAD.left - QUOTE_PAD.right - QUOTE_EDGE,
        |t| kit::measure(t, SAID, MANROPE),
    );
    let why = kit::wrap(&words.why_card(card), room(), |t| {
        kit::measure(t, SAID, MANROPE)
    });
    let mut rows: Vec<Vec<Target>> = vec![Vec::new()];
    let mut used = 0.0;
    for target in [Target::Allow, Target::Refuse, Target::TakeOver] {
        let w = button_width(word(words, card, target));
        if used > 0.0 && used + BUTTON_GAP + w > room() {
            rows.push(Vec::new());
            used = 0.0;
        }
        used += if used > 0.0 { BUTTON_GAP } else { 0.0 } + w;
        if let Some(last) = rows.last_mut() {
            last.push(target);
        }
    }
    Laid {
        head,
        quote,
        why,
        rows,
    }
}

fn word(words: Words, card: &McpCard, target: Target) -> &'static str {
    match target {
        Target::Allow => words.allow(card),
        Target::Refuse => words.refuse(),
        Target::TakeOver => words.take_over(),
    }
}

fn button_height() -> f32 {
    BUTTON * LINE + BUTTON_PAD.top + BUTTON_PAD.bottom + 2.0
}

/// Its size and its top left corner on its screen.
pub fn place(words: Words, card: &McpCard) -> ((u32, u32), (i32, i32)) {
    let laid = lay(words, card);
    let rows = laid.rows.len() as f32;
    let height = 2.0 * (PAD + 1.0)
        + HEAD * LINE
        + GAP
        + QUOTE_PAD.top
        + laid.quote.len() as f32 * SAID * LINE
        + QUOTE_PAD.bottom
        + GAP
        + laid.why.len() as f32 * SAID * LINE
        + GAP
        + rows * button_height()
        + (rows - 1.0).max(0.0) * BUTTON_GAP
        + GAP
        + NOTE * LINE;
    (
        (WIDTH as u32, height.ceil() as u32),
        (card.x + BESIDE.0, card.y + BESIDE.1),
    )
}

pub fn view<'a>(id: window::Id, card: McpCard) -> Element<'a, Message> {
    let words = Words::new(hocus_words::Lang::of_messages());
    let number = card.id;
    let back = container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_| {
        container::Style {
            background: Some(Background::Color(FILL.get())),
            border: Border {
                color: RIM.get(),
                width: 1.0,
                radius: RADIUS.into(),
            },
            ..container::Style::default()
        }
    });
    let built = card.clone();
    Around::new(
        id,
        build(words, &card, focus(number)),
        move |m| handle(number, m),
        Msg::Stale,
    )
    .own(hocus_link::MCP_CARD_NAMESPACE, move || {
        super::card_shows(number)
    })
    .keys(move |pressed| match &pressed.key {
        Key::Named(Named::Escape) => Some(Msg::Act(Target::Refuse)),
        // Enter on the button with the ring, else Allow.
        Key::Named(Named::Enter) => Some(Msg::Act(focus(number).unwrap_or(Target::Allow))),
        _ => None,
    })
    .press(Msg::Pressed)
    .rebuild(move || build(words, &built, focus(number)))
    .on(back)
}

fn build<'a>(words: Words, card: &McpCard, focus: Option<Target>) -> El<'a> {
    let laid = lay(words, card);
    let head = row![
        style::said(laid.head.clone(), HEAD, MANROPE_BOLD, TEXT.get()),
        space::horizontal(),
        style::said(
            words.to_confirm().to_uppercase(),
            MARK,
            MANROPE_BOLD,
            AMBER.get()
        ),
    ]
    .align_y(Vertical::Center)
    .height(HEAD * LINE);
    let mut quote_lines = column![];
    for line in &laid.quote {
        quote_lines = quote_lines.push(style::said(line.clone(), SAID, MANROPE, TEXT.get()));
    }
    let quote = row![
        container(Space::new().width(QUOTE_EDGE).height(Length::Fill))
            .style(|_| style::boxed(AGENT.get(), 0.0)),
        container(quote_lines)
            .padding(QUOTE_PAD)
            .width(Length::Fill),
    ]
    .height(QUOTE_PAD.top + laid.quote.len() as f32 * SAID * LINE + QUOTE_PAD.bottom);
    let quote = container(quote)
        .style(|_| style::boxed(ROW.get(), BUTTON_RADIUS))
        .clip(true);
    let mut why = column![];
    for line in &laid.why {
        why = why.push(style::said(line.clone(), SAID, MANROPE, DIM.get()));
    }
    let mut buttons = column![].spacing(BUTTON_GAP);
    for targets in &laid.rows {
        let mut line = row![].spacing(BUTTON_GAP).align_y(Vertical::Center);
        for target in targets {
            line = line.push(press(word(words, card, *target), *target, focus));
        }
        buttons = buttons.push(line);
    }
    let note = style::said(
        words.card_note(focus.is_some()).to_owned(),
        NOTE,
        MANROPE,
        FAINT.get(),
    );
    let said = format!("{}. {}", laid.head, words.why_card(card));
    container(control(
        column![head, quote, why, buttons, note].spacing(GAP),
        "mcp-card",
        Role::Dialog,
        said,
    ))
    .padding(PAD + 1.0)
    .width(Length::Fill)
    .into()
}

fn press<'a>(word: &str, target: Target, focus: Option<Target>) -> El<'a> {
    let warn = target == Target::Allow;
    let ink = if warn { ON_AMBER.get() } else { TEXT.get() };
    let pressed = button(style::said(word.to_owned(), BUTTON, MANROPE_BOLD, ink))
        .padding(BUTTON_PAD)
        .style(move |_, status| {
            let lit = style::lit(status);
            let fill = match (warn, lit) {
                (true, false) => AMBER,
                (true, true) => AMBER_LIT,
                (false, false) => BUTTON_FILL,
                (false, true) => BUTTON_LIT,
            };
            let mut look = style::filled(fill.get(), BUTTON_RADIUS, ink);
            look.border = Border {
                color: if warn {
                    Color::TRANSPARENT
                } else {
                    BUTTON_RIM.get()
                },
                width: 1.0,
                radius: BUTTON_RADIUS.into(),
            };
            look
        })
        .on_press(Msg::Act(target));
    control(pressed, target.id(), Role::Button, word)
        .focus(focus == Some(target), Msg::Focus(target), BUTTON_RADIUS)
        .activate(Msg::Act(target))
        .into()
}

fn handle(card: u64, message: Msg) {
    match message {
        Msg::Act(target) => {
            tracing::info!(card, ?target, "an MCP agent's card is answered");
            popover::send(vec![Effect::Tell(ToCompositor::McpReply {
                card,
                reply: target.reply(),
            })]);
        }
        Msg::Focus(target) => set_focus(card, target),
        Msg::Pressed => {
            if focus(card).is_none() {
                set_focus(card, Target::Allow);
            }
        }
        Msg::Stale => {}
    }
}
