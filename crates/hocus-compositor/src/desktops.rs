//! Per-screen desktops: the core model decides which windows each output
//! shows and which one has focus; this module makes the space and the seat
//! follow it.

use hocus_api::Event;
use hocus_core::{DesktopId, ModelError, OutputName, Rect, WindowId};
use smithay::{
    desktop::Window,
    output::Output,
    utils::{Logical, Point, Rectangle, Serial},
    wayland::{
        compositor::with_states, foreign_toplevel_list::ForeignToplevelHandle,
        shell::xdg::XdgToplevelSurfaceData,
    },
};

use crate::{State, api};

/// A window the compositor manages: its smithay window, and where it goes
/// when its desktop is shown again.
#[derive(Debug, Clone)]
pub struct Managed {
    pub window: Window,
    pub location: Point<i32, Logical>,
    /// Its ext-foreign-toplevel-list handle.
    pub foreign: ForeignToplevelHandle,
}

/// The model's id of a window, kept in its user data.
#[derive(Debug, Clone, Copy)]
struct ModelId(WindowId);

pub fn model_id(window: &Window) -> Option<WindowId> {
    window.user_data().get::<ModelId>().map(|id| id.0)
}

pub fn rect(r: Rectangle<i32, Logical>) -> Rect {
    Rect {
        x: r.loc.x,
        y: r.loc.y,
        width: r.size.w,
        height: r.size.h,
    }
}

impl State {
    /// Adds an output to the model, with `desktops` desktops the first time
    /// it is seen.
    pub fn plug_output(&mut self, output: &Output, desktops: usize) {
        let geometry = self.space.output_geometry(output).unwrap_or_default();
        if let Err(err) = self
            .model
            .plug(OutputName(output.name()), rect(geometry), desktops)
        {
            tracing::warn!(output = output.name(), %err, "could not add the output");
        }
        self.sync_desktops();
    }

    pub fn output_moved(&mut self, output: &Output) {
        let geometry = self.space.output_geometry(output).unwrap_or_default();
        if let Err(err) = self
            .model
            .set_geometry(&OutputName(output.name()), rect(geometry))
        {
            tracing::warn!(output = output.name(), %err, "could not move the output");
        }
    }

    /// The output under the pointer, or the first one.
    pub fn output_under_pointer(&self) -> Option<Output> {
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        self.space
            .output_under(pointer)
            .next()
            .or_else(|| self.space.outputs().next())
            .cloned()
    }

    /// Starts managing a window that has its size: on the shown desktop of
    /// `output`, centred, focused.
    pub fn manage(&mut self, window: Window, output: &Output, serial: Serial) {
        let name = OutputName(output.name());
        let id = match self.model.open_window(&name) {
            Ok(id) => id,
            Err(err) => {
                tracing::warn!(output = output.name(), %err, "could not open the window");
                return;
            }
        };
        window
            .user_data()
            .insert_if_missing_threadsafe(|| ModelId(id));
        let area = self.space.output_geometry(output).unwrap_or_default();
        let size = window.geometry().size;
        let location =
            area.loc + Point::from(((area.size.w - size.w) / 2, (area.size.h - size.h) / 2));
        let (title, app_id) = title_and_app_id(&window);
        let foreign = self
            .foreign_toplevels
            .new_toplevel::<State>(title.unwrap_or_default(), app_id.unwrap_or_default());
        self.windows.insert(
            id,
            Managed {
                window: window.clone(),
                location,
                foreign,
            },
        );
        self.sync_desktops();
        self.emit(|s| Event::WindowOpened {
            window: s.window_info(&window),
        });
        self.apply_focus(serial);
    }

    /// Stops managing a window that is gone.
    pub fn unmanage(&mut self, window: &Window, serial: Serial) {
        let Some(id) = model_id(window) else {
            return;
        };
        if let Some(managed) = self.windows.remove(&id) {
            self.foreign_toplevels.remove_toplevel(&managed.foreign);
        }
        self.space.unmap_elem(window);
        if let Err(err) = self.model.close_window(id) {
            tracing::warn!(%err, "closing a window the model did not know");
        }
        self.sync_desktops();
        self.emit(|_| Event::WindowClosed { id: id.0 });
        self.apply_focus(serial);
    }

    /// Focuses and raises a window; `None` leaves no window focused.
    pub fn focus(&mut self, window: Option<&Window>, serial: Serial) {
        match window.and_then(model_id) {
            Some(id) => {
                if let Err(err) = self.model.focus(id) {
                    tracing::warn!(%err, "focusing a window the model did not know");
                }
            }
            None => self.model.clear_focus(),
        }
        self.sync_desktops();
        self.apply_focus(serial);
    }

    /// Shows the `n`th desktop (from 0) of an output. An output with fewer
    /// desktops, one with a single desktop among them, does not change.
    pub fn show_desktop(
        &mut self,
        output: &Output,
        n: usize,
        serial: Serial,
    ) -> Result<(), ModelError> {
        let name = OutputName(output.name());
        let before = self.model.output(&name).map(|o| o.shown);
        self.model.show_nth(&name, n)?;
        let after = self.model.output(&name).map(|o| o.shown);
        if before != after {
            self.sync_desktops();
            self.emit(|_| Event::DesktopShown {
                output: name.0.clone(),
                desktop: after.map(|d| d.0).unwrap_or_default(),
            });
            self.apply_focus(serial);
        }
        Ok(())
    }

    /// Sends a window to an output: its `n`th desktop, or the shown one.
    pub fn send_window(
        &mut self,
        id: WindowId,
        output: &Output,
        n: Option<usize>,
        serial: Serial,
    ) -> Result<(), ModelError> {
        let name = OutputName(output.name());
        let desktop: Option<DesktopId> = match n {
            Some(n) => Some(
                self.model
                    .output(&name)
                    .ok_or(ModelError::UnknownOutput)?
                    .desktops
                    .get(n)
                    .copied()
                    .ok_or(ModelError::UnknownDesktop)?,
            ),
            None => None,
        };
        let managed = self
            .windows
            .get(&id)
            .cloned()
            .ok_or(ModelError::UnknownWindow)?;
        let current = self
            .space
            .element_location(&managed.window)
            .unwrap_or(managed.location);
        let old_area = self
            .output_of(&managed.window)
            .and_then(|o| self.space.output_geometry(o));
        self.model.send_to_output(id, &name, desktop)?;
        // It keeps its place relative to the output it leaves, inside the new one.
        let area = self.space.output_geometry(output).unwrap_or_default();
        let offset = old_area.map(|a| current - a.loc).unwrap_or_default();
        let location = clamp_into(area.loc + offset, managed.window.geometry().size, area);
        self.space.unmap_elem(&managed.window);
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.location = location;
        }
        self.sync_desktops();
        self.apply_focus(serial);
        Ok(())
    }

    /// Maps the windows the model shows, bottom to top, where they were;
    /// unmaps the others and remembers where they were.
    pub fn sync_desktops(&mut self) {
        for managed in self.windows.values_mut() {
            if let Some(location) = self.space.element_location(&managed.window) {
                managed.location = location;
            }
        }
        let shown: Vec<WindowId> = self
            .model
            .outputs()
            .iter()
            .flat_map(|o| self.model.visible_windows(&o.name).iter().copied())
            .collect();
        for (id, managed) in &self.windows {
            if !shown.contains(id) {
                self.space.unmap_elem(&managed.window);
            }
        }
        for id in &shown {
            if let Some(managed) = self.windows.get(id) {
                self.space
                    .map_element(managed.window.clone(), managed.location, false);
            }
        }
        self.refresh_workspaces();
        self.needs_redraw = true;
    }

    /// Gives keyboard focus to the model's focused window, activates it, and
    /// says so when it changed.
    pub fn apply_focus(&mut self, serial: Serial) {
        let focused = self
            .model
            .focused()
            .and_then(|id| self.windows.get(&id))
            .map(|m| m.window.clone());
        let before = self.focused_window();
        for managed in self.windows.values() {
            managed
                .window
                .set_activated(Some(&managed.window) == focused.as_ref());
            if let Some(toplevel) = managed.window.toplevel() {
                toplevel.send_pending_configure();
            }
        }
        let surface = focused
            .as_ref()
            .and_then(|w| w.toplevel())
            .map(|t| t.wl_surface().clone());
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface, serial);
        }
        if before != focused {
            self.emit(|_| Event::WindowFocused {
                id: focused.as_ref().map(api::window_id),
            });
        }
        self.needs_redraw = true;
    }

    /// Tells taskbars that a window's title or app id changed.
    pub fn window_renamed(&mut self, window: &Window) {
        let Some(managed) = model_id(window).and_then(|id| self.windows.get(&id)) else {
            return;
        };
        let (title, app_id) = title_and_app_id(window);
        managed.foreign.send_title(&title.unwrap_or_default());
        managed.foreign.send_app_id(&app_id.unwrap_or_default());
        managed.foreign.send_done();
    }
}

/// What the client said its window is.
pub fn title_and_app_id(window: &Window) -> (Option<String>, Option<String>) {
    window
        .toplevel()
        .map(|toplevel| {
            with_states(toplevel.wl_surface(), |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|data| data.lock().ok())
                    .map(|data| (data.title.clone(), data.app_id.clone()))
                    .unwrap_or_default()
            })
        })
        .unwrap_or_default()
}

/// `location` moved so that a window of `size` lies inside `area` as much as
/// it can.
fn clamp_into(
    location: Point<i32, Logical>,
    size: smithay::utils::Size<i32, Logical>,
    area: Rectangle<i32, Logical>,
) -> Point<i32, Logical> {
    let clamp =
        |v: i32, lo: i32, len: i32, area_len: i32| v.clamp(lo, lo + (area_len - len).max(0));
    Point::from((
        clamp(location.x, area.loc.x, size.w, area.size.w),
        clamp(location.y, area.loc.y, size.h, area.size.h),
    ))
}
