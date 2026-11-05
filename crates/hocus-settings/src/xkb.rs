//! XKB's layouts as the page lists them (6.8): xkeyboard-config's registry
//! (`/usr/share/X11/xkb/rules/evdev.xml`, its layouts and their variants),
//! their names in French through xkeyboard-config's own catalog
//! (`xkeyboard-config-2.mo`), as the bar names them (bar.md §3.8; the
//! shell's `layouts/catalog.rs`, whose reader this follows). Read once, on
//! a thread, never the window's.

use std::path::PathBuf;

use crate::lang::Lang;

/// The registry's place (`HOCUS_XKB_REGISTRY` in tests), and the
/// catalogs' (`HOCUS_LOCALE_DIR`, as the shell's).
const REGISTRY_ENV: &str = "HOCUS_XKB_REGISTRY";
const LOCALE_ENV: &str = "HOCUS_LOCALE_DIR";

/// A layout or a variant XKB knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    /// `fr`, `fr(bepo)`.
    pub code: String,
    /// "Français (BÉPO)", translated when a catalog says it.
    pub name: String,
    /// The layout's own name, for a variant.
    pub of: Option<String>,
}

/// Every layout and variant, in the registry's order.
pub fn read(lang: Lang) -> Vec<Known> {
    let path = std::env::var_os(REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/X11/xkb/rules/evdev.xml"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            tracing::info!(%err, path = %path.display(), "no XKB registry: no layout to add");
            return Vec::new();
        }
    };
    let catalog = catalog(lang);
    let name = |english: &str| {
        catalog
            .as_deref()
            .and_then(|c| translate(c, english))
            .unwrap_or(english)
            .to_owned()
    };
    parse(&text)
        .into_iter()
        .map(|(code, english, of)| Known {
            code,
            name: name(&english),
            of: of.map(|o| name(&o)),
        })
        .collect()
}

/// The text between `<tag>` and `</tag>` in `block`, from `from`.
fn inside<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(&block[start..end])
}

/// `&amp;` and its kind, as XML writes them.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The registry's layouts and variants: their codes, English names and,
/// for a variant, its layout's name.
fn parse(text: &str) -> Vec<(String, String, Option<String>)> {
    let mut out = Vec::new();
    let Some(list) = inside(text, "layoutList") else {
        return out;
    };
    for layout in list.split("<layout>").skip(1) {
        let layout = layout.split("</layout>").next().unwrap_or_default();
        let Some(item) = inside(layout, "configItem") else {
            continue;
        };
        let (Some(code), Some(name)) = (inside(item, "name"), inside(item, "description")) else {
            continue;
        };
        let (code, name) = (code.trim().to_owned(), unescape(name.trim()));
        out.push((code.clone(), name.clone(), None));
        let Some(variants) = inside(layout, "variantList") else {
            continue;
        };
        for variant in variants.split("<variant>").skip(1) {
            let Some(item) = inside(variant, "configItem") else {
                continue;
            };
            if let (Some(v), Some(vname)) = (inside(item, "name"), inside(item, "description")) {
                out.push((
                    format!("{code}({})", v.trim()),
                    unescape(vname.trim()),
                    Some(name.clone()),
                ));
            }
        }
    }
    out
}

/// The catalog for `lang`; none for English, XKB's own.
fn catalog(lang: Lang) -> Option<Vec<u8>> {
    if !lang.fr() {
        return None;
    }
    let dir = std::env::var_os(LOCALE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/locale"));
    ["xkeyboard-config-2", "xkeyboard-config"]
        .iter()
        .find_map(|domain| {
            std::fs::read(dir.join("fr/LC_MESSAGES").join(format!("{domain}.mo"))).ok()
        })
}

/// `msgid`'s translation in a `.mo` file's bytes, every offset checked.
fn translate<'a>(catalog: &'a [u8], msgid: &str) -> Option<&'a str> {
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
    let count = word(8, big)?.min(catalog.len() / 8);
    let (originals, translations) = (word(12, big)?, word(16, big)?);
    let string = |table: usize, i: usize| -> Option<&'a [u8]> {
        let entry = table.checked_add(i.checked_mul(8)?)?;
        let (len, at) = (word(entry, big)?, word(entry.checked_add(4)?, big)?);
        catalog.get(at..at.checked_add(len)?)
    };
    let i = (0..count).find(|&i| string(originals, i) == Some(msgid.as_bytes()))?;
    let first = string(translations, i)?.split(|b| *b == 0).next()?;
    std::str::from_utf8(first).ok().filter(|t| !t.is_empty())
}

/// The system's layouts (`/etc/default/keyboard`), for a file that names
/// none: XKB's codes, `fr(bepo)`.
pub fn system_layouts() -> Vec<String> {
    let path = std::env::var_os("HOCUS_SYSTEM_KEYBOARD")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/default/keyboard"));
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let value = |key: &str| -> String {
        text.lines()
            .filter_map(|l| l.trim().split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().trim_matches(['"', '\'']).to_owned())
            .unwrap_or_default()
    };
    let (layouts, variants) = (value("XKBLAYOUT"), value("XKBVARIANT"));
    let mut variants = variants.split(',').map(str::trim);
    layouts
        .split(',')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| match variants.next().unwrap_or_default() {
            "" => l.to_owned(),
            v => format!("{l}({v})"),
        })
        .collect()
}

/// The code on a layout's chip: the layout's in capitals, the variant's
/// when another layout of the list shares it.
pub fn chip(code: &str, list: &[String]) -> String {
    let (layout, variant) = match code.split_once('(') {
        Some((l, v)) => (l, v.trim_end_matches(')')),
        None => (code, ""),
    };
    let shared = list
        .iter()
        .filter(|c| c.split('(').next() == Some(layout))
        .count()
        > 1;
    if shared && !variant.is_empty() {
        variant.to_uppercase()
    } else {
        layout.to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_reads_its_layouts_and_variants() {
        let xml = "<xkbConfigRegistry><layoutList>\
            <layout><configItem><name>fr</name><shortDescription>fr</shortDescription>\
            <description>French</description></configItem><variantList>\
            <variant><configItem><name>bepo</name><description>French (BEPO)</description>\
            </configItem></variant></variantList></layout>\
            <layout><configItem><name>us</name><description>English (US)</description>\
            </configItem></layout></layoutList></xkbConfigRegistry>";
        let read = parse(xml);
        assert_eq!(
            read,
            [
                ("fr".to_owned(), "French".to_owned(), None),
                (
                    "fr(bepo)".to_owned(),
                    "French (BEPO)".to_owned(),
                    Some("French".to_owned())
                ),
                ("us".to_owned(), "English (US)".to_owned(), None),
            ]
        );
        let list = ["fr".to_owned(), "us".to_owned(), "fr(bepo)".to_owned()];
        assert_eq!(chip("fr", &list), "FR");
        assert_eq!(chip("fr(bepo)", &list), "BEPO");
        assert_eq!(chip("us", &list), "US");
    }
}
