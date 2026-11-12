//! The agents' journal (docs/design/mcp.md, scene 4): "Ce que font les
//! agents", from a click on an MCP agent's row in the bar (or the toast's
//! "Voir"). Its head, "Tout arrêter" on its right ("Reprendre" while the
//! agents are stopped), the day's line with the stop key under it; then
//! the last requests and refusals, newest first: a dot in the agent's
//! violet (red when refused), "Claude · rangé 3 fenêtres", where or why
//! under it, "Annuler" while the request can be undone, its time on the
//! right. 300 px wide, 12 px in, its lines 6 × 4 px in.
//!
//! Built with the shell's widget kit ([`crate::ui`]).

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use hocus_link::{McpLine, McpSaid, ToCompositor};
use iced::{
    Border, Color, Element, Length, Padding, Point,
    alignment::Vertical,
    widget::{Space, button, canvas::Path, column, container, row, space},
    window,
};

use super::{Effect, Item};
use crate::{
    levels::draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT},
    mcp::{Words, tag::AGENT},
    theme::Ink,
    ui::{Around, control, parts::drawn, style},
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 300;
const BORDER: f32 = 1.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const TITLE: f32 = 14.5;
const SUB: f32 = 11.5;
const SAID: f32 = 12.5;
const UNDER: f32 = 11.5;
const TIME: f32 = 11.5;
const DOT: f32 = 8.0;
const ENTRY_PAD: Padding = Padding {
    top: 6.0,
    right: 4.0,
    bottom: 6.0,
    left: 4.0,
};
/// The lines shown at most.
const MOST: usize = 8;
const STOP_TEXT: f32 = 11.5;
const STOP_RADIUS: f32 = 7.0;
const RED: Ink = Ink::Dark(Color::from_rgb8(0xF0, 0x7C, 0x7C));
const RED_FILL: Ink = Ink::Dark(Color::from_rgba8(240, 124, 124, 0.10));
const RED_LIT: Ink = Ink::Dark(Color::from_rgba8(240, 124, 124, 0.18));
const RED_RIM: Ink = Ink::Dark(Color::from_rgba8(240, 124, 124, 0.35));

fn head_height() -> f32 {
    (TITLE * LINE).max(STOP_TEXT * LINE + 6.0)
}

fn entry_height() -> f32 {
    ENTRY_PAD.top + (SAID + UNDER) * LINE + ENTRY_PAD.bottom
}

/// Its height for what it lists now.
pub fn height() -> u32 {
    let lines = crate::mcp::shown().journal.len().min(MOST);
    let list = if lines == 0 {
        SAID * LINE + GAP
    } else {
        lines as f32 * entry_height()
    };
    (BORDER + PAD + head_height() + 4.0 + SUB * LINE + GAP + list + PAD + BORDER).ceil() as u32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Stop,
    Undo(u64),
}

impl Target {
    fn id(self) -> String {
        format!("journal:{self:?}")
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Act(Target),
    Focus(Target),
    Pressed,
    Stale,
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

pub fn opening() {
    ui(|ui| ui.focus = None);
}

#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub glass: bool,
}

pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let words = Words::new(hocus_words::Lang::of_messages());
    Around::new(panel.id, build(words), handle, Msg::Stale)
        .rebuild(move || build(words))
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

fn build<'a>(words: Words) -> El<'a> {
    let shown = crate::mcp::shown();
    let focus = ui(|ui| ui.focus);
    tracing::debug!(
        lines = shown.journal.len(),
        stopped = shown.stopped,
        "the agents' journal shows it"
    );
    let stop_word = if shown.stopped {
        words.go_on()
    } else {
        words.stop_all()
    };
    let stop = button(style::said(
        stop_word.to_owned(),
        STOP_TEXT,
        MANROPE_BOLD,
        RED.get(),
    ))
    .padding([3.0, 8.0])
    .style(|_, status| {
        let fill = if style::lit(status) {
            RED_LIT
        } else {
            RED_FILL
        };
        let mut look = style::filled(fill.get(), STOP_RADIUS, RED.get());
        look.border = Border {
            color: RED_RIM.get(),
            width: 1.0,
            radius: STOP_RADIUS.into(),
        };
        look
    })
    .on_press(Msg::Act(Target::Stop));
    let stop = control(stop, Target::Stop.id(), Role::Button, stop_word)
        .focus(
            focus == Some(Target::Stop),
            Msg::Focus(Target::Stop),
            STOP_RADIUS,
        )
        .activate(Msg::Act(Target::Stop));
    let head = row![
        style::said(words.journal().to_owned(), TITLE, MANROPE_BOLD, TEXT.get()),
        space::horizontal(),
        stop,
    ]
    .align_y(Vertical::Center)
    .height(head_height());
    let sub = control(
        style::said(
            words.today(shown.stopped).to_owned(),
            SUB,
            MANROPE,
            FAINT.get(),
        ),
        "journal:today",
        Role::Label,
        words.today(shown.stopped),
    );
    let mut list = column![];
    if shown.journal.is_empty() {
        list = list.push(
            container(style::said(
                words.nothing_yet().to_owned(),
                SAID,
                MANROPE,
                FAINT.get(),
            ))
            .padding(Padding {
                top: GAP,
                ..Padding::ZERO
            }),
        );
    }
    for line in shown.journal.iter().take(MOST) {
        list = list.push(entry(words, line, focus));
    }
    container(column![
        head,
        Space::new().height(4.0),
        sub,
        Space::new().height(GAP),
        list
    ])
    .padding(BORDER + PAD)
    .width(Length::Fill)
    .into()
}

fn entry<'a>(words: Words, line: &McpLine, focus: Option<Target>) -> El<'a> {
    let (said, under, refused) = words.line(line);
    // Cut to the room the dot and the time leave.
    let time = crate::popover::kit::measure(&line.at, TIME, MANROPE);
    let room = WIDTH as f32
        - 2.0 * (BORDER + PAD)
        - ENTRY_PAD.left
        - ENTRY_PAD.right
        - DOT
        - 2.0 * GAP
        - time
        - 1.0;
    let said = crate::popover::kit::elide(&said, room, |t| {
        crate::popover::kit::measure(t, SAID, MANROPE)
    });
    let undoes = matches!(line.said, McpSaid::Did { undo: Some(_), .. });
    let under_room = if undoes {
        room - crate::popover::kit::measure(words.undo(), UNDER, MANROPE) - 12.0
    } else {
        room
    };
    let under = crate::popover::kit::elide(&under, under_room, |t| {
        crate::popover::kit::measure(t, UNDER, MANROPE)
    });
    let dot = drawn(DOT, DOT, move |f, size| {
        let colour = if refused { RED.get() } else { AGENT.get() };
        f.fill(
            &Path::circle(Point::new(size.width / 2.0, size.height / 2.0), DOT / 2.0),
            colour,
        );
    });
    let under_colour = if refused { RED.get() } else { FAINT.get() };
    let mut second =
        row![style::said(under.clone(), UNDER, MANROPE, under_colour)].align_y(Vertical::Center);
    if let McpSaid::Did {
        undo: Some(request),
        ..
    } = line.said
    {
        let target = Target::Undo(request);
        if !under.is_empty() {
            second = second.push(style::said(" · ".to_owned(), UNDER, MANROPE, FAINT.get()));
        }
        let undo = button(
            iced::widget::text(words.undo().to_owned())
                .size(UNDER)
                .font(MANROPE)
                .color(FAINT.get())
                .line_height(iced::widget::text::LineHeight::Relative(LINE)),
        )
        .padding(0)
        .style(|_, status| {
            let mut look = style::clear();
            look.text_color = if style::lit(status) {
                TEXT.get()
            } else {
                FAINT.get()
            };
            look
        })
        .on_press(Msg::Act(target));
        let undo = container(column![
            undo,
            container(Space::new().width(Length::Fill).height(1.0))
                .style(|_| style::boxed(FAINT.get(), 0.0))
        ])
        .width(Length::Shrink);
        second = second.push(
            control(undo, target.id(), Role::Button, words.undo())
                .focus(focus == Some(target), Msg::Focus(target), 4.0)
                .activate(Msg::Act(target)),
        );
    }
    let text = column![
        style::said(said.clone(), SAID, MANROPE, TEXT.get()),
        second.height(UNDER * LINE),
    ]
    .width(Length::Fill);
    let time = style::said(line.at.clone(), TIME, MANROPE, FAINT.get());
    let dot = container(dot).padding(Padding {
        top: (SAID * LINE - DOT) / 2.0,
        ..Padding::ZERO
    });
    let body = row![dot, text, time].spacing(GAP).align_y(Vertical::Top);
    control(
        container(body).padding(ENTRY_PAD).height(entry_height()),
        format!("journal:line:{}:{}", line.at, said),
        Role::ListItem,
        format!("{said}, {under}, {}", line.at),
    )
    .into()
}

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(Target::Stop) => {
            let stop = !crate::mcp::shown().stopped;
            tracing::info!(stop, "the agents' journal stops them, or lets them go on");
            super::send(vec![Effect::Tell(ToCompositor::McpStop(stop))]);
        }
        Msg::Act(Target::Undo(request)) => {
            tracing::info!(request, "the agents' journal undoes a request");
            super::send(vec![Effect::Tell(ToCompositor::McpUndo(request))]);
        }
    }
    super::changed(Item::Journal);
}
