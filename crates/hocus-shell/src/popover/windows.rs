//! The mini form's windows' popover (#604): the windows of the bar's
//! screen's current desktop as the full form lists them over its tiles,
//! up to 3 lines (30 px: the icon, the title, the ✕) and "+ N autres"; a
//! 150 ms hover or a click on the mini item opens it, 240 px wide, the
//! popovers' glass, its bottom on the item's.
//!
//! A click on a line focuses and raises its window and the popover closes;
//! its ✕ asks the window to close, the popover staying (it follows the
//! windows as they go); "+ N" gives way to the current desktop's list.
//! `Tab`, `Shift+Tab` and the arrows go through the lines, their ✕ and
//! "+ N" once it holds the keyboard; `Enter` or `Space` acts, `Esc`
//! closes it.
//!
//! Built with the shell's widget kit ([`crate::ui`]); its lines drawn as
//! the full form's group draws them.

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use hocus_link::ToCompositor;
use iced::{
    Element, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    alignment::{Horizontal, Vertical},
    keyboard, mouse,
    widget::{Space, button, canvas, column, container, mouse_area, stack},
    window,
};

use super::{Effect, Event as Seen, Item, Pressed, event};
use crate::{
    ui::{Around, control, style},
    windows::{
        self, LINE, Line, MORE, Row, Words,
        draw::{Lit, OUT, RADIUS, close_rect, cut, draw_line, draw_more},
    },
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 240;
const BORDER: f32 = 1.0;
const PAD: f32 = 8.0;
/// The lines' sides: the popover's 12 px padding, 6 px past it.
const SIDE: f32 = BORDER + 12.0 - OUT;
const TOP: f32 = BORDER + PAD;
/// The ✕'s ring's corners.
const CLOSE_RADIUS: f32 = 6.0;

/// What it lists for bar `bar`: the group's rows, as many as there are.
fn rows_of(bar: &str) -> Vec<Row> {
    windows::fit(&windows::lines(bar), f32::INFINITY)
}

/// Its height for `rows`.
fn height_of(rows: &[Row]) -> u32 {
    (TOP + windows::rows_height(rows) + PAD + BORDER).round() as u32
}

/// Its height for bar `bar`'s windows now.
pub fn height(bar: &str) -> u32 {
    height_of(&rows_of(bar))
}

/// A line's width.
const fn width() -> f32 {
    WIDTH as f32 - 2.0 * SIDE
}

/// The windows' popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

/// What can hold the keyboard's ring, and be acted on: a line, its ✕,
/// "+ N".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Window(u64),
    Close(u64),
    More,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("windows:{self:?}")
    }
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// Whose windows it shows, and which (closing, what it showed).
    bar: Option<String>,
    rows: Vec<Row>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// The popover opens: nothing holds the ring.
pub fn opening() {
    ui(|ui| ui.focus = None);
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A line, its ✕ or "+ N" pressed, or `Enter` on it.
    Act(Target),
    /// A right click on a line: its window's menu, in the popover's place
    /// (#723).
    Menu(u64),
    /// The ring comes to it.
    Focus(Target),
    /// A press anywhere on the popover.
    Pressed,
    /// The windows changed under the view.
    Stale,
}

/// Whose windows it shows now, as the controller says (none once
/// closing).
fn bar_of(id: window::Id) -> Option<String> {
    super::with(|c| match c.open_on() {
        Some((bar, Item::Windows)) if c.is_open(id) => Some(bar.to_owned()),
        _ => None,
    })
}

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let (id, words) = (panel.id, panel.words);
    let content = build(id, words);
    let order = ui(|ui| targets(&ui.rows));
    Around::new(id, content, handle, Msg::Stale)
        .rebuild(move || build(id, words))
        .stale_when(move || {
            bar_of(id).is_some_and(|bar| {
                let rows = rows_of(&bar);
                ui(|ui| ui.bar.as_deref() != Some(bar.as_str()) || ui.rows != rows)
            })
        })
        .keys(move |pressed| arrows(pressed, &order))
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

/// What can hold the ring, in its order.
fn targets(rows: &[Row]) -> Vec<Target> {
    let mut order = Vec::new();
    for row in rows {
        match row {
            Row::Window(line) => {
                order.push(Target::Window(line.id));
                order.push(Target::Close(line.id));
            }
            Row::More(_) => order.push(Target::More),
        }
    }
    order
}

/// The arrows with nothing holding the ring: the first (down), or the
/// last (up); the kit takes them round after.
fn arrows(pressed: &Pressed, order: &[Target]) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    let down = match &pressed.key {
        Key::Named(Named::ArrowDown) => true,
        Key::Named(Named::ArrowUp) => false,
        _ => return None,
    };
    if ui(|ui| ui.focus.is_some()) {
        return None;
    }
    let target = if down { order.first() } else { order.last() };
    target.map(|t| Msg::Focus(*t))
}

/// Its content, from bar's windows now (closing, what it showed).
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    let bar = bar_of(id);
    ui(|ui| {
        if let Some(bar) = bar {
            let rows = rows_of(&bar);
            if ui.bar.as_deref() != Some(bar.as_str()) || ui.rows != rows {
                tracing::debug!(
                    bar = %bar,
                    shows = %words.said(&rows),
                    "the windows' popover shows it"
                );
                ui.bar = Some(bar);
                ui.rows = rows;
                super::refit(Item::Windows);
            }
        }
        content(words, ui)
    })
}

fn content<'a>(words: Words, ui: &Ui) -> El<'a> {
    let mut lines = column![];
    for row in &ui.rows {
        lines = lines.push(match row {
            Row::Window(line) => line_of(words, line, ui.focus),
            Row::More(n) => more(words, *n, ui.focus),
        });
    }
    container(lines)
        .padding(Padding {
            top: TOP,
            right: SIDE,
            bottom: PAD + BORDER,
            left: SIDE,
        })
        .into()
}

/// A window's line: drawn as the full form's, a button over it, its ✕ a
/// button of its own.
fn line_of<'a>(words: Words, line: &Line, focus: Option<Target>) -> El<'a> {
    let (window, close) = (Target::Window(line.id), Target::Close(line.id));
    let title = cut(line, width()).0;
    let art = canvas(LineArt {
        line: line.clone(),
        title,
    })
    .width(Length::Fill)
    .height(LINE);
    let pressed = button(Space::new().width(Length::Fill).height(LINE))
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Act(window));
    let pressed = control(pressed, window.id(), Role::ListItem, words.name(line))
        .node(|n| n.selected(line.focused))
        .focus(focus == Some(window), Msg::Focus(window), RADIUS)
        .activate(Msg::Act(window))
        .rows(0);
    let cross = close_rect(Rectangle::new(Point::ORIGIN, Size::new(width(), LINE)));
    let x = button(Space::new().width(cross.width).height(cross.height))
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Act(close));
    let x = control(x, close.id(), Role::Button, words.close(&line.title))
        .focus(focus == Some(close), Msg::Focus(close), CLOSE_RADIUS)
        .activate(Msg::Act(close))
        .rows(0);
    let x = container(x)
        .width(Length::Fill)
        .height(LINE)
        .align_x(Horizontal::Right)
        .align_y(Vertical::Center)
        .padding(Padding {
            right: width() - cross.x - cross.width,
            ..Padding::ZERO
        });
    mouse_area(stack![art, pressed, x])
        .on_right_press(Msg::Menu(line.id))
        .into()
}

/// "+ N autres".
fn more<'a>(words: Words, n: usize, focus: Option<Target>) -> El<'a> {
    let said = words.more(n);
    let art = canvas(MoreArt(said.clone()))
        .width(Length::Fill)
        .height(MORE);
    let pressed = button(Space::new().width(Length::Fill).height(MORE))
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Act(Target::More));
    let pressed = control(pressed, Target::More.id(), Role::Button, said)
        .focus(
            focus == Some(Target::More),
            Msg::Focus(Target::More),
            RADIUS,
        )
        .activate(Msg::Act(Target::More))
        .rows(0);
    stack![art, pressed].into()
}

/// What lights a line: the pointer on it (off its ✕), or on its ✕.
fn lit(cursor: mouse::Cursor, bounds: Rectangle) -> Lit {
    let at = cursor.land().position();
    let close = close_rect(bounds);
    let on_close = at.is_some_and(|at| close.contains(at));
    Lit {
        line: at.is_some_and(|at| bounds.contains(at)) && !on_close,
        close: on_close,
    }
}

/// A line as the full form draws it, lit as the pointer is.
struct LineArt {
    line: Line,
    title: String,
}

impl<M> canvas::Program<M> for LineArt {
    type State = Lit;

    fn update(
        &self,
        state: &mut Lit,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let iced::Event::Mouse(_) = event else {
            return None;
        };
        let now = lit(cursor, bounds);
        (now != *state).then(|| {
            *state = now;
            canvas::Action::request_redraw()
        })
    }

    fn draw(
        &self,
        _state: &Lit,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let local = Rectangle::new(Point::ORIGIN, bounds.size());
        let lit = lit(cursor, bounds);
        draw_line(&mut f, local, &self.line, &self.title, lit, 1.0);
        vec![f.into_geometry()]
    }
}

/// "+ N autres" as the full form draws it, lit under the pointer.
struct MoreArt(String);

impl<M> canvas::Program<M> for MoreArt {
    type State = bool;

    fn update(
        &self,
        state: &mut bool,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let iced::Event::Mouse(_) = event else {
            return None;
        };
        let now = cursor.land().is_over(bounds);
        (now != *state).then(|| {
            *state = now;
            canvas::Action::request_redraw()
        })
    }

    fn draw(
        &self,
        _state: &bool,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let local = Rectangle::new(Point::ORIGIN, bounds.size());
        draw_more(
            &mut f,
            local,
            self.0.clone(),
            cursor.land().is_over(bounds),
            1.0,
        );
        vec![f.into_geometry()]
    }
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(Target::Window(id)) => {
            tracing::debug!(window = id, "the windows' popover brings a window");
            super::send(vec![Effect::Bring(id)]);
            event(Seen::Chosen);
        }
        Msg::Act(Target::Close(id)) => {
            tracing::debug!(window = id, "the windows' popover asks a window to close");
            super::send(vec![Effect::Tell(ToCompositor::Close(id))]);
        }
        Msg::Act(Target::More) => {
            tracing::debug!("the windows' popover asks for the overview");
            super::send(vec![Effect::Tell(ToCompositor::Overview)]);
            event(Seen::Chosen);
        }
        Msg::Menu(id) => {
            let (bar, line) = ui(|ui| {
                let line = ui.rows.iter().find_map(|row| match row {
                    Row::Window(line) if line.id == id => Some(line.clone()),
                    _ => None,
                });
                (ui.bar.clone(), line)
            });
            if let (Some(bar), Some(line)) = (bar, line) {
                super::window_menu::ask(&bar, id, None, &line.title, &line.app);
            }
        }
    }
    // A line gone takes the ring with it.
    ui(|ui| {
        if ui.focus.is_some_and(|f| !targets(&ui.rows).contains(&f)) {
            ui.focus = None;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::Line;

    fn line(id: u64) -> Row {
        Row::Window(Line {
            id,
            app: String::new(),
            title: format!("w{id}"),
            ..Line::default()
        })
    }

    #[test]
    fn its_height_is_its_lines_and_plus_n_in_its_padding() {
        assert_eq!(height_of(&[line(1), line(2)]), 1 + 8 + 60 + 8 + 1);
        assert_eq!(
            height_of(&[line(1), line(2), line(3), Row::More(2)]),
            1 + 8 + 110 + 8 + 1
        );
        // The lines span the popover less 7 px each side.
        assert_eq!((SIDE, width()), (7.0, 226.0));
        assert_eq!(
            targets(&[line(1), Row::More(2)]),
            [Target::Window(1), Target::Close(1), Target::More]
        );
    }
}
