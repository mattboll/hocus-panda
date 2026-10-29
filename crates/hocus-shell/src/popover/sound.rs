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

use hocus_link::{LevelKey, POPOVER_WIDTH, RADIUS};
use iced::{
    Color, Pixels, Point, Radians, Rectangle, Renderer, Size, Theme, alignment, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, gradient},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{Event as Seen, event};
use crate::levels::{
    self,
    draw::{
        ACCENT, DIM, FAINT, HOVER, LINE, MANROPE, MANROPE_BOLD, MUTED_FILL, SOUND, TEXT, TRACK,
        VALUE, cell, rgb, ring, stroke,
    },
    icons::Icon,
    model::{STEP, Sound, Words},
    sound::{App, Ask, Kind, Mixer, Set, Sink},
};

// --- The mockup's sizes, at ×1 ------------------------------------------------

const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 12.0;
const PAD_SIDE: f32 = 14.0;
const PAD_BOTTOM: f32 = 14.0;
const GAP: f32 = 6.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = POPOVER_WIDTH as f32 - BORDER - PAD_SIDE;
const HEADER: f32 = 32.0;
const TITLE: f32 = 16.0;
const NAME: f32 = 13.0;
const MASTER: f32 = 36.0;
const BUTTON: f32 = 32.0;
const BUTTON_RADIUS: f32 = 8.0;
const ICON: f32 = 18.0;
const SLIDER: f32 = 24.0;
const TRACK_HEIGHT: f32 = 8.0;
const KNOB: f32 = 16.0;
const VALUE_BOX: f32 = 36.0;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
const LABEL: f32 = 11.5;
/// CSS's `letter-spacing: 0.08em`.
const LABEL_SPACING: f32 = 0.08 * LABEL;
const LABEL_PAD: f32 = 2.0;
const ROW: f32 = 36.0;
const ROW_GAP: f32 = 2.0;
/// The rows reach 6 px past the content's sides.
const ROW_OUT: f32 = 6.0;
const ROW_TEXT: f32 = 14.0;
const ROW_ICON_GAP: f32 = 10.0;
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
/// "No application…", and the others' number: 13.5 px, 4 px above, 2 below.
const NOTE: f32 = 13.5;
const NOTE_TOP: f32 = 4.0;
const NOTE_BOTTOM: f32 = 2.0;
/// Applications listed at most; the others are counted under them.
pub const MOST_APPLICATIONS: usize = 5;

const HEADER_TOP: f32 = BORDER + PAD_TOP;
const MASTER_TOP: f32 = HEADER_TOP + HEADER + GAP;
const RULE_TOP: f32 = MASTER_TOP + MASTER + GAP + RULE;
const LABEL_TOP: f32 = RULE_TOP + 1.0 + RULE + GAP;
const ROWS_TOP: f32 = LABEL_TOP + LABEL_PAD * 2.0 + LABEL * LINE + GAP;

/// The tint over the compositor's blur, a little denser than the bar's.
const GLASS_TOP: Color = Color::from_rgba8(22, 27, 42, 0.72);
const GLASS_BOTTOM: Color = Color::from_rgba8(14, 17, 28, 0.82);
const OPAQUE_TOP: Color = Color::from_rgba8(0x16, 0x1B, 0x2A, 1.0);
const OPAQUE_BOTTOM: Color = Color::from_rgba8(0x0E, 0x11, 0x1C, 1.0);
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.10);
const BUTTON_FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const BUTTON_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const RULE_COLOUR: Color = Color::from_rgba8(255, 255, 255, 0.08);
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.05);
const KNOB_SHADOW: Color = Color::from_rgba8(0, 0, 0, 0.45);
/// The microphone muted: its button tinted, its icon `gauge.critical`'s
/// light end.
const MIC_OFF_FILL: Color = Color::from_rgba8(229, 121, 63, 0.18);
const MIC_OFF_HOVER: Color = Color::from_rgba8(229, 121, 63, 0.26);
const MIC_OFF: Color = rgb(0xF7B98A);
/// A muted application's tile.
const TILE_MUTED: f32 = 0.45;
const LETTER_INK: Color = rgb(0x0B1020);
/// The letter tiles' colours (the mockup's two first), one per application
/// by its name.
const TILES: [Color; 6] = [
    rgb(0xF5B34A),
    rgb(0x8FB3FF),
    rgb(0x7EE0A0),
    rgb(0xF7B98A),
    rgb(0xC9A7F5),
    rgb(0x6FD3D8),
];

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
        (self.end() + PAD_BOTTOM + BORDER).round() as u32
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
    hover: Option<Target>,
    pressed: Option<Target>,
    dragging: Option<Slider>,
    /// A slider's volume while dragged, or asked and not yet said by
    /// PipeWire, and since when.
    asked: Option<(Slider, f32, Instant)>,
    focus: Option<Target>,
    pointer: Option<Point>,
    inside: bool,
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
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => self.refresh(state, *now),
            _ if !super::with(|c| c.is_open(self.id)) => None,
            iced::Event::Mouse(event) => {
                let at = crate::levels::draw::pointer(event, bounds, cursor, state.pointer);
                state.pointer = at;
                self.mouse(state, event, at)
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                self.key(state, key, modifiers.shift())
            }
            _ => None,
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
        self.background(&mut frame, bounds.size());
        self.header(&mut frame, state);
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
        if state.dragging.is_some() {
            mouse::Interaction::Grabbing
        } else if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// A section's divider at `rule` and its label's letters, from `top`.
fn label(f: &mut Frame, letters: &[(char, f32)], rule: f32, top: f32) {
    f.fill_rectangle(
        Point::new(LEFT, rule),
        Size::new(RIGHT - LEFT, 1.0),
        RULE_COLOUR,
    );
    for (ch, x) in letters {
        let mut letter = text(
            ch.to_string(),
            Point::new(*x, top + LABEL_PAD + LABEL * LINE / 2.0),
            LABEL,
            DIM,
            Alignment::Left,
        );
        letter.font = MANROPE_BOLD;
        f.fill_text(letter);
    }
}

/// A slider's track, from `left` to `right` and centred on `middle`: its
/// height, its knob's size.
#[derive(Debug, Clone, Copy)]
pub(super) struct Track {
    pub left: f32,
    pub right: f32,
    pub middle: f32,
    pub height: f32,
    pub knob: f32,
}

impl Track {
    /// Its volume at `x`, in percent: the knob's centre under the pointer,
    /// its travel the track less the knob.
    fn volume_at(self, x: f32) -> f32 {
        (self.share_at(x) * 100.0).round()
    }

    /// The share of its travel at `x`, from 0 to 1.
    pub(super) fn share_at(self, x: f32) -> f32 {
        let travel = self.right - self.left - self.knob;
        ((x - self.left - self.knob / 2.0) / travel).clamp(0.0, 1.0)
    }
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
        let slider = |track: Track, height: f32| {
            Rectangle::new(
                Point::new(track.left, track.middle - height / 2.0),
                Size::new(track.right - track.left, height),
            )
        };
        match self {
            Self::Mute => (button(MASTER_TOP), BUTTON_RADIUS),
            Self::Slider => (slider(layout.track(false), SLIDER), SLIDER / 2.0),
            Self::Output(i) => (row_rect(ROWS_TOP, i), BUTTON_RADIUS),
            Self::MicMute => (button(layout.mic_row()), BUTTON_RADIUS),
            Self::MicSlider => (slider(layout.track(true), SLIDER), SLIDER / 2.0),
            Self::Mic(i) => (row_rect(layout.mics_top(), i), BUTTON_RADIUS),
            Self::Tile(i) => (
                Rectangle::new(
                    Point::new(LEFT, layout.app_row(i) + (APP_ROW - TILE) / 2.0),
                    Size::new(TILE, TILE),
                ),
                BUTTON_RADIUS,
            ),
            Self::AppSlider(i) => (slider(layout.app_parts(i).1, APP_SLIDER), APP_SLIDER / 2.0),
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
            Some((asked, volume, _)) if asked == slider => *volume,
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
        // Keys that reached another of the shell's surfaces (see
        // `popover::key`).
        let keys = super::take_keys();
        let keyed = !keys.is_empty() && super::with(|c| c.is_open(self.id));
        if keyed {
            for pressed in keys {
                let shift = pressed.modifiers.shift();
                let _: Option<canvas::Action<Message>> = self.key(state, &pressed.key, shift);
            }
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
        match &state.asked {
            Some((slider, asked, since)) if state.dragging.is_none() => {
                let said = state
                    .said(slider)
                    .is_some_and(|s| (s.volume.round() - asked).abs() < 0.5);
                let until = *since + ASKED_FOR;
                if said || now >= until {
                    state.asked = None;
                    None
                } else {
                    Some(canvas::Action::request_redraw_at(until))
                }
            }
            _ => None,
        }
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
                .map(|n| elide(n, room, |t| measure(system, t, MANROPE, size)))
                .collect()
        };
        // Between the icon (or the indent) and the check, each with its gap.
        let checked = RIGHT - CHECK - ROW_ICON_GAP;
        state.names = fit(&outputs, checked - (LEFT + ICON + ROW_ICON_GAP), ROW_TEXT);
        state.mic_names = fit(&mics, checked - (LEFT - ROW_OUT + MIC_INDENT), ROW_TEXT);
        state.app_names = fit(&apps, slider_right() - app_left(), APP_NAME);
        let title = measure(system, self.words.sound(), MANROPE_BOLD, TITLE);
        let default = state
            .mixer
            .sinks
            .iter()
            .find(|s| s.default)
            .map(|s| s.description.clone())
            .unwrap_or_default();
        state.header = elide(&default, RIGHT - LEFT - title - 12.0, |t| {
            measure(system, t, MANROPE, NAME)
        });
        let words = [
            self.words.output(),
            self.words.microphone(),
            self.words.applications(),
        ];
        state.labels = words.map(|word| {
            let mut x = LEFT;
            word.chars()
                .map(|ch| {
                    let at = x;
                    x += measure(system, &ch.to_string(), MANROPE_BOLD, LABEL) + LABEL_SPACING;
                    (ch, at)
                })
                .collect()
        });
        state.zero = crate::clock::measure(system, MANROPE, VALUE, VALUE * LINE).map(|m| m.zero);
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
                if !state.inside && at.is_some() {
                    state.inside = true;
                    event(Seen::PopoverEnter);
                }
                if let Some(slider) = state.dragging.clone()
                    && let Some(at) = at
                {
                    self.slide(state, slider, at.x);
                    return redraw;
                }
                if state.hover != target {
                    state.hover = target;
                    return redraw;
                }
                None
            }
            mouse::Event::CursorLeft => {
                state.inside = false;
                event(Seen::PopoverLeave);
                state.hover = None;
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pressed = target;
                if let Some((slider, _)) = target.and_then(|t| state.slider_of(t))
                    && let Some(at) = at
                {
                    state.dragging = Some(slider.clone());
                    self.slide(state, slider, at.x);
                }
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                let pressed = state.pressed.take();
                if state.dragging.take().is_some() {
                    if let Some((_, _, since)) = &mut state.asked {
                        *since = Instant::now();
                    }
                } else if pressed.is_some() && pressed == target {
                    self.act(state, pressed);
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
        let same = matches!(&state.asked, Some((s, v, _)) if *s == slider && *v == volume);
        if !same {
            ask_volume(&state.mixer, &slider, volume);
        }
        state.asked = Some((slider, volume, Instant::now()));
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
        key: &keyboard::Key,
        shift: bool,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let order = state.targets();
        let at = state.focus.and_then(|f| order.iter().position(|t| *t == f));
        let step = |by: isize| {
            let n = order.len() as isize;
            let next = match at {
                Some(i) => (i as isize + by).rem_euclid(n),
                None if by > 0 => 0,
                None => n - 1,
            };
            order.get(next as usize).copied()
        };
        match key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => state.focus = step(if shift { -1 } else { 1 }),
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
                        state.asked = Some((slider, volume, Instant::now()));
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

    fn background(&self, f: &mut Frame, size: Size) {
        let (top, bottom) = if self.glass {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let radius = RADIUS as f32;
        let shape = Path::rounded_rectangle(Point::ORIGIN, size, radius.into());
        let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom);
        f.fill(&shape, canvas::Fill::from(tint));
        let rim = Path::rounded_rectangle(
            Point::new(BORDER / 2.0, BORDER / 2.0),
            Size::new(size.width - BORDER, size.height - BORDER),
            (radius - BORDER / 2.0).into(),
        );
        f.stroke(
            &rim,
            canvas::Stroke {
                style: canvas::Style::Solid(RIM),
                width: BORDER,
                ..canvas::Stroke::default()
            },
        );
    }

    fn header(&self, f: &mut Frame, state: &State) {
        let middle = HEADER_TOP + HEADER / 2.0;
        let mut title = text(
            self.words.sound().to_owned(),
            Point::new(LEFT, middle),
            TITLE,
            TEXT,
            Alignment::Left,
        );
        title.font = MANROPE_BOLD;
        f.fill_text(title);
        f.fill_text(text(
            state.header.clone(),
            Point::new(RIGHT, middle),
            NAME,
            FAINT,
            Alignment::Right,
        ));
    }

    fn master(&self, f: &mut Frame, state: &State) {
        let muted = state.sound.is_some_and(|s| s.muted);
        let (button, radius) = Target::Mute.shape(state.layout);
        let fill = if state.hover == Some(Target::Mute) {
            BUTTON_HOVER
        } else {
            BUTTON_FILL
        };
        f.fill(
            &Path::rounded_rectangle(button.position(), button.size(), radius.into()),
            fill,
        );
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
        row_fill(
            f,
            rect,
            sink.default,
            state.hover == Some(Target::Output(i)),
        );
        let middle = rect.center_y();
        device(f, Point::new(LEFT, middle - ICON / 2.0), sink.kind);
        let name = state.names.get(i).unwrap_or(&sink.description);
        f.fill_text(text(
            name.clone(),
            Point::new(LEFT + ICON + ROW_ICON_GAP, middle),
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
        let hover = state.hover == Some(Target::MicMute);
        let fill = match (muted, hover) {
            (true, false) => MIC_OFF_FILL,
            (true, true) => MIC_OFF_HOVER,
            (false, false) => BUTTON_FILL,
            (false, true) => BUTTON_HOVER,
        };
        f.fill(
            &Path::rounded_rectangle(button.position(), button.size(), radius.into()),
            fill,
        );
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
            row_fill(f, rect, source.default, state.hover == Some(Target::Mic(i)));
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
            let middle = layout.note_top() + NOTE_TOP + NOTE * LINE / 2.0;
            f.fill_text(text(
                note,
                Point::new(LEFT, middle),
                NOTE,
                FAINT,
                Alignment::Left,
            ));
        }
    }

    /// An application's tile: its icon, else its letter on a colour of its
    /// own; faded when muted.
    fn tile(&self, f: &mut Frame, rect: Rectangle, app: &App, muted: bool) {
        let opacity = if muted { TILE_MUTED } else { 1.0 };
        match &app.icon {
            Some(Icon::Raster(handle)) => {
                f.draw_image(rect, canvas::Image::new(handle.clone()).opacity(opacity));
            }
            Some(Icon::Vector(handle)) => {
                f.draw_svg(
                    rect,
                    iced::advanced::svg::Svg::new(handle.clone()).opacity(opacity),
                );
            }
            None => {
                let colour = TILES[tile_colour(&app.name)].scale_alpha(opacity);
                f.fill(
                    &Path::rounded_rectangle(rect.position(), rect.size(), BUTTON_RADIUS.into()),
                    colour,
                );
                let letter = app
                    .name
                    .chars()
                    .find(|c| c.is_alphanumeric())
                    .map(|c| c.to_uppercase().collect::<String>())
                    .unwrap_or_default();
                let mut letter = text(
                    letter,
                    rect.center(),
                    LETTER,
                    LETTER_INK.scale_alpha(opacity),
                    Alignment::Center,
                );
                letter.font = MANROPE_BOLD;
                f.fill_text(letter);
            }
        }
    }
}

/// The colour of an application's letter tile: the same for the same
/// name.
fn tile_colour(name: &str) -> usize {
    let sum = name.bytes().fold(0_usize, |sum, b| {
        sum.wrapping_mul(31).wrapping_add(usize::from(b))
    });
    sum % TILES.len()
}

/// A list's row lit: the chosen one, or the one under the pointer.
fn row_fill(f: &mut Frame, rect: Rectangle, chosen: bool, hovered: bool) {
    let fill = if chosen {
        Some(HOVER)
    } else if hovered {
        Some(ROW_HOVER)
    } else {
        None
    };
    if let Some(fill) = fill {
        f.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), BUTTON_RADIUS.into()),
            fill,
        );
    }
}

/// A slider at `volume`: its track, its fill to the knob (grey when what
/// it sets is muted), its knob.
fn slide(f: &mut Frame, track: Track, volume: f32, grey: bool) {
    slider(f, track, volume / 100.0, (!grey).then_some(SOUND));
}

/// A slider at `share` of its travel: its track, its fill to the knob
/// (`fill`'s gradient, else grey), its knob.
pub(super) fn slider(f: &mut Frame, track: Track, share: f32, fill: Option<(Color, Color)>) {
    let Track {
        left,
        right,
        middle,
        height,
        knob: size,
    } = track;
    let travel = right - left - size;
    let top = middle - height / 2.0;
    let radius = height / 2.0;
    f.fill(
        &Path::rounded_rectangle(
            Point::new(left, top),
            Size::new(right - left, height),
            radius.into(),
        ),
        TRACK,
    );
    let knob = left + travel * share.clamp(0.0, 1.0);
    let filled = Path::rounded_rectangle(
        Point::new(left, top),
        Size::new(knob + size / 2.0 - left, height),
        radius.into(),
    );
    match fill {
        None => f.fill(&filled, MUTED_FILL),
        Some((from, to)) => {
            let fill = gradient::Linear::new(
                Point::new(left, middle),
                Point::new(knob + size / 2.0, middle),
            )
            .add_stop(0.0, from)
            .add_stop(1.0, to);
            f.fill(&filled, canvas::Fill::from(fill));
        }
    }
    let centre = Point::new(knob + size / 2.0, middle);
    // `box-shadow: 0 1px 4px`: rings fading out under the knob, 1 px down.
    for (grow, share) in [(2.5_f32, 0.12_f32), (1.6, 0.2), (0.8, 0.3)] {
        f.fill(
            &Path::circle(Point::new(centre.x, centre.y + 1.0), size / 2.0 + grow),
            KNOB_SHADOW.scale_alpha(share),
        );
    }
    f.fill(&Path::circle(centre, size / 2.0), TEXT);
}

/// A value right-aligned on `at`: `word` in `text.faint` (muted, off),
/// else the volume in tabular figures, in `colour`.
fn value(f: &mut Frame, state: &State, at: Point, word: Option<&str>, volume: f32, colour: Color) {
    if let Some(word) = word {
        f.fill_text(text(word.to_owned(), at, VALUE, FAINT, Alignment::Right));
        return;
    }
    let zero = state.zero.unwrap_or(VALUE * 0.6);
    let digits = (volume.round() as u32).to_string();
    let mut x = at.x - zero * digits.len() as f32;
    for ch in digits.chars() {
        cell(f, ch, x + zero / 2.0, at.y, VALUE, MANROPE, colour);
        x += zero;
    }
}

fn text(
    content: String,
    position: Point,
    size: f32,
    color: Color,
    align: Alignment,
) -> canvas::Text {
    canvas::Text {
        content,
        position,
        max_width: f32::INFINITY,
        color,
        size: Pixels(size),
        line_height: LineHeight::Relative(LINE),
        font: MANROPE,
        align_x: align,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

/// `text` cut with an ellipsis to fit `room` px as `width` measures it.
pub fn elide(text: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> String {
    if width(text) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let cut: String = chars[..keep]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_owned()
            + "…";
        if width(&cut) <= room {
            return cut;
        }
    }
    "…".to_owned()
}

/// `text`'s width in `font` at `size`.
pub(crate) fn measure(
    system: &mut iced::advanced::graphics::text::cosmic_text::FontSystem,
    text: &str,
    font: iced::Font,
    size: f32,
) -> f32 {
    use iced::advanced::graphics::text::cosmic_text;
    let mut buffer = cosmic_text::Buffer::new(system, cosmic_text::Metrics::new(size, size * LINE));
    let attrs = iced::advanced::graphics::text::to_attributes(font);
    buffer.set_text(system, text, &attrs, cosmic_text::Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0, f32::max)
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
    fn long_names_are_cut_with_an_ellipsis() {
        let width = |t: &str| t.chars().count() as f32 * 7.0;
        assert_eq!(elide("Casque", 100.0, width), "Casque");
        assert_eq!(
            elide("Haut-parleurs du portable", 100.0, width),
            "Haut-parleurs…"
        );
        assert_eq!(elide("abc", 1.0, width), "…");
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
                    rect.x + rect.width + out <= POPOVER_WIDTH as f32,
                    "{target:?}"
                );
                assert!(rect.y + rect.height <= height - PAD_BOTTOM, "{target:?}");
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

    #[test]
    fn a_letter_tile_keeps_its_colour() {
        assert_eq!(tile_colour("Musique"), tile_colour("Musique"));
        let colours: std::collections::HashSet<_> =
            ["Firefox", "Musique", "mpv", "Spotify", "Zoom"]
                .iter()
                .map(|n| tile_colour(n))
                .collect();
        assert!(colours.len() > 1);
    }
}
