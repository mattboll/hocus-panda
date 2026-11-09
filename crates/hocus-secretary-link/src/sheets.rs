//! The consent sheets' words (§10.1): what she reads, what she never does
//! there, what leaves the machine, for each kind of a Google account. The
//! Settings show them on their sheet, she shows them on her card in the
//! drawer's first run: the same words in both places, in French and
//! English.

use serde::{Deserialize, Serialize};

use crate::{Notebook, consent::SourceKind};

/// A consent sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sheet {
    /// "Lire tes mails ?"
    pub title: String,
    /// Where it comes from: "Gmail · alex@atelier-lune.fr, déjà dans
    /// Comptes en ligne".
    pub from: String,
    pub sections: Vec<Section>,
    /// The notebook suggested (§7.4), which the user confirms or changes
    /// before "Autoriser".
    pub notebook: Notebook,
    /// "C'est ton compte perso ?", when perso is suggested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<String>,
}

/// One part of a sheet: its spaced label and its lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub label: String,
    pub mark: Mark,
    pub lines: Vec<String>,
}

/// How a section's lines are marked: ✓ what she does, ✕ what she never
/// does, nothing for what leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mark {
    Yes,
    No,
    Plain,
}

/// The three labels, in order.
pub fn labels(fr: bool) -> [&'static str; 3] {
    if fr {
        [
            "CE QUE JE LIS",
            "CE QUE JE N’Y FAIS JAMAIS",
            "CE QUI QUITTE CET ORDINATEUR",
        ]
    } else {
        [
            "WHAT I READ",
            "WHAT I NEVER DO THERE",
            "WHAT LEAVES THIS COMPUTER",
        ]
    }
}

/// A kind's name, short: "Agenda", "Mail", "Anniversaires".
pub fn kind_name(kind: SourceKind, fr: bool) -> &'static str {
    match (kind, fr) {
        (SourceKind::Calendar, true) => "Agenda",
        (SourceKind::Calendar, false) => "Calendar",
        (SourceKind::Mail, _) => "Gmail",
        (SourceKind::Birthdays, true) => "Anniversaires",
        (SourceKind::Birthdays, false) => "Birthdays",
        (SourceKind::Notifications, _) => "Notifications",
    }
}

/// The sheet of `identity`'s `kind` (a Google account of Online
/// Accounts), `suggested` its notebook.
pub fn google(identity: &str, kind: SourceKind, suggested: Notebook, fr: bool) -> Sheet {
    let lines = |list: &[&str]| list.iter().map(|l| (*l).to_owned()).collect::<Vec<_>>();
    let (title, service, reads, never, leaves): (
        &str,
        &str,
        Vec<String>,
        Vec<String>,
        Vec<String>,
    ) = match (kind, fr) {
        (SourceKind::Mail, true) => (
            "Lire tes mails ?",
            "Gmail",
            lines(&[
                "Ta boîte de réception et tes envois, 30 jours en arrière",
                "Le texte des mails ; des pièces jointes, seulement leur nom",
            ]),
            lines(&[
                "Marquer un mail comme lu : j’ouvre ta boîte en lecture seule",
                "Envoyer, supprimer, archiver, déplacer",
                "Obéir à ce qu’un mail demande : je te le signale",
            ]),
            lines(&[
                "Les extraits utiles à une réponse ou à ton point, vers Claude (Anthropic) ; jamais les pièces jointes",
                "Les mots de passe, jetons et codes sont masqués avant que quiconque lise, moi comprise",
                "Le tri «\u{a0}attend une réponse\u{a0}?\u{a0}» se fait ici, sur cet ordinateur",
            ]),
        ),
        (SourceKind::Mail, false) => (
            "Read your mail?",
            "Gmail",
            lines(&[
                "Your inbox and what you sent, 30 days back",
                "The mails’ text; of attachments, only their names",
            ]),
            lines(&[
                "Mark a mail read: I open your mailbox read-only",
                "Send, delete, archive, move",
                "Do what a mail asks: I tell you about it",
            ]),
            lines(&[
                "The parts useful to an answer or your briefing, to Claude (Anthropic); never attachments",
                "Passwords, tokens and codes are masked before anyone reads, me included",
                "The “does it need an answer?” triage runs here, on this computer",
            ]),
        ),
        (SourceKind::Calendar, true) => (
            "Lire ton agenda ?",
            "Google Agenda",
            lines(&[
                "Tous tes agendas, y compris ceux que tu as masqués (congés, famille)",
                "Les événements, qui y est invité, et leur description",
            ]),
            lines(&[
                "Répondre à une invitation sans ton clic",
                "Créer, déplacer ou supprimer un événement",
                "Obéir à ce qu’une description demande : je te le signale",
            ]),
            lines(&[
                "Les événements utiles à une réponse ou à ton point, vers Claude (Anthropic)",
                "Les codes et mots de passe d’une description sont masqués avant",
                "Les jours fériés et les créneaux libres se calculent ici",
            ]),
        ),
        (SourceKind::Calendar, false) => (
            "Read your calendar?",
            "Google Calendar",
            lines(&[
                "All your calendars, hidden ones included (leaves, family)",
                "The events, who is invited, and their descriptions",
            ]),
            lines(&[
                "Answer an invitation without your click",
                "Create, move or delete an event",
                "Do what a description asks: I tell you about it",
            ]),
            lines(&[
                "The events useful to an answer or your briefing, to Claude (Anthropic)",
                "Codes and passwords in a description are masked first",
                "Public holidays and free slots are computed here",
            ]),
        ),
        (_, true) => (
            "Lire les anniversaires de tes contacts ?",
            "Contacts Google",
            lines(&[
                "Les dates d’anniversaire et de fête de tes contacts",
                "Rien d’autre de leurs fiches : ni numéros, ni adresses, ni notes",
            ]),
            lines(&[
                "Modifier ou supprimer un contact",
                "Écrire à quelqu’un sans ton clic",
            ]),
            lines(&[
                "Un nom et une date, quand ils servent à ton point («\u{a0}Claire a 40 ans samedi\u{a0}»)",
                "Les âges se calculent ici",
            ]),
        ),
        (_, false) => (
            "Read your contacts’ birthdays?",
            "Google Contacts",
            lines(&[
                "Your contacts’ birthdays and anniversaries",
                "Nothing else of their cards: no numbers, addresses or notes",
            ]),
            lines(&[
                "Change or delete a contact",
                "Write to anyone without your click",
            ]),
            lines(&[
                "A name and a date, when your briefing needs them (“Claire turns 40 on Saturday”)",
                "Ages are computed here",
            ]),
        ),
    };
    let [read_label, never_label, leave_label] = labels(fr);
    let from = if fr {
        format!("{service} · {identity}, déjà dans Comptes en ligne")
    } else {
        format!("{service} · {identity}, already in Online Accounts")
    };
    Sheet {
        title: title.to_owned(),
        from,
        sections: vec![
            Section {
                label: read_label.to_owned(),
                mark: Mark::Yes,
                lines: reads,
            },
            Section {
                label: never_label.to_owned(),
                mark: Mark::No,
                lines: never,
            },
            Section {
                label: leave_label.to_owned(),
                mark: Mark::Plain,
                lines: leaves,
            },
        ],
        notebook: suggested,
        ask: (suggested == Notebook::Perso).then(|| {
            if fr {
                "C’est ton compte perso ?".to_owned()
            } else {
                "Is it your personal account?".to_owned()
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_says_what_she_reads_never_does_and_what_leaves_in_both_languages() {
        for kind in SourceKind::ALL {
            for fr in [true, false] {
                let sheet = google("alex@atelier-lune.fr", kind, Notebook::Pro, fr);
                assert_eq!(sheet.sections.len(), 3);
                assert!(sheet.sections.iter().all(|s| !s.lines.is_empty()));
                assert!(sheet.from.contains("alex@atelier-lune.fr"));
                assert_eq!(sheet.ask, None);
            }
        }
        let mail = google(
            "alex.martin@gmail.com",
            SourceKind::Mail,
            Notebook::Perso,
            true,
        );
        assert_eq!(mail.ask.as_deref(), Some("C’est ton compte perso ?"));
        assert!(mail.sections[1].lines[0].contains("lecture seule"));
        assert_eq!(mail.sections[1].mark, Mark::No);
    }
}
