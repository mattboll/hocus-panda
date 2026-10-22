//! Screen recording (#70): a region drawn, a window clicked or the whole
//! screen, after a countdown. A red frame marks what is recorded and a
//! small bar pauses or stops it; neither is in the video. Clicks show as
//! rings in the video only.
//!
//! Frames are taken after the turn of the loop, at most `fps` a second and
//! only when the screen drew something new; they go, owned, to a worker
//! thread that feeds an encoder process (ffmpeg) at a steady rate,
//! repeating the last frame while nothing changes. A frame the worker has
//! no room for is dropped: the loop never waits for it.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use hocus_api::Event;
use hocus_core::WindowId;
use hocus_shell::recording::{BarEvent, BarState, RED, RecordBar};
use smithay::{
    backend::renderer::element::{Kind, solid::SolidColorRenderElement},
    output::Output,
    reexports::calloop::{
        LoopHandle, channel,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Point, Rectangle},
};

use crate::{App, State, capture::Pixels, media, scene::SceneElement, shell::Panel};

/// Frames waiting for the worker, at most.
const QUEUE: usize = 4;
/// How long a click's ring shows.
const RING: Duration = Duration::from_millis(600);
/// The red frame's width, logical pixels.
const FRAME: i32 = 3;

/// What is recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// An output, or a region of it in its logical pixels from its corner.
    Output {
        name: String,
        rect: Option<Rectangle<i32, Logical>>,
    },
    Window(WindowId),
}

/// Where the recording is.
#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    Counting {
        target: Target,
        until: Duration,
    },
    Recording(Box<Running>),
    /// The encoder finishes the file.
    Finishing,
}

#[derive(Debug)]
struct Running {
    target: Target,
    feed: Sender<Feed>,
    /// Frames sent and not yet taken by the worker.
    in_flight: Arc<AtomicUsize>,
    /// The video's size, pixels.
    size: (u32, u32),
    started: Duration,
    paused_total: Duration,
    paused_since: Option<Duration>,
    /// The last frame taken: when, and the output's frame it showed.
    last: Option<(Duration, u64)>,
    /// Clicks, where (global) and when.
    clicks: Vec<(Point<f64, Logical>, Duration)>,
    /// The keys pressed are drawn in the video.
    show_keys: bool,
    /// The video's pixels to a logical one.
    scale: f64,
}

/// Where the webcam's bubble is: its mark on the screen, outside the
/// video, that the pointer moves within what is recorded.
struct Bubble {
    output: String,
    /// Its corner, logical, from the output's.
    place: Point<i32, Logical>,
    diameter: i32,
    panel: Panel<hocus_shell::recording::BubbleMark>,
    /// While dragged: where the pointer took it, from its corner.
    held: Option<Point<f64, Logical>>,
}

impl Running {
    /// Time in the video: paused time does not count.
    fn elapsed(&self, now: Duration) -> Duration {
        let paused = self.paused_total + self.paused_since.map_or(Duration::ZERO, |p| now - p);
        now.saturating_sub(self.started).saturating_sub(paused)
    }
}

/// What the worker gets.
#[derive(Debug)]
enum Feed {
    Frame {
        at: Duration,
        pixels: Pixels,
        rings: Vec<Ring>,
    },
    Stop {
        at: Duration,
    },
    /// Keys pressed, drawn from `at` in the video.
    Keys {
        at: Duration,
        caps: hocus_shell::recording::Overlay,
    },
    /// The webcam's bubble moved: its corner, video pixels.
    Bubble {
        x: u32,
        y: u32,
    },
    /// The sound stops while paused, and starts again.
    Pause {
        at: Duration,
    },
    Resume,
}

/// A click's ring in a frame: its centre in pixels, how far it has grown
/// (0 to 1), and the output's scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ring {
    pub x: f32,
    pub y: f32,
    pub age: f32,
    pub scale: f32,
}

/// What the worker says once the file is done.
#[derive(Debug)]
enum Finished {
    Saved {
        path: PathBuf,
        width: u32,
        height: u32,
    },
    Failed(String),
}

#[derive(Default)]
pub struct Recorder {
    phase: Phase,
    /// The bar: on which output, where (logical, from its corner).
    bar: Option<(String, Point<i32, Logical>, Panel<RecordBar>)>,
    /// Frames are being taken: the frame and the bar stay out.
    pub capturing: bool,
    pressed: bool,
    done: Option<channel::Sender<Finished>>,
    /// The red of the frame.
    red: smithay::backend::renderer::element::solid::SolidColorBuffer,
    /// What the next recording takes besides the screen; from the
    /// configuration, changed in the picker for the session.
    pub options: hocus_shell::picker::RecordOptions,
    /// The configuration's options they came from.
    options_from: Option<[bool; 4]>,
    bubble: Option<Bubble>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("phase", &self.phase)
            .finish_non_exhaustive()
    }
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (done, received) = channel::channel::<Finished>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(finished) = event {
            app.timed("recording done", |app| {
                app.state.recording_finished(finished)
            });
        }
    });
    match inserted {
        Ok(_) => state.recorder.done = Some(done),
        Err(err) => tracing::warn!(%err.error, "no screen recording"),
    }
    let c = RED;
    let f = |v: u8| f32::from(v) / 255.0;
    state
        .recorder
        .red
        .update((1, 1), [f(c.r), f(c.g), f(c.b), 1.0]);
}

impl crate::shell::Pixels for RecordBar {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

impl crate::shell::Pixels for hocus_shell::recording::BubbleMark {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

impl State {
    /// The record action: picks what to record, gives up the countdown, or
    /// stops.
    pub fn toggle_recording(&mut self) {
        match &self.recorder.phase {
            Phase::Idle if !self.picking() => {
                self.refresh_record_options();
                self.start_picking_for(hocus_shell::picker::PickMode::Record);
            }
            Phase::Idle | Phase::Finishing => {}
            Phase::Counting { .. } => {
                self.recorder.phase = Phase::Idle;
                self.recorder.bar = None;
                self.needs_redraw = true;
            }
            Phase::Recording(_) => self.stop_recording(),
        }
    }

    /// The options as the configuration gives them, when it changed; the
    /// camera offered only where there is one.
    fn refresh_record_options(&mut self) {
        let r = &self.config.recording;
        let wanted = [r.microphone, r.system_sound, r.camera, r.show_keys];
        if self.recorder.options_from != Some(wanted) {
            self.recorder.options_from = Some(wanted);
            self.recorder.options = hocus_shell::picker::RecordOptions {
                microphone: r.microphone,
                system_sound: r.system_sound,
                camera: Some(r.camera),
                show_keys: r.show_keys,
            };
        }
        // One look at the device, when asked to record.
        let here = r.camera_input != "v4l2" || Path::new(&r.camera_device).exists();
        let o = &mut self.recorder.options;
        o.camera = match (here, o.camera) {
            (false, _) => None,
            (true, Some(on)) => Some(on),
            (true, None) => Some(r.camera),
        };
    }

    /// An option toggled in the picker.
    pub fn toggle_record_option(&mut self, option: hocus_shell::picker::RecordOption) {
        self.recorder.options.toggle(option);
    }

    pub fn recording(&self) -> bool {
        matches!(
            self.recorder.phase,
            Phase::Counting { .. } | Phase::Recording(_)
        )
    }

    /// What to record is chosen: the countdown starts.
    pub fn record(&mut self, target: Target) {
        let now = self.clock.now();
        let countdown = Duration::from_secs(self.config.recording.countdown.into());
        self.recorder.phase = Phase::Counting {
            target: target.clone(),
            until: now + countdown,
        };
        self.place_bar(&target);
        self.tick_each_second();
        self.needs_redraw = true;
    }

    /// Wakes the loop each second while recording: the countdown and the
    /// time move on.
    fn tick_each_second(&mut self) {
        let Some(handle) = &self.loop_handle else {
            return;
        };
        let ticking =
            handle.insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, app| {
                if app.state.recording() {
                    app.state.needs_redraw = true;
                    TimeoutAction::ToDuration(Duration::from_secs(1))
                } else {
                    TimeoutAction::Drop
                }
            });
        if let Err(err) = ticking {
            tracing::warn!(%err.error, "the recording's time will not move");
        }
    }

    /// The bar under what is recorded, else over it, else inside.
    fn place_bar(&mut self, target: &Target) {
        let Some((output, rect)) = self.target_place(target) else {
            return;
        };
        let Some(o) = self.find_output(&output) else {
            return;
        };
        let area = self.space.output_geometry(&o).unwrap_or_default();
        let size = hocus_shell::recording::SIZE;
        let (w, h) = (size.w as i32, size.h as i32);
        let x = (rect.loc.x + rect.size.w / 2 - w / 2).clamp(0, (area.size.w - w).max(0));
        let below = rect.loc.y + rect.size.h + FRAME;
        let above = rect.loc.y - FRAME - h;
        let y = if below + h <= area.size.h {
            below
        } else if above >= 0 {
            above
        } else {
            12
        };
        let mut bar = RecordBar::new(
            self.theme(),
            self.shell_lang(),
            o.current_scale().fractional_scale() as f32,
        );
        let drawn = bar.draw(self.clock.now());
        let mut panel = Panel::new(bar);
        panel.upload(&drawn);
        self.recorder.bar = Some((output, Point::from((x, y)), panel));
    }

    /// Where `target` is now: its output and its rectangle there, logical.
    fn target_place(&self, target: &Target) -> Option<(String, Rectangle<i32, Logical>)> {
        match target {
            Target::Output { name, rect } => {
                let o = self.find_output(name)?;
                let area = self.space.output_geometry(&o)?;
                Some((
                    name.clone(),
                    rect.unwrap_or(Rectangle::from_size(area.size)),
                ))
            }
            Target::Window(id) => {
                let window = self.windows.get(id)?.window.clone();
                let output = self.output_of(&window)?;
                let area = self.space.output_geometry(output)?;
                let at = self.space.element_location(&window)?;
                Some((
                    output.name(),
                    Rectangle::new(at - area.loc, window.geometry().size),
                ))
            }
        }
    }

    /// The video's size: what is recorded at its output's density, even
    /// on both sides as encoders want.
    fn video_size(&self, target: &Target) -> Option<(u32, u32)> {
        let (output, rect) = self.target_place(target)?;
        let scale = self
            .find_output(&output)?
            .current_scale()
            .fractional_scale();
        let p = rect.to_f64().to_physical(scale);
        let even = |v: f64| ((v.round() as u32) / 2 * 2).max(2);
        Some((even(p.size.w), even(p.size.h)))
    }

    fn begin_recording(&mut self, target: Target) {
        let Some(size) = self.video_size(&target) else {
            self.recorder.phase = Phase::Idle;
            self.recorder.bar = None;
            return;
        };
        let Some(done) = self.recorder.done.clone() else {
            return;
        };
        let (feed, frames) = std::sync::mpsc::channel();
        let in_flight = Arc::new(AtomicUsize::new(0));
        let taken = in_flight.clone();
        let settings = Settings {
            folder: folder(&self.config.recording.folder),
            name: format!(
                "Recording {}.mp4",
                self.wall_civil().strftime("%Y-%m-%d %H-%M-%S")
            ),
            size,
            fps: self.config.recording.fps,
            hardware: self.config.recording.hardware,
            encoder: self.config.recording.encoder.clone(),
            sound: self.recorded_sound(),
            camera: None,
            bubble: (0, 0),
        };
        let mut settings = settings;
        let scale = self
            .target_place(&target)
            .and_then(|(o, _)| self.find_output(&o))
            .map_or(1.0, |o| o.current_scale().fractional_scale());
        if self.recorder.options.camera == Some(true) {
            let (x, y, d) = self.place_bubble(&target, size, scale);
            let r = &self.config.recording;
            settings.camera = Some(media::Camera {
                input: r.camera_input.clone(),
                device: r.camera_device.clone(),
                diameter: d,
            });
            settings.bubble = (x, y);
        }
        let spawned = std::thread::Builder::new()
            .name("recording".into())
            .spawn(move || {
                let finished = match encode(&settings, &frames, &taken) {
                    Ok(path) => Finished::Saved {
                        path,
                        width: settings.size.0,
                        height: settings.size.1,
                    },
                    Err(err) => Finished::Failed(err),
                };
                let _ = done.send(finished);
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not start recording");
            self.recorder.phase = Phase::Idle;
            self.recorder.bar = None;
            return;
        }
        tracing::info!(?target, ?size, "recording");
        self.recorder.phase = Phase::Recording(Box::new(Running {
            target,
            feed,
            in_flight,
            size,
            started: self.clock.now(),
            paused_total: Duration::ZERO,
            paused_since: None,
            last: None,
            clicks: Vec::new(),
            show_keys: self.recorder.options.show_keys,
            scale,
        }));
    }

    /// The sound to record, as the options and the configuration say.
    fn recorded_sound(&self) -> Option<media::Sound> {
        let r = &self.config.recording;
        let o = &self.recorder.options;
        let devices: Vec<String> = [
            (o.microphone, &r.microphone_device),
            (o.system_sound, &r.system_device),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, d)| d.clone())
        .collect();
        (!devices.is_empty()).then(|| media::Sound {
            format: r.sound.clone(),
            devices,
        })
    }

    /// The bubble's first place, bottom right of what is recorded, and
    /// its mark on the screen: its corner and diameter in video pixels.
    fn place_bubble(&mut self, target: &Target, size: (u32, u32), scale: f64) -> (u32, u32, u32) {
        let short = size.0.min(size.1);
        let d = ((f64::from(short) * 0.28) as u32).clamp(64, (short / 2).max(16)) / 2 * 2;
        let margin = (f64::from(short) * 0.04) as u32;
        let x = size.0.saturating_sub(d + margin);
        let y = size.1.saturating_sub(d + margin);
        if let Some((output, rect)) = self.target_place(target) {
            let logical = |v: u32| (f64::from(v) / scale).round() as i32;
            let mark = hocus_shell::recording::BubbleMark::new(
                self.theme(),
                self.shell_lang(),
                logical(d) as f32,
                scale as f32,
            );
            let mut panel = Panel::new(mark);
            panel.upload(&[]);
            self.recorder.bubble = Some(Bubble {
                output,
                place: rect.loc + Point::from((logical(x), logical(y))),
                diameter: logical(d),
                panel,
                held: None,
            });
        }
        (x, y, d)
    }

    fn stop_recording(&mut self) {
        let now = self.clock.now();
        if let Phase::Recording(running) = std::mem::take(&mut self.recorder.phase) {
            let at = running.elapsed(now);
            if running.feed.send(Feed::Stop { at }).is_ok() {
                self.recorder.phase = Phase::Finishing;
            }
        }
        self.recorder.bar = None;
        self.recorder.bubble = None;
        self.needs_redraw = true;
    }

    fn recording_finished(&mut self, finished: Finished) {
        self.recorder.phase = Phase::Idle;
        let lang = self.shell_lang();
        match finished {
            Finished::Saved {
                path,
                width,
                height,
            } => {
                let file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let actions = vec![
                    ("open".to_owned(), lang.open_file().to_owned()),
                    ("trim".to_owned(), lang.trim_action().to_owned()),
                    ("delete".to_owned(), lang.delete().to_owned()),
                ];
                if let Some(id) = self.shell_toast_with(lang.recording_saved(), &file, actions) {
                    self.screenshot_toast_file(id, path.clone());
                }
                self.emit(|_| Event::Recording {
                    path: Some(path.display().to_string()),
                    width,
                    height,
                });
            }
            Finished::Failed(err) => {
                tracing::warn!(err, "the recording failed");
                self.shell_toast(lang.recording_failed(), &err);
            }
        }
    }

    /// A key pressed while the keys are shown: a combination, or a key that
    /// is not text, is drawn in the video. Plain typing never is: it may be
    /// a password.
    pub fn recording_key(
        &mut self,
        modifiers: &smithay::input::keyboard::ModifiersState,
        sym: smithay::input::keyboard::Keysym,
    ) {
        let showing = matches!(
            &self.recorder.phase,
            Phase::Recording(r) if r.show_keys && r.paused_since.is_none()
        );
        if !showing || self.auth_open() {
            return;
        }
        let Some(parts) = key_parts(modifiers, sym) else {
            return;
        };
        let theme = self.theme();
        let now = self.clock.now();
        if let Phase::Recording(running) = &self.recorder.phase {
            let caps = hocus_shell::recording::key_caps(&theme, &parts, running.scale as f32);
            let at = running.elapsed(now);
            let _ = running.feed.send(Feed::Keys { at, caps });
        }
    }

    /// A button pressed anywhere: its ring goes in the video.
    pub fn recording_click(&mut self, at: Point<f64, Logical>, pressed: bool) {
        if !pressed || !self.config.recording.show_clicks {
            return;
        }
        let now = self.clock.now();
        if let Phase::Recording(running) = &mut self.recorder.phase {
            running.clicks.push((at, now));
            self.needs_redraw = true;
        }
    }

    /// The webcam's mark takes the pointer: dragged, the bubble moves
    /// within what is recorded.
    fn bubble_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        let Phase::Recording(running) = &self.recorder.phase else {
            return false;
        };
        let Some((output, rect)) = self.target_place(&running.target) else {
            return false;
        };
        let scale = running.scale;
        let Some(area) = self
            .find_output(&output)
            .and_then(|o| self.space.output_geometry(&o))
        else {
            return false;
        };
        let Some(bubble) = &mut self.recorder.bubble else {
            return false;
        };
        if bubble.output != output {
            return false;
        }
        let corner = (area.loc + bubble.place).to_f64();
        let r = f64::from(bubble.diameter) / 2.0;
        let centre = corner + Point::from((r, r));
        let over = (at.x - centre.x).hypot(at.y - centre.y) <= r;
        match (pressed, bubble.held) {
            (Some(true), _) if over => {
                bubble.held = Some(at - corner);
                true
            }
            (None, Some(held)) => {
                let d = bubble.diameter;
                let want: Point<i32, Logical> = (at - held - area.loc.to_f64()).to_i32_round();
                let place = Point::from((
                    want.x
                        .clamp(rect.loc.x, rect.loc.x + (rect.size.w - d).max(0)),
                    want.y
                        .clamp(rect.loc.y, rect.loc.y + (rect.size.h - d).max(0)),
                ));
                if place != bubble.place {
                    bubble.place = place;
                    let video = |v: i32| (f64::from(v.max(0)) * scale).round() as u32;
                    let (x, y) = (video(place.x - rect.loc.x), video(place.y - rect.loc.y));
                    if let Phase::Recording(running) = &self.recorder.phase {
                        let _ = running.feed.send(Feed::Bubble { x, y });
                    }
                    self.needs_redraw = true;
                }
                true
            }
            (Some(false), Some(_)) => {
                bubble.held = None;
                true
            }
            _ => false,
        }
    }

    /// The bar takes the pointer over it: pause, resume, stop; the
    /// webcam's mark is dragged.
    pub fn recording_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        if self.bubble_pointer(at, pressed) {
            return true;
        }
        let Some((name, place, panel)) = &mut self.recorder.bar else {
            return false;
        };
        let Some(area) = self
            .space
            .outputs()
            .find(|o| o.name() == *name)
            .and_then(|o| self.space.output_geometry(o))
        else {
            return false;
        };
        let local = hocus_ui::Point::new(
            (at.x - f64::from(area.loc.x + place.x)) as f32,
            (at.y - f64::from(area.loc.y + place.y)) as f32,
        );
        let mine = panel.view.hit(local) || (pressed == Some(false) && self.recorder.pressed);
        if !mine {
            if pressed.is_none() {
                panel.view.input(local, hocus_ui::Input::PointerLeave);
            }
            return false;
        }
        let input = match pressed {
            None => hocus_ui::Input::PointerMove(local),
            Some(p) => hocus_ui::Input::PointerButton { pressed: p },
        };
        if pressed.is_some() {
            panel.view.input(local, hocus_ui::Input::PointerMove(local));
        }
        if let Some(p) = pressed {
            self.recorder.pressed = p;
        }
        let events = panel.view.input(local, input);
        for event in events {
            match event {
                BarEvent::Stop => self.toggle_recording(),
                BarEvent::Pause => self.pause_recording(true),
                BarEvent::Resume => self.pause_recording(false),
            }
        }
        self.needs_redraw = true;
        true
    }

    fn pause_recording(&mut self, pause: bool) {
        let now = self.clock.now();
        if let Phase::Recording(running) = &mut self.recorder.phase {
            match (pause, running.paused_since) {
                (true, None) => {
                    let at = running.elapsed(now);
                    running.paused_since = Some(now);
                    let _ = running.feed.send(Feed::Pause { at });
                }
                (false, Some(since)) => {
                    running.paused_total += now - since;
                    running.paused_since = None;
                    // The next frame is taken at once.
                    running.last = None;
                    let _ = running.feed.send(Feed::Resume);
                }
                _ => {}
            }
        }
    }

    /// After each dispatch: the countdown ends, the bar shows the time,
    /// old rings go.
    pub fn refresh_recording(&mut self) {
        let now = self.clock.now();
        let state = match &mut self.recorder.phase {
            Phase::Counting { target, until } => {
                if now >= *until {
                    let target = target.clone();
                    self.begin_recording(target);
                    Some(BarState::Recording {
                        elapsed: Duration::ZERO,
                        paused: false,
                    })
                } else {
                    let left = (*until - now).as_secs_f64().ceil() as u32;
                    Some(BarState::Counting(left.max(1)))
                }
            }
            Phase::Recording(running) => {
                running
                    .clicks
                    .retain(|(_, t)| now.saturating_sub(*t) < RING);
                if !running.clicks.is_empty() {
                    self.needs_redraw = true;
                }
                Some(BarState::Recording {
                    elapsed: running.elapsed(now),
                    paused: running.paused_since.is_some(),
                })
            }
            Phase::Idle | Phase::Finishing => None,
        };
        if let (Some(state), Some((_, _, panel))) = (state, &mut self.recorder.bar) {
            panel.view.set(state);
            let drawn = panel.view.draw(now);
            if panel.upload(&drawn) {
                self.needs_redraw = true;
            }
        }
    }

    /// The red frame and the bar on `output`; nothing in the video.
    pub fn recording_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        let mut out = Vec::new();
        if self.recorder.capturing {
            return out;
        }
        let target = match &self.recorder.phase {
            Phase::Counting { target, .. } => target,
            Phase::Recording(running) => &running.target,
            Phase::Idle | Phase::Finishing => return out,
        };
        let scale = output.current_scale().fractional_scale();
        if let Some((name, place, panel)) = &self.recorder.bar
            && *name == output.name()
            && let Some(e) = panel.element(renderer, output, *place)
        {
            out.push(SceneElement::Frame(e));
        }
        if let Some(bubble) = &self.recorder.bubble
            && bubble.output == output.name()
            && let Some(e) = bubble.panel.element(renderer, output, bubble.place)
        {
            out.push(SceneElement::Frame(e));
        }
        let Some((name, rect)) = self.target_place(target) else {
            return out;
        };
        if name != output.name() {
            return out;
        }
        let area = self.space.output_geometry(output).unwrap_or_default();
        // Around a region or a window; inside the edges of a whole screen.
        let whole = rect.size == area.size && rect.loc == Point::default();
        let outer = if whole {
            rect
        } else {
            Rectangle::new(
                rect.loc - Point::from((FRAME, FRAME)),
                (rect.size.w + 2 * FRAME, rect.size.h + 2 * FRAME).into(),
            )
        };
        let sides = [
            Rectangle::new(outer.loc, (outer.size.w, FRAME).into()),
            Rectangle::new(
                outer.loc + Point::from((0, outer.size.h - FRAME)),
                (outer.size.w, FRAME).into(),
            ),
            Rectangle::new(outer.loc, (FRAME, outer.size.h).into()),
            Rectangle::new(
                outer.loc + Point::from((outer.size.w - FRAME, 0)),
                (FRAME, outer.size.h).into(),
            ),
        ];
        for side in sides {
            let mut buffer = self.recorder.red.clone();
            buffer.resize(side.size);
            out.push(SceneElement::Solid(SolidColorRenderElement::from_buffer(
                &buffer,
                side.loc.to_physical_precise_round(scale),
                scale,
                1.0,
                Kind::Unspecified,
            )));
        }
        out
    }

    /// The bar's buttons and the webcam's mark, for the control API.
    pub fn recording_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let at = |name: &str| {
            self.find_output(name)
                .and_then(|o| self.space.output_geometry(&o))
                .map(|g| g.loc)
        };
        if let Some((name, place, panel)) = &self.recorder.bar
            && let Some(loc) = at(name)
        {
            crate::shell::flatten(&panel.view.access(), loc + *place, 0, out);
        }
        if let Some(bubble) = &self.recorder.bubble
            && let Some(loc) = at(&bubble.output)
        {
            crate::shell::flatten(&bubble.panel.view.access(), loc + bubble.place, 0, out);
        }
    }
}

impl App {
    /// After a dispatch: takes a frame when one is due and the screen
    /// drew something new, and hands it to the worker.
    pub fn serve_recording(&mut self) {
        let now = self.state.clock.now();
        let fps = self.state.config.recording.fps.max(1);
        let Phase::Recording(running) = &self.state.recorder.phase else {
            return;
        };
        if running.paused_since.is_some() {
            return;
        }
        let place = self.state.target_place(&running.target);
        let Some((output_name, rect)) = place else {
            // What was recorded is gone: the recording ends.
            self.state.stop_recording();
            return;
        };
        let output = self.state.find_output(&output_name);
        let frame_no = output
            .as_ref()
            .map_or(0, |o| self.state.output_frame_count(o));
        let ringing = !running.clicks.is_empty();
        let due = match running.last {
            None => true,
            Some((t, n)) => {
                now.saturating_sub(t) >= Duration::from_secs(1) / fps && (n != frame_no || ringing)
            }
        };
        if !due {
            return;
        }
        let target = running.target.clone();
        let size = running.size;
        let at = running.elapsed(now);
        let scale = output
            .as_ref()
            .map_or(1.0, |o| o.current_scale().fractional_scale());
        let rings: Vec<Ring> = running
            .clicks
            .iter()
            .filter_map(|(p, t)| {
                let area = self.state.space.output_geometry(output.as_ref()?)?;
                let local = *p - area.loc.to_f64() - rect.loc.to_f64();
                Some(Ring {
                    x: (local.x * scale) as f32,
                    y: (local.y * scale) as f32,
                    age: now.saturating_sub(*t).as_secs_f32() / RING.as_secs_f32(),
                    scale: scale as f32,
                })
            })
            .collect();
        self.state.recorder.capturing = true;
        let taken = self.take_recorded(&target, output.as_ref(), rect, scale);
        self.state.recorder.capturing = false;
        let pixels = match taken {
            Ok(pixels) => fit(pixels, size),
            Err(err) => {
                tracing::debug!("a recording frame was not taken: {err:#}");
                return;
            }
        };
        let Phase::Recording(running) = &mut self.state.recorder.phase else {
            return;
        };
        running.last = Some((now, frame_no));
        // The worker is behind: this frame is dropped, the last repeated.
        if running.in_flight.load(Ordering::Relaxed) >= QUEUE {
            tracing::debug!("the encoder is behind: a frame dropped");
            return;
        }
        running.in_flight.fetch_add(1, Ordering::Relaxed);
        if running
            .feed
            .send(Feed::Frame { at, pixels, rings })
            .is_err()
        {
            tracing::warn!("the encoder stopped");
            self.state.recorder.phase = Phase::Idle;
            self.state.recorder.bar = None;
        }
    }

    fn take_recorded(
        &mut self,
        target: &Target,
        output: Option<&Output>,
        rect: Rectangle<i32, Logical>,
        scale: f64,
    ) -> anyhow::Result<Pixels> {
        use crate::BackendState;
        match target {
            Target::Output { .. } => {
                let output = output.ok_or_else(|| anyhow::anyhow!("no output"))?;
                let state = &self.state;
                let whole = match &mut self.backend {
                    BackendState::Winit(b) => b.capture_output(output, state, true),
                    BackendState::Headless(b) => b.capture_output(output, state, true),
                    BackendState::Tty(b) => b.capture_output(output, state, true),
                }?;
                let p = rect.to_f64().to_physical(scale);
                Ok(crate::screenshot::crop(
                    &whole,
                    p.loc.x.round() as u32,
                    p.loc.y.round() as u32,
                    p.size.w.round() as u32,
                    p.size.h.round() as u32,
                ))
            }
            Target::Window(id) => {
                let window = self
                    .state
                    .windows
                    .get(id)
                    .map(|m| m.window.clone())
                    .ok_or_else(|| anyhow::anyhow!("the window is gone"))?;
                match &mut self.backend {
                    BackendState::Winit(b) => b.capture_window(&window, scale),
                    BackendState::Headless(b) => b.capture_window(&window, scale),
                    BackendState::Tty(b) => b.capture_window(&window, scale),
                }
            }
        }
    }
}

/// The caps of a key pressed with `modifiers`: none for a modifier alone,
/// nor for text typed without Super, Ctrl or Alt.
fn key_parts(
    modifiers: &smithay::input::keyboard::ModifiersState,
    sym: smithay::input::keyboard::Keysym,
) -> Option<Vec<String>> {
    use smithay::input::keyboard::Keysym as K;
    let special = match sym {
        K::Return | K::KP_Enter => Some("Enter"),
        K::Escape => Some("Esc"),
        K::Tab | K::ISO_Left_Tab => Some("Tab"),
        K::BackSpace => Some("Backspace"),
        K::Delete => Some("Del"),
        K::Left => Some("←"),
        K::Right => Some("→"),
        K::Up => Some("↑"),
        K::Down => Some("↓"),
        K::Home => Some("Home"),
        K::End => Some("End"),
        K::Page_Up => Some("PgUp"),
        K::Page_Down => Some("PgDn"),
        K::Print => Some("Print"),
        K::space => Some("Space"),
        _ => None,
    };
    let raw = sym.raw();
    let function = (K::F1.raw()..=K::F24.raw())
        .contains(&raw)
        .then(|| format!("F{}", raw - K::F1.raw() + 1));
    let modifier_alone = matches!(
        sym,
        K::Control_L
            | K::Control_R
            | K::Alt_L
            | K::Alt_R
            | K::Shift_L
            | K::Shift_R
            | K::Super_L
            | K::Super_R
            | K::Meta_L
            | K::Meta_R
            | K::ISO_Level3_Shift
    );
    if modifier_alone {
        return None;
    }
    let held = modifiers.logo || modifiers.ctrl || modifiers.alt;
    // Space and plain keys are typing unless held with a modifier.
    let shown_alone = function.is_some() || special.is_some_and(|s| s != "Space");
    if !held && !shown_alone {
        return None;
    }
    let key = match (special, function, sym.key_char()) {
        (Some(name), _, _) => name.to_owned(),
        (None, Some(f), _) => f,
        (None, None, Some(c)) if !c.is_control() => c.to_uppercase().collect(),
        _ => return None,
    };
    let mut parts = Vec::new();
    for (on, name) in [
        (modifiers.logo, "Super"),
        (modifiers.ctrl, "Ctrl"),
        (modifiers.alt, "Alt"),
        (modifiers.shift, "Shift"),
    ] {
        if on {
            parts.push(name.to_owned());
        }
    }
    parts.push(key);
    Some(parts)
}

/// `pixels` at exactly `size`: cut, or padded with black (a window that
/// grew or shrank).
fn fit(pixels: Pixels, (w, h): (u32, u32)) -> Pixels {
    if pixels.width == w && pixels.height == h {
        return pixels;
    }
    let mut rgba = vec![0; (w * h * 4) as usize];
    for px in rgba.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let (cw, ch) = (pixels.width.min(w), pixels.height.min(h));
    for row in 0..ch {
        let from = (row * pixels.width * 4) as usize;
        let to = (row * w * 4) as usize;
        rgba[to..to + (cw * 4) as usize]
            .copy_from_slice(&pixels.rgba[from..from + (cw * 4) as usize]);
    }
    Pixels {
        width: w,
        height: h,
        rgba,
    }
}

/// Draws `ring` into `pixels`: a circle that grows and fades, white with a
/// dark edge so that it shows on any background.
pub fn paint_ring(pixels: &mut Pixels, ring: Ring) {
    let fade = (1.0 - ring.age).clamp(0.0, 1.0);
    if fade <= 0.0 {
        return;
    }
    let radius = (12.0 + 16.0 * ring.age) * ring.scale;
    let width = 3.0 * ring.scale;
    let reach = radius + width + 1.0;
    let (w, h) = (pixels.width as i64, pixels.height as i64);
    let x0 = ((ring.x - reach).floor() as i64).max(0);
    let x1 = ((ring.x + reach).ceil() as i64).min(w - 1);
    let y0 = ((ring.y - reach).floor() as i64).max(0);
    let y1 = ((ring.y + reach).ceil() as i64).min(h - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ((x as f32 + 0.5 - ring.x).powi(2) + (y as f32 + 0.5 - ring.y).powi(2)).sqrt();
            let off = (d - radius).abs();
            // White on the ring, a dark edge just outside it.
            let (color, cover) = if off <= width / 2.0 {
                (255.0, 1.0)
            } else if off <= width / 2.0 + ring.scale {
                (0.0, 0.5)
            } else {
                continue;
            };
            let a = cover * fade;
            let i = ((y * w + x) * 4) as usize;
            for c in &mut pixels.rgba[i..i + 3] {
                *c = (f32::from(*c) * (1.0 - a) + color * a).round() as u8;
            }
        }
    }
}

/// What the worker needs.
#[derive(Debug, Clone)]
struct Settings {
    folder: PathBuf,
    name: String,
    size: (u32, u32),
    fps: u32,
    hardware: bool,
    encoder: String,
    sound: Option<media::Sound>,
    camera: Option<media::Camera>,
    /// The bubble's corner at the start, video pixels.
    bubble: (u32, u32),
}

/// Feeds the encoder at a steady rate until told to stop: each frame
/// slot gets the latest frame taken before it, with the bubble and the keys
/// over it. The sound, recorded beside, is put with the video at the end.
/// The file's path.
fn encode(
    settings: &Settings,
    frames: &Receiver<Feed>,
    in_flight: &AtomicUsize,
) -> Result<PathBuf, String> {
    let path = reserve(&settings.folder, &settings.name)?;
    // With sound, the picture goes to a file of its own first.
    let video = match &settings.sound {
        Some(_) => hidden_beside(&path, "video.mp4"),
        None => path.clone(),
    };
    let codec = codec(settings);
    let mut child = spawn_encoder(settings, &codec, &video)?;
    let mut stdin = child.stdin.take().ok_or("the encoder has no input")?;
    let mut sound = match &settings.sound {
        Some(s) => {
            let base = hidden_beside(&path, "sound");
            match media::SoundRecorder::start(&settings.encoder, s.clone(), &base) {
                Ok(recorder) => Some(recorder),
                Err(err) => {
                    tracing::warn!(err, "recording without sound");
                    None
                }
            }
        }
        None => None,
    };
    let mut overlays = media::Overlays {
        bubble: settings.bubble,
        camera: settings.camera.as_ref().and_then(|c| {
            match media::CameraFeed::start(&settings.encoder, c) {
                Ok(feed) => Some(feed),
                Err(err) => {
                    tracing::warn!(err, "recording without the camera");
                    None
                }
            }
        }),
        ..media::Overlays::default()
    };
    let slot = Duration::from_secs(1) / settings.fps;
    let mut written: u64 = 0;
    let mut last: Option<Vec<u8>> = None;
    let mut fail = None;
    let mut fill =
        |until: Duration, last: &Option<Vec<u8>>, written: &mut u64, overlays: &media::Overlays| {
            let Some(frame) = last else {
                return Ok(());
            };
            media::write_slots(
                &mut stdin,
                frame,
                settings.size,
                slot,
                written,
                until,
                overlays,
            )
            .map_err(|e| e.to_string())
        };
    loop {
        match frames.recv() {
            Ok(Feed::Frame {
                at,
                mut pixels,
                rings,
            }) => {
                in_flight.fetch_sub(1, Ordering::Relaxed);
                if let Err(err) = fill(at, &last, &mut written, &overlays) {
                    fail = Some(err);
                    break;
                }
                for ring in rings {
                    paint_ring(&mut pixels, ring);
                }
                last = Some(pixels.rgba);
            }
            Ok(Feed::Keys { at, caps }) => overlays.keys = Some((at, caps)),
            Ok(Feed::Bubble { x, y }) => overlays.bubble = (x, y),
            Ok(Feed::Pause { at }) => {
                if let Err(err) = fill(at, &last, &mut written, &overlays) {
                    fail = Some(err);
                    break;
                }
                if let Some(sound) = &mut sound {
                    sound.pause();
                }
            }
            Ok(Feed::Resume) => {
                if let Some(sound) = &mut sound
                    && let Err(err) = sound.resume()
                {
                    tracing::warn!(err, "the sound did not start again");
                }
            }
            Ok(Feed::Stop { at }) => {
                // At least one frame, the last one shown until the end.
                let until = at.max(slot);
                if let Err(err) = fill(until, &last, &mut written, &overlays) {
                    fail = Some(err);
                }
                break;
            }
            // The compositor is gone: what was recorded is kept.
            Err(_) => break,
        }
    }
    drop(fill);
    overlays.camera = None;
    let segments = sound.map(media::SoundRecorder::finish);
    // The end of the input ends the file.
    drop(stdin);
    let mut errors = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut errors);
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let result = match (status.success(), fail) {
        (true, None) => Ok(()),
        (_, fail) => Err(errors
            .lines()
            .last()
            .map(str::to_owned)
            .or(fail)
            .unwrap_or_else(|| format!("the encoder said {status}"))),
    };
    let result = match (result, segments) {
        (Ok(()), Some(segments)) if video != path => {
            let muxed = if segments.is_empty() {
                // No sound came: the picture alone.
                std::fs::rename(&video, &path).map_err(|e| e.to_string())
            } else {
                media::mux(&settings.encoder, &video, &segments, &path)
            };
            for segment in &segments {
                let _ = std::fs::remove_file(segment);
            }
            muxed
        }
        (result, segments) => {
            for segment in segments.iter().flatten() {
                let _ = std::fs::remove_file(segment);
            }
            result
        }
    };
    if video != path {
        let _ = std::fs::remove_file(&video);
    }
    match result {
        Ok(()) => Ok(path),
        Err(err) => {
            let _ = std::fs::remove_file(&path);
            Err(err)
        }
    }
}

/// A hidden file beside `path` for one of its parts.
fn hidden_beside(path: &Path, part: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{part}"))
}

/// The video codec's arguments: the graphics card's when it encodes,
/// else H.264 in software.
fn codec(settings: &Settings) -> Vec<String> {
    let node = Path::new("/dev/dri/renderD128");
    let hardware =
        settings.hardware && node.exists() && encodes_with_vaapi(&settings.encoder, node);
    let args: &[&str] = if hardware {
        &[
            "-vf",
            "format=nv12,hwupload",
            "-c:v",
            "h264_vaapi",
            "-qp",
            "24",
        ]
    } else {
        &[
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-pix_fmt", "yuv420p",
        ]
    };
    let mut out: Vec<String> = Vec::new();
    if hardware {
        out.extend(["-vaapi_device".into(), node.display().to_string()]);
    }
    out.extend(args.iter().map(|s| (*s).to_owned()));
    out
}

/// Whether the encoder takes a frame through VA-API here: a tenth of a
/// second of black, thrown away.
fn encodes_with_vaapi(encoder: &str, node: &Path) -> bool {
    Command::new(encoder)
        .args(["-hide_banner", "-loglevel", "error", "-vaapi_device"])
        .arg(node)
        .args([
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:d=0.1",
            "-vf",
            "format=nv12,hwupload",
            "-c:v",
            "h264_vaapi",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn spawn_encoder(settings: &Settings, codec: &[String], path: &Path) -> Result<Child, String> {
    let (w, h) = settings.size;
    let mut command = Command::new(&settings.encoder);
    command.args(["-hide_banner", "-loglevel", "error", "-y"]);
    // The device comes before the input.
    let (device, rest) = match codec.first().map(String::as_str) {
        Some("-vaapi_device") => codec.split_at(2),
        _ => codec.split_at(0),
    };
    command
        .args(device)
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
        .arg(format!("{w}x{h}"))
        .arg("-r")
        .arg(settings.fps.to_string())
        .args(["-i", "-"])
        .args(rest)
        .args(["-movflags", "+faststart"])
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
        .spawn()
        .map_err(|e| format!("{}: {e}", settings.encoder))
}

/// A new file in `dir`, never over another: a number is added when the
/// name is taken.
fn reserve(dir: &Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = name.trim_end_matches(".mp4");
    for n in 1..1000 {
        let path = if n == 1 {
            dir.join(name)
        } else {
            dir.join(format!("{stem} ({n}).mp4"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(format!("{}: {err}", path.display())),
        }
    }
    Err(format!("{}: no free name", dir.display()))
}

/// Where recordings go: the configured folder, else `Screencasts` in the
/// videos folder (`XDG_VIDEOS_DIR`, else `~/Videos`).
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
    let videos = std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|dirs| {
            dirs.lines().find_map(|l| {
                let v = l.strip_prefix("XDG_VIDEOS_DIR=")?.trim_matches('"');
                Some(PathBuf::from(v.replace("$HOME", &home.to_string_lossy())))
            })
        })
        .unwrap_or_else(|| home.join("Videos"));
    videos.join("Screencasts")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(w: u32, h: u32) -> Pixels {
        Pixels {
            width: w,
            height: h,
            rgba: [128, 128, 128, 255].repeat((w * h) as usize),
        }
    }

    #[test]
    fn a_ring_is_painted_where_the_click_was_and_fades() {
        let mut p = grey(100, 100);
        let ring = Ring {
            x: 50.0,
            y: 50.0,
            age: 0.0,
            scale: 1.0,
        };
        paint_ring(&mut p, ring);
        let at = |p: &Pixels, x: u32, y: u32| p.rgba[((y * p.width + x) * 4) as usize];
        assert_eq!(at(&p, 62, 50), 255, "on the ring");
        assert_eq!(at(&p, 50, 50), 128, "not in its middle");
        let mut q = grey(100, 100);
        paint_ring(&mut q, Ring { age: 1.0, ..ring });
        assert!(
            q.rgba.iter().all(|c| *c == 128 || *c == 255),
            "gone once grown"
        );
    }

    #[test]
    fn combinations_show_but_typing_never_does() {
        use smithay::input::keyboard::{Keysym, ModifiersState};
        let ctrl = ModifiersState {
            ctrl: true,
            ..ModifiersState::default()
        };
        let shift = ModifiersState {
            shift: true,
            ..ModifiersState::default()
        };
        let none = ModifiersState::default();
        let shown = |m: &ModifiersState, k| key_parts(m, k);
        assert_eq!(
            shown(&ctrl, Keysym::c),
            Some(vec!["Ctrl".into(), "C".into()])
        );
        assert_eq!(shown(&none, Keysym::Return), Some(vec!["Enter".into()]));
        assert_eq!(shown(&none, Keysym::F5), Some(vec!["F5".into()]));
        // A password typed, capitals and spaces included: nothing.
        assert_eq!(shown(&none, Keysym::a), None);
        assert_eq!(shown(&shift, Keysym::A), None);
        assert_eq!(shown(&none, Keysym::space), None);
        assert_eq!(shown(&none, Keysym::_1), None);
        assert_eq!(shown(&ctrl, Keysym::Control_L), None);
    }

    #[test]
    fn frames_fit_the_video() {
        let fitted = fit(grey(3, 3), (4, 2));
        assert_eq!((fitted.width, fitted.height), (4, 2));
        assert_eq!(&fitted.rgba[12..16], &[0, 0, 0, 255], "padded black");
    }
}

#[cfg(test)]
mod encoder_tests {
    use super::*;

    #[test]
    fn the_encoder_writes_a_steady_file() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("skipped: no ffmpeg");
            return;
        }
        let dir = tempfile::tempdir().expect("a directory");
        let settings = Settings {
            folder: dir.path().to_path_buf(),
            name: "test.mp4".into(),
            size: (64, 48),
            fps: 30,
            hardware: false,
            encoder: "ffmpeg".into(),
            sound: None,
            camera: None,
            bubble: (0, 0),
        };
        let (feed, frames) = std::sync::mpsc::channel();
        let in_flight = AtomicUsize::new(0);
        for i in 0..3u32 {
            in_flight.fetch_add(1, Ordering::Relaxed);
            feed.send(Feed::Frame {
                at: Duration::from_millis(u64::from(i) * 400),
                pixels: Pixels {
                    width: 64,
                    height: 48,
                    rgba: [40, 80, 120, 255].repeat(64 * 48),
                },
                rings: Vec::new(),
            })
            .expect("sent");
        }
        feed.send(Feed::Stop {
            at: Duration::from_secs(2),
        })
        .expect("sent");
        let path = encode(&settings, &frames, &in_flight).expect("encoded");
        assert!(std::fs::metadata(&path).expect("a file").len() > 0);
    }

    /// Each stream's kind and length, as ffprobe reads them.
    fn streams(path: &Path) -> Vec<(String, f64)> {
        let out = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_type,duration"])
            .args(["-of", "csv=p=0"])
            .arg(path)
            .output()
            .expect("ffprobe");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let (kind, length) = l.split_once(',')?;
                Some((kind.to_owned(), length.parse().ok()?))
            })
            .collect()
    }

    #[test]
    fn sound_paused_and_a_camera_make_one_file_as_long_as_its_picture() {
        if Command::new("ffprobe").arg("-version").output().is_err() {
            eprintln!("skipped: no ffmpeg");
            return;
        }
        let dir = tempfile::tempdir().expect("a directory");
        let settings = Settings {
            folder: dir.path().to_path_buf(),
            name: "sound.mp4".into(),
            size: (160, 120),
            fps: 30,
            hardware: false,
            encoder: "ffmpeg".into(),
            sound: Some(media::Sound {
                format: "lavfi".into(),
                devices: vec![
                    "sine=frequency=440:sample_rate=48000".into(),
                    "sine=frequency=660:sample_rate=48000".into(),
                ],
            }),
            camera: Some(media::Camera {
                input: "lavfi".into(),
                device: "testsrc=size=320x240:rate=15".into(),
                diameter: 48,
            }),
            bubble: (100, 60),
        };
        let (feed, frames) = std::sync::mpsc::channel();
        let in_flight = AtomicUsize::new(1);
        let frame = |at| Feed::Frame {
            at,
            pixels: Pixels {
                width: 160,
                height: 120,
                rgba: [40, 80, 120, 255].repeat(160 * 120),
            },
            rings: Vec::new(),
        };
        let worker = std::thread::spawn(move || encode(&settings, &frames, &in_flight));
        let send = |f: Feed| feed.send(f).expect("sent");
        send(frame(Duration::ZERO));
        std::thread::sleep(Duration::from_millis(300));
        send(Feed::Keys {
            at: Duration::from_millis(300),
            caps: hocus_shell::recording::Overlay {
                width: 20,
                height: 10,
                rgba: [255, 255, 255, 255].repeat(200),
            },
        });
        send(Feed::Pause {
            at: Duration::from_millis(300),
        });
        std::thread::sleep(Duration::from_millis(200));
        send(Feed::Resume);
        std::thread::sleep(Duration::from_millis(300));
        send(Feed::Stop {
            at: Duration::from_millis(1500),
        });
        let path = worker.join().expect("the worker").expect("encoded");
        let found = streams(&path);
        let video = found
            .iter()
            .find(|(k, _)| k == "video")
            .expect("a picture")
            .1;
        let audio = found.iter().find(|(k, _)| k == "audio").expect("sound").1;
        assert!((video - 1.5).abs() < 0.1, "{found:?}");
        assert!((audio - video).abs() < 0.1, "{found:?}");
        // The bubble is in the picture, near the end: its square differs
        // from the screen's colour.
        let frame = Command::new("ffmpeg")
            .args(["-v", "error", "-ss", "1.2", "-i"])
            .arg(&path)
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .expect("a frame")
            .stdout;
        assert_eq!(frame.len(), 160 * 120 * 3);
        let mean_off = |x0: usize, y0: usize| {
            let mut sum = 0u32;
            for y in y0..y0 + 16 {
                for x in x0..x0 + 16 {
                    let p = (y * 160 + x) * 3;
                    sum += frame[p..p + 3]
                        .iter()
                        .zip([40u8, 80, 120])
                        .map(|(a, b)| u32::from(a.abs_diff(b)))
                        .sum::<u32>();
                }
            }
            sum / 256
        };
        assert!(mean_off(116, 76) > 30, "the bubble: {}", mean_off(116, 76));
        assert!(mean_off(10, 10) < 15, "the screen: {}", mean_off(10, 10));
        // Nothing but the file is left.
        let left: Vec<_> = std::fs::read_dir(dir.path()).expect("listed").collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }
}
