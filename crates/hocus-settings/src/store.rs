//! What the configuration file says of each setting, and a setting changed
//! in it: the file is edited as TOML that keeps its comments, order, blank
//! lines and unknown keys (`toml_edit`), only the changed line written.
//!
//! Pure functions on the file's text; [`crate::worker`] reads and writes
//! the file itself, off the window's thread.

use std::collections::BTreeMap;

use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::model::{Codec, Place, Setting, Source, Value, all_settings};

/// One value of a setting: the setting's only one, or its value for a
/// power source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slot {
    pub setting: &'static str,
    pub source: Option<Source>,
}

impl Slot {
    /// `setting`'s value shown with `source` chosen (ignored when it has
    /// one value).
    pub fn of(setting: &Setting, source: Source) -> Self {
        Self {
            setting: setting.id,
            source: setting.places.per_source().then_some(source),
        }
    }

    pub fn setting(self) -> Option<&'static Setting> {
        crate::model::setting(self.setting)
    }

    pub fn place(self) -> Option<&'static Place> {
        let setting = self.setting()?;
        Some(setting.places.of(self.source.unwrap_or(Source::Ac)))
    }

    /// `power.suspend-after@battery`, for the log and the state's file.
    pub fn word(self) -> String {
        match self.source {
            Some(source) => format!("{}@{}", self.setting, source.word()),
            None => self.setting.to_owned(),
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        let (id, source) = match word.split_once('@') {
            Some((id, "battery")) => (id, Some(Source::Battery)),
            Some((id, "ac")) => (id, Some(Source::Ac)),
            Some(_) => return None,
            None => (word, None),
        };
        let setting = crate::model::setting(id)?;
        (setting.places.per_source() == source.is_some()).then_some(Self {
            setting: setting.id,
            source,
        })
    }
}

/// What the file says of a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// Nothing: the default applies.
    Absent,
    Value(Value),
    /// Something not understood (`"3 heures"`, as written): the default
    /// applies.
    NotUnderstood(String),
}

/// The file's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    /// It reads (or is not there yet).
    Reads,
    /// It does not read: its line (from 1) and the parser's message.
    Broken {
        line: Option<usize>,
        message: String,
    },
    /// The compositor names no file: nothing can be written.
    NoFile,
}

/// The file as last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub state: FileState,
    pub readings: BTreeMap<Slot, Reading>,
    /// `[appearance] reduced-motion`: motions only fade.
    pub reduced_motion: bool,
    /// `[appearance] theme` and `accent`: the window draws in them (a
    /// word not understood: the default's).
    pub theme: hocus_link::theme::Choice,
    /// `[locale] language`: the window speaks it (`None`: the system's).
    pub language: Option<crate::lang::Lang>,
    /// `[appearance] text-scale`, in hundredths: the window is drawn this
    /// much larger.
    pub text_scale: u32,
    /// The screens the file places (`[outputs.NAME] position`), by name.
    pub screens_placed: Vec<String>,
    /// `[panel] output`, the primary screen named.
    pub primary_output: Option<String>,
    /// `[wallpaper]` and the screens' wallpapers (6.3).
    pub wallpaper: Box<crate::wallpaper::File>,
    /// The bar's keys the page writes itself (6.4).
    pub bar: Box<crate::bar::File>,
    /// The rules per application (6.14).
    pub rules: Box<crate::desktops::Rules>,
}

impl Snapshot {
    /// What the file says of `slot` (nothing when it does not read).
    pub fn reading(&self, slot: Slot) -> &Reading {
        self.readings.get(&slot).unwrap_or(&Reading::Absent)
    }

    /// The value that applies to `slot`: the file's, else the default.
    pub fn value(&self, slot: Slot) -> Option<Value> {
        let place = slot.place()?;
        Some(match self.reading(slot) {
            Reading::Value(value) => value.clone(),
            _ => place.default.clone(),
        })
    }
}

/// Every slot of every setting.
pub fn slots() -> Vec<Slot> {
    let mut slots = Vec::new();
    for setting in all_settings() {
        // A page's own (the screens'): not a slot of the file.
        if matches!(setting.places, crate::model::Places::Page(_)) {
            continue;
        }
        if setting.places.per_source() {
            for source in [Source::Battery, Source::Ac] {
                slots.push(Slot::of(setting, source));
            }
        } else {
            slots.push(Slot::of(setting, Source::Ac));
        }
    }
    slots
}

/// What `text` says of every setting.
pub fn read(text: &str) -> Snapshot {
    match hocus_config::parse(text) {
        Ok(doc) => Snapshot {
            state: FileState::Reads,
            readings: slots()
                .into_iter()
                .filter_map(|slot| Some((slot, reading_or_old(&doc, slot.place()?))))
                .collect(),
            reduced_motion: doc
                .get("appearance")
                .and_then(|a| a.get("reduced-motion"))
                .and_then(Item::as_bool)
                .unwrap_or(false),
            theme: theme_of(&doc),
            language: match doc
                .get("locale")
                .and_then(|l| l.get("language"))
                .and_then(Item::as_str)
            {
                Some("fr") => Some(crate::lang::Lang::French),
                Some("en") => Some(crate::lang::Lang::English),
                _ => None,
            },
            text_scale: doc
                .get("appearance")
                .and_then(|a| a.get("text-scale"))
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
                .filter(|s| (0.8..=1.5).contains(s))
                .map_or(100, |s| (s * 100.0).round() as u32),
            screens_placed: doc
                .get("outputs")
                .and_then(Item::as_table_like)
                .map(|outputs| {
                    outputs
                        .iter()
                        .filter(|(_, o)| o.get("position").is_some())
                        .map(|(name, _)| name.to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            primary_output: doc
                .get("panel")
                .and_then(|p| p.get("output"))
                .and_then(Item::as_str)
                .map(ToOwned::to_owned),
            wallpaper: Box::new(crate::wallpaper::File::of(&doc)),
            bar: Box::new(crate::bar::File::of(&doc)),
            rules: Box::new(crate::desktops::Rules(crate::desktops::rules_of(&doc))),
        },
        Err(err) => Snapshot {
            state: FileState::Broken {
                line: hocus_config::error_line(text, &err),
                message: err.message().trim().to_owned(),
            },
            readings: BTreeMap::new(),
            reduced_motion: false,
            theme: hocus_link::theme::Choice::default(),
            language: None,
            text_scale: 100,
            screens_placed: Vec::new(),
            primary_output: None,
            wallpaper: Box::default(),
            bar: Box::default(),
            rules: Box::default(),
        },
    }
}

/// The theme `doc` chooses.
fn theme_of(doc: &DocumentMut) -> hocus_link::theme::Choice {
    let word = |key: &str| {
        doc.get("appearance")
            .and_then(|a| a.get(key))
            .and_then(Item::as_str)
    };
    hocus_link::theme::Choice {
        scheme: word("theme")
            .and_then(hocus_link::theme::Scheme::of)
            .unwrap_or_default(),
        accent: word("accent")
            .and_then(hocus_link::theme::Accent::of)
            .unwrap_or_default(),
        // The wallpaper's accent is the window's to find (6.3).
        wallpaper: hocus_link::theme::AccentColours::default(),
    }
}

/// What `doc` says at `place`, or, where it says nothing, what a file
/// 6.1's Settings wrote says (6.10): `[power.battery] screen-off` is the
/// battery's `idle`; `[power.*] lock-after` is `lock-on-idle`, off only
/// when every source it names says `"never"`. The compositor reads them
/// the same way; the window never writes them.
fn reading_or_old(doc: &DocumentMut, place: &Place) -> Reading {
    let read = reading(doc, place);
    if read != Reading::Absent {
        return read;
    }
    match place.key {
        ["power", "battery", "idle"] => reading(
            doc,
            &Place {
                key: &["power", "battery", "screen-off"],
                ..place.clone()
            },
        ),
        ["power", "lock-on-idle"] => {
            let said: Vec<Reading> = ["ac", "battery"]
                .iter()
                .map(|source| {
                    let key: &'static [&'static str] = if *source == "ac" {
                        &["power", "ac", "lock-after"]
                    } else {
                        &["power", "battery", "lock-after"]
                    };
                    reading(
                        doc,
                        &Place {
                            key,
                            codec: Codec::Delay,
                            default: Value::Seconds(Some(0)),
                        },
                    )
                })
                .filter(|r| *r != Reading::Absent)
                .collect();
            if said.is_empty() {
                return Reading::Absent;
            }
            let never = said
                .iter()
                .all(|r| *r == Reading::Value(Value::Seconds(None)));
            Reading::Value(Value::Bool(!never))
        }
        _ => Reading::Absent,
    }
}

/// What `doc` says at `place`.
fn reading(doc: &DocumentMut, place: &Place) -> Reading {
    let Some((name, tables)) = place.key.split_last() else {
        return Reading::Absent;
    };
    let mut table: &dyn TableLike = doc.as_table();
    for part in tables {
        match table.get(part) {
            None => return Reading::Absent,
            Some(item) => match item.as_table_like() {
                Some(inner) => table = inner,
                None => return Reading::NotUnderstood(raw(item)),
            },
        }
    }
    match table.get(name) {
        None => Reading::Absent,
        Some(item) => match item.as_value().and_then(|v| place.codec.read(v)) {
            Some(value) => Reading::Value(value),
            None => Reading::NotUnderstood(raw(item)),
        },
    }
}

/// An item as the user wrote it, a text without its quotes.
fn raw(item: &Item) -> String {
    match item.as_str() {
        Some(text) => text.to_owned(),
        None => item.to_string().trim().to_owned(),
    }
}

/// What a change writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raw {
    Set(Value),
    /// A value as the file wrote it, written back (undoing a change over
    /// one that was not understood).
    Toml(String),
    /// The key taken out: the file did not say it.
    Remove,
}

impl Raw {
    /// What the file says at `slot` now, as a change would write it back.
    pub fn now(doc: &DocumentMut, slot: Slot) -> Option<Self> {
        let place = slot.place()?;
        let (name, tables) = place.key.split_last()?;
        let mut table: &dyn TableLike = doc.as_table();
        for part in tables {
            match table.get(part).and_then(Item::as_table_like) {
                Some(inner) => table = inner,
                None => return Some(Self::Remove),
            }
        }
        Some(match table.get(name).and_then(Item::as_value) {
            None => Self::Remove,
            Some(value) => match place.codec.read(value) {
                Some(value) => Self::Set(value),
                None => Self::Toml(value.to_string().trim().to_owned()),
            },
        })
    }
}

/// `doc` with `raw` at `slot`: only that key changes, its comment kept; the
/// tables it needs made (`[power.battery]`, not `[power]` as well).
pub fn write(doc: &mut DocumentMut, slot: Slot, raw: &Raw) -> Result<(), String> {
    let place = slot.place().ok_or("an unknown setting")?;
    let (name, tables) = place.key.split_last().ok_or("an empty key")?;
    let new = match raw {
        // One option among the line's others (6.8).
        Raw::Set(Value::Word(choice)) if matches!(place.codec, Codec::XkbOption(_)) => {
            let Codec::XkbOption(family) = place.codec else {
                return Err("an XKB option".to_owned());
            };
            let mut table: &dyn TableLike = doc.as_table();
            for part in tables {
                match table.get(part).and_then(Item::as_table_like) {
                    Some(inner) => table = inner,
                    None => break,
                }
            }
            let old = table.get(name).and_then(Item::as_str).unwrap_or_default();
            Some(crate::model::xkb_options(old, family, choice).into())
        }
        Raw::Set(value) => Some(
            place
                .codec
                .write(value)
                .ok_or_else(|| format!("{value:?} cannot go in {}", place.shown()))?,
        ),
        Raw::Toml(text) => Some(
            text.parse::<toml_edit::Value>()
                .map_err(|err| format!("{text}: {err}"))?,
        ),
        Raw::Remove => None,
    };
    let mut table: &mut dyn TableLike = doc.as_table_mut();
    for (i, part) in tables.iter().enumerate() {
        if new.is_none() && table.get(part).is_none() {
            return Ok(());
        }
        let item = table.entry(part).or_insert_with(|| {
            let mut made = Table::new();
            // A table only there to hold another has no header of its own.
            made.set_implicit(i + 1 < tables.len());
            Item::Table(made)
        });
        table = item
            .as_table_like_mut()
            .ok_or_else(|| format!("{} is not a table", tables[..=i].join(".")))?;
    }
    match new {
        None => {
            table.remove(name);
            if table.is_empty() {
                prune(doc, tables);
            }
        }
        Some(mut new) => match table.get_mut(name).and_then(Item::as_value_mut) {
            Some(old) => {
                // The line's comment and spacing stay.
                *new.decor_mut() = old.decor().clone();
                *old = new;
            }
            None => {
                table.insert(name, Item::Value(new));
            }
        },
    }
    Ok(())
}

/// The tables of `path`, innermost first, taken out while they hold
/// nothing and say nothing (no comment of the user's over their header):
/// undoing a change that made `[touchpad]` leaves the file as it was.
fn prune(doc: &mut DocumentMut, path: &[&str]) {
    for depth in (1..=path.len()).rev() {
        let (parents, name) = path[..depth].split_at(depth - 1);
        let mut table: &mut dyn TableLike = doc.as_table_mut();
        for part in parents {
            match table.get_mut(part).and_then(Item::as_table_like_mut) {
                Some(inner) => table = inner,
                None => return,
            }
        }
        let empty = table.get(name[0]).is_some_and(|item| match item {
            Item::Table(t) => {
                let blank = |part: Option<&toml_edit::RawString>| {
                    part.and_then(|p| p.as_str())
                        .is_none_or(|p| p.trim().is_empty())
                };
                t.is_empty() && blank(t.decor().prefix()) && blank(t.decor().suffix())
            }
            _ => false,
        });
        if !empty {
            return;
        }
        table.remove(name[0]);
    }
}

/// `text` with `raw` at `slot`, checked to read back as asked.
pub fn written(text: &str, slot: Slot, raw: &Raw) -> Result<String, String> {
    let mut doc = hocus_config::parse(text).map_err(|err| err.to_string())?;
    write(&mut doc, slot, raw)?;
    let out = doc.to_string();
    check(&out, slot, raw)?;
    Ok(out)
}

/// Whether `text` says `raw` at `slot`.
pub fn check(text: &str, slot: Slot, raw: &Raw) -> Result<(), String> {
    let read = read(text);
    let said = read.reading(slot);
    let fine = match raw {
        Raw::Set(value) => *said == Reading::Value(value.clone()),
        Raw::Remove => *said == Reading::Absent,
        Raw::Toml(_) => read.state == FileState::Reads,
    };
    if fine {
        Ok(())
    } else {
        Err(format!("{} would read {said:?}", slot.word()))
    }
}

/// One key written by a page itself (6.9's screens): its tables then its
/// name (`["outputs", "DP-2", "mode"]`), and its value as TOML, or `None`
/// to take it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEdit {
    pub key: Vec<String>,
    pub value: Option<String>,
}

/// What `doc` says at each of `keys`, as edits that would write it back.
pub fn keys_now(doc: &DocumentMut, keys: &[Vec<String>]) -> Vec<KeyEdit> {
    keys.iter()
        .map(|key| KeyEdit {
            key: key.clone(),
            value: value_at(doc.as_table(), key).map(|v| canonical(&v)),
        })
        .collect()
}

/// The value at `key`; an array of tables (`[[windows.rules]]`) as an
/// array of inline tables.
fn value_at(table: &dyn TableLike, key: &[String]) -> Option<toml_edit::Value> {
    let (name, tables) = key.split_last()?;
    let mut table = table;
    for part in tables {
        table = table.get(part)?.as_table_like()?;
    }
    match table.get(name)? {
        Item::ArrayOfTables(tables) => Some(toml_edit::Value::Array(
            tables
                .iter()
                .map(|t| toml_edit::Value::InlineTable(t.clone().into_inline_table()))
                .collect(),
        )),
        item => item.as_value().cloned(),
    }
}

/// `value` written plainly, whatever its spacing in the file: what two
/// values that say the same thing both are.
fn canonical(value: &toml_edit::Value) -> String {
    match value {
        toml_edit::Value::Array(array) => {
            let items: Vec<String> = array.iter().map(canonical).collect();
            format!("[{}]", items.join(", "))
        }
        toml_edit::Value::InlineTable(table) => {
            let items: Vec<String> = table
                .iter()
                .map(|(k, v)| format!("{} = {}", toml_edit::Key::new(k), canonical(v)))
                .collect();
            format!("{{ {} }}", items.join(", "))
        }
        other => other.clone().decorated("", "").to_string(),
    }
}

/// `doc` with `edits`: each key's line only, its comment kept; the tables
/// they need made; a table emptied by a key taken out, with no comment of
/// its own, taken out too (the file then is what it was before the key
/// came).
pub fn write_keys(doc: &mut DocumentMut, edits: &[KeyEdit]) -> Result<(), String> {
    for edit in edits {
        let (name, tables) = edit.key.split_last().ok_or("an empty key")?;
        let tables_of_key = tables;
        let new = edit
            .value
            .as_ref()
            .map(|text| {
                text.parse::<toml_edit::Value>()
                    .map_err(|err| format!("{text}: {err}"))
            })
            .transpose()?;
        let Some(mut new) = new else {
            remove_key(doc.as_table_mut(), &edit.key);
            continue;
        };
        // An array of inline tables (`[[windows.rules]]`, 6.14) is written
        // as an array of tables, a table each.
        if let Some(tables) = array_of_tables(&new) {
            let mut table: &mut dyn TableLike = doc.as_table_mut();
            for part in tables_of_key {
                let item = table.entry(part).or_insert_with(|| {
                    let mut made = Table::new();
                    made.set_implicit(true);
                    Item::Table(made)
                });
                table = item
                    .as_table_like_mut()
                    .ok_or_else(|| format!("{part} is not a table"))?;
            }
            table.insert(name, Item::ArrayOfTables(tables));
            continue;
        }
        let mut table: &mut dyn TableLike = doc.as_table_mut();
        for (i, part) in tables.iter().enumerate() {
            let item = table.entry(part).or_insert_with(|| {
                let mut made = Table::new();
                made.set_implicit(i + 1 < tables.len());
                Item::Table(made)
            });
            table = item
                .as_table_like_mut()
                .ok_or_else(|| format!("{} is not a table", tables[..=i].join(".")))?;
        }
        match table.get_mut(name).and_then(Item::as_value_mut) {
            Some(old) => {
                *new.decor_mut() = old.decor().clone();
                *old = new;
            }
            None => {
                table.insert(name, Item::Value(new));
            }
        }
    }
    Ok(())
}

/// `key` taken out of `table`; the tables it empties go too, unless they
/// hold a comment.
fn remove_key(table: &mut Table, key: &[String]) {
    let Some((first, rest)) = key.split_first() else {
        return;
    };
    if rest.is_empty() {
        table.remove(first);
        return;
    }
    let emptied = match table.get_mut(first).and_then(Item::as_table_mut) {
        Some(inner) => {
            remove_key(inner, rest);
            let bare = inner
                .decor()
                .prefix()
                .and_then(|p| p.as_str())
                .is_none_or(|p| p.trim().is_empty());
            inner.is_empty() && bare
        }
        None => false,
    };
    if emptied {
        table.remove(first);
    }
}

/// `value` as an array of tables, when it is a list of inline tables.
fn array_of_tables(value: &toml_edit::Value) -> Option<toml_edit::ArrayOfTables> {
    let array = value.as_array().filter(|a| !a.is_empty())?;
    let mut tables = toml_edit::ArrayOfTables::new();
    for v in array {
        tables.push(v.as_inline_table()?.clone().into_table());
    }
    Some(tables)
}

/// Whether `text` says what `edits` wrote.
pub fn check_keys(text: &str, edits: &[KeyEdit]) -> Result<(), String> {
    let doc = hocus_config::parse(text).map_err(|err| err.to_string())?;
    for edit in edits {
        let now = value_at(doc.as_table(), &edit.key).map(|v| canonical(&v));
        let asked = edit
            .value
            .as_ref()
            .and_then(|t| t.parse::<toml_edit::Value>().ok())
            .map(|v| canonical(&v));
        if now != asked {
            return Err(format!("{} would read {now:?}", edit.key.join(".")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::setting;

    const FILE: &str = "# My desktop.\n\
        [keyboard]\n\
        layouts = [\"fr\"]   # mine\n\
        \n\
        # When I leave.\n\
        [idle]\n\
        delay = 300 # five minutes\n\
        \n\
        [power.battery]\n\
        suspend-after = \"3 heures\"\n";

    fn slot(id: &str, source: Source) -> Slot {
        Slot::of(setting(id).expect("a setting"), source)
    }

    #[test]
    fn values_defaults_and_values_not_understood() {
        let snap = read(FILE);
        assert_eq!(snap.state, FileState::Reads);
        let screen = slot("power.idle", Source::Ac);
        assert_eq!(
            snap.reading(screen),
            &Reading::Value(Value::Seconds(Some(300)))
        );
        let suspend = slot("power.suspend-after", Source::Battery);
        assert_eq!(
            snap.reading(suspend),
            &Reading::NotUnderstood("3 heures".to_owned())
        );
        // Not understood: the default applies.
        assert_eq!(snap.value(suspend), Some(Value::Seconds(Some(900))));
        let lid = slot("power.lid", Source::Ac);
        assert_eq!(snap.reading(lid), &Reading::Absent);
        assert_eq!(snap.value(lid), Some(Value::Word("suspend")));
    }

    #[test]
    fn a_change_writes_its_line_only_and_keeps_its_comment() -> anyhow::Result<()> {
        let screen = slot("power.idle", Source::Ac);
        let out = written(FILE, screen, &Raw::Set(Value::Seconds(Some(900))))
            .map_err(anyhow::Error::msg)?;
        assert_eq!(out, FILE.replace("delay = 300 #", "delay = 900 #"));
        // Over a value not understood: that line only.
        let suspend = slot("power.suspend-after", Source::Battery);
        let out = written(FILE, suspend, &Raw::Set(Value::Seconds(Some(1800))))
            .map_err(anyhow::Error::msg)?;
        assert_eq!(out, FILE.replace("\"3 heures\"", "\"30m\""));
        // Written back as it was.
        let back = written(&out, suspend, &Raw::Toml("\"3 heures\"".to_owned()))
            .map_err(anyhow::Error::msg)?;
        assert_eq!(back, FILE);
        Ok(())
    }

    #[test]
    fn new_keys_make_their_tables_and_removing_puts_the_file_back() -> anyhow::Result<()> {
        let text = "# Only a comment.\n[keyboard]\nlayouts = [\"fr\"]\n";
        let ac = slot("power.suspend-after", Source::Ac);
        let out =
            written(text, ac, &Raw::Set(Value::Seconds(Some(3600)))).map_err(anyhow::Error::msg)?;
        assert_eq!(out, format!("{text}\n[power.ac]\nsuspend-after = \"1h\"\n"));
        let lid = slot("power.lid", Source::Ac);
        let out2 =
            written(&out, lid, &Raw::Set(Value::Word("lock"))).map_err(anyhow::Error::msg)?;
        assert!(out2.contains("lid = \"lock\""), "{out2}");
        assert_eq!(read(&out2).value(ac), Some(Value::Seconds(Some(3600))));
        // Removing what was added takes its table out too: the file as it
        // was.
        let gone = written(&out, ac, &Raw::Remove).map_err(anyhow::Error::msg)?;
        assert_eq!(read(&gone).reading(ac), &Reading::Absent);
        assert_eq!(gone, text);
        // A table of the user's own, with its comment, stays.
        let own = "[power.ac] # mine\nsuspend-after = \"1h\"\n";
        let kept = written(own, ac, &Raw::Remove).map_err(anyhow::Error::msg)?;
        assert_eq!(kept, "[power.ac] # mine\n");
        // Removing what is not there changes nothing.
        assert_eq!(
            written(text, ac, &Raw::Remove).map_err(anyhow::Error::msg)?,
            text
        );
        Ok(())
    }

    /// 6.1's keys read where 6.10's are absent: the battery's
    /// `screen-off` is its dim; `lock-after` is `lock-on-idle`, off only
    /// when every source says never.
    #[test]
    fn a_file_written_by_6_1_keeps_its_battery_screen_off_as_the_dim_delay() {
        let old = read(
            "[power.battery]\nscreen-off = \"2m\"\nlock-after = \"5m\"\n\
             [power.ac]\nlock-after = \"never\"\n",
        );
        let dim = slot("power.idle", Source::Battery);
        assert_eq!(old.value(dim), Some(Value::Seconds(Some(120))));
        let lock = slot("power.lock-on-idle", Source::Ac);
        assert_eq!(old.value(lock), Some(Value::Bool(true)));
        let never =
            read("[power.battery]\nlock-after = \"never\"\n[power.ac]\nlock-after = \"never\"\n");
        assert_eq!(never.value(lock), Some(Value::Bool(false)));
        // 6.10's keys win.
        let both = read(
            "[power]\nlock-on-idle = true\n[power.battery]\nidle = \"1m\"\nscreen-off = \"2m\"\nlock-after = \"never\"\n",
        );
        assert_eq!(both.value(dim), Some(Value::Seconds(Some(60))));
        assert_eq!(both.value(lock), Some(Value::Bool(true)));
        assert_eq!(read("").value(dim), Some(Value::Seconds(Some(180))));
    }

    #[test]
    fn a_file_that_does_not_read_says_its_line() {
        let snap = read("[keyboard]\nlayouts = [\"fr\"]\n\n[idle\ndelay = 3\n");
        match snap.state {
            FileState::Broken { line, message } => {
                assert_eq!(line, Some(4));
                assert!(!message.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert!(written("[idle\n", slot("power.lid", Source::Ac), &Raw::Remove).is_err());
    }

    #[test]
    fn keys_written_then_taken_out_leave_the_file_as_it_was() -> anyhow::Result<()> {
        let key = |parts: &[&str]| parts.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();
        let scale = key(&["outputs", "HEADLESS-1", "scale"]);
        let mut doc = hocus_config::parse(FILE)?;
        let before = keys_now(&doc, std::slice::from_ref(&scale));
        assert_eq!(before[0].value, None);
        let set = [KeyEdit {
            key: scale.clone(),
            value: Some("1.25".to_owned()),
        }];
        write_keys(&mut doc, &set).map_err(anyhow::Error::msg)?;
        let out = doc.to_string();
        assert!(
            out.ends_with("[outputs.HEADLESS-1]\nscale = 1.25\n"),
            "{out}"
        );
        check_keys(&out, &set).map_err(anyhow::Error::msg)?;
        let mut doc = hocus_config::parse(&out)?;
        write_keys(&mut doc, &before).map_err(anyhow::Error::msg)?;
        assert_eq!(doc.to_string(), FILE);
        // Places, both screens at once, as arrays.
        let mut doc = hocus_config::parse(FILE)?;
        write_keys(
            &mut doc,
            &[
                KeyEdit {
                    key: key(&["outputs", "A", "position"]),
                    value: Some("[3440, 0]".to_owned()),
                },
                KeyEdit {
                    key: key(&["outputs", "B", "position"]),
                    value: Some("[0, 80]".to_owned()),
                },
            ],
        )
        .map_err(anyhow::Error::msg)?;
        let out = doc.to_string();
        assert!(out.contains("[outputs.A]\nposition = [3440, 0]\n"), "{out}");
        assert!(out.contains("[outputs.B]\nposition = [0, 80]\n"), "{out}");
        Ok(())
    }

    #[test]
    fn slots_go_both_ways_as_words() {
        for slot in slots() {
            assert_eq!(Slot::from_word(&slot.word()), Some(slot));
        }
        assert_eq!(Slot::from_word("power.lid@battery"), None);
    }

    #[test]
    fn rules_are_written_as_an_array_of_tables_and_read_back() -> Result<(), String> {
        let mut doc: DocumentMut = "# Mine.\n[keyboard]\nlayouts = [\"fr\"]\n"
            .parse()
            .map_err(|e| format!("{e}"))?;
        let key = vec!["windows".to_owned(), "rules".to_owned()];
        let rules = "[{ app-id = \"foot\", desktop = 2 }, { app-id = \"kitty\", above = true }]";
        let edit = KeyEdit {
            key: key.clone(),
            value: Some(rules.to_owned()),
        };
        write_keys(&mut doc, std::slice::from_ref(&edit))?;
        let text = doc.to_string();
        assert!(text.starts_with("# Mine.\n[keyboard]"), "{text}");
        assert!(
            text.contains("[[windows.rules]]\napp-id = \"foot\"\ndesktop = 2\n"),
            "{text}"
        );
        assert!(!text.contains("[windows]\n"), "{text}");
        check_keys(&text, &[edit])?;
        // What it says now, to write it back, says the same.
        let now = keys_now(&doc, std::slice::from_ref(&key));
        assert_eq!(now[0].value.as_deref(), Some(rules));
        // Taken out: no rules left.
        write_keys(&mut doc, &[KeyEdit { key, value: None }])?;
        assert!(!doc.to_string().contains("rules"), "{doc}");
        Ok(())
    }
}
