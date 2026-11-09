//! The models she calls (§11), behind one trait and one request shape:
//! the providers' adapters (`crate::providers`: Anthropic's Messages API,
//! OpenAI-compatible servers) and the harness's **scripted fake model**: it
//! answers from a script, keyed by the request's features (its role, the
//! words it contains), after a delay on her clock if the script says so,
//! and the **recorder** writes down what each call contained, so that a
//! test checks what reached a model (a secret, a perso item, a fact that
//! code should have computed) without any network.

use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::clock::Clock;

/// What a call is for (§11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// The local model's triage of an item.
    Triage,
    /// An answer to the user.
    #[default]
    Answer,
    /// The morning briefing.
    Briefing,
    /// The nightly compaction.
    Compaction,
    /// Which of the triage's candidate facts become proposals (§7.1).
    Extraction,
}

impl Role {
    pub const ALL: [Self; 5] = [
        Self::Triage,
        Self::Answer,
        Self::Briefing,
        Self::Compaction,
        Self::Extraction,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Answer => "answer",
            Self::Briefing => "briefing",
            Self::Compaction => "compaction",
            Self::Extraction => "extraction",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.word() == word)
    }
}

/// A part of the context, with the items it was made from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub label: String,
    pub text: String,
    /// The items it holds, by id (`edge:12`, `fixture-mail:m3`): the audit
    /// log keeps them, never the text.
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Message {
    /// `user` or `assistant`.
    pub from: String,
    pub text: String,
    /// An assistant's turn: the tools it called.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// A user's turn after tool calls: what they gave.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResult>,
    /// An assistant's turn as its provider gave it ([`Reply::native`]),
    /// replayed unchanged to the same provider (its thinking blocks must
    /// come back as they were).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<serde_json::Value>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            from: "user".to_owned(),
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            from: "assistant".to_owned(),
            text: text.into(),
            ..Self::default()
        }
    }

    /// A model's reply as the next request's assistant turn.
    pub fn replied(reply: &Reply) -> Self {
        Self {
            from: "assistant".to_owned(),
            text: reply.text.clone(),
            tool_calls: reply.tool_calls.clone(),
            tool_results: Vec::new(),
            native: reply.native.clone(),
        }
    }
}

/// What a tool gave, for the call `id`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub error: bool,
}

/// A call to a model.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Request {
    pub role: Role,
    pub system: String,
    pub context: Vec<Part>,
    pub messages: Vec<Message>,
    /// The tools offered, by name.
    pub tools: Vec<String>,
}

impl Request {
    /// Every item it holds.
    pub fn items(&self) -> Vec<String> {
        self.context.iter().flat_map(|p| p.items.clone()).collect()
    }

    /// Everything a model would read of it: the script's keys, and what
    /// tests search for a secret.
    pub fn text(&self) -> String {
        let mut text = self.system.clone();
        for part in &self.context {
            text.push('\n');
            text.push_str(&part.text);
        }
        for message in &self.messages {
            text.push('\n');
            text.push_str(&message.text);
        }
        text
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ToolCall {
    /// The provider's id for it, which its result repeats.
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub input: serde_json::Value,
}

/// A model's reply.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Reply {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// Who answered: the provider and model the role was set to.
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    /// Input tokens read from, and written to, the provider's cache.
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_written: u64,
    /// Why it stopped, in the provider's word (`end_turn`, `tool_use`,
    /// `max_tokens`, `refusal`).
    #[serde(default)]
    pub stop: String,
    /// The reply's content as the provider gave it, for the next turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<serde_json::Value>,
}

/// A piece of a reply as it comes (§5.4: she shows what she is doing,
/// then her answer streams).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece<'a> {
    /// What she is doing, from a tool call ("elle regarde le tableau de
    /// Chapitô").
    Doing(&'a str),
    /// The next words of her answer.
    Text(&'a str),
}

/// A model: called on a worker thread, never on her loop.
pub trait Model: Send + Sync + std::fmt::Debug {
    fn provider(&self) -> &str;
    fn model(&self) -> &str;
    fn call(&self, request: &Request, clock: &Clock) -> Result<Reply, String>;

    /// The same call, its text handed to `part` as it comes (the drawer's
    /// streamed answer); a model that cannot stream gives it at once.
    fn stream(
        &self,
        request: &Request,
        clock: &Clock,
        part: &mut dyn FnMut(&str),
    ) -> Result<Reply, String> {
        let reply = self.call(request, clock)?;
        if !reply.text.is_empty() {
            part(&reply.text);
        }
        Ok(reply)
    }

    /// [`Model::stream`], with what she is doing too ([`Piece::Doing`]):
    /// the drawer's typing line. A model that says nothing of it streams
    /// its text alone.
    fn stream_pieces(
        &self,
        request: &Request,
        clock: &Clock,
        piece: &mut dyn FnMut(Piece<'_>),
    ) -> Result<Reply, String> {
        self.stream(request, clock, &mut |text| piece(Piece::Text(text)))
    }

    /// Its name as the drawer shows it under the field.
    fn shown(&self) -> String {
        format!("{} · {}", self.provider(), self.model())
    }
}

/// The fake model's script, a TOML file:
///
/// ```toml
/// [[reply]]
/// role = "answer"            # optional: any role
/// contains = ["v2", "Chapitô"]   # optional: all of them, in the request
/// text = "Nadia et Sophie."
/// delay = 30                 # optional: seconds on her clock
/// [[reply.tool_calls]]
/// name = "graph"
/// input = { node = "Chapitô" }
///
/// pace = 1                   # optional: seconds on her clock between words
/// doing = "elle regarde le tableau de Chapitô"   # optional: said before
///
/// [default]                  # optional: when no reply matches
/// text = "D'accord."
/// ```
///
/// `name` at the top (optional) is the model's name the drawer shows
/// (screenshots); else "fake · scripted".
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub reply: Vec<Scripted>,
    #[serde(default)]
    pub default: Option<Scripted>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scripted {
    #[serde(default)]
    pub role: Option<Role>,
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub delay: u64,
    /// Seconds on her clock between two words of the text as it streams.
    #[serde(default)]
    pub pace: u64,
    /// What she says she is doing while she composes.
    #[serde(default)]
    pub doing: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

impl Scripted {
    fn matches(&self, request: &Request, text: &str) -> bool {
        self.role.is_none_or(|r| r == request.role)
            && self.contains.iter().all(|w| text.contains(w.as_str()))
    }
}

/// The scripted fake model.
#[derive(Debug)]
pub struct Fake {
    script: Script,
    recorder: Option<Arc<Recorder>>,
}

impl Fake {
    pub fn new(script: Script, recorder: Option<Arc<Recorder>>) -> Self {
        Self { script, recorder }
    }

    pub fn from_file(path: &Path, recorder: Option<Arc<Recorder>>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let script = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self::new(script, recorder))
    }
}

impl Model for Fake {
    fn provider(&self) -> &str {
        "fake"
    }

    fn model(&self) -> &str {
        "scripted"
    }

    fn call(&self, request: &Request, clock: &Clock) -> Result<Reply, String> {
        self.stream_pieces(request, clock, &mut |_| {})
    }

    fn stream(
        &self,
        request: &Request,
        clock: &Clock,
        part: &mut dyn FnMut(&str),
    ) -> Result<Reply, String> {
        self.stream_pieces(request, clock, &mut |piece| {
            if let Piece::Text(text) = piece {
                part(text);
            }
        })
    }

    fn stream_pieces(
        &self,
        request: &Request,
        clock: &Clock,
        piece: &mut dyn FnMut(Piece<'_>),
    ) -> Result<Reply, String> {
        let started = clock.now();
        let text = request.text();
        let found = self
            .script
            .reply
            .iter()
            .position(|r| r.matches(request, &text));
        let scripted = found
            .and_then(|i| self.script.reply.get(i))
            .or(self.script.default.as_ref());
        if let Some(recorder) = &self.recorder {
            recorder.call(started, request, found);
        }
        let Some(scripted) = scripted else {
            return Err("the fake model has no reply for this request".to_owned());
        };
        if !scripted.doing.is_empty() {
            piece(Piece::Doing(&scripted.doing));
        }
        let mut at = started;
        if scripted.delay > 0 {
            at = at
                .checked_add(seconds(scripted.delay))
                .unwrap_or(Timestamp::MAX);
            clock.sleep_until(at);
        }
        // Word by word, each with the spaces before it.
        for (n, word) in words_of(&scripted.text).into_iter().enumerate() {
            if n > 0 && scripted.pace > 0 {
                at = at
                    .checked_add(seconds(scripted.pace))
                    .unwrap_or(Timestamp::MAX);
                clock.sleep_until(at);
            }
            piece(Piece::Text(word));
        }
        Ok(Reply {
            text: scripted.text.clone(),
            tool_calls: scripted.tool_calls.clone(),
            // A word is about a token and a half: enough for the counts.
            tokens_in: words(&text),
            tokens_out: words(&scripted.text),
            provider: "fake".to_owned(),
            model: "scripted".to_owned(),
            ..Reply::default()
        })
    }

    fn shown(&self) -> String {
        self.script
            .name
            .clone()
            .unwrap_or_else(|| "fake · scripted".to_owned())
    }
}

fn seconds(n: u64) -> SignedDuration {
    SignedDuration::from_secs(i64::try_from(n).unwrap_or(i64::MAX))
}

/// `text` cut before each word, the spaces before a word with it: the
/// pieces join back into it.
fn words_of(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let (mut start, mut cut) = (0, None);
    let mut word = false;
    for (at, c) in text.char_indices() {
        if c.is_whitespace() {
            if word {
                cut = Some(at);
            }
            word = false;
        } else {
            if let Some(cut) = cut.take()
                && cut > start
            {
                pieces.push(&text[start..cut]);
                start = cut;
            }
            word = true;
        }
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
}

fn words(text: &str) -> u64 {
    u64::try_from(text.split_whitespace().count() * 3 / 2).unwrap_or(u64::MAX)
}

/// One line the recorder wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Recorded {
    /// A model call: the whole request, and which scripted reply answered.
    Call {
        at: String,
        request: Request,
        scripted: Option<usize>,
    },
    /// An outside effect (a message sent, a file written, a task added):
    /// none may happen before the user's click (§2.1, AC24).
    Effect {
        at: String,
        kind: String,
        detail: String,
    },
}

/// Writes down every model call and every outside effect, one JSON object
/// per line, in a file a test reads (`--recorder`), or in memory.
#[derive(Debug)]
pub struct Recorder {
    file: Option<PathBuf>,
    lines: Mutex<Vec<Recorded>>,
}

impl Recorder {
    pub fn to_file(file: PathBuf) -> Self {
        Self {
            file: Some(file),
            lines: Mutex::new(Vec::new()),
        }
    }

    pub fn in_memory() -> Self {
        Self {
            file: None,
            lines: Mutex::new(Vec::new()),
        }
    }

    pub fn call(&self, at: Timestamp, request: &Request, scripted: Option<usize>) {
        self.write(Recorded::Call {
            at: at.to_string(),
            request: request.clone(),
            scripted,
        });
    }

    pub fn effect(&self, at: Timestamp, kind: &str, detail: &str) {
        self.write(Recorded::Effect {
            at: at.to_string(),
            kind: kind.to_owned(),
            detail: detail.to_owned(),
        });
    }

    fn write(&self, line: Recorded) {
        if let Some(file) = &self.file {
            let written = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)
                .and_then(|mut f| f.write_all(hocus_secretary_link::line(&line).as_bytes()));
            if let Err(err) = written {
                tracing::warn!(file = %file.display(), %err, "the recorder could not write");
            }
        }
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    /// What it recorded so far.
    pub fn lines(&self) -> Vec<Recorded> {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// What a recorder wrote to `file`.
    pub fn read(file: &Path) -> Vec<Recorded> {
        std::fs::read_to_string(file)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| hocus_secretary_link::read(l).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(role: Role, text: &str) -> Request {
        Request {
            role,
            system: "Tu es la secrétaire.".to_owned(),
            context: vec![Part {
                label: "the day".to_owned(),
                text: "Réunion à 14 h".to_owned(),
                items: vec!["fixture-cal:e1".to_owned()],
            }],
            messages: vec![Message::user(text)],
            tools: vec!["graph".to_owned()],
        }
    }

    #[test]
    fn the_script_answers_by_role_and_words_and_the_recorder_keeps_each_call() {
        let script: Script = toml::from_str(
            r#"
            [[reply]]
            role = "answer"
            contains = ["v2"]
            text = "Nadia et Sophie."
            [[reply.tool_calls]]
            name = "graph"
            input = { node = "Chapitô" }

            [[reply]]
            role = "triage"
            text = "{}"

            [default]
            text = "D'accord."
            "#,
        )
        .expect("a script");
        let clock = Clock::virtual_at("2026-11-09T08:00:00Z".parse().expect("a time"));
        let fake = Fake::new(script, Some(Arc::new(Recorder::in_memory())));
        let reply = fake
            .call(&asked(Role::Answer, "Qui était là pour la v2 ?"), &clock)
            .expect("a reply");
        assert_eq!(reply.text, "Nadia et Sophie.");
        assert_eq!(reply.tool_calls[0].name, "graph");
        let reply = fake
            .call(&asked(Role::Answer, "Merci"), &clock)
            .expect("a reply");
        assert_eq!(reply.text, "D'accord.");
        let reply = fake
            .call(&asked(Role::Triage, "v2"), &clock)
            .expect("a reply");
        assert_eq!(reply.text, "{}");
        let lines = fake.recorder.as_ref().expect("a recorder").lines();
        assert_eq!(lines.len(), 3);
        let Recorded::Call {
            request, scripted, ..
        } = &lines[0]
        else {
            panic!("a call: {lines:?}");
        };
        assert_eq!(*scripted, Some(0));
        assert_eq!(request.items(), ["fixture-cal:e1"]);
        assert!(matches!(&lines[1], Recorded::Call { scripted: None, .. }));
    }

    #[test]
    fn a_reply_streams_word_by_word_at_its_pace() {
        let script: Script = toml::from_str(
            r#"
            [[reply]]
            text = "Bien noté, Alex."
            pace = 2
            doing = "elle regarde"
            "#,
        )
        .expect("a script");
        let clock = Clock::virtual_at("2026-11-09T08:00:00Z".parse().expect("a time"));
        let fake = Arc::new(Fake::new(script, None));
        let (pieces, read) = std::sync::mpsc::channel();
        let (model, on) = (fake.clone(), clock.clone());
        let call = std::thread::spawn(move || {
            model.stream_pieces(&asked(Role::Answer, "x"), &on, &mut |p| {
                let p = match p {
                    Piece::Doing(d) => format!("doing:{d}"),
                    Piece::Text(t) => t.to_owned(),
                };
                let _ = pieces.send(p);
            })
        });
        let next = || {
            read.recv_timeout(std::time::Duration::from_secs(5))
                .expect("a piece")
        };
        assert_eq!(next(), "doing:elle regarde");
        assert_eq!(next(), "Bien");
        // The next word waits for her clock.
        assert!(
            read.recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        clock.advance(std::time::Duration::from_secs(2));
        assert_eq!(next(), " noté,");
        clock.advance(std::time::Duration::from_secs(2));
        assert_eq!(next(), " Alex.");
        let reply = call.join().expect("the call").expect("a reply");
        assert_eq!(reply.text, "Bien noté, Alex.");
        assert_eq!(words_of("  a b"), ["  a", " b"]);
    }
}
