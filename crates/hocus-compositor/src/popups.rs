//! Popup grabs (xdg_popup.grab): a menu takes the keyboard while it is
//! open, and a press on anything that is not its application's closes it,
//! the press going nowhere. Nested menus stack; each closing gives the
//! keyboard back to the one under it, the last to its window.

use smithay::{
    reexports::wayland_server::Resource,
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::shell::xdg::PopupSurface,
};

use crate::{State, focus::KeyboardFocus, model::WindowId};

/// The menus holding the keyboard, and their window.
#[derive(Debug)]
pub struct PopupGrab {
    pub window: WindowId,
    /// Outermost first.
    pub chain: Vec<PopupSurface>,
}

impl State {
    /// A menu of window `window` asks for the keyboard.
    pub fn grab_popup(&mut self, window: WindowId, popup: PopupSurface) {
        match &mut self.popup_grab {
            Some(grab) if grab.window == window => grab.chain.push(popup),
            _ => {
                self.end_popup_grab();
                self.popup_grab = Some(PopupGrab {
                    window,
                    chain: vec![popup],
                });
            }
        }
        self.give_keyboard();
    }

    /// The keyboard goes to the innermost menu.
    pub fn popup_focus(&self) -> Option<KeyboardFocus> {
        let grab = self.popup_grab.as_ref()?;
        let popup = grab.chain.iter().rev().find(|p| p.alive())?;
        Some(KeyboardFocus::Wayland(popup.wl_surface().clone()))
    }

    fn give_keyboard(&mut self) {
        let focus = self.popup_focus().or_else(|| {
            let id = self.model.focused()?;
            let window = &self.windows.get(&id)?.window;
            match window.x11_surface() {
                Some(x11) => Some(KeyboardFocus::X11(x11.clone())),
                None => window
                    .toplevel()
                    .map(|t| KeyboardFocus::Wayland(t.wl_surface().clone())),
            }
        });
        if !self.keyboard_held_by_layer()
            && let Some(keyboard) = self.seat.get_keyboard()
        {
            keyboard.set_focus(self, focus, SERIAL_COUNTER.next_serial());
        }
    }

    /// A menu went: the keyboard goes back to the one under it.
    pub fn popup_gone(&mut self, popup: &PopupSurface) {
        let Some(grab) = &mut self.popup_grab else {
            return;
        };
        let before = grab.chain.len();
        grab.chain.retain(|p| p != popup && p.alive());
        if grab.chain.len() == before {
            return;
        }
        if grab.chain.is_empty() {
            self.popup_grab = None;
        }
        self.give_keyboard();
    }

    /// Every menu of the grab closes.
    pub fn end_popup_grab(&mut self) {
        let Some(grab) = self.popup_grab.take() else {
            return;
        };
        for popup in grab.chain.iter().rev() {
            popup.send_popup_done();
        }
    }

    /// A window went or hid: its menus go with it.
    pub fn end_popup_grab_of(&mut self, id: WindowId) {
        if self.popup_grab.as_ref().is_some_and(|g| g.window == id) {
            self.end_popup_grab();
        }
    }

    /// A press at `at` while menus hold the keyboard: away from their
    /// application, they close and the press goes nowhere. Whether it did.
    pub fn dismiss_popups_at(&mut self, at: Point<f64, Logical>) -> bool {
        let Some(grab) = &self.popup_grab else {
            return false;
        };
        let Some(root) = grab.chain.first().map(|p| p.wl_surface().clone()) else {
            self.popup_grab = None;
            return false;
        };
        let own = self
            .surface_under(at)
            .is_some_and(|(surface, _)| surface.id().same_client_as(&root.id()));
        if own {
            return false;
        }
        self.end_popup_grab();
        self.give_keyboard();
        true
    }
}
