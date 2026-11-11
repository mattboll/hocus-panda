//! The actions palette (#736, docs/design/actions.md): `Super+P` asks the
//! shell to open its list of every action ([`hocus_link::actions`]) on the
//! screen of the window that has the keyboard (the target), else the
//! pointer's; the shell says which one runs ([`RunAction`]) and the
//! compositor runs it here, with the same code as its shortcut, its menu
//! or its row in the bar.
//!
//! While the palette shows (its surface, [`ACTIONS_NAMESPACE`], moves in
//! and out as a popover, `crate::popover`), the compositor dims its
//! screen under it, no blur, and draws the target window above the dim
//! with a green ring ([`State::actions_backdrop`]); both fade with the
//! palette. Nothing is drawn at rest.

use std::path::PathBuf;

use hocus_link::{
    ACTIONS_DIM, ACTIONS_GLOW_ALPHA, ACTIONS_GLOW_WIDTH, ACTIONS_NAMESPACE, ACTIONS_RING,
    ACTIONS_RING_WIDTH, ActionsShown, LeaveAction, LevelKey, RunAction, ToShell, WindowAction,
    actions::{Inert, Runs},
};
use smithay::{
    backend::renderer::element::{Id, Kind, solid::SolidColorRenderElement},
    desktop::{LayerSurface, layer_map_for_output},
    input::keyboard::Keysym,
    output::Output,
    utils::{Logical, Rectangle},
};

use crate::{
    State,
    decoration::BoxStyle,
    model::{Side, Snap, WindowId},
    shortcuts::{Action, CaptureNow, Chord},
};

/// The palette's target and what draws around it.
#[derive(Debug)]
pub struct Palette {
    /// The window it acts on, and its screen, from `Super+P` until the
    /// palette's surface goes.
    pub target: Option<(Option<WindowId>, String)>,
    /// The dim's and the ring's pieces, the same from one frame to the
    /// next for the damage.
    pub dim: Id,
    pub ring: [Id; 9],
    pub glow: [Id; 9],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            target: None,
            dim: Id::new(),
            ring: std::array::from_fn(|_| Id::new()),
            glow: std::array::from_fn(|_| Id::new()),
        }
    }
}

/// A chord for the actions run without one: nothing held.
fn no_chord() -> Chord {
    Chord {
        logo: false,
        shift: false,
        ctrl: false,
        alt: false,
        key: Keysym::NoSymbol,
    }
}

/// The action a palette's id runs as its shortcut does, if it is one.
fn shortcut_action(id: &str) -> Option<Action> {
    Some(match id {
        "terminal" => Action::Terminal,
        "lock" => Action::Lock,
        "log-out" => Action::LogOut,
        "close" => Action::Close,
        "minimize" => Action::Minimize,
        "snap-left" => Action::Snap(Snap::Half(Side::Start)),
        "snap-right" => Action::Snap(Snap::Half(Side::End)),
        "maximize" => Action::Snap(Snap::Maximize),
        "restore" => Action::Snap(Snap::Restore),
        "quarter-up" => Action::Snap(Snap::QuarterTop),
        "quarter-down" => Action::Snap(Snap::QuarterBottom),
        "quarter-left" => Action::Snap(Snap::QuarterLeft),
        "quarter-right" => Action::Snap(Snap::QuarterRight),
        "focus-left" => Action::FocusToward(-1, 0),
        "focus-right" => Action::FocusToward(1, 0),
        "focus-up" => Action::FocusToward(0, -1),
        "focus-down" => Action::FocusToward(0, 1),
        "switch-windows" => Action::SwitchWindows { backward: false },
        "switch-windows-backward" => Action::SwitchWindows { backward: true },
        "take-to-desktop-left" => Action::TakeToDesktopBy(-1),
        "take-to-desktop-right" => Action::TakeToDesktopBy(1),
        "move-to-screen-left" => Action::MoveToScreen(-1),
        "move-to-screen-right" => Action::MoveToScreen(1),
        "focus-bar" => Action::FocusBar,
        "overview" => Action::Overview,
        "clipboard" => Action::Clipboard,
        "emoji" => Action::Emoji,
        "volume-up" => Action::Level(LevelKey::VolumeUp),
        "volume-down" => Action::Level(LevelKey::VolumeDown),
        "mute" => Action::Level(LevelKey::Mute),
        "brightness-up" => Action::Level(LevelKey::BrightnessUp),
        "brightness-down" => Action::Level(LevelKey::BrightnessDown),
        "capture" => Action::Capture,
        "capture-screen" => Action::CaptureNow(CaptureNow::Screen),
        "capture-window" => Action::CaptureNow(CaptureNow::Window),
        "record" => Action::Record,
        "join" => Action::Join,
        "magnifier-in" => Action::MagnifierIn,
        "magnifier-out" => Action::MagnifierOut,
        "magnifier-off" => Action::MagnifierOff,
        "screen-reader" => Action::ScreenReader,
        _ => return None,
    })
}

impl State {
    /// `Super+P`: the shell opens the palette for the window that has the
    /// keyboard, on its screen (else the pointer's); open, it closes.
    pub fn toggle_actions(&mut self) {
        if !self.shell_linked() {
            tracing::info!("no shell: no actions palette");
            return;
        }
        let window = self
            .model
            .focused()
            .filter(|id| self.windows.contains_key(id) && !self.model.is_minimized(*id));
        let Some(output) = window
            .and_then(|id| self.output_of(id))
            .or_else(|| self.output_under_pointer())
        else {
            return;
        };
        let shown = self.actions_shown(window, &output);
        tracing::debug!(
            window = window.map(|w| w.0),
            output = output.name(),
            inert = shown.inert.len(),
            "the actions palette is asked for"
        );
        self.actions.target = Some((window, output.name()));
        self.tell_shell(ToShell::Actions(shown));
    }

    /// What the palette is told as it opens for `window` on `output`.
    fn actions_shown(&mut self, window: Option<WindowId>, output: &Output) -> ActionsShown {
        let mut on = Vec::new();
        let mut inert: Vec<(String, Inert)> = Vec::new();
        let mut say = |id: &str, why: Inert| inert.push((id.to_owned(), why));
        let screens = self.space.outputs().count();
        match window {
            Some(id) => {
                if self.model.is_above(id) {
                    on.push("above".to_owned());
                }
                if self.model.is_everywhere(id) {
                    on.push("all-desktops".to_owned());
                }
                let strip = self.strip(output);
                let desktop = self.model.desktop(id);
                if desktop == 0 {
                    say("previous-desktop", Inert::FirstDesktop);
                }
                if desktop + 1 >= strip.count() {
                    say("next-desktop", Inert::LastDesktop);
                }
                if strip.current() == 0 {
                    say("take-to-desktop-left", Inert::FirstDesktop);
                }
                if strip.current() + 1 >= strip.count() {
                    say("take-to-desktop-right", Inert::LastDesktop);
                }
                for (key, toward) in [("move-to-screen-left", -1), ("move-to-screen-right", 1)] {
                    if screens < 2 {
                        say(key, Inert::OneScreen);
                    } else if self.screen_beside(output, toward).is_none() {
                        say(key, Inert::NoScreenThere);
                    }
                }
                if screens < 2 {
                    say("other-screen", Inert::OneScreen);
                }
            }
            None => {
                for def in hocus_link::actions::catalogue().iter().filter(|d| d.window) {
                    say(def.id, Inert::NoWindow);
                }
            }
        }
        let strip = self.strip(output);
        if strip.current() == 0 {
            say("desktop-left", Inert::FirstDesktop);
        }
        if strip.current() + 1 >= strip.count() {
            say("desktop-right", Inert::LastDesktop);
        }
        if self.magnifier.on.is_none() {
            say("magnifier-out", Inert::MagnifierOff);
            say("magnifier-off", Inert::MagnifierOff);
        }
        if self.screen_reader.running().is_some() {
            on.push("screen-reader".to_owned());
        } else if !self.screen_reader.allowed {
            say("screen-reader", Inert::SessionOnly);
        }
        if self.config.appearance.high_contrast {
            on.push("high-contrast".to_owned());
        }
        if self.config.appearance.reduced_motion {
            on.push("reduce-motion".to_owned());
        }
        for (key, action) in [
            ("suspend", None),
            ("restart", Some(LeaveAction::Restart)),
            ("power-off", Some(LeaveAction::ShutDown)),
        ] {
            if !self.leave_allowed(action) {
                say(key, Inert::NotAllowed);
            }
        }
        let area = match (self.work_area(output), self.space.output_geometry(output)) {
            (Some(area), Some(screen)) => (area.x - screen.loc.x, area.width),
            (None, Some(screen)) => (0, screen.size.w),
            _ => (0, 0),
        };
        ActionsShown {
            output: output.name(),
            scale: self.bar_scale(&output.name()),
            area,
            window: window.map(|w| w.0),
            title: match window {
                Some(id) => crate::windows::title_of(self, id),
                None => crate::radial_draw::screen_label(output),
            },
            on,
            inert,
            shortcuts: self.config.shortcuts.written(),
        }
    }

    /// The shell runs `run`: on its window when it names one still there,
    /// else on the palette's screen.
    pub fn action_asked(&mut self, run: RunAction) {
        let Some(def) = hocus_link::actions::find(&run.id) else {
            tracing::warn!(action = run.id, "no such action");
            return;
        };
        if def.runs != Runs::Compositor {
            tracing::warn!(action = run.id, "an action of the shell's");
            return;
        }
        let window = run
            .window
            .map(WindowId)
            .filter(|id| self.windows.contains_key(id));
        if def.window && window.is_none() {
            tracing::info!(action = run.id, "its window went: nothing runs");
            return;
        }
        let output = window
            .and_then(|id| self.output_of(id))
            .or_else(|| {
                let (_, name) = self.actions.target.as_ref()?;
                self.space.outputs().find(|o| o.name() == *name).cloned()
            })
            .or_else(|| self.output_under_pointer());
        tracing::info!(
            action = run.id,
            arg = run.arg,
            window = run.window,
            "the palette runs"
        );
        // What acts on the focused window acts on the target.
        if let Some(id) = window
            && self.model.focused() != Some(id)
        {
            self.unminimize(id);
            self.focus(Some(id));
        }
        let desktop = run.arg.and_then(|a| usize::try_from(a).ok());
        match (def.id, window, output) {
            ("send-to-desktop", Some(id), _) => {
                if let Some(to) = desktop {
                    self.send_window_to_desktop(id, to, false);
                }
            }
            ("take-to-desktop", Some(id), _) => {
                if let Some(to) = desktop {
                    self.send_window_to_desktop(id, to, true);
                }
            }
            ("desktop", _, Some(output)) => {
                if let Some(to) = desktop.filter(|to| *to < self.strip(&output).count()) {
                    self.switch_desktop(&output, to, None);
                }
            }
            ("desktop-left" | "desktop-right", _, Some(output)) => {
                let delta = if def.id == "desktop-left" { -1 } else { 1 };
                let strip = self.strip(&output);
                let to = strip.current() as i64 + delta;
                if (0..strip.count() as i64).contains(&to) {
                    self.switch_desktop(&output, to as usize, None);
                }
            }
            ("toggle-bar", _, Some(output)) => self.toggle_bar(&output.name()),
            ("launcher", _, Some(output)) => {
                let scale = self.bar_scale(&output.name());
                self.tell_shell(ToShell::Launcher {
                    output: output.name(),
                    scale,
                });
            }
            ("above", Some(id), _) => self.run_window_action(id, WindowAction::Above),
            ("all-desktops", Some(id), _) => self.run_window_action(id, WindowAction::AllDesktops),
            ("other-screen", Some(id), _) => self.run_window_action(id, WindowAction::OtherScreen),
            ("previous-desktop", Some(id), _) => {
                self.run_window_action(id, WindowAction::PreviousDesktop);
            }
            ("next-desktop", Some(id), _) => self.run_window_action(id, WindowAction::NextDesktop),
            ("high-contrast" | "reduce-motion", _, _) => self.actions_switch_appearance(def.id),
            ("suspend", _, _) => self.leave_asked(hocus_link::LeaveAsk::Suspend),
            ("restart" | "power-off", _, Some(output)) => {
                let action = if def.id == "restart" {
                    LeaveAction::Restart
                } else {
                    LeaveAction::ShutDown
                };
                self.leave_asked(hocus_link::LeaveAsk::Ask {
                    action,
                    output: output.name(),
                });
            }
            (id, _, _) => match shortcut_action(id) {
                Some(action) => self.run_action(action, no_chord()),
                None => tracing::warn!(action = id, "nothing runs it here"),
            },
        }
    }

    /// High contrast or reduced motion turned over (`[appearance]`), as
    /// the Settings' switch does: the file written off the loop, the
    /// desktop following it as it reads it again.
    fn actions_switch_appearance(&mut self, id: &str) {
        let (key, on) = match id {
            "high-contrast" => ("high-contrast", !self.config.appearance.high_contrast),
            _ => ("reduced-motion", !self.config.appearance.reduced_motion),
        };
        let Some(file) = self.config.file.clone() else {
            tracing::info!(key, "no configuration file: nothing written");
            return;
        };
        write_appearance(file, key, on);
    }

    /// The palette's target window went: the palette closes.
    pub(crate) fn actions_window_gone(&mut self, id: WindowId) {
        if matches!(self.actions.target, Some((Some(w), _)) if w == id) {
            self.actions.target = None;
            if self.shell_linked() {
                self.tell_shell(ToShell::ActionsEnd);
            }
        }
    }

    /// A layer surface went: the palette's takes its target with it.
    pub(crate) fn actions_gone(&mut self, layer: &LayerSurface) {
        if layer.namespace() == ACTIONS_NAMESPACE {
            let others = self.space.outputs().any(|o| {
                layer_map_for_output(o)
                    .layers()
                    .any(|l| l.namespace() == ACTIONS_NAMESPACE && l != layer)
            });
            if !others {
                self.actions.target = None;
            }
        }
    }

    /// The palette's surface on `output`, if one shows there.
    pub fn actions_on(&self, output: &Output) -> Option<LayerSurface> {
        layer_map_for_output(output)
            .layers()
            .find(|l| l.namespace() == ACTIONS_NAMESPACE)
            .cloned()
    }

    /// What the palette shows behind it on `output` at `alpha`: the
    /// target window, if it is there, and the dim over the rest. The
    /// target's ring goes over it (`ring`, output-local logical).
    pub(crate) fn actions_dim(
        &self,
        output: &Output,
        alpha: f32,
    ) -> Option<SolidColorRenderElement> {
        let size = self.space.output_geometry(output)?.size;
        let scale = output.current_scale().fractional_scale();
        let ([r, g, b], a) = ACTIONS_DIM;
        let a = a * alpha;
        let channel = |c: u8| f32::from(c) / 255.0 * a;
        Some(SolidColorRenderElement::new(
            self.actions.dim.clone(),
            Rectangle::<f64, Logical>::from_size(size.to_f64()).to_physical_precise_round(scale),
            (alpha * 1000.0) as usize,
            [channel(r), channel(g), channel(b), a],
            Kind::Unspecified,
        ))
    }

    /// The target window's ring's two boxes, the glow first: their
    /// styles and where they go around `visual`.
    pub(crate) fn actions_ring(
        visual: Rectangle<f64, Logical>,
        radius: f64,
    ) -> [(BoxStyle, Rectangle<f64, Logical>); 2] {
        let [r, g, b] = ACTIONS_RING;
        let around = |by: f64| {
            Rectangle::new(
                (visual.loc.x - by, visual.loc.y - by).into(),
                (visual.size.w + 2.0 * by, visual.size.h + 2.0 * by).into(),
            )
        };
        let reach = ACTIONS_RING_WIDTH + ACTIONS_GLOW_WIDTH;
        [
            (
                BoxStyle {
                    fill: [0; 4],
                    rim: [r, g, b, (ACTIONS_GLOW_ALPHA * 255.0) as u8],
                    rim_width: (ACTIONS_GLOW_WIDTH * 1000.0) as u32,
                    radius: ((radius + reach) * 1000.0) as u32,
                },
                around(reach),
            ),
            (
                BoxStyle {
                    fill: [0; 4],
                    rim: [r, g, b, 255],
                    rim_width: (ACTIONS_RING_WIDTH * 1000.0) as u32,
                    radius: ((radius + ACTIONS_RING_WIDTH) * 1000.0) as u32,
                },
                around(ACTIONS_RING_WIDTH),
            ),
        ]
    }

    /// The window the palette on `output` acts on, and how far it shows
    /// (0 to 1) at `now`; `None` when it does not show there.
    pub(crate) fn actions_backdrop(
        &self,
        output: &Output,
        now: std::time::Duration,
    ) -> Option<(Option<WindowId>, f32)> {
        let layer = self.actions_on(output)?;
        let look = self
            .panel
            .popovers
            .look(&layer, now, self.popover_motion())?;
        if look.alpha <= 0.0 {
            return None;
        }
        let window = match &self.actions.target {
            Some((window, name)) if *name == output.name() => *window,
            _ => None,
        };
        Some((window, look.alpha))
    }
}

/// `[appearance] KEY = ON` written into `file`, on a thread of its own.
fn write_appearance(file: PathBuf, key: &'static str, on: bool) {
    let started = std::thread::Builder::new()
        .name("actions-appearance".to_owned())
        .spawn(move || {
            let written = hocus_config::edit(
                &file,
                |doc| {
                    doc["appearance"][key] = hocus_config::toml_edit::value(on);
                    Ok(())
                },
                |_| Ok(()),
            );
            match written {
                Ok(_) => tracing::info!(key, on, "the palette turned it over"),
                Err(err) => tracing::warn!(key, %err, "could not write the configuration"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to write the configuration");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shortcut_of_the_file_is_an_action_of_the_palette() {
        let shortcuts = crate::shortcuts::Shortcuts::default();
        for (key, _) in shortcuts.written() {
            if key == "actions" {
                continue;
            }
            let def = hocus_link::actions::of_shortcut(&key);
            assert!(
                def.is_some(),
                "[shortcuts] {key} is not in the palette's catalogue"
            );
        }
        // And each action the palette runs here runs something.
        for def in hocus_link::actions::catalogue() {
            if def.runs != Runs::Compositor || def.step.is_some() {
                continue;
            }
            let own = [
                "above",
                "all-desktops",
                "other-screen",
                "previous-desktop",
                "next-desktop",
                "high-contrast",
                "reduce-motion",
                "suspend",
                "restart",
                "power-off",
                "desktop-left",
                "desktop-right",
                "toggle-bar",
                "launcher",
            ];
            assert!(
                own.contains(&def.id) || shortcut_action(def.id).is_some(),
                "{} runs nothing",
                def.id
            );
        }
    }

    #[test]
    fn the_ring_goes_round_the_window_its_glow_outside() {
        let visual = Rectangle::new((100.0, 50.0).into(), (800.0, 600.0).into());
        let [(glow, around_glow), (ring, around_ring)] = State::actions_ring(visual, 10.0);
        assert_eq!(around_ring.loc, (97.5, 47.5).into());
        assert_eq!(around_ring.size, (805.0, 605.0).into());
        assert_eq!(around_glow.loc, (93.5, 43.5).into());
        assert_eq!(ring.rim, [0x7E, 0xE0, 0xA0, 255]);
        assert_eq!(glow.rim[3], 63);
    }
}
