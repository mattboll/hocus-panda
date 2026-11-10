//! The widgets of our own a popover made of iced's stock widgets needs
//! (spike #770). Nothing here is the Wi-Fi's: every popover would share
//! them.
//!
//! - [`Around`], a popover's whole surface: the pointer coming in and
//!   leaving (the controller's cone and its 300 ms), the keys before the
//!   widgets get them (iced 0.14 gives the keyboard's focus to text fields
//!   only: `Tab`, the arrows, `Enter` on a button are ours), the wake that
//!   builds the view again without the shell's other surfaces knowing, and
//!   the height its content takes by itself.
//! - [`Ring`], the keyboard's ring around a widget: 2 px, 3 px *outside*
//!   it, which a stock widget's border (inside its bounds) cannot draw.
//! - [`reveal`], an operation: a list scrolled as little as shows one of
//!   its rows, found by its id.

use iced::{
    Border, Color, Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{
            Id, Operation, Tree,
            operation::{self, Outcome, Scrollable, scrollable},
            tree,
        },
    },
    keyboard, window,
};

use crate::levels::draw::{RING, RING_OUT};
use crate::popover::{Event as Seen, Pressed, event, kit};

type El<'a, Message> = Element<'a, Message, iced::Theme, iced::Renderer>;

// --- The surface -------------------------------------------------------------------

/// A popover's surface around its widgets.
pub struct Around<'a, Message> {
    id: window::Id,
    content: El<'a, Message>,
    /// What the content was built from changed since: `on_stale` is
    /// published as the surface next draws, and the view is built again.
    stale: Box<dyn Fn() -> bool + 'a>,
    on_stale: Message,
    /// A key, before the content: `Some` takes it.
    on_key: Box<dyn Fn(Pressed) -> Option<Message> + 'a>,
    /// A press anywhere, once the content has had it.
    on_press: Message,
    /// The height the content takes, laid out with all the height it wants.
    natural: Box<dyn Fn(f32) + 'a>,
}

impl<Message> std::fmt::Debug for Around<'_, Message> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Around")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<'a, Message> Around<'a, Message> {
    pub fn new(
        id: window::Id,
        content: impl Into<El<'a, Message>>,
        stale: impl Fn() -> bool + 'a,
        on_stale: Message,
        on_key: impl Fn(Pressed) -> Option<Message> + 'a,
        on_press: Message,
        natural: impl Fn(f32) + 'a,
    ) -> Self {
        Self {
            id,
            content: content.into(),
            stale: Box::new(stale),
            on_stale,
            on_key: Box::new(on_key),
            on_press,
            natural: Box::new(natural),
        }
    }
}

/// The pointer is on the surface.
#[derive(Debug, Default)]
struct Inside(bool);

fn child(layout: Layout<'_>) -> Layout<'_> {
    layout.children().next().unwrap_or(layout)
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for Around<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Inside>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Inside::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max = limits.max();
        // All the height it wants: what the surface should be.
        let free = layout::Limits::new(Size::ZERO, Size::new(max.width, f32::INFINITY));
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &free);
        (self.natural)(content.size().height);
        layout::Node::with_children(max, vec![content])
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
            child(layout),
            cursor,
            viewport,
        );
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
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            child(layout),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let open = crate::popover::with(|c| c.is_open(self.id));
        match event {
            Event::Window(window::Event::RedrawRequested(_)) => {
                if open && (self.stale)() {
                    shell.publish(self.on_stale.clone());
                }
                // The keys that reached another of the shell's surfaces, and
                // the `Tab` of a popover opened by the keyboard.
                for pressed in kit::keys(self.id) {
                    if let Some(message) = (self.on_key)(pressed) {
                        shell.publish(message);
                    }
                }
            }
            // Fading out, it only draws.
            _ if !open => return,
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorEntered) => {
                let inside = tree.state.downcast_mut::<Inside>();
                if !inside.0 && cursor.position_over(layout.bounds()).is_some() {
                    inside.0 = true;
                    event_seen(Seen::PopoverEnter);
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                tree.state.downcast_mut::<Inside>().0 = false;
                event_seen(Seen::PopoverLeave);
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                let pressed = Pressed {
                    key: key.clone(),
                    modifiers: *modifiers,
                    text: text.as_deref().map(str::to_owned),
                };
                if let Some(message) = (self.on_key)(pressed) {
                    shell.publish(message);
                    shell.capture_event();
                    return;
                }
            }
            _ => {}
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child(layout),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) {
            shell.publish(self.on_press.clone());
        }
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
            child(layout),
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
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child(layout),
            renderer,
            viewport,
            translation,
        )
    }
}

fn event_seen(seen: Seen) {
    event(seen);
}

impl<'a, Message: Clone + 'a> From<Around<'a, Message>> for El<'a, Message> {
    fn from(around: Around<'a, Message>) -> Self {
        Element::new(around)
    }
}

// --- The keyboard's ring -----------------------------------------------------------

/// A widget, and the keyboard's ring around it while it holds it.
pub struct Ring<'a, Message> {
    content: El<'a, Message>,
    on: bool,
    radius: f32,
}

/// `content` under the keyboard's ring when `on`, its corners' `radius`.
pub fn ring<'a, Message: 'a>(
    content: impl Into<El<'a, Message>>,
    on: bool,
    radius: f32,
) -> El<'a, Message> {
    Element::new(Ring {
        content: content.into(),
        on,
        radius,
    })
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Ring<'_, Message> {
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
        use iced::advanced::Renderer as _;
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if !self.on {
            return;
        }
        // As `levels::draw::ring`: 2 px of the accent as text, 3 px out;
        // under high contrast 3 px, the background between. In a layer of
        // its own: a list's does not cut it at the list's sides.
        let colour = crate::theme::ACCENT_TEXT.get();
        let bounds = layout.bounds();
        let around = |out: f32, width: f32, colour: Color| renderer::Quad {
            bounds: bounds.expand(out + width),
            border: Border {
                color: colour,
                width,
                radius: (self.radius + out + width).into(),
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
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
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
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

// --- A row shown -------------------------------------------------------------------

/// The list `list` scrolled as little as shows the container `target` in
/// it, whole if it fits.
pub fn reveal<T: 'static>(list: Id, target: Id) -> impl Operation<T> {
    struct Reveal {
        list: Id,
        target: Id,
        /// The list's viewport, how far it is scrolled, its content's
        /// height.
        view: Option<(Rectangle, f32, f32)>,
        found: Option<Rectangle>,
    }

    impl<T: 'static> Operation<T> for Reveal {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }

        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content: Rectangle,
            translation: Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&self.list) {
                self.view = Some((bounds, translation.y, content.height));
            }
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if id == Some(&self.target) {
                self.found = Some(bounds);
            }
        }

        fn finish(&self) -> Outcome<T> {
            let (Some((view, scroll, content)), Some(row)) = (self.view, self.found) else {
                return Outcome::None;
            };
            let most = (content - view.height).max(0.0);
            let to = kit::reveal((row.y - view.y, row.height), view.height, scroll, most);
            if (to - scroll).abs() < 0.5 {
                return Outcome::None;
            }
            Outcome::Chain(Box::new(scrollable::scroll_to(
                self.list.clone(),
                operation::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(to),
                },
            )))
        }
    }

    Reveal {
        list,
        target,
        view: None,
        found: None,
    }
}
