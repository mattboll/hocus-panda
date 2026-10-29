//! The keyboard's layouts for the bar's row (docs/design/bar.md §4.3,
//! 3.8): the compositor owns XKB, so it tells the shell the layouts its
//! keymap holds, and which is active whenever that changes, whatever
//! changed it (the row, a group-switch option of the user's XKB options);
//! and it switches to the next one when the shell asks.
//!
//! The names are the keymap's own (`xkb_keymap_layout_get_name`): the
//! symbols files' group names, which xkeyboard-config writes as the
//! registry's descriptions (`French (BEPO)`), already compiled in, with
//! nothing to read. The shell, which decides the bar's language, translates
//! them.

use hocus_link::{KeyLayout, ToShell};

use crate::{State, config::XkbNames};

/// The XKB codes of the layouts XKB is given, in its groups' order:
/// `fr,fr` with `,bepo` is `["fr", "fr(bepo)"]`. With none named, XKB
/// takes `XKB_DEFAULT_LAYOUT` and `XKB_DEFAULT_VARIANT` (or its own
/// default, unknown here: no code).
pub fn codes(names: &XkbNames) -> Vec<String> {
    let from_env = |name: &str| std::env::var(name).unwrap_or_default();
    let (layout, variant) = if names.layout.is_empty() {
        (
            from_env("XKB_DEFAULT_LAYOUT"),
            from_env("XKB_DEFAULT_VARIANT"),
        )
    } else {
        (names.layout.clone(), names.variant.clone())
    };
    let mut variants = variant.split(',').map(str::trim);
    layout
        .split(',')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|layout| match variants.next().unwrap_or_default() {
            "" => layout.to_owned(),
            variant => format!("{layout}({variant})"),
        })
        .collect()
}

impl State {
    /// The keymap's layouts, by group: their codes and names.
    fn keymap_layouts(&mut self) -> Vec<KeyLayout> {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return Vec::new();
        };
        let names: Vec<String> = keyboard.with_xkb_state(self, |context| {
            let xkb = context
                .xkb()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            xkb.layouts()
                .map(|layout| xkb.layout_name(layout).to_owned())
                .collect()
        });
        names
            .into_iter()
            .enumerate()
            .map(|(group, name)| KeyLayout {
                code: self.layout_codes.get(group).cloned().unwrap_or_default(),
                name,
            })
            .collect()
    }

    /// The active layout's group.
    fn active_layout(&self) -> Option<u32> {
        let keyboard = self.seat.get_keyboard()?;
        Some(keyboard.modifier_state().serialized.layout_effective)
    }

    /// Tells the shell, as it connects, the layouts and the active one.
    pub fn tell_layouts(&mut self) {
        let layouts = self.keymap_layouts();
        tracing::debug!(
            layouts = layouts.len(),
            "the keyboard's layouts to the shell"
        );
        self.tell_shell(ToShell::Layouts(layouts));
        self.shell.layout = None;
        self.layout_maybe_changed();
    }

    /// Tells the shell the active layout if it changed since it was told.
    pub fn layout_maybe_changed(&mut self) {
        let Some(active) = self.active_layout() else {
            return;
        };
        if self.shell.layout == Some(active) || !self.shell_linked() {
            return;
        }
        tracing::debug!(active, "the active layout");
        self.shell.layout = Some(active);
        self.tell_shell(ToShell::Layout(active));
    }

    /// The shell asks for the next layout: the seat's keyboard goes to it,
    /// wrapping around, and the client holding the keyboard gets the new
    /// modifiers (with the group); the others get them as it enters them.
    pub fn next_layout(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        keyboard.with_xkb_state(self, |mut context| context.cycle_next_layout());
        self.layout_maybe_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(layout: &str, variant: &str) -> XkbNames {
        XkbNames {
            model: String::new(),
            layout: layout.to_owned(),
            variant: variant.to_owned(),
            options: String::new(),
            from: "test",
        }
    }

    #[test]
    fn each_group_has_its_layout_and_its_variant() {
        assert_eq!(codes(&names("fr,us", "")), ["fr", "us"]);
        assert_eq!(
            codes(&names("fr,fr,ru", ",bepo,")),
            ["fr", "fr(bepo)", "ru"]
        );
        assert_eq!(codes(&names("us", "intl")), ["us(intl)"]);
    }
}
