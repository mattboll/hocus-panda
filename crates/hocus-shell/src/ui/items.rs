//! A bar's surface of items on the kit (#785): the desktops' tiles, the
//! footer, the tray's icons, the windows, the tasks, Today, the next
//! meeting's card, the in-use sign.
//!
//! What such a surface shows is drawn ([`Items::draw`], its look as
//! before: tiles, glyphs, icons); what it holds is declared, once, where
//! it is laid out ([`Items::controls`]: each control's item, its place,
//! its node for screen readers, its ring). From the declarations the kit
//! does what each surface did by hand:
//!
//! - the pointer's hit-testing: which control is under it, told to the
//!   popovers' controller as it comes and goes (its cone, its 150 ms and
//!   300 ms) and pressed; the control under the pointer is handed to the
//!   drawing to light it, and the pointer's look is a hand there;
//! - the keyboard's ring: the controls join the bar's ring
//!   ([`super::ring`]), which goes through every surface of the bar, and
//!   the ring is drawn around the control holding it;
//! - the screen readers' rows: the declared nodes, with their places.
//!
//! It is a canvas still, each surface on its own layer surface: a hover,
//! a press, the ring moving draw that surface alone, never a message of
//! the shell's (which would build and draw every surface again), and at
//! rest nothing draws.

use hocus_a11y::{Act, Node, Role};
use iced::{
    Point, Rectangle, Renderer, Size, Theme, Vector, keyboard, mouse,
    widget::canvas::{self, Frame, Geometry},
    window,
};

use super::ring::{Declared, Spot};
use crate::{
    Message,
    popover::{self, Item},
};

/// A control of a bar's surface, as it declares it.
#[derive(Debug, Clone)]
pub struct Control {
    pub item: Item,
    /// Where the pointer takes it, in the surface's frame.
    pub rect: Rectangle,
    /// Its node: its role, name and state (its key and place are the
    /// kit's).
    pub node: Node,
    /// The ring's place and corners when it holds it; none, its look says
    /// so itself.
    pub ring: Option<(Rectangle, f32)>,
    /// A part of the control before it (a line's ✕), for screen readers.
    pub part: bool,
}

impl Control {
    /// `item` at `rect`, a `role` named `name`, acted on as its role says,
    /// the ring round `rect` with corners of `radius`.
    pub fn new(
        item: Item,
        rect: Rectangle,
        role: Role,
        name: impl Into<String>,
        radius: f32,
    ) -> Self {
        let node = Node::new(key(item), role, name);
        let node = match role {
            Role::Slider => node.act(Act::Click).act(Act::Increment).act(Act::Decrement),
            Role::Label | Role::Status | Role::Group | Role::List | Role::ProgressBar => node,
            _ => node.act(Act::Click),
        };
        Self {
            item,
            rect,
            node,
            ring: Some((rect, radius)),
            part: false,
        }
    }

    /// Its node as `with` makes it: checked, selected, one of a set…
    pub fn node(mut self, with: impl FnOnce(Node) -> Node) -> Self {
        self.node = with(self.node);
        self
    }

    /// The ring round `rect` with corners of `radius`, not round where
    /// the pointer takes it.
    #[allow(dead_code, reason = "for the bar's items still to come")]
    pub fn ring(mut self, rect: Rectangle, radius: f32) -> Self {
        self.ring = Some((rect, radius));
        self
    }

    /// A part of the control before it.
    pub fn part(mut self) -> Self {
        self.part = true;
        self
    }
}

/// An item's key, for screen readers and the tests: its debug form,
/// unique and stable (`Desktop { row: 0, index: 1 }`).
pub fn key(item: Item) -> String {
    format!("{item:?}")
}

/// What a bar's surface declares, as laid out on a surface of its size.
#[derive(Debug, Clone, Default)]
pub struct Controls {
    /// From the surface's frame to the bar's (from its right edge,
    /// negative inside it, and from its top): what a point is moved by.
    pub to_bar: Vector,
    /// From the top.
    pub controls: Vec<Control>,
    /// The group they make for screen readers (a list, a toolbar).
    pub group: Option<Node>,
}

impl Controls {
    /// The control under `at`, if any: the innermost (a button in a
    /// card, a line's ✕).
    fn under(&self, at: Point) -> Option<&Control> {
        self.controls
            .iter()
            .filter(|c| c.rect.contains(at))
            .min_by(|a, b| area(a.rect).total_cmp(&area(b.rect)))
    }

    fn in_bar(&self, rect: Rectangle) -> Rectangle {
        rect + self.to_bar
    }
}

fn area(rect: Rectangle) -> f32 {
    rect.width * rect.height
}

/// What a press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// As the kit does: a left press acts on the control under it, or
    /// presses elsewhere for the popovers.
    Kit,
    /// The surface did what it does: it took it.
    Taken,
    /// Nothing.
    Passed,
}

/// A bar's surface of items: what it draws and what it declares.
pub trait Items {
    /// What it keeps from a draw to the next.
    type State: Default + 'static;

    /// Its bar's screen's name.
    fn bar(&self) -> &str;

    /// Its surface's namespace.
    fn namespace(&self) -> &'static str;

    /// Its controls as laid out on a surface of `size`, from the top: none
    /// while the bar switches its form.
    fn controls(&self, state: &Self::State, size: Size) -> Controls;

    /// What it draws, `lit` the control under the pointer.
    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        bounds: Rectangle,
        lit: Option<Item>,
    ) -> Vec<Geometry>;

    /// Any event the kit does not take (a frame to draw, the wheel), the
    /// pointer at `at` on the surface.
    fn update(
        &self,
        _state: &mut Self::State,
        _event: &iced::Event,
        _bounds: Rectangle,
        _at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        None
    }

    /// The pointer onto `on` (the bar's frame), off every control with
    /// none: what the surface shows besides (a tooltip).
    fn hovered(&self, _state: &mut Self::State, _on: Option<(Item, Rectangle)>) {}

    /// A press of `button` on `on` (the bar's frame), off every control
    /// with none.
    fn pressed(
        &self,
        _state: &mut Self::State,
        _button: mouse::Button,
        _on: Option<(Item, Rectangle)>,
    ) -> Press {
        Press::Kit
    }
}

/// `program`'s surface on the kit: a canvas.
pub fn surface<P: Items>(program: P) -> Surface<P> {
    Surface(program)
}

/// A bar's surface of items on the kit ([`surface`]).
#[derive(Debug, Clone)]
pub struct Surface<P>(P);

/// What the kit keeps for a surface, beside its own.
#[derive(Debug, Default)]
pub struct State<S> {
    inner: S,
    /// The control under the pointer.
    hover: Option<Item>,
    /// Where the pointer is on the surface.
    pointer: Option<Point>,
    /// Where it was last seen, in the bar's frame.
    last: Option<Point>,
}

impl<P: Items> Surface<P> {
    fn mouse(
        &self,
        state: &mut State<P::State>,
        seen: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        event: &iced::Event,
    ) -> Option<canvas::Action<Message>> {
        let program = &self.0;
        let size = bounds.size();
        let at = pointer(seen, bounds, cursor, state.pointer);
        state.pointer = at;
        let controls = program.controls(&state.inner, size);
        let under = at
            .and_then(|at| controls.under(at))
            .map(|c| (c.item, controls.in_bar(c.rect)));
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                let at = at?;
                let bar_at = at + controls.to_bar;
                state.last = Some(bar_at);
                let now = under.map(|(item, _)| item);
                if now == state.hover {
                    if now.is_none() {
                        popover::event(popover::Event::Pointer { at: bar_at });
                    }
                    return None;
                }
                if state.hover.take().is_some() {
                    popover::event(popover::Event::ItemLeave { at: bar_at });
                }
                state.hover = now;
                if let Some((item, rect)) = under {
                    popover::event(popover::Event::ItemEnter {
                        bar: program.bar().to_owned(),
                        item,
                        rect,
                    });
                }
                program.hovered(&mut state.inner, under);
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::CursorLeft => {
                state.hover.take()?;
                let at = state
                    .last
                    .unwrap_or_else(|| Point::new(size.width / 2.0, size.height) + controls.to_bar);
                popover::event(popover::Event::ItemLeave { at });
                program.hovered(&mut state.inner, None);
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::ButtonPressed(button) => {
                match program.pressed(&mut state.inner, *button, under) {
                    Press::Kit if *button == mouse::Button::Left => {
                        let event = match under {
                            Some((item, rect)) => {
                                tracing::debug!(?item, "a bar's item pressed");
                                popover::Event::ItemPress {
                                    bar: program.bar().to_owned(),
                                    item,
                                    rect,
                                }
                            }
                            None => popover::Event::PressElsewhere,
                        };
                        popover::event(event);
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    Press::Taken => Some(canvas::Action::request_redraw().and_capture()),
                    Press::Kit | Press::Passed => None,
                }
            }
            _ => program.update(&mut state.inner, event, bounds, at),
        }
    }
}

impl<P: Items> canvas::Program<Message> for Surface<P> {
    type State = State<P::State>;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Mouse(seen) => self.mouse(state, seen, bounds, cursor, event),
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => popover::key(key, *modifiers, text.as_deref())
                .then(|| canvas::Action::request_redraw().and_capture()),
            iced::Event::Window(window::Event::RedrawRequested(_)) => {
                let action = self
                    .0
                    .update(&mut state.inner, event, bounds, state.pointer);
                // The control under the pointer went: nothing is lit, nor
                // shown for it.
                if let Some(item) = state.hover {
                    let controls = self.0.controls(&state.inner, bounds.size());
                    if !controls.controls.iter().any(|c| c.item == item) {
                        state.hover = None;
                        self.0.hovered(&mut state.inner, None);
                    }
                }
                action
            }
            _ => self
                .0
                .update(&mut state.inner, event, bounds, state.pointer),
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let program = &self.0;
        let bar = program.bar();
        let controls = program.controls(&state.inner, bounds.size());
        let spots = controls
            .controls
            .iter()
            .map(|c| Spot {
                item: c.item,
                rect: controls.in_bar(c.rect),
                node: Some(c.node.clone()),
                part: c.part,
            })
            .collect();
        let declared = Declared {
            spots,
            group: controls.group.clone(),
        };
        // Declared before it draws: what the ring is on is as it shows.
        let ringed = popover::with(|c| {
            c.declare(bar, program.namespace(), declared);
            controls
                .controls
                .iter()
                .find(|control| c.focused(bar, control.item))
                .and_then(|control| control.ring)
        });
        let mut drawn = program.draw(&state.inner, renderer, bounds, state.hover);
        if let Some((rect, radius)) = ringed {
            let mut frame = Frame::new(renderer, bounds.size());
            super::ring::draw(&mut frame, rect, radius);
            drawn.push(frame.into_geometry());
        }
        drawn
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Where `event` puts the pointer on the surface at `bounds`, or where it
/// was at `before`: iced hands every event of a batch the cursor as it is
/// after the batch, which may have left already when a press and a move
/// away came together.
pub fn pointer(
    event: &mouse::Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
    before: Option<Point>,
) -> Option<Point> {
    match event {
        mouse::Event::CursorMoved { position } => {
            Some(Point::new(position.x - bounds.x, position.y - bounds.y))
        }
        mouse::Event::CursorEntered => cursor.position_in(bounds),
        mouse::Event::CursorLeft => None,
        _ => before,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_takes_the_innermost_control_under_it() {
        let line = Rectangle::new(Point::new(5.0, 5.0), Size::new(184.0, 30.0));
        let close = Rectangle::new(Point::new(163.0, 9.0), Size::new(22.0, 22.0));
        let controls = Controls {
            controls: vec![
                Control::new(Item::Window(1), line, Role::ListItem, "w1", 8.0),
                Control::new(Item::WindowClose(1), close, Role::Button, "Fermer w1", 6.0).part(),
            ],
            ..Controls::default()
        };
        let under = |x, y| controls.under(Point::new(x, y)).map(|c| c.item);
        assert_eq!(under(170.0, 20.0), Some(Item::WindowClose(1)));
        assert_eq!(under(40.0, 20.0), Some(Item::Window(1)));
        assert_eq!(under(40.0, 50.0), None);
    }
}
