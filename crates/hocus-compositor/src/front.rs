//! The bar under the windows, and brought in front of them (docs/design/
//! bar.md §2, issue #519).
//!
//! The bar's own surfaces (its tint and its groups) are drawn under normal
//! windows, and a window over them takes the pointer there: the desktop
//! keeps its windows off the bar (the reserved zone), the user may put one
//! over it. Its popovers, the halo, the peek, the bubbles and the launcher
//! stay above the windows (`render.rs`).
//!
//! The pointer pushed against the screen's right edge over a bar, and
//! still there [`PUSH`] later, brings that bar in front, until the pointer
//! leaves the bar's column (the bar and the strip between it and the
//! edge). One timer of the loop while the pointer is against the edge;
//! nothing per frame. A fullscreen window in front covers the bar, pushed
//! or not.

use std::time::Duration;

use smithay::{
    output::Output,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Point},
};

use crate::{State, model::Mode};

/// How long the pointer rests against the edge before the bar comes in
/// front.
pub const PUSH: Duration = Duration::from_millis(300);

/// Where the bars stand to the windows.
#[derive(Debug, Default)]
pub enum Front {
    /// Every bar under the windows.
    #[default]
    Under,
    /// The pointer against the edge of screen `output`'s bar: in front
    /// when `timer` fires.
    Pushed {
        output: String,
        timer: RegistrationToken,
    },
    /// Screen `output`'s bar in front of the windows until the pointer
    /// leaves it.
    Raised { output: String },
}

/// Whether a layer surface of the shell's is one of the bar's own, under
/// the windows unless the bar is brought in front: its tint and its
/// groups, not its popovers, its peek or the bubbles.
pub fn in_the_bar(namespace: &str) -> bool {
    matches!(
        namespace,
        hocus_link::NAMESPACE
            | hocus_link::CLOCK_NAMESPACE
            | hocus_link::LEVELS_NAMESPACE
            | hocus_link::CONNECTIONS_NAMESPACE
            | hocus_link::DESKTOPS_NAMESPACE
            | hocus_link::AGENTS_NAMESPACE
            | hocus_link::FOOTER_NAMESPACE
            | hocus_link::IN_USE_NAMESPACE
            | hocus_link::MEETING_NAMESPACE
            | hocus_link::TRAY_NAMESPACE
    )
}

/// Which of a layer's surfaces a pass takes, drawing or finding the
/// surface under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    All,
    /// All but the bar's own.
    NotBar,
    /// The bar's own alone.
    Bar,
    Nothing,
}

impl Pick {
    /// The top layer's surfaces above the windows: all of them with the
    /// bar in front, else all but the bar's.
    pub fn above(bar_in_front: bool) -> Self {
        if bar_in_front {
            Self::All
        } else {
            Self::NotBar
        }
    }

    /// The top layer's surfaces under the windows: the bar's, unless it
    /// is in front.
    pub fn below(bar_in_front: bool) -> Self {
        if bar_in_front {
            Self::Nothing
        } else {
            Self::Bar
        }
    }

    pub fn takes(self, namespace: &str) -> bool {
        match self {
            Self::All => true,
            Self::NotBar => !in_the_bar(namespace),
            Self::Bar => in_the_bar(namespace),
            Self::Nothing => false,
        }
    }
}

impl State {
    /// Whether the bar on `output` is in front of the windows.
    pub fn bar_in_front(&self, output: &Output) -> bool {
        matches!(&self.panel.front, Front::Raised { output: o } if *o == output.name())
    }

    /// The pointer moved to `at` (global): against a bar's edge, the wait
    /// for it starts; away from it, or out of the bar in front, the bar
    /// goes back under the windows.
    pub fn bar_front_follow(&mut self, at: Point<f64, Logical>) {
        match &self.panel.front {
            Front::Raised { output } if self.in_bar_column(output, at) => return,
            Front::Pushed { output, .. } if self.pushed_against(output, at) => return,
            Front::Under => {}
            Front::Raised { output } => {
                tracing::debug!(output, "the bar goes back under the windows");
                self.panel.front = Front::Under;
                self.redraw = true;
            }
            Front::Pushed { timer, .. } => {
                self.loop_handle.remove(*timer);
                self.panel.front = Front::Under;
            }
        }
        // A window dragged to the edge snaps; it does not raise the bar.
        if self.seat.get_pointer().is_some_and(|p| p.is_grabbed()) {
            return;
        }
        let Some(output) = self
            .panel
            .bars
            .keys()
            .find(|name| self.pushed_against(name, at))
            .cloned()
        else {
            return;
        };
        let timer = Timer::from_duration(PUSH);
        let inserted = self.loop_handle.insert_source(timer, |_, _, app| {
            app.state.bar_pushed();
            TimeoutAction::Drop
        });
        match inserted {
            Ok(timer) => self.panel.front = Front::Pushed { output, timer },
            Err(err) => tracing::warn!(%err.error, "no timer for the bar's edge"),
        }
    }

    /// The pointer stayed against the bar's edge: the bar comes in front,
    /// and takes the pointer where it is.
    fn bar_pushed(&mut self) {
        let Front::Pushed { output, .. } = std::mem::take(&mut self.panel.front) else {
            return;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let covered = self
            .space
            .outputs()
            .find(|o| o.name() == output)
            .is_none_or(|o| self.fullscreen_in_front(o) || self.launcher_on(o).is_some());
        if covered || pointer.is_grabbed() {
            return;
        }
        tracing::debug!(output, "the bar comes in front of the windows");
        self.panel.front = Front::Raised { output };
        self.redraw = true;
        let at = pointer.current_location();
        let time = self.input_time();
        self.pointer_motion(at, time);
    }

    /// A bar went with its screen: it no longer stands in front.
    pub fn bar_front_gone(&mut self, name: &str) {
        match &self.panel.front {
            Front::Raised { output } if output == name => self.panel.front = Front::Under,
            Front::Pushed { output, timer } if output == name => {
                self.loop_handle.remove(*timer);
                self.panel.front = Front::Under;
            }
            _ => {}
        }
    }

    /// Whether `at` (global) is against the right edge of screen `name`,
    /// level with its bar.
    fn pushed_against(&self, name: &str, at: Point<f64, Logical>) -> bool {
        let Some(output) = self.space.outputs().find(|o| o.name() == name) else {
            return false;
        };
        let (Some(g), Some(bar)) = (
            self.space.output_geometry(output),
            self.bar_rect(output, self.now()),
        ) else {
            return false;
        };
        let g = g.to_f64();
        let top = g.loc.y + bar.loc.y;
        g.contains(at)
            && at.x >= g.loc.x + g.size.w - 1.0
            && (top..top + bar.size.h).contains(&at.y)
    }

    /// Whether `at` (global) is in the column of screen `name`'s bar: from
    /// the bar's left edge to the screen's right one, top to bottom.
    fn in_bar_column(&self, name: &str, at: Point<f64, Logical>) -> bool {
        let Some(output) = self.space.outputs().find(|o| o.name() == name) else {
            return false;
        };
        let (Some(g), Some(bar)) = (
            self.space.output_geometry(output),
            self.bar_rect(output, self.now()),
        ) else {
            return false;
        };
        let g = g.to_f64();
        g.contains(at) && at.x >= g.loc.x + bar.loc.x
    }

    /// Whether the window in front on `output`'s desktop is fullscreen.
    fn fullscreen_in_front(&self, output: &Output) -> bool {
        self.windows_on(output)
            .into_iter()
            .rev()
            .find(|id| !self.model.is_minimized(*id) && self.on_current_desktop(*id))
            .is_some_and(|id| self.model.mode(id) == Mode::Fullscreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bars_own_surfaces_go_under_and_the_rest_stays_above() {
        for ns in [
            hocus_link::NAMESPACE,
            hocus_link::CLOCK_NAMESPACE,
            hocus_link::LEVELS_NAMESPACE,
            hocus_link::CONNECTIONS_NAMESPACE,
            hocus_link::DESKTOPS_NAMESPACE,
            hocus_link::AGENTS_NAMESPACE,
            hocus_link::FOOTER_NAMESPACE,
            hocus_link::TRAY_NAMESPACE,
            hocus_link::MEETING_NAMESPACE,
        ] {
            assert!(in_the_bar(ns), "{ns}");
        }
        for ns in [
            hocus_link::POPOVER_NAMESPACE,
            hocus_link::TIP_NAMESPACE,
            hocus_link::PEEK_NAMESPACE,
            hocus_link::LAUNCHER_NAMESPACE,
            hocus_link::LAUNCHER_TOAST_NAMESPACE,
            hocus_link::BUBBLES_WAITING_NAMESPACE,
            &hocus_link::bubble_namespace(3),
            "waybar",
        ] {
            assert!(!in_the_bar(ns), "{ns}");
        }
        // Under the windows, the bar's surfaces are found after them; in
        // front, before them, with the rest.
        let bar = hocus_link::CLOCK_NAMESPACE;
        let popover = hocus_link::POPOVER_NAMESPACE;
        assert!(!Pick::above(false).takes(bar) && Pick::below(false).takes(bar));
        assert!(Pick::above(false).takes(popover) && !Pick::below(false).takes(popover));
        assert!(Pick::above(true).takes(bar) && !Pick::below(true).takes(bar));
        assert!(Pick::above(true).takes(popover) && !Pick::below(true).takes(popover));
    }
}
