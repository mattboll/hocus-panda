//! The brightness's popover (bar.md §9, the 3.7 mockup): "Luminosité" and
//! what it sets, "Écran du portable"; the panel's brightness on a slider
//! from 5 % to 100 % (never below, as the keys), its value beside it; the
//! keyboard's backlight when the machine has one, its levels named (off,
//! low, high; off, on) when it has four at most, else on a slider; and a
//! line for each other screen plugged in: it is set with its own buttons
//! (variant B: the panel sets only the laptop's screen and its keyboard).
//!
//! Built with the shell's widget kit ([`crate::ui`]), as the sound's:
//! what changes draws this surface alone. The slider sends each brightness as it moves; the system bus's
//! thread asks logind only the last one each time logind is ready. It
//! shows what was asked until the backlight says it, a second at most.
//!
//! The keyboard: `Tab` and `Shift+Tab` go through the slider and the
//! keyboard's levels (or its slider); the arrows move a slider by 5 % (the
//! keyboard's by a level) and choose the next level; `Enter` or `Space`
//! chooses; the wheel over the brightness's row moves it by 5 %; `Esc`
//! closes.

use hocus_words::shell::popover_brightness as txt;
use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use iced::{
    Color, Element, Length, Point, Size,
    alignment::Vertical,
    keyboard, mouse,
    widget::{
        button,
        canvas::{Frame, Path},
        column, container, mouse_area, row,
        text::LineHeight,
    },
    window,
};

use super::{
    Pressed,
    kit::{self, ASIDE, HEADER, HEADER_TOP, ICON, LEFT, MANROPE_SEMIBOLD, RIGHT, Wheel},
};
use crate::levels::{
    self,
    draw::{DIM, FAINT, LIGHT_GAUGE, MANROPE, TEXT, sun},
    model::{BRIGHTNESS_FLOOR, Keyboard, STEP, Snapshot, Words},
    power::Ask,
};
use crate::theme::Ink;
use crate::ui::{
    Around, control,
    parts::{self, drawn, gap},
    slider::Slider,
    style::{self, said as said_in},
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 ------------------------------------------------

const GAP: f32 = 6.0;
/// A row: its icon in a 32 px slot, then the slider or the levels, 8 px
/// apart.
const ROW: f32 = 36.0;
const SLOT: f32 = 32.0;
const BESIDE: f32 = 8.0;
const SLIDER: f32 = 24.0;
const TRACK: f32 = 8.0;
const KNOB: f32 = 16.0;
const VALUE_BOX: f32 = 36.0;
/// The keyboard's levels: a 3 px padded strip, 26 px buttons 4 px apart.
const STRIP_PAD: f32 = 3.0;
const STRIP_RADIUS: f32 = 9.0;
const SEGMENT: f32 = 26.0;
const SEGMENT_GAP: f32 = 4.0;
const SEGMENT_RADIUS: f32 = 7.0;
const SEGMENT_TEXT: f32 = 12.5;
/// A screen's line: 12.5 px on 1.4 lines, 2 px above.
const NOTE: f32 = 12.5;
const NOTE_LINE: f32 = 1.4;
const NOTE_PAD: f32 = 2.0;

const STRIP: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const CHOSEN: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const SEGMENT_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));

/// A slider shows what was asked this long at most, until it is said.
const ASKED_FOR: Duration = Duration::from_secs(1);

const LIGHT_TOP: f32 = HEADER_TOP + HEADER + GAP;
const KEYBOARD_TOP: f32 = LIGHT_TOP + ROW + GAP;

/// Each other screen's line, broken to fit.
fn notes(words: Words, screens: &[String]) -> Vec<Vec<String>> {
    screens
        .iter()
        .map(|name| {
            kit::wrap(&words.own_buttons(name), RIGHT - LEFT, |t| {
                kit::measure(t, NOTE, MANROPE)
            })
        })
        .collect()
}

/// Where its parts are: the keyboard's row, if any, and each screen's
/// line's top.
#[derive(Debug, Clone, PartialEq)]
struct Layout {
    keyboard: bool,
    notes: Vec<(f32, Vec<String>)>,
    end: f32,
}

impl Layout {
    fn of(keyboard: bool, notes: Vec<Vec<String>>) -> Self {
        let mut end = if keyboard {
            KEYBOARD_TOP + ROW
        } else {
            LIGHT_TOP + ROW
        };
        let notes = notes
            .into_iter()
            .map(|lines| {
                let top = end + GAP + NOTE_PAD;
                end = top + lines.len() as f32 * NOTE * NOTE_LINE;
                (top, lines)
            })
            .collect();
        Self {
            keyboard,
            notes,
            end,
        }
    }

    fn height(&self) -> u32 {
        kit::height(self.end)
    }
}

/// The popover opens: nothing said yet, nothing asked.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// Its height for what it shows now.
pub fn height() -> u32 {
    let words = kit::words();
    let keyboard = levels::snapshot().keyboard.is_some();
    Layout::of(keyboard, notes(words, &levels::screens())).height()
}

/// The brightness's share of its slider's travel: 5 % at its left end.
fn light_share(percent: f32) -> f32 {
    ((percent - BRIGHTNESS_FLOOR) / (100.0 - BRIGHTNESS_FLOOR)).clamp(0.0, 1.0)
}

fn light_at(share: f32) -> f32 {
    (BRIGHTNESS_FLOOR + share * (100.0 - BRIGHTNESS_FLOOR)).round()
}

// --- What it keeps while it is open ---------------------------------------------

/// What can hold the keyboard's ring, and be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Light,
    /// One of the keyboard's levels, from off.
    Level(u32),
    /// The keyboard's slider, past four levels.
    Keys,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("brightness:{self:?}")
    }
}

/// What a slider asked, until it is said.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Asked {
    /// The brightness, in percent.
    Light(f32),
    /// The keyboard's level.
    Keys(u32),
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// The slider dragged.
    dragging: Option<Target>,
    asked: [Option<(Asked, Instant)>; 2],
    wheel: Wheel,
    /// What was last said in the log: the keyboard, the screens.
    said: Option<(Option<Keyboard>, Vec<String>)>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

impl Ui {
    /// The brightness shown: the one asked, else the backlight's.
    fn light(&self, levels: &Snapshot) -> f32 {
        match self.asked[0] {
            Some((Asked::Light(percent), _)) => percent,
            _ => levels.brightness.unwrap_or(0.0),
        }
    }

    /// The keyboard's level shown: the one asked, else UPower's.
    fn level(&self, levels: &Snapshot) -> u32 {
        match self.asked[1] {
            Some((Asked::Keys(level), _)) => level,
            _ => levels.keyboard.map_or(0, |k| k.level),
        }
    }

    fn ask(&mut self, asked: Asked) {
        let (slot, ask) = match asked {
            Asked::Light(percent) => (0, Ask::Brightness(percent)),
            Asked::Keys(level) => (1, Ask::Keyboard(level)),
        };
        if self.asked[slot].map(|(a, _)| a) != Some(asked) {
            levels::ask_power(ask);
        }
        self.asked[slot] = Some((asked, Instant::now()));
    }

    /// Until when what was asked still shows, if it does, out of a drag.
    fn until(&self) -> Option<Instant> {
        self.asked
            .iter()
            .flatten()
            .filter(|(asked, _)| !self.dragged(*asked))
            .map(|(_, since)| *since + ASKED_FOR)
            .min()
    }

    fn dragged(&self, asked: Asked) -> bool {
        matches!(
            (asked, self.dragging),
            (Asked::Light(_), Some(Target::Light)) | (Asked::Keys(_), Some(Target::Keys))
        )
    }
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A level pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// A slider dragged to a share of its travel; let go.
    Slide(Target, f32),
    Release,
    /// An arrow on what holds the ring: up (or right), or down.
    Step(bool),
    /// The wheel over the brightness's row.
    Wheel(mouse::ScrollDelta),
    /// A press anywhere on the popover.
    Pressed,
    /// The levels changed under the view.
    Stale,
}

/// The brightness's popover on its surface.
pub fn view<'a>(id: window::Id, words: Words, glass: bool) -> Element<'a, crate::Message> {
    let levels = levels::snapshot();
    let screens = levels::screens();
    let built = ui(|ui| content(words, &levels, screens, ui));
    // Asked and not said: the surface wakes when it stops showing.
    Around::new(id, built, handle, Msg::Stale)
        .rebuild(move || {
            let (levels, screens) = (levels::snapshot(), levels::screens());
            ui(|ui| content(words, &levels, screens, ui))
        })
        .stale_when(|| ui(|ui| ui.until().is_some_and(|until| Instant::now() >= until)))
        .wake_at(|| ui(|ui| ui.until()))
        .keys(arrows)
        .press(Msg::Pressed)
        .subtitle(Some(words.built_in().to_owned()))
        .on_glass(glass)
}

/// An arrow on what holds the ring moves it.
fn arrows(pressed: &Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    let up = match &pressed.key {
        Key::Named(Named::ArrowRight | Named::ArrowUp) => true,
        Key::Named(Named::ArrowLeft | Named::ArrowDown) => false,
        _ => return None,
    };
    ui(|ui| ui.focus.is_some()).then_some(Msg::Step(up))
}

fn content<'a>(words: Words, levels: &Snapshot, screens: Vec<String>, ui: &mut Ui) -> El<'a> {
    let layout = Layout::of(levels.keyboard.is_some(), notes(words, &screens));
    let said = (levels.keyboard, screens);
    if ui.said.as_ref().map(|(k, _)| k) != Some(&said.0) {
        tracing::debug!(keyboard = ?levels.keyboard, "the brightness's popover");
    }
    if ui.said.as_ref().map(|(_, s)| s) != Some(&said.1) {
        tracing::debug!(
            screens = ?layout.notes.iter().map(|(_, l)| l.join(" ")).collect::<Vec<_>>(),
            "the brightness's popover"
        );
    }
    ui.said = Some(said);
    let a11y = crate::a11y::Words::new(words.lang());
    let mut all: Vec<El<'a>> = vec![parts::header(
        words.brightness(),
        Some(said_in(words.built_in(), ASIDE, MANROPE, FAINT.get()).into()),
    )];
    // The panel's brightness.
    let light = ui.light(levels);
    let slider = Slider {
        share: light_share(light),
        fill: Some(LIGHT_GAUGE),
        track: TRACK,
        knob: KNOB,
        on_change: Box::new(|share| Msg::Slide(Target::Light, share)),
        on_release: Msg::Release,
    }
    .view(SLIDER);
    let slider = control(slider, Target::Light.id(), Role::Slider, words.brightness())
        .node(|n| {
            n.value(a11y.percent(light))
                .range(f64::from(light), 0.0, 100.0, Some(f64::from(STEP)))
        })
        .focus(
            ui.focus == Some(Target::Light),
            Msg::Focus(Target::Light),
            SLIDER / 2.0,
        );
    let value = levels.brightness.map(|_| light);
    let row = mouse_area(slider_row(sun_icon(), slider.into(), value)).on_scroll(Msg::Wheel);
    all.push(parts::block(GAP, Some(row.into())));
    if let Some(keyboard) = levels.keyboard {
        all.push(parts::block(
            GAP,
            Some(keyboard_row(words, levels, keyboard, ui)),
        ));
    }
    for (_, lines) in &layout.notes {
        let mut said_lines = column![];
        for line in lines {
            said_lines = said_lines.push(
                said_in(line.clone(), NOTE, MANROPE, FAINT.get())
                    .line_height(LineHeight::Relative(NOTE_LINE)),
            );
        }
        let note = crate::ui::line(parts::inset(said_lines), lines.join(" "));
        all.push(parts::block(GAP + NOTE_PAD, Some(note.into())));
    }
    parts::content(all)
}

/// The sun, in its slot.
fn sun_icon<'a>() -> El<'a> {
    drawn(SLOT, ROW, |f, _| {
        sun(f, Point::new((SLOT - ICON) / 2.0, (ROW - ICON) / 2.0));
    })
}

/// A row: its icon in its slot, `control` beside it, and its `value` (in
/// tabular figures, on the right) when there is one.
fn slider_row<'a>(icon: El<'a>, control: El<'a>, value: Option<f32>) -> El<'a> {
    let figures = drawn(VALUE_BOX, ROW, move |f, s| {
        if let Some(value) = value {
            kit::figures(
                f,
                Point::new(s.width, s.height / 2.0),
                value,
                kit::zero_once(),
                DIM.get(),
            );
        }
    });
    let line = row![icon, gap(BESIDE), control, gap(BESIDE), figures].align_y(Vertical::Center);
    parts::inset(container(line).height(ROW).align_y(Vertical::Center))
}

/// The keyboard's row: its icon, then its levels or its slider.
fn keyboard_row<'a>(words: Words, levels: &Snapshot, keyboard: Keyboard, ui: &Ui) -> El<'a> {
    let a11y = crate::a11y::Words::new(words.lang());
    let icon = drawn(SLOT, ROW, |f, _| {
        keys(f, Point::new((SLOT - ICON) / 2.0, (ROW - ICON) / 2.0));
    });
    let name = txt::KEYBOARD_BACKLIGHT.get(words.lang());
    let level = ui.level(levels).min(keyboard.max);
    if !keyboard.segmented() {
        let share = level as f32 / keyboard.max.max(1) as f32;
        let max = keyboard.max;
        let slider = Slider {
            share,
            fill: Some(LIGHT_GAUGE),
            track: TRACK,
            knob: KNOB,
            on_change: Box::new(move |share| Msg::Slide(Target::Keys, share * max as f32)),
            on_release: Msg::Release,
        }
        .view(SLIDER);
        let slider = control(slider, Target::Keys.id(), Role::Slider, name)
            .node(|n| {
                n.value(a11y.percent(share * 100.0)).range(
                    f64::from(share * 100.0),
                    0.0,
                    100.0,
                    Some(100.0 / f64::from(keyboard.max.max(1))),
                )
            })
            .focus(
                ui.focus == Some(Target::Keys),
                Msg::Focus(Target::Keys),
                SLIDER / 2.0,
            );
        return slider_row(icon, slider.into(), Some(share * 100.0));
    }
    let names = words.keyboard_levels(keyboard.levels());
    let mut segments = row![].spacing(SEGMENT_GAP);
    for i in 0..keyboard.levels() {
        let chosen = i == level;
        let word = names.get(i as usize).copied().unwrap_or_default();
        let target = Target::Level(i);
        let segment = button(
            container(said_in(
                word,
                SEGMENT_TEXT,
                MANROPE_SEMIBOLD,
                if chosen { TEXT } else { DIM }.get(),
            ))
            .center(Length::Fill),
        )
        .padding(0)
        .width(Length::Fill)
        .height(SEGMENT)
        .style(move |_, status| {
            if chosen {
                style::filled(CHOSEN.get(), SEGMENT_RADIUS, TEXT.get())
            } else if style::lit(status) {
                style::hovered(SEGMENT_HOVER.get(), SEGMENT_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Act(target));
        let segment = control(segment, target.id(), Role::RadioButton, word)
            .node(|n| {
                n.checked(chosen)
                    .set(i as usize + 1, keyboard.levels() as usize)
            })
            .focus(ui.focus == Some(target), Msg::Focus(target), SEGMENT_RADIUS)
            .activate(Msg::Act(target));
        segments = segments.push(segment);
    }
    let strip = container(segments)
        .padding(STRIP_PAD)
        .width(Length::Fill)
        .style(|_| style::boxed(STRIP.get(), STRIP_RADIUS));
    let line = row![icon, gap(BESIDE), crate::ui::line(strip, name)].align_y(Vertical::Center);
    parts::inset(container(line).height(ROW).align_y(Vertical::Center))
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    let levels = levels::snapshot();
    let max = levels.keyboard.map_or(0, |k| k.max);
    ui(|ui| match message {
        Msg::Stale => {}
        Msg::Pressed => ui.focus = None,
        Msg::Focus(target) => ui.focus = Some(target),
        Msg::Act(Target::Level(i)) => ui.ask(Asked::Keys(i)),
        Msg::Act(_) => {}
        Msg::Slide(target, value) => {
            ui.dragging = Some(target);
            match target {
                Target::Light => ui.ask(Asked::Light(light_at(value))),
                Target::Keys => ui.ask(Asked::Keys(value.round() as u32)),
                Target::Level(_) => {}
            }
        }
        Msg::Release => {
            ui.dragging = None;
            for (_, since) in ui.asked.iter_mut().flatten() {
                *since = Instant::now();
            }
        }
        Msg::Wheel(delta) => {
            let notches = ui.wheel.notches(&delta, kit::popover_scale());
            if notches != 0 {
                let light =
                    (ui.light(&levels) + notches as f32 * STEP).clamp(BRIGHTNESS_FLOOR, 100.0);
                ui.ask(Asked::Light(light));
            }
        }
        Msg::Step(up) => match ui.focus {
            Some(Target::Light) => {
                let by = if up { STEP } else { -STEP };
                let light = (ui.light(&levels) + by).clamp(BRIGHTNESS_FLOOR, 100.0);
                ui.ask(Asked::Light(light));
            }
            Some(Target::Keys) => {
                let level = ui.level(&levels);
                let level = if up {
                    (level + 1).min(max)
                } else {
                    level.saturating_sub(1)
                };
                ui.ask(Asked::Keys(level));
            }
            // Through the levels, choosing as it goes (a radio group's
            // way).
            Some(Target::Level(i)) => {
                let next = if up {
                    (i + 1).min(max)
                } else {
                    i.saturating_sub(1)
                };
                ui.focus = Some(Target::Level(next));
                ui.ask(Asked::Keys(next));
            }
            None => {}
        },
    });
    // What was asked stays shown until it is said, or a while.
    let said = [
        levels
            .brightness
            .map(|b| Asked::Light(b.round().max(BRIGHTNESS_FLOOR))),
        levels.keyboard.map(|k| Asked::Keys(k.level)),
    ];
    let now = Instant::now();
    ui(|ui| {
        let dragging = ui.dragging;
        for (asked, said) in ui.asked.iter_mut().zip(said) {
            let Some((what, _)) = *asked else {
                continue;
            };
            let dragged = matches!(
                (what, dragging),
                (Asked::Light(_), Some(Target::Light)) | (Asked::Keys(_), Some(Target::Keys))
            );
            if !dragged {
                kit::awaited(asked, said == Some(what), now, ASKED_FOR);
            }
        }
    });
    super::refit(super::Item::Brightness);
}

/// The keyboard, 18 px: `rect x2 y6 w20 h12 rx2`, its keys' dots and its
/// space bar.
fn keys(f: &mut Frame, at: Point) {
    use crate::levels::draw::stroke;
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, TEXT.get());
    f.stroke(
        &Path::rounded_rectangle(p(2.0, 6.0), Size::new(20.0 * s, 12.0 * s), (2.0 * s).into()),
        line,
    );
    for x in [6.0, 10.0, 14.0, 18.0] {
        f.fill(&Path::circle(p(x, 10.0), s), TEXT.get());
    }
    f.stroke(&Path::line(p(7.0, 14.0), p(17.0, 14.0)), line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_height_follows_the_keyboard_and_the_screens() {
        // The header, the brightness's row: 12 + 32 + 6 + 36 + 14, the rim.
        assert_eq!(Layout::of(false, Vec::new()).height(), 102);
        // The keyboard's row.
        assert_eq!(Layout::of(true, Vec::new()).height(), 144);
        // A screen's line on two lines: 6 + 2 + 2 × 17.5.
        let two = vec![vec!["a".to_owned(), "b".to_owned()]];
        assert_eq!(Layout::of(true, two.clone()).height(), 144 + 43);
        let both = vec![two[0].clone(), vec!["c".to_owned()]];
        assert_eq!(Layout::of(true, both).height(), 144 + 43 + 25 + 1);
    }

    #[test]
    fn the_slider_goes_from_5_to_100() {
        assert_eq!(light_at(0.0), 5.0);
        assert_eq!(light_at(1.0), 100.0);
        assert_eq!(light_share(5.0), 0.0);
        assert_eq!(light_share(2.0), 0.0);
        assert_eq!(light_share(100.0), 1.0);
        assert!((light_at(light_share(70.0)) - 70.0).abs() < 0.01);
    }
}
