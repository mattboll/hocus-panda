//! Compiles the catalogue (`words/*.toml`, one file a language) into
//! Rust: the [`Lang`] enum, a constant a plain text, a function a text
//! with arguments or plural forms, a constant a list. What the files
//! disagree on stops the build, with the key and the file named: a key
//! one language lacks, a placeholder English does not have, lists of
//! different lengths. docs/decisions/0006-words.md says why.

#![allow(
    clippy::indexing_slicing,
    reason = "the reference language is checked present before any [0]"
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

/// The language every other is checked against, and the default.
const REFERENCE: &str = "en";

/// A plural entry's forms, in the order they are written.
const FORMS: [&str; 2] = ["one", "other"];

fn main() {
    let dir = PathBuf::from(env("CARGO_MANIFEST_DIR")).join("words");
    println!("cargo::rerun-if-changed={}", dir.display());
    let mut langs = Vec::new();
    let mut entries = fs::read_dir(&dir)
        .unwrap_or_else(|e| fail(&format!("{}: {e}", dir.display())))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect::<Vec<_>>();
    entries.sort();
    for path in &entries {
        println!("cargo::rerun-if-changed={}", path.display());
        langs.push(read(path));
    }
    // The reference first: it is the default, and its order is `ALL`'s.
    langs.sort_by_key(|l| (l.code != REFERENCE, l.code.clone()));
    if langs.first().is_none_or(|l| l.code != REFERENCE) {
        fail(&format!("words/{REFERENCE}.toml is missing"));
    }
    let mut out = String::new();
    head(&mut out, &langs);
    let mut every = String::new();
    module(&mut out, &mut every, &langs, &[]);
    let _ = writeln!(
        out,
        "/// Every key, rendered in `lang`: plural keys for 0, 1 and 2, the\n\
         /// other arguments as `{{name}}`, lists item by item.\n\
         pub fn every(lang: Lang) -> Vec<(String, String)> {{\n\
         let mut all = Vec::new();\n{every}all\n}}"
    );
    let file = PathBuf::from(env("OUT_DIR")).join("words.rs");
    fs::write(&file, out).unwrap_or_else(|e| fail(&format!("{}: {e}", file.display())));
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| fail(&format!("{name} is not set")))
}

fn fail(why: &str) -> ! {
    eprintln!("hocus-words: {why}");
    std::process::exit(1)
}

/// One catalogue file.
struct Language {
    code: String,
    variant: String,
    name: String,
    plural: String,
    file: String,
    words: toml::Table,
}

fn read(path: &Path) -> Language {
    let file = format!(
        "words/{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let text = fs::read_to_string(path).unwrap_or_else(|e| fail(&format!("{file}: {e}")));
    let mut words: toml::Table = text
        .parse()
        .unwrap_or_else(|e| fail(&format!("{file}: {e}")));
    let Some(toml::Value::Table(meta)) = words.remove("language") else {
        fail(&format!("{file}: no [language] table"));
    };
    let field = |key: &str| match meta.get(key) {
        Some(toml::Value::String(s)) => s.clone(),
        _ => fail(&format!("{file}: [language] {key} is missing")),
    };
    let code = field("code");
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    if code != stem {
        fail(&format!(
            "{file}: its code is {code:?}, its name says {stem:?}"
        ));
    }
    let variant = field("variant");
    if !variant.chars().all(|c| c.is_ascii_alphanumeric())
        || !variant.starts_with(|c: char| c.is_ascii_uppercase())
    {
        fail(&format!(
            "{file}: variant {variant:?} is not a Rust name (`German`)"
        ));
    }
    let plural = field("plural");
    if rule(&plural).is_none() {
        fail(&format!(
            "{file}: plural {plural:?} is none of \"n=1\", \"n<2\", \"none\""
        ));
    }
    Language {
        code,
        variant,
        name: field("name"),
        plural,
        file,
        words,
    }
}

/// A plural rule's code, `n` the count.
fn rule(rule: &str) -> Option<&'static str> {
    match rule {
        // English, German, Spanish…: one for 1 only.
        "n=1" => Some("if n == 1 { Plural::One } else { Plural::Other }"),
        // French: 0 and 1 are singular.
        "n<2" => Some("if n < 2 { Plural::One } else { Plural::Other }"),
        // Chinese, Japanese…: one form.
        "none" => Some("{ let _ = n; Plural::Other }"),
        _ => None,
    }
}

fn field_name(code: &str) -> String {
    code.to_ascii_lowercase().replace('-', "_")
}

fn head(out: &mut String, langs: &[Language]) {
    let variants: String = langs
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let default = if i == 0 { "#[default] " } else { "" };
            format!(
                "    /// {}, `words/{}.toml`.\n    {default}{},\n",
                l.name, l.code, l.variant
            )
        })
        .collect();
    let all: Vec<String> = langs
        .iter()
        .map(|l| format!("Lang::{}", l.variant))
        .collect();
    let arms = |f: &dyn Fn(&Language) -> String| -> String {
        langs
            .iter()
            .map(|l| format!("Lang::{} => {},", l.variant, f(l)))
            .collect()
    };
    let codes = arms(&|l| format!("{:?}", l.code));
    let names = arms(&|l| format!("{:?}", l.name));
    let rules = arms(&|l| rule(&l.plural).unwrap_or_default().to_owned());
    let fields: String = langs
        .iter()
        .map(|l| format!("{}: &'static str,", field_name(&l.code)))
        .collect();
    let list_fields: String = langs
        .iter()
        .map(|l| format!("{}: &'static [&'static str],", field_name(&l.code)))
        .collect();
    let gets = arms(&|l| format!("self.{}", field_name(&l.code)));
    let count = langs.len();
    let _ = write!(
        out,
        r#"/// A language the desktop speaks: one a catalogue file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum Lang {{
{variants}}}

impl Lang {{
    /// Every language, English first.
    pub const ALL: [Lang; {count}] = [{all}];

    /// Its code, the start of the locales that speak it (`fr` of
    /// `fr_FR.UTF-8`).
    pub const fn code(self) -> &'static str {{
        match self {{ {codes} }}
    }}

    /// Its name, in itself: "Français".
    pub const fn name(self) -> &'static str {{
        match self {{ {names} }}
    }}

    /// The plural form a count takes.
    pub const fn plural(self, n: u64) -> Plural {{
        match self {{ {rules} }}
    }}
}}

/// A plain text, in every language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text {{ {fields} }}

impl Text {{
    pub const fn get(self, lang: Lang) -> &'static str {{
        match lang {{ {gets} }}
    }}
}}

/// A list of texts (the months), in every language, of one length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct List {{ {list_fields} }}

impl List {{
    pub const fn get(self, lang: Lang) -> &'static [&'static str] {{
        match lang {{ {gets} }}
    }}
}}

"#,
        all = all.join(", "),
    );
}

/// What one key is in one language.
enum Said {
    Text(String),
    Plural(BTreeMap<String, String>),
    List(Vec<String>),
}

fn said(value: &toml::Value) -> Option<Said> {
    match value {
        toml::Value::String(s) => Some(Said::Text(s.clone())),
        toml::Value::Array(items) => items
            .iter()
            .map(|i| i.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .map(Said::List),
        toml::Value::Table(t)
            if t.contains_key("other") && t.keys().all(|k| FORMS.contains(&k.as_str())) =>
        {
            t.iter()
                .map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect::<Option<BTreeMap<_, _>>>()
                .map(Said::Plural)
        }
        _ => None,
    }
}

fn table_at<'a>(words: &'a toml::Table, path: &[String]) -> Option<&'a toml::Table> {
    path.iter()
        .try_fold(words, |t, key| t.get(key).and_then(toml::Value::as_table))
}

fn module(out: &mut String, every: &mut String, langs: &[Language], path: &[String]) {
    let reference = &langs[0];
    let Some(table) = table_at(&reference.words, path) else {
        return;
    };
    // Every other language has the same keys here, no more.
    for lang in &langs[1..] {
        let Some(theirs) = table_at(&lang.words, path) else {
            fail(&format!("{}: [{}] is missing", lang.file, path.join(".")));
        };
        for key in theirs.keys() {
            if !table.contains_key(key) {
                fail(&format!(
                    "{}: {} is not in {}",
                    lang.file,
                    dotted(path, key),
                    reference.file
                ));
            }
        }
    }
    for (key, value) in table {
        ident(key, &dotted(path, key), &reference.file);
        if value.is_table() && said(value).is_none() {
            let mut inner = path.to_vec();
            inner.push(key.clone());
            let _ = writeln!(
                out,
                "pub mod {} {{\n#[allow(unused_imports)] use crate::{{Lang, Plural, Text, List, Count}};",
                raw(key)
            );
            module(out, every, langs, &inner);
            out.push_str("}\n");
            continue;
        }
        let all: Vec<Said> = langs
            .iter()
            .map(|lang| {
                let value = table_at(&lang.words, path).and_then(|t| t.get(key));
                let Some(value) = value else {
                    fail(&format!("{}: {} is missing", lang.file, dotted(path, key)));
                };
                said(value).unwrap_or_else(|| {
                    fail(&format!(
                        "{}: {} is not a text, a list of texts or {{ one, other }}",
                        lang.file,
                        dotted(path, key)
                    ))
                })
            })
            .collect();
        entry(out, every, langs, path, key, &all);
    }
}

fn dotted(path: &[String], key: &str) -> String {
    let mut all = path.to_vec();
    all.push(key.to_owned());
    all.join(".")
}

const KEYWORDS: [&str; 38] = [
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "final", "try",
];

fn ident(key: &str, at: &str, file: &str) {
    let ok = key.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !["self", "super", "crate", "lang", "every"].contains(&key);
    if !ok {
        fail(&format!("{file}: {at}: keys are snake_case names"));
    }
}

fn raw(key: &str) -> String {
    if KEYWORDS.contains(&key) {
        format!("r#{key}")
    } else {
        key.to_owned()
    }
}

/// The names a template's placeholders give, `{name}` or `{name:spec}`.
fn placeholders(template: &str, at: &str, file: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                let inside: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let name = inside.split(':').next().unwrap_or_default();
                if name.is_empty()
                    || !name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                    || !name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    || name == "lang"
                    || KEYWORDS.contains(&name)
                {
                    fail(&format!(
                        "{file}: {at}: {{{inside}}} is not a named placeholder (`{{name}}`; braces are written {{{{ and }}}})"
                    ));
                }
                names.insert(name.to_owned());
            }
            '}' => fail(&format!(
                "{file}: {at}: a lone }} (braces are written {{{{ and }}}})"
            )),
            _ => {}
        }
    }
    names
}

/// `memory_line` → `MemoryLine`: a key's arguments' struct.
fn camel(key: &str) -> String {
    key.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn unescape(template: &str) -> String {
    template.replace("{{", "{").replace("}}", "}")
}

fn doc(out: &mut String, english: &str) {
    let _ = writeln!(out, "#[doc = {:?}]", format!("`{english}`"));
}

fn entry(
    out: &mut String,
    every: &mut String,
    langs: &[Language],
    path: &[String],
    key: &str,
    all: &[Said],
) {
    let at = dotted(path, key);
    let module = path.iter().map(|p| raw(p)).collect::<Vec<_>>().join("::");
    let module = if module.is_empty() {
        "crate".to_owned()
    } else {
        format!("crate::{module}")
    };
    // Lists.
    if all.iter().any(|s| matches!(s, Said::List(_))) {
        let mut lists = Vec::new();
        for (said, lang) in all.iter().zip(langs) {
            let Said::List(items) = said else {
                fail(&format!("{}: {at} is a list in English", lang.file));
            };
            for item in items {
                if !placeholders(item, &at, &lang.file).is_empty() {
                    fail(&format!(
                        "{}: {at}: a list's items take no placeholder",
                        lang.file
                    ));
                }
            }
            lists.push(items.iter().map(|i| unescape(i)).collect::<Vec<_>>());
        }
        let len = lists[0].len();
        for (items, lang) in lists.iter().zip(langs) {
            if items.len() != len {
                fail(&format!(
                    "{}: {at} has {} items, English {len}",
                    lang.file,
                    items.len()
                ));
            }
        }
        doc(out, &lists[0].join(", "));
        let fields: String = lists
            .iter()
            .zip(langs)
            .map(|(items, l)| format!("{}: &{items:?},", field_name(&l.code)))
            .collect();
        let name = key.to_ascii_uppercase();
        let _ = writeln!(out, "pub const {name}: List = List {{ {fields} }};");
        let _ = writeln!(
            every,
            "for (i, item) in {module}::{name}.get(lang).iter().enumerate() {{ all.push((format!(\"{at}[{{i}}]\"), (*item).to_owned())); }}"
        );
        return;
    }
    let plural = all.iter().any(|s| matches!(s, Said::Plural(_)));
    // The forms each language gives, a plain text standing for all.
    let forms: Vec<BTreeMap<String, String>> = all
        .iter()
        .zip(langs)
        .map(|(said, lang)| match said {
            Said::Text(t) => FORMS.iter().map(|f| ((*f).to_owned(), t.clone())).collect(),
            Said::Plural(p) => {
                let other = p.get("other").cloned().unwrap_or_default();
                FORMS
                    .iter()
                    .map(|f| ((*f).to_owned(), p.get(*f).cloned().unwrap_or(other.clone())))
                    .collect()
            }
            Said::List(_) => fail(&format!("{}: {at} is a list in another file", lang.file)),
        })
        .collect();
    let mut params = BTreeSet::new();
    for template in forms[0].values() {
        params.extend(placeholders(template, &at, &langs[0].file));
    }
    if plural {
        params.insert("n".to_owned());
    }
    for (theirs, lang) in forms.iter().zip(langs).skip(1) {
        for template in theirs.values() {
            for name in placeholders(template, &at, &lang.file) {
                if !params.contains(&name) {
                    fail(&format!(
                        "{}: {at}: {{{name}}} is not one of {}'s placeholders",
                        lang.file, langs[0].file
                    ));
                }
            }
        }
    }
    let english = forms[0].get("other").cloned().unwrap_or_default();
    if params.is_empty() {
        doc(out, &unescape(&english));
        let name = key.to_ascii_uppercase();
        let fields: String = forms
            .iter()
            .zip(langs)
            .map(|(f, l)| {
                format!(
                    "{}: {:?},",
                    field_name(&l.code),
                    unescape(f.get("other").map_or("", String::as_str))
                )
            })
            .collect();
        let _ = writeln!(out, "pub const {name}: Text = Text {{ {fields} }};");
        let _ = writeln!(
            every,
            "all.push((\"{at}\".to_owned(), {module}::{name}.get(lang).to_owned()));"
        );
        return;
    }
    // `n` first, the others by name.
    let mut ordered: Vec<String> = Vec::new();
    if plural {
        ordered.push("n".to_owned());
    }
    ordered.extend(params.iter().filter(|p| !plural || *p != "n").cloned());
    // One argument: a parameter. More: a struct of named fields, so that
    // two of one type cannot be swapped at the call.
    let named = ordered.len() > 1;
    let bound = |p: &str| {
        if plural && p == "n" {
            "Count"
        } else {
            "::std::fmt::Display"
        }
    };
    let types: Vec<String> = (0..ordered.len()).map(|i| format!("T{i}")).collect();
    let mut camel = camel(key);
    if ["Lang", "Plural", "Text", "List", "Count"].contains(&camel.as_str()) {
        camel.push_str("Args");
    }
    doc(out, &english);
    if named {
        let fields: String = ordered
            .iter()
            .zip(&types)
            .map(|(p, t)| format!("pub {}: {t},", raw(p)))
            .collect();
        let _ = writeln!(
            out,
            "#[derive(Debug, Clone, Copy)]\npub struct {camel}<{}> {{ {fields} }}",
            types.join(", ")
        );
        let generics: String = ordered
            .iter()
            .zip(&types)
            .map(|(p, t)| format!("{t}: {}", bound(p)))
            .collect::<Vec<_>>()
            .join(", ");
        let names: String = ordered
            .iter()
            .map(|p| raw(p))
            .collect::<Vec<_>>()
            .join(", ");
        doc(out, &english);
        let _ = writeln!(
            out,
            "pub fn {}<{generics}>(lang: Lang, args: {camel}<{}>) -> String {{\nlet {camel} {{ {names} }} = args;",
            raw(key),
            types.join(", ")
        );
    } else {
        let p = ordered.first().map_or("", String::as_str);
        let kind = if bound(p) == "Count" {
            "impl Count"
        } else {
            "impl ::std::fmt::Display"
        };
        let _ = writeln!(
            out,
            "pub fn {}(lang: Lang, {}: {kind}) -> String {{",
            raw(key),
            raw(p)
        );
    }
    let _ = writeln!(out, "match lang {{");
    for (said, (f, lang)) in all.iter().zip(forms.iter().zip(langs)) {
        let one = f.get("one").map_or("", String::as_str);
        let other = f.get("other").map_or("", String::as_str);
        if plural && matches!(said, Said::Plural(_)) && one != other {
            let _ = writeln!(
                out,
                "Lang::{} => match lang.plural(n.count()) {{ Plural::One => format!({one:?}), Plural::Other => format!({other:?}) }},",
                lang.variant
            );
        } else {
            let _ = writeln!(out, "Lang::{} => format!({other:?}),", lang.variant);
        }
    }
    out.push_str("}\n}\n");
    let value = |p: &String| {
        if plural && p == "n" {
            "n".to_owned()
        } else {
            format!("\"{{{p}}}\"")
        }
    };
    let call = if named {
        let fields: String = ordered
            .iter()
            .map(|p| format!("{}: {}, ", raw(p), value(p)))
            .collect();
        format!(
            "{module}::{}(lang, {module}::{camel} {{ {fields} }})",
            raw(key)
        )
    } else {
        format!(
            "{module}::{}(lang, {})",
            raw(key),
            ordered.first().map(value).unwrap_or_default()
        )
    };
    if plural {
        let _ = writeln!(
            every,
            "for n in [0u64, 1, 2] {{ all.push((format!(\"{at}[{{n}}]\"), {call})); }}"
        );
    } else {
        let _ = writeln!(every, "all.push((\"{at}\".to_owned(), {call}));");
    }
}
