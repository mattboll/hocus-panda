//! A list's wheel, as the canvases had it: a mouse's notch scrolls a
//! given height (two rows), not iced's 60 px a line, and a touchpad's
//! pixels are taken back to logical ones (iced_layershell hands them
//! multiplied by the surface's scale, #488). The scrollable inside sees
//! the scrolling as pixels.

use iced::{
    Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
};

use crate::popover::kit::{self, Wheel};

/// `content` (a scrollable), a notch of the wheel scrolling `notch` px.
pub struct Notched<'a, M> {
    content: Element<'a, M>,
    notch: f32,
}

impl<'a, M> Notched<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, notch: f32) -> Self {
        Self {
            content: content.into(),
            notch,
        }
    }
}

impl<M> std::fmt::Debug for Notched<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notched")
            .field("notch", &self.notch)
            .finish_non_exhaustive()
    }
}

impl<'a, M: 'a> From<Notched<'a, M>> for Element<'a, M> {
    fn from(notched: Notched<'a, M>) -> Self {
        Element::new(notched)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Notched<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Wheel>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Wheel::default())
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
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
        let mut scrolled = None;
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event
            && cursor.is_over(layout.bounds())
        {
            let wheel = tree.state.downcast_mut::<Wheel>();
            let by = wheel.down(delta, self.notch, kit::popover_scale());
            if by == 0.0 {
                shell.capture_event();
                return;
            }
            scrolled = Some(Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -by },
            }));
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            scrolled.as_ref().unwrap_or(event),
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
