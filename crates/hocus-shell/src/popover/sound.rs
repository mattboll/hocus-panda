//! The sound's popover (bar.md §9, the 3.4 mockup): its title and the
//! default output's name; the master volume, its mute button and its
//! value; the outputs, the default one checked; the microphone, its mute
//! button, its volume and, when there are several, the one to use; then
//! the applications playing sound, each with its volume and its mute; at
//! its bottom, "Réglages du son…", which opens the Settings on Son (6.5). A
//! click on an output makes it the default, which keeps its own volume
//! (PipeWire's, per sink): the slider then shows that one's.
//!
//! Built with the shell's widget kit ([`crate::ui`]): what changes draws
//! this surface alone, without a message to iced. The sliders' volumes go
//! to the sound's service as they come; it sends PipeWire only the last
//! one each time `wpctl` is ready. A slider shows the volume asked until
//! PipeWire says it, a second at most; moving one unmutes what it sets.
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
//! microphones, then each application's tile and slider, then the
//! Settings' link; `Enter` or `Space` acts; the arrows move a slider by
//! 5 % (and go through the outputs and the microphones); `Esc` closes.
//! Opened by the keyboard, the master slider holds the ring (bar.md §11).

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use hocus_link::LevelKey;
use iced::{
    Color, Element, Length, Point, Radians, Rectangle, Size,
    alignment::Vertical,
    keyboard,
    widget::{
        Space, button,
        canvas::{self, Frame, Path},
        column, container, row, space,
        text::Alignment,
    },
    window,
};

use super::{
    Item, Pressed,
    kit::{
        self, FILL, HEADER, HEADER_TOP, ICON, ICON_GAP, LABEL, LABEL_PAD, LEFT, NOTE, NOTE_TOP,
        RIGHT, ROW, ROW_GAP, ROW_OUT, ROW_RADIUS, ROW_TEXT, TITLE,
    },
};
use crate::levels::{
    self,
    draw::{
        ACCENT, DIM, FAINT, HOVER, LINE, MANROPE, MANROPE_BOLD, SOUND, TEXT, VALUE, rgb, stroke,
    },
    model::{STEP, Sound, Words},
    sound::{App, Ask, Kind, Mixer, Set},
};
use crate::{
    theme::Ink,
    ui::{
        Around, control,
        parts::{self, drawn, gap, note},
        slider::Slider as Track,
        style::{self, clear, filled, hovered, lit, said},
    },
};

type El<'a> = Element<'a, Msg>;

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
/// Between a button and its slider, a slider and its value.
const SLIDER_GAP: f32 = 8.0;
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
/// The Settings' link: its row (6.5).
const LINK: f32 = 36.0;
/// "No application…", and the others' number: a note, 2 px under it.
const NOTE_BOTTOM: f32 = 2.0;
/// Applications listed at most; the others are counted under them.
pub const MOST_APPLICATIONS: usize = 5;

const MASTER_TOP: f32 = HEADER_TOP + HEADER + GAP;
const ROWS_TOP: f32 = MASTER_TOP + MASTER + GAP + RULE + 1.0 + RULE + GAP + LABEL_ROW + GAP;
/// A section's label's line.
const LABEL_ROW: f32 = LABEL_PAD * 2.0 + LABEL * LINE;

const BUTTON_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
/// The microphone muted: its button tinted, its icon `gauge.critical`'s
/// light end.
const MIC_OFF_FILL: Ink = Ink::Dark(Color::from_rgba8(229, 121, 63, 0.18));
const MIC_OFF_HOVER: Ink = Ink::Dark(Color::from_rgba8(229, 121, 63, 0.26));
const MIC_OFF: Ink = Ink::Dark(rgb(0xF7B98A));
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

/// A section's divider, label and gaps, after what is above it.
const SECTION: f32 = GAP + RULE + 1.0 + RULE + GAP + LABEL_ROW + GAP;

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

    fn mic_end(self) -> f32 {
        let end = self.outputs_end();
        match self.microphones {
            0 => end,
            1 => end + SECTION + MASTER,
            n => end + SECTION + MASTER + GAP + rows(n, ROW, ROW_GAP),
        }
    }

    fn applications_end(self) -> f32 {
        let top = self.mic_end() + SECTION;
        let rows = rows(self.applications, APP_ROW, GAP);
        match (self.applications, self.more) {
            (0, _) => top + NOTE_TOP + NOTE * LINE + NOTE_BOTTOM,
            (_, true) => top + rows + GAP + NOTE_TOP + NOTE * LINE + NOTE_BOTTOM,
            (_, false) => top + rows,
        }
    }

    fn end(self) -> f32 {
        self.applications_end() + RULE + 1.0 + RULE + LINK
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
    if opening {
        ui(|ui| *ui = Ui::default());
    }
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

// --- What it keeps while it is open ---------------------------------------------

/// What can hold the keyboard's ring, and be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Mute,
    Slider,
    Output(usize),
    MicMute,
    MicSlider,
    Mic(usize),
    Tile(usize),
    AppSlider(usize),
    /// "Réglages du son…".
    Settings,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("sound:{self:?}")
    }
}

/// A slider, by what it sets: an application's by its key, so that it
/// stays the same when the list changes under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slider {
    Master,
    Mic,
    App(String),
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    dragging: bool,
    /// A slider's volume while dragged, or asked and not yet said by
    /// PipeWire, and since when.
    asked: Option<((Slider, f32), Instant)>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// What the view is built from: the mixer, the applications listed and
/// the others' number, the default output's sound.
struct Shown {
    mixer: Mixer,
    apps: Vec<App>,
    more: usize,
    sound: Option<Sound>,
}

impl Shown {
    fn now() -> Self {
        let mixer = levels::sound::mixer();
        let (apps, more) = listed(&mixer);
        Self {
            mixer,
            apps,
            more,
            sound: levels::snapshot().sound,
        }
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
    fn shown(&self, ui: &Ui, slider: &Slider) -> f32 {
        match &ui.asked {
            Some(((asked, volume), _)) if asked == slider => *volume,
            _ => self.said(slider).map_or(0.0, |s| s.volume),
        }
        .clamp(0.0, 100.0)
    }

    /// The slider a target is.
    fn slider_of(&self, target: Target) -> Option<Slider> {
        match target {
            Target::Slider => Some(Slider::Master),
            Target::MicSlider => Some(Slider::Mic),
            Target::AppSlider(i) => Some(Slider::App(self.apps.get(i)?.key.clone())),
            _ => None,
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

// --- The view ------------------------------------------------------------------------

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A control pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// A slider dragged to a volume; let go.
    Slide(Slider, f32),
    Release,
    /// An arrow on the slider holding the ring: up, or down.
    Step(bool),
    /// A press anywhere on the popover.
    Pressed,
    /// The sound changed under the view.
    Stale,
}

/// The sound's popover on its surface.
pub fn view<'a>(id: window::Id, words: Words, glass: bool) -> Element<'a, crate::Message> {
    let shown = Shown::now();
    let subtitle = shown
        .mixer
        .sinks
        .iter()
        .find(|s| s.default)
        .map(|s| s.description.clone());
    let (content, until) = ui(|ui| (content(words, &shown, ui), until(ui)));
    // Asked and not said: the surface wakes when it stops showing.
    Around::new(id, content, handle, Msg::Stale)
        .stale_when(|| ui(|ui| expired(ui, Instant::now())))
        .wake_at(until)
        .keys(arrows)
        .press(Msg::Pressed)
        .first(Msg::Focus(Target::Slider))
        .subtitle(subtitle)
        .on_glass(glass)
}

/// Until when the volume asked shows without PipeWire saying it, once
/// let go.
fn until(ui: &Ui) -> Option<Instant> {
    let (_, since) = ui.asked.as_ref().filter(|_| !ui.dragging)?;
    Some(*since + ASKED_FOR)
}

/// Whether the volume asked showed long enough without PipeWire saying it.
fn expired(ui: &Ui, now: Instant) -> bool {
    until(ui).is_some_and(|until| now >= until)
}

/// An arrow on a slider holding the ring steps it (5 %).
fn arrows(pressed: &Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    let up = match &pressed.key {
        Key::Named(Named::ArrowRight | Named::ArrowUp) => true,
        Key::Named(Named::ArrowLeft | Named::ArrowDown) => false,
        _ => return None,
    };
    let on_slider = ui(|ui| {
        matches!(
            ui.focus,
            Some(Target::Slider | Target::MicSlider | Target::AppSlider(_))
        )
    });
    on_slider.then_some(Msg::Step(up))
}

fn content<'a>(words: Words, shown: &Shown, ui: &Ui) -> El<'a> {
    let a11y = crate::a11y::Words { fr: words.fr() };
    let focus = ui.focus;
    let mixer = &shown.mixer;
    let master_muted = shown.sound.is_some_and(|s| s.muted);
    let block = |content: El<'a>| parts::block(GAP, Some(content));
    let mut all: Vec<El<'a>> = vec![header(words, mixer)];
    // The master: mute, slider, value.
    all.push(block(volume_row(
        VolumeRow {
            mute: Target::Mute,
            slider: Target::Slider,
            which: Slider::Master,
            muted: master_muted,
            mic: false,
            sound: shown.sound,
            word: shown.sound.filter(|s| s.muted).map(|_| words.muted()),
            names: (a11y.mute().to_owned(), a11y.volume().to_owned()),
        },
        shown,
        ui,
        a11y,
    )));
    // The outputs.
    all.push(block(parts::rule()));
    all.push(block(parts::label(words.output(), LABEL_ROW, None)));
    let outputs = mixer.sinks.len();
    let mut list = column![].spacing(ROW_GAP);
    for (i, sink) in mixer.sinks.iter().enumerate() {
        let kind_of = sink.kind;
        let line = row![
            drawn(ICON, ICON, move |f, _| device(f, Point::ORIGIN, kind_of)),
            gap(ICON_GAP),
        ];
        let room = RIGHT - CHECK - ICON_GAP - (LEFT + ICON + ICON_GAP);
        list = list.push(choice(
            line,
            &sink.description,
            room,
            sink.default,
            Target::Output(i),
            (i, outputs, 0),
            focus,
        ));
    }
    all.push(block(list.into()));
    // The microphone.
    let mics = mixer.sources.len();
    if let Some(mic) = mixer.microphone() {
        let muted = mic.sound.is_some_and(|s| s.muted);
        all.push(block(parts::rule()));
        all.push(block(parts::label(words.microphone(), LABEL_ROW, None)));
        all.push(block(volume_row(
            VolumeRow {
                mute: Target::MicMute,
                slider: Target::MicSlider,
                which: Slider::Mic,
                muted,
                mic: true,
                sound: mic.sound,
                word: muted.then(|| words.off()),
                names: (
                    a11y.mute_microphone().to_owned(),
                    a11y.microphone().to_owned(),
                ),
            },
            shown,
            ui,
            a11y,
        )));
        if mics > 1 {
            let mut list = column![].spacing(ROW_GAP);
            for (i, source) in mixer.sources.iter().enumerate() {
                let room = RIGHT - CHECK - ICON_GAP - (LEFT - ROW_OUT + MIC_INDENT);
                let indent = row![Space::new().width(MIC_INDENT - kit::ROW_PAD)];
                list = list.push(choice(
                    indent,
                    &source.description,
                    room,
                    source.default,
                    Target::Mic(i),
                    (i, mics, 1),
                    focus,
                ));
            }
            all.push(block(list.into()));
        }
    }
    // The applications.
    all.push(block(parts::rule()));
    all.push(block(parts::label(words.applications(), LABEL_ROW, None)));
    let mut apps = column![].spacing(GAP);
    for (i, app) in shown.apps.iter().enumerate() {
        apps = apps.push(app_row(words, a11y, shown, ui, app, i, master_muted));
    }
    if !shown.apps.is_empty() {
        all.push(block(apps.into()));
    }
    let more = match (shown.apps.is_empty(), shown.more) {
        (true, _) => Some(words.no_application().to_owned()),
        (false, 0) => None,
        (false, more) => Some(words.more_applications(more)),
    };
    if let Some(more) = more {
        all.push(block(note(&more)));
    }
    let [rule, link] = parts::settings_link(
        words.sound_settings(),
        &Target::Settings.id(),
        Msg::Act(Target::Settings),
        focus == Some(Target::Settings),
        Msg::Focus(Target::Settings),
    );
    all.push(rule);
    all.push(link);
    parts::content(all)
}

/// "Son", and the default output's name on its right.
fn header<'a>(words: Words, mixer: &Mixer) -> El<'a> {
    let title = kit::measure(words.sound(), TITLE, MANROPE_BOLD);
    let default = mixer
        .sinks
        .iter()
        .find(|s| s.default)
        .map(|s| s.description.clone())
        .unwrap_or_default();
    let name = kit::elide(&default, RIGHT - LEFT - title - TITLE_GAP, |t| {
        kit::measure(t, NAME, MANROPE)
    });
    parts::header(
        words.sound(),
        Some(said(name, NAME, MANROPE, FAINT.get()).into()),
    )
}

/// A row with a mute button, a slider and its value: the master's, the
/// microphone's.
struct VolumeRow {
    mute: Target,
    slider: Target,
    which: Slider,
    muted: bool,
    mic: bool,
    sound: Option<Sound>,
    /// Said in place of the value.
    word: Option<&'static str>,
    /// The button's name and the slider's, for screen readers.
    names: (String, String),
}

fn volume_row<'a>(row_of: VolumeRow, shown: &Shown, ui: &Ui, a11y: crate::a11y::Words) -> El<'a> {
    let VolumeRow {
        mute,
        slider,
        which,
        muted,
        mic,
        sound,
        word,
        names,
    } = row_of;
    let focus = ui.focus;
    let button = button(drawn(BUTTON, BUTTON, move |f, s| {
        let at = Point::new(s.width / 2.0 - ICON / 2.0, s.height / 2.0 - ICON / 2.0);
        if mic {
            microphone(f, at, muted);
        } else {
            speaker(f, at, muted);
        }
    }))
    .padding(0)
    .width(BUTTON)
    .height(BUTTON)
    .style(move |_, status| {
        let fill = match (mic && muted, lit(status)) {
            (true, false) => MIC_OFF_FILL,
            (true, true) => MIC_OFF_HOVER,
            (false, false) => FILL,
            (false, true) => BUTTON_HOVER,
        };
        filled(fill.get(), BUTTON_RADIUS, TEXT.get())
    })
    .on_press(Msg::Act(mute));
    let button = control(button, mute.id(), Role::ToggleButton, names.0)
        .node(|n| n.checked(sound.is_some_and(|s| s.muted)))
        .focus(focus == Some(mute), Msg::Focus(mute), BUTTON_RADIUS)
        .activate(Msg::Act(mute));
    let volume = shown.shown(ui, &which);
    let track = slider_of(&which, volume, muted, TRACK_HEIGHT, KNOB).view(SLIDER);
    let mut track = control(track, slider.id(), Role::Slider, names.1).focus(
        focus == Some(slider),
        Msg::Focus(slider),
        SLIDER / 2.0,
    );
    track = match sound {
        Some(sound) => track.node(|n| {
            n.value(if sound.muted {
                format!("{}, {}", a11y.percent(sound.volume), a11y.muted())
            } else {
                a11y.percent(sound.volume)
            })
            .range(f64::from(sound.volume), 0.0, 100.0, Some(f64::from(STEP)))
        }),
        None => track.node(|n| n.disabled(true)),
    };
    // The master's value shows once PipeWire says it; the microphone's
    // always.
    let value = value(
        volume,
        word,
        DIM.get(),
        MASTER / 2.0,
        mic || sound.is_some(),
    );
    let line = row![
        button,
        gap(SLIDER_GAP),
        track,
        gap(SLIDER_GAP),
        drawn(VALUE_BOX, MASTER, value)
    ]
    .align_y(Vertical::Center);
    parts::inset(container(line).height(MASTER).align_y(Vertical::Center))
}

/// A slider of the popover's: dragging it asks its volume.
fn slider_of<'a>(which: &Slider, volume: f32, grey: bool, track: f32, knob: f32) -> Track<'a, Msg> {
    let which = which.clone();
    Track {
        share: volume / 100.0,
        fill: (!grey).then_some(SOUND),
        track,
        knob,
        on_change: Box::new(move |share: f32| Msg::Slide(which.clone(), (share * 100.0).round())),
        on_release: Msg::Release,
    }
}

/// A value right-aligned on its box's right, at `middle`: `word` in
/// `text.faint` (muted, off), else the volume in tabular figures, in
/// `colour`; nothing when there is none (`shows`).
fn value(
    volume: f32,
    word: Option<&'static str>,
    colour: Color,
    middle: f32,
    shows: bool,
) -> impl Fn(&mut Frame, Size) {
    move |f, s| {
        if !shows {
            return;
        }
        let at = Point::new(s.width, middle);
        if let Some(word) = word {
            f.fill_text(kit::text(
                word.to_owned(),
                at,
                VALUE,
                FAINT.get(),
                Alignment::Right,
            ));
            return;
        }
        kit::figures(f, at, volume, kit::zero_once(), colour);
    }
}

/// An output's row, or a microphone's: what is before its name, its name
/// cut to `room`, the check of the chosen one; a radio button, `i` of `n`,
/// the arrows going through group `group`.
fn choice<'a>(
    before: iced::widget::Row<'a, Msg>,
    name: &str,
    room: f32,
    chosen: bool,
    target: Target,
    (i, n, group): (usize, usize, u8),
    focus: Option<Target>,
) -> El<'a> {
    let cut = kit::elide(name, room, |t| kit::measure(t, ROW_TEXT, MANROPE));
    let mut line = before
        .push(said(cut, ROW_TEXT, MANROPE, TEXT.get()))
        .push(space::horizontal())
        .align_y(Vertical::Center);
    if chosen {
        line = line.push(drawn(CHECK, CHECK, |f, _| check(f, Point::ORIGIN)));
    }
    let pressed = button(container(line).center_y(Length::Fill))
        .padding([0.0, kit::ROW_PAD])
        .width(Length::Fill)
        .height(ROW)
        .style(move |_, status| {
            if chosen {
                hovered(HOVER.get(), ROW_RADIUS)
            } else if lit(status) {
                hovered(style::ROW_HOVER.get(), ROW_RADIUS)
            } else {
                clear()
            }
        })
        .on_press(Msg::Act(target));
    control(pressed, target.id(), Role::RadioButton, name)
        .node(|node| node.checked(chosen).set(i + 1, n))
        .focus(focus == Some(target), Msg::Focus(target), BUTTON_RADIUS)
        .activate(Msg::Act(target))
        .rows(group)
        .into()
}

/// An application's row: its tile (its mute), its name over its slider,
/// its value on the row's bottom.
fn app_row<'a>(
    words: Words,
    a11y: crate::a11y::Words,
    shown: &Shown,
    ui: &Ui,
    app: &App,
    i: usize,
    master_muted: bool,
) -> El<'a> {
    let focus = ui.focus;
    let muted = app.sound.is_some_and(|s| s.muted);
    let (icon, name) = (app.icon.clone(), app.name.clone());
    let tile = button(drawn(TILE, TILE, move |f, s| {
        kit::app_tile(
            f,
            Rectangle::new(Point::ORIGIN, s),
            (icon.as_ref(), &name),
            (BUTTON_RADIUS, LETTER),
            if muted { TILE_MUTED } else { 1.0 },
        );
    }))
    .padding(0)
    .style(|_, _| clear())
    .on_press(Msg::Act(Target::Tile(i)));
    let tile = control(
        tile,
        Target::Tile(i).id(),
        Role::ToggleButton,
        a11y.mute_app(&app.name),
    )
    .node(|n| n.checked(muted))
    .focus(
        focus == Some(Target::Tile(i)),
        Msg::Focus(Target::Tile(i)),
        BUTTON_RADIUS,
    )
    .activate(Msg::Act(Target::Tile(i)));
    let which = Slider::App(app.key.clone());
    let volume = shown.shown(ui, &which);
    let track =
        slider_of(&which, volume, muted || master_muted, APP_TRACK, APP_KNOB).view(APP_SLIDER);
    let target = Target::AppSlider(i);
    let mut track = control(track, target.id(), Role::Slider, app.name.clone()).focus(
        focus == Some(target),
        Msg::Focus(target),
        APP_SLIDER / 2.0,
    );
    track = match app.sound {
        Some(sound) => track.node(|n| {
            n.value(if sound.muted {
                format!("{}, {}", a11y.percent(sound.volume), a11y.muted())
            } else {
                a11y.percent(sound.volume)
            })
            .range(f64::from(sound.volume), 0.0, 100.0, Some(f64::from(STEP)))
        }),
        None => track.node(|n| n.disabled(true)),
    };
    let room = RIGHT - VALUE_BOX - SLIDER_GAP - (LEFT + TILE + SLIDER_GAP);
    let cut = kit::elide(&app.name, room, |t| kit::measure(t, APP_NAME, MANROPE));
    let texts = column![
        said(
            cut,
            APP_NAME,
            MANROPE,
            if muted { FAINT } else { TEXT }.get()
        ),
        Space::new().height(APP_NAME_GAP),
        track
    ]
    .width(Length::Fill);
    let middle = APP_ROW - APP_VALUE_PAD - VALUE * LINE / 2.0;
    let colour = if master_muted { FAINT } else { DIM };
    let value = value(
        volume,
        muted.then(|| words.muted()),
        colour.get(),
        middle,
        true,
    );
    let line = row![
        tile,
        gap(SLIDER_GAP),
        texts,
        gap(SLIDER_GAP),
        drawn(VALUE_BOX, APP_ROW, value)
    ]
    .align_y(Vertical::Center);
    parts::inset(container(line).height(APP_ROW).align_y(Vertical::Center))
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    let shown = Shown::now();
    match message {
        Msg::Stale => {}
        // A press takes the ring away.
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Act(target) => act(&shown, target),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Slide(slider, volume) => ui(|ui| {
            ui.dragging = true;
            let same = matches!(&ui.asked, Some(((s, v), _)) if *s == slider && *v == volume);
            if !same {
                ask_volume(&shown.mixer, &slider, volume);
            }
            ui.asked = Some(((slider, volume), Instant::now()));
        }),
        Msg::Release => ui(|ui| {
            ui.dragging = false;
            if let Some((_, since)) = &mut ui.asked {
                *since = Instant::now();
            }
        }),
        Msg::Step(up) => step(&shown, up),
    }
    // The volume asked stays shown until PipeWire says it, or a while.
    let now = Instant::now();
    ui(|ui| {
        if ui.dragging {
            return;
        }
        let said = ui.asked.as_ref().is_some_and(|((slider, asked), _)| {
            shown
                .said(slider)
                .is_some_and(|s| (s.volume.round() - asked).abs() < 0.5)
        });
        kit::awaited(&mut ui.asked, said, now, ASKED_FOR);
    });
    super::refit(Item::Sound);
}

/// An arrow on the slider holding the ring: the master's through the
/// sound's keys, the others by 5 %.
fn step(shown: &Shown, up: bool) {
    match ui(|ui| ui.focus) {
        Some(Target::Slider) => {
            ui(|ui| ui.asked = None);
            levels::key(if up {
                LevelKey::VolumeUp
            } else {
                LevelKey::VolumeDown
            });
        }
        Some(focus @ (Target::MicSlider | Target::AppSlider(_))) => {
            let Some(slider) = shown.slider_of(focus) else {
                return;
            };
            let by = if up { STEP } else { -STEP };
            ui(|ui| {
                let volume = (shown.shown(ui, &slider) + by).clamp(0.0, 100.0);
                ask_volume(&shown.mixer, &slider, volume);
                ui.asked = Some(((slider, volume), Instant::now()));
            });
        }
        _ => {}
    }
}

fn act(shown: &Shown, target: Target) {
    let mixer = &shown.mixer;
    match target {
        Target::Mute => levels::key(LevelKey::Mute),
        Target::Output(i) => {
            if let Some(sink) = mixer.sinks.get(i).filter(|s| !s.default) {
                levels::ask(Ask::Default(sink.id));
            }
        }
        Target::MicMute => {
            if let Some(mic) = mixer.microphone() {
                let muted = mic.sound.is_some_and(|s| s.muted);
                levels::ask(Ask::Node(mic.id, Set::Mute(!muted)));
            }
        }
        Target::Mic(i) => {
            if let Some(mic) = mixer.sources.get(i).filter(|s| !s.default) {
                levels::ask(Ask::DefaultSource(mic.id));
            }
        }
        Target::Tile(i) => {
            if let Some(app) = shown.apps.get(i) {
                let muted = app.sound.is_some_and(|s| s.muted);
                for id in &app.streams {
                    levels::ask(Ask::Node(*id, Set::Mute(!muted)));
                }
            }
        }
        Target::Settings => super::session::open_settings(Some("sound")),
        Target::Slider | Target::MicSlider | Target::AppSlider(_) => {}
    }
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
    lines(f, at, s, 2.0, colour.get(), &cone);
    if muted {
        lines(f, at, s, 2.0, colour.get(), &[(16.0, 9.0), (21.0, 15.0)]);
        lines(f, at, s, 2.0, colour.get(), &[(21.0, 9.0), (16.0, 15.0)]);
    } else {
        // `M15.5 8.5a5 5 0 0 1 0 7` and `M18.5 6a9 9 0 0 1 0 12`.
        wave(f, at, s, (15.5, 8.5), 5.0, 7.0, colour.get());
        wave(f, at, s, (18.5, 6.0), 9.0, 12.0, colour.get());
    }
}

fn device(f: &mut Frame, at: Point, kind: Kind) {
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, DIM.get());
    let rect = |f: &mut Frame, x: f32, y: f32, w: f32, h: f32, r: f32| {
        f.stroke(
            &Path::rounded_rectangle(p(x, y), Size::new(w * s, h * s), (r * s).into()),
            line,
        );
    };
    match kind {
        Kind::Speaker => {
            rect(f, 3.0, 7.0, 18.0, 11.0, 2.0);
            lines(f, at, s, 2.0, DIM.get(), &[(7.0, 21.0), (17.0, 21.0)]);
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
            lines(f, at, s, 2.0, DIM.get(), &[(8.0, 21.0), (16.0, 21.0)]);
            lines(f, at, s, 2.0, DIM.get(), &[(12.0, 17.0), (12.0, 21.0)]);
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
        ACCENT.get(),
        &[(5.0, 12.5), (10.0, 17.0), (19.0, 7.0)],
    );
}

/// The microphone: `M5 11a7 7 0 0 0 14 0`, its capsule and its foot;
/// crossed out in `MIC_OFF` when muted.
fn microphone(f: &mut Frame, at: Point, muted: bool) {
    let s = ICON / 24.0;
    let colour = if muted { MIC_OFF } else { TEXT };
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour.get());
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
    lines(f, at, s, 2.0, colour.get(), &[(12.0, 18.0), (12.0, 21.0)]);
    if muted {
        lines(f, at, s, 2.0, colour.get(), &[(4.0, 4.0), (20.0, 20.0)]);
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
        // The Settings' link at the bottom adds 45 px (6.5).
        assert_eq!(layout(3, 0, 0).height(), 332 + 45);
        assert_eq!(layout(1, 0, 0).height(), 256 + 45);
        // One microphone: its row; two, their list.
        assert_eq!(layout(3, 1, 0).height(), 332 + 45 + 47 + 36);
        assert_eq!(layout(3, 2, 0).height(), 332 + 45 + 47 + 36 + 6 + 74);
        // Two applications instead of "No application…".
        assert_eq!(layout(3, 2, 2).height(), 568 + 45);
        // At most five, and a line for the others.
        assert_eq!(layout(3, 2, 5).height(), layout(3, 2, 2).height() + 3 * 52);
        assert_eq!(layout(3, 2, 7).height(), 755 + 45);
    }

    fn app(key: &str, playing: bool) -> App {
        App {
            key: key.to_owned(),
            name: key.to_owned(),
            icon: None,
            icon_names: Vec::new(),
            streams: vec![1],
            sound: None,
            playing,
            sink: None,
            target: None,
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
