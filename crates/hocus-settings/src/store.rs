//! What the configuration file says of each setting, and a setting changed
//! in it: the file is edited as TOML that keeps its comments, order, blank
//! lines and unknown keys (`toml_edit`), only the changed line written.
//!
//! Pure functions on the file's text; [`crate::worker`] reads and writes
//! the file itself, off the window's thread.

use std::collections::BTreeMap;

use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::model::{Place, Setting, Source, Value, all_settings};

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
                .filter_map(|slot| Some((slot, reading(&doc, slot.place()?))))
                .collect(),
            reduced_motion: doc
                .get("appearance")
                .and_then(|a| a.get("reduced-motion"))
                .and_then(Item::as_bool)
                .unwrap_or(false),
        },
        Err(err) => Snapshot {
            state: FileState::Broken {
                line: hocus_config::error_line(text, &err),
                message: err.message().trim().to_owned(),
            },
            readings: BTreeMap::new(),
            reduced_motion: false,
        },
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
        let screen = slot("power.screen-off", Source::Ac);
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
        let screen = slot("power.screen-off", Source::Ac);
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
    fn slots_go_both_ways_as_words() {
        for slot in slots() {
            assert_eq!(Slot::from_word(&slot.word()), Some(slot));
        }
        assert_eq!(Slot::from_word("power.lid@battery"), None);
    }
}
