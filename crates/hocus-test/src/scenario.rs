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
use hocus_compositor::{FRAME, Headless, HeadlessConfig, OutputSpec, RendererChoice};
use serde::Deserialize;

use crate::{
    client::{SolidWindow, TestClient},
    image::{Difference, Image, Tolerance},
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
    /// Asks the window of this app id to close, and waits until it is gone.
    Close(String),
    PointerMove([f64; 2]),
    /// Presses and releases a button.
    Click(ButtonName),
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
    ExpectScreenshot(ExpectScreenshot),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenWindow {
    pub app_id: String,
    pub size: [u32; 2],
    /// `#rrggbb`.
    pub color: String,
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
    /// `normal`, `maximized`, `fullscreen` or `minimized`.
    pub state: Option<WindowState>,
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
            Self::Close(app_id) => write!(f, "close {app_id}"),
            Self::PointerMove([x, y]) => write!(f, "pointer_move {x},{y}"),
            Self::Click(button) => write!(f, "click {button:?}"),
            Self::Key(code) => write!(f, "key {code}"),
            Self::Keys(keys) => write!(f, "keys {keys}"),
            Self::Plug(plug) => write!(f, "plug {}", plug.spec),
            Self::Unplug(name) => write!(f, "unplug {name}"),
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
        let compositor = Headless::start(HeadlessConfig {
            outputs,
            // The same pixels on every machine, for reference screenshots.
            renderer: RendererChoice::Pixman,
        })?;
        let mut run = Run {
            scenario: self,
            options,
            compositor,
            clients: Vec::new(),
        };
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
}

impl Run<'_> {
    fn step(&mut self, step: &Step) -> anyhow::Result<()> {
        match step {
            Step::Open(open) => self.open(open),
            Step::Close(app_id) => self.close(app_id),
            Step::PointerMove([x, y]) => {
                self.request(Request::PointerMove { x: *x, y: *y })?;
                Ok(())
            }
            Step::Click(button) => {
                let button = match button {
                    ButtonName::Left => Button::Left,
                    ButtonName::Right => Button::Right,
                    ButtonName::Middle => Button::Middle,
                };
                for pressed in [true, false] {
                    self.request(Request::PointerButton { button, pressed })?;
                }
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
            Step::Unplug(name) => {
                self.request(Request::UnplugOutput {
                    output: name.clone(),
                })?;
                Ok(())
            }
            Step::AdvanceMs(ms) => self.compositor.advance(Duration::from_millis(*ms)),
            Step::Frames(n) => self.compositor.advance(FRAME * *n),
            Step::ExpectWindow(expected) => self.expect_window(expected),
            Step::ExpectNoWindow(app_id) => match self.find(app_id)? {
                None => Ok(()),
                Some(window) => anyhow::bail!("expected no window {app_id}, found {window:?}"),
            },
            Step::ExpectFocus(app_id) => {
                let focused = self.windows()?.into_iter().find(|w| w.focused);
                let focused = focused.and_then(|w| w.app_id);
                anyhow::ensure!(
                    focused.as_deref() == Some(app_id.as_str()),
                    "focus: expected {app_id}, got {}",
                    focused.as_deref().unwrap_or("nothing")
                );
                Ok(())
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
        self.clients
            .push(window.spawn(self.compositor.socket_path())?);
        self.compositor
            .dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() > before)
            .context("the window did not appear")
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
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        loop {
            let wrong = self.window_differences(expected)?;
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
        if let Some(state) = expected.state
            && window.state != state
        {
            wrong.push(format!("state: expected {state:?}, got {:?}", window.state));
        }
        Ok(wrong)
    }

    fn expect_received(&mut self, expected: &ExpectReceived) -> anyhow::Result<()> {
        let index = self
            .clients
            .iter()
            .position(|c| c.spec.app_id == expected.app_id)
            .with_context(|| format!("no client {}", expected.app_id))?;
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        // The client reads its events in its own thread.
        loop {
            let events = self.clients[index].events();
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

    fn expect_screenshot(&mut self, expected: &ExpectScreenshot) -> anyhow::Result<()> {
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
