//! A saved session: the windows that were open, where, and how to start
//! their applications again. Written by the compositor, read at the next
//! start to put everything back.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Session {
    #[serde(rename = "window")]
    pub windows: Vec<SavedWindow>,
}

/// A window as saved. Several windows of one application are told apart
/// by their title at save time, then by their order in the file: the order
/// they were opened in.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SavedWindow {
    pub app_id: String,
    pub title: String,
    /// What starts the application again; empty when unknown.
    pub command: Vec<String>,
    /// The output it was on, by connector name.
    pub output: String,
    /// Its desktop on that output, from 1.
    pub desktop: u32,
    /// Its frame, logical pixels, global.
    pub geometry: [i32; 4],
    /// `normal`, `maximized` or `fullscreen`.
    pub state: String,
    /// Shown on every desktop of its output.
    pub pinned: bool,
}

impl Session {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    /// The applications to start: one per command, in order, however many
    /// windows each had.
    pub fn commands(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for w in &self.windows {
            if !w.command.is_empty() && !out.iter().any(|(_, c)| *c == w.command) {
                out.push((w.app_id.clone(), w.command.clone()));
            }
        }
        out
    }
}

/// Which of the windows waiting to be put back a new window of `app_id`,
/// titled `title`, is (#192): the one of that application saved with this
/// very title, else the first of that application still waiting. Titles
/// change (a browser's follows its page), so a title that matches none
/// never keeps a window from being put back, in the order they were saved.
pub fn take_match(
    pending: &mut std::collections::VecDeque<SavedWindow>,
    app_id: &str,
    title: &str,
) -> Option<SavedWindow> {
    let mine = |w: &SavedWindow| w.app_id == app_id;
    let i = pending
        .iter()
        .position(|w| mine(w) && !title.is_empty() && w.title == title)
        .or_else(|| pending.iter().position(mine))?;
    pending.remove(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_of_one_app_are_told_apart_by_title_then_order() {
        let window = |title: &str, desktop| SavedWindow {
            app_id: "browser".into(),
            title: title.into(),
            desktop,
            ..SavedWindow::default()
        };
        let mut pending: std::collections::VecDeque<SavedWindow> = [
            window("Me", 1),
            window("Chapitô", 2),
            window("Website", 3),
            window("Website", 4),
        ]
        .into();
        let desktop = |w: Option<SavedWindow>| w.map(|w| w.desktop);
        assert_eq!(
            desktop(take_match(&mut pending, "browser", "Website")),
            Some(3)
        );
        // A title that changed since: the first still waiting.
        assert_eq!(
            desktop(take_match(&mut pending, "browser", "New tab")),
            Some(1)
        );
        assert_eq!(desktop(take_match(&mut pending, "browser", "")), Some(2));
        assert_eq!(desktop(take_match(&mut pending, "files", "Website")), None);
        assert_eq!(
            desktop(take_match(&mut pending, "browser", "Website")),
            Some(4)
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn a_session_round_trips_and_starts_each_app_once() {
        let window = |app: &str, desktop| SavedWindow {
            app_id: app.into(),
            title: format!("{app} window"),
            command: vec![app.into()],
            output: "DP-1".into(),
            desktop,
            geometry: [10, 20, 800, 600],
            state: "normal".into(),
            pinned: false,
        };
        let session = Session {
            windows: vec![window("foot", 2), window("foot", 3), window("files", 1)],
        };
        let text = session.to_toml();
        assert_eq!(Session::parse(&text), Ok(session.clone()));
        assert_eq!(
            session.commands(),
            vec![
                ("foot".into(), vec!["foot".into()]),
                ("files".into(), vec!["files".into()])
            ]
        );
    }
}
