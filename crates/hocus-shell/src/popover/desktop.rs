//! A desktop's list of windows (bar.md §4.4, §9; the 3.9b mockup), opened
//! by resting on its tile (or its dot) for 150 ms, or for the current
//! desktop by a click on the windows' "+ N autres" over the tiles (#604): "Bureau N" and "M
//! fenêtres" on the right; one 32 px row per window, front to back, its
//! application's icon (else its letter on a colour) and its title cut
//! with an ellipsis; then the minimised ones, greyed (3.9c); "Aucune
//! fenêtre." on an empty desktop. 240 px wide, its top on the tiles'.
//! Another screen's desktop (8.4) says which under its title: "sur
//! l'écran externe 34 pouces" (12 px, `text.faint`).
//! Past 8 rows it scrolls, as the Wi-Fi's list: the wheel, a touchpad,
//! the row holding the keyboard kept in view.
//!
//! A click on a row (or `Enter`, `Space` on the one holding the keyboard)
//! goes to that desktop and focuses that window, back from minimised, and
//! the list closes. `Tab`, `Shift+Tab` and the arrows go through the rows
//! once it holds the keyboard (clicked); `Esc` closes it. It follows
//! windows opening, closing and retitling while it shows: the
//! compositor's lines draw this surface alone ([`crate::desktops`]).
//!
//! Built with the shell's widget kit ([`crate::ui`]), as the Wi-Fi's.

use hocus_words::shell::popover_desktop as txt;
use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use iced::{
    Element, Length, Padding, Point, Rectangle, Size,
    advanced::widget::Id,
    alignment::Vertical,
    keyboard,
    widget::{Space, column, container, keyed_column, mouse_area, row, space},
    window,
};

use super::{
    Effect, Event as Seen, Item, Pressed, event,
    kit::{self, FILL, List, SMALL_RADIUS},
};
use crate::{
    desktops::{self, Row},
    levels::{
        draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT},
        model::Words,
    },
    ui::{
        self, Around, Later, control,
        parts::{self, drawn, gap},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 ------------------------------------------------

/// Its width, narrower than the other popovers'.
pub const WIDTH: u32 = 240;
const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 10.0;
const PAD_SIDE: f32 = 12.0;
const PAD_BOTTOM: f32 = 12.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = WIDTH as f32 - BORDER - PAD_SIDE;
/// The header: "Bureau N" 14 px 700, "M fenêtres" 12 px; 26 px high, 4 px
/// above what follows.
const HEADER_TOP: f32 = BORDER + PAD_TOP;
const HEADER: f32 = 26.0;
const TITLE: f32 = 14.0;
const COUNT: f32 = 12.0;
const GAP: f32 = 4.0;
const CONTENT_TOP: f32 = HEADER_TOP + HEADER + GAP;
/// Another screen's line under the title: 12 px, `text.faint`, 18 px high.
const SCREEN: f32 = 12.0;
const SCREEN_LINE: f32 = 18.0;

/// Where the content starts: under the screen's line when there is one.
fn content_top(screen: bool) -> f32 {
    CONTENT_TOP + if screen { SCREEN_LINE } else { 0.0 }
}
/// "Aucune fenêtre.": 13 px, 2 px above and under; then the rows' (empty)
/// place, 4 px under it, as the mockup's column has it.
const EMPTY: f32 = 13.0;
const EMPTY_PAD: f32 = 2.0;
/// The rows: 32 px, 2 px apart, reaching 6 px past the content's sides;
/// their icon 16 px 6 px in, 9 px before their title (13.5 px).
const ROW: f32 = 32.0;
const ROW_GAP: f32 = 2.0;
const ROW_OUT: f32 = 6.0;
const ROW_PAD: f32 = 6.0;
const ICON: f32 = 16.0;
const ICON_RADIUS: f32 = 4.0;
const ICON_GAP: f32 = 9.0;
const NAME: f32 = 13.5;
/// An icon's letter, when the application has none.
const LETTER: f32 = 9.0;
/// The room a title has: from after the icon to the row's padding.
const TITLE_ROOM: f32 = RIGHT + ROW_OUT - ROW_PAD - (LEFT - ROW_OUT + ROW_PAD + ICON + ICON_GAP);
/// A minimised window's icon (3.9c).
const MINIMIZED_ICON: f32 = 0.5;
/// The rows shown at most; past them, the list scrolls (3.9c).
const MOST: usize = 8;
/// A notch of a mouse's wheel scrolls two rows, as the Wi-Fi's.
const NOTCH: f32 = 2.0 * (ROW + ROW_GAP);
/// A scrolled list's rows end this far left of the content's right, the
/// scrollbar beside them, as the Wi-Fi's.
const SCROLLED_END: f32 = 2.0;

/// The rows' height, `rows` long.
fn rows_height(rows: usize) -> f32 {
    (rows as f32 * (ROW + ROW_GAP) - ROW_GAP).max(0.0)
}

/// Where its content ends, `rows` windows long: 8 rows at most show.
fn end(rows: usize, top: f32) -> f32 {
    match rows {
        0 => top + EMPTY_PAD + EMPTY * LINE + EMPTY_PAD + GAP,
        n => top + rows_height(n.min(MOST)),
    }
}

/// The list of `rows` windows: its viewport and its rows' height.
fn list_of(rows: usize, top: f32) -> List {
    List {
        view: Rectangle::new(
            Point::new(LEFT - ROW_OUT, top),
            Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, rows_height(rows.min(MOST))),
        ),
        content: rows_height(rows),
    }
}

/// The room a title has in a list that scrolls or not.
fn title_room(scrolls: bool) -> f32 {
    if scrolls {
        TITLE_ROOM - ROW_OUT - SCROLLED_END
    } else {
        TITLE_ROOM
    }
}

/// Its height for `rows` windows.
fn height_of(rows: usize, top: f32) -> u32 {
    (end(rows, top) + PAD_BOTTOM + BORDER).round() as u32
}

/// Its height for desktop `index` of bar `bar`'s row `row` (its screen's,
/// 8.4) as it is now.
pub fn height(bar: &str, row: u8, index: u8) -> u32 {
    let output = desktops::output_of(bar, row);
    height_of(
        desktops::rows(&output, usize::from(index)).len(),
        content_top(row > 0),
    )
}

/// A desktop's list on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
}

/// What it shows: whose, its rows, its title.
#[derive(Debug, Clone, PartialEq)]
struct Source {
    bar: String,
    item: Item,
    /// The bar's row (its screen, 8.4) and the desktop.
    row: u8,
    index: u8,
    /// Another screen's: the line saying which.
    screen: Option<String>,
    rows: Vec<Row>,
    /// Its name (6.14) or "Bureau N", whole.
    title: String,
}

/// What it shows now, as the controller and the compositor say (none
/// once closing).
fn source(id: window::Id, words: Words) -> Option<Source> {
    let (bar, item) = super::with(|c| match c.open_on() {
        Some((bar, item @ (Item::Desktop { .. } | Item::WindowsMore))) if c.is_open(id) => {
            Some((bar.to_owned(), item))
        }
        _ => None,
    })?;
    // The windows' "+ N" (#604): the current desktop's.
    let (row, index) = match item {
        Item::Desktop { row, index } => (row, index),
        _ => (0, u8::try_from(desktops::current(&bar)).unwrap_or(u8::MAX)),
    };
    let shown_screen = desktops::screen_of(&bar, row);
    let output = shown_screen
        .as_ref()
        .map_or_else(|| bar.clone(), |s| s.output.clone());
    let screen = shown_screen
        .filter(|_| row > 0)
        .map(|s| words.on_screen(s.built_in, s.inches, s.number));
    let rows = desktops::rows(&output, usize::from(index));
    let title = words.desktop_title(
        usize::from(index) + 1,
        desktops::name(usize::from(index)).as_deref(),
    );
    Some(Source {
        bar,
        item,
        row,
        index,
        screen,
        rows,
        title,
    })
}

/// A row, by its window's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Window(u64),
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("desktop:{self:?}")
    }
}

const LIST_ID: &str = "desktop:list";

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// What it showed last: closing, it shows it still.
    shown: Option<Source>,
    /// What was last said in the log.
    said: Option<Source>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// The list opens: no row holds the ring.
pub fn opening() {
    ui(|ui| ui.focus = None);
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A row pressed, or `Enter` on it: its window.
    Act(Target),
    /// A right click on a row: its window's menu, in the list's place
    /// (#723).
    Menu(Target),
    /// The ring comes to it.
    Focus(Target),
    /// A press anywhere on the popover.
    Pressed,
    /// What it shows changed under the view.
    Stale,
}

/// What waits for the tree built after a message: the list scrolled.
static LATER: Later = Later::new();

/// The list on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let (id, words) = (panel.id, panel.words);
    let content = build(id, words);
    let (ids, subtitle) = ui(|ui| {
        let rows = ui.shown.as_ref().map_or(&[][..], |s| &s.rows[..]);
        let ids: Vec<u64> = rows.iter().map(|r| r.id).collect();
        (
            ids,
            ui.shown.as_ref().and_then(|s| words.windows(s.rows.len())),
        )
    });
    Around::new(id, content, handle, Msg::Stale)
        .rebuild(move || build(id, words))
        .stale_when(move || {
            source(id, words).is_some_and(|now| ui(|ui| ui.shown.as_ref() != Some(&now)))
        })
        .keys(move |pressed| arrows(pressed, &ids))
        .press(Msg::Pressed)
        .later(&LATER)
        .subtitle(subtitle)
        .on_glass(panel.glass)
}

/// Its content, from what it shows now (closing, what it showed).
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    let now = source(id, words);
    ui(|ui| {
        if now.is_some() {
            ui.shown = now;
        }
        let shown = ui.shown.clone();
        content(words, shown.as_ref(), ui)
    })
}

/// The arrows with no row holding the ring: the first (down), or the
/// last (up); the kit takes them round the rows after.
fn arrows(pressed: &Pressed, ids: &[u64]) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    let down = match &pressed.key {
        Key::Named(Named::ArrowDown) => true,
        Key::Named(Named::ArrowUp) => false,
        _ => return None,
    };
    if ui(|ui| ui.focus.is_some()) {
        return None;
    }
    let id = if down { ids.first() } else { ids.last() };
    id.map(|id| Msg::Focus(Target::Window(*id)))
}

fn content<'a>(words: Words, shown: Option<&Source>, ui: &mut Ui) -> El<'a> {
    let mut all: Vec<El<'a>> = Vec::new();
    if let Some(shown) = shown {
        let count = words.windows(shown.rows.len()).unwrap_or_default();
        let list = list_of(shown.rows.len(), content_top(shown.screen.is_some()));
        let room = title_room(list.scrolls());
        let titles: Vec<String> = shown
            .rows
            .iter()
            .map(|r| kit::elide(&r.title, room, |t| kit::measure(t, NAME, MANROPE)))
            .collect();
        say(words, shown, &count, &titles, ui);
        // A long name keeps 8 px before the count.
        let counted = kit::measure(&count, COUNT, MANROPE);
        let header_room = RIGHT - LEFT - if count.is_empty() { 0.0 } else { counted + 8.0 };
        let title = kit::elide(&shown.title, header_room, |t| {
            kit::measure(t, TITLE, MANROPE_BOLD)
        });
        let line = row![
            said(title, TITLE, MANROPE_BOLD, TEXT.get()),
            space::horizontal(),
            said(count, COUNT, MANROPE, FAINT.get()),
        ]
        .align_y(Vertical::Center);
        all.push(parts::inset(
            container(line).height(HEADER).align_y(Vertical::Center),
        ));
        if let Some(screen) = &shown.screen {
            let line = container(said(screen.clone(), SCREEN, MANROPE, FAINT.get()))
                .height(SCREEN_LINE)
                .align_y(Vertical::Center);
            all.push(ui::line(parts::inset(line), screen.clone()).into());
        }
        all.push(Space::new().height(GAP).into());
        if shown.rows.is_empty() {
            let empty = container(said(words.no_window(), EMPTY, MANROPE, FAINT.get()))
                .padding(Padding {
                    top: EMPTY_PAD,
                    ..Padding::ZERO
                })
                .height(EMPTY_PAD + EMPTY * LINE + EMPTY_PAD + GAP);
            all.push(ui::line(parts::inset(empty), words.no_window()).into());
        } else {
            all.push(rows(words, shown, &titles, list, ui.focus));
        }
    }
    container(iced::widget::Column::with_children(all))
        .padding(Padding {
            top: HEADER_TOP,
            right: LEFT - ROW_OUT,
            bottom: PAD_BOTTOM + BORDER,
            left: LEFT - ROW_OUT,
        })
        .into()
}

/// The rows, in a list that scrolls past 8; another desktop's list from
/// its top.
fn rows<'a>(
    words: Words,
    shown: &Source,
    titles: &[String],
    list: List,
    focus: Option<Target>,
) -> El<'a> {
    let scrolls = list.scrolls();
    let mut column = column![].spacing(ROW_GAP);
    for (row, title) in shown.rows.iter().zip(titles) {
        let target = Target::Window(row.id);
        let found = desktops::icons::of(&row.app);
        let name = if row.app.is_empty() {
            row.title.clone()
        } else {
            letter_of(&row.app).to_owned()
        };
        // Minimised: greyed, its icon at half.
        let (opacity, colour) = if row.minimized {
            (MINIMIZED_ICON, FAINT)
        } else {
            (1.0, TEXT)
        };
        let icon = drawn(ICON, ICON, move |f, s| {
            kit::app_tile(
                f,
                Rectangle::new(Point::ORIGIN, s),
                (found.as_ref(), &name),
                (ICON_RADIUS, LETTER),
                opacity,
            );
        });
        let line = row![
            icon,
            gap(ICON_GAP),
            said(title.clone(), NAME, MANROPE, colour.get())
        ]
        .align_y(Vertical::Center);
        let pressed = iced::widget::button(container(line).center_y(Length::Fill))
            .padding(Padding {
                left: ROW_PAD,
                ..Padding::ZERO
            })
            .width(Length::Fill)
            .height(ROW)
            .style(|_, status| {
                if style::lit(status) {
                    style::hovered(FILL.get(), SMALL_RADIUS)
                } else {
                    style::clear()
                }
            })
            .on_press(Msg::Act(target));
        let mut said_as = row.title.clone();
        if row.minimized {
            said_as.push_str(txt::MINIMISED.get(words.lang()));
        }
        let declared = control(
            mouse_area(pressed).on_right_press(Msg::Menu(target)),
            target.id(),
            Role::ListItem,
            said_as,
        )
        .focus(focus == Some(target), Msg::Focus(target), SMALL_RADIUS)
        .activate(Msg::Act(target))
        .rows(0);
        column = column.push(container(declared).id(Id::from(target.id() + ":row")));
    }
    // The rows reach the scrollbar's 2 px short, as the canvas's.
    let rows = container(column).padding(Padding {
        right: if scrolls { SCROLLED_END } else { 0.0 },
        ..Padding::ZERO
    });
    let list = parts::list_by(rows.into(), LIST_ID, list.view.height, None, Some(NOTCH));
    // Keyed by whose it is: another desktop's starts at its top.
    let key = (u64::from(shown.row) << 8) | u64::from(shown.index);
    keyed_column([(key, list)]).into()
}

/// What it shows, said in the log as it changes.
fn say(words: Words, shown: &Source, count: &str, titles: &[String], ui: &mut Ui) {
    if ui.said.as_ref() == Some(shown) {
        return;
    }
    tracing::debug!(
        bar = %shown.bar,
        title = %shown.title,
        count = %count,
        empty = ?shown.rows.is_empty().then(|| words.no_window()),
        rows = ?titles,
        minimized = shown.rows.iter().filter(|r| r.minimized).count(),
        screen = ?shown.screen,
        "the desktop's list shows it"
    );
    ui.said = Some(shown.clone());
}

/// What an application's letter tile is named by: its id's last part
/// (`org.gnome.Nautilus`'s `Nautilus`), else what is given.
fn letter_of(name: &str) -> &str {
    name.rsplit('.')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(name)
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    let before = ui(|ui| ui.focus);
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(Target::Window(id)) => {
            tracing::debug!(window = id, "a desktop's list brings a window");
            super::send(vec![Effect::Bring(id)]);
            event(Seen::Chosen);
        }
        Msg::Menu(Target::Window(id)) => {
            let shown = ui(|ui| ui.shown.clone());
            if let Some(shown) = shown
                && let Some(row) = shown.rows.iter().find(|r| r.id == id)
            {
                super::window_menu::ask(&shown.bar, id, None, &row.title, &row.app);
            }
        }
    }
    let (focus, item) = ui(|ui| {
        let shown = ui.shown.as_ref();
        // A row gone takes the ring with it.
        if let Some(Target::Window(id)) = ui.focus
            && !shown.is_some_and(|s| s.rows.iter().any(|r| r.id == id))
        {
            ui.focus = None;
        }
        (ui.focus, shown.map(|s| s.item))
    });
    if let Some(target) = focus.filter(|f| Some(*f) != before) {
        LATER.push(ui::reveal(Id::new(LIST_ID), Id::from(target.id() + ":row")));
    }
    if let Some(item) = item {
        super::refit(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_height_follows_its_rows() {
        // The mockup's: 10 + 26 + 4 inside the rim, 12 under; 32 px rows
        // 2 px apart.
        assert_eq!(height_of(1, CONTENT_TOP), 1 + 10 + 26 + 4 + 32 + 12 + 1);
        assert_eq!(
            height_of(3, CONTENT_TOP),
            1 + 10 + 26 + 4 + 3 * 32 + 2 * 2 + 12 + 1
        );
        // Another screen's: its line under the title, 18 px.
        assert_eq!(
            height_of(1, content_top(true)),
            height_of(1, CONTENT_TOP) + 18
        );
        // Empty: its line, and the rows' empty place under it.
        assert_eq!(height_of(0, CONTENT_TOP), 80);
        assert_eq!(TITLE_ROOM, 189.0);
        // Past 8 rows, 8 show and the rest scrolls.
        assert_eq!(height_of(12, CONTENT_TOP), height_of(8, CONTENT_TOP));
        assert!(!list_of(8, CONTENT_TOP).scrolls());
        assert_eq!(
            list_of(12, CONTENT_TOP).most_scroll(),
            4.0 * (ROW + ROW_GAP)
        );
    }
}
