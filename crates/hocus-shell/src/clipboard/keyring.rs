//! The clipboard's history's key in the session's keyring (5.6): the
//! freedesktop Secret Service (`org.freedesktop.secrets`: GNOME Keyring,
//! KWallet, KeePassXC…), on the session bus the compositor names, never
//! another. One item, its attributes `application=hocuspanda` and
//! `hocuspanda=clipboard`, holding the key's 32 bytes, in the default
//! collection.
//!
//! Asked on the history's own thread, through the shell's keyring
//! ([`crate::keyring`]), with its timeout.

use crate::keyring::{self, Attributes};

const ATTRIBUTES: [(&str, &str); 2] = [("application", "hocuspanda"), ("hocuspanda", "clipboard")];

/// What the keyring said.
#[derive(Debug)]
pub enum Found {
    Key(Vec<u8>),
    /// It answered, and holds no key of the history's.
    Absent,
    /// It did not answer, or not in time.
    NoAnswer,
}

fn attributes() -> Attributes {
    ATTRIBUTES
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// The history's key, as the keyring on `bus` holds it.
pub fn find(bus: &str) -> Found {
    let found = keyring::blocking(bus, async |keyring| keyring.find(&attributes()).await);
    match found {
        Some(Some(key)) if key.len() == 32 => Found::Key(key),
        Some(Some(_)) => {
            tracing::warn!("the keyring's key for the clipboard is not one");
            Found::NoAnswer
        }
        Some(None) => Found::Absent,
        None => Found::NoAnswer,
    }
}

/// Keeps `key` in the keyring on `bus`: whether it did.
pub fn keep(bus: &str, key: &[u8]) -> bool {
    let key = key.to_vec();
    keyring::blocking(bus, async move |keyring| {
        keyring
            .keep(
                "Hocuspanda: the clipboard's history",
                &attributes(),
                key,
                "application/octet-stream",
            )
            .await
    })
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bus_that_is_not_there_is_no_answer() {
        let dir = tempfile::tempdir().expect("a directory");
        let bus = format!("unix:path={}", dir.path().join("none").display());
        assert!(matches!(find(&bus), Found::NoAnswer));
        assert!(!keep(&bus, &[0; 32]));
    }
}
