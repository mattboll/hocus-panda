//! Keyboard layouts as the configuration writes them: an XKB layout and,
//! in brackets, its variant, as XKB's own rules do: `fr`, `fr(bepo)`,
//! `us(intl)`. XKB takes the layouts and the variants as two lists.

/// A layout's XKB layout and variant: `fr(bepo)` is `("fr", "bepo")`, `fr`
/// is `("fr", "")`. `None` for anything else: XKB names are letters,
/// digits, `_` and `-`.
pub fn split(code: &str) -> Option<(&str, &str)> {
    let name = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    match code.split_once('(') {
        Some((layout, rest)) => {
            let variant = rest.strip_suffix(')')?;
            (name(layout) && name(variant)).then_some((layout, variant))
        }
        None => name(code).then_some((code, "")),
    }
}

/// The layouts and the variants XKB is given, each list comma-separated
/// in the same order: `["fr", "fr(bepo)"]` is `("fr,fr", ",bepo")`.
/// A code that does not read goes as it is, for XKB to refuse.
pub fn xkb_lists(codes: &[String]) -> (String, String) {
    let (layouts, variants): (Vec<&str>, Vec<&str>) = codes
        .iter()
        .map(|c| split(c).unwrap_or((c.as_str(), "")))
        .unzip();
    (layouts.join(","), variants.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_and_its_variant() {
        assert_eq!(split("fr"), Some(("fr", "")));
        assert_eq!(split("fr(bepo)"), Some(("fr", "bepo")));
        assert_eq!(split("us(altgr-intl)"), Some(("us", "altgr-intl")));
        assert_eq!(split("fr(bepo_afnor)"), Some(("fr", "bepo_afnor")));
        for bad in [
            "", "fr(", "fr()", "(bepo)", "fr(bepo", "fr bepo", "fr,us", "fr(a)(b)",
        ] {
            assert_eq!(split(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn xkb_gets_two_lists_in_the_same_order() {
        let codes = [
            "fr".to_owned(),
            "fr(bepo)".to_owned(),
            "us(intl)".to_owned(),
        ];
        assert_eq!(xkb_lists(&codes), ("fr,fr,us".into(), ",bepo,intl".into()));
        assert_eq!(xkb_lists(&[]), (String::new(), String::new()));
    }
}
