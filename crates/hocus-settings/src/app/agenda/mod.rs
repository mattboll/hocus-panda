//! Settings › Calendar and tasks (#926, mockup s31-agenda): the calendars
//! and the tasks' board the bar shows, chosen at any time.
//!
//! - **AGENDAS**: a group per Google account `[day]` names, its state
//!   (up to date, or what is wrong and what fixes it), then its calendars
//!   as Day.1's step 3 draws them (box, swatch unfolding the eight, star,
//!   name, "lecture seule"); Online Accounts' Google accounts not yet in
//!   the bar, each with "Brancher"; none at all: the row that opens Online
//!   Accounts.
//! - **TÂCHES**: the Planka board and its three columns' menus, "Changer
//!   de tableau…" unfolding the address and the token in place; none:
//!   "Sur cet ordinateur" and "Brancher un tableau Planka…".
//!
//! Each change is written at once by the file's worker, as Day.1 writes
//! it ([`hocus_day::edit`]), toasted with "Annuler"; the shell starts its
//! accounts again when the compositor sees `[day]` change. Online Accounts,
//! the Calendar API, Planka and the keyring are asked on blocking tasks
//! ([`crate::agenda`]), never the window's thread.

mod view;

use hocus_words::settings::agenda as words;
use std::{collections::BTreeMap, time::Instant};

use hocus_day::{
    choice::{self, CHOICES, Refusal, Row},
    config::{CalendarEntry, CalendarKind, Config, TaskEntry, TaskKind},
    edit::{Choice, PlankaChoice},
    goa::GoogleAccount,
    model::Calendar,
    planka,
    secrets::Token,
};
use iced::{Task, futures::channel::mpsc::UnboundedSender};

use super::{App, Doing, KeysSaid, Message};
use crate::agenda::{self as model, Board, Checked};

/// The longest token pasted, in characters (Day.1's).
const LONGEST_TOKEN: usize = 4096;

/// Online Accounts, as last seen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Online {
    #[default]
    Unknown,
    /// Not on the bus, or no bus.
    Missing,
    Listed(Vec<GoogleAccount>),
}

/// A Google account's calendars, as the page read them.
#[derive(Debug, Clone, PartialEq)]
pub enum Listing {
    Reading,
    Failed,
    Read {
        calendars: Vec<Calendar>,
        at: Instant,
    },
}

/// The board `[day]` sets, as read.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum BoardRead {
    #[default]
    Unknown,
    Reading,
    Read(Board),
}

/// "Changer de tableau…" unfolded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Form {
    pub address: String,
    pub token: String,
    pub checking: bool,
    pub refused: Option<Refusal>,
    /// A server's address alone: the user's boards there, to choose.
    pub boards: Option<(planka::Server, Vec<planka::Listed>)>,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    to: Option<UnboundedSender<Message>>,
    watch: Option<hocus_day::goa::Watch>,
    online: Online,
    /// Each account's calendars, by its e-mail in lower case.
    read: BTreeMap<String, Listing>,
    /// The account being added ("Brancher"), its calendars read first.
    plugging: Option<String>,
    /// The swatch unfolded: its account and its calendar.
    unfolded: Option<(String, usize)>,
    /// The column menu open.
    menu: Option<usize>,
    board: BoardRead,
    /// The board read, by what `[day]` names it.
    board_of: Option<String>,
    local: Option<usize>,
    form: Option<Form>,
    /// The page was opened: Online Accounts is followed, the board read.
    opened: bool,
}

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    Accounts(Option<Vec<GoogleAccount>>),
    Calendars(String, Result<Vec<Calendar>, String>),
    Retry(String),
    Board(String, Option<Board>),
    Local(usize),
    Toggle(String, usize),
    Feature(String, usize),
    Unfold(String, usize),
    Colour(String, usize, usize),
    Plug(String),
    Remove(String),
    Panel,
    Menu(usize),
    Column(usize, usize),
    ChangeBoard,
    CancelBoard,
    Address(String),
    Token(String),
    SignIn,
    Pick(usize),
    Checked(Box<Checked>),
    RemoveBoard,
}

impl State {
    pub fn with(to: &UnboundedSender<Message>) -> Self {
        Self {
            to: Some(to.clone()),
            ..Self::default()
        }
    }
}

/// A connected account: its entry and what Online Accounts says of it.
pub(super) struct Connected<'a> {
    pub entry: &'a CalendarEntry,
    pub account: &'a str,
    pub online: Option<&'a GoogleAccount>,
}

/// `account` (an e-mail) as a key.
fn key(account: &str) -> String {
    account.trim().to_lowercase()
}

impl App {
    /// The day's sources as the file sets them.
    pub(super) fn agenda_config(&self) -> Config {
        self.snapshot
            .as_ref()
            .map(|s| (*s.day).clone())
            .unwrap_or_default()
    }

    /// The page shows: Online Accounts followed (once), the board and the
    /// local list read.
    pub(super) fn agenda_open(&mut self) -> Task<Message> {
        if self.agenda.watch.is_none()
            && let Some(to) = self.agenda.to.clone()
        {
            match model::watch(move |seen| {
                let _ = to.unbounded_send(Message::Agenda(Msg::Accounts(seen)));
            }) {
                Some(watch) => self.agenda.watch = Some(watch),
                None => self.agenda.online = Online::Missing,
            }
        }
        let local = Task::perform(
            async {
                tokio::task::spawn_blocking(model::local_tasks)
                    .await
                    .unwrap_or_default()
            },
            |n| Message::Agenda(Msg::Local(n)),
        );
        Task::batch([local, self.agenda_board_read(true)])
    }

    /// The configuration's board read again, when it names another (or
    /// `again`).
    fn agenda_board_read(&mut self, again: bool) -> Task<Message> {
        let config = self.agenda_config();
        let named = config
            .tasks
            .first()
            .filter(|t| t.kind == TaskKind::Planka)
            .map(|t| format!("{} {}", t.address, t.board));
        if named == self.agenda.board_of && !again {
            return Task::none();
        }
        self.agenda.board_of.clone_from(&named);
        let Some(named) = named else {
            self.agenda.board = BoardRead::Unknown;
            return Task::none();
        };
        self.agenda.board = BoardRead::Reading;
        let file = self.file.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || model::board(&config, file.as_deref()))
                    .await
                    .ok()
                    .flatten()
            },
            move |read| Message::Agenda(Msg::Board(named.clone(), read)),
        )
    }

    /// The Google accounts `[day]` names, in its order.
    pub(super) fn agenda_connected(&self) -> Vec<Connected<'_>> {
        let listed: &[GoogleAccount] = match &self.agenda.online {
            Online::Listed(listed) => listed,
            _ => &[],
        };
        self.snapshot
            .as_ref()
            .map(|s| {
                s.day
                    .calendars
                    .iter()
                    .filter(|e| e.kind == CalendarKind::Google && !e.account.trim().is_empty())
                    .map(|entry| Connected {
                        entry,
                        account: entry.account.trim(),
                        online: listed.iter().find(|a| {
                            a.identity.eq_ignore_ascii_case(entry.account.trim())
                                || a.name.eq_ignore_ascii_case(entry.account.trim())
                        }),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Online Accounts' Google accounts, their calendar on, not in the bar.
    pub(super) fn agenda_others(&self) -> Vec<&GoogleAccount> {
        let Online::Listed(listed) = &self.agenda.online else {
            return Vec::new();
        };
        let connected = self.agenda_connected();
        listed
            .iter()
            .filter(|a| a.calendar)
            .filter(|a| {
                !connected
                    .iter()
                    .any(|c| c.online.is_some_and(|o| o.path == a.path))
            })
            .collect()
    }

    /// An account's calendars as rows: as read, with its entry over them;
    /// else as its entry names them.
    pub(super) fn agenda_rows(&self, connected: &Connected<'_>) -> Vec<Row> {
        if let Some(Listing::Read { calendars, .. }) = self.agenda.read.get(&key(connected.account))
        {
            return calendars
                .iter()
                .map(|c| Row::of(c, Some(connected.entry)))
                .collect();
        }
        // Not read (the account needs signing in again): what the file
        // says, still to be changed.
        let entry = connected.entry;
        let mut names: Vec<String> = entry.shown.clone().unwrap_or_default();
        for name in entry.colors.keys() {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                names.push(name.clone());
            }
        }
        let has = |list: &Option<Vec<String>>, name: &str| {
            list.as_ref()
                .is_none_or(|l| l.iter().any(|n| n.eq_ignore_ascii_case(name)))
        };
        names
            .into_iter()
            .map(|name| Row {
                colour: entry
                    .colors
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(&name))
                    .map_or(0, |(_, c)| choice::nearest(*c)),
                shown: has(&entry.shown, &name),
                featured: has(&entry.featured, &name),
                read_only: false,
                name,
            })
            .collect()
    }

    /// The board `[day]` sets, if a Planka one.
    pub(super) fn agenda_board(&self) -> Option<TaskEntry> {
        self.snapshot
            .as_ref()
            .and_then(|s| s.day.tasks.first().cloned())
    }

    /// The configuration's columns chosen in the board read.
    pub(super) fn agenda_columns(&self) -> Option<(&planka::FoundBoard, [usize; CHOICES])> {
        let entry = self.agenda_board()?;
        let BoardRead::Read(Board::Read(found)) = &self.agenda.board else {
            return None;
        };
        Some((found, choice::columns_named(&found.board, &entry.columns())))
    }

    pub(super) fn agenda_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Accounts(seen) => {
                self.agenda.online = match seen {
                    Some(listed) => Online::Listed(listed),
                    None => Online::Missing,
                };
                tracing::debug!(online = ?self.agenda.online, "Online Accounts as the page sees it");
                self.agenda_read_missing()
            }
            Msg::Calendars(account, read) => {
                let read = match read {
                    Ok(calendars) => {
                        tracing::info!(calendars = calendars.len(), "an account's calendars read");
                        Listing::Read {
                            calendars,
                            at: Instant::now(),
                        }
                    }
                    Err(err) => {
                        tracing::info!(err, "an account's calendars do not read");
                        Listing::Failed
                    }
                };
                let plugged = self.agenda.plugging.as_deref() == Some(account.as_str());
                self.agenda.read.insert(account.clone(), read);
                if plugged {
                    self.agenda.plugging = None;
                    self.agenda_plugged(&account);
                }
                Task::none()
            }
            Msg::Retry(account) => self.agenda_read(&account),
            Msg::Board(named, read) => {
                if self.agenda.board_of.as_deref() == Some(named.as_str()) {
                    self.agenda.board = read.map_or(BoardRead::Unknown, BoardRead::Read);
                }
                Task::none()
            }
            Msg::Local(n) => {
                self.agenda.local = Some(n);
                Task::none()
            }
            Msg::Toggle(account, i) => {
                self.agenda_change(&account, i, model::CALENDARS, |row, lang| {
                    row.shown = !row.shown;
                    if row.shown {
                        words::shown_done(lang, &row.name)
                    } else {
                        words::hidden_done(lang, &row.name)
                    }
                });
                Task::none()
            }
            Msg::Feature(account, i) => {
                self.agenda_change(&account, i, model::CALENDARS, |row, lang| {
                    row.featured = !row.featured;
                    if row.featured {
                        words::featured_done(lang, &row.name)
                    } else {
                        words::unfeatured_done(lang, &row.name)
                    }
                });
                Task::none()
            }
            Msg::Unfold(account, i) => {
                let what = Some((account, i));
                self.agenda.unfolded = if self.agenda.unfolded == what {
                    None
                } else {
                    what
                };
                Task::none()
            }
            Msg::Colour(account, i, colour) => {
                self.agenda.unfolded = None;
                self.agenda_change(&account, i, model::COLOURS, |row, lang| {
                    row.colour = colour;
                    words::colour_done(lang, &row.name)
                });
                Task::none()
            }
            Msg::Plug(account) => {
                tracing::info!("a Google account added to the bar");
                self.agenda.plugging = Some(key(&account));
                self.agenda_read(&account)
            }
            Msg::Remove(account) => {
                let lang = self.lang;
                tracing::info!("a Google account taken out of the bar");
                self.agenda.unfolded = None;
                let said = said(words::removed_done(lang, &account), lang, model::CALENDARS);
                self.ask_day(Choice::NoGoogle(account), said, Doing::New, None);
                Task::none()
            }
            Msg::Panel => {
                model::open_panel();
                Task::none()
            }
            Msg::Menu(which) => {
                self.agenda.menu = if self.agenda.menu == Some(which) {
                    None
                } else {
                    Some(which)
                };
                Task::none()
            }
            Msg::Column(which, column) => {
                self.agenda.menu = None;
                self.agenda_column(which, column);
                Task::none()
            }
            Msg::ChangeBoard => {
                self.agenda.form = Some(Form::default());
                iced::widget::operation::focus(view::ADDRESS)
            }
            Msg::CancelBoard => {
                self.agenda.form = None;
                Task::none()
            }
            Msg::Address(text) => {
                if let Some(form) = &mut self.agenda.form {
                    form.address = text.replace(['\n', '\r'], " ");
                    form.refused = None;
                }
                Task::none()
            }
            Msg::Token(text) => {
                if let Some(form) = &mut self.agenda.form {
                    form.token = text
                        .chars()
                        .filter(|c| !matches!(c, '\n' | '\r'))
                        .take(LONGEST_TOKEN)
                        .collect();
                    form.refused = None;
                }
                Task::none()
            }
            Msg::SignIn => self.agenda_sign_in(None),
            Msg::Pick(i) => self.agenda_sign_in(Some(i)),
            Msg::Checked(checked) => {
                self.agenda_checked(*checked);
                Task::none()
            }
            Msg::RemoveBoard => {
                let lang = self.lang;
                let name = match &self.agenda.board {
                    BoardRead::Read(Board::Read(found)) => found.board.name.clone(),
                    _ => self.agenda_board().map(|b| b.address).unwrap_or_default(),
                };
                tracing::info!("the board taken out of the bar");
                let said = said(
                    words::removed_done(lang, words::board_removed(lang, &name)),
                    lang,
                    model::BOARD,
                );
                self.ask_day(Choice::NoBoard, said, Doing::New, None);
                Task::none()
            }
        }
    }

    /// After each message: the page opened the first time, or the file
    /// read again naming another board or account.
    pub(super) fn agenda_after(&mut self) -> Task<Message> {
        if self.page != super::Page::Section(crate::model::SectionId::Agenda) {
            return Task::none();
        }
        if !self.agenda.opened {
            if self.snapshot.is_none() {
                return Task::none();
            }
            self.agenda.opened = true;
            return Task::batch([self.agenda_open(), self.agenda_read_missing()]);
        }
        Task::batch([self.agenda_board_read(false), self.agenda_read_missing()])
    }

    /// The connected accounts' calendars not read yet, read.
    fn agenda_read_missing(&mut self) -> Task<Message> {
        let wanted: Vec<String> = self
            .agenda_connected()
            .iter()
            .filter(|c| c.online.is_some_and(|o| o.calendar && !o.attention))
            .map(|c| c.account.to_owned())
            .filter(|a| !self.agenda.read.contains_key(&key(a)))
            .collect();
        Task::batch(
            wanted
                .iter()
                .map(|a| self.agenda_read(a))
                .collect::<Vec<_>>(),
        )
    }

    /// `account`'s calendars read, off the window's thread.
    fn agenda_read(&mut self, account: &str) -> Task<Message> {
        let Online::Listed(listed) = &self.agenda.online else {
            return Task::none();
        };
        let Some(online) = listed
            .iter()
            .find(|a| {
                a.identity.eq_ignore_ascii_case(account) || a.name.eq_ignore_ascii_case(account)
            })
            .cloned()
        else {
            return Task::none();
        };
        let id = key(account);
        self.agenda.read.insert(id.clone(), Listing::Reading);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || model::calendars(&online))
                    .await
                    .unwrap_or_else(|err| Err(err.to_string()))
            },
            move |read| Message::Agenda(Msg::Calendars(id.clone(), read)),
        )
    }

    /// "Brancher": the account's calendars, read, written as Day.1 would
    /// at first (ticked as the account marks them, every star lit).
    fn agenda_plugged(&mut self, account: &str) {
        let Some(Listing::Read { calendars, .. }) = self.agenda.read.get(account) else {
            return;
        };
        let identity = match &self.agenda.online {
            Online::Listed(listed) => listed
                .iter()
                .find(|a| key(&a.identity) == account)
                .map_or_else(|| account.to_owned(), |a| a.identity.clone()),
            _ => account.to_owned(),
        };
        let rows: Vec<Row> = calendars.iter().map(|c| Row::of(c, None)).collect();
        let lang = self.lang;
        let said = said(words::plugged_done(lang, &identity), lang, model::CALENDARS);
        self.ask_day(
            Choice::Google(choice::google(&identity, &rows)),
            said,
            Doing::New,
            None,
        );
    }

    /// A calendar of `account` changed by `change`, which says it: the
    /// account's entry written whole.
    fn agenda_change(
        &mut self,
        account: &str,
        i: usize,
        row_id: &'static str,
        change: impl FnOnce(&mut Row, crate::lang::Lang) -> String,
    ) {
        let connected = self.agenda_connected();
        let Some(found) = connected.iter().find(|c| c.account == account) else {
            return;
        };
        let mut rows = self.agenda_rows(found);
        let Some(row) = rows.get_mut(i) else {
            return;
        };
        let lang = self.lang;
        let done = change(row, lang);
        let choice = Choice::Google(choice::google(account, &rows));
        self.ask_day(choice, said(done, lang, row_id), Doing::New, None);
    }

    /// Column `which` of the board set to `column`.
    fn agenda_column(&mut self, which: usize, column: usize) {
        let Some(entry) = self.agenda_board() else {
            return;
        };
        let Some((found, mut columns)) = self.agenda_columns() else {
            return;
        };
        choice::set_column(&mut columns, which, column);
        let name = |i: usize| {
            found
                .board
                .columns
                .get(i)
                .map(|c| c.name.clone())
                .unwrap_or_default()
        };
        let Ok((server, board)) = planka::address(&entry.address, &entry.board) else {
            return;
        };
        let titles = [
            hocus_words::shell::setup::TASKS_COLUMN,
            hocus_words::shell::setup::DONE_COLUMN,
            hocus_words::shell::setup::ADD_TO,
        ];
        let lang = self.lang;
        let done = words::column_done(
            lang,
            words::ColumnDone {
                column: titles.get(which).map_or("", |t| t.get(lang)),
                name: &name(column),
            },
        );
        let choice = Choice::Planka(PlankaChoice {
            address: format!("{server}/boards/{board}"),
            tasks: name(columns[0]),
            done: name(columns[1]),
            add: name(columns[2]),
        });
        self.ask_day(choice, said(done, lang, model::BOARD), Doing::New, None);
    }

    /// "Se connecter" (or a board chosen among a server's): checked, off
    /// the window's thread.
    fn agenda_sign_in(&mut self, pick: Option<usize>) -> Task<Message> {
        let Some(form) = &mut self.agenda.form else {
            return Task::none();
        };
        if form.checking || form.token.is_empty() || form.address.trim().is_empty() {
            return Task::none();
        }
        let token = Token::new(form.token.clone());
        let address = form.address.trim().to_owned();
        let picked = pick.and_then(|i| {
            let (server, boards) = form.boards.as_ref()?;
            Some((server.clone(), boards.get(i)?.id.clone()))
        });
        if pick.is_some() && picked.is_none() {
            return Task::none();
        }
        form.checking = true;
        form.refused = None;
        tracing::info!("a Planka board checked");
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || match picked {
                    Some((server, id)) => model::pick(&server, &id, &token),
                    None => model::check(&address, &token),
                })
                .await
                .unwrap_or(Checked::Refused(Refusal::NoServer))
            },
            |checked| Message::Agenda(Msg::Checked(Box::new(checked))),
        )
    }

    fn agenda_checked(&mut self, checked: Checked) {
        let Some(form) = &mut self.agenda.form else {
            return;
        };
        form.checking = false;
        match checked {
            Checked::Refused(refusal) => {
                tracing::info!(?refusal, "the board is refused");
                form.refused = Some(refusal);
            }
            Checked::Boards { server, boards } => {
                tracing::info!(boards = boards.len(), "a server's boards");
                form.boards = Some((server, boards));
            }
            Checked::Board { address, found } => {
                self.agenda.form = None;
                let columns = choice::first_columns(&found.board);
                let name = |i: usize| {
                    found
                        .board
                        .columns
                        .get(i)
                        .map(|c| c.name.clone())
                        .unwrap_or_default()
                };
                let lang = self.lang;
                let done = words::board_done(lang, &found.board.name);
                let choice = Choice::Planka(PlankaChoice {
                    address,
                    tasks: name(columns[0]),
                    done: name(columns[1]),
                    add: name(columns[2]),
                });
                // Read again once written, with the token now kept.
                self.agenda.board_of = None;
                self.ask_day(choice, said(done, lang, model::BOARD), Doing::New, None);
            }
        }
    }

    /// What the page shows, for the log (tests).
    pub(super) fn agenda_shown(&self) -> serde_json::Value {
        let accounts: Vec<serde_json::Value> = self
            .agenda_connected()
            .iter()
            .map(|c| {
                let rows: Vec<serde_json::Value> = self
                    .agenda_rows(c)
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "name": r.name,
                            "shown": r.shown,
                            "featured": r.featured,
                            "colour": r.colour,
                            "read_only": r.read_only,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "account": c.account,
                    "state": self.agenda_state(c).0,
                    "calendars": rows,
                })
            })
            .collect();
        let others: Vec<&str> = self
            .agenda_others()
            .iter()
            .map(|a| a.identity.as_str())
            .collect();
        let board = self.agenda_columns().map(|(found, columns)| {
            let names: Vec<&str> = columns
                .iter()
                .map(|&i| found.board.columns.get(i).map_or("", |c| c.name.as_str()))
                .collect();
            serde_json::json!({ "name": found.board.name, "columns": names })
        });
        serde_json::json!({
            "online": match &self.agenda.online {
                Online::Unknown => "unknown",
                Online::Missing => "missing",
                Online::Listed(_) => "listed",
            },
            "accounts": accounts,
            "others": others,
            "unfolded": self.agenda.unfolded.as_ref().map(|(a, i)| format!("{a}:{i}")),
            "menu": self.agenda.menu,
            "board": board,
            "board_set": self.agenda_board().map(|b| b.address),
            "local": self.agenda.local,
            "form": self.agenda.form.as_ref().map(|f| serde_json::json!({
                "checking": f.checking,
                "refused": f.refused.map(|r| format!("{r:?}")),
                "boards": f.boards.as_ref().map_or(0, |b| b.1.len()),
            })),
        })
    }
}

/// What the toasts say of a change: done, undone, redone.
fn said(done: String, lang: crate::lang::Lang, row: &'static str) -> KeysSaid {
    KeysSaid {
        undone: words::undone(lang, &done),
        redone: words::redone(lang, &done),
        done,
        row,
    }
}
