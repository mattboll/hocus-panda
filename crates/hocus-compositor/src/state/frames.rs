//! What the frames are drawn with and when: the GPU, the glass and the
//! wallpapers, the motions' time.

use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use smithay::xwayland::X11Wm;

#[derive(Debug, Default)]
pub struct Graphics {
    /// The render node of the GPU the compositor draws with, which the
    /// shell draws with too; `None` without one.
    pub gpu: Option<crate::backend::DevId>,
    /// The renderer can blur what is behind the bars (GLES).
    pub blur: bool,
    /// The glass behind each bar, as last blurred.
    pub panes: RefCell<crate::glass::Panes>,
    /// The screens' wallpapers (6.3), drawn on a thread of their own.
    pub backgrounds: RefCell<crate::wallpaper::Backgrounds>,
    /// The screens' wallpapers that move (#433).
    pub walls: RefCell<crate::moving::Walls>,
}

#[derive(Debug, Default)]
pub struct Frames {
    /// The motions' time while a test holds it (`Headless::hold_clock`):
    /// it moves only as the test steps it, so that a motion is seen at
    /// the same moments however slowly a loaded machine draws.
    pub held_clock: Option<Duration>,
    /// How far ahead of `clock` the motions' time is since a test let it
    /// go, having stepped it faster than it ran: it never goes back.
    pub clock_ahead: Duration,
    /// A frame is asked for at this time, after a pause.
    pub wake: Option<Duration>,
    /// When the last frame was drawn: motions are judged as of then (see
    /// `tick`). Set while drawing, which borrows the state shared.
    pub drawn_at: Cell<Option<Duration>>,
    /// Nothing moved as of the last frame drawn, as `tick` judged it then:
    /// that frame shows every motion at its end (the tests' `at_rest`).
    pub still: bool,
}

#[derive(Debug, Default)]
pub struct XWayland {
    /// The X11 window manager once XWayland is ready, and its display.
    pub wm: Option<X11Wm>,
    pub display: Option<u32>,
}
