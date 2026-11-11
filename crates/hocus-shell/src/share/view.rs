//! The chooser on its surface, the 5.11 mockup's sizes × the launcher's
//! zoom: the asking application's icon, what it asks and what it says of
//! itself; the tabs; the grid of three columns of cards, each a thumbnail's
//! box (the compositor draws the thumbnail in it), a name and a line; "show
//! the pointer"; Cancel and the blue Share. The compositor darkens the
//! screen, draws its shadow and pops it in.
//!
//! On the widget kit (#772): each tab, card and button is the kit's,
//! placed where the layout puts it (the compositor draws the thumbnails
//! where it says they go).

use hocus_a11y::Role;
use hocus_link::{
    LAUNCHER_ZOOM, SHARE_CHOOSER_NAMESPACE, SHARE_CHOOSER_RADIUS, SHARE_THUMB_RADIUS,
};
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Size, Theme,
    alignment::Vertical,
    mouse,
    widget::{
        Space, Stack, button, canvas,
        canvas::{Frame, Geometry, LineCap, LineJoin, Path, Stroke, gradient},
        container, mouse_area, pin, row,
        text::Alignment,
    },
    window,
};

use super::{Chooser, Tab, Words};
use crate::theme::Ink;
use crate::{
    levels::draw::{DIM, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::{
        Pressed,
        kit::{self, MANROPE_SEMIBOLD},
    },
    ui::{self, Around, control, parts::drawn, style},
};

type El<'a> = Element<'a, Msg>;

const Z: f32 = LAUNCHER_ZOOM as f32;
/// 560 wide, 16 px in, 12 px between its parts, a 1 px rim.
const WIDTH: f32 = 560.0 * Z;
const PAD: f32 = 16.0 * Z;
const GAP: f32 = 12.0 * Z;
const BORDER: f32 = 1.0;
/// The header: a 32 px icon (radius 9), 10 px before the words; the title
/// 15 px bold over the line 12 px, 1 px apart.
const ICON: f32 = 32.0 * Z;
pub const ICON_PX: u32 = (32.0 * LAUNCHER_ZOOM) as u32;
const ICON_RADIUS: f32 = 9.0 * Z;
const ICON_GAP: f32 = 10.0 * Z;
const TITLE: f32 = 15.0 * Z;
const SUB: f32 = 12.0 * Z;
const LINE_GAP: f32 = 1.0 * Z;
/// The tabs: 3 px in their box (radius 10), each 30 px, 14 px in, radius 8,
/// 12.5 px bold.
const TABS_PAD: f32 = 3.0 * Z;
const TABS_RADIUS: f32 = 10.0 * Z;
const TAB: f32 = 30.0 * Z;
const TAB_PAD: f32 = 14.0 * Z;
const TAB_RADIUS: f32 = 8.0 * Z;
const TAB_TEXT: f32 = 12.5 * Z;
/// The grid: three columns 10 px apart; a card 6 px in, radius 12, its
/// parts 4 px apart: the thumbnail's box 86 px tall (radius 8), the name
/// 12 px 600, the line 10.5 px.
pub const COLUMNS: usize = 3;
const CELL_GAP: f32 = 10.0 * Z;
const CARD_PAD: f32 = 6.0 * Z;
const CARD_GAP: f32 = 4.0 * Z;
const CARD_RADIUS: f32 = 12.0 * Z;
const THUMB: f32 = 86.0 * Z;
const NAME: f32 = 12.0 * Z;
const LINE: f32 = 10.5 * Z;
/// "Montrer le pointeur": 12.5 px, its box 8 px before it.
const CHECK: f32 = 13.0 * Z;
const CHECK_TEXT: f32 = 12.5 * Z;
const CHECK_GAP: f32 = 8.0 * Z;
/// The buttons: 34 px, radius 10, 8 px apart, 12.5 px; Annuler 14 px in,
/// Partager 16.
const BUTTON: f32 = 34.0 * Z;
const BUTTON_RADIUS: f32 = 10.0 * Z;
const BUTTON_GAP: f32 = 8.0 * Z;
const BUTTON_TEXT: f32 = 12.5 * Z;
const CANCEL_PAD: f32 = 14.0 * Z;
const SHARE_PAD: f32 = 16.0 * Z;
/// The screen's margin the chooser keeps, at the least.
const MARGIN: f32 = 24.0 * Z;
/// Rows shown at most.
const MOST_ROWS: usize = 3;

/// The mockup's panel: `rgba(22,27,42,0.97) → rgba(14,17,28,0.98)`, its rim
/// `rgba(255,255,255,0.12)`; opaque without glass.
const TINT: (Ink, Ink) = (
    Ink::Dark(Color::from_rgba8(22, 27, 42, 0.97)),
    Ink::Dark(Color::from_rgba8(14, 17, 28, 0.98)),
);
const OPAQUE: (Ink, Ink) = (Ink::Dark(rgb(0x161B2A)), Ink::Dark(rgb(0x0E111C)));
const RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const TABS_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const TAB_ON: Ink = Ink::Dark(rgb(0xEEF1F6));
const TAB_ON_INK: Ink = Ink::Dark(rgb(0x0B1020));
const TAB_INK: Ink = Ink::Dark(rgb(0xC9D0DC));
const BLUE: Ink = Ink::Dark(rgb(0x5B8DEF));
const CHOSEN: Ink = Ink::Dark(Color::from_rgba8(91, 141, 239, 0.16));
const CHOSEN_RING: Ink = Ink::Dark(Color::from_rgba8(91, 141, 239, 0.8));
const CARD_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const THUMB_FILL: Ink = Ink::Dark(rgb(0x161C2B));
const THUMB_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const LINE_INK: Ink = Ink::Dark(rgb(0x7C8698));
const CHECK_INK: Ink = Ink::Dark(rgb(0xC9D0DC));
const GREEN: Ink = Ink::Accent(1.0);
const CANCEL_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const CANCEL_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const LETTER_FROM: Ink = Ink::Dark(rgb(0x5C6576));
const LETTER_TO: Ink = Ink::Dark(rgb(0x9AA3B4));
const LINE_HEIGHT: f32 = crate::levels::draw::LINE;

/// A card: what it shows (its index in the tab's list), where, and its
/// thumbnail's box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Card {
    pub index: usize,
    pub rect: Rectangle,
    pub thumb: Rectangle,
}

/// Where the chooser's parts are, and its size.
#[derive(Debug, Clone)]
pub struct Layout {
    pub size: Size,
    pub icon: Rectangle,
    pub title_at: Point,
    pub sub_at: Point,
    pub tabs_box: Rectangle,
    pub tabs: Vec<(Tab, Rectangle)>,
    pub cards: Vec<Card>,
    /// Rows in the list, and shown.
    pub rows: usize,
    pub rows_shown: usize,
    pub check: Rectangle,
    pub cancel: Rectangle,
    pub share: Rectangle,
}

fn card_height() -> f32 {
    2.0 * BORDER + 2.0 * CARD_PAD + THUMB + 2.0 * CARD_GAP + (NAME + LINE) * LINE_HEIGHT
}

impl Layout {
    pub fn of(chooser: &Chooser) -> Self {
        let words = super::words();
        let left = BORDER + PAD;
        let inner = WIDTH - 2.0 * left;
        let mut y = left;
        let header = ICON.max((TITLE + SUB) * LINE_HEIGHT + LINE_GAP);
        let icon = Rectangle::new(
            Point::new(left, y + (header - ICON) / 2.0),
            Size::new(ICON, ICON),
        );
        let words_top = y + (header - ((TITLE + SUB) * LINE_HEIGHT + LINE_GAP)) / 2.0;
        let title_at = Point::new(
            left + ICON + ICON_GAP,
            words_top + TITLE * LINE_HEIGHT / 2.0,
        );
        let sub_at = Point::new(
            title_at.x,
            words_top + TITLE * LINE_HEIGHT + LINE_GAP + SUB * LINE_HEIGHT / 2.0,
        );
        y += header + GAP;
        // The tabs.
        let mut x = left + TABS_PAD;
        let mut tabs = Vec::new();
        for tab in chooser.tabs() {
            let w = (2.0 * TAB_PAD + kit::measure(words.tab(tab), TAB_TEXT, MANROPE_BOLD)).ceil();
            tabs.push((
                tab,
                Rectangle::new(Point::new(x, y + TABS_PAD), Size::new(w, TAB)),
            ));
            x += w;
        }
        let tabs_box = Rectangle::new(
            Point::new(left, y),
            Size::new(x + TABS_PAD - left, TAB + 2.0 * TABS_PAD),
        );
        y += tabs_box.height + GAP;
        // The grid, as many rows as fit the screen, three at most.
        let fixed = |rows: usize| {
            let grid = rows as f32 * card_height() + rows.saturating_sub(1) as f32 * CELL_GAP;
            y + grid + GAP + CHECK_TEXT * LINE_HEIGHT + GAP + BUTTON + PAD + BORDER
        };
        let count = match chooser.tab {
            Tab::Screens => chooser.choose.screens.len(),
            Tab::Windows => chooser.choose.windows.len(),
        };
        let rows = count.div_ceil(COLUMNS).max(1);
        let room = chooser.choose.height as f32 - 2.0 * MARGIN;
        let mut rows_shown = rows.min(MOST_ROWS);
        while rows_shown > 1 && fixed(rows_shown) > room {
            rows_shown -= 1;
        }
        let column = (inner - (COLUMNS as f32 - 1.0) * CELL_GAP) / COLUMNS as f32;
        let mut cards = Vec::new();
        let first = chooser.scroll.min(rows.saturating_sub(rows_shown)) * COLUMNS;
        for index in first..count.min(first + rows_shown * COLUMNS) {
            let (row, col) = ((index - first) / COLUMNS, (index - first) % COLUMNS);
            let rect = Rectangle::new(
                Point::new(
                    left + col as f32 * (column + CELL_GAP),
                    y + row as f32 * (card_height() + CELL_GAP),
                ),
                Size::new(column, card_height()),
            );
            let thumb = Rectangle::new(
                Point::new(rect.x + BORDER + CARD_PAD, rect.y + BORDER + CARD_PAD),
                Size::new(rect.width - 2.0 * (BORDER + CARD_PAD), THUMB),
            );
            cards.push(Card { index, rect, thumb });
        }
        y += rows_shown as f32 * card_height() + (rows_shown - 1) as f32 * CELL_GAP + GAP;
        let check_w = CHECK + CHECK_GAP + kit::measure(words.pointer(), CHECK_TEXT, MANROPE);
        let check = Rectangle::new(
            Point::new(left, y),
            Size::new(check_w, CHECK_TEXT * LINE_HEIGHT),
        );
        y += check.height + GAP;
        let share_w =
            (2.0 * SHARE_PAD + kit::measure(words.share(), BUTTON_TEXT, MANROPE_BOLD)).ceil();
        let cancel_w =
            (2.0 * CANCEL_PAD + kit::measure(words.cancel(), BUTTON_TEXT, MANROPE_SEMIBOLD)).ceil();
        let share = Rectangle::new(
            Point::new(WIDTH - left - share_w, y),
            Size::new(share_w, BUTTON),
        );
        let cancel = Rectangle::new(
            Point::new(share.x - BUTTON_GAP - cancel_w, y),
            Size::new(cancel_w, BUTTON),
        );
        y += BUTTON + PAD + BORDER;
        Self {
            size: Size::new(WIDTH, y.ceil()),
            icon,
            title_at,
            sub_at,
            tabs_box,
            tabs,
            cards,
            rows,
            rows_shown,
            check,
            cancel,
            share,
        }
    }

    /// Its surface's size.
    pub fn surface(&self) -> (u32, u32) {
        (
            self.size.width.ceil() as u32,
            self.size.height.ceil() as u32,
        )
    }
}

// --- The surface, on the widget kit ------------------------------------------------

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A tab clicked.
    Tab(Tab),
    /// A card clicked (twice in a row: shared).
    Card(usize),
    /// "Montrer le pointeur".
    Pointer,
    Cancel,
    Share,
    /// The wheel, by rows (down positive).
    Wheel(mouse::ScrollDelta),
    /// A key: the chooser's own (`Tab` the other tab, `Space` the pointer).
    Key(Pressed),
    /// What it shows changed under the view.
    Stale,
}

/// A control's id: what tests and screen readers know it by.
fn id_of(what: &str) -> String {
    format!("share:{what}")
}

/// The chooser on its surface `id`, on the glass (else opaque).
pub fn view<'a>(id: window::Id, words: Words, glass: bool) -> Element<'a, crate::Message> {
    Around::new(id, build(id, words), move |m| handle(id, m), Msg::Stale)
        .own(SHARE_CHOOSER_NAMESPACE, move || super::is_surface(id))
        .rebuild(move || build(id, words))
        .keys(|pressed| Some(Msg::Key(pressed.clone())))
        .on(canvas(Tint(glass)).width(Length::Fill).height(Length::Fill))
}

fn handle(id: window::Id, message: Msg) {
    let now = std::time::Instant::now();
    match message {
        Msg::Tab(tab) => super::act(id, super::Act::Tab(tab), now),
        Msg::Card(index) => super::act(id, super::Act::Card(index), now),
        Msg::Pointer => super::act(id, super::Act::Pointer, now),
        Msg::Cancel => super::act(id, super::Act::Cancel, now),
        Msg::Share => super::act(id, super::Act::Share, now),
        Msg::Wheel(delta) => {
            let y = match delta {
                mouse::ScrollDelta::Lines { y, .. } => y,
                mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
            };
            if y != 0.0 {
                super::act(id, super::Act::Wheel(if y > 0.0 { -1 } else { 1 }), now);
            }
        }
        Msg::Key(pressed) => {
            super::key_on(id, &pressed.key);
        }
        Msg::Stale => {}
    }
}

/// What the chooser last said it shows, for the log.
static SAID: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// Its content, as the chooser is now; said in the log when it changed.
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    let Some(chooser) = super::with(|c| c.as_ref().filter(|c| c.id == id).cloned()) else {
        return Space::new().into();
    };
    let (content, said) = content(&chooser, words);
    let mut last = SAID
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if last.as_ref() != Some(&said) {
        tracing::debug!(?said, "the share's chooser shows it");
        *last = Some(said);
    }
    content
}

/// `content` placed at `at`.
fn at<'a>(content: impl Into<El<'a>>, at: Point) -> El<'a> {
    pin(content).x(at.x).y(at.y).into()
}

/// The chooser's widgets where its layout places them, and what it says.
fn content<'a>(chooser: &Chooser, words: Words) -> (El<'a>, Vec<String>) {
    let layout = Layout::of(chooser);
    let mut said = Vec::new();
    let mut all: Vec<El<'a>> = Vec::new();
    // The header.
    let app = super::app_name(&chooser.choose.app);
    let icon_name = super::entry_of(&chooser.choose.app).and_then(|e| e.icon);
    let scale = chooser.choose.scale as f32 / 120.0;
    let px = crate::launcher::icons::pixels(ICON_PX, scale);
    let handle = icon_name.and_then(|name| crate::launcher::icons::of(&name, px).flatten());
    let initial = app.clone().unwrap_or_else(|| "?".to_owned());
    all.push(at(
        drawn(ICON, ICON, move |f, size| {
            let rect = Rectangle::new(Point::ORIGIN, size);
            match &handle {
                Some(handle) => f.draw_image(rect, canvas::Image::new(handle.clone())),
                None => letter(f, rect, &initial),
            }
        }),
        layout.icon.position(),
    ));
    let room = WIDTH - layout.title_at.x - PAD - BORDER;
    let title = elided(&words.asks(app.as_deref()), room, TITLE, MANROPE_BOLD);
    let sub = elided(&words.choose(&chooser.choose.detail), room, SUB, MANROPE);
    all.push(at(
        ui::line(
            said_in(title.clone(), TITLE, MANROPE_BOLD, TEXT.get()),
            title.clone(),
        ),
        Point::new(
            layout.title_at.x,
            layout.title_at.y - TITLE * LINE_HEIGHT / 2.0,
        ),
    ));
    all.push(at(
        ui::line(said_in(sub.clone(), SUB, MANROPE, DIM.get()), sub.clone()),
        Point::new(layout.sub_at.x, layout.sub_at.y - SUB * LINE_HEIGHT / 2.0),
    ));
    said.push(title);
    said.push(sub);
    // The tabs.
    all.push(at(
        container(
            Space::new()
                .width(layout.tabs_box.width)
                .height(layout.tabs_box.height),
        )
        .style(|_| style::boxed(TABS_FILL.get(), TABS_RADIUS)),
        layout.tabs_box.position(),
    ));
    for (tab, rect) in &layout.tabs {
        let on = chooser.tab == *tab;
        let name = words.tab(*tab);
        let pressed = button(
            container(said_in(
                name,
                TAB_TEXT,
                MANROPE_BOLD,
                if on { TAB_ON_INK.get() } else { TAB_INK.get() },
            ))
            .center(Length::Fill),
        )
        .padding(0)
        .width(rect.width)
        .height(rect.height)
        .style(move |_, _| {
            if on {
                style::filled(TAB_ON.get(), TAB_RADIUS, TAB_ON_INK.get())
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Tab(*tab));
        all.push(at(
            control(
                pressed,
                id_of(&format!("Tab({tab:?})")),
                Role::RadioButton,
                name,
            )
            .node(|n| n.checked(on)),
            rect.position(),
        ));
        if on {
            said.push(format!("[{name}]"));
        }
    }
    // The cards: the thumbnail's box (the compositor draws in it), the
    // name and the line.
    for card in &layout.cards {
        let selected = chooser.selected == card.index;
        let (name, line) = card_words(words, chooser, card.index);
        let text_room = card.thumb.width;
        let name = elided(&name, text_room, NAME, MANROPE_SEMIBOLD);
        let line = elided(&line, text_room, LINE, MANROPE);
        let thumb = card.thumb;
        let inside = Point::new(thumb.x - card.rect.x, thumb.y - card.rect.y);
        let name_top = inside.y + thumb.height + CARD_GAP;
        let line_top = name_top + NAME * LINE_HEIGHT + CARD_GAP;
        let thumb_radius = SHARE_THUMB_RADIUS as f32;
        let face = Stack::with_children([
            at(
                container(Space::new().width(thumb.width).height(thumb.height)).style(move |_| {
                    container::Style {
                        background: Some(THUMB_FILL.get().into()),
                        border: iced::Border {
                            color: THUMB_RIM.get(),
                            width: BORDER,
                            radius: thumb_radius.into(),
                        },
                        ..container::Style::default()
                    }
                }),
                inside,
            ),
            at(
                said_in(name.clone(), NAME, MANROPE_SEMIBOLD, TEXT.get()),
                Point::new(inside.x, name_top),
            ),
            at(
                said_in(line.clone(), LINE, MANROPE, LINE_INK.get()),
                Point::new(inside.x, line_top),
            ),
        ])
        .width(card.rect.width)
        .height(card.rect.height);
        let pressed = button(face)
            .padding(0)
            .style(move |_, status| {
                if selected {
                    button::Style {
                        background: Some(CHOSEN.get().into()),
                        border: iced::Border {
                            color: CHOSEN_RING.get(),
                            width: BORDER,
                            radius: CARD_RADIUS.into(),
                        },
                        ..style::clear()
                    }
                } else if style::lit(status) {
                    style::hovered(CARD_HOVER.get(), CARD_RADIUS)
                } else {
                    style::clear()
                }
            })
            .on_press(Msg::Card(card.index));
        all.push(at(
            control(
                pressed,
                id_of(&format!("Card({})", card.index)),
                Role::ListItem,
                name.clone(),
            )
            .node(|n| n.describe(line.clone()).selected(selected)),
            card.rect.position(),
        ));
        said.push(if selected {
            format!("({name} · {line})")
        } else {
            format!("{name} · {line}")
        });
    }
    // Show the pointer.
    let on = chooser.pointer;
    let check = button(
        row![
            drawn(CHECK, CHECK, move |f, size| {
                checkbox(f, Rectangle::new(Point::ORIGIN, size), on);
            }),
            Space::new().width(CHECK_GAP),
            said_in(words.pointer(), CHECK_TEXT, MANROPE, CHECK_INK.get()),
        ]
        .align_y(Vertical::Center),
    )
    .padding(0)
    .width(layout.check.width)
    .height(layout.check.height)
    .style(|_, _| style::clear())
    .on_press(Msg::Pointer);
    all.push(at(
        control(check, id_of("Pointer"), Role::CheckBox, words.pointer()).node(|n| n.checked(on)),
        layout.check.position(),
    ));
    said.push(format!(
        "{} {}",
        if on { "[x]" } else { "[ ]" },
        words.pointer()
    ));
    // The buttons.
    let cancel = button(
        container(said_in(
            words.cancel(),
            BUTTON_TEXT,
            MANROPE_SEMIBOLD,
            TEXT.get(),
        ))
        .center(Length::Fill),
    )
    .padding(0)
    .width(layout.cancel.width)
    .height(layout.cancel.height)
    .style(|_, status| {
        let lit = style::lit(status);
        button::Style {
            background: (lit && !crate::theme::contrast()).then(|| CANCEL_HOVER.get().into()),
            border: iced::Border {
                color: if lit && crate::theme::contrast() {
                    DIM.get()
                } else {
                    CANCEL_RIM.get()
                },
                width: BORDER,
                radius: BUTTON_RADIUS.into(),
            },
            ..style::clear()
        }
    })
    .on_press(Msg::Cancel);
    all.push(at(
        control(cancel, id_of("Cancel"), Role::Button, words.cancel()),
        layout.cancel.position(),
    ));
    let share = button(
        container(said_in(
            words.share(),
            BUTTON_TEXT,
            MANROPE_BOLD,
            Color::WHITE,
        ))
        .center(Length::Fill),
    )
    .padding(0)
    .width(layout.share.width)
    .height(layout.share.height)
    .style(|_, _| style::filled(BLUE.get(), BUTTON_RADIUS, Color::WHITE))
    .on_press(Msg::Share);
    all.push(at(
        control(share, id_of("Share"), Role::Button, words.share()),
        layout.share.position(),
    ));
    said.push(words.cancel().to_owned());
    said.push(words.share().to_owned());
    let content = mouse_area(
        Stack::with_children(all)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_scroll(Msg::Wheel)
    .into();
    (content, said)
}

fn said_in<'a>(
    text: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
    font: iced::Font,
    colour: Color,
) -> iced::widget::Text<'a> {
    style::said(text, size, font, colour)
}

/// Its background: the mockup's panel (opaque without glass) and its rim.
#[derive(Debug, Clone, Copy)]
struct Tint(bool);

impl<M> canvas::Program<M> for Tint {
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
        let size = bounds.size();
        let radius = SHARE_CHOOSER_RADIUS as f32;
        let (top, bottom) = if self.0 { TINT } else { OPAQUE };
        let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top.get())
            .add_stop(1.0, bottom.get());
        f.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, radius.into()),
            canvas::Fill::from(tint),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (radius - BORDER / 2.0).into(),
            ),
            Stroke::default().with_color(RIM.get()).with_width(BORDER),
        );
        vec![f.into_geometry()]
    }
}

/// A card's name and its line: a screen's and its size, a window's title
/// and its desktop.
fn card_words(words: Words, chooser: &Chooser, index: usize) -> (String, String) {
    match chooser.tab {
        Tab::Screens => chooser
            .choose
            .screens
            .get(index)
            .map(|s| {
                (
                    words.screen(s.builtin, &s.model),
                    words.size(s.width, s.height),
                )
            })
            .unwrap_or_default(),
        Tab::Windows => chooser
            .choose
            .windows
            .get(index)
            .map(|w| {
                let name = if w.title.is_empty() {
                    super::app_name(&w.app).unwrap_or_default()
                } else {
                    w.title.clone()
                };
                (name, words.desktop(w.here, w.desktop))
            })
            .unwrap_or_default(),
    }
}

fn elided(text: &str, room: f32, size: f32, font: iced::Font) -> String {
    match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => kit::elide(text, room, |t| kit::measure_in(system.raw(), t, size, font)),
        Err(_) => text.to_owned(),
    }
}

/// The application's letter tile, as the launcher's: its first letter in
/// white on the neutral gradient.
fn letter(f: &mut Frame, rect: Rectangle, name: &str) {
    let fill = gradient::Linear::new(
        rect.position(),
        Point::new(rect.x + rect.width, rect.y + rect.height),
    )
    .add_stop(0.0, LETTER_FROM.get())
    .add_stop(1.0, LETTER_TO.get());
    f.fill(&kit::rounded(rect, ICON_RADIUS), canvas::Fill::from(fill));
    let first: String = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            first,
            rect.center(),
            13.0 * Z,
            Color::WHITE,
            Alignment::Center,
        ),
    ));
}

/// The pointer's box: green and ticked when on, a rim when off.
fn checkbox(f: &mut Frame, rect: Rectangle, on: bool) {
    let radius = 3.0 * Z;
    if on {
        f.fill(&kit::rounded(rect, radius), GREEN.get());
        let k = rect.width / 16.0;
        let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
        let tick = Path::new(|b| {
            b.move_to(p(4.0, 8.5));
            b.line_to(p(7.0, 11.5));
            b.line_to(p(12.5, 5.0));
        });
        f.stroke(
            &tick,
            Stroke::default()
                .with_color(TAB_ON_INK.get())
                .with_width(2.0 * k)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round),
        );
    } else {
        f.stroke(
            &kit::rounded(
                Rectangle::new(
                    Point::new(rect.x + 0.75, rect.y + 0.75),
                    Size::new(rect.width - 1.5, rect.height - 1.5),
                ),
                radius,
            ),
            Stroke::default()
                .with_color(CANCEL_RIM.get())
                .with_width(1.5),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_is_the_mockups_zoomed_and_three_fit_a_row() {
        let h = card_height();
        // 6 + 86 + 4 + 12 × 1.366 + 4 + 10.5 × 1.366 + 6, × 1.5, and the
        // 1 px rims.
        assert!((h - 2.0 - Z * 136.735).abs() < 0.01, "{h}");
        let inner = WIDTH - 2.0 * (BORDER + PAD);
        let column = (inner - 2.0 * CELL_GAP) / 3.0;
        assert!((column / Z - 168.7).abs() < 0.5, "{column}");
    }
}
