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
//!
//! A drawing's toast (#825) says « Effacer » while the agent's shapes show,
//! in « Annuler »'s place; a refusal's toast (a `draw` without its right)
//! has its mark in red, « Permettre… », which opens Settings › Agir sur le
//! bureau, and « Voir ».

use hocus_a11y::Role;
use hocus_link::{McpRefusedToast, McpToast, ToCompositor};
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
/// The Settings' page where an agent's rights are (« Permettre… »).
const ACTING_PAGE: &str = "act-on-desktop";
/// A refusal's mark: the mockup's `.who.bad`.
const REFUSED: Ink = Ink::Dark(Color::from_rgb8(0xF0, 0x7C, 0x7C));

/// Its buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Undo,
    See,
    /// The agent's shapes fade (#825).
    Erase,
    /// Settings › Agir sur le bureau.
    Allow,
}

/// What a toast shows.
#[derive(Debug, Clone)]
struct Said {
    agent: String,
    said: String,
    under: String,
    refused: bool,
    buttons: Vec<(&'static str, Target)>,
}

impl Said {
    fn of(words: Words, toast: &McpToast, inked: &[String]) -> Self {
        let mut buttons = Vec::new();
        if toast.summary.drawn > 0 && inked.contains(&toast.agent) {
            buttons.push((words.erase(), Target::Erase));
        }
        if toast.undo {
            buttons.push((words.undo(), Target::Undo));
        }
        buttons.push((words.see(), Target::See));
        Self {
            agent: toast.agent.clone(),
            said: words.toast(toast),
            under: words.toast_under(toast),
            refused: false,
            buttons,
        }
    }

    fn refused(words: Words, toast: &McpRefusedToast) -> Self {
        Self {
            agent: toast.agent.clone(),
            said: words.refused_toast(toast),
            under: words.refused_under().to_owned(),
            refused: true,
            buttons: vec![
                (words.allow_it(), Target::Allow),
                (words.see(), Target::See),
            ],
        }
    }
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
fn texts(said: &Said) -> (String, String, f32) {
    let (said, under) = (said.said.clone(), said.under.clone());
    let room = TEXT_MOST;
    let said = kit::elide(&said, room, |t| kit::measure(t, SAID, MANROPE));
    let under = kit::elide(&under, room, |t| kit::measure(t, UNDER, MANROPE));
    let width = kit::measure(&said, SAID, MANROPE).max(kit::measure(&under, UNDER, MANROPE));
    (said, under, width.ceil() + 1.0)
}

/// Its size and its top left corner on its screen.
pub fn place(words: Words, toast: &McpToast, inked: &[String]) -> ((u32, u32), (i32, i32)) {
    size_at(&Said::of(words, toast, inked), toast.left, toast.bottom)
}

/// A refusal's toast's size and top left corner.
pub fn place_refused(words: Words, toast: &McpRefusedToast) -> ((u32, u32), (i32, i32)) {
    size_at(&Said::refused(words, toast), toast.left, toast.bottom)
}

fn size_at(said: &Said, left: i32, bottom: i32) -> ((u32, u32), (i32, i32)) {
    let (_, _, text) = texts(said);
    let buttons: f32 = said
        .buttons
        .iter()
        .map(|(word, _)| button_width(word))
        .sum::<f32>()
        + BUTTON_GAP * said.buttons.len().saturating_sub(1) as f32;
    let width = PAD.left + MARK + GAP + text + GAP + buttons + PAD.right;
    let height = PAD.top + (SAID + UNDER) * LINE + PAD.bottom;
    let (w, h) = (width.ceil() as u32 + 2, height.ceil() as u32 + 2);
    ((w, h), (left, bottom - h as i32))
}

pub fn view<'a>(
    id: window::Id,
    toast: McpToast,
    inked: Vec<String>,
    _glass: bool,
) -> Element<'a, Message> {
    let words = Words::new(hocus_words::Lang::of_messages());
    let request = toast.request;
    let said = Said::of(words, &toast, &inked);
    let agent = toast.agent.clone();
    surface(
        id,
        said,
        move |m| handle(request, &agent, m),
        move || super::toast_shows(request),
    )
}

/// A refusal's toast.
pub fn view_refused<'a>(id: window::Id, toast: McpRefusedToast) -> Element<'a, Message> {
    let words = Words::new(hocus_words::Lang::of_messages());
    let said = Said::refused(words, &toast);
    let (shown, agent) = (toast.id, toast.agent.clone());
    surface(
        id,
        said,
        move |m| handle(0, &agent, m),
        move || super::refused_shows(shown),
    )
}

fn surface<'a>(
    id: window::Id,
    said: Said,
    handle: impl Fn(Msg) + 'static,
    shows: impl Fn() -> bool + 'static,
) -> Element<'a, Message> {
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
    let built = said.clone();
    Around::new(id, build(&said, focus()), handle, Msg::Stale)
        .own(hocus_link::MCP_TOAST_NAMESPACE, shows)
        .rebuild(move || build(&built, focus()))
        .on(back)
}

fn build<'a>(shown: &Said, focus: Option<Target>) -> El<'a> {
    let (said, under, text) = texts(shown);
    let letter = shown
        .agent
        .chars()
        .next()
        .map(String::from)
        .unwrap_or_default();
    let refused = shown.refused;
    let mark = drawn(MARK, MARK, move |f, size| {
        let agent = if refused { REFUSED.get() } else { AGENT.get() };
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
    for (word, target) in &shown.buttons {
        buttons = buttons.push(press(word, *target, focus));
    }
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

fn handle(request: u64, agent: &str, message: Msg) {
    match message {
        Msg::Act(Target::Undo) => {
            tracing::info!(request, "the MCP toast's Annuler");
            popover::send(vec![Effect::Tell(ToCompositor::McpUndo(request))]);
        }
        Msg::Act(Target::Erase) => {
            tracing::info!(agent, "the MCP toast's Effacer");
            popover::send(vec![Effect::Tell(ToCompositor::McpErase(agent.to_owned()))]);
        }
        Msg::Act(Target::Allow) => {
            tracing::info!(agent, "the MCP refusal's Permettre");
            crate::popover::open_settings_page(ACTING_PAGE);
        }
        Msg::Act(Target::See) => {
            tracing::info!(request, "the MCP toast's Voir");
            let shown = super::shown();
            let output = shown
                .toast
                .map(|t| t.output)
                .or_else(|| shown.refused.map(|t| t.output))
                .unwrap_or_default();
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
