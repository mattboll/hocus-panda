//! Names and aliases as they are compared: accents, case, apostrophes and
//! spaces folded, so that "Égaluantes", "egaluantes" and "  égaluantes "
//! are one.

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.nfd().filter(|c| !is_combining_mark(*c)) {
        let c = match c {
            '\u{2019}' | '\u{2018}' | '`' => '\'',
            'œ' | 'Œ' => {
                if space && !out.is_empty() {
                    out.push(' ');
                }
                space = false;
                out.push_str("oe");
                continue;
            }
            c => c,
        };
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.extend(c.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn accents_case_and_spaces_fold() {
        assert_eq!(fold("  Égaluantes "), "egaluantes");
        assert_eq!(fold("Chapitô"), "chapito");
        assert_eq!(fold("le  site\tWeb"), "le site web");
        assert_eq!(fold("L’Œuvre"), "l'oeuvre");
    }
}
