//! Day.1 (#425, variant B): choosing the calendar and the tasks' sources
//! from the bar, step by step, and the first-run card that offers it.
//!
//! - **The card** ([`card`]) takes the full form's day groups' place (on
//!   the to-do list's surface) while `[day]` sets no source and the user
//!   has not said "Plus tard", which is kept in
//!   `$XDG_STATE_HOME/hocuspanda/day-sources` for good.
//! - **The popover** ([`crate::popover::setup`]), from its "Choisir…":
//!   step 1 the kinds of account (Google through GNOME Online Accounts, a
//!   Planka board); step 2 a board's address and a pasted token, checked;
//!   step 3 what to show (the calendars, their colours and their stars,
//!   which feature them, or the board's columns). "Terminé" writes the
//!   choice into the configuration file the compositor names, keeps the
//!   token in the keyring (or its file), and starts the day's workers
//!   again from it.
//!
//! What blocks (Online Accounts, the network, the files) runs on a thread
//! of its own (`day: sources`), which puts what it finds in the flow and
//! wakes the popover or the card: the shell's loop never waits for it.

pub mod card;

use hocus_words::shell::setup as txt;
use std::{
    path::PathBuf,
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
};

use hocus_day::{
    SourceError,
    edit::{self, Choice, GoogleChoice, PlankaChoice},
    goa::{self, GoogleAccount},
    model::{Calendar, Colour},
    planka,
    secrets::Token,
};
use hocus_link::{Launch, TASKS_NAMESPACE, ToCompositor};

use crate::{
    clock::time::Lang,
    field::{self, Entry, Took},
    popover::{self, Effect, Item},
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
    (0..PALETTE.len())
        .min_by_key(|i| distance(&PALETTE[*i]))
        .unwrap_or(0)
}

/// The longest token pasted, in characters (the two cookies, or a
/// request's `Cookie:` line).
const LONGEST_TOKEN: usize = 4096;
/// The longest address typed or pasted.
const LONGEST_ADDRESS: usize = 512;
/// What keeps "Plus tard", in the shell's state folder.
const LATER_FILE: &str = "day-sources";

// --- The card --------------------------------------------------------------------------

/// Whether "Plus tard" was said: unknown until its file is read.
static LATER: AtomicU8 = AtomicU8::new(UNKNOWN);
const UNKNOWN: u8 = 0;
const NO: u8 = 1;
const YES: u8 = 2;

/// Whether the first-run card shows (in the full form): `[day]` sets no
/// source, and "Plus tard" was not said; not until both are known.
pub fn card_shows() -> bool {
    crate::day::sources() == Some(false) && LATER.load(Ordering::Acquire) == NO
}

/// "Plus tard": the card goes, for good.
pub fn later() {
    if LATER.swap(YES, Ordering::AcqRel) == YES {
        return;
    }
    tracing::info!("the day's first-run card: later, for good");
    ask(Job::WriteLater);
    wake_card();
}

/// The card's surfaces draw again (to their new size, as they draw).
fn wake_card() {
    popover::send(vec![Effect::Wake(TASKS_NAMESPACE)]);
}

/// From now on: "Plus tard" read, and the card shown when the day's
/// configuration is read.
pub fn start() {
    ask(Job::ReadLater);
    let mut seen = None;
    crate::day::on_change(move || {
        let now = crate::day::sources();
        if now != seen {
            seen = now;
            wake_card();
        }
        true
    });
}

// --- The popover's flow ----------------------------------------------------------------

/// Where the popover is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// 1, "Ajouter un compte".
    Accounts,
    /// 2, a Planka board's address and token.
    Planka,
    /// 3, "Quoi montrer ?".
    Show,
}

/// GNOME Online Accounts, as last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Online {
    /// Not read yet.
    Unknown,
    /// Not on the bus (or no bus).
    Missing,
    /// Its Google accounts with their calendar on.
    Listed(Vec<GoogleAccount>),
}

/// Why the board was not taken.
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

/// A calendar of step 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    /// Its colour among the eight.
    pub colour: usize,
    pub shown: bool,
    /// Its star, "Mettre en avant": its events feed the meeting card, the
    /// reminders, the halo and `Super+J` (written in `featured`).
    pub featured: bool,
    pub read_only: bool,
}

/// A Google account's calendars in step 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Calendars {
    /// Being read (that ask's number).
    Reading(u64),
    Failed,
    Read(Vec<Row>),
}

/// A board's three choices: the tasks' column, the done one, where `+`
/// adds; each a column's index.
pub const CHOICES: usize = 3;

/// What step 3 shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Show {
    Google {
        account: GoogleAccount,
        calendars: Calendars,
    },
    Planka {
        found: planka::Found,
        /// The columns chosen, once the board is read.
        columns: [usize; CHOICES],
        /// A board being read (a server's address: one of its boards
        /// chosen), that ask's number.
        reading: Option<u64>,
    },
}

/// What is unfolded under a row of step 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unfold {
    /// A calendar's colours.
    Colours(usize),
    /// A choice's columns.
    Columns(usize),
    /// The server's boards.
    Boards,
}

/// The popover's fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Address,
    Token,
}

/// The popover's steps, from its opening to its closing.
#[derive(Debug)]
pub struct Flow {
    pub step: Step,
    pub online: Online,
    /// `gnome-control-center`'s path, once looked for and found.
    pub panel: Option<String>,
    /// The board's address and its token (a secret: masked, never copied
    /// out): the shell's text fields (#771).
    pub address: Entry,
    pub token: Entry,
    /// A check under way: its number.
    pub checking: Option<u64>,
    pub refused: Option<Refusal>,
    pub show: Option<Show>,
    pub unfolded: Option<Unfold>,
    /// Asks' numbers: an answer to an older one is dropped.
    asked: u64,
}

impl Flow {
    fn new() -> Self {
        Self {
            step: Step::Accounts,
            online: Online::Unknown,
            panel: None,
            address: Entry::new(field::Kind::Line, LONGEST_ADDRESS),
            token: Entry::new(field::Kind::Secret, LONGEST_TOKEN),
            checking: None,
            refused: None,
            show: None,
            unfolded: None,
            asked: 0,
        }
    }

    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self::new()
    }

    fn next(&mut self) -> u64 {
        self.asked += 1;
        self.asked
    }

    /// Online Accounts' Google accounts to list: their calendar on.
    pub fn accounts(&self) -> Vec<&GoogleAccount> {
        match &self.online {
            Online::Listed(listed) => listed.iter().filter(|a| a.calendar).collect(),
            Online::Unknown | Online::Missing => Vec::new(),
        }
    }
}

static FLOW: Mutex<Option<Flow>> = Mutex::new(None);

/// `with` on the flow, while the popover is open.
pub fn with<T>(with: impl FnOnce(&mut Flow) -> T) -> Option<T> {
    FLOW.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
        .map(with)
}

/// The popover shows something new: drawn again, to its new size.
fn changed() {
    popover::changed(Item::DayChoose);
}

/// The popover opens: step 1, Online Accounts followed.
pub fn opening() {
    *FLOW.lock().unwrap_or_else(PoisonError::into_inner) = Some(Flow::new());
    ask(Job::Watch);
}

/// It closes: what was typed goes (the token zeroed as it drops).
pub fn closing() {
    let flow = FLOW.lock().unwrap_or_else(PoisonError::into_inner).take();
    drop(flow);
    ask(Job::Unwatch);
}

/// A Google account of Online Accounts chosen: step 3, its calendars read.
pub fn choose_google(index: usize) {
    let asked = with(|flow| {
        let account = flow.accounts().get(index).map(|a| (*a).clone())?;
        let n = flow.next();
        tracing::debug!(account = account.identity, "a Google account chosen");
        flow.step = Step::Show;
        flow.unfolded = None;
        flow.show = Some(Show::Google {
            account: account.clone(),
            calendars: Calendars::Reading(n),
        });
        Some(Job::Calendars { n, account })
    })
    .flatten();
    if let Some(job) = asked {
        ask(job);
        changed();
    }
}

/// The Online Accounts panel, to add a Google account (none listed): its
/// effect, when it is installed.
pub fn open_panel() -> Option<Effect> {
    let program = with(|flow| flow.panel.clone()).flatten()?;
    tracing::info!("Online Accounts' panel opens");
    let launch = Launch {
        id: "gnome-control-center".to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec![program, "online-accounts".to_owned()],
    };
    launch
        .fits()
        .then_some(Effect::Tell(ToCompositor::Launch(launch)))
}

/// The Planka row: step 2, what was typed there before kept.
pub fn choose_planka() {
    with(|flow| {
        flow.step = Step::Planka;
        flow.unfolded = None;
    });
    tracing::debug!("the Planka step opens");
    changed();
}

/// Back one step (the back arrow, `Esc`): whether there was one.
pub fn back() -> bool {
    let went = with(|flow| {
        if flow.unfolded.take().is_some() {
            return true;
        }
        match flow.step {
            Step::Accounts => false,
            Step::Planka => {
                flow.step = Step::Accounts;
                flow.checking = None;
                true
            }
            Step::Show => {
                flow.step = match flow.show.take() {
                    Some(Show::Planka { .. }) => Step::Planka,
                    _ => Step::Accounts,
                };
                flow.checking = None;
                true
            }
        }
    })
    .unwrap_or(false);
    if went {
        tracing::debug!("the sources' popover goes back");
        changed();
    }
    went
}

impl Flow {
    /// Field `field`.
    pub fn entry(&mut self, field: Field) -> &mut Entry {
        match field {
            Field::Address => &mut self.address,
            Field::Token => &mut self.token,
        }
    }
}

/// `field` edited by `edit` (a key, an input method, the pointer): what
/// was refused goes with what changes, a paste is asked for. What it took.
pub fn edit(field: Field, edit: impl FnOnce(&mut Entry) -> Took) -> Took {
    let took = with(|flow| {
        let took = edit(flow.entry(field));
        if took == Took::Edited {
            flow.refused = None;
        }
        took
    })
    .unwrap_or(Took::No);
    match took {
        Took::Edited => changed(),
        Took::Paste => paste(field),
        Took::No | Took::Same | Took::Moved => {}
    }
    took
}

/// The clipboard's text at the caret of `field` (`Ctrl+V`, 3.5b), off the
/// loop: on one line for the address; for the token, its line breaks
/// dropped, never in a `String` of ours but the field's.
fn paste(field: Field) {
    let clean = match field {
        Field::Address => crate::paste::Clean::Spaced,
        Field::Token => crate::paste::Clean::Dropped,
    };
    let asked = crate::paste::request(clean, move |text| {
        let Some(text) = text.filter(|t| !t.is_empty()) else {
            return;
        };
        let open = with(|flow| flow.step == Step::Planka).unwrap_or(false);
        let pasted = |entry: &mut Entry| match entry.paste(text) {
            true => Took::Edited,
            false => Took::Same,
        };
        if open && edit(field, pasted) == Took::Edited {
            tracing::debug!(?field, "the clipboard's text pasted into the Planka step");
        } else {
            tracing::debug!(
                ?field,
                "the clipboard's text does not fit in the Planka step"
            );
        }
    });
    if !asked {
        tracing::debug!("nothing to paste");
    }
}

/// "Se connecter": the address and the token checked off the loop.
pub fn sign_in() {
    let asked = with(|flow| {
        if flow.checking.is_some() || flow.address.text().trim().is_empty() || flow.token.is_empty()
        {
            return None;
        }
        let n = flow.next();
        flow.checking = Some(n);
        flow.refused = None;
        Some(Job::Check {
            n,
            address: flow.address.text().trim().to_owned(),
            token: Token::new(flow.token.text()),
        })
    })
    .flatten();
    if let Some(job) = asked {
        tracing::debug!("a Planka board checked");
        ask(job);
        changed();
    }
}

/// A calendar ticked or unticked.
pub fn toggle(index: usize) {
    with(|flow| {
        if let Some(Show::Google {
            calendars: Calendars::Read(rows),
            ..
        }) = &mut flow.show
            && let Some(row) = rows.get_mut(index)
        {
            row.shown = !row.shown;
        }
    });
    changed();
}

/// A calendar's star lit or put out: featured or not.
pub fn feature(index: usize) {
    with(|flow| {
        if let Some(Show::Google {
            calendars: Calendars::Read(rows),
            ..
        }) = &mut flow.show
            && let Some(row) = rows.get_mut(index)
        {
            row.featured = !row.featured;
        }
    });
    changed();
}

/// What unfolds under a row (or folds, if it shows).
pub fn unfold(what: Unfold) {
    with(|flow| {
        flow.unfolded = if flow.unfolded == Some(what) {
            None
        } else {
            Some(what)
        };
    });
    changed();
}

/// Calendar `index` takes colour `colour`; its row folds.
pub fn colour(index: usize, colour: usize) {
    with(|flow| {
        if let Some(Show::Google {
            calendars: Calendars::Read(rows),
            ..
        }) = &mut flow.show
            && let Some(row) = rows.get_mut(index)
        {
            row.colour = colour.min(PALETTE.len() - 1);
        }
        flow.unfolded = None;
    });
    changed();
}

/// Choice `which` takes column `column`; its menu folds. The tasks'
/// column carries "Ajout dans" with it while they were the same.
pub fn column(which: usize, column: usize) {
    with(|flow| {
        if let Some(Show::Planka { columns, .. }) = &mut flow.show
            && which < CHOICES
        {
            if which == 0 && columns[2] == columns[0] {
                columns[2] = column;
            }
            columns[which] = column;
        }
        flow.unfolded = None;
    });
    changed();
}

/// One of a server's boards chosen: read off the loop.
pub fn board(index: usize) {
    let asked = with(|flow| {
        let token = Token::new(flow.token.text());
        let n = flow.next();
        flow.unfolded = None;
        let Some(Show::Planka { found, reading, .. }) = &mut flow.show else {
            return None;
        };
        let board = found.boards.get(index)?.id.clone();
        *reading = Some(n);
        Some(Job::Board {
            n,
            server: found.server.clone(),
            board,
            token,
        })
    })
    .flatten();
    if let Some(job) = asked {
        ask(job);
        changed();
    }
}

/// "Terminé" (or ✕ in step 3): the choice written, its token kept, the
/// day's workers started again from it; the popover closes.
pub fn done() {
    let finish = with(|flow| {
        let choice = match flow.show.as_ref()? {
            Show::Google {
                account,
                calendars: Calendars::Read(rows),
            } => Choice::Google(GoogleChoice {
                account: account.identity.clone(),
                shown: rows
                    .iter()
                    .filter(|r| r.shown)
                    .map(|r| r.name.clone())
                    .collect(),
                // A star counts only on a calendar shown.
                featured: rows
                    .iter()
                    .filter(|r| r.shown && r.featured)
                    .map(|r| r.name.clone())
                    .collect(),
                colours: rows
                    .iter()
                    .map(|r| (r.name.clone(), PALETTE[r.colour]))
                    .collect(),
            }),
            Show::Planka { found, columns, .. } => {
                let board = found.board.as_ref()?;
                let name = |i: usize| {
                    board
                        .board
                        .columns
                        .get(columns[i])
                        .map(|c| c.name.clone())
                        .unwrap_or_default()
                };
                Choice::Planka(PlankaChoice {
                    address: found.address()?,
                    tasks: name(0),
                    done: name(1),
                    add: name(2),
                })
            }
            Show::Google { .. } => return None,
        };
        let token = matches!(choice, Choice::Planka(_)).then(|| Token::new(flow.token.text()));
        Some(Job::Finish { choice, token })
    })
    .flatten();
    if let Some(job) = finish {
        tracing::info!("the day's sources chosen");
        ask(job);
    }
    popover::event(popover::Event::Chosen);
}

// --- Off the loop ----------------------------------------------------------------------

/// What the sources' thread is asked.
enum Job {
    ReadLater,
    WriteLater,
    /// Online Accounts followed, and its panel looked for.
    Watch,
    Unwatch,
    Check {
        n: u64,
        address: String,
        token: Token,
    },
    Board {
        n: u64,
        server: planka::Server,
        board: String,
        token: Token,
    },
    Calendars {
        n: u64,
        account: GoogleAccount,
    },
    Finish {
        choice: Choice,
        token: Option<Token>,
    },
}

static JOBS: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

/// `job` to the sources' thread, started the first time.
fn ask(job: Job) {
    let jobs = JOBS.get_or_init(|| {
        let (jobs, asked) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("day: sources".to_owned())
            .spawn(move || work(&asked));
        if let Err(err) = started {
            tracing::warn!(%err, "no thread for the day's sources");
        }
        Mutex::new(jobs)
    });
    if jobs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .send(job)
        .is_err()
    {
        tracing::warn!("the day's sources' thread is gone");
    }
}

/// The sources' thread: each job in its order, asleep between.
fn work(asked: &mpsc::Receiver<Job>) {
    let mut watch: Option<goa::Watch> = None;
    for job in asked {
        match job {
            Job::ReadLater => {
                let said = later_file().is_some_and(|f| f.exists());
                LATER.store(if said { YES } else { NO }, Ordering::Release);
                tracing::debug!(later = said, "the day's first-run card's \"later\" read");
                wake_card();
            }
            Job::WriteLater => write_later(),
            Job::Watch => {
                let panel = crate::launcher::program("gnome-control-center");
                with(|flow| flow.panel.clone_from(&panel));
                watch = follow();
                changed();
            }
            Job::Unwatch => {
                if let Some(watch) = watch.take() {
                    watch.end();
                }
            }
            Job::Check { n, address, token } => {
                let found = planka::find(&address, &token);
                drop(token);
                checked(n, found);
            }
            Job::Board {
                n,
                server,
                board,
                token,
            } => {
                let found = planka::find_board(&server, &board, &token);
                drop(token);
                board_read(n, found);
            }
            Job::Calendars { n, account } => {
                let read = match crate::day::session_bus() {
                    Some(bus) => goa::calendars(
                        &bus,
                        &account.path,
                        &account.identity,
                        &crate::day::google_api(),
                    ),
                    None => Err(SourceError::Offline("no session bus".to_owned())),
                };
                calendars_read(n, read);
            }
            Job::Finish { choice, token } => finish(&choice, token),
        }
    }
}

fn later_file() -> Option<PathBuf> {
    crate::launcher::apps::state_file(LATER_FILE)
}

fn write_later() {
    let Some(file) = later_file() else {
        tracing::warn!("no state folder: \"later\" is not kept");
        return;
    };
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&file, "later\n"));
    match written {
        Ok(()) => tracing::debug!(file = %file.display(), "\"later\" kept"),
        Err(err) => tracing::warn!(file = %file.display(), %err, "\"later\" is not kept"),
    }
}

/// Online Accounts followed while the popover is open, on the bus the
/// compositor names; none: missing.
fn follow() -> Option<goa::Watch> {
    let Some(bus) = crate::day::session_bus() else {
        with(|flow| flow.online = Online::Missing);
        tracing::debug!("no session bus: no Online Accounts");
        return None;
    };
    goa::watch_accounts(bus, |seen| {
        let online = match seen {
            Some(listed) => Online::Listed(listed),
            None => Online::Missing,
        };
        tracing::debug!(?online, "Online Accounts as the sources' popover sees it");
        with(|flow| flow.online = online);
        changed();
    })
}

/// A check's answer, if it is still awaited.
fn checked(n: u64, found: hocus_day::source::Result<planka::Found>) {
    let said = with(|flow| {
        if flow.checking != Some(n) {
            return None;
        }
        flow.checking = None;
        match found {
            Ok(found) => {
                let columns = found
                    .board
                    .as_ref()
                    .map(|b| first_columns(&b.board))
                    .unwrap_or_default();
                let said = format!(
                    "board={} boards={}",
                    found
                        .board
                        .as_ref()
                        .map_or("none", |b| b.board.name.as_str()),
                    found.boards.len()
                );
                flow.step = Step::Show;
                flow.unfolded = None;
                flow.show = Some(Show::Planka {
                    found,
                    columns,
                    reading: None,
                });
                Some(said)
            }
            Err(error) => {
                let refusal = refusal(&error);
                flow.refused = Some(refusal);
                Some(format!("refused={refusal:?} ({error})"))
            }
        }
    })
    .flatten();
    if let Some(said) = said {
        tracing::debug!("a Planka board's check: {said}");
        changed();
    }
}

/// What the user is told of `error`.
fn refusal(error: &SourceError) -> Refusal {
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

/// The columns first chosen: the one named TODO (else the first), the one
/// named DONE (else the last), and the tasks' for "Ajout dans".
fn first_columns(board: &hocus_day::model::TaskBoard) -> [usize; CHOICES] {
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

fn board_read(n: u64, found: hocus_day::source::Result<planka::FoundBoard>) {
    let said = with(|flow| {
        let Some(Show::Planka {
            found: kept,
            columns,
            reading,
        }) = &mut flow.show
        else {
            return None;
        };
        if *reading != Some(n) {
            return None;
        }
        *reading = None;
        match found {
            Ok(board) => {
                *columns = first_columns(&board.board);
                let said = board.board.name.clone();
                kept.board = Some(board);
                Some(format!("board={said}"))
            }
            Err(error) => Some(format!("refused ({error})")),
        }
    })
    .flatten();
    if let Some(said) = said {
        tracing::debug!("a server's board read: {said}");
        changed();
    }
}

fn calendars_read(n: u64, read: hocus_day::source::Result<Vec<Calendar>>) {
    let said = with(|flow| {
        let Some(Show::Google { calendars, .. }) = &mut flow.show else {
            return None;
        };
        if *calendars != Calendars::Reading(n) {
            return None;
        }
        Some(match read {
            Ok(read) => {
                let rows: Vec<Row> = read
                    .into_iter()
                    .map(|c| Row {
                        colour: nearest(c.colour),
                        name: c.name,
                        shown: c.shown,
                        featured: c.featured,
                        read_only: c.read_only,
                    })
                    .collect();
                let said = rows.len();
                *calendars = Calendars::Read(rows);
                format!("{said} calendars")
            }
            Err(error) => {
                *calendars = Calendars::Failed;
                format!("failed ({error})")
            }
        })
    })
    .flatten();
    if let Some(said) = said {
        tracing::debug!("a Google account's calendars read: {said}");
        changed();
    }
}

/// `choice` written into the configuration file, `token` kept under the
/// board's account, then the day's workers started again.
fn finish(choice: &Choice, token: Option<Token>) {
    let Some(file) = crate::day::config_file() else {
        tracing::warn!("no configuration file: the day's sources are not kept");
        return;
    };
    if let Err(why) = edit::write(&file, choice) {
        tracing::warn!("the day's sources are not written: {why}");
        return;
    }
    if let Some(token) = token {
        let config = hocus_day::config::read(&file);
        let account = config
            .as_ref()
            .ok()
            .and_then(|c| c.tasks_account(file.parent()))
            .map(|(account, _)| account.id);
        match account {
            Some(account) => match crate::day::secrets().keep(&account, &token) {
                Ok(kept) => tracing::info!(%account, ?kept, "the board's token kept"),
                Err(err) => tracing::warn!(%account, %err, "the board's token is not kept"),
            },
            None => tracing::warn!("the board's account is not in the configuration"),
        }
    }
    tracing::info!(file = %file.display(), "the day's sources written");
    crate::day::reconfigure();
    wake_card();
}

// --- Words -----------------------------------------------------------------------------

/// The card's and the popover's words, in the bar's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(if self.fr {
            hocus_words::Lang::French
        } else {
            hocus_words::Lang::English
        })
    }

    pub fn card_title(self) -> &'static str {
        self.pick(txt::YOUR_DAY_HERE)
    }

    pub fn card_text(self) -> &'static str {
        self.pick(txt::CONNECT_YOUR_CALENDAR_AND_TASKS_THE)
    }

    pub fn choose(self) -> &'static str {
        self.pick(txt::CHOOSE)
    }

    pub fn later(self) -> &'static str {
        self.pick(txt::LATER)
    }

    pub fn back(self) -> &'static str {
        self.pick(txt::BACK)
    }

    pub fn close(self) -> &'static str {
        self.pick(txt::CLOSE)
    }

    pub fn add_account(self) -> &'static str {
        self.pick(txt::ADD_AN_ACCOUNT)
    }

    pub fn google(self) -> &'static str {
        self.pick(txt::GOOGLE_CALENDAR)
    }

    pub fn google_line(self) -> &'static str {
        self.pick(txt::YOUR_MEETINGS_AND_APPOINTMENTS)
    }

    pub fn google_none(self) -> &'static str {
        self.pick(txt::ADD_A_GOOGLE_ACCOUNT_IN_ONLINE)
    }

    pub fn google_missing(self) -> &'static str {
        self.pick(txt::GNOME_ONLINE_ACCOUNTS_MISSING)
    }

    pub fn planka(self) -> &'static str {
        self.pick(txt::PLANKA_BOARD)
    }

    pub fn planka_line(self) -> &'static str {
        self.pick(txt::YOUR_TASKS_THE_CARDS_OF_A)
    }

    pub fn local_note(self) -> &'static str {
        self.pick(txt::YOUR_TASKS_STAY_ON_THIS_COMPUTER)
    }

    pub fn board_address(self) -> &'static str {
        self.pick(txt::BOARD_ADDRESS)
    }

    pub fn token(self) -> &'static str {
        self.pick(txt::ACCESS_TOKEN)
    }

    pub fn token_help(self) -> &'static str {
        self.pick(txt::IN_PLANKA_ONCE_SIGNED_IN_THE)
    }

    pub fn cancel(self) -> &'static str {
        self.pick(txt::CANCEL)
    }

    pub fn sign_in(self) -> &'static str {
        self.pick(txt::SIGN_IN)
    }

    pub fn signing_in(self) -> &'static str {
        self.pick(txt::SIGNING_IN)
    }

    pub fn refusal(self, refusal: Refusal) -> &'static str {
        match refusal {
            Refusal::Token => self.pick(txt::TOKEN_REFUSED_OR_EXPIRED_PASTE_A),
            Refusal::NoServer => self.pick(txt::NO_PLANKA_SERVER_ANSWERS_AT_THIS),
            Refusal::NotShared => self.pick(txt::THIS_BOARD_DOES_NOT_EXIST_OR),
            Refusal::Address => self.pick(txt::PASTE_A_BOARD_S_ADDRESS_HTTPS),
        }
    }

    pub fn what_to_show(self) -> &'static str {
        self.pick(txt::WHAT_TO_SHOW)
    }

    pub fn calendars(self) -> &'static str {
        self.pick(txt::CALENDARS)
    }

    /// A calendar's star's name.
    pub fn feature(self) -> &'static str {
        self.pick(txt::FEATURE)
    }

    pub fn read_only(self) -> &'static str {
        self.pick(txt::READ_ONLY)
    }

    pub fn reading(self) -> &'static str {
        self.pick(txt::READING_THE_CALENDARS)
    }

    pub fn unread(self) -> &'static str {
        self.pick(txt::THIS_ACCOUNT_S_CALENDARS_COULD_NOT)
    }

    pub fn tasks(self) -> &'static str {
        self.pick(txt::TASKS)
    }

    pub fn board(self) -> &'static str {
        self.pick(txt::BOARD)
    }

    /// The three choices of a board.
    pub fn choice(self, which: usize) -> &'static str {
        match which {
            0 => self.pick(txt::TASKS_COLUMN),
            1 => self.pick(txt::DONE_COLUMN),
            _ => self.pick(txt::ADD_TO),
        }
    }

    /// "12 cartes", "1 carte".
    pub fn cards(self, n: usize) -> String {
        match (self.fr, n) {
            (true, 0 | 1) => format!("{n} carte"),
            (true, n) => format!("{n} cartes"),
            (false, 1) => "1 card".to_owned(),
            (false, n) => format!("{n} cards"),
        }
    }

    pub fn done(self) -> &'static str {
        self.pick(txt::DONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_day::model::{BoardId, Column, ColumnId, TaskBoard};

    #[test]
    fn an_accounts_colour_is_the_nearest_of_the_eight() {
        assert_eq!(nearest(Colour([0x8F, 0xB3, 0xFF])), 0);
        // Some of Google's own.
        assert_eq!(nearest(Colour([0x16, 0xA7, 0x65])), 1);
        assert_eq!(nearest(Colour([0xF8, 0x3A, 0x22])), 2);
        assert_eq!(nearest(Colour([0xB9, 0x9A, 0xFF])), 3);
        assert_eq!(nearest(Colour([0xF6, 0x91, 0xB2])), 4);
        assert_eq!(nearest(Colour([0xFA, 0xD1, 0x65])), 5);
        assert_eq!(nearest(Colour([0x42, 0xD6, 0x92])), 1);
        assert_eq!(nearest(Colour([0x9F, 0xE1, 0xE7])), 6);
        assert_eq!(nearest(Colour([0xC2, 0xC2, 0xC2])), 7);
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
    fn its_words_in_french_and_english() {
        let fr = Words { fr: true };
        let en = Words { fr: false };
        assert_eq!(fr.card_title(), "Ta journée ici");
        assert_eq!(en.choose(), "Choose…");
        assert_eq!(fr.cards(12), "12 cartes");
        assert_eq!(fr.cards(1), "1 carte");
        assert_eq!(en.cards(1), "1 card");
        assert_eq!(
            fr.refusal(Refusal::Token),
            "Jeton refusé ou expiré : colle un nouveau jeton."
        );
        assert_eq!(
            en.refusal(Refusal::NotShared),
            "This board does not exist or is not shared with you."
        );
        assert_eq!(fr.choice(2), "Ajout dans");
        assert_eq!(en.what_to_show(), "What to show?");
        assert_eq!(fr.feature(), "Mettre en avant");
        assert_eq!(en.feature(), "Feature");
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
