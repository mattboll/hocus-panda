//! What the pointer, the keyboard and the touchpad's gestures leave
//! between two events.

use std::{collections::HashSet, time::Duration};

use smithay::{
    backend::input::KeyState,
    input::{
        keyboard::Keycode,
        pointer::{CursorIcon, CursorImageStatus},
    },
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
};

use crate::{cursor::CursorImages, model::WindowId, popups::PopupGrab};

#[derive(Debug)]
pub struct Pointer {
    /// The pointer and the overview's hot corners (#727).
    pub hot_corner: crate::hot_corner::HotCorner,
    /// The user has moved the pointer (a device's motion; in a headless
    /// compositor, from its start: tests put it where they want): screens
    /// lit after no longer bring it to the primary screen's middle.
    pub placed: bool,
    /// The cursor over a window's frame or grip, or during a grab: it
    /// wins over the client's.
    pub frame_cursor: Option<CursorIcon>,
    /// The last press on a grip, for a double click.
    pub last_grip_press: Option<(WindowId, Duration)>,
    /// Buttons whose press the desktop kept: their release too.
    pub kept_buttons: HashSet<u32>,
    /// The last press, where (global) and when: a window closed by a
    /// click shatters from there (#798).
    pub last_press: Option<(Point<f64, Logical>, Duration)>,
    pub popup_grab: Option<PopupGrab>,
    /// What the pointer shows, as the client under it sets it.
    pub cursor: CursorImageStatus,
    pub cursor_images: CursorImages,
}

#[derive(Debug)]
pub struct Keyboard {
    /// Keys whose press was a shortcut: their release is kept from the
    /// clients too.
    pub kept_keys: HashSet<Keycode>,
    /// The keymap's layouts' XKB codes (`fr`, `fr(bepo)`), by group, for
    /// the bar's row; empty when XKB refused them and took its defaults.
    pub layout_codes: Vec<String>,
    /// The keys typed for a layer surface that takes the keyboard before
    /// it has drawn (#821).
    pub coming: Coming,
}

/// A layer surface asking for the keyboard always, between its first
/// commit and its first buffer. Its client may not read keys yet: iced
/// makes a surface's window at its first configure's refresh, and drops
/// the keys and the keyboard's entering before it (#821, the launcher's
/// Escape typed as it opened on a busy shell). So it gets the keyboard
/// once it has drawn, and the keys typed meanwhile then, in their order.
#[derive(Debug, Default)]
pub enum Coming {
    /// No such surface: the keys go where the keyboard is.
    #[default]
    Nothing,
    /// It has not drawn yet: its keys wait, since the monotonic `since`.
    Waiting {
        surface: WlSurface,
        since: Duration,
        keys: Vec<(Keycode, KeyState, u32)>,
    },
    /// It did not draw within [`Coming::PATIENCE`]: it has the keyboard
    /// all the same, and its keys as they come, as any client.
    Late(WlSurface),
}

impl Coming {
    /// How long keys wait for a surface to draw: a busy shell draws within
    /// it, and a client that never draws holds no key longer.
    pub const PATIENCE: Duration = Duration::from_secs(1);
}

#[derive(Debug, Default)]
pub struct Gestures {
    /// The screens whose strips follow three fingers on the touchpad (all
    /// of them when linked, #947); none while no swipe goes on.
    pub swiping: Vec<Output>,
    /// The touchpad's swipe, and whether the desktop takes it (6.13).
    pub hold: crate::gestures::Hold,
    /// Swipes given to clients so far, and their moves, for tests.
    pub swipes_given: (u64, u64),
}

impl Pointer {
    pub(super) fn new(cursor_images: CursorImages) -> Self {
        Self {
            hot_corner: crate::hot_corner::HotCorner::default(),
            placed: false,
            frame_cursor: None,
            last_grip_press: None,
            kept_buttons: HashSet::new(),
            last_press: None,
            popup_grab: None,
            cursor: CursorImageStatus::default_named(),
            cursor_images,
        }
    }
}

impl Keyboard {
    pub(super) fn new(layout_codes: Vec<String>) -> Self {
        Self {
            kept_keys: HashSet::new(),
            layout_codes,
            coming: Coming::Nothing,
        }
    }
}
