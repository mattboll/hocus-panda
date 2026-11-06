//! The notifications' bubbles (5.3): layer surfaces of the shell's, one per
//! bubble, each of its own namespace ([`hocus_link::bubble_namespace`]),
//! the shell's server deciding what shows, where and for how long.
//!
//! The shell draws a bubble, its shadow and its countdown; the compositor
//! moves it: from its first frame it slides in from the bar's side
//! ([`BUBBLE_SHIFT`] logical pixels) and fades in over [`BUBBLE_IN`], and
//! out over [`BUBBLE_OUT`] once the shell says it goes (`closing`), the
//! shell removing it after that. With reduced motion, or a shell on the
//! CPU, it fades only. A bubble updated in place commits again and does not
//! move.
//!
//! The compositor also tells the shell which screen the bubbles show on,
//! the focused window's (else the pointer's), when it changes; and brings
//! the window of a bubble's application when the bubble is clicked.

use std::time::Duration;

use hocus_link::{BUBBLE_IN, BUBBLE_OUT, BUBBLE_SHIFT, ToShell};
use smithay::{
    backend::renderer::utils::with_renderer_surface_state, desktop::LayerSurface, output::Output,
};

use crate::{
    State,
    model::WindowId,
    motion::{Bezier, Tween},
};

/// The mockup's way in, `cubic-bezier(.2, .8, .2, 1)`.
const IN: Bezier = Bezier(0.2, 0.8, 0.2, 1.0);

/// The bubbles shown, with their motion, and the screen told the shell.
#[derive(Debug, Default)]
pub struct Bubbles {
    shown: Vec<Shown>,
    told: Option<String>,
    /// Commits of the bubbles' surfaces so far (tests).
    pub commits: u64,
}

#[derive(Debug)]
struct Shown {
    layer: LayerSurface,
    /// Its first frame, on the compositor's clock.
    opened: Duration,
    /// Since when it goes.
    closing: Option<Duration>,
}

/// How a bubble is drawn at a moment: how opaque, and how far toward the
/// bar from where it is (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub alpha: f32,
    pub shift: f64,
}

/// How a bubble opened at `opened` (and going since `closing`) is drawn at
/// `now`; `slide`: it moves too, else it only fades.
fn look(opened: Duration, closing: Option<Duration>, now: Duration, slide: bool) -> Look {
    let shown = match closing {
        None => Tween::new(opened, BUBBLE_IN, IN).at(now),
        Some(since) => 1.0 - Tween::new(since, BUBBLE_OUT, Bezier::EASE_IN).at(now),
    }
    .clamp(0.0, 1.0);
    Look {
        alpha: shown as f32,
        shift: if slide {
            BUBBLE_SHIFT * (1.0 - shown)
        } else {
            0.0
        },
    }
}

impl Shown {
    fn moving(&self, now: Duration) -> bool {
        match self.closing {
            Some(since) => now < since + BUBBLE_OUT,
            None => now < self.opened + BUBBLE_IN,
        }
    }
}

impl Bubbles {
    /// Bubble `layer`'s look at `now`, if it shows; `None` for a bubble
    /// not drawn yet (it shows from its first frame).
    pub fn look(&self, layer: &LayerSurface, now: Duration, slide: bool) -> Option<Look> {
        let shown = self.shown.iter().find(|s| &s.layer == layer)?;
        Some(look(shown.opened, shown.closing, now, slide))
    }

    /// Whether one still moves at `now`.
    pub fn tick(&self, now: Duration) -> bool {
        self.shown.iter().any(|s| s.moving(now))
    }
}

impl State {
    /// Whether bubbles slide in and out (else they fade only).
    pub fn bubbles_slide(&self) -> bool {
        !self.reduced_motion() && !self.shell.software
    }

    /// A commit of bubble `layer`: its first frame starts its motion.
    pub fn bubble_committed(&mut self, layer: &LayerSurface) {
        self.bubbles.commits += 1;
        if self.bubbles.shown.iter().any(|s| &s.layer == layer) {
            return;
        }
        let drawn = with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some())
            .unwrap_or(false);
        if !drawn {
            return;
        }
        let opened = self.now();
        // Said, so that the journal tells a bubble the shell opened from
        // one the screen showed (#710).
        if hocus_link::is_bubble(layer.namespace()) {
            tracing::info!(namespace = layer.namespace(), "a bubble is drawn");
        }
        self.bubbles.shown.push(Shown {
            layer: layer.clone(),
            opened,
            closing: None,
        });
        self.redraw = true;
    }

    /// The shell says the bubble of `namespace` goes.
    pub fn bubble_closing(&mut self, namespace: &str) {
        let now = self.now();
        for shown in &mut self.bubbles.shown {
            if shown.layer.namespace() == namespace {
                shown.closing.get_or_insert(now);
                self.redraw = true;
            }
        }
    }

    /// A layer surface went: if it was a bubble, its motion goes too.
    /// One that never showed is said: the shell took it for shown, the
    /// user never saw it (#710).
    pub fn bubble_gone(&mut self, layer: &LayerSurface) {
        let shown = self.bubbles.shown.iter().any(|s| &s.layer == layer);
        if !shown && hocus_link::is_bubble(layer.namespace()) {
            tracing::warn!(
                namespace = layer.namespace(),
                "a bubble's surface went without ever being drawn"
            );
        }
        self.bubbles.shown.retain(|s| &s.layer != layer);
    }

    /// The screen the bubbles show on: the focused window's, else the one
    /// under the pointer.
    pub(crate) fn bubbles_screen(&self) -> Option<Output> {
        self.model
            .focused()
            .filter(|id| self.windows.contains_key(id) && !self.model.is_minimized(*id))
            .and_then(|id| self.output_of(id))
            .or_else(|| self.output_under_pointer())
    }

    /// Once a turn of the loop: the shell is told the bubbles' screen when
    /// it changed.
    pub fn flush_bubbles_screen(&mut self) {
        if !self.shell_linked() {
            return;
        }
        let Some(screen) = self.bubbles_screen().map(|o| o.name()) else {
            return;
        };
        if self.bubbles.told.as_deref() == Some(screen.as_str()) {
            return;
        }
        tracing::debug!(screen, "the bubbles' screen");
        self.bubbles.told = Some(screen.clone());
        self.tell_shell(ToShell::Focus(screen));
    }

    /// The shell connected: it is told the screen again.
    pub fn tell_bubbles_screen(&mut self) {
        self.bubbles.told = None;
    }

    /// A bubble was clicked: the window of its application `app` (by its
    /// id), else of the process `pid` that sent it or one of its parents
    /// (a terminal running `notify-send`), comes to the front.
    pub fn raise_app(&mut self, pid: u32, app: &str) {
        let found = self
            .window_of_app(app)
            .or_else(|| self.window_of_process(pid));
        match found {
            Some(id) => {
                tracing::debug!(app, pid, window = id.0, "a bubble brings its window");
                self.window_asked(id.0);
            }
            None => tracing::debug!(app, pid, "a bubble's application has no window"),
        }
    }

    /// The topmost window whose application's id is `app`'s, ignoring
    /// case, or its last part (`org.mozilla.firefox`, `firefox`).
    fn window_of_app(&self, app: &str) -> Option<WindowId> {
        let app = app.trim_end_matches(".desktop").to_lowercase();
        if app.is_empty() {
            return None;
        }
        let last = app.rsplit('.').next().unwrap_or_default().to_owned();
        self.model.stack().iter().rev().copied().find(|id| {
            self.windows
                .get(id)
                .and_then(|m| crate::windows::app_id(&m.window))
                .is_some_and(|id| {
                    let id = id.to_lowercase();
                    id == app || id == last || id.rsplit('.').next() == Some(last.as_str())
                })
        })
    }

    /// The topmost window of process `pid`, else of its nearest parent
    /// that has one.
    fn window_of_process(&self, pid: u32) -> Option<WindowId> {
        if pid == 0 {
            return None;
        }
        self.window_of_ancestry(&hocus_agents::process::ancestry(pid))
    }

    /// The topmost window of the first process of `ancestry` (a process,
    /// then its parents, nearest first) that has one.
    pub(crate) fn window_of_ancestry(&self, ancestry: &[u32]) -> Option<WindowId> {
        let mut best: Option<(usize, WindowId)> = None;
        for id in self.model.stack().iter().copied() {
            let Some(window_pid) = self.windows.get(&id).and_then(|m| self.pid_of(&m.window))
            else {
                continue;
            };
            let Some(depth) = ancestry.iter().position(|p| *p == window_pid) else {
                continue;
            };
            // The nearest parent; of its windows, the topmost (the stack
            // goes bottom first).
            if best.is_none_or(|(d, _)| depth <= d) {
                best = Some((depth, id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// The process of a window's client.
    fn pid_of(&self, window: &smithay::desktop::Window) -> Option<u32> {
        use smithay::reexports::wayland_server::Resource;
        if let Some(x11) = window.x11_surface() {
            return x11.pid();
        }
        let surface = smithay::wayland::seat::WaylandFocus::wl_surface(window)?;
        let client = self.display_handle.get_client(surface.id()).ok()?;
        let credentials = client.get_credentials(&self.display_handle).ok()?;
        u32::try_from(credentials.pid).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(opened: u64, closing: Option<u64>, now: u64, slide: bool) -> Look {
        let ms = Duration::from_millis;
        look(ms(opened), closing.map(ms), ms(now), slide)
    }

    #[test]
    fn a_bubble_slides_in_from_the_bar_then_out() {
        // Its first frame: transparent, 26 px toward the bar.
        let start = at(1000, None, 1000, true);
        assert_eq!((start.alpha, start.shift), (0.0, BUBBLE_SHIFT));
        // Most of the way at half its time; home at 220 ms.
        let mid = at(1000, None, 1110, true);
        assert!(mid.alpha > 0.8 && mid.shift < 5.0, "{mid:?}");
        assert_eq!(
            at(1000, None, 1220, true),
            Look {
                alpha: 1.0,
                shift: 0.0
            }
        );
        // Out, ease-in: little gone at first, gone at 170 ms.
        let early = at(1000, Some(2000), 2030, true);
        assert!(early.alpha > 0.8, "{early:?}");
        let gone = at(1000, Some(2000), 2170, true);
        assert_eq!((gone.alpha, gone.shift), (0.0, BUBBLE_SHIFT));
        // Without sliding: it fades, in place.
        let fading = at(1000, None, 1050, false);
        assert!(fading.alpha > 0.0 && fading.alpha < 1.0, "{fading:?}");
        assert_eq!(fading.shift, 0.0);
        assert_eq!(at(1000, Some(2000), 2100, false).shift, 0.0);
    }
}
