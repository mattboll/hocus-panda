//! Day.1's choices written into the configuration file (#425): a Google
//! account's `[[day.calendars]]` or a Planka board's `[[day.tasks]]`,
//! edited in place with `toml_edit`, so that the file's other sections,
//! its comments and its order stay as the user wrote them. No secret goes
//! in it: a board's token is kept by [`crate::secrets`].
//!
//! The file is written whole or not at all (beside it, then renamed over
//! it), through a symbolic link to where it really is, with the mode it
//! had ([`hocus_config`], which the Settings share). Blocking: from a
//! thread of its own, never the shell's loop.

use std::path::Path;

use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, value};

use crate::model::Colour;

/// A Google account of Online Accounts and its calendars, as chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleChoice {
    /// Its e-mail, as Online Accounts names it.
    pub account: String,
    /// The calendars shown, by name.
    pub shown: Vec<String>,
    /// Those of them featured (the meeting card, the reminders, the halo,
    /// `Super+J`), by name.
    pub featured: Vec<String>,
    /// Each calendar's colour, by name.
    pub colours: Vec<(String, Colour)>,
}

/// A Planka board and its columns, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlankaChoice {
    /// The board's address, `https://host/boards/ID`.
    pub address: String,
    pub tasks: String,
    pub done: String,
    pub add: String,
}

/// What Day.1's popover chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Google(GoogleChoice),
    Planka(PlankaChoice),
}

/// `text` (a configuration file's) with `choice` in its `[day]`: a Google
/// account replaces the `[[day.calendars]]` that names it, else comes
/// after the others; a board replaces the first `[[day.tasks]]` (the only
/// one used), else is the first. Everything else is kept as it was.
pub fn apply(text: &str, choice: &Choice) -> Result<String, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|err| format!("the configuration does not read: {err}"))?;
    let day = doc
        .entry("day")
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_mut()
        .ok_or("[day] is not a table")?;
    match choice {
        Choice::Google(google) => {
            let calendars = entries(day, "calendars")?;
            let named = calendars.iter().position(|t| {
                t.get("kind").and_then(Item::as_str) == Some("google")
                    && t.get("account")
                        .and_then(Item::as_str)
                        .is_some_and(|a| a.trim().eq_ignore_ascii_case(&google.account))
            });
            match named.and_then(|i| calendars.get_mut(i)) {
                Some(entry) => fill_google(entry, google),
                None => {
                    let mut entry = Table::new();
                    fill_google(&mut entry, google);
                    calendars.push(entry);
                }
            }
        }
        Choice::Planka(planka) => {
            let tasks = entries(day, "tasks")?;
            match tasks.get_mut(0) {
                Some(entry) => fill_planka(entry, planka),
                None => {
                    let mut entry = Table::new();
                    fill_planka(&mut entry, planka);
                    tasks.push(entry);
                }
            }
        }
    }
    let written = doc.to_string();
    // What is written reads back as the shell reads it.
    crate::config::from_toml(&written)?;
    Ok(written)
}

/// `day`'s array of tables `name`, made if it is not there.
fn entries<'a>(day: &'a mut Table, name: &str) -> Result<&'a mut ArrayOfTables, String> {
    day.entry(name)
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| format!("day.{name} is not a list of [[day.{name}]]"))
}

fn fill_google(entry: &mut Table, google: &GoogleChoice) {
    entry.insert("kind", value("google"));
    entry.insert("account", value(google.account.as_str()));
    entry.remove("file");
    let shown: Array = google.shown.iter().map(String::as_str).collect();
    entry.insert("shown", value(shown));
    let featured: Array = google.featured.iter().map(String::as_str).collect();
    entry.insert("featured", value(featured));
    let mut colours = InlineTable::new();
    for (name, colour) in &google.colours {
        colours.insert(name, colour.to_string().into());
    }
    entry.insert("colors", value(colours));
}

fn fill_planka(entry: &mut Table, planka: &PlankaChoice) {
    entry.insert("kind", value("planka"));
    entry.insert("address", value(planka.address.as_str()));
    // The address holds the board.
    entry.remove("board");
    entry.remove("file");
    entry.insert("tasks", value(planka.tasks.as_str()));
    entry.insert("done", value(planka.done.as_str()));
    entry.insert("add", value(planka.add.as_str()));
}

/// `choice` written into the configuration file at `path` (none yet: one
/// made with it alone), whole or not at all ([`hocus_config::edit`]).
pub fn write(path: &Path, choice: &Choice) -> Result<String, String> {
    let read = hocus_config::read(path)?;
    let written = apply(&read.text, choice)?;
    hocus_config::replace(&read.real, written.as_bytes(), read.mode)
        .map_err(|err| format!("{}: {err}", read.real.display()))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r##"# My settings.
[keyboard]
layouts = ["fr", "fr(bepo)"]   # both

# The bar.
[panel]
form = "full"

[[day.calendars]]
kind = "fixture"
file = "day.toml"

[shortcuts]
terminal = "Super+Return"
"##;

    fn google() -> Choice {
        Choice::Google(GoogleChoice {
            account: "alex@example.org".to_owned(),
            shown: vec!["Travail".to_owned(), "Perso".to_owned()],
            featured: vec!["Travail".to_owned()],
            colours: vec![
                ("Travail".to_owned(), Colour([0x8F, 0xB3, 0xFF])),
                ("Perso".to_owned(), Colour([0xF2, 0x8F, 0xB1])),
            ],
        })
    }

    fn planka(address: &str) -> Choice {
        Choice::Planka(PlankaChoice {
            address: address.to_owned(),
            tasks: "TODO".to_owned(),
            done: "Done".to_owned(),
            add: "TODO".to_owned(),
        })
    }

    #[test]
    fn a_choice_goes_into_day_and_the_rest_stays_as_written() -> anyhow::Result<()> {
        let written = apply(FILE, &google()).map_err(anyhow::Error::msg)?;
        // Every line of the file is still there, in its order.
        let mut from = 0;
        for line in FILE.lines() {
            let at = written[from..]
                .find(line)
                .ok_or_else(|| anyhow::anyhow!("{line:?} lost:\n{written}"))?;
            from += at + line.len();
        }
        let config = crate::config::from_toml(&written).map_err(anyhow::Error::msg)?;
        assert_eq!(config.calendars.len(), 2);
        let entry = &config.calendars[1];
        assert_eq!(entry.account, "alex@example.org");
        assert_eq!(
            entry.shown.as_deref(),
            Some(&["Travail".to_owned(), "Perso".to_owned()][..])
        );
        assert_eq!(entry.featured.as_deref(), Some(&["Travail".to_owned()][..]));
        assert_eq!(entry.colors.get("Perso"), Some(&Colour([0xF2, 0x8F, 0xB1])));
        // After the fixture's entry, before [shortcuts].
        let at = written.find("\"google\"").unwrap_or(0);
        assert!(
            written.find("day.toml").unwrap_or(usize::MAX) < at,
            "{written}"
        );
        assert!(at < written.find("[shortcuts]").unwrap_or(0), "{written}");
        // The same account again replaces its entry.
        let again = apply(&written, &google()).map_err(anyhow::Error::msg)?;
        assert_eq!(again.matches("\"google\"").count(), 1, "{again}");
        Ok(())
    }

    #[test]
    fn featured_is_written_into_the_accounts_entry_its_other_lines_kept() -> anyhow::Result<()> {
        let mine = format!(
            "{FILE}\n# Work's calendars.\n[[day.calendars]]\nkind = \"google\"\n\
             account = \"alex@example.org\"\nfeatured = [\"Perso\"]   # for now\n"
        );
        let written = apply(&mine, &google()).map_err(anyhow::Error::msg)?;
        for line in FILE.lines().chain(["# Work's calendars."]) {
            assert!(written.contains(line), "{line:?} lost:\n{written}");
        }
        assert!(written.contains("featured = [\"Travail\"]"), "{written}");
        let config = crate::config::from_toml(&written).map_err(anyhow::Error::msg)?;
        assert_eq!(config.calendars.len(), 2, "{written}");
        assert_eq!(
            config.calendars[1].featured.as_deref(),
            Some(&["Travail".to_owned()][..])
        );
        Ok(())
    }

    #[test]
    fn a_board_is_the_first_tasks_entry() -> anyhow::Result<()> {
        let written = apply("", &planka("https://planka.example.org/boards/12"))
            .map_err(anyhow::Error::msg)?;
        assert!(written.contains("[[day.tasks]]"), "{written}");
        assert!(!written.contains("[day]"), "{written}");
        let config = crate::config::from_toml(&written).map_err(anyhow::Error::msg)?;
        assert_eq!(config.tasks[0].kind, crate::config::TaskKind::Planka);
        assert_eq!(config.tasks[0].columns().done, "Done");
        // Another board replaces it, its comment kept.
        let commented = format!("# Work's board.\n{written}");
        let again = apply(&commented, &planka("https://planka.example.org/boards/34"))
            .map_err(anyhow::Error::msg)?;
        let config = crate::config::from_toml(&again).map_err(anyhow::Error::msg)?;
        assert_eq!(config.tasks.len(), 1);
        assert!(config.tasks[0].address.ends_with("/34"), "{again}");
        assert!(again.starts_with("# Work's board.\n"), "{again}");
        Ok(())
    }

    #[test]
    fn a_file_that_does_not_read_is_left_alone() {
        assert!(apply("[keyboard\n", &google()).is_err());
        assert!(apply("day = 3\n", &google()).is_err());
        assert!(apply("[day]\ncalendars = 3\n", &google()).is_err());
    }

    #[test]
    fn it_is_written_through_its_link_with_its_mode() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir()?;
        let real = dir.path().join("dotfiles/config.toml");
        std::fs::create_dir_all(dir.path().join("dotfiles"))?;
        std::fs::write(&real, FILE)?;
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600))?;
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink(&real, &link)?;
        write(&link, &google()).map_err(anyhow::Error::msg)?;
        assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
        assert!(std::fs::read_to_string(&real)?.contains("alex@example.org"));
        assert_eq!(
            std::fs::metadata(&real)?.permissions().mode() & 0o777,
            0o600
        );
        // Nothing left beside it.
        assert_eq!(std::fs::read_dir(dir.path().join("dotfiles"))?.count(), 1);
        // No file yet: one made.
        let new = dir.path().join("new/config.toml");
        write(&new, &planka("https://planka.example.org/boards/1")).map_err(anyhow::Error::msg)?;
        assert!(std::fs::read_to_string(&new)?.contains("[[day.tasks]]"));
        Ok(())
    }
}
