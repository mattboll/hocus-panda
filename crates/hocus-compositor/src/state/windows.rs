//! The windows on their way in and out, and where a dragged one would go.

use std::{collections::HashMap, time::Duration};

use smithay::{
    backend::renderer::element::Id, desktop::Window,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
};

use crate::{
    model::{self, Rect, WindowId},
    motion::Tween,
    snapshot::{Grabber, Snapshot},
    windows::{Leaving, Preview},
};

#[derive(Debug, Default)]
pub struct Arrivals {
    /// Toplevels whose first buffer has not come: mapped once it does.
    pub unmapped: Vec<Window>,
    /// Activation tokens handed out, when, and whether they may focus a
    /// window (made from the user's input, or by the desktop).
    pub activation_tokens: HashMap<String, (Duration, bool)>,
    /// The user started a program through the desktop then: its window
    /// takes the focus.
    pub spawned_at: Option<Duration>,
    /// Windows not mapped yet that asked for the focus with a valid token.
    pub activate_on_map: Vec<WlSurface>,
    /// Windows not mapped yet that asked to open maximised or fullscreen
    /// (§10): they show so from their first frame.
    pub opens_as: Vec<(WlSurface, model::Mode)>,
    /// The last click on a tray icon, while its application may still
    /// focus a window with it (5.14b).
    pub tray_click: Option<crate::tray::TrayClick>,
}

#[derive(Debug, Default)]
pub struct Departures {
    pub grabber: Grabber,
    /// Windows' pictures taken as their surfaces went.
    pub pictures: HashMap<WindowId, Snapshot>,
    pub leaving: Vec<Leaving>,
    /// The windows' rains so far, for their seeds (#432).
    pub rain_seeds: u32,
}

#[derive(Debug)]
pub struct Snap {
    /// Where a dragged window would snap, and the one fading out.
    pub preview: Option<Preview>,
    pub preview_out: Option<(Rect, Tween)>,
    pub preview_ids: [Id; 9],
    /// The handle shown on the seam of a pair near the pointer,
    /// fading like a grip (§8).
    pub seam_handle: Option<(model::Seam, crate::windows::Grip)>,
    pub seam_handle_id: Id,
}

impl Default for Snap {
    fn default() -> Self {
        Self {
            preview: None,
            preview_out: None,
            preview_ids: std::array::from_fn(|_| Id::new()),
            seam_handle: None,
            seam_handle_id: Id::new(),
        }
    }
}
