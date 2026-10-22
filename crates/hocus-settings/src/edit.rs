//! Changes to the configuration file: one key at a time, the rest of the
//! file as the user wrote it (comments, order, layout). A change that
//! would make the file invalid is not written.

use std::{io, path::Path};

use toml_edit::{DocumentMut, Item, Table, Value};

/// One key of one section set to a value.
#[derive(Debug, Clone)]
pub struct Change {
    pub section: &'static str,
    pub key: &'static str,
    pub value: Value,
}

impl Change {
    pub fn new(section: &'static str, key: &'static str, value: impl Into<Value>) -> Self {
        Self {
            section,
            key,
            value: value.into(),
        }
    }
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
    let section = doc
        .entry(change.section)
        .or_insert_with(|| Item::Table(Table::new()));
    let Some(table) = section.as_table_like_mut() else {
        return Err(EditError::Invalid(format!(
            "{} is not a section",
            change.section
        )));
    };
    match table.get_mut(change.key).and_then(Item::as_value_mut) {
        // Keeps the comment after the value.
        Some(old) => {
            let decor = old.decor().clone();
            *old = change.value.clone();
            *old.decor_mut() = decor;
        }
        None => {
            table.insert(change.key, Item::Value(change.value.clone()));
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
