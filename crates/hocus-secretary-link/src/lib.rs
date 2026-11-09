//! The private line to the secretary's process (`hocus-secretary`,
//! docs/design/secretary.md §16): one JSON object per line, each way.
//!
//! The compositor starts her with her socket's path ([`SOCKET_ENV`]), next
//! to the Wayland socket (`<socket>.secretary`, [`socket_path`]), in the
//! session's runtime directory, `0600`. The shell and the `hocus` command
//! connect to it; the compositor never does: it only starts and supervises
//! her.
//!
//! Every connection starts with [`ToSecretary::Hello`], which names the
//! protocol's [`VERSION`]; she answers [`FromSecretary::Hello`] with hers,
//! or [`FromSecretary::Refused`] and closes when they differ. A request
//! carries an `id` that its answer repeats; facts (the session locked, the
//! machine resumed) have none and no answer. A line that does not read is
//! answered with an [`FromSecretary::Error`] and the connection stays: a
//! newer client's new message is refused, not fatal.
//!
//! Neither side waits on the other on a loop: the shell writes and reads
//! her lines on a thread of its own.
//!
//! **Version 2** brings the drawer's conversation (§5.4, lot 2): the
//! user's sentences ([`ToSecretary::Ask`]), a quick reply chosen
//! ([`ToSecretary::Choose`]), a card's button ([`ToSecretary::Decide`]),
//! "Annuler" under what she kept ([`ToSecretary::Undo`]), the drawer open
//! or closed; and from her, the thread's items ([`FromSecretary::Item`],
//! the same id again when one changes), her typing
//! ([`FromSecretary::Typing`]), her answer's text as it comes
//! ([`FromSecretary::Answer`]), and her face and status line
//! ([`FromSecretary::Look`]).

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

pub mod consent;

/// The protocol's version: a change that an older side would misread
/// raises it.
pub const VERSION: u32 = 2;

/// The environment variable that names her socket, for her and for the
/// programs the compositor starts.
pub const SOCKET_ENV: &str = "HOCUS_SECRETARY_SOCKET";

/// The environment variable naming the session bus whose keyring holds her
/// memory's key: an address, or `session`; unset, the key is in a file.
pub const BUS_ENV: &str = "HOCUS_SECRETARY_BUS";

/// Her data folder: `$XDG_DATA_HOME/hocuspanda/secretary`, or under
/// `~/.local/share`; `None` without an absolute one. Her memory and her
/// consent book ([`consent::CONSENTS`]) are there.
pub fn data_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let data = var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    data.is_absolute()
        .then(|| data.join("hocuspanda/secretary"))
}

/// Her socket's path next to the Wayland socket's.
pub fn socket_path(wayland: &Path) -> PathBuf {
    let mut path = wayland.as_os_str().to_owned();
    path.push(".secretary");
    PathBuf::from(path)
}

/// Her socket's path next to the compositor's control socket
/// (`<socket>.control`), as the `hocus` command is given it.
pub fn beside_control(control: &Path) -> Option<PathBuf> {
    let name = control.file_name()?.to_str()?;
    let wayland = name.strip_suffix(".control")?;
    Some(socket_path(&control.with_file_name(wayland)))
}

/// Who is on the other end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Client {
    Shell,
    Cli,
    Test,
}

/// The two notebooks every item of her memory is in (§7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Notebook {
    Pro,
    Perso,
}

impl Notebook {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Pro => "pro",
            Self::Perso => "perso",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        match word {
            "pro" => Some(Self::Pro),
            "perso" => Some(Self::Perso),
            _ => None,
        }
    }
}

/// Where a fact comes from (§7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provenance {
    /// The user said it.
    Told,
    /// Read explicitly in a source.
    Extracted,
    /// Her deduction: a proposal until accepted.
    Inferred,
}

impl Provenance {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Told => "told",
            Self::Extracted => "extracted",
            Self::Inferred => "inferred",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        match word {
            "told" => Some(Self::Told),
            "extracted" => Some(Self::Extracted),
            "inferred" => Some(Self::Inferred),
            _ => None,
        }
    }
}

/// A line to her.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToSecretary {
    /// The first line of a connection; `lang`, the desktop's language
    /// (`fr`, `en`), for the words she makes herself.
    Hello {
        version: u32,
        client: Client,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lang: Option<String>,
    },
    /// How she is: [`FromSecretary::Status`].
    Status { id: u64 },
    /// Her graph around a node, now or as of a date:
    /// [`FromSecretary::Graph`].
    Graph { id: u64, query: GraphQuery },
    /// A sentence of the user's: her answer comes in
    /// [`FromSecretary::Answer`]s, the last one `done`; to a shell, the
    /// sentence and what she makes of it come as thread items too.
    Ask { id: u64, text: String },
    /// Quick reply `choice` (from 0) of question `item` clicked: answered
    /// as a sentence is.
    Choose { id: u64, item: u64, choice: u32 },
    /// A card's button: card `item` accepted, rejected, or to edit. An
    /// outside effect happens only after an accept, on that exact card
    /// (§2.1).
    Decide {
        id: u64,
        item: u64,
        decision: Decision,
    },
    /// "Annuler" under what she kept (`item`, a [`Body::Learned`]).
    Undo { id: u64, item: u64 },
    /// The thread so far: [`FromSecretary::Thread`].
    Thread { id: u64 },
    /// The drawer opened or closed.
    Drawer { open: bool },
    /// The session locked or unlocked: she keeps working, and never speaks
    /// while locked (§3.10).
    Session { locked: bool },
    /// The machine resumed after `slept` seconds (unknown: she measures
    /// it): her sources catch up (§3.10).
    Resumed {
        #[serde(default)]
        slept: Option<u64>,
    },
    /// Her clock moved on by `advance` seconds: only when she runs on a
    /// clock of her own (tests, `--virtual-clock`); refused otherwise.
    Clock { advance: u64 },
    /// A notification of an application the user consented to, as the
    /// shell's server received it (lot 7): what it says, never what it can
    /// do. Nothing goes back: no line of hers can invoke, close or mark a
    /// notification (Slack's would mark the channel read).
    Notification(Notification),
}

/// A notification forwarded to her ([`ToSecretary::Notification`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    /// The server's id for it (from 1 again when the shell starts).
    pub id: u32,
    /// Its application's name (`Slack`).
    pub app: String,
    /// Its application's desktop entry, when it names one.
    #[serde(default)]
    pub entry: Option<String>,
    /// Its title (the specification's summary) and its text, markup out.
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// When it came, milliseconds since the epoch (the shell's wall clock).
    pub at_ms: i64,
}

/// A line from her.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FromSecretary {
    Hello {
        version: u32,
    },
    /// The connection is refused (another version), and closes.
    Refused {
        reason: String,
    },
    Status {
        id: u64,
        status: Box<Status>,
    },
    Graph {
        id: u64,
        graph: Graph,
    },
    /// A part of her answer to an [`ToSecretary::Ask`]; the last is `done`.
    Answer {
        id: u64,
        text: String,
        done: bool,
    },
    /// She composes her answer to request `id`; `doing`, what she is
    /// doing, from her tool calls ("elle regarde le tableau de Chapitô"),
    /// empty while she only writes.
    Typing {
        id: u64,
        doing: String,
    },
    /// A thread item, new, or changed when its id is known.
    Item {
        item: Item,
    },
    /// The thread so far, oldest first.
    Thread {
        id: u64,
        items: Vec<Item>,
    },
    /// Her face and her status line, as they change.
    Look {
        look: Look,
    },
    Error {
        #[serde(default)]
        id: Option<u64>,
        message: String,
    },
}

impl FromSecretary {
    /// The request it answers.
    pub fn id(&self) -> Option<u64> {
        match self {
            Self::Status { id, .. }
            | Self::Graph { id, .. }
            | Self::Answer { id, .. }
            | Self::Typing { id, .. }
            | Self::Thread { id, .. } => Some(*id),
            Self::Error { id, .. } => *id,
            Self::Hello { .. } | Self::Refused { .. } | Self::Item { .. } | Self::Look { .. } => {
                None
            }
        }
    }
}

/// One item of the drawer's thread (§5.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Hers, from 1; the same id again replaces it.
    pub id: u64,
    /// When, on her clock (RFC 3339).
    pub at: String,
    /// The request it belongs to: the sentence it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
    pub body: Body,
}

/// What a thread item shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Body {
    /// The user's sentence: a bubble on the right.
    User { text: String },
    /// Her words: a bubble on the left (the streamed answer, once whole).
    /// `note`, a small line under them; `assumed`, "Supposé · corriger".
    Her {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        assumed: bool,
    },
    /// A question, one at a time, with its quick replies (pills); `chosen`
    /// once one is clicked, the others greyed.
    Question {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        replies: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chosen: Option<u32>,
    },
    /// A card: a proposal when it has actions.
    Card { card: Card },
    /// "Retenu : … · Annuler", once something is kept (§9.3).
    Learned {
        text: String,
        #[serde(default, skip_serializing_if = "is_false")]
        undone: bool,
    },
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// A card (§5.4): at most three buttons, the first the primary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub kind: CardKind,
    /// Its spaced label ("TA JOURNÉE · MARDI 10").
    pub label: String,
    #[serde(default)]
    pub text: String,
    /// Its lines (a plan's steps), or a draft's quoted text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
    /// What the user decided, once a button is clicked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided: Option<Decision>,
    /// What came of it ("Fait.", "Écarté."), under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

/// A card's colour and meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CardKind {
    /// Green: a plan, a task, an event.
    Plan,
    /// Amber: a rule.
    Rule,
    /// Blue: a person.
    Person,
    /// Orange: a warning ("Je ne l'ai pas fait").
    Warning,
    /// A quoted draft: a message, a post, a mail.
    Draft,
    /// Violet: work given to Claude Code (§12).
    Delegation,
}

/// A card's button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub label: String,
    pub decision: Decision,
}

/// What a card's button says (§16: `accept`/`reject`/`edit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    Accept,
    Edit,
    Reject,
}

/// Her face (§5.6), and her header's status line (§5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    pub mood: Mood,
    /// "Slack · Gmail · Agenda à jour", "Gmail ne répond plus"…
    pub status: String,
    pub tone: Tone,
    /// The model that answers, under the field ("Claude Sonnet 5.5"); empty
    /// without one.
    #[serde(default)]
    pub model: String,
}

/// Her mood (§5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mood {
    /// The drawer open, idle.
    Listen,
    /// Composing.
    Type,
    /// A question to the user.
    Ask,
    /// The peek, the briefing ready.
    Nudge,
    /// Something learned.
    Happy,
    /// A source error, a masked secret, a suspicious mail.
    Worry,
    /// Offline.
    Sleep,
}

impl Mood {
    pub const ALL: [Self; 7] = [
        Self::Listen,
        Self::Type,
        Self::Ask,
        Self::Nudge,
        Self::Happy,
        Self::Worry,
        Self::Sleep,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Listen => "listen",
            Self::Type => "type",
            Self::Ask => "ask",
            Self::Nudge => "nudge",
            Self::Happy => "happy",
            Self::Worry => "worry",
            Self::Sleep => "sleep",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.word() == word)
    }
}

/// The status line's dot: green, amber, orange, grey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone {
    Ok,
    Warn,
    Bad,
    Off,
}

/// How she is.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    pub version: u32,
    /// Her clock now, RFC 3339.
    pub now: String,
    pub started: String,
    /// She runs on a clock of her own (tests).
    pub virtual_clock: bool,
    pub locked: bool,
    /// Where her memory's key is: `keyring`, `file`.
    pub key: String,
    pub counts: Counts,
    pub sources: Vec<SourceStatus>,
    /// The last catch-up, after a start or a resume.
    pub catch_up: Option<CatchUp>,
    pub connections: Vec<ConnectionStatus>,
    /// Her loop's wakeups so far.
    pub wakes: u64,
    /// The model she calls: `fake`, or none yet.
    pub model: String,
    /// Model calls under way.
    pub calls_running: u32,
    /// The local triage's queue, when she has one.
    pub triage: Option<TriageStatus>,
}

/// The local triage's queue (§11.1).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TriageStatus {
    /// `idle`, `loaded`, `running`, `resting`, or `paused: <why>`.
    pub state: String,
    /// Its provider and model: `local/qwen3:8b`.
    pub model: String,
    pub pending: u64,
    /// Items checked by both passes so far, and left unchecked after
    /// failures.
    pub done: u64,
    pub given_up: u64,
    /// Items held with their text, checked or not.
    pub held: u64,
}

/// How many of each her memory holds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Counts {
    pub nodes: u64,
    pub aliases: u64,
    pub edges: u64,
    pub rules: u64,
    pub episodes: u64,
    pub summaries: u64,
    pub journal: u64,
    pub audit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceStatus {
    pub name: String,
    pub notebook: Option<Notebook>,
    /// Where its next sync starts (its sync token).
    pub cursor: Option<String>,
    pub synced_at: Option<String>,
    /// Items read so far, this run.
    pub items: u64,
    /// A sync under way.
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CatchUp {
    pub at: String,
    /// How long she was away (asleep, off).
    pub gap_seconds: u64,
    /// Items read on the way back.
    pub items: u64,
    /// Over 4 hours: a "Pendant ton absence" briefing is due (§3.10).
    pub away_briefing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub client: Option<Client>,
    /// Lines she sent it.
    pub sent: u64,
}

/// A question to her graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQuery {
    /// A node's name or alias.
    pub node: String,
    /// `YYYY-MM-DD` or RFC 3339; none: now.
    #[serde(default)]
    pub as_of: Option<String>,
    #[serde(default = "two")]
    pub hops: u8,
    /// One notebook only; none: both.
    #[serde(default)]
    pub notebook: Option<Notebook>,
}

const fn two() -> u8 {
    2
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Graph {
    /// The time the edges are valid at.
    pub as_of: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub notebook: Notebook,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: i64,
    pub src: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub dst: Option<i64>,
    pub value: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub provenance: Provenance,
    pub pointer: String,
    pub notebook: Notebook,
}

/// `message` as its line, with its newline.
pub fn line<T: Serialize>(message: &T) -> String {
    // Our types always serialise.
    let mut text = serde_json::to_string(message).unwrap_or_else(|_| "{}".to_owned());
    text.push('\n');
    text
}

/// A line read back.
pub fn read<'a, T: Deserialize<'a>>(line: &'a str) -> Result<T, String> {
    serde_json::from_str(line.trim()).map_err(|e| e.to_string())
}

/// A blocking connection to her, for the `hocus` command and tests: never
/// on a loop.
#[derive(Debug)]
pub struct Connection {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
}

impl Connection {
    /// Connects and says hello; fails when she does not answer within
    /// `timeout` or refuses.
    pub fn open(path: &Path, client: Client, timeout: Duration) -> Result<Self, String> {
        let place = |e: std::io::Error| format!("{}: {e}", path.display());
        let stream = UnixStream::connect(path).map_err(place)?;
        stream.set_read_timeout(Some(timeout)).map_err(place)?;
        stream.set_write_timeout(Some(timeout)).map_err(place)?;
        let reader = BufReader::new(stream.try_clone().map_err(place)?);
        let mut connection = Self {
            writer: stream,
            reader,
            next: 1,
        };
        connection.send(&ToSecretary::Hello {
            version: VERSION,
            client,
            lang: None,
        })?;
        match connection.receive()? {
            FromSecretary::Hello { .. } => Ok(connection),
            FromSecretary::Refused { reason } => Err(reason),
            other => Err(format!("an unexpected hello: {other:?}")),
        }
    }

    /// A new request's id.
    pub fn id(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    pub fn send(&mut self, message: &ToSecretary) -> Result<(), String> {
        self.writer
            .write_all(line(message).as_bytes())
            .map_err(|e| e.to_string())
    }

    pub fn receive(&mut self) -> Result<FromSecretary, String> {
        let mut text = String::new();
        match self.reader.read_line(&mut text) {
            Ok(0) => Err("she closed the connection".to_owned()),
            Ok(_) => read(&text),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Sends `make(id)` and returns its answer, the last one for a
    /// streamed answer (its parts joined).
    pub fn request(
        &mut self,
        make: impl FnOnce(u64) -> ToSecretary,
    ) -> Result<FromSecretary, String> {
        let id = self.id();
        self.send(&make(id))?;
        let mut parts = String::new();
        loop {
            let message = self.receive()?;
            if message.id() != Some(id) {
                continue;
            }
            match message {
                FromSecretary::Answer { text, done, .. } => {
                    parts.push_str(&text);
                    if done {
                        return Ok(FromSecretary::Answer {
                            id,
                            text: parts,
                            done,
                        });
                    }
                }
                FromSecretary::Error { message, .. } => return Err(message),
                other => return Ok(other),
            }
        }
    }

    pub fn status(&mut self) -> Result<Status, String> {
        match self.request(|id| ToSecretary::Status { id })? {
            FromSecretary::Status { status, .. } => Ok(*status),
            other => Err(format!("not a status: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_back_and_name_their_kind() {
        let hello = ToSecretary::Hello {
            version: VERSION,
            client: Client::Shell,
            lang: None,
        };
        let text = line(&hello);
        assert_eq!(text, "{\"hello\":{\"version\":2,\"client\":\"shell\"}}\n");
        assert_eq!(read::<ToSecretary>(&text), Ok(hello));
        let resumed: ToSecretary = read("{\"resumed\":{}}").expect("reads");
        assert_eq!(resumed, ToSecretary::Resumed { slept: None });
        let query: ToSecretary = read(
            "{\"graph\":{\"id\":3,\"query\":{\"node\":\"Chapitô\",\"as_of\":\"2026-04-15\"}}}",
        )
        .expect("reads");
        assert_eq!(
            query,
            ToSecretary::Graph {
                id: 3,
                query: GraphQuery {
                    node: "Chapitô".to_owned(),
                    as_of: Some("2026-04-15".to_owned()),
                    hops: 2,
                    notebook: None,
                }
            }
        );
        assert!(read::<ToSecretary>("{\"fly\":{}}").is_err());
        let slack: ToSecretary = read(
            "{\"notification\":{\"id\":3,\"app\":\"Slack\",\"title\":\"Karim\",\"body\":\"tu as vu ?\",\"at_ms\":1794215520000}}",
        )
        .expect("reads");
        assert_eq!(
            slack,
            ToSecretary::Notification(Notification {
                id: 3,
                app: "Slack".to_owned(),
                entry: None,
                title: "Karim".to_owned(),
                body: "tu as vu ?".to_owned(),
                at_ms: 1_794_215_520_000,
            })
        );
    }

    #[test]
    fn the_drawers_lines_read_back() {
        let card = FromSecretary::Item {
            item: Item {
                id: 4,
                at: "2026-11-09T08:55:00Z".to_owned(),
                request: Some(2),
                body: Body::Card {
                    card: Card {
                        kind: CardKind::Draft,
                        label: "Message à Léa".to_owned(),
                        text: String::new(),
                        lines: vec!["Je regarde ton article cet après-midi.".to_owned()],
                        actions: vec![Action {
                            label: "Envoyer".to_owned(),
                            decision: Decision::Accept,
                        }],
                        decided: None,
                        outcome: None,
                    },
                },
            },
        };
        assert_eq!(read::<FromSecretary>(&line(&card)), Ok(card));
        let learned = "{\"item\":{\"item\":{\"id\":5,\"at\":\"2026-11-09T08:55:00Z\",\
                       \"body\":{\"learned\":{\"text\":\"Retenu\"}}}}}";
        assert!(matches!(
            read::<FromSecretary>(learned),
            Ok(FromSecretary::Item {
                item: Item {
                    body: Body::Learned { undone: false, .. },
                    ..
                }
            })
        ));
        let decide: ToSecretary =
            read("{\"decide\":{\"id\":7,\"item\":4,\"decision\":\"accept\"}}").expect("reads");
        assert_eq!(
            decide,
            ToSecretary::Decide {
                id: 7,
                item: 4,
                decision: Decision::Accept
            }
        );
        // A first version's hello still reads, and is refused by its number.
        let old: ToSecretary =
            read("{\"hello\":{\"version\":1,\"client\":\"cli\"}}").expect("reads");
        assert!(matches!(old, ToSecretary::Hello { version: 1, .. }));
        for mood in Mood::ALL {
            assert_eq!(Mood::of(mood.word()), Some(mood));
        }
    }

    #[test]
    fn her_socket_is_beside_the_control_socket() {
        assert_eq!(
            beside_control(Path::new("/run/user/1000/hocus-0.control")),
            Some(PathBuf::from("/run/user/1000/hocus-0.secretary"))
        );
        assert_eq!(beside_control(Path::new("/run/user/1000/hocus-0")), None);
    }
}
