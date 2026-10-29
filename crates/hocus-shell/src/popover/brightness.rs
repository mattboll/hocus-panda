//! The brightness's popover (bar.md §9, the 3.7 mockup): "Luminosité" and
//! what it sets, "Écran du portable"; the panel's brightness on a slider
//! from 5 % to 100 % (never below, as the keys), its value beside it; the
//! keyboard's backlight when the machine has one, its levels named (off,
//! low, high; off, on) when it has four at most, else on a slider; and a
//! line for each other screen plugged in: it is set with its own buttons
//! (variant B: the panel sets only the laptop's screen and its keyboard).
//!
//! One canvas draws it, as the sound's: what changes draws this surface
//! alone. The slider sends each brightness as it moves; the system bus's
//! thread asks logind only the last one each time logind is ready. It
//! shows what was asked until the backlight says it, a second at most.
//!
//! The keyboard: `Tab` and `Shift+Tab` go through the slider and the
//! keyboard's levels (or its slider); the arrows move a slider by 5 % (the
//! keyboard's by a level) and choose the next level; `Enter` or `Space`
//! chooses; the wheel over the brightness's row moves it by 5 %; `Esc`
//! closes.

use std::time::{Duration, Instant};

use hocus_link::POPOVER_WIDTH;
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, event,
    sound::{Track, slider},
    wifi::{MANROPE_SEMIBOLD, background, measure, text, wrap},
};
use crate::levels::{
    self,
    draw::{DIM, FAINT, LIGHT_GAUGE, LINE, MANROPE, MANROPE_BOLD, TEXT, VALUE, cell, ring, sun},
    model::{BRIGHTNESS_FLOOR, Keyboard, STEP, Words},
    power::Ask,
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
const HEADER_TOP: f32 = BORDER + PAD_TOP;
const TITLE: f32 = 16.0;
const NAME: f32 = 13.0;
/// A row: its icon in a 32 px slot, then the slider or the levels.
const ROW: f32 = 36.0;
const SLOT: f32 = 32.0;
const ICON: f32 = 18.0;
const ROW_GAP: f32 = 8.0;
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

const STRIP: Color = Color::from_rgba8(255, 255, 255, 0.06);
const CHOSEN: Color = Color::from_rgba8(255, 255, 255, 0.14);
const SEGMENT_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.08);

/// A slider shows what was asked this long at most, until it is said.
const ASKED_FOR: Duration = Duration::from_secs(1);

const LIGHT_TOP: f32 = HEADER_TOP + HEADER + GAP;
const KEYBOARD_TOP: f32 = LIGHT_TOP + ROW + GAP;

/// The words of the shell's language, for what the height measures.
fn words() -> Words {
    Words::new(crate::clock::time::Lang::of_messages())
}

/// Each other screen's line, broken to fit.
fn notes(words: Words, screens: &[String]) -> Vec<Vec<String>> {
    screens
        .iter()
        .map(|name| {
            wrap(&words.own_buttons(name), RIGHT - LEFT, |t| {
                measure(t, NOTE, MANROPE)
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
        (self.end + PAD_BOTTOM + BORDER).round() as u32
    }
}

/// The popover opens: nothing to start.
pub fn opening() {}

/// Its height for what it shows now.
pub fn height() -> u32 {
    let words = words();
    let keyboard = levels::snapshot().keyboard.is_some();
    Layout::of(keyboard, notes(words, &levels::screens())).height()
}

/// The brightness's popover on its surface.
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
    Light,
    /// One of the keyboard's levels, from off.
    Level(u32),
    /// The keyboard's slider, past four levels.
    Keys,
}

/// What a slider asked, until it is said.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Asked {
    /// The brightness, in percent.
    Light(f32),
    /// The keyboard's level.
    Keys(u32),
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    brightness: Option<f32>,
    keyboard: Option<Keyboard>,
    layout: Option<Layout>,
    /// What it was laid out for: the keyboard there, the screens.
    made_for: Option<(bool, Vec<String>)>,
    /// The `0`'s advance of the value's font, for tabular figures.
    zero: Option<f32>,
    hover: Option<Target>,
    pressed: Option<Target>,
    dragging: Option<Target>,
    asked: [Option<(Asked, Instant)>; 2],
    focus: Option<Target>,
    pointer: Option<Point>,
    inside: bool,
    /// A wheel's notch comes as lines, then as pixels, which count for
    /// nothing; a touchpad's pixels add up to notches.
    after_notch: bool,
    scroll: f32,
}

impl State {
    /// What can hold the keyboard, in `Tab`'s order.
    fn targets(&self) -> Vec<Target> {
        let mut order = vec![Target::Light];
        match self.keyboard {
            Some(k) if k.segmented() => order.extend((0..=k.max).map(Target::Level)),
            Some(_) => order.push(Target::Keys),
            None => {}
        }
        order
    }

    /// The brightness shown: the one asked, else the backlight's.
    fn light(&self) -> f32 {
        match self.asked[0] {
            Some((Asked::Light(percent), _)) => percent,
            _ => self.brightness.unwrap_or(0.0),
        }
    }

    /// The keyboard's level shown: the one asked, else UPower's.
    fn level(&self) -> u32 {
        match self.asked[1] {
            Some((Asked::Keys(level), _)) => level,
            _ => self.keyboard.map_or(0, |k| k.level),
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
}

/// The brightness's slider: from the icon's slot to the value.
fn track(top: f32) -> Track {
    Track {
        left: LEFT + SLOT + ROW_GAP,
        right: RIGHT - VALUE_BOX - ROW_GAP,
        middle: top + ROW / 2.0,
        height: TRACK,
        knob: KNOB,
    }
}

/// The brightness's share of its slider's travel: 5 % at its left end.
fn light_share(percent: f32) -> f32 {
    ((percent - BRIGHTNESS_FLOOR) / (100.0 - BRIGHTNESS_FLOOR)).clamp(0.0, 1.0)
}

fn light_at(share: f32) -> f32 {
    (BRIGHTNESS_FLOOR + share * (100.0 - BRIGHTNESS_FLOOR)).round()
}

/// The keyboard's levels' strip, and level `i`'s button in it.
fn strip() -> Rectangle {
    let left = LEFT + SLOT + ROW_GAP;
    let height = SEGMENT + 2.0 * STRIP_PAD;
    Rectangle::new(
        Point::new(left, KEYBOARD_TOP + (ROW - height) / 2.0),
        Size::new(RIGHT - left, height),
    )
}

fn segment(i: u32, levels: u32) -> Rectangle {
    let strip = strip();
    let n = levels.max(1) as f32;
    let width = (strip.width - 2.0 * STRIP_PAD - (n - 1.0) * SEGMENT_GAP) / n;
    Rectangle::new(
        Point::new(
            strip.x + STRIP_PAD + i as f32 * (width + SEGMENT_GAP),
            strip.y + STRIP_PAD,
        ),
        Size::new(width, SEGMENT),
    )
}

impl Target {
    /// Where it is, and its corners' radius.
    fn shape(self, keyboard: Option<Keyboard>) -> (Rectangle, f32) {
        let slider = |track: Track| {
            Rectangle::new(
                Point::new(track.left, track.middle - SLIDER / 2.0),
                Size::new(track.right - track.left, SLIDER),
            )
        };
        match self {
            Self::Light => (slider(track(LIGHT_TOP)), SLIDER / 2.0),
            Self::Keys => (slider(track(KEYBOARD_TOP)), SLIDER / 2.0),
            Self::Level(i) => (
                segment(i, keyboard.map_or(1, Keyboard::levels)),
                SEGMENT_RADIUS,
            ),
        }
    }
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
        background(&mut frame, bounds.size(), self.glass);
        let Some(layout) = &state.layout else {
            return vec![frame.into_geometry()];
        };
        self.header(&mut frame);
        // The panel's brightness.
        let middle = LIGHT_TOP + ROW / 2.0;
        sun(
            &mut frame,
            Point::new(LEFT + (SLOT - ICON) / 2.0, middle - ICON / 2.0),
        );
        let light = state.light();
        slider(
            &mut frame,
            track(LIGHT_TOP),
            light_share(light),
            Some(LIGHT_GAUGE),
        );
        let zero = state.zero.unwrap_or(VALUE * 0.6);
        if state.brightness.is_some() {
            number(&mut frame, Point::new(RIGHT, middle), light, zero);
        }
        if let Some(keyboard) = state.keyboard.filter(|_| layout.keyboard) {
            self.keyboard(&mut frame, state, keyboard, zero);
        }
        for (top, lines) in &layout.notes {
            for (i, line) in lines.iter().enumerate() {
                let mut note = text(
                    line.clone(),
                    Point::new(LEFT, top + (i as f32 + 0.5) * NOTE * NOTE_LINE),
                    NOTE,
                    FAINT,
                    Alignment::Left,
                );
                note.line_height = iced::widget::text::LineHeight::Relative(NOTE_LINE);
                frame.fill_text(note);
            }
        }
        if let Some(focus) = state.focus {
            let (rect, radius) = focus.shape(state.keyboard);
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

/// `value` right-aligned on `at` in tabular figures.
fn number(f: &mut Frame, at: Point, value: f32, zero: f32) {
    let digits = (value.round() as u32).to_string();
    let mut x = at.x - zero * digits.len() as f32;
    for ch in digits.chars() {
        cell(f, ch, x + zero / 2.0, at.y, VALUE, MANROPE, DIM);
        x += zero;
    }
}

impl Panel {
    /// What it shows, read afresh: the brightness, the keyboard's
    /// backlight, the other screens.
    fn refresh<Message>(&self, state: &mut State, now: Instant) -> Option<canvas::Action<Message>> {
        // Keys that reached another of the shell's surfaces (see
        // `popover::key`).
        let keys = super::take_keys();
        if !keys.is_empty() && super::with(|c| c.is_open(self.id)) {
            for pressed in keys {
                let shift = pressed.modifiers.shift();
                let _: Option<canvas::Action<Message>> = self.key(state, &pressed.key, shift);
            }
        }
        let snapshot = levels::snapshot();
        if state.layout.is_none() || state.keyboard != snapshot.keyboard {
            tracing::debug!(keyboard = ?snapshot.keyboard, "the brightness's popover");
        }
        state.brightness = snapshot.brightness;
        state.keyboard = snapshot.keyboard;
        let made_for = (snapshot.keyboard.is_some(), levels::screens());
        if state.made_for.as_ref() != Some(&made_for) || state.layout.is_none() {
            let layout = Layout::of(made_for.0, notes(self.words, &made_for.1));
            tracing::debug!(
                screens = ?layout.notes.iter().map(|(_, l)| l.join(" ")).collect::<Vec<_>>(),
                "the brightness's popover"
            );
            state.layout = Some(layout);
            state.made_for = Some(made_for);
            // A change of size reaches the controller, which resizes the
            // surface (without waking it again).
            super::refit(super::Item::Brightness);
        }
        if state.zero.is_none()
            && let Ok(mut system) = iced::advanced::graphics::text::font_system().write()
        {
            state.zero =
                crate::clock::measure(system.raw(), MANROPE, VALUE, VALUE * LINE).map(|m| m.zero);
        }
        if state
            .focus
            .is_some_and(|focus| !state.targets().contains(&focus))
        {
            state.focus = None;
        }
        // What was asked stays shown until it is said, or a while.
        let said = [
            state
                .brightness
                .map(|b| Asked::Light(b.round().max(BRIGHTNESS_FLOOR))),
            state.keyboard.map(|k| Asked::Keys(k.level)),
        ];
        let mut until = None;
        for (asked, said) in state.asked.iter_mut().zip(said) {
            let Some((what, since)) = *asked else {
                continue;
            };
            let dragged = matches!(
                (what, state.dragging),
                (Asked::Light(_), Some(Target::Light)) | (Asked::Keys(_), Some(Target::Keys))
            );
            if dragged {
                continue;
            }
            if said == Some(what) || now >= since + ASKED_FOR {
                *asked = None;
            } else {
                until =
                    Some(until.map_or(since + ASKED_FOR, |u: Instant| u.min(since + ASKED_FOR)));
            }
        }
        until.map(canvas::Action::request_redraw_at)
    }

    fn header(&self, f: &mut Frame) {
        let middle = HEADER_TOP + HEADER / 2.0;
        let mut title = text(
            self.words.brightness().to_owned(),
            Point::new(LEFT, middle),
            TITLE,
            TEXT,
            Alignment::Left,
        );
        title.font = MANROPE_BOLD;
        f.fill_text(title);
        f.fill_text(text(
            self.words.built_in().to_owned(),
            Point::new(RIGHT, middle),
            NAME,
            FAINT,
            Alignment::Right,
        ));
    }

    /// The keyboard's row: its icon, then its levels or its slider.
    fn keyboard(&self, f: &mut Frame, state: &State, keyboard: Keyboard, zero: f32) {
        let middle = KEYBOARD_TOP + ROW / 2.0;
        keys(
            f,
            Point::new(LEFT + (SLOT - ICON) / 2.0, middle - ICON / 2.0),
        );
        let level = state.level().min(keyboard.max);
        if !keyboard.segmented() {
            let share = level as f32 / keyboard.max as f32;
            slider(f, track(KEYBOARD_TOP), share, Some(LIGHT_GAUGE));
            number(f, Point::new(RIGHT, middle), share * 100.0, zero);
            return;
        }
        let strip = strip();
        f.fill(
            &Path::rounded_rectangle(strip.position(), strip.size(), STRIP_RADIUS.into()),
            STRIP,
        );
        let names = self.words.keyboard_levels(keyboard.levels());
        for i in 0..keyboard.levels() {
            let rect = segment(i, keyboard.levels());
            let chosen = i == level;
            let fill = if chosen {
                Some(CHOSEN)
            } else if state.hover == Some(Target::Level(i)) {
                Some(SEGMENT_HOVER)
            } else {
                None
            };
            if let Some(fill) = fill {
                f.fill(
                    &Path::rounded_rectangle(rect.position(), rect.size(), SEGMENT_RADIUS.into()),
                    fill,
                );
            }
            let mut name = text(
                names
                    .get(i as usize)
                    .copied()
                    .unwrap_or_default()
                    .to_owned(),
                rect.center(),
                SEGMENT_TEXT,
                if chosen { TEXT } else { DIM },
                Alignment::Center,
            );
            name.font = MANROPE_SEMIBOLD;
            f.fill_text(name);
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| {
            state
                .targets()
                .into_iter()
                .find(|t| t.shape(state.keyboard).0.contains(at))
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                if !state.inside && at.is_some() {
                    state.inside = true;
                    event(Seen::PopoverEnter);
                }
                if let (Some(slider), Some(at)) = (state.dragging, at) {
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
                if let (Some(slider @ (Target::Light | Target::Keys)), Some(at)) = (target, at) {
                    state.dragging = Some(slider);
                    self.slide(state, slider, at.x);
                }
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                let pressed = state.pressed.take();
                if state.dragging.take().is_some() {
                    for (_, since) in state.asked.iter_mut().flatten() {
                        *since = Instant::now();
                    }
                } else if let Some(Target::Level(i)) = pressed.filter(|p| Some(*p) == target) {
                    state.ask(Asked::Keys(i));
                }
                redraw
            }
            // The wheel over the brightness's row: 5 % a notch.
            mouse::Event::WheelScrolled { delta } => {
                let row = Rectangle::new(Point::new(LEFT, LIGHT_TOP), Size::new(RIGHT - LEFT, ROW));
                if !at.is_some_and(|at| row.contains(at)) {
                    return None;
                }
                let notches = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => {
                        state.after_notch = true;
                        state.scroll = 0.0;
                        y.signum() as i32
                    }
                    mouse::ScrollDelta::Pixels { .. } if state.after_notch => {
                        state.after_notch = false;
                        0
                    }
                    mouse::ScrollDelta::Pixels { y, .. } => {
                        state.scroll += y;
                        let n = (state.scroll / 15.0).trunc();
                        state.scroll -= n * 15.0;
                        n as i32
                    }
                };
                if notches != 0 {
                    let light =
                        (state.light() + notches as f32 * STEP).clamp(BRIGHTNESS_FLOOR, 100.0);
                    state.ask(Asked::Light(light));
                }
                redraw
            }
            _ => None,
        }
    }

    /// A slider moved to `x`: that value, asked at once.
    fn slide(&self, state: &mut State, slider: Target, x: f32) {
        match slider {
            Target::Light => {
                let share = track(LIGHT_TOP).share_at(x);
                state.ask(Asked::Light(light_at(share)));
            }
            Target::Keys => {
                let Some(keyboard) = state.keyboard else {
                    return;
                };
                let share = track(KEYBOARD_TOP).share_at(x);
                state.ask(Asked::Keys((share * keyboard.max as f32).round() as u32));
            }
            Target::Level(_) => {}
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
        match key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => {
                let n = order.len() as isize;
                let by = if shift { -1 } else { 1 };
                let next = match at {
                    Some(i) => (i as isize + by).rem_euclid(n),
                    None if by > 0 => 0,
                    None => n - 1,
                };
                state.focus = order.get(next as usize).copied();
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(Target::Level(i)) = state.focus {
                    state.ask(Asked::Keys(i));
                }
            }
            Key::Named(
                arrow @ (Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown),
            ) => {
                let up = matches!(arrow, Named::ArrowRight | Named::ArrowUp);
                let max = state.keyboard.map_or(0, |k| k.max);
                match state.focus {
                    Some(Target::Light) => {
                        let by = if up { STEP } else { -STEP };
                        let light = (state.light() + by).clamp(BRIGHTNESS_FLOOR, 100.0);
                        state.ask(Asked::Light(light));
                    }
                    Some(Target::Keys) => {
                        let level = state.level();
                        let level = if up {
                            (level + 1).min(max)
                        } else {
                            level.saturating_sub(1)
                        };
                        state.ask(Asked::Keys(level));
                    }
                    // Through the levels, choosing as it goes (a radio
                    // group's way).
                    Some(Target::Level(i)) => {
                        let next = if up {
                            (i + 1).min(max)
                        } else {
                            i.saturating_sub(1)
                        };
                        state.focus = Some(Target::Level(next));
                        state.ask(Asked::Keys(next));
                    }
                    None => return None,
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
}

/// The keyboard, 18 px: `rect x2 y6 w20 h12 rx2`, its keys' dots and its
/// space bar.
fn keys(f: &mut Frame, at: Point) {
    use crate::levels::draw::stroke;
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, TEXT);
    f.stroke(
        &Path::rounded_rectangle(p(2.0, 6.0), Size::new(20.0 * s, 12.0 * s), (2.0 * s).into()),
        line,
    );
    for x in [6.0, 10.0, 14.0, 18.0] {
        f.fill(&Path::circle(p(x, 10.0), s), TEXT);
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
        let t = track(LIGHT_TOP);
        assert_eq!(light_at(t.share_at(t.left)), 5.0);
        assert_eq!(light_at(t.share_at(t.right)), 100.0);
        assert_eq!(light_share(5.0), 0.0);
        assert_eq!(light_share(2.0), 0.0);
        assert_eq!(light_share(100.0), 1.0);
        assert!((light_at(light_share(70.0)) - 70.0).abs() < 0.01);
    }

    #[test]
    fn the_keyboards_levels_share_its_strip() {
        let strip = strip();
        for levels in [2, 3, 4] {
            let first = segment(0, levels);
            let last = segment(levels - 1, levels);
            assert_eq!(first.x, strip.x + STRIP_PAD);
            assert!((last.x + last.width - (strip.x + strip.width - STRIP_PAD)).abs() < 0.01);
            assert!(first.width > 40.0, "{levels}: {}", first.width);
        }
        let keyboard = |max| State {
            keyboard: Some(Keyboard { level: 0, max }),
            ..State::default()
        };
        assert_eq!(
            keyboard(2).targets(),
            [
                Target::Light,
                Target::Level(0),
                Target::Level(1),
                Target::Level(2)
            ]
        );
        assert_eq!(keyboard(5).targets(), [Target::Light, Target::Keys]);
        assert_eq!(State::default().targets(), [Target::Light]);
    }
}
