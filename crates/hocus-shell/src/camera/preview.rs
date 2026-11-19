//! The camera window's preview: what the others see, read from the
//! Hocuspanda source while the window shows (`hocus_camera::preview`),
//! each frame made 624 × 352 (the preview's 312 × 176 at scale 2) and kept
//! as an image for the window to draw. Its thread goes as the window
//! closes, and with the last reader the camera goes off.

use std::{
    sync::{Mutex, PoisonError},
    time::Instant,
};

use iced::widget::image;

/// The preview's frames' size.
pub const SIZE: (u32, u32) = (624, 352);

struct Live {
    _preview: hocus_camera::preview::Preview,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);
/// The last frame, and when it came.
static FRAME: Mutex<Option<(image::Handle, Instant)>> = Mutex::new(None);

/// The window opens: the source is read.
pub fn start() {
    let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    if live.is_some() {
        return;
    }
    *FRAME.lock().unwrap_or_else(PoisonError::into_inner) = None;
    let preview = hocus_camera::preview::start(None, SIZE, |frame| {
        let handle = image::Handle::from_rgba(frame.size.0, frame.size.1, frame.rgba);
        let first = FRAME
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace((handle, Instant::now()))
            .is_none();
        if first {
            tracing::debug!("the camera's preview shows a frame");
            crate::popover::changed(crate::popover::Item::Camera);
            crate::popover::changed(crate::popover::Item::Microphone);
        }
    });
    tracing::debug!(started = preview.is_some(), "the camera's preview");
    *live = preview.map(|p| Live { _preview: p });
}

/// The window closes: the source is no longer read.
pub fn stop() {
    if LIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .is_some()
    {
        tracing::debug!("the camera's preview ends");
    }
    *FRAME.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// The last frame, and when it came.
pub fn frame() -> Option<(image::Handle, Instant)> {
    FRAME.lock().unwrap_or_else(PoisonError::into_inner).clone()
}
