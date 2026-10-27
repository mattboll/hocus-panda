//! Control protocol types, socket server and event stream.
//!
//! One JSON object per line over a UNIX socket next to the Wayland socket.
//! A client sends requests with an id; the compositor answers each with the
//! same id, and streams events to clients that subscribed:
//!
//! ```text
//! → {"id":1,"request":"windows"}
//! ← {"id":1,"ok":{"windows":[{"id":1,"app_id":"foot",…}]}}
//! → {"id":2,"request":"pointer_move","x":10.0,"y":20.0}
//! ← {"id":2,"error":{"code":"permission_denied","message":"…"}}
//! ← {"event":{"type":"window_focused","id":1}}
//! ```

pub mod client;
pub mod mcp;
pub mod server;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use client::{Client, ClientError};
pub use server::{Agents, ConnectionId, Handler, Server};

/// Version of the protocol, raised on incompatible changes.
pub const PROTOCOL_VERSION: u32 = 0;

/// Set for the commands the compositor starts: the control socket's path.
pub const SOCKET_ENV: &str = "HOCUS_SOCKET";

/// The control socket that goes with a Wayland socket.
pub fn socket_for(wayland_socket: &Path) -> PathBuf {
    let mut path = wayland_socket.as_os_str().to_owned();
    path.push(".api");
    PathBuf::from(path)
}

/// The control socket of the compositor this process runs in: `$HOCUS_SOCKET`,
/// or the one next to `$WAYLAND_DISPLAY`.
pub fn default_socket() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(SOCKET_ENV) {
        return Some(PathBuf::from(path));
    }
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY")?);
    let display = if display.is_absolute() {
        display
    } else {
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(display)
    };
    Some(socket_for(&display))
}

/// What a connection may do. Observing is open to every client of the
/// user's session; driving input and dumping the scene need the compositor
/// to be started for tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    #[default]
    Observe,
    Test,
}

/// A request line: an id chosen by the client, echoed in the reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestLine {
    pub id: u64,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    Version,
    Outputs,
    /// Every connected screen, on or off, with what it can do: for the
    /// settings' Screens page.
    Screens,
    /// Each screen shows its name on it for a few seconds.
    IdentifyOutputs,
    /// The shell asks the user what to share (#116), for the screen-cast
    /// portal's chooser; answered once chosen, as `Monitor: NAME`,
    /// `Window: IDENTIFIER` (ext-foreign-toplevel-list) or nothing.
    PickShare,
    /// Calendar accounts, how they synced, and the next events (#75).
    Calendars,
    /// The password of calendar account `account` (`user@url`), kept in
    /// the keyring; the configuration names the account.
    CalendarSecret {
        account: String,
        secret: String,
    },
    /// Opens GNOME's own settings for online accounts, where Google
    /// accounts are added and removed (#137), when they are installed.
    OpenOnlineAccounts,
    /// The key of AI provider `provider` (`claude`, `openai`, `llamacpp`),
    /// kept in the keyring (#44); an empty one forgets it.
    AiSecret {
        provider: String,
        secret: String,
    },
    /// Which AI providers have a key kept, never the keys (#67).
    Ai,
    /// What the document search's indexer does (#45): never the
    /// documents themselves.
    Search,
    /// Its folders looked at again now, not at the next half hour (#67):
    /// what changed is read, nothing while on battery.
    SearchAgain,
    /// System updates waiting, and whether they are being looked for or
    /// installed (#84).
    Updates,
    /// Installs them, through polkit.
    InstallUpdates,
    /// The machine's sound, network, Bluetooth and power as quick settings
    /// know them, for the settings' Hardware pages (#67).
    Hardware,
    /// Changes one of them, as quick settings would.
    SetHardware {
        change: HardwareChange,
    },
    /// Do not disturb, and the applications that sent notifications this
    /// session, for the settings' Notifications page (#67).
    Notifications,
    /// Turns do not disturb on or off, as quick settings do.
    SetDoNotDisturb {
        on: bool,
    },
    /// The phone as the session knows it, through KDE Connect (#85).
    Phone,
    /// Sends files to the phone within reach, by absolute path.
    SendToPhone {
        paths: Vec<String>,
    },
    /// Puts a PNG on the clipboard, kept by the compositor (the annotation
    /// window's Copy, #128).
    CopyPng {
        png_base64: String,
    },
    Windows,
    /// The whole scene: outputs, their layers and windows, focus, pointer.
    Scene,
    /// Starts the event stream on this connection.
    Subscribe,
    /// Moves the pointer to a point of the global logical space.
    PointerMove {
        x: f64,
        y: f64,
    },
    PointerButton {
        button: Button,
        pressed: bool,
    },
    /// A touchpad swipe with `fingers`: begun, moved by `dx`, `dy` logical
    /// pixels, ended or cancelled.
    Swipe {
        phase: SwipePhase,
        fingers: u32,
        dx: f64,
        dy: f64,
    },
    /// A wheel scroll in logical pixels, 15 per notch, down positive.
    PointerScroll {
        dy: f64,
    },
    /// A key by its Linux evdev code (`KEY_A` is 30).
    Key {
        code: u32,
        pressed: bool,
    },
    Screenshot {
        target: Target,
        /// Draw the pointer's cursor too (outputs only).
        #[serde(default)]
        cursor: bool,
    },
    /// Frame times, slow loop handlers and loop stalls.
    Stats,
    /// The configuration in effect, and the last error reading the file.
    Config,
    /// Each output's desktops, with their windows, and the one it shows.
    Desktops,
    /// The shell's interface as screen readers see it: roles, labels and
    /// where each element is.
    Ui,
    /// A window's own accessibility tree, as its application exposes it
    /// over AT-SPI (#175): roles, labels and places in the window. An
    /// agent reads those of the windows it may act on alone.
    WindowTree {
        window: u64,
    },
    /// Saves the session now.
    SaveSession,
    /// Offers to put the saved session back, as at start.
    OfferSession,
    /// Opens the settings app on a page (`hocus settings keyboard`), or
    /// brings it forward.
    OpenSettings {
        #[serde(default)]
        page: Option<String>,
    },
    /// Shows the `index`th desktop (from 0) of an output.
    ShowDesktop {
        output: String,
        index: usize,
    },
    /// Sends a window to an output: its `desktop`th desktop (from 0), or the
    /// one it shows.
    SendWindow {
        window: u64,
        output: String,
        desktop: Option<usize>,
    },
    /// Plugs a headless output, `WIDTHxHEIGHT[@SCALE][+X+Y][/DESKTOPS]`;
    /// `name` brings back one unplugged before.
    PlugOutput {
        spec: String,
        name: Option<String>,
    },
    /// Unplugs an output, as pulling its cable would.
    UnplugOutput {
        output: String,
    },
    /// A headless output stops presenting its frames, as a stuck screen
    /// would, until the compositor recovers it.
    StallOutput {
        output: String,
    },
    /// A headless output's connector, still connected, now prefers another
    /// mode, `WIDTHxHEIGHT`: as a monitor whose EDID changed behind a dock,
    /// or a VM's window resized.
    PreferMode {
        output: String,
        mode: String,
    },
    /// The output that takes what an unplugged one showed.
    SetPrimaryOutput {
        output: String,
    },
    /// Maximizes, makes fullscreen, minimizes or restores a window.
    SetWindowState {
        window: u64,
        state: WindowState,
    },
    /// Snaps a window: `left`, `right`, `top-left`, `top-right`,
    /// `bottom-left`, `bottom-right`, `top` (maximize).
    SnapWindow {
        window: u64,
        zone: String,
    },
    /// Asks a window to close, as its close button would.
    CloseWindow {
        window: u64,
    },
    /// This connection is the MCP agent `name` of `[[agents.mcp]]` (#42):
    /// from then on it makes only the requests agents may make, with the
    /// rights the configuration gives that agent.
    Agent {
        name: String,
    },
    /// Input an agent aims at a window or at an element of the shell
    /// (#42), through the agent's own seat (#43): the compositor checks the
    /// agent may act on it, shows the agent's intent (a click waits a
    /// moment under a ring), and asks the user first when the action is
    /// sensitive; the reply comes once it is carried out or refused.
    Act {
        target: ActTarget,
        action: Action,
        /// What the agent means to do, in one line, shown before it acts:
        /// "Sends the message".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<String>,
    },
    /// What MCP agents did this session, oldest first, and whether they
    /// are stopped (#176).
    AgentsJournal,
    /// Stops every MCP agent at once, or allows them again, as the stop
    /// key and the column's button do (#67).
    SetAgentsStopped {
        stopped: bool,
    },
}

impl Request {
    pub fn permission(&self) -> Permission {
        match self {
            Self::Version
            | Self::Outputs
            | Self::Screens
            | Self::IdentifyOutputs
            | Self::PickShare
            | Self::Calendars
            // The user's own account, typed in the settings.
            | Self::CalendarSecret { .. }
            // The user's own key, typed by the user.
            | Self::AiSecret { .. }
            // Which providers have one, not the keys.
            | Self::Ai
            // Counts only, never a document.
            | Self::Search
            // The user's own folders, read as they are every half hour.
            | Self::SearchAgain
            | Self::Updates
            // The system asks for the password itself.
            | Self::InstallUpdates
            | Self::Hardware
            // What quick settings change, which any application of the
            // session may change through the same services.
            | Self::SetHardware { .. }
            | Self::Notifications
            // As quick settings: the user's own toasts.
            | Self::SetDoNotDisturb { .. }
            | Self::Phone
            // Files of the user's own, to the user's own paired phone.
            | Self::SendToPhone { .. }
            // A picture the user asked to copy.
            | Self::CopyPng { .. }
            | Self::Windows
            | Self::Subscribe
            | Self::Stats
            | Self::Config
            | Self::Desktops
            | Self::Ui
            // What any program of the session may read on the
            // accessibility bus itself.
            | Self::WindowTree { .. }
            | Self::Screenshot { .. }
            // What agents did: the user's to read, and the agents' own.
            | Self::AgentsJournal
            // As the stop key: an agent itself may not (`agents_may`).
            | Self::SetAgentsStopped { .. }
            // Showing the settings window changes nothing by itself.
            | Self::OpenSettings { .. }
            // Nor GNOME's, where the user signs in and out.
            | Self::OpenOnlineAccounts => Permission::Observe,
            Self::Scene
            | Self::PointerMove { .. }
            | Self::PointerButton { .. }
            | Self::PointerScroll { .. }
            | Self::Swipe { .. }
            | Self::Key { .. }
            | Self::CloseWindow { .. }
            | Self::SetWindowState { .. }
            | Self::SnapWindow { .. }
            | Self::ShowDesktop { .. }
            | Self::SendWindow { .. }
            | Self::PlugOutput { .. }
            | Self::UnplugOutput { .. }
            | Self::StallOutput { .. }
            | Self::PreferMode { .. }
            | Self::SetPrimaryOutput { .. }
            | Self::SaveSession
            | Self::OfferSession => Permission::Test,
            // Naming itself only narrows what a connection may do.
            Self::Agent { .. } => Permission::Observe,
            // An agent's rights are checked by the compositor, per window;
            // any other connection needs the test permission.
            Self::Act { .. } => Permission::Test,
        }
    }

    /// Whether a connection that named itself an agent may make it: reading
    /// windows, outputs, the shell's interface and captures, and acting
    /// through [`Request::Act`]. Nothing that changes the session's state,
    /// reads its configuration or its accounts, or injects raw input.
    pub fn agents_may(&self) -> bool {
        matches!(
            self,
            Self::Version
                | Self::Outputs
                | Self::Windows
                | Self::Desktops
                | Self::Ui
                | Self::WindowTree { .. }
                | Self::Screenshot { .. }
                | Self::Subscribe
                | Self::Agent { .. }
                | Self::Act { .. }
                | Self::AgentsJournal
        )
    }
}

/// What an agent aims at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActTarget {
    /// A window by id, at a point relative to its top-left corner (logical
    /// pixels); its centre when `at` is left out.
    Window {
        window: u64,
        #[serde(default)]
        at: Option<Point>,
    },
    /// The shell's element with this label (as `ui` lists it), at its
    /// centre.
    Element { label: String },
    /// An element of a window's own accessibility tree (#175), by its
    /// label (exactly, else a part of it, ignoring case) and its role when
    /// given (`push button`), at its centre; refused when several match.
    AppElement {
        window: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        label: String,
    },
}

/// What an agent does there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Presses and releases `button`, `count` times (2: a double click).
    Click { button: Button, count: u32 },
    /// A wheel scroll in logical pixels, 15 per notch, down positive.
    Scroll { dy: f64 },
    /// Types text with the keyboard layout in effect; the window gets the
    /// keyboard first. Never a shortcut of the compositor's.
    Type { text: String },
    /// Presses keys together and lets them go, as `Ctrl+A`, `Return`,
    /// `Shift+Tab` say them (the shortcuts' names); never a shortcut of the
    /// compositor's.
    Keys { keys: String },
}

/// What an MCP agent may do (#42), as `[[agents.mcp]]` says it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// Windows, the shell's interface and captures.
    #[default]
    Read,
    /// Reading, and input aimed at the windows of the apps it is given.
    Act,
}

/// An agent as the configuration in effect describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub access: Access,
    /// App ids (globs) whose windows it may act on.
    pub apps: Vec<String>,
    /// Whether it may click the shell's own interface.
    pub shell: bool,
}

/// What MCP agents did (#176), oldest first: the last few hundred entries
/// of the session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentsJournal {
    /// The user stopped every agent: their requests are refused until
    /// allowed again.
    pub stopped: bool,
    pub entries: Vec<JournalEntry>,
}

/// One action of an agent, carried out or refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// When, as the user reads it: the local date and time,
    /// `2026-10-21T14:37:05`.
    pub at: String,
    pub agent: String,
    pub action: JournalAction,
    /// What it aimed at; none for a connection.
    #[serde(default)]
    pub target: Option<JournalTarget>,
    /// Why it was refused; `None` when carried out.
    #[serde(default)]
    pub refused: Option<String>,
    /// A sensitive action the user confirmed before it was carried out
    /// (#43).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirmed: bool,
}

/// What an agent did, as the journal keeps it: typed text only when the
/// configuration asks for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalAction {
    Click {
        button: Button,
        count: u32,
    },
    Scroll {
        dy: f64,
    },
    Type {
        characters: usize,
        /// What was typed, when `[agents] journal_text` keeps it.
        #[serde(default)]
        text: Option<String>,
    },
    /// The combination pressed; `None` when it is not kept (the session
    /// locked, or a password asked for).
    Keys {
        #[serde(default)]
        keys: Option<String>,
    },
    /// The agent named itself; kept when refused, as while agents are
    /// stopped.
    Connect,
    /// The user handed it a question and an AI's answer (#197): a coding
    /// agent's prompt, its text when `[agents] journal_text` keeps it.
    Prompt {
        characters: usize,
        #[serde(default)]
        text: Option<String>,
    },
}

/// What an agent aimed at, as it was then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalTarget {
    Window {
        id: u64,
        #[serde(default)]
        app_id: Option<String>,
        #[serde(default)]
        title: Option<String>,
        /// The element of its accessibility tree aimed at, by label (#175).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        element: Option<String>,
    },
    /// An element of the shell, by its label.
    Element { label: String },
}

impl std::fmt::Display for JournalEntry {
    /// One line: `2026-10-21 14:37:05 helper: type 4 characters on window
    /// 3 notes "Shopping list"`, then why it was refused.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: ", self.at.replacen('T', " ", 1), self.agent)?;
        match &self.action {
            JournalAction::Click { button, count } => {
                let times = match count {
                    0 | 1 => "",
                    2 => "double ",
                    _ => "triple ",
                };
                let button = match button {
                    Button::Left => "",
                    Button::Right => "right ",
                    Button::Middle => "middle ",
                };
                write!(f, "{times}{button}click")?;
            }
            JournalAction::Scroll { dy } => {
                let way = if *dy < 0.0 { "up" } else { "down" };
                write!(f, "scroll {way} {}", dy.abs())?;
            }
            JournalAction::Type {
                text: Some(text), ..
            } => write!(f, "type {text:?}")?,
            JournalAction::Type {
                characters: 1,
                text: None,
            } => write!(f, "type 1 character")?,
            JournalAction::Type { characters, .. } => write!(f, "type {characters} characters")?,
            JournalAction::Keys { keys: Some(keys) } => write!(f, "keys {keys}")?,
            JournalAction::Keys { keys: None } => write!(f, "keys (not kept)")?,
            JournalAction::Connect => write!(f, "connect")?,
            JournalAction::Prompt {
                text: Some(text), ..
            } => write!(f, "prompted {text:?}")?,
            JournalAction::Prompt { characters, .. } => {
                write!(f, "prompted with {characters} characters")?;
            }
        }
        match &self.target {
            None => {}
            Some(JournalTarget::Window {
                id,
                app_id,
                title,
                element,
            }) => {
                match element {
                    Some(label) => write!(f, " on {label:?} of window {id}")?,
                    None => write!(f, " on window {id}")?,
                }
                if let Some(app_id) = app_id {
                    write!(f, " {app_id}")?;
                }
                if let Some(title) = title {
                    write!(f, " {title:?}")?;
                }
            }
            Some(JournalTarget::Element { label }) => write!(f, " on the shell's {label:?}")?,
        }
        if let Some(why) = &self.refused {
            write!(f, "; refused: {why}")?;
        } else if self.confirmed {
            write!(f, "; confirmed by the user")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    /// The Linux evdev code (`BTN_LEFT`…).
    pub fn code(self) -> u32 {
        match self {
            Self::Left => 0x110,
            Self::Right => 0x111,
            Self::Middle => 0x112,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// An output by name.
    Output(String),
    /// A window by id.
    Window(u64),
}

/// A line from the compositor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerLine {
    Reply {
        id: u64,
        ok: Reply,
    },
    /// `id` is missing when the request line could not be read.
    Error {
        id: Option<u64>,
        error: ApiError,
    },
    Event {
        event: Event,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Version(VersionInfo),
    Outputs(Vec<OutputInfo>),
    Screens(Vec<ScreenInfo>),
    /// What the user chose to share, or nothing.
    Shared(Option<String>),
    Updates(UpdatesInfo),
    Hardware(HardwareInfo),
    Notifications(NotificationsInfo),
    Phone(PhoneInfo),
    Calendars(CalendarsInfo),
    Windows(Vec<WindowInfo>),
    Scene(Scene),
    Screenshot(Screenshot),
    Stats(Stats),
    Desktops(Vec<OutputDesktops>),
    Config(ConfigInfo),
    Ui(Vec<UiNode>),
    AppTree(AppTree),
    /// The agent this connection is now.
    Agent(AgentInfo),
    AgentsJournal(AgentsJournal),
    Ai(AiInfo),
    Search(SearchInfo),
    /// The request was carried out.
    Done,
}

/// What the document search's indexer does (#45).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SearchInfo {
    /// `off`, `waiting` (to start, or for the next look at the folders),
    /// `paused` (on battery), `indexing` or `ready`.
    pub state: String,
    pub documents: u64,
    /// Documents with vectors, searched by meaning too.
    pub embedded: u64,
    /// Entries left out by the list of what is never read, in the last
    /// look: they were not opened.
    pub never_read: u64,
    /// Entries that could not be read, in the last look.
    pub unreadable: u64,
    /// Search by meaning works.
    pub meaning: bool,
    /// Seconds since the last look at the folders ended; `None` before
    /// the first.
    #[serde(default)]
    pub last_look_seconds: Option<u64>,
}

/// What the session knows of the AI providers (#67): which have a key
/// kept, by their names in `[ai] provider`; never the keys themselves.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AiInfo {
    /// `None` until the keyring (or the sealed file) was read.
    pub keys: Option<Vec<String>>,
}

/// An element of the shell's interface, depth first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiNode {
    /// `group`, `text`, `button`, `image`, `icon` or `custom`.
    pub role: String,
    pub label: Option<String>,
    /// In the global logical space.
    pub rect: Rect,
    /// How deep in the tree, from 0.
    pub depth: u32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub selected: bool,
}

/// A window's own accessibility tree (#175), read over AT-SPI.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppTree {
    pub window: u64,
    /// The application as the accessibility bus names it; `None` when
    /// the window has no tree, and `missing` says why.
    #[serde(default)]
    pub application: Option<String>,
    #[serde(default)]
    pub toolkit: Option<String>,
    /// Depth first, the window itself first.
    pub elements: Vec<AppElement>,
    /// The reading stopped at a limit: what was left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
}

/// One element of an application's tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppElement {
    /// As AT-SPI names it: `push button`, `entry`, `password text`.
    pub role: String,
    pub label: Option<String>,
    /// From the window's top-left corner, logical pixels, as its
    /// application says; `None` when it does not.
    pub rect: Option<Rect>,
    /// How deep in the window, from 0.
    pub depth: u32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub selected: bool,
    /// For check boxes, radio buttons, toggles and switches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// What a field holds, its first characters; never a password's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// An output's desktops: its own first, then guests from unplugged outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputDesktops {
    pub output: String,
    pub desktops: Vec<DesktopInfo>,
    /// Id of the desktop it shows.
    pub shown: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopInfo {
    pub id: u32,
    pub name: String,
    /// The output it belongs to and goes back to.
    pub home: String,
    /// Window ids, bottom to top.
    pub windows: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigInfo {
    /// The whole configuration in effect, defaults included.
    pub toml: String,
    /// Why the last file read was not applied, if it was not.
    pub error: Option<String>,
    /// What the configuration in effect left out (unknown actions,
    /// conflicting shortcuts), and why.
    pub warnings: Vec<String>,
    /// Counts reads of the file, applied or not.
    pub generation: u64,
    /// The shell's accent in effect, `#rrggbb` (it may come from the
    /// wallpaper).
    #[serde(default)]
    pub accent: Option<String>,
    /// The file the configuration is read from, if any.
    #[serde(default)]
    pub path: Option<String>,
    /// The account the session is, as it says it.
    #[serde(default)]
    pub user: String,
}

/// Where a swipe is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwipePhase {
    Begin,
    Update,
    End,
    Cancel,
}

/// How well the compositor keeps up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub outputs: Vec<OutputStats>,
    /// The latest loop handlers that went over their budget, oldest first.
    pub slow_handlers: Vec<SlowHandler>,
    /// Times the loop did not answer the watchdog in time, oldest first.
    pub stalls: Vec<Stall>,
    /// Times the loop woke up and handled something, since the start.
    /// Only a compositor running its own loop counts them: one a test
    /// harness turns counts none (`handlers` tells what ran).
    #[serde(default)]
    pub wakeups: u64,
    /// How many times each loop handler ran since the start, by name: what
    /// woke the loop (the watchdog, a client, a worker's result, a timer).
    #[serde(default)]
    pub handlers: Vec<HandlerRuns>,
}

/// A loop handler and how many times it ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandlerRuns {
    pub name: String,
    pub runs: u64,
}

/// Time spent drawing the frames an output presented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputStats {
    pub name: String,
    /// Frames that drew something and were presented.
    pub frames: u64,
    /// Times the scene was built and compared with the last frame, drawn
    /// or not: a pass that finds nothing changed still costs.
    #[serde(default)]
    pub renders: u64,
    pub histogram: Vec<Bucket>,
    pub max_ms: f64,
    /// Times the blur behind something on it was computed (#37): only when
    /// what is behind changes, not every frame.
    #[serde(default)]
    pub blurs: u64,
    /// Frames of a closing window's glitch in the Matrix theme painted
    /// with its colours apart, on the GPU (#169).
    #[serde(default)]
    pub glitches: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bucket {
    /// Upper bound; `None` for the last bucket.
    pub up_to_ms: Option<f64>,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlowHandler {
    pub name: String,
    pub ms: f64,
    /// When it ended, in milliseconds of compositor time.
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stall {
    /// How long the loop had not answered when the watchdog fired.
    pub seconds: f64,
    /// The file with every thread's backtrace, if it could be written.
    pub dump: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    pub protocol: u32,
    pub compositor: String,
}

/// A rectangle in the global logical space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= f64::from(self.x)
            && y >= f64::from(self.y)
            && x < f64::from(self.x + self.width)
            && y < f64::from(self.y + self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub name: String,
    /// Logical position and size.
    pub geometry: Rect,
    pub pixel_width: i32,
    pub pixel_height: i32,
    pub scale: f64,
    /// Night light's colour temperature on it, kelvin; 6500 is daylight.
    #[serde(default = "daylight")]
    pub temperature: u32,
}

fn daylight() -> u32 {
    6500
}

/// Calendars as the session knows them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CalendarsInfo {
    pub accounts: Vec<CalendarStatus>,
    /// Passwords kept, all accounts together.
    pub secrets: usize,
    /// Events from now on, cached ones included.
    pub events: Vec<EventInfo>,
    /// Google accounts through GNOME Online Accounts (#137).
    #[serde(default)]
    pub online: OnlineAccountsInfo,
}

/// GNOME Online Accounts as the session follows it, for the settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OnlineAccountsInfo {
    /// `unknown` until looked at (or without a session bus), `absent`
    /// (not installed), or `installed`.
    pub state: String,
    /// Its Google accounts with their calendar on, read-only here: GOA
    /// adds and removes them.
    pub accounts: Vec<CalendarStatus>,
    /// GNOME's settings for them can be opened (`OpenOnlineAccounts`).
    pub settings: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarStatus {
    pub id: String,
    pub name: String,
    /// Events of it in the agenda.
    pub events: usize,
    /// Why its last sync failed, if it did.
    pub error: Option<String>,
    /// It failed because the server refused the name or password.
    #[serde(default)]
    pub refused: bool,
    pub synced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventInfo {
    pub summary: String,
    /// RFC 3339.
    pub start: String,
    pub link: Option<String>,
}

/// System updates as the session knows them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UpdatesInfo {
    pub updates: Vec<UpdateInfo>,
    /// `idle`, `checking`, `installing`, or `unsupported` (no package
    /// manager known).
    pub state: String,
    /// What the package manager said last went wrong.
    pub error: Option<String>,
}

/// The machine's devices as quick settings know them (#67).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HardwareInfo {
    /// Whether the services were heard from yet; until then the rest is
    /// not known.
    pub known: bool,
    pub wifi: bool,
    /// The network joined, when connected.
    pub network: Option<String>,
    pub airplane: bool,
    pub bluetooth: bool,
    /// The device connected, when there is one.
    pub bluetooth_device: Option<String>,
    /// 0 to 100 (150 when raised past it elsewhere).
    pub volume: u8,
    pub muted: bool,
    pub output_device: String,
    pub input_device: String,
    /// `power-saver`, `balanced` or `performance`.
    pub profile: String,
    /// Those the machine offers.
    pub profiles: Vec<String>,
    pub battery: Option<BatteryInfo>,
    /// Whether the machine can hibernate, as logind says.
    pub can_hibernate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub percent: u8,
    /// While discharging.
    pub minutes_left: Option<u32>,
    /// `charging`, `discharging` (on battery), `full` or `not-charging`.
    pub state: String,
}

/// Notifications as the session knows them (#67).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NotificationsInfo {
    /// Do not disturb: toasts are held, the history keeps them.
    pub dnd: bool,
    /// The applications that sent notifications this session, by the name
    /// they give, the latest first.
    pub apps: Vec<String>,
    /// What the column's history holds now.
    pub history: usize,
}

/// What [`Request::SetHardware`] changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum HardwareChange {
    Wifi {
        on: bool,
    },
    Bluetooth {
        on: bool,
    },
    Airplane {
        on: bool,
    },
    /// 0 to 100.
    Volume {
        percent: u8,
    },
    Muted {
        on: bool,
    },
    /// `power-saver`, `balanced` or `performance`.
    Profile {
        profile: String,
    },
}

/// The phone as the session knows it (#85).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PhoneInfo {
    /// KDE Connect: `off` (the configuration), `looking`, `absent`,
    /// `stopped` (installed, not running) or `running`.
    pub state: String,
    /// The phones paired and within reach; the first is the column's.
    pub phones: Vec<PhoneDevice>,
    /// Wireless adb (#153): `off` (the configuration), `looking`,
    /// `absent` (not installed), `ready` or `pairing`.
    #[serde(default)]
    pub adb: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneDevice {
    pub id: String,
    pub name: String,
    pub battery: Option<u8>,
    pub charging: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub name: String,
    pub from: Option<String>,
    pub to: String,
}

/// A connected screen, on or off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenInfo {
    pub name: String,
    pub make: String,
    pub model: String,
    pub enabled: bool,
    pub modes: Vec<ModeInfo>,
    pub current_mode: Option<usize>,
    pub preferred_mode: Option<usize>,
    pub scale: f64,
    /// `normal`, `90`, `180`, `270`, `flipped`, `flipped-90`…
    pub transform: String,
    /// Logical; for a screen off, where it was.
    pub geometry: Rect,
    pub vrr: bool,
    pub vrr_capable: bool,
    /// Where unplugged screens' desktops go.
    pub primary: bool,
    /// Desktops of its own.
    pub desktops: u32,
    /// The `[[layouts]]` entry in effect for the screens connected.
    pub layout: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModeInfo {
    pub width: i32,
    pub height: i32,
    /// mHz.
    pub refresh: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    /// The output showing most of it.
    pub output: Option<String>,
    pub geometry: Rect,
    pub focused: bool,
    /// Id of its desktop.
    pub desktop: u32,
    /// Whether its desktop is shown and it is not minimized.
    pub visible: bool,
    pub state: WindowState,
    /// 0.1 to 1.
    pub opacity: f64,
    /// Shown on every desktop of its output.
    pub pinned: bool,
    /// It asks for attention (an activation not let take the focus, an
    /// X11 urgency hint), until it has the keyboard.
    #[serde(default)]
    pub attention: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowState {
    #[default]
    Normal,
    Maximized,
    Fullscreen,
    Minimized,
    /// Snapped to a half or a quarter.
    Snapped,
    /// Laid out by its desktop's tiling.
    Tiled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub outputs: Vec<SceneOutput>,
    pub focused: Option<u64>,
    pub pointer: Point,
    /// The cursor the pointer shows: its shape's name as cursor-shape-v1
    /// and CSS say it (`default`, `text`, `e-resize`, `move`…), the
    /// compositor's over a window's frame band, `surface` for a client's
    /// own image, `hidden`.
    #[serde(default)]
    pub cursor: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneOutput {
    pub output: OutputInfo,
    /// Bottom to top.
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    /// Bottom to top.
    pub windows: Vec<WindowInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    /// The PNG file, base64-encoded.
    pub png_base64: String,
}

impl Screenshot {
    pub fn png(&self) -> Result<Vec<u8>, base64::DecodeError> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(&self.png_base64)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    WindowOpened {
        window: WindowInfo,
    },
    WindowClosed {
        id: u64,
    },
    /// `id` is `None` when no window has focus any more.
    WindowFocused {
        id: Option<u64>,
    },
    OutputAdded {
        output: OutputInfo,
    },
    ConfigApplied,
    /// The keyboard switched to another layout.
    KeyboardLayout {
        layout: String,
    },
    /// A notification is shown as a toast.
    Notification {
        id: u32,
        app: String,
        summary: String,
    },
    /// The file was not applied; the previous configuration stays.
    ConfigError {
        message: String,
    },
    /// An output shows another of its desktops.
    DesktopShown {
        output: String,
        desktop: u32,
    },
    OutputRemoved {
        name: String,
    },
    /// A screenshot was taken (#69): where it was saved, if it was, and its
    /// size in pixels.
    Screenshot {
        path: Option<String>,
        width: u32,
        height: u32,
    },
    /// The calendars synced, or their events changed (#75).
    Calendars,
    /// The updates waiting, or their state, changed (#84).
    Updates {
        count: usize,
        state: String,
    },
    /// The sound, network, Bluetooth, power profile or battery changed
    /// (#67), as `Request::Hardware` says them.
    Hardware,
    /// Do not disturb changed, or an application sent its first
    /// notification of the session (#67).
    Notifications,
    /// KDE Connect came or went, or the phones within reach changed
    /// (#85): its state, as [`PhoneInfo`] says it, and how many phones.
    Phone {
        state: String,
        phones: usize,
    },
    /// A screen recording was saved (#70), with its size in pixels.
    Recording {
        path: Option<String>,
        width: u32,
        height: u32,
    },
    /// Leaving the session (#72): `action` is `log-out`, `restart`,
    /// `shut-down`, `suspend` or `hibernate`; `stage` is `countdown`,
    /// `closing`, `cancelled` or `now`.
    Leaving {
        action: String,
        stage: String,
    },
    /// An output's mode, scale, rotation or place changed.
    OutputChanged {
        output: OutputInfo,
    },
    /// An output stopped presenting its frames and was recovered.
    OutputRecovered {
        name: String,
        /// Looks, a second apart, that found the same frame still out.
        missed: u32,
    },
    /// The settings app is asked to show a page (`None`: its home).
    OpenSettings {
        page: Option<String>,
    },
    /// An AI provider's key was kept or forgotten (#67), as `Request::Ai`
    /// says them.
    Ai,
    /// Every MCP agent was stopped, or allowed again (#176).
    AgentsStopped {
        stopped: bool,
    },
    /// The document search's indexer went from one state to another
    /// (#67), as `Request::Search` says it: `off`, `waiting`, `paused`,
    /// `indexing` or `ready`.
    Search {
        state: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:?})", self.message, self.code)
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The line is not a request of this protocol version.
    BadRequest,
    PermissionDenied,
    /// No such output or window.
    NotFound,
    /// The compositor could not carry it out.
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(value: &T, json: &str)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let text = serde_json::to_string(value).expect("serializes");
        assert_eq!(text, json);
        let back: T = serde_json::from_str(&text).expect("deserializes");
        assert_eq!(&back, value);
    }

    #[test]
    fn requests_are_flat_json_lines() {
        round_trip(
            &RequestLine {
                id: 1,
                request: Request::Windows,
            },
            r#"{"id":1,"request":"windows"}"#,
        );
        round_trip(
            &RequestLine {
                id: 2,
                request: Request::PointerButton {
                    button: Button::Left,
                    pressed: true,
                },
            },
            r#"{"id":2,"request":"pointer_button","button":"left","pressed":true}"#,
        );
        round_trip(
            &RequestLine {
                id: 3,
                request: Request::Screenshot {
                    target: Target::Output("HEADLESS-1".into()),
                    cursor: false,
                },
            },
            r#"{"id":3,"request":"screenshot","target":{"output":"HEADLESS-1"},"cursor":false}"#,
        );
    }

    #[test]
    fn a_hardware_change_says_what_it_is() {
        round_trip(
            &RequestLine {
                id: 6,
                request: Request::SetHardware {
                    change: HardwareChange::Volume { percent: 40 },
                },
            },
            r#"{"id":6,"request":"set_hardware","change":{"what":"volume","percent":40}}"#,
        );
        assert!(!Request::Hardware.agents_may());
    }

    #[test]
    fn request_fields_never_shadow_the_line_id() {
        // `id` belongs to the line: a request naming a window says `window`.
        round_trip(
            &RequestLine {
                id: 4,
                request: Request::CloseWindow { window: 9 },
            },
            r#"{"id":4,"request":"close_window","window":9}"#,
        );
    }

    #[test]
    fn replies_errors_and_events_are_told_apart() {
        round_trip(
            &ServerLine::Reply {
                id: 1,
                ok: Reply::Done,
            },
            r#"{"id":1,"ok":"done"}"#,
        );
        round_trip(
            &ServerLine::Error {
                id: Some(2),
                error: ApiError::new(ErrorCode::NotFound, "no output named X"),
            },
            r#"{"id":2,"error":{"code":"not_found","message":"no output named X"}}"#,
        );
        round_trip(
            &ServerLine::Event {
                event: Event::WindowFocused { id: Some(4) },
            },
            r#"{"event":{"type":"window_focused","id":4}}"#,
        );
    }

    #[test]
    fn driving_input_needs_the_test_permission() {
        assert_eq!(Request::Windows.permission(), Permission::Observe);
        assert_eq!(
            Request::Key {
                code: 30,
                pressed: true
            }
            .permission(),
            Permission::Test
        );
        assert_eq!(Request::Scene.permission(), Permission::Test);
    }

    #[test]
    fn an_agent_acts_aimed_at_a_window_or_an_element() {
        round_trip(
            &RequestLine {
                id: 5,
                request: Request::Act {
                    target: ActTarget::Window {
                        window: 3,
                        at: Some(Point { x: 10.0, y: 20.0 }),
                    },
                    action: Action::Click {
                        button: Button::Left,
                        count: 1,
                    },
                    intent: None,
                },
            },
            r#"{"id":5,"request":"act","target":{"window":{"window":3,"at":{"x":10.0,"y":20.0}}},"action":{"click":{"button":"left","count":1}}}"#,
        );
        round_trip(
            &RequestLine {
                id: 8,
                request: Request::Act {
                    target: ActTarget::Window {
                        window: 3,
                        at: None,
                    },
                    action: Action::Keys {
                        keys: "Return".into(),
                    },
                    intent: Some("Sends the message".into()),
                },
            },
            r#"{"id":8,"request":"act","target":{"window":{"window":3,"at":null}},"action":{"keys":{"keys":"Return"}},"intent":"Sends the message"}"#,
        );
        round_trip(
            &RequestLine {
                id: 6,
                request: Request::Agent {
                    name: "helper".into(),
                },
            },
            r#"{"id":6,"request":"agent","name":"helper"}"#,
        );
        round_trip(
            &RequestLine {
                id: 9,
                request: Request::Act {
                    target: ActTarget::AppElement {
                        window: 3,
                        role: Some("push button".into()),
                        label: "Send".into(),
                    },
                    action: Action::Click {
                        button: Button::Left,
                        count: 1,
                    },
                    intent: None,
                },
            },
            r#"{"id":9,"request":"act","target":{"app_element":{"window":3,"role":"push button","label":"Send"}},"action":{"click":{"button":"left","count":1}}}"#,
        );
    }

    #[test]
    fn an_application_tree_is_read_by_window() {
        round_trip(
            &RequestLine {
                id: 4,
                request: Request::WindowTree { window: 3 },
            },
            r#"{"id":4,"request":"window_tree","window":3}"#,
        );
        assert!(Request::WindowTree { window: 3 }.agents_may());
        assert_eq!(
            Request::WindowTree { window: 3 }.permission(),
            Permission::Observe
        );
        let tree = AppTree {
            window: 3,
            application: Some("mail".into()),
            toolkit: None,
            elements: vec![AppElement {
                role: "push button".into(),
                label: Some("Send".into()),
                rect: Some(Rect {
                    x: 1,
                    y: 2,
                    width: 3,
                    height: 4,
                }),
                depth: 2,
                focused: false,
                disabled: false,
                selected: false,
                checked: None,
                value: None,
            }],
            partial: None,
            missing: None,
        };
        let line = ServerLine::Reply {
            id: 4,
            ok: Reply::AppTree(tree),
        };
        let text = serde_json::to_string(&line).expect("serializes");
        assert!(
            !text.contains("value") && !text.contains("partial"),
            "{text}"
        );
        let back: ServerLine = serde_json::from_str(&text).expect("deserializes");
        assert_eq!(back, line);
    }

    #[test]
    fn a_journal_entry_reads_as_one_line() {
        let entry = JournalEntry {
            at: "2026-10-21T14:37:05".into(),
            agent: "helper".into(),
            action: JournalAction::Type {
                characters: 4,
                text: None,
            },
            target: Some(JournalTarget::Window {
                id: 3,
                app_id: Some("notes".into()),
                title: Some("Shopping list".into()),
                element: None,
            }),
            refused: None,
            confirmed: false,
        };
        assert_eq!(
            entry.to_string(),
            "2026-10-21 14:37:05 helper: type 4 characters on window 3 notes \"Shopping list\""
        );
        let on_element = JournalEntry {
            action: JournalAction::Click {
                button: Button::Left,
                count: 1,
            },
            target: Some(JournalTarget::Window {
                id: 3,
                app_id: Some("mail".into()),
                title: Some("Compose".into()),
                element: Some("Send".into()),
            }),
            ..entry.clone()
        };
        assert_eq!(
            on_element.to_string(),
            "2026-10-21 14:37:05 helper: click on \"Send\" of window 3 mail \"Compose\""
        );
        let refused = JournalEntry {
            action: JournalAction::Click {
                button: Button::Right,
                count: 2,
            },
            target: Some(JournalTarget::Element {
                label: "Close".into(),
            }),
            refused: Some("agents are stopped".into()),
            ..entry.clone()
        };
        assert_eq!(
            refused.to_string(),
            "2026-10-21 14:37:05 helper: double right click on the shell's \"Close\"; refused: \
             agents are stopped"
        );
        let keys = JournalEntry {
            action: JournalAction::Keys { keys: None },
            ..entry
        };
        assert!(keys.to_string().contains("keys (not kept) on window 3"));
        let confirmed = JournalEntry {
            confirmed: true,
            ..keys.clone()
        };
        assert!(confirmed.to_string().ends_with("; confirmed by the user"));
        let connect = JournalEntry {
            action: JournalAction::Connect,
            target: None,
            refused: Some("agents are stopped".into()),
            ..keys
        };
        assert_eq!(
            connect.to_string(),
            "2026-10-21 14:37:05 helper: connect; refused: agents are stopped"
        );
        round_trip(
            &RequestLine {
                id: 7,
                request: Request::AgentsJournal,
            },
            r#"{"id":7,"request":"agents_journal"}"#,
        );
        assert!(Request::AgentsJournal.agents_may());
        assert_eq!(Request::AgentsJournal.permission(), Permission::Observe);
    }

    #[test]
    fn agents_read_and_act_but_never_drive_raw_input_or_change_the_session() {
        assert!(Request::Windows.agents_may());
        assert!(
            Request::Act {
                target: ActTarget::Element {
                    label: "Close".into()
                },
                action: Action::Scroll { dy: 15.0 },
                intent: None,
            }
            .agents_may()
        );
        for request in [
            Request::PointerMove { x: 0.0, y: 0.0 },
            Request::Key {
                code: 30,
                pressed: true,
            },
            Request::Config,
            Request::Scene,
            Request::Calendars,
            Request::InstallUpdates,
            Request::CloseWindow { window: 1 },
            Request::CopyPng {
                png_base64: String::new(),
            },
            // An agent never allows itself again, nor reads the keys kept.
            Request::SetAgentsStopped { stopped: false },
            Request::Ai,
            Request::Search,
            Request::SearchAgain,
        ] {
            assert!(!request.agents_may(), "{request:?}");
        }
        // Naming oneself is open; acting otherwise needs the test permission.
        assert_eq!(
            Request::Agent { name: "x".into() }.permission(),
            Permission::Observe
        );
    }

    #[test]
    fn the_control_socket_sits_next_to_the_wayland_one() {
        assert_eq!(
            socket_for(Path::new("/run/user/1000/hocus-0")),
            Path::new("/run/user/1000/hocus-0.api")
        );
    }
}
