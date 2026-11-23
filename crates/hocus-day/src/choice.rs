//! What Day.1's popover (the bar) and Settings › Calendar and tasks share
//! of choosing the day's sources (#425, #926): the eight colours a
//! calendar takes, a calendar's row as both draw it and what it writes,
//! a board's columns chosen, and why a board was refused. Pure: no bus,
//! no network, no file.

use crate::{
    config::CalendarEntry,
    edit::GoogleChoice,
    model::{Calendar, Colour, TaskBoard},
    source::{ColumnNames, SourceError},
};

/// The eight colours a calendar can take, in their order: blue, green,
/// orange, violet, pink, yellow, turquoise, grey.
pub const PALETTE: [Colour; 8] = [
    Colour([0x8F, 0xB3, 0xFF]),
    Colour([0x6C, 0xC2, 0x87]),
    Colour([0xF5, 0xA3, 0x5B]),
    Colour([0xC8, 0x9B, 0xFF]),
    Colour([0xF2, 0x8F, 0xB1]),
    Colour([0xF5, 0xC4, 0x6E]),
    Colour([0x6F, 0xD3, 0xD8]),
    Colour([0xA9, 0xB2, 0xC2]),
];

/// The colour of the eight nearest `colour` (an account's own).
pub fn nearest(colour: Colour) -> usize {
    let distance = |c: &Colour| {
        c.0.iter()
            .zip(colour.0)
            .map(|(a, b)| (i32::from(*a) - i32::from(b)).pow(2))
            .sum::<i32>()
    };
    PALETTE
        .iter()
        .enumerate()
        .min_by_key(|(_, c)| distance(c))
        .map_or(0, |(i, _)| i)
}

/// Colour `index` of the eight (past them, the last).
pub fn colour(index: usize) -> Colour {
    PALETTE
        .get(index)
        .or(PALETTE.last())
        .copied()
        .unwrap_or(Colour::DEFAULT)
}

/// A calendar as it is chosen: shown (its box), its colour among the
/// eight, featured (its star: its events feed the meeting card, the
/// reminders, the halo and `Super+J`), read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub colour: usize,
    pub shown: bool,
    pub featured: bool,
    pub read_only: bool,
}

impl Row {
    /// `calendar` as its account gives it (ticked as the account marks
    /// it, its star lit), or as `entry` says over it.
    pub fn of(calendar: &Calendar, entry: Option<&CalendarEntry>) -> Self {
        let mut calendar = calendar.clone();
        if let Some(entry) = entry {
            entry.over().apply(&mut calendar);
        }
        Self {
            colour: nearest(calendar.colour),
            name: calendar.name,
            shown: calendar.shown,
            featured: calendar.featured,
            read_only: calendar.read_only,
        }
    }
}

/// `account`'s entry with `rows`: the ticked shown, those of them with
/// their star lit featured, every colour by name.
pub fn google(account: &str, rows: &[Row]) -> GoogleChoice {
    GoogleChoice {
        account: account.to_owned(),
        shown: rows
            .iter()
            .filter(|r| r.shown)
            .map(|r| r.name.clone())
            .collect(),
        featured: rows
            .iter()
            .filter(|r| r.shown && r.featured)
            .map(|r| r.name.clone())
            .collect(),
        colours: rows
            .iter()
            .map(|r| (r.name.clone(), colour(r.colour)))
            .collect(),
    }
}

/// A board's three choices: the tasks' column, the done one, where `+`
/// adds; each a column's index.
pub const CHOICES: usize = 3;

/// A board's columns at first: TODO by name, else the first; DONE, else
/// the last; adding into the tasks' column.
pub fn first_columns(board: &TaskBoard) -> [usize; CHOICES] {
    let named = |name: &str| {
        board
            .columns
            .iter()
            .position(|c| c.name.trim().eq_ignore_ascii_case(name))
    };
    let tasks = named("TODO").unwrap_or(0);
    let done = named("DONE").unwrap_or(board.columns.len().saturating_sub(1));
    [tasks, done, tasks]
}

/// The columns `names` (the configuration's) choose in `board`, those it
/// does not find as [`first_columns`] has them.
pub fn columns_named(board: &TaskBoard, names: &ColumnNames) -> [usize; CHOICES] {
    let first = first_columns(board);
    let named = |name: &str, or: usize| {
        board
            .columns
            .iter()
            .position(|c| {
                !name.trim().is_empty() && c.name.trim().eq_ignore_ascii_case(name.trim())
            })
            .unwrap_or(or)
    };
    let tasks = named(&names.tasks, first[0]);
    [
        tasks,
        named(&names.done, first[1]),
        named(&names.add, tasks),
    ]
}

/// `which` of `columns` set to `column`: adding follows the tasks' column
/// until it is chosen apart.
pub fn set_column(columns: &mut [usize; CHOICES], which: usize, column: usize) {
    if which == 0 && columns[2] == columns[0] {
        columns[2] = column;
    }
    if let Some(chosen) = columns.get_mut(which) {
        *chosen = column;
    }
}

/// Why a board was not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The token is refused or expired.
    Token,
    /// No Planka server answers at the address.
    NoServer,
    /// No such board, or not shared with the user.
    NotShared,
    /// Not a board's address.
    Address,
}

/// What `error`, from [`crate::planka::find`], says to the user.
pub fn refusal(error: &SourceError) -> Refusal {
    match error {
        SourceError::SignIn(_) => Refusal::Token,
        SourceError::Gone | SourceError::Refused(_) => Refusal::NotShared,
        SourceError::Invalid(why)
            if why.contains("address") || why.contains("board's id") || why.contains("https") =>
        {
            Refusal::Address
        }
        SourceError::Offline(_) | SourceError::Invalid(_) | SourceError::Expired => {
            Refusal::NoServer
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BoardId, Column, ColumnId};

    fn board(names: &[&str]) -> TaskBoard {
        TaskBoard {
            id: BoardId::new("1"),
            name: "Sprint".to_owned(),
            columns: names
                .iter()
                .map(|n| Column {
                    id: ColumnId::new(*n),
                    name: (*n).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_nearest_of_the_eight() {
        assert_eq!(nearest(Colour([0x16, 0xA7, 0x65])), 1);
        assert_eq!(nearest(Colour([0xF6, 0x91, 0xB2])), 4);
        assert_eq!(nearest(PALETTE[6]), 6);
    }

    #[test]
    fn the_columns_named_and_adding_following_the_tasks() {
        let sprint = board(&["Backlog", "TODO", "Doing", "Done"]);
        assert_eq!(first_columns(&sprint), [1, 3, 1]);
        let names = ColumnNames {
            tasks: "doing".to_owned(),
            done: "Done".to_owned(),
            add: String::new(),
        };
        let mut columns = columns_named(&sprint, &names);
        assert_eq!(columns, [2, 3, 2]);
        // Adding follows the tasks' column, until chosen apart.
        set_column(&mut columns, 0, 0);
        assert_eq!(columns, [0, 3, 0]);
        set_column(&mut columns, 2, 1);
        set_column(&mut columns, 0, 2);
        assert_eq!(columns, [2, 3, 1]);
    }

    #[test]
    fn a_rows_choice_features_only_the_shown() {
        let row = |name: &str, shown, featured| Row {
            name: name.to_owned(),
            colour: 9,
            shown,
            featured,
            read_only: false,
        };
        let chosen = google(
            "alex@example.org",
            &[row("Travail", true, true), row("Congés", false, true)],
        );
        assert_eq!(chosen.shown, ["Travail"]);
        assert_eq!(chosen.featured, ["Travail"]);
        // A colour out of the eight is the last.
        assert_eq!(chosen.colours[1].1, PALETTE[7]);
    }

    #[test]
    fn the_columns_are_todo_and_done_by_name_else_the_first_and_the_last() {
        let board = |names: &[&str]| TaskBoard {
            id: BoardId::new("1"),
            name: "Sprint".to_owned(),
            columns: names
                .iter()
                .map(|n| Column {
                    id: ColumnId::new(*n),
                    name: (*n).to_owned(),
                })
                .collect(),
        };
        assert_eq!(
            first_columns(&board(&["Backlog", "todo", "Done"])),
            [1, 2, 1]
        );
        assert_eq!(first_columns(&board(&["A", "B", "C"])), [0, 2, 0]);
        assert_eq!(first_columns(&board(&[])), [0, 0, 0]);
    }

    #[test]
    fn a_refusal_says_what_went_wrong() {
        assert_eq!(refusal(&SourceError::SignIn(String::new())), Refusal::Token);
        assert_eq!(refusal(&SourceError::Gone), Refusal::NotShared);
        assert_eq!(
            refusal(&SourceError::Offline(String::new())),
            Refusal::NoServer
        );
        assert_eq!(
            refusal(&SourceError::Invalid(
                "sent elsewhere (302), not Planka's API".to_owned()
            )),
            Refusal::NoServer
        );
        assert_eq!(
            refusal(&SourceError::Invalid(
                "x: an address is https://…".to_owned()
            )),
            Refusal::Address
        );
    }
}
