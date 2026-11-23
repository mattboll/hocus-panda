//! Notifications per application (#899, docs/design/notifications.md):
//! how each application's show (`[notifications.apps]`, which the
//! compositor follows and tells the shell), and the applications that sent
//! some, counted per day (the senders' file, which the shell writes and the
//! Settings read).
//!
//! An application goes by its desktop entry (the `desktop-entry` hint),
//! else by the name it gives (`app_name`): [`notify_app_id`].
//!
//! The bell keeps what still needs the user (#923): which window has the
//! user's focus ([`AppFocus`]), whether opening an application folds its
//! notifications (`[notifications.seen-on-focus]`, with the levels), and
//! what an agent asks of the bell through `hocus mcp` ([`NotesAsk`],
//! answered by [`NotesAnswer`]).

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
/// named show everything. And `[notifications.seen-on-focus]` (#923):
/// whether opening an application folds its notifications, by its id;
/// those not named do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyLevels {
    pub apps: BTreeMap<String, NotifyLevel>,
    #[serde(default)]
    pub seen_on_focus: BTreeMap<String, bool>,
}

/// `map`'s value for `id`: as named, else named in another case (an entry
/// written by hand).
fn named<'a, T>(map: &'a BTreeMap<String, T>, id: &str) -> Option<&'a T> {
    map.get(id).or_else(|| {
        map.iter()
            .find(|(named, _)| named.eq_ignore_ascii_case(id))
            .map(|(_, value)| value)
    })
}

impl NotifyLevels {
    /// The level of application `id`: as named, else named in another case
    /// (an entry written by hand), else everything.
    pub fn of(&self, id: &str) -> NotifyLevel {
        named(&self.apps, id).copied().unwrap_or(NotifyLevel::All)
    }

    /// Whether opening application `id` folds its notifications (#923):
    /// unless the configuration says it does not.
    pub fn folds_on_focus(&self, id: &str) -> bool {
        named(&self.seen_on_focus, id).copied().unwrap_or(true)
    }
}

/// Whether a notification's application `a` and a window's `b` are one: as
/// `raise` matches them (5.3), ignoring case and a trailing `.desktop`,
/// the whole id or its last dotted part on either side
/// (`org.mozilla.firefox` and `firefox`).
pub fn same_app(a: &str, b: &str) -> bool {
    let tidy = |id: &str| {
        let id = id.trim();
        let id = id.strip_suffix(".desktop").unwrap_or(id);
        id.to_lowercase()
    };
    let (a, b) = (tidy(a), tidy(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let last = |id: &str| id.rsplit('.').next().unwrap_or_default().to_owned();
    a == b || last(&a) == last(&b)
}

// --- The user's focus, and the agents' asks (#923) -----------------------------------

/// The window that took the user's keyboard focus: its application's id
/// (an X11 window's class) and its process; and every window's, for a
/// notification whose application has no window (it is the window's of
/// its sender or of its sender's nearest parent that has one).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppFocus {
    pub app: String,
    pub pid: u32,
    /// Each window's application id and process.
    pub windows: Vec<(String, u32)>,
}

/// Where a notification is in the bell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteState {
    /// Counted by the badge, its line with the blue dot.
    New,
    /// Seen in passing: its line stays open, not counted.
    Noticed,
    /// Its application opened, its line clicked, or an agent tidied it:
    /// folded under "N vues".
    Seen,
}

impl NoteState {
    pub const fn word(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Noticed => "noticed",
            Self::Seen => "seen",
        }
    }
}

/// An agent's pin on a notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotePin {
    pub agent: String,
    /// One line, 80 characters at most; none: no reason shown.
    #[serde(default)]
    pub reason: Option<String>,
    /// When, on the wall clock (milliseconds).
    pub at: i64,
}

/// The longest reason a pin shows.
pub const PIN_REASON: usize = 80;
/// The most notifications pinned at once.
pub const PINS: usize = 3;

/// What an agent asks of the bell, its right checked by the compositor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotesAsk {
    /// The bell's notifications, newest first.
    List {
        app: Option<String>,
        state: Option<NoteState>,
        limit: u32,
    },
    /// The journal's lines holding every word of `text`.
    Search {
        text: String,
        app: Option<String>,
        /// Milliseconds since the epoch.
        since: Option<i64>,
        until: Option<i64>,
        limit: u32,
    },
    Pin {
        id: u64,
        agent: String,
        reason: Option<String>,
    },
    Unpin {
        id: u64,
    },
    /// These, or every new and noticed one of `app`, become seen.
    Tidy {
        ids: Vec<u64>,
        app: Option<String>,
    },
}

/// A notification as it was before an agent changed it, for « Annuler ».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteBefore {
    pub id: u64,
    pub state: NoteState,
    pub pin: Option<NotePin>,
    /// How many times the user had changed it then: one they changed
    /// since stays as they left it.
    pub touched: u64,
}

/// The bell's answer to an agent's ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotesAnswer {
    /// What the agent reads, as JSON's text; for a pin or a tidying, how
    /// many changed, of which application (its name when they are all
    /// one's), and what they were before.
    Done {
        value: String,
        count: u32,
        app: Option<String>,
        before: Vec<NoteBefore>,
    },
    Refused(String),
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
            seen_on_focus: BTreeMap::new(),
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
    fn an_application_and_a_window_match_as_raise_does() {
        assert!(same_app("org.mozilla.firefox", "firefox"));
        assert!(same_app("Firefox.desktop", "org.mozilla.firefox"));
        assert!(same_app("Slack", "slack"));
        assert!(!same_app("slack", "signal"));
        assert!(!same_app("", ""));
        let levels = NotifyLevels {
            apps: BTreeMap::new(),
            seen_on_focus: [("org.example.Agenda".to_owned(), false)].into(),
        };
        assert!(!levels.folds_on_focus("org.example.agenda"));
        assert!(levels.folds_on_focus("org.example.Chat"));
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
