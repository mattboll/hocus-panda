//! `[appearance]` followed live (#402): the theme and the accent go to the
//! shell and to the applications (the settings portal, [`portal`]); reduced
//! motion and reduced transparency to the shell's bars and to what the
//! compositor moves; high contrast to the focused window's rim; the
//! cursor's theme and size to the cursor, and to the programs started
//! afterwards.

pub mod portal;

use hocus_link::ToShell;

use crate::{State, config::Appearance};

impl State {
    /// What the settings portal says of the appearance now.
    pub fn portal_appearance(&self) -> portal::Values {
        let appearance = &self.config.appearance;
        portal::Values::of(appearance.theme_choice(), appearance.high_contrast)
    }

    /// The file's `[appearance]` is now `new`: whether it changed.
    pub fn appearance_changed(&mut self, new: Appearance) -> bool {
        let old = std::mem::replace(&mut self.config.appearance, new);
        let now = self.config.appearance.clone();
        if now == old {
            return false;
        }
        if now.theme_choice() != old.theme_choice() {
            let choice = now.theme_choice();
            tracing::info!(theme = %choice.scheme, accent = %choice.accent, "the theme changed");
            self.tell_shell(ToShell::Theme(choice));
        }
        if (now.reduced_motion, now.reduce_transparency)
            != (old.reduced_motion, old.reduce_transparency)
        {
            self.tell_shell(self.bar_look());
        }
        if (now.cursor_theme.as_ref(), now.cursor_size)
            != (old.cursor_theme.as_ref(), old.cursor_size)
        {
            let choice = crate::cursor::Choice::of(
                now.cursor_theme.as_deref(),
                now.cursor_size,
                &self.config.cursor_env,
            );
            self.restart_cursor(choice);
        }
        self.share.portal_appearance(self.portal_appearance());
        // The rims, the glass and what moves are drawn from it.
        self.redraw = true;
        true
    }
}
