//! The consent sheets' words (§10.1): what she reads, what she never does
//! there, what leaves the machine, for each kind of a Google account. The
//! Settings show them on their sheet, she shows them on her card in the
//! drawer's first run: the same words in both places, in French and
//! English.

use serde::{Deserialize, Serialize};

use crate::{Notebook, consent::SourceKind, gate::Hours};

/// A consent sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sheet {
    /// "Lire tes mails ?"
    pub title: String,
    /// Where it comes from: "Gmail · alex@atelier-lune.fr, déjà connecté
    /// à cet ordinateur".
    pub from: String,
    pub sections: Vec<Section>,
    /// The notebook suggested (§7.4), which the user confirms or changes
    /// before "Autoriser": « C'est privé » checked for the perso one.
    pub notebook: Notebook,
    /// « C'est privé » and what it does (#752).
    #[serde(default)]
    pub private: Private,
    /// An Agenda's: the calendars she will read, each to be left out
    /// (#752); empty before they are known.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calendars: Vec<crate::calendars::CalendarInfo>,
    /// An account's card (#758): what she may read of it, each a switch;
    /// her question over them, her promise under them, the link that
    /// unfolds the sections, and the account she asks about next.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<Read>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ask: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub promise: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub more: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub then: Option<Then>,
}

/// One thing an account's card asks to read (#758), a switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Read {
    pub kind: SourceKind,
    /// "Ton agenda", and its line: "Tous tes agendas, ceux qu'on te
    /// partage compris".
    pub title: String,
    pub line: String,
    /// On before the user touches it: the calendar and the mail.
    pub on: bool,
}

/// The account she asks about after this one (#758), dashed under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Then {
    pub identity: String,
    /// « ensuite · je te propose « Privé » ».
    pub note: String,
}

/// The calendars' label on an Agenda's sheet.
pub fn calendars_label(fr: bool) -> &'static str {
    if fr {
        "LES AGENDAS QU’ELLE LIRA"
    } else {
        "THE CALENDARS SHE WILL READ"
    }
}

/// « C'est privé »: its box's words, and what checking it does, as the
/// code does it (the shell's speak gate, her quiet hours, her work drafts,
/// "Oublier tout le privé").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Private {
    pub label: String,
    pub lines: Vec<String>,
    /// The choice in two words (#758), « Travail » and « Privé », and
    /// what « Privé » changes, in one line.
    #[serde(default)]
    pub choice: [String; 2],
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub short: String,
}

/// « C'est privé » and its consequences; `quiet`, the private notebook's
/// quiet hours (`[secretary.speak] quiet-perso`).
pub fn private(fr: bool, quiet: Hours) -> Private {
    let hour = |t: jiff::civil::Time| {
        if fr {
            if t.minute() == 0 {
                format!("{} h", t.hour())
            } else {
                format!("{} h {:02}", t.hour(), t.minute())
            }
        } else {
            format!("{:02}:{:02}", t.hour(), t.minute())
        }
    };
    let (from, to) = (hour(quiet.from), hour(quiet.to));
    let (choice, short) = if fr {
        (
            ["Travail".to_owned(), "Privé".to_owned()],
            "Privé : je n’en parle jamais pendant une réunion ou un partage d’écran, ni dans ce que je prépare pour le travail.".to_owned(),
        )
    } else {
        (
            ["Work".to_owned(), "Private".to_owned()],
            "Private: I never bring it up during a meeting or a screen share, nor in what I draft for work.".to_owned(),
        )
    };
    if fr {
        Private {
            choice,
            short,
            label: "C’est privé".to_owned(),
            lines: vec![
                "Jamais montré pendant une réunion, un partage d’écran ou un appel.".to_owned(),
                "Jamais dans un message ou une tâche qu’elle prépare pour le travail.".to_owned(),
                format!(
                    "Elle t’en parle aussi le soir et le week-end, jamais entre {from} et {to}."
                ),
                "Ce qu’elle sait › « Oublier tout le privé » l’efface d’un coup.".to_owned(),
            ],
        }
    } else {
        Private {
            choice,
            short,
            label: "It’s private".to_owned(),
            lines: vec![
                "Never shown during a meeting, a screen share or a call.".to_owned(),
                "Never in a message or a task she drafts for work.".to_owned(),
                format!(
                    "She brings it up evenings and weekends too, never between {from} and {to}."
                ),
                "What she knows › “Forget everything private” erases it at once.".to_owned(),
            ],
        }
    }
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
/// Accounts), `suggested` its notebook; `quiet`, the private notebook's
/// quiet hours.
pub fn google(
    identity: &str,
    kind: SourceKind,
    suggested: Notebook,
    quiet: Hours,
    fr: bool,
) -> Sheet {
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
                "Tous tes agendas, ceux que tu as masqués et ceux qu’on te partage compris, sauf ceux que tu laisses de côté",
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
                "All your calendars, hidden and shared ones included, but those you leave out",
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
        format!("{service} · {identity}, déjà connecté à cet ordinateur")
    } else {
        format!("{service} · {identity}, already signed in on this computer")
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
        private: private(fr, quiet),
        calendars: Vec::new(),
        reads: Vec::new(),
        ask: String::new(),
        promise: String::new(),
        more: String::new(),
        then: None,
    }
}

/// An account's card in her drawer (#758): one card for all `kinds` of
/// `identity`, a switch each (the calendar and the mail on, the
/// birthdays off), « Travail | Privé », and the three sections of each
/// kind's sheet folded under « Ce que je lis exactement… ».
pub fn account(
    identity: &str,
    kinds: &[SourceKind],
    suggested: Notebook,
    quiet: Hours,
    fr: bool,
) -> Sheet {
    let mut sections: Vec<Section> = Vec::new();
    let mut reads = Vec::new();
    // The calendar, the mail, the birthdays: the order of the card.
    let mut kinds = kinds.to_vec();
    kinds.sort_by_key(|k| match k {
        SourceKind::Calendar => 0,
        SourceKind::Mail => 1,
        SourceKind::Birthdays => 2,
        SourceKind::Notifications => 3,
    });
    for kind in &kinds {
        let sheet = google(identity, *kind, suggested, quiet, fr);
        let name = kind_name(*kind, fr);
        for (n, section) in sheet.sections.into_iter().enumerate() {
            let lines = section.lines.into_iter().map(|l| format!("{name} : {l}"));
            match sections.get_mut(n) {
                Some(s) => s.lines.extend(lines),
                None => sections.push(Section {
                    lines: lines.collect(),
                    ..section
                }),
            }
        }
        let (title, line, on) = match (kind, fr) {
            (SourceKind::Calendar, true) => (
                "Ton agenda",
                "Tous tes agendas, ceux qu’on te partage compris",
                true,
            ),
            (SourceKind::Calendar, false) => (
                "Your calendar",
                "All your calendars, shared ones included",
                true,
            ),
            (SourceKind::Mail, true) => {
                ("Tes mails", "Les 30 derniers jours, reçus et envoyés", true)
            }
            (SourceKind::Mail, false) => ("Your mail", "The last 30 days, received and sent", true),
            (_, true) => (
                "Les anniversaires de tes contacts",
                "Le nom et la date, rien d’autre de leur fiche",
                false,
            ),
            (_, false) => (
                "Your contacts’ birthdays",
                "The name and the date, nothing else of their card",
                false,
            ),
        };
        reads.push(Read {
            kind: *kind,
            title: title.to_owned(),
            line: line.to_owned(),
            on,
        });
    }
    let t = |f: &str, e: &str| if fr { f.to_owned() } else { e.to_owned() };
    Sheet {
        title: identity.to_owned(),
        from: t(
            "déjà connecté à cet ordinateur",
            "already signed in on this computer",
        ),
        sections,
        notebook: suggested,
        private: private(fr, quiet),
        calendars: Vec::new(),
        reads,
        ask: t("Qu’est-ce que je peux lire ?", "What may I read?"),
        promise: t(
            "Je ne marque rien comme lu, je n’envoie rien, je ne réponds à aucune invitation sans ton clic.",
            "I mark nothing read, send nothing, answer no invitation without your click.",
        ),
        more: t(
            "Ce que je lis exactement, et ce qui part chez Anthropic ›",
            "What I read exactly, and what goes to Anthropic ›",
        ),
        then: None,
    }
}

/// "Autoriser l'agenda et les mails": what an account's card allows,
/// from its switches on (#758).
pub fn allow_label(on: &[SourceKind], fr: bool) -> String {
    let word = |kind: &SourceKind| match (kind, fr) {
        (SourceKind::Calendar, true) => "l’agenda",
        (SourceKind::Calendar, false) => "the calendar",
        (SourceKind::Mail, true) => "les mails",
        (SourceKind::Mail, false) => "the mail",
        (SourceKind::Birthdays, true) => "les anniversaires",
        (SourceKind::Birthdays, false) => "the birthdays",
        (SourceKind::Notifications, true) => "les notifications",
        (SourceKind::Notifications, false) => "the notifications",
    };
    let words: Vec<&str> = on.iter().map(word).collect();
    let and = if fr { " et " } else { " and " };
    let list = match words.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{}{and}{last}", rest.join(", ")),
    };
    match (list.is_empty(), fr) {
        (true, true) => "Ne rien lire".to_owned(),
        (true, false) => "Read nothing".to_owned(),
        (false, true) => format!("Autoriser {list}"),
        (false, false) => format!("Allow {list}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_says_what_she_reads_never_does_and_what_leaves_in_both_languages() {
        let quiet = crate::gate::Restraint::default().perso;
        for kind in SourceKind::ALL {
            for fr in [true, false] {
                let sheet = google("alex@atelier-lune.fr", kind, Notebook::Pro, quiet, fr);
                assert_eq!(sheet.sections.len(), 3);
                assert!(sheet.sections.iter().all(|s| !s.lines.is_empty()));
                assert!(sheet.from.contains("alex@atelier-lune.fr"));
                assert_eq!(sheet.private.lines.len(), 4);
            }
        }
        // One card per account (#758): a switch per kind, the calendar and
        // the mail on; the button says what it allows.
        let card = account(
            "alex@atelier-lune.fr",
            &SourceKind::ALL,
            Notebook::Pro,
            quiet,
            true,
        );
        assert_eq!(card.reads.len(), 3);
        let on: Vec<SourceKind> = card.reads.iter().filter(|r| r.on).map(|r| r.kind).collect();
        assert_eq!(allow_label(&on, true), "Autoriser l’agenda et les mails");
        assert_eq!(card.sections.len(), 3);
        assert!(!card.from.contains("Comptes en ligne"));
        assert_eq!(
            card.private.choice,
            ["Travail".to_owned(), "Privé".to_owned()]
        );
        let mail = google(
            "alex.martin@gmail.com",
            SourceKind::Mail,
            Notebook::Perso,
            quiet,
            true,
        );
        assert_eq!(mail.notebook, Notebook::Perso, "checked");
        assert_eq!(mail.private.label, "C’est privé");
        assert!(mail.sections[1].lines[0].contains("lecture seule"));
        assert_eq!(mail.sections[1].mark, Mark::No);
    }

    /// What « C'est privé » says is what the code does: the speak gate
    /// holds a private item in a meeting, a share or a call; work drafts
    /// take the work notebook only; its quiet hours are the private
    /// notebook's, whatever the file says.
    #[test]
    fn c_est_prive_says_what_the_code_does() {
        use crate::gate::{Gate, Restraint, Screen, Verdict, Why};
        let restraint = Restraint::default();
        let fr = private(true, restraint.perso);
        assert_eq!(
            fr.lines[2],
            "Elle t’en parle aussi le soir et le week-end, jamais entre 22 h et 8 h."
        );
        let late = private(true, Hours::new((21, 30), (7, 0)));
        assert!(late.lines[2].contains("entre 21 h 30 et 7 h"));
        let en = private(false, restraint.perso);
        assert!(en.lines[2].contains("between 22:00 and 08:00"));
        // A private item in a call is held; quiet at 23:00, not at 20:00
        // nor on a Saturday afternoon.
        let mut gate = Gate::new(restraint);
        gate.screen = Screen {
            camera: true,
            ..Screen::default()
        };
        let now: jiff::Timestamp = "2026-11-10T14:00:00Z".parse().expect("a time");
        let local = |t: &str| t.parse::<jiff::civil::DateTime>().expect("a time");
        let speak = crate::Speak {
            id: 1,
            kind: crate::SpeakKind::Proposal,
            text: "Claire : dimanche ?".to_owned(),
            action: None,
            item: None,
            notebook: Notebook::Perso,
            vip: false,
            deadline: None,
        };
        assert_eq!(
            gate.ask(speak, now, local("2026-11-10T14:00")),
            Verdict::Held(Why::Camera)
        );
        assert!(restraint.quiet(Notebook::Perso, local("2026-11-10T23:00")));
        assert!(!restraint.quiet(Notebook::Perso, local("2026-11-10T20:00")));
        assert!(!restraint.quiet(Notebook::Perso, local("2026-11-14T15:00")));
    }
}
