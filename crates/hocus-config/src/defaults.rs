//! Every setting's default, defined once: `packaging/config.toml`, the
//! commented file the package ships, which lists every setting with its
//! default. Uncommented, it reads as no file at all for the compositor
//! (its test checks its `Default`s against it), and the Settings take
//! their rows' defaults from it, so that the three cannot drift apart.
//!
//! Why the file and not shared structs: the compositor's structs hold
//! what only it knows (its shortcuts' parsing, its desktops' limits, the
//! cursor's sizes), and the Settings hold values as their rows show them
//! (tenths, words, delays); a TOML document both read is the one form
//! they share without either depending on the other.

use std::sync::OnceLock;

use toml_edit::{DocumentMut, Item};

/// The shipped file, commented.
pub const SHIPPED: &str = include_str!("../../../packaging/config.toml");

/// `shipped` uncommented: its settings' lines (`# key = value`, `#
/// [table]`), without its prose and its indented examples.
pub fn uncomment(shipped: &str) -> String {
    shipped
        .lines()
        .filter_map(|l| l.strip_prefix("# ").filter(|l| !l.starts_with(' ')))
        .filter(|l| l.starts_with('[') || l.contains(" = "))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// The keys the file leaves unset by default, the system's value applying
/// (the cursor's environment, the locale's hours, the touchpad's own way
/// of clicking right): the shipped file shows them as examples only.
pub const UNSET: &[&[&str]] = &[
    &["appearance", "cursor-theme"],
    &["appearance", "cursor-size"],
    &["locale", "time"],
    &["touchpad", "right-click"],
];

/// `path` is a key of the configuration: with a default, or unset by
/// default ([`UNSET`]).
pub fn known(path: &[&str]) -> bool {
    value(path).is_some() || UNSET.contains(&path)
}

/// The defaults, as a TOML document.
pub fn document() -> &'static DocumentMut {
    static DOC: OnceLock<DocumentMut> = OnceLock::new();
    DOC.get_or_init(|| {
        // A test reads it; a file that did not read would fail it first.
        crate::parse(&uncomment(SHIPPED)).unwrap_or_default()
    })
}

/// The default of the key at `path` (`["power", "battery",
/// "suspend-after"]`), when the defaults list it.
pub fn value(path: &[&str]) -> Option<&'static toml_edit::Value> {
    let (name, tables) = path.split_last()?;
    let mut table = document().as_table() as &dyn toml_edit::TableLike;
    for t in tables {
        table = table.get(t)?.as_table_like()?;
    }
    match table.get(name)? {
        Item::Value(value) => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_reads_and_names_its_keys() {
        assert!(crate::parse(&uncomment(SHIPPED)).is_ok());
        assert_eq!(
            value(&["idle", "delay"]).and_then(toml_edit::Value::as_integer),
            Some(300)
        );
        assert!(value(&["power", "battery", "suspend-after"]).is_some());
        assert!(value(&["power", "battery"]).is_none(), "a table");
        assert!(value(&["no", "such"]).is_none());
    }

    #[test]
    fn an_unset_key_is_only_an_example_of_the_shipped_file() {
        for path in UNSET {
            assert!(value(path).is_none(), "{path:?} has a default");
            let (name, tables) = path.split_last().unwrap_or((&"", &[]));
            let table = format!("# [{}]\n", tables.join("."));
            let section = SHIPPED.split(&table).nth(1).unwrap_or_default();
            let section = section.split("\n# [").next().unwrap_or_default();
            assert!(
                section.contains(&format!("#   {name} = ")),
                "{path:?}: no example in {table}"
            );
        }
    }
}
