//! The sound's popover (bar.md §9, the 3.4 mockup): its title and the
//! default output's name; the master volume, its mute button and its
//! value; the outputs, the default one checked; the microphone, its mute
//! button, its volume and, when there are several, the one to use; then
//! the applications playing sound, each with its volume and its mute. A
//! click on an output makes it the default, which keeps its own volume
//! (PipeWire's, per sink): the slider then shows that one's.
//!
//! One canvas draws it all, and takes the pointer and the keys itself:
//! what changes draws this surface alone, without a message to iced. The
//! sliders' volumes go to the sound's service as they come; it sends
//! PipeWire only the last one each time `wpctl` is ready. A slider shows
//! the volume asked until PipeWire says it, a second at most; moving one
//! unmutes what it sets.
//!
//! **The applications listed**: on opening, those with a stream playing
//! now (its node running, not corked: a paused tab's stream lingers for
//! minutes, idle); while it is open, one that starts to play joins the
//! list, and one leaves it only when its last stream goes, so that no row
//! moves under the pointer when a video is paused. At most
//! [`MOST_APPLICATIONS`] rows, the others counted under them. One row per
//! application, setting all its streams.
//!
//! The keyboard: `Tab` and `Shift+Tab` go through the mute button, the
//! slider, the outputs, the microphone's mute button and slider, the
//! microphones, then each application's tile and slider; `Enter` or
//! `Space` acts; the arrows move a slider by 5 % (and go through the
//! outputs and the microphones); `Esc` closes.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::LevelKey;
use iced::{
    Color, Point, Radians, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Pressed, event,
    kit::{
        self, FILL, HEADER, HEADER_TOP, ICON, ICON_GAP, Input, LABEL, LABEL_PAD, LEFT, NOTE,
        NOTE_TOP, Pointer, RIGHT, ROW, ROW_GAP, ROW_OUT, ROW_TEXT, TITLE, Track, rounded, text,
    },
};
use crate::levels::{
    self,
    draw::{
        ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, SOUND, TEXT, VALUE, rgb, ring, stroke,
    },
    model::{STEP, Sound, Words},
    sound::{App, Ask, Kind, Mixer, Set, Sink},
};

// --- The mockup's sizes, at ×1 ------------------------------------------------

const GAP: f32 = 6.0;
/// The header's word on its right: the title's room less this.
const TITLE_GAP: f32 = 12.0;
const NAME: f32 = 13.0;
const MASTER: f32 = 36.0;
const BUTTON: f32 = 32.0;
const BUTTON_RADIUS: f32 = 8.0;
const SLIDER: f32 = 24.0;
const TRACK_HEIGHT: f32 = 8.0;
const KNOB: f32 = 16.0;
const VALUE_BOX: f32 = 36.0;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
const CHECK: f32 = 16.0;
/// The microphones' rows: their names 48 px into the row.
const MIC_INDENT: f32 = 48.0;
/// An application's row: its tile, its name over a slimmer slider.
const APP_ROW: f32 = 46.0;
const TILE: f32 = BUTTON;
const APP_NAME: f32 = 13.5;
const APP_SLIDER: f32 = 18.0;
const APP_TRACK: f32 = 6.0;
const APP_KNOB: f32 = 14.0;
/// Between the name and its slider.
const APP_NAME_GAP: f32 = 2.0;
/// The value sits on the row's bottom, 2 px up.
const APP_VALUE_PAD: f32 = 2.0;
const LETTER: f32 = 14.0;
/// "No application…", and the others' number: a note, 2 px under it.
const NOTE_BOTTOM: f32 = 2.0;
/// Applications listed at most; the others are counted under them.
pub const MOST_APPLICATIONS: usize = 5;

const MASTER_TOP: f32 = HEADER_TOP + HEADER + GAP;
const RULE_TOP: f32 = MASTER_TOP + MASTER + GAP + RULE;
const LABEL_TOP: f32 = RULE_TOP + 1.0 + RULE + GAP;
const ROWS_TOP: f32 = LABEL_TOP + LABEL_PAD * 2.0 + LABEL * LINE + GAP;

const BUTTON_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
/// The microphone muted: its button tinted, its icon `gauge.critical`'s
/// light end.
const MIC_OFF_FILL: Color = Color::from_rgba8(229, 121, 63, 0.18);
const MIC_OFF_HOVER: Color = Color::from_rgba8(229, 121, 63, 0.26);
const MIC_OFF: Color = rgb(0xF7B98A);
/// A muted application's tile.
const TILE_MUTED: f32 = 0.45;

/// A slider shows the volume asked this long at most, until PipeWire says
/// it.
const ASKED_FOR: Duration = Duration::from_secs(1);

/// What the popover lists, and so its height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Layout {
    pub outputs: usize,
    /// The microphones: none hides the section, one shows no list.
    pub microphones: usize,
    /// The applications' rows, and whether others are counted under them.
    pub applications: usize,
    pub more: bool,
}

/// The height of `n` rows `height` tall, `gap` apart.
fn rows(n: usize, height: f32, gap: f32) -> f32 {
    if n == 0 {
        0.0
    } else {
        n as f32 * (height + gap) - gap
    }
}

/// A section's divider after `end`, its label's top and its content's.
fn section(end: f32) -> (f32, f32, f32) {
    let rule = end + GAP + RULE;
    let label = rule + 1.0 + RULE + GAP;
    (rule, label, label + LABEL_PAD * 2.0 + LABEL * LINE + GAP)
}

impl Layout {
    /// What `mixer` shows with `listed` applications listed.
    pub fn of(mixer: &Mixer, listed: usize) -> Self {
        Self {
            outputs: mixer.sinks.len(),
            microphones: mixer.sources.len(),
            applications: listed.min(MOST_APPLICATIONS),
            more: listed > MOST_APPLICATIONS,
        }
    }

    fn outputs_end(self) -> f32 {
        ROWS_TOP + rows(self.outputs, ROW, ROW_GAP)
    }

    /// The microphone's section, if any.
    fn microphone(self) -> Option<(f32, f32, f32)> {
        (self.microphones > 0).then(|| section(self.outputs_end()))
    }

    /// Its row: mute, slider, value.
    fn mic_row(self) -> f32 {
        self.microphone().map_or(0.0, |(_, _, top)| top)
    }

    /// Its list, when there are several.
    fn mics_top(self) -> f32 {
        self.mic_row() + MASTER + GAP
    }

    fn mic_end(self) -> f32 {
        match self.microphones {
            0 => self.outputs_end(),
            1 => self.mic_row() + MASTER,
            n => self.mics_top() + rows(n, ROW, ROW_GAP),
        }
    }

    fn applications_section(self) -> (f32, f32, f32) {
        section(self.mic_end())
    }

    fn app_row(self, i: usize) -> f32 {
        self.applications_section().2 + i as f32 * (APP_ROW + GAP)
    }

    /// The line under the applications' rows ("No application…", or the
    /// others' number): its top.
    fn note_top(self) -> f32 {
        match self.applications {
            0 => self.applications_section().2,
            n => self.app_row(n),
        }
    }

    fn end(self) -> f32 {
        if self.applications == 0 || self.more {
            self.note_top() + NOTE_TOP + NOTE * LINE + NOTE_BOTTOM
        } else {
            self.app_row(self.applications) - GAP
        }
    }

    /// The popover's height, logical pixels.
    pub fn height(self) -> u32 {
        kit::height(self.end())
    }
}

// --- The applications listed, kept while the popover is open -----------------

static LISTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The applications' keys listed now, from `apps`: on `opening`, those
/// playing; else those listed, less those gone, with those that started
/// to play.
fn relist(listed: &mut Vec<String>, apps: &[App], opening: bool) {
    if opening {
        listed.clear();
    }
    listed.retain(|key| apps.iter().any(|a| a.key == *key));
    for app in apps.iter().filter(|a| a.playing) {
        if !listed.contains(&app.key) {
            listed.push(app.key.clone());
        }
    }
}

/// The popover's layout now, its list of applications made again (from
/// scratch when `opening`): the popover's controller asks, as it opens it
/// and when the sound changes.
pub fn layout(opening: bool) -> Layout {
    let mixer = levels::sound::mixer();
    let mut listed = LISTED.lock().unwrap_or_else(PoisonError::into_inner);
    relist(&mut listed, &mixer.apps, opening);
    Layout::of(&mixer, listed.len())
}

/// The applications listed, in their order, the others' number.
fn listed(mixer: &Mixer) -> (Vec<App>, usize) {
    let listed = LISTED.lock().unwrap_or_else(PoisonError::into_inner);
    let mut apps: Vec<App> = listed
        .iter()
        .filter_map(|key| mixer.apps.iter().find(|a| a.key == *key).cloned())
        .collect();
    let more = apps.len().saturating_sub(MOST_APPLICATIONS);
    apps.truncate(MOST_APPLICATIONS);
    (apps, more)
}

/// The sound's popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
}

/// What can be pointed at, or hold the keyboard's focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Mute,
    Slider,
    Output(usize),
    MicMute,
    MicSlider,
    Mic(usize),
    Tile(usize),
    AppSlider(usize),
}

/// A slider, by what it sets: an application's by its key, so that it
/// stays the same when the list changes under it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slider {
    Master,
    Mic,
    App(String),
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// What it shows, as last drawn: the outputs, the microphones, the
    /// applications listed and the others' number, the default output's
    /// sound.
    mixer: Mixer,
    apps: Vec<App>,
    more: usize,
    sound: Option<Sound>,
    layout: Layout,
    /// The names as they fit (outputs', microphones', applications'), the
    /// title's output name, and the labels' letters' places.
    names: Vec<String>,
    mic_names: Vec<String>,
    app_names: Vec<String>,
    header: String,
    labels: [Vec<(char, f32)>; 3],
    /// What the names were made for.
    made_for: Vec<String>,
    /// The `0`'s advance of the value's font, for tabular figures.
    zero: Option<f32>,
    pointer: Pointer<Target>,
    dragging: Option<Slider>,
    /// A slider's volume while dragged, or asked and not yet said by
    /// PipeWire, and since when.
    asked: Option<((Slider, f32), Instant)>,
    focus: Option<Target>,
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(now) => self.refresh(state, now),
            Input::Mouse(event, at) => self.mouse(state, event, at),
            Input::Key(pressed) => self.key(state, &pressed),
            Input::Nothing => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let layout = state.layout;
        kit::background(&mut frame, bounds.size(), self.glass);
        kit::title(&mut frame, self.words.sound());
        kit::title_aside(&mut frame, state.header.clone());
        self.master(&mut frame, state);
        label(&mut frame, &state.labels[0], RULE_TOP, LABEL_TOP);
        for (i, sink) in state.mixer.sinks.iter().enumerate() {
            self.output(&mut frame, state, i, sink);
        }
        if let Some((rule, top, _)) = layout.microphone() {
            label(&mut frame, &state.labels[1], rule, top);
            self.microphone(&mut frame, state);
        }
        let (rule, top, _) = layout.applications_section();
        label(&mut frame, &state.labels[2], rule, top);
        self.applications(&mut frame, state);
        if let Some(target) = state.focus {
            let (rect, radius) = target.shape(layout);
            ring(&mut frame, rect, radius);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        state.pointer.interaction(state.dragging.is_some())
    }
}

/// A section's divider at `rule` and its label's letters, from `top`.
fn label(f: &mut Frame, letters: &[(char, f32)], rule: f32, top: f32) {
    kit::rule(f, rule);
    kit::label(f, letters, kit::label_middle(top));
}

impl Layout {
    /// The application's row `i`'s parts: its name's middle, its slider.
    fn app_parts(self, i: usize) -> (f32, Track) {
        let column = APP_NAME * LINE + APP_NAME_GAP + APP_SLIDER;
        let top = self.app_row(i) + (APP_ROW - column) / 2.0;
        let track = Track {
            left: app_left(),
            right: slider_right(),
            middle: top + APP_NAME * LINE + APP_NAME_GAP + APP_SLIDER / 2.0,
            height: APP_TRACK,
            knob: APP_KNOB,
        };
        (top + APP_NAME * LINE / 2.0, track)
    }

    /// The master's slider, or the microphone's.
    fn track(self, mic: bool) -> Track {
        let top = if mic { self.mic_row() } else { MASTER_TOP };
        Track {
            left: slider_left(),
            right: slider_right(),
            middle: top + MASTER / 2.0,
            height: TRACK_HEIGHT,
            knob: KNOB,
        }
    }
}

impl Target {
    /// Where it is, and its corners' radius.
    fn shape(self, layout: Layout) -> (Rectangle, f32) {
        let button = |top: f32| {
            Rectangle::new(
                Point::new(LEFT, top + (MASTER - BUTTON) / 2.0),
                Size::new(BUTTON, BUTTON),
            )
        };
        match self {
            Self::Mute => (button(MASTER_TOP), BUTTON_RADIUS),
            Self::Slider => (layout.track(false).rect(SLIDER), SLIDER / 2.0),
            Self::Output(i) => (row_rect(ROWS_TOP, i), BUTTON_RADIUS),
            Self::MicMute => (button(layout.mic_row()), BUTTON_RADIUS),
            Self::MicSlider => (layout.track(true).rect(SLIDER), SLIDER / 2.0),
            Self::Mic(i) => (row_rect(layout.mics_top(), i), BUTTON_RADIUS),
            Self::Tile(i) => (
                Rectangle::new(
                    Point::new(LEFT, layout.app_row(i) + (APP_ROW - TILE) / 2.0),
                    Size::new(TILE, TILE),
                ),
                BUTTON_RADIUS,
            ),
            Self::AppSlider(i) => (layout.app_parts(i).1.rect(APP_SLIDER), APP_SLIDER / 2.0),
        }
    }
}

fn slider_left() -> f32 {
    LEFT + BUTTON + 8.0
}

fn slider_right() -> f32 {
    RIGHT - VALUE_BOX - 8.0
}

fn app_left() -> f32 {
    LEFT + TILE + 8.0
}

/// Row `i` of a list from `top`.
fn row_rect(top: f32, i: usize) -> Rectangle {
    Rectangle::new(
        Point::new(LEFT - ROW_OUT, top + i as f32 * (ROW + ROW_GAP)),
        Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, ROW),
    )
}

impl State {
    /// What can be pointed at, in the keyboard's order.
    fn targets(&self) -> Vec<Target> {
        let mics = self.mixer.sources.len();
        let mut order = vec![Target::Mute, Target::Slider];
        order.extend((0..self.mixer.sinks.len()).map(Target::Output));
        if mics > 0 {
            order.extend([Target::MicMute, Target::MicSlider]);
        }
        if mics > 1 {
            order.extend((0..mics).map(Target::Mic));
        }
        for i in 0..self.apps.len() {
            order.extend([Target::Tile(i), Target::AppSlider(i)]);
        }
        order
    }

    /// What is under `at`.
    fn target_at(&self, at: Point) -> Option<Target> {
        self.targets()
            .into_iter()
            .find(|t| t.shape(self.layout).0.contains(at))
    }

    /// The sound a slider sets, as PipeWire says it.
    fn said(&self, slider: &Slider) -> Option<Sound> {
        match slider {
            Slider::Master => self.sound,
            Slider::Mic => self.mixer.microphone().and_then(|m| m.sound),
            Slider::App(key) => self
                .mixer
                .apps
                .iter()
                .find(|a| a.key == *key)
                .and_then(|a| a.sound),
        }
    }

    /// A slider's volume as shown: the one asked, else PipeWire's.
    fn shown(&self, slider: &Slider) -> f32 {
        match &self.asked {
            Some(((asked, volume), _)) if asked == slider => *volume,
            _ => self.said(slider).map_or(0.0, |s| s.volume),
        }
        .clamp(0.0, 100.0)
    }

    fn slider_of(&self, target: Target) -> Option<(Slider, Track)> {
        match target {
            Target::Slider => Some((Slider::Master, self.layout.track(false))),
            Target::MicSlider => Some((Slider::Mic, self.layout.track(true))),
            Target::AppSlider(i) => {
                let app = self.apps.get(i)?;
                Some((Slider::App(app.key.clone()), self.layout.app_parts(i).1))
            }
            _ => None,
        }
    }

    fn track_of(&self, slider: &Slider) -> Option<Track> {
        match slider {
            Slider::Master => Some(self.layout.track(false)),
            Slider::Mic => Some(self.layout.track(true)),
            Slider::App(key) => {
                let i = self.apps.iter().position(|a| a.key == *key)?;
                Some(self.layout.app_parts(i).1)
            }
        }
    }
}

/// Asks for `slider`'s volume `volume`, unmuting what it sets: the master
/// through the default sink's asks, the others on their nodes.
fn ask_volume(mixer: &Mixer, slider: &Slider, volume: f32) {
    let nodes: Vec<(u32, bool)> = match slider {
        Slider::Master => {
            levels::ask(Ask::Volume(volume));
            return;
        }
        Slider::Mic => mixer
            .microphone()
            .map(|m| (m.id, m.sound.is_some_and(|s| s.muted)))
            .into_iter()
            .collect(),
        Slider::App(key) => mixer
            .apps
            .iter()
            .filter(|a| a.key == *key)
            .flat_map(|a| {
                let muted = a.sound.is_some_and(|s| s.muted);
                a.streams.iter().map(move |id| (*id, muted))
            })
            .collect(),
    };
    for (id, muted) in nodes {
        levels::ask(Ask::Node(id, Set::Volume(volume)));
        if muted {
            levels::ask(Ask::Node(id, Set::Mute(false)));
        }
    }
}

impl Panel {
    /// What the popover shows, read afresh; the names made to fit when
    /// they changed.
    fn refresh<Message>(&self, state: &mut State, now: Instant) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        state.mixer = levels::sound::mixer();
        (state.apps, state.more) = listed(&state.mixer);
        state.layout = Layout::of(&state.mixer, state.apps.len() + state.more);
        state.sound = levels::snapshot().sound;
        let default = state.mixer.sinks.iter().position(|s| s.default);
        let made_for: Vec<String> = (state.mixer.sinks.iter().map(|s| &s.description))
            .chain(std::iter::once(&String::new()))
            .chain(state.mixer.sources.iter().map(|s| &s.description))
            .chain(std::iter::once(&String::new()))
            .chain(state.apps.iter().map(|a| &a.name))
            .cloned()
            .chain(std::iter::once(format!("{default:?}")))
            .collect();
        if state.made_for != made_for || state.labels[0].is_empty() {
            self.fit(state);
            state.made_for = made_for;
        }
        if state
            .focus
            .is_some_and(|focus| !state.targets().contains(&focus))
        {
            state.focus = None;
        }
        // The volume asked stays shown until PipeWire says it, or a while.
        if state.dragging.is_some() {
            return None;
        }
        let said = state.asked.as_ref().is_some_and(|((slider, asked), _)| {
            state
                .said(slider)
                .is_some_and(|s| (s.volume.round() - asked).abs() < 0.5)
        });
        kit::awaited(&mut state.asked, said, now, ASKED_FOR).map(canvas::Action::request_redraw_at)
    }

    /// The names elided to fit, the title's output name too, and the
    /// labels' letters spaced.
    fn fit(&self, state: &mut State) {
        let outputs: Vec<String> = state
            .mixer
            .sinks
            .iter()
            .map(|s| s.description.clone())
            .collect();
        let mics: Vec<String> = state
            .mixer
            .sources
            .iter()
            .map(|s| s.description.clone())
            .collect();
        let apps: Vec<String> = state.apps.iter().map(|a| a.name.clone()).collect();
        let Ok(mut system) = iced::advanced::graphics::text::font_system().write() else {
            (state.names, state.mic_names, state.app_names) = (outputs, mics, apps);
            return;
        };
        let system = system.raw();
        let mut fit = |names: &[String], room: f32, size: f32| -> Vec<String> {
            names
                .iter()
                .map(|n| kit::elide(n, room, |t| kit::measure_in(system, t, size, MANROPE)))
                .collect()
        };
        // Between the icon (or the indent) and the check, each with its gap.
        let checked = RIGHT - CHECK - ICON_GAP;
        state.names = fit(&outputs, checked - (LEFT + ICON + ICON_GAP), ROW_TEXT);
        state.mic_names = fit(&mics, checked - (LEFT - ROW_OUT + MIC_INDENT), ROW_TEXT);
        state.app_names = fit(&apps, slider_right() - app_left(), APP_NAME);
        let title = kit::measure_in(system, self.words.sound(), TITLE, MANROPE_BOLD);
        let default = state
            .mixer
            .sinks
            .iter()
            .find(|s| s.default)
            .map(|s| s.description.clone())
            .unwrap_or_default();
        state.header = kit::elide(&default, RIGHT - LEFT - title - TITLE_GAP, |t| {
            kit::measure_in(system, t, NAME, MANROPE)
        });
        let words = [
            self.words.output(),
            self.words.microphone(),
            self.words.applications(),
        ];
        state.labels = words
            .map(|word| kit::spaced(word, |ch| kit::measure_in(system, ch, LABEL, MANROPE_BOLD)));
        state.zero = kit::zero(system);
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| state.target_at(at));
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                if let Some(slider) = state.dragging.clone()
                    && let Some(at) = at
                {
                    self.slide(state, slider, at.x);
                    return redraw;
                }
                if state.pointer.over(target) {
                    return redraw;
                }
                None
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pointer.pressed = target;
                if let Some((slider, _)) = target.and_then(|t| state.slider_of(t))
                    && let Some(at) = at
                {
                    state.dragging = Some(slider.clone());
                    self.slide(state, slider, at.x);
                }
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                let clicked = state.pointer.release(target);
                if state.dragging.take().is_some() {
                    if let Some((_, since)) = &mut state.asked {
                        *since = Instant::now();
                    }
                } else if clicked.is_some() {
                    self.act(state, clicked);
                }
                redraw
            }
            _ => None,
        }
    }

    /// Slider `slider` moved to `x`: that volume, asked at once.
    fn slide(&self, state: &mut State, slider: Slider, x: f32) {
        let Some(track) = state.track_of(&slider) else {
            return;
        };
        let volume = track.volume_at(x);
        let same = matches!(&state.asked, Some(((s, v), _)) if *s == slider && *v == volume);
        if !same {
            ask_volume(&state.mixer, &slider, volume);
        }
        state.asked = Some(((slider, volume), Instant::now()));
    }

    fn act(&self, state: &State, target: Option<Target>) {
        match target {
            Some(Target::Mute) => levels::key(LevelKey::Mute),
            Some(Target::Output(i)) => {
                if let Some(sink) = state.mixer.sinks.get(i).filter(|s| !s.default) {
                    levels::ask(Ask::Default(sink.id));
                }
            }
            Some(Target::MicMute) => {
                if let Some(mic) = state.mixer.microphone() {
                    let muted = mic.sound.is_some_and(|s| s.muted);
                    levels::ask(Ask::Node(mic.id, Set::Mute(!muted)));
                }
            }
            Some(Target::Mic(i)) => {
                if let Some(mic) = state.mixer.sources.get(i).filter(|s| !s.default) {
                    levels::ask(Ask::DefaultSource(mic.id));
                }
            }
            Some(Target::Tile(i)) => {
                if let Some(app) = state.apps.get(i) {
                    let muted = app.sound.is_some_and(|s| s.muted);
                    for id in &app.streams {
                        levels::ask(Ask::Node(*id, Set::Mute(!muted)));
                    }
                }
            }
            Some(Target::Slider | Target::MicSlider | Target::AppSlider(_)) | None => {}
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => {
                state.focus = kit::step(&state.targets(), state.focus, pressed.modifiers.shift());
            }
            Key::Named(Named::Enter | Named::Space) => self.act(state, state.focus),
            Key::Named(
                arrow @ (Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown),
            ) => {
                let up = matches!(arrow, Named::ArrowRight | Named::ArrowUp);
                let vertical = matches!(arrow, Named::ArrowUp | Named::ArrowDown);
                // Through a list: the next row, round.
                let next = |i: usize, n: usize| if up { i + n - 1 } else { i + 1 } % n.max(1);
                match state.focus {
                    Some(Target::Slider) => {
                        state.asked = None;
                        levels::key(if up {
                            LevelKey::VolumeUp
                        } else {
                            LevelKey::VolumeDown
                        });
                    }
                    Some(focus @ (Target::MicSlider | Target::AppSlider(_))) => {
                        let (slider, _) = state.slider_of(focus)?;
                        let by = if up { STEP } else { -STEP };
                        let volume = (state.shown(&slider) + by).clamp(0.0, 100.0);
                        ask_volume(&state.mixer, &slider, volume);
                        state.asked = Some(((slider, volume), Instant::now()));
                    }
                    Some(Target::Output(i)) if vertical => {
                        state.focus = Some(Target::Output(next(i, state.mixer.sinks.len())));
                    }
                    Some(Target::Mic(i)) if vertical => {
                        state.focus = Some(Target::Mic(next(i, state.mixer.sources.len())));
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }

    fn master(&self, f: &mut Frame, state: &State) {
        let muted = state.sound.is_some_and(|s| s.muted);
        let (button, radius) = Target::Mute.shape(state.layout);
        let fill = if state.pointer.hover == Some(Target::Mute) {
            BUTTON_HOVER
        } else {
            FILL
        };
        f.fill(&rounded(button, radius), fill);
        let icon = Point::new(
            button.center_x() - ICON / 2.0,
            button.center_y() - ICON / 2.0,
        );
        speaker(f, icon, muted);
        let track = state.layout.track(false);
        let volume = state.shown(&Slider::Master);
        slide(f, track, volume, muted);
        let at = Point::new(RIGHT, track.middle);
        let word = state.sound.filter(|s| s.muted).map(|_| self.words.muted());
        if state.sound.is_some() {
            value(f, state, at, word, volume, DIM);
        }
    }

    fn output(&self, f: &mut Frame, state: &State, i: usize, sink: &Sink) {
        let rect = row_rect(ROWS_TOP, i);
        kit::row_fill(
            f,
            rect,
            sink.default,
            state.pointer.hover == Some(Target::Output(i)),
        );
        let middle = rect.center_y();
        device(f, Point::new(LEFT, middle - ICON / 2.0), sink.kind);
        let name = state.names.get(i).unwrap_or(&sink.description);
        f.fill_text(text(
            name.clone(),
            Point::new(LEFT + ICON + ICON_GAP, middle),
            ROW_TEXT,
            TEXT,
            Alignment::Left,
        ));
        if sink.default {
            check(f, Point::new(RIGHT - CHECK, middle - CHECK / 2.0));
        }
    }

    fn microphone(&self, f: &mut Frame, state: &State) {
        let Some(mic) = state.mixer.microphone() else {
            return;
        };
        let muted = mic.sound.is_some_and(|s| s.muted);
        let (button, radius) = Target::MicMute.shape(state.layout);
        let hover = state.pointer.hover == Some(Target::MicMute);
        let fill = match (muted, hover) {
            (true, false) => MIC_OFF_FILL,
            (true, true) => MIC_OFF_HOVER,
            (false, false) => FILL,
            (false, true) => BUTTON_HOVER,
        };
        f.fill(&rounded(button, radius), fill);
        let icon = Point::new(
            button.center_x() - ICON / 2.0,
            button.center_y() - ICON / 2.0,
        );
        microphone(f, icon, muted);
        let track = state.layout.track(true);
        let volume = state.shown(&Slider::Mic);
        slide(f, track, volume, muted);
        let word = muted.then(|| self.words.off());
        value(f, state, Point::new(RIGHT, track.middle), word, volume, DIM);
        if state.mixer.sources.len() < 2 {
            return;
        }
        for (i, source) in state.mixer.sources.iter().enumerate() {
            let rect = row_rect(state.layout.mics_top(), i);
            let hover = state.pointer.hover == Some(Target::Mic(i));
            kit::row_fill(f, rect, source.default, hover);
            let name = state.mic_names.get(i).unwrap_or(&source.description);
            f.fill_text(text(
                name.clone(),
                Point::new(rect.x + MIC_INDENT, rect.center_y()),
                ROW_TEXT,
                TEXT,
                Alignment::Left,
            ));
            if source.default {
                check(f, Point::new(RIGHT - CHECK, rect.center_y() - CHECK / 2.0));
            }
        }
    }

    fn applications(&self, f: &mut Frame, state: &State) {
        let layout = state.layout;
        let master_muted = state.sound.is_some_and(|s| s.muted);
        for (i, app) in state.apps.iter().enumerate() {
            let muted = app.sound.is_some_and(|s| s.muted);
            let (tile, _) = Target::Tile(i).shape(layout);
            self.tile(f, tile, app, muted);
            let (name_middle, track) = layout.app_parts(i);
            let name = state.app_names.get(i).unwrap_or(&app.name);
            f.fill_text(text(
                name.clone(),
                Point::new(track.left, name_middle),
                APP_NAME,
                if muted { FAINT } else { TEXT },
                Alignment::Left,
            ));
            let volume = state.shown(&Slider::App(app.key.clone()));
            slide(f, track, volume, muted || master_muted);
            let middle = layout.app_row(i) + APP_ROW - APP_VALUE_PAD - VALUE * LINE / 2.0;
            let word = muted.then(|| self.words.muted());
            let colour = if master_muted { FAINT } else { DIM };
            value(f, state, Point::new(RIGHT, middle), word, volume, colour);
        }
        let note = match (state.apps.is_empty(), state.more) {
            (true, _) => Some(self.words.no_application().to_owned()),
            (false, 0) => None,
            (false, more) => Some(self.words.more_applications(more)),
        };
        if let Some(note) = note {
            f.fill_text(kit::note(note, layout.note_top()));
        }
    }

    /// An application's tile: its icon, else its letter on a colour of its
    /// own; faded when muted.
    fn tile(&self, f: &mut Frame, rect: Rectangle, app: &App, muted: bool) {
        let opacity = if muted { TILE_MUTED } else { 1.0 };
        kit::app_tile(
            f,
            rect,
            (app.icon.as_ref(), &app.name),
            (BUTTON_RADIUS, LETTER),
            opacity,
        );
    }
}

/// A slider at `volume`: its track, its fill to the knob (grey when what
/// it sets is muted), its knob.
fn slide(f: &mut Frame, track: Track, volume: f32, grey: bool) {
    kit::slider(f, track, volume / 100.0, (!grey).then_some(SOUND));
}

/// A value right-aligned on `at`: `word` in `text.faint` (muted, off),
/// else the volume in tabular figures, in `colour`.
fn value(f: &mut Frame, state: &State, at: Point, word: Option<&str>, volume: f32, colour: Color) {
    if let Some(word) = word {
        f.fill_text(text(word.to_owned(), at, VALUE, FAINT, Alignment::Right));
        return;
    }
    kit::figures(f, at, volume, state.zero.unwrap_or(kit::ZERO), colour);
}

// --- Icons, from the mockup's SVGs (24 units, drawn 18 px) --------------------

/// Lines through `points` (SVG units from `at`, `scale` px a unit).
fn lines(f: &mut Frame, at: Point, scale: f32, width: f32, colour: Color, points: &[(f32, f32)]) {
    let p = |(x, y): (f32, f32)| Point::new(at.x + x * scale, at.y + y * scale);
    let path = Path::new(|b| {
        if let Some((first, rest)) = points.split_first() {
            b.move_to(p(*first));
            for point in rest {
                b.line_to(p(*point));
            }
        }
    });
    f.stroke(&path, stroke(width * scale, colour));
}

/// An arc of an SVG `a r r 0 0 1` from `(x, y)` down by `chord`, bulging
/// right: the speaker's waves.
fn wave(
    f: &mut Frame,
    at: Point,
    scale: f32,
    (x, y): (f32, f32),
    r: f32,
    chord: f32,
    colour: Color,
) {
    let half = chord / 2.0;
    let centre_x = x - (r * r - half * half).sqrt();
    let angle = (half / r).asin();
    let arc = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center: Point::new(at.x + centre_x * scale, at.y + (y + half) * scale),
            radius: r * scale,
            start_angle: Radians(-angle),
            end_angle: Radians(angle),
        });
    });
    f.stroke(&arc, stroke(2.0 * scale, colour));
}

fn speaker(f: &mut Frame, at: Point, muted: bool) {
    let s = ICON / 24.0;
    let colour = if muted { FAINT } else { TEXT };
    let cone = [
        (11.0, 5.0),
        (6.0, 9.0),
        (2.0, 9.0),
        (2.0, 15.0),
        (6.0, 15.0),
        (11.0, 19.0),
        (11.0, 5.0),
        (6.0, 9.0),
    ];
    lines(f, at, s, 2.0, colour, &cone);
    if muted {
        lines(f, at, s, 2.0, colour, &[(16.0, 9.0), (21.0, 15.0)]);
        lines(f, at, s, 2.0, colour, &[(21.0, 9.0), (16.0, 15.0)]);
    } else {
        // `M15.5 8.5a5 5 0 0 1 0 7` and `M18.5 6a9 9 0 0 1 0 12`.
        wave(f, at, s, (15.5, 8.5), 5.0, 7.0, colour);
        wave(f, at, s, (18.5, 6.0), 9.0, 12.0, colour);
    }
}

fn device(f: &mut Frame, at: Point, kind: Kind) {
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, DIM);
    let rect = |f: &mut Frame, x: f32, y: f32, w: f32, h: f32, r: f32| {
        f.stroke(
            &Path::rounded_rectangle(p(x, y), Size::new(w * s, h * s), (r * s).into()),
            line,
        );
    };
    match kind {
        Kind::Speaker => {
            rect(f, 3.0, 7.0, 18.0, 11.0, 2.0);
            lines(f, at, s, 2.0, DIM, &[(7.0, 21.0), (17.0, 21.0)]);
        }
        Kind::Headphones => {
            // `M4 15v-3a8 8 0 0 1 16 0v3`.
            let band = Path::new(|b| {
                b.move_to(p(4.0, 15.0));
                b.line_to(p(4.0, 12.0));
                b.arc(canvas::path::Arc {
                    center: p(12.0, 12.0),
                    radius: 8.0 * s,
                    start_angle: Radians(std::f32::consts::PI),
                    end_angle: Radians(2.0 * std::f32::consts::PI),
                });
                b.line_to(p(20.0, 15.0));
            });
            f.stroke(&band, line);
            rect(f, 3.0, 14.0, 4.0, 6.0, 1.5);
            rect(f, 17.0, 14.0, 4.0, 6.0, 1.5);
        }
        Kind::Screen => {
            rect(f, 2.0, 4.0, 20.0, 13.0, 2.0);
            lines(f, at, s, 2.0, DIM, &[(8.0, 21.0), (16.0, 21.0)]);
            lines(f, at, s, 2.0, DIM, &[(12.0, 17.0), (12.0, 21.0)]);
        }
    }
}

/// The default output's check, 16 px: `M5 12.5 10 17 19 7`.
fn check(f: &mut Frame, at: Point) {
    let s = CHECK / 24.0;
    lines(
        f,
        at,
        s,
        2.6,
        ACCENT,
        &[(5.0, 12.5), (10.0, 17.0), (19.0, 7.0)],
    );
}

/// The microphone: `M5 11a7 7 0 0 0 14 0`, its capsule and its foot;
/// crossed out in `MIC_OFF` when muted.
fn microphone(f: &mut Frame, at: Point, muted: bool) {
    let s = ICON / 24.0;
    let colour = if muted { MIC_OFF } else { TEXT };
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    f.stroke(
        &Path::rounded_rectangle(p(9.0, 3.0), Size::new(6.0 * s, 11.0 * s), (3.0 * s).into()),
        line,
    );
    let cup = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center: p(12.0, 11.0),
            radius: 7.0 * s,
            start_angle: Radians(0.0),
            end_angle: Radians(std::f32::consts::PI),
        });
    });
    f.stroke(&cup, line);
    lines(f, at, s, 2.0, colour, &[(12.0, 18.0), (12.0, 21.0)]);
    if muted {
        lines(f, at, s, 2.0, colour, &[(4.0, 4.0), (20.0, 20.0)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(outputs: usize, microphones: usize, listed: usize) -> Layout {
        Layout {
            outputs,
            microphones,
            applications: listed.min(MOST_APPLICATIONS),
            more: listed > MOST_APPLICATIONS,
        }
    }

    #[test]
    fn its_height_follows_what_it_lists() {
        // The mockup's, at ×1: the outputs' list ends 15 px above the
        // bottom of 3.4a's (261 px with three outputs, 185 with one).
        assert_eq!(layout(3, 0, 0).outputs_end().round() + 15.0, 261.0);
        assert_eq!(layout(1, 0, 0).outputs_end().round() + 15.0, 185.0);
        // Then each section: its divider, its label, its rows.
        assert_eq!(layout(3, 0, 0).height(), 332);
        assert_eq!(layout(1, 0, 0).height(), 256);
        // One microphone: its row; two, their list.
        assert_eq!(layout(3, 1, 0).height(), 332 + 47 + 36);
        assert_eq!(layout(3, 2, 0).height(), 332 + 47 + 36 + 6 + 74);
        // Two applications instead of "No application…".
        assert_eq!(layout(3, 2, 2).height(), 568);
        // At most five, and a line for the others.
        assert_eq!(layout(3, 2, 5).height(), layout(3, 2, 2).height() + 3 * 52);
        assert_eq!(layout(3, 2, 7).height(), 755);
    }

    #[test]
    fn the_sliders_cover_0_to_100() {
        let master = layout(3, 1, 1).track(false);
        let (left, right) = (master.left, master.right);
        assert_eq!(right - left, 166.0);
        assert_eq!(master.volume_at(left), 0.0);
        assert_eq!(master.volume_at(left + KNOB / 2.0), 0.0);
        assert_eq!(master.volume_at(right - KNOB / 2.0), 100.0);
        assert_eq!(master.volume_at(right + 50.0), 100.0);
        assert_eq!(master.volume_at((left + right) / 2.0), 50.0);
        // An application's: from its tile to the value, its knob 14 px.
        let (_, app) = layout(3, 1, 1).app_parts(0);
        assert_eq!((app.left, app.right), (left, right));
        assert_eq!(app.volume_at(left + APP_KNOB / 2.0), 0.0);
        assert_eq!(app.volume_at(right - APP_KNOB / 2.0), 100.0);
    }

    #[test]
    fn its_rows_and_controls_lie_inside_it_and_apart() {
        for layout in [layout(3, 2, 2), layout(1, 1, 0), layout(2, 0, 7)] {
            let height = layout.height() as f32;
            let mut targets = vec![Target::Mute, Target::Slider, Target::Output(0)];
            if layout.microphones > 0 {
                targets.extend([Target::MicMute, Target::MicSlider]);
            }
            if layout.microphones > 1 {
                targets.extend([Target::Mic(0), Target::Mic(layout.microphones - 1)]);
            }
            for i in 0..layout.applications {
                targets.extend([Target::Tile(i), Target::AppSlider(i)]);
            }
            let out = crate::levels::draw::RING_OUT + crate::levels::draw::RING;
            for target in &targets {
                let (rect, _) = target.shape(layout);
                // With the keyboard's ring.
                assert!(rect.x - out >= 0.0, "{target:?}");
                assert!(
                    rect.x + rect.width + out <= hocus_link::POPOVER_WIDTH as f32,
                    "{target:?}"
                );
                assert!(
                    rect.y + rect.height <= height - kit::PAD_BOTTOM,
                    "{target:?}"
                );
                for other in targets.iter().filter(|o| *o != target) {
                    let (o, _) = other.shape(layout);
                    assert!(rect.intersection(&o).is_none(), "{target:?} on {other:?}");
                }
            }
        }
    }

    fn app(key: &str, playing: bool) -> App {
        App {
            key: key.to_owned(),
            name: key.to_owned(),
            icon: None,
            streams: vec![1],
            sound: None,
            playing,
        }
    }

    #[test]
    fn the_applications_playing_are_listed_and_kept_while_it_is_open() {
        let mut listed = vec!["old".to_owned()];
        // On opening: those playing, not a paused one.
        let apps = [
            app("firefox", true),
            app("paused", false),
            app("music", true),
        ];
        relist(&mut listed, &apps, true);
        assert_eq!(listed, ["firefox", "music"]);
        // Firefox pauses: it stays; another starts: it comes last.
        let apps = [app("firefox", false), app("music", true), app("mpv", true)];
        relist(&mut listed, &apps, false);
        assert_eq!(listed, ["firefox", "music", "mpv"]);
        // Its stream gone, Firefox goes.
        relist(&mut listed, &apps[1..], false);
        assert_eq!(listed, ["music", "mpv"]);
        // Opened again: those playing only.
        let apps = [app("music", false), app("mpv", true)];
        relist(&mut listed, &apps, true);
        assert_eq!(listed, ["mpv"]);
    }
}
