//! Notifications per application (#899, docs/design/notifications.md):
//! how each application's show (`[notifications.apps]`, which the
//! compositor follows and tells the shell), and the applications that sent
//! some, counted per day (the senders' file, which the shell writes and the
//! Settings read).
//!
//! An application goes by its desktop entry (the `desktop-entry` hint),
//! else by the name it gives (`app_name`): [`notify_app_id`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How an application's notifications show.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum NotifyLevel {
    /// A bubble, and the sound it asks for.
    #[default]
    All,
    /// A bubble without its sound.
    Silent,
    /// No bubble: kept in the bell's history, and counted.
    History,
    /// Dropped.
    None,
}

/// What a notification does, its application's level and its urgency
/// taken together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyFate {
    /// Its bubble (the history keeps it too); `sound`: the sound it asks
    /// for plays.
    Bubble { sound: bool },
    /// To the history only, counted by the bell.
    History,
    /// Nowhere.
    Drop,
}

impl NotifyLevel {
    /// The four, in the menus' order.
    pub const ALL: [Self; 4] = [Self::All, Self::Silent, Self::History, Self::None];

    /// The file's word for it.
    pub const fn word(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Silent => "silent",
            Self::History => "history",
            Self::None => "none",
        }
    }

    /// The level the file's `word` says.
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.word() == word.trim())
    }

    /// What a notification of this level does; `critical`: of critical
    /// urgency, which always makes a bubble, unless dropped. Do not disturb
    /// and a low urgency are the stack's, after this.
    pub const fn fate(self, critical: bool) -> NotifyFate {
        match (self, critical) {
            (Self::None, _) => NotifyFate::Drop,
            (Self::All, _) => NotifyFate::Bubble { sound: true },
            (Self::Silent, _) | (Self::History, true) => NotifyFate::Bubble { sound: false },
            (Self::History, false) => NotifyFate::History,
        }
    }
}

/// The id an application's notifications go by: its desktop entry, else
/// the name it gives.
pub fn notify_app_id(entry: Option<&str>, app: &str) -> String {
    entry
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| app.trim())
        .to_owned()
}

/// `[notifications.apps]`: each application's level, by its id; those not
/// named show everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyLevels {
    pub apps: BTreeMap<String, NotifyLevel>,
}

impl NotifyLevels {
    /// The level of application `id`: as named, else named in another case
    /// (an entry written by hand), else everything.
    pub fn of(&self, id: &str) -> NotifyLevel {
        self.apps.get(id).copied().unwrap_or_else(|| {
            self.apps
                .iter()
                .find(|(named, _)| named.eq_ignore_ascii_case(id))
                .map_or(NotifyLevel::All, |(_, level)| *level)
        })
    }
}

// --- The senders ----------------------------------------------------------------------

/// The senders' file's name, in `$XDG_STATE_HOME/hocuspanda`.
pub const SENDERS_FILE: &str = "notification-senders";
/// The days a count is said over: this one and the six before.
pub const WEEK: i64 = 7;
/// The days a sender is kept after its last notification.
pub const SENDERS_KEPT: i64 = 30;

/// An application that sent notifications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifySender {
    /// What it goes by ([`notify_app_id`]).
    pub id: String,
    /// Its name, as its last notification gave it.
    pub name: String,
    /// Its icon, as a name of the icon themes or a path, when known.
    #[serde(default)]
    pub icon: Option<String>,
    /// How many it sent each day, by the day's number (days since 1970-01-01
    /// on the wall clock's calendar, in the system's time zone).
    pub days: BTreeMap<i64, u32>,
}

impl NotifySender {
    /// How many it sent in the week to `today`.
    pub fn week(&self, today: i64) -> u32 {
        self.days
            .range(today - WEEK + 1..=today)
            .map(|(_, n)| *n)
            .sum()
    }

    /// The last day it sent one.
    pub fn last(&self) -> Option<i64> {
        self.days.keys().next_back().copied()
    }
}

/// The applications that sent notifications.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifySenders {
    pub apps: Vec<NotifySender>,
}

impl NotifySenders {
    /// Application `id` sent one `today`, under `name`, its icon `icon`.
    pub fn count(&mut self, id: &str, name: &str, icon: Option<String>, today: i64) {
        self.prune(today);
        if !self.apps.iter().any(|s| s.id == id) {
            self.apps.push(NotifySender {
                id: id.to_owned(),
                name: name.to_owned(),
                icon: None,
                days: BTreeMap::new(),
            });
        }
        let Some(sender) = self.apps.iter_mut().find(|s| s.id == id) else {
            return;
        };
        if !name.trim().is_empty() {
            name.clone_into(&mut sender.name);
        }
        if icon.is_some() {
            sender.icon = icon;
        }
        *sender.days.entry(today).or_default() += 1;
    }

    /// What is older than [`SENDERS_KEPT`] days on `today` goes, and the
    /// senders left with nothing.
    pub fn prune(&mut self, today: i64) {
        for sender in &mut self.apps {
            sender.days.retain(|day, _| *day > today - SENDERS_KEPT);
        }
        self.apps.retain(|s| !s.days.is_empty());
    }

    /// The senders, the busiest this week first, then the latest, then by
    /// name.
    pub fn busiest(&self, today: i64) -> Vec<&NotifySender> {
        let mut all: Vec<&NotifySender> = self.apps.iter().collect();
        all.sort_by(|a, b| {
            b.week(today)
                .cmp(&a.week(today))
                .then(b.last().cmp(&a.last()))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        all
    }

    /// As the file holds it.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The file's text; nothing from a text that does not read.
    pub fn decode(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_application_goes_by_its_entry_else_its_name() {
        assert_eq!(notify_app_id(Some("slack"), "Slack"), "slack");
        assert_eq!(notify_app_id(Some(" "), "Slack "), "Slack");
        assert_eq!(notify_app_id(None, "Claude Code"), "Claude Code");
        let levels = NotifyLevels {
            apps: [
                ("slack".to_owned(), NotifyLevel::Silent),
                ("Firefox".to_owned(), NotifyLevel::None),
            ]
            .into(),
        };
        assert_eq!(levels.of("slack"), NotifyLevel::Silent);
        // Written by hand in another case.
        assert_eq!(levels.of("firefox"), NotifyLevel::None);
        // Not named: everything.
        assert_eq!(levels.of("thunderbird"), NotifyLevel::All);
        assert_eq!(
            NotifyLevel::from_word("history"),
            Some(NotifyLevel::History)
        );
        assert_eq!(NotifyLevel::from_word("loud"), None);
    }

    #[test]
    fn the_four_levels_and_an_urgent_one() {
        use NotifyFate::{Bubble, Drop, History};
        let fates = |critical| NotifyLevel::ALL.map(|l| l.fate(critical));
        assert_eq!(
            fates(false),
            [
                Bubble { sound: true },
                Bubble { sound: false },
                History,
                Drop
            ]
        );
        // An urgent one always bubbles, unless dropped.
        assert_eq!(
            fates(true),
            [
                Bubble { sound: true },
                Bubble { sound: false },
                Bubble { sound: false },
                Drop
            ]
        );
    }

    #[test]
    fn the_weeks_counts_roll_over() {
        let mut senders = NotifySenders::default();
        let monday = 20_000;
        senders.count("slack", "Slack", Some("slack".to_owned()), monday);
        senders.count("slack", "Slack", None, monday);
        senders.count("firefox", "Firefox", None, monday + 2);
        let slack = |s: &NotifySenders| s.apps.iter().find(|a| a.id == "slack").cloned();
        // Its icon kept when the next one names none.
        assert_eq!(
            slack(&senders).and_then(|s| s.icon),
            Some("slack".to_owned())
        );
        // Counted the six days after, not the seventh.
        assert_eq!(slack(&senders).map(|s| s.week(monday + 6)), Some(2));
        assert_eq!(slack(&senders).map(|s| s.week(monday + 7)), Some(0));
        // The busiest first; on the seventh day, Firefox's one is busier.
        let order = |senders: &NotifySenders, today| -> Vec<String> {
            senders
                .busiest(today)
                .into_iter()
                .map(|s| s.id.clone())
                .collect()
        };
        assert_eq!(order(&senders, monday + 2), ["slack", "firefox"]);
        assert_eq!(order(&senders, monday + 7), ["firefox", "slack"]);
        // Kept 30 days after its last.
        senders.prune(monday + SENDERS_KEPT);
        assert_eq!(order(&senders, monday + SENDERS_KEPT), ["firefox"]);
        // Through the file.
        assert_eq!(NotifySenders::decode(&senders.encode()), senders);
        assert_eq!(NotifySenders::decode("{"), NotifySenders::default());
    }
}
