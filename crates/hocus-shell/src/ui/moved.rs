//! A widget drawn moved by an offset that changes with time (a field
//! shaking at a wrong password), drawn again each frame while it moves,
//! and only then: its surface alone. Laid out, hit and read where it
//! rests.

use std::time::Instant;

use iced::{
    Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
        widget::{Operation, Tree, tree},
    },
    window,
};

/// What it is moved by at a moment.
type Offset<'a> = Box<dyn Fn(Instant) -> Vector + 'a>;
/// Whether it moves at a moment.
type Moving<'a> = Box<dyn Fn(Instant) -> bool + 'a>;

/// `content` drawn moved by `offset` at the moment it draws, drawn again
/// each frame while `moving` says so then.
pub fn moved<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    offset: impl Fn(Instant) -> Vector + 'a,
    moving: impl Fn(Instant) -> bool + 'a,
) -> Element<'a, M> {
    Element::new(Moved {
        content: content.into(),
        offset: Box::new(offset),
        moving: Box::new(moving),
    })
}

struct Moved<'a, M> {
    content: Element<'a, M>,
    offset: Offset<'a>,
    moving: Moving<'a>,
}

/// The moment of the frame it draws.
#[derive(Debug, Default)]
struct Frame(Option<Instant>);

impl<M> Widget<M, iced::Theme, iced::Renderer> for Moved<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Frame>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Frame::default())
    }

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
        let now = tree.state.downcast_ref::<Frame>().0;
        let by = (self.offset)(now.unwrap_or_else(Instant::now));
        let content = &self.content;
        renderer.with_translation(by, |renderer| {
            content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
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
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            tree.state.downcast_mut::<Frame>().0 = Some(*now);
            if (self.moving)(*now) {
                shell.request_redraw();
            }
        }
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
}
