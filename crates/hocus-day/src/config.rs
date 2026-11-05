//! The configuration's `[day]`: the calendars' and the tasks' accounts
//! (Day.1). No secret goes in it: an account's token is kept by
//! [`crate::secrets`].
//!
//! ```toml
//! [[day.calendars]]
//! kind = "google"                 # or "fixture"
//! account = "alex@example.org"    # google: the Online Accounts' account
//! file = "day.toml"               # fixture: its file, beside config.toml
//! shown = ["Travail", "Perso"]    # by name or id; none: as the account says
//! featured = ["Travail"]          # by name or id; none: every shown calendar
//! colors = { "Perso" = "#F28FB1" }
//!
//! [[day.tasks]]
//! kind = "planka"                 # or "fixture"
//! address = "https://planka.example.org"
//! board = "https://planka.example.org/boards/1234"   # or its id
//! tasks = "TODO"                  # the tasks' column; default TODO, else the first
//! done = "DONE"                   # where a ticked task goes; default DONE, else the last
//! add = ""                        # where + adds; default the tasks' column
//! file = "day.toml"               # fixture
//! ```

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    model::{AccountId, Calendar, Colour},
    snapshot::{Account, Holds, Kind},
    source::ColumnNames,
};

/// `[day]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub calendars: Vec<CalendarEntry>,
    /// The bar's tasks are the first's; the local list's without any.
    pub tasks: Vec<TaskEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarKind {
    Google,
    Fixture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    Planka,
    Fixture,
}

/// A `[[day.calendars]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarEntry {
    pub kind: CalendarKind,
    /// Which account of the service (Google: its e-mail, as Online
    /// Accounts names it).
    #[serde(default)]
    pub account: String,
    /// A fixture's file; relative: to the configuration's folder.
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// The calendars shown, by name or id; `None`: as the account marks
    /// them.
    #[serde(default)]
    pub shown: Option<Vec<String>>,
    /// The calendars that feed what is about to start (the meeting card,
    /// the reminders, the halo, `Super+J`), by name or id; `None`: every
    /// shown calendar.
    #[serde(default)]
    pub featured: Option<Vec<String>>,
    /// Colours, by calendar name or id, over the account's own.
    #[serde(default, deserialize_with = "colours")]
    pub colors: BTreeMap<String, Colour>,
}

impl CalendarEntry {
    /// What it says over its account's calendars.
    pub fn over(&self) -> Over {
        Over {
            shown: self.shown.clone(),
            featured: self.featured.clone(),
            colours: self.colors.clone(),
        }
    }
}

/// What a `[[day.calendars]]` says over its account's calendars, each
/// named by its name (whatever its case) or its id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Over {
    /// `None`: shown as the account marks them.
    pub shown: Option<Vec<String>>,
    /// `None`: every shown calendar is featured.
    pub featured: Option<Vec<String>>,
    pub colours: BTreeMap<String, Colour>,
}

impl Over {
    /// `calendar` as the configuration has it: shown or not, featured or
    /// not ([`Calendar::featured`], the configuration's alone), its colour.
    pub fn apply(&self, calendar: &mut Calendar) {
        let names = |name: &String| {
            calendar.id.as_str() == name || calendar.name.to_lowercase() == name.to_lowercase()
        };
        if let Some(shown) = &self.shown {
            calendar.shown = shown.iter().any(names);
        }
        calendar.featured = self
            .featured
            .as_ref()
            .is_none_or(|featured| featured.iter().any(names));
        if let Some((_, colour)) = self.colours.iter().find(|(name, _)| names(name)) {
            calendar.colour = *colour;
        }
    }
}

fn colours<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Colour>, D::Error> {
    BTreeMap::<String, String>::deserialize(deserializer)?
        .into_iter()
        .map(|(name, colour)| Ok((name, colour.parse().map_err(serde::de::Error::custom)?)))
        .collect()
}

/// A `[[day.tasks]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEntry {
    pub kind: TaskKind,
    /// The server's address.
    #[serde(default)]
    pub address: String,
    /// The board: its address (`…/boards/ID`) or its id.
    #[serde(default)]
    pub board: String,
    /// The columns, by name: the tasks', the done one, where `+` adds.
    #[serde(default)]
    pub tasks: String,
    #[serde(default)]
    pub done: String,
    #[serde(default)]
    pub add: String,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

impl TaskEntry {
    pub fn columns(&self) -> ColumnNames {
        ColumnNames {
            tasks: self.tasks.clone(),
            done: self.done.clone(),
            add: self.add.clone(),
        }
    }
}

/// The configuration file's `[day]`, read from its text; the rest of the
/// file is the compositor's.
pub fn from_toml(text: &str) -> Result<Config, String> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        day: Config,
    }
    toml::from_str::<File>(text)
        .map(|f| f.day)
        .map_err(|err| err.to_string())
}

/// `[day]` of the configuration file at `path`: none when there is no
/// file; an error (to log) when it does not read.
pub fn read(path: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => from_toml(&text).map_err(|err| format!("{}: {err}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

impl Config {
    /// The bar's tasks' account the configuration sets (the first
    /// `[[day.tasks]]`), with its id as [`crate::Workers::start`] makes
    /// it: the one its token is kept under (`hocus day planka-token`).
    pub fn tasks_account(&self, base: Option<&Path>) -> Option<(Account, &TaskEntry)> {
        let mut taken = vec![local_account().id];
        for entry in &self.calendars {
            entry.account(base, &mut taken);
        }
        let entry = self.tasks.first()?;
        Some((entry.account(base, &mut taken), entry))
    }
}

/// The local list's account, always there.
pub fn local_account() -> Account {
    Account {
        id: AccountId::new("local"),
        kind: Kind::Local,
        holds: Holds::Tasks,
        name: String::new(),
    }
}

impl CalendarEntry {
    pub fn account(&self, base: Option<&Path>, taken: &mut Vec<AccountId>) -> Account {
        let (kind, name, short) = match self.kind {
            CalendarKind::Google => (Kind::Google, self.account.clone(), self.account.clone()),
            CalendarKind::Fixture => fixture(self.file.as_deref(), base),
        };
        account(kind, Holds::Calendars, name, &short, taken)
    }
}

impl TaskEntry {
    pub fn account(&self, base: Option<&Path>, taken: &mut Vec<AccountId>) -> Account {
        let (kind, name, short) = match self.kind {
            TaskKind::Planka => match crate::planka::address(&self.address, &self.board) {
                Ok((server, board)) => (
                    Kind::Planka,
                    server.to_string(),
                    format!("{}-{board}", server.host()),
                ),
                // Not usable as set (its worker says why): named as written.
                Err(_) => (
                    Kind::Planka,
                    self.address.clone(),
                    format!("{}-{}", self.address, self.board),
                ),
            },
            TaskKind::Fixture => fixture(self.file.as_deref(), base),
        };
        account(kind, Holds::Tasks, name, &short, taken)
    }
}

/// The account of Online Accounts' Google account `identity` (its
/// e-mail), named `name` for the user: `google-calendars-<identity>`.
pub fn google_account(identity: &str, name: String) -> Account {
    account(
        Kind::Google,
        Holds::Calendars,
        name,
        identity,
        &mut vec![local_account().id],
    )
}

/// A file of the configuration's, relative to its folder.
pub fn resolve(file: &Path, base: Option<&Path>) -> PathBuf {
    match base {
        Some(base) if file.is_relative() => base.join(file),
        _ => file.to_path_buf(),
    }
}

/// A fixture's kind, its file's path and its file's stem.
fn fixture(file: Option<&Path>, base: Option<&Path>) -> (Kind, String, String) {
    let stem = file
        .and_then(Path::file_stem)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = file
        .map(|f| resolve(f, base).display().to_string())
        .unwrap_or_default();
    (Kind::Fixture, path, stem)
}

/// An account named `name`: its id, its kind, what it holds and `short`,
/// made safe for a file's name, unique among `taken`.
fn account(
    kind: Kind,
    holds: Holds,
    name: String,
    short: &str,
    taken: &mut Vec<AccountId>,
) -> Account {
    let holds_word = match holds {
        Holds::Calendars => "calendars",
        Holds::Tasks => "tasks",
    };
    let mut slug: String = format!("{}-{holds_word}-{short}", kind.name())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "@._-".contains(c) {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug
        .trim_end_matches('-')
        .chars()
        .take(80)
        .collect::<String>();
    let mut id = AccountId::new(slug.clone());
    let mut n = 2;
    while taken.contains(&id) {
        id = AccountId::new(format!("{slug}-{n}"));
        n += 1;
    }
    taken.push(id.clone());
    Account {
        id,
        kind,
        holds,
        name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_reads_its_accounts_and_lets_the_rest_through() -> anyhow::Result<()> {
        let config = from_toml(
            r##"
            [keyboard]
            layouts = ["fr"]

            [[day.calendars]]
            kind = "google"
            account = "alex@example.org"
            shown = ["Travail"]
            colors = { "Travail" = "#6CC287" }

            [[day.calendars]]
            kind = "fixture"
            file = "day.toml"

            [[day.tasks]]
            kind = "planka"
            address = "https://planka.example.org"
            board = "https://planka.example.org/boards/1234"
            tasks = "À faire"
            "##,
        )
        .map_err(anyhow::Error::msg)?;
        assert_eq!(config.calendars.len(), 2);
        assert_eq!(config.calendars[0].kind, CalendarKind::Google);
        assert_eq!(
            config.calendars[0].colors.get("Travail"),
            Some(&Colour([0x6C, 0xC2, 0x87]))
        );
        assert_eq!(config.tasks[0].columns().tasks, "À faire");
        assert_eq!(from_toml(""), Ok(Config::default()));
        for bad in [
            "[[day.calendars]]\nkind = \"ics\"\n",
            "[[day.calendars]]\nkind = \"google\"\npassword = \"x\"\n",
            "[[day.calendars]]\nkind = \"fixture\"\ncolors = { a = \"blue\" }\n",
            "[day]\nnotes = []\n",
        ] {
            assert!(from_toml(bad).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn featured_calendars_are_named_and_without_featured_every_shown_one_is() {
        let calendar = |id: &str, name: &str| Calendar {
            id: crate::model::CalendarId::new(id),
            account: AccountId::new("google-1"),
            name: name.to_owned(),
            colour: Colour::DEFAULT,
            read_only: false,
            shown: true,
            featured: false,
        };
        let entry = |featured: Option<&[&str]>| CalendarEntry {
            kind: CalendarKind::Google,
            account: String::new(),
            file: None,
            shown: None,
            featured: featured.map(|f| f.iter().map(|n| (*n).to_owned()).collect()),
            colors: BTreeMap::new(),
        };
        let featured = |over: &Over| -> Vec<bool> {
            [calendar("work@x", "Travail"), calendar("lou@x", "Congés")]
                .into_iter()
                .map(|mut c| {
                    over.apply(&mut c);
                    c.featured
                })
                .collect()
        };
        // By name, whatever its case, or by id.
        assert_eq!(featured(&entry(Some(&["travail"])).over()), [true, false]);
        assert_eq!(featured(&entry(Some(&["lou@x"])).over()), [false, true]);
        assert_eq!(featured(&entry(Some(&[])).over()), [false, false]);
        // None: every one, as before.
        assert_eq!(featured(&entry(None).over()), [true, true]);
        let config = from_toml("[[day.calendars]]\nkind = \"google\"\nfeatured = [\"Travail\"]\n")
            .map(|c| c.calendars[0].featured.clone());
        assert_eq!(config, Ok(Some(vec!["Travail".to_owned()])));
    }

    #[test]
    fn accounts_have_ids_fit_for_a_files_name() {
        let mut taken = vec![local_account().id];
        let planka = TaskEntry {
            kind: TaskKind::Planka,
            address: "https://planka.example.org".to_owned(),
            board: "https://planka.example.org/boards/1234".to_owned(),
            tasks: String::new(),
            done: String::new(),
            add: String::new(),
            file: None,
        };
        let a = planka.account(None, &mut taken);
        assert_eq!(a.id.as_str(), "planka-tasks-planka.example.org-1234");
        let again = planka.account(None, &mut taken);
        assert_eq!(again.id.as_str(), format!("{}-2", a.id));
        let fixture = CalendarEntry {
            kind: CalendarKind::Fixture,
            account: String::new(),
            file: Some(PathBuf::from("my day.toml")),
            shown: None,
            featured: None,
            colors: BTreeMap::new(),
        };
        let f = fixture.account(Some(Path::new("/home/alex/.config/hocuspanda")), &mut taken);
        assert_eq!(f.id.as_str(), "fixture-calendars-my-day");
        assert_eq!(f.name, "/home/alex/.config/hocuspanda/my day.toml");
    }

    /// The commented `[day]` of the shipped configuration, uncommented as
    /// the compositor's test does, reads as no accounts.
    #[test]
    fn the_shipped_example_has_no_account() -> anyhow::Result<()> {
        let example = include_str!("../../../packaging/config.toml");
        let uncommented: String = example
            .lines()
            .filter_map(|l| l.strip_prefix("# ").filter(|l| !l.starts_with(' ')))
            .filter(|l| l.starts_with('[') || l.contains(" = "))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(uncommented.contains("[day]"), "{uncommented}");
        assert_eq!(
            from_toml(&uncommented).map_err(anyhow::Error::msg)?,
            Config::default()
        );
        Ok(())
    }
}
