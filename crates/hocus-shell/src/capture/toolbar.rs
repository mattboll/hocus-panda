//! The capture's toolbar on the widget kit (#772): its buttons are the
//! kit's controls, placed where its layout puts them, each drawing its
//! look; its tint, rim, separator and the switch's tint are drawn under
//! them ([`Back`]). It never holds the keyboard (the compositor takes
//! `Escape` and `Enter`): no ring, but screen readers read it.

use hocus_a11y::Role;
use hocus_link::{CAPTURE_TOOLBAR_NAMESPACE, CaptureMode};
use iced::{
    Element, Length, Vector,
    widget::{Space, Stack, button, canvas, pin},
    window,
};

use super::{
    Button, Words,
    draw::{self, Back, Layout},
};
use crate::{
    Message,
    ui::{Around, control, parts::drawn_lit, style},
};

type El<'a> = Element<'a, Msg>;

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A button clicked.
    Act(Button),
    /// What it shows changed under the view.
    Stale,
}

/// A button's id: what tests and screen readers know it by.
fn id_of(button: Button) -> String {
    format!("toolbar:{button:?}")
}

/// The toolbar on its surface `id`, on the glass (else opaque).
pub fn view<'a>(id: window::Id, words: Words, glass: bool) -> Element<'a, Message> {
    let back = canvas(Back { id, words, glass })
        .width(Length::Fill)
        .height(Length::Fill);
    Around::new(id, build(id, words), move |m| handle(id, m), Msg::Stale)
        .own(CAPTURE_TOOLBAR_NAMESPACE, move || super::is_surface(id))
        .rebuild(move || build(id, words))
        .on(back)
}

fn handle(id: window::Id, message: Msg) {
    match message {
        Msg::Act(button) => super::clicked(id, button),
        Msg::Stale => {}
    }
}

/// The toolbar's buttons where its layout puts them.
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    let Some(bar) = super::with(|t| t.as_ref().filter(|t| t.id == id).map(|t| t.bar.clone()))
    else {
        return Space::new().into();
    };
    let layout = Layout::of(words, bar.video);
    let modes = layout
        .buttons
        .iter()
        .filter(|(b, _)| matches!(b, Button::Mode(_)))
        .count();
    let mut all: Vec<El<'a>> = Vec::new();
    for (which, rect) in layout.buttons.iter().copied() {
        let shown = bar.clone();
        let look = drawn_lit(rect.width, rect.height, 1.0, move |f, _, lit| {
            f.translate(Vector::new(-rect.x, -rect.y));
            draw::draw_button(f, &which, &rect, &shown, words, lit);
        });
        let pressed = button(look)
            .padding(0)
            .style(|_, _| style::clear())
            .on_press(Msg::Act(which));
        let (role, name, node): (Role, String, Option<bool>) = match which {
            Button::Kind(video) => (
                Role::RadioButton,
                words.kind(video).to_owned(),
                Some(bar.video == video),
            ),
            Button::Mode(mode) => (
                Role::RadioButton,
                words.mode(mode).to_owned(),
                Some(bar.mode == mode),
            ),
            Button::Delay => (Role::Button, words.delay(bar.delay), None),
            Button::Pointer => (
                Role::ToggleButton,
                words.pointer().to_owned(),
                Some(bar.pointer),
            ),
            Button::Sound => (
                Role::ToggleButton,
                words.sound().to_owned(),
                Some(bar.sound),
            ),
            Button::Micro => (
                Role::ToggleButton,
                words.micro().to_owned(),
                Some(bar.micro),
            ),
            Button::Shoot if bar.video => (Role::Button, words.start().to_owned(), None),
            Button::Shoot => (Role::Button, words.shoot().to_owned(), None),
        };
        let declared = control(pressed, id_of(which), role, name).node(|n| {
            let n = match node {
                Some(on) => n.checked(on),
                None => n,
            };
            match which {
                Button::Kind(video) => n.set(usize::from(video) + 1, 2),
                Button::Mode(mode) => n.set(mode_index(mode, &layout) + 1, modes),
                _ => n,
            }
        });
        all.push(pin(declared).x(rect.x).y(rect.y).into());
    }
    Stack::with_children(all)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Mode `mode`'s place among the modes shown.
fn mode_index(mode: CaptureMode, layout: &Layout) -> usize {
    layout
        .buttons
        .iter()
        .filter_map(|(b, _)| match b {
            Button::Mode(m) => Some(*m),
            _ => None,
        })
        .position(|m| m == mode)
        .unwrap_or(0)
}

/// The toolbar for screen readers: what its buttons declared.
pub fn a11y() -> Option<hocus_a11y::Window> {
    let id = super::with(|t| t.as_ref().map(|t| t.id))?;
    let rows = crate::a11y::rows::of(id)?;
    let mut window = hocus_a11y::Window::new("capture-toolbar", super::words().toolbar());
    let node = hocus_a11y::Node::new("toolbar", Role::Toolbar, super::words().toolbar())
        .children(rows.nodes);
    window.nodes = vec![node];
    Some(window)
}

/// A screen reader clicks button `key` of the toolbar.
pub fn a11y_click(key: &str) {
    let Some((id, video)) = super::with(|t| t.as_ref().map(|t| (t.id, t.bar.video))) else {
        return;
    };
    let layout = Layout::of(super::words(), video);
    if let Some((button, _)) = layout.buttons.iter().find(|(b, _)| id_of(*b) == key) {
        super::clicked(id, *button);
    }
}
