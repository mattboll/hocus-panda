//! Settings › Calendar and tasks (#926): what the page reads and asks,
//! off the window's thread. Online Accounts' Google accounts and each
//! one's calendars, a Planka board's columns, the local list's count; a
//! board checked with a pasted token, the token kept. The configuration
//! itself is written by the file's worker, as Day.1 writes it
//! ([`hocus_day::edit`]).
//!
//! Online Accounts and the keyring are on the session bus the compositor
//! gives the programs it starts (`DBUS_SESSION_BUS_ADDRESS`), as the
//! portal's; without one, no Google account and the token in its file.

use hocus_day::{
    choice::{Refusal, refusal},
    config::{self, Config},
    goa::{self, GoogleAccount},
    model::Calendar,
    planka::{self, FoundBoard},
    secrets::{Secrets, Token},
    source::ColumnNames,
};
use hocus_words::settings::agenda as words;

use crate::model::{Outside, SectionId};

/// The page's rows the search finds.
pub const CALENDARS: &str = "agenda.calendars";
pub const COLOURS: &str = "agenda.colours";
pub const BOARD: &str = "agenda.board";

/// The page's settings, which live in `[day]` as Day.1 writes them: found
/// by the search (agenda, calendrier, couleur, Planka).
pub fn outside() -> &'static [Outside] {
    const OUTSIDE: [Outside; 3] = [
        Outside {
            id: CALENDARS,
            section: SectionId::Agenda,
            group: words::CALENDARS,
            name: words::CALENDARS_SHOWN,
            keywords: (
                &[
                    "agenda",
                    "calendrier",
                    "google",
                    "réunions",
                    "comptes en ligne",
                    "étoile",
                ],
                &[
                    "calendar",
                    "agenda",
                    "google",
                    "meetings",
                    "online accounts",
                    "star",
                ],
            ),
        },
        Outside {
            id: COLOURS,
            section: SectionId::Agenda,
            group: words::CALENDARS,
            name: words::COLOUR,
            keywords: (
                &["couleur", "agenda", "pastille"],
                &["colour", "color", "calendar"],
            ),
        },
        Outside {
            id: BOARD,
            section: SectionId::Agenda,
            group: words::TASKS,
            name: words::BOARD,
            keywords: (
                &["planka", "tâches", "tableau", "kanban", "à faire"],
                &["planka", "tasks", "board", "kanban", "to do"],
            ),
        },
    ];
    &OUTSIDE
}

/// The session bus Online Accounts and the keyring are on.
pub fn bus() -> Option<String> {
    std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .ok()
        .filter(|b| !b.is_empty())
}

/// The Calendar API's address (a fake's, for tests).
fn api() -> String {
    hocus_day::google::api(std::env::var(hocus_day::google::API_ENV).ok().as_deref())
}

/// Where the boards' tokens are kept, as the shell reads them.
fn secrets() -> Secrets {
    Secrets::new(bus(), hocus_day::secrets::accounts_dir())
}

/// `gnome-control-center`, when it is in `PATH`: Online Accounts' panel.
pub fn panel() -> Option<&'static str> {
    const PANEL: &str = "gnome-control-center";
    crate::applications::in_path(PANEL, std::env::var_os("PATH").as_deref()).then_some(PANEL)
}

/// Online Accounts' panel opened, on a thread the window never waits for.
pub fn open_panel() {
    let Some(panel) = panel() else {
        return;
    };
    tracing::info!("Online Accounts' panel opens");
    let started = std::thread::Builder::new()
        .name("settings: online accounts".to_owned())
        .spawn(move || {
            let ran = std::process::Command::new(panel)
                .arg("online-accounts")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if let Err(err) = ran {
                tracing::warn!(%err, "Online Accounts' panel does not run");
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to open Online Accounts");
    }
}

/// Follows Online Accounts' Google accounts, `seen` called with them as
/// it starts and each time they change (`None`: Online Accounts is not
/// there); `None` without a bus.
pub fn watch(seen: impl FnMut(Option<Vec<GoogleAccount>>) + Send + 'static) -> Option<goa::Watch> {
    let bus = bus()?;
    goa::watch_accounts(bus, seen)
}

/// Google account `account`'s calendars, as Online Accounts' token reads
/// them. Blocking.
pub fn calendars(account: &GoogleAccount) -> Result<Vec<Calendar>, String> {
    let bus = bus().ok_or_else(|| "no session bus".to_owned())?;
    goa::calendars(&bus, &account.path, &account.identity, &api()).map_err(|e| e.to_string())
}

/// The board `[day]` sets, as read with its kept token.
#[derive(Debug, Clone, PartialEq)]
pub enum Board {
    Read(FoundBoard),
    Refused(Refusal),
}

/// The configuration's board, read with its token. Blocking.
pub fn board(config: &Config, file: Option<&std::path::Path>) -> Option<Board> {
    let (account, entry) = config.tasks_account(file.and_then(|f| f.parent()))?;
    if entry.kind != config::TaskKind::Planka {
        return None;
    }
    let read = planka::address(&entry.address, &entry.board).and_then(|(server, board)| {
        let token = secrets()
            .get(&account.id)
            .ok_or_else(|| hocus_day::SourceError::SignIn("no token kept".to_owned()))?;
        planka::find_board(&server, &board, &token)
    });
    Some(match read {
        Ok(found) => Board::Read(found),
        Err(err) => {
            tracing::info!(%err, "the board does not read");
            Board::Refused(refusal(&err))
        }
    })
}

/// How many tasks the local list holds. Blocking.
pub fn local_tasks() -> usize {
    use hocus_day::KanbanSource as _;
    let file = hocus_config::state_dir().map(|d| d.join("tasks.json"));
    hocus_day::local::LocalList::open(file)
        .cards(&hocus_day::model::ColumnId::new(hocus_day::local::TODO))
        .map_or(0, |cards| cards.len())
}

/// What a board's address and pasted token found.
#[derive(Debug, Clone, PartialEq)]
pub enum Checked {
    /// A board: its address, as the configuration keeps it, and its
    /// columns chosen at first.
    Board {
        address: String,
        found: FoundBoard,
    },
    /// A server's address alone: the user's boards there.
    Boards {
        server: planka::Server,
        boards: Vec<planka::Listed>,
    },
    Refused(Refusal),
}

/// `address` checked with `token` (`hocus_day::planka::find`); a board
/// found has its token kept for it before the configuration names it.
/// Blocking.
pub fn check(address: &str, token: &Token) -> Checked {
    match planka::find(address, token) {
        Ok(found) => match (found.address(), found.board.clone()) {
            (Some(address), Some(board)) => keep(address, board, token),
            _ => Checked::Boards {
                server: found.server,
                boards: found.boards,
            },
        },
        Err(err) => Checked::Refused(refusal(&err)),
    }
}

/// Board `id` on `server`, chosen among the user's. Blocking.
pub fn pick(server: &planka::Server, id: &str, token: &Token) -> Checked {
    match planka::find_board(server, id, token) {
        Ok(board) => keep(format!("{server}/boards/{id}"), board, token),
        Err(err) => Checked::Refused(refusal(&err)),
    }
}

/// The token kept under the account the board at `address` will be.
fn keep(address: String, found: FoundBoard, token: &Token) -> Checked {
    let names = ColumnNames::default();
    let text = format!(
        "[[day.tasks]]\nkind = \"planka\"\naddress = {}\ntasks = {:?}\ndone = {:?}\n",
        toml_edit::Value::from(address.as_str()),
        names.tasks,
        names.done
    );
    let account = config::from_toml(&text)
        .ok()
        .and_then(|c| c.tasks_account(None).map(|(a, _)| a.id));
    match account {
        Some(account) => match secrets().keep(&account, token) {
            Ok(kept) => tracing::info!(%account, ?kept, "the board's token kept"),
            Err(err) => tracing::warn!(%account, %err, "the board's token is not kept"),
        },
        None => tracing::warn!("the board's account is not known: its token is not kept"),
    }
    Checked::Board { address, found }
}
