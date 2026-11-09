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
//!
//! **Version 3** brings "Ce qu'elle sait" (§8, lot 6): the tab asks her
//! what she knows ([`ToSecretary::Knows`], [`knows`]) and acts on it
//! ([`ToSecretary::Memory`]); she answers with the view
//! ([`FromSecretary::Knows`]) and a toast when there is something to take
//! back ([`FromSecretary::Toast`]). What she kept settles after ten
//! seconds: its "Annuler" goes ([`Body::Learned`]'s `settled`).
//!
//! **Version 4** brings her speaking up (§6, lot 5): she asks to speak
//! ([`FromSecretary::Speak`]) and the shell's gate ([`gate`]) answers
//! ([`ToSecretary::Spoken`]: shown, held or dropped) from the screen's
//! state and her restraint ([`FromSecretary::Restraint`]); what the user
//! does with her peek ([`ToSecretary::Peeked`]: "Fait", "Plus tard ▾",
//! the bubble); the briefings as thread items ([`Body::Briefing`]); every
//! item's notebook, so that the drawer hides perso in public moments;
//! the moment itself ([`ToSecretary::Moment`]); and the outside effects
//! the shell carries out for her ([`FromSecretary::Effect`], answered by
//! [`ToSecretary::Effected`]): the first is a task in the To do list.
//!
//! **Version 5** brings her first run and the sources' consent in the
//! drawer (#750): a consent card ([`CardKind::Consent`]) carries its
//! sheet ([`sheets::Sheet`]: what she reads, what she never does there,
//! what leaves the machine, the notebook suggested), and "Autoriser" says
//! the notebook chosen ([`ToSecretary::Decide`]'s `notebook`); her face
//! has a second line, how to fix what her status says ([`Look::hint`]).
//!
//! **Version 6** brings #752: her key given in the drawer or the Settings
//! ([`ToSecretary::Key`], answered by [`FromSecretary::Keyed`]; the
//! Settings connect as [`Client::Settings`]), a copy of it taken out of
//! the clipboard's history ([`FromSecretary::ClipSecret`]), her name and
//! the key her drawer asks for in her face ([`Look::name`], [`Look::key`]),
//! a consent sheet's "C'est privé" and the calendars it lists
//! ([`sheets::Sheet`]), and the calendars left out of a consent card's
//! "Autoriser" ([`ToSecretary::Decide`]'s `left_out`).

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

pub mod consent;
pub mod gate;
#[cfg(feature = "goa")]
pub mod goa;
pub mod knows;
pub mod sheets;

/// The protocol's version: a change that an older side would misread
/// raises it.
pub const VERSION: u32 = 6;

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

/// Her socket in this session: [`SOCKET_ENV`], else beside the Wayland
/// socket (`$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY.secretary`).
pub fn session_socket() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(path) = var(SOCKET_ENV) {
        return Some(PathBuf::from(path));
    }
    let display = PathBuf::from(var("WAYLAND_DISPLAY")?);
    let wayland = if display.is_absolute() {
        display
    } else {
        PathBuf::from(var("XDG_RUNTIME_DIR")?).join(display)
    };
    Some(socket_path(&wayland))
}

/// `key` given to her for `provider` (none: erased), over her socket at
/// `path`, as the Settings do: what became of it. Blocking (her check
/// with the provider and her keyring take their time).
pub fn give_key(
    path: &Path,
    client: Client,
    provider: &str,
    key: Option<Secret>,
) -> Result<KeyOutcome, String> {
    let mut her = Connection::open(path, client, Duration::from_secs(60))?;
    match her.request(|id| ToSecretary::Key {
        id,
        provider: provider.to_owned(),
        key,
    })? {
        FromSecretary::Keyed { outcome, .. } => Ok(outcome),
        FromSecretary::Error { message, .. } => Err(message),
        _ => Err("an unexpected answer".to_owned()),
    }
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
    /// The Settings (her key, version 6).
    Settings,
}

/// An API key on its way to her keyring: never printed, its bytes
/// overwritten with zeros when it goes.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: String) -> Self {
        Self(text)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// `text`'s bytes overwritten with zeros, then emptied: what held a key
/// does not keep it in memory once it is used.
pub fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    // The zeros are read back, so that the write is not left out.
    std::hint::black_box(&bytes);
}

/// The clipboard history's digest of a text (FNV-1a, the shell's): what
/// she gives the shell of a key she kept, so that a copy of it leaves the
/// history, without the key itself.
pub fn clip_digest(text: &str) -> u64 {
    text.trim()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// What became of a key given to her ([`ToSecretary::Key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyOutcome {
    /// Checked by the provider, kept in the keyring.
    Kept,
    /// Taken out of the keyring.
    Erased,
    /// Not a key of that provider's (empty, spaces, the wrong prefix).
    Malformed,
    /// The provider refuses it.
    Refused,
    /// The provider could not be reached.
    Offline,
    /// The provider answered something else.
    Failed,
    /// The keyring did not keep it (or does not answer).
    NoKeyring,
}

impl KeyOutcome {
    /// What the user reads.
    pub fn words(self, provider: &str, fr: bool) -> String {
        let name = match provider {
            "anthropic" => "Anthropic",
            other => other,
        };
        match (self, fr) {
            (Self::Kept, true) => format!("Clé {name} enregistrée dans ton trousseau."),
            (Self::Kept, false) => format!("{name} key saved in your keyring."),
            (Self::Erased, true) => format!("Clé {name} effacée de ton trousseau."),
            (Self::Erased, false) => format!("{name} key erased from your keyring."),
            (Self::Malformed, true) if provider == "anthropic" => {
                "Ce n’est pas une clé Anthropic : elle commence par sk-ant-.".to_owned()
            }
            (Self::Malformed, false) if provider == "anthropic" => {
                "That is not an Anthropic key: it starts with sk-ant-.".to_owned()
            }
            (Self::Malformed, true) => format!("Ce n’est pas une clé {name}."),
            (Self::Malformed, false) => format!("That is not a {name} key."),
            (Self::Refused, true) => format!("Clé refusée par {name}."),
            (Self::Refused, false) => format!("Key refused by {name}."),
            (Self::Offline, true) => format!("Pas de réseau : {name} ne répond pas. Réessaie plus tard."),
            (Self::Offline, false) => format!("No network: {name} does not answer. Try again later."),
            (Self::Failed, true) => format!("{name} n’a pas pu vérifier la clé. Réessaie plus tard."),
            (Self::Failed, false) => format!("{name} could not check the key. Try again later."),
            (Self::NoKeyring, true) => "Ton trousseau ne répond pas : la clé n’est pas enregistrée.".to_owned(),
            (Self::NoKeyring, false) => "Your keyring does not answer: the key is not saved.".to_owned(),
        }
    }
}

/// The two notebooks every item of her memory is in (§7.4).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Notebook {
    #[default]
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
        /// A consent card's "Autoriser": the notebook chosen on it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notebook: Option<Notebook>,
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
    /// "Ce qu'elle sait" (lot 6): her view of what she knows for `query`,
    /// sent again while it changes; none stops it (the tab left).
    Knows {
        id: u64,
        #[serde(default)]
        query: Option<knows::Query>,
    },
    /// Something done in "Ce qu'elle sait": answered with the view again,
    /// and a toast when it can be taken back.
    Memory { id: u64, act: knows::Do },
    /// What the gate did with her request to speak `speak`
    /// ([`FromSecretary::Speak`]).
    Spoken { speak: u64, outcome: Spoken },
    /// What the user did with her peek of `speak`.
    Peeked {
        id: u64,
        speak: u64,
        answer: PeekAnswer,
    },
    /// The moment the screen is in (§7.4), as it changes: in a public one
    /// nothing perso is shown, and nothing perso goes to a model.
    Moment { moment: Moment },
    /// `provider`'s API key, pasted in her drawer or the Settings: she
    /// checks it with the provider and keeps it in her keyring; none
    /// erases it. Answered by [`FromSecretary::Keyed`].
    Key {
        id: u64,
        provider: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<Secret>,
    },
    /// Effect `effect` carried out by the shell (`ok`), or not, and why.
    Effected {
        effect: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        detail: String,
    },
}

/// What the gate did with a request to speak (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Spoken {
    /// Her peek came out.
    Shown,
    /// Held for now: shown when what holds it ends, if still actionable.
    Held { why: gate::Why },
    /// Not shown: it goes to her next briefing.
    Dropped { why: gate::Why },
}

/// What the user did with her peek (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PeekAnswer {
    /// "Fait": done.
    Done,
    /// "Plus tard ▾" and one of its choices.
    Later { later: Later },
    /// The bubble, or its action: the drawer opens on the item.
    Open,
}

/// "Plus tard ▾"'s choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Later {
    /// "Dans 1 heure".
    Hour,
    /// "Après ma prochaine réunion".
    AfterMeeting,
    /// "Cet après-midi".
    Afternoon,
    /// "Demain matin, dans le point".
    Tomorrow,
    /// "Ne plus me le dire".
    Never,
}

impl Later {
    pub const ALL: [Self; 5] = [
        Self::Hour,
        Self::AfterMeeting,
        Self::Afternoon,
        Self::Tomorrow,
        Self::Never,
    ];

    /// Its words in the menu.
    pub fn words(self, fr: bool) -> &'static str {
        match (self, fr) {
            (Self::Hour, true) => "Dans 1 heure",
            (Self::Hour, false) => "In an hour",
            (Self::AfterMeeting, true) => "Après ma prochaine réunion",
            (Self::AfterMeeting, false) => "After my next meeting",
            (Self::Afternoon, true) => "Cet après-midi",
            (Self::Afternoon, false) => "This afternoon",
            (Self::Tomorrow, true) => "Demain matin, dans le point",
            (Self::Tomorrow, false) => "Tomorrow morning, in the briefing",
            (Self::Never, true) => "Ne plus me le dire",
            (Self::Never, false) => "Don’t tell me again",
        }
    }
}

/// The moment the screen is in (§7.4): a meeting taking time, the screen
/// shared or recorded, the camera or the microphone in use are public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Moment {
    #[default]
    Quiet,
    Meeting,
    Sharing,
    Camera,
}

impl Moment {
    pub const fn public(self) -> bool {
        !matches!(self, Self::Quiet)
    }
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
    /// "Ce qu'elle sait" for the last query of the connection's
    /// ([`ToSecretary::Knows`]), `id` that request's.
    Knows {
        id: u64,
        view: Box<knows::Knowledge>,
    },
    /// A short word at the bottom of the tab for five seconds ("Oublié"),
    /// with "Annuler" when `undo` is a token to give back
    /// ([`knows::Do::Restore`]).
    Toast {
        id: u64,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        undo: Option<u64>,
    },
    /// She asks to speak (§6.2, §6.3): the shell's gate answers
    /// [`ToSecretary::Spoken`].
    Speak {
        speak: Speak,
    },
    /// Her restraint's settings, for the shell's gate: on hello and as
    /// they change.
    Restraint {
        restraint: gate::Restraint,
    },
    /// An outside effect the user accepted on a card, for the shell to
    /// carry out (it owns what it touches): answered by
    /// [`ToSecretary::Effected`].
    Effect {
        effect: Effect,
    },
    /// What became of [`ToSecretary::Key`] `id`.
    Keyed {
        id: u64,
        provider: String,
        outcome: KeyOutcome,
    },
    /// A key she kept: a copy of it leaves the clipboard's history (its
    /// [`clip_digest`]), to the shells.
    ClipSecret {
        digest: u64,
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
            | Self::Thread { id, .. }
            | Self::Knows { id, .. }
            | Self::Toast { id, .. }
            | Self::Keyed { id, .. } => Some(*id),
            Self::Error { id, .. } => *id,
            Self::Hello { .. }
            | Self::Refused { .. }
            | Self::Item { .. }
            | Self::Look { .. }
            | Self::Speak { .. }
            | Self::Restraint { .. }
            | Self::ClipSecret { .. }
            | Self::Effect { .. } => None,
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
    /// Its notebook, when it is about one: a perso item is hidden in a
    /// public moment (§7.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notebook: Option<Notebook>,
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
    /// "Retenu : … · Annuler", once something is kept (§9.3); `settled`
    /// ten seconds later, its "Annuler" gone (it is then corrected in
    /// "Ce qu'elle sait").
    Learned {
        text: String,
        #[serde(default, skip_serializing_if = "is_false")]
        undone: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        settled: bool,
    },
    /// A briefing (§6.1): the morning's, folded on one line with "Ouvrir
    /// le point"; "Pendant ton absence" (§3.10), shown whole.
    Briefing { briefing: Briefing },
}

/// A briefing, its parts in order, each only when it has something.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Briefing {
    pub kind: BriefingKind,
    /// "Le point du matin", "Pendant ton absence · ven. 18 h → lun. 8 h 40".
    pub title: String,
    /// "Bonjour Alex"; the date; her sentence on the day.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hello: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sentence: String,
    #[serde(default)]
    pub sections: Vec<Section>,
    /// What was read, and when.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub footer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BriefingKind {
    #[default]
    Morning,
    Absence,
}

/// A part of a briefing: "AUJOURD'HUI", "EN CONGÉS", "À RÉPONDRE"…
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Section {
    /// Which part: `today`, `leaves`, `answer`, `rest`, `yesterday`,
    /// `plan`, `reminders`, `away`, `perso`.
    pub key: String,
    /// Its spaced label ("EN CONGÉS"), and a word after it ("calendrier
    /// « Congés »").
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default)]
    pub lines: Vec<Line>,
    /// A proposal card of the thread shown under it (the move of a
    /// meeting whose presenter is away).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<u64>,
}

/// A line of a briefing.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Line {
    /// "9 h 30", in Martian Mono.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub time: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// When, on the right ("hier 17 h 40").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub when: String,
    /// Something needs the user: amber.
    #[serde(default, skip_serializing_if = "is_false")]
    pub attention: bool,
    /// A plan's line: ticked or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    pub notebook: Notebook,
}

/// Her request to speak (§6.3's `speak(item, priority, deadline)`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Speak {
    pub id: u64,
    pub kind: SpeakKind,
    /// What her bubble says, `**` around the key words.
    pub text: String,
    /// The action button's label, when it has one ("Ouvrir le point").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// The thread item it is about: the bubble opens the drawer on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u64>,
    pub notebook: Notebook,
    /// Someone the user named as passing always (§3.7): past the counts,
    /// never past a meeting.
    #[serde(default, skip_serializing_if = "is_false")]
    pub vip: bool,
    /// Past this (RFC 3339), it can no longer be acted on: not shown, it
    /// goes to the next briefing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpeakKind {
    /// "Ton point est prêt".
    Briefing,
    /// "Pendant ton absence".
    Absence,
    /// A reminder come due (a commitment, a task's time).
    Reminder,
    /// Something to decide now (a meeting whose presenter is away).
    Proposal,
}

impl SpeakKind {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Briefing => "briefing",
            Self::Absence => "absence",
            Self::Reminder => "reminder",
            Self::Proposal => "proposal",
        }
    }
}

/// An outside effect for the shell to carry out (§2.1: only after the
/// user's click on that card).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    /// A task at the top of the To do list (Day.4).
    Task {
        id: u64,
        title: String,
        notebook: Notebook,
    },
}

impl Effect {
    pub const fn id(&self) -> u64 {
        match self {
            Self::Task { id, .. } => *id,
        }
    }
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
    /// A consent card's sheet: its sections and its notebook's choice.
    /// Its buttons stay under its outcome ("Ne plus lire" once granted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<sheets::Sheet>,
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
    /// A source's consent sheet (§10.1): nothing is read before its
    /// "Autoriser".
    Consent,
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
    /// Under the status, what the user can do about it ("Dans un
    /// terminal : hocus-secretary key anthropic"); empty when all is well.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hint: String,
    /// The provider whose key she misses: her drawer shows its field
    /// (version 6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Her name (`[secretary] name`): empty, "Ta secrétaire" (version 6).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
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
    /// The provider whose key her answers need, and whether her keyring
    /// misses it (as last asked): the Settings' "Clé Anthropic" row.
    pub key_provider: Option<String>,
    pub key_missing: bool,
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
    /// Where the local model is held, when its server says: « tri : sur la
    /// carte graphique », « tri : en partie sur le processeur » (#752).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
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
        let mut text = line(message);
        let wrote = self
            .writer
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string());
        if matches!(message, ToSecretary::Key { .. }) {
            wipe(&mut text);
        }
        wrote
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
        assert_eq!(text, "{\"hello\":{\"version\":6,\"client\":\"shell\"}}\n");
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
                notebook: None,
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
                        sheet: None,
                    },
                },
            },
        };
        assert_eq!(read::<FromSecretary>(&line(&card)), Ok(card));
        let consent = Body::Card {
            card: Card {
                kind: CardKind::Consent,
                label: "Gmail · alex.martin@gmail.com".to_owned(),
                text: String::new(),
                lines: Vec::new(),
                actions: Vec::new(),
                decided: None,
                outcome: None,
                sheet: Some(sheets::google(
                    "alex.martin@gmail.com",
                    consent::SourceKind::Mail,
                    Notebook::Perso,
                    true,
                )),
            },
        };
        let said = serde_json::to_string(&consent).expect("writes");
        assert_eq!(serde_json::from_str::<Body>(&said).ok(), Some(consent));
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
                decision: Decision::Accept,
                notebook: None,
            }
        );
        let allowed: ToSecretary = read(
            "{\"decide\":{\"id\":8,\"item\":5,\"decision\":\"accept\",\"notebook\":\"perso\"}}",
        )
        .expect("reads");
        assert!(matches!(
            allowed,
            ToSecretary::Decide {
                notebook: Some(Notebook::Perso),
                ..
            }
        ));
        // A first version's hello still reads, and is refused by its number.
        let old: ToSecretary =
            read("{\"hello\":{\"version\":1,\"client\":\"cli\"}}").expect("reads");
        assert!(matches!(old, ToSecretary::Hello { version: 1, .. }));
        for mood in Mood::ALL {
            assert_eq!(Mood::of(mood.word()), Some(mood));
        }
    }

    #[test]
    fn her_speaking_lines_read_back() {
        let speak = FromSecretary::Speak {
            speak: Speak {
                id: 1,
                kind: SpeakKind::Briefing,
                text: "Ton point est prêt".to_owned(),
                action: Some("Ouvrir le point".to_owned()),
                item: Some(4),
                notebook: Notebook::Pro,
                vip: false,
                deadline: None,
            },
        };
        assert_eq!(read::<FromSecretary>(&line(&speak)), Ok(speak));
        let spoken: ToSecretary =
            read("{\"spoken\":{\"speak\":1,\"outcome\":{\"held\":{\"why\":\"dnd\"}}}}")
                .expect("reads");
        assert_eq!(
            spoken,
            ToSecretary::Spoken {
                speak: 1,
                outcome: Spoken::Held {
                    why: gate::Why::Dnd
                }
            }
        );
        let later = ToSecretary::Peeked {
            id: 2,
            speak: 1,
            answer: PeekAnswer::Later {
                later: Later::AfterMeeting,
            },
        };
        assert_eq!(read::<ToSecretary>(&line(&later)), Ok(later));
        let effect = FromSecretary::Effect {
            effect: Effect::Task {
                id: 7,
                title: "Envoyer le devis à Léa".to_owned(),
                notebook: Notebook::Pro,
            },
        };
        assert_eq!(read::<FromSecretary>(&line(&effect)), Ok(effect));
        let restraint = FromSecretary::Restraint {
            restraint: gate::Restraint::default(),
        };
        assert_eq!(read::<FromSecretary>(&line(&restraint)), Ok(restraint));
        let moment: ToSecretary = read("{\"moment\":{\"moment\":\"sharing\"}}").expect("reads");
        assert_eq!(
            moment,
            ToSecretary::Moment {
                moment: Moment::Sharing
            }
        );
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
