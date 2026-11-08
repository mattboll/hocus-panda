//! Every Google account of GNOME Online Accounts (§10.1), on the bus the
//! process is given (the session's, as the compositor names it), never
//! another: the work account and a personal one alike, each with what it
//! has on (calendar, mail, contacts) and a suggested notebook, which the
//! user confirms or changes on its consent sheet ("C'est ton compte
//! perso ?").
//!
//! Online Accounts' objects: `org.gnome.OnlineAccounts.Account`
//! (`ProviderType`, `Id`, `Identity`, `PresentationIdentity`,
//! `CalendarDisabled`, `MailDisabled`, `ContactsDisabled`,
//! `AttentionNeeded`), `…OAuth2Based` (`GetAccessToken`), and for mail
//! `…Mail` (`EmailAddress`, `ImapHost`, `ImapUserName`, `ImapUseSsl`).
//! Tokens are asked for each time ([`token`]), never kept.

use std::{collections::HashMap, time::Duration};

use hocus_day::google::AccessToken;
use serde::{Deserialize, Serialize};
use zbus::{OwnedObjectPath, OwnedVariant, connection::Builder};

use crate::{
    Result, SourceError,
    item::{Notebook, SourceKind},
};

pub const NAME: &str = "org.gnome.OnlineAccounts";
pub const ROOT: &str = "/org/gnome/OnlineAccounts";
pub const ACCOUNT: &str = "org.gnome.OnlineAccounts.Account";
pub const OAUTH2: &str = "org.gnome.OnlineAccounts.OAuth2Based";
pub const MAIL: &str = "org.gnome.OnlineAccounts.Mail";
const WAIT: Duration = Duration::from_secs(30);

/// Where an account's mail is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Imap {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// TLS from the start (993); else the connection is refused, never
    /// read in clear (STARTTLS is not offered).
    pub tls: bool,
}

/// A Google account of Online Accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleAccount {
    /// Online Accounts' id for it (stable across sessions): the sources'
    /// key.
    pub id: String,
    /// Its object, which gives its tokens.
    pub path: String,
    /// Its e-mail.
    pub identity: String,
    pub calendar: bool,
    pub mail: bool,
    pub contacts: bool,
    /// It needs signing in again.
    pub attention: bool,
    pub imap: Imap,
    /// The notebook suggested on its sheet.
    pub suggested: Notebook,
}

impl GoogleAccount {
    /// What of it Online Accounts has on, to be offered on its sheet.
    pub fn kinds(&self) -> Vec<SourceKind> {
        let mut kinds = Vec::new();
        if self.calendar {
            kinds.push(SourceKind::Calendar);
        }
        if self.contacts {
            kinds.push(SourceKind::Birthdays);
        }
        if self.mail {
            kinds.push(SourceKind::Mail);
        }
        kinds
    }
}

/// The domains of personal Google accounts.
const PERSONAL: [&str; 2] = ["gmail.com", "googlemail.com"];

fn personal(identity: &str) -> bool {
    identity
        .rsplit_once('@')
        .is_some_and(|(_, domain)| PERSONAL.contains(&domain.to_ascii_lowercase().as_str()))
}

/// The notebooks to suggest: an account on a company's domain is pro, a
/// gmail.com one perso; when every account is a gmail.com one, the first
/// is suggested pro.
fn suggest(accounts: &mut [GoogleAccount]) {
    let any_company = accounts.iter().any(|a| !personal(&a.identity));
    for (i, account) in accounts.iter_mut().enumerate() {
        account.suggested = if !personal(&account.identity) || (!any_company && i == 0) {
            Notebook::Pro
        } else {
            Notebook::Perso
        };
    }
}

/// Every Google account Online Accounts has on `bus` (an address), in
/// its objects' order, each with a suggested notebook. Blocking, within
/// 30 s.
pub fn discover(bus: &str) -> Result<Vec<GoogleAccount>> {
    type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedVariant>>>;
    let objects: Objects = block(bus, async |connection| {
        let reply = connection
            .call_method(
                Some(NAME),
                ROOT,
                Some("org.freedesktop.DBus.ObjectManager"),
                "GetManagedObjects",
                &(),
            )
            .await?;
        reply.body().deserialize::<Objects>()
    })?;
    let mut accounts: Vec<GoogleAccount> = objects
        .into_iter()
        .filter_map(|(path, interfaces)| {
            let account = interfaces.get(ACCOUNT)?;
            interfaces.get(OAUTH2)?;
            let text = |props: &HashMap<String, OwnedVariant>, name: &str| {
                props
                    .get(name)
                    .and_then(|v| v.downcast_ref::<&str>().ok().map(str::to_owned))
                    .unwrap_or_default()
            };
            let flag = |props: &HashMap<String, OwnedVariant>, name: &str| {
                props
                    .get(name)
                    .and_then(|v| v.downcast_ref::<bool>().ok())
                    .unwrap_or(false)
            };
            if text(account, "ProviderType") != "google" {
                return None;
            }
            let identity = text(account, "Identity");
            let mail = interfaces.get(MAIL);
            let imap = Imap {
                host: mail
                    .map(|m| text(m, "ImapHost"))
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| "imap.gmail.com".to_owned()),
                port: 993,
                user: mail
                    .map(|m| text(m, "ImapUserName"))
                    .filter(|u| !u.is_empty())
                    .unwrap_or_else(|| identity.clone()),
                tls: mail.is_none_or(|m| m.get("ImapUseSsl").is_none() || flag(m, "ImapUseSsl")),
            };
            let id = text(account, "Id");
            Some(GoogleAccount {
                id: if id.is_empty() {
                    path.as_str()
                        .rsplit('/')
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                } else {
                    id
                },
                path: path.to_string(),
                calendar: !flag(account, "CalendarDisabled"),
                mail: !flag(account, "MailDisabled"),
                contacts: !flag(account, "ContactsDisabled"),
                attention: flag(account, "AttentionNeeded"),
                imap,
                identity,
                suggested: Notebook::Pro,
            })
        })
        .collect();
    accounts.sort_by(|a, b| a.path.cmp(&b.path));
    suggest(&mut accounts);
    Ok(accounts)
}

/// The account at `path`'s access tokens, asked of Online Accounts on
/// `bus` each time one is needed (`hocus-day`'s).
pub fn token(bus: &str, path: &str) -> Result<Box<dyn AccessToken>> {
    let path = OwnedObjectPath::try_from(path)
        .map_err(|err| SourceError::Invalid(format!("{path}: {err}")))?;
    Ok(Box::new(hocus_day::goa::GoaToken::new(
        bus.to_owned(),
        path,
    )))
}

fn block<T>(bus: &str, work: impl AsyncFnOnce(zbus::Connection) -> zbus::Result<T>) -> Result<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| SourceError::Offline(err.to_string()))?;
    runtime.block_on(async {
        let asked = async { work(Builder::address(bus).build().await?).await };
        match tokio::time::timeout(WAIT, asked).await {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(err)) => Err(SourceError::Offline(format!(
                "Online Accounts does not answer: {err}"
            ))),
            Err(_) => Err(SourceError::Offline(
                "Online Accounts does not answer in time".to_owned(),
            )),
        }
    })
}

/// A consent sheet's words (§10.1): what she reads, what she never does,
/// what leaves the machine. Shown before "Autoriser".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sheet {
    pub title: String,
    pub reads: &'static str,
    pub never: &'static str,
    pub leaves: &'static str,
    /// "C'est ton compte perso ?" when perso is suggested.
    pub ask_notebook: Option<&'static str>,
}

/// `account`'s sheet for `kind`.
pub fn sheet(account: &GoogleAccount, kind: SourceKind) -> Sheet {
    let (title, reads, never) = match kind {
        SourceKind::Calendar => (
            "Agenda",
            "Tous tes agendas, y compris ceux que tu as masqués (congés, famille).",
            "Je ne réponds jamais à une invitation sans ton clic.",
        ),
        SourceKind::Birthdays => (
            "Anniversaires",
            "Les anniversaires et fêtes de tes contacts, rien d'autre de leurs fiches.",
            "Je ne modifie aucun contact.",
        ),
        SourceKind::Mail => (
            "Mail",
            "Ta boîte de réception et tes envoyés, 30 jours en arrière, en lecture seule.",
            "Je ne marque rien comme lu, je n'envoie, ne supprime ni ne déplace rien.",
        ),
    };
    Sheet {
        title: format!("{title} · {}", account.identity),
        reads,
        never,
        leaves: "Vers Claude : un extrait, après masquage des mots de passe, codes et numéros. Jamais les pièces jointes.",
        ask_notebook: (account.suggested == Notebook::Perso).then_some("C'est ton compte perso ?"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(identity: &str) -> GoogleAccount {
        GoogleAccount {
            id: identity.to_owned(),
            path: String::new(),
            identity: identity.to_owned(),
            calendar: true,
            mail: true,
            contacts: true,
            attention: false,
            imap: Imap {
                host: "imap.gmail.com".to_owned(),
                port: 993,
                user: identity.to_owned(),
                tls: true,
            },
            suggested: Notebook::Pro,
        }
    }

    #[test]
    fn a_company_account_is_suggested_pro_and_a_gmail_one_perso() {
        let mut accounts = vec![
            account("alex.perso@gmail.com"),
            account("alex@atelier-lune.fr"),
        ];
        suggest(&mut accounts);
        assert_eq!(accounts[0].suggested, Notebook::Perso);
        assert_eq!(accounts[1].suggested, Notebook::Pro);
        let mut gmail_only = vec![account("a@gmail.com"), account("b@gmail.com")];
        suggest(&mut gmail_only);
        assert_eq!(gmail_only[0].suggested, Notebook::Pro);
        assert_eq!(gmail_only[1].suggested, Notebook::Perso);
    }
}
