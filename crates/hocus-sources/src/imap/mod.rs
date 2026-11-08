//! Gmail over IMAP, read-only (§10.1, AC15): XOAUTH2 with Online
//! Accounts' token, `EXAMINE` and `BODY.PEEK` only ([`allow`]), the inbox
//! and the sent mailbox, 30 days back on a first read.
//!
//! Each mailbox keeps a cursor, its `UIDVALIDITY` and `UIDNEXT`: the next
//! read fetches only the UIDs from the last `UIDNEXT` on, nothing at all
//! when it has not moved, and everything since 30 days again when the
//! server says its UIDs changed (a new `UIDVALIDITY`, the mailbox's
//! items then void).
//!
//! TLS from the start (993), rustls on ring against the system's
//! certificates. A connection in clear is only made to this machine (a
//! test's fake server).

pub mod allow;
pub mod wire;

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpStream,
    sync::Arc,
    time::Duration,
};

use hocus_day::google::AccessToken;
use jiff::{ToSpan, civil::Date, tz::TimeZone};
use serde::{Deserialize, Serialize};

use self::{
    allow::{Command, FetchItem},
    wire::Session,
};
use crate::{Result, SourceError, consent::Grant, goa::Imap, item::Item, mail};

/// How far back a first read goes.
pub const DAYS_BACK: i64 = 30;
/// The most of a message fetched with its text.
pub const MESSAGE_MOST: u32 = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);

/// Where a mailbox's reading stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxCursor {
    pub uidvalidity: u32,
    pub uidnext: u32,
}

/// Where each mailbox's reading stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub mailboxes: BTreeMap<String, MailboxCursor>,
}

/// What a read gives.
#[derive(Debug, Default)]
pub struct Synced {
    pub items: Vec<Item>,
    /// Mailboxes whose UIDs changed: what was read of them before is void.
    pub reset: Vec<String>,
    /// The commands sent, by name (the audit log's "what, by id").
    pub sent: Vec<String>,
}

/// The connection, in TLS or (to this machine only) in clear.
enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Tls(s) => s.flush(),
        }
    }
}

fn local(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "::1" | "localhost")
}

fn connect(imap: &Imap) -> Result<Session<Stream>> {
    if !imap.tls && !local(&imap.host) {
        return Err(SourceError::Invalid(format!(
            "IMAP in clear to {} is not made",
            imap.host
        )));
    }
    let offline = |err: std::io::Error| SourceError::Offline(format!("IMAP {}: {err}", imap.host));
    let tcp = TcpStream::connect((imap.host.as_str(), imap.port)).map_err(offline)?;
    tcp.set_read_timeout(Some(TIMEOUT)).map_err(offline)?;
    tcp.set_write_timeout(Some(TIMEOUT)).map_err(offline)?;
    let stream = if imap.tls {
        use rustls_platform_verifier::BuilderVerifierExt as _;
        let tls = |err: rustls::Error| SourceError::Offline(format!("IMAP TLS: {err}"));
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(tls)?
        .with_platform_verifier()
        .map_err(tls)?
        .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(imap.host.clone())
            .map_err(|err| SourceError::Invalid(format!("IMAP host {}: {err}", imap.host)))?;
        let connection = rustls::ClientConnection::new(Arc::new(config), name).map_err(tls)?;
        Stream::Tls(Box::new(rustls::StreamOwned::new(connection, tcp)))
    } else {
        Stream::Plain(tcp)
    };
    Session::start(stream)
}

/// A session signed in with XOAUTH2, its server's capabilities.
fn sign_in(imap: &Imap, tokens: &mut dyn AccessToken) -> Result<(Session<Stream>, String)> {
    let mut session = connect(imap)?;
    let capabilities = session.ok(&Command::Capability)?;
    let capabilities = capabilities
        .untagged
        .iter()
        .map(|l| l.text.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(" ");
    let (token, _) = tokens.get()?;
    session.ok(&Command::Authenticate {
        user: imap.user.clone(),
        token: token.expose().to_owned(),
    })?;
    Ok((session, capabilities))
}

/// The items fetched with each message.
fn items(gmail: bool, text: bool) -> Vec<FetchItem> {
    let mut items = vec![FetchItem::Uid, FetchItem::Flags, FetchItem::InternalDate];
    if gmail {
        items.push(FetchItem::GmailId);
    }
    items.push(if text {
        FetchItem::Message(MESSAGE_MOST)
    } else {
        FetchItem::Header
    });
    items
}

/// What changed in `grant`'s mailboxes since `cursor` (inbox and sent),
/// with each message's text when `text`. Blocking.
pub fn sync(
    grant: &Grant,
    imap: &Imap,
    tokens: &mut dyn AccessToken,
    cursor: &mut Cursor,
    today: Date,
    zone: &TimeZone,
    text: bool,
) -> Result<Synced> {
    let (mut session, capabilities) = sign_in(imap, tokens)?;
    let gmail = capabilities.contains("X-GM-EXT-1");
    let mut read = Synced::default();
    let listed = session.ok(&Command::List)?;
    let mut mailboxes = vec![("INBOX".to_owned(), false)];
    if let Some(sent) = wire::mailbox_with(&listed.untagged, "\\Sent") {
        mailboxes.push((sent, true));
    }
    read.sent
        .extend(["CAPABILITY", "AUTHENTICATE", "LIST"].map(str::to_owned));
    for (mailbox, sent) in mailboxes {
        let examined = session.ok(&Command::Examine(mailbox.clone()))?;
        read.sent.push(format!("EXAMINE {mailbox}"));
        let validity = wire::code(&examined.untagged, "UIDVALIDITY")
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        let next = wire::code(&examined.untagged, "UIDNEXT").and_then(|v| u32::try_from(v).ok());
        let kept = cursor.mailboxes.get(&mailbox).copied();
        let set = match kept {
            Some(kept) if kept.uidvalidity == validity => match next {
                Some(next) if next == kept.uidnext => None,
                _ => Some((format!("{}:*", kept.uidnext), kept.uidnext)),
            },
            _ => {
                if kept.is_some() {
                    read.reset.push(mailbox.clone());
                }
                let since = today.checked_sub(DAYS_BACK.days()).unwrap_or(today);
                let found = session.ok(&Command::UidSearchSince(since))?;
                read.sent.push(format!("UID SEARCH {mailbox}"));
                let uids = wire::searched(&found.untagged);
                if uids.is_empty() {
                    None
                } else {
                    let set: Vec<String> = uids.iter().map(u32::to_string).collect();
                    Some((set.join(","), 0))
                }
            }
        };
        let mut highest = 0;
        if let Some((set, from)) = set {
            let answer = session.ok(&Command::UidFetch {
                set,
                items: items(gmail, text),
            })?;
            read.sent.push(format!("UID FETCH {mailbox}"));
            for message in wire::fetched(&answer.untagged) {
                // `n:*` gives the last message even when it is older.
                if message.uid < from {
                    continue;
                }
                highest = highest.max(message.uid);
                read.items
                    .push(item(grant, &mailbox, sent, validity, &message, zone));
            }
        }
        let uidnext = next
            .or_else(|| kept.map(|k| k.uidnext.max(highest + 1)))
            .unwrap_or(highest + 1);
        cursor.mailboxes.insert(
            mailbox,
            MailboxCursor {
                uidvalidity: validity,
                uidnext,
            },
        );
    }
    let _ = session.run(&Command::Logout);
    Ok(read)
}

fn item(
    grant: &Grant,
    mailbox: &str,
    sent: bool,
    validity: u32,
    message: &wire::Fetched,
    zone: &TimeZone,
) -> Item {
    let place = mail::Place {
        mailbox,
        sent,
        uidvalidity: validity,
        uid: message.uid,
        gmail_id: message.gmail_id,
        seen: message
            .flags
            .iter()
            .any(|f| f.eq_ignore_ascii_case("\\Seen")),
        internal_date: message
            .internal_date
            .as_deref()
            .and_then(wire::internal_date),
    };
    mail::item(
        grant,
        &place,
        message.bytes.as_deref().unwrap_or_default(),
        zone,
    )
}

/// One mail with its text, by its pointer's id (`mailbox/uidvalidity/uid`),
/// on demand. `None` when it is no longer there (its UIDs changed).
pub fn fetch(
    grant: &Grant,
    imap: &Imap,
    tokens: &mut dyn AccessToken,
    id: &str,
    zone: &TimeZone,
) -> Result<Option<Item>> {
    let mut parts = id.rsplitn(3, '/');
    let (Some(uid), Some(validity), Some(mailbox)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(SourceError::Invalid(format!("not a mail's id: {id}")));
    };
    let (uid, validity): (u32, u32) = (
        uid.parse()
            .map_err(|_| SourceError::Invalid(format!("not a mail's id: {id}")))?,
        validity
            .parse()
            .map_err(|_| SourceError::Invalid(format!("not a mail's id: {id}")))?,
    );
    let (mut session, capabilities) = sign_in(imap, tokens)?;
    let examined = session.ok(&Command::Examine(mailbox.to_owned()))?;
    if wire::code(&examined.untagged, "UIDVALIDITY") != Some(u64::from(validity)) {
        let _ = session.run(&Command::Logout);
        return Ok(None);
    }
    let sent = mailbox != "INBOX";
    let answer = session.ok(&Command::UidFetch {
        set: uid.to_string(),
        items: items(capabilities.contains("X-GM-EXT-1"), true),
    })?;
    let _ = session.run(&Command::Logout);
    Ok(wire::fetched(&answer.untagged)
        .into_iter()
        .find(|m| m.uid == uid)
        .map(|m| item(grant, mailbox, sent, validity, &m, zone)))
}
