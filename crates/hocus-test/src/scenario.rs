//! Scenarios: TOML files of steps and expectations, each run against its own
//! headless compositor on a virtual clock, through the control API.
//!
//! ```toml
//! description = "A click on the back window focuses and raises it"
//! outputs = ["1280x800"]
//!
//! [[step]]
//! open = { app_id = "back", size = [400, 300], color = "#3399ff" }
//! [[step]]
//! pointer_move = [450, 260]
//! [[step]]
//! click = "left"
//! [[step]]
//! expect_focus = "back"
//! ```

use std::{
    fmt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::Context;
use hocus_api::{Button, Reply, Request, Target, WindowInfo, WindowState};
use hocus_compositor::{
    FRAME, HarnessOptions, Headless, HeadlessConfig, OutputSpec, RendererChoice,
};
use serde::Deserialize;

use crate::{
    client::{SolidWindow, TestClient},
    image::{Difference, Image, Tolerance},
    x11::{X11Client, X11Window},
};

/// How long a step may wait for a client, in real time.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long an expectation waits for a client to catch up (a resize takes
/// milliseconds), before it fails.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// What the scenario shows, in a sentence.
    pub description: String,
    /// Output specs, `WIDTHxHEIGHT[@SCALE][+X+Y]`; one 1280×800 by default.
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Starts XWayland, for X11 test clients; skipped where it is missing.
    #[serde(default)]
    pub xwayland: bool,
    /// Shows the widget column on the primary output.
    #[serde(default)]
    pub shell: bool,
    /// Gives the compositor a private D-Bus bus (notifications).
    #[serde(default)]
    pub dbus: bool,
    /// A stand-in for timedated in this time zone, on a private system bus
    /// the compositor and the settings follow; never the machine's.
    pub timedated: Option<String>,
    /// Stand-ins for UPower (a battery at 80 % on its charger),
    /// NetworkManager (Wi-Fi on, joined), BlueZ (on) and power profiles
    /// (balanced), on a private system bus; never the machine's.
    #[serde(default)]
    pub devices: bool,
    /// Stand-ins for AccountsService ("Alex Panda", no picture) and fprintd
    /// (a reader, one finger enrolled), on a private system bus the
    /// settings ask; never the machine's.
    #[serde(default)]
    pub accounts: bool,
    /// A stand-in for GNOME Online Accounts on the scenario's private
    /// session bus (never the session's), with a Google account signed in,
    /// and one for Google's CalDAV on 127.0.0.1 serving the `.ics` files of
    /// `dir` as its calendar: `{ identity, dir, running? = true }` (#137).
    pub goa: Option<GoaFixture>,
    /// Programs the scenario runs; it is skipped where one is missing.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Where the launcher reads desktop entries, relative to the scenario.
    #[serde(default)]
    pub data_dirs: Vec<PathBuf>,
    /// The system's configuration directories (autostart entries,
    /// `mimeapps.list`), relative to the scenario, which the settings read
    /// after the user's own: a private one, written.
    #[serde(default)]
    pub config_dirs: Vec<PathBuf>,
    /// Windows move as in a session, for scenarios that sample motion;
    /// otherwise they appear and go at once.
    #[serde(default)]
    pub motion: bool,
    /// The first session: the welcome opens (#82).
    #[serde(default)]
    pub first_run: bool,
    /// What draws: pixman, the same pixels everywhere, or GLES on a render
    /// node, for shaders (skipped where there is none; its pixels are the
    /// GPU's, so no screenshots).
    #[serde(default)]
    pub renderer: ScenarioRenderer,
    /// What started applications inherit (`PATH` is added if missing),
    /// instead of the test's own environment; `{scenario_dir}` in values.
    pub child_env: Option<std::collections::BTreeMap<String, String>>,
    /// A home of the scenario's own: this folder (relative to the
    /// scenario) copied into a fresh temporary directory, `{home}` in
    /// configuration, `type` and `unreadable` steps. Documents are searched
    /// there, never in the developer's; it is the settings app's home.
    pub home: Option<PathBuf>,
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
    /// The file's name without extension.
    #[serde(skip)]
    pub name: String,
    /// Where reference images are looked up.
    #[serde(skip)]
    pub dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Starts a test client with a window.
    Open(OpenWindow),
    /// Starts an X11 test client with a window, through XWayland.
    OpenX11(OpenWindow),
    /// The client of this app id, Wayland or X11, takes the clipboard.
    Copy(Clipboard),
    /// The client of this app id reads this text from the clipboard.
    ExpectClipboard(Clipboard),
    /// The shell's accent in effect has a hue within `hue = [from, to]`
    /// (degrees), and reads on the theme's surfaces.
    ExpectAccent(ExpectAccent),
    /// Types text, as on a US keyboard; `{caldav}` is the test server's
    /// calendar, `{ai}` the AI provider's address.
    Type(String),
    /// A file a started program wrote in the compositor's runtime
    /// directory: `{ name, contains?, lacks? }`.
    ExpectRuntimeFile(ExpectFile),
    /// A file of the compositor's state directory (notes, the session):
    /// `{ name, contains?, lacks? }`, once written.
    ExpectStateFile(ExpectFile),
    /// The notification server sent these signals, in this order (others
    /// between them), since the first `notify` step: `["ActivationToken
    /// 1", "ActionInvoked 1 reply"]`.
    ExpectNotificationSignals(Vec<String>),
    /// Runs `notify-send` with these arguments on the private bus.
    NotifySend(Vec<String>),
    /// Sends a notification on the private bus.
    Notify(NotifyStep),
    /// Clicks the shell's button with this label (or accessible name).
    ClickUi(String),
    /// Calls a tool of the MCP server (`hocus mcp`, #42) in a session of
    /// its own, as an agent of `[[agents.mcp]]` (or in that agent's lasting
    /// session, when one is open), and checks the result: `{ agent?, tool,
    /// window?, arguments?, contains?, error?, wait? }`. While it waits, an
    /// action shown before it lands moves the clock to when it does; with
    /// `wait = false` the step goes on, and `expect_mcp` checks the result
    /// later.
    Mcp(McpCall),
    /// An MCP session as this agent that lasts until `mcp_end` or the end
    /// of the scenario: its seat (#43) stays meanwhile.
    McpSession(String),
    /// That agent's lasting session ends.
    McpEnd(String),
    /// The result of the oldest call left with `wait = false` (that
    /// agent's, when `agent` is given), once it comes: `{ agent?,
    /// contains?, error? }`.
    ExpectMcp(ExpectMcp),
    /// The agents' journal (#176), as `hocus msg agents-journal` prints it:
    /// `{ has? = […], lacks? = […], stopped? }`, the lines holding `has` in
    /// that order (one after the other, a later string on the same line or
    /// a later one), none holding `lacks`; whether agents are stopped.
    ExpectJournal(ExpectJournal),
    /// The same with the secondary button.
    RightClickUi(String),
    /// A tray application registers on the private bus: `{ title, color,
    /// menu?, delay_ms? }`; a menu entry `"-"` is a separator.
    TrayApp(TrayAppStep),
    /// A tray application asks for attention, or stops: `{ title,
    /// attention }`.
    TrayAttention(TrayAttention),
    /// A tray application was asked this (`activate`, `clicked 2`).
    ExpectTray(ExpectTray),
    /// KDE Connect, a stand-in on the private bus, with a phone paired
    /// and within reach: `{ name, battery, charging? }`; again, its
    /// battery changes.
    Phone(PhoneStep),
    /// The phone posts a notification: `{ app, title, text, reply? }`.
    PhoneNotification(PhoneNotificationStep),
    /// The notification the phone posted `n`th (from 1) goes on the phone.
    PhoneRemove(usize),
    /// The phone was asked this: `ring`, `reply ID TEXT`, `clipboard
    /// TEXT`, `share URL`, `browse` (`{runtime_dir}` as in `config`).
    ExpectPhone(String),
    /// The timedated stand-in was asked this: `SetTimezone ZONE
    /// interactive`.
    ExpectTimedated(String),
    /// The system's zone changes in the timedated stand-in, as if set
    /// elsewhere.
    TimedatedZone(String),
    /// The timedated stand-in refuses to set the zone from now on, as
    /// polkit would, or no longer.
    TimedatedRefuses(bool),
    /// The charger is plugged in (`true`) or pulled out, as UPower says.
    Charger(bool),
    /// The account's stand-ins were asked this: `SetRealName NAME`,
    /// `SetIconFile PATH`.
    ExpectAccounts(String),
    /// A file of the private configuration directory the settings write
    /// (`mimeapps.list`, `autostart/x.desktop`): `{ name, contains?,
    /// lacks? }`, once written.
    ExpectXdgFile(ExpectFile),
    /// The devices' stand-ins were asked this: `WirelessEnabled false`,
    /// `Powered true`, `ActiveProfile power-saver`.
    ExpectDevices(String),
    /// KDE Connect as the session found it, once so: `absent`,
    /// `stopped`, `running`, `off`.
    ExpectPhoneState(String),
    /// The phone scans the pairing QR code the popover shows (#153): the
    /// name and password it holds go to the adb stand-in, as `NAME
    /// PASSWORD` in `adb-phone` in the compositor's runtime directory.
    ScanPairingCode(bool),
    /// Wireless adb as the session has it, once so (within seconds: adb
    /// runs on workers): `off`, `absent`, `ready`, `pairing`.
    ExpectAdb(String),
    /// The shell shows an element with this label: `{ label, disabled? }`.
    ExpectUi(ExpectUi),
    /// Writes the saved session file (TOML); `{test_client}` is the test
    /// client program.
    SessionFile(String),
    /// Saves the session now.
    SaveSession(bool),
    /// Offers to put the saved session back, as at start.
    OfferSession(bool),
    /// The saved session file holds these strings.
    ExpectSessionFile(ExpectConfigFile),
    /// A lock client locks the session with a surface of this colour on
    /// every output: `"#rrggbb"`; waits until it is locked.
    LockClient(String),
    /// The lock client unlocks.
    UnlockClient(bool),
    /// The lock client goes away without unlocking, as a crash would.
    CrashLockClient(bool),
    /// A screen sharing tool starts copying the first output again and
    /// again (wlr-screencopy with damage).
    Share(bool),
    /// It stops.
    StopSharing(bool),
    /// The compositor ended it: the tool got no more frames.
    ExpectSharingEnded(bool),
    /// A screen-cast portal's chooser asks what to share, and waits.
    AskShare(bool),
    /// It was answered this (`Monitor: HEADLESS-1`); `""` for nothing.
    ExpectShared(String),
    /// Starts a layer-shell client: `{ namespace, layer, anchor?, size,
    /// exclusive_zone?, keyboard?, color }`; waits until it is shown.
    OpenLayer(crate::layer::LayerSpec),
    /// Closes the layer client of this namespace.
    CloseLayer(String),
    /// The client of this app id asks for its window to come forward,
    /// with an activation token of its own.
    Activate(String),
    /// The same with a token made without a serial, from no input of the
    /// user's: it asks for attention.
    ActivateWithoutSerial(String),
    /// The same with the activation token a program the compositor
    /// started was given, read off a runtime file (`env` written there),
    /// as a running instance is handed it by a new one: `{ app_id,
    /// token_from }`.
    ActivateWith(ActivateWith),
    /// An X11 client sets its urgency hint or takes it back: `{ app_id,
    /// urgent }`.
    X11Urgent(X11Urgent),
    /// The same with a token it took more than ten seconds before (the
    /// virtual clock moves on meanwhile): too old to bring it forward, it
    /// asks for attention.
    AskAttention(String),
    /// Starts the settings app on a page (or its home), and waits for its
    /// window: `{ page? }`.
    OpenSettings(OpenSettings),
    /// The settings window shows an element: `{ label, focused?,
    /// selected?, shown? }`.
    ExpectSettings(ExpectSettings),
    /// Clicks the element of the settings window with this label.
    ClickSettings(String),
    /// Clicks it at a place other than its middle: `{ label, at = [x, y]
    /// }`, fractions of its width and height from its corner (a city on
    /// the world map).
    ClickSettingsAt(ClickAt),
    /// The compositor goes as a crash would, and a new one starts on the
    /// same configuration and state; clients go with the old one.
    Restart(bool),
    /// Waits until the wallpapers are made and drawn: before a capture
    /// the compositor takes itself.
    WaitWallpapers(bool),
    /// A CalDAV server on 127.0.0.1 serves the `.ics` files of `dir`
    /// (relative to the scenario) to `user` and `password`; its calendar's
    /// address is `{caldav}` in later steps.
    Caldav(CaldavStep),
    /// The last recording saved, as ffprobe and ffmpeg read it, once
    /// done: `{ audio?, keys_at_ms? }`: with a sound track as long as the
    /// picture; keys drawn near the bottom from this time in the video.
    ExpectRecording(ExpectRecording),
    /// Nothing is drawn while `for_ms` of virtual time pass: at rest, no
    /// frame.
    ExpectIdle(ExpectIdle),
    /// Every output draws at least `at_least` frames while `for_ms` of
    /// virtual time pass: something moves.
    ExpectFrames(ExpectFrames),
    /// A touchpad swipe on the screen under the pointer: `{ fingers, by =
    /// [dx, dy], begin? = true, release? = true }`, moved in eight steps a
    /// frame apart; without `begin`, it goes on with the one held.
    Swipe(SwipeStep),
    /// The CalDAV server goes, as the network would.
    CaldavStop(bool),
    /// A herdr for the test (never the user's), its socket in the
    /// compositor's runtime directory: `{ agents = [{ name, status }] }`;
    /// the socket is `{herdr}` in later configuration.
    Herdr(HerdrStep),
    /// An agent's status changes in it, once the compositor follows that
    /// agent: `{ agent, status, label? }`.
    HerdrStatus(HerdrStatus),
    /// It was asked this: `agent.focus refacto`, `agent.send_keys refacto
    /// enter`.
    ExpectHerdr(String),
    /// The settings keep a calendar account's password: `{ id, password }`.
    CalendarPassword(CalendarPassword),
    /// An AI provider on 127.0.0.1 (ollama, OpenAI's chat completions,
    /// Claude's Messages API) answers every question with these pieces,
    /// `delay_ms` before each (and `first_ms` more before the first); its
    /// address is `{ai}` in later configuration.
    AiServer(AiServerStep),
    /// An AI provider's key is kept, as `hocus msg ai-key` does: `{
    /// provider, key }`.
    AiKey(AiKeyStep),
    /// The AI provider was asked, once so: `{ path?, contains? = […],
    /// header? }` (a header as `name: value`, the name in lower case).
    ExpectAi(ExpectAi),
    /// The document search's indexer, once so: `{ state?, documents?,
    /// embedded?, never_read?, unreadable?, meaning? }` (#45).
    ExpectSearch(ExpectSearch),
    /// A file or folder of the private home made unreadable (mode 000):
    /// reading it would fail, and show as `unreadable`.
    Unreadable(String),
    /// The calendars as the session knows them, once so: `{ events?,
    /// secrets?, contains?, error?, online?, online_accounts? }`.
    ExpectCalendars(ExpectCalendars),
    /// The sealed calendar cache, opened, once so: `{ contains?, lacks? }`.
    ExpectCalendarCache(ExpectCache),
    /// Something changes in GNOME Online Accounts' stand-in: `run` (its
    /// name taken), `sign_in`, `remove`, `calendar_off`, `calendar_on`,
    /// `revoke` (Google refuses the token it holds until refreshed),
    /// `break_tokens` (Google refuses every token).
    Goa(GoaChange),
    /// GOA's and Google's stand-ins were asked these, in this order, each
    /// the start of a line of their log: `["GetAccessToken", "REPORT"]`.
    ExpectGoa(Vec<String>),
    /// Opens the annotation window on a capture (`{runtime_dir}` is the
    /// compositor's temporary directory), and waits for it.
    OpenAnnotate(String),
    /// The annotation window shows an element, as `expect_settings`.
    ExpectAnnotate(ExpectSettings),
    /// Clicks its element with this label.
    ClickAnnotate(String),
    /// Drags from its element's middle by `by`, as `drag_settings`.
    DragAnnotate(DragSettings),
    /// Opens the recording editor on a file (`{last_recording}` is the
    /// last one saved), and waits for it.
    OpenTrim(String),
    /// The same three as the annotation window's, for the editor.
    ExpectTrim(ExpectSettings),
    ClickTrim(String),
    DragTrim(DragSettings),
    /// A file the editor exported beside the recording, once written:
    /// `{ name_has, length = [from, to]?, max_bytes? }`, read by ffprobe.
    ExpectExport(ExpectExport),
    /// Presses on the settings window's element with this label, moves by
    /// `by`, and lets go: a drag.
    DragSettings(DragSettings),
    /// The configuration file holds these strings, or lacks them.
    ExpectConfigFile(ExpectConfigFile),
    /// Asks the window of this app id to close, and waits until it is gone.
    Close(String),
    PointerMove([f64; 2]),
    /// Moves the pointer through these points in turn: a drawn line.
    PointerPath(Vec<[f64; 2]>),
    /// Presses and releases a button.
    Click(ButtonName),
    /// Presses a button and holds it (for drags).
    Press(ButtonName),
    Release(ButtonName),
    /// Scrolls the wheel, in logical pixels: 15 per notch, down positive.
    Scroll(f64),
    /// Holds a key down, by name or evdev code (for Alt+drag).
    KeyDown(String),
    KeyUp(String),
    /// Presses and releases a key, by evdev code.
    Key(u32),
    /// Presses keys in order and releases them in reverse, e.g.
    /// `"super+shift+2"`: names or evdev codes.
    Keys(String),
    /// Sends a window to an output: its `desktop`th desktop (from 1), or the
    /// one it shows.
    Send(SendWindow),
    /// Plugs an output; `name` brings back one unplugged before.
    Plug(PlugOutput),
    /// Unplugs an output by name.
    Unplug(String),
    /// The output stops presenting its frames, as a stuck screen would.
    StallOutput(String),
    /// Maximizes, makes fullscreen, minimizes or restores (`normal`) a
    /// window: `{ app_id, state }`.
    SetState(SetState),
    /// Writes the configuration file, and waits until it is read.
    Config(String),
    /// The last configuration file was refused with a message containing
    /// this; `""` checks it was applied.
    ExpectConfigError(String),
    /// What the configuration in effect left out contains this.
    ExpectConfigWarning(String),
    /// The compositor said this in its event stream since the start:
    /// `{ type = "keyboard_layout", has = "French" }`.
    ExpectEvent(ExpectEvent),
    /// Moves the virtual clock forward.
    AdvanceMs(u64),
    /// Moves the virtual clock forward by whole frames.
    Frames(u32),
    ExpectWindow(ExpectWindow),
    ExpectNoWindow(String),
    /// The app id of the focused window.
    ExpectFocus(String),
    /// The desktop an output shows, from 1.
    ExpectShown(ExpectShown),
    /// Sets a screen through wlr-output-management, as wlr-randr would.
    Randr(crate::randr::Change),
    /// An output's logical geometry, pixel size and scale; or that it is
    /// off (`shown = false`).
    ExpectOutput(ExpectOutput),
    /// App ids of all windows, bottom to top.
    ExpectStack(Vec<String>),
    /// The client got this event, e.g. `"key 30 pressed"`.
    ExpectReceived(ExpectReceived),
    /// The client never got this event.
    ExpectNotReceived(ExpectReceived),
    ExpectScreenshot(ExpectScreenshot),
    /// One pixel of an output's screenshot, in its pixels.
    ExpectPixel(ExpectPixel),
    /// Every frame so far drawn within `max_ms`, and no loop handler over
    /// its budget.
    ExpectBudget(ExpectBudget),
    /// The blur behind panels was computed so many times since the last
    /// such step, or the start: `{ at_least?, at_most? }`.
    ExpectBlur(ExpectBlur),
    /// Frames of a closing window's glitch painted with its colours apart
    /// on the GPU (#169), so many since the last such step, or the start:
    /// `{ at_least?, at_most? }`.
    ExpectGlitch(ExpectBlur),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectBlur {
    pub at_least: Option<u64>,
    pub at_most: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectPixel {
    pub output: String,
    /// `[x, y]` in the output's pixels.
    pub at: [u32; 2],
    /// `#rrggbb`.
    pub color: String,
    /// Largest difference on a channel; 24 by default.
    pub tolerance: Option<u8>,
    /// Whether the colour must be there or must not.
    #[serde(default = "yes")]
    pub is: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenWindow {
    pub app_id: String,
    pub size: [u32; 2],
    /// `#rrggbb`.
    pub color: String,
    /// Holds the compositor's shortcuts back while it has the keyboard.
    #[serde(default)]
    pub inhibit_shortcuts: bool,
    /// Keeps the session awake while shown, as a video player does.
    #[serde(default)]
    pub inhibit_idle: bool,
    /// Its title; none by default.
    #[serde(default)]
    pub title: Option<String>,
    /// Its field is for a password, as its text inputs say (#43).
    #[serde(default)]
    pub password: bool,
    /// Draws a white mark for each key and button pressed, paced on frame
    /// callbacks as applications are (#229).
    #[serde(default)]
    pub marks: bool,
    /// Exposes an accessibility tree on a private accessibility bus the
    /// compositor reads (#175), its elements drawn where the tree says:
    /// `compose` (a subject, a password, Send, Send later, a disabled
    /// Delete), its frame named as the window's title.
    #[serde(default)]
    pub accessible: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectJournal {
    #[serde(default)]
    pub has: Vec<String>,
    #[serde(default)]
    pub lacks: Vec<String>,
    pub stopped: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCall {
    /// The agent the server names itself; none: a plain client.
    pub agent: Option<String>,
    pub tool: String,
    /// The window of this app id, as the `window` argument (its id).
    pub window: Option<String>,
    #[serde(default)]
    pub arguments: toml::Table,
    /// The result's text holds these strings.
    #[serde(default)]
    pub contains: Vec<String>,
    /// The tool failed, with this in its message; otherwise it must
    /// succeed.
    pub error: Option<String>,
    /// The step waits for the result (by default), or goes on.
    #[serde(default = "yes")]
    pub wait: bool,
    /// The image of the result (a capture) is this reference, next to the
    /// scenario.
    pub image: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectMcp {
    pub agent: Option<String>,
    #[serde(default)]
    pub contains: Vec<String>,
    pub error: Option<String>,
}

/// A call to a lasting session, and where its answer goes.
type McpCallIn = (
    serde_json::Value,
    std::sync::mpsc::Sender<Option<serde_json::Value>>,
);

/// An MCP session that lasts, on a thread of its own: calls go in, each
/// answered on its own channel.
#[derive(Debug)]
struct McpSession {
    agent: String,
    calls: std::sync::mpsc::Sender<McpCallIn>,
}

/// A call whose answer is still to come.
#[derive(Debug)]
struct McpWaiting {
    agent: Option<String>,
    answer: std::sync::mpsc::Receiver<Option<serde_json::Value>>,
    /// A session of its own: the agents' seats there were before it.
    fresh: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectAccent {
    pub hue: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwipeStep {
    pub fingers: u32,
    pub by: [f64; 2],
    #[serde(default = "yes")]
    pub begin: bool,
    #[serde(default = "yes")]
    pub release: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectIdle {
    pub for_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectFrames {
    pub for_ms: u64,
    pub at_least: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioRenderer {
    #[default]
    Pixman,
    Gles,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectExport {
    pub name_has: String,
    /// Seconds.
    pub length: Option<[f64; 2]>,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectRecording {
    pub audio: Option<bool>,
    pub keys_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaldavStep {
    pub dir: PathBuf,
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoaFixture {
    pub identity: String,
    pub dir: PathBuf,
    /// Its name taken from the start; otherwise once a `goa = "run"` step.
    #[serde(default = "yes")]
    pub running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoaChange {
    Run,
    SignIn,
    Remove,
    CalendarOff,
    CalendarOn,
    Revoke,
    BreakTokens,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectCache {
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default)]
    pub lacks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarPassword {
    pub id: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectCalendars {
    /// Events from now on.
    pub events: Option<usize>,
    /// Passwords kept.
    pub secrets: Option<usize>,
    /// Events by title.
    #[serde(default)]
    pub contains: Vec<String>,
    /// Whether an account failed its last sync.
    pub error: Option<bool>,
    /// GNOME Online Accounts: `unknown`, `absent` or `installed`.
    pub online: Option<String>,
    /// Its Google accounts with their calendar on.
    pub online_accounts: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectFile {
    pub name: String,
    /// How many lines it has.
    pub lines: Option<usize>,
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default)]
    pub lacks: Vec<String>,
}

/// Which client window the settings steps drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Driving {
    #[default]
    Settings,
    /// The annotation window or the recording editor, by app id.
    Editor(&'static str),
}

impl Driving {
    fn app_id(self) -> &'static str {
        match self {
            Self::Settings => "hocus-settings",
            Self::Editor(app_id) => app_id,
        }
    }
}

const ANNOTATE: Driving = Driving::Editor("hocus-annotate");
const TRIM: Driving = Driving::Editor("hocus-trim");

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrayAppStep {
    pub title: String,
    /// `"#rrggbb"`.
    pub color: String,
    #[serde(default)]
    pub menu: Vec<String>,
    #[serde(default)]
    pub delay_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrayAttention {
    pub title: String,
    pub attention: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiServerStep {
    pub pieces: Vec<String>,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default)]
    pub first_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiKeyStep {
    pub provider: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectAi {
    pub path: Option<String>,
    #[serde(default)]
    pub contains: Vec<String>,
    pub header: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectSearch {
    pub state: Option<String>,
    pub documents: Option<u64>,
    pub embedded: Option<u64>,
    pub never_read: Option<u64>,
    pub unreadable: Option<u64>,
    pub meaning: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HerdrStep {
    /// Several herdr have names: `{herdr:NAME}` is the socket of one.
    pub name: Option<String>,
    pub agents: Vec<HerdrAgent>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HerdrAgent {
    pub name: String,
    /// `working`, `blocked`, `done`, `idle`.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HerdrStatus {
    pub agent: String,
    pub status: String,
    /// What its integration says with it: the question, while blocked.
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectTray {
    pub title: String,
    pub asked: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhoneStep {
    pub name: String,
    pub battery: i32,
    #[serde(default)]
    pub charging: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhoneNotificationStep {
    pub app: String,
    pub title: String,
    pub text: String,
    /// Its application takes replies.
    #[serde(default = "yes")]
    pub reply: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotifyStep {
    pub app: String,
    pub summary: String,
    #[serde(default)]
    pub body: String,
    /// `[key, label]` pairs; `default` is a click on the toast.
    #[serde(default)]
    pub actions: Vec<[String; 2]>,
    /// `low`, `normal` or `critical`.
    pub urgency: Option<String>,
    /// -1 lets the server choose, 0 never expires.
    pub timeout_ms: Option<i32>,
    pub replaces: Option<u32>,
    /// Hints of text: `{ "x-kde-appname" = "kdeconnect" }`.
    #[serde(default)]
    pub hints: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectUi {
    pub label: String,
    pub disabled: Option<bool>,
    /// `[x, y, width, height]` the element lies within, global logical.
    pub inside: Option<[i32; 4]>,
    /// The current choice in a list.
    pub selected: Option<bool>,
    /// `false` checks that no element has this label.
    #[serde(default = "yes")]
    pub shown: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenSettings {
    pub page: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectSettings {
    pub label: String,
    /// Has the keyboard.
    pub focused: Option<bool>,
    /// On, or the current choice.
    pub selected: Option<bool>,
    /// `false` checks that no element has this label.
    #[serde(default = "yes")]
    pub shown: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectConfigFile {
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default)]
    pub lacks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateWith {
    pub app_id: String,
    /// A file of the runtime directory with an `XDG_ACTIVATION_TOKEN=`
    /// line.
    pub token_from: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct X11Urgent {
    pub app_id: String,
    pub urgent: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clipboard {
    pub app_id: String,
    pub text: String,
    /// Marked secret, as a password manager copies.
    #[serde(default)]
    pub secret: bool,
    /// Copied at once after `text`, marked secret, in the same request.
    #[serde(default)]
    pub then_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectEvent {
    #[serde(rename = "type")]
    pub kind: String,
    /// Text the event holds, in its JSON form.
    pub has: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetState {
    pub app_id: String,
    pub state: WindowState,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlugOutput {
    /// `WIDTHxHEIGHT[@SCALE][+X+Y][/DESKTOPS]`.
    pub spec: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendWindow {
    pub app_id: String,
    pub output: String,
    pub desktop: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClickAt {
    pub label: String,
    pub at: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DragSettings {
    pub label: String,
    /// Logical pixels.
    pub by: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectShown {
    pub output: String,
    pub desktop: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectOutput {
    pub name: String,
    pub geometry: Option<[i32; 4]>,
    pub pixels: Option<[i32; 2]>,
    pub scale: Option<f64>,
    /// Night light's temperature on it, kelvin, between these.
    pub temperature: Option<[u32; 2]>,
    #[serde(default = "yes")]
    pub shown: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectBudget {
    /// The longest frame any output drew, milliseconds.
    pub max_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonName {
    Left,
    Right,
    Middle,
}

impl ButtonName {
    fn api(self) -> Button {
        match self {
            Self::Left => Button::Left,
            Self::Right => Button::Right,
            Self::Middle => Button::Middle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectWindow {
    pub app_id: String,
    /// Among windows of one application, the one with this title.
    pub title: Option<String>,
    pub output: Option<String>,
    /// `[x, y, width, height]`, logical.
    pub geometry: Option<[i32; 4]>,
    pub focused: Option<bool>,
    /// Whether its desktop is shown and it is not minimized.
    pub visible: Option<bool>,
    /// `normal`, `maximized`, `fullscreen`, `minimized` or `snapped`.
    pub state: Option<WindowState>,
    pub opacity: Option<f64>,
    pub pinned: Option<bool>,
    /// Whether it asks for attention.
    pub attention: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectReceived {
    pub app_id: String,
    pub event: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectScreenshot {
    pub output: String,
    /// A PNG next to the scenario file.
    pub reference: PathBuf,
    /// Largest difference on a channel for a pixel to count as equal.
    #[serde(default)]
    pub channel_tolerance: Option<u8>,
    /// Share of pixels, 0 to 1, that may differ.
    #[serde(default)]
    pub pixel_tolerance: Option<f64>,
    /// Draw the cursor too.
    #[serde(default)]
    pub cursor: bool,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(open) => write!(f, "open {}", open.app_id),
            Self::OpenX11(open) => write!(f, "open_x11 {}", open.app_id),
            Self::ClickUi(label) => write!(f, "click_ui {label:?}"),
            Self::Mcp(call) => write!(f, "mcp {}", call.tool),
            Self::McpSession(agent) => write!(f, "mcp_session {agent}"),
            Self::McpEnd(agent) => write!(f, "mcp_end {agent}"),
            Self::ExpectMcp(_) => write!(f, "expect_mcp"),
            Self::ExpectJournal(_) => write!(f, "expect_journal"),
            Self::OpenSettings(o) => write!(f, "open_settings {:?}", o.page),
            Self::Activate(app_id) => write!(f, "activate {app_id}"),
            Self::ActivateWithoutSerial(app_id) => write!(f, "activate_without_serial {app_id}"),
            Self::ActivateWith(a) => write!(f, "activate_with {} {}", a.app_id, a.token_from),
            Self::X11Urgent(u) => write!(f, "x11_urgent {} {}", u.app_id, u.urgent),
            Self::AskAttention(app_id) => write!(f, "ask_attention {app_id}"),
            Self::Share(_) => write!(f, "share"),
            Self::LockClient(_) => write!(f, "lock_client"),
            Self::SessionFile(_) => write!(f, "session_file"),
            Self::SaveSession(_) => write!(f, "save_session"),
            Self::OfferSession(_) => write!(f, "offer_session"),
            Self::ExpectSessionFile(_) => write!(f, "expect_session_file"),
            Self::UnlockClient(_) => write!(f, "unlock_client"),
            Self::CrashLockClient(_) => write!(f, "crash_lock_client"),
            Self::StopSharing(_) => write!(f, "stop_sharing"),
            Self::ExpectSharingEnded(_) => write!(f, "expect_sharing_ended"),
            Self::AskShare(_) => write!(f, "ask_share"),
            Self::ExpectShared(a) => write!(f, "expect_shared {a:?}"),
            Self::OpenLayer(spec) => write!(f, "open_layer {}", spec.namespace),
            Self::CloseLayer(namespace) => write!(f, "close_layer {namespace}"),
            Self::ExpectSettings(e) => write!(f, "expect_settings {:?}", e.label),
            Self::ClickSettings(label) => write!(f, "click_settings {label:?}"),
            Self::ClickSettingsAt(c) => write!(f, "click_settings_at {:?} {:?}", c.label, c.at),
            Self::OpenAnnotate(file) => write!(f, "open_annotate {file:?}"),
            Self::WaitWallpapers(_) => write!(f, "wait_wallpapers"),
            Self::Restart(_) => write!(f, "restart"),
            Self::ExpectRecording(e) => write!(f, "expect_recording {e:?}"),
            Self::ExpectIdle(e) => write!(f, "expect_idle {} ms", e.for_ms),
            Self::ExpectFrames(e) => write!(f, "expect_frames {} in {} ms", e.at_least, e.for_ms),
            Self::Swipe(s) => write!(f, "swipe {} by {:?}", s.fingers, s.by),
            Self::Caldav(c) => write!(f, "caldav {}", c.dir.display()),
            Self::CaldavStop(_) => write!(f, "caldav_stop"),
            Self::Herdr(h) => write!(f, "herdr with {} agents", h.agents.len()),
            Self::HerdrStatus(h) => write!(f, "herdr_status {} {}", h.agent, h.status),
            Self::ExpectHerdr(asked) => write!(f, "expect_herdr {asked:?}"),
            Self::CalendarPassword(p) => write!(f, "calendar_password {}", p.id),
            Self::AiServer(a) => write!(f, "ai_server with {} pieces", a.pieces.len()),
            Self::AiKey(k) => write!(f, "ai_key {}", k.provider),
            Self::ExpectAi(e) => write!(f, "expect_ai {e:?}"),
            Self::ExpectSearch(e) => write!(f, "expect_search {e:?}"),
            Self::Unreadable(path) => write!(f, "unreadable {path:?}"),
            Self::ExpectCalendars(e) => write!(f, "expect_calendars {e:?}"),
            Self::ExpectCalendarCache(e) => write!(f, "expect_calendar_cache {e:?}"),
            Self::Goa(change) => write!(f, "goa {change:?}"),
            Self::ExpectGoa(asked) => write!(f, "expect_goa {asked:?}"),
            Self::ExpectAnnotate(e) => write!(f, "expect_annotate {:?}", e.label),
            Self::ClickAnnotate(label) => write!(f, "click_annotate {label:?}"),
            Self::DragAnnotate(d) => write!(f, "drag_annotate {:?}", d.label),
            Self::OpenTrim(file) => write!(f, "open_trim {file:?}"),
            Self::ExpectTrim(e) => write!(f, "expect_trim {:?}", e.label),
            Self::ClickTrim(label) => write!(f, "click_trim {label:?}"),
            Self::DragTrim(d) => write!(f, "drag_trim {:?}", d.label),
            Self::ExpectExport(e) => write!(f, "expect_export {:?}", e.name_has),
            Self::DragSettings(d) => write!(f, "drag_settings {}", d.label),
            Self::ExpectConfigFile(_) => write!(f, "expect_config_file"),
            Self::Type(text) => write!(f, "type {text:?}"),
            Self::ExpectAccent(e) => write!(f, "expect_accent {:?}", e.hue),
            Self::ExpectRuntimeFile(e) => write!(f, "expect_runtime_file {}", e.name),
            Self::ExpectStateFile(e) => write!(f, "expect_state_file {}", e.name),
            Self::NotifySend(args) => write!(f, "notify_send {args:?}"),
            Self::Notify(n) => write!(f, "notify {} {:?}", n.app, n.summary),
            Self::ExpectNotificationSignals(s) => write!(f, "expect_notification_signals {s:?}"),
            Self::RightClickUi(label) => write!(f, "right_click_ui {label:?}"),
            Self::TrayApp(t) => write!(f, "tray_app {:?}", t.title),
            Self::TrayAttention(t) => write!(f, "tray_attention {:?} {}", t.title, t.attention),
            Self::ExpectTray(t) => write!(f, "expect_tray {:?} {:?}", t.title, t.asked),
            Self::Phone(p) => write!(f, "phone {:?} {}", p.name, p.battery),
            Self::PhoneNotification(n) => write!(f, "phone_notification {:?}", n.title),
            Self::PhoneRemove(n) => write!(f, "phone_remove {n}"),
            Self::ExpectPhone(asked) => write!(f, "expect_phone {asked:?}"),
            Self::ExpectPhoneState(state) => write!(f, "expect_phone_state {state:?}"),
            Self::ScanPairingCode(_) => write!(f, "scan_pairing_code"),
            Self::ExpectAdb(state) => write!(f, "expect_adb {state:?}"),
            Self::ExpectTimedated(asked) => write!(f, "expect_timedated {asked:?}"),
            Self::TimedatedZone(zone) => write!(f, "timedated_zone {zone}"),
            Self::TimedatedRefuses(r) => write!(f, "timedated_refuses {r}"),
            Self::Charger(plugged) => write!(f, "charger {plugged}"),
            Self::ExpectAccounts(asked) => write!(f, "expect_accounts {asked:?}"),
            Self::ExpectXdgFile(e) => write!(f, "expect_xdg_file {}", e.name),
            Self::ExpectDevices(asked) => write!(f, "expect_devices {asked:?}"),
            Self::ExpectUi(e) => write!(f, "expect_ui {:?}", e.label),
            Self::Copy(c) => write!(f, "copy {} {:?}", c.app_id, c.text),
            Self::ExpectClipboard(c) => write!(f, "expect_clipboard {} {:?}", c.app_id, c.text),
            Self::Close(app_id) => write!(f, "close {app_id}"),
            Self::PointerMove([x, y]) => write!(f, "pointer_move {x},{y}"),
            Self::PointerPath(points) => write!(f, "pointer_path of {} points", points.len()),
            Self::Click(button) => write!(f, "click {button:?}"),
            Self::Press(button) => write!(f, "press {button:?}"),
            Self::Scroll(dy) => write!(f, "scroll {dy}"),
            Self::Release(button) => write!(f, "release {button:?}"),
            Self::KeyDown(key) => write!(f, "key_down {key}"),
            Self::KeyUp(key) => write!(f, "key_up {key}"),
            Self::ExpectNotReceived(e) => {
                write!(f, "expect_not_received {} `{}`", e.app_id, e.event)
            }
            Self::Key(code) => write!(f, "key {code}"),
            Self::Keys(keys) => write!(f, "keys {keys}"),
            Self::Plug(plug) => write!(f, "plug {}", plug.spec),
            Self::Unplug(name) => write!(f, "unplug {name}"),
            Self::StallOutput(name) => write!(f, "stall output {name}"),
            Self::Config(_) => write!(f, "config"),
            Self::ExpectConfigError(e) => write!(f, "expect_config_error {e:?}"),
            Self::ExpectConfigWarning(e) => write!(f, "expect_config_warning {e:?}"),
            Self::ExpectEvent(e) => write!(f, "expect_event {}", e.kind),
            Self::SetState(s) => write!(f, "set_state {} {:?}", s.app_id, s.state),
            Self::Send(send) => write!(f, "send {} to {}", send.app_id, send.output),
            Self::ExpectShown(e) => write!(f, "expect_shown {} {}", e.output, e.desktop),
            Self::Randr(c) => write!(f, "randr {}", c.output),
            Self::ExpectOutput(e) => write!(f, "expect_output {}", e.name),
            Self::AdvanceMs(ms) => write!(f, "advance {ms} ms"),
            Self::Frames(n) => write!(f, "frames {n}"),
            Self::ExpectWindow(e) => write!(f, "expect_window {}", e.app_id),
            Self::ExpectNoWindow(app_id) => write!(f, "expect_no_window {app_id}"),
            Self::ExpectFocus(app_id) => write!(f, "expect_focus {app_id}"),
            Self::ExpectStack(_) => write!(f, "expect_stack"),
            Self::ExpectReceived(e) => write!(f, "expect_received {} `{}`", e.app_id, e.event),
            Self::ExpectBudget(e) => write!(f, "expect_budget {} ms", e.max_ms),
            Self::ExpectBlur(e) => write!(f, "expect_blur {:?} to {:?}", e.at_least, e.at_most),
            Self::ExpectGlitch(e) => {
                write!(f, "expect_glitch {:?} to {:?}", e.at_least, e.at_most)
            }
            Self::ExpectPixel(e) => write!(
                f,
                "expect_pixel {} {:?} {} {}",
                e.output,
                e.at,
                if e.is { "is" } else { "is not" },
                e.color
            ),
            Self::ExpectScreenshot(e) => {
                write!(
                    f,
                    "expect_screenshot {} {}",
                    e.output,
                    e.reference.display()
                )
            }
        }
    }
}

/// Something done `done` times, as `expected` says.
fn counted(what: &str, done: u64, expected: &ExpectBlur) -> anyhow::Result<()> {
    if let Some(least) = expected.at_least {
        anyhow::ensure!(
            done >= least,
            "{what} {done} times, at least {least} expected"
        );
    }
    if let Some(most) = expected.at_most {
        anyhow::ensure!(done <= most, "{what} {done} times, at most {most} expected");
    }
    Ok(())
}

/// Where a run writes images, and whether it records new references.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Actual, expected and diff images of failed comparisons go to
    /// `<artifacts>/<scenario>/`.
    pub artifacts: PathBuf,
    /// Write the actual screenshots as references instead of comparing.
    pub bless: bool,
    /// The test client program, `{test_client}` in configuration steps.
    pub test_client: Option<PathBuf>,
}

impl Scenario {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let mut scenario = Self::parse(&text).with_context(|| format!("in {}", path.display()))?;
        scenario.name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        scenario.dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(scenario)
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(text)?)
    }

    /// Runs every step on a fresh compositor; the first step that fails ends
    /// the run, and the error names it.
    pub fn run(&self, options: &RunOptions) -> anyhow::Result<()> {
        let outputs = if self.outputs.is_empty() {
            vec![OutputSpec::new(1280, 800)]
        } else {
            self.outputs
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?
        };
        let bus = if self.dbus || self.goa.is_some() {
            Some(crate::bus::PrivateBus::start()?)
        } else {
            None
        };
        let goa = match (&bus, &self.goa) {
            (Some(bus), Some(goa)) => Some(crate::goa::FakeGoa::start(
                &bus.address,
                &goa.identity,
                self.dir.join(&goa.dir),
                goa.running,
            )?),
            _ => None,
        };
        // The system bus, with the stand-ins asked for alone on it.
        let system_bus = if self.timedated.is_some() || self.devices || self.accounts {
            Some(crate::bus::PrivateBus::start()?)
        } else {
            None
        };
        let timedated = match (&system_bus, &self.timedated) {
            (Some(bus), Some(zone)) => {
                Some(crate::timedated::FakeTimedated::start(&bus.address, zone)?)
            }
            _ => None,
        };
        let devices = match &system_bus {
            Some(bus) if self.devices => Some(crate::devices::FakeDevices::start(&bus.address)?),
            _ => None,
        };
        let accounts = match &system_bus {
            Some(bus) if self.accounts => Some(crate::accounts::FakeAccounts::start(&bus.address)?),
            _ => None,
        };
        let home = match &self.home {
            Some(fixture) => Some(PrivateHome::copy(&self.dir.join(fixture))?),
            None => None,
        };
        let compositor = Headless::start_options(HarnessOptions {
            headless: HeadlessConfig {
                outputs,
                // The same pixels on every machine, for reference
                // screenshots, unless shaders are what is tested.
                renderer: match self.renderer {
                    ScenarioRenderer::Pixman => RendererChoice::Pixman,
                    ScenarioRenderer::Gles => RendererChoice::Gles,
                },
            },
            api_permission: hocus_api::Permission::Test,
            watchdog: None,
            xwayland: self.xwayland,
            shell: self.shell,
            session_bus: bus.as_ref().map(|b| b.address.clone()),
            system_bus: system_bus.as_ref().map(|b| b.address.clone()),
            data_dirs: (!self.data_dirs.is_empty())
                .then(|| self.data_dirs.iter().map(|d| self.dir.join(d)).collect()),
            child_env: self.child_env.as_ref().map(|env| {
                // Stand-ins for the user's programs live beside the scenario.
                let dir = self.dir.canonicalize().unwrap_or_else(|_| self.dir.clone());
                let dir = dir.to_string_lossy();
                let mut env: std::collections::BTreeMap<String, String> = env
                    .iter()
                    .map(|(k, v)| (k.clone(), v.replace("{scenario_dir}", &dir)))
                    .collect();
                env.entry("PATH".into())
                    .or_insert_with(|| std::env::var("PATH").unwrap_or_default());
                env.into_iter().map(|(k, v)| (k.into(), v.into())).collect()
            }),
            polkit_helper: None,
            motion: self.motion,
            first_run: self.first_run,
            google_caldav: goa.as_ref().map(|g| g.google().to_owned()),
        })?;
        let mut run = Run {
            scenario: self,
            options,
            compositor,
            clients: Vec::new(),
            x11_clients: Vec::new(),
            notification_signals: None,
            settings: None,
            editor: None,
            driving: Driving::Settings,
            layers: Vec::new(),
            sharing: None,
            lock_client: None,
            bus,
            timedated,
            devices,
            accounts,
            system_bus,
            tray_apps: Vec::new(),
            phone: None,
            phone_posted: Vec::new(),
            goa,
            asking: None,
            events: Vec::new(),
            caldav: None,
            herdrs: Vec::new(),
            ai: None,
            blurs: 0,
            mcp_sessions: Vec::new(),
            mcp_waiting: Vec::new(),
            glitches: 0,
            a11y_apps: Vec::new(),
            a11y: None,
            home,
        };
        run.request(Request::Subscribe)?;
        for (n, step) in self.steps.iter().enumerate() {
            run.step(step)
                .with_context(|| format!("step {} ({step})", n + 1))?;
        }
        Ok(())
    }
}

struct Run<'a> {
    scenario: &'a Scenario,
    options: &'a RunOptions,
    compositor: Headless,
    clients: Vec<TestClient>,
    x11_clients: Vec<X11Client>,
    /// The notification server's signals, heard from the first `notify`.
    notification_signals: Option<crate::bus::Signals>,
    settings: Option<hocus_settings::client::Running>,
    /// The annotation window or the recording editor.
    editor: Option<hocus_annotate::client::Running>,
    /// Which of the two the settings steps drive.
    driving: Driving,
    layers: Vec<crate::layer::LayerClient>,
    lock_client: Option<crate::session_lock::LockClient>,
    sharing: Option<(
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::thread::JoinHandle<anyhow::Result<()>>,
    )>,
    /// A chooser waiting for its answer.
    asking: Option<std::thread::JoinHandle<anyhow::Result<Option<String>>>>,
    /// Tray applications by title; before the bus, so they go first.
    tray_apps: Vec<(String, crate::tray::TrayApp)>,
    /// KDE Connect's stand-in, and the notifications it posted.
    phone: Option<crate::phone::FakePhone>,
    phone_posted: Vec<String>,
    /// GNOME Online Accounts' stand-in, and Google's.
    goa: Option<crate::goa::FakeGoa>,
    bus: Option<crate::bus::PrivateBus>,
    /// timedated's stand-in; before its bus, so that it goes first.
    timedated: Option<crate::timedated::FakeTimedated>,
    /// The devices' stand-ins, the same.
    devices: Option<crate::devices::FakeDevices>,
    /// The account's stand-ins, the same.
    accounts: Option<crate::accounts::FakeAccounts>,
    system_bus: Option<crate::bus::PrivateBus>,
    /// Every event since the start.
    events: Vec<hocus_api::Event>,
    caldav: Option<crate::caldav::Server>,
    /// The herdr of the test, by name (`None` for `{herdr}`).
    herdrs: Vec<(Option<String>, crate::herdr::Server)>,
    ai: Option<crate::ai::Server>,
    /// Blurs computed at the last `expect_blur`.
    blurs: u64,
    /// MCP sessions that last, by agent (#43).
    mcp_sessions: Vec<McpSession>,
    /// Calls left with `wait = false`, oldest first.
    mcp_waiting: Vec<McpWaiting>,
    /// Glitch frames painted at the last `expect_glitch`.
    glitches: u64,
    /// Applications exposing accessibility trees (#175), on their private
    /// bus with its registry, made at the first.
    a11y_apps: Vec<crate::atspi::FakeApp>,
    a11y: Option<(crate::bus::PrivateBus, crate::atspi::FakeRegistry)>,
    /// The scenario's own home, last: the compositor is gone before it.
    home: Option<PrivateHome>,
}

/// A copy of a fixture folder in a fresh temporary directory: the home of
/// a scenario. What a step made unreadable is made readable again before
/// it goes.
struct PrivateHome {
    dir: tempfile::TempDir,
    locked: Vec<PathBuf>,
}

impl PrivateHome {
    fn copy(fixture: &Path) -> anyhow::Result<Self> {
        fn copy_into(from: &Path, to: &Path) -> std::io::Result<()> {
            std::fs::create_dir_all(to)?;
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                let target = to.join(entry.file_name());
                if entry.file_type()?.is_dir() {
                    copy_into(&entry.path(), &target)?;
                } else {
                    std::fs::copy(entry.path(), &target)?;
                }
            }
            Ok(())
        }
        let dir = tempfile::Builder::new().prefix("hocus-home-").tempdir()?;
        copy_into(fixture, dir.path())
            .with_context(|| format!("could not copy {}", fixture.display()))?;
        Ok(Self {
            dir,
            locked: Vec::new(),
        })
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for PrivateHome {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        // The innermost last locked, unlocked first.
        for path in self.locked.iter().rev() {
            let mode = if path.is_dir() { 0o700 } else { 0o600 };
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
        }
    }
}

impl Run<'_> {
    /// Frames drawn so far, per output.
    fn frames(&mut self) -> anyhow::Result<Vec<(String, u64)>> {
        let Reply::Stats(stats) = self.request(Request::Stats)? else {
            anyhow::bail!("expected stats");
        };
        Ok(stats
            .outputs
            .into_iter()
            .map(|o| (o.name, o.frames))
            .collect())
    }

    fn step(&mut self, step: &Step) -> anyhow::Result<()> {
        match step {
            Step::Open(open) => self.open(open),
            Step::OpenX11(open) => self.open_x11(open),
            Step::Copy(copy) => self.copy(copy),
            Step::ExpectClipboard(expected) => self.expect_clipboard(expected),
            Step::ClickUi(label) => self.click_ui(label, Button::Left),
            Step::Mcp(call) => self.mcp(call),
            Step::McpSession(agent) => self.mcp_session(agent),
            Step::McpEnd(agent) => {
                self.mcp_sessions.retain(|s| s.agent != *agent);
                Ok(())
            }
            Step::ExpectMcp(expected) => self.expect_mcp(expected),
            Step::ExpectJournal(expected) => self.expect_journal(expected),
            Step::RightClickUi(label) => self.click_ui(label, Button::Right),
            Step::TrayApp(t) => {
                let [_, r, g, b] = parse_color(&t.color)?.to_be_bytes();
                let app = crate::tray::TrayApp::start(
                    self.bus()?,
                    &t.title,
                    [r, g, b],
                    &t.menu,
                    Duration::from_millis(t.delay_ms),
                )?;
                self.tray_apps.push((t.title.clone(), app));
                Ok(())
            }
            Step::TrayAttention(t) => self.tray_app(&t.title)?.set_attention(t.attention),
            Step::Phone(p) => match &self.phone {
                Some(phone) => phone.set_battery(p.battery, p.charging),
                None => {
                    let mount = self.compositor.runtime_dir().join("phone-files");
                    std::fs::create_dir_all(&mount)?;
                    let phone = crate::phone::FakePhone::start(
                        self.bus()?,
                        &p.name,
                        p.battery,
                        p.charging,
                        mount,
                    )?;
                    self.phone = Some(phone);
                    Ok(())
                }
            },
            Step::PhoneNotification(n) => {
                let phone = self
                    .phone
                    .as_mut()
                    .context("no phone: a `phone` step first")?;
                let id = phone.post(&n.app, &n.title, &n.text, n.reply)?;
                self.phone_posted.push(id);
                Ok(())
            }
            Step::PhoneRemove(n) => {
                let id = n
                    .checked_sub(1)
                    .and_then(|i| self.phone_posted.get(i))
                    .with_context(|| format!("the phone posted no notification {n}"))?;
                self.phone
                    .as_ref()
                    .context("no phone: a `phone` step first")?
                    .remove(id)
            }
            Step::ExpectTimedated(expected) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    let asked = self
                        .timedated
                        .as_ref()
                        .context("no timedated: the scenario needs `timedated = \"ZONE\"`")?
                        .asked();
                    if asked.contains(expected) {
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!("timedated was asked {asked:?}, not {expected:?}");
                    }
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::TimedatedZone(zone) => self
                .timedated
                .as_ref()
                .context("no timedated: the scenario needs `timedated = \"ZONE\"`")?
                .set_zone(zone),
            Step::TimedatedRefuses(refuse) => {
                self.timedated
                    .as_ref()
                    .context("no timedated: the scenario needs `timedated = \"ZONE\"`")?
                    .refuse(*refuse);
                Ok(())
            }
            Step::Charger(plugged) => self
                .devices
                .as_ref()
                .context("no devices: the scenario needs `devices = true`")?
                .set_charger(*plugged),
            Step::ExpectAccounts(expected) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    let asked = self
                        .accounts
                        .as_ref()
                        .context("no accounts: the scenario needs `accounts = true`")?
                        .asked();
                    if asked.contains(expected) {
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!("the account was asked {asked:?}, not {expected:?}");
                    }
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ExpectXdgFile(expected) => {
                let path = self.xdg_home().join(&expected.name);
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                loop {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let ok = !text.is_empty()
                        && expected.contains.iter().all(|c| text.contains(c.as_str()))
                        && !expected.lacks.iter().any(|l| text.contains(l.as_str()));
                    if ok {
                        return Ok(());
                    }
                    anyhow::ensure!(Instant::now() < deadline, "{} is:\n{text}", expected.name);
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ExpectDevices(expected) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    let asked = self
                        .devices
                        .as_ref()
                        .context("no devices: the scenario needs `devices = true`")?
                        .asked();
                    if asked.contains(expected) {
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!("the devices were asked {asked:?}, not {expected:?}");
                    }
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ExpectPhone(expected) => {
                let runtime = self.compositor.runtime_dir().to_string_lossy().into_owned();
                let expected = expected.replace("{runtime_dir}", &runtime);
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    let asked = self
                        .phone
                        .as_ref()
                        .context("no phone: a `phone` step first")?
                        .asked();
                    if asked.contains(&expected) {
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!("the phone was asked {asked:?}, not {expected:?}");
                    }
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ScanPairingCode(_) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                let qr = loop {
                    if let Some(qr) = self.compositor.pairing_code() {
                        break qr;
                    }
                    anyhow::ensure!(Instant::now() < deadline, "no pairing code shown");
                    self.compositor.dispatch(Duration::from_millis(10))?;
                };
                let code = hocus_platform::adb::Code::parse(&qr)
                    .with_context(|| format!("not a pairing code: {qr:?}"))?;
                let dir = self
                    .compositor
                    .api_socket()
                    .parent()
                    .context("no runtime directory")?
                    .to_path_buf();
                // Whole: the stand-in reads it as soon as it is there.
                hocus_compositor::write_whole(
                    &dir.join("adb-phone"),
                    format!("{} {}\n", code.name, code.password).as_bytes(),
                )?;
                Ok(())
            }
            Step::ExpectAdb(expected) => {
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                loop {
                    let Reply::Phone(info) = self.request(Request::Phone)? else {
                        anyhow::bail!("expected the phone");
                    };
                    if info.adb == *expected {
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!("adb is {:?}, not {expected:?}", info.adb);
                    }
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ExpectPhoneState(expected) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    let Reply::Phone(info) = self.request(Request::Phone)? else {
                        anyhow::bail!("expected the phone");
                    };
                    if info.state == *expected {
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!("the phone is {:?}, not {expected:?}", info.state);
                    }
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ExpectTray(t) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    let asked = self.tray_app(&t.title)?.asked();
                    if asked.contains(&t.asked) {
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!("{:?} was asked {asked:?}, not {:?}", t.title, t.asked);
                    }
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::OpenSettings(open) => self.open_settings(open),
            Step::SessionFile(text) => {
                let mut text = text.clone();
                if let Some(client) = &self.options.test_client {
                    text = text.replace("{test_client}", &client.to_string_lossy());
                }
                let dir = self.compositor.state_dir();
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join("session.toml"), text)?;
                Ok(())
            }
            Step::SaveSession(_) => {
                self.request(Request::SaveSession)?;
                Ok(())
            }
            Step::OfferSession(_) => {
                self.request(Request::OfferSession)?;
                self.compositor.advance(FRAME)
            }
            Step::ExpectSessionFile(expected) => {
                let path = self.compositor.state_dir().join("session.toml");
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for wanted in &expected.contains {
                    anyhow::ensure!(
                        text.contains(wanted.as_str()),
                        "the session file lacks {wanted:?}:\n{text}"
                    );
                }
                for unwanted in &expected.lacks {
                    anyhow::ensure!(
                        !text.contains(unwanted.as_str()),
                        "the session file has {unwanted:?}:\n{text}"
                    );
                }
                Ok(())
            }
            Step::LockClient(color) => {
                let client = crate::session_lock::LockClient::spawn(
                    self.compositor.socket_path(),
                    parse_color(color)?,
                )?;
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !client.is_locked() {
                    anyhow::ensure!(Instant::now() < deadline, "the session never locked");
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                self.lock_client = Some(client);
                // Its surfaces get configured and drawn.
                let until = Instant::now() + Duration::from_millis(100);
                while Instant::now() < until {
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                self.compositor.advance(FRAME)
            }
            Step::UnlockClient(_) | Step::CrashLockClient(_) => {
                let client = self.lock_client.take().context("no lock client")?;
                if matches!(step, Step::UnlockClient(_)) {
                    client.unlock();
                } else {
                    client.vanish();
                }
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !client.is_finished() {
                    anyhow::ensure!(Instant::now() < deadline, "the lock client never left");
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                let until = Instant::now() + Duration::from_millis(50);
                while Instant::now() < until {
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                self.compositor.advance(FRAME)
            }
            Step::Share(_) => {
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let socket = self.compositor.socket_path().to_path_buf();
                let thread = std::thread::spawn({
                    let stop = stop.clone();
                    move || crate::screencopy::share(&socket, stop)
                });
                self.sharing = Some((stop, thread));
                // Its first copy is asked for, at this moment of the
                // virtual clock however long its thread takes (#212).
                self.compositor
                    .dispatch_until(CLIENT_TIMEOUT, Headless::sharing)
                    .context("the sharing tool never asked for a copy")
            }
            Step::AskShare(_) => {
                let socket = self.compositor.api_socket().to_path_buf();
                self.asking = Some(std::thread::spawn(move || {
                    let mut client = hocus_api::Client::connect(&socket)?;
                    client.set_timeout(None)?;
                    match client.request(Request::PickShare)? {
                        Reply::Shared(answer) => Ok(answer),
                        other => anyhow::bail!("unexpected {other:?}"),
                    }
                }));
                Ok(())
            }
            Step::ExpectShared(expected) => {
                let asking = self.asking.take().context("nothing was asked")?;
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !asking.is_finished() {
                    anyhow::ensure!(Instant::now() < deadline, "the chooser was never answered");
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                let answer = asking
                    .join()
                    .map_err(|_| anyhow::anyhow!("the chooser panicked"))??
                    .unwrap_or_default();
                anyhow::ensure!(answer == *expected, "answered {answer:?}");
                Ok(())
            }
            Step::ExpectSharingEnded(_) => {
                let (_, thread) = self.sharing.take().context("not sharing")?;
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !thread.is_finished() {
                    anyhow::ensure!(Instant::now() < deadline, "the sharing never ended");
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("the sharing tool panicked"))??;
                Ok(())
            }
            Step::StopSharing(_) => {
                let (stop, thread) = self.sharing.take().context("not sharing")?;
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !thread.is_finished() {
                    anyhow::ensure!(Instant::now() < deadline, "the sharing tool never stopped");
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("the sharing tool panicked"))??;
                Ok(())
            }
            Step::OpenLayer(spec) => {
                let argb = parse_color(&spec.color)?;
                let client = spec.clone().spawn(self.compositor.socket_path(), argb)?;
                self.layers.push(client);
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !self.layers.last().is_some_and(|l| l.is_drawn()) {
                    anyhow::ensure!(Instant::now() < deadline, "the layer surface never showed");
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                // Its buffer arrives after its configure.
                self.compositor.advance(FRAME)
            }
            Step::CloseLayer(namespace) => {
                let client = self
                    .layers
                    .iter()
                    .find(|l| l.spec.namespace == *namespace)
                    .with_context(|| format!("no layer {namespace}"))?;
                client.close();
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !self
                    .layers
                    .iter()
                    .all(|l| l.spec.namespace != *namespace || l.is_finished())
                {
                    anyhow::ensure!(Instant::now() < deadline, "the layer client never left");
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                self.layers.retain(|l| l.spec.namespace != *namespace);
                self.compositor.advance(FRAME)
            }
            Step::Activate(app_id) => self
                .clients
                .iter()
                .find(|c| c.spec.app_id == *app_id)
                .with_context(|| format!("no client {app_id}"))?
                .activate(),
            Step::ActivateWithoutSerial(app_id) => self
                .clients
                .iter()
                .find(|c| c.spec.app_id == *app_id)
                .with_context(|| format!("no client {app_id}"))?
                .activate_without_serial(),
            Step::ActivateWith(a) => self.activate_with(a),
            Step::X11Urgent(u) => self
                .x11_clients
                .iter()
                .find(|c| c.spec.app_id == u.app_id)
                .with_context(|| format!("no X11 client {}", u.app_id))?
                .urgent(u.urgent),
            Step::AskAttention(app_id) => self.ask_attention(app_id),
            Step::ExpectSettings(expected) => self.expect_settings(expected),
            Step::ClickSettings(label) => self.click_settings(label),
            Step::ClickSettingsAt(c) => self.click_settings_at(&c.label, c.at),
            Step::DragSettings(drag) => self.drag_settings(drag),
            Step::OpenAnnotate(file) => {
                self.open_editor(file, hocus_annotate::client::Editing::Annotate)
            }
            Step::OpenTrim(file) => self.open_editor(file, hocus_annotate::client::Editing::Trim),
            Step::ExpectTrim(expected) => {
                self.driving = TRIM;
                let done = self.expect_settings(expected);
                self.driving = Driving::Settings;
                done
            }
            Step::ClickTrim(label) => {
                self.driving = TRIM;
                let done = self.click_settings(label);
                self.driving = Driving::Settings;
                done
            }
            Step::DragTrim(drag) => {
                self.driving = TRIM;
                let done = self.drag_settings(drag);
                self.driving = Driving::Settings;
                done
            }
            Step::ExpectExport(expected) => self.expect_export(expected),
            Step::WaitWallpapers(_) => self.wallpapers(),
            Step::Caldav(c) => {
                let dir = self.scenario.dir.join(&c.dir);
                self.caldav = Some(crate::caldav::Server::start(dir, &c.user, &c.password)?);
                Ok(())
            }
            Step::CaldavStop(_) => {
                self.caldav = None;
                Ok(())
            }
            Step::Herdr(h) => {
                let agents: Vec<_> = h
                    .agents
                    .iter()
                    .map(|a| (a.name.clone(), a.status.clone()))
                    .collect();
                let dir = self.compositor.runtime_dir().to_path_buf();
                let file = match &h.name {
                    Some(name) => format!("herdr-{name}.sock"),
                    None => "herdr.sock".to_owned(),
                };
                self.herdrs.retain(|(n, _)| *n != h.name);
                let server = crate::herdr::Server::start(&dir.join(file), &agents)?;
                self.herdrs.push((h.name.clone(), server));
                Ok(())
            }
            Step::HerdrStatus(h) => {
                let herdr = self
                    .herdrs
                    .iter()
                    .map(|(_, s)| s)
                    .find(|s| s.has(&h.agent))
                    .with_context(|| format!("no herdr step with {:?} before", h.agent))?;
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !herdr.followed(&h.agent) {
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "the compositor never followed {:?}",
                        h.agent
                    );
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
                herdr.set_status(&h.agent, &h.status, h.label.as_deref())?;
                // It reaches the loop before the next step, even on a
                // loaded machine.
                let until = Instant::now() + Duration::from_millis(200);
                while Instant::now() < until {
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                Ok(())
            }
            Step::ExpectHerdr(wanted) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    anyhow::ensure!(!self.herdrs.is_empty(), "no herdr step before");
                    let asked: Vec<String> =
                        self.herdrs.iter().flat_map(|(_, s)| s.asked()).collect();
                    if asked.contains(wanted) {
                        return Ok(());
                    }
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "herdr was asked {asked:?}, not {wanted:?}"
                    );
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::CalendarPassword(p) => {
                let id = match &self.caldav {
                    Some(server) => p.id.replace("{caldav}", server.url()),
                    None => p.id.clone(),
                };
                self.request(Request::CalendarSecret {
                    account: id,
                    secret: p.password.clone(),
                })?;
                Ok(())
            }
            Step::ExpectCalendars(expected) => self.expect_calendars(expected),
            Step::ExpectCalendarCache(expected) => self.expect_calendar_cache(expected),
            Step::Goa(change) => {
                let goa = self
                    .goa
                    .as_ref()
                    .context("no GOA: the scenario needs `goa = { identity, dir }`")?;
                match change {
                    GoaChange::Run => goa.run(),
                    GoaChange::SignIn => goa.sign_in(),
                    GoaChange::Remove => goa.remove(),
                    GoaChange::CalendarOff => goa.set_calendar(false),
                    GoaChange::CalendarOn => goa.set_calendar(true),
                    GoaChange::Revoke => {
                        goa.revoke();
                        Ok(())
                    }
                    GoaChange::BreakTokens => {
                        goa.break_tokens();
                        Ok(())
                    }
                }
            }
            Step::ExpectGoa(expected) => self.expect_goa(expected),
            Step::AiServer(a) => {
                self.ai = Some(crate::ai::Server::start(crate::ai::Answers {
                    pieces: a.pieces.clone(),
                    delay: Duration::from_millis(a.delay_ms),
                    first: Duration::from_millis(a.first_ms),
                })?);
                Ok(())
            }
            Step::AiKey(k) => {
                self.request(Request::AiSecret {
                    provider: k.provider.clone(),
                    secret: k.key.clone(),
                })?;
                // Kept by a worker before the next step.
                let until = Instant::now() + Duration::from_millis(200);
                while Instant::now() < until {
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                Ok(())
            }
            Step::ExpectAi(expected) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    let asked = self
                        .ai
                        .as_ref()
                        .context("no ai_server step before")?
                        .asked();
                    let found = asked.iter().any(|a| {
                        expected.path.as_ref().is_none_or(|p| *p == a.path)
                            && expected.contains.iter().all(|c| a.body.contains(c))
                            && expected
                                .header
                                .as_ref()
                                .is_none_or(|h| a.headers.contains(h))
                    });
                    if found {
                        return Ok(());
                    }
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "the AI provider was asked {asked:?}"
                    );
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ExpectSearch(expected) => self.expect_search(expected),
            Step::Unreadable(path) => {
                use std::os::unix::fs::PermissionsExt;
                let home = self.home.as_mut().context("no home for the scenario")?;
                let path = PathBuf::from(path.replace("{home}", &home.path().to_string_lossy()));
                anyhow::ensure!(path.starts_with(home.path()), "only in the scenario's home");
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
                    .with_context(|| format!("could not lock {}", path.display()))?;
                home.locked.push(path);
                Ok(())
            }
            Step::ExpectRecording(expected) => self.expect_recording(expected),
            Step::Swipe(swipe) => {
                use hocus_api::SwipePhase;
                let send = |run: &mut Self, phase, dx, dy| {
                    run.request(Request::Swipe {
                        phase,
                        fingers: swipe.fingers,
                        dx,
                        dy,
                    })
                    .map(drop)
                };
                if swipe.begin {
                    send(self, SwipePhase::Begin, 0.0, 0.0)?;
                }
                for _ in 0..8 {
                    send(
                        self,
                        SwipePhase::Update,
                        swipe.by[0] / 8.0,
                        swipe.by[1] / 8.0,
                    )?;
                    self.compositor.advance(Duration::from_millis(16))?;
                }
                if swipe.release {
                    send(self, SwipePhase::End, 0.0, 0.0)?;
                }
                Ok(())
            }
            Step::ExpectFrames(expected) => {
                let before = self.frames()?;
                self.compositor
                    .advance(Duration::from_millis(expected.for_ms))?;
                let after = self.frames()?;
                for ((name, b), (_, a)) in before.iter().zip(&after) {
                    anyhow::ensure!(
                        a - b >= expected.at_least,
                        "{name} drew {} frames, expected at least {}",
                        a - b,
                        expected.at_least
                    );
                }
                Ok(())
            }
            Step::ExpectIdle(expected) => {
                let frames = |run: &mut Self| run.frames();
                let before = frames(self)?;
                self.compositor
                    .advance(Duration::from_millis(expected.for_ms))?;
                let after = frames(self)?;
                anyhow::ensure!(
                    before == after,
                    "frames were drawn at rest: {before:?} then {after:?}"
                );
                Ok(())
            }
            Step::Restart(_) => {
                self.clients.clear();
                self.x11_clients.clear();
                self.settings = None;
                self.editor = None;
                self.layers.clear();
                self.lock_client = None;
                self.compositor.restart()?;
                self.request(Request::Subscribe)?;
                Ok(())
            }
            Step::ExpectAnnotate(expected) => {
                self.driving = ANNOTATE;
                let done = self.expect_settings(expected);
                self.driving = Driving::Settings;
                done
            }
            Step::ClickAnnotate(label) => {
                self.driving = ANNOTATE;
                let done = self.click_settings(label);
                self.driving = Driving::Settings;
                done
            }
            Step::DragAnnotate(drag) => {
                self.driving = ANNOTATE;
                let done = self.drag_settings(drag);
                self.driving = Driving::Settings;
                done
            }
            Step::ExpectConfigFile(expected) => self.expect_config_file(expected),
            Step::Type(text) => {
                let mut text = match &self.caldav {
                    Some(server) => text.replace("{caldav}", server.url()),
                    None => text.clone(),
                };
                if let Some(ai) = &self.ai {
                    text = text.replace("{ai}", ai.url());
                }
                if let Some(home) = &self.home {
                    text = text.replace("{home}", &home.path().to_string_lossy());
                }
                self.type_text(&text)
            }
            Step::ExpectRuntimeFile(expected) => self.expect_runtime_file(expected),
            Step::ExpectStateFile(expected) => self.expect_state_file(expected),
            Step::NotifySend(args) => self.notify_send(args),
            Step::Notify(n) => self.notify(n),
            Step::ExpectNotificationSignals(expected) => {
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                loop {
                    let got: Vec<String> = self
                        .notification_signals
                        .as_ref()
                        .context("no notification sent yet")?
                        .lock()
                        .map(|s| s.clone())
                        .unwrap_or_default();
                    let mut rest = got.iter();
                    if expected.iter().all(|e| rest.any(|g| g == e)) {
                        return Ok(());
                    }
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "notification signals: expected {expected:?} in order, got {got:?}"
                    );
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ExpectUi(expected) => self.expect_ui(expected),
            Step::Close(app_id) => self.close(app_id),
            Step::PointerMove([x, y]) => {
                self.request(Request::PointerMove { x: *x, y: *y })?;
                Ok(())
            }
            Step::PointerPath(points) => {
                for [x, y] in points {
                    self.request(Request::PointerMove { x: *x, y: *y })?;
                }
                Ok(())
            }
            Step::Click(button) => {
                for pressed in [true, false] {
                    self.request(Request::PointerButton {
                        button: button.api(),
                        pressed,
                    })?;
                }
                Ok(())
            }
            Step::Press(button) | Step::Release(button) => {
                self.request(Request::PointerButton {
                    button: button.api(),
                    pressed: matches!(step, Step::Press(_)),
                })?;
                Ok(())
            }
            Step::Scroll(dy) => {
                self.request(Request::PointerScroll { dy: *dy })?;
                Ok(())
            }
            Step::KeyDown(key) | Step::KeyUp(key) => {
                for code in parse_keys(key)? {
                    self.request(Request::Key {
                        code,
                        pressed: matches!(step, Step::KeyDown(_)),
                    })?;
                }
                Ok(())
            }
            Step::ExpectNotReceived(expected) => {
                let events = self.events_of(&expected.app_id)?;
                // What was sent has had time to arrive: earlier steps waited.
                let got: Vec<String> = events.iter().map(ToString::to_string).collect();
                anyhow::ensure!(
                    !got.contains(&expected.event),
                    "{} received `{}`; it received: {}",
                    expected.app_id,
                    expected.event,
                    got.join(", ")
                );
                Ok(())
            }
            Step::Key(code) => {
                for pressed in [true, false] {
                    self.request(Request::Key {
                        code: *code,
                        pressed,
                    })?;
                }
                Ok(())
            }
            Step::Keys(keys) => {
                let codes = parse_keys(keys)?;
                for code in &codes {
                    self.request(Request::Key {
                        code: *code,
                        pressed: true,
                    })?;
                }
                for code in codes.iter().rev() {
                    self.request(Request::Key {
                        code: *code,
                        pressed: false,
                    })?;
                }
                Ok(())
            }
            Step::Send(send) => {
                let window = self
                    .find(&send.app_id)?
                    .with_context(|| format!("no window {}", send.app_id))?;
                self.request(Request::SendWindow {
                    window: window.id,
                    output: send.output.clone(),
                    desktop: send.desktop.map(|n| n.saturating_sub(1)),
                })?;
                Ok(())
            }
            Step::Randr(change) => {
                let socket = self.compositor.socket_path().to_path_buf();
                let thread = std::thread::spawn({
                    let change = change.clone();
                    move || crate::randr::apply(&socket, &change, false, false)
                });
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                while !thread.is_finished() {
                    anyhow::ensure!(Instant::now() < deadline, "the screen tool never finished");
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                let outcome = thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("the screen tool panicked"))??;
                anyhow::ensure!(
                    outcome == crate::randr::Outcome::Succeeded,
                    "the configuration {outcome:?}"
                );
                Ok(())
            }
            // The file is read in its time: the output gets a moment.
            Step::ExpectOutput(expected) => {
                let deadline = Instant::now() + Duration::from_secs(3);
                loop {
                    match self.check_output(expected) {
                        Ok(()) => return Ok(()),
                        Err(err) if Instant::now() > deadline => return Err(err),
                        Err(_) => self.compositor.dispatch(Duration::from_millis(10))?,
                    }
                }
            }
            Step::ExpectShown(expected) => {
                let Reply::Desktops(outputs) = self.request(Request::Desktops)? else {
                    anyhow::bail!("expected desktops");
                };
                let output = outputs
                    .iter()
                    .find(|o| o.output == expected.output)
                    .with_context(|| format!("no output {}", expected.output))?;
                let shown = output
                    .desktops
                    .iter()
                    .position(|d| d.id == output.shown)
                    .map(|i| i + 1)
                    .unwrap_or_default();
                anyhow::ensure!(
                    shown == expected.desktop,
                    "{} shows desktop {shown}, expected {}",
                    expected.output,
                    expected.desktop
                );
                Ok(())
            }
            Step::Plug(plug) => {
                self.request(Request::PlugOutput {
                    spec: plug.spec.clone(),
                    name: plug.name.clone(),
                })?;
                Ok(())
            }
            Step::SetState(set) => {
                let window = self
                    .find(&set.app_id)?
                    .with_context(|| format!("no window {}", set.app_id))?;
                self.request(Request::SetWindowState {
                    window: window.id,
                    state: set.state,
                })?;
                Ok(())
            }
            Step::Config(text) => {
                // Fixtures are found from the scenario's own directory.
                let dir = self
                    .scenario
                    .dir
                    .canonicalize()
                    .unwrap_or_else(|_| self.scenario.dir.clone());
                let mut text = text.replace("{scenario_dir}", &dir.to_string_lossy());
                // Files a scenario makes go to the compositor's own
                // temporary directory, never the developer's.
                let runtime = self.compositor.runtime_dir().to_string_lossy().into_owned();
                text = text.replace("{runtime_dir}", &runtime);
                if let Some(server) = &self.caldav {
                    text = text.replace("{caldav}", server.url());
                }
                for (name, herdr) in &self.herdrs {
                    let key = match name {
                        Some(name) => format!("{{herdr:{name}}}"),
                        None => "{herdr}".to_owned(),
                    };
                    text = text.replace(&key, &herdr.path().to_string_lossy());
                }
                if let Some(ai) = &self.ai {
                    text = text.replace("{ai}", ai.url());
                }
                if let Some(client) = &self.options.test_client {
                    text = text.replace("{test_client}", &client.to_string_lossy());
                }
                if let Some(home) = &self.home {
                    text = text.replace("{home}", &home.path().to_string_lossy());
                }
                self.compositor.write_config(&text)?;
                Ok(())
            }
            Step::ExpectAccent(expected) => self.expect_accent(expected),
            Step::ExpectConfigError(expected) => {
                let Reply::Config(info) = self.request(Request::Config)? else {
                    anyhow::bail!("expected the configuration");
                };
                match (&info.error, expected.is_empty()) {
                    (None, true) => Ok(()),
                    (Some(error), false) if error.contains(expected.as_str()) => Ok(()),
                    (error, _) => anyhow::bail!(
                        "configuration error: expected {}, got {}",
                        if expected.is_empty() {
                            "none".into()
                        } else {
                            format!("{expected:?}")
                        },
                        error.as_deref().map_or("none".into(), |e| format!("{e:?}"))
                    ),
                }
            }
            Step::ExpectConfigWarning(expected) => {
                let Reply::Config(info) = self.request(Request::Config)? else {
                    anyhow::bail!("expected the configuration");
                };
                anyhow::ensure!(
                    info.warnings.iter().any(|w| w.contains(expected.as_str())),
                    "no configuration warning with {expected:?}; warnings: {:?}",
                    info.warnings
                );
                Ok(())
            }
            Step::ExpectEvent(expected) => {
                // Events arrive with replies: a request brings the latest;
                // one a worker sends comes in its time.
                let wanted_type = format!("\"type\":\"{}\"", expected.kind);
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                let found = loop {
                    self.request(Request::Version)?;
                    self.events.extend(self.compositor.take_events());
                    let found = self.events.iter().any(|e| {
                        let json = serde_json::to_string(e).unwrap_or_default();
                        json.contains(&wanted_type)
                            && expected
                                .has
                                .as_ref()
                                .is_none_or(|h| json.contains(h.as_str()))
                    });
                    if found || Instant::now() > deadline {
                        break found;
                    }
                    self.compositor.dispatch(Duration::from_millis(10))?;
                };
                anyhow::ensure!(
                    found,
                    "no {} event{}; events: {:?}",
                    expected.kind,
                    expected
                        .has
                        .as_ref()
                        .map(|h| format!(" with {h:?}"))
                        .unwrap_or_default(),
                    self.events
                );
                Ok(())
            }
            Step::Unplug(name) => {
                self.request(Request::UnplugOutput {
                    output: name.clone(),
                })?;
                Ok(())
            }
            Step::StallOutput(name) => {
                self.request(Request::StallOutput {
                    output: name.clone(),
                })?;
                Ok(())
            }
            Step::AdvanceMs(ms) => self.compositor.advance(Duration::from_millis(*ms)),
            Step::Frames(n) => self.compositor.advance(FRAME * *n),
            Step::ExpectWindow(expected) => self.expect_window(expected),
            // A client closing goes away in its own time.
            Step::ExpectNoWindow(app_id) => {
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    match self.find(app_id)? {
                        None => return Ok(()),
                        Some(window) if Instant::now() > deadline => {
                            anyhow::bail!("expected no window {app_id}, found {window:?}")
                        }
                        Some(_) => self.compositor.dispatch(Duration::from_millis(10))?,
                    }
                }
            }
            Step::ExpectFocus(app_id) => {
                // What a client asked for (activation) may be on its way.
                let deadline = Instant::now() + SETTLE_TIMEOUT;
                loop {
                    let focused = self.windows()?.into_iter().find(|w| w.focused);
                    let focused = focused.and_then(|w| w.app_id);
                    if focused.as_deref() == Some(app_id.as_str()) {
                        return Ok(());
                    }
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "focus: expected {app_id}, got {}",
                        focused.as_deref().unwrap_or("nothing")
                    );
                    self.compositor.dispatch(Duration::from_millis(10))?;
                }
            }
            Step::ExpectStack(expected) => {
                let stack: Vec<String> = self
                    .windows()?
                    .into_iter()
                    .filter(|w| w.visible)
                    .map(|w| w.app_id.unwrap_or_default())
                    .collect();
                anyhow::ensure!(
                    &stack == expected,
                    "stack, bottom to top: expected {expected:?}, got {stack:?}"
                );
                Ok(())
            }
            Step::ExpectReceived(expected) => self.expect_received(expected),
            Step::ExpectScreenshot(expected) => self.expect_screenshot(expected),
            Step::ExpectPixel(expected) => self.expect_pixel(expected),
            Step::ExpectBudget(expected) => {
                let Reply::Stats(stats) = self.request(Request::Stats)? else {
                    anyhow::bail!("expected stats");
                };
                for o in &stats.outputs {
                    anyhow::ensure!(
                        o.max_ms <= expected.max_ms,
                        "{} drew a frame in {:.1} ms",
                        o.name,
                        o.max_ms
                    );
                }
                let held = crate::held_the_loop(&stats);
                anyhow::ensure!(held.is_empty(), "the loop was held: {held:?}");
                Ok(())
            }
            Step::ExpectBlur(expected) => {
                let Reply::Stats(stats) = self.request(Request::Stats)? else {
                    anyhow::bail!("expected stats");
                };
                let now: u64 = stats.outputs.iter().map(|o| o.blurs).sum();
                // A restart counts from zero again.
                let done = now.checked_sub(self.blurs).unwrap_or(now);
                self.blurs = now;
                counted("blurred", done, expected)
            }
            Step::ExpectGlitch(expected) => {
                let Reply::Stats(stats) = self.request(Request::Stats)? else {
                    anyhow::bail!("expected stats");
                };
                let now: u64 = stats.outputs.iter().map(|o| o.glitches).sum();
                let done = now.checked_sub(self.glitches).unwrap_or(now);
                self.glitches = now;
                counted("glitched", done, expected)
            }
        }
    }

    fn request(&mut self, request: Request) -> anyhow::Result<Reply> {
        Ok(self.compositor.request(request)?)
    }

    fn windows(&mut self) -> anyhow::Result<Vec<WindowInfo>> {
        match self.request(Request::Windows)? {
            Reply::Windows(windows) => Ok(windows),
            other => anyhow::bail!("expected windows, got {other:?}"),
        }
    }

    fn find(&mut self, app_id: &str) -> anyhow::Result<Option<WindowInfo>> {
        Ok(self
            .windows()?
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app_id)))
    }

    fn open(&mut self, open: &OpenWindow) -> anyhow::Result<()> {
        // A closed client may come back under the same app id.
        self.clients.retain(|c| !c.is_finished());
        anyhow::ensure!(
            self.clients.iter().all(|c| c.spec.app_id != open.app_id),
            "app id {} is already open",
            open.app_id
        );
        let window = SolidWindow {
            app_id: open.app_id.clone(),
            width: open.size[0],
            height: open.size[1],
            argb: parse_color(&open.color)?,
        };
        let elements = match open.accessible.as_deref() {
            None => Vec::new(),
            Some(tree) => self.accessible(open, tree)?,
        };
        let before = self.compositor.window_count();
        self.clients.push(window.spawn_with(
            self.compositor.socket_path(),
            crate::client::ClientOptions {
                inhibit_shortcuts: open.inhibit_shortcuts,
                inhibit_idle: open.inhibit_idle,
                title: open.title.clone(),
                password: open.password,
                marks: open.marks,
                elements,
            },
        )?);
        self.compositor
            .dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() > before)
            .context("the window did not appear")
    }

    /// Puts the window's application on the accessibility bus with its
    /// tree, the bus and its registry made first if they are not; the
    /// elements its window draws.
    fn accessible(
        &mut self,
        open: &OpenWindow,
        tree: &str,
    ) -> anyhow::Result<Vec<crate::client::Drawn>> {
        use crate::atspi::{FakeApp, FakeElement, FakeRegistry};
        let title = open.title.clone().unwrap_or_default();
        let window = match tree {
            "compose" => FakeElement::compose(&title),
            other => anyhow::bail!("no accessibility tree named {other:?}: compose"),
        };
        if self.a11y.is_none() {
            let bus = crate::bus::PrivateBus::start()?;
            let registry = FakeRegistry::start(&bus.address)?;
            self.compositor.set_a11y_bus(&bus.address);
            self.a11y = Some((bus, registry));
        }
        let address = self
            .a11y
            .as_ref()
            .map(|(bus, _)| bus.address.clone())
            .unwrap_or_default();
        let drawn = window.drawn();
        self.a11y_apps
            .push(FakeApp::start(&address, &open.app_id, vec![window])?);
        Ok(drawn)
    }

    fn open_x11(&mut self, open: &OpenWindow) -> anyhow::Result<()> {
        self.x11_clients.retain(|c| !c.is_finished());
        let display = self
            .compositor
            .x11_display()
            .context("no XWayland: the scenario needs `xwayland = true`")?;
        let argb = parse_color(&open.color)?;
        let window = X11Window {
            app_id: open.app_id.clone(),
            width: u16::try_from(open.size[0])?,
            height: u16::try_from(open.size[1])?,
            rgb: argb & 0x00ff_ffff,
        };
        let before = self.compositor.window_count();
        self.x11_clients.push(window.spawn(display)?);
        self.compositor
            .dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() > before)
            .context("the X11 window did not appear")
    }

    /// The accent may wait on a wallpaper made off the loop.
    fn expect_accent(&mut self, expected: &ExpectAccent) -> anyhow::Result<()> {
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            let Reply::Config(info) = self.request(Request::Config)? else {
                anyhow::bail!("expected the configuration");
            };
            let hue = info
                .accent
                .as_deref()
                .and_then(hocus_ui::Color::hex)
                .map(|c| c.hsl().0);
            if hue.is_some_and(|h| (expected.hue[0]..=expected.hue[1]).contains(&h)) {
                return Ok(());
            }
            if Instant::now() > deadline {
                anyhow::bail!(
                    "accent {:?} (hue {hue:?}), expected a hue in {:?}",
                    info.accent,
                    expected.hue
                );
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn expect_journal(&mut self, expected: &ExpectJournal) -> anyhow::Result<()> {
        let Reply::AgentsJournal(journal) = self.request(Request::AgentsJournal)? else {
            anyhow::bail!("expected the agents' journal");
        };
        let lines: Vec<String> = journal.entries.iter().map(ToString::to_string).collect();
        let all = lines.join("\n");
        if let Some(stopped) = expected.stopped {
            anyhow::ensure!(
                journal.stopped == stopped,
                "agents are {}stopped",
                if journal.stopped { "" } else { "not " }
            );
        }
        // Each string from where the one before it was found.
        let (mut line, mut column) = (0, 0);
        for wanted in &expected.has {
            let found = lines.iter().enumerate().skip(line).find_map(|(i, l)| {
                let from = if i == line { column } else { 0 };
                l.get(from..)
                    .and_then(|rest| rest.find(wanted.as_str()))
                    .map(|at| (i, from + at + wanted.len()))
            });
            let Some(next) = found else {
                anyhow::bail!("the journal lacks {wanted:?} there, in order:\n{all}");
            };
            (line, column) = next;
        }
        for unwanted in &expected.lacks {
            anyhow::ensure!(
                !all.contains(unwanted.as_str()),
                "the journal holds {unwanted:?}:\n{all}"
            );
        }
        Ok(())
    }

    /// A tool call, through the agent's lasting session when one is open,
    /// else in a session of its own; checked when the step waits, kept for
    /// `expect_mcp` when it does not.
    fn mcp(&mut self, call: &McpCall) -> anyhow::Result<()> {
        use serde_json::json;
        let mut arguments = serde_json::to_value(&call.arguments)?;
        if let Some(app_id) = &call.window {
            let window = self
                .find(app_id)?
                .with_context(|| format!("no window of {app_id}"))?;
            arguments["window"] = window.id.into();
        }
        let params = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": call.tool, "arguments": arguments },
        });
        if !call.wait {
            anyhow::ensure!(
                call.image.is_none(),
                "an image is compared once the call is answered: not with wait = false"
            );
            let fresh = self.mcp_fresh(call);
            let answer = self.mcp_send(call, params)?;
            // Once it is in, past its window's application taking the seat,
            // or answered: later steps move the clock.
            let (before, _) = self.compositor.agent_pending();
            let deadline = Instant::now() + CLIENT_TIMEOUT;
            let answer = loop {
                if let Ok(early) = answer.try_recv() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let _ = tx.send(early);
                    break rx;
                }
                let (pending, binding) = self.compositor.agent_pending();
                if !binding && pending > before {
                    break answer;
                }
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "the action never came in, or its window's application never took the \
                     agent's seat"
                );
                self.compositor.dispatch(Duration::from_millis(5))?;
            };
            self.mcp_waiting.push(McpWaiting {
                agent: call.agent.clone(),
                answer,
                fresh,
            });
            return Ok(());
        }
        // An application draws on its own time: an image is taken again
        // until it is the reference, for a moment; recorded once that
        // moment passed.
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        if self.options.bless && call.image.is_some() {
            while Instant::now() < deadline {
                self.compositor.dispatch(Duration::from_millis(20))?;
            }
        }
        loop {
            let fresh = self.mcp_fresh(call);
            let answer = self.mcp_send(call, params.clone())?;
            let answer = self.mcp_answer(&answer)?;
            self.mcp_gone(fresh)?;
            mcp_check(&answer, call.error.as_deref(), &call.contains)?;
            let Some(reference) = &call.image else {
                return Ok(());
            };
            let actual = Image::from_png(&mcp_image(&answer)?)?;
            match self.compare_reference(&actual, reference, None, &format!("{} image", call.tool))
            {
                Err(err) if Instant::now() < deadline => {
                    tracing::debug!(%err, "the image is not the reference yet");
                    self.compositor.dispatch(Duration::from_millis(20))?;
                }
                done => return done,
            }
        }
    }

    /// The agents' seats there were before a call in a session of its own.
    fn mcp_fresh(&self, call: &McpCall) -> Option<usize> {
        let lasting = self
            .mcp_sessions
            .iter()
            .any(|s| Some(&s.agent) == call.agent.as_ref());
        (!lasting).then(|| self.compositor.agent_seats())
    }

    /// Sends a call through the agent's lasting session when one is open,
    /// else in a session of its own; its answer comes on the receiver.
    fn mcp_send(
        &mut self,
        call: &McpCall,
        params: serde_json::Value,
    ) -> anyhow::Result<std::sync::mpsc::Receiver<Option<serde_json::Value>>> {
        let (tx, rx) = std::sync::mpsc::channel();
        match self
            .mcp_sessions
            .iter()
            .find(|s| Some(&s.agent) == call.agent.as_ref())
        {
            Some(session) => session
                .calls
                .send((params, tx))
                .map_err(|_| anyhow::anyhow!("the MCP session ended"))?,
            None => {
                let socket = self.compositor.api_socket().to_path_buf();
                let agent = call.agent.clone();
                std::thread::spawn(move || {
                    let mut bridge = hocus_api::mcp::Bridge::new(socket, agent);
                    mcp_initialize(&mut bridge);
                    let _ = tx.send(bridge.handle(params));
                });
            }
        }
        Ok(rx)
    }

    /// A session of its own ends with its call: its seat goes before the
    /// next step, so that seats are named the same every run.
    fn mcp_gone(&mut self, fresh: Option<usize>) -> anyhow::Result<()> {
        if let Some(before) = fresh {
            self.compositor
                .dispatch_until(CLIENT_TIMEOUT, |c| c.agent_seats() <= before)
                .context("the agent's seat stayed after its session")?;
        }
        Ok(())
    }

    /// Opens a lasting MCP session as `agent`, named at once (its seat
    /// comes then).
    fn mcp_session(&mut self, agent: &str) -> anyhow::Result<()> {
        use serde_json::json;
        anyhow::ensure!(
            !self.mcp_sessions.iter().any(|s| s.agent == agent),
            "agent {agent} already has a session"
        );
        let socket = self.compositor.api_socket().to_path_buf();
        let (calls, to_call) = std::sync::mpsc::channel::<McpCallIn>();
        let (first_tx, first) = std::sync::mpsc::channel();
        let name = agent.to_owned();
        std::thread::spawn(move || {
            let mut bridge = hocus_api::mcp::Bridge::new(socket, Some(name));
            mcp_initialize(&mut bridge);
            // A first call connects, and names the agent.
            let answer = bridge.handle(json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "list_windows", "arguments": {} },
            }));
            if first_tx.send(answer).is_err() {
                return;
            }
            // One call at a time, in order, each answered on its own.
            while let Ok((call, answer)) = to_call.recv() {
                let _ = answer.send(bridge.handle(call));
            }
        });
        let answer = self.mcp_answer(&first)?;
        mcp_check(&answer, None, &[])?;
        // The windows' applications take the new seat a moment later: the
        // scenario goes on once they did, whatever its clock does next.
        self.compositor
            .dispatch_until(CLIENT_TIMEOUT, Headless::agent_seats_taken)
            .context("the windows' applications did not take the agent's seat")?;
        self.mcp_sessions.push(McpSession {
            agent: agent.to_owned(),
            calls,
        });
        Ok(())
    }

    /// Waits for an MCP answer. An agent's action shown before it lands
    /// moves the virtual clock to when it does; one waiting for the user's
    /// confirmation is the scenario's to answer.
    fn mcp_answer(
        &mut self,
        answer: &std::sync::mpsc::Receiver<Option<serde_json::Value>>,
    ) -> anyhow::Result<serde_json::Value> {
        use std::sync::mpsc::TryRecvError;
        // As long as the longest wait a scenario asks for, and some.
        let deadline = Instant::now() + Duration::from_secs(30);
        // A capture waiting for its window to draw: the application gets a
        // moment of real time to, before the clock moves to the end of the
        // wait.
        let mut drawing: Option<Instant> = None;
        loop {
            match answer.try_recv() {
                Ok(answer) => return answer.context("no answer"),
                Err(TryRecvError::Disconnected) => anyhow::bail!("the MCP session ended"),
                Err(TryRecvError::Empty) => {}
            }
            anyhow::ensure!(Instant::now() < deadline, "the MCP server never answered");
            if let Some(until) = self.compositor.agent_capture_deadline() {
                let since = *drawing.get_or_insert_with(Instant::now);
                if since.elapsed() < Duration::from_millis(500) {
                    self.compositor.dispatch(Duration::from_millis(5))?;
                } else if until > self.compositor.now() {
                    let by = until - self.compositor.now();
                    self.compositor.advance(by)?;
                } else {
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
                continue;
            }
            let (due, asking) = self.compositor.agent_deadline();
            anyhow::ensure!(
                !asking || !self.mcp_waiting.is_empty(),
                "the action waits for the user's confirmation: call it with wait = false, \
                 then answer its card"
            );
            match due {
                Some(due) if due > self.compositor.now() => {
                    let by = due - self.compositor.now();
                    self.compositor.advance(by)?;
                }
                _ => self.compositor.dispatch(Duration::from_millis(5))?,
            }
        }
    }

    fn expect_mcp(&mut self, expected: &ExpectMcp) -> anyhow::Result<()> {
        let i = self
            .mcp_waiting
            .iter()
            .position(|w| expected.agent.is_none() || w.agent == expected.agent)
            .context("no MCP call left waiting with wait = false")?;
        let waiting = self.mcp_waiting.remove(i);
        let answer = self.mcp_answer(&waiting.answer)?;
        self.mcp_gone(waiting.fresh)?;
        mcp_check(&answer, expected.error.as_deref(), &expected.contains)
    }

    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        const SHIFT: u32 = 42;
        for c in text.chars() {
            let (code, shift) = us_key(c).with_context(|| format!("no key types {c:?}"))?;
            if shift {
                self.request(Request::Key {
                    code: SHIFT,
                    pressed: true,
                })?;
            }
            for pressed in [true, false] {
                self.request(Request::Key { code, pressed })?;
            }
            if shift {
                self.request(Request::Key {
                    code: SHIFT,
                    pressed: false,
                })?;
            }
        }
        Ok(())
    }

    /// Started programs run in their own time: the file gets a moment.
    /// A file of the state directory holds these strings, and lacks those,
    /// once its worker wrote it.
    /// The encoder finishes on its own threads: waited for in real time.
    fn expect_recording(&mut self, expected: &ExpectRecording) -> anyhow::Result<()> {
        let path = self.last_recording()?;
        let probe = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-show_entries"])
            .args(["stream=codec_type,duration,width,height", "-of", "csv=p=0"])
            .arg(&path)
            .output()?;
        let streams = String::from_utf8_lossy(&probe.stdout).into_owned();
        let length = |kind: &str| {
            streams
                .lines()
                .filter(|l| l.starts_with(kind))
                .find_map(|l| l.rsplit(',').next()?.parse::<f64>().ok())
        };
        let (video, audio) = (length("video"), length("audio"));
        if let Some(wanted) = expected.audio {
            anyhow::ensure!(audio.is_some() == wanted, "{path}: streams\n{streams}");
            if let (true, Some(v), Some(a)) = (wanted, video, audio) {
                anyhow::ensure!((v - a).abs() < 0.15, "{path}: picture {v} s, sound {a} s");
            }
        }
        if let Some(at) = expected.keys_at_ms {
            let size = streams
                .lines()
                .find(|l| l.starts_with("video"))
                .and_then(|l| {
                    let mut f = l.split(',').skip(1);
                    Some((
                        f.next()?.parse::<usize>().ok()?,
                        f.next()?.parse::<usize>().ok()?,
                    ))
                })
                .context("no picture")?;
            let frame = |ms: u64| -> anyhow::Result<Vec<u8>> {
                let out = std::process::Command::new("ffmpeg")
                    .args([
                        "-v",
                        "error",
                        "-ss",
                        &format!("{:.3}", ms as f64 / 1000.0),
                        "-i",
                    ])
                    .arg(&path)
                    .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "gray", "-"])
                    .output()?;
                anyhow::ensure!(out.stdout.len() == size.0 * size.1, "no frame at {ms} ms");
                Ok(out.stdout)
            };
            let (before, after) = (frame(at.saturating_sub(400).max(50))?, frame(at + 300)?);
            // Mean difference over a band: rows and columns as fractions.
            let band = |y0: f64, y1: f64, x0: f64, x1: f64| {
                let (w, h) = (size.0 as f64, size.1 as f64);
                let (mut sum, mut n) = (0u64, 0u64);
                for y in (h * y0) as usize..(h * y1) as usize {
                    for x in (w * x0) as usize..(w * x1) as usize {
                        let i = y * size.0 + x;
                        sum += u64::from(before[i].abs_diff(after[i]));
                        n += 1;
                    }
                }
                sum / n.max(1)
            };
            let bottom = band(0.75, 1.0, 0.2, 0.8);
            let top = band(0.0, 0.5, 0.0, 1.0);
            anyhow::ensure!(
                bottom > 8 && top < 4,
                "keys at {at} ms: the bottom changed by {bottom}, the top by {top}"
            );
        }
        Ok(())
    }

    /// Syncs run on threads, against real sockets: waited for in real time.
    fn expect_calendars(&mut self, expected: &ExpectCalendars) -> anyhow::Result<()> {
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            let Reply::Calendars(info) = self.request(Request::Calendars)? else {
                anyhow::bail!("expected the calendars");
            };
            let failed = info
                .accounts
                .iter()
                .chain(&info.online.accounts)
                .any(|a| a.error.is_some());
            let ok = expected.events.is_none_or(|n| info.events.len() == n)
                && expected
                    .online
                    .as_ref()
                    .is_none_or(|o| *o == info.online.state)
                && expected
                    .online_accounts
                    .is_none_or(|n| info.online.accounts.len() == n)
                && expected.secrets.is_none_or(|n| info.secrets == n)
                && expected.error.is_none_or(|e| e == failed)
                && expected
                    .contains
                    .iter()
                    .all(|t| info.events.iter().any(|e| e.summary == *t));
            if ok {
                return Ok(());
            }
            anyhow::ensure!(Instant::now() < deadline, "the calendars are {info:#?}");
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    /// The cache is written off the loop: waited for in real time.
    fn expect_calendar_cache(&mut self, expected: &ExpectCache) -> anyhow::Result<()> {
        let path = self.compositor.state_dir().join("calendar-cache");
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            let text = hocus_platform::sealed::Sealed::new(&path)
                .read()
                .map_err(|e| anyhow::anyhow!(e))?
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            let ok = expected.contains.iter().all(|c| text.contains(c.as_str()))
                && !expected.lacks.iter().any(|l| text.contains(l.as_str()));
            if ok {
                return Ok(());
            }
            anyhow::ensure!(Instant::now() < deadline, "the calendar cache is:\n{text}");
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    /// Syncs run on threads: waited for in real time.
    fn expect_goa(&mut self, expected: &[String]) -> anyhow::Result<()> {
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            let asked = self
                .goa
                .as_ref()
                .context("no GOA: the scenario needs `goa = { identity, dir }`")?
                .asked();
            let mut lines = asked.iter();
            if expected
                .iter()
                .all(|e| lines.any(|l| l.starts_with(e.as_str())))
            {
                return Ok(());
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "GOA and Google were asked {asked:#?}"
            );
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn expect_state_file(&mut self, expected: &ExpectFile) -> anyhow::Result<()> {
        let path = self.compositor.state_dir().join(&expected.name);
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let ok = expected.contains.iter().all(|c| text.contains(c.as_str()))
                && !expected.lacks.iter().any(|l| text.contains(l.as_str()));
            if ok {
                return Ok(());
            }
            anyhow::ensure!(Instant::now() < deadline, "{} is:\n{text}", expected.name);
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn expect_runtime_file(&mut self, expected: &ExpectFile) -> anyhow::Result<()> {
        let dir = self
            .compositor
            .api_socket()
            .parent()
            .context("no runtime directory")?
            .to_path_buf();
        let path = dir.join(&expected.name);
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        let text = loop {
            match std::fs::read_to_string(&path) {
                Ok(text) if !text.is_empty() => break text,
                _ if Instant::now() > deadline => {
                    anyhow::bail!("no {} in the runtime directory", expected.name)
                }
                _ => self.compositor.dispatch(Duration::from_millis(10))?,
            }
        };
        if let Some(lines) = expected.lines {
            anyhow::ensure!(
                text.lines().count() == lines,
                "{} has {} lines, expected {lines}:\n{text}",
                expected.name,
                text.lines().count()
            );
        }
        for wanted in &expected.contains {
            anyhow::ensure!(
                text.contains(wanted.as_str()),
                "{} lacks {wanted:?}:\n{text}",
                expected.name
            );
        }
        for unwanted in &expected.lacks {
            anyhow::ensure!(
                !text.contains(unwanted.as_str()),
                "{} has {unwanted:?}:\n{text}",
                expected.name
            );
        }
        Ok(())
    }

    fn bus(&self) -> anyhow::Result<&str> {
        self.bus
            .as_ref()
            .map(|b| b.address.as_str())
            .context("no bus: the scenario needs `dbus = true`")
    }

    fn notify_send(&mut self, args: &[String]) -> anyhow::Result<()> {
        let mut child = std::process::Command::new("notify-send")
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("DBUS_SESSION_BUS_ADDRESS", self.bus()?)
            .spawn()
            .context("could not run notify-send")?;
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            if let Some(status) = child.try_wait()? {
                anyhow::ensure!(status.success(), "notify-send failed: {status}");
                return Ok(());
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("notify-send did not return");
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn notify(&mut self, n: &NotifyStep) -> anyhow::Result<()> {
        if self.notification_signals.is_none() {
            self.notification_signals = Some(crate::bus::listen_notifications(self.bus()?)?);
        }
        let urgency = match n.urgency.as_deref() {
            Some("low") => 0_u8,
            Some("critical") => 2,
            Some("normal") | None => 1,
            Some(other) => anyhow::bail!("unknown urgency {other:?}"),
        };
        let actions: Vec<&str> = n
            .actions
            .iter()
            .flat_map(|[k, l]| [k.as_str(), l.as_str()])
            .collect();
        let hints: Vec<(&str, &str)> = n
            .hints
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        crate::bus::notify(
            self.bus()?,
            &crate::bus::Note {
                app: &n.app,
                replaces: n.replaces.unwrap_or(0),
                summary: &n.summary,
                body: &n.body,
                actions: &actions,
                urgency,
                timeout_ms: n.timeout_ms.unwrap_or(-1),
                hints: &hints,
            },
        )?;
        Ok(())
    }

    fn ui(&mut self) -> anyhow::Result<Vec<hocus_api::UiNode>> {
        match self.request(Request::Ui)? {
            Reply::Ui(nodes) => Ok(nodes),
            other => anyhow::bail!("expected the interface, got {other:?}"),
        }
    }

    /// The user's configuration directory the settings write, beside the
    /// compositor's own private configuration.
    fn xdg_home(&self) -> PathBuf {
        self.compositor.state_dir().with_file_name("xdg-config")
    }

    fn open_settings(&mut self, open: &OpenSettings) -> anyhow::Result<()> {
        let wayland = self.compositor.socket_path().to_path_buf();
        let running = hocus_settings::client::spawn(hocus_settings::client::Options {
            api: hocus_api::socket_for(&wayland),
            wayland,
            page: open
                .page
                .as_deref()
                .map(|p| {
                    hocus_settings::catalogue::Page::from_slug(p)
                        .with_context(|| format!("no settings page {p}"))
                })
                .transpose()?,
            lang: hocus_shell::Lang::En,
            app_id: "hocus-settings".into(),
            // Never the machine's: the stand-in's, or none.
            system_bus: match &self.system_bus {
                Some(bus) => hocus_settings::client::SystemBus::At(bus.address.clone()),
                None => hocus_settings::client::SystemBus::Off,
            },
            // A private directory of the user's, then the scenario's.
            xdg: hocus_settings::client::Xdg::At(hocus_platform::mime::Dirs {
                config: std::iter::once(self.xdg_home())
                    .chain(
                        self.scenario
                            .config_dirs
                            .iter()
                            .map(|d| self.scenario.dir.join(d)),
                    )
                    .collect(),
                data: self
                    .scenario
                    .data_dirs
                    .iter()
                    .map(|d| self.scenario.dir.join(d))
                    .collect(),
            }),
            // The scenarios' own copy of the zones' places: the map draws
            // the same wherever the tests run.
            zoneinfo: self.scenario.dir.join("zoneinfo"),
            // The scenario's own home, never the developer's.
            home: self.home.as_ref().map(|h| h.path().to_owned()),
        })?;
        self.settings = Some(running);
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            let shown = self.find("hocus-settings")?.is_some()
                && self.settings.as_ref().is_some_and(|s| s.access().is_some());
            if shown {
                return Ok(());
            }
            if let Some(s) = self.settings.take_if(|s| s.is_finished()) {
                return Err(s
                    .join()
                    .err()
                    .unwrap_or_else(|| anyhow::anyhow!("the settings app stopped")));
            }
            anyhow::ensure!(Instant::now() < deadline, "the settings window never came");
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
    }

    /// The last recording saved, as its event said.
    fn last_recording(&mut self) -> anyhow::Result<String> {
        let deadline = Instant::now() + CLIENT_TIMEOUT * 2;
        loop {
            // Events arrive with replies.
            self.request(Request::Version)?;
            self.events.extend(self.compositor.take_events());
            let saved = self.events.iter().rev().find_map(|e| match e {
                hocus_api::Event::Recording { path, .. } => path.clone(),
                _ => None,
            });
            if let Some(path) = saved {
                return Ok(path);
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "no recording saved; events: {:?}",
                self.events
            );
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    /// An open editor has done what it was asked (a drag, a frame to
    /// show), and the compositor has its last frame.
    fn editor_caught_up(&mut self) -> anyhow::Result<()> {
        if self.editor.is_none() {
            return Ok(());
        }
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        while self
            .editor
            .as_ref()
            .and_then(|e| e.shown())
            .is_none_or(|s| s.drawing)
        {
            anyhow::ensure!(Instant::now() < deadline, "the editor is still busy");
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
        let until = Instant::now() + Duration::from_millis(40);
        while Instant::now() < until {
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
        Ok(())
    }

    /// A file the editor exported beside the last recording.
    fn expect_export(&mut self, expected: &ExpectExport) -> anyhow::Result<()> {
        let recording = PathBuf::from(self.last_recording()?);
        let dir = recording.parent().context("the recording has no folder")?;
        let deadline = Instant::now() + CLIENT_TIMEOUT * 4;
        let path = loop {
            let found = std::fs::read_dir(dir)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .find(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().contains(&expected.name_has))
                });
            // Written whole: the editor names it once the encoder is done.
            let said = self
                .editor
                .as_ref()
                .and_then(|e| e.access())
                .is_some_and(|a| {
                    a.dump()
                        .lines()
                        .any(|l| l.contains("Saved as") && l.contains(&expected.name_has))
                });
            if let Some(path) = found.filter(|_| said) {
                break path;
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "no {:?} beside {}; the editor shows:\n{}",
                expected.name_has,
                recording.display(),
                self.editor
                    .as_ref()
                    .and_then(|e| e.access())
                    .map(|a| a.dump())
                    .unwrap_or_default()
            );
            self.compositor.dispatch(Duration::from_millis(20))?;
        };
        let size = std::fs::metadata(&path)?.len();
        if let Some(max) = expected.max_bytes {
            anyhow::ensure!(size <= max, "{}: {size} bytes", path.display());
        }
        if let Some([from, to]) = expected.length {
            let probe = std::process::Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&path)
                .output()?;
            let length: f64 = String::from_utf8_lossy(&probe.stdout).trim().parse()?;
            anyhow::ensure!(
                (from..=to).contains(&length),
                "{}: {length} s",
                path.display()
            );
        }
        Ok(())
    }

    fn open_editor(
        &mut self,
        file: &str,
        editing: hocus_annotate::client::Editing,
    ) -> anyhow::Result<()> {
        let runtime = self.compositor.runtime_dir().to_string_lossy().into_owned();
        let mut file = file.replace("{runtime_dir}", &runtime);
        if file.contains("{last_recording}") {
            let last = self.last_recording()?;
            file = file.replace("{last_recording}", &last);
        }
        if file.contains("{last_screenshot}") {
            // The last capture saved, as its event said.
            self.request(Request::Stats)?;
            self.events.extend(self.compositor.take_events());
            let last = self
                .events
                .iter()
                .rev()
                .find_map(|e| match e {
                    hocus_api::Event::Screenshot { path, .. } => path.clone(),
                    _ => None,
                })
                .context("no screenshot saved yet")?;
            file = file.replace("{last_screenshot}", &last);
        }
        let wayland = self.compositor.socket_path().to_path_buf();
        let running = hocus_annotate::client::spawn(hocus_annotate::client::Options {
            api: Some(hocus_api::socket_for(&wayland)),
            wayland,
            file: file.into(),
            lang: hocus_shell::Lang::En,
            theme: hocus_ui::Theme::default(),
            editing,
        })?;
        self.editor = Some(running);
        let app_id = match editing {
            hocus_annotate::client::Editing::Annotate => ANNOTATE,
            hocus_annotate::client::Editing::Trim => TRIM,
        }
        .app_id();
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            let shown = self.find(app_id)?.is_some()
                && self.editor.as_ref().is_some_and(|s| s.access().is_some());
            if shown {
                return Ok(());
            }
            if let Some(s) = self.editor.take_if(|s| s.is_finished()) {
                return Err(s
                    .join()
                    .err()
                    .unwrap_or_else(|| anyhow::anyhow!("the editor window stopped")));
            }
            anyhow::ensure!(Instant::now() < deadline, "the editor window never came");
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
    }

    /// The settings window's element with `label`, and the window's corner.
    fn settings_node(
        &mut self,
        label: &str,
    ) -> anyhow::Result<(Option<hocus_ui::AccessNode>, hocus_ui::AccessNode)> {
        fn find(node: &hocus_ui::AccessNode, label: &str) -> Option<hocus_ui::AccessNode> {
            if node.label.as_deref() == Some(label) {
                return Some(node.clone());
            }
            node.children.iter().find_map(|c| find(c, label))
        }
        let access = match self.driving {
            Driving::Settings => self.settings.as_ref().and_then(|s| s.access()),
            Driving::Editor(_) => self.editor.as_ref().and_then(|s| s.access()),
        }
        .context("no such window; open it first")?;
        Ok((find(&access, label), access))
    }

    /// The settings app redraws after each change: it gets a moment.
    fn expect_settings(&mut self, expected: &ExpectSettings) -> anyhow::Result<()> {
        let deadline = Instant::now() + SETTLE_TIMEOUT.max(Duration::from_secs(2));
        loop {
            let (node, all) = self.settings_node(&expected.label)?;
            let wrong = match (&node, expected.shown) {
                (None, true) => Some("not shown".to_owned()),
                (Some(_), false) => Some("shown".to_owned()),
                (None, false) => None,
                (Some(n), true) => {
                    let mut wrong = Vec::new();
                    if expected.focused.is_some_and(|f| f != n.focused) {
                        wrong.push(format!("focused {}", n.focused));
                    }
                    if expected.selected.is_some_and(|s| s != n.selected) {
                        wrong.push(format!("selected {}", n.selected));
                    }
                    (!wrong.is_empty()).then(|| wrong.join(", "))
                }
            };
            let Some(wrong) = wrong else {
                return Ok(());
            };
            if Instant::now() > deadline {
                anyhow::bail!(
                    "{:?}: {wrong}; the window shows:\n{}",
                    expected.label,
                    all.dump()
                );
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    /// An open settings app first catches up with what is in effect (the
    /// configuration and the screens), and the compositor takes its frame:
    /// on a slow machine it may not have heard of the last change yet.
    fn settings_caught_up(&mut self) -> anyhow::Result<()> {
        if self.settings.is_none() {
            return Ok(());
        }
        let Reply::Config(info) = self.request(Request::Config)? else {
            anyhow::bail!("expected the configuration");
        };
        let Reply::Outputs(outputs) = self.request(Request::Outputs)? else {
            anyhow::bail!("expected the outputs");
        };
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            if self
                .settings
                .as_ref()
                .is_some_and(|s| s.shows(&info.toml, &outputs))
            {
                break;
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "the settings never showed the configuration in effect"
            );
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
        let until = Instant::now() + Duration::from_millis(40);
        while Instant::now() < until {
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
        Ok(())
    }

    /// Waits until the window drew the same thing twice in a row: what is
    /// clicked is where it shows.
    fn settle_window(&mut self) -> anyhow::Result<()> {
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        let mut last = None;
        loop {
            let until = Instant::now() + Duration::from_millis(40);
            while Instant::now() < until {
                self.compositor.dispatch(Duration::from_millis(5))?;
            }
            let (_, now) = self.settings_node("")?;
            if last.as_ref() == Some(&now) || Instant::now() > deadline {
                return Ok(());
            }
            last = Some(now);
        }
    }

    fn click_settings(&mut self, label: &str) -> anyhow::Result<()> {
        self.click_settings_at(label, [0.5, 0.5])
    }

    fn click_settings_at(&mut self, label: &str, at: [f32; 2]) -> anyhow::Result<()> {
        self.settings_caught_up()?;
        self.settle_window()?;
        let (node, all) = self.settings_node(label)?;
        let node =
            node.with_context(|| format!("no {label:?}; the window shows:\n{}", all.dump()))?;
        let window = self
            .find(self.driving.app_id())?
            .context("no such window")?;
        let g = window.geometry;
        let (x, y) = (
            f64::from(g.x) + f64::from(node.rect.x + node.rect.w * at[0]),
            f64::from(g.y) + f64::from(node.rect.y + node.rect.h * at[1]),
        );
        self.request(Request::PointerMove { x, y })?;
        for pressed in [true, false] {
            self.request(Request::PointerButton {
                button: Button::Left,
                pressed,
            })?;
        }
        // The app gets the click.
        let until = Instant::now() + Duration::from_millis(100);
        while Instant::now() < until {
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
        Ok(())
    }

    fn check_output(&mut self, expected: &ExpectOutput) -> anyhow::Result<()> {
        let Reply::Outputs(outputs) = self.request(Request::Outputs)? else {
            anyhow::bail!("expected outputs");
        };
        let found = outputs.iter().find(|o| o.name == expected.name);
        let Some(output) = found else {
            anyhow::ensure!(!expected.shown, "no output {}", expected.name);
            return Ok(());
        };
        anyhow::ensure!(expected.shown, "{} is on", expected.name);
        let g = &output.geometry;
        if let Some(want) = expected.geometry {
            let got = [g.x, g.y, g.width, g.height];
            anyhow::ensure!(
                got == want,
                "{} is at {got:?}, expected {want:?}",
                expected.name
            );
        }
        if let Some(want) = expected.pixels {
            let got = [output.pixel_width, output.pixel_height];
            anyhow::ensure!(
                got == want,
                "{} has {got:?} pixels, expected {want:?}",
                expected.name
            );
        }
        if let Some([low, high]) = expected.temperature {
            anyhow::ensure!(
                (low..=high).contains(&output.temperature),
                "{} is at {} K, expected {low} to {high}",
                expected.name,
                output.temperature
            );
        }
        if let Some(want) = expected.scale {
            anyhow::ensure!(
                (output.scale - want).abs() < 1e-6,
                "{} is at scale {}, expected {want}",
                expected.name,
                output.scale
            );
        }
        Ok(())
    }

    fn drag_settings(&mut self, drag: &DragSettings) -> anyhow::Result<()> {
        let (node, all) = self.settings_node(&drag.label)?;
        let node = node
            .with_context(|| format!("no {:?}; the window shows:\n{}", drag.label, all.dump()))?;
        let window = self
            .find(self.driving.app_id())?
            .context("no such window")?;
        let g = window.geometry;
        let (x, y) = (
            f64::from(g.x) + f64::from(node.rect.x + node.rect.w / 2.0),
            f64::from(g.y) + f64::from(node.rect.y + node.rect.h / 2.0),
        );
        let pause = |s: &mut Self| -> anyhow::Result<()> {
            let until = Instant::now() + Duration::from_millis(30);
            while Instant::now() < until {
                s.compositor.dispatch(Duration::from_millis(5))?;
            }
            Ok(())
        };
        self.request(Request::PointerMove { x, y })?;
        self.request(Request::PointerButton {
            button: Button::Left,
            pressed: true,
        })?;
        pause(self)?;
        // In steps, as a hand moves.
        for i in 1..=8 {
            let f = f64::from(i) / 8.0;
            self.request(Request::PointerMove {
                x: x + drag.by[0] * f,
                y: y + drag.by[1] * f,
            })?;
            pause(self)?;
        }
        self.request(Request::PointerButton {
            button: Button::Left,
            pressed: false,
        })?;
        let until = Instant::now() + Duration::from_millis(100);
        while Instant::now() < until {
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
        // The annotation window draws the shape once it has the release:
        // on a slow machine, later than any fixed wait.
        if matches!(self.driving, Driving::Editor(_)) {
            let end = (
                node.rect.x + node.rect.w / 2.0 + drag.by[0] as f32,
                node.rect.y + node.rect.h / 2.0 + drag.by[1] as f32,
            );
            let deadline = Instant::now() + CLIENT_TIMEOUT;
            loop {
                let shown = self.editor.as_ref().and_then(|a| a.shown());
                let done = shown.is_some_and(|s| {
                    !s.drawing
                        && s.pointer
                            .is_some_and(|p| (p.x - end.0).abs() < 1.5 && (p.y - end.1).abs() < 1.5)
                });
                if done {
                    break;
                }
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "the annotation window never drew the drag"
                );
                self.compositor.dispatch(Duration::from_millis(5))?;
            }
        }
        Ok(())
    }

    /// The settings app writes the file from its own thread: it gets a
    /// moment.
    fn expect_config_file(&mut self, expected: &ExpectConfigFile) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let text = std::fs::read_to_string(self.compositor.config_path()).unwrap_or_default();
            let ok = expected.contains.iter().all(|c| text.contains(c.as_str()))
                && expected.lacks.iter().all(|l| !text.contains(l.as_str()));
            if ok {
                return Ok(());
            }
            if Instant::now() > deadline {
                anyhow::bail!("the configuration file is:\n{text}");
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn tray_app(&self, title: &str) -> anyhow::Result<&crate::tray::TrayApp> {
        self.tray_apps
            .iter()
            .find(|(t, _)| t == title)
            .map(|(_, app)| app)
            .with_context(|| format!("no tray application {title:?}"))
    }

    /// What a button shows may come from a worker (a check, a scan): the
    /// step waits a moment for it, as `expect_ui` does.
    fn click_ui(&mut self, label: &str, button: Button) -> anyhow::Result<()> {
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        let r = loop {
            let nodes = self.ui()?;
            // A button first; else any element so labelled (a card's text).
            let labelled = |n: &&hocus_api::UiNode| n.label.as_deref() == Some(label);
            if let Some(node) = nodes
                .iter()
                .filter(labelled)
                .find(|n| n.role == "button")
                .or_else(|| nodes.iter().find(labelled))
            {
                break node.rect;
            }
            if Instant::now() > deadline {
                let buttons: Vec<_> = nodes
                    .iter()
                    .filter(|n| n.role == "button")
                    .filter_map(|n| n.label.clone())
                    .collect();
                anyhow::bail!("no button {label:?}; there are {buttons:?}");
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        };
        let (x, y) = (
            f64::from(r.x) + f64::from(r.width) / 2.0,
            f64::from(r.y) + f64::from(r.height) / 2.0,
        );
        self.request(Request::PointerMove { x, y })?;
        for pressed in [true, false] {
            self.request(Request::PointerButton { button, pressed })?;
        }
        Ok(())
    }

    /// The shell redraws after each dispatch: it gets a moment.
    /// The indexer works on threads of its own, on real time: it may
    /// take a few seconds on a loaded machine.
    fn expect_search(&mut self, expected: &ExpectSearch) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let Reply::Search(info) = self.request(Request::Search)? else {
                anyhow::bail!("expected the search's state");
            };
            let ok = expected.state.as_ref().is_none_or(|s| *s == info.state)
                && expected.documents.is_none_or(|n| n == info.documents)
                && expected.embedded.is_none_or(|n| n == info.embedded)
                && expected.never_read.is_none_or(|n| n == info.never_read)
                && expected.unreadable.is_none_or(|n| n == info.unreadable)
                && expected.meaning.is_none_or(|m| m == info.meaning);
            if ok {
                return Ok(());
            }
            anyhow::ensure!(Instant::now() < deadline, "the search is {info:?}");
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn expect_ui(&mut self, expected: &ExpectUi) -> anyhow::Result<()> {
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        loop {
            let nodes = self.ui()?;
            let found = nodes
                .iter()
                .find(|n| n.label.as_deref() == Some(expected.label.as_str()));
            let within = |r: &hocus_api::Rect| {
                expected.inside.is_none_or(|[x, y, w, h]| {
                    r.x >= x && r.y >= y && r.x + r.width <= x + w && r.y + r.height <= y + h
                })
            };
            let ok = match (found, expected.shown) {
                (Some(n), true) => {
                    expected.disabled.is_none_or(|d| d == n.disabled)
                        && expected.selected.is_none_or(|s| s == n.selected)
                        && within(&n.rect)
                }
                (None, false) => true,
                _ => false,
            };
            if ok {
                return Ok(());
            }
            if Instant::now() > deadline {
                let labels: Vec<_> = nodes.iter().filter_map(|n| n.label.clone()).collect();
                anyhow::bail!(
                    "{:?} {}; the interface shows {labels:?}",
                    expected.label,
                    match (found, expected.shown) {
                        (Some(n), true) => format!(
                            "has disabled = {}, selected = {} and is at {:?}",
                            n.disabled, n.selected, n.rect
                        ),
                        (Some(_), false) => "is shown".into(),
                        _ => "is not shown".into(),
                    }
                );
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn copy(&mut self, copy: &Clipboard) -> anyhow::Result<()> {
        let (before, _) = self.compositor.clipboard_quiet();
        if let Some(client) = self.clients.iter().find(|c| c.spec.app_id == copy.app_id) {
            if let Some(secret) = &copy.then_secret {
                client.copy_then_secret(&copy.text, secret)?;
            } else if copy.secret {
                client.copy_secret(&copy.text)?;
            } else {
                client.copy(&copy.text)?;
            }
        } else if let Some(client) = self
            .x11_clients
            .iter()
            .find(|c| c.spec.app_id == copy.app_id)
        {
            client.copy(&copy.text)?;
        } else {
            anyhow::bail!("no client {}", copy.app_id);
        }
        // The compositor has the copy, read or set aside, before the next
        // step: the read runs on a worker, slow on a loaded machine.
        self.compositor
            .dispatch_until(CLIENT_TIMEOUT, |c| {
                let (seen, quiet) = c.clipboard_quiet();
                seen > before && quiet
            })
            .map_err(|_| anyhow::anyhow!("the compositor never read the copy {:?}", copy.text))
    }

    /// Reads the clipboard as the client sees it; it may take a few round
    /// trips through the compositor and XWayland.
    fn expect_clipboard(&mut self, expected: &Clipboard) -> anyhow::Result<()> {
        let wayland = self
            .clients
            .iter()
            .position(|c| c.spec.app_id == expected.app_id);
        let x11 = self
            .x11_clients
            .iter()
            .position(|c| c.spec.app_id == expected.app_id);
        anyhow::ensure!(
            wayland.is_some() || x11.is_some(),
            "no client {}",
            expected.app_id
        );
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        let mut asked = Instant::now() - Duration::from_secs(1);
        loop {
            let got = match (wayland, x11) {
                (Some(i), _) => self.clients[i].clipboard(),
                (_, Some(i)) => {
                    // Asked again now and then: the first request may come
                    // before the clipboard reached X11.
                    if asked.elapsed() > Duration::from_millis(200) {
                        self.x11_clients[i].paste()?;
                        asked = Instant::now();
                    }
                    self.x11_clients[i].pasted()
                }
                (None, None) => None,
            };
            if got.as_deref() == Some(expected.text.as_str()) {
                return Ok(());
            }
            if Instant::now() > deadline {
                anyhow::bail!(
                    "{} read {:?} from the clipboard, expected {:?}",
                    expected.app_id,
                    got,
                    expected.text
                );
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn close(&mut self, app_id: &str) -> anyhow::Result<()> {
        let window = self
            .find(app_id)?
            .with_context(|| format!("no window {app_id}"))?;
        let before = self.compositor.window_count();
        self.request(Request::CloseWindow { window: window.id })?;
        self.compositor
            .dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() < before)
            .context("the window did not go away")
    }

    /// The token a started program was given, read off what it wrote,
    /// used by a client for its own window.
    fn activate_with(&mut self, a: &ActivateWith) -> anyhow::Result<()> {
        let path = self.compositor.runtime_dir().join(&a.token_from);
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        let token = loop {
            let found = std::fs::read_to_string(&path).ok().and_then(|text| {
                text.lines()
                    .find_map(|l| l.strip_prefix("XDG_ACTIVATION_TOKEN="))
                    .map(str::to_owned)
            });
            match found {
                Some(token) => break token,
                None if Instant::now() > deadline => {
                    anyhow::bail!("no activation token in {}", a.token_from)
                }
                None => self.compositor.dispatch(Duration::from_millis(10))?,
            }
        };
        self.clients
            .iter()
            .find(|c| c.spec.app_id == a.app_id)
            .with_context(|| format!("no client {}", a.app_id))?
            .activate_with(&token)
    }

    /// The client takes a token, the virtual clock moves past the time a
    /// token brings a window forward, and only then it asks with it.
    fn ask_attention(&mut self, app_id: &str) -> anyhow::Result<()> {
        let client = self
            .clients
            .iter()
            .find(|c| c.spec.app_id == app_id)
            .with_context(|| format!("no client {app_id}"))?;
        client.take_token()?;
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        loop {
            let has = self
                .clients
                .iter()
                .find(|c| c.spec.app_id == app_id)
                .is_some_and(|c| c.events().contains(&crate::client::ClientEvent::Token));
            if has {
                break;
            }
            anyhow::ensure!(Instant::now() < deadline, "{app_id} got no token");
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
        self.compositor.advance(Duration::from_secs(11))?;
        self.clients
            .iter()
            .find(|c| c.spec.app_id == app_id)
            .with_context(|| format!("no client {app_id}"))?
            .use_token()?;
        // It reaches the loop before the next step.
        let until = Instant::now() + Duration::from_millis(200);
        while Instant::now() < until {
            self.compositor.dispatch(Duration::from_millis(5))?;
        }
        Ok(())
    }

    /// Clients resize in their own threads: the window gets a moment to
    /// match before the step fails.
    fn expect_window(&mut self, expected: &ExpectWindow) -> anyhow::Result<()> {
        // A program the compositor started may still be on its way.
        let deadline = Instant::now() + SETTLE_TIMEOUT.max(CLIENT_TIMEOUT / 2);
        loop {
            let wrong = match self.window_differences(expected) {
                Ok(wrong) => wrong,
                Err(missing) if Instant::now() <= deadline => vec![missing.to_string()],
                Err(missing) => return Err(missing),
            };
            if wrong.is_empty() {
                return Ok(());
            }
            if Instant::now() > deadline {
                anyhow::bail!("{}", wrong.join("; "));
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    fn window_differences(&mut self, expected: &ExpectWindow) -> anyhow::Result<Vec<String>> {
        let window = self
            .windows()?
            .into_iter()
            .find(|w| {
                w.app_id.as_deref() == Some(expected.app_id.as_str())
                    && expected
                        .title
                        .as_ref()
                        .is_none_or(|t| w.title.as_ref() == Some(t))
            })
            .with_context(|| match &expected.title {
                Some(title) => format!("no window {} titled {title:?}", expected.app_id),
                None => format!("no window {}", expected.app_id),
            })?;
        let mut wrong = Vec::new();
        if let Some(output) = &expected.output
            && window.output.as_ref() != Some(output)
        {
            wrong.push(format!(
                "output: expected {output}, got {}",
                window.output.as_deref().unwrap_or("none")
            ));
        }
        if let Some(geometry) = expected.geometry {
            let g = window.geometry;
            let actual = [g.x, g.y, g.width, g.height];
            if actual != geometry {
                wrong.push(format!("geometry: expected {geometry:?}, got {actual:?}"));
            }
        }
        if let Some(focused) = expected.focused
            && window.focused != focused
        {
            wrong.push(format!(
                "focused: expected {focused}, got {}",
                window.focused
            ));
        }
        if let Some(visible) = expected.visible
            && window.visible != visible
        {
            wrong.push(format!(
                "visible: expected {visible}, got {}",
                window.visible
            ));
        }
        if let Some(opacity) = expected.opacity
            && (window.opacity - opacity).abs() > 0.001
        {
            wrong.push(format!(
                "opacity: expected {opacity}, got {}",
                window.opacity
            ));
        }
        if let Some(pinned) = expected.pinned
            && window.pinned != pinned
        {
            wrong.push(format!("pinned: expected {pinned}, got {}", window.pinned));
        }
        if let Some(attention) = expected.attention
            && window.attention != attention
        {
            wrong.push(format!(
                "attention: expected {attention}, got {}",
                window.attention
            ));
        }
        if let Some(state) = expected.state
            && window.state != state
        {
            wrong.push(format!("state: expected {state:?}, got {:?}", window.state));
        }
        Ok(wrong)
    }

    /// What the window client of `app_id`, or the layer client of that
    /// namespace, received so far.
    fn events_of(&self, app_id: &str) -> anyhow::Result<Vec<crate::client::ClientEvent>> {
        if let Some(c) = self.clients.iter().find(|c| c.spec.app_id == app_id) {
            return Ok(c.events());
        }
        self.layers
            .iter()
            .find(|l| l.spec.namespace == app_id)
            .map(|l| l.events())
            .with_context(|| format!("no client {app_id}"))
    }

    fn expect_received(&mut self, expected: &ExpectReceived) -> anyhow::Result<()> {
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        // The client reads its events in its own thread.
        loop {
            let events = self.events_of(&expected.app_id)?;
            if events.iter().any(|e| e.to_string() == expected.event) {
                return Ok(());
            }
            if Instant::now() > deadline {
                let got: Vec<String> = events.iter().map(ToString::to_string).collect();
                anyhow::bail!(
                    "{} never received `{}`; it received: {}",
                    expected.app_id,
                    expected.event,
                    got.join(", ")
                );
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    /// Some pixels are made off the loop (wallpapers): the step gets a
    /// moment before it fails.
    fn expect_pixel(&mut self, expected: &ExpectPixel) -> anyhow::Result<()> {
        self.wallpapers()?;
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        loop {
            let reply = self.request(Request::Screenshot {
                target: Target::Output(expected.output.clone()),
                cursor: false,
            })?;
            let Reply::Screenshot(shot) = reply else {
                anyhow::bail!("expected a screenshot, got {reply:?}");
            };
            let image = Image::from_png(&shot.png()?)?;
            let [x, y] = expected.at;
            let got = image
                .pixel(x, y)
                .with_context(|| format!("{x},{y} is outside {}x{}", image.width, image.height))?;
            let want = parse_color(&expected.color)?.to_be_bytes();
            let tolerance = expected.tolerance.unwrap_or(24);
            let close = (0..3).all(|i| got[i].abs_diff(want[i + 1]) <= tolerance);
            if close == expected.is {
                return Ok(());
            }
            if Instant::now() > deadline {
                anyhow::bail!(
                    "pixel {x},{y} is #{:02x}{:02x}{:02x}, expected {}{}",
                    got[0],
                    got[1],
                    got[2],
                    if expected.is { "" } else { "anything but " },
                    expected.color
                );
            }
            self.compositor.dispatch(Duration::from_millis(10))?;
        }
    }

    /// Wallpapers are made off the loop: pictures wait for them.
    fn wallpapers(&mut self) -> anyhow::Result<()> {
        self.compositor
            .dispatch_until(CLIENT_TIMEOUT, |c| c.wallpapers_ready())
            .context("the wallpapers never came")?;
        // What arrived is drawn.
        self.compositor.advance(FRAME)
    }

    fn expect_screenshot(&mut self, expected: &ExpectScreenshot) -> anyhow::Result<()> {
        self.wallpapers()?;
        self.editor_caught_up()?;
        self.settings_caught_up()?;
        let reply = self.request(Request::Screenshot {
            target: Target::Output(expected.output.clone()),
            cursor: expected.cursor,
        })?;
        let Reply::Screenshot(shot) = reply else {
            anyhow::bail!("expected a screenshot, got {reply:?}");
        };
        let actual = Image::from_png(&shot.png()?)?;
        let tolerance = Tolerance {
            channel: expected
                .channel_tolerance
                .unwrap_or(Tolerance::default().channel),
            pixels: expected
                .pixel_tolerance
                .unwrap_or(Tolerance::default().pixels),
        };
        self.compare_reference(
            &actual,
            &expected.reference,
            Some(tolerance),
            &format!("screenshot of {}", expected.output),
        )
    }

    /// `actual` is the reference image next to the scenario (recorded
    /// with HOCUS_BLESS); otherwise the images go to the artifacts.
    fn compare_reference(
        &self,
        actual: &Image,
        reference: &Path,
        tolerance: Option<Tolerance>,
        what: &str,
    ) -> anyhow::Result<()> {
        let path = self.scenario.dir.join(reference);
        if self.options.bless {
            actual.save(&path)?;
            return Ok(());
        }
        let stem = reference
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "screenshot".into());
        let artifacts = self.options.artifacts.join(&self.scenario.name);
        let actual_path = artifacts.join(format!("{stem}.actual.png"));
        if !path.exists() {
            actual.save(&actual_path)?;
            anyhow::bail!(
                "no reference image at {}; the actual one is in {}. Run with HOCUS_BLESS=1 to \
                 record it",
                path.display(),
                actual_path.display()
            );
        }
        let wanted = Image::load(&path)?;
        let Some(difference) = actual.compare(&wanted, tolerance.unwrap_or_default()) else {
            return Ok(());
        };
        actual.save(&actual_path)?;
        wanted.save(&artifacts.join(format!("{stem}.expected.png")))?;
        if let Difference::Pixels { diff, .. } = &difference {
            diff.save(&artifacts.join(format!("{stem}.diff.png")))?;
        }
        anyhow::bail!(
            "{what} differs from {}: {difference}; images in {}",
            path.display(),
            artifacts.display()
        )
    }
}

/// `"super+shift+2"` to evdev codes.
fn parse_keys(keys: &str) -> anyhow::Result<Vec<u32>> {
    keys.split('+')
        .map(|name| {
            let name = name.trim().to_ascii_lowercase();
            if let Ok(code) = name.parse::<u32>()
                && name.len() > 1
            {
                return Ok(code);
            }
            key_code(&name).with_context(|| format!("unknown key {name} in {keys}"))
        })
        .collect()
}

/// Evdev codes of the keys scenarios name.
fn key_code(name: &str) -> Option<u32> {
    const DIGITS: [u32; 10] = [11, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    const LETTERS: &str = "qwertyuiopasdfghjklzxcvbnm";
    const LETTER_CODES: [u32; 26] = [
        16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 30, 31, 32, 33, 34, 35, 36, 37, 38, 44, 45, 46, 47,
        48, 49, 50,
    ];
    let code = match name {
        "super" | "logo" => 125,
        "shift" => 42,
        "ctrl" => 29,
        "alt" => 56,
        "enter" => 28,
        "escape" | "esc" => 1,
        "tab" => 15,
        "backtick" | "grave" => 41,
        "comma" => 51,
        "space" => 57,
        "backspace" => 14,
        "left" => 105,
        "right" => 106,
        "up" => 103,
        "down" => 108,
        _ => {
            let mut chars = name.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            return match c {
                '0'..='9' => DIGITS.get(c.to_digit(10)? as usize).copied(),
                'a'..='z' => LETTERS.find(c).and_then(|i| LETTER_CODES.get(i).copied()),
                _ => None,
            };
        }
    };
    Some(code)
}

/// `#rrggbb` to opaque ARGB.
fn parse_color(color: &str) -> anyhow::Result<u32> {
    let hex = color
        .strip_prefix('#')
        .filter(|h| h.len() == 6)
        .with_context(|| format!("colour {color}: expected #rrggbb"))?;
    let rgb = u32::from_str_radix(hex, 16).with_context(|| format!("colour {color}"))?;
    Ok(0xff00_0000 | rgb)
}

/// The evdev code typing `c` on a US keyboard, and whether with Shift.
fn us_key(c: char) -> Option<(u32, bool)> {
    const LETTERS: &str = "qwertyuiopasdfghjklzxcvbnm";
    const LETTER_CODES: [u32; 26] = [
        16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 30, 31, 32, 33, 34, 35, 36, 37, 38, 44, 45, 46, 47,
        48, 49, 50,
    ];
    if let Some(i) = LETTERS.find(c.to_ascii_lowercase()) {
        return Some((LETTER_CODES[i], c.is_ascii_uppercase()));
    }
    let plain = |code| Some((code, false));
    let shifted = |code| Some((code, true));
    match c {
        '1'..='9' => plain(2 + c as u32 - '1' as u32),
        '0' => plain(11),
        ' ' => plain(57),
        '-' => plain(12),
        '=' => plain(13),
        '[' => plain(26),
        ']' => plain(27),
        ';' => plain(39),
        '\'' => plain(40),
        '`' => plain(41),
        '\\' => plain(43),
        ',' => plain(51),
        '.' => plain(52),
        '/' => plain(53),
        '!' => shifted(2),
        '@' => shifted(3),
        '#' => shifted(4),
        '$' => shifted(5),
        '%' => shifted(6),
        '^' => shifted(7),
        '&' => shifted(8),
        '*' => shifted(9),
        '(' => shifted(10),
        ')' => shifted(11),
        '_' => shifted(12),
        '+' => shifted(13),
        ':' => shifted(39),
        '"' => shifted(40),
        '<' => shifted(51),
        '>' => shifted(52),
        '?' => shifted(53),
        '~' => shifted(41),
        _ => None,
    }
}

/// Initializes an MCP session, as a client does first.
fn mcp_initialize(bridge: &mut hocus_api::mcp::Bridge) {
    use serde_json::json;
    bridge.handle(json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": hocus_api::mcp::PROTOCOL_VERSIONS[0],
            "capabilities": {},
            "clientInfo": { "name": "scenario", "version": "0" },
        },
    }));
    bridge.handle(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
}

/// The PNG a tool's answer holds (a capture's).
fn mcp_image(answer: &serde_json::Value) -> anyhow::Result<Vec<u8>> {
    use base64::Engine as _;
    let data = answer["result"]["content"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["type"] == "image"))
        .and_then(|c| c["data"].as_str())
        .with_context(|| format!("no image in {answer}"))?;
    Ok(base64::engine::general_purpose::STANDARD.decode(data)?)
}

/// A tool's answer failed with `error` in its message, or succeeded; its
/// text holds `contains`.
fn mcp_check(
    answer: &serde_json::Value,
    error: Option<&str>,
    contains: &[String],
) -> anyhow::Result<()> {
    use serde_json::Value;
    let result = answer
        .get("result")
        .with_context(|| format!("not a result: {answer}"))?;
    let failed = result["isError"] == Value::Bool(true);
    let text: Vec<&str> = result["content"]
        .as_array()
        .map(|c| c.iter().filter_map(|c| c["text"].as_str()).collect())
        .unwrap_or_default();
    let text = text.join("\n");
    match error {
        Some(expected) => anyhow::ensure!(
            failed && text.contains(expected),
            "expected the tool to fail with {expected:?}, got {answer}"
        ),
        None => anyhow::ensure!(!failed, "the tool failed: {text}"),
    }
    for expected in contains {
        anyhow::ensure!(
            text.contains(expected.as_str()),
            "the result lacks {expected:?}: {text}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_as_one_key_tables() -> anyhow::Result<()> {
        let scenario = Scenario::parse(
            r##"
            description = "d"
            outputs = ["2560x1600@2"]
            [[step]]
            open = { app_id = "a", size = [400, 300], color = "#3399ff" }
            [[step]]
            pointer_move = [450, 260]
            [[step]]
            click = "left"
            [[step]]
            expect_window = { app_id = "a", geometry = [440, 250, 400, 300] }
            "##,
        )?;
        assert_eq!(scenario.steps.len(), 4);
        assert_eq!(scenario.steps[2], Step::Click(ButtonName::Left));
        Ok(())
    }

    #[test]
    fn unknown_steps_are_rejected() {
        let parsed = Scenario::parse(
            r#"
            description = "d"
            [[step]]
            fly = 1
            "#,
        );
        assert!(parsed.is_err());
    }

    #[test]
    fn keys_are_named_or_coded() -> anyhow::Result<()> {
        assert_eq!(parse_keys("super+2")?, [125, 3]);
        assert_eq!(parse_keys("Super+Shift+a")?, [125, 42, 30]);
        assert_eq!(parse_keys("ctrl+30")?, [29, 30]);
        assert!(parse_keys("super+nope").is_err());
        Ok(())
    }

    #[test]
    fn colours_are_opaque() -> anyhow::Result<()> {
        assert_eq!(parse_color("#3399ff")?, 0xff33_99ff);
        assert!(parse_color("3399ff").is_err());
        Ok(())
    }
}
