//! The clock at the top of the bar (bar.md §4 group 1, §5), drawn on a
//! surface of its own over the bar's top.
//!
//! It wakes itself: a canvas that, each time it is drawn, asks iced to draw
//! it again just past the next wall-clock second, or at the next frame
//! while its digits move. iced draws again only the surface that asked, so
//! at rest the shell draws this small surface once a second and nothing
//! else; the rest of the shell hears nothing of it.
//!
//! Every character sits in a cell as wide as the font's `0` (the colon
//! 0.55 of it), centred, so that nothing shifts as digits change. On a
//! minute change the changed digits move ([`motion`]) inside their cells;
//! the seconds change without motion. With reduced motion, or on the CPU,
//! nothing moves.

pub mod motion;
pub mod time;

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use hocus_link::{Form, Transition};
use iced::{
    Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector,
    advanced::graphics::text::cosmic_text,
    alignment, font, mouse,
    widget::{
        canvas::{self, Geometry},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use self::{
    motion::{Pivot, Pose},
    time::{Lang, Shown, Wall},
};

/// The bundled fonts (SIL Open Font License 1.1, in `fonts/`): Martian
/// Mono in the weights the clock uses, Manrope for the date.
pub const FONTS: [&[u8]; 3] = [
    include_bytes!("../../fonts/MartianMono-Regular.ttf"),
    include_bytes!("../../fonts/MartianMono-Light.ttf"),
    include_bytes!("../../fonts/Manrope-Regular.ttf"),
];

const MANROPE: Font = Font::with_name("Manrope");

/// `text.dim` and `text.faint` (bar.md §13): the date and the seconds.
const DIM: Color = Color::from_rgb8(0xA9, 0xB2, 0xC2);
const FAINT: Color = Color::from_rgb8(0x8C, 0x96, 0xA8);

/// The clock's surface starts this far below the bar's top: the mini
/// form's padding (§5). The full form's 14 px are 2 px further, inside.
pub const SURFACE_TOP: i32 = 12;
/// The surface's sizes: the bar's width less its 1 px rim on each side,
/// and as tall as the form's clock.
pub const FULL_SURFACE: (u32, u32) = (194, 62);
pub const MINI_SURFACE: (u32, u32) = (54, 32);

/// Full form: `HH:MM` at 34 px, line height 1, in cells 1.05 em high; the
/// seconds at 15 px, 4 px after, on the same baseline; the date 6 px below,
/// Manrope 13 px, its line as CSS's `normal`.
const FULL_TOP: f32 = 2.0;
const BIG: f32 = 34.0;
const SECONDS: f32 = 15.0;
const SECONDS_GAP: f32 = 4.0;
const DATE: f32 = 13.0;
const DATE_GAP: f32 = 6.0;
const DATE_LINE: f32 = 1.366;
/// Mini form: hours over minutes, 15 px, line height 1.1.
const MINI: f32 = 15.0;
const MINI_LINE: f32 = 1.1;
/// The blur's copies, in half deviations.
const BLUR_STEPS: [f32; 9] = [-4.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0];
/// A cell's height, in ems.
const CELL_HEIGHT: f32 = 1.05;
/// The colon's cell, in the width of a `0`.
const COLON: f32 = 0.55;

/// How the clock looks, as configured, once its font is found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub font: Font,
    pub color: Color,
    pub transition: Transition,
    pub seconds: bool,
}

impl Style {
    pub fn light(&self) -> Font {
        Font {
            weight: font::Weight::Light,
            ..self.font
        }
    }
}

/// The clock of one bar, as the shell sees it at its last update.
#[derive(Debug, Clone)]
pub struct Clock {
    pub style: Style,
    /// The digits may move (motion and a GPU).
    pub motion: bool,
    pub wall: Wall,
    pub lang: Lang,
    /// The forms to draw, with their opacity: one at rest, two while the
    /// bar switches (a cross-fade).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn now (it moves while it switches), and the
    /// surface's: the bar's right edge is 1 px right of the surface's.
    pub bar_width: f32,
    pub surface_width: f32,
    /// Without glass: the bar's opaque colour behind the clock (its
    /// surface has no alpha on the CPU).
    pub backdrop: Option<Color>,
}

/// The clock's state between draws.
#[derive(Debug, Default)]
pub struct State {
    shown: Option<Shown>,
    motion: Moving,
    /// When it was last updated to be drawn.
    at: Option<Instant>,
    /// Font measures by font and size.
    measures: HashMap<(Font, u32, u32), Measure>,
}

/// Whether the digits move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Moving {
    #[default]
    Still,
    /// From `from`'s digits, since then.
    Since { from: [char; 5], since: Instant },
}

impl Moving {
    /// The motion after showing `next` at `now`, `before` having been
    /// shown: a new one when the minute changed, however far the clock
    /// went (a jump or a resume move once, never once per minute missed).
    pub fn after(
        self,
        before: Option<&Shown>,
        next: &Shown,
        now: Instant,
        transition: Option<Transition>,
    ) -> Self {
        let Some(transition) = transition.filter(|t| *t != Transition::None) else {
            return Self::Still;
        };
        match before {
            Some(before) if before.hm != next.hm => Self::Since {
                from: before.hm,
                since: now,
            },
            _ => match self {
                Self::Since { since, .. }
                    if now.saturating_duration_since(since) >= motion::duration(transition) =>
                {
                    Self::Still
                }
                other => other,
            },
        }
    }
}

/// A font's `0` advance and its baseline, at one size and line height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measure {
    pub zero: f32,
    pub baseline: f32,
}

/// Measures `font` at `size` in lines `line` high, as iced lays text out.
pub fn measure(
    system: &mut cosmic_text::FontSystem,
    font: Font,
    size: f32,
    line: f32,
) -> Option<Measure> {
    let mut buffer = cosmic_text::Buffer::new(system, cosmic_text::Metrics::new(size, line));
    let attrs = iced::advanced::graphics::text::to_attributes(font);
    buffer.set_text(system, "0", &attrs, cosmic_text::Shaping::Basic, None);
    buffer.shape_until_scroll(system, false);
    let run = buffer.layout_runs().next()?;
    Some(Measure {
        zero: run.line_w,
        baseline: run.line_y - run.line_top,
    })
}

impl State {
    fn measure(&self, font: Font, size: f32, line: f32) -> Measure {
        self.measures
            .get(&(font, size.to_bits(), line.to_bits()))
            .copied()
            .unwrap_or(Measure {
                zero: size * 0.6,
                baseline: line * 0.8,
            })
    }

    fn remember(&mut self, font: Font, size: f32, line: f32) {
        let key = (font, size.to_bits(), line.to_bits());
        if self.measures.contains_key(&key) {
            return;
        }
        let Ok(mut system) = iced::advanced::graphics::text::font_system().write() else {
            return;
        };
        if let Some(m) = measure(system.raw(), font, size, line) {
            self.measures.insert(key, m);
        }
    }
}

impl<Message> canvas::Program<Message> for Clock {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let now = *now;
        let time = self.wall.now();
        let shown = Shown::at(&time, self.lang);
        let transition = self.motion.then_some(self.style.transition);
        state.motion = state
            .motion
            .after(state.shown.as_ref(), &shown, now, transition);
        state.shown = Some(shown);
        state.at = Some(now);
        for (font, size, line) in [
            (self.style.font, BIG, BIG),
            (self.style.light(), SECONDS, SECONDS),
            (self.style.font, MINI, MINI * MINI_LINE),
        ] {
            state.remember(font, size, line);
        }
        Some(match state.motion {
            Moving::Since { .. } => canvas::Action::request_redraw(),
            Moving::Still => canvas::Action::request_redraw_at(
                Instant::now() + time::until_next_second(time::subsec(&time)),
            ),
        })
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Some(colour) = self.backdrop {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), colour);
        }
        let Some(shown) = &state.shown else {
            return vec![frame.into_geometry()];
        };
        // The bar's inside, less its rim, in the surface's coordinates.
        let right = self.surface_width;
        let left = right + 2.0 - self.bar_width;
        let column = Rectangle::new(
            Point::new(left.max(0.0), 0.0),
            Size::new((right - left.max(0.0)).max(0.0), bounds.height),
        );
        let centre = (left + right) / 2.0;
        let moving = match state.motion {
            Moving::Since { from, since } => {
                let elapsed = state
                    .at
                    .map_or(Duration::ZERO, |at| at.saturating_duration_since(since));
                Some((from, elapsed))
            }
            Moving::Still => None,
        };
        let mut draw = Draw {
            frame: &mut frame,
            column,
            transition: self.style.transition,
            moving,
        };
        for &(form, alpha) in &self.forms {
            match form {
                Form::Full => self.draw_full(&mut draw, state, shown, centre, alpha),
                Form::Mini => self.draw_mini(&mut draw, state, shown, centre, alpha),
            }
        }
        vec![frame.into_geometry()]
    }
}

impl Clock {
    fn draw_full(
        &self,
        draw: &mut Draw<'_>,
        state: &State,
        shown: &Shown,
        centre: f32,
        alpha: f32,
    ) {
        let big = state.measure(self.style.font, BIG, BIG);
        let small = state.measure(self.style.light(), SECONDS, SECONDS);
        let cells = cells(&shown.hm, big.zero);
        let mut width: f32 = cells.iter().sum();
        if self.style.seconds {
            width += SECONDS_GAP + 2.0 * small.zero;
        }
        let mut x = centre - width / 2.0;
        let colour = faded(self.style.color, alpha);
        for (i, (&ch, &w)) in shown.hm.iter().zip(&cells).enumerate() {
            let cell = Rectangle::new(Point::new(x, FULL_TOP), Size::new(w, BIG * CELL_HEIGHT));
            let glyph = Glyph {
                font: self.style.font,
                size: BIG,
                line: BIG,
                colour,
            };
            draw.digit(cell, glyph, ch, i);
            x += w;
        }
        if self.style.seconds {
            x += SECONDS_GAP;
            let top = FULL_TOP + big.baseline - small.baseline;
            for ch in shown.ss {
                let cell = Rectangle::new(Point::new(x, top), Size::new(small.zero, SECONDS));
                let glyph = Glyph {
                    font: self.style.light(),
                    size: SECONDS,
                    line: SECONDS,
                    colour: faded(FAINT, alpha),
                };
                draw.glyph(cell, glyph, ch, Pose::REST);
                x += small.zero;
            }
        }
        let top = FULL_TOP + BIG * CELL_HEIGHT + DATE_GAP;
        let clip = draw.column;
        draw.frame.with_clip(clip, |f| {
            f.fill_text(canvas::Text {
                content: shown.date.clone(),
                position: Point::new(centre, top),
                max_width: f32::INFINITY,
                color: faded(DIM, alpha),
                size: Pixels(DATE),
                line_height: LineHeight::Relative(DATE_LINE),
                font: MANROPE,
                align_x: Alignment::Center,
                align_y: alignment::Vertical::Top,
                shaping: Shaping::Advanced,
            });
        });
    }

    fn draw_mini(
        &self,
        draw: &mut Draw<'_>,
        state: &State,
        shown: &Shown,
        centre: f32,
        alpha: f32,
    ) {
        let line = MINI * MINI_LINE;
        let m = state.measure(self.style.font, MINI, line);
        let height = MINI * CELL_HEIGHT;
        let glyph = Glyph {
            font: self.style.font,
            size: MINI,
            line,
            colour: faded(self.style.color, alpha),
        };
        // Hours over minutes: the digits at 0, 1 and 3, 4 of `HH:MM`.
        for (row, at) in [[0, 1], [3, 4]].into_iter().enumerate() {
            let top = row as f32 * height;
            for (col, i) in at.into_iter().enumerate() {
                let x = centre - m.zero + col as f32 * m.zero;
                let cell = Rectangle::new(Point::new(x, top), Size::new(m.zero, height));
                draw.digit(cell, glyph, shown.hm[i], i);
            }
        }
    }
}

/// The cells' widths of `HH:MM`.
pub fn cells(hm: &[char; 5], zero: f32) -> [f32; 5] {
    hm.map(|ch| if ch == ':' { COLON * zero } else { zero })
}

fn faded(colour: Color, alpha: f32) -> Color {
    Color {
        a: colour.a * alpha,
        ..colour
    }
}

/// A glyph's font, size, line and colour.
#[derive(Debug, Clone, Copy)]
struct Glyph {
    font: Font,
    size: f32,
    line: f32,
    colour: Color,
}

/// One draw of the clock.
struct Draw<'a> {
    frame: &'a mut canvas::Frame,
    /// Nothing is drawn outside the bar.
    column: Rectangle,
    transition: Transition,
    /// The digits shown before the minute changed, and the time since.
    moving: Option<([char; 5], Duration)>,
}

impl Draw<'_> {
    /// Character `i` of `HH:MM`, `ch` now: moving if it changed.
    fn digit(&mut self, cell: Rectangle, glyph: Glyph, ch: char, i: usize) {
        let changed = self
            .moving
            .map(|(from, elapsed)| (from[i], elapsed))
            .filter(|(old, _)| *old != ch);
        match changed {
            Some((old, elapsed)) => {
                if let Some(pose) = motion::outgoing(self.transition, elapsed) {
                    self.glyph(cell, glyph, old, pose);
                }
                self.glyph(cell, glyph, ch, motion::incoming(self.transition, elapsed));
            }
            None => self.glyph(cell, glyph, ch, Pose::REST),
        }
    }

    /// `ch` in `cell`, centred, at `pose`, clipped to the cell and the bar.
    fn glyph(&mut self, cell: Rectangle, glyph: Glyph, ch: char, pose: Pose) {
        let Some(clip) = cell.intersection(&self.column) else {
            return;
        };
        if pose.alpha <= 0.0 {
            return;
        }
        let x = cell.x + cell.width / 2.0;
        let y = cell.y + pose.dy * glyph.line;
        // A vertical blur (CSS's `blur()`, a standard deviation), as copies
        // spread over two deviations each way and weighed as a Gaussian:
        // where they all cover, they are together as opaque as the glyph.
        let opacity = pose.alpha.min(1.0);
        let copies: Vec<(f32, f32)> = if pose.blur < 0.25 {
            vec![(0.0, opacity)]
        } else {
            let weights = BLUR_STEPS.map(|k| (-k * k / 8.0).exp());
            let total: f32 = weights.iter().sum();
            // Opaque copies would each cover in full: no fading edge.
            let opacity = opacity.min(0.97);
            BLUR_STEPS
                .iter()
                .zip(weights)
                .map(|(k, w)| (k * pose.blur / 2.0, 1.0 - (1.0 - opacity).powf(w / total)))
                .collect()
        };
        self.frame.with_clip(clip, |f| {
            if pose.fold != 1.0 {
                let pivot = match pose.pivot {
                    Pivot::Top => y,
                    Pivot::Bottom => y + glyph.line,
                };
                f.translate(Vector::new(0.0, pivot));
                f.scale_nonuniform(Vector::new(1.0, pose.fold));
                f.translate(Vector::new(0.0, -pivot));
            }
            for (dy, alpha) in copies {
                f.fill_text(canvas::Text {
                    content: ch.to_string(),
                    position: Point::new(x, y + dy),
                    max_width: f32::INFINITY,
                    color: faded(glyph.colour, alpha),
                    size: Pixels(glyph.size),
                    line_height: LineHeight::Absolute(Pixels(glyph.line)),
                    font: glyph.font,
                    align_x: Alignment::Center,
                    align_y: alignment::Vertical::Top,
                    shaping: Shaping::Basic,
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(hm: &str) -> Shown {
        let c: Vec<char> = hm.chars().collect();
        Shown {
            hm: [c[0], c[1], c[2], c[3], c[4]],
            ss: ['0', '0'],
            date: String::new(),
        }
    }

    #[test]
    fn a_minute_change_moves_once_then_rests() {
        let t0 = Instant::now();
        let odo = Some(Transition::Odometer);
        let a = shown("09:59");
        let b = shown("10:00");
        let still = Moving::Still.after(Some(&a), &a, t0, odo);
        assert_eq!(still, Moving::Still);
        let moving = still.after(Some(&a), &b, t0, odo);
        assert_eq!(
            moving,
            Moving::Since {
                from: a.hm,
                since: t0
            }
        );
        // Still moving within its 520 ms, at rest after.
        let ms = Duration::from_millis;
        assert_eq!(moving.after(Some(&b), &b, t0 + ms(300), odo), moving);
        assert_eq!(moving.after(Some(&b), &b, t0 + ms(520), odo), Moving::Still);
        // A jump of hours moves once, from what was shown.
        let later = shown("13:27");
        let jump = Moving::Still.after(Some(&b), &later, t0, odo);
        assert_eq!(
            jump,
            Moving::Since {
                from: b.hm,
                since: t0
            }
        );
        // The first look, reduced motion, or no transition: no motion.
        assert_eq!(Moving::Still.after(None, &b, t0, odo), Moving::Still);
        assert_eq!(Moving::Still.after(Some(&a), &b, t0, None), Moving::Still);
        assert_eq!(
            moving.after(Some(&a), &b, t0, Some(Transition::None)),
            Moving::Still
        );
    }

    /// The bundled fonts, alone, as iced lays them out.
    fn bundled() -> cosmic_text::FontSystem {
        let mut db = cosmic_text::fontdb::Database::new();
        for font in FONTS {
            db.load_font_data(font.to_vec());
        }
        cosmic_text::FontSystem::new_with_locale_and_db("en".into(), db)
    }

    #[test]
    fn every_digit_is_as_wide_as_its_cell() {
        let mut system = bundled();
        let martian = Font::with_name("Martian Mono");
        let light = Font {
            weight: font::Weight::Light,
            ..martian
        };
        for (font, size) in [(martian, BIG), (light, SECONDS), (martian, MINI)] {
            let zero = measure(&mut system, font, size, size)
                .expect("a measure")
                .zero;
            assert!(zero > size * 0.5, "{zero} at {size}");
            for digit in '0'..='9' {
                let mut buffer =
                    cosmic_text::Buffer::new(&mut system, cosmic_text::Metrics::new(size, size));
                let attrs = iced::advanced::graphics::text::to_attributes(font);
                buffer.set_text(
                    &mut system,
                    &digit.to_string(),
                    &attrs,
                    cosmic_text::Shaping::Basic,
                    None,
                );
                buffer.shape_until_scroll(&mut system, false);
                let w = buffer.layout_runs().next().map(|r| r.line_w);
                assert_eq!(w, Some(zero), "{digit} at {size}");
            }
        }
        // The cells do not depend on the digits.
        assert_eq!(
            cells(&['0', '0', ':', '0', '0'], 20.0),
            cells(&['2', '3', ':', '5', '9'], 20.0)
        );
        assert_eq!(cells(&['1', '1', ':', '1', '1'], 20.0)[2], 11.0);
    }

    #[test]
    fn the_full_clock_fits_in_the_bar() {
        let mut system = bundled();
        let martian = Font::with_name("Martian Mono");
        let light = Font {
            weight: font::Weight::Light,
            ..martian
        };
        let big = measure(&mut system, martian, BIG, BIG).expect("a measure");
        let small = measure(&mut system, light, SECONDS, SECONDS).expect("a measure");
        let width: f32 = cells(&['2', '3', ':', '5', '9'], big.zero)
            .iter()
            .sum::<f32>()
            + SECONDS_GAP
            + 2.0 * small.zero;
        // 196 px less 12 px of padding on each side.
        assert!(width <= 172.0, "{width}");
        // The seconds' baseline sits on the minutes'.
        assert!(big.baseline > small.baseline);
        let mini = measure(&mut system, martian, MINI, MINI * MINI_LINE).expect("a measure");
        assert!(2.0 * mini.zero <= 56.0 - 2.0 * 8.0, "{}", mini.zero);
        // The surfaces hold their form's clock.
        let full = FULL_TOP + BIG * CELL_HEIGHT + DATE_GAP + DATE * DATE_LINE;
        assert!(full <= FULL_SURFACE.1 as f32, "{full}");
        assert!(2.0 * MINI * CELL_HEIGHT <= MINI_SURFACE.1 as f32);
    }
}
