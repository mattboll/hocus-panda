//! The secretary's Google sources (docs/design/secretary.md §10.1, #750),
//! on the Agents page under "LA SECRÉTAIRE" beside the applications'
//! notifications, until her own page (lot 8): each kind (calendar,
//! birthdays, mail) of each Google account of GNOME Online Accounts,
//! consented to on its own sheet (the same words as her card in the
//! drawer, `hocus_secretary_link::sheets`), its notebook suggested by the
//! account's domain (pro for a company's, perso for gmail.com) and chosen
//! there.
//!
//! What the page lists is found, not configured: the accounts Online
//! Accounts has on the session's bus (the one the compositor gives its
//! programs), each with what it has on. "Autoriser" writes a grant in her
//! consent book, "Ne plus lire" a revocation; her Google source reads the
//! book at its next cycle and forgets what came from a source revoked.
//!
//! Read and written on a thread of the page's, never the window's.

use std::path::Path;

use hocus_secretary_link::{
    Notebook,
    consent::{ConsentBook, SourceKey, SourceKind},
    goa::{self, GoogleAccount},
    sheets,
};
use jiff::Timestamp;

use crate::lang::Lang;

/// A kind of a Google account she may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Online Accounts' id for the account.
    pub account: String,
    /// Its e-mail.
    pub identity: String,
    pub kind: SourceKind,
    /// The notebook its sheet suggests.
    pub suggested: Notebook,
    /// Online Accounts asks to sign in again.
    pub attention: bool,
    /// Read now, into this notebook.
    pub granted: Option<Notebook>,
}

impl Source {
    pub fn key(&self) -> SourceKey {
        SourceKey::new(&self.account, self.kind)
    }

    /// Its name in the log and the places' list: `account:kind`.
    pub fn word(&self) -> String {
        self.key().to_string()
    }

    /// Its title: "Agenda · alex@atelier-lune.fr".
    pub fn title(&self, lang: Lang) -> String {
        format!(
            "{} · {}",
            sheets::kind_name(self.kind, lang == Lang::French),
            self.identity
        )
    }

    /// Its line under its title.
    pub fn line(&self, lang: Lang) -> String {
        let fr = lang == Lang::French;
        let what = match (self.kind, fr) {
            (SourceKind::Calendar, true) => {
                "Tous tes agendas, masqués compris ; elle ne répond à aucune invitation"
            }
            (SourceKind::Calendar, false) => {
                "All your calendars, hidden ones included; she answers no invitation"
            }
            (SourceKind::Mail, true) => {
                "Ta boîte et tes envois, 30 jours, en lecture seule : rien n’est marqué lu"
            }
            (SourceKind::Mail, false) => {
                "Your inbox and sent mail, 30 days, read-only: nothing is marked read"
            }
            (_, true) => "Les anniversaires de tes contacts, rien d’autre de leurs fiches",
            (_, false) => "Your contacts’ birthdays, nothing else of their cards",
        };
        let mut line = match (self.granted, fr) {
            (Some(notebook), true) => format!("Elle lit · carnet {} · {what}", notebook.word()),
            (Some(notebook), false) => format!("She reads · {} notebook · {what}", notebook.word()),
            (None, _) => what.to_owned(),
        };
        if self.attention {
            line.push_str(if fr {
                " · Comptes en ligne demande de te reconnecter"
            } else {
                " · Online Accounts asks you to sign in again"
            });
        }
        line
    }

    /// Its consent sheet.
    pub fn sheet(&self, lang: Lang) -> sheets::Sheet {
        sheets::google(
            &self.identity,
            self.kind,
            self.suggested,
            lang == Lang::French,
        )
    }
}

/// Each kind each account has on, as `book` has it; accounts in Online
/// Accounts' order, kinds as their sheets come (calendar, birthdays,
/// mail).
pub fn found(accounts: &[GoogleAccount], book: &ConsentBook) -> Vec<Source> {
    accounts
        .iter()
        .flat_map(|account| {
            account.kinds().into_iter().map(|kind| Source {
                account: account.id.clone(),
                identity: account.identity.clone(),
                kind,
                suggested: account.suggested,
                attention: account.attention,
                granted: book
                    .grant(&SourceKey::new(&account.id, kind))
                    .map(|g| g.notebook()),
            })
        })
        .collect()
}

/// The Google accounts on the session's bus now, with her book. Blocking
/// (Online Accounts answers within 30 s); none without a bus.
pub fn read() -> Vec<Source> {
    let Some(bus) = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .ok()
        .filter(|b| !b.is_empty())
    else {
        return Vec::new();
    };
    let accounts = match goa::discover(&bus) {
        Ok(accounts) => accounts,
        Err(err) => {
            tracing::info!(%err, "no Google account for the secretary");
            return Vec::new();
        }
    };
    let book = crate::notifiers::book_path()
        .map(|p| ConsentBook::load(&p))
        .unwrap_or_default();
    found(&accounts, &book)
}

/// The user's decision on `source`: read into `notebook` ("Autoriser"),
/// or no longer (`None`, "Ne plus lire"), written in the book at `path`.
/// Blocking.
pub fn decide(
    path: &Path,
    source: &SourceKey,
    identity: &str,
    notebook: Option<Notebook>,
    at: Timestamp,
) -> std::io::Result<()> {
    let mut book = ConsentBook::load(path);
    match notebook {
        Some(notebook) => book.allow(source.clone(), identity, notebook, at),
        None => book.revoke(source, at),
    }
    book.save(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, identity: &str, suggested: Notebook) -> GoogleAccount {
        GoogleAccount {
            id: id.to_owned(),
            path: format!("/org/gnome/OnlineAccounts/Accounts/{id}"),
            identity: identity.to_owned(),
            calendar: true,
            mail: true,
            contacts: false,
            attention: false,
            imap: goa::Imap {
                host: "imap.gmail.com".to_owned(),
                port: 993,
                user: identity.to_owned(),
                tls: true,
            },
            suggested,
        }
    }

    #[test]
    fn each_kind_on_is_a_row_and_autoriser_writes_a_grant_ne_plus_lire_a_revocation() {
        let dir = tempfile::tempdir().expect("a folder");
        let path = dir.path().join("secretary/consents.json");
        let accounts = [
            account("account_1", "alex@atelier-lune.fr", Notebook::Pro),
            account("account_2", "alex.martin@gmail.com", Notebook::Perso),
        ];
        let rows = found(&accounts, &ConsentBook::load(&path));
        let words: Vec<String> = rows.iter().map(Source::word).collect();
        assert_eq!(
            words,
            [
                "account_1:calendar",
                "account_1:mail",
                "account_2:calendar",
                "account_2:mail"
            ],
            "contacts off: no birthdays"
        );
        assert_eq!(
            rows[2].title(Lang::French),
            "Agenda · alex.martin@gmail.com"
        );
        assert_eq!(rows[2].sheet(Lang::French).notebook, Notebook::Perso);
        decide(
            &path,
            &rows[2].key(),
            &rows[2].identity,
            Some(Notebook::Perso),
            Timestamp::UNIX_EPOCH,
        )
        .expect("written");
        let rows = found(&accounts, &ConsentBook::load(&path));
        assert_eq!(rows[2].granted, Some(Notebook::Perso));
        assert!(
            rows[2]
                .line(Lang::French)
                .starts_with("Elle lit · carnet perso")
        );
        decide(
            &path,
            &rows[2].key(),
            &rows[2].identity,
            None,
            Timestamp::UNIX_EPOCH,
        )
        .expect("written");
        let book = ConsentBook::load(&path);
        assert_eq!(book.revoked(), [rows[2].key()]);
        assert_eq!(found(&accounts, &book)[2].granted, None);
    }
}
