//! "Ce qu'elle sait" (docs/design/secretary.md §8, lot 6): the drawer's
//! second tab asks her what she knows ([`Query`]), and she answers with
//! the view whole ([`Knowledge`]): sections of items, each with where she
//! has it from ("d'où je le sais") and what can be done with it. The view
//! is made by her, from her memory; the shell only draws it.
//!
//! While the tab is open she sends it again when her memory changes (her
//! answer to the last [`crate::ToSecretary::Knows`], its id repeated);
//! [`crate::ToSecretary::Knows`] without a query stops it. Nothing is sent
//! at rest.
//!
//! What the user does with an item is a [`Act`] on it: forget it (with
//! five seconds to take it back, [`crate::FromSecretary::Toast`]), close it
//! ("N'est plus vrai"), keep it, accept or reject a proposal, put an
//! observation off, forget the perso notebook.

use serde::{Deserialize, Serialize};

use crate::{Notebook, Provenance};

/// What the tab asks: one notebook or both, one kind of item or all, the
/// words searched.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    pub notebook: Option<Notebook>,
    pub kind: Option<Kind>,
    pub search: String,
}

/// The filter chips (§8): what an item is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    People,
    Projects,
    Rules,
    Tools,
    /// « Ma façon d'être »: what the user asked her to be (#753, a chip
    /// of its own since #758).
    Manner,
    You,
    Maintenance,
    PersoFiles,
}

impl Kind {
    pub const ALL: [Self; 8] = [
        Self::People,
        Self::Projects,
        Self::Rules,
        Self::Tools,
        Self::Manner,
        Self::You,
        Self::Maintenance,
        Self::PersoFiles,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Self::People => "people",
            Self::Projects => "projects",
            Self::Rules => "rules",
            Self::Tools => "tools",
            Self::Manner => "manner",
            Self::You => "you",
            Self::Maintenance => "maintenance",
            Self::PersoFiles => "perso-files",
        }
    }
}

/// Her memory as the tab shows it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Knowledge {
    /// The query it answers.
    pub query: Query,
    /// The chips, each with its count: none chosen shows everything
    /// (#758: no « Tout »).
    pub chips: Vec<Chip>,
    pub sections: Vec<Section>,
    /// How many things she knows in all ("412 souvenirs").
    pub total: u64,
    /// Perso items left out in a public moment ("2 choses perso
    /// masquées", §7.4).
    pub hidden: u32,
}

impl Knowledge {
    /// Every item, in the order shown.
    pub fn items(&self) -> impl Iterator<Item = &Known> {
        self.sections.iter().flat_map(|s| s.items.iter())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chip {
    pub kind: Option<Kind>,
    pub label: String,
    pub count: u32,
}

/// A section: À CONFIRMER, APPRIS CETTE SEMAINE, PERSONNES…
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Section {
    /// `confirm`, `week`, `people`, `projects`, `rules`, `tools`,
    /// `style`, `you`, `close`.
    pub key: String,
    /// Its spaced title, upper case.
    pub title: String,
    /// What follows it: "4 · à garder ou oublier".
    pub note: String,
    pub items: Vec<Known>,
}

/// One thing she knows.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Known {
    /// What it is in her memory (`node:3`, `edge:12`, `rule:4`,
    /// `alias:7`, `seen:2` for an observation): what acts name.
    pub item: String,
    /// Its bold part ("Nadia Ferreira"), and the rest of its line.
    pub title: String,
    pub text: String,
    /// Small tags after the line: aliases, "nouvelle", "en congés…".
    pub tags: Vec<Tag>,
    /// A person's history: arrival, changes, departure.
    pub timeline: Vec<Step>,
    /// Smaller lines under it: a project's links, a tool's rules.
    pub lines: Vec<String>,
    /// "D'où je le sais".
    pub origin: Origin,
    pub notebook: Option<Notebook>,
    pub actions: Vec<Act>,
    /// Gone (a departed person): dashed, "gardé pour l'histoire".
    pub left: bool,
    /// A proposal: it drives nothing until accepted.
    pub proposal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub text: String,
    pub tone: Tone,
}

/// A tag's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone {
    Plain,
    /// Blue: new this week.
    New,
    /// Amber: on leave, to confirm.
    Warn,
    /// Rose: perso (§7.4).
    Perso,
}

/// A step of a person's timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub text: String,
    pub tone: StepTone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepTone {
    /// Green: a fact of its history.
    Done,
    /// Amber: new.
    New,
    /// Grey: the end.
    Ended,
}

/// Where she has it from (§7.1, §8).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Origin {
    pub provenance: Option<Provenance>,
    /// "tu me l'as dit · 9 nov.", "lu dans le mail de Léa, 14 juin",
    /// "déduit de CHAP-412, accepté le 10 nov.", "à confirmer".
    pub text: String,
    /// What opens its source item (an extracted fact's), for the user's
    /// click only; and the quoted words.
    pub open: Option<String>,
    pub quote: Option<String>,
}

/// What can be done with an item (§8), and what the footer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Act {
    /// "Modifier": tell her, in the conversation.
    Edit,
    /// "N'est plus vrai": closed at today, kept.
    Close,
    /// "Oublier": gone everywhere, with five seconds to take it back.
    Forget,
    /// "Garder": seen in the week's digest, kept.
    Keep,
    /// "C'est vrai", "Oui": a proposal accepted.
    Accept,
    /// "Non", "Ce n'est pas ça": a proposal rejected.
    Reject,
    /// "Plus tard": asked again in a week.
    Later,
}

/// What the user does in the tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Do {
    /// `act` on item `item` (a [`Known::item`]).
    Item { item: String, act: Act },
    /// "Annuler" in the toast: what a forget took back, by its token.
    Restore { token: u64 },
    /// "Oublier tout le perso", confirmed (§7.4): the other notebook stays.
    ForgetNotebook { notebook: Notebook },
    /// "Exporter": Markdown and JSON, of the notebook shown or both.
    Export { notebook: Option<Notebook> },
}
