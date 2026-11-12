//! The tasks' popover (Day.4, variant C): every open task, from the mini
//! form's to-do item (a 150 ms hover or a click) and from the full form's
//! "+ N autres" (a click). 280 px, the popovers' glass, its top on what
//! opened it.
//!
//! Its header: "À faire" (14 px, bold), the count of open tasks (12 px,
//! `text.dim`) and a `+` as the group's. Then the field to add a task when
//! open (30 px, radius 8, 4 px above and under, 13.5 px; its text wraps
//! as it is typed, the field growing 20 px a row up to 5, then
//! scrolling), and up to 6 tasks, each box in a 26 × 26 button 5 px past
//! the content's left, the title 5 px after it (13.5 px) whole, wrapped
//! onto as many rows as it takes, 18 px apart: a line is 30 px, 18 more a
//! row past the first, the box beside the first. They tick as the group's
//! do ([`crate::tasks`]). Past 6 the list scrolls (the wheel), and "+ N
//! autres" under it says how many are past the six. No task: "Rien à
//! faire." (13.5 px, `text.faint`).
//!
//! The keyboard: `Tab` goes through the `+` and each box, `Space` or
//! `Enter` acts; in the field, `Enter` adds and `Esc` closes it, a second
//! `Esc` the popover.

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use hocus_link::POPOVER_WIDTH;
use iced::{
    Element, Length, Padding, Point, Rectangle, Size,
    alignment::Vertical,
    widget::{Id, button, column, container, row, space},
    window,
};

use super::{
    Pressed,
    kit::{self, BORDER, LEFT, RIGHT},
};
use crate::{
    field::view::{Look as FieldLook, Style},
    levels::draw::{DIM, FAINT, HOVER, MANROPE, MANROPE_BOLD, TEXT},
    tasks::{
        self, At, FIELD_ROWS, Key, Lines, Look, POPOVER_LINES, Words,
        draw::{FIELD_BORDER, FIELD_FILL, TITLE_ROW, plus, tick_box, title, title_rows, unsaved},
    },
    ui::{
        self, Around, Later, control,
        field::Field,
        parts::{drawn, gap, list_by},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

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
/// A box's button under the pointer, as the group's.
const BOX_HOVER_RADIUS: f32 = 6.0;
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
pub enum Target {
    Add,
    Field,
    Box(Key),
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(&self) -> String {
        match self {
            Self::Box(Key::Task(id)) => format!("tasks:Box({})", id.as_str()),
            Self::Box(Key::Adding(n)) => format!("tasks:Adding({n:?})"),
            target => format!("tasks:{target:?}"),
        }
    }
}

/// The list's scrollable.
const LIST_ID: &str = "tasks:list";

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

/// The room across for a title.
fn title_room() -> f32 {
    RIGHT - (LEFT - BUTTON_OUT + BUTTON + TITLE_GAP)
}

/// A title's rows: whole, wrapped.
fn rows_of(title: &str) -> Vec<String> {
    title_rows(title, title_room(), None, |t| {
        kit::measure(t, LINE_TEXT, MANROPE)
    })
    .0
}

/// A line's height for its title's `rows`: 30 px, 18 more a row past the
/// first.
fn line_height(rows: usize) -> f32 {
    LINE + rows.saturating_sub(1) as f32 * TITLE_ROW
}

/// The field's text: 13.5 px, wrapped onto 5 rows at most.
fn field_style() -> Style {
    Style::wrapped(LINE_TEXT, MANROPE, FIELD_ROWS)
}

/// The field's height if it is open here: 30 px for its first row, a row
/// more for each other in view.
fn field_height() -> Option<f32> {
    let style = field_style();
    tasks::field_height(&At::Popover, RIGHT - LEFT - 2.0 * FIELD_PAD, style)
        .map(|text| FIELD + (text - style.line).max(0.0))
}

/// `lines` laid out, `heights` tall, under the field if open (`field` px
/// tall).
fn lay_out(lines: &Lines, heights: &[f32], field: Option<f32>, words: Words) -> Laid {
    let mut y = HEADER_TOP + HEADER;
    let field = field.map(|height| {
        let rect = Rectangle::new(
            Point::new(LEFT, y + FIELD_MARGIN),
            Size::new(RIGHT - LEFT, height),
        );
        y += height + 2.0 * FIELD_MARGIN;
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
        // The first six in view, whatever their heights.
        let view: f32 = heights.iter().take(POPOVER_LINES).sum();
        let list = Rectangle::new(
            Point::new(LEFT - BUTTON_OUT, y),
            Size::new(RIGHT - LEFT + BUTTON_OUT, view),
        );
        out.list = Some((list, heights.iter().sum()));
        y += view;
        if n > POPOVER_LINES {
            out.more = Some((words.more(n - POPOVER_LINES), y + MORE / 2.0));
            y += MORE;
        }
    }
    out.end = y;
    out
}

/// The lines' heights, their titles wrapped.
fn heights(lines: &Lines) -> Vec<f32> {
    lines
        .lines
        .iter()
        .map(|l| line_height(rows_of(&l.title).len()))
        .collect()
}

/// Its height for what it shows now.
pub fn height() -> u32 {
    let lines = tasks::peek();
    kit::height(
        lay_out(
            &lines,
            &heights(&lines),
            field_height(),
            Words::new(crate::clock::time::Lang::of_messages()),
        )
        .end,
    )
}

/// It opens: nothing holds the ring, nothing said yet.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// It closes: its field with it.
pub fn closing() {
    tasks::drop_field_at(&At::Popover);
}

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// The press now is in the field.
    in_field: bool,
    said: Option<String>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);
static LATER: Later = Later::new();

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A button pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// A press in the field.
    InField,
    /// The field took the pointer or an input method.
    Took,
    /// A press anywhere on the popover.
    Pressed,
    /// The tasks changed under the view, or a line is going.
    Stale,
}

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let words = panel.words;
    Around::new(panel.id, build(words), handle, Msg::Stale)
        .rebuild(move || build(words))
        .stale_when(|| tasks::peek().moving())
        .keys(|pressed: &Pressed| tasks::field_key(&At::Popover, pressed).then_some(Msg::Stale))
        .subtitle(Some(words.count(tasks::peek().open())))
        .later(&LATER)
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

/// Its content, as the tasks are now.
fn build<'a>(words: Words) -> El<'a> {
    let lines = tasks::lines();
    let field = tasks::field().filter(|f| f.at == At::Popover);
    let laid = lay_out(&lines, &heights(&lines), field_height(), words);
    ui(|ui| {
        say(words, &lines, &laid, field.as_ref(), ui);
        content(words, &lines, &laid, field.as_ref(), ui.focus.as_ref())
    })
}

/// The field, as the tasks hold it, to the kit's field.
fn entry(with: &mut dyn FnMut(&mut crate::field::Entry)) {
    tasks::popover_entry(with);
}

fn content<'a>(
    words: Words,
    lines: &Lines,
    laid: &Laid,
    field: Option<&tasks::Field>,
    focus: Option<&Target>,
) -> El<'a> {
    // The boxes' buttons reach 5 px past the content's left; the list's
    // scrollbar 4 px past its right.
    let right = RIGHT + 4.0 + kit::SCROLLBAR;
    let add = control(
        button(drawn(ADD.width, ADD.height, |f, s| {
            plus(f, Rectangle::new(Point::ORIGIN, s), false, 1.0);
        }))
        .padding(0)
        .style(|_, status| {
            if style::lit(status) {
                style::hovered(HOVER.get(), ADD_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Act(Target::Add)),
        Target::Add.id(),
        Role::Button,
        words.add(),
    )
    .focus(
        focus == Some(&Target::Add),
        Msg::Focus(Target::Add),
        ADD_RADIUS,
    )
    .activate(Msg::Act(Target::Add));
    let header = row![
        gap(BUTTON_OUT),
        said(words.title(), TITLE, MANROPE_BOLD, TEXT.get()),
        gap(COUNT_GAP),
        said(lines.open().to_string(), COUNT, MANROPE, DIM.get()),
        space::horizontal(),
        add,
        gap(right - (RIGHT + ADD_OUT)),
    ]
    .height(HEADER)
    .align_y(Vertical::Center);
    let mut all = column![header];
    if let Some(field) = field {
        let typed = Field {
            with: Box::new(entry),
            style: field_style(),
            placeholder: words.placeholder().into(),
            look: FieldLook::of(TEXT.get(), FAINT.get()),
            focused: field.focused,
            left: FIELD_PAD,
            right: FIELD_PAD,
            on_focus: Msg::InField,
            on_took: |_| Msg::Took,
        }
        .view(laid.field.map_or(FIELD, |f| f.height));
        let boxed = container(typed).style(|_| iced::widget::container::Style {
            background: Some(FIELD_FILL.get().into()),
            border: iced::Border {
                color: FIELD_BORDER.get(),
                width: 1.0,
                radius: FIELD_RADIUS.into(),
            },
            ..iced::widget::container::Style::default()
        });
        let declared = control(
            boxed,
            Target::Field.id(),
            Role::TextField,
            words.placeholder(),
        )
        .node(|n| n.value(field.text.clone()))
        .typing();
        all = all.push(container(declared).padding(Padding {
            top: FIELD_MARGIN,
            right: right - RIGHT,
            bottom: FIELD_MARGIN,
            left: BUTTON_OUT,
        }));
    }
    if laid.empty.is_some() {
        all = all.push(ui::line(
            container(said(words.nothing(), LINE_TEXT, MANROPE, FAINT.get()))
                .padding([0.0, BUTTON_OUT])
                .height(LINE)
                .align_y(Vertical::Center),
            words.nothing(),
        ));
    }
    if let Some((view, _)) = laid.list {
        let mut rows = column![].width(Length::Fill);
        for line in &lines.lines {
            rows = rows.push(line_of(words, line, focus));
        }
        all = all.push(list_by(rows.into(), LIST_ID, view.height, None, Some(LINE)));
    }
    if let Some((more, _)) = &laid.more {
        all = all.push(ui::line(
            container(said(more.clone(), MORE_TEXT, MANROPE, FAINT.get()))
                .padding([0.0, BUTTON_OUT])
                .height(MORE)
                .align_y(Vertical::Center),
            more.clone(),
        ));
    }
    container(all)
        .padding(Padding {
            top: HEADER_TOP,
            right: WIDTH as f32 - right,
            bottom: 0.0,
            left: LEFT - BUTTON_OUT,
        })
        .into()
}

/// A task's line: its box, a button, and its title (struck through when
/// ticked, "Pas enregistré" when refused).
fn line_of<'a>(words: Words, line: &tasks::Line, focus: Option<&Target>) -> El<'a> {
    let alpha = line.going.map_or(1.0, |t| 1.0 - t);
    let ticked = line.look.ticked();
    let target = Target::Box(line.key.clone());
    let acts = acts(line.look, &line.key);
    let tick = button(drawn(BUTTON, BUTTON, move |f, s| {
        tick_box(f, Rectangle::new(Point::ORIGIN, s), ticked, false, alpha);
    }))
    .padding(0)
    .style(move |_, status| {
        if acts && style::lit(status) {
            let mut lit = style::hovered(HOVER.get(), BOX_HOVER_RADIUS);
            lit.background = lit.background.map(|b| b.scale_alpha(alpha));
            lit
        } else {
            style::clear()
        }
    })
    .on_press_maybe(acts.then(|| Msg::Act(target.clone())));
    let mut declared = control(tick, target.id(), Role::CheckBox, line.title.clone()).node(|n| {
        let n = n.checked(ticked);
        if matches!(line.look, Look::Unsaved { .. }) {
            n.describe(words.unsaved())
        } else {
            n
        }
    });
    if acts {
        declared = declared
            .focus(
                focus == Some(&target),
                Msg::Focus(target.clone()),
                BUTTON_RADIUS,
            )
            .activate(Msg::Act(target));
    } else {
        declared = declared.node(|n| n.disabled(true));
    }
    let look = line.look;
    let said = rows_of(&line.title);
    let height = line_height(said.len());
    // The first row beside the box, the others under it.
    let shown = drawn(title_room(), height, move |f, _| {
        let at = Point::new(0.0, LINE / 2.0);
        match look {
            Look::Unsaved { .. } => unsaved(f, words, at, alpha),
            look => {
                for (i, said) in said.iter().enumerate() {
                    let at = at + iced::Vector::new(0.0, i as f32 * TITLE_ROW);
                    title(f, said, at, LINE_TEXT, look.ticked(), alpha);
                }
            }
        }
    });
    let tick = container(declared).height(LINE).align_y(Vertical::Center);
    row![tick, gap(TITLE_GAP), shown]
        .height(height)
        .align_y(Vertical::Top)
        .into()
}

/// What it shows, for the log, when it changed.
fn say(words: Words, lines: &Lines, laid: &Laid, field: Option<&tasks::Field>, ui: &mut Ui) {
    let rows: Vec<String> = lines
        .lines
        .iter()
        .take(POPOVER_LINES)
        .map(|l| {
            let name = words.tick(&l.title, l.look.ticked());
            match l.look {
                Look::Unsaved { .. } => format!("{name}: {}", words.unsaved()),
                _ => name,
            }
        })
        .collect();
    // How many rows each title takes.
    let titles: Vec<String> = lines
        .lines
        .iter()
        .take(POPOVER_LINES)
        .map(|l| rows_of(&l.title).len().to_string())
        .collect();
    let said = format!(
        "count={} rows=[{}] more={} empty={} field={} titles=[{}] field_height={}",
        lines.open(),
        rows.join("; "),
        laid.more.as_ref().map_or("none", |(m, _)| m.as_str()),
        laid.empty.is_some(),
        field.map_or_else(|| "none".to_owned(), |f| format!("«{}»", f.text)),
        titles.join(","),
        laid.field.map_or(0.0, |f| f.height),
    );
    if ui.said.as_ref() != Some(&said) {
        tracing::debug!("the tasks' popover shows it: {said}");
        ui.said = Some(said);
    }
}

/// Whether a line's box acts.
fn acts(look: Look, key: &Key) -> bool {
    look != Look::Adding && matches!(key, Key::Task(_))
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    match message {
        // What the field took is in it; its view is built again.
        Msg::Stale | Msg::Took => {}
        Msg::InField => {
            ui(|ui| ui.in_field = true);
            tasks::open_field(At::Popover);
        }
        Msg::Pressed => {
            let in_field = ui(|ui| {
                ui.focus = None;
                std::mem::take(&mut ui.in_field)
            });
            if !in_field {
                tasks::pressed_elsewhere();
            }
        }
        Msg::Focus(target) => {
            LATER.push(ui::reveal(Id::new(LIST_ID), Id::from(target.id())));
            ui(|ui| ui.focus = Some(target));
        }
        Msg::Act(Target::Add | Target::Field) => tasks::open_field(At::Popover),
        Msg::Act(Target::Box(key)) => tasks::toggle(&key),
    }
    // Its height follows what it shows.
    super::refit(super::Item::Tasks);
    super::refit(super::Item::TasksMore);
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
        let lay_out = |lines: &Lines, field: bool, words| {
            let heights = vec![LINE; lines.lines.len()];
            lay_out(lines, &heights, field.then_some(FIELD), words)
        };
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

    #[test]
    fn taller_lines_the_six_first_in_view_and_a_grown_field() {
        let words = Words::new(crate::clock::time::Lang::French);
        // A title on 3 rows: 30 + 2 × 18 px.
        assert_eq!(line_height(3), 66.0);
        let heights = [30.0, 66.0, 30.0, 48.0, 30.0, 30.0, 66.0];
        let laid = lay_out(&lines(7), &heights, Some(70.0), words);
        let (view, content) = laid.list.expect("a list");
        assert_eq!((view.height, content), (234.0, 300.0));
        assert_eq!(laid.field.map(|f| f.height), Some(70.0));
        assert_eq!(view.y, HEADER_TOP + HEADER + 70.0 + 8.0);
    }
}
