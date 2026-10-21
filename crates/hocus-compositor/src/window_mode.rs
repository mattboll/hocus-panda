//! Maximized, fullscreen and minimized windows.

use hocus_core::WindowId;
use smithay::{
    desktop::Window,
    output::Output,
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{Logical, Point, Rectangle, Serial},
};

use crate::State;

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
}

impl State {
    /// Where windows go when maximized or placed: the output minus what the
    /// shell reserves (nothing yet; the column will) and the outer gap.
    pub fn work_area(&self, output: &Output) -> Rectangle<i32, Logical> {
        let area = self.space.output_geometry(output).unwrap_or_default();
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

    /// Lays maximized windows out again: the work area changed.
    pub fn relayout(&mut self) {
        let maximized: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, m)| matches!(m.mode, WindowMode::Maximized { .. }))
            .map(|(id, _)| *id)
            .collect();
        for id in maximized {
            if let Some(output) = self.output_for(id) {
                let area = self.work_area(&output);
                self.configure(id, area, Some(xdg_toplevel::State::Maximized));
            }
        }
        self.sync_desktops();
    }

    /// The window's current place and size.
    fn current_rect(&self, id: WindowId) -> Option<Rectangle<i32, Logical>> {
        let managed = self.windows.get(&id)?;
        let location = self
            .space
            .element_location(&managed.window)
            .unwrap_or(managed.location);
        Some(Rectangle::new(location, managed.window.geometry().size))
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
            (WindowMode::Normal, true) => {
                let (Some(restore), Some(output)) = (self.current_rect(id), self.output_for(id))
                else {
                    return;
                };
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
        let on = matches!(
            self.windows.get(&id).map(|m| m.mode),
            Some(WindowMode::Normal)
        );
        self.set_maximized(id, on, serial);
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
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        if let Some(toplevel) = managed.window.toplevel() {
            toplevel.with_pending_state(|pending| {
                pending.size = Some(rect.size);
                for s in [
                    xdg_toplevel::State::Maximized,
                    xdg_toplevel::State::Fullscreen,
                ] {
                    if Some(s) == state {
                        pending.states.set(s);
                    } else {
                        pending.states.unset(s);
                    }
                }
            });
            toplevel.send_configure();
        }
        managed.location = rect.loc;
        let window: Window = managed.window.clone();
        // Mapped again at its new place by the next sync.
        self.space.unmap_elem(&window);
    }
}
