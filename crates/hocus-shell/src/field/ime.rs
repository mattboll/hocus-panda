//! An input method for a surface's field (#763): the shell draws its
//! fields on canvases, which cannot ask for one, so every surface's
//! widgets are wrapped in [`Asking`], which asks iced for it at each
//! redraw while a field there has the keyboard: text-input-v3 is enabled
//! on the surface, with the caret's place (where an input method opens
//! its window) and what the field takes (a secret: no suggestions). Its
//! preedit and its commits come back to the canvas as iced's
//! `InputMethod` events.
//!
//! Which field has the keyboard, a surface says by drawing it so
//! ([`drawn`], from [`super::Entry::draw`]): the wrapper hears it as its
//! widgets draw, and asks from the next redraw on.

use std::cell::Cell;

use iced::{
    Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget,
        input_method::{InputMethod, Purpose},
        layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
    window,
};

/// What a field asks of an input method: where its caret is (on its
/// canvas) and what it takes.
type Ask = (Rectangle, Purpose);

thread_local! {
    /// The field drawn with the keyboard, among the widgets being drawn.
    static DRAWN: Cell<Option<Ask>> = const { Cell::new(None) };
}

/// A field was drawn with the keyboard, its caret at `caret`.
pub fn drawn(caret: Rectangle, purpose: Purpose) {
    DRAWN.set(Some((caret, purpose)));
}

/// What the wrapper keeps from a draw to the next redraw.
#[derive(Debug, Default)]
struct State {
    /// The field last drawn with the keyboard.
    typing: Cell<Option<Ask>>,
    /// Whether it was asked for since.
    asked: Cell<bool>,
}

/// A surface's widgets, which ask for an input method for its field.
pub struct Asking<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
}

impl<'a, Message, Theme, Renderer> Asking<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

impl<Message, Theme, Renderer> std::fmt::Debug for Asking<'_, Message, Theme, Renderer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asking").finish_non_exhaustive()
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Asking<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        DRAWN.set(None);
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        let typing = DRAWN.take();
        let state = tree.state.downcast_ref::<State>();
        if state.typing.get() != typing {
            state.typing.set(typing);
            state.asked.set(false);
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
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
        renderer: &Renderer,
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
        renderer: &Renderer,
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
        let state = tree.state.downcast_ref::<State>();
        let Some((caret, purpose)) = state.typing.get() else {
            return;
        };
        // Asked at each redraw, as iced's own fields do: what is asked
        // then is what the surface's text input says.
        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            let at = layout.bounds().position();
            shell.request_input_method(&InputMethod::<&str>::Enabled {
                cursor: caret + Vector::new(at.x, at.y),
                purpose,
                // Shown in the field itself.
                preedit: None,
            });
            state.asked.set(true);
        } else if !state.asked.get() {
            // A field took the keyboard as it last drew: asked at once.
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
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
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Asking<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(asking: Asking<'a, Message, Theme, Renderer>) -> Self {
        Element::new(asking)
    }
}
