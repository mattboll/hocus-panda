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
pub mod server;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use client::{Client, ClientError};
pub use server::{ConnectionId, Handler, Server};

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
}

impl Request {
    pub fn permission(&self) -> Permission {
        match self {
            Self::Version
            | Self::Outputs
            | Self::Screens
            | Self::IdentifyOutputs
            | Self::PickShare
            // A picture the user asked to copy.
            | Self::CopyPng { .. }
            | Self::Windows
            | Self::Subscribe
            | Self::Stats
            | Self::Config
            | Self::Desktops
            | Self::Ui
            | Self::Screenshot { .. }
            // Showing the settings window changes nothing by itself.
            | Self::OpenSettings { .. } => Permission::Observe,
            Self::Scene
            | Self::PointerMove { .. }
            | Self::PointerButton { .. }
            | Self::PointerScroll { .. }
            | Self::Key { .. }
            | Self::CloseWindow { .. }
            | Self::SetWindowState { .. }
            | Self::SnapWindow { .. }
            | Self::ShowDesktop { .. }
            | Self::SendWindow { .. }
            | Self::PlugOutput { .. }
            | Self::UnplugOutput { .. }
            | Self::StallOutput { .. }
            | Self::SetPrimaryOutput { .. }
            | Self::SaveSession
            | Self::OfferSession => Permission::Test,
        }
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
    Windows(Vec<WindowInfo>),
    Scene(Scene),
    Screenshot(Screenshot),
    Stats(Stats),
    Desktops(Vec<OutputDesktops>),
    Config(ConfigInfo),
    Ui(Vec<UiNode>),
    /// The request was carried out.
    Done,
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
}

/// How well the compositor keeps up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub outputs: Vec<OutputStats>,
    /// The latest loop handlers that went over their budget, oldest first.
    pub slow_handlers: Vec<SlowHandler>,
    /// Times the loop did not answer the watchdog in time, oldest first.
    pub stalls: Vec<Stall>,
}

/// Time spent drawing the frames an output presented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputStats {
    pub name: String,
    pub frames: u64,
    pub histogram: Vec<Bucket>,
    pub max_ms: f64,
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
    fn the_control_socket_sits_next_to_the_wayland_one() {
        assert_eq!(
            socket_for(Path::new("/run/user/1000/hocus-0")),
            Path::new("/run/user/1000/hocus-0.api")
        );
    }
}
