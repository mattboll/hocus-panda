//! Changes to the configuration file: one key at a time, the rest of the
//! file as the user wrote it (comments, order, layout). A change that
//! would make the file invalid is not written.

use std::{io, path::Path};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

/// One key of one table set to a value, or taken out.
#[derive(Debug, Clone)]
pub struct Change {
    pub place: Place,
    /// Empty, with no value: the whole table goes.
    pub key: String,
    /// `None` takes the key out.
    pub value: Option<Value>,
}

/// Which table a change goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// `[section]`, or `[a.b]` for `"a.b"`.
    Section(&'static str),
    /// The `[[outputs]]` entry with this name, made when missing.
    Output(String),
    /// The `[[layouts]]` entry for exactly these screens, made when missing
    /// and named after them.
    Layout(Vec<String>),
    /// The `[[calendars]]` entry of this address and user, made when
    /// missing.
    Calendar { url: String, user: String },
    /// The `[[wallpaper.screens]]` entry of this output, made when missing.
    WallpaperScreen(String),
}

impl Change {
    pub fn new(section: &'static str, key: impl Into<String>, value: impl Into<Value>) -> Self {
        Self {
            place: Place::Section(section),
            key: key.into(),
            value: Some(value.into()),
        }
    }

    pub fn output(name: &str, key: &'static str, value: impl Into<Value>) -> Self {
        Self {
            place: Place::Output(name.to_string()),
            key: key.into(),
            value: Some(value.into()),
        }
    }

    /// A key of an `[[outputs]]` entry taken out.
    pub fn output_unset(name: &str, key: &'static str) -> Self {
        Self {
            place: Place::Output(name.to_string()),
            key: key.into(),
            value: None,
        }
    }

    pub fn layout(screens: &[String], key: &'static str, value: impl Into<Value>) -> Self {
        Self {
            place: Place::Layout(screens.to_vec()),
            key: key.into(),
            value: Some(value.into()),
        }
    }

    pub fn calendar(url: &str, user: &str, key: &'static str, value: impl Into<Value>) -> Self {
        Self {
            place: Place::Calendar {
                url: url.to_owned(),
                user: user.to_owned(),
            },
            key: key.into(),
            value: Some(value.into()),
        }
    }

    /// The calendar account goes.
    pub fn forget_calendar(url: &str, user: &str) -> Self {
        Self {
            place: Place::Calendar {
                url: url.to_owned(),
                user: user.to_owned(),
            },
            key: String::new(),
            value: None,
        }
    }

    /// A wallpaper setting: every screen's (`[wallpaper]`) with no
    /// `output`, else that screen's own; `None` takes the key out.
    pub fn wallpaper(output: Option<&str>, key: &'static str, value: Option<Value>) -> Self {
        Self {
            place: output.map_or(Place::Section("wallpaper"), |o| {
                Place::WallpaperScreen(o.to_owned())
            }),
            key: key.into(),
            value,
        }
    }

    /// The screen's own wallpaper goes: it shows every screen's again.
    pub fn forget_wallpaper(output: &str) -> Self {
        Self {
            place: Place::WallpaperScreen(output.to_owned()),
            key: String::new(),
            value: None,
        }
    }

    /// The whole `[section]` goes: back to what it was before any.
    pub fn forget(section: &'static str) -> Self {
        Self {
            place: Place::Section(section),
            key: String::new(),
            value: None,
        }
    }
}

/// The entry of `array` whose `key` holds what `matches` accepts, or a new
/// one with `make` set.
fn entry<'a>(
    doc: &'a mut Table,
    array: &str,
    matches: impl Fn(&Item) -> bool,
    make: impl FnOnce(&mut Table),
) -> Result<&'a mut Table, EditError> {
    let item = doc
        .entry(array)
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
    let Some(tables) = item.as_array_of_tables_mut() else {
        return Err(EditError::Invalid(format!(
            "{array} is not a list of tables"
        )));
    };
    let found = tables.iter().position(|t| matches(&Item::Table(t.clone())));
    let index = match found {
        Some(i) => i,
        None => {
            let mut table = Table::new();
            make(&mut table);
            tables.push(table);
            tables.len() - 1
        }
    };
    tables
        .get_mut(index)
        .ok_or_else(|| EditError::Invalid(format!("no entry in {array}")))
}

/// The screens of a layout, in order, as written.
fn sorted(screens: &[String]) -> Vec<String> {
    let mut s = screens.to_vec();
    s.sort();
    s.dedup();
    s
}

#[derive(Debug)]
pub enum EditError {
    Io(io::Error),
    /// The file is not TOML: it is left alone.
    Unreadable(String),
    /// The change would not be accepted by the compositor.
    Invalid(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Unreadable(err) => write!(f, "the configuration file is not valid TOML: {err}"),
            Self::Invalid(err) => write!(f, "the change was refused: {err}"),
        }
    }
}

impl std::error::Error for EditError {}

/// `text` with `change` made.
pub fn apply(text: &str, change: &Change) -> Result<String, EditError> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| EditError::Unreadable(e.to_string()))?;
    if let (Place::Section(section), "", None) = (&change.place, change.key.as_str(), &change.value)
    {
        doc.remove(section);
        let out = doc.to_string();
        hocus_core::Config::parse(&out).map_err(|e| EditError::Invalid(e.to_string()))?;
        return Ok(out);
    }
    let is_calendar = |t: &Table, url: &str, user: &str| {
        t.get("url").and_then(Item::as_str) == Some(url)
            && t.get("user").and_then(Item::as_str).unwrap_or("") == user
    };
    if let (Place::Calendar { url, user }, "", None) =
        (&change.place, change.key.as_str(), &change.value)
    {
        if let Some(tables) = doc
            .get_mut("calendars")
            .and_then(Item::as_array_of_tables_mut)
        {
            tables.retain(|t| !is_calendar(t, url, user));
            if tables.is_empty() {
                doc.remove("calendars");
            }
        }
        let out = doc.to_string();
        hocus_core::Config::parse(&out).map_err(|e| EditError::Invalid(e.to_string()))?;
        return Ok(out);
    }
    let is_screen =
        |t: &Table, output: &str| t.get("output").and_then(Item::as_str) == Some(output);
    if let (Place::WallpaperScreen(output), "", None) =
        (&change.place, change.key.as_str(), &change.value)
    {
        if let Some(wallpaper) = doc.get_mut("wallpaper").and_then(Item::as_table_mut) {
            if let Some(tables) = wallpaper
                .get_mut("screens")
                .and_then(Item::as_array_of_tables_mut)
            {
                tables.retain(|t| !is_screen(t, output));
                if tables.is_empty() {
                    wallpaper.remove("screens");
                }
            }
            if wallpaper.is_empty() {
                doc.remove("wallpaper");
            }
        }
        let out = doc.to_string();
        hocus_core::Config::parse(&out).map_err(|e| EditError::Invalid(e.to_string()))?;
        return Ok(out);
    }
    let table: &mut dyn toml_edit::TableLike = match &change.place {
        Place::Section(section) => {
            let item = doc
                .entry(section)
                .or_insert_with(|| Item::Table(Table::new()));
            item.as_table_like_mut()
                .ok_or_else(|| EditError::Invalid(format!("{section} is not a section")))?
        }
        Place::Output(name) => entry(
            doc.as_table_mut(),
            "outputs",
            |t| t.get("name").and_then(Item::as_str) == Some(name.as_str()),
            |t| {
                t.insert("name", toml_edit::value(name.as_str()));
            },
        )?,
        Place::Layout(screens) => {
            let screens = sorted(screens);
            entry(
                doc.as_table_mut(),
                "layouts",
                |t| {
                    let listed: Option<Vec<String>> = t.get("outputs").and_then(|o| {
                        o.as_array()?
                            .iter()
                            .map(|v| v.as_str().map(str::to_string))
                            .collect()
                    });
                    listed.is_some_and(|l| sorted(&l) == screens)
                },
                |t| {
                    t.insert("name", toml_edit::value(screens.join(" + ")));
                    let list: toml_edit::Array = screens.iter().map(String::as_str).collect();
                    t.insert("outputs", toml_edit::value(list));
                },
            )?
        }
        Place::Calendar { url, user } => entry(
            doc.as_table_mut(),
            "calendars",
            |t| t.as_table().is_some_and(|t| is_calendar(t, url, user)),
            |t| {
                t.insert("name", toml_edit::value(url.as_str()));
                t.insert("url", toml_edit::value(url.as_str()));
                if !user.is_empty() {
                    t.insert("user", toml_edit::value(user.as_str()));
                }
            },
        )?,
        Place::WallpaperScreen(output) => {
            let wallpaper = doc.entry("wallpaper").or_insert_with(|| {
                // `[[wallpaper.screens]]` alone needs no `[wallpaper]` above.
                let mut table = Table::new();
                table.set_implicit(true);
                Item::Table(table)
            });
            let wallpaper = wallpaper
                .as_table_mut()
                .ok_or_else(|| EditError::Invalid("wallpaper is not a section".into()))?;
            entry(
                wallpaper,
                "screens",
                |t| t.as_table().is_some_and(|t| is_screen(t, output)),
                |t| {
                    t.insert("output", toml_edit::value(output.as_str()));
                },
            )?
        }
    };
    match (
        table.get_mut(&change.key).and_then(Item::as_value_mut),
        &change.value,
    ) {
        // Keeps the comment after the value.
        (Some(old), Some(value)) => {
            let decor = old.decor().clone();
            *old = value.clone();
            *old.decor_mut() = decor;
        }
        (None, Some(value)) => {
            table.insert(&change.key, Item::Value(value.clone()));
        }
        (_, None) => {
            table.remove(&change.key);
        }
    }
    let out = doc.to_string();
    hocus_core::Config::parse(&out).map_err(|e| EditError::Invalid(e.to_string()))?;
    Ok(out)
}

/// Makes `change` in the file at `path`, created if missing; the new file
/// replaces the old one at once, never half written.
pub fn write(path: &Path, change: &Change) -> Result<(), EditError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(EditError::Io(err)),
    };
    let out = apply(&text, change)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(EditError::Io)?;
    }
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, out).map_err(EditError::Io)?;
    std::fs::rename(&tmp, path).map_err(EditError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendars_are_added_and_taken_out() {
        let url = "https://dav.example.org/alex/";
        let out = apply("# mine\n", &Change::calendar(url, "alex", "name", "Work")).expect("added");
        let link = "webcal://example.org/holidays.ics";
        let out = apply(&out, &Change::calendar(link, "", "name", "Holidays")).expect("added");
        let out = apply(&out, &Change::calendar(link, "", "kind", "subscription")).expect("set");
        let config = hocus_core::Config::parse(&out).expect("valid");
        assert_eq!(config.calendars.len(), 2, "{out}");
        assert_eq!(config.calendars[0].name, "Work");
        assert_eq!(config.calendars[1].kind, "subscription");
        let out = apply(&out, &Change::forget_calendar(url, "alex")).expect("taken out");
        let config = hocus_core::Config::parse(&out).expect("valid");
        assert_eq!(config.calendars.len(), 1, "{out}");
        assert!(out.contains("# mine"), "the rest of the file stays: {out}");
        // In the clear to another machine: refused.
        let plain = Change::calendar("http://dav.example.org/", "alex", "name", "Work");
        assert!(apply(&out, &plain).is_err());
    }

    #[test]
    fn shortcuts_are_written_quoted_taken_out_and_forgotten() {
        let text = "[shortcuts]\n\"Super+M\" = \"toggle-maximized\"\n";
        let out = apply(text, &Change::new("shortcuts", "Super+Up", "none")).expect("applied");
        assert!(out.contains("\"Super+Up\" = \"none\""), "{out}");
        let gone = Change {
            value: None,
            ..Change::new("shortcuts", "Super+M", "")
        };
        let out = apply(&out, &gone).expect("applied");
        assert!(!out.contains("Super+M"), "{out}");
        let out = apply(&out, &Change::forget("shortcuts")).expect("applied");
        assert!(!out.contains("shortcuts"), "{out}");
    }

    #[test]
    fn a_change_keeps_the_rest_of_the_file() {
        let text = "# Mine.\n[appearance]\ntheme = \"light\"  # for the day\naccent = \"blue\"\n\n[windows.gaps]\ninner = 8\n";
        let out = apply(text, &Change::new("appearance", "accent", "amber")).expect("applied");
        assert_eq!(
            out,
            "# Mine.\n[appearance]\ntheme = \"light\"  # for the day\naccent = \"amber\"\n\n[windows.gaps]\ninner = 8\n"
        );
        let out = apply(&out, &Change::new("appearance", "theme", "dark")).expect("applied");
        assert!(out.contains("theme = \"dark\"  # for the day"), "{out}");
    }

    #[test]
    fn screens_and_layouts_are_entries_found_or_made() {
        let text = "# Mine.\n[[outputs]]\nname = \"DP-1\"\nscale = 1.0  # sharp\n";
        let out = apply(text, &Change::output("DP-1", "scale", 1.5)).expect("applied");
        assert!(
            out.contains("name = \"DP-1\"\nscale = 1.5  # sharp"),
            "{out}"
        );
        let out = apply(&out, &Change::output("eDP-1", "primary", true)).expect("applied");
        assert!(
            out.contains("[[outputs]]\nname = \"eDP-1\"\nprimary = true"),
            "{out}"
        );

        let screens = vec!["eDP-1".to_string(), "DP-1".to_string()];
        let mut at = toml_edit::InlineTable::new();
        at.insert("DP-1", toml_edit::Value::from_iter([0i64, 0]));
        at.insert("eDP-1", toml_edit::Value::from_iter([0i64, 1440]));
        let out = apply(&out, &Change::layout(&screens, "positions", at)).expect("applied");
        assert!(out.contains("name = \"DP-1 + eDP-1\""), "{out}");
        // The same screens in another order: the same entry.
        let renamed = Change::layout(&["DP-1".into(), "eDP-1".into()], "name", "Home");
        let out = apply(&out, &renamed).expect("applied");
        assert_eq!(out.matches("[[layouts]]").count(), 1, "{out}");
        let config = hocus_core::Config::parse(&out).expect("valid");
        let layout = config.layout_for(&["eDP-1", "DP-1"]).expect("found");
        assert_eq!(layout.name, "Home");
        assert_eq!(layout.positions.get("eDP-1"), Some(&[0, 1440]));
    }

    #[test]
    fn a_screen_has_its_own_wallpaper_then_the_others_again() {
        let text = "[wallpaper]\nkind = \"glow\"\n";
        let mine = Change::wallpaper(Some("DP-1"), "kind", Some("colour".into()));
        let out = apply(text, &mine).expect("applied");
        let colour = Change::wallpaper(Some("DP-1"), "colour", Some("#203040".into()));
        let out = apply(&out, &colour).expect("applied");
        assert_eq!(out.matches("[[wallpaper.screens]]").count(), 1, "{out}");
        let config = hocus_core::Config::parse(&out).expect("valid");
        let spec = config.wallpaper.for_output("DP-1");
        assert_eq!(
            (spec.kind.as_str(), spec.colour.as_deref()),
            ("colour", Some("#203040"))
        );
        assert_eq!(config.wallpaper.for_output("eDP-1").kind, "glow");
        let out = apply(&out, &Change::forget_wallpaper("DP-1")).expect("applied");
        assert_eq!(out, text);
        // With no [wallpaper] at all, none is written above the screen's.
        let out = apply("", &mine).expect("applied");
        assert_eq!(
            out,
            "[[wallpaper.screens]]\noutput = \"DP-1\"\nkind = \"colour\"\n"
        );
        let out = apply(&out, &Change::forget_wallpaper("DP-1")).expect("applied");
        assert_eq!(out, "");
        // Every screen's colour taken out.
        let out = apply(
            "[wallpaper]\ncolour = \"#000000\"\n",
            &Change::wallpaper(None, "colour", None),
        )
        .expect("applied");
        assert_eq!(out, "[wallpaper]\n");
    }

    #[test]
    fn a_missing_section_or_file_is_made() {
        let out = apply("", &Change::new("appearance", "high_contrast", true)).expect("applied");
        assert_eq!(out, "[appearance]\nhigh_contrast = true\n");
        let dir = tempfile::tempdir().expect("applied");
        let path = dir.path().join("hocuspanda/config.toml");
        write(&path, &Change::new("appearance", "text_size", 120)).expect("applied");
        assert_eq!(
            std::fs::read_to_string(&path).expect("written"),
            "[appearance]\ntext_size = 120\n"
        );
    }

    #[test]
    fn an_invalid_change_is_not_made() {
        let err = apply("", &Change::new("appearance", "text_size", 400)).expect_err("refused");
        assert!(matches!(err, EditError::Invalid(_)), "{err}");
        let err =
            apply("not = [toml", &Change::new("appearance", "theme", "dark")).expect_err("refused");
        assert!(matches!(err, EditError::Unreadable(_)), "{err}");
    }
}
