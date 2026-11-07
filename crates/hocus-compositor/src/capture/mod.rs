//! The screen capture (5.8, docs/design/screenshots.md): choose, then
//! capture.
//!
//! - **Opening**: `Print` opens it on every screen. The compositor darkens
//!   what is not chosen and draws the area, its handles and its size, the
//!   window's ring, the countdown and the flash itself, so that dragging
//!   an area is drawn in the same frame as the pointer's motion: no line
//!   goes to the shell for it. The shell draws only the toolbar
//!   ([`hocus_link::CAPTURE_TOOLBAR_NAMESPACE`]), at the bottom of the
//!   screen under the pointer, told what it shows (`capture`) and saying
//!   what is clicked on it (`capture mode|delay|pointer|shoot`).
//! - **While it is open** the compositor keeps the keyboard's keys
//!   (`Escape` cancels, `Enter` captures, the arrows move the area and
//!   `Shift` and the arrows resize it) and the pointer, but over the
//!   toolbar, which gets it as any surface does.
//! - **Taking it**: the image is drawn again for it, off the screen, at
//!   the screens' own resolution, without what the capture shows (nor the
//!   toolbar, nor a toast), the pointer only if asked (in a window's,
//!   where it is on it, when it is over it), a password manager's window
//!   black ([`shoot`]). Then a thread cuts or puts the pieces together,
//!   encodes the PNG and writes it in the pictures' folder ([`save`]), and
//!   makes the line for the clipboard's history; back on the loop, the
//!   clipboard offers it (`image/png`), the history lists it first, its
//!   item known by the capture's number (5.8b), and the shell shows the
//!   toast.
//! - `Shift+Print` and `Alt+Print` take the screen under the pointer and
//!   the focused window at once.
//! - The file goes to `[capture] folder` when it can be written, else to
//!   the pictures' folder's own, as PNG or JPEG (`[capture] format`); the
//!   shutter is heard as it flashes (`[capture] sound`, [`shutter`]),
//!   all read at each capture (6.17).
//! - The area is kept in the state directory, written off the loop as the
//!   capture closes.
//! - Then (5.9) the shell shows the capture's thumbnail and its editor,
//!   and asks for what is the compositor's ([`after`]).

mod after;
mod draw;
pub mod geometry;
pub(crate) mod save;
pub mod shoot;
mod shutter;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use hocus_link::{
    CAPTURE_DELAYS, CAPTURE_TOOLBAR_NAMESPACE, CaptureAsk, CaptureBar, CaptureMode, Captured,
    ClipKind, LAUNCHER_ZOOM, ToShell,
};
use smithay::{
    backend::input::ButtonState,
    desktop::{LayerSurface, WindowSurfaceType, layer_map_for_output},
    input::{
        keyboard::Keysym,
        pointer::{ButtonEvent, CursorIcon, MotionEvent},
    },
    output::Output,
    reexports::{
        calloop::{
            RegistrationToken,
            channel::{self, Event, Sender},
            timer::{TimeoutAction, Timer},
        },
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

pub use geometry::Area;
pub use save::pictures_dir;

use crate::grabs::EdgesExt as _;
use crate::{
    State,
    grabs::BTN_LEFT,
    model::{Edges, WindowId},
    programs,
    shortcuts::CaptureNow,
};

/// How near a handle or an edge a press takes it: the handle's own half
/// width (4 px, zoomed) and a little more.
const REACH: f64 = 6.0 * LAUNCHER_ZOOM;
/// The flash over what was taken: from 0.85 to nothing.
const FLASH: Duration = Duration::from_millis(260);
const FLASH_FROM: f32 = 0.85;
/// The countdown's number comes in over this (the mockup's `tick`).
const TICK: Duration = Duration::from_millis(300);
const SECOND: Duration = Duration::from_secs(1);

/// What a capture takes.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// An area of screen `output`, logical pixels of it.
    Area {
        output: String,
        area: Area,
    },
    Window(WindowId),
    Screen(String),
    All,
}

/// A capture to take: what, with the pointer or not, and the screen its
/// toast shows on.
#[derive(Debug, Clone)]
pub struct Shot {
    pub target: Target,
    pub pointer: bool,
    pub toast: String,
    pub mode: CaptureMode,
}

/// A drag of the pointer, from where it was pressed (logical pixels of
/// the area's screen).
#[derive(Debug, Clone, PartialEq)]
enum Drag {
    /// A new area, drawn from `from`; the one there was, if it ends too
    /// small.
    Draw {
        from: (f64, f64),
        before: Option<(String, Area)>,
    },
    Move {
        from: (f64, f64),
        start: Area,
    },
    Resize {
        edges: Edges,
        from: (f64, f64),
        start: Area,
    },
}

#[derive(Debug)]
struct Choosing {
    /// The toolbar's screen.
    toolbar: String,
    drag: Option<Drag>,
    /// The window under the pointer, in window mode.
    hovered: Option<WindowId>,
}

/// What a countdown ends in: a capture taken, or (5.10) a recording
/// started.
#[derive(Debug, Clone)]
pub(crate) enum Then {
    Shoot(Shot),
    Record(crate::record::Plan),
}

#[derive(Debug)]
struct Counting {
    then: Then,
    /// The number shown, and since when.
    left: u8,
    since: Duration,
    /// The screen it shows on.
    output: String,
    timer: Option<RegistrationToken>,
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Closed,
    Choosing(Choosing),
    Counting(Counting),
}

/// What a thread made of a capture.
#[derive(Debug)]
struct Finished {
    shot: Shot,
    saved: Result<save::Saved, String>,
    /// Its line for the clipboard's history (5.8b).
    history: Option<String>,
    /// Its number ([`Capture::numbered`]).
    number: u64,
}

/// What the captures' threads send back to the loop.
#[derive(Debug)]
enum Back {
    Finished(Finished),
    /// Capture `id`'s file, annotated, read back (5.9).
    Annotated {
        id: u64,
        png: Result<Vec<u8>, String>,
        history: Option<String>,
    },
}

/// The capture's state, owned by the loop.
#[derive(Debug, Default)]
pub struct Capture {
    phase: Phase,
    /// What the toolbar has chosen, kept from one capture to the next.
    mode: CaptureMode,
    delay: u8,
    pointer: bool,
    /// Vidéo chosen (5.10), with the computer's sound and the microphone.
    video: bool,
    sound: bool,
    micro: bool,
    /// The area, on its screen.
    area: Option<(String, Area)>,
    /// Where the area is kept, and what was last written there.
    file: Option<PathBuf>,
    kept: Option<(String, Area)>,
    /// A capture for the backend to take, at its next turn.
    pending: Option<Shot>,
    /// What the flash covers (screens, and their parts), since when.
    flash: Option<(Vec<(String, Area)>, Duration)>,
    /// The toolbar's surface, since its first frame.
    toolbar: Option<(LayerSurface, Duration)>,
    /// The buttons pressed while it was open: their releases are its own.
    held: HashSet<u32>,
    done: Option<Sender<Back>>,
    /// Captures drawn: each one's number, which the clipboard's history
    /// knows its item by (5.8b), and the shell its thumbnail and its
    /// editor (5.9).
    numbered: u64,
    /// What is kept of the captures taken, and their thumbnail's and
    /// editor's surfaces (5.9).
    after: after::After,
    /// Captures saved, and the last file written: for tests.
    taken: u64,
    last_file: Option<PathBuf>,
    drawn: draw::Drawn,
}

/// What the capture shows, for tests.
#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    Closed,
    Choosing {
        mode: CaptureMode,
        delay: u8,
        pointer: bool,
        area: Option<(String, Area)>,
        hovered: Option<WindowId>,
        toolbar: String,
        /// Vidéo (5.10), and its sound and microphone.
        video: bool,
        sound: bool,
        micro: bool,
    },
    /// Counting down, this number shown; `record`: before a recording.
    Counting {
        left: u8,
        record: bool,
    },
}

/// A key while the capture is open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key {
    Cancel,
    Shoot,
    /// The area moved, or (`resize`) its right or bottom edge.
    Nudge {
        dx: f64,
        dy: f64,
        resize: bool,
    },
    Other,
}

impl Key {
    pub fn of(sym: Keysym, shift: bool) -> Self {
        let nudge = |dx, dy| Self::Nudge {
            dx,
            dy,
            resize: shift,
        };
        match sym {
            Keysym::Escape => Self::Cancel,
            Keysym::Return | Keysym::KP_Enter => Self::Shoot,
            Keysym::Left | Keysym::KP_Left => nudge(-1.0, 0.0),
            Keysym::Right | Keysym::KP_Right => nudge(1.0, 0.0),
            Keysym::Up | Keysym::KP_Up => nudge(0.0, -1.0),
            Keysym::Down | Keysym::KP_Down => nudge(0.0, 1.0),
            _ => Self::Other,
        }
    }
}

impl Capture {
    /// With the area kept in `state_dir`, read as the compositor starts.
    pub fn load(state_dir: Option<&Path>) -> Self {
        let file = state_dir.map(save::area_file);
        let area = file
            .as_deref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|text| save::read_area(&text));
        Self {
            file,
            kept: area.clone(),
            area,
            // The computer's sound is recorded by default, not the
            // microphone (5.10).
            sound: true,
            ..Self::default()
        }
    }

    /// Whether it is open, choosing: it takes the keys and the pointer.
    pub fn choosing(&self) -> bool {
        matches!(self.phase, Phase::Choosing(_))
    }

    pub fn shown(&self) -> Shown {
        match &self.phase {
            Phase::Closed => Shown::Closed,
            Phase::Choosing(c) => Shown::Choosing {
                mode: self.mode,
                delay: self.delay,
                pointer: self.pointer,
                area: self.area.clone(),
                hovered: c.hovered,
                toolbar: c.toolbar.clone(),
                video: self.video,
                sound: self.sound,
                micro: self.micro,
            },
            Phase::Counting(c) => Shown::Counting {
                left: c.left,
                record: matches!(c.then, Then::Record(_)),
            },
        }
    }

    /// Captures saved so far, and the last file written.
    pub fn taken(&self) -> (u64, Option<PathBuf>) {
        (self.taken, self.last_file.clone())
    }

    /// Commits of the capture's editor's surface so far: for tests.
    pub fn editor_commits(&self) -> u64 {
        self.after.editor_commits
    }

    /// Whether the backend has a capture to take.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The capture the backend is to take now.
    pub fn take_pending(&mut self) -> Option<Shot> {
        self.pending.take()
    }
}

impl State {
    /// `Print`: the capture opens on every screen, its toolbar on the one
    /// under the pointer, the area kept shown (else a third of that
    /// screen, in its middle).
    pub fn open_capture(&mut self) {
        if !matches!(self.capture.phase, Phase::Closed) {
            return;
        }
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        let kept =
            self.capture.area.clone().and_then(|(name, area)| {
                Some((name.clone(), area.held(self.logical_size(&name)?)))
            });
        let area = kept.or_else(|| {
            let (w, h) = self.logical_size(&output.name())?;
            let third = Area {
                x: w / 3.0,
                y: h / 3.0,
                w: w / 3.0,
                h: h / 3.0,
            };
            Some((output.name(), third.rounded()))
        });
        self.capture.area = area;
        self.capture.phase = Phase::Choosing(Choosing {
            toolbar: output.name(),
            drag: None,
            hovered: None,
        });
        tracing::info!(
            output = output.name(),
            mode = self.capture.mode.name(),
            "the capture opens"
        );
        self.tell_capture_bar();
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            self.capture_motion(at, self.input_time());
        }
        self.redraw = true;
    }

    /// `Shift+Print`, `Alt+Print`: taken at once, unless the capture is
    /// open.
    pub fn capture_now(&mut self, what: CaptureNow) {
        if !matches!(self.capture.phase, Phase::Closed) {
            return;
        }
        let pointer_output = self.output_under_pointer().map(|o| o.name());
        let (target, mode) = match what {
            CaptureNow::Screen => (
                pointer_output.clone().map(Target::Screen),
                CaptureMode::Screen,
            ),
            CaptureNow::Window => (
                self.model
                    .focused()
                    .filter(|id| self.windows.contains_key(id))
                    .map(Target::Window),
                CaptureMode::Window,
            ),
        };
        let Some(target) = target else {
            tracing::info!(?what, "nothing to capture");
            return;
        };
        let toast = self
            .target_output(&target)
            .or(pointer_output)
            .unwrap_or_default();
        tracing::info!(?what, "a capture at once");
        self.capture.pending = Some(Shot {
            target,
            pointer: false,
            toast,
            mode,
        });
        self.redraw = true;
    }

    /// The toolbar's screen's name, what it shows, to the shell.
    fn tell_capture_bar(&mut self) {
        let Phase::Choosing(choosing) = &self.capture.phase else {
            return;
        };
        let bar = CaptureBar {
            output: choosing.toolbar.clone(),
            scale: self.bar_scale(&choosing.toolbar),
            mode: self.capture.mode,
            delay: self.capture.delay,
            pointer: self.capture.pointer,
            video: self.capture.video,
            sound: self.capture.sound,
            micro: self.capture.micro,
        };
        self.tell_shell(ToShell::Capture(bar));
    }

    /// The shell's toolbar was clicked; or (5.9) a capture's thumbnail, its
    /// editor or its history asks for what is the compositor's.
    pub fn capture_asked(&mut self, ask: CaptureAsk) {
        match ask {
            CaptureAsk::Forget(id) => return self.capture_forget(id),
            CaptureAsk::Annotated(id) => return self.capture_annotated(id),
            CaptureAsk::Drag(id) => return self.capture_drag(id),
            _ => {}
        }
        if !self.capture.choosing() {
            return;
        }
        match ask {
            // No "all screens" for a video: its screen instead.
            CaptureAsk::Mode(CaptureMode::All) if self.capture.video => {}
            CaptureAsk::Mode(mode) => {
                self.capture.mode = mode;
                if let Phase::Choosing(c) = &mut self.capture.phase {
                    c.drag = None;
                    c.hovered = None;
                }
                if let Some(pointer) = self.seat.get_pointer() {
                    let at = pointer.current_location();
                    self.capture_follow(at);
                }
            }
            CaptureAsk::Delay => {
                let at = CAPTURE_DELAYS
                    .iter()
                    .position(|d| *d == self.capture.delay)
                    .unwrap_or(0);
                self.capture.delay = CAPTURE_DELAYS[(at + 1) % CAPTURE_DELAYS.len()];
            }
            CaptureAsk::Pointer => self.capture.pointer = !self.capture.pointer,
            CaptureAsk::Video(video) => {
                self.capture.video = video;
                if video && self.capture.mode == CaptureMode::All {
                    self.capture.mode = CaptureMode::Screen;
                }
            }
            CaptureAsk::Sound => self.capture.sound = !self.capture.sound,
            CaptureAsk::Micro => self.capture.micro = !self.capture.micro,
            CaptureAsk::Shoot => {
                self.capture_shoot();
                return;
            }
            CaptureAsk::Forget(_) | CaptureAsk::Annotated(_) | CaptureAsk::Drag(_) => return,
        }
        tracing::debug!(
            mode = self.capture.mode.name(),
            delay = self.capture.delay,
            pointer = self.capture.pointer,
            video = self.capture.video,
            sound = self.capture.sound,
            micro = self.capture.micro,
            "the capture's toolbar"
        );
        self.tell_capture_bar();
        self.redraw = true;
    }

    /// `Enter`, the green button, a click on a window: what is chosen is
    /// taken, now or once counted down.
    fn capture_shoot(&mut self) {
        let Phase::Choosing(choosing) = &self.capture.phase else {
            return;
        };
        if self.capture.video {
            let hovered = choosing.hovered;
            return self.record_from_capture(hovered);
        }
        let pointer_output = self.output_under_pointer().map(|o| o.name());
        let target = match self.capture.mode {
            CaptureMode::Area => self
                .capture
                .area
                .clone()
                .map(|(output, area)| Target::Area {
                    output,
                    area: area.rounded(),
                }),
            CaptureMode::Window => choosing
                .hovered
                .or_else(|| self.model.focused())
                .filter(|id| self.windows.contains_key(id))
                .map(Target::Window),
            CaptureMode::Screen => pointer_output.clone().map(Target::Screen),
            CaptureMode::All => Some(Target::All),
        };
        let Some(target) = target else {
            tracing::info!(mode = self.capture.mode.name(), "nothing chosen to capture");
            return;
        };
        let toast = self
            .target_output(&target)
            .or(pointer_output)
            .unwrap_or_else(|| choosing.toolbar.clone());
        let shot = Shot {
            target,
            pointer: self.capture.pointer,
            toast,
            mode: self.capture.mode,
        };
        let delay = self.capture.delay;
        self.close_capture();
        if delay == 0 {
            self.capture.pending = Some(shot);
        } else {
            let output = self
                .target_output(&shot.target)
                .unwrap_or_else(|| shot.toast.clone());
            self.count_down(Then::Shoot(shot), delay, output);
        }
        self.redraw = true;
    }

    /// Démarrer (5.10): what is chosen is recorded once counted down, with
    /// the sound and the microphone as chosen; the capture's UI goes.
    fn record_from_capture(&mut self, hovered: Option<WindowId>) {
        if self.record.recording() {
            tracing::info!("a recording is under way: one at a time");
            return;
        }
        let Some(plan) = self.record_plan(hovered) else {
            tracing::info!(mode = self.capture.mode.name(), "nothing chosen to record");
            return;
        };
        self.close_capture();
        self.record_count_down(plan);
        self.redraw = true;
    }

    /// What the capture's settings record now: the area, the window
    /// (`hovered`, else the focused one) or the screen under the pointer.
    pub(crate) fn record_plan(&self, hovered: Option<WindowId>) -> Option<crate::record::Plan> {
        use crate::record::Source;
        let source = match self.capture.mode {
            CaptureMode::Area => self
                .capture
                .area
                .clone()
                .map(|(output, area)| Source::Area {
                    output,
                    area: area.rounded(),
                }),
            CaptureMode::Window => hovered
                .or_else(|| self.model.focused())
                .filter(|id| self.windows.contains_key(id))
                .map(|id| Source::Window(id.0)),
            CaptureMode::Screen | CaptureMode::All => self
                .output_under_pointer()
                .map(|o| Source::Screen(o.name())),
        }?;
        Some(crate::record::Plan {
            source,
            sound: self.capture.sound,
            micro: self.capture.micro,
        })
    }

    /// The capture's countdown before recording `plan` (5.10): three
    /// seconds, in the middle of the screen recorded, with a red ring.
    pub(crate) fn record_count_down(&mut self, plan: crate::record::Plan) {
        let output = self.record_output(&plan.source).unwrap_or_default();
        self.count_down(Then::Record(plan), hocus_link::RECORD_COUNTDOWN, output);
    }

    /// Whether the capture counts down to a recording.
    pub(crate) fn counting_to_record(&self) -> bool {
        matches!(&self.capture.phase, Phase::Counting(c) if matches!(c.then, Then::Record(_)))
    }

    /// Whether the capture is open, choosing (its UI shows).
    pub(crate) fn capture_open(&self) -> bool {
        self.capture.choosing()
    }

    /// The capture's UI goes, or its countdown, nothing taken.
    pub(crate) fn capture_away(&mut self) {
        self.close_capture();
    }

    /// The screen a capture of `target` is on: an area's, the window's
    /// (where most of it is), the screen; none for all of them.
    fn target_output(&self, target: &Target) -> Option<String> {
        match target {
            Target::Area { output, .. } | Target::Screen(output) => Some(output.clone()),
            Target::Window(id) => self.window_output(*id).map(|o| o.name()),
            Target::All => None,
        }
    }

    /// Whether the pointer is over window `id`: its capture shows it then,
    /// when asked (5.8b).
    pub(crate) fn pointer_over(&self, id: WindowId) -> bool {
        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        let at = pointer.current_location();
        self.space
            .element_under(at)
            .and_then(|(window, _)| self.id_of(window))
            == Some(id)
    }

    /// The screen most of window `id` is on.
    pub(crate) fn window_output(&self, id: WindowId) -> Option<Output> {
        let rect = self.rect_of(id)?;
        let (cx, cy) = rect.centre();
        self.space
            .output_under((cx, cy))
            .next()
            .or_else(|| self.space.outputs().next())
            .cloned()
    }

    /// The capture's countdown from `delay` seconds, in the middle of
    /// `output`, a timer each second.
    fn count_down(&mut self, then: Then, delay: u8, output: String) {
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(SECOND), |_, _, app| {
                app.state.capture_second()
            });
        let timer = match timer {
            Ok(token) => Some(token),
            Err(err) => {
                tracing::warn!(%err.error, "no countdown: at once");
                match then {
                    Then::Shoot(shot) => self.capture.pending = Some(shot),
                    Then::Record(plan) => self.record_start(plan),
                }
                return;
            }
        };
        tracing::info!(delay, "the capture counts down");
        self.capture.phase = Phase::Counting(Counting {
            then,
            left: delay,
            since: self.now(),
            output,
            timer,
        });
    }

    /// A second of the countdown went: the next number, or the capture.
    fn capture_second(&mut self) -> TimeoutAction {
        let now = self.now();
        let Phase::Counting(counting) = &mut self.capture.phase else {
            return TimeoutAction::Drop;
        };
        counting.left = counting.left.saturating_sub(1);
        counting.since = now;
        self.redraw = true;
        if counting.left > 0 {
            return TimeoutAction::ToDuration(SECOND);
        }
        counting.timer = None;
        let then = counting.then.clone();
        self.capture.phase = Phase::Closed;
        match then {
            Then::Shoot(shot) => self.capture.pending = Some(shot),
            Then::Record(plan) => self.record_start(plan),
        }
        TimeoutAction::Drop
    }

    /// `Escape`: nothing is taken.
    fn cancel_capture(&mut self) {
        tracing::info!("the capture is cancelled");
        self.close_capture();
    }

    /// The capture's UI goes: the toolbar, the cursor it set; the area is
    /// kept; the pointer finds what is under it again.
    fn close_capture(&mut self) {
        match std::mem::take(&mut self.capture.phase) {
            Phase::Closed => return,
            Phase::Counting(counting) => {
                if let Some(timer) = counting.timer {
                    self.loop_handle.remove(timer);
                }
            }
            Phase::Choosing(_) => self.tell_shell(ToShell::CaptureEnd),
        }
        self.keep_area();
        self.frame_cursor = None;
        self.redraw = true;
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            self.pointer_motion(at, self.input_time());
        }
    }

    /// The area written to the state directory when it changed, off the
    /// loop.
    fn keep_area(&mut self) {
        let (Some(file), Some((output, area))) = (&self.capture.file, &self.capture.area) else {
            return;
        };
        if self.capture.kept.as_ref() == Some(&(output.clone(), *area)) {
            return;
        }
        self.capture.kept = Some((output.clone(), *area));
        let (file, text) = (file.clone(), save::write_area(output, *area));
        programs::on_a_thread("keep the capture's area".to_owned(), move || {
            if let Some(dir) = file.parent()
                && let Err(err) = std::fs::create_dir_all(dir)
            {
                tracing::warn!(%err, "could not keep the capture's area");
                return;
            }
            let part = file.with_extension("new");
            let written = std::fs::write(&part, text).and_then(|()| std::fs::rename(&part, &file));
            if let Err(err) = written {
                tracing::warn!(file = %file.display(), %err, "could not keep the capture's area");
            }
        });
    }

    /// A key while the capture is open.
    pub fn capture_key(&mut self, key: Key) {
        match key {
            Key::Cancel => self.cancel_capture(),
            Key::Shoot => self.capture_shoot(),
            Key::Nudge { dx, dy, resize } => {
                if self.capture.mode != CaptureMode::Area {
                    return;
                }
                let Some((output, area)) = self.capture.area.clone() else {
                    return;
                };
                let Some(size) = self.logical_size(&output) else {
                    return;
                };
                let area = if resize {
                    let edges = Edges {
                        right: dx != 0.0,
                        bottom: dy != 0.0,
                        ..Edges::default()
                    };
                    area.resized(edges, dx, dy, size)
                } else {
                    area.moved(dx, dy, size)
                };
                self.capture.area = Some((output, area));
                self.redraw = true;
            }
            Key::Other => {}
        }
    }

    /// The pointer moved to `at` while the capture is open: the toolbar
    /// gets it when it is over it, else the area or the window follows,
    /// and the cursor says what a press does. Whether it took it.
    /// Whether the capture is open, choosing: it holds the pointer.
    pub(crate) fn capture_choosing(&self) -> bool {
        matches!(self.capture.phase, Phase::Choosing(_))
    }

    pub fn capture_motion(&mut self, at: Point<f64, Logical>, time: u32) -> bool {
        let Phase::Choosing(choosing) = &self.capture.phase else {
            return false;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return true;
        };
        let toolbar = if choosing.drag.is_none() {
            self.capture_toolbar_under(at)
        } else {
            None
        };
        let cursor = match toolbar {
            Some(_) => None,
            None => Some(self.capture_follow(at)),
        };
        self.frame_cursor = cursor;
        pointer.motion(
            self,
            toolbar,
            &MotionEvent {
                location: at,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
        self.redraw = true;
        true
    }

    /// What follows the pointer at `at`: the drag, or the window under it;
    /// the cursor that says what a press there does.
    fn capture_follow(&mut self, at: Point<f64, Logical>) -> CursorIcon {
        let mode = self.capture.mode;
        let window = match mode {
            CaptureMode::Window => self
                .space
                .element_under(at)
                .and_then(|(window, _)| self.id_of(window)),
            _ => None,
        };
        let area = self.capture.area.clone();
        let local = |state: &Self, name: &str| {
            let origin = state.output_origin(name)?;
            Some((at.x - origin.0, at.y - origin.1))
        };
        let Phase::Choosing(choosing) = &mut self.capture.phase else {
            return CursorIcon::Default;
        };
        choosing.hovered = window;
        let drag = choosing.drag.clone();
        match (mode, drag) {
            (CaptureMode::Area, Some(drag)) => {
                let Some((output, _)) = area else {
                    return CursorIcon::Crosshair;
                };
                let (Some(p), Some(size)) = (local(self, &output), self.logical_size(&output))
                else {
                    return CursorIcon::Crosshair;
                };
                let (area, icon) = match drag {
                    Drag::Draw { from, .. } => {
                        (Area::between(from, p, size), CursorIcon::Crosshair)
                    }
                    Drag::Move { from, start } => (
                        start.moved(p.0 - from.0, p.1 - from.1, size),
                        CursorIcon::Move,
                    ),
                    Drag::Resize { edges, from, start } => (
                        start.resized(edges, p.0 - from.0, p.1 - from.1, size),
                        edges.cursor(),
                    ),
                };
                self.capture.area = Some((output, area));
                icon
            }
            (CaptureMode::Area, None) => {
                let hit = area.and_then(|(output, area)| Some(area.hit_at(local(self, &output)?)));
                match hit {
                    Some(geometry::Hit::Edges(edges)) => edges.cursor(),
                    Some(geometry::Hit::Inside) => CursorIcon::Move,
                    _ => CursorIcon::Crosshair,
                }
            }
            (CaptureMode::Window, _) if window.is_some() => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        }
    }

    /// A button while the capture is open: over the toolbar, the
    /// toolbar's; elsewhere the capture's (a drag of the area, a click on
    /// a window), and its release too. Whether it took it.
    pub fn capture_button(&mut self, button: u32, state: ButtonState, time: u32) -> bool {
        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        let at = pointer.current_location();
        if state == ButtonState::Released {
            if !self.capture.held.remove(&button) {
                return false;
            }
            if button == BTN_LEFT {
                self.capture_release(at);
            }
            return true;
        }
        let Phase::Choosing(choosing) = &self.capture.phase else {
            return false;
        };
        if choosing.drag.is_none() && self.capture_toolbar_under(at).is_some() {
            pointer.button(
                self,
                &ButtonEvent {
                    button,
                    state,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
            return true;
        }
        self.capture.held.insert(button);
        if button == BTN_LEFT && self.capture.mode == CaptureMode::Area {
            self.capture_press(at);
        }
        true
    }

    /// A press in area mode: a handle resizes, the inside moves, elsewhere
    /// a new area is drawn.
    fn capture_press(&mut self, at: Point<f64, Logical>) {
        let Some(output) = self.space.output_under(at).next().map(|o| o.name()) else {
            return;
        };
        let area = self.capture.area.clone();
        let Some(origin) = self.output_origin(&output) else {
            return;
        };
        let p = (at.x - origin.0, at.y - origin.1);
        let drag = match &area {
            Some((on, a)) if *on == output => match a.hit(p.0, p.1, REACH) {
                geometry::Hit::Edges(edges) => Drag::Resize {
                    edges,
                    from: p,
                    start: *a,
                },
                geometry::Hit::Inside => Drag::Move { from: p, start: *a },
                geometry::Hit::Outside => Drag::Draw {
                    from: p,
                    before: area.clone(),
                },
            },
            _ => Drag::Draw {
                from: p,
                before: area.clone(),
            },
        };
        if matches!(drag, Drag::Draw { .. }) {
            self.capture.area = Some((
                output,
                Area {
                    x: p.0,
                    y: p.1,
                    w: 0.0,
                    h: 0.0,
                },
            ));
        }
        if let Phase::Choosing(choosing) = &mut self.capture.phase {
            choosing.drag = Some(drag);
        }
        self.redraw = true;
    }

    /// The left button let go: a drag ends (a new area too small leaves
    /// the one there was), a click in window mode captures its window.
    fn capture_release(&mut self, at: Point<f64, Logical>) {
        let Phase::Choosing(choosing) = &mut self.capture.phase else {
            return;
        };
        match choosing.drag.take() {
            Some(Drag::Draw { before, .. }) => {
                let drawn = self.capture.area.as_ref().is_some_and(|(_, a)| a.drawn());
                if drawn {
                    if let Some((output, area)) = &mut self.capture.area {
                        tracing::debug!(output, ?area, "a capture's area drawn");
                    }
                } else {
                    self.capture.area = before;
                }
            }
            Some(_) => {}
            None if self.capture.mode == CaptureMode::Window => {
                if let Some(id) = self
                    .space
                    .element_under(at)
                    .and_then(|(window, _)| self.id_of(window))
                    && let Phase::Choosing(choosing) = &mut self.capture.phase
                {
                    choosing.hovered = Some(id);
                }
                self.capture_shoot();
                return;
            }
            None => {}
        }
        let cursor = Some(self.capture_follow(at));
        self.frame_cursor = cursor;
        self.redraw = true;
    }

    /// The toolbar's surface under `at`, and where its origin is.
    fn capture_toolbar_under(
        &self,
        at: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let output = self.space.output_under(at).next()?;
        let origin = self.space.output_geometry(output)?.loc;
        let map = layer_map_for_output(output);
        let layer = map
            .layers()
            .find(|l| l.namespace() == CAPTURE_TOOLBAR_NAMESPACE)?;
        let geometry = map.layer_geometry(layer)?;
        let loc = origin + geometry.loc;
        let (surface, p) = layer.surface_under(at - loc.to_f64(), WindowSurfaceType::ALL)?;
        Some((surface, (p + loc).to_f64()))
    }

    /// A commit of the toolbar's `layer`: its first frame starts its fade.
    pub fn capture_toolbar_committed(&mut self, layer: &LayerSurface) {
        if self
            .capture
            .toolbar
            .as_ref()
            .is_some_and(|(l, _)| l == layer)
        {
            return;
        }
        let drawn = smithay::backend::renderer::utils::with_renderer_surface_state(
            layer.wl_surface(),
            |s| s.buffer().is_some(),
        )
        .unwrap_or(false);
        if drawn {
            self.capture.toolbar = Some((layer.clone(), self.now()));
            self.redraw = true;
        }
    }

    /// A layer surface went: if it was the toolbar, its fade goes too.
    pub fn capture_toolbar_gone(&mut self, layer: &LayerSurface) {
        if self
            .capture
            .toolbar
            .as_ref()
            .is_some_and(|(l, _)| l == layer)
        {
            self.capture.toolbar = None;
        }
    }

    /// Taken by the backend: the flash starts over it, and a thread makes
    /// the image of `job`.
    pub(crate) fn capture_taken(&mut self, shot: Shot, job: save::Job, flash: Vec<(String, Area)>) {
        if !self.reduced_motion() && !flash.is_empty() {
            self.capture.flash = Some((flash, self.now()));
        }
        // The shutter, with the flash (6.17).
        if self.config.capture.sound {
            shutter::Player {
                programs: self.config.sound_programs.clone(),
                remote: self.config.pipewire_remote.clone(),
            }
            .play();
        }
        self.redraw = true;
        let Some(done) = self.capture_done() else {
            return;
        };
        self.capture.numbered += 1;
        let number = self.capture.numbered;
        programs::on_a_thread("save the capture".to_owned(), move || {
            let saved = save::save(job).map_err(|err| format!("{err:#}"));
            let history = saved
                .as_ref()
                .ok()
                .and_then(|saved| crate::clipboard::capture_line(number, &saved.png));
            let _ = done.send(Back::Finished(Finished {
                shot,
                saved,
                history,
                number,
            }));
        });
    }

    /// Where the threads' captures come back, made once.
    fn capture_done(&mut self) -> Option<Sender<Back>> {
        if let Some(done) = &self.capture.done {
            return Some(done.clone());
        }
        let (sender, receiver) = channel::channel::<Back>();
        let inserted = self
            .loop_handle
            .insert_source(receiver, |event, _, app| match event {
                Event::Msg(Back::Finished(finished)) => app.state.capture_finished(finished),
                Event::Msg(Back::Annotated { id, png, history }) => {
                    app.state.capture_reread(id, png, history);
                }
                Event::Closed => {}
            });
        match inserted {
            Ok(_) => {
                self.capture.done = Some(sender.clone());
                Some(sender)
            }
            Err(err) => {
                tracing::warn!(%err.error, "could not watch the captures' thread");
                None
            }
        }
    }

    /// A capture made: the clipboard offers it, and the shell shows its
    /// thumbnail (its toast when it was not saved).
    fn capture_finished(&mut self, finished: Finished) {
        let Finished {
            shot,
            saved,
            history,
            number,
        } = finished;
        let saved = match saved {
            Ok(saved) => saved,
            Err(err) => {
                tracing::warn!(err, "the capture could not be made");
                return;
            }
        };
        tracing::info!(
            width = saved.width,
            height = saved.height,
            file = ?saved.file,
            "captured"
        );
        self.capture.taken += 1;
        let id = number;
        self.capture.last_file.clone_from(&saved.file);
        let offered = self.offer_clipboard(ClipKind::Png.types(), saved.png);
        self.capture.after.keep(id, saved.file.clone(), &offered);
        if let Some(line) = history {
            self.capture_in_history(line);
        }
        let file = saved
            .file
            .as_deref()
            .and_then(Path::to_str)
            .map(str::to_owned)
            .unwrap_or_default();
        self.tell_shell(ToShell::Captured(Captured {
            id,
            output: shot.toast,
            mode: shot.mode,
            width: saved.width,
            height: saved.height,
            folder: saved.folder,
            file,
        }));
    }

    /// Screen `name`'s size, logical pixels.
    pub(crate) fn logical_size(&self, name: &str) -> Option<(f64, f64)> {
        let output = self.space.outputs().find(|o| o.name() == name)?;
        let size = self.space.output_geometry(output)?.size;
        Some((f64::from(size.w), f64::from(size.h)))
    }

    /// Screen `name`'s top left corner in the desktop.
    pub(crate) fn output_origin(&self, name: &str) -> Option<(f64, f64)> {
        let output = self.space.outputs().find(|o| o.name() == name)?;
        let loc = self.space.output_geometry(output)?.loc;
        Some((f64::from(loc.x), f64::from(loc.y)))
    }

    /// Whether the capture's flash, countdown or toolbar is moving at
    /// `now`.
    pub fn capture_tick(&mut self, now: Duration) -> bool {
        let mut moving = false;
        if let Some((_, since)) = &self.capture.flash {
            if now < *since + FLASH {
                moving = true;
            } else {
                self.capture.flash = None;
            }
        }
        if let Phase::Counting(counting) = &self.capture.phase {
            moving |= !self.reduced_motion() && now < counting.since + TICK;
        }
        if let Some((_, opened)) = &self.capture.toolbar {
            moving |= now < *opened + hocus_link::CAPTURE_TOOLBAR_IN;
        }
        moving |= self.capture_after_moving(now);
        moving
    }

    /// The capture taken now, when it was asked for: when the image says
    /// it was taken.
    pub(crate) fn capture_when(&self) -> SystemTime {
        SystemTime::now()
    }
}

impl Area {
    /// What a press at `p` takes.
    fn hit_at(self, p: (f64, f64)) -> geometry::Hit {
        self.hit(p.0, p.1, REACH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_of_the_capture() {
        assert_eq!(Key::of(Keysym::Escape, false), Key::Cancel);
        assert_eq!(Key::of(Keysym::Return, false), Key::Shoot);
        assert_eq!(Key::of(Keysym::KP_Enter, true), Key::Shoot);
        assert_eq!(
            Key::of(Keysym::Left, false),
            Key::Nudge {
                dx: -1.0,
                dy: 0.0,
                resize: false
            }
        );
        assert_eq!(
            Key::of(Keysym::Down, true),
            Key::Nudge {
                dx: 0.0,
                dy: 1.0,
                resize: true
            }
        );
        assert_eq!(Key::of(Keysym::a, false), Key::Other);
    }
}
