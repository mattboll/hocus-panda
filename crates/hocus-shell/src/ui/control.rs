//! A control: a widget declared once where it is built, with what it is
//! to the keyboard and to a screen reader, under the keyboard's ring while
//! it holds it.
//!
//! The declaration rides the widget tree: an operation over the laid-out
//! tree ([`super::walk::Walk`]) reads it, with the bounds the control was
//! laid out at, in the tree's order. The surface ([`super::Around`]) takes
//! from there the order `Tab` goes in, what `Enter` acts on, the screen
//! reader's nodes and the tests' dump; nothing of it is written twice.

use std::any::Any;

use hocus_a11y::{Act, Node, Role};
use iced::{
    Border, Color, Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Id, Operation, Tree},
    },
};

use crate::levels::draw::{RING, RING_OUT};

type El<'a, M> = Element<'a, M>;

/// What a control is, as the tree carries it.
#[derive(Debug, Clone)]
pub struct Declared<M> {
    /// Its node for screen readers: its key is the control's id; its
    /// bounds are where it was laid out.
    pub node: Node,
    /// The ring is on it.
    pub focused: bool,
    /// What the surface is told when the ring comes to it: none, and the
    /// ring never does.
    pub on_focus: Option<M>,
    /// What `Enter` (and `Space`) on it does.
    pub on_activate: Option<M>,
    /// The arrows go through the controls of the same group, round.
    pub rows: Option<u8>,
    /// It takes typing: while it holds the ring the keys are its own, but
    /// `Tab`, `Escape` and `Enter`.
    pub typing: bool,
}

/// A widget, declared.
pub struct Control<'a, M> {
    content: El<'a, M>,
    id: Option<Id>,
    declared: Option<Declared<M>>,
    /// The ring's corners.
    radius: f32,
    ring: bool,
}

/// `content` as the control `id`, a `role` named `name`: it is acted on
/// as its role says (a button is clicked, a slider stepped, a field
/// focused).
pub fn control<'a, M>(
    content: impl Into<El<'a, M>>,
    id: impl Into<String>,
    role: Role,
    name: impl Into<String>,
) -> Control<'a, M> {
    let id = id.into();
    let mut node = Node::new(id.clone(), role, name);
    node = match role {
        Role::Slider => node.act(Act::Increment).act(Act::Decrement),
        Role::PasswordField | Role::TextField => node.act(Act::Focus),
        Role::Label | Role::Status | Role::Group | Role::List | Role::ProgressBar => node,
        _ => node.act(Act::Click),
    };
    Control {
        content: content.into(),
        id: Some(Id::from(id)),
        declared: Some(Declared {
            node,
            focused: false,
            on_focus: None,
            on_activate: None,
            rows: None,
            typing: false,
        }),
        radius: 0.0,
        ring: false,
    }
}

/// `content`, a line of text that nothing holds (a note, a section's
/// label), read as `text`.
pub fn line<'a, M>(content: impl Into<El<'a, M>>, text: impl Into<String>) -> Control<'a, M> {
    let text = text.into();
    control(content, format!("line:{text}"), Role::Label, text)
}

/// `content` under the keyboard's ring when `on`, its corners' `radius`,
/// declaring nothing (a surface that is not the kit's).
pub fn ring<'a, M: 'static>(content: impl Into<El<'a, M>>, on: bool, radius: f32) -> El<'a, M> {
    Element::new(Control {
        content: content.into(),
        id: None,
        declared: None,
        radius,
        ring: on,
    })
}

impl<M> std::fmt::Debug for Control<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control")
            .field("id", &self.id)
            .field("ring", &self.ring)
            .finish_non_exhaustive()
    }
}

impl<M> Control<'_, M> {
    fn declared(mut self, with: impl FnOnce(&mut Declared<M>)) -> Self {
        if let Some(declared) = &mut self.declared {
            with(declared);
        }
        self
    }

    /// Its node as `with` makes it: checked, disabled, described…
    pub fn node(self, with: impl FnOnce(Node) -> Node) -> Self {
        self.declared(|d| d.node = with(d.node.clone()))
    }

    /// The ring can come to it, which tells the surface `on_focus`; it is
    /// on it when `focused`, its corners' `radius`.
    pub fn focus(mut self, focused: bool, on_focus: M, radius: f32) -> Self {
        self.radius = radius;
        self.ring = focused;
        self.declared(|d| {
            d.focused = focused;
            d.on_focus = Some(on_focus);
        })
    }

    /// What `Enter` on it does.
    pub fn activate(self, message: M) -> Self {
        self.declared(|d| d.on_activate = Some(message))
    }

    /// The arrows go through `group`'s controls.
    pub fn rows(self, group: u8) -> Self {
        self.declared(|d| d.rows = Some(group))
    }

    /// It takes typing, and its caret says it holds the keyboard: no ring.
    pub fn typing(mut self) -> Self {
        self.ring = false;
        self.declared(|d| d.typing = true)
    }
}

impl<'a, M: 'static> From<Control<'a, M>> for El<'a, M> {
    fn from(control: Control<'a, M>) -> Self {
        Element::new(control)
    }
}

impl<M: 'static> Widget<M, iced::Theme, iced::Renderer> for Control<'_, M> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if self.ring {
            draw_ring(renderer, layout.bounds(), self.radius);
        }
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.id.is_some() {
            operation.container(self.id.as_ref(), layout.bounds());
        }
        if let Some(declared) = &mut self.declared {
            operation.custom(self.id.as_ref(), layout.bounds(), declared as &mut dyn Any);
        }
        let content = &mut self.content;
        let child = &mut tree.children[0];
        operation.traverse(&mut |operation| {
            content
                .as_widget_mut()
                .operate(child, layout, renderer, operation);
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

/// The keyboard's ring around `bounds`, as `levels::draw::ring`: 2 px of
/// the accent as text, 3 px out; under high contrast 3 px, the background
/// between. In a layer of its own: a list's does not cut it at the list's
/// sides. A stock widget's border, inside its bounds, cannot draw it.
fn draw_ring(renderer: &mut iced::Renderer, bounds: Rectangle, radius: f32) {
    use iced::advanced::Renderer as _;
    let colour = crate::theme::ACCENT_TEXT.get();
    let around = |out: f32, width: f32, colour: Color| renderer::Quad {
        bounds: bounds.expand(out + width),
        border: Border {
            color: colour,
            width,
            radius: (radius + out + width).into(),
        },
        ..renderer::Quad::default()
    };
    let reach = RING_OUT + RING;
    renderer.with_layer(bounds.expand(reach + 1.0), |renderer| {
        if crate::theme::contrast() {
            let gap = RING_OUT - 1.0;
            let back = crate::theme::Ink::Token(|t| t.popover_opaque_bottom).get();
            renderer.fill_quad(around(0.0, gap, back), Color::TRANSPARENT);
            renderer.fill_quad(around(gap, RING + 1.0, colour), Color::TRANSPARENT);
        } else {
            renderer.fill_quad(around(RING_OUT, RING, colour), Color::TRANSPARENT);
        }
    });
}
