//! Her triage queue shown (#754): the drawer asks for it
//! ([`crate::ToSecretary::Queue`]) when her status line is clicked, and
//! she answers with the view whole ([`View`]), again while it changes and
//! the view is open; never at rest, never once it is closed.
//!
//! The view says what the local model reads now, the rest grouped by
//! sender (most first), how fast it goes (measured) and how long is left,
//! and why it waits when it does. Of an item it never says more than its
//! sender, its subject and its date: what the drawer already shows of a
//! mail.
//!
//! On a sender's group, "Ne pas trier, chercher si besoin" ([`Act::Aside`])
//! takes its waiting items out of the queue, and its later ones: a rule
//! she keeps, listed in "Ce qu'elle sait", which [`Act::Back`] (or its
//! removal there) undoes.

use serde::{Deserialize, Serialize};

/// The queue as the drawer shows it. Its words are hers, in the
/// desktop's language.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    /// What she does now: "Je trie", "Je souffle entre deux", "En pause",
    /// "Rien à trier".
    pub state: String,
    /// The item the local model reads now.
    pub now: Option<Waiting>,
    /// Why it waits, when it does: "Le processeur chauffe : 101 °C en
    /// moyenne sur une minute, je reprends sous 90 °C".
    pub pause: Option<String>,
    /// What slows it, when its server says ("Le modèle tourne en partie
    /// sur le processeur : c'est plus lent").
    pub slow: Option<String>,
    /// The pace measured over the last items ("≈ 39 s par élément"), and
    /// in seconds; empty before one is measured.
    pub rate: String,
    pub per_item: Option<u64>,
    /// The time left at that pace ("encore ≈ 10 h 41"), and in seconds.
    pub left: String,
    pub left_seconds: Option<u64>,
    /// "987 en attente · 312 triés · 412 mis de côté".
    pub counts: String,
    pub waiting: u64,
    pub done: u64,
    pub set_aside: u64,
    /// The local model ("local/qwen3:8b").
    pub model: String,
    /// What waits, by sender, the most first.
    pub groups: Vec<Group>,
    /// What is left of the groups past the most shown ("… 12 autres
    /// expéditeurs · 25 éléments").
    pub more: String,
    /// What is set aside, by sender, the most first.
    pub aside: Vec<Group>,
}

/// An item, as much as the drawer shows of one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Waiting {
    /// Its sender ("GitHub <notifications@github.com>"), an application's
    /// name for a notification, a calendar's for an event.
    pub sender: String,
    /// Its subject or title, through the first pass (masked).
    pub subject: String,
    /// Where it comes from ("Gmail · alex@atelier-lune.fr", "Slack").
    pub source: String,
    /// When, as the user reads it ("9 nov. 07:42").
    pub date: String,
}

/// A sender's items.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    /// What names it in an [`Act`]: the sender's address, lowercase
    /// (`notifications@github.com`); `app:…` or `calendar:…` otherwise.
    pub key: String,
    /// How it is shown ("notifications@github.com").
    pub label: String,
    pub count: u64,
    /// Why it looks automated, when it does ("List-Unsubscribe").
    pub automated: Option<String>,
    /// Its small line ("automatique · List-Unsubscribe", "Gmail").
    pub note: String,
    /// It can be set aside (a mail sender's).
    pub can_aside: bool,
}

/// What the user does with a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "act")]
pub enum Act {
    /// "Ne pas trier, chercher si besoin": a rule, its items out of the
    /// queue, kept with their sender, subject and date.
    Aside { key: String },
    /// "Trier quand même": the rule goes, its items come back.
    Back { key: String },
}
