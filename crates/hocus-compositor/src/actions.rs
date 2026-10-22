//! What shortcuts do: the actions of `hocus_core::Action`, carried out on
//! the compositor, and the keyboard's layouts from the configuration.

use std::process::Command;

use hocus_api::Event;
use hocus_core::{Action, Combo, Key, Modifiers, Scope};
use smithay::{
    input::keyboard::{KeysymHandle, ModifiersState, XkbConfig, xkb},
    output::Output,
    utils::Serial,
};

use crate::State;

/// Evdev codes of the top row's digits 1 to 9 then 0, and XKB's offset.
const KEY_1: u32 = 2;
const KEY_0: u32 = 11;
const XKB_OFFSET: u32 = 8;

/// The combination a key press makes, as shortcuts name them: digits by
/// position, other keys by their unshifted symbol in the active layout.
pub fn combo_of(modifiers: &ModifiersState, key: &KeysymHandle<'_>) -> Option<Combo> {
    let evdev = key.raw_code().raw().checked_sub(XKB_OFFSET)?;
    let key = if (KEY_1..=KEY_0).contains(&evdev) {
        Key::Digit(((evdev - KEY_1 + 1) % 10) as u8)
    } else {
        let sym = key.raw_latin_sym_or_raw_current_sym()?;
        Key::Named(xkb::keysym_get_name(sym).to_ascii_lowercase())
    };
    Some(Combo {
        modifiers: Modifiers {
            logo: modifiers.logo,
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
        },
        key,
    })
}

impl State {
    pub fn run_action(&mut self, action: &Action, serial: Serial) {
        match action {
            Action::Quit => {
                tracing::info!("quit requested");
                self.loop_signal.stop();
            }
            Action::Desktop(n) => {
                if let Some(output) = self.output_under_pointer() {
                    // An output with fewer desktops, or one, does not switch.
                    let _ = self.show_desktop(&output, n - 1, serial);
                }
            }
            Action::NextDesktop(scope) => self.step_desktop(*scope, 1, serial),
            Action::PreviousDesktop(scope) => self.step_desktop(*scope, -1, serial),
            Action::SendToDesktop(n) => {
                if let (Some(output), Some(window)) =
                    (self.output_under_pointer(), self.model.focused())
                {
                    let _ = self.send_window(window, &output, Some(n - 1), serial);
                }
            }
            Action::SendToNextDesktop => self.send_step(1, serial),
            Action::SendToPreviousDesktop => self.send_step(-1, serial),
            Action::SendToOutputLeft => self.send_to_side(-1, serial),
            Action::SendToOutputRight => self.send_to_side(1, serial),
            Action::ToggleMaximized => {
                if let Some(window) = self.model.focused() {
                    self.toggle_maximized(window, serial);
                }
            }
            Action::Snap(zone) => {
                if let Some(window) = self.model.focused() {
                    self.snap(window, *zone, serial);
                }
            }
            Action::ToggleTiling => {
                if let Some(output) = self.output_under_pointer()
                    && let Some(out) = self.model.output(&hocus_core::OutputName(output.name()))
                {
                    let desktop = out.shown;
                    let on = !self.model.is_tiling(desktop);
                    let _ = self.model.set_tiling(desktop, on);
                    self.sync_desktops();
                }
            }
            Action::MakeMaster => {
                if let Some(window) = self.model.focused() {
                    self.model.make_master(window);
                    self.sync_desktops();
                }
            }
            Action::GrowMaster | Action::ShrinkMaster => {
                let desktop = self.model.focused().and_then(|w| self.model.desktop_of(w));
                if let Some(desktop) = desktop {
                    let by = if *action == Action::GrowMaster {
                        0.05
                    } else {
                        -0.05
                    };
                    self.model.grow_master(desktop, by);
                    self.sync_desktops();
                }
            }
            Action::ToggleFloating => {
                if let Some(window) = self.model.focused() {
                    let floating = !self.model.is_floating(window);
                    self.model.set_floating(window, floating);
                    self.sync_desktops();
                }
            }
            Action::ToggleFullscreen => {
                if let Some(window) = self.model.focused() {
                    let on = !matches!(
                        self.windows.get(&window).map(|m| m.mode),
                        Some(crate::window_mode::WindowMode::Fullscreen { .. })
                    );
                    self.set_fullscreen(window, on, None, serial);
                }
            }
            Action::Minimize => {
                if let Some(window) = self.model.focused() {
                    self.minimize(window, serial);
                }
            }
            Action::Close => {
                let toplevel = self
                    .model
                    .focused()
                    .and_then(|id| self.windows.get(&id))
                    .and_then(|m| m.window.toplevel().cloned());
                if let Some(toplevel) = toplevel {
                    toplevel.send_close();
                }
            }
            Action::SwitchLayout => self.switch_layout(),
            Action::SwitchWindows(_) => {
                tracing::debug!("the window switcher comes with #39");
            }
            Action::Spawn(command) => self.spawn_command(command),
        }
    }

    /// The outputs an action with `scope` acts on.
    fn outputs_in(&self, scope: Scope) -> Vec<Output> {
        match scope {
            Scope::PointerOutput => self.output_under_pointer().into_iter().collect(),
            Scope::AllOutputs => self.space.outputs().cloned().collect(),
        }
    }

    /// Shows the next or previous desktop, round, on the outputs in scope.
    fn step_desktop(&mut self, scope: Scope, step: isize, serial: Serial) {
        for output in self.outputs_in(scope) {
            let Some(out) = self.model.output(&hocus_core::OutputName(output.name())) else {
                continue;
            };
            let count = out.desktops.len() as isize;
            let Some(current) = out.desktops.iter().position(|d| *d == out.shown) else {
                continue;
            };
            let next = (current as isize + step).rem_euclid(count) as usize;
            let _ = self.show_desktop(&output, next, serial);
        }
    }

    /// Sends the focused window to the next or previous desktop of its output.
    fn send_step(&mut self, step: isize, serial: Serial) {
        let Some(window) = self.model.focused() else {
            return;
        };
        let Some(host) = self.model.output_showing(window).cloned() else {
            return;
        };
        let count = host.desktops.len() as isize;
        let Some(current) = host.desktops.iter().position(|d| *d == host.shown) else {
            return;
        };
        let next = (current as isize + step).rem_euclid(count) as usize;
        if let Some(output) = self.find_output(&host.name.0) {
            let _ = self.send_window(window, &output, Some(next), serial);
        }
    }

    /// Sends the focused window to the nearest output on its left (`-1`) or
    /// right (`1`).
    fn send_to_side(&mut self, side: i32, serial: Serial) {
        let Some(window) = self.model.focused() else {
            return;
        };
        let Some(from) = self
            .windows
            .get(&window)
            .and_then(|m| self.output_of(&m.window))
            .and_then(|o| self.space.output_geometry(o))
        else {
            return;
        };
        let target = self
            .space
            .outputs()
            .filter_map(|o| Some((o.clone(), self.space.output_geometry(o)?)))
            .filter(|(_, g)| (g.loc.x - from.loc.x).signum() == side)
            .min_by_key(|(_, g)| (g.loc.x - from.loc.x).abs())
            .map(|(o, _)| o);
        if let Some(output) = target {
            let _ = self.send_window(window, &output, None, serial);
        }
    }

    /// The next keyboard layout, said in the event stream.
    fn switch_layout(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let layout = keyboard.with_xkb_state(self, |mut ctx| {
            ctx.cycle_next_layout();
            ctx.xkb()
                .lock()
                .map(|xkb| xkb.layout_name(xkb.active_layout()).to_string())
                .unwrap_or_default()
        });
        tracing::info!(layout, "keyboard layout");
        self.emit(|_| Event::KeyboardLayout { layout });
    }

    /// Starts a command through the shell; a thread waits for it, so the
    /// loop never does.
    fn spawn_command(&mut self, command: &str) {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(command)
            .env("WAYLAND_DISPLAY", &self.socket_path)
            .env(hocus_api::SOCKET_ENV, self.api.path())
            .env_remove("DISPLAY")
            .env_remove("LD_PRELOAD");
        if let Some(dir) = self.socket_path.parent() {
            cmd.env("XDG_RUNTIME_DIR", dir);
        }
        match cmd.spawn() {
            Ok(mut child) => {
                let spawned = std::thread::Builder::new()
                    .name("child".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
                if let Err(err) = spawned {
                    tracing::warn!(%err, "could not watch a started command");
                }
            }
            Err(err) => tracing::warn!(%err, command, "could not start"),
        }
    }

    /// Layouts, options and key repeat from the configuration; Num Lock on
    /// the first time, if asked.
    pub fn apply_keyboard_config(&mut self, first: bool) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let k = self.config.keyboard.clone();
        let layout = k.layouts.join(",");
        let xkb_config = XkbConfig {
            layout: &layout,
            options: (!k.options.is_empty()).then(|| k.options.clone()),
            ..Default::default()
        };
        if let Err(err) = keyboard.set_xkb_config(self, xkb_config) {
            tracing::warn!(?err, layout, "keyboard layouts not applied");
            self.config_warnings
                .push(format!("keyboard.layouts {:?}: unknown to XKB", k.layouts));
        }
        keyboard.change_repeat_info(
            i32::try_from(k.repeat_rate).unwrap_or(25),
            i32::try_from(k.repeat_delay).unwrap_or(200),
        );
        if first && k.numlock {
            let mut mods = keyboard.modifier_state();
            mods.num_lock = true;
            keyboard.set_modifier_state(mods);
        }
    }
}
