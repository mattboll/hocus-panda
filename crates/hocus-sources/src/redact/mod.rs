//! Redaction (§14, §14.1): noise removed, then the secrets' first pass,
//! code detectors run before any model (the local one included), each
//! secret replaced by `[secret masqué — voir <source>]`.
//!
//! The detectors, in order: known token formats (`rules.toml`, adapted
//! from gitleaks), identity and bank numbers checked by their keys
//! ([`ids`]), labelled values in French and English ([`labelled`]),
//! random-looking strings ([`entropy`]), and the secrets the process
//! itself holds (a token it was given), wherever they appear.
//!
//! Code alone cannot be exhaustive ("le portail s'ouvre avec 4821"): an
//! item leaves [`Redactor::redact`] as [`Check::FirstPass`], and only the
//! second pass ([`Redacted::second_pass`], the local model's spans, lot 4)
//! makes it [`Check::Done`]. Until then [`Redacted::outbound`] gives only
//! its metadata (fail closed).

pub mod entropy;
pub mod ids;
pub mod labelled;
pub mod noise;

use std::{fmt, ops::Range};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::item::{Content, Item};

/// What a secret was taken for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretKind {
    /// A known token format (GitHub, GitLab, Slack, AWS, a JWT, a key…).
    Token,
    PrivateKey,
    /// A string too random to be a word.
    HighEntropy,
    /// A value its label names (a password, a code, a PIN, a login).
    Labelled,
    /// A one-time code ("482913 est votre code").
    OneTimeCode,
    /// Codes listed under "codes de récupération".
    RecoveryCode,
    Iban,
    Card,
    SocialSecurity,
    Passport,
    IdentityCard,
    TaxNumber,
    /// A secret the process holds (a token it was handed), found as is.
    Known,
    /// Found by the second pass (the local model).
    Model,
}

/// A span found, in bytes of the text searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub kind: SecretKind,
}

impl Found {
    pub fn new(start: usize, end: usize, kind: SecretKind) -> Self {
        Self { start, end, kind }
    }
}

/// Which of an item's texts a mask is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Field {
    Subject,
    Body,
    Attachment(usize),
    Title,
    Description,
    Place,
    Name,
}

/// A secret masked: what it was taken for and where it was, never the
/// secret itself. The original is at the item's pointer; only the user
/// opens it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mask {
    pub kind: SecretKind,
    pub field: Field,
    /// Its place in that text as it was searched (noise removed).
    pub start: usize,
    pub end: usize,
}

/// How far an item's secrets have been checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Check {
    /// Code's pass only: its text does not leave for Claude yet.
    FirstPass,
    /// Both passes: its redacted text may leave.
    Done,
}

/// An item after redaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redacted {
    /// The item, its texts denoised and masked.
    pub item: Item,
    pub masks: Vec<Mask>,
    pub check: Check,
    /// The first pass's rules' version (a change re-runs the corpus).
    pub rules: u32,
}

/// What of an item may go to Claude.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Outbound<'a> {
    /// Both passes ran: the redacted item.
    Checked(&'a Item),
    /// Not checked by the second pass yet: its sender, subject (through
    /// the first pass) and date only, said as possibly holding a secret.
    MetadataOnly {
        pointer: &'a crate::item::Pointer,
        notebook: crate::item::Notebook,
        at: jiff::Timestamp,
        from: Option<&'a str>,
        subject: &'a str,
        note: &'static str,
    },
}

/// What the metadata says of an item not checked yet.
pub const NOT_CHECKED: &str = "peut contenir un secret, pas encore vérifié";

/// The second pass: spans of a text that look like secrets (the local
/// model's `secrets: [{start, end, kind}]`, §14.1, lot 4). It points;
/// code masks.
pub trait SecondPass {
    /// The secrets' spans in `text`, in bytes; an error leaves the item
    /// unchecked (fail closed).
    fn secrets(&mut self, text: &str) -> Result<Vec<Range<usize>>, String>;
}

impl Redacted {
    /// The second pass over every text, masking what it points at. On an
    /// error the item stays [`Check::FirstPass`].
    pub fn second_pass(&mut self, pass: &mut dyn SecondPass) -> Result<(), String> {
        let label = self.item.pointer.label.clone();
        let mut masks = Vec::new();
        for (field, text) in texts(&mut self.item.content) {
            let spans = pass.secrets(text)?;
            let found: Vec<Found> = spans
                .into_iter()
                .filter(|r| r.start < r.end && r.end <= text.len())
                .filter(|r| text.is_char_boundary(r.start) && text.is_char_boundary(r.end))
                .map(|r| Found::new(r.start, r.end, SecretKind::Model))
                .collect();
            let (masked, mut kept) = mask(text, merge(found), field, &label);
            *text = masked;
            masks.append(&mut kept);
        }
        self.masks.append(&mut masks);
        self.check = Check::Done;
        Ok(())
    }

    /// What of it may go to Claude now (§14.1, 3).
    pub fn outbound(&self) -> Outbound<'_> {
        match self.check {
            Check::Done => Outbound::Checked(&self.item),
            Check::FirstPass => {
                let (from, subject) = match &self.item.content {
                    Content::Mail(mail) => (Some(mail.from.as_str()), mail.subject.as_str()),
                    Content::Event(event) => (None, event.title.as_str()),
                    Content::Birthday(birthday) => (None, birthday.name.as_str()),
                };
                Outbound::MetadataOnly {
                    pointer: &self.item.pointer,
                    notebook: self.item.notebook,
                    at: self.item.at,
                    from,
                    subject,
                    note: NOT_CHECKED,
                }
            }
        }
    }
}

/// The first pass's rules, from `rules.toml`.
#[derive(Deserialize)]
struct RuleFile {
    version: u32,
    rule: Vec<RuleEntry>,
}

#[derive(Deserialize)]
struct RuleEntry {
    id: String,
    regex: String,
    #[serde(default)]
    secret: usize,
}

struct Rule {
    id: String,
    regex: Regex,
    secret: usize,
}

/// The first pass.
pub struct Redactor {
    rules: Vec<Rule>,
    version: u32,
    /// Secrets the process holds, masked wherever they appear.
    known: Vec<String>,
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redactor")
            .field(
                "rules",
                &self.rules.iter().map(|r| &r.id).collect::<Vec<_>>(),
            )
            .field("version", &self.version)
            .field("known", &self.known.len())
            .finish()
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

/// The rules shipped, as data.
const RULES: &str = include_str!("rules.toml");

impl Redactor {
    pub fn new() -> Self {
        // The shipped file is checked by this crate's tests: it reads.
        let file: RuleFile = toml::from_str(RULES).unwrap_or_else(|err| {
            tracing::error!(%err, "the secrets' rules do not read: none but the built-in detectors");
            RuleFile {
                version: 0,
                rule: Vec::new(),
            }
        });
        let rules = file
            .rule
            .into_iter()
            .filter_map(|r| match Regex::new(&r.regex) {
                Ok(regex) => Some(Rule {
                    id: r.id,
                    regex,
                    secret: r.secret,
                }),
                Err(err) => {
                    tracing::error!(rule = r.id, %err, "a secrets' rule does not compile: left out");
                    None
                }
            })
            .collect();
        Self {
            rules,
            version: file.version,
            known: Vec::new(),
        }
    }

    /// The rules' version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// `secret` (a token the process was handed, a keyring's value) is
    /// masked wherever it appears.
    pub fn know(&mut self, secret: impl Into<String>) {
        let secret = secret.into();
        if secret.len() >= 4 && !self.known.contains(&secret) {
            self.known.push(secret);
        }
    }

    /// The secrets in `text`, merged and in order.
    pub fn find(&self, text: &str) -> Vec<Found> {
        let mut found = Vec::new();
        for rule in &self.rules {
            for c in rule.regex.captures_iter(text) {
                if let Some(m) = c.get(rule.secret) {
                    let kind = if rule.id == "private-key" {
                        SecretKind::PrivateKey
                    } else {
                        SecretKind::Token
                    };
                    found.push(Found::new(m.start(), m.end(), kind));
                }
            }
        }
        ids::find(text, &mut found);
        labelled::find(text, &mut found);
        entropy::find(text, &mut found);
        for secret in &self.known {
            for (at, _) in text.match_indices(secret.as_str()) {
                found.push(Found::new(at, at + secret.len(), SecretKind::Known));
            }
        }
        merge(found)
    }

    /// `text` with its secrets masked, each saying to look at `source`.
    pub fn mask_text(&self, text: &str, source: &str) -> String {
        mask(text, self.find(text), Field::Body, source).0
    }

    /// `item` denoised (a mail's body) and masked, every text of it.
    pub fn redact(&self, mut item: Item) -> Redacted {
        if let Content::Mail(mail) = &mut item.content
            && let Some(body) = &mut mail.body
        {
            *body = noise::denoise(body);
        }
        let label = item.pointer.label.clone();
        let mut masks = Vec::new();
        for (field, text) in texts(&mut item.content) {
            let found = self.find(text);
            let (masked, mut kept) = mask(text, found, field, &label);
            *text = masked;
            masks.append(&mut kept);
        }
        Redacted {
            item,
            masks,
            check: Check::FirstPass,
            rules: self.version,
        }
    }
}

/// An item's texts, each with its field.
fn texts(content: &mut Content) -> Vec<(Field, &mut String)> {
    let mut out = Vec::new();
    match content {
        Content::Mail(mail) => {
            out.push((Field::Subject, &mut mail.subject));
            if let Some(body) = &mut mail.body {
                out.push((Field::Body, body));
            }
            for (i, name) in mail.attachments.iter_mut().enumerate() {
                out.push((Field::Attachment(i), name));
            }
        }
        Content::Event(event) => {
            out.push((Field::Title, &mut event.title));
            if let Some(description) = &mut event.description {
                out.push((Field::Description, description));
            }
            if let Some(place) = &mut event.place {
                out.push((Field::Place, place));
            }
        }
        Content::Birthday(birthday) => out.push((Field::Name, &mut birthday.name)),
    }
    out
}

/// Spans sorted, overlapping ones joined (the first's kind kept).
fn merge(mut found: Vec<Found>) -> Vec<Found> {
    found.sort_by_key(|f| (f.start, std::cmp::Reverse(f.end)));
    let mut merged: Vec<Found> = Vec::with_capacity(found.len());
    for f in found {
        if f.start >= f.end {
            continue;
        }
        match merged.last_mut() {
            Some(last) if f.start < last.end => last.end = last.end.max(f.end),
            _ => merged.push(f),
        }
    }
    merged
}

/// What a secret becomes.
pub fn placeholder(source: &str) -> String {
    format!("[secret masqué — voir {source}]")
}

fn mask(text: &str, found: Vec<Found>, field: Field, source: &str) -> (String, Vec<Mask>) {
    if found.is_empty() {
        return (text.to_owned(), Vec::new());
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut masks = Vec::with_capacity(found.len());
    for f in found {
        out.push_str(&text[at..f.start]);
        out.push_str(&placeholder(source));
        at = f.end;
        masks.push(Mask {
            kind: f.kind,
            field,
            start: f.start,
            end: f.end,
        });
    }
    out.push_str(&text[at..]);
    (out, masks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_rules_read_and_compile() {
        let file: RuleFile = toml::from_str(RULES).expect("rules.toml reads");
        let redactor = Redactor::new();
        assert_eq!(redactor.rules.len(), file.rule.len());
        assert!(redactor.version() >= 1);
    }

    #[test]
    fn token_formats_are_masked() {
        let redactor = Redactor::new();
        for token in [
            // Split, so that a forge's secret scanning does not take these
            // made-up tokens for real ones.
            concat!("gh", "p_0123456789abcdefghijABCDEFGHIJ012345"),
            concat!("gl", "pat-AbCdEf0123456789xYz_"),
            concat!("xo", "xb-1234567890-0987654321-AbCdEfGhIjKl"),
            concat!("AK", "IAIOSFODNN7EXAMPLE"),
            concat!(
                "ey",
                "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"
            ),
        ] {
            let text = format!("le jeton : {token} merci");
            let masked = redactor.mask_text(&text, "le mail de Léa");
            assert!(!masked.contains(token), "{token} in {masked}");
            assert!(masked.contains("[secret masqué — voir le mail de Léa]"));
        }
    }

    #[test]
    fn a_private_key_is_masked_whole() {
        let text = "voici la clé\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\nmerci";
        let masked = Redactor::new().mask_text(text, "x");
        assert_eq!(masked, "voici la clé\n[secret masqué — voir x]\nmerci");
    }

    #[test]
    fn known_secrets_are_masked_wherever_they_are() {
        let mut redactor = Redactor::new();
        redactor.know("tok-1");
        assert_eq!(
            redactor.mask_text("Bearer tok-1 here", "y"),
            "Bearer [secret masqué — voir y] here"
        );
    }
}
