//! The applications whose notifications the secretary may read
//! (docs/design/secretary.md §10.1, §20 lot 7), on the Agents page under
//! "LA SECRÉTAIRE" until her own page (lot 8): Slack through its
//! notifications, and any other application that notifies, each consented
//! to on its own, its items going to the notebook chosen then.
//!
//! What the page lists is found, not configured: the applications that
//! notified in the history the shell keeps (seven days), those already in
//! her consent book, and Slack when it is installed. Its "Autoriser"
//! writes a record in her consent book (her folder's `consents.json`),
//! which the shell reads before forwarding her a notification and she
//! reads as she takes it; "Ne plus lire" writes a revocation, and she
//! forgets what came from it. Applications are keyed by their names: a
//! notification names its application whether or not it names its desktop
//! entry.
//!
//! Read and written on a thread of the page's, never the window's.

use std::path::{Path, PathBuf};

use hocus_secretary_link::{
    Notebook,
    consent::{CONSENTS, ConsentBook, SourceKey, SourceKind},
};
use jiff::Timestamp;

use crate::lang::{Lang, Words, w};

/// Slack's key: the application the spec reads by its notifications by
/// default (§10.1), listed first.
const SLACK: &str = "slack";

/// An application whose notifications she may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notifier {
    /// As it names itself (`Slack`).
    pub name: String,
    /// Its key in the consent book.
    pub key: String,
    /// Read now, into this notebook.
    pub granted: Option<Notebook>,
}

impl Notifier {
    pub fn is_slack(&self) -> bool {
        self.key == SLACK
    }

    /// Its line under its name.
    pub fn line(&self, lang: Lang) -> String {
        match (self.granted, lang) {
            (Some(notebook), Lang::French) => format!(
                "Elle lit ses notifications, carnet {} · elle n’en ouvre ni n’en ferme aucune : rien n’est marqué lu",
                notebook.word()
            ),
            (Some(notebook), Lang::English) => format!(
                "She reads its notifications, {} notebook · she opens and closes none: nothing is marked read",
                notebook.word()
            ),
            (None, _) if self.is_slack() => w(
                "Ses notifications seulement : rien n’est marqué lu, rien ne sort de Slack",
                "Its notifications only: nothing is marked read, nothing leaves Slack",
            )
            .get(lang)
            .to_owned(),
            (None, _) => w(
                "Elle ne lit pas ses notifications",
                "She does not read its notifications",
            )
            .get(lang)
            .to_owned(),
        }
    }
}

/// The notebook proposed on the sheet: the work one (§7.4: Slack's is
/// pro); the user chooses another there.
pub fn proposed(_notifier: &Notifier) -> Notebook {
    Notebook::Pro
}

/// What the sheet says, for `name`: what she reads, what she never does
/// there, what leaves the machine (§10.1).
pub fn sheet(name: &str, slack: bool, lang: Lang) -> [(Words, String); 3] {
    let reads = match (slack, lang) {
        (true, Lang::French) => "Ce que Slack affiche dans ses notifications : qui écrit, où, et le message. Seulement ce que Slack notifie, souvent coupé ; rien quand Slack est fermé ; je ne vois pas tes réponses.".to_owned(),
        (true, Lang::English) => "What Slack shows in its notifications: who writes, where, and the message. Only what Slack notifies, often cut; nothing while Slack is closed; I do not see your replies.".to_owned(),
        (false, Lang::French) => format!("Ce que {name} affiche dans ses notifications : leur titre et leur texte."),
        (false, Lang::English) => format!("What {name} shows in its notifications: their title and their text."),
    };
    let never = match lang {
        Lang::French => format!(
            "Ouvrir, fermer ou répondre à une notification : rien n’est marqué lu dans {name}, rien n’y est envoyé. Obéir à ce qu’un message demande : je te le signale."
        ),
        Lang::English => format!(
            "Open, close or answer a notification: nothing is marked read in {name}, nothing is sent there. Do what a message asks: I tell you about it."
        ),
    };
    let leaves = w(
        "Les extraits utiles à une réponse ou à ton point, vers Claude (Anthropic), une fois les mots de passe, jetons et codes masqués ; le tri se fait ici.",
        "The parts useful to an answer or your briefing, to Claude (Anthropic), once passwords, tokens and codes are masked; the triage runs here.",
    )
    .get(lang)
    .to_owned();
    [
        (w("CE QUE JE LIS", "WHAT I READ"), reads),
        (
            w("CE QUE JE N’Y FAIS JAMAIS", "WHAT I NEVER DO THERE"),
            never,
        ),
        (
            w("CE QUI QUITTE CET ORDINATEUR", "WHAT LEAVES THIS COMPUTER"),
            leaves,
        ),
    ]
}

/// The applications found: the consent book's, the history's (`history`,
/// the shell's file: a JSON object a line; the agents' tools' own
/// notifications left out), and Slack when one of `installed` (desktop
/// entries' ids) is its. Slack first, then by name.
pub fn found(book: &ConsentBook, history: &str, installed: &[String]) -> Vec<Notifier> {
    let mut names: Vec<(String, String)> = Vec::new();
    let mut add = |name: &str| {
        let name = name.trim();
        let key = name.to_lowercase();
        if !name.is_empty() && !names.iter().any(|(k, _)| *k == key) {
            names.push((key, name.to_owned()));
        }
    };
    for record in book.records() {
        if record.source.kind == SourceKind::Notifications {
            add(&record.identity);
        }
    }
    for line in history.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("agent").and_then(serde_json::Value::as_bool) == Some(true) {
            continue;
        }
        if let Some(app) = value.get("app").and_then(serde_json::Value::as_str) {
            add(app);
        }
    }
    if installed.iter().any(|id| id.to_lowercase().contains(SLACK)) {
        add("Slack");
    }
    let mut found: Vec<Notifier> = names
        .into_iter()
        .map(|(key, name)| Notifier {
            granted: book
                .grant(&SourceKey::new(&key, SourceKind::Notifications))
                .map(|g| g.notebook()),
            name,
            key,
        })
        .collect();
    found.sort_by_key(|n| (!n.is_slack(), n.name.to_lowercase()));
    found
}

/// Her consent book's file.
pub fn book_path() -> Option<PathBuf> {
    hocus_secretary_link::data_dir().map(|d| d.join(CONSENTS))
}

/// The applications found on this machine now. Blocking.
pub fn read() -> Vec<Notifier> {
    let book = book_path()
        .map(|p| ConsentBook::load(&p))
        .unwrap_or_default();
    let history = hocus_config::state_dir()
        .and_then(|d| std::fs::read_to_string(d.join("notifications")).ok())
        .unwrap_or_default();
    let installed: Vec<String> = hocus_xdg::desktop::data_dirs()
        .iter()
        .flat_map(|d| hocus_xdg::desktop::files(&d.join("applications")))
        .map(|(id, _)| id)
        .collect();
    found(&book, &history, &installed)
}

/// The user's decision on `name`'s notifications: read into `notebook`
/// ("Autoriser"), or no longer (`None`, "Ne plus lire"), written in the
/// book at `path`. Blocking.
pub fn decide(
    path: &Path,
    name: &str,
    notebook: Option<Notebook>,
    at: Timestamp,
) -> std::io::Result<()> {
    let mut book = ConsentBook::load(path);
    let source = SourceKey::notifications(name, None);
    match notebook {
        Some(notebook) => book.allow(source, name.trim(), notebook, at),
        None => book.revoke(&source, at),
    }
    book.save(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_applications_are_found_in_the_book_the_history_and_slack_installed() {
        let mut book = ConsentBook::default();
        book.allow(
            SourceKey::notifications("Signal", None),
            "Signal",
            Notebook::Perso,
            Timestamp::UNIX_EPOCH,
        );
        let history = [
            r#"{"app":"Thunderbird","summary":"Facture","agent":false}"#,
            r#"{"app":"signal","summary":"Claire"}"#,
            r#"{"app":"Claude Code","summary":"waiting for your input","agent":true}"#,
            "not json",
        ]
        .join("\n");
        let found = found(&book, &history, &["com.slack.Slack".to_owned()]);
        let names: Vec<(&str, Option<Notebook>)> =
            found.iter().map(|n| (n.name.as_str(), n.granted)).collect();
        assert_eq!(
            names,
            [
                ("Slack", None),
                ("Signal", Some(Notebook::Perso)),
                ("Thunderbird", None)
            ]
        );
        assert!(found[0].is_slack());
        assert_eq!(proposed(&found[0]), Notebook::Pro);
        assert!(found[1].line(Lang::French).contains("carnet perso"));
        assert!(found[0].line(Lang::French).contains("rien n’est marqué lu"));
        assert!(super::found(&ConsentBook::default(), "", &[]).is_empty());
    }

    #[test]
    fn autoriser_writes_a_grant_the_shell_honours_and_ne_plus_lire_a_revocation() {
        let dir = tempfile::tempdir().expect("a folder");
        let path = dir.path().join("secretary").join(CONSENTS);
        decide(&path, "Slack", Some(Notebook::Pro), Timestamp::UNIX_EPOCH).expect("written");
        let book = ConsentBook::load(&path);
        // As the shell and she match a notification: by its entry, or its
        // application's name.
        let grant = book
            .notifications("Slack", Some("com.slack.Slack"))
            .expect("granted");
        assert_eq!(grant.notebook(), Notebook::Pro);
        assert_eq!(grant.identity(), "Slack");
        decide(&path, "Slack", None, Timestamp::UNIX_EPOCH).expect("written");
        let book = ConsentBook::load(&path);
        assert!(book.notifications("Slack", None).is_none());
        assert_eq!(book.records().len(), 2, "a revocation is a later record");
        let found = found(&book, "", &[]);
        assert_eq!(found[0].granted, None, "still listed, not read");
    }
}
