//! A Planka board as a [`KanbanSource`] (#566), through Planka's REST API,
//! blocking, on its account's worker: never the shell's loop.
//!
//! - **The board**: `GET /api/boards/{id}`, its `included` lists (the
//!   `active` and `closed` ones are its columns, by position; `archive`
//!   and `trash` are not) and their cards, by position. A card's due date
//!   is its `dueDate`.
//! - **Create**: `POST /api/lists/{listId}/cards` with the board's
//!   `defaultCardType`, at the top: a position below the first card's.
//! - **Move**: `PATCH /api/cards/{id}` with `listId` and `position`.
//! - **Sign-in**: a token pasted from the server (Planka's `accessToken`
//!   cookie, the server may sign in through SSO only, #425's amendment),
//!   sent as a bearer token. Planka 2's web sessions pair it with an
//!   `httpOnlyToken` cookie, which it then wants too: the token kept may
//!   be the browser's cookies for the server (`accessToken=…;
//!   httpOnlyToken=…`), each sent as Planka wants it. The token is read
//!   from [`Secrets`] when first needed, and again after the server
//!   refused it: a token pasted meanwhile is taken at the next read.
//!   `GET /api/users/me` checks a token as it is read.
//! - **How the server signs in** ([`Login`]): its `oidc`, from
//!   `GET /api/bootstrap` (Planka 2) or `GET /api/config` (Planka 1), read
//!   once.
//!
//! A `401` is [`SourceError::SignIn`]; a `404` on the board or a card is
//! [`SourceError::Gone`] (Planka says the same of a board not shared with
//! the user); no answer, or a `5xx`, is [`SourceError::Offline`].

use std::{collections::HashMap, fmt, time::Duration};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    model::{AccountId, BoardId, Column, ColumnId, Place, Task, TaskBoard, TaskId, sort_tasks},
    secrets::{Secrets, Token},
    snapshot::Login,
    source::{KanbanSource, Result, SourceError},
};

/// How long a request may take, connecting included.
const TIMEOUT: Duration = Duration::from_secs(20);
/// The most an answer may weigh (a board with all its cards).
const MOST: u64 = 32 << 20;
/// Planka's own gap between positions: a card's in an empty list.
const GAP: f64 = 65536.0;

/// A Planka server's address, `https://host` (and a path, when it is
/// served under one), without a trailing `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server(String);

impl Server {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Its host (and port, and path): what names it for the user.
    pub fn host(&self) -> &str {
        self.0.split_once("://").map_or(&self.0, |(_, rest)| rest)
    }
}

impl fmt::Display for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The server and the board the configuration's `address` and `board`
/// say: a board's address (`https://host/boards/ID`) in either gives
/// both; else `address` is the server and `board` the board's id. A token
/// goes over `https` only, but to this computer.
pub fn address(address: &str, board: &str) -> Result<(Server, String)> {
    let invalid = |why: String| SourceError::Invalid(why);
    let (address, board) = (address.trim(), board.trim());
    let (server, id) = match (board_address(board), board_address(address)) {
        (Some((server, id)), _) | (None, Some((server, id))) => (server, id),
        (None, None) if !address.is_empty() && !board.is_empty() && !board.contains('/') => {
            (address.trim_end_matches('/').to_owned(), board.to_owned())
        }
        (None, None) if address.is_empty() && board.is_empty() => {
            return Err(invalid(
                "a Planka board has its address (address = \"https://…/boards/ID\")".to_owned(),
            ));
        }
        _ => {
            return Err(invalid(format!(
                "{address:?} and {board:?}: a board's address (https://…/boards/ID), \
                 or a server's and the board's id"
            )));
        }
    };
    let lower = server.to_ascii_lowercase();
    let rest = if let Some(rest) = lower.strip_prefix("https://") {
        rest
    } else if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or("");
        let local = ["localhost", "127.0.0.1", "[::1]"]
            .iter()
            .any(|h| host == *h || host.starts_with(&format!("{h}:")));
        if !local {
            return Err(invalid(format!(
                "{server}: the token goes over https only (https://…)"
            )));
        }
        rest
    } else {
        return Err(invalid(format!("{server}: an address is https://…")));
    };
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return Err(invalid(format!("{server}: no server in this address")));
    }
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(invalid(format!("{id:?}: not a board's id")));
    }
    Ok((Server(server), id))
}

/// `https://host[/path]/boards/ID[/…][?…]`: the server and the id.
fn board_address(text: &str) -> Option<(String, String)> {
    let text = text.split(['?', '#']).next().unwrap_or(text);
    if !text.contains("://") {
        return None;
    }
    let (server, after) = text.rsplit_once("/boards/")?;
    let id = after.split('/').next().unwrap_or("");
    Some((server.trim_end_matches('/').to_owned(), id.to_owned()))
}

/// What a request carries to sign in: the access token, and the
/// `httpOnlyToken` cookie its session was made with, if any.
#[derive(Clone, PartialEq, Eq)]
struct Credentials {
    access: String,
    http_only: Option<String>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credentials(…)")
    }
}

impl Credentials {
    /// A token as pasted: the access token alone, or the server's cookies
    /// (`accessToken=…; httpOnlyToken=…`, as the browser's tools show
    /// them, or a `Cookie:` line).
    fn from_token(token: &Token) -> Option<Self> {
        let text = token.expose().trim();
        let text = text
            .strip_prefix("Cookie:")
            .or_else(|| text.strip_prefix("cookie:"))
            .unwrap_or(text)
            .trim();
        if !text.contains('=') {
            // A JWT has no `=` (base64url), nor a space.
            return (!text.is_empty() && !text.contains(char::is_whitespace)).then(|| Self {
                access: text.to_owned(),
                http_only: None,
            });
        }
        let mut access = None;
        let mut http_only = None;
        for pair in text.split(';') {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"').to_owned();
            match name.trim() {
                "accessToken" => access = Some(value),
                "httpOnlyToken" => http_only = Some(value),
                _ => {}
            }
        }
        Some(Self {
            access: access.filter(|a| !a.is_empty())?,
            http_only: http_only.filter(|h| !h.is_empty()),
        })
    }
}

/// Whether `token` signs in to `server`: the user's name, as
/// `GET /api/users/me` gives it. For `hocus day planka-token`, which
/// checks a token before keeping it.
pub fn check(server: &Server, token: &Token) -> Result<String> {
    let credentials = Credentials::from_token(token)
        .ok_or_else(|| SourceError::SignIn("this is not a token".to_owned()))?;
    let client = Client::new(server.clone());
    let me: Answer<User> = client.get("/api/users/me", Some(&credentials), Not::Found)?;
    Ok(me.item.shown())
}

/// A Planka board.
#[derive(Debug)]
pub struct Planka {
    client: Client,
    board: String,
    account: AccountId,
    secrets: Secrets,
    /// The token read from the secrets, until the server refuses it.
    credentials: Option<Credentials>,
    login: Option<Login>,
    /// The board's type for new cards (Planka 2: `project` or `story`).
    card_type: String,
    /// The cards of each column, as last read or changed: where a top is.
    columns: HashMap<ColumnId, Vec<Task>>,
}

impl Planka {
    /// `board` on `server`, for `account`, whose token `secrets` keeps.
    pub fn new(server: Server, board: String, account: AccountId, secrets: Secrets) -> Self {
        Self {
            client: Client::new(server),
            board,
            account,
            secrets,
            credentials: None,
            login: None,
            card_type: "project".to_owned(),
            columns: HashMap::new(),
        }
    }

    /// The token, read from the secrets if it is not yet (and checked).
    fn credentials(&mut self) -> Result<Credentials> {
        if let Some(credentials) = &self.credentials {
            return Ok(credentials.clone());
        }
        let token = self.secrets.get(&self.account).ok_or_else(|| {
            SourceError::SignIn(format!(
                "no token kept: hocus day planka-token {}",
                self.account
            ))
        })?;
        let credentials = Credentials::from_token(&token)
            .ok_or_else(|| SourceError::SignIn("the token kept is not one".to_owned()))?;
        let me: Answer<User> = self
            .client
            .get("/api/users/me", Some(&credentials), Not::Found)?;
        tracing::info!(account = %self.account, user = %me.item.shown(), "a Planka token taken");
        self.credentials = Some(credentials.clone());
        Ok(credentials)
    }

    /// `request` with the token; a refusal forgets it, for the next to
    /// read it again.
    fn signed<T>(&mut self, request: impl FnOnce(&Client, &Credentials) -> Result<T>) -> Result<T> {
        let credentials = self.credentials()?;
        let answer = request(&self.client, &credentials);
        if matches!(answer, Err(SourceError::SignIn(_))) {
            self.credentials = None;
        }
        answer
    }

    /// The top of `column`'s cards, `card` left out: half the first's
    /// position (Planka's are never below 0, and it makes room itself when
    /// two meet).
    fn top(&self, column: &ColumnId, card: Option<&TaskId>) -> f64 {
        self.columns
            .get(column)
            .into_iter()
            .flatten()
            .filter(|t| Some(&t.id) != card)
            .map(|t| t.position)
            .min_by(f64::total_cmp)
            .map_or(GAP, |first| (first / 2.0).max(0.0))
    }

    /// `card` now at `place`, as the server said.
    fn moved(&mut self, card: Task, column: &ColumnId) {
        for cards in self.columns.values_mut() {
            cards.retain(|t| t.id != card.id);
        }
        let cards = self.columns.entry(column.clone()).or_default();
        cards.push(card);
        sort_tasks(cards);
    }
}

impl KanbanSource for Planka {
    fn board(&mut self) -> Result<TaskBoard> {
        let path = format!("/api/boards/{}", self.board);
        let answer: BoardAnswer =
            self.signed(|client, credentials| client.get(&path, Some(credentials), Not::Gone))?;
        if let Some(kind) = answer.item.default_card_type {
            self.card_type = kind;
        }
        let mut lists: Vec<(f64, Column)> = answer
            .included
            .lists
            .into_iter()
            .filter(|l| {
                l.kind
                    .as_deref()
                    .is_none_or(|k| k == "active" || k == "closed")
            })
            .filter_map(|l| {
                Some((
                    l.position?,
                    Column {
                        id: ColumnId::new(l.id.0),
                        name: l.name?,
                    },
                ))
            })
            .collect();
        lists.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        let mut columns: HashMap<ColumnId, Vec<Task>> = lists
            .iter()
            .map(|(_, c)| (c.id.clone(), Vec::new()))
            .collect();
        for card in answer.included.cards {
            let Some(cards) = columns.get_mut(&ColumnId::new(card.list_id.0.clone())) else {
                continue;
            };
            cards.push(card.task());
        }
        for cards in columns.values_mut() {
            sort_tasks(cards);
        }
        self.columns = columns;
        Ok(TaskBoard {
            id: BoardId::new(answer.item.id.0),
            name: answer.item.name,
            columns: lists.into_iter().map(|(_, c)| c).collect(),
        })
    }

    fn cards(&mut self, column: &ColumnId) -> Result<Vec<Task>> {
        Ok(self.columns.get(column).cloned().unwrap_or_default())
    }

    fn create(&mut self, column: &ColumnId, title: &str) -> Result<Task> {
        let path = format!("/api/lists/{column}/cards");
        let body = json!({
            "type": self.card_type,
            "position": self.top(column, None),
            "name": title,
        });
        let answer: Answer<Card> = self.signed(|client, credentials| {
            client.send("POST", &path, &body, credentials, Not::Refused)
        })?;
        let task = answer.item.task();
        self.moved(task.clone(), column);
        Ok(task)
    }

    fn move_card(&mut self, card: &TaskId, place: &Place) -> Result<()> {
        let position = place
            .position
            .unwrap_or_else(|| self.top(&place.column, Some(card)));
        let path = format!("/api/cards/{card}");
        let body = json!({"listId": place.column.as_str(), "position": position});
        let answer: Answer<Card> = self.signed(|client, credentials| {
            client.send("PATCH", &path, &body, credentials, Not::Gone)
        })?;
        self.moved(answer.item.task(), &place.column);
        Ok(())
    }

    fn login(&mut self) -> Option<Login> {
        if self.login.is_none() {
            self.login = self.client.login();
        }
        self.login
    }
}

/// What a `404` means for a request.
#[derive(Debug, Clone, Copy)]
enum Not {
    /// The board or the card is no more (or not shared with the user).
    Gone,
    /// What the change goes to is not there: kept, as refused.
    Refused,
    /// Nothing there (a server too old for it).
    Found,
}

/// The server's HTTP client.
struct Client {
    server: Server,
    agent: ureq::Agent,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

impl Client {
    fn new(server: Server) -> Self {
        Self {
            server,
            agent: crate::http::agent(TIMEOUT),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server)
    }

    fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        credentials: Option<&Credentials>,
        not: Not,
    ) -> Result<T> {
        let mut request = self.agent.get(self.url(path));
        if let Some(credentials) = credentials {
            request = signed(request, credentials);
        }
        read(request.call(), path, not)
    }

    fn send<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        path: &str,
        body: &Value,
        credentials: &Credentials,
        not: Not,
    ) -> Result<T> {
        let url = self.url(path);
        let request = match method {
            "POST" => self.agent.post(url),
            _ => self.agent.patch(url),
        };
        let answer = signed(request, credentials)
            .content_type("application/json")
            .send(body.to_string());
        read(answer, path, not)
    }

    /// How the server signs in: Planka 2's `/api/bootstrap`, else Planka
    /// 1's `/api/config`; `None` when neither says.
    fn login(&self) -> Option<Login> {
        #[derive(Deserialize)]
        struct Item {
            #[serde(default)]
            oidc: Option<Oidc>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Oidc {
            #[serde(default)]
            is_enforced: bool,
        }
        let read = |path: &str| self.get::<Answer<Item>>(path, None, Not::Found);
        let item = match read("/api/bootstrap") {
            Ok(answer) => answer.item,
            Err(SourceError::Invalid(_)) => match read("/api/config") {
                Ok(answer) => answer.item,
                Err(error) => {
                    tracing::debug!(server = %self.server, %error, "how the server signs in is not known");
                    return None;
                }
            },
            Err(error) => {
                tracing::debug!(server = %self.server, %error, "how the server signs in is not known");
                return None;
            }
        };
        Some(match item.oidc {
            None => Login::Password,
            Some(Oidc { is_enforced: false }) => Login::PasswordOrSso,
            Some(Oidc { is_enforced: true }) => Login::SsoOnly,
        })
    }
}

fn signed<B>(
    request: ureq::RequestBuilder<B>,
    credentials: &Credentials,
) -> ureq::RequestBuilder<B> {
    let request = request.header("Authorization", format!("Bearer {}", credentials.access));
    match &credentials.http_only {
        Some(pair) => request.header("Cookie", format!("httpOnlyToken={pair}")),
        None => request,
    }
}

/// An answer read: its JSON, or what its status means.
fn read<T: for<'de> Deserialize<'de>>(
    answer: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    path: &str,
    not: Not,
) -> Result<T> {
    let (status, text) = crate::http::read(answer, path, MOST)?;
    match status {
        200..=299 => serde_json::from_str(&text)
            .map_err(|err| SourceError::Invalid(format!("{path}: not Planka's answer: {err}"))),
        401 => Err(SourceError::SignIn(
            "the token is refused or expired: paste a new one".to_owned(),
        )),
        404 => Err(match not {
            Not::Gone => SourceError::Gone,
            Not::Refused => SourceError::Refused(format!("{path}: not there")),
            Not::Found => SourceError::Invalid(format!("{path}: not there")),
        }),
        403 => Err(SourceError::Refused(format!("{path}: not enough rights"))),
        300..=399 => Err(SourceError::Invalid(format!(
            "{path}: sent elsewhere ({status}), not Planka's API"
        ))),
        500..=599 => Err(SourceError::Offline(format!(
            "{path}: the server says {status}"
        ))),
        _ => Err(SourceError::Refused(format!(
            "{path}: {status} {}",
            first_words(&text)
        ))),
    }
}

/// What a refusal says, short, for the log.
fn first_words(text: &str) -> String {
    text.chars().take(160).collect()
}

/// Planka's `{"item": …}`.
#[derive(Deserialize)]
struct Answer<T> {
    item: T,
}

/// An id: a string (Planka's big integers), or a number.
#[derive(Debug, Clone)]
struct Id(String);

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::String(s) => Ok(Self(s)),
            Value::Number(n) => Ok(Self(n.to_string())),
            other => Err(serde::de::Error::custom(format!("{other}: not an id"))),
        }
    }
}

#[derive(Deserialize)]
struct User {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

impl User {
    fn shown(&self) -> String {
        [&self.name, &self.username, &self.email]
            .into_iter()
            .flatten()
            .find(|n| !n.is_empty())
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Deserialize)]
struct BoardAnswer {
    item: BoardItem,
    included: Included,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoardItem {
    id: Id,
    #[serde(default)]
    name: String,
    /// Planka 2's; Planka 1 has no card types.
    #[serde(default)]
    default_card_type: Option<String>,
}

#[derive(Deserialize)]
struct Included {
    #[serde(default)]
    lists: Vec<List>,
    #[serde(default)]
    cards: Vec<Card>,
}

#[derive(Deserialize)]
struct List {
    id: Id,
    /// `None` for Planka 2's archive and trash.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    position: Option<f64>,
    /// Planka 2's `active`, `closed`, `archive`, `trash`; none in Planka 1.
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Card {
    id: Id,
    list_id: Id,
    #[serde(default)]
    name: String,
    #[serde(default)]
    position: Option<f64>,
    #[serde(default)]
    due_date: Option<String>,
}

impl Card {
    fn task(self) -> Task {
        let due = self.due_date.as_deref().and_then(|due| {
            due.parse()
                .inspect_err(|err| tracing::debug!(due, %err, "a card's due date does not read"))
                .ok()
        });
        Task {
            id: TaskId::new(self.id.0),
            title: self.name,
            due,
            position: self.position.unwrap_or(f64::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boards_address_gives_its_server_and_its_id() {
        let ok =
            |a: &str, b: &str| address(a, b).map(|(server, id)| (server.as_str().to_owned(), id));
        let expected = Ok(("https://planka.example.org".to_owned(), "1234".to_owned()));
        assert_eq!(ok("https://planka.example.org/boards/1234", ""), expected);
        assert_eq!(ok("", "https://planka.example.org/boards/1234/"), expected);
        assert_eq!(ok("https://planka.example.org/", "1234"), expected);
        assert_eq!(
            ok(
                "https://planka.example.org",
                "https://planka.example.org/boards/1234?x=1"
            ),
            expected
        );
        assert_eq!(
            ok("https://example.org/planka/boards/9/cards/3", ""),
            Ok(("https://example.org/planka".to_owned(), "9".to_owned()))
        );
        // Plain http to this computer only: the token is a password.
        assert!(ok("http://127.0.0.1:8080", "1").is_ok());
        assert!(ok("http://localhost/boards/1", "").is_ok());
        for (a, b) in [
            ("http://planka.example.org", "1"),
            ("https://planka.example.org", ""),
            ("", ""),
            ("planka.example.org", "1"),
            ("https://alex@planka.example.org", "1"),
            ("https://planka.example.org", "1/2"),
            ("https://planka.example.org/boards/", ""),
        ] {
            assert!(
                matches!(address(a, b), Err(SourceError::Invalid(_))),
                "{a:?} {b:?}"
            );
        }
    }

    #[test]
    fn a_token_is_pasted_alone_or_with_its_cookie_pair() {
        let parse = |t: &str| Credentials::from_token(&Token::new(t));
        assert_eq!(
            parse(" eyJh.e30.sig \n"),
            Some(Credentials {
                access: "eyJh.e30.sig".to_owned(),
                http_only: None
            })
        );
        let pair = Some(Credentials {
            access: "eyJh.e30.sig".to_owned(),
            http_only: Some("29aa-3e38".to_owned()),
        });
        assert_eq!(
            parse("accessToken=eyJh.e30.sig; httpOnlyToken=29aa-3e38; lang=fr"),
            pair
        );
        assert_eq!(
            parse("Cookie: lang=fr; httpOnlyToken=29aa-3e38; accessToken=eyJh.e30.sig"),
            pair
        );
        assert_eq!(parse(""), None);
        assert_eq!(parse("two words"), None);
        assert_eq!(parse("httpOnlyToken=29aa"), None);
        assert_eq!(format!("{:?}", parse("secret")), "Some(Credentials(…))");
    }
}
