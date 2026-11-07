//! What the meetings' reminders (Day.6) need of the compositor: `Super+J`,
//! the screens a fullscreen window covers (their tab shows at the screen's
//! edge, over it), and the screens shared (their tab hides its title). The
//! shell draws the tabs; the compositor moves them in and out as bubbles
//! ([`hocus_link::slides_in`]).

use hocus_link::{ShareSource, ToShell};

use crate::{
    State,
    model::{Mode, Zone},
};

/// What the shell was told.
#[derive(Debug, Default)]
pub struct Reminders {
    fullscreen: Option<Vec<String>>,
    /// The screens whose bar is opaque behind a maximised window.
    opaque: Option<Vec<String>>,
}

impl State {
    /// `Super+J`: the shell joins the meeting whose reminder shows, else
    /// the one starting next within 10 min.
    pub fn join_shortcut(&mut self) {
        if !self.shell_linked() {
            tracing::info!("no shell: no meeting to join");
            return;
        }
        self.tell_shell(ToShell::Join);
    }

    /// The screens whose front window is full screen.
    fn fullscreen_screens(&self) -> Vec<String> {
        let mut screens: Vec<String> = self
            .space
            .outputs()
            .filter(|o| self.fullscreen_in_front(o))
            .map(|o| o.name())
            .collect();
        screens.sort();
        screens
    }

    /// Once a turn of the loop: the shell is told the screens a fullscreen
    /// window covers, when they changed.
    pub fn flush_fullscreen(&mut self) {
        if !self.shell_linked() {
            return;
        }
        let screens = self.fullscreen_screens();
        if self.reminders.fullscreen.as_ref() == Some(&screens) {
            return;
        }
        tracing::debug!(?screens, "the screens in full screen");
        self.reminders.fullscreen = Some(screens.clone());
        self.tell_shell(ToShell::Fullscreen(screens));
    }

    /// The shell connected: it is told them again.
    pub fn tell_fullscreen_again(&mut self) {
        self.reminders.fullscreen = None;
        self.reminders.opaque = None;
    }

    /// The screens with a bar whose current desktop shows a maximised
    /// window: their bar's glass falls back to opaque (bar.md §11), as
    /// with reduced transparency, the glass beside it a band of blurred
    /// wallpaper between two opaque surfaces otherwise.
    fn opaque_screens(&self) -> Vec<String> {
        let mut screens: Vec<String> = self
            .space
            .outputs()
            .filter(|o| self.panel.bars.contains_key(&o.name()))
            .filter(|o| {
                self.windows_on(o).into_iter().any(|id| {
                    !self.model.is_minimized(id)
                        && self.on_current_desktop(id)
                        && self.model.mode(id) == Mode::Snapped(Zone::Maximized)
                })
            })
            .map(|o| o.name())
            .collect();
        screens.sort();
        screens
    }

    /// Whether the bar on `output` is opaque behind a maximised window, as
    /// the shell was last told.
    pub fn bar_opaque(&self, output: &str) -> bool {
        self.reminders
            .opaque
            .as_ref()
            .is_some_and(|o| o.iter().any(|s| s == output))
    }

    /// Once a turn of the loop: the shell is told the screens whose bar is
    /// opaque behind a maximised window, when they changed; the glass
    /// goes, or comes back, with the next frame.
    pub fn flush_opaque_bars(&mut self) {
        if !self.shell_linked() {
            return;
        }
        let screens = self.opaque_screens();
        if self.reminders.opaque.as_ref() == Some(&screens) {
            return;
        }
        tracing::debug!(?screens, "the bars opaque behind a maximised window");
        self.reminders.opaque = Some(screens.clone());
        self.tell_shell(ToShell::OpaqueBars(screens));
        self.redraw = true;
    }

    /// The screens shared now, by their connectors' names (5.11).
    pub fn tell_shared_screens(&mut self) {
        let mut screens: Vec<String> = self
            .share
            .shares
            .values()
            .filter(|s| s.node.is_some())
            .filter_map(|s| match &s.source {
                ShareSource::Screen(name) => Some(name.clone()),
                ShareSource::Window(_) => None,
            })
            .collect();
        screens.sort();
        screens.dedup();
        self.tell_shell(ToShell::SharedScreens(screens));
    }
}
