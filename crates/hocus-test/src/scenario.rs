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
use hocus_api::{Button, Reply, Request, Target, WindowInfo};
use hocus_compositor::{FRAME, Headless, HeadlessConfig, OutputSpec, RendererChoice};
use serde::Deserialize;

use crate::{
    client::{SolidWindow, TestClient},
    image::{Difference, Image, Tolerance},
};

/// How long a step may wait for a client, in real time.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// Moves the virtual clock forward.
    AdvanceMs(u64),
    /// Moves the virtual clock forward by whole frames.
    Frames(u32),
    ExpectWindow(ExpectWindow),
    ExpectNoWindow(String),
    /// The app id of the focused window.
    ExpectFocus(String),
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
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(open) => write!(f, "open {}", open.app_id),
            Self::Close(app_id) => write!(f, "close {app_id}"),
            Self::PointerMove([x, y]) => write!(f, "pointer_move {x},{y}"),
            Self::Click(button) => write!(f, "click {button:?}"),
            Self::Key(code) => write!(f, "key {code}"),
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

    fn expect_window(&mut self, expected: &ExpectWindow) -> anyhow::Result<()> {
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
        anyhow::ensure!(wrong.is_empty(), "{}", wrong.join("; "));
        Ok(())
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
    fn colours_are_opaque() -> anyhow::Result<()> {
        assert_eq!(parse_color("#3399ff")?, 0xff33_99ff);
        assert!(parse_color("3399ff").is_err());
        Ok(())
    }
}
