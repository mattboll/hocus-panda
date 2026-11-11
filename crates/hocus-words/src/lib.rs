//! The desktop's languages, and every word it says in them.
//!
//! [`Lang`] is the one language type of the workspace. The words live in
//! `words/<code>.toml`, one file a language, English the reference; the
//! build compiles them into one module a section:
//!
//! - a plain text is a [`Text`] constant: `hocus_words::perf::MEGABYTES.get(lang)`;
//! - a text with `{placeholders}`, or plural forms (`{ one, other }`), is a
//!   function taking the language and the arguments by name order (`n`,
//!   the count, first): `hocus_words::perf::frames(lang, 3)`;
//! - a list (the months) is a [`List`] constant.
//!
//! A key one file lacks, a placeholder English does not have, a list of
//! another length: the build stops and names it. A key the code asks
//! for and no file has does not compile. Adding a language is adding a
//! file (docs/design/settings.md, "Words").

/// A plural form. French counts 0 and 1 as one, English 1 only; a language
/// with more forms adds them here and its rule in `build.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Plural {
    One,
    Other,
}

/// What a plural text counts: any integer, its sign left out.
pub trait Count: std::fmt::Display + Copy {
    fn count(self) -> u64;
}

macro_rules! counts {
    ($($t:ty),*) => {$(
        impl Count for $t {
            fn count(self) -> u64 {
                self.unsigned_abs() as u64
            }
        }
    )*};
}

macro_rules! unsigned_counts {
    ($($t:ty),*) => {$(
        impl Count for $t {
            fn count(self) -> u64 {
                self as u64
            }
        }
    )*};
}

counts!(i8, i16, i32, i64, isize);
unsigned_counts!(u8, u16, u32, u64, usize);

use std::sync::atomic::{AtomicUsize, Ordering};

#[allow(
    clippy::useless_format,
    clippy::needless_borrows_for_generic_args,
    clippy::match_single_binding,
    reason = "generated: one shape for every key"
)]
mod catalogue {
    #[allow(unused_imports)]
    use crate::{Count, Plural};
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

pub use catalogue::*;

/// The language a program's file chose (`[locale] language`), over the
/// session's locale: 0 for none, else its place in [`Lang::ALL`] plus one.
static CHOSEN: AtomicUsize = AtomicUsize::new(0);

impl Lang {
    /// From now on, `of_messages`, `of_time` and `of_numbers` say `lang`
    /// when it is one, the session's locale when `None`.
    pub fn choose(lang: Option<Self>) {
        let at = lang.map_or(0, |lang| {
            Self::ALL
                .iter()
                .position(|l| *l == lang)
                .map_or(0, |i| i + 1)
        });
        CHOSEN.store(at, Ordering::Relaxed);
    }

    /// The language chosen, if one is.
    pub fn chosen() -> Option<Self> {
        let at = CHOSEN.load(Ordering::Relaxed);
        at.checked_sub(1).and_then(|i| Self::ALL.get(i).copied())
    }

    /// From a locale's name: the language whose code it starts with
    /// (`fr_FR.UTF-8`), else English.
    pub fn of(locale: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|lang| locale.starts_with(lang.code()))
            .unwrap_or_default()
    }

    /// The first of these variables set and not empty, as the C library
    /// takes them.
    pub fn from_vars(names: &[&str]) -> Self {
        let locale = names
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        Self::of(&locale)
    }

    /// The messages' language: the one chosen, else `LC_ALL`,
    /// `LC_MESSAGES`, then `LANG`.
    pub fn of_messages() -> Self {
        Self::chosen().unwrap_or_else(|| Self::from_vars(&["LC_ALL", "LC_MESSAGES", "LANG"]))
    }

    /// The dates' language: the one chosen, else `LC_ALL`, `LC_TIME`,
    /// then `LANG`.
    pub fn of_time() -> Self {
        Self::chosen().unwrap_or_else(|| Self::from_vars(&["LC_ALL", "LC_TIME", "LANG"]))
    }

    /// How numbers are written: the one chosen, else `LC_ALL`,
    /// `LC_NUMERIC`, then `LANG`.
    pub fn of_numbers() -> Self {
        Self::chosen().unwrap_or_else(|| Self::from_vars(&["LC_ALL", "LC_NUMERIC", "LANG"]))
    }

    /// The language a code names (`[locale] language = "fr"`).
    pub fn of_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.code() == code)
    }
}

/// For the crates' tests of their move to the catalogue (#775): every key
/// of `before` says, in French then English, exactly its two texts, and
/// every key of `formats` the same words around its placeholders, whatever
/// their names. Panics on the first that does not.
#[doc(hidden)]
#[expect(clippy::panic, reason = "a tests' helper: a mismatch fails the test")]
pub fn check_before(before: &[(&str, &str, &str)], formats: &[(&str, &str, &str)]) {
    use std::collections::HashMap;
    let said = |lang| every(lang).into_iter().collect::<HashMap<_, _>>();
    let (fr, en) = (said(Lang::French), said(Lang::English));
    let text = |all: &HashMap<String, String>, key: &str| {
        all.get(key)
            .cloned()
            .unwrap_or_else(|| panic!("{key} is not in the catalogue"))
    };
    for (key, before_fr, before_en) in before {
        assert_eq!(text(&fr, key), *before_fr, "{key}");
        assert_eq!(text(&en, key), *before_en, "{key}");
    }
    let bare = |s: &str| {
        let mut out = String::new();
        let mut inside = false;
        for c in s.replace("{{", "\u{1}").replace("}}", "\u{2}").chars() {
            match c {
                '{' => inside = true,
                '}' => {
                    inside = false;
                    out.push_str("{}");
                }
                _ if inside => {}
                '\u{1}' => out.push('{'),
                '\u{2}' => out.push('}'),
                c => out.push(c),
            }
        }
        out
    };
    for (key, before_fr, before_en) in formats {
        assert_eq!(bare(&text(&fr, key)), bare(before_fr), "{key}");
        assert_eq!(bare(&text(&en, key)), bare(before_en), "{key}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locale_names_its_language_and_english_is_the_default() {
        assert_eq!(Lang::of("fr_FR.UTF-8"), Lang::French);
        assert_eq!(Lang::of("fr"), Lang::French);
        assert_eq!(Lang::of("en_GB.UTF-8"), Lang::English);
        assert_eq!(Lang::of("de_DE.UTF-8"), Lang::English);
        assert_eq!(Lang::of(""), Lang::English);
        assert_eq!(Lang::default(), Lang::English);
        assert_eq!(Lang::of_code("fr"), Some(Lang::French));
        assert_eq!(Lang::of_code("system"), None);
    }

    #[test]
    fn french_counts_zero_and_one_as_singular() {
        assert_eq!(Lang::French.plural(0), Plural::One);
        assert_eq!(Lang::French.plural(1), Plural::One);
        assert_eq!(Lang::French.plural(2), Plural::Other);
        assert_eq!(Lang::English.plural(0), Plural::Other);
        assert_eq!(Lang::English.plural(1), Plural::One);
        assert_eq!(Lang::English.plural(2), Plural::Other);
    }

    #[test]
    fn every_key_says_something_in_every_language() {
        for lang in Lang::ALL {
            let all = every(lang);
            assert!(!all.is_empty());
            for (key, text) in all {
                assert!(!text.trim().is_empty(), "{key} is empty in {lang:?}");
            }
        }
    }

    /// Adding a language is adding a file: this crate, copied with a third
    /// file (English's words under the code `xx`), builds, and its
    /// language is chosen by a locale and by its code like the others.
    #[test]
    fn a_third_language_is_one_more_file() {
        use std::{fs, path::Path, process::Command};

        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dir = std::env::temp_dir().join(format!("hocus-words-third-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("words")).unwrap();
        fs::create_dir_all(dir.join("tests")).unwrap();
        fs::copy(here.join("build.rs"), dir.join("build.rs")).unwrap();
        fs::copy(here.join("src/lib.rs"), dir.join("src/lib.rs")).unwrap();
        for code in ["en", "fr"] {
            let file = format!("words/{code}.toml");
            fs::copy(here.join(&file), dir.join(&file)).unwrap();
        }
        let english = fs::read_to_string(here.join("words/en.toml")).unwrap();
        let (_, words) = english.split_once("plural = \"n=1\"\n").unwrap();
        fs::write(
            dir.join("words/xx.toml"),
            format!(
                "[language]\ncode = \"xx\"\nvariant = \"Test\"\nname = \"Test\"\nplural = \"n=1\"\n{words}"
            ),
        )
        .unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"hocus-words\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             [build-dependencies]\ntoml = \"1\"\n[workspace]\n",
        )
        .unwrap();
        let lock = here.join("../../Cargo.lock");
        fs::copy(lock, dir.join("Cargo.lock")).unwrap();
        fs::write(
            dir.join("tests/third.rs"),
            r#"use hocus_words::Lang;

#[test]
fn the_third_language_is_there() {
    assert_eq!(Lang::ALL.len(), 3);
    assert_eq!(Lang::of("xx_XX.UTF-8"), Lang::Test);
    assert_eq!(Lang::of_code("xx"), Some(Lang::Test));
    assert_eq!(Lang::Test.name(), "Test");
    assert_eq!(hocus_words::every(Lang::Test), hocus_words::every(Lang::English));
    Lang::choose(Some(Lang::Test));
    assert_eq!(Lang::of_messages(), Lang::Test);
}
"#,
        )
        .unwrap();
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let out = Command::new(cargo)
            .args(["test", "--offline", "--quiet", "--test", "third"])
            .current_dir(&dir)
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stderr).into_owned();
        let _ = fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{said}");
    }
}
