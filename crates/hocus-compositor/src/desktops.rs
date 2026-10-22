//! Per-screen desktops: the core model decides which windows each output
//! shows and which one has focus; this module makes the space and the seat
//! follow it.

use std::collections::BTreeMap;

use hocus_api::Event;
use hocus_core::{DesktopId, ModelError, OutputName, Rect, WindowId};
use smithay::{
    desktop::Window,
    output::Output,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Serial, Size},
    wayland::foreign_toplevel_list::ForeignToplevelHandle,
    wayland::seat::WaylandFocus as _,
};

use crate::{
    State, api,
    surface::{self, Shown},
    window_mode::WindowMode,
};

pub use crate::surface::title_and_app_id;

/// Window places per layout of outputs (see [`State::remember_places`]).
pub type Places = BTreeMap<String, BTreeMap<WindowId, Point<i32, Logical>>>;

/// A window the compositor manages: its smithay window, and where it goes
/// when its desktop is shown again.
#[derive(Debug, Clone)]
pub struct Managed {
    pub window: Window,
    pub location: Point<i32, Logical>,
    /// Its ext-foreign-toplevel-list handle.
    pub foreign: ForeignToplevelHandle,
    pub mode: WindowMode,
    /// 0.1 to 1.
    pub opacity: f32,
    /// Stacked above the other windows.
    pub on_top: bool,
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
    ///
    /// Call [`State::remember_places`] before mapping it in the space.
    pub fn plug_output(&mut self, output: &Output, desktops: usize) {
        let geometry = self.space.output_geometry(output).unwrap_or_default();
        if let Err(err) = self
            .model
            .plug(OutputName(output.name()), rect(geometry), desktops)
        {
            tracing::warn!(output = output.name(), %err, "could not add the output");
        }
        self.restore_places();
        self.sync_desktops();
        self.apply_focus(SERIAL_COUNTER.next_serial());
    }

    /// Removes an output: the windows it showed go to the primary output,
    /// at the same offset from its corner, and come back when it does.
    pub fn unplug_output(&mut self, output: &Output) {
        self.remember_places();
        let name = OutputName(output.name());
        let area = self.space.output_geometry(output).unwrap_or_default();
        let leaving: Vec<(WindowId, Point<i32, Logical>)> = self
            .model
            .visible_windows(&name)
            .iter()
            .filter_map(|id| {
                let managed = self.windows.get(id)?;
                let at = self
                    .space
                    .element_location(&managed.window)
                    .unwrap_or(managed.location);
                Some((*id, at - area.loc))
            })
            .collect();
        if let Err(err) = self.model.unplug(&name) {
            tracing::warn!(output = output.name(), %err, "could not remove the output");
        }
        self.space.unmap_output(output);
        if let Some(host) = self
            .model
            .primary()
            .and_then(|o| self.find_output(&o.name.0))
            .and_then(|o| self.space.output_geometry(&o))
        {
            for (id, offset) in leaving {
                if let Some(managed) = self.windows.get_mut(&id) {
                    self.space.unmap_elem(&managed.window);
                    managed.location =
                        clamp_into(host.loc + offset, managed.window.geometry().size, host);
                }
            }
        }
        self.restore_places();
        self.sync_desktops();
        self.apply_focus(SERIAL_COUNTER.next_serial());
        self.emit(|_| Event::OutputRemoved {
            name: name.0.clone(),
        });
    }

    /// Notes where every window is, for the current set of outputs and their
    /// layout: when that layout comes back, so do the windows.
    pub fn remember_places(&mut self) {
        let key = self.layout_key();
        let places: BTreeMap<WindowId, Point<i32, Logical>> = self
            .windows
            .iter()
            .map(|(id, m)| {
                let at = self.space.element_location(&m.window).unwrap_or(m.location);
                (*id, at)
            })
            .collect();
        self.places.insert(key, places);
    }

    /// Puts windows back where they were in this layout, if it was seen.
    fn restore_places(&mut self) {
        let Some(places) = self.places.get(&self.layout_key()) else {
            return;
        };
        for (id, at) in places {
            if let Some(managed) = self.windows.get_mut(id) {
                self.space.unmap_elem(&managed.window);
                managed.location = *at;
            }
        }
    }

    /// The outputs in the space and where they are, as one key.
    fn layout_key(&self) -> String {
        let mut outputs: Vec<String> = self
            .space
            .outputs()
            .filter_map(|o| {
                let g = self.space.output_geometry(o)?;
                Some(format!(
                    "{}@{},{}:{}x{}",
                    o.name(),
                    g.loc.x,
                    g.loc.y,
                    g.size.w,
                    g.size.h
                ))
            })
            .collect();
        outputs.sort();
        outputs.join(" ")
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
        let (title, app_id) = title_and_app_id(&window);
        let rule = self.config.rule_for(
            app_id.as_deref().unwrap_or(""),
            title.as_deref().unwrap_or(""),
        );
        // A rule's output, when it is plugged.
        let output = rule
            .output
            .as_deref()
            .and_then(|name| self.find_output(name))
            .unwrap_or_else(|| output.clone());
        let output = &output;
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
        if let Some(n) = rule.desktop
            && let Some(desktop) = self
                .model
                .output(&name)
                .and_then(|o| o.desktops.get(n as usize - 1).copied())
            && let Err(err) = self.model.send_to_desktop(id, desktop)
        {
            tracing::warn!(%err, "a rule's desktop");
        }
        if rule.pinned == Some(true) {
            let _ = self.model.set_pinned(id, true);
        }
        let size = rule
            .size
            .map(|[w, h]| Size::from((w, h)))
            .unwrap_or(window.geometry().size);
        // Placed with its bar.
        let bar = if self.wants_bar(&window) {
            crate::bars::BAR
        } else {
            0
        };
        let location =
            self.place(output, Size::from((size.w, size.h + bar))) + Point::from((0, bar));
        let opacity = rule
            .opacity
            .map(|o| o as f32)
            .or_else(|| {
                app_id
                    .as_ref()
                    .and_then(|a| self.app_opacity.get(a).copied())
            })
            .unwrap_or(1.0);
        let foreign = self.foreign_toplevels.new_toplevel::<State>(
            title.clone().unwrap_or_default(),
            app_id.clone().unwrap_or_default(),
        );
        self.windows.insert(
            id,
            Managed {
                window: window.clone(),
                location,
                foreign,
                mode: WindowMode::Normal,
                opacity,
                on_top: rule.on_top == Some(true),
            },
        );
        if rule.size.is_some() {
            let frame = Rectangle::new(
                location - Point::from((0, bar)),
                (size.w, size.h + bar).into(),
            );
            self.resize_to(id, frame, false);
        }
        self.sync_desktops();
        self.emit(|s| Event::WindowOpened {
            window: s.window_info(&window),
        });
        self.apply_focus(serial);
        if rule.maximized == Some(true) {
            self.set_maximized(id, true, serial);
        }
        // Asked before its first buffer.
        match surface::wanted(&window) {
            Shown::Fullscreen => self.set_fullscreen(id, true, None, serial),
            Shown::Maximized => self.set_maximized(id, true, serial),
            Shown::Normal => {}
        }
        self.adopt_dropdown(id, app_id.as_deref(), serial);
    }

    /// Where a new window of `size` goes on `output`: centred in its work
    /// area, then one step down and right for each window already there, so
    /// that none hides another; back to the top when it would leave the
    /// area.
    fn place(&self, output: &Output, size: Size<i32, Logical>) -> Point<i32, Logical> {
        const STEP: i32 = 32;
        let area = self.work_area(output);
        let centred =
            area.loc + Point::from(((area.size.w - size.w) / 2, (area.size.h - size.h) / 2));
        let taken: Vec<Point<i32, Logical>> = self
            .model
            .visible_windows(&OutputName(output.name()))
            .iter()
            .filter_map(|id| self.windows.get(id))
            .filter_map(|m| self.space.element_location(&m.window))
            .collect();
        let mut at = centred;
        for _ in 0..taken.len() {
            if !taken
                .iter()
                .any(|p| (p.x - at.x).abs() < 4 && (p.y - at.y).abs() < 4)
            {
                break;
            }
            at += Point::from((STEP, STEP));
            if at.x + size.w > area.loc.x + area.size.w || at.y + size.h > area.loc.y + area.size.h
            {
                at = area.loc + Point::from((STEP, STEP));
            }
        }
        clamp_into(at, size, area)
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
        self.layout_tiles();
        for managed in self.windows.values_mut() {
            if let Some(location) = self.space.element_location(&managed.window) {
                managed.location = location;
            }
        }
        let shown: Vec<WindowId> = self
            .model
            .outputs()
            .iter()
            .flat_map(|o| self.model.visible_windows(&o.name))
            .collect();
        for (id, managed) in &self.windows {
            if !shown.contains(id) {
                self.space.unmap_elem(&managed.window);
            }
        }
        // Always on top last, so above the others.
        let mut shown = shown;
        shown.sort_by_key(|id| self.windows.get(id).is_some_and(|m| m.on_top));
        for id in &shown {
            // A window never shows off its output: one that would is brought
            // inside it.
            let area = self
                .model
                .output_showing(*id)
                .and_then(|o| self.find_output(&o.name.0))
                .and_then(|o| self.space.output_geometry(&o));
            if let Some(managed) = self.windows.get_mut(id) {
                if let Some(area) = area {
                    let size = managed.window.geometry().size;
                    let inside = Rectangle::new(managed.location, size).overlaps(area);
                    if !inside {
                        managed.location = clamp_into(managed.location, size, area);
                    }
                }
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
            surface::activate(&managed.window, Some(&managed.window) == focused.as_ref());
        }
        let surface = focused
            .as_ref()
            .and_then(|w| w.wl_surface())
            .map(|s| s.into_owned());
        // X11 clients see their own stacking order.
        if let Some(xwm) = self.xwm.as_mut()
            && let Some(x11) = focused.as_ref().and_then(|w| w.x11_surface())
            && let Err(err) = xwm.raise_window(x11)
        {
            tracing::debug!(%err, "could not raise an X11 window");
        }
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
