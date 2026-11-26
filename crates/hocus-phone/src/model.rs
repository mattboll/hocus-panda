//! What the bar shows of KDE Connect: the paired phones and tablets
//! ([`Device`]), which of them get a row and in what order ([`rows`]), and
//! a transfer's progress ([`Transfer`]).

use std::collections::HashMap;

use zbus::{OwnedVariant, Variant};

/// An interface's properties, as KDE Connect says them.
pub type Props = HashMap<String, OwnedVariant>;

/// The rows at most: past them, those seen in reach last.
pub const MOST: usize = 4;

/// A battery at this level or under, not charging, is low: the row's dot.
pub const LOW: u8 = 15;

/// What a device is, for its icon and its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Phone,
    Tablet,
}

impl Kind {
    /// KDE Connect's `type`: `phone` (`smartphone` from older peers) or
    /// `tablet`; a computer, a TV or anything else gets no row.
    pub fn of(kind: &str) -> Option<Self> {
        match kind {
            "phone" | "smartphone" => Some(Self::Phone),
            "tablet" => Some(Self::Tablet),
            _ => None,
        }
    }
}

/// A device's battery, as its plugin last said it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// Percent.
    pub charge: u8,
    pub charging: bool,
}

/// A paired phone or tablet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// KDE Connect's id for it: its object's last part.
    pub id: String,
    /// As the phone calls itself (the user may rename it in KDE Connect).
    pub name: String,
    pub kind: Kind,
    pub reachable: bool,
    /// While in reach and its battery plugin says one.
    pub battery: Option<Battery>,
    /// Its plugins loaded now (they are only while it is in reach).
    pub ring: bool,
    pub share: bool,
    /// When it came in reach last, in the order devices came (0: never
    /// seen in reach since KDE Connect was read).
    pub came: u64,
    /// The transfer to it under way, or one stopped and not yet let go.
    pub transfer: Option<Transfer>,
}

impl Device {
    /// Its battery at [`LOW`] or under, not charging.
    pub fn low(&self) -> bool {
        self.battery
            .is_some_and(|b| b.charge <= LOW && !b.charging && self.reachable)
    }
}

/// A transfer KDE Connect reports to the job view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    /// Its view's number, which its ✕ cancels it by.
    pub view: u32,
    /// The file's name (the one being sent, of several).
    pub file: String,
    /// How many files it sends.
    pub files: u32,
    /// Bytes.
    pub processed: u64,
    pub total: u64,
    /// Stopped before its end: said until the popover closes.
    pub failed: bool,
}

impl Transfer {
    /// How far it is, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.processed as f64 / self.total as f64).clamp(0.0, 1.0) as f32
    }
}

/// KDE Connect as the bar shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Phones {
    /// KDE Connect's daemon is on the session bus.
    pub present: bool,
    /// A desktop portal that chooses files is there: "Envoyer un fichier…".
    pub portal: bool,
    /// The rows, in their order ([`rows`]).
    pub devices: Vec<Device>,
}

impl Phones {
    pub fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }
}

/// What the worker says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum News {
    /// KDE Connect as it is now.
    Phones(Phones),
    /// Device `device` sent something (its share plugin's
    /// `shareReceived`): a file KDE Connect saved, or a link it opened.
    Received {
        device: String,
        kind: Kind,
        url: String,
    },
}

/// The rows: phones and tablets only, four at most (past four, those in
/// reach last), those in reach first, the one that came in reach last
/// first, then by name.
pub fn rows(mut all: Vec<Device>) -> Vec<Device> {
    all.sort_by(|a, b| b.came.cmp(&a.came).then_with(|| a.name.cmp(&b.name)));
    all.truncate(MOST);
    all.sort_by(|a, b| {
        b.reachable
            .cmp(&a.reachable)
            .then_with(|| b.came.cmp(&a.came))
            .then_with(|| a.name.cmp(&b.name))
    });
    all
}

/// A property of `props`.
pub fn get<T>(props: &Props, name: &str) -> Option<T>
where
    T: for<'a> TryFrom<&'a Variant<'a>>,
    for<'a> <T as TryFrom<&'a Variant<'a>>>::Error: Into<zbus::Error>,
{
    props.get(name)?.downcast_ref::<T>().ok()
}

/// An integer property, whatever its width.
pub fn number(props: &Props, name: &str) -> Option<i64> {
    number_of(props.get(name)?)
}

fn number_of(value: &Variant<'_>) -> Option<i64> {
    match value {
        Variant::U8(n) => Some(i64::from(*n)),
        Variant::I16(n) => Some(i64::from(*n)),
        Variant::U16(n) => Some(i64::from(*n)),
        Variant::I32(n) => Some(i64::from(*n)),
        Variant::U32(n) => Some(i64::from(*n)),
        Variant::I64(n) => Some(*n),
        Variant::U64(n) => i64::try_from(*n).ok(),
        Variant::Variant(inner) => number_of(inner),
        _ => None,
    }
}

/// `text` without its markup: what is between `<` and `>` taken out, the
/// usual entities read, its ends trimmed. KDE Connect's conversations
/// come as HTML; a notification's server shows them as text.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut tag = false;
    for c in text.chars() {
        match (tag, c) {
            (false, '<') => tag = true,
            (true, '>') => tag = false,
            (false, c) => out.push(c),
            (true, _) => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, reachable: bool, came: u64) -> Device {
        Device {
            id: name.to_lowercase(),
            name: name.to_owned(),
            kind: Kind::Phone,
            reachable,
            battery: None,
            ring: reachable,
            share: reachable,
            came,
            transfer: None,
        }
    }

    #[test]
    fn those_in_reach_come_first_the_last_come_first_then_by_name() {
        let shown = rows(vec![
            device("Tab", false, 1),
            device("Pixel", true, 2),
            device("Old", true, 0),
            device("New", true, 5),
        ]);
        let names: Vec<&str> = shown.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["New", "Pixel", "Old", "Tab"]);
    }

    #[test]
    fn past_four_those_seen_in_reach_last() {
        let shown = rows(vec![
            device("A", false, 1),
            device("B", false, 2),
            device("C", false, 3),
            device("D", false, 4),
            device("E", true, 5),
        ]);
        let names: Vec<&str> = shown.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["E", "D", "C", "B"]);
    }

    #[test]
    fn phones_and_tablets_only() {
        assert_eq!(Kind::of("phone"), Some(Kind::Phone));
        assert_eq!(Kind::of("smartphone"), Some(Kind::Phone));
        assert_eq!(Kind::of("tablet"), Some(Kind::Tablet));
        for other in ["laptop", "desktop", "tv", "unknown", ""] {
            assert_eq!(Kind::of(other), None, "{other}");
        }
    }

    #[test]
    fn low_is_at_fifteen_or_under_and_not_charging() {
        let mut d = device("Pixel", true, 1);
        d.battery = Some(Battery {
            charge: 15,
            charging: false,
        });
        assert!(d.low());
        d.battery = Some(Battery {
            charge: 12,
            charging: true,
        });
        assert!(!d.low());
        d.battery = Some(Battery {
            charge: 16,
            charging: false,
        });
        assert!(!d.low());
    }

    #[test]
    fn a_conversations_markup_reads_as_its_text() {
        assert_eq!(
            plain("<b>Camille</b>\nOn se voit à 15 h ?<br/>Tu &amp; moi"),
            "Camille On se voit à 15 h ?Tu & moi"
        );
        assert_eq!(plain("  Oui  "), "Oui");
    }

    #[test]
    fn a_transfers_fraction() {
        let t = Transfer {
            view: 1,
            file: "a".into(),
            files: 1,
            processed: 42,
            total: 100,
            failed: false,
        };
        assert!((t.fraction() - 0.42).abs() < 1e-6);
        let none = Transfer { total: 0, ..t };
        assert!(none.fraction().abs() < f32::EPSILON);
    }
}
