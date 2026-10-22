//! The built-in screenshot tool (#69): the screen under the pointer, every
//! screen, a window clicked, or a region drawn, at once or after a delay.
//! The capture is taken after the turn of the loop, where the renderer is;
//! a short flash shows what was taken; encoding, saving and reading text
//! (OCR) run on worker threads. The picture goes to the clipboard and to
//! the screenshots folder, and a toast offers to open it, delete it or copy
//! its text.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use hocus_api::Event;
use hocus_core::{ScreenshotMode, WindowId};
use hocus_shell::picker::{PickMode, Picker};
use smithay::{
    backend::renderer::element::{Kind, solid::SolidColorRenderElement},
    output::Output,
    reexports::calloop::{
        LoopHandle, channel,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Point, Rectangle},
};

use crate::{
    App, BackendState, State, capture::Pixels, desktops::model_id, scene::SceneElement,
    shell::Panel,
};

/// How long the flash lasts.
const FLASH: Duration = Duration::from_millis(250);

/// What is taken after the turn.
#[derive(Debug, Clone)]
enum Shot {
    Output(String),
    All,
    Window(WindowId),
    /// On one output, in its logical pixels from its corner.
    Region {
        output: String,
        rect: Rectangle<i32, Logical>,
    },
}

/// A region or window being chosen.
#[derive(Debug)]
struct Picking {
    mode: PickMode,
    panels: Vec<(String, Panel<Picker>)>,
    /// Where the region began, and on which output.
    from: Option<(Point<f64, Logical>, String)>,
}

/// What the workers send back.
#[derive(Debug)]
enum Done {
    Saved {
        path: Option<PathBuf>,
        png: Arc<[u8]>,
        width: u32,
        height: u32,
        /// tesseract is installed.
        ocr: bool,
    },
    Failed(String),
    Text(Option<String>),
}

#[derive(Debug, Default)]
pub struct Screenshots {
    wanted: Vec<Shot>,
    picking: Option<Picking>,
    /// A delayed one, due on the compositor's clock.
    due: Option<(Duration, ScreenshotMode)>,
    /// What was taken, flashing: output, local rectangle, start.
    flashes: Vec<(String, Rectangle<i32, Logical>, Duration)>,
    flash_buffer: smithay::backend::renderer::element::solid::SolidColorBuffer,
    done: Option<channel::Sender<Done>>,
    /// The shell's toasts about screenshots, and their file.
    toasts: HashMap<u32, PathBuf>,
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (done, received) = channel::channel::<Done>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(done) = event {
            app.timed("screenshot done", |app| app.state.screenshot_done(done));
        }
    });
    match inserted {
        Ok(_) => state.shots.done = Some(done),
        Err(err) => tracing::warn!(%err.error, "could not follow screenshots"),
    }
    state
        .shots
        .flash_buffer
        .update((1, 1), [1.0, 1.0, 1.0, 1.0]);
}

impl State {
    /// The screenshot action: now, after a delay, or once a region or a
    /// window is chosen.
    pub fn screenshot(&mut self, mode: ScreenshotMode, delay: u32) {
        if delay > 0 {
            self.shots.due = Some((self.clock.now() + Duration::from_secs(delay.into()), mode));
            // Wakes the loop then; the clock decides.
            if let Some(handle) = &self.loop_handle {
                let woken = handle.insert_source(
                    Timer::from_duration(Duration::from_secs(delay.into())),
                    |_, _, app| {
                        app.state.needs_redraw = true;
                        TimeoutAction::Drop
                    },
                );
                if let Err(err) = woken {
                    tracing::warn!(%err.error, "could not time the screenshot");
                }
            }
            return;
        }
        match mode {
            ScreenshotMode::Screen => {
                if let Some(output) = self.output_under_pointer() {
                    self.shots.wanted.push(Shot::Output(output.name()));
                }
            }
            ScreenshotMode::All => self.shots.wanted.push(Shot::All),
            ScreenshotMode::Region => self.start_picking(PickMode::Region),
            ScreenshotMode::Window => self.start_picking(PickMode::Window),
        }
        self.needs_redraw = true;
    }

    fn start_picking(&mut self, mode: PickMode) {
        self.start_picking_for(mode);
    }

    /// Dims the screens to choose a region or a window, for a screenshot or
    /// a recording.
    pub fn start_picking_for(&mut self, mode: PickMode) {
        let theme = self.theme();
        let lang = self.shell_lang();
        let panels = self
            .space
            .outputs()
            .filter_map(|o| {
                let g = self.space.output_geometry(o)?;
                let mut picker = Picker::new(
                    theme.clone(),
                    lang,
                    hocus_ui::Size::new(g.size.w as f32, g.size.h as f32),
                    o.current_scale().fractional_scale() as f32,
                    mode,
                );
                picker.draw(self.clock.now());
                let mut panel = Panel::new(picker);
                panel.upload(&[]);
                Some((o.name(), panel))
            })
            .collect();
        self.shots.picking = Some(Picking {
            mode,
            panels,
            from: None,
        });
        self.needs_redraw = true;
    }

    pub fn picking(&self) -> bool {
        self.shots.picking.is_some()
    }

    /// While choosing, the pointer is the picker's.
    pub fn picker_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        let Some(picking) = &self.shots.picking else {
            return false;
        };
        let mode = picking.mode;
        match (mode, pressed) {
            (PickMode::Region, Some(true)) => {
                let output = self.space.output_under(at).next().map(Output::name);
                if let (Some(output), Some(picking)) = (output, &mut self.shots.picking) {
                    picking.from = Some((at, output));
                }
            }
            (PickMode::Region, Some(false)) => {
                let from = self.shots.picking.as_ref().and_then(|p| p.from.clone());
                if let Some((from, output)) = from {
                    let rect = self.region(&output, from, at);
                    self.shots.picking = None;
                    if let Some(rect) = rect.filter(|r| r.size.w > 1 && r.size.h > 1) {
                        self.shots.wanted.push(Shot::Region { output, rect });
                    }
                }
            }
            (PickMode::Region, None) => {
                let from = self.shots.picking.as_ref().and_then(|p| p.from.clone());
                if let Some((from, output)) = from {
                    let rect = self.region(&output, from, at);
                    self.show_chosen(&output, rect);
                }
            }
            (PickMode::Window, Some(true)) => {
                let id = self.space.element_under(at).and_then(|(w, _)| model_id(w));
                self.shots.picking = None;
                if let Some(id) = id {
                    self.shots.wanted.push(Shot::Window(id));
                }
            }
            (PickMode::Window, Some(false)) => {}
            // Recording: a region drawn; a click alone takes the window
            // under it, or the screen.
            (PickMode::Record, Some(true)) => {
                let output = self.space.output_under(at).next().map(Output::name);
                if let (Some(output), Some(picking)) = (output, &mut self.shots.picking) {
                    picking.from = Some((at, output));
                }
            }
            (PickMode::Record, Some(false)) => {
                let from = self.shots.picking.as_ref().and_then(|p| p.from.clone());
                self.shots.picking = None;
                let Some((from, output)) = from else {
                    return true;
                };
                let rect = self.region(&output, from, at);
                let target = match rect {
                    Some(rect) if rect.size.w > 4 && rect.size.h > 4 => {
                        crate::recording::Target::Output {
                            name: output,
                            rect: Some(rect),
                        }
                    }
                    _ => match self.space.element_under(at).and_then(|(w, _)| model_id(w)) {
                        Some(id) => crate::recording::Target::Window(id),
                        None => crate::recording::Target::Output {
                            name: output,
                            rect: None,
                        },
                    },
                };
                self.record(target);
            }
            (PickMode::Record, None) => {
                let from = self.shots.picking.as_ref().and_then(|p| p.from.clone());
                if let Some((from, output)) = from {
                    let rect = self.region(&output, from, at);
                    self.show_chosen(&output, rect);
                }
            }
            (PickMode::Window, None) => {
                let hovered = self.space.element_under(at).and_then(|(w, loc)| {
                    let output = self.space.output_under(at).next()?.clone();
                    let g = self.space.output_geometry(&output)?;
                    let rect = Rectangle::new(loc - g.loc, w.geometry().size);
                    Some((output.name(), rect))
                });
                let names: Vec<String> = self
                    .shots
                    .picking
                    .as_ref()
                    .map(|p| p.panels.iter().map(|(n, _)| n.clone()).collect())
                    .unwrap_or_default();
                for name in names {
                    let rect = hovered
                        .as_ref()
                        .filter(|(n, _)| *n == name)
                        .map(|(_, r)| *r);
                    self.show_chosen(&name, rect);
                }
            }
        }
        self.needs_redraw = true;
        true
    }

    /// The region from `from` to `to`, on `output`, in its local logical
    /// pixels, kept within it.
    fn region(
        &self,
        output: &str,
        from: Point<f64, Logical>,
        to: Point<f64, Logical>,
    ) -> Option<Rectangle<i32, Logical>> {
        let g = self
            .find_output(output)
            .and_then(|o| self.space.output_geometry(&o))?;
        let clamp = |p: Point<f64, Logical>| {
            Point::<i32, Logical>::from((
                (p.x.round() as i32).clamp(g.loc.x, g.loc.x + g.size.w),
                (p.y.round() as i32).clamp(g.loc.y, g.loc.y + g.size.h),
            ))
        };
        let (a, b) = (clamp(from), clamp(to));
        Some(Rectangle::new(
            Point::from((a.x.min(b.x), a.y.min(b.y))) - g.loc,
            ((a.x - b.x).abs(), (a.y - b.y).abs()).into(),
        ))
    }

    fn show_chosen(&mut self, output: &str, rect: Option<Rectangle<i32, Logical>>) {
        let Some(picking) = &mut self.shots.picking else {
            return;
        };
        for (name, panel) in &mut picking.panels {
            let mine = rect.filter(|_| name == output);
            panel.view.set(mine.map(|r| {
                hocus_ui::Rect::new(
                    r.loc.x as f32,
                    r.loc.y as f32,
                    r.size.w as f32,
                    r.size.h as f32,
                )
            }));
        }
    }

    /// Escape gives up choosing.
    pub fn picker_key(&mut self, sym: smithay::input::keyboard::Keysym) -> bool {
        if self.shots.picking.is_none() {
            return false;
        }
        if sym == smithay::input::keyboard::Keysym::Escape {
            self.shots.picking = None;
            self.needs_redraw = true;
        }
        // Recording: Enter takes the screen under the pointer.
        let recording = self
            .shots
            .picking
            .as_ref()
            .is_some_and(|p| p.mode == PickMode::Record);
        if recording && sym == smithay::input::keyboard::Keysym::Return {
            self.shots.picking = None;
            if let Some(output) = self.output_under_pointer() {
                self.record(crate::recording::Target::Output {
                    name: output.name(),
                    rect: None,
                });
            }
        }
        true
    }

    /// After each dispatch: a delayed screenshot falls due, the picker and
    /// the flash draw.
    pub fn refresh_screenshots(&mut self) {
        let now = self.clock.now();
        if let Some((due, mode)) = self.shots.due
            && now >= due
        {
            self.shots.due = None;
            self.screenshot(mode, 0);
        }
        if let Some(picking) = &mut self.shots.picking {
            for (_, panel) in &mut picking.panels {
                let drawn = panel.view.draw(now);
                if panel.upload(&drawn) {
                    self.needs_redraw = true;
                }
            }
        }
        let before = self.shots.flashes.len();
        self.shots
            .flashes
            .retain(|(_, _, start)| now.saturating_sub(*start) < FLASH);
        // A flash fades frame by frame, then once more to take it away.
        if !self.shots.flashes.is_empty() || before > 0 {
            self.needs_redraw = true;
        }
    }

    /// The picker and the flash on `output`, front first.
    pub fn screenshot_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        let mut out = Vec::new();
        let scale = output.current_scale().fractional_scale();
        let now = self.clock.now();
        for (name, rect, start) in &self.shots.flashes {
            if *name != output.name() {
                continue;
            }
            let t = now.saturating_sub(*start).as_secs_f32() / FLASH.as_secs_f32();
            let alpha = (0.7 * (1.0 - t)).clamp(0.0, 1.0);
            let mut buffer = self.shots.flash_buffer.clone();
            buffer.resize(rect.size);
            out.push(SceneElement::Solid(SolidColorRenderElement::from_buffer(
                &buffer,
                rect.loc.to_physical_precise_round(scale),
                scale,
                alpha,
                Kind::Unspecified,
            )));
        }
        if let Some(picking) = &self.shots.picking
            && let Some((_, panel)) = picking.panels.iter().find(|(n, _)| *n == output.name())
            && let Some(e) = panel.element(renderer, output, Point::default())
        {
            out.push(SceneElement::Frame(e));
        }
        out
    }

    pub fn picker_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(picking) = &self.shots.picking else {
            return;
        };
        for (name, panel) in &picking.panels {
            if let Some(g) = self
                .find_output(name)
                .and_then(|o| self.space.output_geometry(&o))
            {
                crate::shell::flatten(&panel.view.access(), g.loc, 0, out);
            }
        }
    }

    fn screenshot_done(&mut self, done: Done) {
        let lang = self.shell_lang();
        match done {
            Done::Saved {
                path,
                png,
                width,
                height,
                ocr,
            } => {
                let copied = self.config.screenshots.copy;
                if copied {
                    self.copy_png(png);
                }
                let file = path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned());
                let mut actions = Vec::new();
                if path.is_some() {
                    actions.push(("open".to_owned(), lang.open_file().to_owned()));
                    if ocr {
                        actions.push(("text".to_owned(), lang.copy_text().to_owned()));
                    }
                    actions.push(("delete".to_owned(), lang.delete().to_owned()));
                }
                let id = self.shell_toast_with(
                    lang.screenshot_taken(),
                    &lang.screenshot_where(file.as_deref(), copied),
                    actions,
                );
                if let (Some(id), Some(path)) = (id, &path) {
                    self.shots.toasts.insert(id, path.clone());
                }
                self.emit(|_| Event::Screenshot {
                    path: path.as_ref().map(|p| p.display().to_string()),
                    width,
                    height,
                });
            }
            Done::Failed(err) => {
                tracing::warn!(err, "the screenshot could not be saved");
                self.shell_toast(lang.screenshot_taken(), &err);
            }
            Done::Text(Some(text)) => {
                self.copy_text(&text);
                self.shell_toast(lang.text_copied(), "");
            }
            Done::Text(None) => self.shell_toast(lang.no_text_found(), ""),
        }
    }

    /// A toast about a file: its Open and Delete act on it.
    pub fn screenshot_toast_file(&mut self, id: u32, path: PathBuf) {
        self.shots.toasts.insert(id, path);
    }

    /// A screenshot toast's action: open, delete, copy its text.
    pub fn screenshot_action(&mut self, id: u32, key: &str) -> bool {
        let Some(path) = self.shots.toasts.get(&id).cloned() else {
            return false;
        };
        match key {
            "open" => self.launch("xdg-open", &[path.display().to_string()]),
            "delete" => {
                self.shots.toasts.remove(&id);
                spawn("screenshot delete", move || {
                    if let Err(err) = std::fs::remove_file(&path) {
                        tracing::warn!(%err, path = %path.display(), "could not delete the screenshot");
                    }
                });
            }
            "text" => {
                let done = self.shots.done.clone();
                let ocr = self.config.screenshots.ocr.clone();
                spawn("screenshot text", move || {
                    let text = read_text(&ocr, &path);
                    if let Some(done) = done {
                        let _ = done.send(Done::Text(text));
                    }
                });
            }
            _ => {}
        }
        true
    }
}

impl App {
    /// After a dispatch: takes the screenshots asked for, then hands them
    /// to a worker.
    pub fn serve_screenshots(&mut self) {
        if self.state.shots.wanted.is_empty() {
            return;
        }
        let wanted = std::mem::take(&mut self.state.shots.wanted);
        for shot in wanted {
            match self.take(&shot) {
                Ok(pixels) => self.hand_over(pixels),
                Err(err) => tracing::warn!("could not take the screenshot: {err:#}"),
            }
        }
    }

    fn capture_output(&mut self, output: &Output) -> anyhow::Result<Pixels> {
        let state = &self.state;
        match &mut self.backend {
            BackendState::Winit(b) => b.capture_output(output, state, false),
            BackendState::Headless(b) => b.capture_output(output, state, false),
            BackendState::Tty(b) => b.capture_output(output, state, false),
        }
    }

    fn take(&mut self, shot: &Shot) -> anyhow::Result<Pixels> {
        let now = self.state.clock.now();
        match shot {
            Shot::Output(name) => {
                let output = self
                    .state
                    .find_output(name)
                    .ok_or_else(|| anyhow::anyhow!("no output {name}"))?;
                let size = self
                    .state
                    .space
                    .output_geometry(&output)
                    .unwrap_or_default()
                    .size;
                self.state
                    .shots
                    .flashes
                    .push((name.clone(), Rectangle::from_size(size), now));
                self.capture_output(&output)
            }
            Shot::Region { output, rect } => {
                let o = self
                    .state
                    .find_output(output)
                    .ok_or_else(|| anyhow::anyhow!("no output {output}"))?;
                let scale = o.current_scale().fractional_scale();
                self.state.shots.flashes.push((output.clone(), *rect, now));
                let whole = self.capture_output(&o)?;
                let p = rect.to_f64().to_physical(scale);
                Ok(crop(
                    &whole,
                    p.loc.x.round() as u32,
                    p.loc.y.round() as u32,
                    p.size.w.round() as u32,
                    p.size.h.round() as u32,
                ))
            }
            Shot::Window(id) => {
                let managed = self
                    .state
                    .windows
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("the window is gone"))?;
                let window = managed.window.clone();
                let output = self.state.output_of(&window);
                let scale = output
                    .as_ref()
                    .map_or(1.0, |o| o.current_scale().fractional_scale());
                if let (Some(output), Some(at)) =
                    (&output, self.state.space.element_location(&window))
                    && let Some(g) = self.state.space.output_geometry(output)
                {
                    let rect = Rectangle::new(at - g.loc, window.geometry().size);
                    self.state.shots.flashes.push((output.name(), rect, now));
                }
                let state = &self.state;
                let _ = state;
                match &mut self.backend {
                    BackendState::Winit(b) => b.capture_window(&window, scale),
                    BackendState::Headless(b) => b.capture_window(&window, scale),
                    BackendState::Tty(b) => b.capture_window(&window, scale),
                }
            }
            Shot::All => {
                let outputs: Vec<Output> = self.state.space.outputs().cloned().collect();
                let mut parts = Vec::new();
                for o in &outputs {
                    let g = self.state.space.output_geometry(o).unwrap_or_default();
                    self.state
                        .shots
                        .flashes
                        .push((o.name(), Rectangle::from_size(g.size), now));
                    parts.push((g, self.capture_output(o)?));
                }
                Ok(arrange(&parts))
            }
        }
    }

    /// Encodes, saves and reports on a worker.
    fn hand_over(&mut self, pixels: Pixels) {
        let Some(done) = self.state.shots.done.clone() else {
            return;
        };
        let config = self.state.config.screenshots.clone();
        let name = format!(
            "Screenshot {}.png",
            self.state.wall_civil().strftime("%Y-%m-%d %H-%M-%S")
        );
        spawn("screenshot", move || {
            let (width, height) = (pixels.width, pixels.height);
            let png: Arc<[u8]> = match pixels.encode_png() {
                Ok(png) => png.into(),
                Err(err) => {
                    let _ = done.send(Done::Failed(format!("{err:#}")));
                    return;
                }
            };
            let path = if config.save {
                match save(&folder(&config.folder), &name, &png) {
                    Ok(path) => Some(path),
                    Err(err) => {
                        let _ = done.send(Done::Failed(err));
                        return;
                    }
                }
            } else {
                None
            };
            let _ = done.send(Done::Saved {
                path,
                png,
                width,
                height,
                ocr: on_path(&config.ocr),
            });
        });
    }
}

/// A part of `whole`, kept within it.
pub fn crop(whole: &Pixels, x: u32, y: u32, w: u32, h: u32) -> Pixels {
    let x = x.min(whole.width);
    let y = y.min(whole.height);
    let w = w.min(whole.width - x);
    let h = h.min(whole.height - y);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * whole.width + x) * 4) as usize;
        rgba.extend_from_slice(&whole.rgba[start..start + (w * 4) as usize]);
    }
    Pixels {
        width: w,
        height: h,
        rgba,
    }
}

/// Every output in one picture, as they are arranged, each at its own
/// pixel density: an output's pixels go where its logical corner is at the
/// highest scale; a lower-density output keeps its own size.
fn arrange(parts: &[(Rectangle<i32, Logical>, Pixels)]) -> Pixels {
    let scale = parts
        .iter()
        .map(|(g, p)| f64::from(p.width) / f64::from(g.size.w.max(1)))
        .fold(1.0_f64, f64::max);
    let left = parts.iter().map(|(g, _)| g.loc.x).min().unwrap_or(0);
    let top = parts.iter().map(|(g, _)| g.loc.y).min().unwrap_or(0);
    let place = |v: i32, from: i32| (f64::from(v - from) * scale).round() as u32;
    let width = parts
        .iter()
        .map(|(g, p)| place(g.loc.x, left) + p.width)
        .max()
        .unwrap_or(0);
    let height = parts
        .iter()
        .map(|(g, p)| place(g.loc.y, top) + p.height)
        .max()
        .unwrap_or(0);
    let mut rgba = vec![0; (width * height * 4) as usize];
    for (g, p) in parts {
        let (x, y) = (place(g.loc.x, left), place(g.loc.y, top));
        for row in 0..p.height {
            let from = (row * p.width * 4) as usize;
            let to = (((y + row) * width + x) * 4) as usize;
            rgba[to..to + (p.width * 4) as usize]
                .copy_from_slice(&p.rgba[from..from + (p.width * 4) as usize]);
        }
    }
    Pixels {
        width,
        height,
        rgba,
    }
}

/// Where screenshots go: the configured folder, else `Screenshots` in the
/// pictures folder (`XDG_PICTURES_DIR`, else `~/Pictures`).
fn folder(configured: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    if !configured.is_empty() {
        return match configured.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(configured),
        };
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let pictures = std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|dirs| {
            dirs.lines().find_map(|l| {
                let v = l.strip_prefix("XDG_PICTURES_DIR=")?.trim_matches('"');
                Some(PathBuf::from(v.replace("$HOME", &home.to_string_lossy())))
            })
        })
        .unwrap_or_else(|| home.join("Pictures"));
    pictures.join("Screenshots")
}

/// Writes the file, never over another: a number is added when the name is
/// taken.
fn save(dir: &Path, name: &str, png: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = name.trim_end_matches(".png");
    for n in 1..1000 {
        let path = if n == 1 {
            dir.join(name)
        } else {
            dir.join(format!("{stem} ({n}).png"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(png)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                return Ok(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(format!("{}: {err}", path.display())),
        }
    }
    Err(format!("{}: no free name", dir.display()))
}

/// Whether `program` runs: a path to it, or a name on `PATH`.
fn on_path(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// The text in a picture, through tesseract or what replaces it; `None`
/// when there is none.
fn read_text(program: &str, path: &Path) -> Option<String> {
    let out = Command::new(program).arg(path).arg("-").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !text.is_empty()).then_some(text)
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%err, "could not start a thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> Pixels {
        Pixels {
            width: w,
            height: h,
            rgba: vec![v; (w * h * 4) as usize],
        }
    }

    #[test]
    fn screens_keep_their_own_density_in_the_arrangement() {
        // A laptop at scale 2 (1280×800 logical), an external screen at 1
        // on its right.
        let laptop = (
            Rectangle::new((0, 0).into(), (1280, 800).into()),
            solid(2560, 1600, 10),
        );
        let external = (
            Rectangle::new((1280, 0).into(), (1920, 1080).into()),
            solid(1920, 1080, 200),
        );
        let all = arrange(&[laptop, external]);
        assert_eq!((all.width, all.height), (2560 + 1920, 1600));
        let at = |x: u32, y: u32| all.rgba[((y * all.width + x) * 4) as usize];
        assert_eq!(at(10, 10), 10);
        assert_eq!(at(2560 + 10, 10), 200);
        // Below the external screen's pixels: nothing.
        assert_eq!(at(2560 + 10, 1200), 0);
    }

    #[test]
    fn a_region_is_cut_within_the_picture() {
        let mut p = solid(100, 50, 0);
        p.rgba[((10 * 100 + 20) * 4) as usize] = 99;
        let c = crop(&p, 20, 10, 30, 20);
        assert_eq!((c.width, c.height), (30, 20));
        assert_eq!(c.rgba[0], 99);
        assert_eq!(crop(&p, 90, 40, 30, 20).width, 10);
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let dir = tempfile::tempdir().expect("a directory");
        let a = save(dir.path(), "Screenshot.png", b"a").expect("saved");
        let b = save(dir.path(), "Screenshot.png", b"b").expect("saved");
        assert_eq!(
            a.file_name().and_then(|n| n.to_str()),
            Some("Screenshot.png")
        );
        assert_eq!(
            b.file_name().and_then(|n| n.to_str()),
            Some("Screenshot (2).png")
        );
    }
}
