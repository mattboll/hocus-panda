//! A surface's rows for screen readers: each popover, and each surface of
//! the widget kit's own, says as it draws what it shows by the kit's kinds
//! (buttons, switches, sliders, radio buttons, fields, lines), the order
//! its keyboard's ring goes through them, and which one the ring is on.
//! Only one popover shows at a time; a screen reader's click on a row is
//! the keys that would reach it (`Tab` to it, then `Enter`), so that every
//! popover does what it does for the keyboard.

use std::sync::{Mutex, PoisonError};

use hocus_a11y::{Node, Rect};
use iced::{Rectangle, window};

/// What a popover shows, for screen readers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rows {
    /// The line under its title.
    pub subtitle: Option<String>,
    pub nodes: Vec<Node>,
    /// The keys of the rows the keyboard's ring goes through, in its
    /// order (`Tab`).
    pub order: Vec<String>,
    /// The row the ring is on.
    pub focus: Option<String>,
    /// The rows are gone through with the arrows (a list whose selection
    /// moves), not `Tab`.
    pub arrows: bool,
}

/// The surfaces that said what they show, the last last: a popover, and
/// the kit's surfaces of their own (the actions palette, the emoji
/// picker…), which may show beside it.
static ROWS: Mutex<Vec<(window::Id, Rows)>> = Mutex::new(Vec::new());

/// How many surfaces' rows are kept: those closed go first.
const KEPT: usize = 8;

/// Surface `surface` shows `rows` now.
pub fn publish(surface: window::Id, rows: Rows) {
    let mut kept = ROWS.lock().unwrap_or_else(PoisonError::into_inner);
    match kept.iter_mut().find(|(s, _)| *s == surface) {
        Some((_, shown)) => {
            if *shown != rows {
                *shown = rows;
            }
        }
        None => {
            if kept.len() >= KEPT {
                kept.remove(0);
            }
            kept.push((surface, rows));
        }
    }
}

/// What surface `surface` last said it shows.
pub fn of(surface: window::Id) -> Option<Rows> {
    ROWS.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(s, _)| *s == surface)
        .map(|(_, r)| r.clone())
}

/// A rectangle of the popover's, as a node's bounds.
pub fn at(rect: Rectangle) -> Option<Rect> {
    Some(Rect::new(rect.x, rect.y, rect.width, rect.height))
}

/// The `Tab`s that take the ring from `focus` to `to` in `order` (round
/// its end; from nothing, `Tab` goes to the first).
pub fn tabs(order: &[String], focus: Option<&str>, to: &str) -> Option<usize> {
    let to = order.iter().position(|k| k == to)?;
    let n = order.len();
    Some(
        match focus.and_then(|f| order.iter().position(|k| k == f)) {
            Some(at) => (to + n - at) % n,
            None => to + 1,
        },
    )
}

/// The arrows that take the selection from `focus` to `to` in `order`
/// (`true`: down), the first from nothing being `Down`.
pub fn arrows(order: &[String], focus: Option<&str>, to: &str) -> Option<(usize, bool)> {
    let to = order.iter().position(|k| k == to)?;
    Some(
        match focus.and_then(|f| order.iter().position(|k| k == f)) {
            Some(at) if to >= at => (to - at, true),
            Some(at) => (at - to, false),
            None => (to + 1, true),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_goes_to_a_row_by_tabs() {
        let order: Vec<String> = ["a", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(tabs(&order, None, "a"), Some(1));
        assert_eq!(tabs(&order, None, "c"), Some(3));
        assert_eq!(tabs(&order, Some("b"), "b"), Some(0));
        assert_eq!(tabs(&order, Some("c"), "a"), Some(1));
        assert_eq!(tabs(&order, Some("a"), "c"), Some(2));
        assert_eq!(tabs(&order, Some("a"), "z"), None);
        assert_eq!(arrows(&order, None, "b"), Some((2, true)));
        assert_eq!(arrows(&order, Some("c"), "a"), Some((2, false)));
        assert_eq!(arrows(&order, Some("b"), "c"), Some((1, true)));
    }
}
