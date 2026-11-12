//! Every key of the configuration a row of the Settings writes, named as
//! that row names it, and its values as its choices say them: what the
//! agents' toast and journal say of a setting an agent changed ("mis
//! « Thème » sur « Sombre »", docs/design/mcp.md), in the words catalogue
//! the Settings show.
//!
//! The table is generated from the Settings' model (crate
//! `hocus-settings`, which the shell does not link): its test
//! `the_labels_are_the_rows` fails when a row and this table disagree, and
//! `HOCUS_LABELS_WRITE=1` writes the table again.

use hocus_words::Text;

/// How a key's row says its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said {
    /// On or off: `true`, `false`.
    Switch,
    /// Its choices: the value as the file writes it (a word unquoted), its
    /// label.
    Choices(&'static [(&'static str, Text)]),
    /// A whole number, followed by its unit (` %`).
    Number(&'static str),
    /// Said by its name alone (a delay, a colour, a path, a list).
    Other,
}

/// A key's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label {
    /// Dotted: `appearance.theme`, `power.battery.suspend-after`.
    pub key: &'static str,
    /// Its row's name.
    pub name: Text,
    pub said: Said,
}

impl Label {
    /// What `value` (as the file writes it, a word unquoted) reads as on
    /// its row, when its row says it otherwise than as it is.
    pub fn value(&self, value: &str, lang: hocus_words::Lang) -> Option<String> {
        match self.said {
            Said::Choices(choices) => choices
                .iter()
                .find(|(v, _)| *v == value)
                .map(|(_, label)| label.get(lang).to_owned()),
            Said::Number(unit) => value.parse::<i64>().ok().map(|n| format!("{n}{unit}")),
            Said::Switch | Said::Other => None,
        }
    }
}

include!("labels_table.rs");

/// The row of the dotted `key`, if a row writes it.
pub fn of(key: &str) -> Option<&'static Label> {
    ALL.iter().find(|label| label.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_words::Lang;

    #[test]
    fn a_key_is_named_as_its_row_names_it() {
        let theme = of("appearance.theme").expect("the theme's row");
        assert_eq!(theme.name.get(Lang::French), "Thème");
        assert_eq!(theme.value("dark", Lang::French).as_deref(), Some("Sombre"));
        assert_eq!(
            of("appearance.reduced-motion").map(|l| l.said),
            Some(Said::Switch)
        );
        assert!(of("no.such-key").is_none());
        let mut keys: Vec<&str> = ALL.iter().map(|l| l.key).collect();
        let all = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), all, "one row a key");
    }
}
