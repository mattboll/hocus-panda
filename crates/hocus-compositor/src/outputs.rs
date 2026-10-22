//! Screens set as asked: by the configuration's `[[outputs]]`, or for the
//! running session by a third-party tool (wlr-output-management). A change
//! of mode, scale, rotation or place moves nothing else: windows keep their
//! place on their screen, maximized and fullscreen ones fill it again, and a
//! layout seen before puts every window back where it was. A screen turned
//! off keeps its desktops and windows for when it comes back.
//!
//! The backend says what each screen can do and carries changes out on the
//! hardware; what follows on the desktop is the same for every backend.

use std::collections::HashMap;

use hocus_api::Event;
use hocus_core::outputs::{Mode, ModeSpec, Transform, round_scale};
use smithay::{
    desktop::layer_map_for_output,
    output::{Mode as WlMode, Output, Scale},
    utils::{Logical, Point},
};

use crate::{App, BackendState, State};

/// What a backend knows of one of its screens, on or off.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub name: String,
    pub make: String,
    pub model: String,
    /// Millimetres; zero when unknown.
    pub physical_size: (i32, i32),
    pub modes: Vec<Mode>,
    pub preferred: Option<usize>,
    pub vrr_capable: bool,
    pub now: Setting,
}

/// What a screen is set to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setting {
    pub enabled: bool,
    pub mode: Mode,
    pub scale: f64,
    pub transform: Transform,
    /// Logical position of its top-left corner.
    pub position: (i32, i32),
    pub vrr: bool,
}

/// What screens were set to before anyone asked, and what tools asked.
#[derive(Debug, Default)]
pub struct Settings {
    /// Each screen as the backend first lit it: what a setting not in the
    /// file goes back to.
    base: HashMap<String, Setting>,
    /// Set by a tool for the running session; the file wins again once it
    /// changes.
    overrides: HashMap<String, Setting>,
    /// Screens, the file or a tool changed: settle them after this turn of
    /// the loop.
    pub changed: bool,
}

impl Setting {
    /// What `output` is set to now, as the desktop sees it.
    pub fn of(output: &Output, position: (i32, i32), enabled: bool, vrr: bool) -> Self {
        let mode = output.current_mode().unwrap_or(WlMode {
            size: (0, 0).into(),
            refresh: 0,
        });
        Self {
            enabled,
            mode: Mode {
                width: mode.size.w,
                height: mode.size.h,
                refresh: mode.refresh,
            },
            scale: output.current_scale().fractional_scale(),
            transform: transform_from(output.current_transform()),
            position,
            vrr,
        }
    }
}

pub fn transform_to(t: Transform) -> smithay::utils::Transform {
    use smithay::utils::Transform as T;
    match t {
        Transform::Normal => T::Normal,
        Transform::Rotate90 => T::_90,
        Transform::Rotate180 => T::_180,
        Transform::Rotate270 => T::_270,
        Transform::Flipped => T::Flipped,
        Transform::Flipped90 => T::Flipped90,
        Transform::Flipped180 => T::Flipped180,
        Transform::Flipped270 => T::Flipped270,
    }
}

pub fn transform_from(t: smithay::utils::Transform) -> Transform {
    use smithay::utils::Transform as T;
    match t {
        T::Normal => Transform::Normal,
        T::_90 => Transform::Rotate90,
        T::_180 => Transform::Rotate180,
        T::_270 => Transform::Rotate270,
        T::Flipped => Transform::Flipped,
        T::Flipped90 => Transform::Flipped90,
        T::Flipped180 => Transform::Flipped180,
        T::Flipped270 => Transform::Flipped270,
    }
}

impl State {
    /// Notes how the backend first lit `output`, and settles every screen
    /// after this turn of the loop.
    pub fn output_lit(&mut self, output: &Output) {
        let at = self
            .space
            .output_geometry(output)
            .map_or((0, 0), |g| (g.loc.x, g.loc.y));
        self.output_settings
            .base
            .entry(output.name())
            .or_insert_with(|| Setting::of(output, at, true, false));
        self.output_settings.changed = true;
    }

    /// The file changed: what it says wins over what tools set.
    pub fn outputs_config_changed(&mut self) {
        self.output_settings.overrides.clear();
        self.output_settings.changed = true;
    }

    /// A tool set `name` for the running session.
    pub fn override_output(&mut self, name: &str, setting: Setting) {
        self.output_settings
            .overrides
            .insert(name.to_string(), setting);
        self.output_settings.changed = true;
    }

    /// What `head` should be set to: a tool's setting, else the file over
    /// how the screen was first lit.
    pub fn wanted(&self, head: &Head) -> Setting {
        if let Some(setting) = self.output_settings.overrides.get(&head.name) {
            return *setting;
        }
        let base = self
            .output_settings
            .base
            .get(&head.name)
            .copied()
            .unwrap_or(head.now);
        let Some(file) = self.config.output(&head.name) else {
            return base;
        };
        let mode = file.mode.as_deref().and_then(|m| {
            let spec: ModeSpec = m.parse().ok()?;
            let found = spec.pick(&head.modes).map(|i| head.modes[i]);
            if found.is_none() {
                tracing::warn!(
                    output = head.name,
                    mode = m,
                    "no such mode; keeping the current one"
                );
            }
            found
        });
        Setting {
            enabled: file.enabled.unwrap_or(true),
            mode: mode.unwrap_or(base.mode),
            scale: file.scale.map_or(base.scale, round_scale),
            transform: file.transform.unwrap_or(base.transform),
            position: file.position.map_or(base.position, |[x, y]| (x, y)),
            vrr: file.vrr.unwrap_or(false) && head.vrr_capable,
        }
    }

    /// Sets a lit screen's mode (as the backend now drives it), scale,
    /// rotation and place, and moves what it shows along.
    pub fn set_output(&mut self, output: &Output, setting: &Setting) {
        self.remember_places();
        let name = output.name();
        let old = self.space.output_geometry(output).unwrap_or_default();
        // Where each window it shows is, from its corner, and whether it
        // was whole on it.
        let offsets: Vec<(hocus_core::WindowId, Point<i32, Logical>, bool)> = self
            .model
            .visible_windows(&hocus_core::OutputName(name.clone()))
            .iter()
            .filter_map(|id| {
                let managed = self.windows.get(id)?;
                let at = self
                    .space
                    .element_location(&managed.window)
                    .unwrap_or(managed.location);
                let whole = old.contains_rect(smithay::utils::Rectangle::new(
                    at,
                    managed.window.geometry().size,
                ));
                Some((*id, at - old.loc, whole))
            })
            .collect();

        let scale = if setting.scale.fract() == 0.0 {
            Scale::Integer(setting.scale as i32)
        } else {
            Scale::Fractional(setting.scale)
        };
        output.change_current_state(
            Some(WlMode {
                size: (setting.mode.width, setting.mode.height).into(),
                refresh: setting.mode.refresh,
            }),
            Some(transform_to(setting.transform)),
            Some(scale),
            Some(setting.position.into()),
        );
        self.space.map_output(output, setting.position);
        self.output_moved(output);
        let new = self.space.output_geometry(output).unwrap_or_default();
        // A layout seen before puts every window back; otherwise each keeps
        // its place on its screen, and one that was whole on it stays whole.
        if !self.restore_places() {
            for (id, offset, whole) in offsets {
                if let Some(managed) = self.windows.get_mut(&id) {
                    self.space.unmap_elem(&managed.window);
                    let at = new.loc + offset;
                    managed.location = if whole {
                        crate::desktops::clamp_into(at, managed.window.geometry().size, new)
                    } else {
                        at
                    };
                }
            }
        }
        layer_map_for_output(output).arrange();
        self.relayout();
        self.emit(|s| Event::OutputChanged {
            output: s.output_info(output),
        });
        self.needs_redraw = true;
        tracing::info!(
            output = name,
            mode = %format!("{}x{}@{:.2}", setting.mode.width, setting.mode.height, f64::from(setting.mode.refresh) / 1000.0),
            scale = setting.scale,
            transform = ?setting.transform,
            position = ?setting.position,
            "screen set"
        );
    }

    /// Lights a screen that was off: it comes back with its desktops and
    /// windows.
    pub fn enable_output(&mut self, output: &Output, setting: &Setting, desktops: usize) {
        self.remember_places();
        output.change_current_state(
            Some(WlMode {
                size: (setting.mode.width, setting.mode.height).into(),
                refresh: setting.mode.refresh,
            }),
            Some(transform_to(setting.transform)),
            Some(if setting.scale.fract() == 0.0 {
                Scale::Integer(setting.scale as i32)
            } else {
                Scale::Fractional(setting.scale)
            }),
            Some(setting.position.into()),
        );
        self.space.map_output(output, setting.position);
        self.plug_output(output, desktops);
        self.emit(|s| Event::OutputAdded {
            output: s.output_info(output),
        });
        tracing::info!(output = output.name(), "screen on");
    }

    /// Takes an output's global away: clients are told at once, and it is
    /// destroyed a little later, so that a client binding it meanwhile is
    /// not cut off with a protocol error.
    pub fn retire_global(&mut self, global: smithay::reexports::wayland_server::backend::GlobalId) {
        use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
        self.display_handle.disable_global::<State>(global.clone());
        let Some(handle) = &self.loop_handle else {
            self.display_handle.remove_global::<State>(global);
            return;
        };
        let removed = handle.insert_source(
            Timer::from_duration(std::time::Duration::from_secs(5)),
            move |_, _, app| {
                app.state
                    .display_handle
                    .remove_global::<State>(global.clone());
                TimeoutAction::Drop
            },
        );
        if let Err(err) = removed {
            tracing::warn!(%err.error, "could not schedule a global's removal");
        }
    }

    /// Turns a screen off: what it showed goes to the primary screen until
    /// it comes back.
    pub fn disable_output(&mut self, output: &Output) {
        self.unplug_output(output);
        tracing::info!(output = output.name(), "screen off");
    }
}

impl App {
    /// Every screen of the backend, on and off.
    pub fn heads(&self) -> Vec<Head> {
        match &self.backend {
            BackendState::Winit(winit) => winit.heads(&self.state),
            BackendState::Headless(headless) => headless.heads(&self.state),
            BackendState::Tty(tty) => tty.heads(&self.state),
        }
    }

    /// Sets the screen `name` as asked.
    pub fn apply_output(&mut self, name: &str, setting: &Setting) -> anyhow::Result<()> {
        let App { state, backend } = self;
        match backend {
            BackendState::Winit(winit) => winit.apply(name, setting, state),
            BackendState::Headless(headless) => headless.apply(name, setting, state),
            BackendState::Tty(tty) => tty.apply(name, setting, state),
        }
    }

    /// After a turn of the loop where screens, the file or a tool changed:
    /// sets each screen as wanted, and tells the tools.
    pub fn settle_outputs(&mut self) {
        if !self.state.output_settings.changed {
            return;
        }
        self.state.output_settings.changed = false;
        let heads = self.heads();
        let mut lit = heads.iter().filter(|h| h.now.enabled).count();
        for head in heads {
            let wanted = self.state.wanted(&head);
            if wanted == head.now {
                continue;
            }
            // The last screen on stays on: nobody could turn one on again.
            if head.now.enabled && !wanted.enabled {
                if lit <= 1 {
                    tracing::warn!(output = head.name, "the last screen on stays on");
                    self.state.output_management.failed = true;
                    continue;
                }
                lit -= 1;
            }
            if !head.now.enabled && wanted.enabled {
                lit += 1;
            }
            if let Err(err) = self.apply_output(&head.name, &wanted) {
                tracing::warn!(output = head.name, "could not set the screen: {err:#}");
                self.state.output_management.failed = true;
            }
        }
        // Screens turned off or on through output-power.
        for (name, on) in std::mem::take(&mut self.state.output_management.power_wanted) {
            let App { state, backend } = self;
            let result = match backend {
                BackendState::Winit(_) => Err(anyhow::anyhow!("the nested window has no power")),
                BackendState::Headless(headless) => headless.power(&name, on, state),
                BackendState::Tty(tty) => tty.power(&name, on, state),
            };
            match result {
                Ok(()) => {
                    let dark = &mut self.state.output_management.dark;
                    dark.retain(|n| *n != name);
                    if !on {
                        dark.push(name.clone());
                    }
                    self.state.power_changed(&name);
                }
                Err(err) => tracing::warn!(
                    output = name,
                    "could not turn the screen {}: {err:#}",
                    if on { "on" } else { "off" }
                ),
            }
        }
        let heads = self.heads();
        self.state.refresh_output_management(&heads);
        self.state.answer_output_configurations();
    }
}
