//! A Planka board as the bar's tasks (#566), through the workers as the
//! shell starts them, against a fake Planka server of this test's own on
//! 127.0.0.1: never the network. Its answers have the shapes of Planka's
//! REST API (`GET /api/boards/{id}` with its `included` lists and cards,
//! `POST /api/lists/{listId}/cards`, `PATCH /api/cards/{id}`,
//! `GET /api/users/me`, `GET /api/bootstrap`). Each test waits on the
//! snapshots the workers send, never on the clock.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::Duration,
};

use hocus_day::{
    Change, Clock, Day, Env, Login, Snapshot, SourceError, State, Workers, config,
    model::TaskId,
    secrets::{Secrets, Token},
};
use serde_json::{Value, json};

const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.e30.test";

/// A list of the fake board: Planka's `active` and `closed` lists are its
/// columns; `archive` and `trash` have no name and no position.
#[derive(Clone)]
struct List {
    id: &'static str,
    name: Option<&'static str>,
    position: Option<f64>,
    kind: &'static str,
}

#[derive(Clone)]
struct Card {
    id: String,
    list: String,
    name: String,
    position: f64,
    due: Option<&'static str>,
}

struct Board {
    /// The token it takes, and the paired `httpOnlyToken` cookie its
    /// session was made with, if any (Planka 2's web sign-in).
    token: String,
    http_only: Option<String>,
    /// It closes every connection without a word.
    offline: bool,
    /// `/api/bootstrap`'s `oidc`: none, or whether it is enforced.
    oidc: Option<bool>,
    lists: Vec<List>,
    cards: Vec<Card>,
    /// What it was asked to change: `POST …` and `PATCH …` with their
    /// bodies.
    asked: Vec<(String, Value)>,
    next: u64,
}

impl Board {
    fn cards_of(&self, list: &str) -> Vec<&Card> {
        let mut cards: Vec<&Card> = self.cards.iter().filter(|c| c.list == list).collect();
        cards.sort_by(|a, b| a.position.total_cmp(&b.position));
        cards
    }

    fn card_json(&self, card: &Card) -> Value {
        json!({
            "id": card.id,
            "boardId": "1",
            "listId": card.list,
            "creatorUserId": "100",
            "prevListId": null,
            "coverAttachmentId": null,
            "type": "project",
            "position": card.position,
            "name": card.name,
            "description": null,
            "dueDate": card.due,
            "isDueCompleted": false,
            "stopwatch": null,
            "commentsTotal": 0,
            "isClosed": false,
            "listChangedAt": "2026-11-01T09:00:00.000Z",
            "createdAt": "2026-11-01T09:00:00.000Z",
            "updatedAt": null,
            "isSubscribed": false,
        })
    }

    fn board_json(&self) -> Value {
        let finite = |l: &List| l.kind == "active" || l.kind == "closed";
        let lists: Vec<Value> = self
            .lists
            .iter()
            .map(|l| {
                json!({
                    "id": l.id,
                    "boardId": "1",
                    "type": l.kind,
                    "position": l.position,
                    "name": l.name,
                    "color": null,
                    "createdAt": "2026-11-01T09:00:00.000Z",
                    "updatedAt": null,
                })
            })
            .collect();
        // Planka gives the cards of its finite lists only.
        let cards: Vec<Value> = self
            .cards
            .iter()
            .filter(|c| self.lists.iter().any(|l| l.id == c.list && finite(l)))
            .map(|c| self.card_json(c))
            .collect();
        json!({
            "item": {
                "id": "1",
                "projectId": "2",
                "position": 65536,
                "name": "Sprint",
                "defaultView": "kanban",
                "defaultCardType": "story",
                "limitCardTypesToDefaultOne": false,
                "alwaysDisplayCardCreator": false,
                "displayCardAges": false,
                "expandTaskListsByDefault": false,
                "createdAt": "2026-11-01T09:00:00.000Z",
                "updatedAt": null,
                "isSubscribed": false,
            },
            "included": {
                "users": [{"id": "100", "name": "Alex", "username": "alex"}],
                "projects": [{"id": "2", "name": "Équipe"}],
                "boardMemberships": [],
                "labels": [],
                "lists": lists,
                "cards": cards,
                "cardMemberships": [],
                "cardLabels": [],
                "taskLists": [],
                "tasks": [],
                "attachments": [],
                "customFieldGroups": [],
                "customFields": [],
                "customFieldValues": [],
            },
        })
    }
}

/// The fake server: its address, and its board to set and read.
#[derive(Clone)]
struct Fake {
    address: String,
    board: Arc<Mutex<Board>>,
}

impl Fake {
    /// A board with the lists Backlog, TODO, Done (closed), an archive and
    /// a trash; its cards as `cards` says (list, id, title, position).
    fn start(cards: &[(&str, &str, &str, f64)]) -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = format!("http://{}", listener.local_addr()?);
        let board = Arc::new(Mutex::new(Board {
            token: TOKEN.to_owned(),
            http_only: None,
            offline: false,
            oidc: None,
            lists: vec![
                List {
                    id: "10",
                    name: Some("Backlog"),
                    position: Some(65536.0),
                    kind: "active",
                },
                // Out of order: the columns are by position.
                List {
                    id: "12",
                    name: Some("Done"),
                    position: Some(196608.0),
                    kind: "closed",
                },
                List {
                    id: "11",
                    name: Some("TODO"),
                    position: Some(131072.0),
                    kind: "active",
                },
                List {
                    id: "13",
                    name: None,
                    position: None,
                    kind: "archive",
                },
                List {
                    id: "14",
                    name: None,
                    position: None,
                    kind: "trash",
                },
            ],
            cards: cards
                .iter()
                .map(|(list, id, name, position)| Card {
                    id: (*id).to_owned(),
                    list: (*list).to_owned(),
                    name: (*name).to_owned(),
                    position: *position,
                    due: None,
                })
                .collect(),
            asked: Vec::new(),
            next: 0,
        }));
        let served = board.clone();
        std::thread::Builder::new()
            .name("fake planka".to_owned())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    serve(stream, &served);
                }
            })?;
        Ok(Self { address, board })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Board) -> T) -> T {
        f(&mut self.board.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The ids of `list`'s cards, in its order.
    fn ids(&self, list: &str) -> Vec<String> {
        self.with(|b| b.cards_of(list).iter().map(|c| c.id.clone()).collect())
    }

    fn asked(&self) -> Vec<(String, Value)> {
        self.with(|b| b.asked.clone())
    }
}

/// One request, one answer, the connection closed.
fn serve(mut stream: TcpStream, board: &Mutex<Board>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(reader) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut words = line.split_whitespace();
    let (method, path) = (
        words.next().unwrap_or("").to_owned(),
        words.next().unwrap_or("").to_owned(),
    );
    let mut headers = HashMap::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_lowercase(), value.trim().to_owned());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let mut board = board.lock().unwrap_or_else(PoisonError::into_inner);
    if board.offline {
        // Not a word: as a server that is not there.
        return;
    }
    let (status, answer) = answer(&mut board, &method, &path, &headers, &body);
    drop(board);
    let text = answer.to_string();
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Unprocessable Entity",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
}

fn answer(
    board: &mut Board,
    method: &str,
    path: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> (u16, Value) {
    let not_found = |what: &str| (404, json!({"code": "E_NOT_FOUND", "problems": [what]}));
    if method == "GET" && path == "/api/bootstrap" {
        let oidc = board.oidc.map(|enforced| {
            json!({
                "authorizationUrl": "https://sso.example.org/auth",
                "endSessionUrl": null,
                "isEnforced": enforced,
            })
        });
        return (200, json!({"item": {"oidc": oidc, "version": "2.0.0"}}));
    }
    let bearer = headers
        .get("authorization")
        .and_then(|a| a.strip_prefix("Bearer "));
    let cookie = headers.get("cookie").cloned().unwrap_or_default();
    let paired = board.http_only.as_ref().is_none_or(|pair| {
        cookie
            .split(';')
            .any(|c| c.trim() == format!("httpOnlyToken={pair}"))
    });
    if bearer != Some(board.token.as_str()) || !paired {
        return (
            401,
            json!({"code": "E_UNAUTHORIZED", "message": "Access token is missing"}),
        );
    }
    let body: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method, segments.as_slice()) {
        ("GET", ["api", "users", "me"]) => (
            200,
            json!({
                "item": {"id": "100", "name": "Alex", "username": "alex", "role": "boardUser"},
                "included": {"notificationServices": []},
            }),
        ),
        ("GET", ["api", "boards", "1"]) => (200, board.board_json()),
        ("GET", ["api", "boards", _]) => not_found("Board not found"),
        ("POST", ["api", "lists", list, "cards"]) => {
            board
                .asked
                .push((format!("POST /api/lists/{list}/cards"), body.clone()));
            if !board.lists.iter().any(|l| l.id == *list) {
                return not_found("List not found");
            }
            let (Some(kind), Some(name)) = (body["type"].as_str(), body["name"].as_str()) else {
                return (400, json!({"code": "E_MISSING_OR_INVALID_PARAMS"}));
            };
            if !["project", "story"].contains(&kind) {
                return (400, json!({"code": "E_MISSING_OR_INVALID_PARAMS"}));
            }
            let Some(position) = body["position"].as_f64().filter(|p| *p >= 0.0) else {
                return (422, json!({"code": "E_UNPROCESSABLE_ENTITY"}));
            };
            board.next += 1;
            let card = Card {
                id: format!("n{}", board.next),
                list: (*list).to_owned(),
                name: name.to_owned(),
                position,
                due: None,
            };
            let item = board.card_json(&card);
            board.cards.push(card);
            (200, json!({"item": item}))
        }
        ("PATCH", ["api", "cards", id]) => {
            board
                .asked
                .push((format!("PATCH /api/cards/{id}"), body.clone()));
            let Some(list) = body["listId"].as_str().map(str::to_owned) else {
                return (422, json!({"code": "E_UNPROCESSABLE_ENTITY"}));
            };
            let Some(position) = body["position"].as_f64().filter(|p| *p >= 0.0) else {
                return (
                    422,
                    json!({"code": "E_UNPROCESSABLE_ENTITY", "problems": ["Position must be present"]}),
                );
            };
            if !board.lists.iter().any(|l| l.id == list) {
                return not_found("List not found");
            }
            let Some(card) = board.cards.iter_mut().find(|c| c.id == *id) else {
                return not_found("Card not found");
            };
            card.list = list;
            card.position = position;
            let card = card.clone();
            (200, json!({"item": board.card_json(&card)}))
        }
        _ => not_found("Not found"),
    }
}

/// The workers for `[day]`, the token kept for its board's account (none
/// when `token` is `None`).
struct Running {
    workers: Workers,
    day: Day,
    snapshots: mpsc::Receiver<Snapshot>,
    secrets: Secrets,
    account: hocus_day::model::AccountId,
    _dir: tempfile::TempDir,
}

fn start(day_toml: &str, token: Option<&str>) -> anyhow::Result<Running> {
    let dir = tempfile::tempdir()?;
    let config = config::from_toml(day_toml).map_err(anyhow::Error::msg)?;
    let now = jiff::civil::date(2026, 11, 4)
        .at(14, 0, 0, 0)
        .to_zoned(jiff::tz::TimeZone::UTC)?;
    // Files only: no bus, so never the session's keyring.
    let secrets = Secrets::new(None, Some(dir.path().join("state/accounts")));
    let (account, _) = config
        .tasks_account(Some(dir.path()))
        .ok_or_else(|| anyhow::anyhow!("no tasks' account"))?;
    if let Some(token) = token {
        secrets.keep(&account.id, &Token::new(token))?;
    }
    let env = Env {
        clock: Clock::new(move || now.clone()),
        secrets: secrets.clone(),
        state_dir: Some(dir.path().join("state")),
        cache_dir: Some(dir.path().join("cache")),
        config_dir: Some(dir.path().to_path_buf()),
    };
    let (out, snapshots) = mpsc::channel();
    let (workers, day) = Workers::start(&config, &env, &out);
    Ok(Running {
        workers,
        day,
        snapshots,
        secrets,
        account: account.id,
        _dir: dir,
    })
}

impl Running {
    /// The snapshots, until `ok` says the bar's tasks are as expected.
    fn until(&mut self, ok: impl Fn(&Snapshot) -> bool) -> anyhow::Result<Snapshot> {
        loop {
            if let Some(tasks) = self.day.tasks().filter(|t| ok(t)) {
                return Ok(tasks.clone());
            }
            let snapshot = self
                .snapshots
                .recv_timeout(Duration::from_secs(20))
                .map_err(|_| {
                    anyhow::anyhow!("not as expected: {:?}", self.day.tasks().map(summary))
                })?;
            self.day.update(snapshot);
        }
    }

    fn change(&mut self, change: Change) {
        assert!(self.workers.change(change).is_some(), "the board's worker");
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.workers.flush(Duration::from_secs(10));
    }
}

fn summary(s: &Snapshot) -> String {
    format!(
        "{:?} tasks={:?} pending={:?}",
        s.state,
        titles(s),
        s.pending
    )
}

fn titles(s: &Snapshot) -> Vec<&str> {
    s.tasks.iter().map(|t| t.title.as_str()).collect()
}

fn fresh(s: &Snapshot) -> bool {
    matches!(s.state, State::Fresh { .. })
}

fn planka(fake: &Fake, rest: &str) -> String {
    format!(
        "[[day.tasks]]\nkind = \"planka\"\naddress = \"{}\"\nboard = \"1\"\n{rest}",
        fake.address
    )
}

/// The cards of a sprint: three to do (out of order), one done, one in
/// the backlog.
fn sprint() -> anyhow::Result<Fake> {
    let fake = Fake::start(&[
        ("11", "c3", "Écrire le bilan", 196608.0),
        ("11", "c1", "Relire la spec", 65536.0),
        ("10", "b1", "Un jour", 65536.0),
        ("11", "c2", "Appeler Sam", 131072.0),
        ("12", "d1", "Fait hier", 4096.0),
    ])?;
    fake.with(|b| {
        if let Some(c) = b.cards.iter_mut().find(|c| c.id == "c1") {
            c.due = Some("2026-11-05T17:00:00.000Z");
        }
    });
    Ok(fake)
}

#[test]
fn the_boards_todo_cards_are_the_bars_tasks_in_the_columns_order() -> anyhow::Result<()> {
    let fake = sprint()?;
    let mut run = start(&planka(&fake, ""), Some(TOKEN))?;
    let tasks = run.until(fresh)?;
    assert_eq!(
        titles(&tasks),
        ["Relire la spec", "Appeler Sam", "Écrire le bilan"]
    );
    let ids: Vec<&str> = tasks.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["c1", "c2", "c3"]);
    assert_eq!(
        tasks.tasks[0].due,
        Some("2026-11-05T17:00:00Z".parse::<jiff::Timestamp>()?)
    );
    assert_eq!(tasks.tasks[1].due, None);
    // The board's columns, in their order; no archive, no trash.
    let board = tasks
        .board
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("a board"))?;
    assert_eq!(board.name, "Sprint");
    let columns: Vec<&str> = board.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(columns, ["Backlog", "TODO", "Done"]);
    // A server that does not enforce SSO: its own sign-in too.
    assert_eq!(tasks.login, Some(Login::Password));
    Ok(())
}

#[test]
fn a_tick_moves_the_card_to_the_top_of_done() -> anyhow::Result<()> {
    let fake = sprint()?;
    let mut run = start(&planka(&fake, ""), Some(TOKEN))?;
    run.until(fresh)?;
    run.change(Change::Tick {
        task: TaskId::new("c2"),
    });
    let tasks = run.until(|s| s.pending.is_empty() && !titles(s).contains(&"Appeler Sam"))?;
    assert_eq!(titles(&tasks), ["Relire la spec", "Écrire le bilan"]);
    assert_eq!(fake.ids("12"), ["c2", "d1"]);
    let asked = fake.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].0, "PATCH /api/cards/c2");
    assert_eq!(asked[0].1["listId"], "12");
    let position = asked[0].1["position"].as_f64().unwrap_or(-1.0);
    assert!((0.0..4096.0).contains(&position), "{position}");
    Ok(())
}

/// The bar unticks within its 1.5 s (Day.4); the board's worker puts the
/// card back in its column at the position it had.
#[test]
fn an_untick_puts_the_card_back_where_it_was() -> anyhow::Result<()> {
    let fake = sprint()?;
    let mut run = start(&planka(&fake, ""), Some(TOKEN))?;
    run.until(fresh)?;
    let task = TaskId::new("c2");
    run.change(Change::Tick { task: task.clone() });
    run.until(|s| s.pending.is_empty() && s.tasks.len() == 2)?;
    run.change(Change::Untick { task });
    let tasks = run.until(|s| s.pending.is_empty() && s.tasks.len() == 3)?;
    assert_eq!(
        titles(&tasks),
        ["Relire la spec", "Appeler Sam", "Écrire le bilan"]
    );
    assert_eq!(fake.ids("11"), ["c1", "c2", "c3"]);
    assert_eq!(fake.ids("12"), ["d1"]);
    let asked = fake.asked();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert_eq!(asked[1].0, "PATCH /api/cards/c2");
    assert_eq!(
        asked[1].1,
        json!({"listId": "11", "position": 131072.0}),
        "back where it was"
    );
    Ok(())
}

#[test]
fn add_creates_a_card_at_the_top_of_the_add_column() -> anyhow::Result<()> {
    let fake = sprint()?;
    // The tasks' column by default.
    let mut run = start(&planka(&fake, ""), Some(TOKEN))?;
    run.until(fresh)?;
    run.change(Change::Add {
        title: "Nouvelle tâche".to_owned(),
    });
    let tasks = run.until(|s| s.pending.is_empty() && s.tasks.len() == 4)?;
    assert_eq!(titles(&tasks)[0], "Nouvelle tâche");
    let asked = fake.asked();
    assert_eq!(asked[0].0, "POST /api/lists/11/cards");
    // The board's own type for new cards, a position above the first's.
    assert_eq!(asked[0].1["type"], "story");
    assert_eq!(asked[0].1["name"], "Nouvelle tâche");
    let position = asked[0].1["position"].as_f64().unwrap_or(-1.0);
    assert!((0.0..65536.0).contains(&position), "{position}");
    drop(run);

    // A column of its own: its top, the bar's tasks unchanged.
    let fake = sprint()?;
    let mut run = start(&planka(&fake, "add = \"backlog\"\n"), Some(TOKEN))?;
    run.until(fresh)?;
    run.change(Change::Add {
        title: "Plus tard".to_owned(),
    });
    // Nothing changes in the bar: done once the worker is.
    run.workers.flush(Duration::from_secs(20));
    assert_eq!(fake.asked()[0].0, "POST /api/lists/10/cards");
    let backlog = fake.with(|b| {
        b.cards_of("10")
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(backlog, ["Plus tard", "Un jour"]);
    assert_eq!(
        run.day.tasks().map(|s| s.tasks.len()),
        Some(3),
        "the tasks' column has what it had"
    );
    Ok(())
}

#[test]
fn a_refused_token_turns_the_account_to_sign_in_again() -> anyhow::Result<()> {
    let fake = sprint()?;
    let mut run = start(&planka(&fake, ""), Some("an.old.token"))?;
    let failed = run.until(|s| matches!(s.state, State::Failed { .. }))?;
    assert!(
        matches!(
            &failed.state,
            State::Failed {
                error: SourceError::SignIn(_),
                ..
            }
        ),
        "{:?}",
        failed.state
    );
    assert!(failed.tasks.is_empty());
    // A new token pasted (`hocus day planka-token`): read at the next read.
    run.secrets.keep(&run.account, &Token::new(TOKEN))?;
    run.workers.refresh();
    let tasks = run.until(fresh)?;
    assert_eq!(tasks.tasks.len(), 3);
    // Expired while in use: sign in again, the tasks read before kept.
    fake.with(|b| b.token = "another".to_owned());
    run.workers.refresh();
    let failed = run.until(|s| matches!(s.state, State::Failed { .. }))?;
    assert!(matches!(
        &failed.state,
        State::Failed {
            error: SourceError::SignIn(_),
            fetched: Some(_)
        }
    ));
    assert_eq!(failed.tasks.len(), 3);
    drop(run);

    // No token kept at all: the same.
    let mut run = start(&planka(&fake, ""), None)?;
    let failed = run.until(|s| matches!(s.state, State::Failed { .. }))?;
    assert!(matches!(
        &failed.state,
        State::Failed {
            error: SourceError::SignIn(_),
            ..
        }
    ));
    Ok(())
}

#[test]
fn offline_the_change_is_kept_and_sent_when_the_server_answers() -> anyhow::Result<()> {
    let fake = sprint()?;
    let mut run = start(&planka(&fake, ""), Some(TOKEN))?;
    run.until(fresh)?;
    fake.with(|b| b.offline = true);
    run.change(Change::Tick {
        task: TaskId::new("c1"),
    });
    run.change(Change::Add {
        title: "Hors ligne".to_owned(),
    });
    let kept = run.until(|s| {
        s.pending.len() == 2
            && s.pending
                .iter()
                .all(|p| matches!(p.refused, Some(SourceError::Offline(_))))
    })?;
    // What was read before is kept meanwhile.
    assert_eq!(kept.tasks.len(), 3);
    assert!(fake.asked().is_empty());
    // The server answers again: read on the schedule or on resume.
    fake.with(|b| b.offline = false);
    run.workers.refresh();
    let tasks = run.until(|s| fresh(s) && s.pending.is_empty())?;
    assert_eq!(
        titles(&tasks),
        ["Hors ligne", "Appeler Sam", "Écrire le bilan"]
    );
    let asked: Vec<String> = fake.asked().into_iter().map(|(what, _)| what).collect();
    assert_eq!(asked, ["PATCH /api/cards/c1", "POST /api/lists/11/cards"]);
    assert_eq!(fake.ids("12"), ["c1", "d1"]);
    Ok(())
}

#[test]
fn columns_are_found_by_name_case_insensitively() -> anyhow::Result<()> {
    let fake = sprint()?;
    // The board by its address, the columns named in another case.
    let toml = format!(
        "[[day.tasks]]\nkind = \"planka\"\naddress = \"{}/boards/1\"\n\
         tasks = \"backlog\"\ndone = \"done\"\nadd = \"todo\"\n",
        fake.address
    );
    let mut run = start(&toml, Some(TOKEN))?;
    let tasks = run.until(fresh)?;
    assert_eq!(titles(&tasks), ["Un jour"]);
    run.change(Change::Tick {
        task: TaskId::new("b1"),
    });
    run.until(|s| s.pending.is_empty() && s.tasks.is_empty())?;
    assert_eq!(fake.ids("12"), ["b1", "d1"]);
    drop(run);

    // A column named that the board does not have: said.
    let mut run = start(&planka(&fake, "tasks = \"En cours\"\n"), Some(TOKEN))?;
    let failed = run.until(|s| matches!(s.state, State::Failed { .. }))?;
    assert!(
        matches!(&failed.state, State::Failed { error: SourceError::Invalid(why), .. } if why.contains("En cours")),
        "{:?}",
        failed.state
    );
    Ok(())
}

#[test]
fn a_board_not_shared_is_gone_and_sso_and_the_cookie_pair_are_known() -> anyhow::Result<()> {
    let fake = sprint()?;
    let toml = format!(
        "[[day.tasks]]\nkind = \"planka\"\naddress = \"{}\"\nboard = \"{}/boards/77\"\n",
        fake.address, fake.address
    );
    let mut run = start(&toml, Some(TOKEN))?;
    let failed = run.until(|s| matches!(s.state, State::Failed { .. }))?;
    assert_eq!(
        failed.state,
        State::Failed {
            error: SourceError::Gone,
            fetched: None
        }
    );
    drop(run);

    // A server signing in through SSO only, whose web sessions pair the
    // token with an `httpOnlyToken` cookie: the cookies pasted whole.
    fake.with(|b| {
        b.oidc = Some(true);
        b.http_only = Some("29aa3e38-8d24".to_owned());
    });
    let mut run = start(&planka(&fake, ""), Some(TOKEN))?;
    let failed = run.until(|s| matches!(s.state, State::Failed { .. }))?;
    assert!(matches!(
        &failed.state,
        State::Failed {
            error: SourceError::SignIn(_),
            ..
        }
    ));
    let cookies = format!("accessToken={TOKEN}; httpOnlyToken=29aa3e38-8d24; lang=fr");
    run.secrets.keep(&run.account, &Token::new(cookies))?;
    run.workers.refresh();
    let tasks = run.until(fresh)?;
    assert_eq!(tasks.tasks.len(), 3);
    assert_eq!(tasks.login, Some(Login::SsoOnly));
    Ok(())
}

/// The account's id is the one `hocus day planka-token` keeps the token
/// under, whatever the configuration's folder.
#[test]
fn the_tasks_account_is_the_one_the_workers_start() -> anyhow::Result<()> {
    let config = config::from_toml(
        "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\
         [[day.tasks]]\nkind = \"planka\"\naddress = \"https://planka.example.org/boards/1234\"\n",
    )
    .map_err(anyhow::Error::msg)?;
    let (account, entry) = config
        .tasks_account(Some(Path::new("/nowhere")))
        .ok_or_else(|| anyhow::anyhow!("an account"))?;
    assert_eq!(account.id.as_str(), "planka-tasks-planka.example.org-1234");
    assert_eq!(entry.kind, config::TaskKind::Planka);
    Ok(())
}
