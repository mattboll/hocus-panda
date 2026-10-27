//! The column's tasks (#255), as boards Bureau and D13 draw them: a
//! caption and its summary ("3 to do", "1 late", "4 of 4"), the tasks
//! late or due today first, each with a box to tick and its due date,
//! muted or in the danger colour when late; those done today struck
//! through after them; "All done for today." with a panda when nothing is
//! left. A field under them adds one, and a task's menu (the secondary
//! button) edits it, sets when it is due or deletes it.
//!
//! The compositor reads the tasks from their sources and gives the column
//! a [`TasksView`], made again only when they or the minute change.

use std::time::Duration;

use hocus_core::config::ClockFormat;
use hocus_ui::{
    Align, Button, ButtonKind, Canvas, Direction, Draw, Insets, Kind, Length, Look, NodeId, Paint,
    Point, Rect, Size, Style, Theme, Tree,
};

use crate::{
    i18n::Lang,
    icons,
    panda::{Character, Mood, Panda},
};

/// The tasks shown at most.
pub const ROWS: usize = 4;
const ROW: f32 = 28.0;
const FIELD: f32 = 36.0;
const PANDA: f32 = 64.0;

/// What the column shows of the tasks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TasksView {
    /// The tasks shown, [`ROWS`] at most: late ones, today's, the others,
    /// then those done today.
    pub rows: Vec<TaskRow>,
    /// Every task not done, shown or not.
    pub open: usize,
    /// Those of them late.
    pub late: usize,
    /// Those done today.
    pub done_today: usize,
    /// "All done" shows a panda.
    pub pandas: bool,
}

/// A task as the column lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    /// Which task, for what is done to it.
    pub id: String,
    pub text: String,
    pub due: Option<Due>,
    pub late: bool,
    pub done: bool,
}

/// When a task is due, as the column says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    Today,
    /// Today at this time.
    At {
        hour: u8,
        minute: u8,
    },
    Yesterday,
    Tomorrow,
    /// Within the coming week: its weekday, 0 for Monday.
    Weekday(u8),
    /// Further, or longer ago.
    Day {
        day: u8,
        month: u8,
    },
}

/// What the menu sets a task's due date to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueChoice {
    Today,
    Tomorrow,
    None,
}

/// How much of the section the column has room for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fit {
    /// Tasks shown at most.
    pub rows: usize,
    /// "All done" with its panda.
    pub panda: bool,
    /// "All done" on a card of its own, rather than beside the caption.
    pub card: bool,
    /// The field to add one; otherwise a + beside the caption.
    pub field: bool,
}

impl Fit {
    pub(crate) const MOST: Self = Self {
        rows: ROWS,
        panda: true,
        card: true,
        field: true,
    };

    /// Without the panda, while it shows.
    pub(crate) fn less_panda(self, view: &TasksView) -> Option<Self> {
        (self.panda && view.pandas && view.open == 0).then_some(Self {
            panda: false,
            ..self
        })
    }

    /// "All done" beside the caption, while on its card.
    pub(crate) fn less_card(self, view: &TasksView) -> Option<Self> {
        (self.card && view.open == 0).then_some(Self {
            card: false,
            ..self
        })
    }

    /// One task fewer, down to `least`.
    pub(crate) fn less_rows(self, view: &TasksView, least: usize) -> Option<Self> {
        let rows = self.rows.min(view.rows.len());
        (view.open > 0 && rows > least).then(|| Self {
            rows: rows - 1,
            ..self
        })
    }

    /// A + beside the caption rather than the field.
    pub(crate) fn less_field(self) -> Option<Self> {
        self.field.then_some(Self {
            field: false,
            ..self
        })
    }
}

/// What a click in the section does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Toggle(String),
    Add,
}

const WEEKDAYS_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_FR: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

impl Lang {
    /// The section's caption.
    pub fn tasks(self) -> &'static str {
        self.pick("TASKS", "TÂCHES")
    }

    /// Beside the caption: the late ones when some are, else those to do,
    /// else today's done of today's; nothing without any.
    pub fn tasks_summary(self, view: &TasksView) -> String {
        match (self, view.late, view.open, view.done_today) {
            (Self::En, late, _, _) if late > 0 => format!("{late} late"),
            (Self::Fr, late, _, _) if late > 0 => format!("{late} en retard"),
            (Self::En, _, open, _) if open > 0 => format!("{open} to do"),
            (Self::Fr, _, open, _) if open > 0 => format!("{open} à faire"),
            (_, _, _, 0) => String::new(),
            (Self::En, _, _, done) => format!("{done} of {done}"),
            (Self::Fr, _, _, done) => format!("{done} sur {done}"),
        }
    }

    pub fn all_done_today(self) -> &'static str {
        self.pick("All done for today.", "Tout est fait pour aujourd’hui.")
    }

    pub fn add_task(self) -> &'static str {
        self.pick("Add a task", "Ajouter une tâche")
    }

    /// When a task is due, short: "today", "15:00", "yesterday", "Fri",
    /// "Oct 28".
    pub fn due(self, due: Due, format: ClockFormat) -> String {
        match (self, due) {
            (_, Due::At { hour, minute }) => format.time(hour, minute),
            (Self::En, Due::Today) => "today".into(),
            (Self::Fr, Due::Today) => "auj.".into(),
            (Self::En, Due::Yesterday) => "yesterday".into(),
            (Self::Fr, Due::Yesterday) => "hier".into(),
            (Self::En, Due::Tomorrow) => "tomorrow".into(),
            (Self::Fr, Due::Tomorrow) => "demain".into(),
            (Self::En, Due::Weekday(w)) => WEEKDAYS_EN[usize::from(w % 7)].into(),
            (Self::Fr, Due::Weekday(w)) => WEEKDAYS_FR[usize::from(w % 7)].into(),
            (Self::En, Due::Day { day, month }) => {
                format!("{} {day}", MONTHS_EN[usize::from(month.clamp(1, 12) - 1)])
            }
            (Self::Fr, Due::Day { day, month }) => {
                format!("{day} {}", MONTHS_FR[usize::from(month.clamp(1, 12) - 1)])
            }
        }
    }

    /// A task's menu.
    pub fn edit_task(self) -> &'static str {
        self.pick("Edit", "Modifier")
    }

    pub fn due_today(self) -> &'static str {
        self.pick("Due today", "Pour aujourd’hui")
    }

    pub fn due_tomorrow(self) -> &'static str {
        self.pick("Due tomorrow", "Pour demain")
    }

    pub fn no_due_date(self) -> &'static str {
        self.pick("No due date", "Sans date")
    }

    pub fn delete_task(self) -> &'static str {
        self.pick("Delete", "Supprimer")
    }

    /// The launcher adding a task, or editing one.
    pub fn task_placeholder(self) -> &'static str {
        self.pick("A task to do", "Une tâche à faire")
    }

    pub fn task_hint(self, editing: bool) -> &'static str {
        match editing {
            false => self.pick(
                "Enter adds it to the tasks · Escape closes",
                "Entrée l’ajoute aux tâches · Échap ferme",
            ),
            true => self.pick(
                "Enter saves the task · Escape leaves it as it was",
                "Entrée enregistre la tâche · Échap la laisse comme elle était",
            ),
        }
    }

    /// The command field's offer, for "todo …".
    pub fn add_the_task(self, text: &str) -> String {
        match self {
            Self::En => format!("Add the task “{text}”"),
            Self::Fr => format!("Ajouter la tâche «\u{a0}{text}\u{a0}»"),
        }
    }

    pub fn task_kind(self) -> &'static str {
        self.pick("Task", "Tâche")
    }
}

/// What is typed after "todo", "task:" or "tâche :" in the command field:
/// a task to add.
pub fn typed_task(query: &str) -> Option<&str> {
    let lower = query.to_lowercase();
    let rest = ["todo:", "todo ", "task:", "tâche:", "tâche :", "tache:"]
        .iter()
        .find(|p| lower.starts_with(*p))
        .and_then(|p| query.get(p.len()..))?;
    let rest = rest.trim();
    (!rest.is_empty()).then_some(rest)
}

/// The section's room above the notifications: some, or none.
pub(crate) fn room(shown: bool) -> Style {
    Style {
        gap: 10.0,
        padding: Insets {
            bottom: if shown { 18.0 } else { 0.0 },
            ..Insets::default()
        },
        height: if shown { Length::Auto } else { Length::Px(0.0) },
        clip: true,
        ..Style::default()
    }
}

/// A line through a task done.
struct Strike;

impl Draw for Strike {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _: Duration) {
        canvas.fill_rect(rect, 0.0, theme.colors.muted);
    }

    fn size(&self) -> Size {
        Size::new(1.0, 1.0)
    }
}

/// Fills the section from `view`; what its parts do comes back.
pub(crate) fn fill(
    tree: &mut Tree,
    room: NodeId,
    theme: &Theme,
    lang: Lang,
    view: &TasksView,
    clock: ClockFormat,
    fit: Fit,
) -> Vec<(NodeId, Target)> {
    let missing = || unreachable!("adding to a node of this tree");
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };
    let row = |gap: f32| Style {
        direction: Direction::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    };
    let mut targets = Vec::new();
    let head = add(tree, room, row(8.0), Kind::Box(Look::default()));
    // The caption takes what is left: the summary and the + go to the end.
    add(
        tree,
        head,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line(lang.tasks(), theme.text.caption),
    );
    let mut summary = lang.tasks_summary(view);
    // Short of room, all done is said beside the caption.
    if view.open == 0 && !fit.card {
        summary = lang.all_done_today().to_owned();
    }
    if !summary.is_empty() {
        add(
            tree,
            head,
            Style {
                grow: 0.0,
                ..Style::default()
            },
            Kind::text_line(
                summary,
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            ),
        );
    }
    if !fit.field {
        let plus = add(
            tree,
            head,
            Style {
                width: Length::Px(24.0),
                height: Length::Px(24.0),
                grow: 0.0,
                ..Style::default()
            },
            Kind::Button(
                Button::new("", ButtonKind::Ghost, theme)
                    .with_icon(crate::column::stroked(icons::PLUS, 14.0))
                    .with_name(lang.add_task()),
            ),
        );
        targets.push((plus, Target::Add));
    }
    if view.open == 0 && fit.card {
        let chip = add(
            tree,
            room,
            Style {
                padding: Insets::symmetric(10.0, 12.0),
                ..row(12.0)
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 14.0,
                ..Look::default()
            }),
        );
        if view.pandas && fit.panda {
            add(
                tree,
                chip,
                Style {
                    width: Length::Px(PANDA),
                    height: Length::Px(PANDA),
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Panda::themed(Character::Apprentice, Mood::Done, theme)),
                    label: Some(lang.panda(Mood::Done).to_owned()),
                },
            );
        }
        add(
            tree,
            chip,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text(lang.all_done_today(), theme.text.body),
        );
    } else if view.open > 0 {
        for task in view.rows.iter().take(fit.rows.min(ROWS)) {
            let line = add(
                tree,
                room,
                Style {
                    height: Length::Px(ROW),
                    ..row(10.0)
                },
                Kind::Box(Look::default()),
            );
            tree.set_clickable(line, &task.text);
            add(
                tree,
                line,
                Style {
                    width: Length::Px(18.0),
                    height: Length::Px(18.0),
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(crate::checkbox::Checkbox(task.done)),
                    label: None,
                },
            );
            let ink = if task.done {
                Paint::Muted
            } else if task.late {
                Paint::Error
            } else {
                Paint::Text
            };
            let text = Kind::text_line(task.text.clone(), theme.text.body.with_color(ink));
            if task.done {
                // Struck through: the line as long as the text.
                let holder = add(
                    tree,
                    line,
                    Style {
                        width: Length::Fill,
                        clip: true,
                        ..row(0.0)
                    },
                    Kind::Box(Look::default()),
                );
                let words = add(
                    tree,
                    holder,
                    Style {
                        grow: 0.0,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                );
                add(tree, words, Style::default(), text);
                let middle = theme.text.body.size * theme.text.body.line_height / 2.0;
                add(
                    tree,
                    words,
                    Style {
                        at: Some(Point::new(0.0, middle.round())),
                        width: Length::Fill,
                        height: Length::Px(1.0),
                        ..Style::default()
                    },
                    Kind::Custom {
                        draw: Box::new(Strike),
                        label: None,
                    },
                );
            } else {
                add(
                    tree,
                    line,
                    Style {
                        width: Length::Fill,
                        ..Style::default()
                    },
                    text,
                );
            }
            if let Some(due) = task.due.filter(|_| !task.done) {
                add(
                    tree,
                    line,
                    Style {
                        grow: 0.0,
                        ..Style::default()
                    },
                    Kind::text_line(
                        lang.due(due, clock),
                        theme.text.caption.with_color(if task.late {
                            Paint::Error
                        } else {
                            Paint::Muted
                        }),
                    ),
                );
            }
            targets.push((line, Target::Toggle(task.id.clone())));
        }
    }
    if !fit.field {
        return targets;
    }
    let field = add(
        tree,
        room,
        Style {
            height: Length::Px(FIELD),
            padding: Insets::symmetric(0.0, 12.0),
            ..row(8.0)
        },
        Kind::Box(Look {
            radius: 10.0,
            border: Some(Paint::Border),
            ..Look::default()
        }),
    );
    tree.set_clickable(field, lang.add_task());
    add(
        tree,
        field,
        Style {
            grow: 0.0,
            ..Style::default()
        },
        Kind::Icon {
            icon: crate::column::stroked(icons::PLUS, 14.0),
            color: Paint::Muted,
            label: None,
        },
    );
    add(
        tree,
        field,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line(lang.add_task(), theme.text.body.with_color(Paint::Muted)),
    );
    targets.push((field, Target::Add));
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(open: usize, late: usize, done_today: usize) -> TasksView {
        TasksView {
            open,
            late,
            done_today,
            ..TasksView::default()
        }
    }

    #[test]
    fn the_summary_says_late_then_to_do_then_all_of_today() {
        assert_eq!(Lang::En.tasks_summary(&view(3, 1, 1)), "1 late");
        assert_eq!(Lang::Fr.tasks_summary(&view(3, 1, 1)), "1 en retard");
        assert_eq!(Lang::En.tasks_summary(&view(3, 0, 1)), "3 to do");
        assert_eq!(Lang::Fr.tasks_summary(&view(3, 0, 1)), "3 à faire");
        assert_eq!(Lang::En.tasks_summary(&view(0, 0, 4)), "4 of 4");
        assert_eq!(Lang::Fr.tasks_summary(&view(0, 0, 4)), "4 sur 4");
        assert_eq!(Lang::En.tasks_summary(&view(0, 0, 0)), "");
    }

    #[test]
    fn due_dates_read_short_in_both() {
        let h24 = ClockFormat::TwentyFour;
        assert_eq!(Lang::Fr.due(Due::Today, h24), "auj.");
        assert_eq!(Lang::Fr.due(Due::Yesterday, h24), "hier");
        assert_eq!(
            Lang::En.due(
                Due::At {
                    hour: 15,
                    minute: 0
                },
                h24
            ),
            "15:00"
        );
        assert_eq!(Lang::En.due(Due::Weekday(4), h24), "Fri");
        assert_eq!(
            Lang::Fr.due(Due::Day { day: 28, month: 10 }, h24),
            "28 oct."
        );
        assert_eq!(Lang::En.due(Due::Day { day: 28, month: 10 }, h24), "Oct 28");
    }

    #[test]
    fn todo_and_task_add_what_follows() {
        assert_eq!(typed_task("todo Buy milk"), Some("Buy milk"));
        assert_eq!(typed_task("Task: call Léa "), Some("call Léa"));
        assert_eq!(typed_task("tâche : réserver"), Some("réserver"));
        assert_eq!(typed_task("todo"), None);
        assert_eq!(typed_task("todoist"), None);
        assert_eq!(typed_task("task manager"), None);
    }
}
