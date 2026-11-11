//! A popover's rows for screen readers: each popover says, as it draws,
//! what it shows by the kit's kinds (buttons, switches, sliders, radio
//! buttons, fields, lines), the order its keyboard's ring goes through
//! them, and which one the ring is on. Only one popover shows at a time;
//! a screen reader's click on a row is the keys that would reach it (`Tab`
//! to it, then `Enter`), so that every popover does what it does for the
//! keyboard.

use std::sync::{Mutex, PoisonError};

use hocus_a11y::{Act, Node, Rect, Role};
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

static ROWS: Mutex<Option<(window::Id, Rows)>> = Mutex::new(None);

/// Popover `surface` shows `rows` now.
pub fn publish(surface: window::Id, rows: Rows) {
    let mut kept = ROWS.lock().unwrap_or_else(PoisonError::into_inner);
    if kept
        .as_ref()
        .is_none_or(|(s, r)| *s != surface || *r != rows)
    {
        *kept = Some((surface, rows));
    }
}

/// What popover `surface` last said it shows.
pub fn of(surface: window::Id) -> Option<Rows> {
    ROWS.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|(s, _)| *s == surface)
        .map(|(_, r)| r.clone())
}

/// A rectangle of the popover's, as a node's bounds.
pub fn at(rect: Rectangle) -> Option<Rect> {
    Some(Rect::new(rect.x, rect.y, rect.width, rect.height))
}

/// A target's key: its debug form.
pub fn key(target: impl std::fmt::Debug) -> String {
    format!("{target:?}")
}

/// A row the keyboard's ring goes to (`target`), `Enter` acting on it.
pub fn act(
    target: impl std::fmt::Debug,
    role: Role,
    name: impl Into<String>,
    rect: Rectangle,
) -> Node {
    Node::new(key(target), role, name)
        .bounds(at(rect))
        .act(Act::Click)
}

/// A line of text that nothing holds: a note, a section's label.
pub fn line(text: impl Into<String>) -> Node {
    let text = text.into();
    Node::new(format!("line:{text}"), Role::Label, text)
}

/// The keys of `targets`, the ring's order.
pub fn order<T: std::fmt::Debug>(targets: impl IntoIterator<Item = T>) -> Vec<String> {
    targets.into_iter().map(key).collect()
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
