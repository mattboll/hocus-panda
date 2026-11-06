//! The window's keys, handed to the app in the order they came, among the
//! messages of the widgets they reached (#669).
//!
//! iced gives a subscription its events through the runtime's executor:
//! they come back as messages a moment later, after the widgets have taken
//! the keys that followed. On a loaded machine that moment is long enough
//! for a word typed after `Ctrl+F` to reach the search field before the
//! app hears `Ctrl+F`, whose selection then took the letters already typed
//! and lost them to the next. Here each key is a message published by the
//! widget around the whole window, right after the widgets under it have
//! seen it, so the app hears it in its place; and `Ctrl+F` gives the search
//! field the keyboard, its text selected, before the next key is read, not
//! with an operation applied after every key already received.

use iced::{
    Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, operation},
    },
    keyboard::{self, Key, Modifiers},
};

/// Whether `key` with `modifiers` is `Ctrl+F`, the search's.
pub(super) fn is_ctrl_f(key: &Key, modifiers: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character(c) if modifiers.control() && c.eq_ignore_ascii_case("f"))
}

/// A key pressed, as the app hears it.
pub(super) struct Pressed {
    pub key: Key,
    pub physical: keyboard::key::Physical,
    pub modifiers: Modifiers,
    pub text: Option<String>,
    /// A widget under the pointer or with the keyboard took it.
    pub captured: bool,
}

/// `content`, its keys published in order.
pub(super) struct KeysIn<'a, Message> {
    content: Element<'a, Message>,
    on_key: Box<dyn Fn(Pressed) -> Message + 'a>,
    on_modifiers: Box<dyn Fn(Modifiers) -> Message + 'a>,
    /// The text field `Ctrl+F` gives the keyboard to, its text selected;
    /// `None` while `Ctrl+F` goes elsewhere (a shortcut being recorded).
    search: Option<&'static str>,
}

impl<'a, Message> KeysIn<'a, Message> {
    pub(super) fn new(
        content: impl Into<Element<'a, Message>>,
        on_key: impl Fn(Pressed) -> Message + 'a,
        on_modifiers: impl Fn(Modifiers) -> Message + 'a,
        search: Option<&'static str>,
    ) -> Self {
        Self {
            content: content.into(),
            on_key: Box::new(on_key),
            on_modifiers: Box::new(on_modifiers),
            search,
        }
    }
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for KeysIn<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
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
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            text,
            ..
        }) = event
            && let Some(search) = self.search
            && is_ctrl_f(key, *modifiers)
        {
            // At once, so that the keys read after it reach the field and
            // replace its text.
            let content = self.content.as_widget_mut();
            let child = &mut tree.children[0];
            content.operate(
                child,
                layout,
                renderer,
                &mut operation::focusable::focus(search.into()),
            );
            content.operate(
                child,
                layout,
                renderer,
                &mut operation::text_input::select_all(search.into()),
            );
            shell.capture_event();
            shell.request_redraw();
            shell.publish((self.on_key)(Pressed {
                key: key.clone(),
                physical: *physical_key,
                modifiers: *modifiers,
                text: text.as_ref().map(ToString::to_string),
                captured: false,
            }));
            return;
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
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                text,
                ..
            }) => {
                let captured = shell.is_event_captured();
                shell.publish((self.on_key)(Pressed {
                    key: key.clone(),
                    physical: *physical_key,
                    modifiers: *modifiers,
                    text: text.as_ref().map(ToString::to_string),
                    captured,
                }));
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                shell.publish((self.on_modifiers)(*modifiers));
            }
            _ => {}
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
            layout,
            cursor,
            viewport,
            renderer,
        )
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

impl<'a, Message: 'a> From<KeysIn<'a, Message>> for Element<'a, Message> {
    fn from(keys: KeysIn<'a, Message>) -> Self {
        Element::new(keys)
    }
}
