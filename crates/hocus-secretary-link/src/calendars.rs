//! The calendars of each Google account, as her source last listed them
//! (#752): what the Agenda's consent sheet and row show, each to be read
//! or left out. Her process writes them in her folder ([`CALENDARS`]) as
//! it lists them; the Settings read them there. Names and who shares
//! them only: never an event.

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

/// The file, in her folder.
pub const CALENDARS: &str = "calendars.json";

/// A calendar of an account.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CalendarInfo {
    /// Google's id for it: what a consent leaves out.
    pub id: String,
    pub name: String,
    /// Someone else's, shared with the user: by whom (an address), when
    /// Google says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_by: Option<String>,
    /// Shared by someone without saying who.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
    /// Hidden from the user's list in Google Agenda.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Only its busy times may be read (free/busy shared).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub busy_only: bool,
    /// It could not be read the last time, and why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
}

impl CalendarInfo {
    /// What its row says after its name: « partagé par X », « masqué »,
    /// « occupé seulement », « illisible ».
    pub fn note(&self, fr: bool) -> String {
        let mut parts = Vec::new();
        match (&self.shared_by, self.shared, fr) {
            (Some(by), _, true) => parts.push(format!("partagé par {by}")),
            (Some(by), _, false) => parts.push(format!("shared by {by}")),
            (None, true, true) => parts.push("partagé".to_owned()),
            (None, true, false) => parts.push("shared".to_owned()),
            (None, false, _) => {}
        }
        if self.hidden {
            parts.push(if fr { "masqué" } else { "hidden" }.to_owned());
        }
        if self.busy_only {
            parts.push(
                if fr {
                    "ses heures occupées seulement"
                } else {
                    "its busy times only"
                }
                .to_owned(),
            );
        }
        if self.unreadable.is_some() {
            parts.push(if fr { "illisible" } else { "unreadable" }.to_owned());
        }
        parts.join(" · ")
    }
}

/// Every account's calendars, by Online Accounts' id for the account.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Calendars(pub BTreeMap<String, Vec<CalendarInfo>>);

impl Calendars {
    /// The file at `path`; none when it is not there or does not read.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::consent::write_atomically(path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn of(&self, account: &str) -> &[CalendarInfo] {
        self.0.get(account).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_calendar_says_who_shares_it_hidden_and_busy_only() {
        let shared = CalendarInfo {
            id: "bob@atelier-lune.fr".to_owned(),
            name: "Bob".to_owned(),
            shared_by: Some("bob@atelier-lune.fr".to_owned()),
            busy_only: true,
            ..CalendarInfo::default()
        };
        assert_eq!(
            shared.note(true),
            "partagé par bob@atelier-lune.fr · ses heures occupées seulement"
        );
        let hidden = CalendarInfo {
            hidden: true,
            ..CalendarInfo::default()
        };
        assert_eq!(hidden.note(true), "masqué");
        assert_eq!(hidden.note(false), "hidden");
    }
}
