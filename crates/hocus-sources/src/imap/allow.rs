//! The allow-list (§2.5, §14, AC15): the only IMAP commands she can
//! send, and the guard every line passes before it leaves.
//!
//! Commands are values of [`Command`]: there is no variant for `SELECT`,
//! `STORE`, `EXPUNGE`, `COPY`, `MOVE`, `APPEND`, `CLOSE`, nor for a fetch
//! that is not a peek, so none can be built. [`check`] then reads the
//! exact line about to be written and refuses anything else: a second
//! wall, should a variant ever be added carelessly or a mailbox's name
//! try to smuggle a command in.
//!
//! What `\Seen` depends on (RFC 3501, 6.4.5): `BODY[…]` sets it,
//! `BODY.PEEK[…]` does not, nor do `RFC822.HEADER` and `RFC822.SIZE`;
//! `RFC822` and `RFC822.TEXT` set it. Only `EXAMINE` is used to open a
//! mailbox: read-only, so even a mistaken fetch could not set it.

use std::fmt;

use jiff::civil::Date;

/// What a fetch may ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchItem {
    Uid,
    Flags,
    InternalDate,
    Size,
    /// Gmail's message id (`X-GM-MSGID`), for the link that opens it.
    GmailId,
    /// The header, peeked: `BODY.PEEK[HEADER]`.
    Header,
    /// The whole message, peeked, at most this many bytes:
    /// `BODY.PEEK[]<0.N>`.
    Message(u32),
}

impl FetchItem {
    fn wire(self) -> String {
        match self {
            Self::Uid => "UID".to_owned(),
            Self::Flags => "FLAGS".to_owned(),
            Self::InternalDate => "INTERNALDATE".to_owned(),
            Self::Size => "RFC822.SIZE".to_owned(),
            Self::GmailId => "X-GM-MSGID".to_owned(),
            Self::Header => "BODY.PEEK[HEADER]".to_owned(),
            Self::Message(most) => format!("BODY.PEEK[]<0.{most}>"),
        }
    }
}

/// A command she may send.
#[derive(Clone, PartialEq, Eq)]
pub enum Command {
    Capability,
    /// XOAUTH2 with its initial response (SASL-IR).
    Authenticate {
        user: String,
        token: String,
    },
    /// Every mailbox, to find the sent one (`\Sent`).
    List,
    /// A mailbox opened read-only.
    Examine(String),
    /// The UIDs of the messages since a day.
    UidSearchSince(Date),
    /// Some messages' items, by UID.
    UidFetch {
        set: String,
        items: Vec<FetchItem>,
    },
    Noop,
    Logout,
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authenticate { user, .. } => write!(f, "Authenticate({user}, …)"),
            other => f.write_str(&other.render_args().unwrap_or_default()),
        }
    }
}

/// A mailbox's name quoted, or `None` when it holds what a line cannot
/// (a line break, a control character).
fn quoted(name: &str) -> Option<String> {
    if name.chars().any(char::is_control) {
        return None;
    }
    Some(format!(
        "\"{}\"",
        name.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl Command {
    fn render_args(&self) -> Option<String> {
        Some(match self {
            Self::Capability => "CAPABILITY".to_owned(),
            Self::Authenticate { user, token } => {
                use base64::Engine as _;
                let response = format!("user={user}\x01auth=Bearer {token}\x01\x01");
                format!(
                    "AUTHENTICATE XOAUTH2 {}",
                    base64::engine::general_purpose::STANDARD.encode(response)
                )
            }
            Self::List => "LIST \"\" \"*\"".to_owned(),
            Self::Examine(mailbox) => format!("EXAMINE {}", quoted(mailbox)?),
            Self::UidSearchSince(date) => format!(
                "UID SEARCH SINCE {}-{}-{}",
                date.day(),
                MONTHS[usize::try_from(date.month() - 1).unwrap_or(0)],
                date.year()
            ),
            Self::UidFetch { set, items } => {
                if set.is_empty()
                    || !set
                        .chars()
                        .all(|c| c.is_ascii_digit() || matches!(c, ':' | ',' | '*'))
                {
                    return None;
                }
                let items: Vec<String> = items.iter().map(|i| i.wire()).collect();
                format!("UID FETCH {set} ({})", items.join(" "))
            }
            Self::Noop => "NOOP".to_owned(),
            Self::Logout => "LOGOUT".to_owned(),
        })
    }

    /// The line to send under `tag`, CRLF included; `None` when it cannot
    /// be made safely.
    pub fn render(&self, tag: &str) -> Option<String> {
        Some(format!("{tag} {}\r\n", self.render_args()?))
    }
}

/// Why a line was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "refused by the IMAP allow-list: {}", self.0)
    }
}

/// The fetch items allowed, as the server reads them (upper case).
fn fetch_item_allowed(item: &str) -> bool {
    let item = item.to_ascii_uppercase();
    if matches!(
        item.as_str(),
        "UID"
            | "FLAGS"
            | "INTERNALDATE"
            | "RFC822.SIZE"
            | "X-GM-MSGID"
            | "X-GM-THRID"
            | "RFC822.HEADER"
    ) {
        return true;
    }
    // A peek, of any section, partial or not.
    item.starts_with("BODY.PEEK[")
}

/// Whether `line` (one command, CRLF ended) may be sent. Checked on the
/// exact bytes about to be written.
pub fn check(line: &str) -> Result<(), Refused> {
    let refuse = |why: &str| Err(Refused(why.to_owned()));
    let Some(body) = line.strip_suffix("\r\n") else {
        return refuse("a command is one line ending in CRLF");
    };
    if body.contains(['\r', '\n']) || body.chars().any(|c| c.is_control()) {
        return refuse("a line break or a control character inside a command");
    }
    let mut words = body.split(' ');
    let _tag = words.next();
    let verb = words.next().unwrap_or("").to_ascii_uppercase();
    match verb.as_str() {
        "CAPABILITY" | "NOOP" | "LOGOUT" | "LIST" | "EXAMINE" => Ok(()),
        "AUTHENTICATE" => match words.next().map(str::to_ascii_uppercase).as_deref() {
            Some("XOAUTH2") => Ok(()),
            _ => refuse("only XOAUTH2"),
        },
        "UID" => {
            let sub = words.next().unwrap_or("").to_ascii_uppercase();
            match sub.as_str() {
                "SEARCH" => Ok(()),
                "FETCH" => {
                    let _set = words.next();
                    let rest: Vec<&str> = words.collect();
                    let items = rest.join(" ");
                    let items = items
                        .strip_prefix('(')
                        .and_then(|i| i.strip_suffix(')'))
                        .unwrap_or(&items);
                    let mut depth = 0usize;
                    let mut current = String::new();
                    let mut all = Vec::new();
                    for c in items.chars() {
                        match c {
                            '[' => {
                                depth += 1;
                                current.push(c);
                            }
                            ']' => {
                                depth = depth.saturating_sub(1);
                                current.push(c);
                            }
                            ' ' if depth == 0 => all.push(std::mem::take(&mut current)),
                            c => current.push(c),
                        }
                    }
                    all.push(current);
                    match all
                        .iter()
                        .filter(|i| !i.is_empty())
                        .find(|i| !fetch_item_allowed(i))
                    {
                        Some(item) => Err(Refused(format!("fetch item {item}"))),
                        None => Ok(()),
                    }
                }
                other => Err(Refused(format!("UID {other}"))),
            }
        }
        other => Err(Refused(format!("command {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_built_passes_the_guard() {
        let commands = [
            Command::Capability,
            Command::Authenticate {
                user: "alex@example.org".to_owned(),
                token: "tok".to_owned(),
            },
            Command::List,
            Command::Examine("[Gmail]/Messages envoyés".to_owned()),
            Command::UidSearchSince(jiff::civil::date(2026, 10, 9)),
            Command::UidFetch {
                set: "1:*".to_owned(),
                items: vec![
                    FetchItem::Uid,
                    FetchItem::Flags,
                    FetchItem::InternalDate,
                    FetchItem::Size,
                    FetchItem::GmailId,
                    FetchItem::Header,
                    FetchItem::Message(1024),
                ],
            },
            Command::Noop,
            Command::Logout,
        ];
        for command in commands {
            let line = command.render("a1").expect("a line");
            assert_eq!(check(&line), Ok(()), "{line}");
        }
    }

    #[test]
    fn writes_and_reads_that_mark_seen_are_refused() {
        for line in [
            "a1 SELECT INBOX\r\n",
            "a1 STORE 1 +FLAGS (\\Seen)\r\n",
            "a1 UID STORE 1 +FLAGS (\\Seen)\r\n",
            "a1 EXPUNGE\r\n",
            "a1 UID EXPUNGE 1\r\n",
            "a1 COPY 1 Trash\r\n",
            "a1 UID MOVE 1 Trash\r\n",
            "a1 APPEND INBOX {3}\r\n",
            "a1 CLOSE\r\n",
            "a1 DELETE INBOX\r\n",
            "a1 UID FETCH 1 (BODY[])\r\n",
            "a1 UID FETCH 1 (BODY[TEXT])\r\n",
            "a1 UID FETCH 1 (RFC822)\r\n",
            "a1 UID FETCH 1 (RFC822.TEXT)\r\n",
            "a1 FETCH 1 (BODY.PEEK[])\r\n",
            "a1 AUTHENTICATE PLAIN\r\n",
            "a1 EXAMINE \"INBOX\"\r\na2 STORE 1 +FLAGS (\\Seen)\r\n",
        ] {
            assert!(check(line).is_err(), "{line:?} passed");
        }
    }

    #[test]
    fn a_mailbox_name_cannot_smuggle_a_command() {
        let command = Command::Examine("INBOX\r\na2 STORE 1 +FLAGS (\\Seen)".to_owned());
        assert_eq!(command.render("a1"), None);
        let command = Command::UidFetch {
            set: "1) (BODY[]".to_owned(),
            items: vec![FetchItem::Uid],
        };
        assert_eq!(command.render("a1"), None);
    }
}
