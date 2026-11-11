//! What the pointer, the keyboard and the touchpad's gestures leave
//! between two events.

use std::{collections::HashSet, time::Duration};

use smithay::{
    input::{
        keyboard::Keycode,
        pointer::{CursorIcon, CursorImageStatus},
    },
    output::Output,
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
}

#[derive(Debug, Default)]
pub struct Gestures {
    /// The screen whose strip follows three fingers on the touchpad.
    pub swiping: Option<Output>,
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
        }
    }
}
