//! IMAP's wire: lines and literals read, commands written through the
//! allow-list's guard, and the few answers read (`EXAMINE`'s codes,
//! `LIST`, `SEARCH`, `FETCH`).

use std::io::{BufRead, BufReader, Read, Write};

use super::allow::{self, Command};
use crate::{Result, SourceError};

/// A line the server sent, its literals apart: `{n}` in `text` stands for
/// the next of `literals`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub literals: Vec<Vec<u8>>,
}

/// What a command was answered.
#[derive(Debug, Default)]
pub struct Answer {
    pub untagged: Vec<Line>,
    /// `OK`, `NO` or `BAD`, and what follows.
    pub status: String,
    pub said: String,
}

/// The largest literal read (a message's first megabytes, a header).
const LARGEST: usize = 16 * 1024 * 1024;

/// A connection, after its greeting.
pub struct Session<S: Read + Write> {
    stream: BufReader<S>,
    tag: u32,
}

impl<S: Read + Write> std::fmt::Debug for Session<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("tag", &self.tag).finish()
    }
}

fn offline(err: std::io::Error) -> SourceError {
    SourceError::Offline(format!("IMAP: {err}"))
}

impl<S: Read + Write> Session<S> {
    /// The server's greeting read on `stream`.
    pub fn start(stream: S) -> Result<Self> {
        let mut session = Self {
            stream: BufReader::new(stream),
            tag: 0,
        };
        let greeting = session.line()?;
        let upper = greeting.text.to_ascii_uppercase();
        if upper.starts_with("* OK") || upper.starts_with("* PREAUTH") {
            Ok(session)
        } else {
            Err(SourceError::Offline(format!(
                "IMAP greets: {}",
                greeting.text
            )))
        }
    }

    /// One line, with the literals it announces.
    fn line(&mut self) -> Result<Line> {
        let mut line = Line::default();
        loop {
            let mut raw = Vec::new();
            let read = (&mut self.stream)
                .take(64 * 1024)
                .read_until(b'\n', &mut raw)
                .map_err(offline)?;
            if read == 0 {
                return Err(SourceError::Offline("IMAP: the server closed".to_owned()));
            }
            let text = String::from_utf8_lossy(&raw);
            let text = text.trim_end_matches(['\r', '\n']);
            line.text.push_str(text);
            // A literal follows: `{n}` at the end of the line.
            let Some(open) = text.rfind('{').filter(|_| text.ends_with('}')) else {
                return Ok(line);
            };
            let size: usize = text[open + 1..text.len() - 1]
                .trim_end_matches('+')
                .parse()
                .map_err(|_| SourceError::Refused(format!("IMAP: a literal of {text}")))?;
            if size > LARGEST {
                return Err(SourceError::Refused(format!(
                    "IMAP: a literal of {size} bytes"
                )));
            }
            let mut literal = vec![0; size];
            self.stream.read_exact(&mut literal).map_err(offline)?;
            line.literals.push(literal);
        }
    }

    /// `command` sent, through the guard, and its answer read. A `+`
    /// continuation (XOAUTH2's error) is answered with an empty line.
    pub fn run(&mut self, command: &Command) -> Result<Answer> {
        self.tag += 1;
        let tag = format!("h{}", self.tag);
        let line = command.render(&tag).ok_or_else(|| {
            SourceError::Invalid(format!("IMAP: {command:?} cannot be sent safely"))
        })?;
        allow::check(&line).map_err(|refused| SourceError::Invalid(refused.to_string()))?;
        let stream = self.stream.get_mut();
        stream.write_all(line.as_bytes()).map_err(offline)?;
        stream.flush().map_err(offline)?;
        let mut answer = Answer::default();
        loop {
            let line = self.line()?;
            if line.text.starts_with('+') {
                let stream = self.stream.get_mut();
                stream.write_all(b"\r\n").map_err(offline)?;
                stream.flush().map_err(offline)?;
                continue;
            }
            if let Some(rest) = line.text.strip_prefix(&format!("{tag} ")) {
                let (status, said) = rest.split_once(' ').unwrap_or((rest, ""));
                answer.status = status.to_ascii_uppercase();
                answer.said = said.to_owned();
                return Ok(answer);
            }
            answer.untagged.push(line);
        }
    }

    /// `command`, which must be answered `OK`.
    pub fn ok(&mut self, command: &Command) -> Result<Answer> {
        let answer = self.run(command)?;
        match answer.status.as_str() {
            "OK" => Ok(answer),
            _ if matches!(command, Command::Authenticate { .. }) => Err(SourceError::SignIn(
                format!("IMAP refuses the token: {}", answer.said),
            )),
            _ => Err(SourceError::Refused(format!(
                "IMAP answers {} {}",
                answer.status, answer.said
            ))),
        }
    }
}

/// A value of an answer's parenthesised data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Atom(String),
    Text(Vec<u8>),
    List(Vec<Value>),
    Nil,
}

impl Value {
    pub fn atom(&self) -> Option<&str> {
        match self {
            Self::Atom(a) => Some(a),
            _ => None,
        }
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Text(t) => Some(t),
            _ => None,
        }
    }
}

/// The values of `line` from byte `at`, literals taken in order.
pub fn values(line: &Line, at: usize) -> Vec<Value> {
    let bytes = line.text.as_bytes();
    let mut literals = line.literals.iter();
    let mut i = at;
    let mut stack: Vec<Vec<Value>> = vec![Vec::new()];
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' => i += 1,
            b'(' => {
                stack.push(Vec::new());
                i += 1;
            }
            b')' => {
                let list = stack.pop().unwrap_or_default();
                match stack.last_mut() {
                    Some(parent) => parent.push(Value::List(list)),
                    None => stack.push(vec![Value::List(list)]),
                }
                i += 1;
            }
            b'"' => {
                let mut text = Vec::new();
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                    }
                    text.push(bytes[i]);
                    i += 1;
                }
                i += 1;
                if let Some(top) = stack.last_mut() {
                    top.push(Value::Text(text));
                }
            }
            b'{' => {
                // The literal's place: skip to its closing brace.
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                i += 1;
                let literal = literals.next().cloned().unwrap_or_default();
                if let Some(top) = stack.last_mut() {
                    top.push(Value::Text(literal));
                }
            }
            _ => {
                // An atom; a section in brackets is part of it, spaces and
                // all ("BODY[HEADER.FIELDS (FROM)]").
                let start = i;
                let mut depth = 0usize;
                while i < bytes.len() {
                    match bytes[i] {
                        b'[' => depth += 1,
                        b']' => depth = depth.saturating_sub(1),
                        b' ' | b')' | b'(' if depth == 0 => break,
                        _ => {}
                    }
                    i += 1;
                }
                let atom = String::from_utf8_lossy(&bytes[start..i]).into_owned();
                if let Some(top) = stack.last_mut() {
                    top.push(if atom.eq_ignore_ascii_case("NIL") {
                        Value::Nil
                    } else {
                        Value::Atom(atom)
                    });
                }
            }
        }
    }
    stack.into_iter().flatten().collect()
}

/// A response code's number: `[UIDVALIDITY 3]` in `* OK [UIDVALIDITY 3]`.
pub fn code(untagged: &[Line], name: &str) -> Option<u64> {
    untagged.iter().find_map(|line| {
        let upper = line.text.to_ascii_uppercase();
        let at = upper.find(&format!("[{name} "))?;
        let rest = &line.text[at + name.len() + 2..];
        rest.split(']').next()?.trim().parse().ok()
    })
}

/// The mailbox `LIST` marks with `attribute` (`\Sent`).
pub fn mailbox_with(untagged: &[Line], attribute: &str) -> Option<String> {
    untagged.iter().find_map(|line| {
        if !line.text.to_ascii_uppercase().starts_with("* LIST ") {
            return None;
        }
        let values = values(line, "* LIST ".len());
        let flags = match values.first()? {
            Value::List(flags) => flags,
            _ => return None,
        };
        flags
            .iter()
            .any(|f| f.atom().is_some_and(|a| a.eq_ignore_ascii_case(attribute)))
            .then(|| match values.get(2)? {
                Value::Text(name) => Some(String::from_utf8_lossy(name).into_owned()),
                Value::Atom(name) => Some(name.clone()),
                _ => None,
            })
            .flatten()
    })
}

/// `SEARCH`'s numbers.
pub fn searched(untagged: &[Line]) -> Vec<u32> {
    untagged
        .iter()
        .filter_map(|l| l.text.strip_prefix("* SEARCH"))
        .flat_map(|rest| rest.split_whitespace().filter_map(|n| n.parse().ok()))
        .collect()
}

/// A message as `FETCH` gave it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fetched {
    pub uid: u32,
    pub flags: Vec<String>,
    pub internal_date: Option<String>,
    pub size: Option<u64>,
    pub gmail_id: Option<u64>,
    /// `BODY[HEADER]`'s or `BODY[]`'s bytes.
    pub bytes: Option<Vec<u8>>,
}

/// Each message of `FETCH`'s answers.
pub fn fetched(untagged: &[Line]) -> Vec<Fetched> {
    let mut out = Vec::new();
    for line in untagged {
        let upper = line.text.to_ascii_uppercase();
        let Some(at) = upper.find(" FETCH ") else {
            continue;
        };
        if !upper.starts_with("* ") {
            continue;
        }
        let values = values(line, at + " FETCH ".len());
        let Some(Value::List(pairs)) = values.first() else {
            continue;
        };
        let mut message = Fetched::default();
        for pair in pairs.chunks(2) {
            let (Some(key), Some(value)) = (pair[0].atom(), pair.get(1)) else {
                continue;
            };
            let key = key.to_ascii_uppercase();
            match key.as_str() {
                "UID" => message.uid = value.atom().and_then(|v| v.parse().ok()).unwrap_or(0),
                "FLAGS" => {
                    if let Value::List(flags) = value {
                        message.flags = flags
                            .iter()
                            .filter_map(|f| f.atom().map(str::to_owned))
                            .collect();
                    }
                }
                "INTERNALDATE" => {
                    message.internal_date = value
                        .bytes()
                        .map(|b| String::from_utf8_lossy(b).into_owned());
                }
                "RFC822.SIZE" => message.size = value.atom().and_then(|v| v.parse().ok()),
                "X-GM-MSGID" => message.gmail_id = value.atom().and_then(|v| v.parse().ok()),
                key if key.starts_with("BODY[") => {
                    message.bytes = value.bytes().map(<[u8]>::to_vec)
                }
                _ => {}
            }
        }
        if message.uid != 0 {
            out.push(message);
        }
    }
    out
}

/// `INTERNALDATE`'s form: `05-Nov-2026 10:12:00 +0100`.
pub fn internal_date(text: &str) -> Option<jiff::Timestamp> {
    jiff::fmt::strtime::parse("%d-%b-%Y %H:%M:%S %z", text.trim())
        .ok()?
        .to_timestamp()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, literals: &[&[u8]]) -> Line {
        Line {
            text: text.to_owned(),
            literals: literals.iter().map(|l| l.to_vec()).collect(),
        }
    }

    #[test]
    fn a_fetch_answer_reads() {
        let answer = [line(
            "* 3 FETCH (UID 42 FLAGS (\\Seen \\Flagged) INTERNALDATE \"05-Nov-2026 10:12:00 +0100\" X-GM-MSGID 1780000000000000001 BODY[HEADER] {14})",
            &[b"Subject: Hi\r\n\r\n"],
        )];
        let fetched = fetched(&answer);
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].uid, 42);
        assert_eq!(fetched[0].flags, ["\\Seen", "\\Flagged"]);
        assert_eq!(fetched[0].gmail_id, Some(1_780_000_000_000_000_001));
        assert_eq!(
            fetched[0].bytes.as_deref(),
            Some(&b"Subject: Hi\r\n\r\n"[..])
        );
        assert_eq!(
            internal_date("05-Nov-2026 10:12:00 +0100").map(|t| t.as_second()),
            Some(1_793_869_920)
        );
    }

    #[test]
    fn the_sent_mailbox_is_found_by_its_attribute() {
        let answer = [
            line("* LIST (\\HasNoChildren) \"/\" \"INBOX\"", &[]),
            line(
                "* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Messages envoy&AOk-s\"",
                &[],
            ),
        ];
        assert_eq!(
            mailbox_with(&answer, "\\Sent").as_deref(),
            Some("[Gmail]/Messages envoy&AOk-s")
        );
        let codes = [line("* OK [UIDVALIDITY 7] UIDs valid", &[])];
        assert_eq!(code(&codes, "UIDVALIDITY"), Some(7));
    }
}
