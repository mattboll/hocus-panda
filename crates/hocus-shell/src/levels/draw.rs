//! The levels' surface, near the bar's bottom, as the 3.3 mockup draws it
//! at the larger sizes of #378.
//!
//! Full form: a row for the sound, the brightness and the battery (icon,
//! 8 px gauge with its gradient, value in tabular figures), then the status
//! line, right-aligned; the group's bottom sits 61 px above the bar's,
//! above the footer's place. Mini form: the sound's vertical gauge, then
//! the battery's percentage (a bolt when charging), 41 px above the bar's
//! bottom, above the place of what comes under it. A row whose service is
//! missing is not drawn, and the group closes up from the bottom.
//!
//! A gauge's length moves to a new value over 160 ms, CSS's `ease-out`;
//! with reduced motion, or on the CPU, it changes at once. The surface asks
//! to be drawn again only while one moves.
//!
//! The sound's row (the gauge, in the mini form) opens its popover (§9,
//! [`crate::popover`]): a click, or in the mini form a hover; it lights up
//! under the pointer and while its popover is open, and wears a ring while
//! it holds the keyboard; the wheel over it changes the volume, 5 % a
//! notch. All of it draws this surface alone.

use std::time::{Duration, Instant};

use hocus_link::{Form, LevelKey};
use iced::{
    Color, Font, Pixels, Point, Radians, Rectangle, Renderer, Size, Theme, alignment, font,
    keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke, Style, gradient},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::model::{Band, Snapshot, Words};
use crate::{
    clock::{self, motion::bezier},
    popover::{self, Item},
};

/// The surface: as wide as the bar inside its rim, from 41 px above the
/// bar's bottom; tall enough for its form's group (the full one while the
/// form switches), and the sound's item lit and ringed above it.
pub const BOTTOM: i32 = 41;
pub const FULL_SURFACE: (u32, u32) = (194, 175);
pub const MINI_SURFACE: (u32, u32) = (54, 97);
/// The full group's bottom above the surface's: the footer's place.
const FULL_RAISE: f32 = 20.0;
/// From the bar's inside edges to the group's: 12 px padding less the rim.
pub(crate) const SIDE: f32 = 11.0;

pub(crate) const MANROPE: Font = Font::with_name("Manrope");
pub(crate) const MANROPE_BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..MANROPE
};
/// CSS's `normal` line height for Manrope.
pub(crate) const LINE: f32 = 1.366;
pub(crate) const VALUE: f32 = 14.5;
const STATUS: f32 = 13.5;
const MINI_VALUE: f32 = 14.0;
const ROW: f32 = VALUE * LINE;
const ROW_GAP: f32 = 10.0;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 8.0;
const VALUE_BOX: f32 = 36.0;
const GAUGE: f32 = 8.0;
const MINI_GAUGE: f32 = 48.0;
const MINI_GAP: f32 = 16.0;
/// The mini form's bolt and the gap after it, in the mockup's units: a
/// pixel each at its 11 px percentage, scaled with it.
const BOLT_GAP: f32 = 2.0;
const MINI_BOLT: f32 = MINI_VALUE / 11.0;

/// The sound's item: the whole row in the full form (30 px, 6 px past the
/// group's sides), around the gauge in the mini (44 × 64), as the 3.4
/// mockup's buttons; lit under the pointer, more while its popover is
/// open; the keyboard's ring 3 px wide, 2 px out, in the accent.
pub(crate) const ITEM_HEIGHT: f32 = 30.0;
pub(crate) const ITEM_OUT: f32 = 6.0;
pub(crate) const ITEM_RADIUS: f32 = 8.0;
const MINI_ITEM: Size = Size::new(44.0, 64.0);
pub(crate) const MINI_ITEM_RADIUS: f32 = 10.0;
pub(crate) const HOVER: Color = Color::from_rgba8(255, 255, 255, 0.06);
pub(crate) const OPEN: Color = Color::from_rgba8(255, 255, 255, 0.08);
pub(crate) const ACCENT: Color = rgb(0x6CC287);
pub(crate) const RING: f32 = 3.0;
pub(crate) const RING_OUT: f32 = 2.0;
/// A wheel's notch is 15 px of scrolling, as the compositor writes it.
const NOTCH: f32 = 15.0;

pub(crate) const fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// `text`, `text.dim`, `text.faint` (bar.md §13).
pub(crate) const TEXT: Color = rgb(0xEEF1F6);
pub(crate) const DIM: Color = rgb(0xA9B2C2);
pub(crate) const FAINT: Color = rgb(0x8C96A8);
pub(crate) const TRACK: Color = Color::from_rgba8(255, 255, 255, 0.10);
pub(crate) const MUTED_FILL: Color = Color::from_rgba8(255, 255, 255, 0.22);
/// `gauge.sound`, `gauge.light`, `gauge.battery`, and the battery's low
/// and critical ones, from the left (or the bottom) to the right (the top).
pub(crate) const SOUND: (Color, Color) = (rgb(0x5B8DEF), rgb(0x8FB3FF));
const LIGHT: (Color, Color) = (rgb(0xE8B44E), rgb(0xFFE08A));
const BATTERY: (Color, Color) = (rgb(0x3FA66B), rgb(0x7EE0A0));
const LOW: (Color, Color) = (rgb(0xE8A13A), rgb(0xF5C46E));
const CRITICAL: (Color, Color) = (rgb(0xE5793F), rgb(0xF7B98A));
const BOLT: Color = rgb(0x7EE0A0);

/// A gauge's motion: 160 ms, `ease-out`.
const MOVE: Duration = Duration::from_millis(160);
const EASE_OUT: (f32, f32, f32, f32) = (0.0, 0.0, 0.58, 1.0);

/// The levels of one bar, as the shell sees them at its last update.
#[derive(Debug, Clone)]
pub struct Levels {
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn now; the surface's right edge is the
    /// bar's inside right edge.
    pub bar_width: f32,
    pub motion: bool,
    pub words: Words,
    /// Without glass: the bar's opaque colour behind (no alpha on the CPU).
    pub backdrop: Option<Color>,
    /// Its bar's output: where its items are in the bar's frame, for the
    /// popovers.
    pub bar: String,
}

/// A gauge's length, moving or still.
#[derive(Debug, Clone, Copy, Default)]
struct Tween {
    from: f32,
    to: f32,
    since: Option<Instant>,
}

impl Tween {
    fn at(&self, now: Instant) -> f32 {
        let Some(since) = self.since else {
            return self.to;
        };
        let t = now.saturating_duration_since(since).as_secs_f32() / MOVE.as_secs_f32();
        if t >= 1.0 {
            self.to
        } else {
            self.from + (self.to - self.from) * bezier(EASE_OUT, t)
        }
    }

    fn moving(&self, now: Instant) -> bool {
        self.since
            .is_some_and(|since| now.saturating_duration_since(since) < MOVE)
    }

    /// Toward `to` from where it is at `now`; at once without motion, or
    /// when it was not shown.
    fn toward(&mut self, to: Option<f32>, now: Instant, motion: bool, shown: bool) {
        let Some(to) = to.map(|t| t.clamp(0.0, 100.0)) else {
            return;
        };
        if to == self.to {
            return;
        }
        *self = if motion && shown {
            Self {
                from: self.at(now),
                to,
                since: Some(now),
            }
        } else {
            Self {
                from: to,
                to,
                since: None,
            }
        };
    }
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    levels: Snapshot,
    /// The status line, and its lines as they fit in the bar.
    status: Option<(String, Vec<String>)>,
    /// Sound, brightness, battery.
    gauges: [Tween; 3],
    at: Option<Instant>,
    /// The `0`'s advance of the values' fonts, for tabular figures.
    zero: Option<(f32, f32)>,
    /// The pointer on the surface; on the sound's item; and where it was
    /// last seen, in the bar's frame.
    pointer: Option<Point>,
    hover: bool,
    last: Option<Point>,
    /// Scrolling not yet a notch; a wheel's notch also comes as pixels,
    /// right after it, which then count for nothing.
    scroll: f32,
    after_notch: bool,
    scroll_up: bool,
}

impl<Message> canvas::Program<Message> for Levels {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let now = match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => *now,
            iced::Event::Mouse(event) => return self.mouse(state, event, bounds, cursor),
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                return popover::key(key, *modifiers, text.as_deref())
                    .then(|| canvas::Action::request_redraw().and_capture());
            }
            _ => return None,
        };
        let levels = super::snapshot();
        let before = state.levels;
        if levels != before {
            tracing::debug!("the levels' surface shows the change");
        }
        let targets = [
            (levels.sound.map(|s| s.volume), before.sound.is_some()),
            (levels.brightness, before.brightness.is_some()),
            (levels.battery.map(|b| b.percent), before.battery.is_some()),
        ];
        let shown = state.at.is_some();
        for (gauge, (to, was)) in state.gauges.iter_mut().zip(targets) {
            gauge.toward(to, now, self.motion, shown && was);
        }
        state.levels = levels;
        state.at = Some(now);
        let status = self.words.status(&levels);
        if state.status.as_ref().map(|(s, _)| s) != status.as_ref() {
            state.status = status.map(|status| {
                let lines = match iced::advanced::graphics::text::font_system().write() {
                    Ok(mut system) => {
                        let fits = FULL_SURFACE.0 as f32 - 2.0 * SIDE;
                        lines(&status, fits, |text| width(system.raw(), text))
                    }
                    Err(_) => vec![status.clone()],
                };
                (status, lines)
            });
        }
        // The connections sit above the group: they follow its top.
        let lines = state.status.as_ref().map_or(0, |(_, l)| l.len());
        crate::network::draw::levels_moved(&self.bar, group_tops(&levels, lines));
        if state.zero.is_none()
            && let Ok(mut system) = iced::advanced::graphics::text::font_system().write()
        {
            let mut zero = |font, size| clock::measure(system.raw(), font, size, size * LINE);
            state.zero = zero(MANROPE, VALUE)
                .zip(zero(MANROPE_BOLD, MINI_VALUE))
                .map(|(a, b)| (a.zero, b.zero));
        }
        state
            .gauges
            .iter()
            .any(|g| g.moving(now))
            .then(canvas::Action::request_redraw)
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
        if let Some(colour) = self.backdrop {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), colour);
        }
        let Some(at) = state.at else {
            return vec![frame.into_geometry()];
        };
        let right = bounds.width;
        let left = right + 2.0 - self.bar_width;
        let column = Rectangle::new(
            Point::new(left.max(0.0), 0.0),
            Size::new((right - left.max(0.0)).max(0.0), bounds.height),
        );
        if let Some(item) = self.item(state, bounds.size()) {
            let (open, focused) = popover::with(|c| {
                (
                    c.shows(&self.bar, Item::Sound),
                    c.focused(&self.bar, Item::Sound),
                )
            });
            let radius = match self.forms.first() {
                Some((Form::Mini, _)) => MINI_ITEM_RADIUS,
                _ => ITEM_RADIUS,
            };
            let lit = if open {
                Some(OPEN)
            } else if state.hover {
                Some(HOVER)
            } else {
                None
            };
            if let Some(colour) = lit {
                frame.fill(
                    &Path::rounded_rectangle(item.position(), item.size(), radius.into()),
                    colour,
                );
            }
            if focused {
                ring(&mut frame, item, radius);
            }
        }
        let gauges = state.gauges.map(|g| g.at(at));
        let (zero, zero_bold) = state.zero.unwrap_or((VALUE * 0.6, MINI_VALUE * 0.65));
        let drawn = Drawn {
            levels: &state.levels,
            status: state
                .status
                .as_ref()
                .map_or(&[], |(_, lines)| lines.as_slice()),
            gauges,
            words: self.words,
            left: left + SIDE,
            right: right - SIDE,
            bottom: bounds.height,
            zero,
            zero_bold,
        };
        frame.with_clip(column, |f| {
            for &(form, alpha) in &self.forms {
                match form {
                    Form::Full => drawn.full(f, alpha),
                    Form::Mini => drawn.mini(f, (left + right) / 2.0, alpha),
                }
            }
        });
        vec![frame.into_geometry()]
    }
}

impl Levels {
    /// Where the sound's item is on the surface (`size`), if it shows and
    /// the bar is not switching.
    fn item(&self, state: &State, size: Size) -> Option<Rectangle> {
        let [(form, _)] = self.forms.as_slice() else {
            return None;
        };
        state.levels.sound?;
        state.at?;
        let right = size.width;
        let left = right + 2.0 - self.bar_width;
        Some(match form {
            Form::Full => {
                let lines = state.status.as_ref().map_or(0, |(_, l)| l.len());
                let middle = full_rows(&state.levels, lines, size.height)[2]?;
                let (from, to) = (left + SIDE - ITEM_OUT, right - SIDE + ITEM_OUT);
                Rectangle::new(
                    Point::new(from, middle - ITEM_HEIGHT / 2.0),
                    Size::new(to - from, ITEM_HEIGHT),
                )
            }
            Form::Mini => {
                let middle = mini_gauge_bottom(&state.levels, size.height) - MINI_GAUGE / 2.0;
                Rectangle::new(
                    Point::new(
                        (left + right - MINI_ITEM.width) / 2.0,
                        middle - MINI_ITEM.height / 2.0,
                    ),
                    MINI_ITEM,
                )
            }
        })
    }

    /// A point of the surface (`size`) in the bar's frame: the surface's
    /// right edge is 1 px inside the bar's, its bottom [`BOTTOM`] px above
    /// the bar's.
    fn in_bar(&self, at: Point, size: Size) -> Point {
        Point::new(
            at.x - size.width - 1.0,
            popover::with(|c| c.bar_height(&self.bar)) - BOTTOM as f32 - size.height + at.y,
        )
    }

    fn in_bar_rect(&self, rect: Rectangle, size: Size) -> Rectangle {
        Rectangle::new(self.in_bar(rect.position(), size), rect.size())
    }

    /// The pointer on the surface: the item lights up, its popover opens or
    /// stays; the wheel over it changes the volume.
    fn mouse<Message>(
        &self,
        state: &mut State,
        event: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        let item = self.item(state, size);
        let at = pointer(event, bounds, cursor, state.pointer);
        state.pointer = at;
        let over = item.zip(at).is_some_and(|(item, at)| item.contains(at));
        match event {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                let at = at?;
                let bar_at = self.in_bar(at, size);
                state.last = Some(bar_at);
                if over == state.hover {
                    if !over {
                        popover::event(popover::Event::Pointer { at: bar_at });
                    }
                    return None;
                }
                state.hover = over;
                match item.filter(|_| over) {
                    Some(item) => popover::event(popover::Event::ItemEnter {
                        bar: self.bar.clone(),
                        item: Item::Sound,
                        rect: self.in_bar_rect(item, size),
                    }),
                    None => popover::event(popover::Event::ItemLeave { at: bar_at }),
                }
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::CursorLeft => {
                if !std::mem::take(&mut state.hover) {
                    return None;
                }
                let at = state.last.unwrap_or_default();
                popover::event(popover::Event::ItemLeave { at });
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let item = item.filter(|_| over);
                let event = match item {
                    Some(item) => popover::Event::ItemPress {
                        bar: self.bar.clone(),
                        item: Item::Sound,
                        rect: self.in_bar_rect(item, size),
                    },
                    None => popover::Event::PressElsewhere,
                };
                popover::event(event);
                Some(canvas::Action::request_redraw().and_capture())
            }
            mouse::Event::WheelScrolled { delta } if over => {
                for _ in 0..notches(state, *delta).unsigned_abs() {
                    let up = state.scroll_up;
                    super::key(if up {
                        LevelKey::VolumeUp
                    } else {
                        LevelKey::VolumeDown
                    });
                }
                Some(canvas::Action::capture())
            }
            _ => None,
        }
    }
}

/// Where the pointer is on a canvas at `bounds` as of `event`, which was
/// at `before`: iced hands every event of a batch the cursor as it is after
/// the batch, which may have left already when a press and a move away
/// came together.
pub(crate) fn pointer(
    event: &mouse::Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
    before: Option<Point>,
) -> Option<Point> {
    match event {
        mouse::Event::CursorMoved { position } => {
            Some(Point::new(position.x - bounds.x, position.y - bounds.y))
        }
        mouse::Event::CursorEntered => cursor.position_in(bounds),
        mouse::Event::CursorLeft => None,
        _ => before,
    }
}

/// The notches a scroll makes, with what was scrolled before it: a wheel
/// sends its notches as lines, then the same as pixels, which count for
/// nothing; a touchpad sends pixels, a notch each [`NOTCH`]. Positive: up,
/// louder; the direction is kept in `state.scroll_up`.
fn notches(state: &mut State, delta: mouse::ScrollDelta) -> i32 {
    let n = match delta {
        mouse::ScrollDelta::Lines { y, .. } => {
            state.after_notch = true;
            state.scroll = 0.0;
            if y > 0.0 {
                1
            } else if y < 0.0 {
                -1
            } else {
                0
            }
        }
        mouse::ScrollDelta::Pixels { y: 0.0, .. } => 0,
        mouse::ScrollDelta::Pixels { .. } if state.after_notch => {
            state.after_notch = false;
            0
        }
        mouse::ScrollDelta::Pixels { y, .. } => {
            if state.scroll * y < 0.0 {
                state.scroll = 0.0;
            }
            state.scroll += y;
            let n = (state.scroll / NOTCH).trunc();
            state.scroll -= n * NOTCH;
            n as i32
        }
    };
    state.scroll_up = n > 0;
    n
}

/// The full form's rows' middles, from the surface's bottom: the
/// battery's, the brightness's and the sound's, each if it shows, under a
/// status line of `status` lines.
fn full_rows(levels: &Snapshot, status: usize, bottom: f32) -> [Option<f32>; 3] {
    let mut bottom = bottom - FULL_RAISE;
    if status > 0 {
        bottom -= status as f32 * STATUS * LINE + ROW_GAP;
    }
    let mut middles = [None; 3];
    let shown = [
        levels.battery.is_some(),
        levels.brightness.is_some(),
        levels.sound.is_some(),
    ];
    for (middle, shown) in middles.iter_mut().zip(shown) {
        if shown {
            *middle = Some(bottom - ROW / 2.0);
            bottom -= ROW + ROW_GAP;
        }
    }
    middles
}

/// How far above the surface's bottom the group's top is, in the full
/// form and in the mini: its first row's item (30 px) in the full form, the
/// gauge (or the battery's percentage) in the mini; nothing shown, the
/// group's bottom.
pub fn group_tops(levels: &Snapshot, status: usize) -> (f32, f32) {
    let height = FULL_SURFACE.1 as f32;
    let rows = full_rows(levels, status, height);
    let full = match rows.iter().flatten().copied().reduce(f32::min) {
        Some(middle) => height - (middle - ITEM_HEIGHT / 2.0),
        None if status > 0 => FULL_RAISE + status as f32 * STATUS * LINE,
        None => FULL_RAISE,
    };
    let height = MINI_SURFACE.1 as f32;
    let mini = match (levels.sound, levels.battery) {
        (Some(_), _) => height - (mini_gauge_bottom(levels, height) - MINI_GAUGE),
        (None, Some(_)) => MINI_VALUE * LINE,
        (None, None) => 0.0,
    };
    (full, mini)
}

/// The mini form's sound gauge's bottom: over the battery's percentage;
/// without it, high enough for the gauge's light and ring under it.
fn mini_gauge_bottom(levels: &Snapshot, bottom: f32) -> f32 {
    match levels.battery {
        Some(_) => bottom - MINI_VALUE * LINE - MINI_GAP,
        None => bottom - (MINI_ITEM.height - MINI_GAUGE) / 2.0 - RING_OUT - RING,
    }
}

/// The keyboard's ring around `rect`: 3 px of the accent, 2 px out.
pub(crate) fn ring(f: &mut Frame, rect: Rectangle, radius: f32) {
    let out = RING_OUT + RING / 2.0;
    let path = Path::rounded_rectangle(
        Point::new(rect.x - out, rect.y - out),
        Size::new(rect.width + 2.0 * out, rect.height + 2.0 * out),
        (radius + out).into(),
    );
    f.stroke(
        &path,
        Stroke {
            style: Style::Solid(ACCENT),
            width: RING,
            ..Stroke::default()
        },
    );
}

/// One draw of the levels.
struct Drawn<'a> {
    levels: &'a Snapshot,
    /// The status line's lines.
    status: &'a [String],
    /// The gauges' lengths as drawn now, in percent.
    gauges: [f32; 3],
    words: Words,
    /// The full group's sides, in the surface's coordinates.
    left: f32,
    right: f32,
    bottom: f32,
    zero: f32,
    zero_bold: f32,
}

impl Drawn<'_> {
    fn full(&self, f: &mut Frame, alpha: f32) {
        let mut bottom = self.bottom - FULL_RAISE;
        let levels = self.levels;
        let middles = full_rows(levels, self.status.len(), self.bottom);
        let band = levels.battery.map_or(Band::Ok, |b| b.band());
        let colour = match band {
            Band::Ok => FAINT,
            Band::Low => LOW.1,
            Band::Critical => CRITICAL.1,
        };
        for line in self.status.iter().rev() {
            f.fill_text(canvas::Text {
                content: line.clone(),
                position: Point::new(self.right, bottom - STATUS * LINE / 2.0),
                max_width: f32::INFINITY,
                color: faded(colour, alpha),
                size: Pixels(STATUS),
                line_height: LineHeight::Relative(LINE),
                font: MANROPE,
                align_x: Alignment::Right,
                align_y: alignment::Vertical::Center,
                shaping: Shaping::Advanced,
            });
            bottom -= STATUS * LINE;
        }
        let row = |f: &mut Frame, middle: f32, icon: Icon, gauge: f32, fill: Fill, value: Value| {
            icon.draw(f, Point::new(self.left, middle - ICON / 2.0), alpha);
            let x = self.left + ICON + ICON_GAP;
            let track = Rectangle::new(
                Point::new(x, middle - GAUGE / 2.0),
                Size::new(self.right - VALUE_BOX - ICON_GAP - x, GAUGE),
            );
            bar(f, track, gauge, fill, Direction::Right, alpha);
            value.draw(f, Point::new(self.right, middle), self.zero, alpha);
        };
        if let (Some(battery), Some(middle)) = (levels.battery, middles[0]) {
            let (fill, colour) = match band {
                Band::Ok => (BATTERY, DIM),
                Band::Low => (LOW, LOW.1),
                Band::Critical => (CRITICAL, CRITICAL.1),
            };
            let value = Value::Number(battery.shown(), colour);
            row(
                f,
                middle,
                Icon::Battery {
                    bolt: battery.bolt(),
                },
                self.gauges[2],
                Fill::Gradient(fill),
                value,
            );
        }
        if let (Some(brightness), Some(middle)) = (levels.brightness, middles[1]) {
            let value = Value::Number(brightness.round() as u32, DIM);
            row(
                f,
                middle,
                Icon::Sun,
                self.gauges[1],
                Fill::Gradient(LIGHT),
                value,
            );
        }
        if let (Some(sound), Some(middle)) = (levels.sound, middles[2]) {
            let (fill, value) = if sound.muted {
                (
                    Fill::Flat(MUTED_FILL),
                    Value::Text(self.words.muted(), FAINT),
                )
            } else {
                (
                    Fill::Gradient(SOUND),
                    Value::Number(sound.volume.round() as u32, DIM),
                )
            };
            row(
                f,
                middle,
                Icon::Speaker { muted: sound.muted },
                self.gauges[0],
                fill,
                value,
            );
        }
    }

    fn mini(&self, f: &mut Frame, centre: f32, alpha: f32) {
        let bottom = self.bottom;
        let levels = self.levels;
        if let Some(battery) = levels.battery {
            let colour = match battery.band() {
                Band::Ok => BOLT,
                Band::Low => LOW.1,
                Band::Critical => CRITICAL.1,
            };
            let digits = battery.shown().to_string();
            let bolt = if battery.bolt() {
                (8.0 + BOLT_GAP) * MINI_BOLT
            } else {
                0.0
            };
            let width = bolt + self.zero_bold * digits.len() as f32;
            let mut x = centre - width / 2.0;
            let middle = bottom - MINI_VALUE * LINE / 2.0;
            if battery.bolt() {
                Icon::MiniBolt.draw(f, Point::new(x, middle - 5.0 * MINI_BOLT), alpha);
                x += bolt;
            }
            for ch in digits.chars() {
                cell(
                    f,
                    ch,
                    x + self.zero_bold / 2.0,
                    middle,
                    MINI_VALUE,
                    MANROPE_BOLD,
                    faded(colour, alpha),
                );
                x += self.zero_bold;
            }
        }
        if let Some(sound) = levels.sound {
            let bottom = mini_gauge_bottom(levels, bottom);
            let fill = if sound.muted {
                Fill::Flat(MUTED_FILL)
            } else {
                Fill::Gradient(SOUND)
            };
            let track = Rectangle::new(
                Point::new(centre - GAUGE / 2.0, bottom - MINI_GAUGE),
                Size::new(GAUGE, MINI_GAUGE),
            );
            bar(f, track, self.gauges[0], fill, Direction::Up, alpha);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Fill {
    Gradient((Color, Color)),
    Flat(Color),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Right,
    Up,
}

/// A gauge: its track, rounded, and its fill to `percent`, from the left
/// or from the bottom.
fn bar(f: &mut Frame, track: Rectangle, percent: f32, fill: Fill, way: Direction, alpha: f32) {
    let radius = GAUGE / 2.0;
    f.fill(
        &Path::rounded_rectangle(track.position(), track.size(), radius.into()),
        faded(TRACK, alpha),
    );
    let share = (percent / 100.0).clamp(0.0, 1.0);
    let (rect, start, end) = match way {
        Direction::Right => {
            let w = track.width * share;
            let rect = Rectangle::new(track.position(), Size::new(w, track.height));
            let y = track.center_y();
            (rect, Point::new(track.x, y), Point::new(track.x + w, y))
        }
        Direction::Up => {
            let h = track.height * share;
            let rect = Rectangle::new(
                Point::new(track.x, track.y + track.height - h),
                Size::new(track.width, h),
            );
            let x = track.center_x();
            (rect, Point::new(x, rect.y + h), Point::new(x, rect.y))
        }
    };
    if rect.width < 0.25 || rect.height < 0.25 {
        return;
    }
    // Across, the fill is rounded at both ends, as the mockup's; upward,
    // it is the track's shape cut flat at its level (drawn as that shape:
    // the CPU renderer does not clip a canvas's paths).
    let path = match way {
        Direction::Right => Path::rounded_rectangle(
            rect.position(),
            rect.size(),
            radius.min(rect.width / 2.0).into(),
        ),
        Direction::Up => track_below(track, rect.y),
    };
    match fill {
        Fill::Gradient((from, to)) => {
            let gradient = gradient::Linear::new(start, end)
                .add_stop(0.0, faded(from, alpha))
                .add_stop(1.0, faded(to, alpha));
            f.fill(&path, canvas::Fill::from(gradient));
        }
        Fill::Flat(colour) => f.fill(&path, faded(colour, alpha)),
    }
}

/// The part of an upright track (rounded at its ends, as wide as their
/// diameter) below `top`: its outline, down its left side and up its right.
fn track_below(track: Rectangle, top: f32) -> Path {
    const STEPS: usize = 8;
    let radius = track.width / 2.0;
    let (centre, bottom) = (track.center_x(), track.y + track.height);
    let top = top.clamp(track.y, bottom);
    // The track's half width at `y`.
    let half = |y: f32| {
        let from_end = (y - track.y).min(bottom - y);
        if from_end >= radius {
            radius
        } else {
            let d = radius - from_end;
            (radius * radius - d * d).max(0.0).sqrt()
        }
    };
    // Points close together in the rounded ends, none needed between.
    let span =
        |from: f32, to: f32| (0..=STEPS).map(move |i| from + (to - from) * i as f32 / STEPS as f32);
    let mut ys = vec![top];
    if top < track.y + radius {
        ys.extend(span(top, track.y + radius));
    }
    ys.extend(span(top.max(bottom - radius), bottom));
    Path::new(|p| {
        p.move_to(Point::new(centre - half(top), top));
        for &y in &ys {
            p.line_to(Point::new(centre - half(y), y));
        }
        for &y in ys.iter().rev() {
            p.line_to(Point::new(centre + half(y), y));
        }
        p.close();
    })
}

/// A value at the right of its row.
#[derive(Debug, Clone, Copy)]
enum Value {
    /// Tabular figures: each in a cell as wide as a `0`.
    Number(u32, Color),
    Text(&'static str, Color),
}

impl Value {
    fn draw(self, f: &mut Frame, right_middle: Point, zero: f32, alpha: f32) {
        match self {
            Self::Number(n, colour) => {
                let digits = n.to_string();
                let mut x = right_middle.x - zero * digits.len() as f32;
                for ch in digits.chars() {
                    cell(
                        f,
                        ch,
                        x + zero / 2.0,
                        right_middle.y,
                        VALUE,
                        MANROPE,
                        faded(colour, alpha),
                    );
                    x += zero;
                }
            }
            Self::Text(text, colour) => f.fill_text(canvas::Text {
                content: text.to_owned(),
                position: right_middle,
                max_width: f32::INFINITY,
                color: faded(colour, alpha),
                size: Pixels(VALUE),
                line_height: LineHeight::Relative(LINE),
                font: MANROPE,
                align_x: Alignment::Right,
                align_y: alignment::Vertical::Center,
                shaping: Shaping::Advanced,
            }),
        }
    }
}

/// The status line in lines that fit in `fits` px as `width` measures
/// them: broken between its parts (" · "), never inside one; a part too
/// wide alone keeps its own line.
pub fn lines(text: &str, fits: f32, mut width: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for part in text.split(" · ") {
        match lines.last_mut() {
            Some(line) if width(&format!("{line} · {part}")) <= fits => {
                line.push_str(" · ");
                line.push_str(part);
            }
            _ => lines.push(part.to_owned()),
        }
    }
    lines
}

/// The width of `text` in the status line's font.
fn width(system: &mut iced::advanced::graphics::text::cosmic_text::FontSystem, text: &str) -> f32 {
    use iced::advanced::graphics::text::cosmic_text;
    let mut buffer =
        cosmic_text::Buffer::new(system, cosmic_text::Metrics::new(STATUS, STATUS * LINE));
    let attrs = iced::advanced::graphics::text::to_attributes(MANROPE);
    buffer.set_text(system, text, &attrs, cosmic_text::Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0, f32::max)
}

/// `ch` centred on `x`, its line centred on `y`.
pub(crate) fn cell(f: &mut Frame, ch: char, x: f32, y: f32, size: f32, font: Font, colour: Color) {
    f.fill_text(canvas::Text {
        content: ch.to_string(),
        position: Point::new(x, y),
        max_width: f32::INFINITY,
        color: colour,
        size: Pixels(size),
        line_height: LineHeight::Relative(LINE),
        font,
        align_x: Alignment::Center,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Basic,
    });
}

/// The rows' icons, from the mockup's SVGs (a 24-unit box drawn 18 px).
#[derive(Debug, Clone, Copy)]
enum Icon {
    Speaker {
        muted: bool,
    },
    Sun,
    Battery {
        bolt: bool,
    },
    /// The mini form's bolt, 8 × 10 units.
    MiniBolt,
}

impl Icon {
    fn draw(self, f: &mut Frame, at: Point, alpha: f32) {
        // The SVGs' units, drawn in a box `scale` px a unit.
        let line = |f: &mut Frame, scale: f32, width: f32, colour: Color, points: &[(f32, f32)]| {
            let p = |(x, y): (f32, f32)| Point::new(at.x + x * scale, at.y + y * scale);
            let path = Path::new(|b| {
                if let Some((first, rest)) = points.split_first() {
                    b.move_to(p(*first));
                    for point in rest {
                        b.line_to(p(*point));
                    }
                }
            });
            f.stroke(&path, stroke(width * scale, faded(colour, alpha)));
        };
        let s = ICON / 24.0;
        match self {
            Self::Speaker { muted } => {
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
                line(f, s, 2.2, colour, &cone);
                if muted {
                    line(f, s, 2.2, colour, &[(16.0, 9.0), (21.0, 15.0)]);
                    line(f, s, 2.2, colour, &[(21.0, 9.0), (16.0, 15.0)]);
                } else {
                    // `a5 5 0 0 1 0 7` from (15.5, 8.5): the circle's
                    // centre is 3.57 units left of the chord.
                    let half = 3.5_f32;
                    let cx = 15.5 - (25.0 - half * half).sqrt();
                    let angle = (half / 5.0).asin();
                    let arc = Path::new(|b| {
                        b.arc(canvas::path::Arc {
                            center: Point::new(at.x + cx * s, at.y + 12.0 * s),
                            radius: 5.0 * s,
                            start_angle: Radians(-angle),
                            end_angle: Radians(angle),
                        });
                    });
                    f.stroke(&arc, stroke(2.2 * s, faded(colour, alpha)));
                }
            }
            Self::Sun => {
                let circle = Path::circle(Point::new(at.x + 12.0 * s, at.y + 12.0 * s), 4.0 * s);
                f.stroke(&circle, stroke(2.2 * s, faded(TEXT, alpha)));
                for ray in [
                    [(12.0, 2.0), (12.0, 4.0)],
                    [(12.0, 20.0), (12.0, 22.0)],
                    [(2.0, 12.0), (4.0, 12.0)],
                    [(20.0, 12.0), (22.0, 12.0)],
                    [(4.9, 4.9), (6.3, 6.3)],
                    [(17.7, 17.7), (19.1, 19.1)],
                    [(4.9, 19.1), (6.3, 17.7)],
                    [(17.7, 6.3), (19.1, 4.9)],
                ] {
                    line(f, s, 2.2, TEXT, &ray);
                }
            }
            Self::Battery { bolt } => {
                // A 28 × 16 box, as wide as the others: centred.
                let s = ICON / 28.0;
                let top = at.y + (ICON - 16.0 * s) / 2.0;
                let body = Path::rounded_rectangle(
                    Point::new(at.x + 1.0 * s, top + 2.0 * s),
                    Size::new(22.0 * s, 12.0 * s),
                    (3.0 * s).into(),
                );
                f.stroke(&body, stroke(1.8 * s, faded(TEXT, alpha)));
                let tip = Path::rounded_rectangle(
                    Point::new(at.x + 24.5 * s, top + 6.0 * s),
                    Size::new(2.0 * s, 4.0 * s),
                    (1.0 * s).into(),
                );
                f.fill(&tip, faded(TEXT, alpha));
                if bolt {
                    let at = Point::new(at.x, top);
                    let p = |(x, y): (f32, f32)| Point::new(at.x + x * s, at.y + y * s);
                    let path = Path::new(|b| {
                        b.move_to(p((13.5, 4.2)));
                        b.line_to(p((9.5, 8.6)));
                        b.line_to(p((13.5, 8.6)));
                        b.line_to(p((10.5, 12.2)));
                    });
                    f.stroke(&path, stroke(1.8 * s, faded(BOLT, alpha)));
                }
            }
            Self::MiniBolt => {
                line(
                    f,
                    MINI_BOLT,
                    1.4,
                    BOLT,
                    &[(5.0, 0.8), (1.4, 5.2), (4.4, 5.2), (2.4, 9.2)],
                );
            }
        }
    }
}

pub(crate) fn stroke(width: f32, colour: Color) -> Stroke<'static> {
    Stroke {
        style: Style::Solid(colour),
        width,
        line_cap: canvas::LineCap::Round,
        line_join: canvas::LineJoin::Round,
        ..Stroke::default()
    }
}

pub(crate) fn faded(colour: Color, alpha: f32) -> Color {
    Color {
        a: colour.a * alpha,
        ..colour
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gauge_moves_in_160_ms_and_only_with_motion() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut gauge = Tween::default();
        // First shown: no motion.
        gauge.toward(Some(62.0), t0, true, false);
        assert_eq!(gauge.at(t0), 62.0);
        assert!(!gauge.moving(t0));
        gauge.toward(Some(67.0), t0, true, true);
        assert!(gauge.moving(t0 + ms(100)));
        let mid = gauge.at(t0 + ms(80));
        // Ease-out: past halfway at half the time.
        assert!(mid > 64.5 && mid < 67.0, "{mid}");
        assert_eq!(gauge.at(t0 + ms(160)), 67.0);
        assert!(!gauge.moving(t0 + ms(160)));
        // Without motion, at once; past 100, the gauge is full.
        gauge.toward(Some(120.0), t0, false, true);
        assert_eq!(gauge.at(t0), 100.0);
        // A service gone keeps the gauge where it was.
        gauge.toward(None, t0, true, true);
        assert_eq!(gauge.at(t0), 100.0);
    }

    #[test]
    fn a_wheels_notches_and_a_touchpads_pixels_make_steps() {
        let mut state = State::default();
        let lines = |y| mouse::ScrollDelta::Lines { x: 0.0, y };
        let pixels = |y| mouse::ScrollDelta::Pixels { x: 0.0, y };
        // A wheel: a notch each, its pixels counting for nothing.
        assert_eq!(notches(&mut state, pixels(0.0)), 0);
        assert_eq!(notches(&mut state, lines(1.0)), 1);
        assert!(state.scroll_up);
        assert_eq!(notches(&mut state, pixels(15.0)), 0);
        // At scale 2, a notch is still one.
        assert_eq!(notches(&mut state, lines(-2.0)), -1);
        assert!(!state.scroll_up);
        assert_eq!(notches(&mut state, pixels(-30.0)), 0);
        // A touchpad: 15 px a notch, the rest kept, a turn back starting
        // afresh.
        assert_eq!(notches(&mut state, pixels(10.0)), 0);
        assert_eq!(notches(&mut state, pixels(10.0)), 1);
        assert_eq!(notches(&mut state, pixels(40.0)), 3);
        assert_eq!(notches(&mut state, pixels(-4.0)), 0);
        assert_eq!(notches(&mut state, pixels(-12.0)), -1);
    }

    #[test]
    fn the_sound_row_sits_over_the_others() {
        let levels = Snapshot {
            sound: Some(super::super::model::Sound {
                volume: 62.0,
                muted: false,
            }),
            brightness: Some(70.0),
            battery: None,
            profile: None,
        };
        // No battery, no status line: brightness at the bottom, sound over.
        let [battery, light, sound] = full_rows(&levels, 0, 175.0);
        assert_eq!(battery, None);
        let light = light.expect("brightness");
        let sound = sound.expect("sound");
        assert!((light - (175.0 - FULL_RAISE - ROW / 2.0)).abs() < 0.01);
        assert!((light - sound - ROW - ROW_GAP).abs() < 0.01);
        // Alone in the mini form, the gauge keeps its light and its ring
        // on the surface.
        assert_eq!(mini_gauge_bottom(&levels, 97.0), 97.0 - 8.0 - 5.0);
    }

    #[test]
    fn a_long_status_line_breaks_between_its_parts() {
        // Five pixels a character.
        let width = |t: &str| t.chars().count() as f32 * 5.0;
        assert_eq!(
            lines("3 h 40 restantes · équilibré", 172.0, width),
            ["3 h 40 restantes · équilibré"]
        );
        assert_eq!(
            lines(
                "9 min restantes · branche le chargeur · équilibré",
                172.0,
                width
            ),
            ["9 min restantes", "branche le chargeur · équilibré"]
        );
        assert_eq!(lines("a · b", 1.0, width), ["a", "b"]);
        assert_eq!(lines("économie", 172.0, width), ["économie"]);
    }

    #[test]
    fn an_upright_fill_is_the_tracks_shape_below_its_level() {
        let track = Rectangle::new(Point::new(10.0, 0.0), Size::new(6.0, 40.0));
        for top in [0.0, 1.5, 20.0, 38.0, 40.0] {
            let path = track_below(track, top);
            let points: Vec<_> = path.raw().iter().flat_map(|e| [e.from(), e.to()]).collect();
            assert!(
                points.iter().all(|p| p.y >= top - 0.01 && p.y <= 40.01),
                "{top}"
            );
            assert!(points.iter().all(|p| p.x >= 9.99 && p.x <= 16.01), "{top}");
        }
        // Half way: as wide as the track at its level, both sides, and a
        // point at its end.
        let points: Vec<_> = track_below(track, 20.0)
            .raw()
            .iter()
            .map(|e| e.to())
            .collect();
        for (x, y) in [
            (10.0, 20.0),
            (16.0, 20.0),
            (13.0, 40.0),
            (10.0, 37.0),
            (16.0, 37.0),
        ] {
            assert!(
                points
                    .iter()
                    .any(|p| (p.x - x).abs() < 0.01 && (p.y - y).abs() < 0.01),
                "{x}, {y}"
            );
        }
    }

    #[test]
    fn the_groups_fit_their_surfaces() {
        // Full: three rows and the status line on three lines (a line a
        // part at most: the time, the charger's hint, the profile), above
        // the footer's place, and the sound's row lit and ringed.
        let full = 3.0 * ROW + 3.0 * STATUS * LINE + 3.0 * ROW_GAP + FULL_RAISE;
        let lit = (ITEM_HEIGHT - ROW) / 2.0 + RING_OUT + RING;
        assert!(full + lit <= FULL_SURFACE.1 as f32, "{full} + {lit}");
        // Mini: the gauge, and the battery under it, the gauge lit and
        // ringed, as wide as the surface at most.
        let mini = MINI_GAUGE + MINI_GAP + MINI_VALUE * LINE;
        let lit = (MINI_ITEM.height - MINI_GAUGE) / 2.0 + RING_OUT + RING;
        assert!(mini + lit <= MINI_SURFACE.1 as f32, "{mini} + {lit}");
        assert!(MINI_ITEM.width + 2.0 * (RING_OUT + RING) <= MINI_SURFACE.0 as f32);
        // The full group's bottom is 61 px above the bar's: 12 px of
        // padding, the footer's 34 px and 14 px between, and the rim.
        assert_eq!(BOTTOM as f32 + FULL_RAISE, 1.0 + 12.0 + 34.0 + 14.0);
        // The mini's, 41 px: 10 px of padding, 18 px and 12 px between (the
        // place of what comes under the battery, as the 3.3 mockup has it).
        assert_eq!(BOTTOM, 1 + 10 + 18 + 12);
    }
}
