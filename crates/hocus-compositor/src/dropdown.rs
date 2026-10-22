//! The drop-down terminal (Super+Backtick): started once, then shown and
//! hidden on the screen under the pointer, full width at the top of its
//! work area. It is an ordinary window kept on top and on every desktop of
//! its screen; hidden, it is minimized and keeps running. It slides on the
//! frame clock: its offset is read from the time when drawn.

use std::time::Duration;

use hocus_core::WindowId;
use hocus_ui::motion::{self, Animated, Curve, Easing};
use smithay::utils::{Logical, Rectangle, SERIAL_COUNTER, Serial};

use crate::State;

/// How long a start may take before Super+Backtick starts it again.
const START_PATIENCE: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct Dropdown {
    window: Option<WindowId>,
    /// When its command was started, until its window comes.
    started: Option<Duration>,
    shown: bool,
    /// 0 hidden above the screen, 1 in place.
    slide: Animated,
    height: i32,
}

impl Default for Dropdown {
    fn default() -> Self {
        Self {
            window: None,
            started: None,
            shown: false,
            slide: Animated::new(0.0),
            height: 0,
        }
    }
}

impl State {
    pub fn toggle_dropdown(&mut self) {
        let now = self.clock.now();
        let window = self
            .dropdown
            .window
            .filter(|id| self.windows.contains_key(id));
        match window {
            Some(_) if self.dropdown.shown => self.hide_dropdown(now),
            Some(id) => self.show_dropdown(id, SERIAL_COUNTER.next_serial()),
            None => {
                let starting = self
                    .dropdown
                    .started
                    .is_some_and(|t| now.saturating_sub(t) < START_PATIENCE);
                if !starting {
                    let command = self.config.dropdown.command.clone();
                    tracing::info!(command, "starting the drop-down terminal");
                    self.launch("/bin/sh", &["-c".to_owned(), command]);
                    self.dropdown.started = Some(now);
                }
            }
        }
    }

    /// A window just managed: the drop-down's when it has its app id.
    pub fn adopt_dropdown(&mut self, id: WindowId, app_id: Option<&str>, serial: Serial) {
        if app_id != Some(self.config.dropdown.app_id.as_str()) {
            return;
        }
        self.dropdown.window = Some(id);
        self.dropdown.started = None;
        self.show_dropdown(id, serial);
    }

    fn show_dropdown(&mut self, id: WindowId, serial: Serial) {
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        let here = self
            .windows
            .get(&id)
            .and_then(|m| self.output_of(&m.window))
            .is_some_and(|o| *o == output);
        if !here && let Err(err) = self.send_window(id, &output, None, serial) {
            tracing::warn!(%err, "could not move the drop-down terminal");
        }
        let _ = self.model.set_pinned(id, true);
        self.model.set_floating(id, true);
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.on_top = true;
        }
        let area = self.work_area(&output);
        let height = (f64::from(area.size.h) * self.config.dropdown.height).round() as i32;
        let rect = Rectangle::<i32, Logical>::new(area.loc, (area.size.w, height).into());
        self.dropdown.height = height;
        self.resize_to(id, rect, false);
        // Back from minimized, focused, on top.
        self.restore(id, serial);
        let now = self.clock.now();
        self.dropdown
            .slide
            .set(1.0, Curve::Ease(Easing::OutCubic, motion::STANDARD), now);
        self.dropdown.shown = true;
        self.needs_redraw = true;
    }

    fn hide_dropdown(&mut self, now: Duration) {
        self.dropdown.shown = false;
        self.dropdown
            .slide
            .set(0.0, Curve::Ease(Easing::InCubic, motion::SMALL), now);
        self.needs_redraw = true;
    }

    /// After each dispatch: minimizes it once it slid away, keeps frames
    /// coming while it slides, and forgets a window that closed.
    pub fn refresh_dropdown(&mut self) {
        let now = self.clock.now();
        let Some(id) = self.dropdown.window else {
            return;
        };
        if !self.windows.contains_key(&id) {
            self.dropdown = Dropdown::default();
            return;
        }
        if self.dropdown.slide.is_running(now) {
            self.needs_redraw = true;
        } else if !self.dropdown.shown && !self.model.is_minimized(id) {
            self.minimize(id, SERIAL_COUNTER.next_serial());
        }
    }

    /// How far above its place the drop-down is drawn now, logical pixels.
    pub fn dropdown_offset(&self, id: WindowId) -> Option<i32> {
        if self.dropdown.window != Some(id) {
            return None;
        }
        let hidden = 1.0 - self.dropdown.slide.value(self.clock.now());
        Some(-((hidden * (self.dropdown.height + 24) as f32).round() as i32))
    }
}
