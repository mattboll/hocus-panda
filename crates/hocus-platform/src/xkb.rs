//! The keyboard layouts the system knows: XKB's registry of layouts and
//! variants (`rules/evdev.xml`, from xkeyboard-config), with their names
//! translated by its message catalogue (`xkeyboard-config.mo`) where it has
//! them. Reading takes a few milliseconds: it is done on a thread, never on
//! the compositor's loop.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use quick_xml::{Reader, events::Event as Xml};

/// Where the registry and its translations are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registry {
    /// The machine's: `$XKB_CONFIG_ROOT`, or XKB's usual places.
    Machine,
    /// A copy of XKB's data (tests): `rules/evdev.xml` and
    /// `locale/<lang>/LC_MESSAGES/xkeyboard-config.mo` below it.
    At(PathBuf),
    /// None: only the names built in are known (tests without a copy).
    Off,
}

/// A layout, or one of its variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// As the configuration writes it: `fr`, `fr(bepo)`.
    pub code: String,
    /// XKB's name, in English: `French (BEPO)`.
    pub description: String,
    /// The same in the language asked, when the catalogue has it:
    /// `Français (BÉPO)`.
    pub translated: Option<String>,
}

impl Layout {
    /// Its name in the language asked, else in English.
    pub fn name(&self) -> &str {
        self.translated.as_deref().unwrap_or(&self.description)
    }
}

impl Registry {
    fn rules(&self) -> Option<PathBuf> {
        match self {
            Self::Machine => {
                let root = std::env::var_os("XKB_CONFIG_ROOT").map(PathBuf::from);
                root.into_iter()
                    .chain(["/usr/share/X11/xkb", "/usr/local/share/X11/xkb"].map(PathBuf::from))
                    .map(|r| r.join("rules/evdev.xml"))
                    .find(|p| p.is_file())
            }
            Self::At(dir) => Some(dir.join("rules/evdev.xml")),
            Self::Off => None,
        }
    }

    fn catalogue(&self, lang: &str) -> Option<PathBuf> {
        let file = format!("{lang}/LC_MESSAGES/xkeyboard-config.mo");
        match self {
            Self::Machine => ["/usr/share/locale", "/usr/local/share/locale"]
                .map(|d| Path::new(d).join(&file))
                .into_iter()
                .find(|p| p.is_file()),
            Self::At(dir) => Some(dir.join("locale").join(file)),
            Self::Off => None,
        }
    }

    /// Every layout and variant, in the registry's order (each layout
    /// before its variants), named in `lang` (`fr`) where the catalogue
    /// says. Nothing when there is no registry; English names when there
    /// is no catalogue.
    pub fn read(&self, lang: &str) -> Vec<Layout> {
        let Some(rules) = self.rules() else {
            return Vec::new();
        };
        let xml = match std::fs::read_to_string(&rules) {
            Ok(xml) => xml,
            Err(err) => {
                tracing::info!(%err, path = %rules.display(), "no keyboard layouts listed");
                return Vec::new();
            }
        };
        let words = self
            .catalogue(lang)
            .and_then(|p| std::fs::read(p).ok())
            .map(|mo| catalogue(&mo))
            .unwrap_or_default();
        layouts(&xml)
            .into_iter()
            .map(|(code, description)| Layout {
                translated: words.get(&description).cloned(),
                code,
                description,
            })
            .collect()
    }
}

/// What is being read inside a `configItem`.
#[derive(Debug, Default)]
struct Item {
    name: String,
    description: String,
}

/// The layouts and variants of a registry: code and English name.
fn layouts(xml: &str) -> Vec<(String, String)> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut item = Item::default();
    // The layout whose variants are being read.
    let mut layout = String::new();
    let mut out = Vec::new();
    let text = |path: &[String], item: &mut Item, t: &str| {
        // A name or description right in a layout's or a variant's item;
        // not a model's or an option's, nor a language's.
        let [.., owner, config, field] = path else {
            return;
        };
        if config != "configItem" || !(owner == "layout" || owner == "variant") {
            return;
        }
        match field.as_str() {
            "name" => item.name.push_str(t),
            "description" => item.description.push_str(t),
            _ => {}
        }
    };
    loop {
        match reader.read_event() {
            Ok(Xml::Start(e)) => {
                path.push(String::from_utf8_lossy(e.local_name().as_ref()).into_owned());
            }
            Ok(Xml::End(_)) => {
                let closed = path.pop();
                if closed.as_deref() != Some("configItem") {
                    continue;
                }
                let item = std::mem::take(&mut item);
                let (name, description) = (item.name.trim(), item.description.trim());
                if name.is_empty() {
                    continue;
                }
                match path.last().map(String::as_str) {
                    Some("layout") => {
                        layout = name.to_owned();
                        out.push((layout.clone(), description.to_owned()));
                    }
                    Some("variant") if !layout.is_empty() => {
                        out.push((format!("{layout}({name})"), description.to_owned()));
                    }
                    _ => {}
                }
            }
            Ok(Xml::Text(t)) => {
                let t = t
                    .xml10_content()
                    .map(|c| c.into_owned())
                    .unwrap_or_default();
                text(&path, &mut item, &t);
            }
            Ok(Xml::GeneralRef(r)) => {
                let t = match String::from_utf8_lossy(&r).as_ref() {
                    "lt" => "<".to_owned(),
                    "gt" => ">".to_owned(),
                    "amp" => "&".to_owned(),
                    "quot" => "\"".to_owned(),
                    "apos" => "'".to_owned(),
                    _ => r
                        .resolve_char_ref()
                        .ok()
                        .flatten()
                        .map(String::from)
                        .unwrap_or_default(),
                };
                text(&path, &mut item, &t);
            }
            Ok(Xml::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// A GNU message catalogue (`.mo`): each original string and its
/// translation. Nothing for a file that does not read as one.
fn catalogue(mo: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let word = |at: usize, big: bool| -> Option<usize> {
        let b: [u8; 4] = mo.get(at..at + 4)?.try_into().ok()?;
        let n = if big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        };
        usize::try_from(n).ok()
    };
    let big = match mo.get(..4) {
        Some([0xde, 0x12, 0x04, 0x95]) => false,
        Some([0x95, 0x04, 0x12, 0xde]) => true,
        _ => return out,
    };
    let (Some(count), Some(originals), Some(translations)) =
        (word(8, big), word(12, big), word(16, big))
    else {
        return out;
    };
    let string = |table: usize, i: usize| -> Option<&str> {
        let entry = table.checked_add(i.checked_mul(8)?)?;
        let (len, at) = (word(entry, big)?, word(entry + 4, big)?);
        std::str::from_utf8(mo.get(at..at.checked_add(len)?)?).ok()
    };
    for i in 0..count {
        let (Some(original), Some(translation)) = (string(originals, i), string(translations, i))
        else {
            break;
        };
        // A context comes before the string, after EOT; plural forms after
        // it, after NUL: the string itself, and its first form.
        let original = original.rsplit('\u{4}').next().unwrap_or(original);
        let original = original.split('\0').next().unwrap_or(original);
        let translation = translation.split('\0').next().unwrap_or(translation);
        if !original.is_empty() && !translation.is_empty() {
            out.insert(original.to_owned(), translation.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xkbConfigRegistry version="1.1">
  <modelList>
    <model><configItem><name>pc105</name><description>Generic 105-key PC</description></configItem></model>
  </modelList>
  <layoutList>
    <layout>
      <configItem>
        <name>fr</name>
        <shortDescription>fr</shortDescription>
        <description>French</description>
        <languageList><iso639Id>fra</iso639Id></languageList>
      </configItem>
      <variantList>
        <variant><configItem><name>bepo</name><description>French (BEPO)</description></configItem></variant>
        <variant><configItem><name>oss</name><description>French (alt.)</description></configItem></variant>
      </variantList>
    </layout>
    <layout>
      <configItem><name>us</name><description>English (US)</description></configItem>
      <variantList>
        <variant><configItem><name>dvorak</name><description>English (Dvorak &amp; more)</description></configItem></variant>
      </variantList>
    </layout>
  </layoutList>
  <optionList>
    <group allowMultipleSelection="true">
      <configItem><name>caps</name><description>Caps Lock behavior</description></configItem>
    </group>
  </optionList>
</xkbConfigRegistry>
"#;

    /// A catalogue as msgfmt writes it, little-endian.
    fn mo(pairs: &[(&str, &str)]) -> Vec<u8> {
        let n = pairs.len();
        let header = 28;
        let tables = header + 16 * n;
        let mut strings = Vec::new();
        let mut originals = Vec::new();
        let mut translations = Vec::new();
        for (o, _) in pairs {
            originals.push((o.len(), tables + strings.len()));
            strings.extend_from_slice(o.as_bytes());
            strings.push(0);
        }
        for (_, t) in pairs {
            translations.push((t.len(), tables + strings.len()));
            strings.extend_from_slice(t.as_bytes());
            strings.push(0);
        }
        let mut out = Vec::new();
        let w = |out: &mut Vec<u8>, v: usize| out.extend_from_slice(&(v as u32).to_le_bytes());
        for v in [0x9504_12de, 0, n, header, header + 8 * n, 0, 0] {
            w(&mut out, v);
        }
        for (len, at) in originals.into_iter().chain(translations) {
            w(&mut out, len);
            w(&mut out, at);
        }
        out.extend_from_slice(&strings);
        out
    }

    #[test]
    fn layouts_come_before_their_variants_and_options_are_not_layouts() {
        assert_eq!(
            layouts(RULES),
            [
                ("fr".into(), "French".into()),
                ("fr(bepo)".into(), "French (BEPO)".into()),
                ("fr(oss)".into(), "French (alt.)".into()),
                ("us".into(), "English (US)".into()),
                ("us(dvorak)".into(), "English (Dvorak & more)".into()),
            ]
        );
        assert!(layouts("<not xml").is_empty());
    }

    #[test]
    fn names_are_translated_where_the_catalogue_says() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join("rules"))?;
        std::fs::write(dir.path().join("rules/evdev.xml"), RULES)?;
        let registry = Registry::At(dir.path().to_owned());
        // No catalogue: English.
        let en = registry.read("fr");
        assert_eq!(en.len(), 5);
        assert!(en.iter().all(|l| l.translated.is_none()));
        let messages = dir.path().join("locale/fr/LC_MESSAGES");
        std::fs::create_dir_all(&messages)?;
        let catalogue = mo(&[
            ("", "Content-Type: text/plain; charset=UTF-8\n"),
            ("French", "Français"),
            ("French (BEPO)", "Français (BÉPO)"),
        ]);
        std::fs::write(messages.join("xkeyboard-config.mo"), catalogue)?;
        let fr = registry.read("fr");
        let name = |code: &str| fr.iter().find(|l| l.code == code).map(Layout::name);
        assert_eq!(name("fr"), Some("Français"));
        assert_eq!(name("fr(bepo)"), Some("Français (BÉPO)"));
        assert_eq!(name("fr(oss)"), Some("French (alt.)"));
        assert!(Registry::Off.read("fr").is_empty());
        assert!(Registry::At(dir.path().join("none")).read("fr").is_empty());
        Ok(())
    }

    #[test]
    fn a_broken_catalogue_translates_nothing() {
        assert!(catalogue(b"").is_empty());
        assert!(catalogue(&[0xde, 0x12, 0x04, 0x95, 0, 0, 0, 0, 9, 0, 0, 0]).is_empty());
        let mut cut = mo(&[("French", "Français")]);
        cut.truncate(40);
        assert!(catalogue(&cut).is_empty());
    }
}
