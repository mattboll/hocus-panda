//! Settings › Notifications (#899, docs/design/settings.md
//! "Notifications"): do not disturb, the bell's own (the shell keeps it;
//! the compositor's line says it and sets it), and each application that
//! sent notifications, the busiest this week first, with its level
//! (`[notifications.apps]`) and whether opening it folds its notifications
//! (`[notifications.seen-on-focus]`, #923); the journal of the last 30
//! days, which « Ouvrir » opens.
//!
//! The applications are the shell's count
//! (`$XDG_STATE_HOME/hocuspanda/notification-senders`), read as the page
//! opens, off the window's thread; those the file names but that sent
//! nothing lately follow, so that a level set stays in reach.

use std::{collections::BTreeMap, path::PathBuf};

use hocus_link::{NotifyLevel, NotifySenders};
use hocus_words::settings::model as model_words;

use crate::{
    applications::Picture,
    model::{Outside, SectionId},
    store::KeyEdit,
};

/// Do not disturb's row.
pub const DND: &str = "notifications.dnd";
/// The applications' check box, in each one's menu (#923).
pub const SEEN_ON_OPEN: &str = "notifications.seen-on-open";
/// The journal's row (#923).
pub const JOURNAL: &str = "notifications.journal";

/// The settings of the page that live outside the file: do not disturb,
/// the applications' check box, the journal.
pub fn outside() -> &'static [Outside] {
    use hocus_words::settings::notifications as words;
    const OUTSIDE: [Outside; 3] = [
        Outside {
            id: DND,
            section: SectionId::Notifications,
            group: model_words::NOTIFICATIONS,
            name: words::DND,
            keywords: (
                &["ne pas déranger", "silence", "cloche", "bulles", "calme"],
                &["do not disturb", "quiet", "bell", "bubbles", "silence"],
            ),
        },
        Outside {
            id: SEEN_ON_OPEN,
            section: SectionId::Notifications,
            group: model_words::NOTIFICATIONS,
            name: words::SEEN_ON_OPEN,
            keywords: (
                &["vues", "ouvrir", "ranger", "cloche", "lues"],
                &["seen", "open", "tidy", "bell", "read"],
            ),
        },
        Outside {
            id: JOURNAL,
            section: SectionId::Notifications,
            group: model_words::NOTIFICATIONS,
            name: words::JOURNAL,
            keywords: (
                &[
                    "journal des notifications",
                    "historique",
                    "30 jours",
                    "retrouver",
                ],
                &["notifications journal", "history", "30 days", "find"],
            ),
        },
    ];
    &OUTSIDE
}

/// The notifications' journal, the shell's
/// (`$XDG_STATE_HOME/hocuspanda/notifications.log`).
pub fn journal_file() -> Option<PathBuf> {
    hocus_config::state_dir().map(|dir| dir.join("notifications.log"))
}

/// `file` as its row says it: the home folder said `~`.
pub fn journal_line(file: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|home| file.strip_prefix(home).ok()) {
        Some(under) => format!("~/{}", under.display()),
        None => file.display().to_string(),
    }
}

/// Whether opening application `id` folds its notifications, as the file
/// says it (`named`): unless it says it does not.
pub fn seen_on_open(named: &BTreeMap<String, bool>, id: &str) -> bool {
    named
        .get(id)
        .or_else(|| {
            named
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(id))
                .map(|(_, v)| v)
        })
        .copied()
        .unwrap_or(true)
}

/// What writes whether opening application `id` folds (`on`): its key
/// `= false`, or taken out for `true` (what applies to those not named).
pub fn seen_on_open_keys(named: &BTreeMap<String, bool>, id: &str, on: bool) -> Vec<KeyEdit> {
    let key = named
        .keys()
        .find(|k| k.eq_ignore_ascii_case(id))
        .cloned()
        .unwrap_or_else(|| id.to_owned());
    vec![KeyEdit {
        key: vec!["notifications".to_owned(), "seen-on-focus".to_owned(), key],
        value: (!on).then(|| "false".to_owned()),
    }]
}

/// An application's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRow {
    /// What it goes by: its desktop entry, else its name.
    pub id: String,
    pub name: String,
    /// Its icon: a name of the icon themes, or a path.
    pub icon: Option<String>,
    /// How many it sent this week.
    pub week: u32,
}

/// The senders' file.
pub fn senders_file() -> Option<PathBuf> {
    hocus_config::state_dir().map(|dir| dir.join(hocus_link::SENDERS_FILE))
}

/// The senders, as the shell last wrote them (none without the file).
/// Blocking: off the window's thread.
pub fn read_senders() -> NotifySenders {
    senders_file()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| NotifySenders::decode(&text))
        .unwrap_or_default()
}

/// Today's number: days since 1970-01-01 on the system's calendar.
pub fn today() -> i64 {
    jiff::Zoned::now()
        .date()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .map_or(0, |z| z.timestamp().as_second().div_euclid(86_400))
}

/// The rows: the senders, the busiest this week first; then the
/// applications `named` in the file that sent nothing lately, by name.
pub fn rows(senders: &NotifySenders, named: &BTreeMap<String, String>, today: i64) -> Vec<AppRow> {
    let mut rows: Vec<AppRow> = senders
        .busiest(today)
        .into_iter()
        .map(|s| AppRow {
            id: s.id.clone(),
            name: if s.name.trim().is_empty() {
                s.id.clone()
            } else {
                s.name.clone()
            },
            icon: s.icon.clone(),
            week: s.week(today),
        })
        .collect();
    let mut quiet: Vec<AppRow> = named
        .keys()
        .filter(|id| !rows.iter().any(|r| r.id.eq_ignore_ascii_case(id)))
        .map(|id| AppRow {
            id: id.clone(),
            name: id.clone(),
            icon: None,
            week: 0,
        })
        .collect();
    quiet.sort_by_key(|r| r.name.to_lowercase());
    rows.extend(quiet);
    rows
}

/// Application `id`'s level as the file says it (`named`), else everything.
pub fn level_of(named: &BTreeMap<String, String>, id: &str) -> NotifyLevel {
    named
        .get(id)
        .or_else(|| {
            named
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(id))
                .map(|(_, v)| v)
        })
        .and_then(|word| NotifyLevel::from_word(word))
        .unwrap_or_default()
}

/// The file's key for application `id`, as `named` holds it (another case
/// written by hand), else `id`.
fn key_of(named: &BTreeMap<String, String>, id: &str) -> String {
    named
        .keys()
        .find(|k| k.eq_ignore_ascii_case(id))
        .cloned()
        .unwrap_or_else(|| id.to_owned())
}

/// What writes `level` for application `id`: its key, or none for "all"
/// (what applies to those not named).
pub fn level_keys(named: &BTreeMap<String, String>, id: &str, level: NotifyLevel) -> Vec<KeyEdit> {
    vec![KeyEdit {
        key: vec![
            "notifications".to_owned(),
            "apps".to_owned(),
            key_of(named, id),
        ],
        value: (level != NotifyLevel::All)
            .then(|| toml_edit::Value::from(level.word()).to_string()),
    }]
}

/// The icons of `rows`, by what they are known by. Blocking: off the
/// window's thread.
pub fn icons(rows: &[AppRow]) -> Vec<(String, Picture)> {
    let mut themes = hocus_xdg::icons::Themes::load(&hocus_xdg::desktop::data_dirs());
    let mut out = Vec::new();
    for name in rows.iter().filter_map(|r| r.icon.as_deref()) {
        if out.iter().any(|(n, _)| n == name) {
            continue;
        }
        let Some(file) = themes.find(name, 32, 2) else {
            continue;
        };
        if let Some(picture) = crate::applications::picture(&file) {
            out.push((name.to_owned(), picture));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_busiest_first_then_those_the_file_names() {
        let today = 20_000;
        let mut senders = NotifySenders::default();
        for _ in 0..3 {
            senders.count("slack", "Slack", None, today);
        }
        senders.count("thunderbird", "Thunderbird", None, today - 1);
        senders.count("org.mozilla.firefox", "Firefox", None, today - 10);
        let named: BTreeMap<String, String> = [
            ("Slack".to_owned(), "silent".to_owned()),
            ("Signal".to_owned(), "none".to_owned()),
        ]
        .into();
        let rows = rows(&senders, &named, today);
        let said: Vec<(&str, u32)> = rows.iter().map(|r| (r.id.as_str(), r.week)).collect();
        assert_eq!(
            said,
            [
                ("slack", 3),
                ("thunderbird", 1),
                ("org.mozilla.firefox", 0),
                ("Signal", 0)
            ]
        );
        assert_eq!(level_of(&named, "signal"), NotifyLevel::None);
        assert_eq!(level_of(&named, "thunderbird"), NotifyLevel::All);
        // The file's own key, in its case; "all" takes it out.
        let keys = level_keys(&named, "slack", NotifyLevel::History);
        assert_eq!(keys[0].key, ["notifications", "apps", "Slack"]);
        assert_eq!(keys[0].value.as_deref(), Some("\"history\""));
        assert_eq!(level_keys(&named, "slack", NotifyLevel::All)[0].value, None);
    }

    #[test]
    fn seen_on_open_is_written_false_its_own_key_kept_and_the_journal_said_from_home() {
        let named: BTreeMap<String, bool> = [("org.example.Agenda".to_owned(), false)].into();
        assert!(!seen_on_open(&named, "org.example.agenda"));
        assert!(seen_on_open(&named, "org.example.Chat"));
        let keys = seen_on_open_keys(&named, "org.example.agenda", true);
        assert_eq!(
            keys[0].key,
            ["notifications", "seen-on-focus", "org.example.Agenda"]
        );
        assert_eq!(keys[0].value, None);
        let keys = seen_on_open_keys(&named, "Chat", false);
        assert_eq!(keys[0].value.as_deref(), Some("false"));
        let home = std::path::Path::new("/home/alex");
        let file = home.join(".local/state/hocuspanda/notifications.log");
        assert_eq!(
            journal_line(&file, Some(home)),
            "~/.local/state/hocuspanda/notifications.log"
        );
        assert_eq!(
            journal_line(&file, Some(std::path::Path::new("/srv"))),
            "/home/alex/.local/state/hocuspanda/notifications.log"
        );
    }
}
