//! The lock on one screen, on the widget kit (#772): the field, its
//! button and the layout's are the kit's controls, placed where the
//! lock's layout puts them on its screen, each drawing its look; the
//! field and its button shake together after a wrong password. The
//! password is typed in the compositor, which keeps it: the field shows
//! its dots, as many as it says, and a click on it, on the arrow or on the
//! layout's button goes back to it (`lock focus`, `lock submit`,
//! `next-layout`). What nothing holds is drawn under them
//! ([`draw::View`]).

use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_a11y::Role;
use hocus_link::{LOCK_NAMESPACE, LockAsk, LockFocus, LockPhase, ToCompositor};
use iced::{
    Element, Length, Point, Size, Vector,
    widget::{Space, Stack, button, canvas, mouse_area, pin, stack},
    window,
};

use super::{
    Words,
    draw::{self, Layer, Layout},
};
use crate::{
    Message,
    clock::time::{Lang, Wall},
    ui::{
        self, Around, control, moved,
        parts::{drawn_lit, drawn_out, moving},
        sized::Sized,
        style,
    },
};

type El<'a> = Element<'a, Msg>;

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A control clicked: the field takes the keys, the arrow sends the
    /// password, the layout's button changes the layout.
    Act(LockFocus),
    /// The ring comes to a control.
    Focus(LockFocus),
    /// What it shows changed under the view.
    Stale,
}

/// Each lock surface's size, as it was laid out: its controls are placed
/// from it.
static SIZES: Mutex<Option<HashMap<window::Id, Size>>> = Mutex::new(None);

fn size_of(id: window::Id) -> Size {
    SIZES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|s| s.get(&id).copied())
        .unwrap_or(Size::ZERO)
}

/// The lock on its surface `id`, on screen `output`.
pub fn view<'a>(
    id: window::Id,
    output: String,
    wall: Wall,
    lang: Lang,
    words: Words,
) -> Element<'a, Message> {
    let layer = |layer| {
        canvas(draw::View {
            layer,
            output: output.clone(),
            wall: wall.clone(),
            lang,
            words,
        })
        .width(Length::Fill)
        .height(Length::Fill)
    };
    let under = stack![layer(Layer::Back), layer(Layer::Front)];
    // The size its controls were placed from.
    let seen: Rc<Cell<Size>> = Rc::default();
    let (built, rebuilt, stale) = (seen.clone(), seen.clone(), seen);
    let again = output.clone();
    Around::new(id, build(id, &output, words, &built), handle, Msg::Stale)
        .own(LOCK_NAMESPACE, move || super::output_of(id).is_some())
        .rebuild(move || build(id, &again, words, &rebuilt))
        .stale_when(move || size_of(id) != stale.get())
        .on(under)
}

fn handle(message: Msg) {
    let ask = match message {
        Msg::Act(LockFocus::Button) => ToCompositor::Lock(LockAsk::Submit),
        Msg::Act(LockFocus::Layout) => ToCompositor::NextLayout,
        Msg::Act(LockFocus::Field) | Msg::Focus(LockFocus::Field) => {
            ToCompositor::Lock(LockAsk::Focus(LockFocus::Field))
        }
        Msg::Focus(to) => ToCompositor::Lock(LockAsk::Focus(to)),
        Msg::Stale => return,
    };
    tracing::debug!(?ask, "the lock's click");
    crate::popover::tell(ask);
}

/// A control's id: what tests and screen readers know it by.
fn id_of(control: LockFocus) -> String {
    format!("lock:{control:?}")
}

/// `content` placed at `at`.
fn at<'a>(content: impl Into<El<'a>>, at: Point) -> El<'a> {
    pin(content).x(at.x).y(at.y).into()
}

/// The lock's state when it draws.
fn now_shown() -> Option<super::Lock> {
    super::with(|l| l.clone())
}

/// The field and its button shaken after a wrong password, at `now`.
fn shake(now: Instant) -> Vector {
    Vector::new(now_shown().map_or(0.0, |l| draw::shaken(&l, now)), 0.0)
}

/// The controls of the lock on screen `output`, from the size its surface
/// was laid out at (none before it is known, nor on a screen with the
/// clock alone); told the size it is laid out at.
fn build<'a>(id: window::Id, output: &str, words: Words, seen: &Cell<Size>) -> El<'a> {
    let size = size_of(id);
    seen.set(size);
    let content = super::with(|l| {
        let lock = l.as_ref()?;
        (size.width > 0.0 && draw::form(lock, output)).then(|| controls(lock, size, words))
    })
    .unwrap_or_else(|| Space::new().into());
    Sized::new(content, move |room| {
        let mut sizes = SIZES.lock().unwrap_or_else(PoisonError::into_inner);
        let sizes = sizes.get_or_insert_with(HashMap::new);
        // Surfaces gone go with the next one told.
        sizes.retain(|s, _| super::output_of(*s).is_some());
        sizes.insert(id, room);
    })
    .into()
}

/// The field, its button, the layout's button and the agents' line, on
/// a screen `size` large; the ring where the compositor says.
fn controls<'a>(lock: &super::Lock, size: Size, words: Words) -> El<'a> {
    let layout = Layout::of(size, true);
    let shown = &lock.shown;
    let keys = shown.phase.takes_keys();
    let ring = |on: LockFocus| shown.focus == on && keys;
    let mut all: Vec<El<'a>> = Vec::new();
    // The field: its box and dots drawn from what the compositor says.
    let field = layout.field;
    let local = layout.from(field.position());
    let look = drawn_out(field.width, field.height, 1.0, move |f, _| {
        if let Some(lock) = now_shown() {
            draw::draw_field(f, &lock, &local, words);
        }
    });
    let clicked = mouse_area(look)
        .on_release(Msg::Act(LockFocus::Field))
        .interaction(iced::mouse::Interaction::Text);
    let (said, _) = draw::message(lock, words);
    let waiting = matches!(
        shown.phase,
        LockPhase::Checking | LockPhase::Opening | LockPhase::Waiting(_)
    );
    all.push(at(
        moved(
            control(
                clicked,
                id_of(LockFocus::Field),
                Role::PasswordField,
                words.password(),
            )
            .node(|n| n.describe(said).disabled(waiting))
            .focus(ring(LockFocus::Field), Msg::Focus(LockFocus::Field), 12.0)
            .typing(),
            shake,
            |_| false,
        ),
        field.position(),
    ));
    // Its button: the arrow, or the spinner turning.
    let rect = layout.button;
    let arrow = moving(
        rect.width,
        rect.height,
        move |f, _, now| {
            if let Some(lock) = now_shown() {
                f.translate(Vector::new(-rect.x, -rect.y));
                draw::draw_button(f, &lock, rect, now);
            }
        },
        |_| now_shown().is_some_and(|l| draw::spinning(&l)),
    );
    let pressed = button(arrow)
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Act(LockFocus::Button));
    all.push(at(
        moved(
            control(
                pressed,
                id_of(LockFocus::Button),
                Role::Button,
                words.unlock(),
            )
            .focus(ring(LockFocus::Button), Msg::Focus(LockFocus::Button), 9.0)
            .activate(Msg::Act(LockFocus::Button)),
            shake,
            |_| false,
        ),
        rect.position(),
    ));
    // The agents waiting: a count.
    if shown.agents > 0 {
        all.push(at(
            ui::line(
                Space::new().width(layout.message.width).height(34.0),
                words.agents(shown.agents),
            ),
            Point::new(layout.message.x, layout.agents_top),
        ));
    }
    // The layout's button, when there is a choice.
    if let Some((rect, code)) = draw::row(size, layout.row_centre).layout {
        let name = crate::layouts::snapshot()
            .shown()
            .map(|l| l.name.clone())
            .unwrap_or_else(|| code.clone());
        let look = drawn_lit(rect.width, rect.height, 0.0, move |f, size, lit| {
            draw::draw_layout(f, iced::Rectangle::new(Point::ORIGIN, size), &code, lit);
        });
        let pressed = button(look)
            .padding(0)
            .style(|_, _| style::clear())
            .on_press(Msg::Act(LockFocus::Layout));
        all.push(at(
            control(
                pressed,
                id_of(LockFocus::Layout),
                Role::Button,
                words.layout(&name),
            )
            .focus(ring(LockFocus::Layout), Msg::Focus(LockFocus::Layout), 8.0)
            .activate(Msg::Act(LockFocus::Layout)),
            rect.position(),
        ));
    }
    Stack::with_children(all)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The lock on screen `output` for screen readers: what its controls
/// declared, after `before` (the clock); the form's screen holds the
/// keyboard, its ring where the compositor says.
pub fn a11y(
    surface: window::Id,
    output: &str,
    words: crate::a11y::Words,
    before: Vec<hocus_a11y::Node>,
) -> hocus_a11y::Window {
    let mut window = hocus_a11y::Window::new(format!("lock:{output}"), words.lock());
    window.nodes = before;
    let form = super::with(|l| l.as_ref().is_some_and(|l| draw::form(l, output)));
    if let Some(rows) = crate::a11y::rows::of(surface).filter(|_| form) {
        window.nodes.extend(rows.nodes);
        window.active = true;
        window.focus = rows.focus;
    }
    window
}

/// A screen reader acts on control `key` of a lock.
pub fn a11y_click(key: &str) {
    let control = match key {
        "lock:Field" => LockFocus::Field,
        "lock:Button" => LockFocus::Button,
        "lock:Layout" => LockFocus::Layout,
        _ => return,
    };
    handle(Msg::Act(control));
}
