//! An IMAP server on 127.0.0.1 (in clear: a test's), its mailboxes set by
//! the test. It answers what a read-only client sends, and keeps as a
//! **violation** anything else: a command outside `CAPABILITY`,
//! `AUTHENTICATE XOAUTH2`, `LIST`, `EXAMINE`, `UID SEARCH`, `UID FETCH`,
//! `NOOP` and `LOGOUT`, or a fetch that would set `\Seen` (`BODY[…]`
//! without `.PEEK`, `RFC822`, `RFC822.TEXT`), which it then applies, as a
//! server would. Its own list, written apart from the client's.

use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use base64::Engine as _;

#[derive(Debug, Clone)]
pub struct Message {
    pub uid: u32,
    pub flags: Vec<String>,
    /// `05-Nov-2026 10:12:00 +0100`.
    pub internal: String,
    pub raw: Vec<u8>,
    pub gmail_id: u64,
}

#[derive(Debug, Clone)]
pub struct Mailbox {
    pub name: String,
    /// `\Sent`, for the sent one.
    pub special: Option<String>,
    pub uidvalidity: u32,
    pub messages: Vec<Message>,
}

impl Mailbox {
    pub fn uidnext(&self) -> u32 {
        self.messages.iter().map(|m| m.uid).max().unwrap_or(0) + 1
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub mailboxes: Vec<Mailbox>,
    /// Every line received, the `AUTHENTICATE`'s response left out.
    pub commands: Vec<String>,
    pub violations: Vec<String>,
    /// Each connection's XOAUTH2 user and token.
    pub signed_in: Vec<(String, String)>,
    pub connections: usize,
}

pub struct FakeImap {
    pub port: u16,
    pub state: Arc<Mutex<State>>,
}

impl FakeImap {
    pub fn start(mailboxes: Vec<Mailbox>) -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let state = Arc::new(Mutex::new(State {
            mailboxes,
            ..State::default()
        }));
        let served = state.clone();
        std::thread::Builder::new()
            .name("fake imap".to_owned())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let state = served.clone();
                    std::thread::spawn(move || serve(stream, &state));
                }
            })?;
        Ok(Self { port, state })
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn violations(&self) -> Vec<String> {
        self.with(|s| s.violations.clone())
    }

    /// The commands' verbs, in order (`UID FETCH` as such).
    pub fn verbs(&self) -> Vec<String> {
        self.with(|s| {
            s.commands
                .iter()
                .map(|c| {
                    let words: Vec<&str> = c.split(' ').skip(1).take(2).collect();
                    match words.as_slice() {
                        [uid, sub] if uid.eq_ignore_ascii_case("UID") => {
                            format!("UID {}", sub.to_ascii_uppercase())
                        }
                        [verb, ..] => verb.to_ascii_uppercase(),
                        [] => String::new(),
                    }
                })
                .collect()
        })
    }

    /// Every message's flags, by mailbox and UID.
    pub fn flags(&self) -> Vec<(String, u32, Vec<String>)> {
        self.with(|s| {
            s.mailboxes
                .iter()
                .flat_map(|m| {
                    m.messages
                        .iter()
                        .map(|msg| (m.name.clone(), msg.uid, msg.flags.clone()))
                })
                .collect()
        })
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn day(text: &str) -> Option<(i32, usize, i32)> {
    // `5-Nov-2026` or `05-Nov-2026 10:12:00 +0100`.
    let date = text.split(' ').next()?;
    let mut parts = date.split('-');
    let d: i32 = parts.next()?.parse().ok()?;
    let month = parts.next()?;
    let m = MONTHS.iter().position(|m| m.eq_ignore_ascii_case(month))?;
    let y: i32 = parts.next()?.parse().ok()?;
    Some((y, m, d))
}

/// What the server holds against a fetch item, if anything.
fn sets_seen(item: &str) -> bool {
    let item = item.to_ascii_uppercase();
    (item.starts_with("BODY[") && !item.starts_with("BODY.PEEK["))
        || item == "RFC822"
        || item == "RFC822.TEXT"
}

fn uids(set: &str, mailbox: &Mailbox) -> Vec<u32> {
    let last = mailbox.messages.iter().map(|m| m.uid).max().unwrap_or(0);
    let mut out = Vec::new();
    for part in set.split(',') {
        let (from, to) = part.split_once(':').unwrap_or((part, part));
        let number = |n: &str| {
            if n == "*" {
                last
            } else {
                n.parse().unwrap_or(0)
            }
        };
        let (a, b) = (number(from), number(to));
        let (a, b) = (a.min(b), a.max(b));
        out.extend(
            mailbox
                .messages
                .iter()
                .map(|m| m.uid)
                .filter(|u| (a..=b).contains(u)),
        );
        // `n:*` with n past the last gives the last message (RFC 3501).
        if to == "*" && a > last && last > 0 {
            out.push(last);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The fetch items of `( … )`, sections kept whole.
fn fetch_items(text: &str) -> Vec<String> {
    let inner = text
        .trim()
        .strip_prefix('(')
        .and_then(|t| t.strip_suffix(')'))
        .unwrap_or(text.trim());
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    for c in inner.chars() {
        match c {
            '[' => {
                depth += 1;
                current.push(c);
            }
            ']' => {
                depth -= 1;
                current.push(c);
            }
            ' ' if depth == 0 => items.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    items.push(current);
    items.retain(|i| !i.is_empty());
    items
}

fn serve(stream: TcpStream, state: &Mutex<State>) {
    let lock = || state.lock().unwrap_or_else(PoisonError::into_inner);
    lock().connections += 1;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let mut out = writer;
    let mut reader = BufReader::new(stream);
    let _ = out
        .write_all(b"* OK [CAPABILITY IMAP4rev1 AUTH=XOAUTH2 SASL-IR X-GM-EXT-1] fake ready\r\n");
    let mut selected: Option<usize> = None;
    let mut signed_in = false;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = line.trim_end_matches(['\r', '\n']).to_owned();
        let mut words = line.splitn(3, ' ');
        let tag = words.next().unwrap_or("*").to_owned();
        let verb = words.next().unwrap_or("").to_ascii_uppercase();
        let rest = words.next().unwrap_or("").to_owned();
        let shown = if verb == "AUTHENTICATE" {
            format!("{tag} AUTHENTICATE XOAUTH2 …")
        } else {
            line.clone()
        };
        lock().commands.push(shown);
        let mut reply = String::new();
        let mut literal_reply: Vec<u8> = Vec::new();
        match verb.as_str() {
            "CAPABILITY" => {
                reply = format!(
                    "* CAPABILITY IMAP4rev1 AUTH=XOAUTH2 SASL-IR X-GM-EXT-1\r\n{tag} OK done\r\n"
                );
            }
            "NOOP" => reply = format!("{tag} OK done\r\n"),
            "LOGOUT" => {
                let _ = out.write_all(format!("* BYE\r\n{tag} OK bye\r\n").as_bytes());
                return;
            }
            "AUTHENTICATE" => {
                let mut parts = rest.splitn(2, ' ');
                let mechanism = parts.next().unwrap_or("").to_ascii_uppercase();
                let response = parts.next().unwrap_or("");
                if mechanism != "XOAUTH2" {
                    lock().violations.push(format!("AUTHENTICATE {mechanism}"));
                    reply = format!("{tag} BAD only XOAUTH2\r\n");
                } else {
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(response)
                        .unwrap_or_default();
                    let decoded = String::from_utf8_lossy(&decoded).into_owned();
                    let user = decoded
                        .split('\x01')
                        .find_map(|p| p.strip_prefix("user="))
                        .unwrap_or("")
                        .to_owned();
                    let token = decoded
                        .split('\x01')
                        .find_map(|p| p.strip_prefix("auth=Bearer "))
                        .unwrap_or("")
                        .to_owned();
                    signed_in = !token.is_empty();
                    lock().signed_in.push((user, token));
                    reply = if signed_in {
                        format!("{tag} OK signed in\r\n")
                    } else {
                        format!("{tag} NO no token\r\n")
                    };
                }
            }
            "LIST" => {
                let s = lock();
                for m in &s.mailboxes {
                    let special = m.special.as_deref().unwrap_or("");
                    reply.push_str(&format!(
                        "* LIST (\\HasNoChildren {special}) \"/\" \"{}\"\r\n",
                        m.name
                    ));
                }
                reply.push_str(&format!("{tag} OK listed\r\n"));
            }
            "EXAMINE" if signed_in => {
                let name = rest.trim().trim_matches('"').to_owned();
                let s = lock();
                match s.mailboxes.iter().position(|m| m.name == name) {
                    Some(i) => {
                        selected = Some(i);
                        let m = &s.mailboxes[i];
                        reply = format!(
                            "* {} EXISTS\r\n* OK [UIDVALIDITY {}] valid\r\n* OK [UIDNEXT {}] next\r\n* OK [PERMANENTFLAGS ()] read-only\r\n{tag} OK [READ-ONLY] examined\r\n",
                            m.messages.len(),
                            m.uidvalidity,
                            m.uidnext()
                        );
                    }
                    None => reply = format!("{tag} NO no such mailbox\r\n"),
                }
            }
            "UID" if signed_in && selected.is_some() => {
                let mut parts = rest.splitn(3, ' ');
                let sub = parts.next().unwrap_or("").to_ascii_uppercase();
                let mailbox = selected.unwrap_or(0);
                match sub.as_str() {
                    "SEARCH" => {
                        let criteria = parts.collect::<Vec<_>>().join(" ");
                        let since = criteria
                            .to_ascii_uppercase()
                            .strip_prefix("SINCE ")
                            .and_then(|_| day(criteria[6..].trim()));
                        let s = lock();
                        let found: Vec<String> = s.mailboxes[mailbox]
                            .messages
                            .iter()
                            .filter(|m| match (since, day(&m.internal)) {
                                (Some(since), Some(at)) => at >= since,
                                _ => true,
                            })
                            .map(|m| m.uid.to_string())
                            .collect();
                        reply = format!("* SEARCH {}\r\n{tag} OK searched\r\n", found.join(" "));
                    }
                    "FETCH" => {
                        let set = parts.next().unwrap_or("");
                        let items = fetch_items(parts.next().unwrap_or(""));
                        let mut s = lock();
                        let seen: Vec<String> =
                            items.iter().filter(|i| sets_seen(i)).cloned().collect();
                        for item in &seen {
                            s.violations.push(format!("FETCH {item} (sets \\Seen)"));
                        }
                        let wanted = uids(set, &s.mailboxes[mailbox]);
                        let mut unknown = Vec::new();
                        for (seq, message) in s.mailboxes[mailbox].messages.iter_mut().enumerate() {
                            if !wanted.contains(&message.uid) {
                                continue;
                            }
                            if !seen.is_empty() && !message.flags.iter().any(|f| f == "\\Seen") {
                                message.flags.push("\\Seen".to_owned());
                            }
                            let mut head = format!("* {} FETCH (", seq + 1);
                            let mut fields = Vec::new();
                            let mut literal: Option<(String, Vec<u8>)> = None;
                            for item in &items {
                                let upper = item.to_ascii_uppercase();
                                match upper.as_str() {
                                    "UID" => fields.push(format!("UID {}", message.uid)),
                                    "FLAGS" => {
                                        fields.push(format!("FLAGS ({})", message.flags.join(" ")))
                                    }
                                    "INTERNALDATE" => fields
                                        .push(format!("INTERNALDATE \"{}\"", message.internal)),
                                    "RFC822.SIZE" => {
                                        fields.push(format!("RFC822.SIZE {}", message.raw.len()))
                                    }
                                    "X-GM-MSGID" => {
                                        fields.push(format!("X-GM-MSGID {}", message.gmail_id))
                                    }
                                    item if item.contains("[HEADER]") => {
                                        let end = message
                                            .raw
                                            .windows(4)
                                            .position(|w| w == b"\r\n\r\n")
                                            .map_or(message.raw.len(), |p| p + 4);
                                        literal = Some((
                                            "BODY[HEADER]".to_owned(),
                                            message.raw[..end].to_vec(),
                                        ));
                                    }
                                    item if item.contains("[]") => {
                                        let most = item
                                            .split('.')
                                            .next_back()
                                            .and_then(|n| {
                                                n.trim_end_matches('>').parse::<usize>().ok()
                                            })
                                            .unwrap_or(usize::MAX);
                                        let bytes =
                                            message.raw[..message.raw.len().min(most)].to_vec();
                                        let key = if item.contains('<') {
                                            "BODY[]<0>"
                                        } else {
                                            "BODY[]"
                                        };
                                        literal = Some((key.to_owned(), bytes));
                                    }
                                    _ => {
                                        unknown.push(format!("FETCH {item} (unknown)"));
                                    }
                                }
                            }
                            head.push_str(&fields.join(" "));
                            literal_reply.extend_from_slice(head.as_bytes());
                            match literal {
                                Some((key, bytes)) => {
                                    literal_reply.extend_from_slice(
                                        format!(" {key} {{{}}}\r\n", bytes.len()).as_bytes(),
                                    );
                                    literal_reply.extend_from_slice(&bytes);
                                    literal_reply.extend_from_slice(b")\r\n");
                                }
                                None => literal_reply.extend_from_slice(b")\r\n"),
                            }
                        }
                        s.violations.extend(unknown);
                        literal_reply.extend_from_slice(format!("{tag} OK fetched\r\n").as_bytes());
                    }
                    other => {
                        lock().violations.push(format!("UID {other}"));
                        reply = format!("{tag} BAD not allowed\r\n");
                    }
                }
            }
            other => {
                lock().violations.push(format!("{other} {rest}"));
                reply = format!("{tag} BAD not allowed\r\n");
            }
        }
        let written = if literal_reply.is_empty() {
            out.write_all(reply.as_bytes())
        } else {
            out.write_all(&literal_reply)
        };
        if written.is_err() {
            return;
        }
    }
}

/// A raw message.
pub fn message(from: &str, to: &str, subject: &str, date: &str, body: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {date}\r\nMessage-ID: <{}@example.org>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}",
        subject.len() + body.len()
    )
    .into_bytes()
}

/// A raw message with an attachment.
pub fn with_attachment(
    from: &str,
    subject: &str,
    date: &str,
    body: &str,
    name: &str,
    content: &str,
) -> Vec<u8> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(content);
    format!(
        "From: {from}\r\nTo: alex@atelier-lune.fr\r\nSubject: {subject}\r\nDate: {date}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n--b1\r\nContent-Type: application/pdf; name=\"{name}\"\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{encoded}\r\n--b1--\r\n"
    )
    .into_bytes()
}
