//! The side of the screens the bars stand on (`[panel] side`, 6.4), as
//! the compositor says it (`bar-side`).
//!
//! The bar's surfaces, and what opens beside it (the popovers, the panda
//! peeking out, the reminders' tabs, the tooltips), are placed from the
//! screen's right edge wherever they are computed. On the left, their
//! anchors and margins are mirrored here, as they are opened or moved:
//! what is beside the bar stays beside it, on its inner side. What is
//! drawn inside a surface is not mirrored; the few surfaces whose drawing
//! leans on the bar's side ask [`left`].
//!
//! A change of side comes with each bar made again (`nobar`, then `bar`):
//! no surface is moved across.

use std::{
    collections::HashSet,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use hocus_link::Side;
use iced::window;
use iced_layershell::reexport::Anchor;

/// Whether the bars stand on the left.
static LEFT: AtomicBool = AtomicBool::new(false);

/// The popovers' surfaces opened beside the bar: their margins are
/// mirrored as they move.
static BESIDE: Mutex<Option<HashSet<window::Id>>> = Mutex::new(None);

/// The side the compositor says.
pub fn set(side: Side) {
    tracing::info!(side = side.name(), "the bars' side");
    LEFT.store(side == Side::Left, Ordering::Relaxed);
}

/// Whether the bars stand on the screens' left.
pub fn left() -> bool {
    LEFT.load(Ordering::Relaxed)
}

/// `anchor`, said for a bar on the right, for the bar's side.
pub fn anchor(anchor: Anchor) -> Anchor {
    mirrored_anchor(anchor, left())
}

fn mirrored_anchor(anchor: Anchor, left: bool) -> Anchor {
    if !left || anchor.contains(Anchor::Left) == anchor.contains(Anchor::Right) {
        return anchor;
    }
    let mut mirrored = anchor - (Anchor::Left | Anchor::Right);
    if anchor.contains(Anchor::Right) {
        mirrored |= Anchor::Left;
    } else {
        mirrored |= Anchor::Right;
    }
    mirrored
}

/// Layer-shell margins (top, right, bottom, left), said for a bar on the
/// right, for the bar's side.
pub fn margin(margin: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    mirrored_margin(margin, left())
}

fn mirrored_margin(
    (top, right, bottom, left_): (i32, i32, i32, i32),
    left: bool,
) -> (i32, i32, i32, i32) {
    if left {
        (top, left_, bottom, right)
    } else {
        (top, right, bottom, left_)
    }
}

/// Surface `id` opens beside the bar: its later margins are mirrored too
/// ([`moved`]).
pub fn opened(id: window::Id) {
    BESIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_or_insert_with(HashSet::new)
        .insert(id);
}

/// Surface `id` is gone.
pub fn gone(id: window::Id) {
    if let Some(ids) = BESIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
    {
        ids.remove(&id);
    }
}

/// Surface `id`'s new margins: mirrored if it opened beside the bar.
pub fn moved(id: window::Id, to: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let beside = BESIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|ids| ids.contains(&id));
    if beside { margin(to) } else { to }
}

/// The distance from the screen's edge on the bar's side to `x`, on a
/// surface `width` wide placed with `margin` (said for a bar on the right):
/// where a tooltip under something drawn on it goes.
pub fn from_edge(margin: (i32, i32, i32, i32), width: f32, x: f32) -> f32 {
    from_edge_on(left(), margin, width, x)
}

fn from_edge_on(left: bool, margin: (i32, i32, i32, i32), width: f32, x: f32) -> f32 {
    if left {
        margin.1 as f32 + x
    } else {
        margin.1 as f32 + width - x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_the_left_the_right_becomes_the_left() {
        let top_right = Anchor::Right | Anchor::Top;
        assert_eq!(mirrored_anchor(top_right, true), Anchor::Left | Anchor::Top);
        assert_eq!(mirrored_anchor(top_right, false), top_right);
        let column = Anchor::Top | Anchor::Bottom;
        assert_eq!(mirrored_anchor(column, true), column);
        let all = Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right;
        assert_eq!(mirrored_anchor(all, true), all);
        assert_eq!(mirrored_margin((1, 2, 3, 4), true), (1, 4, 3, 2));
        assert_eq!(mirrored_margin((1, 2, 3, 4), false), (1, 2, 3, 4));
        assert_eq!(from_edge_on(true, (0, 212, 0, 0), 100.0, 30.0), 242.0);
        assert_eq!(from_edge_on(false, (0, 212, 0, 0), 100.0, 30.0), 282.0);
    }
}
