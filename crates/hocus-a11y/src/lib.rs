//! What the shell shows, for screen readers (docs/design/screen-reader.md).
//!
//! iced exposes no accessibility tree (docs/design/spike-iced.md, 6): the
//! shell describes each of its surfaces itself, from the controls its
//! widget kit declares (decision 0005) and the model it draws, as a
//! [`Window`] of [`Node`]s (role, name, value, states, bounds,
//! actions), and the [`Bridge`] hands them to AT-SPI through AccessKit,
//! one top-level window each. AccessKit's adapter runs on a thread of its
//! own with its own D-Bus connection; the shell only hands it owned trees,
//! and what a screen reader asks comes back as an [`Event`].
//!
//! Nothing is built while no assistive technology listens ([`listening`]):
//! the shell asks before describing anything, and [`built`] counts the
//! trees made, for the counted budgets.

mod bridge;
mod tree;

pub use bridge::{Asked, Bridge, Event, built, listening};
pub use tree::{Act, Node, Range, Rect, Role, Window};
