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

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

/// The protocol's version: a change that an older side would misread
/// raises it.
pub const VERSION: u32 = 1;

/// The environment variable that names her socket, for her and for the
/// programs the compositor starts.
pub const SOCKET_ENV: &str = "HOCUS_SECRETARY_SOCKET";

/// The environment variable naming the session bus whose keyring holds her
/// memory's key: an address, or `session`; unset, the key is in a file.
pub const BUS_ENV: &str = "HOCUS_SECRETARY_BUS";

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
    /// The first line of a connection.
    Hello { version: u32, client: Client },
    /// How she is: [`FromSecretary::Status`].
    Status { id: u64 },
    /// Her graph around a node, now or as of a date:
    /// [`FromSecretary::Graph`].
    Graph { id: u64, query: GraphQuery },
    /// A question: her answer comes in [`FromSecretary::Answer`]s, the last
    /// one `done`.
    Ask { id: u64, text: String },
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
            Self::Status { id, .. } | Self::Graph { id, .. } | Self::Answer { id, .. } => Some(*id),
            Self::Error { id, .. } => *id,
            Self::Hello { .. } | Self::Refused { .. } => None,
        }
    }
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
        };
        let text = line(&hello);
        assert_eq!(text, "{\"hello\":{\"version\":1,\"client\":\"shell\"}}\n");
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
