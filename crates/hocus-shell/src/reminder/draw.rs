//! A reminder's tab (Day.6, variant B), as the mockup draws it.
//!
//! 340 × 44, radius 12, the bubbles' tint (`rgba(22,27,42,0.96) →
//! rgba(14,17,28,0.97)`) in a 1 px `rgba(143,179,255,0.55)` rim; on a GPU
//! its shadow `0 10px 30px rgba(0,0,0,0.40)` and a glow `0 0 20px
//! rgba(143,179,255,0.22)` around it, on the CPU the tab alone (the
//! compositor cuts its corners). 12 px in on the left and 6 on the right:
//! its title (13 px, 700, one line) over its line (11.5 px `#A9C4FF`:
//! "10:00 · dans 5 min · visio"), then its buttons, 30 px high, radius 8,
//! 6 px apart: Rejoindre (a 14 px camera, 12.5 px 700 white on `#3F6FD8`,
//! `#4A7BE6` under the pointer) or Voir (`rgba(255,255,255,0.06)` in a
//! 1 px `rgba(255,255,255,0.14)` rim), Later (a 30 × 30 clock, until the
//! start) and ✕ (30 × 30, `text.faint`). The icon buttons are lit `0.10`
//! under the pointer, and say their names after 500 ms. A click on the
//! text acts as Voir.
//!
//! The compositor slides it in and out ([`hocus_link::slides_in`]); here it
//! draws when what it shows changes and under the pointer, never at rest.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke},
        text::Alignment,
    },
    window,
};

use super::{Act, Seen, Tab, Words};
use crate::{
    Message,
    levels::draw::{FAINT, LINE, MANROPE_BOLD, TEXT, pointer, rgb},
    popover::kit,
};

const BORDER: f32 = 1.0;
const RADIUS: f32 = 12.0;
const PAD_LEFT: f32 = 12.0;
const PAD_RIGHT: f32 = 6.0;
const GAP: f32 = 6.0;
const TITLE: f32 = 13.0;
const LINE_TEXT: f32 = 11.5;
const LINES_GAP: f32 = 1.0;
const BUTTON: f32 = 30.0;
const BUTTON_RADIUS: f32 = 8.0;
const BUTTON_PAD: f32 = 12.0;
const BUTTON_TEXT: f32 = 12.5;
const CAMERA: f32 = 14.0;
const CAMERA_GAP: f32 = 6.0;
const CLOCK: f32 = 16.0;
const CROSS: f32 = 14.0;
/// The icon buttons' names show after this long under the pointer.
pub const TIP_DELAY: Duration = Duration::from_millis(500);
/// Under the tab, the tooltip's top.
const TIP_GAP: f32 = 6.0;

const GLASS_TOP: Color = Color::from_rgba8(22, 27, 42, 0.96);
const GLASS_BOTTOM: Color = Color::from_rgba8(14, 17, 28, 0.97);
const OPAQUE_TOP: Color = rgb(0x161B2A);
const OPAQUE_BOTTOM: Color = rgb(0x0E111C);
const RIM: Color = Color::from_rgba8(143, 179, 255, 0.55);
pub const LINE_BLUE: Color = rgb(0xA9C4FF);
pub const JOIN: Color = rgb(0x3F6FD8);
pub const JOIN_HOVER: Color = rgb(0x4A7BE6);
const OPEN_FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const OPEN_FILL_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.12);
const OPEN_RIM: Color = Color::from_rgba8(255, 255, 255, 0.14);
pub const LATER_FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const ICON_LIT: Color = Color::from_rgba8(255, 255, 255, 0.10);
const CLOCK_INK: Color = rgb(0xC9D0DC);
/// The shadow and the glow: offset, deviation, alpha.
const SHADOW: (f32, f32, f32) = (10.0, 15.0, 0.40);
const GLOW: (f32, f32, f32) = (0.0, 10.0, 0.22);

/// A part of the tab that takes the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Text,
    Main,
    Later,
    Dismiss,
}

impl Part {
    fn act(self, join: bool) -> Act {
        match self {
            Self::Main if join => Act::Join,
            Self::Text | Self::Main => Act::Open,
            Self::Later => Act::Later,
            Self::Dismiss => Act::Dismiss,
        }
    }
}

/// Where the parts are, in the tab's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub main: Rectangle,
    /// Rejoindre (a link), else Voir.
    pub join: bool,
    pub label: &'static str,
    pub later: Option<Rectangle>,
    pub dismiss: Rectangle,
    /// The text's room, from its left.
    pub text: Rectangle,
}

/// The tab's parts for `tab`.
pub fn lay(tab: &Tab, words: Words) -> Laid {
    let middle = HEIGHT / 2.0;
    let square = |right: f32| {
        Rectangle::new(
            Point::new(right - BUTTON, middle - BUTTON / 2.0),
            Size::new(BUTTON, BUTTON),
        )
    };
    let dismiss = square(super::WIDTH - BORDER - PAD_RIGHT);
    let later = (tab.left > 0).then(|| square(dismiss.x - GAP));
    let join = tab.meeting.video.is_some();
    let label = if join { words.join() } else { words.open() };
    let mut width = 2.0 * BUTTON_PAD + kit::measure(label, BUTTON_TEXT, MANROPE_BOLD);
    if join {
        width += CAMERA + CAMERA_GAP;
    }
    let right = later.map_or(dismiss.x, |l| l.x) - GAP;
    let main = Rectangle::new(
        Point::new(right - width, middle - BUTTON / 2.0),
        Size::new(width, BUTTON),
    );
    let left = BORDER + PAD_LEFT;
    let text = Rectangle::new(
        Point::new(left, BORDER),
        Size::new((main.x - GAP - left).max(0.0), HEIGHT - 2.0 * BORDER),
    );
    Laid {
        main,
        join,
        label,
        later,
        dismiss,
        text,
    }
}

const HEIGHT: f32 = super::HEIGHT;

/// The part at `at` (the tab's frame).
fn part(laid: &Laid, at: Point) -> Option<Part> {
    let inside = Rectangle::new(Point::ORIGIN, Size::new(super::WIDTH, HEIGHT));
    if !inside.contains(at) {
        return None;
    }
    if laid.dismiss.contains(at) {
        Some(Part::Dismiss)
    } else if laid.later.is_some_and(|l| l.contains(at)) {
        Some(Part::Later)
    } else if laid.main.contains(at) {
        Some(Part::Main)
    } else {
        Some(Part::Text)
    }
}

/// A tab's surface.
#[derive(Debug, Clone)]
pub struct View {
    pub id: window::Id,
    /// The compositor blurs behind the shell (else the opaque colours).
    pub glass: bool,
    pub words: Words,
}

#[derive(Debug, Default)]
pub struct State {
    at: Option<Point>,
    over: Option<Part>,
}

/// What each surface last said it shows at rest, for the log.
static SAID: Mutex<Option<HashMap<window::Id, String>>> = Mutex::new(None);

impl canvas::Program<Message> for View {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Mouse(seen) = event else {
            return None;
        };
        let shown = super::seen(self.id)?;
        let laid = lay(&shown.tab, self.words);
        let at = pointer(seen, bounds, cursor, state.at);
        state.at = at;
        let over = at
            .map(|p| Point::new(p.x - shown.at.x, p.y - shown.at.y))
            .and_then(|p| part(&laid, p));
        match seen {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let over = over?;
                crate::tray::tip::leave();
                tracing::debug!(serial = shown.tab.serial, ?over, "a click on the reminder");
                super::act(shown.tab.serial, over.act(laid.join));
                Some(canvas::Action::capture())
            }
            mouse::Event::CursorEntered
            | mouse::Event::CursorMoved { .. }
            | mouse::Event::CursorLeft => {
                if over == state.over {
                    return None;
                }
                state.over = over;
                self.tip(&shown, &laid, over);
                Some(canvas::Action::request_redraw())
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
        let mut f = Frame::new(renderer, bounds.size());
        let Some(shown) = super::seen(self.id) else {
            return vec![f.into_geometry()];
        };
        let laid = lay(&shown.tab, self.words);
        if !shown.card {
            f.draw_svg(
                Rectangle::new(Point::ORIGIN, bounds.size()),
                svg::Svg::new(shadow(bounds.size(), shown.at)),
            );
        }
        f.with_save(|f| {
            f.translate(iced::Vector::new(shown.at.x, shown.at.y));
            draw_tab(
                f,
                &shown,
                &laid,
                self.words,
                state.over,
                self.glass && !shown.card,
            );
        });
        let said = format!(
            "bar={} «{}» {} main={} later={} lit={:?}",
            shown.bar,
            self.words.title(&shown.tab, shown.shared),
            self.words.line(&shown.tab),
            laid.label,
            laid.later
                .map(|_| self.words.later(shown.tab.left))
                .unwrap_or("none"),
            state.over,
        );
        let mut kept = SAID.lock().unwrap_or_else(PoisonError::into_inner);
        let kept = kept.get_or_insert_with(HashMap::new);
        if kept.get(&self.id) != Some(&said) {
            tracing::debug!(
                said = %said,
                serial = shown.tab.serial,
                name = %self.words.name(&shown.tab, shown.shared),
                "the reminder is drawn at rest"
            );
            kept.insert(self.id, said);
        }
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.over.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl View {
    /// The icon button under the pointer says its name after 500 ms, under
    /// the tab; elsewhere, no tooltip.
    fn tip(&self, shown: &Seen, laid: &Laid, over: Option<Part>) {
        let button = match over {
            Some(Part::Later) => laid.later.map(|r| (r, self.words.later(shown.tab.left))),
            Some(Part::Dismiss) => Some((laid.dismiss, self.words.dismiss())),
            _ => None,
        };
        let Some((rect, name)) = button else {
            crate::tray::tip::leave();
            return;
        };
        let (top, right) = (shown.margin.0 as f32, shown.margin.1 as f32);
        let surface_width = shown.at.x + super::WIDTH + if shown.card { 0.0 } else { 8.0 };
        let centre = right + surface_width - (shown.at.x + rect.center_x());
        crate::tray::tip::hover_under(
            &shown.bar,
            top + shown.at.y + HEIGHT + TIP_GAP,
            centre,
            TIP_DELAY,
            name.to_owned(),
        );
    }
}

/// The tab, in its frame.
fn draw_tab(
    f: &mut Frame,
    shown: &Seen,
    laid: &Laid,
    words: Words,
    over: Option<Part>,
    glass: bool,
) {
    let size = Size::new(super::WIDTH, HEIGHT);
    let (top, bottom) = if glass {
        (GLASS_TOP, GLASS_BOTTOM)
    } else {
        (OPAQUE_TOP, OPAQUE_BOTTOM)
    };
    let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
        .add_stop(0.0, top)
        .add_stop(1.0, bottom);
    f.fill(
        &Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into()),
        canvas::Fill::from(tint),
    );
    f.stroke(
        &Path::rounded_rectangle(
            Point::new(BORDER / 2.0, BORDER / 2.0),
            Size::new(size.width - BORDER, size.height - BORDER),
            (RADIUS - BORDER / 2.0).into(),
        ),
        Stroke::default().with_color(RIM).with_width(BORDER),
    );
    // The title over the line, 1 px apart, centred.
    let (title_h, line_h) = (TITLE * LINE, LINE_TEXT * LINE);
    let y = (HEIGHT - title_h - LINES_GAP - line_h) / 2.0;
    let room = laid.text.width;
    let title = kit::elide(&words.title(&shown.tab, shown.shared), room, |t| {
        kit::measure(t, TITLE, MANROPE_BOLD)
    });
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            title,
            Point::new(laid.text.x, y + title_h / 2.0),
            TITLE,
            TEXT,
            Alignment::Left,
        ),
    ));
    let line = kit::elide(&words.line(&shown.tab), room, |t| {
        kit::measure(t, LINE_TEXT, crate::levels::draw::MANROPE)
    });
    f.fill_text(kit::text(
        line,
        Point::new(laid.text.x, y + title_h + LINES_GAP + line_h / 2.0),
        LINE_TEXT,
        LINE_BLUE,
        Alignment::Left,
    ));
    // The main button.
    let lit = |part| over == Some(part);
    let main = kit::rounded(laid.main, BUTTON_RADIUS);
    let mut x = laid.main.x + BUTTON_PAD;
    if laid.join {
        f.fill(&main, if lit(Part::Main) { JOIN_HOVER } else { JOIN });
        crate::meeting::draw::camera(
            f,
            Rectangle::new(
                Point::new(x, laid.main.center_y() - CAMERA / 2.0),
                Size::new(CAMERA, CAMERA),
            ),
            Color::WHITE,
        );
        x += CAMERA + CAMERA_GAP;
    } else {
        f.fill(
            &main,
            if lit(Part::Main) {
                OPEN_FILL_HOVER
            } else {
                OPEN_FILL
            },
        );
        f.stroke(
            &kit::rounded(
                Rectangle::new(
                    Point::new(laid.main.x + BORDER / 2.0, laid.main.y + BORDER / 2.0),
                    Size::new(laid.main.width - BORDER, laid.main.height - BORDER),
                ),
                BUTTON_RADIUS - BORDER / 2.0,
            ),
            Stroke::default().with_color(OPEN_RIM).with_width(BORDER),
        );
    }
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            laid.label.to_owned(),
            Point::new(x, laid.main.center_y()),
            BUTTON_TEXT,
            if laid.join { Color::WHITE } else { TEXT },
            Alignment::Left,
        ),
    ));
    // Later: a clock.
    if let Some(later) = laid.later {
        let fill = if lit(Part::Later) {
            ICON_LIT
        } else {
            LATER_FILL
        };
        f.fill(&kit::rounded(later, BUTTON_RADIUS), fill);
        clock(
            f,
            centred(later, CLOCK),
            if lit(Part::Later) { TEXT } else { CLOCK_INK },
        );
    }
    // ✕.
    if lit(Part::Dismiss) {
        f.fill(&kit::rounded(laid.dismiss, BUTTON_RADIUS), ICON_LIT);
    }
    cross(
        f,
        centred(laid.dismiss, CROSS),
        if lit(Part::Dismiss) { TEXT } else { FAINT },
    );
}

/// A square of `side` centred in `rect`.
fn centred(rect: Rectangle, side: f32) -> Rectangle {
    Rectangle::new(
        Point::new(rect.center_x() - side / 2.0, rect.center_y() - side / 2.0),
        Size::new(side, side),
    )
}

fn stroke(colour: Color, width: f32) -> Stroke<'static> {
    Stroke::default()
        .with_color(colour)
        .with_width(width)
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round)
}

/// The mockup's clock (24 px drawing, stroked 2 px at 24) in `rect`.
fn clock(f: &mut Frame, rect: Rectangle, colour: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| {
        b.move_to(p(12.0, 7.5));
        b.line_to(p(12.0, 12.0));
        b.line_to(p(15.0, 14.0));
        b.circle(p(12.0, 12.0), 8.5 * k);
    });
    f.stroke(&path, stroke(colour, 2.0 * k));
}

/// The mockup's ✕ (24 px drawing, stroked 2.2 px at 24) in `rect`.
fn cross(f: &mut Frame, rect: Rectangle, colour: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| {
        b.move_to(p(6.0, 6.0));
        b.line_to(p(18.0, 18.0));
        b.move_to(p(18.0, 6.0));
        b.line_to(p(6.0, 18.0));
    });
    f.stroke(&path, stroke(colour, 2.2 * k));
}

/// The tab's shadow and glow on a surface of `size`, the tab at `tab`:
/// blurred, outside the tab only (CSS draws none under it).
fn shadow(size: Size, tab: Rectangle) -> svg::Handle {
    let (w, h) = (size.width, size.height);
    let Rectangle {
        x,
        y,
        width,
        height,
    } = tab;
    let filter = |deviation: f32, id: &str| {
        format!(
            "<filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" \
             width=\"{w}\" height=\"{h}\"><feGaussianBlur stdDeviation=\"{deviation}\"/>\
             </filter>"
        )
    };
    let rect = |(dy, _, alpha): (f32, f32, f32), colour: &str, id: &str| {
        format!(
            "<rect x=\"{x}\" y=\"{}\" width=\"{width}\" height=\"{height}\" rx=\"{RADIUS}\" \
             fill=\"{colour}\" fill-opacity=\"{alpha}\" filter=\"url(#{id})\"/>",
            y + dy
        )
    };
    svg::Handle::from_memory(
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" \
             height=\"{h}\"><defs>{}{}<mask id=\"outside\" maskUnits=\"userSpaceOnUse\" \
             x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\"><rect width=\"{w}\" \
             height=\"{h}\" fill=\"#fff\"/><rect x=\"{x}\" y=\"{y}\" width=\"{width}\" \
             height=\"{height}\" rx=\"{RADIUS}\" fill=\"#000\"/></mask></defs>\
             <g mask=\"url(#outside)\">{}{}</g></svg>",
            filter(SHADOW.1, "shadow"),
            filter(GLOW.1, "glow"),
            rect(SHADOW, "#000", "shadow"),
            rect(GLOW, "#8FB3FF", "glow"),
        )
        .into_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{clock::time::Lang, reminder::Meeting};

    fn tab(left: i64, video: bool) -> Tab {
        Tab {
            serial: 1,
            meeting: Meeting {
                id: "r".to_owned(),
                title: "Revue de sprint".to_owned(),
                start: 100,
                end: 130,
                lead: 5,
                hhmm: "10:00".to_owned(),
                video: video.then(|| "https://meet.example.org/abc".to_owned()),
                page: None,
                place: None,
            },
            left,
        }
    }

    #[test]
    fn its_buttons_6_px_apart_from_6_px_in_on_the_right() {
        let words = Words::new(Lang::French);
        let laid = lay(&tab(5, true), words);
        // ✕ 6 px in from the right (inside the rim), Later 6 px left of it.
        assert_eq!(laid.dismiss.x + laid.dismiss.width, 340.0 - 7.0);
        let later = laid.later.expect("Later before the start");
        assert_eq!(later.x + later.width, laid.dismiss.x - 6.0);
        assert_eq!(laid.main.x + laid.main.width, later.x - 6.0);
        assert_eq!((laid.main.height, laid.main.center_y()), (30.0, 22.0));
        assert!(laid.join);
        // The text 12 px in, up to 6 px before Rejoindre.
        assert_eq!(laid.text.x, 13.0);
        assert_eq!(laid.text.x + laid.text.width, laid.main.x - 6.0);
        // Started: no Later; without a link, Voir.
        let laid = lay(&tab(0, false), words);
        assert_eq!(laid.later, None);
        assert_eq!(laid.main.x + laid.main.width, laid.dismiss.x - 6.0);
        assert_eq!((laid.join, laid.label), (false, "Voir"));
    }

    #[test]
    fn a_click_on_the_text_opens_on_a_button_acts() {
        let laid = lay(&tab(5, true), Words::new(Lang::French));
        let at = |r: Rectangle| part(&laid, r.center());
        assert_eq!(at(laid.dismiss), Some(Part::Dismiss));
        assert_eq!(laid.later.and_then(at), Some(Part::Later));
        assert_eq!(at(laid.main), Some(Part::Main));
        assert_eq!(at(laid.text), Some(Part::Text));
        assert_eq!(part(&laid, Point::new(-1.0, 20.0)), None);
        assert_eq!(Part::Main.act(true), Act::Join);
        assert_eq!(Part::Main.act(false), Act::Open);
        assert_eq!(Part::Text.act(true), Act::Open);
    }
}
