//! XKB's layout names in the bar's language: xkeyboard-config's own
//! gettext catalog, `xkeyboard-config-2.mo` (or its older name
//! `xkeyboard-config.mo`), which translates the registry's
//! descriptions ("French (BEPO)" is "Français (BÉPO)") for every desktop
//! that lists layouts. It ships with XKB's data (Debian's `xkb-data`), so
//! the names come from where the layouts do; a catalog missing leaves
//! XKB's English names.
//!
//! A `.mo` file is a header, two tables of (length, offset) pairs, and the
//! strings they point at; read here in a few lines, every offset checked,
//! rather than through a gettext library and the C library's locale state.

use std::path::PathBuf;

use crate::clock::time::Lang;

/// Where the catalogs are (`/usr/share/locale` otherwise), for tests.
pub const DIR_ENV: &str = "HOCUS_LOCALE_DIR";
const DOMAINS: [&str; 2] = ["xkeyboard-config-2", "xkeyboard-config"];

/// The catalog for `lang`, if one is installed; none for English, XKB's
/// own. Reads a file: off the loop.
pub fn read(lang: Lang) -> Option<Vec<u8>> {
    let code = match lang {
        Lang::French => "fr",
        Lang::English => return None,
    };
    let dir = std::env::var_os(DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/locale"));
    let found = DOMAINS.iter().find_map(|domain| {
        let path = dir
            .join(code)
            .join("LC_MESSAGES")
            .join(format!("{domain}.mo"));
        std::fs::read(&path).ok().map(|bytes| (path, bytes))
    });
    match found {
        Some((path, bytes)) => {
            tracing::debug!(catalog = %path.display(), "the layouts' names");
            Some(bytes)
        }
        None => {
            tracing::info!(dir = %dir.display(), "no XKB catalog: the layouts' names in English");
            None
        }
    }
}

/// `msgid`'s translation in `catalog`, a `.mo` file's bytes; `None` when it
/// has none, or the file does not read.
pub fn translate<'a>(catalog: &'a [u8], msgid: &str) -> Option<&'a str> {
    let word = |at: usize, big: bool| -> Option<usize> {
        let bytes: [u8; 4] = catalog.get(at..at.checked_add(4)?)?.try_into().ok()?;
        let n = if big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        };
        usize::try_from(n).ok()
    };
    let big = match word(0, false)? {
        0x9504_12de => false,
        0xde12_0495 => true,
        _ => return None,
    };
    // No more strings than the file has room for their entries.
    let count = word(8, big)?.min(catalog.len() / 8);
    let (originals, translations) = (word(12, big)?, word(16, big)?);
    let string = |table: usize, i: usize| -> Option<&'a [u8]> {
        let entry = table.checked_add(i.checked_mul(8)?)?;
        let (len, at) = (word(entry, big)?, word(entry.checked_add(4)?, big)?);
        catalog.get(at..at.checked_add(len)?)
    };
    let i = (0..count).find(|&i| string(originals, i) == Some(msgid.as_bytes()))?;
    let translation = string(translations, i)?;
    // Plural forms follow the first, after a NUL.
    let first = translation.split(|b| *b == 0).next()?;
    std::str::from_utf8(first).ok().filter(|t| !t.is_empty())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A little-endian `.mo` file of `pairs`, as msgfmt writes one.
    pub(crate) fn mo(pairs: &[(&str, &str)]) -> Vec<u8> {
        let n = pairs.len();
        let originals = 28;
        let translations = originals + 8 * n;
        let mut strings = translations + 8 * n;
        let mut tables = Vec::new();
        let mut data = Vec::new();
        for side in [0, 1] {
            for pair in pairs {
                let text = if side == 0 { pair.0 } else { pair.1 };
                tables.extend((text.len() as u32).to_le_bytes());
                tables.extend((strings as u32).to_le_bytes());
                data.extend(text.as_bytes());
                data.push(0);
                strings += text.len() + 1;
            }
        }
        let mut file = Vec::new();
        for word in [
            0x9504_12de_u32,
            0,
            n as u32,
            originals as u32,
            translations as u32,
            0,
            0,
        ] {
            file.extend(word.to_le_bytes());
        }
        file.extend(tables);
        file.extend(data);
        file
    }

    #[test]
    fn a_catalog_translates_what_it_holds() {
        let catalog = mo(&[
            ("English (US)", "Anglais (US)"),
            ("French", "Français"),
            ("French (BEPO)", "Français (BÉPO)"),
        ]);
        assert_eq!(translate(&catalog, "French"), Some("Français"));
        assert_eq!(
            translate(&catalog, "French (BEPO)"),
            Some("Français (BÉPO)")
        );
        assert_eq!(translate(&catalog, "Russian"), None);
        // Big-endian, as some writers make them.
        let mut big = catalog.clone();
        // The header's words and the tables'.
        for word in big[..28 + 48].as_chunks_mut::<4>().0 {
            word.reverse();
        }
        assert_eq!(translate(&big, "English (US)"), Some("Anglais (US)"));
    }

    #[test]
    fn a_broken_catalog_translates_nothing() {
        let catalog = mo(&[("French", "Français")]);
        assert_eq!(translate(&catalog[..30], "French"), None);
        assert_eq!(translate(b"not a catalog at all", "French"), None);
        assert_eq!(translate(&[], "French"), None);
        // A count far past the file's end.
        let mut lying = catalog.clone();
        lying[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(translate(&lying, "Russian"), None);
        // An offset past it.
        let mut far = catalog;
        far[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(translate(&far, "French"), None);
    }
}
