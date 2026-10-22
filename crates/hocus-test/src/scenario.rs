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
    /// Programs the scenario runs; it is skipped where one is missing.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Where the launcher reads desktop entries, relative to the scenario.
    #[serde(default)]
    pub data_dirs: Vec<PathBuf>,
    /// What started applications inherit (`PATH` is added if missing),
    /// instead of the test's own environment.
    pub child_env: Option<std::collections::BTreeMap<String, String>>,
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
    /// Types text, as on a US keyboard.
    Type(String),
    /// A file a started program wrote in the compositor's runtime
    /// directory: `{ name, contains?, lacks? }`.
    ExpectRuntimeFile(ExpectFile),
    /// Runs `notify-send` with these arguments on the private bus.
    NotifySend(Vec<String>),
    /// Sends a notification on the private bus.
    Notify(NotifyStep),
    /// Clicks the shell's button with this label (or accessible name).
    ClickUi(String),
    /// The shell shows an element with this label: `{ label, disabled? }`.
    ExpectUi(ExpectUi),
    /// A screen sharing tool starts copying the first output again and
    /// again (wlr-screencopy with damage).
    Share(bool),
    /// It stops.
    StopSharing(bool),
    /// Starts a layer-shell client: `{ namespace, layer, anchor?, size,
    /// exclusive_zone?, keyboard?, color }`; waits until it is shown.
    OpenLayer(crate::layer::LayerSpec),
    /// Closes the layer client of this namespace.
    CloseLayer(String),
    /// The client of this app id asks for its window to come forward,
    /// with an activation token of its own.
    Activate(String),
    /// Starts the settings app on a page (or its home), and waits for its
    /// window: `{ page? }`.
    OpenSettings(OpenSettings),
    /// The settings window shows an element: `{ label, focused?,
    /// selected?, shown? }`.
    ExpectSettings(ExpectSettings),
    /// Clicks the element of the settings window with this label.
    ClickSettings(String),
    /// The configuration file holds these strings, or lacks them.
    ExpectConfigFile(ExpectConfigFile),
    /// Asks the window of this app id to close, and waits until it is gone.
    Close(String),
    PointerMove([f64; 2]),
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
    /// App ids of all windows, bottom to top.
    ExpectStack(Vec<String>),
    /// The client got this event, e.g. `"key 30 pressed"`.
    ExpectReceived(ExpectReceived),
    /// The client never got this event.
    ExpectNotReceived(ExpectReceived),
    ExpectScreenshot(ExpectScreenshot),
    /// One pixel of an output's screenshot, in its pixels.
    ExpectPixel(ExpectPixel),
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
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectAccent {
    pub hue: [f32; 2],
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
pub struct Clipboard {
    pub app_id: String,
    pub text: String,
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
pub struct ExpectShown {
    pub output: String,
    pub desktop: usize,
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
            Self::OpenSettings(o) => write!(f, "open_settings {:?}", o.page),
            Self::Activate(app_id) => write!(f, "activate {app_id}"),
            Self::Share(_) => write!(f, "share"),
            Self::StopSharing(_) => write!(f, "stop_sharing"),
            Self::OpenLayer(spec) => write!(f, "open_layer {}", spec.namespace),
            Self::CloseLayer(namespace) => write!(f, "close_layer {namespace}"),
            Self::ExpectSettings(e) => write!(f, "expect_settings {:?}", e.label),
            Self::ClickSettings(label) => write!(f, "click_settings {label:?}"),
            Self::ExpectConfigFile(_) => write!(f, "expect_config_file"),
            Self::Type(text) => write!(f, "type {text:?}"),
            Self::ExpectAccent(e) => write!(f, "expect_accent {:?}", e.hue),
            Self::ExpectRuntimeFile(e) => write!(f, "expect_runtime_file {}", e.name),
            Self::NotifySend(args) => write!(f, "notify_send {args:?}"),
            Self::Notify(n) => write!(f, "notify {} {:?}", n.app, n.summary),
            Self::ExpectUi(e) => write!(f, "expect_ui {:?}", e.label),
            Self::Copy(c) => write!(f, "copy {} {:?}", c.app_id, c.text),
            Self::ExpectClipboard(c) => write!(f, "expect_clipboard {} {:?}", c.app_id, c.text),
            Self::Close(app_id) => write!(f, "close {app_id}"),
            Self::PointerMove([x, y]) => write!(f, "pointer_move {x},{y}"),
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
            Self::Config(_) => write!(f, "config"),
            Self::ExpectConfigError(e) => write!(f, "expect_config_error {e:?}"),
            Self::ExpectConfigWarning(e) => write!(f, "expect_config_warning {e:?}"),
            Self::ExpectEvent(e) => write!(f, "expect_event {}", e.kind),
            Self::SetState(s) => write!(f, "set_state {} {:?}", s.app_id, s.state),
            Self::Send(send) => write!(f, "send {} to {}", send.app_id, send.output),
            Self::ExpectShown(e) => write!(f, "expect_shown {} {}", e.output, e.desktop),
            Self::AdvanceMs(ms) => write!(f, "advance {ms} ms"),
            Self::Frames(n) => write!(f, "frames {n}"),
            Self::ExpectWindow(e) => write!(f, "expect_window {}", e.app_id),
            Self::ExpectNoWindow(app_id) => write!(f, "expect_no_window {app_id}"),
            Self::ExpectFocus(app_id) => write!(f, "expect_focus {app_id}"),
            Self::ExpectStack(_) => write!(f, "expect_stack"),
            Self::ExpectReceived(e) => write!(f, "expect_received {} `{}`", e.app_id, e.event),
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
        let bus = if self.dbus {
            Some(crate::bus::PrivateBus::start()?)
        } else {
            None
        };
        let compositor = Headless::start_options(HarnessOptions {
            headless: HeadlessConfig {
                outputs,
                // The same pixels on every machine, for reference screenshots.
                renderer: RendererChoice::Pixman,
            },
            api_permission: hocus_api::Permission::Test,
            watchdog: None,
            xwayland: self.xwayland,
            shell: self.shell,
            session_bus: bus.as_ref().map(|b| b.address.clone()),
            data_dirs: (!self.data_dirs.is_empty())
                .then(|| self.data_dirs.iter().map(|d| self.dir.join(d)).collect()),
            child_env: self.child_env.as_ref().map(|env| {
                let mut env = env.clone();
                env.entry("PATH".into())
                    .or_insert_with(|| std::env::var("PATH").unwrap_or_default());
                env.into_iter().map(|(k, v)| (k.into(), v.into())).collect()
            }),
        })?;
        let mut run = Run {
            scenario: self,
            options,
            compositor,
            clients: Vec::new(),
            x11_clients: Vec::new(),
            settings: None,
            layers: Vec::new(),
            sharing: None,
            bus,
            events: Vec::new(),
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
    settings: Option<hocus_settings::client::Running>,
    layers: Vec<crate::layer::LayerClient>,
    sharing: Option<(
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::thread::JoinHandle<anyhow::Result<()>>,
    )>,
    bus: Option<crate::bus::PrivateBus>,
    /// Every event since the start.
    events: Vec<hocus_api::Event>,
}

impl Run<'_> {
    fn step(&mut self, step: &Step) -> anyhow::Result<()> {
        match step {
            Step::Open(open) => self.open(open),
            Step::OpenX11(open) => self.open_x11(open),
            Step::Copy(copy) => self.copy(copy),
            Step::ExpectClipboard(expected) => self.expect_clipboard(expected),
            Step::ClickUi(label) => self.click_ui(label),
            Step::OpenSettings(open) => self.open_settings(open),
            Step::Share(_) => {
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let socket = self.compositor.socket_path().to_path_buf();
                let thread = std::thread::spawn({
                    let stop = stop.clone();
                    move || crate::screencopy::share(&socket, stop)
                });
                self.sharing = Some((stop, thread));
                // Its first copy is asked for.
                let until = Instant::now() + Duration::from_millis(100);
                while Instant::now() < until {
                    self.compositor.dispatch(Duration::from_millis(5))?;
                }
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
            Step::ExpectSettings(expected) => self.expect_settings(expected),
            Step::ClickSettings(label) => self.click_settings(label),
            Step::ExpectConfigFile(expected) => self.expect_config_file(expected),
            Step::Type(text) => self.type_text(text),
            Step::ExpectRuntimeFile(expected) => self.expect_runtime_file(expected),
            Step::NotifySend(args) => self.notify_send(args),
            Step::Notify(n) => self.notify(n),
            Step::ExpectUi(expected) => self.expect_ui(expected),
            Step::Close(app_id) => self.close(app_id),
            Step::PointerMove([x, y]) => {
                self.request(Request::PointerMove { x: *x, y: *y })?;
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
                if let Some(client) = &self.options.test_client {
                    text = text.replace("{test_client}", &client.to_string_lossy());
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
                // Events arrive with replies: one request brings the latest.
                self.request(Request::Version)?;
                self.events.extend(self.compositor.take_events());
                let wanted_type = format!("\"type\":\"{}\"", expected.kind);
                let found = self.events.iter().any(|e| {
                    let json = serde_json::to_string(e).unwrap_or_default();
                    json.contains(&wanted_type)
                        && expected
                            .has
                            .as_ref()
                            .is_none_or(|h| json.contains(h.as_str()))
                });
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
        let before = self.compositor.window_count();
        self.clients.push(window.spawn_with(
            self.compositor.socket_path(),
            crate::client::ClientOptions {
                inhibit_shortcuts: open.inhibit_shortcuts,
            },
        )?);
        self.compositor
            .dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() > before)
            .context("the window did not appear")
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
        let access = self
            .settings
            .as_ref()
            .and_then(|s| s.access())
            .context("no settings window; open_settings first")?;
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

    fn click_settings(&mut self, label: &str) -> anyhow::Result<()> {
        let (node, all) = self.settings_node(label)?;
        let node =
            node.with_context(|| format!("no {label:?}; the window shows:\n{}", all.dump()))?;
        let window = self.find("hocus-settings")?.context("no settings window")?;
        let g = window.geometry;
        let (x, y) = (
            f64::from(g.x) + f64::from(node.rect.x + node.rect.w / 2.0),
            f64::from(g.y) + f64::from(node.rect.y + node.rect.h / 2.0),
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

    fn click_ui(&mut self, label: &str) -> anyhow::Result<()> {
        let nodes = self.ui()?;
        let node = nodes
            .iter()
            .find(|n| n.role == "button" && n.label.as_deref() == Some(label))
            .with_context(|| {
                let buttons: Vec<_> = nodes
                    .iter()
                    .filter(|n| n.role == "button")
                    .filter_map(|n| n.label.clone())
                    .collect();
                format!("no button {label:?}; there are {buttons:?}")
            })?;
        let r = node.rect;
        let (x, y) = (
            f64::from(r.x) + f64::from(r.width) / 2.0,
            f64::from(r.y) + f64::from(r.height) / 2.0,
        );
        self.request(Request::PointerMove { x, y })?;
        for pressed in [true, false] {
            self.request(Request::PointerButton {
                button: Button::Left,
                pressed,
            })?;
        }
        Ok(())
    }

    /// The shell redraws after each dispatch: it gets a moment.
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
        if let Some(client) = self.clients.iter().find(|c| c.spec.app_id == copy.app_id) {
            client.copy(&copy.text)?;
        } else if let Some(client) = self
            .x11_clients
            .iter()
            .find(|c| c.spec.app_id == copy.app_id)
        {
            client.copy(&copy.text)?;
        } else {
            anyhow::bail!("no client {}", copy.app_id);
        }
        // The compositor hears of it before the next step.
        self.compositor.dispatch(Duration::from_millis(50))?;
        Ok(())
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
            .find(&expected.app_id)?
            .with_context(|| format!("no window {}", expected.app_id))?;
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
        let reply = self.request(Request::Screenshot {
            target: Target::Output(expected.output.clone()),
            cursor: expected.cursor,
        })?;
        let Reply::Screenshot(shot) = reply else {
            anyhow::bail!("expected a screenshot, got {reply:?}");
        };
        let actual = Image::from_png(&shot.png()?)?;
        let reference = self.scenario.dir.join(&expected.reference);
        if self.options.bless {
            actual.save(&reference)?;
            return Ok(());
        }
        let stem = expected
            .reference
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "screenshot".into());
        let artifacts = self.options.artifacts.join(&self.scenario.name);
        let actual_path = artifacts.join(format!("{stem}.actual.png"));
        if !reference.exists() {
            actual.save(&actual_path)?;
            anyhow::bail!(
                "no reference image at {}; the actual one is in {}. Run with HOCUS_BLESS=1 to \
                 record it",
                reference.display(),
                actual_path.display()
            );
        }
        let wanted = Image::load(&reference)?;
        let tolerance = Tolerance {
            channel: expected
                .channel_tolerance
                .unwrap_or(Tolerance::default().channel),
            pixels: expected
                .pixel_tolerance
                .unwrap_or(Tolerance::default().pixels),
        };
        let Some(difference) = actual.compare(&wanted, tolerance) else {
            return Ok(());
        };
        actual.save(&actual_path)?;
        wanted.save(&artifacts.join(format!("{stem}.expected.png")))?;
        if let Difference::Pixels { diff, .. } = &difference {
            diff.save(&artifacts.join(format!("{stem}.diff.png")))?;
        }
        anyhow::bail!(
            "screenshot of {} differs from {}: {difference}; images in {}",
            expected.output,
            reference.display(),
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
        _ => None,
    }
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
