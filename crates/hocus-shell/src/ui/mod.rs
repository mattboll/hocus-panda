//! The shell's widget kit (#770, decision 0005): a surface is iced's stock
//! widgets (`container`, `row`, `column`, `button`, `scrollable`, `text`,
//! `mouse_area`), a few of ours, and canvases only where the look draws
//! (a glyph, a switch's knob, a slider's track).
//!
//! A surface builds its controls once per view, each declared where it is
//! built ([`control`]: its id, its role and name for screen readers, its
//! state, what the keyboard's ring does on it). From that one tree:
//!
//! - the pointer's hit-testing and hover are the stock widgets';
//! - the ring's order, `Tab`, the arrows, `Enter`, `Space`, `Escape` are
//!   [`keys`], over the controls in the tree's order;
//! - the ring is drawn by the control holding it, outside its bounds;
//! - screen readers' nodes are the controls' declarations with the bounds
//!   they were laid out at ([`walk`], published by [`Around`]);
//! - tests read the same walk in the log (`shows {…}`), by id.
//!
//! [`Around`] is the popover's surface: it handles its widgets' messages
//! at once and builds its own view again, not the bar's. The shared text
//! field ([`crate::field`]) is a widget here ([`field::Field`]).
//!
//! How a surface is built with it: docs/design/bar.md, "The shell's widget
//! kit".

pub mod control;
pub mod field;
pub mod keys;
pub mod notched;
pub mod parts;
pub mod sized;
pub mod slider;
pub mod style;
pub mod surface;
pub mod unsaid;
pub mod walk;

pub use control::{control, line, ring};
pub use surface::{Around, Later, touch};
pub use unsaid::unsaid;
pub use walk::reveal;
