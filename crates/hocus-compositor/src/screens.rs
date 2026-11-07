//! Screens coming and going (docs/design/windows.md §12): the windows of a
//! screen unplugged go to one still there, each on the same-numbered
//! desktop and in its state; when a screen of the same identity comes
//! back ([`crate::screen_sets`]), those the user did not place meanwhile
//! fly back where they were in 320 ms (fade in place with reduced
//! motion). The winking panda says what moved ([`State::screens_peek`]).
//!
//! What clients are told when a screen goes comes in an order they rely
//! on (#536): every surface leaves its `wl_output` before the global goes,
//! and nothing names that output after. A client that drops its
//! `wl_output` on `global_remove` (kitty) would otherwise get a
//! `wl_surface.leave` naming an object it no longer has, which its
//! libwayland hands it as null.

use std::time::Duration;

use smithay::{
    desktop::{LayerSurface, layer_map_for_output, space::SpaceElement},
    output::Output,
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_server::backend::GlobalId,
    },
    utils::{Point, SERIAL_COUNTER},
};

use crate::{
    State,
    model::{self, Mode, Rect, WindowId},
    motion::{Bezier, Tween},
    windows::rect_of,
};

/// Where a window was on a screen that went, and where it was put then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameFrom {
    /// The screen's key (its identity, [`crate::screen_sets`]), which it
    /// has again when it comes back, on any connector.
    pub output: String,
    pub desktop: usize,
    /// The screen's size, and its work area from its top-left corner.
    pub size: (i32, i32),
    pub area: Rect,
    /// The window's place, and where it floated before it snapped, from
    /// the screen's top-left corner.
    pub rect: Rect,
    pub restore: Option<Rect>,
    /// Where it was put: as long as it is still there, the user has not
    /// placed it elsewhere.
    pub put: Put,
}

/// Where a window is, as far as the user places it: its screen, its
/// desktop, its state, and a floating window's place and size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Put {
    output: String,
    desktop: usize,
    mode: Mode,
    rect: Option<Rect>,
}

/// A screen moved or resized (6.9): its work area before, its geometry
/// and work area now, and the windows that were on it.
#[derive(Debug, Clone)]
pub(crate) struct Moved {
    pub output: Output,
    pub was_area: Rect,
    pub geometry: Rect,
    pub area: Rect,
    pub windows: Vec<WindowId>,
}

/// How long an unplugged screen's `wl_output` global stays, disabled,
/// before it is removed: a client binding it as it goes (its
/// `global_remove` not read yet) still finds it, rather than being killed
/// for binding a global that is no longer there.
const GLOBAL_LINGERS: Duration = Duration::from_secs(5);

/// `rect` moved by `by`'s corner, or back with `-1`.
fn shifted(rect: Rect, by: Rect, sign: i32) -> Rect {
    Rect::new(
        rect.x + sign * by.x,
        rect.y + sign * by.y,
        rect.width,
        rect.height,
    )
}

impl State {
    /// Screen `output`, whose `wl_output` global is `global`, is unplugged
    /// (by the tty backend, or by a test on the headless one): it leaves
    /// the desktop as `output_gone` says, every surface on it is told it
    /// left, and only then do clients learn that its global went
    /// (docs/design/windows.md §12, "What clients are told").
    pub fn unplug_output(&mut self, output: &Output, global: GlobalId) {
        self.perf
            .event(hocus_perf::EventKind::ScreenUnplugged, output.name());
        self.display_removed(output);
        self.output_gone(output, true);
        // Also when it had no place on the desktop, which `output_gone`
        // then leaves as it is: nothing may be left on it.
        self.leave_output(output);
        self.close_layers_on(output);
        self.retire_output_global(global);
    }

    /// `output` leaves the desktop: a drag or a resize under way ends, and
    /// Alt+Tab or a swipe on it; its layer surfaces are closed; its
    /// windows go to the screen still there nearest to it, on the
    /// same-numbered desktop and in the same state, floating ones at their
    /// place across the screen, inside its work area. The focus and the
    /// pointer end up on a screen still there. Every surface on it leaves
    /// it, before its global goes.
    fn output_gone(&mut self, output: &Output, say: bool) {
        let name = output.name();
        let key = self.screen_key(&name);
        let (Some(geometry), Some(area)) = (
            self.space.output_geometry(output).map(rect_of),
            self.work_area(output),
        ) else {
            return;
        };
        self.end_on_output(output);
        self.share_output_gone(&name);
        self.record_output_gone(&name);
        self.ink.output_gone(&name);
        // Each window's place there, and where it came from first if it
        // came from another screen gone before and stayed where it was put.
        let leaving: Vec<(WindowId, Rect, Option<Rect>, Option<CameFrom>)> = self
            .windows_on(output)
            .into_iter()
            .filter_map(|id| {
                let rect = self.home_rect(id)?;
                let restore = self.model.placement(id).restore;
                let earlier = self
                    .windows
                    .get(&id)?
                    .came_from
                    .clone()
                    .filter(|c| self.put_now(id).as_ref() == Some(&c.put));
                Some((id, rect, restore, earlier))
            })
            .collect();
        self.space.unmap_output(output);
        // The space would tell its windows they left at its next refresh,
        // after the global is gone: they are told now.
        self.leave_output(output);
        self.close_layers_on(output);
        // Its bar goes to a screen still there, before its windows do.
        self.panel_refresh();
        self.panes.borrow_mut().forget(&name);
        if let Some(strip) = self.strips.get_mut(&name) {
            strip.still();
        }
        let Some(target) = self.nearest_output(geometry) else {
            tracing::warn!(output = name, "no screen left: its windows wait for one");
            self.redraw = true;
            return;
        };
        let (Some(to_geometry), Some(to_area)) = (
            self.space.output_geometry(&target).map(rect_of),
            self.work_area(&target),
        ) else {
            return;
        };
        tracing::info!(
            from = name,
            to = target.name(),
            windows = leaving.len(),
            "a screen went: its windows move"
        );
        let moved: Vec<WindowId> = leaving.iter().map(|l| l.0).collect();
        for (id, rect, restore, earlier) in leaving {
            let desktop = self.model.desktop(id);
            let placed = match self.model.mode(id) {
                Mode::Floating => model::carried(rect, area, to_area),
                Mode::Snapped(zone) => zone.rect(to_area),
                Mode::Fullscreen => to_geometry,
            };
            let carried = restore.map(|r| model::carried(r, area, to_area));
            self.relocate(id, &target, desktop, placed, carried);
            let Some(put) = self.put_now(id) else {
                continue;
            };
            let came_from = match earlier {
                Some(earlier) => CameFrom { put, ..earlier },
                None => CameFrom {
                    output: key.clone(),
                    desktop,
                    size: (geometry.width, geometry.height),
                    area: shifted(area, geometry, -1),
                    rect: shifted(rect, geometry, -1),
                    restore: restore.map(|r| shifted(r, geometry, -1)),
                    put,
                },
            };
            if let Some(managed) = self.windows.get_mut(&id) {
                managed.came_from = Some(came_from);
            }
        }
        self.refocus();
        if say {
            self.say_screens_moved(output, true, &moved);
        }
        // The pointer was there: to the middle of the screen it went to.
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            if !self.on_a_screen(Rect::new(at.x.floor() as i32, at.y.floor() as i32, 0, 0)) {
                let (x, y) = to_geometry.centre();
                let time = self.input_time();
                self.pointer_motion(Point::from((x, y)), time);
            }
        }
        self.rearrange();
        self.redraw = true;
    }

    /// Every window's surfaces, its subsurfaces and popups, leave `output`,
    /// mapped or not: those that were not on it are not told anything.
    fn leave_output(&self, output: &Output) {
        let windows = self
            .space
            .elements()
            .cloned()
            .chain(self.windows.values().map(|m| m.window.clone()));
        for window in windows {
            // Also forgets the window's overlap with it, which the window
            // would otherwise enter again on its next refresh.
            SpaceElement::output_leave(&window, output);
        }
    }

    /// The layer surfaces on `output` (bars, wallpapers, the shell's
    /// overlays) are closed, as wlr-layer-shell asks when their output
    /// goes; they leave it as they are unmapped.
    fn close_layers_on(&mut self, output: &Output) {
        let layers: Vec<LayerSurface> = layer_map_for_output(output).layers().cloned().collect();
        for layer in layers {
            layer_map_for_output(output).unmap_layer(&layer);
            tracing::debug!(
                output = output.name(),
                namespace = layer.namespace(),
                "a layer surface closed: its screen went"
            );
            layer.layer_surface().send_close();
            self.layer_gone(&layer);
        }
    }

    /// `global`, an unplugged screen's `wl_output`, goes: clients are told
    /// at once, and it is removed a while later.
    fn retire_output_global(&mut self, global: GlobalId) {
        self.display_handle.disable_global::<State>(global.clone());
        let later = global.clone();
        let inserted = self.loop_handle.insert_source(
            Timer::from_duration(GLOBAL_LINGERS),
            move |_, _, app| {
                app.state
                    .display_handle
                    .remove_global::<State>(later.clone());
                TimeoutAction::Drop
            },
        );
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "could not wait to remove a screen's global: removed now");
            self.display_handle.remove_global::<State>(global);
        }
    }

    /// `output` turned off by the file (6.9): it leaves the desktop as one
    /// unplugged does, its `wl_output` kept, and comes back by
    /// [`State::output_back`].
    /// `say`: the winking panda says what moved (the lid's, not the
    /// file's: the user turned it off there).
    pub(crate) fn screen_turned_off(&mut self, output: &Output, say: bool) {
        self.output_gone(output, say);
        self.leave_output(output);
        self.close_layers_on(output);
    }

    /// Screens moved or resized (6.9: a mode, a scale, a place): the
    /// windows on each go along, at the same share of its work area,
    /// snapped ones in their zone, fullscreen ones over it.
    pub(crate) fn screens_moved(&mut self, moved: Vec<Moved>, put: &[WindowId]) {
        for Moved {
            output,
            was_area,
            geometry,
            area,
            windows,
        } in moved
        {
            for id in windows {
                let Some(rect) = self.home_rect(id) else {
                    continue;
                };
                let placed = match self.model.mode(id) {
                    Mode::Floating => model::carried(rect, was_area, area),
                    Mode::Snapped(zone) => zone.rect(area),
                    Mode::Fullscreen => geometry,
                };
                let restore = self
                    .model
                    .placement(id)
                    .restore
                    .map(|r| model::carried(r, was_area, area));
                let desktop = self.model.desktop(id);
                self.relocate(id, &output, desktop, placed, restore);
                // Moved by the desktop, not by the user: a window that came
                // from a screen gone still goes back when it returns.
                if put.contains(&id)
                    && let Some(now) = self.put_now(id)
                    && let Some(came) = self.windows.get_mut(&id).and_then(|m| m.came_from.as_mut())
                {
                    came.put = now;
                }
            }
        }
    }

    /// The windows that came from a screen gone and are still where they
    /// were put, before the screens move (6.9).
    pub(crate) fn still_put(&self) -> Vec<WindowId> {
        self.windows
            .iter()
            .filter(|(id, m)| {
                m.came_from
                    .as_ref()
                    .is_some_and(|c| self.put_now(**id).as_ref() == Some(&c.put))
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Whether `output` is one of the desktop's screens: an unplugged one
    /// lives on a moment, its global disabled.
    pub(crate) fn is_plugged(&self, output: &Output) -> bool {
        self.space.outputs().any(|o| o == output)
    }

    /// `output` joined the desktop: the windows that left it go back where
    /// they were, unless the user placed them elsewhere since; those shown
    /// before and after fly there. `say`: the winking panda says so (a
    /// screen plugged, or the lid opened; not the file turning it on).
    pub fn output_back(&mut self, output: &Output, say: bool) {
        let name = output.name();
        if say {
            self.perf
                .event(hocus_perf::EventKind::ScreenPlugged, name.clone());
        }
        let key = self.screen_key(&name);
        let now = self.now();
        let reduced = self.reduced_motion();
        let (Some(geometry), Some(area)) = (
            self.space.output_geometry(output).map(rect_of),
            self.work_area(output),
        ) else {
            return;
        };
        let returning: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, m)| m.came_from.as_ref().is_some_and(|c| c.output == key))
            .map(|(id, _)| *id)
            .collect();
        let mut back = 0;
        let mut flown = Vec::new();
        for id in returning {
            let Some(came) = self.windows.get_mut(&id).and_then(|m| m.came_from.take()) else {
                continue;
            };
            if self.put_now(id).as_ref() != Some(&came.put) {
                continue;
            }
            // Its place as it was; on a screen now of another size, at
            // the same share of it.
            let place = |r: Rect| {
                let r = shifted(r, geometry, 1);
                if came.size == (geometry.width, geometry.height) {
                    r
                } else {
                    model::carried(r, shifted(came.area, geometry, 1), area)
                }
            };
            let rect = match self.model.mode(id) {
                Mode::Floating => place(came.rect),
                Mode::Snapped(zone) => zone.rect(area),
                Mode::Fullscreen => geometry,
            };
            let from = self
                .model
                .is_shown(id)
                .then(|| self.visual_rect(id, now))
                .flatten();
            self.relocate(id, output, came.desktop, rect, came.restore.map(place));
            self.fly_back(id, from, now, reduced);
            back += 1;
            flown.push(id);
        }
        // Windows on no screen, every screen having gone before: to this
        // one, in the middle of its work area.
        let lost: Vec<WindowId> = self
            .windows
            .keys()
            .copied()
            .filter(|id| self.home_rect(*id).is_some_and(|r| !self.on_a_screen(r)))
            .collect();
        for id in lost {
            let Some(rect) = self.home_rect(id) else {
                continue;
            };
            let placed = match self.model.mode(id) {
                Mode::Floating => model::carried(rect, rect, area),
                Mode::Snapped(zone) => zone.rect(area),
                Mode::Fullscreen => geometry,
            };
            let restore = self.model.placement(id).restore;
            let restore = restore.map(|r| model::carried(r, r, area));
            let desktop = self.model.desktop(id);
            self.relocate(id, output, desktop, placed, restore);
            back += 1;
        }
        if back > 0 {
            tracing::info!(
                output = name,
                windows = back,
                "a screen came: windows go to it"
            );
            self.refocus();
        }
        if say {
            self.say_screens_moved(output, false, &flown);
        }
        self.rearrange();
        self.redraw = true;
    }

    /// Window `id`, gone back to its screen, drawn from `from` (where it
    /// showed before, global) to its place in 320 ms along the desktops'
    /// curve; with reduced motion it fades in there in 120 ms. Not shown
    /// before or now: placed at once.
    fn fly_back(
        &mut self,
        id: WindowId,
        from: Option<smithay::utils::Rectangle<f64, smithay::utils::Logical>>,
        now: Duration,
        reduced: bool,
    ) {
        let Some(from) = from else {
            return;
        };
        if !self.model.is_shown(id) {
            return;
        }
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        if reduced {
            managed.opening = Some(Tween::new(now, crate::motion::FADE_REDUCED, Bezier::LINEAR));
        } else {
            managed.settle = Some((
                from,
                Tween::new(now, crate::motion::SCREEN_BACK, Bezier::STRIP),
            ));
        }
        self.redraw = true;
    }

    /// The windows that wait for a screen gone and would go back to it:
    /// that screen's key, once for each.
    pub(crate) fn windows_waiting(&self) -> Vec<String> {
        self.windows
            .iter()
            .filter_map(|(id, m)| {
                let came = m.came_from.as_ref()?;
                (self.put_now(*id).as_ref() == Some(&came.put)).then(|| came.output.clone())
            })
            .collect()
    }

    /// `windows` moved with `output`, gone (`gone`) or back: the winking
    /// panda says so, naming their applications, the focused one first.
    fn say_screens_moved(&mut self, output: &Output, gone: bool, windows: &[WindowId]) {
        if windows.is_empty() {
            return;
        }
        let focused = self.model.focused();
        let mut ordered: Vec<WindowId> = windows.to_vec();
        ordered.sort_by_key(|id| (Some(*id) != focused, !self.model.is_shown(*id)));
        let apps = ordered
            .iter()
            .filter_map(|id| self.windows.get(id))
            .map(|m| crate::windows::app_id(&m.window).unwrap_or_default())
            .collect();
        let desktops: std::collections::BTreeSet<usize> =
            windows.iter().map(|id| self.model.desktop(*id)).collect();
        let desktop = match desktops.iter().next() {
            Some(d) if desktops.len() == 1 => d + 1,
            _ => 0,
        };
        let mm = output
            .user_data()
            .get::<crate::displays::Facts>()
            .map_or((0, 0), |f| f.millimetres);
        let inches = if mm.0 > 0 && mm.1 > 0 {
            ((f64::from(mm.0).powi(2) + f64::from(mm.1).powi(2)).sqrt() / 25.4).round() as u32
        } else {
            0
        };
        self.screens_peek(hocus_link::ScreensMoved {
            gone,
            built_in: crate::away::is_internal(output),
            inches,
            desktop,
            apps,
        });
    }

    /// Whether `rect`'s centre (a point's, for an empty one) is on a
    /// screen.
    /// The pointer to the primary screen's middle as the screens light up,
    /// until the user moves it: the session starts there, wherever the
    /// display manager left it (its place is not handed over), never in a
    /// screen's corner. And whenever the screens change with it outside
    /// all of them.
    pub(crate) fn pointer_onto_primary(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let at = pointer.current_location();
        let off = !self.on_a_screen(Rect::new(at.x.floor() as i32, at.y.floor() as i32, 0, 0));
        if self.pointer_placed && !off {
            return;
        }
        let names: Vec<String> = self.space.outputs().map(Output::name).collect();
        let primary =
            crate::panel::primary(&names, &self.primary_wanted(), self.panel.home.as_deref());
        let Some(geometry) = primary
            .and_then(|name| self.space.outputs().find(|o| o.name() == name).cloned())
            .and_then(|o| self.space.output_geometry(&o))
        else {
            return;
        };
        let (x, y) = rect_of(geometry).centre();
        tracing::debug!(x, y, "the pointer to the primary screen's middle");
        let time = self.input_time();
        self.pointer_motion(Point::from((x, y)), time);
    }

    fn on_a_screen(&self, rect: Rect) -> bool {
        let (x, y) = rect.centre();
        self.space.outputs().any(|o| {
            self.space
                .output_geometry(o)
                .is_some_and(|g| rect_of(g).contains(x, y))
        })
    }

    /// What goes on ends: a drag or a resize under way, wherever it is (it
    /// may hold a window of `output`, or one on its way there), without
    /// snapping; Alt+Tab shown on `output`, a swipe of its desktops.
    fn end_on_output(&mut self, output: &Output) {
        if let Some(pointer) = self.seat.get_pointer()
            && pointer.is_grabbed()
        {
            // No snap where the preview was.
            self.preview = None;
            self.preview_out = None;
            let time = self.input_time();
            pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), time);
        }
        if self.preview.as_ref().is_some_and(|p| p.output == *output) {
            self.preview = None;
        }
        if self.switcher.as_ref().is_some_and(|s| s.output == *output) {
            self.end_switcher(false);
        }
        if self
            .switcher_out
            .as_ref()
            .is_some_and(|(s, _)| s.output == *output)
        {
            self.switcher_out = None;
        }
        if self.swiping.as_ref() == Some(output) {
            self.swiping = None;
        }
        if self.radial.as_ref().is_some_and(|r| r.output == *output) {
            self.close_radial(None);
        }
        self.radial_out = None;
        self.seam_handle = None;
    }

    /// The screen still there nearest to `geometry` (a screen gone).
    fn nearest_output(&self, geometry: Rect) -> Option<Output> {
        let (cx, cy) = geometry.centre();
        self.space
            .outputs()
            .filter_map(|o| {
                let (x, y) = rect_of(self.space.output_geometry(o)?).centre();
                Some((o, (x - cx).powi(2) + (y - cy).powi(2)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(o, _)| o.clone())
    }

    /// Where `id` is, as far as the user places it.
    pub(crate) fn put_now(&self, id: WindowId) -> Option<Put> {
        let mode = self.model.mode(id);
        Some(Put {
            output: self.output_of(id)?.name(),
            desktop: self.model.desktop(id),
            mode,
            rect: (mode == Mode::Floating)
                .then(|| self.target_rect(id))
                .flatten(),
        })
    }

    /// Puts `id` on `output`'s desktop `desktop` at `rect` (global), at
    /// once: shown or not as that desktop is, minimised as it was;
    /// `restore`: where it floats again.
    pub(crate) fn relocate(
        &mut self,
        id: WindowId,
        output: &Output,
        desktop: usize,
        rect: Rect,
        restore: Option<Rect>,
    ) {
        let desktop = desktop.min(self.strip(output).count() - 1);
        let current = self.strip(output).current();
        self.end_popup_grab_of(id);
        if self.wobble.as_ref().is_some_and(|(w, _)| *w == id) {
            self.wobble = None;
        }
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        // It keeps to that screen, snapped now or once out of fullscreen.
        managed.snapped_on = Some(output.clone());
        managed.settle = None;
        managed.travel = None;
        managed.resizing = None;
        let window = managed.window.clone();
        // What it may take is now the new screen's work area, in the
        // configure that moves it there.
        if let (Some(toplevel), Some(area)) = (window.toplevel(), self.work_area(output)) {
            toplevel.with_pending_state(|p| p.bounds = Some((area.width, area.height).into()));
        }
        self.model.set_desktop(id, desktop);
        self.model.set_restore(id, restore);
        self.model.set_hidden(id, desktop != current);
        if self.model.is_shown(id) {
            if let Some(managed) = self.windows.get_mut(&id) {
                managed.away = None;
            }
        } else {
            self.space.unmap_elem(&window);
        }
        self.configure(id, rect);
    }

    /// The focus stays on its window if it still shows; otherwise it goes
    /// to the window on top that does.
    pub(crate) fn refocus(&mut self) {
        let next = self
            .model
            .focused()
            .filter(|id| self.model.is_shown(*id))
            .or_else(|| self.model.top_shown());
        self.focus(next);
    }

    /// Milliseconds on the monotonic clock, as input events carry.
    pub(crate) fn input_time(&self) -> u32 {
        // Wraps as input times do.
        self.now().as_millis() as u32
    }
}
