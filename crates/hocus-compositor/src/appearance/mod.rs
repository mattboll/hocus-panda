//! `[appearance]` and `[locale]` followed live (#402): the theme and the accent go to the
//! shell and to the applications (the settings portal, [`portal`]); reduced
//! motion and reduced transparency to the shell's bars and to what the
//! compositor moves; high contrast to the theme the shell draws in (it
//! implies reduced transparency) and to the focused window's rim; the
//! cursor's theme and size to the cursor, and to the programs started
//! afterwards; the language and the formats to the shell, the language to
//! the programs started afterwards (`LANGUAGE`). The text size (#928) to
//! the shell, which draws its bar, popovers, launcher and notifications
//! at it, to the bar's room and glass, and to the applications (the
//! portal's `text-scaling-factor`).

pub mod portal;

use hocus_link::ToShell;

use crate::{State, config::Appearance};

impl State {
    /// What the settings portal says of the appearance now.
    pub fn portal_appearance(&self) -> portal::Values {
        portal::Values::of(
            self.theme_choice(),
            self.config.appearance.high_contrast,
            self.text_scale(),
        )
    }

    /// The file's `[appearance]` is now `new`: whether it changed.
    pub fn appearance_changed(&mut self, new: Appearance) -> bool {
        let old = std::mem::replace(&mut self.config.appearance, new);
        let now = self.config.appearance.clone();
        if now == old {
            return false;
        }
        let wallpaper = self.graphics.backgrounds.borrow().accent;
        if now.theme_choice(wallpaper) != old.theme_choice(wallpaper) {
            let choice = now.theme_choice(wallpaper);
            tracing::info!(theme = %choice.scheme, accent = %choice.accent, "the theme changed");
            self.tell_shell(ToShell::Theme(choice));
        }
        if (now.high_contrast, now.theme, now.accent) != (old.high_contrast, old.theme, old.accent)
        {
            // What the compositor draws itself, drawn again in its colours.
            self.radial_menu.paint = crate::radial_draw::RadialPaint::default();
            self.overview_paint = crate::overview_draw::OverviewPaint::default();
            self.magnifier.paint = crate::magnifier::draw::Paint::default();
        }
        if (
            now.reduced_motion,
            now.reduce_transparency,
            now.high_contrast,
        ) != (
            old.reduced_motion,
            old.reduce_transparency,
            old.high_contrast,
        ) {
            self.tell_shell(self.bar_look());
        }
        if now.text_scale != old.text_scale {
            tracing::info!(text_scale = now.text_scale.0, "the text size changed");
            self.tell_shell(ToShell::TextScale(now.text_scale));
            self.panel.popovers.zoom(now.text_scale.factor());
            // The bar's room is its width at the text size: the windows
            // the desktop placed take the new work area.
            self.rearrange();
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

    /// The file's `[locale]` is now `new`: whether it changed.
    pub fn locale_changed(&mut self, new: hocus_link::locale::Locale) -> bool {
        if new == self.config.locale {
            return false;
        }
        self.config.locale = new;
        tracing::info!(locale = %new, "the language and formats changed");
        self.tell_shell(ToShell::Locale(new));
        true
    }
}
