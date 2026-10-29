//! The keyboard's layouts in the bar (bar.md §4.3, §5; 3.8): a row under
//! Bluetooth's in the full form, the layout's code in a button under the
//! battery in the mini form, drawn only with two layouts or more. A click
//! (or `Enter` on the row holding the keyboard) asks the compositor for the
//! next layout; the row changes when the compositor says which is active,
//! not before.
//!
//! The compositor owns XKB: it sends the keymap's layouts (their XKB codes
//! and XKB's English names) as the shell connects, and the active one as it
//! changes, on the link. The link makes them what the bar shows ([`made`],
//! off the loop: it reads XKB's catalog), keeps them here, and wakes the
//! surfaces that show them: the connections' in the full form, the levels'
//! in the mini, nothing else.

pub mod catalog;
pub mod draw;

use std::sync::{Mutex, PoisonError};

use hocus_link::{CONNECTIONS_NAMESPACE, Form, KeyLayout, LEVELS_NAMESPACE};

use crate::{clock::time::Lang, popover};

/// A layout as the bar shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// Its full name in the bar's language: "Français (BÉPO)".
    pub name: String,
    /// Its short code: "FR", "BÉPO".
    pub code: String,
}

/// The layouts, and the active one's index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layouts {
    pub list: Vec<Layout>,
    pub active: usize,
}

impl Layouts {
    /// The layout the row shows: the active one, when there is a choice
    /// (two layouts or more); `None`: no row.
    pub fn shown(&self) -> Option<&Layout> {
        if self.list.len() < 2 {
            return None;
        }
        self.list.get(self.active)
    }
}

static LAYOUTS: Mutex<Layouts> = Mutex::new(Layouts {
    list: Vec::new(),
    active: 0,
});

/// The layouts as last said.
pub fn snapshot() -> Layouts {
    LAYOUTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// The compositor's layouts made into what the bar shows, in `lang`: the
/// names translated by XKB's catalog when it has them, the codes from the
/// XKB codes. Reads the catalog: off the loop.
pub fn made(layouts: &[KeyLayout], lang: Lang) -> Vec<Layout> {
    let catalog = catalog::read(lang);
    let names: Vec<String> = layouts
        .iter()
        .map(|layout| {
            let translated = catalog
                .as_deref()
                .and_then(|c| catalog::translate(c, &layout.name));
            match translated {
                Some(name) => name.to_owned(),
                None if layout.name.is_empty() => layout.code.clone(),
                None => layout.name.clone(),
            }
        })
        .collect();
    let codes: Vec<(&str, &str)> = layouts
        .iter()
        .zip(&names)
        .map(|(layout, name)| (layout.code.as_str(), name.as_str()))
        .collect();
    names
        .iter()
        .zip(codes_of(&codes))
        .map(|(name, code)| Layout {
            name: name.clone(),
            code,
        })
        .collect()
}

/// The short codes of layouts given by their XKB codes and names: the
/// layout's in capitals ("FR", "US"); when another layout given shares it,
/// the variant's, spelled as the name writes it when it holds it ("BÉPO"
/// for `fr(bepo)` named "Français (BÉPO)", "BEPO" for "French (BEPO)"),
/// else as XKB names it ("NODEADKEYS"). Without a code, the name's first
/// two letters.
pub fn codes_of(layouts: &[(&str, &str)]) -> Vec<String> {
    let split = |code: &str| -> (String, String) {
        match code.split_once('(') {
            Some((layout, variant)) => {
                (layout.to_owned(), variant.trim_end_matches(')').to_owned())
            }
            None => (code.to_owned(), String::new()),
        }
    };
    let parts: Vec<(String, String)> = layouts.iter().map(|(code, _)| split(code)).collect();
    layouts
        .iter()
        .zip(&parts)
        .map(|((_, name), (layout, variant))| {
            if layout.is_empty() {
                return name
                    .chars()
                    .filter(|c| c.is_alphabetic())
                    .take(2)
                    .collect::<String>()
                    .to_uppercase();
            }
            let shared = parts.iter().filter(|(other, _)| other == layout).count() > 1;
            if shared && !variant.is_empty() {
                spelled(variant, name).unwrap_or(variant).to_uppercase()
            } else {
                layout.to_uppercase()
            }
        })
        .collect()
}

/// The word of `name` that is `variant` with its accents ("BÉPO" in
/// "Français (BÉPO)" for `bepo`): as many letters, the same where the
/// word's are plain ASCII, any letter where they are not.
fn spelled<'a>(variant: &str, name: &'a str) -> Option<&'a str> {
    let combining = |c: char| ('\u{300}'..='\u{36f}').contains(&c);
    name.split(|c: char| !(c.is_alphanumeric() || combining(c)))
        .find(|word| {
            let letters: Vec<char> = word.chars().filter(|c| !combining(*c)).collect();
            letters.len() == variant.chars().count()
                && letters.iter().zip(variant.chars()).all(|(w, v)| {
                    if w.is_ascii() {
                        w.eq_ignore_ascii_case(&v)
                    } else {
                        w.is_alphabetic() && v.is_ascii_alphabetic()
                    }
                })
        })
}

/// The layouts are `list` now: kept, and the surfaces that show them
/// woken (the row comes or goes: both, the connections' resized).
pub fn set(list: Vec<Layout>) {
    let (before, after) = {
        let mut kept = LAYOUTS.lock().unwrap_or_else(PoisonError::into_inner);
        let before = kept.shown().cloned();
        kept.list = list;
        (before, kept.shown().cloned())
    };
    tracing::debug!(shown = ?after, "the keyboard's layouts");
    if before.is_some() != after.is_some() {
        crate::network::draw::rows_changed();
        wake(&[Form::Full, Form::Mini]);
    } else if before != after {
        wake(&popover::with(|c| c.forms()));
    }
}

/// Layout `index` is active now: the surfaces that show it woken, if what
/// they show changes.
pub fn activate(index: u32) {
    let (before, after) = {
        let mut kept = LAYOUTS.lock().unwrap_or_else(PoisonError::into_inner);
        let before = kept.shown().cloned();
        kept.active = usize::try_from(index).unwrap_or(usize::MAX);
        (before, kept.shown().cloned())
    };
    if before != after {
        tracing::debug!(shown = ?after, "the active layout");
        wake(&popover::with(|c| c.forms()));
    }
}

/// The surfaces showing the layouts in `forms` draw again: the
/// connections' (the full form's row), the levels' (the mini form's
/// button).
fn wake(forms: &[Form]) {
    let effects = [
        (Form::Full, CONNECTIONS_NAMESPACE),
        (Form::Mini, LEVELS_NAMESPACE),
    ]
    .into_iter()
    .filter(|(form, _)| forms.contains(form))
    .map(|(_, namespace)| popover::Effect::Wake(namespace))
    .collect();
    popover::send(effects);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_its_layouts_unless_another_shares_it() {
        assert_eq!(
            codes_of(&[("fr", "Français"), ("us", "Anglais (US)")]),
            ["FR", "US"]
        );
        // Two French layouts: the variant's, as the name spells it.
        assert_eq!(
            codes_of(&[
                ("fr", "Français"),
                ("fr(bepo)", "Français (BÉPO)"),
                ("ru", "Russe")
            ]),
            ["FR", "BÉPO", "RU"]
        );
        assert_eq!(
            codes_of(&[("fr", "French"), ("fr(bepo)", "French (BEPO)")]),
            ["FR", "BEPO"]
        );
        // Its accent written apart (decomposed) still reads.
        assert_eq!(
            codes_of(&[("fr", "Français"), ("fr(bepo)", "Français (BE\u{301}PO)")]),
            ["FR", "BE\u{301}PO"]
        );
        // A variant the name does not spell: as XKB names it.
        assert_eq!(
            codes_of(&[
                ("us", "English (US)"),
                ("us(intl)", "English (US, intl., with dead keys)"),
                ("de(nodeadkeys)", "German (no dead keys)"),
            ]),
            ["US", "INTL", "DE"]
        );
        assert_eq!(
            codes_of(&[
                ("fr(oss)", "French (alt.)"),
                ("fr(azerty)", "French (AZERTY)")
            ]),
            ["OSS", "AZERTY"]
        );
        // No code: the name's first letters.
        assert_eq!(codes_of(&[("", "English (US)")]), ["EN"]);
    }

    #[test]
    fn the_row_shows_only_with_a_choice() {
        let one = Layouts {
            list: vec![Layout {
                name: "Français".into(),
                code: "FR".into(),
            }],
            active: 0,
        };
        assert_eq!(one.shown(), None);
        let mut two = one.clone();
        two.list.push(Layout {
            name: "Anglais (US)".into(),
            code: "US".into(),
        });
        assert_eq!(two.shown().map(|l| l.code.as_str()), Some("FR"));
        two.active = 1;
        assert_eq!(two.shown().map(|l| l.code.as_str()), Some("US"));
        // A group past the list (more than XKB took): none.
        two.active = 4;
        assert_eq!(two.shown(), None);
    }

    #[test]
    fn names_are_xkbs_without_a_catalog() {
        let layouts = [
            KeyLayout {
                code: "fr".into(),
                name: "French".into(),
            },
            KeyLayout {
                code: "fr(bepo)".into(),
                name: "French (BEPO)".into(),
            },
        ];
        let made = made(&layouts, Lang::English);
        assert_eq!(made[0].name, "French");
        assert_eq!(made[1].code, "BEPO");
    }
}
