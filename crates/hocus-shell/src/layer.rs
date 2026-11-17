//! The shell's layer surfaces opened, resized and closed through
//! iced_exwlshell's messages, in the shapes the shell says them: a size as
//! (width, height), 0 on an axis for "as long as its anchors make it", and
//! margins as (top, right, bottom, left).
//!
//! iced_exwlshell changes a surface's size only together with its anchor
//! (`LayoutChange`): each surface's anchor is kept from its opening for its
//! resizes, and forgotten as it closes.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use iced::{Rectangle, window};
use iced_exwlshell::{
    InputRegion,
    actions::RegionActionCB,
    reexport::{Anchor, Extent, LayerSize, Margin, NewLayerShellSettings},
};

use crate::Message;

/// The anchors the surfaces open were opened with.
static ANCHORS: Mutex<Option<HashMap<window::Id, Anchor>>> = Mutex::new(None);

fn anchors<T>(f: impl FnOnce(&mut HashMap<window::Id, Anchor>) -> T) -> T {
    f(ANCHORS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new))
}

/// A size as the shell says it: 0 on an axis leaves it to the anchors.
pub fn size((width, height): (u32, u32)) -> LayerSize {
    let extent = |px| Extent::try_px(px).unwrap_or(Extent::Fill);
    LayerSize {
        width: extent(width),
        height: extent(height),
    }
}

/// Margins as the shell says them: top, right, bottom, left.
pub fn margin((top, right, bottom, left): (i32, i32, i32, i32)) -> Margin {
    Margin {
        top,
        right,
        bottom,
        left,
    }
}

/// Opens surface `id` with `settings`.
pub fn open(id: window::Id, settings: NewLayerShellSettings) -> Message {
    anchors(|anchors| anchors.insert(id, settings.anchor));
    Message::NewLayerShell { settings, id }
}

/// Surface `id` to `size`, against the anchor it opened with; nothing if
/// it is not open.
pub fn resize(id: window::Id, to: (u32, u32)) -> Option<Message> {
    let Some(anchor) = anchors(|anchors| anchors.get(&id).copied()) else {
        tracing::debug!(?id, "a size for a surface not open");
        return None;
    };
    Some(Message::LayoutChange {
        id,
        anchor,
        size: size(to),
    })
}

/// Wider than any surface: a region as large as can be.
const EVERYWHERE: f32 = (1 << 20) as f32;

/// All of a surface, as an input region.
pub const ALL: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: EVERYWHERE,
    height: EVERYWHERE,
};

/// Surface `id` takes the pointer in `rects` only (each grown to whole
/// pixels), nowhere if there are none.
pub fn input(id: window::Id, rects: impl IntoIterator<Item = Rectangle>) -> Message {
    let rects: Vec<(i32, i32, i32, i32)> = rects
        .into_iter()
        .map(|r| {
            let (x, y) = (r.x.floor() as i32, r.y.floor() as i32);
            let right = (r.x + r.width).ceil() as i32;
            let bottom = (r.y + r.height).ceil() as i32;
            (x, y, right - x, bottom - y)
        })
        .collect();
    // Each region iced_exwlshell hands is new, empty.
    let callback = RegionActionCB::new(move |region: &mut InputRegion| {
        for &(x, y, width, height) in &rects {
            if let Err(err) = region.add(x, y, width, height) {
                tracing::warn!(%err, "a surface's input region");
            }
        }
    });
    Message::SetInputRegion { id, callback }
}

/// Closes surface `id`.
pub fn close(id: window::Id) -> Message {
    closed(id);
    Message::RemoveWindow(id)
}

/// Surface `id` is gone, closed by the shell or by the compositor.
pub fn closed(id: window::Id) {
    anchors(|anchors| anchors.remove(&id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_leaves_its_axis_to_the_anchors() {
        assert_eq!(size((0, 0)), LayerSize::FILL);
        assert_eq!(size((300, 0)), LayerSize::fill_height(300));
        assert_eq!(size((300, 40)), LayerSize::px(300, 40));
    }

    #[test]
    fn a_surface_resizes_against_its_anchor_until_it_closes() {
        let id = window::Id::unique();
        let anchor = Anchor::Right | Anchor::Bottom;
        let settings = NewLayerShellSettings {
            anchor,
            ..NewLayerShellSettings::default()
        };
        assert!(matches!(open(id, settings), Message::NewLayerShell { .. }));
        assert!(matches!(
            resize(id, (10, 20)),
            Some(Message::LayoutChange { anchor: a, size: s, .. }) if a == anchor && s == LayerSize::px(10, 20)
        ));
        assert!(matches!(close(id), Message::RemoveWindow(i) if i == id));
        assert!(resize(id, (10, 20)).is_none());
    }
}
