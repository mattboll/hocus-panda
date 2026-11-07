//! The tasks' popover (Day.4, variant C): every open task, from the mini
//! form's to-do item (a 150 ms hover or a click) and from the full form's
//! "+ N autres" (a click). 280 px, the popovers' glass, its top on what
//! opened it.
//!
//! Its header: "À faire" (14 px, bold), the count of open tasks (12 px,
//! `text.dim`) and a `+` as the group's. Then the field to add a task when
//! open (30 px, radius 8, 4 px above and under, 13.5 px), and up to 6
//! tasks on 30 px lines, each box in a 26 × 26 button 5 px past the
//! content's left, the title 5 px after it (13.5 px): they tick as the
//! group's do ([`crate::tasks`]). Past 6 the list scrolls (the wheel), and
//! "+ N autres" under it says how many are past the six. No task: "Rien à
//! faire." (13.5 px, `text.faint`).
//!
//! The keyboard: `Tab` goes through the `+` and each box, `Space` or
//! `Enter` acts; in the field, `Enter` adds and `Esc` closes it, a second
//! `Esc` the popover.

use hocus_link::POPOVER_WIDTH;
use iced::{
    Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Pressed, event,
    kit::{self, BORDER, Input, LEFT, Pointer, RIGHT},
};
use crate::{
    levels::draw::{DIM, FAINT, MANROPE, MANROPE_BOLD, TEXT, ring},
    tasks::{
        self, At, Key, Lines, Look, POPOVER_LINES, Words,
        draw::{plus, tick_box, title, unsaved},
    },
};

pub const WIDTH: u32 = POPOVER_WIDTH.unsigned_abs();
/// The header: 30 px from the padding's top; its title 14 px, the count
/// 12 px 6 px after it; the `+` as the group's, 4 px past the content.
const HEADER_TOP: f32 = BORDER + 12.0;
const HEADER: f32 = 30.0;
const TITLE: f32 = 14.0;
const COUNT: f32 = 12.0;
const COUNT_GAP: f32 = 6.0;
const ADD: Size = Size::new(22.0, 18.0);
const ADD_OUT: f32 = 4.0;
const ADD_RADIUS: f32 = 6.0;
/// The field: 30 px, 4 px above and under, radius 8, 9 px in, 13.5 px.
const FIELD: f32 = 30.0;
const FIELD_MARGIN: f32 = 4.0;
const FIELD_RADIUS: f32 = 8.0;
const FIELD_PAD: f32 = 9.0;
/// A line: 30 px; its box's button 26 × 26, 5 px past the content's
/// left, radius 7; the title 5 px after it, 13.5 px.
const LINE: f32 = 30.0;
const BUTTON: f32 = 26.0;
const BUTTON_OUT: f32 = 5.0;
const BUTTON_RADIUS: f32 = 7.0;
const TITLE_GAP: f32 = 5.0;
const LINE_TEXT: f32 = 13.5;
/// "+ N autres" past six: 24 px, 12.5 px.
const MORE: f32 = 24.0;
const MORE_TEXT: f32 = 12.5;

/// What can be acted on.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    Add,
    Field,
    Box(Key),
}

/// The popover laid out, from its top.
#[derive(Debug, Clone, PartialEq, Default)]
struct Laid {
    field: Option<Rectangle>,
    /// The list's viewport, and its content's height.
    list: Option<(Rectangle, f32)>,
    empty: Option<f32>,
    more: Option<(String, f32)>,
    end: f32,
}

fn add_rect() -> Rectangle {
    Rectangle::new(
        Point::new(
            RIGHT + ADD_OUT - ADD.width,
            HEADER_TOP + (HEADER - ADD.height) / 2.0,
        ),
        ADD,
    )
}

fn lay_out(lines: &Lines, field: bool, words: Words) -> Laid {
    let mut y = HEADER_TOP + HEADER;
    let field = field.then(|| {
        let rect = Rectangle::new(
            Point::new(LEFT, y + FIELD_MARGIN),
            Size::new(RIGHT - LEFT, FIELD),
        );
        y += FIELD + 2.0 * FIELD_MARGIN;
        rect
    });
    let n = lines.lines.len();
    let mut out = Laid {
        field,
        ..Laid::default()
    };
    if n == 0 {
        if field.is_none() {
            out.empty = Some(y + LINE / 2.0);
            y += LINE;
        }
    } else {
        let view = n.min(POPOVER_LINES) as f32 * LINE;
        let list = Rectangle::new(
            Point::new(LEFT - BUTTON_OUT, y),
            Size::new(RIGHT - LEFT + BUTTON_OUT, view),
        );
        out.list = Some((list, n as f32 * LINE));
        y += view;
        if n > POPOVER_LINES {
            out.more = Some((words.more(n - POPOVER_LINES), y + MORE / 2.0));
            y += MORE;
        }
    }
    out.end = y;
    out
}

/// Its height for what it shows now.
pub fn height() -> u32 {
    let lines = tasks::peek();
    let field = tasks::field().is_some_and(|f| f.at == At::Popover);
    kit::height(
        lay_out(
            &lines,
            field,
            Words::new(crate::clock::time::Lang::of_messages()),
        )
        .end,
    )
}

/// It opens: its list at its top.
pub fn opening() {
    SCROLL.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// It closes: its field with it.
pub fn closing() {
    tasks::drop_field_at(&At::Popover);
}

/// How far the list is scrolled, in lines: kept between draws, from the
/// popover's opening.
static SCROLL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

#[derive(Debug, Default)]
pub struct State {
    pointer: Pointer<usize>,
    /// What the pointer is over, and what holds the keyboard.
    over: Option<Target>,
    focus: Option<Target>,
    lines: Lines,
    laid: Laid,
    said: Option<String>,
}

impl State {
    /// The lines in the list's viewport now: their index, and their top.
    fn visible(&self) -> Vec<(usize, f32)> {
        let Some((view, _)) = self.laid.list else {
            return Vec::new();
        };
        let scroll = SCROLL.load(std::sync::atomic::Ordering::Relaxed);
        (scroll..self.lines.lines.len())
            .take(POPOVER_LINES)
            .enumerate()
            .map(|(i, index)| (index, view.y + i as f32 * LINE))
            .collect()
    }

    /// What `Tab` goes through: the `+`, then each box shown.
    fn order(&self) -> Vec<Target> {
        let mut order = vec![Target::Add];
        for (index, _) in self.visible() {
            if let Some(line) = self.lines.lines.get(index)
                && acts(line.look, &line.key)
            {
                order.push(Target::Box(line.key.clone()));
            }
        }
        order
    }

    /// What is under `at`.
    fn target(&self, at: Point) -> Option<Target> {
        if add_rect().contains(at) {
            return Some(Target::Add);
        }
        if self.laid.field.is_some_and(|f| f.contains(at)) {
            return Some(Target::Field);
        }
        self.visible().into_iter().find_map(|(index, top)| {
            let line = self.lines.lines.get(index)?;
            (acts(line.look, &line.key) && button(top).contains(at))
                .then(|| Target::Box(line.key.clone()))
        })
    }
}

/// Whether a line's box acts.
fn acts(look: Look, key: &Key) -> bool {
    look != Look::Adding && matches!(key, Key::Task(_))
}

/// A line's box's button, its line from `top`.
fn button(top: f32) -> Rectangle {
    Rectangle::new(
        Point::new(LEFT - BUTTON_OUT, top + (LINE - BUTTON) / 2.0),
        Size::new(BUTTON, BUTTON),
    )
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
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(_) => {
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
                }
                self.refresh(state);
                state.lines.moving().then(canvas::Action::request_redraw)
            }
            Input::Mouse(seen, at) => self.mouse(state, seen, at),
            Input::Key(pressed) => self.key(state, &pressed),
            Input::Nothing => None,
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
        kit::background(&mut frame, bounds.size(), self.glass);
        let f = &mut frame;
        let middle = HEADER_TOP + HEADER / 2.0;
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(
                self.words.title().to_owned(),
                Point::new(LEFT, middle),
                TITLE,
                TEXT.get(),
                Alignment::Left,
            ),
        ));
        let title_width = kit::measure(self.words.title(), TITLE, MANROPE_BOLD);
        f.fill_text(kit::text(
            state.lines.open().to_string(),
            Point::new(LEFT + title_width + COUNT_GAP, middle),
            COUNT,
            DIM.get(),
            Alignment::Left,
        ));
        let add = add_rect();
        plus(f, add, state.over == Some(Target::Add), 1.0);
        if state.focus == Some(Target::Add) {
            ring(f, add, ADD_RADIUS);
        }
        let laid = &state.laid;
        if let (Some(rect), Some(open)) = (laid.field, tasks::field()) {
            tasks::draw::field(
                f,
                rect,
                (&open.text, open.focused),
                (LINE_TEXT, FIELD_RADIUS, FIELD_PAD),
                self.words,
                1.0,
            );
        }
        if let Some(middle) = laid.empty {
            f.fill_text(kit::text(
                self.words.nothing().to_owned(),
                Point::new(LEFT, middle),
                LINE_TEXT,
                FAINT.get(),
                Alignment::Left,
            ));
        }
        if let Some((view, content)) = laid.list {
            for (index, top) in state.visible() {
                let Some(line) = state.lines.lines.get(index) else {
                    continue;
                };
                let alpha = line.going.map_or(1.0, |t| 1.0 - t);
                let target = Target::Box(line.key.clone());
                let rect = button(top);
                tick_box(
                    f,
                    rect,
                    line.look.ticked(),
                    state.over.as_ref() == Some(&target),
                    alpha,
                );
                if state.focus.as_ref() == Some(&target) {
                    ring(f, rect, BUTTON_RADIUS);
                }
                let at = Point::new(rect.x + BUTTON + TITLE_GAP, top + LINE / 2.0);
                match line.look {
                    Look::Unsaved { .. } => unsaved(f, self.words, at, alpha),
                    look => {
                        let room = RIGHT - at.x;
                        let said =
                            kit::elide(&line.title, room, |t| kit::measure(t, LINE_TEXT, MANROPE));
                        title(f, &said, at, LINE_TEXT, look.ticked(), alpha);
                    }
                }
            }
            let list = kit::List {
                view: Rectangle::new(
                    Point::new(view.x, view.y),
                    Size::new(view.width, view.height),
                ),
                content,
            };
            let scroll = SCROLL.load(std::sync::atomic::Ordering::Relaxed) as f32 * LINE;
            list.scrollbar_at(f, scroll, RIGHT + 4.0);
        }
        if let Some((said, middle)) = &laid.more {
            f.fill_text(kit::text(
                said.clone(),
                Point::new(LEFT, *middle),
                MORE_TEXT,
                FAINT.get(),
                Alignment::Left,
            ));
        }
        self.say(state);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match state.over {
            Some(Target::Field) => mouse::Interaction::Text,
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

impl Panel {
    /// What it shows, for screen readers (crate::a11y::rows): the `+`, the
    /// field, the tasks' boxes, the empty list's line, "+ N".
    fn say(&self, state: &State) {
        use crate::a11y::rows::{self, Rows};
        use hocus_a11y::{Node, Role};
        let mut nodes = vec![rows::act(
            Target::Add,
            Role::Button,
            self.words.add(),
            add_rect(),
        )];
        if let Some(rect) = state.laid.field {
            let text = tasks::field()
                .filter(|f| f.at == At::Popover)
                .map(|f| f.text);
            nodes.push(
                Node::new(
                    rows::key(Target::Field),
                    Role::TextField,
                    self.words.placeholder(),
                )
                .value(text.unwrap_or_default())
                .bounds(rows::at(rect)),
            );
        }
        for (index, top) in state.visible() {
            let Some(line) = state.lines.lines.get(index) else {
                continue;
            };
            let mut node = Node::new(
                rows::key(Target::Box(line.key.clone())),
                Role::CheckBox,
                line.title.clone(),
            )
            .checked(line.look.ticked())
            .bounds(rows::at(button(top)));
            if acts(line.look, &line.key) {
                node = node.act(hocus_a11y::Act::Click);
            }
            if matches!(line.look, Look::Unsaved { .. }) {
                node = node.describe(self.words.unsaved());
            }
            nodes.push(node);
        }
        if state.laid.empty.is_some() {
            nodes.push(rows::line(self.words.nothing()));
        }
        if let Some((more, _)) = &state.laid.more {
            nodes.push(rows::line(more.clone()));
        }
        rows::publish(
            self.id,
            Rows {
                subtitle: Some(self.words.count(state.lines.open())),
                nodes,
                order: rows::order(state.order()),
                focus: state.focus.as_ref().map(rows::key),
            },
        );
    }

    /// What it shows, read afresh while it is open.
    fn refresh(&self, state: &mut State) {
        if !super::with(|c| c.is_open(self.id)) {
            return;
        }
        let lines = tasks::lines();
        let field = tasks::field().filter(|f| f.at == At::Popover);
        state.laid = lay_out(&lines, field.is_some(), self.words);
        // The list scrolled no further than its end.
        let most = lines.lines.len().saturating_sub(POPOVER_LINES);
        if SCROLL.load(std::sync::atomic::Ordering::Relaxed) > most {
            SCROLL.store(most, std::sync::atomic::Ordering::Relaxed);
        }
        state.lines = lines;
        // Its height follows what it shows.
        super::refit(super::Item::Tasks);
        super::refit(super::Item::TasksMore);
        let rows: Vec<String> = state
            .visible()
            .into_iter()
            .filter_map(|(i, _)| state.lines.lines.get(i))
            .map(|l| {
                let name = self.words.tick(&l.title, l.look.ticked());
                match l.look {
                    Look::Unsaved { .. } => format!("{name}: {}", self.words.unsaved()),
                    _ => name,
                }
            })
            .collect();
        let said = format!(
            "count={} rows=[{}] more={} empty={} field={}",
            state.lines.open(),
            rows.join("; "),
            state.laid.more.as_ref().map_or("none", |(m, _)| m.as_str()),
            state.laid.empty.is_some(),
            field.map_or_else(|| "none".to_owned(), |f| format!("«{}»", f.text)),
        );
        if state.said.as_ref() != Some(&said) {
            tracing::debug!("the tasks' popover shows it: {said}");
            state.said = Some(said);
        }
    }

    fn act(target: Option<Target>) {
        match target {
            Some(Target::Add | Target::Field) => tasks::open_field(At::Popover),
            Some(Target::Box(key)) => tasks::toggle(&key),
            None => {}
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| state.target(at));
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                if state.over == target {
                    return None;
                }
                state.over = target;
                redraw
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                state.over = None;
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                match target {
                    Some(target) => Self::act(Some(target)),
                    // A press elsewhere in it: the field, empty, closes.
                    None => tasks::pressed_elsewhere(),
                }
                redraw
            }
            mouse::Event::WheelScrolled { delta } => {
                let lines = state.lines.lines.len();
                if lines <= POPOVER_LINES {
                    return None;
                }
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / LINE,
                };
                let most = lines - POPOVER_LINES;
                let scroll = SCROLL.load(std::sync::atomic::Ordering::Relaxed);
                let next = if y < 0.0 {
                    (scroll + 1).min(most)
                } else if y > 0.0 {
                    scroll.saturating_sub(1)
                } else {
                    scroll
                };
                SCROLL.store(next, std::sync::atomic::Ordering::Relaxed);
                state.over = at.and_then(|at| state.target(at));
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key as K, key::Named};
        if tasks::field_key(&At::Popover, pressed) {
            return Some(canvas::Action::request_redraw().and_capture());
        }
        match &pressed.key {
            K::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            K::Named(Named::Tab) => {
                let order = state.order();
                let at = state
                    .focus
                    .as_ref()
                    .and_then(|f| order.iter().position(|t| t == f));
                let places: Vec<usize> = (0..order.len()).collect();
                state.focus = kit::step(&places, at, pressed.modifiers.shift())
                    .and_then(|i| order.get(i).cloned());
            }
            K::Named(Named::Space | Named::Enter) => Self::act(state.focus.clone()),
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::Line;
    use hocus_day::model::TaskId;

    fn lines(n: usize) -> Lines {
        Lines {
            shows: true,
            lines: (0..n)
                .map(|i| Line {
                    key: Key::Task(TaskId::new(i.to_string())),
                    title: format!("Task {i}"),
                    look: Look::Open,
                    going: None,
                })
                .collect(),
        }
    }

    #[test]
    fn up_to_six_lines_of_30_px_then_the_rest_and_the_list_scrolls() {
        let words = Words::new(crate::clock::time::Lang::French);
        let three = lay_out(&lines(3), false, words);
        let (view, content) = three.list.expect("a list");
        assert_eq!((view.height, content), (90.0, 90.0));
        assert_eq!(three.more, None);
        let nine = lay_out(&lines(9), false, words);
        let (view, content) = nine.list.expect("a list");
        assert_eq!((view.height, content), (180.0, 270.0));
        assert_eq!(nine.more.as_ref().map(|m| m.0.as_str()), Some("+ 3 autres"));
        // The field first; nothing open, "Rien à faire.".
        let field = lay_out(&lines(1), true, words);
        assert_eq!(field.field.map(|f| f.height), Some(30.0));
        assert_eq!(field.list.map(|l| l.0.y), Some(HEADER_TOP + HEADER + 38.0));
        let none = lay_out(&lines(0), false, words);
        assert!(none.empty.is_some() && none.list.is_none());
    }
}
