//! The toast once an MCP agent's request settled (docs/design/mcp.md,
//! scene 2): the agent's mark, one sentence of what it did, the request's
//! first intent under it, "Annuler" when something of it can be undone,
//! and "Voir", which opens the journal. At the bottom left of the
//! screen's work area, for [`hocus_link::MCP_TOAST`] (the compositor ends
//! it); a layer surface of its own ([`hocus_link::MCP_TOAST_NAMESPACE`])
//! built with the shell's kit, which takes the pointer for its buttons.
//!
//! The mockup's `.toast`: 10 × 12 px in, 10 px between its parts, radius
//! 12, `#262C3C` (the theme's `toast`); its mark 22 px, its text 13 px over
//! 11.5 px; its buttons `.btn`, 12.5 px bold, 6 × 10 px, radius 8.

use hocus_a11y::Role;
use hocus_link::{McpToast, ToCompositor};
use iced::{
    Background, Border, Color, Element, Length, Padding, Point,
    alignment::Vertical,
    widget::{Space, button, column, container, row},
    window,
};

use super::{Words, tag::AGENT};
use crate::{
    Message,
    levels::draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT},
    popover::{self, Effect, Item, kit},
    theme::Ink,
    ui::{Around, control, parts::drawn, style},
};

type El<'a> = Element<'a, Msg>;

const PAD: Padding = Padding {
    top: 10.0,
    right: 12.0,
    bottom: 10.0,
    left: 12.0,
};
const GAP: f32 = 10.0;
const BUTTON_GAP: f32 = 6.0;
const MARK: f32 = 22.0;
const SAID: f32 = 13.0;
const UNDER: f32 = 11.5;
const BUTTON: f32 = 12.5;
const BUTTON_PAD: Padding = Padding {
    top: 6.0,
    right: 10.0,
    bottom: 6.0,
    left: 10.0,
};
const BUTTON_RADIUS: f32 = 8.0;
const RADIUS: f32 = 12.0;
/// Its text's room at most.
const TEXT_MOST: f32 = 420.0;
const FILL: Ink = Ink::Token(|t| t.toast);
const EDGE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const BUTTON_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.09));
const BUTTON_LIT: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.15));
const BUTTON_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.07));

/// Its buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Undo,
    See,
}

impl Target {
    fn id(self) -> String {
        format!("mcp-toast:{self:?}")
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Act(Target),
    Focus(Target),
    Stale,
}

/// Which of its buttons holds the keyboard's ring.
static FOCUS: std::sync::Mutex<Option<Target>> = std::sync::Mutex::new(None);

fn focus() -> Option<Target> {
    *FOCUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn button_width(word: &str) -> f32 {
    kit::measure(word, BUTTON, MANROPE_BOLD) + BUTTON_PAD.left + BUTTON_PAD.right + 2.0
}

/// What its text says, cut to its room.
fn texts(words: Words, toast: &McpToast) -> (String, String, f32) {
    let said = words.toast(toast);
    let under = words.toast_under(toast);
    let room = TEXT_MOST;
    let said = kit::elide(&said, room, |t| kit::measure(t, SAID, MANROPE));
    let under = kit::elide(&under, room, |t| kit::measure(t, UNDER, MANROPE));
    let width = kit::measure(&said, SAID, MANROPE).max(kit::measure(&under, UNDER, MANROPE));
    (said, under, width.ceil() + 1.0)
}

/// Its size and its top left corner on its screen.
pub fn place(words: Words, toast: &McpToast) -> ((u32, u32), (i32, i32)) {
    let (_, _, text) = texts(words, toast);
    let mut width = PAD.left + MARK + GAP + text + GAP + button_width(words.see()) + PAD.right;
    if toast.undo {
        width += button_width(words.undo()) + BUTTON_GAP;
    }
    let height = PAD.top + (SAID + UNDER) * LINE + PAD.bottom;
    let (w, h) = (width.ceil() as u32 + 2, height.ceil() as u32 + 2);
    ((w, h), (toast.left, toast.bottom - h as i32))
}

pub fn view<'a>(id: window::Id, toast: McpToast, _glass: bool) -> Element<'a, Message> {
    let words = Words::new(hocus_words::Lang::of_messages());
    let request = toast.request;
    let back = container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_| {
        container::Style {
            background: Some(Background::Color(FILL.get())),
            border: Border {
                color: EDGE.get(),
                width: 1.0,
                radius: RADIUS.into(),
            },
            ..container::Style::default()
        }
    });
    let built = toast.clone();
    Around::new(
        id,
        build(words, &toast, focus()),
        move |m| handle(request, m),
        Msg::Stale,
    )
    .own(hocus_link::MCP_TOAST_NAMESPACE, move || {
        super::toast_shows(request)
    })
    .rebuild(move || build(words, &built, focus()))
    .on(back)
}

fn build<'a>(words: Words, toast: &McpToast, focus: Option<Target>) -> El<'a> {
    let (said, under, text) = texts(words, toast);
    let letter = toast
        .agent
        .chars()
        .next()
        .map(String::from)
        .unwrap_or_default();
    let mark = drawn(MARK, MARK, move |f, size| {
        let agent = AGENT.get();
        let centre = Point::new(size.width / 2.0, size.height / 2.0);
        let circle = iced::widget::canvas::Path::circle(centre, MARK / 2.0 - 0.5);
        f.fill(&circle, Color { a: 0.16, ..agent });
        f.stroke(
            &circle,
            iced::widget::canvas::Stroke::default()
                .with_color(Color { a: 0.55, ..agent })
                .with_width(1.0),
        );
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(
                letter.clone(),
                centre,
                11.0,
                agent,
                iced::widget::text::Alignment::Center,
            ),
        ));
    });
    let lines = column![
        style::said(said.clone(), SAID, MANROPE, TEXT.get()),
        style::said(under, UNDER, MANROPE, FAINT.get()),
    ]
    .width(text);
    let mut line = row![mark, lines].spacing(GAP).align_y(Vertical::Center);
    let mut buttons = row![].spacing(BUTTON_GAP).align_y(Vertical::Center);
    if toast.undo {
        buttons = buttons.push(press(words.undo(), Target::Undo, focus));
    }
    buttons = buttons.push(press(words.see(), Target::See, focus));
    line = line.push(buttons);
    container(control(line, "mcp-toast", Role::Status, said))
        .padding(Padding {
            top: PAD.top + 1.0,
            right: PAD.right + 1.0,
            bottom: PAD.bottom + 1.0,
            left: PAD.left + 1.0,
        })
        .into()
}

fn press<'a>(word: &str, target: Target, focus: Option<Target>) -> El<'a> {
    let pressed = button(style::said(
        word.to_owned(),
        BUTTON,
        MANROPE_BOLD,
        TEXT.get(),
    ))
    .padding(BUTTON_PAD)
    .style(|_, status| {
        let fill = if style::lit(status) {
            BUTTON_LIT
        } else {
            BUTTON_FILL
        };
        let mut look = style::filled(fill.get(), BUTTON_RADIUS, TEXT.get());
        look.border = Border {
            color: BUTTON_RIM.get(),
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

fn handle(request: u64, message: Msg) {
    match message {
        Msg::Act(Target::Undo) => {
            tracing::info!(request, "the MCP toast's Annuler");
            popover::send(vec![Effect::Tell(ToCompositor::McpUndo(request))]);
        }
        Msg::Act(Target::See) => {
            tracing::info!(request, "the MCP toast's Voir");
            let output = super::shown().toast.map(|t| t.output).unwrap_or_default();
            popover::open_asked(&output, Item::Journal, Some(Item::Secretary));
        }
        Msg::Focus(target) => {
            *FOCUS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(target);
        }
        Msg::Stale => {}
    }
}
