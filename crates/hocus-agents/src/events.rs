//! A subscription to the desktop's events (docs/api.md, "Events"): the
//! line `msg subscribe KINDS`, `KINDS` the kinds wanted, comma-separated
//! (`window,desktop,screen,agent`), all of them when none is said. The
//! connection then stays open, and the desktop writes one JSON object a
//! line on it for each event of those kinds.

use std::fmt;

/// What a subscription line starts with.
const OPENING: &str = "msg subscribe";

/// A kind of event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// A window opened, closed, or got the keyboard.
    Window,
    /// A screen showing another of its desktops.
    Desktop,
    /// A screen plugged or unplugged.
    Screen,
    /// An agent's state.
    Agent,
}

impl Kind {
    pub const ALL: [Self; 4] = [Self::Window, Self::Desktop, Self::Screen, Self::Agent];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Desktop => "desktop",
            Self::Screen => "screen",
            Self::Agent => "agent",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.word() == word)
    }
}

/// The kinds a subscriber wants, each once, in [`Kind::ALL`]'s order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    kinds: Vec<Kind>,
}

impl Subscription {
    /// Every kind.
    pub fn all() -> Self {
        Self {
            kinds: Kind::ALL.to_vec(),
        }
    }

    /// These kinds; every kind when none.
    pub fn of_kinds(kinds: &[Kind]) -> Self {
        if kinds.is_empty() {
            return Self::all();
        }
        let mut kinds = kinds.to_vec();
        kinds.sort_unstable();
        kinds.dedup();
        Self { kinds }
    }

    pub fn wants(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }

    pub fn kinds(&self) -> &[Kind] {
        &self.kinds
    }

    /// Its line, without the line break.
    pub fn line(&self) -> String {
        format!("{OPENING} {self}")
    }

    /// What a line subscribes to, if it is a subscription: the kinds, or
    /// why it is refused (a kind that is not one).
    pub fn read(line: &str) -> Option<Result<Self, String>> {
        let rest = line.trim_end_matches(['\n', '\r']).strip_prefix(OPENING)?;
        let words = match rest {
            "" => "",
            rest => rest.strip_prefix(' ')?.trim(),
        };
        let mut kinds = Vec::new();
        for word in words.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            match Kind::of(word) {
                Some(kind) => kinds.push(kind),
                None => {
                    let known: Vec<&str> = Kind::ALL.iter().map(|k| k.word()).collect();
                    return Some(Err(format!("no events {word:?}: {}", known.join(", "))));
                }
            }
        }
        Some(Ok(Self::of_kinds(&kinds)))
    }
}

/// The kinds, comma-separated.
impl fmt::Display for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let words: Vec<&str> = self.kinds.iter().map(|k| k.word()).collect();
        f.write_str(&words.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subscription_reads_back_and_refuses_an_unknown_kind() {
        let some = Subscription::of_kinds(&[Kind::Agent, Kind::Window, Kind::Agent]);
        assert_eq!(some.line(), "msg subscribe window,agent");
        assert_eq!(Subscription::read(&some.line()), Some(Ok(some.clone())));
        assert_eq!(
            Subscription::read("msg subscribe\n"),
            Some(Ok(Subscription::all()))
        );
        assert_eq!(
            Subscription::read("msg subscribe window, screen"),
            Some(Ok(Subscription::of_kinds(&[Kind::Window, Kind::Screen])))
        );
        assert!(matches!(
            Subscription::read("msg subscribe window,mouse"),
            Some(Err(why)) if why.contains("\"mouse\"")
        ));
        assert_eq!(Subscription::read("msg subscribers"), None);
        assert_eq!(Subscription::read("msg idle"), None);
        assert!(crate::Question::of(&some.line()).is_none());
    }
}
