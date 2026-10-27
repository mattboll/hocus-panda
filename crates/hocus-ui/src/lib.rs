//! The shell's widget toolkit: a retained tree of widgets laid out in rows
//! and columns, drawn on the CPU (vello_cpu, parley; decision 0009) into a
//! surface that the compositor uploads as a texture, and drawn again only
//! where something changed.
//!
//! - [`Tree`]: nodes, flex-like layout, hit testing, keyboard focus, input,
//!   the accessibility tree.
//! - [`Surface`]: the pixels, kept between frames.
//! - [`Theme`]: colours, radii, spacing and the type scale.
//! - [`motion`]: durations, easings and springs, evaluated at a given time.
//!
//! Nothing here knows about smithay or Wayland, and nothing reads the
//! clock: the caller passes the time of the frame.

pub mod access;
pub mod canvas;
pub mod geom;
pub mod motion;
pub mod shape;
pub mod text;
pub mod theme;
pub mod tree;

pub use access::{AccessNode, Role};
pub use canvas::Canvas;
pub use geom::{Insets, PixelRect, Point, Rect, Size};
pub use shape::{Shape, Transform};
pub use theme::{
    Accent, Casting, Color, Daylight, Face, Paint, Shadow, TextStyle, Theme, ThemeName, Typeface,
};
pub use tree::{
    Align, Button, ButtonKind, Cast, Direction, Draw, Event, Glass, Icon, Input, Justify, Key,
    Kind, Length, Look, NodeId, Picture, Property, Style, Surface, TextField, Tree,
};
