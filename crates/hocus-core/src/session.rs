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

#[cfg(test)]
mod tests {
    use super::*;

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
