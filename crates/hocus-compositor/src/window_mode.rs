//! Maximized, fullscreen and minimized windows.

use hocus_core::{Rect, WindowId, Zone};
use smithay::{
    desktop::Window,
    output::Output,
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{Logical, Point, Rectangle, Serial},
};

use crate::{
    State,
    desktops::rect,
    surface::{Shown, request_size},
};

/// A core rectangle as smithay's.
pub fn rectangle(r: Rect) -> Rectangle<i32, Logical> {
    Rectangle::new((r.x, r.y).into(), (r.width, r.height).into())
}

/// How a window is shown. Maximized and fullscreen remember the geometry
/// they go back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowMode {
    #[default]
    Normal,
    Maximized {
        restore: Rectangle<i32, Logical>,
    },
    Fullscreen {
        restore: Rectangle<i32, Logical>,
        /// Fullscreen left back to maximized.
        was_maximized: bool,
    },
    /// Snapped to a half or a quarter of the work area.
    Snapped {
        zone: Zone,
        restore: Rectangle<i32, Logical>,
    },
    /// Laid out by its desktop's tiling.
    Tiled {
        restore: Rectangle<i32, Logical>,
    },
}

impl WindowMode {
    /// The place and size to go back to, when laid out.
    pub fn restore(self) -> Option<Rectangle<i32, Logical>> {
        match self {
            Self::Normal => None,
            Self::Maximized { restore }
            | Self::Snapped { restore, .. }
            | Self::Tiled { restore } => Some(restore),
            Self::Fullscreen { restore, .. } => Some(restore),
        }
    }
}

impl State {
    /// Where windows go when maximized or placed: the output minus what the
    /// shell reserves (nothing yet; the column will) and the outer gap.
    pub fn work_area(&self, output: &Output) -> Rectangle<i32, Logical> {
        let mut area = self.space.output_geometry(output).unwrap_or_default();
        // What layer surfaces reserve (third-party bars).
        if let Some(zone) = self.non_exclusive_zone(output)
            && let Some(inside) = area.intersection(zone)
        {
            area = inside;
        }
        // The widget column takes the right of its output.
        if let Some(column) = self.column_rect(output) {
            area.size.w = (column.loc.x - area.loc.x).max(1);
        }
        let gap = i32::try_from(self.config.windows.gaps.outer).unwrap_or(0);
        let inner = Rectangle::new(
            area.loc + Point::from((gap, gap)),
            (area.size.w - 2 * gap, area.size.h - 2 * gap).into(),
        );
        if inner.size.w > 0 && inner.size.h > 0 {
            inner
        } else {
            area
        }
    }

    /// Lays maximized, snapped and fullscreen windows out again: the work
    /// area, the gaps or the screen changed.
    pub fn relayout(&mut self) {
        let laid_out: Vec<(WindowId, WindowMode)> = self
            .windows
            .iter()
            .filter(|(_, m)| {
                matches!(
                    m.mode,
                    WindowMode::Maximized { .. }
                        | WindowMode::Snapped { .. }
                        | WindowMode::Fullscreen { .. }
                )
            })
            .map(|(id, m)| (*id, m.mode))
            .collect();
        for (id, mode) in laid_out {
            let Some(output) = self.output_for(id) else {
                continue;
            };
            let area = self.work_area(&output);
            match mode {
                WindowMode::Snapped { zone, .. } => {
                    let rect = rectangle(zone.rect(rect(area), self.config.windows.gaps.inner));
                    self.configure(id, rect, None);
                }
                WindowMode::Fullscreen { .. } => {
                    let full = self.space.output_geometry(&output).unwrap_or(area);
                    self.configure(id, full, Some(xdg_toplevel::State::Fullscreen));
                }
                _ => self.configure(id, area, Some(xdg_toplevel::State::Maximized)),
            }
        }
        self.sync_desktops();
    }

    /// Snaps a window to a zone of its output's work area; the top
    /// maximizes.
    pub fn snap(&mut self, id: WindowId, zone: Zone, serial: Serial) {
        if zone == Zone::Top {
            self.set_maximized(id, true, serial);
            return;
        }
        let Some(mode) = self.windows.get(&id).map(|m| m.mode) else {
            return;
        };
        if matches!(mode, WindowMode::Fullscreen { .. }) {
            return;
        }
        let (Some(output), Some(current)) = (self.output_for(id), self.current_rect(id)) else {
            return;
        };
        let restore = mode.restore().unwrap_or(current);
        let target = rectangle(zone.rect(
            rect(self.work_area(&output)),
            self.config.windows.gaps.inner,
        ));
        self.configure(id, target, None);
        self.set_mode(id, WindowMode::Snapped { zone, restore }, serial);
    }

    /// Back to normal from maximized or snapped, at the place it had.
    pub fn unlay(&mut self, id: WindowId, serial: Serial) {
        match self.windows.get(&id).map(|m| m.mode) {
            Some(WindowMode::Maximized { .. }) => self.set_maximized(id, false, serial),
            Some(WindowMode::Snapped { restore, .. }) => {
                self.configure(id, restore, None);
                self.set_mode(id, WindowMode::Normal, serial);
            }
            // Out of the tiling: floating at its own size.
            Some(WindowMode::Tiled { .. }) => {
                self.model.set_floating(id, true);
                self.sync_desktops();
                self.apply_focus(serial);
            }
            _ => {}
        }
    }

    /// Places the windows of tiled desktops, and gives the others that were
    /// tiled their floating place back. Only windows whose place changes
    /// are told.
    pub fn layout_tiles(&mut self) {
        let mut tiled = std::collections::BTreeSet::new();
        let outputs: Vec<_> = self.model.outputs().to_vec();
        for out in outputs {
            let Some(output) = self.find_output(&out.name.0) else {
                continue;
            };
            let area = rect(self.work_area(&output));
            for desktop in &out.desktops {
                if !self.model.is_tiling(*desktop) {
                    continue;
                }
                let tiles = self.model.tiles(*desktop);
                let ratio = self.model.master_ratio(*desktop);
                let rects = hocus_core::tiling::layout(
                    area,
                    tiles.len(),
                    ratio,
                    self.config.windows.gaps.inner,
                );
                for (id, r) in tiles.into_iter().zip(rects) {
                    let Some(mode) = self.windows.get(&id).map(|m| m.mode) else {
                        continue;
                    };
                    let restore = match mode {
                        WindowMode::Normal => self.current_rect(id),
                        WindowMode::Tiled { restore } => Some(restore),
                        // Maximized, fullscreen and snapped windows keep
                        // their own place.
                        _ => None,
                    };
                    let Some(restore) = restore else {
                        continue;
                    };
                    tiled.insert(id);
                    let target = rectangle(r);
                    if self.current_rect(id) != Some(target) {
                        self.configure(id, target, None);
                    }
                    if let Some(managed) = self.windows.get_mut(&id) {
                        managed.mode = WindowMode::Tiled { restore };
                    }
                }
            }
        }
        let released: Vec<(WindowId, Rectangle<i32, Logical>)> = self
            .windows
            .iter()
            .filter(|(id, _)| !tiled.contains(*id))
            .filter_map(|(id, m)| match m.mode {
                WindowMode::Tiled { restore } => Some((*id, restore)),
                _ => None,
            })
            .collect();
        for (id, restore) in released {
            self.configure(id, restore, None);
            if let Some(managed) = self.windows.get_mut(&id) {
                managed.mode = WindowMode::Normal;
            }
        }
    }

    /// The window's current place and size.
    pub fn current_rect(&self, id: WindowId) -> Option<Rectangle<i32, Logical>> {
        let managed = self.windows.get(&id)?;
        let location = self
            .space
            .element_location(&managed.window)
            .unwrap_or(managed.location);
        // The frame: the bar above the client.
        let bar = self.bar_height(id);
        let size = managed.window.geometry().size;
        Some(Rectangle::new(
            location - Point::from((0, bar)),
            (size.w, size.h + bar).into(),
        ))
    }

    /// The output a window is on, or the one under the pointer.
    fn output_for(&self, id: WindowId) -> Option<Output> {
        self.windows
            .get(&id)
            .and_then(|m| self.output_of(&m.window).cloned())
            .or_else(|| self.output_under_pointer())
    }

    pub fn set_maximized(&mut self, id: WindowId, on: bool, serial: Serial) {
        let Some(mode) = self.windows.get(&id).map(|m| m.mode) else {
            return;
        };
        let next = match (mode, on) {
            (WindowMode::Normal | WindowMode::Snapped { .. }, true) => {
                let (Some(current), Some(output)) = (self.current_rect(id), self.output_for(id))
                else {
                    return;
                };
                let restore = mode.restore().unwrap_or(current);
                let area = self.work_area(&output);
                self.configure(id, area, Some(xdg_toplevel::State::Maximized));
                WindowMode::Maximized { restore }
            }
            (WindowMode::Maximized { restore }, false) => {
                self.configure(id, restore, None);
                WindowMode::Normal
            }
            // Fullscreen wins; it remembers to come back maximized.
            (WindowMode::Fullscreen { restore, .. }, on) => WindowMode::Fullscreen {
                restore,
                was_maximized: on,
            },
            (mode, _) => mode,
        };
        self.set_mode(id, next, serial);
    }

    /// Fullscreen on `output`, or on the window's own output.
    pub fn set_fullscreen(
        &mut self,
        id: WindowId,
        on: bool,
        output: Option<Output>,
        serial: Serial,
    ) {
        let Some(mode) = self.windows.get(&id).map(|m| m.mode) else {
            return;
        };
        let next = match (mode, on) {
            (
                WindowMode::Fullscreen {
                    restore,
                    was_maximized,
                },
                false,
            ) => {
                if was_maximized && let Some(output) = self.output_for(id) {
                    let area = self.work_area(&output);
                    self.configure(id, area, Some(xdg_toplevel::State::Maximized));
                    WindowMode::Maximized { restore }
                } else {
                    self.configure(id, restore, None);
                    WindowMode::Normal
                }
            }
            (WindowMode::Fullscreen { .. }, true) => mode,
            (mode, true) => {
                let Some(output) = output.or_else(|| self.output_for(id)) else {
                    return;
                };
                let (restore, was_maximized) = match mode {
                    WindowMode::Maximized { restore } => (restore, true),
                    WindowMode::Snapped { restore, .. } => (restore, false),
                    _ => match self.current_rect(id) {
                        Some(rect) => (rect, false),
                        None => return,
                    },
                };
                let whole = self.space.output_geometry(&output).unwrap_or_default();
                self.configure(id, whole, Some(xdg_toplevel::State::Fullscreen));
                // Above everything on its output.
                let _ = self.model.focus(id);
                WindowMode::Fullscreen {
                    restore,
                    was_maximized,
                }
            }
            (mode, false) => mode,
        };
        self.set_mode(id, next, serial);
    }

    pub fn minimize(&mut self, id: WindowId, serial: Serial) {
        if let Err(err) = self.model.minimize(id) {
            tracing::warn!(%err, "minimizing a window the model did not know");
            return;
        }
        self.sync_desktops();
        self.apply_focus(serial);
    }

    /// Brings a minimized window back, focused, where it was.
    pub fn restore(&mut self, id: WindowId, serial: Serial) {
        if let Err(err) = self.model.focus(id) {
            tracing::warn!(%err, "restoring a window the model did not know");
            return;
        }
        self.sync_desktops();
        self.apply_focus(serial);
    }

    pub fn toggle_maximized(&mut self, id: WindowId, serial: Serial) {
        match self.windows.get(&id).map(|m| m.mode) {
            Some(WindowMode::Maximized { .. }) => self.set_maximized(id, false, serial),
            _ => self.set_maximized(id, true, serial),
        }
    }

    fn set_mode(&mut self, id: WindowId, mode: WindowMode, serial: Serial) {
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.mode = mode;
        }
        self.sync_desktops();
        self.apply_focus(serial);
    }

    /// Asks the client for `rect`'s size with `state` on (and the others
    /// off), and moves the window to `rect`'s corner.
    fn configure(
        &mut self,
        id: WindowId,
        rect: Rectangle<i32, Logical>,
        state: Option<xdg_toplevel::State>,
    ) {
        // Fullscreen has no bar.
        let rect = match state {
            Some(xdg_toplevel::State::Fullscreen) => rect,
            _ if self
                .windows
                .get(&id)
                .is_some_and(|m| self.wants_bar(&m.window)) =>
            {
                Rectangle::new(
                    rect.loc + Point::from((0, crate::bars::BAR)),
                    (rect.size.w, (rect.size.h - crate::bars::BAR).max(1)).into(),
                )
            }
            _ => rect,
        };
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        let shown = match state {
            Some(xdg_toplevel::State::Maximized) => Shown::Maximized,
            Some(xdg_toplevel::State::Fullscreen) => Shown::Fullscreen,
            _ => Shown::Normal,
        };
        request_size(&managed.window, rect, shown, false);
        managed.location = rect.loc;
        let window: Window = managed.window.clone();
        // Mapped again at its new place by the next sync.
        self.space.unmap_elem(&window);
    }
}
