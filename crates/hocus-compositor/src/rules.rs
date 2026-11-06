//! Rules per application, `[[windows.rules]]` (6.14, docs/design/
//! windows.md §5): a window of an application with a rule opens on its
//! desktop (on the screen it opens on), at its size, and is kept above
//! the others. A dialog follows its parent, not the rule.
//!
//! The size is given in the window's first configure, as a window asking
//! to open maximised has it (§10), so that its first frame fits; one that
//! asks for a size of its own keeps it. The desktop and the size apply as
//! a window opens; `above` follows the file as it changes, for the windows
//! open too.

use smithay::{desktop::Window, wayland::seat::WaylandFocus};

use crate::{config::Rule, model::WindowId, windows::app_id};

impl crate::State {
    /// The rule of `window`, about to open: none for a dialog.
    pub(crate) fn opening_rule(&self, window: &Window) -> Option<Rule> {
        if window
            .wl_surface()
            .is_some_and(|s| self.dialog_of(&s).is_some())
        {
            return None;
        }
        let app = app_id(window)?;
        self.config.windows.rule(&app).cloned()
    }

    /// The rules changed in the file: the windows open are kept above the
    /// others, or no longer, as theirs say.
    pub(crate) fn rules_changed(&mut self) {
        let ids: Vec<WindowId> = self.model.stack().to_vec();
        for id in ids {
            if self.model.parent(id).is_some() {
                continue;
            }
            let above = self
                .windows
                .get(&id)
                .and_then(|m| app_id(&m.window))
                .and_then(|app| self.config.windows.rule(&app).map(|r| r.above))
                .unwrap_or(false);
            if above != self.model.is_above(id) {
                tracing::debug!(window = id.0, above, "kept above the others, or no longer");
                self.model.set_above(id, above);
            }
        }
        self.restack();
        self.redraw = true;
        // The bar marks them (#723).
        self.tell_window_states(false);
    }
}
