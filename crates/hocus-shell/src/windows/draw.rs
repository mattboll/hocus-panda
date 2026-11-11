//! The windows' surface (#604), 14 px over the desktops' tiles (or dots).
//!
//! - **A line**: 30 px, 6 px past the group's sides (radius 8): its
//!   application's icon (18 px, else its letter on a colour), 8 px, its
//!   title (Manrope 13 px, `text`, one line, cut with "…"; whole in a
//!   tooltip after 600 ms when cut), and on the right, 4 px in, its ✕: a
//!   22 × 22 button (radius 6), a 10 px cross of 1.5 px strokes in
//!   `text.dim`; under the pointer `rgba(242,148,91,0.20)` and the cross
//!   `#F7B98A` (the only warning colour, as the draft's close). The
//!   focused window's line is lit `rgba(255,255,255,0.06)`, a line under
//!   the pointer too (`0.10` for the focused one); a minimised window's
//!   title is `text.faint` and its icon at half. The pointer is a hand.
//! - **"+ N autres"**: 20 px, 12.5 px `text.faint`, lit under the
//!   pointer: the current desktop's list.
//! - **The mini form's item**: 44 × 34, radius 10, the first line's
//!   window's icon (20 px), lit `0.06` under the pointer and `0.08` while
//!   its popover shows.
//!
//! Nothing moves: a change of focus reorders the lines at once. While the
//! form switches it holds both forms, which cross-fade as the other groups
//! do.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use hocus_a11y::{Act, Node, Role};
use hocus_link::{Form, WINDOWS_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Vector, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};

use super::{ITEM, LINE, Line, MORE, Row, Words};
use crate::theme::Ink;
use crate::{
    Message,
    levels::draw::{DIM, FAINT, HOVER, MANROPE, OPEN, TEXT, faded, rgb},
    popover::{self, Item, kit},
    ui::items::{Control, Controls, Items, Press},
};

/// The surfaces' widths: the bar inside its rim.
pub const FULL_WIDTH: u32 = 194;
pub const MINI_WIDTH: u32 = 54;
/// Around what it draws: the keyboard's ring, 2 px out and 3 px wide.
pub const ROOM: f32 = 5.0;
/// The group's side on the surface (the bar's 12 px padding, less its
/// rim), its width, and how far a line goes past it.
const SIDE: f32 = 11.0;
pub const WIDTH: f32 = 172.0;
pub const OUT: f32 = 6.0;
pub const RADIUS: f32 = 8.0;
/// A line's icon and title.
pub const ICON: f32 = 18.0;
const ICON_RADIUS: f32 = 4.0;
const LETTER: f32 = 10.0;
const ICON_GAP: f32 = 8.0;
pub const TITLE: f32 = 13.0;
/// The ✕: a 22 × 22 button 4 px in from the line's right, its cross 10 px.
const CLOSE: f32 = 22.0;
const CLOSE_IN: f32 = 4.0;
const CLOSE_RADIUS: f32 = 6.0;
const CROSS: f32 = 10.0;
const CROSS_STROKE: f32 = 1.5;
/// Between the title and the ✕.
const TITLE_END: f32 = 6.0;
const MORE_TEXT: f32 = 12.5;
pub const CLOSE_HOVER: Ink = Ink::Dark(Color::from_rgba8(242, 148, 91, 0.20));
pub const CLOSE_INK: Ink = Ink::Dark(rgb(0xF7B98A));
/// The focused window's line, and a line under the pointer.
pub const LIT: Ink = HOVER;
pub const LIT_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
/// A minimised window's icon (3.9c).
const MINIMIZED_ICON: f32 = 0.5;
/// The mini item: 44 × 34, radius 10, its icon 20 px.
const MINI: Size = Size::new(44.0, ITEM);
const MINI_RADIUS: f32 = 10.0;
const MINI_ICON: f32 = 20.0;
const MINI_ICON_RADIUS: f32 = 5.0;
const MINI_LETTER: f32 = 11.0;
/// A cut title's tooltip, after resting this long on it.
const TIP: Duration = Duration::from_millis(600);

/// Where a line's parts are, in a line at `rect`.
pub fn icon_rect(rect: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(rect.x + OUT, rect.center_y() - ICON / 2.0),
        Size::new(ICON, ICON),
    )
}

pub fn close_rect(rect: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(
            rect.x + rect.width - CLOSE_IN - CLOSE,
            rect.center_y() - CLOSE / 2.0,
        ),
        Size::new(CLOSE, CLOSE),
    )
}

/// The room a title has in a line `width` wide.
pub fn title_room(width: f32) -> f32 {
    width - OUT - ICON - ICON_GAP - TITLE_END - CLOSE - CLOSE_IN
}

/// A state's mark after a title (#723): 12 px, 4 px apart.
const MARK: f32 = 12.0;
const MARK_GAP: f32 = 4.0;

/// The room `line`'s marks take after its title.
fn marks_room(line: &Line) -> f32 {
    let n = u8::from(line.above) + u8::from(line.everywhere);
    f32::from(n) * (MARK + MARK_GAP)
}

/// `line`'s title cut to the room of a line `width` wide, its marks
/// after it, and whether it was.
pub fn cut(line: &Line, width: f32) -> (String, bool) {
    let title = line.title.as_str();
    let shown = kit::elide(title, title_room(width) - marks_room(line), |t| {
        kit::measure(t, TITLE, MANROPE)
    });
    let cut = shown != title;
    (shown, cut)
}

/// `line`'s marks from `x`, on its middle `y`: kept above (two windows,
/// the front one up), on all desktops (three panes).
fn draw_marks(f: &mut Frame, line: &Line, mut x: f32, y: f32, alpha: f32) {
    let ink = faded(DIM.get(), alpha);
    let stroke = Stroke::default().with_color(ink).with_width(1.3);
    if line.above {
        let back = Rectangle::new(Point::new(x, y - 2.0), Size::new(7.5, 6.0));
        let front = Rectangle::new(Point::new(x + 4.0, y - 5.0), Size::new(7.5, 6.0));
        f.stroke(
            &kit::rounded(back, 1.5),
            stroke.with_color(faded(DIM.get(), alpha * 0.55)),
        );
        f.fill(&kit::rounded(front, 1.5), faded(DIM.get(), alpha * 0.25));
        f.stroke(&kit::rounded(front, 1.5), stroke);
        x += MARK + MARK_GAP;
    }
    if line.everywhere {
        for i in 0..3 {
            let pane = Rectangle::new(
                Point::new(x + 0.5 + 4.0 * i as f32, y - 4.0),
                Size::new(3.0, 8.0),
            );
            f.stroke(&kit::rounded(pane, 1.0), stroke);
        }
    }
}

/// How a line's parts are lit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lit {
    /// The pointer is on the line (off its ✕).
    pub line: bool,
    /// The pointer is on its ✕.
    pub close: bool,
}

/// A window's line at `rect`, its title as cut (`title`), at `alpha`.
pub fn draw_line(f: &mut Frame, rect: Rectangle, line: &Line, title: &str, lit: Lit, alpha: f32) {
    let ground = match (line.focused, lit.line) {
        (true, true) => Some(LIT_HOVER),
        (true, false) | (false, true) => Some(LIT),
        (false, false) => None,
    };
    if let Some(ground) = ground {
        f.fill(&kit::rounded(rect, RADIUS), faded(ground.get(), alpha));
    }
    let icon = icon_rect(rect);
    let found = crate::desktops::icons::of(&line.app);
    let name = letter_of(line);
    let opacity = if line.minimized { MINIMIZED_ICON } else { 1.0 };
    kit::app_tile(
        f,
        icon,
        (found.as_ref(), name),
        (ICON_RADIUS, LETTER),
        opacity * alpha,
    );
    let colour = if line.minimized { FAINT } else { TEXT };
    let x = icon.x + ICON + ICON_GAP;
    f.fill_text(kit::text(
        title.to_owned(),
        Point::new(x, rect.center_y()),
        TITLE,
        faded(colour.get(), alpha),
        Alignment::Left,
    ));
    if line.above || line.everywhere {
        let end = x + kit::measure(title, TITLE, MANROPE) + MARK_GAP + 2.0;
        draw_marks(f, line, end, rect.center_y(), alpha);
    }
    let close = close_rect(rect);
    if lit.close {
        f.fill(
            &kit::rounded(close, CLOSE_RADIUS),
            faded(CLOSE_HOVER.get(), alpha),
        );
    }
    let ink = if lit.close { CLOSE_INK } else { DIM };
    cross(f, close.center(), faded(ink.get(), alpha));
}

/// What a window's letter tile, when its application has no icon, is
/// named by: its application id's last part (`org.gnome.Nautilus`'s
/// `Nautilus`), else its title.
fn letter_of(line: &Line) -> &str {
    if line.app.is_empty() {
        return &line.title;
    }
    line.app
        .rsplit('.')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(&line.app)
}

/// The ✕'s cross, centred on `at`.
fn cross(f: &mut Frame, at: Point, colour: Color) {
    let half = CROSS / 2.0;
    let path = Path::new(|p| {
        p.move_to(Point::new(at.x - half, at.y - half));
        p.line_to(Point::new(at.x + half, at.y + half));
        p.move_to(Point::new(at.x + half, at.y - half));
        p.line_to(Point::new(at.x - half, at.y + half));
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(colour)
            .with_width(CROSS_STROKE)
            .with_line_cap(canvas::LineCap::Round),
    );
}

/// "+ N autres" at `rect`, lit under the pointer.
pub fn draw_more(f: &mut Frame, rect: Rectangle, said: String, lit: bool, alpha: f32) {
    if lit {
        f.fill(&kit::rounded(rect, RADIUS), faded(LIT.get(), alpha));
    }
    f.fill_text(kit::text(
        said,
        Point::new(rect.x + OUT, rect.center_y()),
        MORE_TEXT,
        faded(FAINT.get(), alpha),
        Alignment::Left,
    ));
}

/// The rows' places from `top`, each `width` wide from `x`.
pub fn row_rects(rows: &[Row], x: f32, top: f32, width: f32) -> Vec<Rectangle> {
    let mut y = top;
    rows.iter()
        .map(|row| {
            let height = match row {
                Row::Window(_) => LINE,
                Row::More(_) => MORE,
            };
            let rect = Rectangle::new(Point::new(x, y), Size::new(width, height));
            y += height;
            rect
        })
        .collect()
}

/// The items of `rows` at `rects`: a line, its ✕, "+ N".
pub fn row_items(rows: &[Row], rects: &[Rectangle]) -> Vec<(Item, Rectangle)> {
    let mut items = Vec::new();
    for (row, rect) in rows.iter().zip(rects) {
        match row {
            Row::Window(line) => {
                items.push((Item::Window(line.id), *rect));
                items.push((Item::WindowClose(line.id), close_rect(*rect)));
            }
            Row::More(_) => items.push((Item::WindowsMore, *rect)),
        }
    }
    items
}

/// The windows of one bar, on their surface.
#[derive(Debug, Clone)]
pub struct View {
    pub bar: String,
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn now; the surface's right edge is the bar's
    /// inside right edge.
    pub bar_width: f32,
    pub words: Words,
    /// On the CPU, the bar's colour under it.
    pub backdrop: Option<crate::agents::draw::Backdrop>,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// The cut title under the pointer, shown whole after a rest.
    tip: Option<u64>,
    /// What it shows, as last read: its rows, their titles as cut.
    shown: super::Shown,
    titles: Vec<(String, bool)>,
}

/// What each bar's last said it shows at rest, for the log.
static SAID: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

impl View {
    /// The form that shows alone (not switching): its items take the
    /// pointer.
    fn alone(&self) -> Option<Form> {
        match self.forms.as_slice() {
            [(form, _)] => Some(*form),
            _ => None,
        }
    }

    fn alpha(&self, form: Form) -> f32 {
        self.forms
            .iter()
            .find(|(f, _)| *f == form)
            .map_or(0.0, |(_, a)| *a)
    }

    /// The group's top left on the surface.
    ///
    /// In a form alone, from the surface's bottom as it is: the group sits
    /// on the desktops, and a line keeps its place on the screen as lines
    /// come over it. A press read while the compositor still has the
    /// surface where it was finds the line it was on there, never the one
    /// the next place will put there (#617).
    fn origin(&self, size: Size, shown: &super::Shown) -> Point {
        let left = size.width + 2.0 - self.bar_width;
        let top = match self.alone() {
            Some(_) => size.height - ROOM - super::rows_height(&shown.rows),
            None => shown.full_top - shown.surface_top,
        };
        Point::new(left + SIDE, top)
    }

    /// The mini item on the surface, centred in the bar as wide as it is
    /// drawn.
    fn mini_rect(&self, size: Size, shown: &super::Shown) -> Rectangle {
        let centre = size.width + 1.0 - self.bar_width / 2.0;
        let top = match self.alone() {
            Some(_) => size.height - ROOM - MINI.height,
            None => shown.mini_top - shown.surface_top,
        };
        Rectangle::new(Point::new((centre - MINI.width / 2.0).round(), top), MINI)
    }

    /// The full form's rows' lines on the surface.
    fn rects(&self, size: Size, shown: &super::Shown) -> Vec<Rectangle> {
        let o = self.origin(size, shown);
        row_rects(&shown.rows, o.x - OUT, o.y, WIDTH + 2.0 * OUT)
    }

    /// Its items on the surface, in a form alone.
    fn items(&self, size: Size, shown: &super::Shown) -> Vec<(Item, Rectangle)> {
        match self.alone() {
            Some(Form::Full) => row_items(&shown.rows, &self.rects(size, shown)),
            Some(Form::Mini) if shown.item.is_some() => {
                vec![(Item::Windows, self.mini_rect(size, shown))]
            }
            _ => Vec::new(),
        }
    }

    /// From the surface to the bar's frame, from its bottom as its lines
    /// are.
    fn to_bar(&self, size: Size, shown: &super::Shown) -> Vector {
        Vector::new(-size.width - 1.0, shown.surface_bottom - size.height)
    }

    /// What it shows, read afresh, its titles cut once.
    fn refresh(&self, state: &mut State) {
        let shown = super::shown(&self.bar);
        if shown == state.shown && state.titles.len() == shown.rows.len() {
            return;
        }
        state.titles = shown
            .rows
            .iter()
            .map(|row| match row {
                Row::Window(line) => cut(line, WIDTH + 2.0 * OUT),
                Row::More(n) => (self.words.more(*n), false),
            })
            .collect();
        state.shown = shown;
    }

    /// What it shows, said once at rest, with its items' middles in the
    /// bar's frame (for tests, which click them).
    fn say(&self, state: &State, items: &[(Item, Rectangle)], lit: Option<Item>) {
        let Some(form) = self.alone() else {
            return;
        };
        let at: Vec<String> = items
            .iter()
            .map(|(item, r)| {
                let c = r.center();
                format!("{item:?}@{:.0},{:.0}", c.x, c.y)
            })
            .collect();
        let mut names: Vec<String> = Vec::new();
        for row in &state.shown.rows {
            match row {
                Row::Window(line) => {
                    names.push(self.words.name(line));
                    names.push(self.words.close(&line.title));
                }
                Row::More(n) => names.push(self.words.more(*n)),
            }
        }
        if state.shown.item.is_some() && form == Form::Mini {
            names.push(self.words.item(state.shown.count));
        }
        let item = state.shown.item.as_ref().map(|l| l.title.clone());
        let said = format!(
            "bar={} form={} shows={} item={:?} lit={:?} items={}",
            self.bar,
            form.name(),
            self.words.said(&state.shown.rows),
            item,
            lit,
            at.join(" ")
        );
        let mut kept = SAID.lock().unwrap_or_else(PoisonError::into_inner);
        let kept = kept.get_or_insert_with(HashMap::new);
        if kept.get(&self.bar) != Some(&said) {
            tracing::debug!(names = ?names, "the windows are drawn at rest: {said}");
            kept.insert(self.bar.clone(), said);
        }
    }

    /// A cut title under the pointer: shown whole after a rest; the mini
    /// item's title too.
    fn tip(&self, state: &mut State, found: Option<(Item, Rectangle)>) {
        let shown = &state.shown;
        let whole = match found {
            Some((Item::Window(id), rect)) => shown
                .rows
                .iter()
                .zip(&state.titles)
                .find_map(|(row, (_, cut))| match row {
                    Row::Window(line) if line.id == id && *cut => Some((id, line.title.clone())),
                    _ => None,
                })
                .map(|(id, title)| (id, title, rect)),
            Some((Item::Windows, rect)) => shown
                .item
                .as_ref()
                .map(|line| (line.id, line.title.clone(), rect)),
            _ => None,
        };
        match whole {
            Some((id, title, rect)) if state.tip != Some(id) => {
                crate::tray::tip::hover_text(&self.bar, rect, TIP, title, String::new());
                state.tip = Some(id);
            }
            None if state.tip.take().is_some() => crate::tray::tip::leave(),
            _ => {}
        }
    }

    fn group(&self, f: &mut Frame, state: &State, size: Size, alpha: f32, hover: Option<Item>) {
        let shown = &state.shown;
        let rects = self.rects(size, shown);
        for ((row, rect), (title, _)) in shown.rows.iter().zip(&rects).zip(&state.titles) {
            match row {
                Row::Window(line) => {
                    let lit = Lit {
                        line: hover == Some(Item::Window(line.id)),
                        close: hover == Some(Item::WindowClose(line.id)),
                    };
                    draw_line(f, *rect, line, title, lit, alpha);
                }
                Row::More(_) => draw_more(
                    f,
                    *rect,
                    title.clone(),
                    hover == Some(Item::WindowsMore),
                    alpha,
                ),
            }
        }
    }

    fn mini(&self, f: &mut Frame, state: &State, size: Size, alpha: f32, hover: Option<Item>) {
        let Some(line) = &state.shown.item else {
            return;
        };
        let rect = self.mini_rect(size, &state.shown);
        let open = popover::with(|c| c.shows(&self.bar, Item::Windows));
        match (open, hover == Some(Item::Windows)) {
            (true, _) => f.fill(&kit::rounded(rect, MINI_RADIUS), faded(OPEN.get(), alpha)),
            (false, true) => crate::theme::hover(
                f,
                &kit::rounded(rect, MINI_RADIUS),
                faded(HOVER.get(), alpha),
            ),
            (false, false) => {}
        }
        let icon = Rectangle::new(
            Point::new(
                (rect.center_x() - MINI_ICON / 2.0).round(),
                (rect.center_y() - MINI_ICON / 2.0).round(),
            ),
            Size::new(MINI_ICON, MINI_ICON),
        );
        let found = crate::desktops::icons::of(&line.app);
        kit::app_tile(
            f,
            icon,
            (found.as_ref(), letter_of(line)),
            (MINI_ICON_RADIUS, MINI_LETTER),
            alpha,
        );
    }
}

impl Items for View {
    type State = State;

    fn bar(&self) -> &str {
        &self.bar
    }

    fn namespace(&self) -> &'static str {
        WINDOWS_NAMESPACE
    }

    fn controls(&self, state: &State, size: Size) -> Controls {
        let shown = &state.shown;
        let said = crate::a11y::Words::new(self.words.lang);
        let line = |id: u64| {
            shown.rows.iter().find_map(|r| match r {
                Row::Window(line) if line.id == id => Some(line),
                _ => None,
            })
        };
        let lines = shown
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Window(_)))
            .count();
        let controls: Vec<Control> = self
            .items(size, shown)
            .into_iter()
            .map(|(item, rect)| match item {
                Item::Window(id) => {
                    let line = line(id);
                    let name = line.map(|l| self.words.name(l)).unwrap_or_default();
                    let focused = line.is_some_and(|l| l.focused);
                    Control::new(item, rect, Role::ListItem, name, RADIUS)
                        .node(|n| n.selected(focused).act(Act::Menu))
                }
                Item::WindowClose(id) => {
                    let name = line(id).map(|l| self.words.close(&l.title));
                    Control::new(
                        item,
                        rect,
                        Role::Button,
                        name.unwrap_or_default(),
                        CLOSE_RADIUS,
                    )
                    .part()
                }
                Item::Windows => {
                    let name = self.words.item(shown.count);
                    Control::new(item, rect, Role::Button, name, MINI_RADIUS)
                }
                _ => {
                    let name = said.more_windows(shown.count.saturating_sub(lines));
                    Control::new(item, rect, Role::Button, name, RADIUS)
                }
            })
            .collect();
        Controls {
            to_bar: self.to_bar(size, shown),
            group: (controls.len() > 1)
                .then(|| Node::new("windows", Role::List, said.windows(shown.count))),
            controls,
        }
    }

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        _bounds: Rectangle,
        _at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        if let iced::Event::Window(window::Event::RedrawRequested(_)) = event {
            self.refresh(state);
        }
        None
    }

    fn hovered(&self, state: &mut State, on: Option<(Item, Rectangle)>) {
        self.tip(state, on);
    }

    /// A right click on a line (or the mini item): its window's menu
    /// beside the bar, its top on the line's (#723).
    fn pressed(
        &self,
        state: &mut State,
        button: mouse::Button,
        on: Option<(Item, Rectangle)>,
    ) -> Press {
        if state.tip.take().is_some() {
            crate::tray::tip::leave();
        }
        if button != mouse::Button::Right {
            return Press::Kit;
        }
        let Some((item, rect)) = on else {
            return Press::Passed;
        };
        let shown = &state.shown;
        let line = match item {
            Item::Window(id) | Item::WindowClose(id) => {
                shown.rows.iter().find_map(|row| match row {
                    Row::Window(line) if line.id == id => Some(line.clone()),
                    _ => None,
                })
            }
            Item::Windows => shown.item.clone(),
            _ => None,
        };
        let Some(line) = line else {
            return Press::Passed;
        };
        popover::window_menu::ask(&self.bar, line.id, Some(rect), &line.title, &line.app);
        Press::Taken
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        bounds: Rectangle,
        lit: Option<Item>,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let shown = &state.shown;
        // Nothing to show: nothing drawn, the bar seen through.
        let empty = shown.rows.is_empty() && shown.item.is_none();
        let colour = self.backdrop.filter(|_| !empty).map(|backdrop| {
            let bar = popover::with(|c| c.bar_height(&self.bar));
            let middle = shown.surface_top + size.height / 2.0;
            backdrop.at(middle, bar)
        });
        let under = crate::backdrop::under(renderer, size, colour);
        let to_bar = self.to_bar(size, shown);
        let in_bar: Vec<(Item, Rectangle)> = self
            .items(size, shown)
            .into_iter()
            .map(|(item, r)| (item, r + to_bar))
            .collect();
        // The bar's inside, as wide as it is while it switches.
        let left = (size.width + 2.0 - self.bar_width).max(0.0);
        let column = Rectangle::new(
            Point::new(left, 0.0),
            Size::new((size.width - left).max(0.0), size.height),
        );
        frame.with_clip(column, |f| {
            for &(form, _) in &self.forms {
                let alpha = self.alpha(form);
                match form {
                    Form::Full => self.group(f, state, size, alpha, lit),
                    Form::Mini => self.mini(f, state, size, alpha, lit),
                }
            }
        });
        self.say(state, &in_bar, lit);
        crate::backdrop::over(under, frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: u64) -> Line {
        Line {
            id,
            app: String::new(),
            title: format!("w{id}"),
            ..Line::default()
        }
    }

    #[test]
    fn a_lines_icon_title_and_close_sit_in_its_30_px() {
        // The group's lines: 6 px past its 172 px.
        let rows = vec![Row::Window(line(1)), Row::Window(line(2)), Row::More(3)];
        let rects = row_rects(&rows, 5.0, 5.0, WIDTH + 2.0 * OUT);
        assert_eq!(
            rects[0],
            Rectangle::new(Point::new(5.0, 5.0), Size::new(184.0, 30.0))
        );
        assert_eq!(rects[1].y, 35.0);
        assert_eq!(
            rects[2],
            Rectangle::new(Point::new(5.0, 65.0), Size::new(184.0, 20.0))
        );
        // The icon on the group's side, the ✕ 4 px in from the line's right.
        assert_eq!(
            icon_rect(rects[0]),
            Rectangle::new(Point::new(11.0, 11.0), Size::new(18.0, 18.0))
        );
        assert_eq!(
            close_rect(rects[0]),
            Rectangle::new(Point::new(163.0, 9.0), Size::new(22.0, 22.0))
        );
        assert_eq!(
            title_room(184.0),
            184.0 - 6.0 - 18.0 - 8.0 - 6.0 - 22.0 - 4.0
        );
        // Its items: each line and its ✕, then "+ N".
        let items = row_items(&rows, &rects);
        assert_eq!(
            items.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            [
                Item::Window(1),
                Item::WindowClose(1),
                Item::Window(2),
                Item::WindowClose(2),
                Item::WindowsMore
            ]
        );
    }
}
