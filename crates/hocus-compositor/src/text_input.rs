//! text-input-v3 (`zwp_text_input_v3`), with the compositor as the input
//! method: what the emoji picker needs (5.7). An application's text field
//! tells where its caret is (`set_cursor_rectangle`), which places the
//! picker under it; an emoji chosen goes in with `commit_string`.
//!
//! smithay has its own text-input, which hands everything to an input
//! method client (`zwp_input_method_v2`) and says nothing while there is
//! none: the caret's rectangle never reaches the compositor, and only that
//! client could commit text. Here the compositor keeps each text input's
//! state itself. A real input method (fcitx5, IBus) has no place yet: that
//! would be smithay's relay, with the picker beside it.
//!
//! The protocol's rules kept: a text input enters the surface holding the
//! keyboard when it is its client's, and leaves it when the keyboard goes;
//! what a client asks takes effect on its `commit`, `enable` resetting
//! what it said before; `done` carries the number of commits received.

use smithay::{
    reexports::{
        wayland_protocols::wp::text_input::zv3::server::{
            zwp_text_input_manager_v3::{self, ZwpTextInputManagerV3},
            zwp_text_input_v3::{self, ZwpTextInputV3},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            backend::ClientId, protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Rectangle},
    wayland::seat::WaylandFocus,
};

use crate::State;

/// The text inputs of every client.
#[derive(Debug, Default)]
pub struct TextInputs {
    inputs: Vec<Input>,
}

#[derive(Debug)]
struct Input {
    resource: ZwpTextInputV3,
    /// The surface it entered, if it is in one.
    entered: Option<WlSurface>,
    /// As of its last commit: enabled, and its caret (surface-local).
    enabled: bool,
    cursor: Option<Rectangle<i32, Logical>>,
    /// What it asked since: `Some(true)` enable, `Some(false)` disable.
    pending_enable: Option<bool>,
    pending_cursor: Option<Rectangle<i32, Logical>>,
    /// Its commits so far, for `done`.
    commits: u32,
    /// The surface it was enabled on when the keyboard left it: the
    /// application will enable it again as the keyboard comes back.
    left_enabled: Option<WlSurface>,
}

impl TextInputs {
    /// The text input enabled in `surface`, as its client last committed.
    fn active(&self, surface: &WlSurface) -> Option<&Input> {
        self.inputs
            .iter()
            .find(|i| i.enabled && i.entered.as_ref() == Some(surface))
    }

    /// Where the caret of `surface`'s enabled text input is, surface-local.
    pub fn caret(&self, surface: &WlSurface) -> Option<Rectangle<i32, Logical>> {
        self.active(surface)?.cursor
    }

    /// Whether `surface` takes text through a text input: one is enabled
    /// in it, or was as the keyboard left it.
    pub fn takes_text(&self, surface: &WlSurface) -> bool {
        self.active(surface).is_some()
            || self
                .inputs
                .iter()
                .any(|i| i.left_enabled.as_ref() == Some(surface))
    }

    /// Whether `surface` has a text input enabled now.
    pub fn enabled(&self, surface: &WlSurface) -> bool {
        self.active(surface).is_some()
    }

    /// `text` goes into `surface`'s enabled text input; whether one took
    /// it.
    pub fn commit_string(&self, surface: &WlSurface, text: &str) -> bool {
        let Some(input) = self.active(surface) else {
            return false;
        };
        input.resource.commit_string(Some(text.to_owned()));
        input.resource.done(input.commits);
        true
    }

    /// An input method's text being composed into `surface`'s enabled
    /// text input, with its cursor (bytes): whether one took it. The
    /// compositor has no input method yet; the tests compose as one would
    /// (#763).
    pub fn preedit_string(&self, surface: &WlSurface, text: &str, cursor: (i32, i32)) -> bool {
        let Some(input) = self.active(surface) else {
            return false;
        };
        input
            .resource
            .preedit_string(Some(text.to_owned()), cursor.0, cursor.1);
        input.resource.done(input.commits);
        true
    }

    /// The keyboard went to `focus`: the text inputs in the surface it left
    /// leave it, those of its client enter it.
    pub fn focus_changed(&mut self, focus: Option<&WlSurface>) {
        for input in &mut self.inputs {
            if let Some(old) = &input.entered
                && Some(old) != focus
            {
                if input.enabled {
                    input.left_enabled = Some(old.clone());
                }
                if old.is_alive() {
                    input.resource.leave(old);
                }
                input.entered = None;
                input.enabled = false;
                input.cursor = None;
            }
            if let Some(surface) = focus
                && input.entered.is_none()
                && surface.id().same_client_as(&input.resource.id())
            {
                input.resource.enter(surface);
                input.entered = Some(surface.clone());
            }
        }
    }
}

impl State {
    /// The surface holding the keyboard, if it is a Wayland one.
    pub(crate) fn keyboard_surface(&self) -> Option<WlSurface> {
        let focus = self.seat.get_keyboard()?.current_focus()?;
        focus.wl_surface().map(|s| s.into_owned())
    }
}

impl GlobalDispatch<ZwpTextInputManagerV3, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwpTextInputManagerV3>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwpTextInputManagerV3, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ZwpTextInputManagerV3,
        request: zwp_text_input_manager_v3::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwp_text_input_manager_v3::Request::GetTextInput { id, .. } = request {
            let resource = data_init.init(id, ());
            let mut input = Input {
                resource,
                entered: None,
                enabled: false,
                cursor: None,
                pending_enable: None,
                pending_cursor: None,
                commits: 0,
                left_enabled: None,
            };
            // Its client may hold the keyboard already.
            if let Some(surface) = state.keyboard_surface()
                && surface.id().same_client_as(&input.resource.id())
            {
                input.resource.enter(&surface);
                input.entered = Some(surface);
            }
            state.emoji.inputs.inputs.push(input);
        }
    }
}

impl Dispatch<ZwpTextInputV3, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &ZwpTextInputV3,
        request: zwp_text_input_v3::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let Some(input) = state
            .emoji
            .inputs
            .inputs
            .iter_mut()
            .find(|i| &i.resource == resource)
        else {
            return;
        };
        match request {
            zwp_text_input_v3::Request::Enable => {
                input.pending_enable = Some(true);
                input.pending_cursor = None;
            }
            zwp_text_input_v3::Request::Disable => input.pending_enable = Some(false),
            zwp_text_input_v3::Request::SetCursorRectangle {
                x,
                y,
                width,
                height,
            } => {
                input.pending_cursor = Some(Rectangle::new((x, y).into(), (width, height).into()));
            }
            zwp_text_input_v3::Request::Commit => {
                input.commits = input.commits.wrapping_add(1);
                match input.pending_enable.take() {
                    Some(true) => {
                        input.enabled = input.entered.is_some();
                        input.cursor = None;
                        input.left_enabled = None;
                    }
                    Some(false) => {
                        input.enabled = false;
                        input.cursor = None;
                    }
                    None => {}
                }
                let mut caret_moved = false;
                if let Some(cursor) = input.pending_cursor.take()
                    && input.enabled
                {
                    caret_moved = input.cursor != Some(cursor);
                    input.cursor = Some(cursor);
                }
                let enabled = input.enabled;
                let entered = input.entered.clone();
                if enabled {
                    // An emoji may be waiting for it.
                    state.emoji_text_input_ready();
                }
                // The magnifier keeps the caret in its view (#424).
                if caret_moved && let Some(surface) = entered {
                    state.magnifier_caret(&surface);
                }
            }
            // Surrounding text, its change's cause and the content's type
            // are an input method's business: nothing here uses them.
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &ZwpTextInputV3, _: &()) {
        state
            .emoji
            .inputs
            .inputs
            .retain(|i| &i.resource != resource);
    }
}

/// The text-input-v3 global.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, ZwpTextInputManagerV3, ()>(1, ());
}
