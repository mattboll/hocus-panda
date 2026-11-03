//! An item's menu, as `com.canonical.dbusmenu`'s `GetLayout` gives it: a
//! tree of `(ia{sv}av)`, each entry its id, its properties and its
//! children. What the popover draws: labels (their mnemonics' underscores
//! left out), separators, entries that are off, checkmarks and radio
//! items, icons, submenus; hidden entries left out.

use std::collections::HashMap;

use iced::advanced::image;
use zbus::Variant;

/// What an entry toggles, and whether it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    None,
    Check(bool),
    Radio(bool),
}

/// What an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Standard,
    Separator,
}

/// An entry of a menu, and its own when it is a submenu.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i32,
    pub kind: Kind,
    pub label: String,
    pub enabled: bool,
    pub toggle: Toggle,
    pub icon: Option<image::Handle>,
    pub children: Vec<Entry>,
}

impl Entry {
    /// Whether it opens a submenu.
    pub fn submenu(&self) -> bool {
        !self.children.is_empty()
    }

    /// Whether a click acts on it.
    pub fn acts(&self) -> bool {
        self.kind == Kind::Standard && self.enabled
    }

    /// The entry `id` under it, itself included.
    #[cfg(test)]
    pub fn find(&self, id: i32) -> Option<&Entry> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }
}

/// What an entry's icon is, for the thread to decode.
pub enum IconOf<'a> {
    Name(&'a str),
    Png(&'a [u8]),
}

/// `label` without its mnemonic's mark: one underscore goes, two make one.
pub fn label(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                out.push('_');
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The entry a layout's `(ia{sv}av)` describes, its icons decoded by
/// `icon`; `None` when it does not read, or is hidden.
pub fn parse(
    value: &Variant<'_>,
    icon: &mut dyn FnMut(IconOf<'_>) -> Option<image::Handle>,
) -> Option<Entry> {
    let value = match value {
        Variant::Variant(inner) => inner.as_ref(),
        other => other,
    };
    let Variant::Structure(entry) = value else {
        return None;
    };
    let [
        Variant::I32(id),
        Variant::Dict(props),
        Variant::Array(children),
    ] = entry.fields()
    else {
        return None;
    };
    let props: HashMap<String, &Variant<'_>> = props
        .iter()
        .filter_map(|(k, v)| {
            let key = match k {
                Variant::Str(s) => s.as_str().to_owned(),
                _ => return None,
            };
            Some((key, inner(v)))
        })
        .collect();
    let text = |name: &str| match props.get(name) {
        Some(Variant::Str(s)) => Some(s.as_str().to_owned()),
        _ => None,
    };
    let flag = |name: &str, default: bool| match props.get(name) {
        Some(Variant::Bool(b)) => *b,
        _ => default,
    };
    if !flag("visible", true) {
        return None;
    }
    let kind = match text("type").as_deref() {
        Some("separator") => Kind::Separator,
        _ => Kind::Standard,
    };
    let on = match props.get("toggle-state") {
        Some(Variant::I32(n)) => *n == 1,
        _ => false,
    };
    let toggle = match text("toggle-type").as_deref() {
        Some("checkmark") => Toggle::Check(on),
        Some("radio") => Toggle::Radio(on),
        _ => Toggle::None,
    };
    let decoded = match (props.get("icon-data"), text("icon-name")) {
        (Some(Variant::Array(bytes)), _) => {
            let bytes: Vec<u8> = bytes
                .inner()
                .iter()
                .filter_map(|b| match b {
                    Variant::U8(b) => Some(*b),
                    _ => None,
                })
                .collect();
            icon(IconOf::Png(&bytes))
        }
        (_, Some(name)) if !name.is_empty() => icon(IconOf::Name(&name)),
        _ => None,
    };
    let children = children
        .inner()
        .iter()
        .filter_map(|child| parse(child, icon))
        .collect();
    Some(Entry {
        id: *id,
        kind,
        label: label(&text("label").unwrap_or_default()),
        enabled: flag("enabled", true),
        toggle,
        icon: decoded,
        children,
    })
}

fn inner<'a, 'b>(value: &'b Variant<'a>) -> &'b Variant<'a> {
    match value {
        Variant::Variant(inner) => inner.as_ref(),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use zbus::{Array, Dict, Signature, Structure};

    use super::*;

    fn entry<'a>(
        id: i32,
        props: &[(&'a str, Variant<'a>)],
        children: Vec<Variant<'a>>,
    ) -> Variant<'a> {
        let mut dict = Dict::new(&Signature::Str, &Signature::Variant);
        for (k, v) in props {
            dict.append(
                Variant::from(*k),
                Variant::Variant(Box::new(v.try_clone().expect("a value"))),
            )
            .expect("an entry");
        }
        let mut array = Array::new(&Signature::Variant);
        for child in children {
            array
                .append(Variant::Variant(Box::new(child)))
                .expect("a child");
        }
        Variant::Structure(
            Structure::builder()
                .append_field(Variant::I32(id))
                .append_field(Variant::Dict(dict))
                .append_field(Variant::Array(array))
                .build()
                .expect("an entry"),
        )
    }

    #[test]
    fn a_layout_reads_as_its_entries() {
        let root = entry(
            0,
            &[("children-display", Variant::from("submenu"))],
            vec![
                entry(1, &[("label", Variant::from("_Ouvrir Slack"))], vec![]),
                entry(2, &[("type", Variant::from("separator"))], vec![]),
                entry(
                    3,
                    &[
                        ("label", Variant::from("Ne pas déranger")),
                        ("toggle-type", Variant::from("checkmark")),
                        ("toggle-state", Variant::I32(1)),
                    ],
                    vec![],
                ),
                entry(
                    4,
                    &[
                        ("label", Variant::from("Off")),
                        ("enabled", Variant::Bool(false)),
                    ],
                    vec![],
                ),
                entry(5, &[("visible", Variant::Bool(false))], vec![]),
                entry(
                    6,
                    &[("label", Variant::from("Statut"))],
                    vec![entry(
                        7,
                        &[
                            ("label", Variant::from("Actif")),
                            ("toggle-type", Variant::from("radio")),
                            ("toggle-state", Variant::I32(0)),
                        ],
                        vec![],
                    )],
                ),
            ],
        );
        let root = parse(&root, &mut |_| None).expect("a menu");
        let ids: Vec<i32> = root.children.iter().map(|c| c.id).collect();
        assert_eq!(ids, [1, 2, 3, 4, 6]);
        assert_eq!(root.children[0].label, "Ouvrir Slack");
        assert!(root.children[0].acts());
        assert_eq!(root.children[1].kind, Kind::Separator);
        assert!(!root.children[1].acts());
        assert_eq!(root.children[2].toggle, Toggle::Check(true));
        assert!(!root.children[3].enabled);
        assert!(root.children[4].submenu());
        assert_eq!(root.find(7).map(|e| e.toggle), Some(Toggle::Radio(false)));
    }

    #[test]
    fn mnemonics_marks_go() {
        assert_eq!(label("_Quitter"), "Quitter");
        assert_eq!(label("Se_ttings"), "Settings");
        assert_eq!(label("snake__case"), "snake_case");
        assert_eq!(label(""), "");
    }
}
