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

/// How long each screen's name shows when identifying them.
const IDENTIFY: std::time::Duration = std::time::Duration::from_secs(3);

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
    /// how the screen was first lit; the place from the layout of the
    /// screens `connected`, if the file has one.
    pub fn wanted(&self, head: &Head, connected: &[&str]) -> Setting {
        if let Some(setting) = self.output_settings.overrides.get(&head.name) {
            return *setting;
        }
        let base = self
            .output_settings
            .base
            .get(&head.name)
            .copied()
            .unwrap_or(head.now);
        let placed = self
            .config
            .layout_for(connected)
            .and_then(|l| l.positions.get(&head.name).copied());
        let Some(file) = self.config.output(&head.name) else {
            return Setting {
                position: placed.map_or(base.position, |[x, y]| (x, y)),
                ..base
            };
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
            position: placed
                .or(file.position)
                .map_or(base.position, |[x, y]| (x, y)),
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
        // Showing another's picture: only its mode and scale change.
        if self.is_mirroring(&name) {
            self.needs_redraw = true;
            return;
        }
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
        let names: Vec<String> = heads.iter().map(|h| h.name.clone()).collect();
        let connected: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut lit = heads.iter().filter(|h| h.now.enabled).count();
        for head in heads {
            let wanted = self.state.wanted(&head, &connected);
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
        self.state.settle_desktops_and_primary();
        let heads = self.heads();
        self.state.settle_mirrors(&heads);
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

impl State {
    /// Each lit screen's own desktops and the primary one, as the file says.
    fn settle_desktops_and_primary(&mut self) {
        let lit: Vec<String> = self.space.outputs().map(Output::name).collect();
        let mut changed = false;
        for name in &lit {
            let key = hocus_core::OutputName(name.clone());
            if self.name_desktops(&key) {
                changed = true;
            }
            let Some(file) = self.config.output(name) else {
                continue;
            };
            if file.primary == Some(true)
                && self.model.primary().map(|o| &o.name) != Some(&key)
                && self.model.set_primary(&key).is_ok()
            {
                changed = true;
            }
            if let Some(n) = file.desktops {
                let n = n as usize;
                if self.model.desktop_count(&key) != Some(n) {
                    if let Err(err) = self.model.set_desktop_count(&key, n) {
                        tracing::warn!(output = name, %err, "could not change its desktops");
                    }
                    changed = true;
                }
            }
        }
        if changed {
            self.sync_desktops();
            self.apply_focus(smithay::utils::SERIAL_COUNTER.next_serial());
        }
    }

    /// An output's own desktops take the names the file gives them, the
    /// others their number; whether one changed.
    fn name_desktops(&mut self, output: &hocus_core::OutputName) -> bool {
        let Some(own) = self.model.own_desktops(output).map(<[_]>::to_vec) else {
            return false;
        };
        let mut changed = false;
        for (i, id) in own.into_iter().enumerate() {
            let wanted = self.desktop_name(id).unwrap_or_else(|| (i + 1).to_string());
            if self.model.desktop(id).is_some_and(|d| d.name != wanted) {
                if let Err(err) = self.model.rename(id, wanted) {
                    tracing::warn!(output = output.0, %err, "could not name a desktop");
                }
                changed = true;
            }
        }
        changed
    }

    /// The name the file gives a desktop: its output's `desktop_names`,
    /// else the look of that desktop number (#218).
    pub fn desktop_name(&self, id: hocus_core::DesktopId) -> Option<String> {
        let home = &self.model.desktop(id)?.home;
        let index = self
            .model
            .own_desktops(home)?
            .iter()
            .position(|d| *d == id)?;
        let named = |name: &str| {
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_owned())
        };
        self.config
            .output(&home.0)
            .and_then(|o| o.desktop_names.get(index))
            .and_then(|n| named(n))
            .or_else(|| {
                self.config
                    .desktops
                    .look(&home.0, index)?
                    .name
                    .as_deref()
                    .and_then(named)
            })
    }

    /// Shows each lit screen's name on it for three seconds.
    pub fn identify_outputs(&mut self) {
        use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
        let theme = self.theme();
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        self.identifying = outputs
            .iter()
            .map(|o| {
                let props = o.physical_properties();
                let mode = o.current_mode().map(|m| m.size).unwrap_or_default();
                let detail = format!("{} {} · {} × {}", props.make, props.model, mode.w, mode.h);
                let mut card = hocus_shell::identify::Identify::new(
                    theme.clone(),
                    o.current_scale().fractional_scale() as f32,
                    &o.name(),
                    &detail,
                );
                card.draw(self.clock.now());
                let mut panel = crate::shell::Panel::new(card);
                panel.upload(&[]);
                (o.name(), panel)
            })
            .collect();
        self.needs_redraw = true;
        // On the compositor's clock, as every moment: a timer on real time
        // would take them away whenever it fired, in a test too (#212).
        self.identifying_until = Some(self.clock.now() + IDENTIFY);
        if let Some(handle) = self.waker() {
            let woken = handle.insert_source(Timer::from_duration(IDENTIFY), |_, _, _| {
                TimeoutAction::Drop
            });
            if let Err(err) = woken {
                tracing::warn!(%err.error, "the screens' names will go on the next event");
            }
        }
    }

    /// After each dispatch: the screens' names go when their time is up.
    pub fn refresh_identify(&mut self) {
        if self
            .identifying_until
            .is_some_and(|until| self.clock.now() >= until)
        {
            self.identifying_until = None;
            self.identifying.clear();
            self.needs_redraw = true;
        }
    }

    /// Where a screen's name card goes: its middle.
    fn identify_at(&self, output: &Output) -> Option<Point<i32, Logical>> {
        let g = self.space.output_geometry(output)?;
        let size = hocus_shell::identify::SIZE;
        Some(Point::from((
            ((g.size.w as f32 - size.w) / 2.0) as i32,
            ((g.size.h as f32 - size.h) / 2.0) as i32,
        )))
    }

    pub fn identify_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Option<smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<R>>
    where
        R: smithay::backend::renderer::Renderer + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let (_, panel) = self.identifying.iter().find(|(n, _)| *n == output.name())?;
        panel.element(renderer, output, self.identify_at(output)?)
    }

    pub fn identify_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        for (name, panel) in &self.identifying {
            let Some(output) = self.find_output(name) else {
                continue;
            };
            let (Some(g), Some(at)) = (
                self.space.output_geometry(&output),
                self.identify_at(&output),
            ) else {
                continue;
            };
            crate::shell::flatten(&panel.view.access(), g.loc + at, 0, out);
        }
    }
}

impl App {
    /// Every connected screen, for the settings' Screens page.
    pub fn screens(&self) -> Vec<hocus_api::ScreenInfo> {
        let heads = self.heads();
        let names: Vec<&str> = heads.iter().map(|h| h.name.as_str()).collect();
        let layout = self.state.config.layout_for(&names).map(|l| l.name.clone());
        let state = &self.state;
        heads
            .iter()
            .map(|head| {
                let key = hocus_core::OutputName(head.name.clone());
                let geometry = state
                    .find_output(&head.name)
                    .and_then(|o| state.space.output_geometry(&o))
                    .map(crate::desktops::rect)
                    .unwrap_or(hocus_core::Rect {
                        x: head.now.position.0,
                        y: head.now.position.1,
                        width: head.now.mode.width,
                        height: head.now.mode.height,
                    });
                hocus_api::ScreenInfo {
                    name: head.name.clone(),
                    make: head.make.clone(),
                    model: head.model.clone(),
                    enabled: head.now.enabled,
                    modes: head
                        .modes
                        .iter()
                        .map(|m| hocus_api::ModeInfo {
                            width: m.width,
                            height: m.height,
                            refresh: m.refresh,
                        })
                        .collect(),
                    current_mode: head.modes.iter().position(|m| *m == head.now.mode),
                    preferred_mode: head.preferred,
                    scale: head.now.scale,
                    transform: transform_name(head.now.transform).into(),
                    geometry: hocus_api::Rect {
                        x: geometry.x,
                        y: geometry.y,
                        width: geometry.width,
                        height: geometry.height,
                    },
                    vrr: head.now.vrr,
                    vrr_capable: head.vrr_capable,
                    primary: state.model.primary().is_some_and(|o| o.name == key),
                    desktops: state.model.desktop_count(&key).unwrap_or(0) as u32,
                    layout: layout.clone(),
                }
            })
            .collect()
    }
}

fn transform_name(t: Transform) -> &'static str {
    match t {
        Transform::Normal => "normal",
        Transform::Rotate90 => "90",
        Transform::Rotate180 => "180",
        Transform::Rotate270 => "270",
        Transform::Flipped => "flipped",
        Transform::Flipped90 => "flipped-90",
        Transform::Flipped180 => "flipped-180",
        Transform::Flipped270 => "flipped-270",
    }
}
