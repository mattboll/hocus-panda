//! The popovers' shared parts, as widgets at the mockups' sizes: the
//! content's frame, the header, dividers, notes, spaced labels, the small
//! button, a connected block, a row, a list that scrolls, the Settings'
//! link at the bottom; and the canvases the look needs (a glyph, the
//! switch, a spinner).

use std::time::Instant;

use iced::{
    Color, Element, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    alignment::Vertical,
    mouse,
    widget::{
        Space, button, canvas, container, mouse_area, row, scrollable, space,
        text::{LineHeight, Shaping, Wrapping},
    },
};

use super::{
    control::{Control, control},
    style::{self, boxed, clear, filled, hovered, lit, said},
};
use crate::{
    levels::draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, stroke},
    popover::kit::{
        self, FILL, FILL_HOVER, HEADER, HEADER_TOP, ICON_GAP, LABEL, NOTE, NOTE_HEIGHT, NOTE_TOP,
        PAD_BOTTOM, ROW_OUT, SCROLLBAR, SMALL_PAD, SMALL_RADIUS, STATUS, SWITCH, Spin, TITLE,
    },
    theme::Ink,
};

type El<'a, M> = Element<'a, M>;

/// A connected block: 48 px at least, lit green.
pub const BLOCK: f32 = 48.0;
const BLOCK_FILL: Ink = Ink::Accent(0.10);
const SMALL_BUTTON: f32 = 28.0;
const RULE: f32 = 4.0;
const RULE_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
/// The Settings' link (6.5): its row, its tile, its words.
const LINK: f32 = 36.0;
const LINK_TILE: f32 = 24.0;
const LINK_TILE_RADIUS: f32 = 7.0;
const LINK_TEXT: f32 = 13.5;
const LINK_RADIUS: f32 = 8.0;
const LINK_TILE_FILL: Color = Color::from_rgba8(255, 255, 255, 0.07);
const LINK_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.06);
/// A spinner, and the gap after it.
pub const SPINNER: f32 = 12.0;

/// A popover's content: its blocks one under the other, in its padding.
pub fn content<'a, M: 'a>(blocks: Vec<El<'a, M>>) -> El<'a, M> {
    container(iced::widget::Column::with_children(blocks))
        .padding(Padding {
            top: HEADER_TOP,
            right: kit::LEFT - ROW_OUT,
            bottom: PAD_BOTTOM + kit::BORDER,
            left: kit::LEFT - ROW_OUT,
        })
        .into()
}

/// `content` 6 px down from the block above, or nothing when it does not
/// show: a block keeps its place in the tree either way, and with it what
/// its widgets keep (how far a list is scrolled).
pub fn block<'a, M: 'a>(gap: f32, content: Option<El<'a, M>>) -> El<'a, M> {
    match content {
        Some(content) => container(content)
            .padding(Padding {
                top: gap,
                ..Padding::ZERO
            })
            .into(),
        None => Space::new().into(),
    }
}

/// The header: the title, and on its right `aside` (a switch, a word).
pub fn header<'a, M: 'a>(title: &str, aside: Option<El<'a, M>>) -> El<'a, M> {
    let mut line = row![
        said(title.to_owned(), TITLE, MANROPE_BOLD, TEXT.get()),
        space::horizontal()
    ]
    .align_y(Vertical::Center);
    if let Some(aside) = aside {
        line = line.push(aside);
    }
    inset(container(line).height(HEADER).align_y(Vertical::Center))
}

/// `content` between the content's sides, 6 px in from the rows'.
pub fn inset<'a, M: 'a>(content: impl Into<El<'a, M>>) -> El<'a, M> {
    container(content).padding([0.0, ROW_OUT]).into()
}

pub fn gap<'a, M: 'a>(width: f32) -> El<'a, M> {
    Space::new().width(width).into()
}

/// A divider: 1 px across the content, `RULE` px above and below.
pub fn rule<'a, M: 'a>() -> El<'a, M> {
    rule_of(RULE)
}

/// A divider: 1 px across the content, `around` px above and below.
pub fn rule_of<'a, M: 'a>(around: f32) -> El<'a, M> {
    container(
        container(Space::new().width(Length::Fill).height(1.0))
            .style(|_| boxed(RULE_FILL.get(), 0.0)),
    )
    .padding(Padding {
        top: around,
        right: ROW_OUT,
        bottom: around,
        left: ROW_OUT,
    })
    .into()
}

/// What is said in place of a list, or under one: a line for screen
/// readers too.
pub fn note<'a, M: 'static>(note: &str) -> El<'a, M> {
    super::line(
        container(said(note.to_owned(), NOTE, MANROPE, FAINT.get()))
            .height(NOTE_HEIGHT)
            .padding(Padding {
                top: NOTE_TOP,
                right: ROW_OUT,
                bottom: 0.0,
                left: ROW_OUT,
            }),
        note,
    )
    .into()
}

/// A section's label: its capitals spaced (CSS's `letter-spacing:
/// 0.08em`, which iced's text has not: a letter a widget), `height` tall,
/// `aside` on its right; a line for screen readers.
pub fn label<'a, M: 'static>(word: &str, height: f32, aside: Option<El<'a, M>>) -> El<'a, M> {
    label_as(word, word, height, aside)
}

/// A section's label drawn as `word`, read as `said` (not in capitals).
pub fn label_as<'a, M: 'static>(
    word: &str,
    said_as: &str,
    height: f32,
    aside: Option<El<'a, M>>,
) -> El<'a, M> {
    let letters = spaced(word, LABEL, DIM.get(), f32::INFINITY);
    let mut line = row![letters, space::horizontal()].align_y(Vertical::Center);
    if let Some(aside) = aside {
        line = line.push(aside);
    }
    super::line(
        inset(container(line).height(height).align_y(Vertical::Center)),
        said_as,
    )
    .into()
}

/// `word`'s capitals in bold at `size`, spaced 0.08 em (CSS's
/// `letter-spacing`, which iced's text has not: a letter a widget), as
/// many as fit in `room`.
pub fn spaced<'a, M: 'a>(word: &str, size: f32, colour: Color, room: f32) -> El<'a, M> {
    spaced_in(word, size, MANROPE_BOLD, colour, 0.08, room)
}

/// `word` in `font` at `size`, its letters `em` apart (CSS's
/// `letter-spacing`), all of it.
pub fn spaced_by<'a, M: 'a>(
    word: &str,
    size: f32,
    font: iced::Font,
    colour: Color,
    em: f32,
) -> El<'a, M> {
    spaced_in(word, size, font, colour, em, f32::INFINITY)
}

/// `word` in `font` at `size`, its letters `em` apart, as many as fit in
/// `room`.
fn spaced_in<'a, M: 'a>(
    word: &str,
    size: f32,
    font: iced::Font,
    colour: Color,
    em: f32,
    room: f32,
) -> El<'a, M> {
    let spacing = em * size;
    let mut letters = row![].spacing(spacing);
    let mut x = 0.0;
    for ch in word.chars() {
        let ch = ch.to_string();
        let width = kit::measure(&ch, size, font);
        if x + width > room {
            break;
        }
        x += width + spacing;
        letters = letters.push(said(ch, size, font, colour));
    }
    letters.into()
}

/// A word in a button, in the middle of its height, in the button's
/// colour.
pub fn centred<'a, M: 'a>(word: &str, size: f32, font: iced::Font) -> El<'a, M> {
    container(
        iced::widget::text(word.to_owned())
            .size(size)
            .font(font)
            .line_height(LineHeight::Relative(LINE))
            .shaping(Shaping::Advanced)
            .wrapping(Wrapping::None),
    )
    .center_y(Length::Fill)
    .into()
}

/// A small button ("Déconnecter"): 28 px, its word dim, lit under the
/// pointer.
pub fn small_button<'a, M: Clone + 'a>(word: &str, press: M) -> button::Button<'a, M> {
    button(centred(word, STATUS, MANROPE))
        .padding([0.0, SMALL_PAD])
        .height(SMALL_BUTTON)
        .style(|_, status| quiet(status, SMALL_RADIUS))
        .on_press(press)
}

/// A quiet button: filled, lit under the pointer (under high contrast, a
/// border as well), its words dim until then.
pub fn quiet(status: button::Status, radius: f32) -> button::Style {
    let on = lit(status);
    let mut style = filled(
        if on && !crate::theme::contrast() {
            FILL_HOVER
        } else {
            FILL
        }
        .get(),
        radius,
        if on { TEXT } else { DIM }.get(),
    );
    if on && crate::theme::contrast() {
        style.border = hovered(FILL_HOVER.get(), radius).border;
    }
    style
}

/// A connected block: `content` in its lit frame, 48 px at least.
pub fn connected<'a, M: 'a>(content: impl Into<El<'a, M>>) -> El<'a, M> {
    container(row![content.into(), Space::new().height(BLOCK)].align_y(Vertical::Center))
        .padding([0.0, ROW_OUT])
        .style(|_| boxed(BLOCK_FILL.get(), kit::ROW_RADIUS))
        .into()
}

/// A row of a list: `line` across it, lit under the pointer while it can
/// be pressed (`press`).
pub fn row_button<'a, M: Clone + 'a>(
    line: impl Into<El<'a, M>>,
    height: f32,
    press: Option<M>,
) -> button::Button<'a, M> {
    let can = press.is_some();
    button(container(line).center_y(Length::Fill))
        .padding([0.0, kit::ROW_PAD])
        .width(Length::Fill)
        .height(height)
        .style(move |_, status| {
            if can && lit(status) {
                hovered(style::ROW_HOVER.get(), kit::ROW_RADIUS)
            } else {
                clear()
            }
        })
        .on_press_maybe(press)
}

/// A list's rows, scrolling past `most` px, its scrollbar 6 px; whether
/// the pointer is over it told (`over`), for its rows hold their order
/// then. Named `id`, which a row is revealed in.
pub fn list<'a, M: Clone + 'a>(
    rows: El<'a, M>,
    id: &'static str,
    most: f32,
    over: fn(bool) -> M,
) -> El<'a, M> {
    list_by(rows, id, most, Some(over), None)
}

/// A list as [`list`], the pointer over it told if asked, a notch of the
/// wheel scrolling `notch` px when given ([`super::notched`]), else
/// iced's 60.
pub fn list_by<'a, M: Clone + 'a>(
    rows: El<'a, M>,
    id: &'static str,
    most: f32,
    over: Option<fn(bool) -> M>,
    notch: Option<f32>,
) -> El<'a, M> {
    let scrolled = scrollable(rows)
        .id(id)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(SCROLLBAR)
                .scroller_width(SCROLLBAR)
                .margin(0)
                .spacing(0),
        ))
        .style(|_, _| style::scrollbar(SCROLLBAR));
    let scrolled: El<'a, M> = match notch {
        Some(notch) => super::notched::Notched::new(scrolled, notch).into(),
        None => scrolled.into(),
    };
    let told: El<'a, M> = match over {
        Some(over) => mouse_area(scrolled)
            .on_enter(over(true))
            .on_exit(over(false))
            .into(),
        None => scrolled,
    };
    container(told).max_height(most).into()
}

/// "Réglages du …": the sliders in their tile, the words, a chevron; the
/// control `id`, its divider above it.
pub fn settings_link<'a, M: Clone + 'static>(
    words: &str,
    id: &str,
    press: M,
    focused: bool,
    focus: M,
) -> [El<'a, M>; 2] {
    let tile = container(drawn(LINK_TILE, LINK_TILE, |f, _| {
        let at = Point::new(4.5, 4.5);
        for segment in [
            [(4.0, 7.0), (13.0, 7.0)],
            [(17.0, 7.0), (20.0, 7.0)],
            [(15.0, 4.5), (15.0, 9.5)],
            [(4.0, 17.0), (7.0, 17.0)],
            [(11.0, 17.0), (20.0, 17.0)],
            [(9.0, 14.5), (9.0, 19.5)],
        ] {
            polyline(f, at, 15.0 / 24.0, 2.2, DIM.get(), &segment);
        }
    }))
    .style(|_| boxed(LINK_TILE_FILL, LINK_TILE_RADIUS));
    let line = row![
        tile,
        gap(ICON_GAP),
        said(
            words.to_owned(),
            LINK_TEXT,
            kit::MANROPE_SEMIBOLD,
            TEXT.get()
        ),
        space::horizontal(),
        drawn(14.0, 14.0, |f, _| {
            polyline(
                f,
                Point::ORIGIN,
                14.0 / 24.0,
                2.4,
                FAINT.get(),
                &[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)],
            );
        }),
    ]
    .align_y(Vertical::Center);
    let link = button(container(line).center_y(Length::Fill))
        .padding([0.0, ROW_OUT])
        .width(Length::Fill)
        .height(LINK)
        .style(|_, status| {
            if lit(status) {
                hovered(LINK_HOVER, LINK_RADIUS)
            } else {
                clear()
            }
        })
        .on_press(press.clone());
    let link: Control<'a, M> = control(link, id, hocus_a11y::Role::Button, words)
        .focus(focused, focus, LINK_RADIUS)
        .activate(press);
    [rule(), link.into()]
}

// --- What the look draws ---------------------------------------------------------------

/// A canvas `width` × `height` drawn by `draw` (its frame, its size).
pub fn drawn<'a, M: 'a>(
    width: f32,
    height: f32,
    draw: impl Fn(&mut canvas::Frame, Size) + 'a,
) -> El<'a, M> {
    canvas(Drawn(Box::new(draw), 0.0))
        .width(width)
        .height(height)
        .into()
}

/// A canvas `width` × `height` drawn by `draw` as [`drawn`], which may
/// draw `out` px past its bounds (a ring around an icon).
pub fn drawn_out<'a, M: 'a>(
    width: f32,
    height: f32,
    out: f32,
    draw: impl Fn(&mut canvas::Frame, Size) + 'a,
) -> El<'a, M> {
    canvas(Drawn(Box::new(draw), out))
        .width(width)
        .height(height)
        .into()
}

/// What draws a canvas: its frame, its size.
type Draw<'a> = Box<dyn Fn(&mut canvas::Frame, Size) + 'a>;

/// What draws a canvas, and how far past its bounds.
struct Drawn<'a>(Draw<'a>, f32);

impl<M> canvas::Program<M> for Drawn<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let out = self.1;
        let mut f = if out > 0.0 {
            let reach = Rectangle::new(
                Point::new(-out, -out),
                Size::new(bounds.width + 2.0 * out, bounds.height + 2.0 * out),
            );
            canvas::Frame::with_bounds(renderer, reach)
        } else {
            canvas::Frame::new(renderer, bounds.size())
        };
        (self.0)(&mut f, bounds.size());
        vec![f.into_geometry()]
    }
}

/// Lines through `points` (SVG units from `at`, `scale` px a unit).
pub fn polyline(
    f: &mut canvas::Frame,
    at: Point,
    scale: f32,
    width: f32,
    colour: Color,
    points: &[(f32, f32)],
) {
    let p = |(x, y): (f32, f32)| Point::new(at.x + x * scale, at.y + y * scale);
    let path = canvas::Path::new(|b| {
        if let Some((first, rest)) = points.split_first() {
            b.move_to(p(*first));
            for point in rest {
                b.line_to(p(*point));
            }
        }
    });
    f.stroke(&path, stroke(width * scale, colour));
}

/// A spinner, turning: its surface alone draws again each frame.
pub fn spinner<'a, M: 'a>(colour: Color) -> El<'a, M> {
    canvas(Spinner(colour))
        .width(SPINNER)
        .height(SPINNER)
        .into()
}

struct Spinner(Color);

/// A spinner's turn.
#[derive(Debug, Default)]
pub struct Turning {
    spin: Spin,
    now: Option<Instant>,
}

impl<M> canvas::Program<M> for Spinner {
    type State = Turning;

    fn update(
        &self,
        state: &mut Turning,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        state.now = Some(*now);
        state.spin.run(true, *now);
        Some(canvas::Action::request_redraw())
    }

    fn draw(
        &self,
        state: &Turning,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let turn = state.now.map_or(0.0, |now| state.spin.turn(now));
        kit::spinner(&mut f, Point::ORIGIN, self.0, turn);
        vec![f.into_geometry()]
    }
}

/// The header's switch: its knob where `knob` says at a moment (0 off, 1
/// on; with `motion`, it slides), drawn again while `sliding` says so.
pub struct Knob {
    pub knob: fn(Instant, bool) -> f32,
    pub sliding: fn(Instant) -> bool,
    pub motion: bool,
}

impl Knob {
    /// As a button that does `press`.
    pub fn view<'a, M: Clone + 'static>(self, press: M) -> button::Button<'a, M> {
        button(canvas(self).width(SWITCH.width).height(SWITCH.height))
            .padding(0)
            .style(|_, _| clear())
            .on_press(press)
    }
}

impl<M> canvas::Program<M> for Knob {
    type State = Option<Instant>;

    fn update(
        &self,
        state: &mut Option<Instant>,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        *state = Some(*now);
        (self.motion && (self.sliding)(*now)).then(canvas::Action::request_redraw)
    }

    fn draw(
        &self,
        state: &Option<Instant>,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let rect = kit::Switch::rect();
        f.translate(iced::Vector::new(-rect.x, -rect.y));
        kit::Switch::draw(
            &mut f,
            (self.knob)(state.unwrap_or_else(Instant::now), self.motion),
        );
        vec![f.into_geometry()]
    }
}
