//! A notification's bubble on its surface, on the widget kit (#772): its
//! ✕ and its actions are the kit's buttons, placed where its layout puts
//! them, each drawing its look; the card under them takes a click (its
//! default action) and holds every bubble's countdown while the pointer
//! is on it. What nothing holds is drawn under them ([`draw::Back`]).

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Mutex, PoisonError},
};

use hocus_a11y::Role;
use hocus_link::ToCompositor;
use iced::{
    Element, Length, Point,
    widget::{Space, Stack, button, canvas, mouse_area, pin},
    window,
};

use super::{
    Input,
    draw::{self, Laid},
};
use crate::{
    Message,
    ui::{Around, control, parts::drawn_lit, style},
};

type El<'a> = Element<'a, Msg>;

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The pointer came onto the card, or left it.
    Hover(bool),
    /// The card clicked: its default action.
    Card,
    Close,
    Action(String),
    /// What it shows changed under the view.
    Stale,
}

/// The bubbles whose card is under the pointer: lit.
static LIT: Mutex<Vec<window::Id>> = Mutex::new(Vec::new());

/// Whether bubble `id`'s card is under the pointer.
pub fn lit(id: window::Id) -> bool {
    LIT.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&id)
}

/// Bubble `id` on its surface, on the glass (else opaque); `card`: on the
/// CPU, the surface is the card alone.
pub fn view<'a>(id: window::Id, glass: bool, card: bool) -> Element<'a, Message> {
    // What its controls were built from: a bubble updated in place builds
    // them again.
    let seen: Rc<RefCell<Option<Laid>>> = Rc::default();
    let (built, rebuilt, stale) = (seen.clone(), seen.clone(), seen);
    Around::new(
        id,
        build(id, card, &built),
        move |m| handle(id, m),
        Msg::Stale,
    )
    .own_by(move || wake(id), move || shown(id).is_some())
    .rebuild(move || build(id, card, &rebuilt))
    .stale_when(move || shown(id).map(|(_, laid)| laid) != *stale.borrow())
    .on(canvas(draw::Back { id, glass, card })
        .width(Length::Fill)
        .height(Length::Fill))
}

/// The serial and layout of the bubble on `id`.
fn shown(id: window::Id) -> Option<(u64, Laid)> {
    super::with(|c| c.stack.bubble(id).map(|b| (b.serial, b.laid.clone())))
}

/// Its surface draws again.
fn wake(id: window::Id) {
    if let Some(namespace) = super::with(|c| c.stack.bubble(id).map(|b| b.namespace())) {
        crate::popover::tell(ToCompositor::Wake(namespace));
    }
}

fn handle(id: window::Id, message: Msg) {
    let Some((serial, _)) = shown(id) else {
        return;
    };
    match message {
        Msg::Hover(on) => {
            let mut lit = LIT.lock().unwrap_or_else(PoisonError::into_inner);
            lit.retain(|l| *l != id);
            if on {
                lit.push(id);
            }
            drop(lit);
            super::input(Input::Hover { serial, on });
        }
        Msg::Card => super::input(Input::Card(serial)),
        Msg::Close => super::input(Input::Dismiss(serial)),
        Msg::Action(key) => super::input(Input::Action { serial, key }),
        Msg::Stale => {}
    }
}

/// `content` placed at `at`.
fn at<'a>(content: impl Into<El<'a>>, at: Point) -> El<'a> {
    pin(content).x(at.x).y(at.y).into()
}

/// The bubble's controls on its card, where its layout puts them.
fn build<'a>(id: window::Id, card: bool, seen: &RefCell<Option<Laid>>) -> El<'a> {
    let Some((note, laid)) =
        super::with(|c| c.stack.bubble(id).map(|b| (b.note.clone(), b.laid.clone())))
    else {
        // Gone: its card is no longer under the pointer.
        *seen.borrow_mut() = None;
        LIT.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|l| *l != id);
        return Space::new().into();
    };
    *seen.borrow_mut() = Some(laid.clone());
    let words = draw::Words::now();
    let mut parts: Vec<El<'a>> = Vec::new();
    // Its progress, read as a bar.
    if let (Some(top), Some(value)) = (laid.progress, note.progress) {
        let percent = crate::a11y::Words::new(crate::clock::time::Lang::of_messages())
            .percent(f32::from(value));
        parts.push(at(
            control(
                Space::new().width(draw::COLUMN).height(draw::PROGRESS),
                "bubble:Progress",
                Role::ProgressBar,
                "",
            )
            .node(|n| n.value(percent).range(f64::from(value), 0.0, 100.0, None)),
            Point::new(draw::COLUMN_X, top),
        ));
    }
    // The ✕.
    let close = draw::close_rect();
    let pressed = button(drawn_lit(close.width, close.height, 1.0, |f, _, lit| {
        draw::draw_close(f, lit);
    }))
    .padding(0)
    .style(|_, _| style::clear())
    .on_press(Msg::Close);
    parts.push(at(
        control(pressed, "bubble:Close", Role::Button, words.close()),
        close.position(),
    ));
    // Its actions.
    for (key, label, rect) in laid.actions {
        let size = rect.size();
        let name = label.clone();
        let pressed = button(drawn_lit(size.width, size.height, 1.0, move |f, _, lit| {
            draw::draw_action(f, size, &label, lit);
        }))
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Action(key.clone()));
        parts.push(at(
            control(pressed, format!("bubble:Action({key})"), Role::Button, name),
            rect.position(),
        ));
    }
    let size = laid.size;
    let origin = draw::card_origin(card);
    let card = mouse_area(
        Stack::with_children(parts)
            .width(size.width)
            .height(size.height),
    )
    .on_press(Msg::Card)
    .on_enter(Msg::Hover(true))
    .on_exit(Msg::Hover(false))
    .interaction(iced::mouse::Interaction::Pointer);
    Stack::with_children([at(card, origin)])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A screen reader clicks control `key` of bubble `serial` (`note`: the
/// bubble itself, its default action).
pub fn a11y_click(serial: u64, key: &str) {
    let input = match key {
        "note" => Input::Card(serial),
        "bubble:Close" => Input::Dismiss(serial),
        _ => match key
            .strip_prefix("bubble:Action(")
            .and_then(|k| k.strip_suffix(')'))
        {
            Some(action) => Input::Action {
                serial,
                key: action.to_owned(),
            },
            None => return,
        },
    };
    super::input(input);
}
