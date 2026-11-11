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
use crate::theme::Ink;
use crate::{
    Message,
    levels::draw::{FAINT, LINE, MANROPE_BOLD, TEXT, rgb},
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

const GLASS_TOP: Ink = Ink::Dark(Color::from_rgba8(22, 27, 42, 0.96));
const GLASS_BOTTOM: Ink = Ink::Dark(Color::from_rgba8(14, 17, 28, 0.97));
const OPAQUE_TOP: Ink = Ink::Dark(rgb(0x161B2A));
const OPAQUE_BOTTOM: Ink = Ink::Dark(rgb(0x0E111C));
const RIM: Ink = Ink::Dark(Color::from_rgba8(143, 179, 255, 0.55));
pub const LINE_BLUE: Ink = Ink::Dark(rgb(0xA9C4FF));
pub const JOIN: Ink = Ink::Dark(rgb(0x3F6FD8));
pub const JOIN_HOVER: Ink = Ink::Dark(rgb(0x4A7BE6));
const OPEN_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const OPEN_FILL_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const OPEN_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
pub const LATER_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const ICON_LIT: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const CLOCK_INK: Ink = Ink::Dark(rgb(0xC9D0DC));
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

/// A tab's surface: what nothing holds, the tab as the mockup draws it,
/// its parts lit as the pointer is on them ([`LIT`]); its controls are
/// the kit's, over it ([`view`]).
#[derive(Debug, Clone)]
pub struct Back {
    pub id: window::Id,
    /// The compositor blurs behind the shell (else the opaque colours).
    pub glass: bool,
    pub words: Words,
}

/// The parts each surface has under the pointer: the text's room is under
/// the buttons, which win.
static LIT: Mutex<Option<HashMap<window::Id, Vec<Part>>>> = Mutex::new(None);

/// The part lit on surface `id`: a button the pointer is on, else the text.
fn over(id: window::Id) -> Option<Part> {
    let lit = LIT.lock().unwrap_or_else(PoisonError::into_inner);
    let parts = lit.as_ref()?.get(&id)?;
    parts
        .iter()
        .copied()
        .find(|p| *p != Part::Text)
        .or_else(|| parts.first().copied())
}

/// What each surface last said it shows at rest, for the log.
static SAID: Mutex<Option<HashMap<window::Id, String>>> = Mutex::new(None);

/// Surface `id` went: what it said goes too.
pub fn forget(id: window::Id) {
    if let Some(said) = SAID.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        said.remove(&id);
    }
    if let Some(lit) = LIT.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        lit.remove(&id);
    }
}

impl canvas::Program<Message> for Back {
    type State = ();

    fn draw(
        &self,
        _state: &(),
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
        let over = over(self.id);
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
                over,
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
            over,
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
}

// --- Its controls, on the widget kit (#772) ------------------------------------------

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A part pressed: it acts at once.
    Act(Part),
    /// The pointer comes on a part, or leaves it.
    Over(Part, bool),
    Stale,
}

/// A part's id: what tests and screen readers know it by.
fn id_of(part: Part) -> String {
    format!("reminder:{part:?}")
}

/// Tab surface `id`: [`Back`] draws it, its parts are the kit's controls
/// over it, where the tab is on its surface.
pub fn view<'a>(id: window::Id, glass: bool, words: Words) -> iced::Element<'a, Message> {
    let back = canvas::Canvas::new(Back { id, glass, words })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill);
    crate::ui::Around::new(
        id,
        build(id, words),
        move |m| handle(id, words, m),
        Msg::Stale,
    )
    .own_by(
        move || {
            if let Some(shown) = super::seen(id) {
                crate::popover::tell(hocus_link::ToCompositor::Wake(
                    hocus_link::reminder_namespace(shown.tab.serial),
                ));
            }
        },
        move || super::seen(id).is_some(),
    )
    .rebuild(move || build(id, words))
    .on(back)
}

fn handle(id: window::Id, words: Words, message: Msg) {
    let Some(shown) = super::seen(id) else {
        return;
    };
    let laid = lay(&shown.tab, words);
    match message {
        Msg::Act(part) => {
            crate::tray::tip::leave();
            tracing::debug!(serial = shown.tab.serial, over = ?part, "a click on the reminder");
            super::act(shown.tab.serial, part.act(laid.join));
        }
        Msg::Over(part, on) => {
            let before = over(id);
            {
                let mut lit = LIT.lock().unwrap_or_else(PoisonError::into_inner);
                let parts = lit.get_or_insert_with(HashMap::new).entry(id).or_default();
                parts.retain(|p| *p != part);
                if on {
                    parts.push(part);
                }
            }
            let now = over(id);
            if now != before {
                tip(words, &shown, &laid, now);
            }
        }
        Msg::Stale => {}
    }
}

/// The tab's text, its main button, Later and ✕, each a control where the
/// tab puts it; drawn under them.
fn build<'a>(id: window::Id, words: Words) -> iced::Element<'a, Msg> {
    use hocus_a11y::Role;
    use iced::widget::{Space, Stack, mouse_area, pin};
    let Some(shown) = super::seen(id) else {
        return Space::new().into();
    };
    let laid = lay(&shown.tab, words);
    let whole = Rectangle::new(Point::ORIGIN, Size::new(super::WIDTH, HEIGHT));
    let mut parts = vec![
        (
            Part::Text,
            whole,
            Role::Button,
            words.name(&shown.tab, shown.shared),
        ),
        (Part::Main, laid.main, Role::Button, laid.label.to_owned()),
    ];
    if let Some(later) = laid.later {
        parts.push((
            Part::Later,
            later,
            Role::Button,
            words.later(shown.tab.left).to_owned(),
        ));
    }
    parts.push((
        Part::Dismiss,
        laid.dismiss,
        Role::Button,
        words.dismiss().to_owned(),
    ));
    let all: Vec<iced::Element<'a, Msg>> = parts
        .into_iter()
        .map(|(part, rect, role, name)| {
            let spot = mouse_area(Space::new().width(rect.width).height(rect.height))
                .on_press(Msg::Act(part))
                .on_enter(Msg::Over(part, true))
                .on_exit(Msg::Over(part, false))
                .interaction(mouse::Interaction::Pointer);
            pin(crate::ui::control(spot, id_of(part), role, name))
                .x(shown.at.x + rect.x)
                .y(shown.at.y + rect.y)
                .into()
        })
        .collect();
    Stack::with_children(all)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// The icon button under the pointer says its name after 500 ms, under
/// the tab; elsewhere, no tooltip.
fn tip(words: Words, shown: &Seen, laid: &Laid, over: Option<Part>) {
    let button = match over {
        Some(Part::Later) => laid.later.map(|r| (r, words.later(shown.tab.left))),
        Some(Part::Dismiss) => Some((laid.dismiss, words.dismiss())),
        _ => None,
    };
    let Some((rect, name)) = button else {
        crate::tray::tip::leave();
        return;
    };
    let top = shown.margin.0 as f32;
    let surface_width = shown.at.x + super::WIDTH + if shown.card { 0.0 } else { 8.0 };
    let centre = crate::side::from_edge(shown.margin, surface_width, shown.at.x + rect.center_x());
    crate::tray::tip::hover_under(
        &shown.bar,
        top + shown.at.y + HEIGHT + TIP_GAP,
        centre,
        TIP_DELAY,
        name.to_owned(),
    );
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
        .add_stop(0.0, top.get())
        .add_stop(1.0, bottom.get());
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
        Stroke::default().with_color(RIM.get()).with_width(BORDER),
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
            TEXT.get(),
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
        LINE_BLUE.get(),
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
            Stroke::default()
                .with_color(OPEN_RIM.get())
                .with_width(BORDER),
        );
    }
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            laid.label.to_owned(),
            Point::new(x, laid.main.center_y()),
            BUTTON_TEXT,
            if laid.join { Color::WHITE } else { TEXT.get() },
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
        f.fill(&kit::rounded(later, BUTTON_RADIUS), fill.get());
        clock(
            f,
            centred(later, CLOCK),
            if lit(Part::Later) {
                TEXT.get()
            } else {
                CLOCK_INK.get()
            },
        );
    }
    // ✕.
    if lit(Part::Dismiss) {
        f.fill(&kit::rounded(laid.dismiss, BUTTON_RADIUS), ICON_LIT.get());
    }
    cross(
        f,
        centred(laid.dismiss, CROSS),
        if lit(Part::Dismiss) {
            TEXT.get()
        } else {
            FAINT.get()
        },
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
        assert_eq!(Part::Main.act(true), Act::Join);
        assert_eq!(Part::Main.act(false), Act::Open);
        assert_eq!(Part::Text.act(true), Act::Open);
    }
}
