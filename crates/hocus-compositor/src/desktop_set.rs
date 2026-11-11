//! The set of desktops changing while the session runs (6.14,
//! docs/design/windows.md §12): `[desktops] count` and `names` followed
//! as the file changes, and a desktop taken out or put back by the
//! Settings.
//!
//! - **The file's count.** More desktops are added after the last, empty,
//!   on every screen; fewer take out the last ones, their windows going to
//!   the last that stays. Nothing slides: a screen showing a desktop taken
//!   out shows the one its windows went to.
//! - **The Settings' line** (`msg screens`, displays.rs) takes a desktop
//!   out wherever it is: `desktop-remove N` (from 1) gives its windows to
//!   the desktop before it (the first's to the next one), on every
//!   screen, and the desktops after it move down one. The answer names
//!   the windows that moved, which `desktop-restore N ID…` (the toast's
//!   "Annuler") takes back to a desktop put back at `N`. The Settings ask
//!   first and write the file once answered: the file then says the count
//!   the desktop already has, and nothing moves twice.
//! - **The names** go to the shell (the tiles' list, their names for
//!   screen readers) and to Alt+Tab's other desktops' suffix.
//!
//! The Settings' line hears how many windows each desktop has, every
//! screen's together, as they change: the page's chips say it.

use hocus_link::ToShell;
use smithay::output::Output;

use crate::{desktops::MAX_COUNT, model::WindowId};

/// Windows on each desktop, every screen's together, as the Settings were
/// last told.
pub type Counts = (usize, [u16; MAX_COUNT]);

/// Alt+Tab's suffix to a window of another desktop (from 0): its name,
/// else its number (6.14).
pub fn suffix(desktops: &crate::config::Desktops, desktop: usize) -> String {
    match desktops.name(desktop) {
        Some(name) => format!(" · {name}"),
        None => format!(" · {}", desktop + 1),
    }
}

impl crate::State {
    /// `[desktops]` changed in the file: its count and names followed.
    /// Whether something changed.
    pub(crate) fn desktops_config_changed(&mut self, new: &crate::config::Desktops) -> bool {
        let mut changed = false;
        if new.count != self.config.desktops.count {
            let count = new.count;
            tracing::info!(
                from = self.config.desktops.count,
                to = count,
                "the desktops' count changed"
            );
            self.renumber_desktops(count, |d| d.min(count - 1), &[]);
            changed = true;
        }
        if new.names != self.config.desktops.names {
            self.config.desktops.names = new.names.clone();
            self.desktop_names_changed();
            changed = true;
        }
        changed
    }

    /// The names changed: the shell is told them, Alt+Tab says them.
    fn desktop_names_changed(&mut self) {
        self.tell_shell(ToShell::DesktopNames(self.config.desktops.shown_names()));
        if self.switching.open.is_some() {
            self.redraw = true;
        }
    }

    /// The desktops are now `count` on every screen; a window on desktop
    /// `d` goes to `map(d)`, as does a screen showing `d`, and each of
    /// `put` to its own. Nothing slides: windows show or hide at once, and
    /// the focus goes to the top window shown if its window no longer
    /// shows.
    fn renumber_desktops(
        &mut self,
        count: usize,
        map: impl Fn(usize) -> usize,
        put: &[(WindowId, usize)],
    ) {
        let count = count.clamp(1, MAX_COUNT);
        self.config.desktops.count = count;
        let last = count - 1;
        let ids: Vec<WindowId> = self.model.stack().to_vec();
        for &id in &ids {
            let to = put
                .iter()
                .find(|(w, _)| *w == id)
                .map_or_else(|| map(self.model.desktop(id)), |(_, d)| *d);
            self.model.set_desktop(id, to.min(last));
            if let Some(managed) = self.windows.get_mut(&id) {
                managed.travel = None;
            }
        }
        // Every screen's strip, those of screens unplugged too, which they
        // show again if they come back.
        let names: Vec<String> = self.strips.keys().cloned().collect();
        for name in names {
            if let Some(strip) = self.strips.get_mut(&name) {
                let current = map(strip.current());
                strip.renumber(count, current);
            }
        }
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for output in &outputs {
            let strip = self.strip(output);
            self.strips.insert(output.name(), strip);
            let current = strip.current();
            for id in self.windows_on(output) {
                if Some(id) == self.moving {
                    // Dragged: it stays under the pointer, on the desktop
                    // shown.
                    self.model.set_desktop(id, current);
                    continue;
                }
                let shown = !self.model.placement(id).hidden;
                if self.model.desktop(id) == current {
                    self.unhide(id);
                } else if shown {
                    self.hide(id);
                }
            }
        }
        let next = self
            .model
            .focused()
            .filter(|id| self.model.is_shown(*id))
            .or_else(|| self.model.top_shown());
        if next != self.model.focused() {
            self.focus(next);
        }
        // Alt+Tab's list was of the desktops before.
        if self.switching.open.is_some() {
            self.end_switcher(false);
        }
        self.redraw = true;
    }

    /// The Settings take desktop `number` (from 1) out of every screen:
    /// its windows go to the one before it (the first's to the next one),
    /// the desktops after it move down one, its name goes with it. The
    /// answer for the Settings' line, or why it cannot be.
    pub(crate) fn desktop_remove(&mut self, number: usize) -> Result<serde_json::Value, String> {
        let count = self.config.desktops.count;
        if count <= 1 {
            return Err("the last desktop stays".to_owned());
        }
        let Some(gone) = number.checked_sub(1).filter(|g| *g < count) else {
            return Err(format!("no desktop {number} of {count}"));
        };
        let moved: Vec<u64> = self
            .model
            .stack()
            .iter()
            .filter(|id| self.model.desktop(**id) == gone)
            .map(|id| id.0)
            .collect();
        // The one before it, or the next one, which moves down to its place.
        let to = gone.saturating_sub(1);
        tracing::info!(
            desktop = number,
            windows = moved.len(),
            to = to + 1,
            "a desktop taken out by the Settings"
        );
        self.renumber_desktops(
            count - 1,
            |d| match d.cmp(&gone) {
                std::cmp::Ordering::Less => d,
                std::cmp::Ordering::Equal => to,
                std::cmp::Ordering::Greater => d - 1,
            },
            &[],
        );
        if gone < self.config.desktops.names.len() {
            self.config.desktops.names.remove(gone);
            self.desktop_names_changed();
        }
        Ok(serde_json::json!({
            "desktop-removed": { "desktop": number, "to": to + 1, "windows": moved }
        }))
    }

    /// The Settings put a desktop back at `number` (from 1) on every
    /// screen, `windows` on it (those still there): the toast's "Annuler"
    /// after [`Self::desktop_remove`]. The answer for the Settings' line,
    /// or why it cannot be.
    pub(crate) fn desktop_restore(
        &mut self,
        number: usize,
        windows: &[u64],
    ) -> Result<serde_json::Value, String> {
        let count = self.config.desktops.count;
        if count >= MAX_COUNT {
            return Err(format!("{MAX_COUNT} desktops at most"));
        }
        let Some(at) = number.checked_sub(1).filter(|a| *a <= count) else {
            return Err(format!("no place {number} among {count} desktops"));
        };
        let put: Vec<(WindowId, usize)> = windows
            .iter()
            .map(|id| WindowId(*id))
            .filter(|id| self.windows.contains_key(id))
            .map(|id| (id, at))
            .collect();
        tracing::info!(
            desktop = number,
            windows = put.len(),
            "a desktop put back by the Settings"
        );
        self.renumber_desktops(count + 1, |d| if d >= at { d + 1 } else { d }, &put);
        if at < self.config.desktops.names.len() {
            self.config.desktops.names.insert(at, String::new());
            self.desktop_names_changed();
        }
        Ok(serde_json::json!({
            "desktop-restored": { "desktop": number, "windows": put.iter().map(|(id, _)| id.0).collect::<Vec<_>>() }
        }))
    }

    /// How many windows each desktop has, every screen's together,
    /// minimised ones too.
    pub(crate) fn desktop_counts(&self) -> Counts {
        let mut windows = [0u16; MAX_COUNT];
        for id in self.model.stack() {
            if let Some(n) = windows.get_mut(self.model.desktop(*id)) {
                *n = n.saturating_add(1);
            }
        }
        (self.config.desktops.count, windows)
    }

    /// The Settings' line's line for `counts`.
    pub(crate) fn desktops_line((count, windows): Counts) -> String {
        serde_json::json!({
            "desktops": { "count": count, "windows": windows.get(..count).unwrap_or(&windows) }
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn alt_tab_says_another_desktops_name_or_its_number() {
        let desktops = crate::config::Desktops {
            count: 4,
            names: vec!["Travail".into(), "Courrier".into(), String::new()],
        };
        assert_eq!(super::suffix(&desktops, 1), " · Courrier");
        assert_eq!(super::suffix(&desktops, 2), " · 3");
        assert_eq!(super::suffix(&desktops, 3), " · 4");
    }

    #[test]
    fn the_line_says_each_desktops_windows() {
        let line = crate::State::desktops_line((3, [2, 0, 1, 0, 0, 0, 0, 0, 0]));
        assert_eq!(line, r#"{"desktops":{"count":3,"windows":[2,0,1]}}"#);
    }
}
