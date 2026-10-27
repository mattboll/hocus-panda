//! What goes on on the desktops not shown (#192), for the desktop
//! switchers' badges: an agent waiting for an answer, a window that asked
//! to come forward too late to be let to (it asks for attention until it
//! has the keyboard), an agent done since its desktop was last shown (until
//! a screen shows it). Agents come from `agents`; this module keeps the
//! windows' side and forgets what was seen.

use std::collections::BTreeSet;

use hocus_core::{DesktopId, OutputName, WindowId};
use hocus_shell::activity::Badge;

use crate::State;

#[derive(Debug, Default)]
pub struct Activity {
    /// Windows asking for attention.
    attention: BTreeSet<WindowId>,
}

impl Activity {
    /// Whether a window asks for attention.
    pub fn asks(&self, id: WindowId) -> bool {
        self.attention.contains(&id)
    }
}

impl State {
    /// A window asks for attention: its desktop says so until it has the
    /// keyboard. The one with the keyboard needs none.
    pub fn ask_attention(&mut self, id: WindowId) {
        if self.model.focused() == Some(id) || !self.windows.contains_key(&id) {
            return;
        }
        if self.activity.attention.insert(id) {
            tracing::debug!(window = id.0, "asks for attention");
            self.redraw.all();
        }
    }

    /// It asks no more: an X11 window took its urgency back.
    pub fn drop_attention(&mut self, id: WindowId) {
        if self.activity.attention.remove(&id) {
            self.redraw.all();
        }
    }

    /// Whether a window asks for attention.
    pub fn asks_attention(&self, id: WindowId) -> bool {
        self.activity.asks(id)
    }

    /// After each dispatch: what was seen is forgotten. A window with the
    /// keyboard, or gone, asks no more; a desktop a screen shows has no
    /// agent done unseen.
    pub fn refresh_activity(&mut self) {
        let focused = self.model.focused();
        let windows = &self.windows;
        let before = self.activity.attention.len();
        self.activity
            .attention
            .retain(|id| Some(*id) != focused && windows.contains_key(id));
        let shown: Vec<DesktopId> = self.model.outputs().iter().map(|o| o.shown).collect();
        let board_before = self.agents.done_unseen.len();
        self.agents
            .done_unseen
            .retain(|_, desktop| !shown.contains(desktop));
        // An agent no longer done is no longer news.
        let done: Vec<String> = self.agents.done_unseen.keys().cloned().collect();
        for key in done {
            if !self.agent_still_done(&key) {
                self.agents.done_unseen.remove(&key);
            }
        }
        if self.activity.attention.len() != before || self.agents.done_unseen.len() != board_before
        {
            self.redraw.all();
        }
    }

    /// Each desktop's badge on `output`, in its order; none on the one it
    /// shows, nor on one another screen shows.
    pub fn desktop_badges(&self, output: &OutputName) -> Vec<Badge> {
        let Some(out) = self.model.output(output) else {
            return Vec::new();
        };
        let waiting = self.desktops_waiting();
        let asking: Vec<DesktopId> = self
            .activity
            .attention
            .iter()
            .filter_map(|id| self.model.desktop_of(*id))
            .collect();
        out.desktops
            .iter()
            .map(|d| {
                if self.desktop_is_shown(*d) {
                    Badge::None
                } else if waiting.contains(d) {
                    Badge::Waits
                } else if asking.contains(d) {
                    Badge::Attention
                } else if self.agents.done_unseen.values().any(|x| x == d) {
                    Badge::Done
                } else {
                    Badge::None
                }
            })
            .collect()
    }

    /// Each desktop's name on `output`, in its order: empty for one that
    /// has none.
    pub fn desktop_names(&self, output: &OutputName) -> Vec<String> {
        self.model.output(output).map_or_else(Vec::new, |o| {
            o.desktops
                .iter()
                .map(|d| self.desktop_name(*d).unwrap_or_default())
                .collect()
        })
    }
}
