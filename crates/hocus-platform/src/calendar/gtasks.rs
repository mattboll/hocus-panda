//! Google Tasks (#257): not CalDAV, its own REST API (tasks/v1), with the
//! access token GNOME Online Accounts hands out for a Google account
//! ([`super::goa`]):
//!
//! - `GET users/@me/lists` lists the task lists, `GET lists/{list}/tasks`
//!   their tasks (`showCompleted` and `showHidden`: those done too, which
//!   the column shows the day they were done), a page at a time;
//! - `POST lists/{list}/tasks` adds one, `PATCH lists/{list}/tasks/{task}`
//!   changes the fields given, `DELETE` removes it.
//!
//! A task is JSON: `id`, `title`, `status` (`needsAction` or `completed`),
//! `due` (RFC 3339, of which Google keeps the date only), `completed`,
//! `updated`. A change sends only the fields the desktop changed, so that
//! one made elsewhere to another field stays.
//!
//! A token without the tasks scope (`https://www.googleapis.com/auth/tasks`)
//! is refused with a 403 that says so: the account does not share its
//! tasks ([`Error::Unshared`]), which the settings show; nothing else
//! stops for it. The token goes to the API's own server only, and is never
//! printed.
//!
//! Blocking: worker threads call it, never the loop.

use jiff::{Timestamp, civil::Date};
use serde_json::{Map, Value};

use super::{
    caldav::{Client, FetchError, Token},
    ics::When,
};
use crate::tasks::{Source, Task};

/// Google Tasks' API, as Google names it.
pub const GOOGLE_TASKS: &str = "https://tasks.googleapis.com/tasks/v1/";
/// The scope a token needs for it.
pub const SCOPE: &str = "https://www.googleapis.com/auth/tasks";
/// Tasks asked for at a time, as many as the API gives.
const PAGE: usize = 100;
/// Pages read of one list at most: years of tasks done stay at Google.
const PAGES: usize = 20;

/// Why Google Tasks could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The token was refused (401): renewed once, it may be taken.
    Refused(String),
    /// The token does not grant the tasks scope, or the API is not open to
    /// the account: its tasks are not shared.
    Unshared(String),
    /// Unreachable, or an answer not understood.
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(e) | Self::Unshared(e) | Self::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Error {}

impl From<FetchError> for Error {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::Refused(e) => Self::Refused(e),
            FetchError::Failed(e) => Self::Failed(e),
        }
    }
}

/// A task list, with its tasks.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub id: String,
    pub name: String,
    pub tasks: Vec<Task>,
}

/// `text` as one segment of a path or a query's value.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The base with its final slash.
fn base_of(base: &str) -> String {
    format!("{}/", base.trim_end_matches('/'))
}

/// Says the account's tasks are not shared: the token lacks the scope
/// (Google's `insufficientPermissions`, `ACCESS_TOKEN_SCOPE_INSUFFICIENT`,
/// `insufficient_scope`), or the API is not open to it.
fn unshared(challenge: Option<&str>, body: &str) -> bool {
    challenge.is_some_and(|c| c.contains("insufficient_scope"))
        || [
            "insufficientPermissions",
            "ACCESS_TOKEN_SCOPE_INSUFFICIENT",
            "accessNotConfigured",
            "SERVICE_DISABLED",
        ]
        .iter()
        .any(|r| body.contains(r))
}

/// Google's own words for an error, short.
fn reason(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A request to the API at `url`, its answer's text when it took it; a
/// gone resource is `None` when `gone_ok`.
fn call(
    client: &Client,
    method: &str,
    url: &str,
    body: Option<&str>,
    gone_ok: bool,
) -> Result<Option<String>, Error> {
    let headers: &[(&str, &str)] = match body {
        Some(_) => &[("Content-Type", "application/json; charset=utf-8")],
        None => &[],
    };
    // The address without its query, in what is said of it.
    let at = url.split('?').next().unwrap_or(url);
    let answer = client.exchange(method, url, headers, body.unwrap_or(""))?;
    match answer.status {
        200..=299 => Ok(Some(answer.text)),
        404 | 410 if gone_ok => Ok(None),
        401 => Err(Error::Refused(format!(
            "{at}: Google refused the access token"
        ))),
        403 if unshared(answer.challenge.as_deref(), &answer.text) => Err(Error::Unshared(
            format!("{at}: the account's token does not grant Google Tasks ({SCOPE})"),
        )),
        s => Err(Error::Failed(format!(
            "{at}: Google answered {s} {}",
            reason(&answer.text)
        ))),
    }
}

/// Every item of a paged collection at `url`, a page at a time.
fn paged(client: &Client, url: &str, joiner: char) -> Result<Vec<Value>, Error> {
    let mut items = Vec::new();
    let mut next: Option<String> = None;
    for _ in 0..PAGES {
        let mut at = format!("{url}{joiner}maxResults={PAGE}");
        if let Some(token) = &next {
            at.push_str(&format!("&pageToken={}", escape(token)));
        }
        let text = call(client, "GET", &at, None, false)?.unwrap_or_default();
        let page: Value = serde_json::from_str(&text)
            .map_err(|e| Error::Failed(format!("{url}: not understood: {e}")))?;
        if let Some(found) = page["items"].as_array() {
            items.extend(found.iter().cloned());
        }
        next = page["nextPageToken"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        if next.is_none() {
            break;
        }
    }
    Ok(items)
}

/// Every task list of the account behind `token`, at `base` (Google's API,
/// or a test's stand-in), with its tasks, those of `account`.
pub fn lists(base: &str, token: &Token, account: &str) -> Result<Vec<List>, Error> {
    let base = base_of(base);
    let client = Client::bearer(token, &base);
    let mut out = Vec::new();
    for list in paged(&client, &format!("{base}users/@me/lists"), '?')? {
        let Some(id) = list["id"].as_str().filter(|i| !i.is_empty()) else {
            continue;
        };
        let name = list["title"]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(id)
            .to_owned();
        let url = format!(
            "{base}lists/{}/tasks?showCompleted=true&showHidden=true",
            escape(id)
        );
        let tasks = paged(&client, &url, '&')?
            .iter()
            .filter_map(|t| task_of(t, account, id, &name))
            .collect();
        out.push(List {
            id: id.to_owned(),
            name,
            tasks,
        });
    }
    Ok(out)
}

/// A task as Google writes it, of `account`'s list `list` named `name`;
/// `None` for one deleted, or without an id or a text.
pub fn parse(text: &str, account: &str, list: &str, name: &str) -> Option<Task> {
    task_of(&serde_json::from_str(text).ok()?, account, list, name)
}

fn task_of(v: &Value, account: &str, list: &str, name: &str) -> Option<Task> {
    let id = v["id"].as_str().filter(|i| !i.is_empty())?;
    if v["deleted"].as_bool() == Some(true) {
        return None;
    }
    let summary = v["title"]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())?;
    let moment = |key: &str| v[key].as_str().and_then(|t| t.parse::<Timestamp>().ok());
    let due = v["due"]
        .as_str()
        .and_then(|d| d.get(..10))
        .and_then(|d| Date::strptime("%Y-%m-%d", d).ok())
        .map(When::Date);
    Some(Task {
        uid: id.to_owned(),
        summary: summary.to_owned(),
        done: v["status"].as_str() == Some("completed"),
        completed: moment("completed"),
        due,
        // Google keeps no creation time: its last change stands for it.
        created: moment("updated"),
        list: Some(name.to_owned()),
        source: Source::Google {
            account: account.to_owned(),
            list: list.to_owned(),
            id: Some(id.to_owned()),
        },
        raw: serde_json::to_string(v).unwrap_or_default(),
    })
}

/// A moment as Google writes it.
fn rfc3339(at: Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

/// A due date as Google keeps it: the day alone, at midnight UTC.
fn due_of(due: &When) -> String {
    let day = match due {
        When::Date(d) => *d,
        When::Local(dt, _) => dt.date(),
        When::Utc(t) => t.to_zoned(jiff::tz::TimeZone::UTC).date(),
    };
    format!("{}T00:00:00.000Z", day.strftime("%Y-%m-%d"))
}

/// What to send of `task`: the fields that differ from `before` (all of
/// them for a new one, `before` being `None`).
pub fn body(task: &Task, before: Option<&Task>) -> String {
    let mut fields = Map::new();
    if before.is_none_or(|b| b.summary != task.summary) {
        fields.insert("title".into(), Value::from(task.summary.clone()));
    }
    if before.is_none_or(|b| b.done != task.done || b.completed != task.completed) {
        let status = if task.done {
            "completed"
        } else {
            "needsAction"
        };
        fields.insert("status".into(), Value::from(status));
        match task.completed.filter(|_| task.done) {
            Some(at) => {
                fields.insert("completed".into(), Value::from(rfc3339(at)));
            }
            // Undone, its completion goes.
            None if before.is_some() => {
                fields.insert("completed".into(), Value::Null);
            }
            None => {}
        }
    }
    if before.is_none_or(|b| b.due != task.due) {
        match &task.due {
            Some(due) => {
                fields.insert("due".into(), Value::from(due_of(due)));
            }
            None if before.is_some() => {
                fields.insert("due".into(), Value::Null);
            }
            None => {}
        }
    }
    Value::Object(fields).to_string()
}

/// The id of a task as Google wrote it.
pub fn id_of(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    v["id"]
        .as_str()
        .filter(|i| !i.is_empty())
        .map(str::to_owned)
}

/// Adds a task to the list `list`: Google's text of it, with its id.
pub fn insert(base: &str, token: &Token, list: &str, body: &str) -> Result<String, Error> {
    let base = base_of(base);
    let url = format!("{base}lists/{}/tasks", escape(list));
    call(
        &Client::bearer(token, &base),
        "POST",
        &url,
        Some(body),
        false,
    )
    .map(Option::unwrap_or_default)
}

/// Changes the fields `body` gives of the task `id`: Google's text of it;
/// `None` when it is gone, deleted elsewhere.
pub fn patch(
    base: &str,
    token: &Token,
    list: &str,
    id: &str,
    body: &str,
) -> Result<Option<String>, Error> {
    let base = base_of(base);
    let url = format!("{base}lists/{}/tasks/{}", escape(list), escape(id));
    call(
        &Client::bearer(token, &base),
        "PATCH",
        &url,
        Some(body),
        true,
    )
}

/// Removes the task `id`; one already gone is gone.
pub fn delete(base: &str, token: &Token, list: &str, id: &str) -> Result<(), Error> {
    let base = base_of(base);
    let url = format!("{base}lists/{}/tasks/{}", escape(list), escape(id));
    call(&Client::bearer(token, &base), "DELETE", &url, None, true).map(|_| ())
}

#[cfg(test)]
mod tests {
    use jiff::civil::{date, datetime};

    use super::*;

    const BAMBOO: &str = r#"{"kind":"tasks#task","id":"YmFtYm9v","etag":"\"LTE4\"","title":"Water the bamboo","updated":"2026-10-19T08:00:00.000Z","status":"needsAction","due":"2026-10-21T00:00:00.000Z","notes":"The big one","position":"00000000000000000001"}"#;

    #[test]
    fn a_task_reads_with_its_date_and_status() {
        let task = parse(BAMBOO, "goa:a", "bXk", "My Tasks").expect("a task");
        assert_eq!(task.uid, "YmFtYm9v");
        assert_eq!(task.summary, "Water the bamboo");
        assert!(!task.done);
        assert_eq!(task.due, Some(When::Date(date(2026, 10, 21))));
        assert_eq!(task.list.as_deref(), Some("My Tasks"));
        assert!(task.created.is_some());
        assert_eq!(
            task.source,
            Source::Google {
                account: "goa:a".into(),
                list: "bXk".into(),
                id: Some("YmFtYm9v".into()),
            }
        );
        // What it was read with is kept, for the cache.
        assert_eq!(parse(&task.raw, "goa:a", "bXk", "My Tasks"), Some(task));
        let done = BAMBOO.replace(
            r#""status":"needsAction""#,
            r#""status":"completed","completed":"2026-10-21T09:30:00.000Z""#,
        );
        let done = parse(&done, "goa:a", "bXk", "My Tasks").expect("a task");
        assert!(done.done);
        assert_eq!(done.completed, "2026-10-21T09:30:00Z".parse().ok());
        // Deleted, or without a text: none.
        let deleted = BAMBOO.replace(r#""title""#, r#""deleted":true,"title""#);
        assert_eq!(parse(&deleted, "goa:a", "bXk", "My Tasks"), None);
        let blank = BAMBOO.replace("Water the bamboo", " ");
        assert_eq!(parse(&blank, "goa:a", "bXk", "My Tasks"), None);
    }

    #[test]
    fn a_change_sends_only_what_changed() {
        let before = parse(BAMBOO, "goa:a", "bXk", "My Tasks").expect("a task");
        let now: Timestamp = "2026-10-21T14:37:00Z".parse().expect("a time");
        let mut ticked = before.clone();
        ticked.set_done(true, now);
        let sent: Value = serde_json::from_str(&body(&ticked, Some(&before))).expect("JSON");
        assert_eq!(
            sent,
            serde_json::json!({"status": "completed", "completed": "2026-10-21T14:37:00.000Z"})
        );
        let mut unticked = ticked.clone();
        unticked.set_done(false, now);
        let sent: Value = serde_json::from_str(&body(&unticked, Some(&ticked))).expect("JSON");
        assert_eq!(
            sent,
            serde_json::json!({"status": "needsAction", "completed": null})
        );
        let mut edited = before.clone();
        edited.set_summary("Water the fern");
        edited.due = None;
        let sent: Value = serde_json::from_str(&body(&edited, Some(&before))).expect("JSON");
        assert_eq!(
            sent,
            serde_json::json!({"title": "Water the fern", "due": null})
        );
        // A new one: all of it, nothing null.
        let new = Task::new_google(
            "goa:a",
            "bXk",
            "My Tasks",
            "Renew the passport",
            Some(When::Local(datetime(2026, 10, 22, 15, 0, 0, 0), None)),
            now,
        )
        .expect("new");
        let sent: Value = serde_json::from_str(&body(&new, None)).expect("JSON");
        assert_eq!(
            sent,
            serde_json::json!({"title": "Renew the passport", "status": "needsAction", "due": "2026-10-22T00:00:00.000Z"})
        );
        assert_eq!(id_of(BAMBOO).as_deref(), Some("YmFtYm9v"));
    }

    #[test]
    fn a_token_without_the_scope_is_told_apart() {
        let scope = r#"{"error":{"code":403,"message":"Request had insufficient authentication scopes.","errors":[{"message":"Insufficient Permission","domain":"global","reason":"insufficientPermissions"}],"status":"PERMISSION_DENIED","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#;
        assert!(unshared(None, scope));
        assert!(unshared(
            Some(r#"Bearer realm="https://accounts.google.com/", error="insufficient_scope""#),
            ""
        ));
        let limited = r#"{"error":{"code":403,"message":"Rate Limit Exceeded","errors":[{"reason":"rateLimitExceeded"}]}}"#;
        assert!(!unshared(None, limited));
        assert_eq!(reason(limited), "Rate Limit Exceeded");
        assert_eq!(escape("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(escape("MTIz-_x.~"), "MTIz-_x.~");
    }
}
