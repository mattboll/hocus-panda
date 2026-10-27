//! Fetching calendars: a CalDAV account (RFC 4791), found from its server,
//! its principal or a calendar's own address, or a subscription (an ICS
//! link). The calendars that say they hold VTODO are task lists (#256):
//! their tasks come with their ETags, and are written back with PUT and
//! DELETE on those ETags, a change made elsewhere meanwhile refused (412).
//! Blocking: the compositor calls it on worker threads.

use std::time::Duration;

use base64::Engine;
use jiff::Timestamp;
use quick_xml::{Reader, events::Event as Xml};

/// Where events come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    CalDav {
        url: String,
        user: String,
        password: String,
    },
    /// An ICS link, read as it is.
    Subscription { url: String },
    /// A CalDAV account behind an OAuth access token (Google's, through
    /// GNOME Online Accounts): found from `url`, and the token goes to
    /// `url`'s own server only.
    Bearer { url: String, token: Token },
}

/// An OAuth access token: never printed, never kept.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(pub String);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

/// Why a fetch failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The server refused the name or password.
    Refused(String),
    /// Unreachable, or an answer not understood.
    Failed(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(e) | Self::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for FetchError {}

/// One calendar's events, as iCalendar texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub calendar: String,
    pub ics: Vec<String>,
}

/// A task list: a calendar that holds VTODO, and its tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskList {
    /// Its address, which names it.
    pub url: String,
    /// Its name; its address's last part when it has none.
    pub name: String,
    /// `#RRGGBB` or `#RRGGBBAA`, when the server keeps one.
    pub color: Option<String>,
    pub items: Vec<Resource>,
}

/// One calendar object: its address, its ETag, its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub href: String,
    pub etag: Option<String>,
    pub ics: String,
}

/// What an account's sync brings: its calendars' events, its task lists.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Synced {
    pub events: Vec<Fetched>,
    pub lists: Vec<TaskList>,
}

/// What a write expects of the resource on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// Nothing there yet: a new one.
    New,
    /// As it was at this ETag; changed since, the write is refused.
    Etag(String),
    /// Whatever it is (its ETag was never known).
    Any,
}

/// Why a write failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// Changed on the server since it was read (412).
    Conflict,
    /// The server refused the name or password.
    Refused(String),
    /// Unreachable, or refused for another reason.
    Failed(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => f.write_str("changed on the server meanwhile"),
            Self::Refused(e) | Self::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<FetchError> for WriteError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::Refused(e) => Self::Refused(e),
            FetchError::Failed(e) => Self::Failed(e),
        }
    }
}

const PROPFIND: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/">
  <d:prop><d:resourcetype/><d:displayname/><d:current-user-principal/><c:calendar-home-set/><c:supported-calendar-component-set/><a:calendar-color/></d:prop>
</d:propfind>"#;

/// Every task of a list, with its ETag: those done too, which the column
/// shows the day they were done.
const TASKS_REPORT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><d:getetag/><c:calendar-data/></d:prop>
  <c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VTODO"/></c:comp-filter></c:filter>
</c:calendar-query>"#;

fn report(from: Timestamp, to: Timestamp) -> String {
    let at = |t: Timestamp| t.strftime("%Y%m%dT%H%M%SZ").to_string();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><c:calendar-data/></d:prop>
  <c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT">
    <c:time-range start="{}" end="{}"/>
  </c:comp-filter></c:comp-filter></c:filter>
</c:calendar-query>"#,
        at(from),
        at(to)
    )
}

/// What a multistatus says of one resource.
#[derive(Debug, Default, Clone)]
struct Response {
    href: String,
    calendar: bool,
    collection: bool,
    name: String,
    principal: Option<String>,
    home: Option<String>,
    data: String,
    etag: String,
    color: String,
    /// The components it says it holds; `None` when it does not say.
    components: Option<Vec<String>>,
}

impl Response {
    /// Says it holds `component`. A calendar that does not say holds
    /// events only, as far as the desktop goes: task lists say so.
    fn holds(&self, component: &str) -> bool {
        match &self.components {
            Some(c) => c.iter().any(|c| c.eq_ignore_ascii_case(component)),
            None => component == "VEVENT",
        }
    }
}

/// A component of a `supported-calendar-component-set`: its name.
fn component(r: &mut Response, e: &quick_xml::events::BytesStart<'_>) {
    let name = e
        .attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == b"name")
        .map(|a| String::from_utf8_lossy(&a.value).trim().to_owned());
    if let Some(name) = name.filter(|n| !n.is_empty()) {
        r.components.get_or_insert_with(Vec::new).push(name);
    }
}

/// A WebDAV multistatus, by local names: namespaces' prefixes differ
/// between servers.
fn multistatus(xml: &str) -> Vec<Response> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut current: Option<Response> = None;
    let mut path: Vec<String> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Xml::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name == "response" {
                    current = Some(Response::default());
                }
                if let (Some(r), Some(parent)) = (current.as_mut(), path.last()) {
                    match (parent.as_str(), name.as_str()) {
                        ("supported-calendar-component-set", "comp") => component(r, &e),
                        (_, "supported-calendar-component-set") => {
                            r.components.get_or_insert_with(Vec::new);
                        }
                        _ => {}
                    }
                }
                path.push(name);
            }
            Ok(Xml::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if let (Some(r), Some(parent)) = (current.as_mut(), path.last()) {
                    match (parent.as_str(), name.as_str()) {
                        ("resourcetype", "calendar") => r.calendar = true,
                        ("resourcetype", "collection") => r.collection = true,
                        ("supported-calendar-component-set", "comp") => component(r, &e),
                        _ => {}
                    }
                }
            }
            Ok(Xml::End(_)) => {
                if path.pop().as_deref() == Some("response")
                    && let Some(r) = current.take()
                {
                    out.push(r);
                }
            }
            Ok(Xml::Text(t)) => {
                let text = t
                    .xml10_content()
                    .map(|c| c.into_owned())
                    .unwrap_or_default();
                text_into(&mut current, &path, &text);
            }
            // `&lt;` and the like come apart from the text around them.
            Ok(Xml::GeneralRef(r)) => {
                let name = String::from_utf8_lossy(&r).into_owned();
                let text = match name.as_str() {
                    "lt" => "<".to_owned(),
                    "gt" => ">".to_owned(),
                    "amp" => "&".to_owned(),
                    "quot" => "\"".to_owned(),
                    "apos" => "'".to_owned(),
                    _ => r
                        .resolve_char_ref()
                        .ok()
                        .flatten()
                        .map(String::from)
                        .unwrap_or_default(),
                };
                text_into(&mut current, &path, &text);
            }
            Ok(Xml::CData(t)) => {
                let text = String::from_utf8_lossy(&t).into_owned();
                text_into(&mut current, &path, &text);
            }
            Ok(Xml::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The events of a REPORT's answer; for robustness tests, which feed it
/// what a server should never send.
#[doc(hidden)]
pub fn calendar_data(xml: &str) -> Vec<String> {
    multistatus(xml).into_iter().map(|r| r.data).collect()
}

fn text_into(current: &mut Option<Response>, path: &[String], text: &str) {
    let Some(r) = current.as_mut() else {
        return;
    };
    let last = path.last().map(String::as_str);
    let parent = path
        .len()
        .checked_sub(2)
        .and_then(|i| path.get(i))
        .map(String::as_str);
    match (parent, last) {
        (Some("response"), Some("href")) => r.href.push_str(text.trim()),
        (Some("current-user-principal"), Some("href")) => {
            r.principal = Some(text.trim().to_owned())
        }
        (Some("calendar-home-set"), Some("href")) => r.home = Some(text.trim().to_owned()),
        (_, Some("displayname")) => r.name.push_str(text),
        (_, Some("calendar-data")) => r.data.push_str(text),
        (_, Some("getetag")) => r.etag.push_str(text.trim()),
        (_, Some("calendar-color")) => r.color.push_str(text.trim()),
        _ => {}
    }
}

/// `href` against `base`: absolute, or a path on its server.
fn join(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_owned();
    }
    format!("{}{}", origin(base), href)
}

/// `scheme://host[:port]` of `url`.
fn origin(url: &str) -> &str {
    let end = url
        .find("://")
        .and_then(|i| url[i + 3..].find('/').map(|j| i + 3 + j))
        .unwrap_or(url.len());
    &url[..end]
}

pub(crate) struct Client {
    agent: ureq::Agent,
    auth: Option<String>,
    /// A token goes to this origin only, whatever the server's answers
    /// point at.
    only: Option<String>,
}

impl Client {
    fn new(user: Option<(&str, &str)>) -> Self {
        let auth = user.map(|(u, p)| {
            let token = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"));
            format!("Basic {token}")
        });
        Self {
            agent: agent(),
            auth,
            only: None,
        }
    }

    pub(crate) fn bearer(token: &Token, url: &str) -> Self {
        Self {
            agent: agent(),
            auth: Some(format!("Bearer {}", token.0)),
            only: Some(origin(url).to_owned()),
        }
    }

    fn send(&self, method: &str, url: &str, depth: &str, body: &str) -> Result<String, FetchError> {
        let answer = self.request(
            method,
            url,
            &[
                ("Depth", depth),
                ("Content-Type", "application/xml; charset=utf-8"),
            ],
            body,
        )?;
        match answer.status {
            200..=299 => Ok(answer.text),
            s => Err(FetchError::Failed(format!(
                "{url}: the server answered {s}"
            ))),
        }
    }

    /// A request and its answer, whatever its status but a refusal of
    /// the name, password or token.
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<Answer, FetchError> {
        let answer = self.exchange(method, url, headers, body)?;
        if matches!(answer.status, 401 | 403) {
            return Err(FetchError::Refused(format!(
                "{url}: the server refused the {}",
                if self.only.is_some() {
                    "access token"
                } else {
                    "name or password"
                }
            )));
        }
        Ok(answer)
    }

    /// A request and its answer, whatever its status.
    pub(crate) fn exchange(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<Answer, FetchError> {
        let failed = |e: &dyn std::fmt::Display| FetchError::Failed(format!("{url}: {e}"));
        if self.only.as_deref().is_some_and(|o| o != origin(url)) {
            return Err(failed(&"elsewhere than the account's server"));
        }
        let mut request = ureq::http::Request::builder().method(method).uri(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some(auth) = &self.auth {
            request = request.header("Authorization", auth);
        }
        let request = request.body(body.to_owned()).map_err(|e| failed(&e))?;
        let mut response = self.agent.run(request).map_err(|e| failed(&e))?;
        let status = response.status().as_u16();
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        let challenge = response
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| failed(&e))?;
        Ok(Answer {
            status,
            etag,
            challenge,
            text,
        })
    }
}

/// What a server answered.
pub(crate) struct Answer {
    pub(crate) status: u16,
    etag: Option<String>,
    /// Its `WWW-Authenticate`, which says why a token was refused.
    pub(crate) challenge: Option<String>,
    pub(crate) text: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .allow_non_standard_methods(true)
        .http_status_as_error(false)
        .build()
        .into()
}

/// The calendars of a CalDAV account: the address itself when it is one,
/// else those of its home, found through the principal if needed; each
/// with its address.
fn calendars(client: &Client, url: &str) -> Result<Vec<Response>, FetchError> {
    let nothing = |at: &str| FetchError::Failed(format!("{at}: no calendars here"));
    let here = multistatus(&client.send("PROPFIND", url, "0", PROPFIND)?);
    let first = here.first().cloned().unwrap_or_default();
    if first.calendar {
        return Ok(vec![Response {
            href: url.to_owned(),
            ..first
        }]);
    }
    let home = match (first.home, first.principal) {
        (Some(home), _) => join(url, &home),
        (None, Some(principal)) => {
            let at = join(url, &principal);
            let principal = multistatus(&client.send("PROPFIND", &at, "0", PROPFIND)?);
            let home = principal
                .first()
                .and_then(|r| r.home.clone())
                .ok_or_else(|| nothing(&at))?;
            join(url, &home)
        }
        (None, None) => url.to_owned(),
    };
    let listed = multistatus(&client.send("PROPFIND", &home, "1", PROPFIND)?);
    let found: Vec<Response> = listed
        .into_iter()
        .filter(|r| r.calendar)
        .map(|r| Response {
            href: join(url, &r.href),
            ..r
        })
        .collect();
    if found.is_empty() {
        return Err(nothing(url));
    }
    Ok(found)
}

/// Every calendar of `source`, with the events that overlap `from`..`to`
/// (a subscription gives them all).
pub fn fetch(source: &Source, from: Timestamp, to: Timestamp) -> Result<Vec<Fetched>, FetchError> {
    sync(source, from, to).map(|s| s.events)
}

/// Every calendar of `source` with the events that overlap `from`..`to`
/// (a subscription gives them all), and its task lists with their tasks.
pub fn sync(source: &Source, from: Timestamp, to: Timestamp) -> Result<Synced, FetchError> {
    match source {
        Source::Subscription { url } => {
            let url = url.replacen("webcal://", "https://", 1);
            let client = Client::new(None);
            let ics = client.send("GET", &url, "0", "")?;
            Ok(Synced {
                events: vec![Fetched {
                    calendar: url,
                    ics: vec![ics],
                }],
                lists: Vec::new(),
            })
        }
        Source::CalDav {
            url,
            user,
            password,
        } => account(&Client::new(Some((user, password))), url, from, to),
        Source::Bearer { url, token } => account(&Client::bearer(token, url), url, from, to),
    }
}

/// Every calendar of the account at `url`, with the events of `from`..`to`;
/// those that hold tasks, with all of them.
fn account(
    client: &Client,
    url: &str,
    from: Timestamp,
    to: Timestamp,
) -> Result<Synced, FetchError> {
    let body = report(from, to);
    let mut out = Synced::default();
    for calendar in calendars(client, url)? {
        if calendar.holds("VEVENT") {
            let answer = multistatus(&client.send("REPORT", &calendar.href, "1", &body)?);
            out.events.push(Fetched {
                calendar: if calendar.name.trim().is_empty() {
                    calendar.href.clone()
                } else {
                    calendar.name.clone()
                },
                ics: answer
                    .into_iter()
                    .map(|r| r.data)
                    .filter(|d| !d.is_empty())
                    .collect(),
            });
        }
        if calendar.holds("VTODO") {
            let answer = multistatus(&client.send("REPORT", &calendar.href, "1", TASKS_REPORT)?);
            let name = match calendar.name.trim() {
                "" => last_part(&calendar.href).to_owned(),
                name => name.to_owned(),
            };
            out.lists.push(TaskList {
                name,
                color: color(&calendar.color),
                items: answer
                    .into_iter()
                    .filter(|r| !r.data.is_empty() && !r.href.is_empty())
                    .map(|r| Resource {
                        href: join(url, &r.href),
                        etag: Some(r.etag).filter(|e| !e.is_empty()),
                        ics: r.data,
                    })
                    .collect(),
                url: calendar.href,
            });
        }
    }
    Ok(out)
}

/// A calendar's colour, as servers keep it: `#RRGGBB` or `#RRGGBBAA`.
fn color(text: &str) -> Option<String> {
    let hex = text.trim().strip_prefix('#')?;
    (matches!(hex.len(), 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{hex}"))
}

/// The last part of an address's path: a list's name when it has none.
fn last_part(url: &str) -> &str {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|p| !p.is_empty())
        .unwrap_or(url)
}

/// The client for writing to `source`'s account: a subscription is read
/// only.
fn writer(source: &Source, href: &str) -> Result<Client, WriteError> {
    match source {
        Source::CalDav { user, password, .. } => Ok(Client::new(Some((user, password)))),
        Source::Bearer { url, token } => Ok(Client::bearer(token, url)),
        Source::Subscription { .. } => Err(WriteError::Failed(format!(
            "{href}: a subscription cannot be written to"
        ))),
    }
}

/// Writes a task's text at `href`, as `expect` says it should be there;
/// its new ETag, when the server says it.
pub fn put(
    source: &Source,
    href: &str,
    ics: &str,
    expect: &Expect,
) -> Result<Option<String>, WriteError> {
    let client = writer(source, href)?;
    let mut headers = vec![("Content-Type", "text/calendar; charset=utf-8")];
    match expect {
        Expect::New => headers.push(("If-None-Match", "*")),
        Expect::Etag(etag) => headers.push(("If-Match", etag)),
        Expect::Any => {}
    }
    let answer = client.request("PUT", href, &headers, ics)?;
    match answer.status {
        200..=299 => Ok(answer.etag),
        412 => Err(WriteError::Conflict),
        s => Err(WriteError::Failed(format!(
            "{href}: the server answered {s}"
        ))),
    }
}

/// Removes the task at `href`, as `expect` says it should be there; one
/// already gone is gone.
pub fn delete(source: &Source, href: &str, expect: &Expect) -> Result<(), WriteError> {
    let client = writer(source, href)?;
    let mut headers = Vec::new();
    if let Expect::Etag(etag) = expect {
        headers.push(("If-Match", etag.as_str()));
    }
    let answer = client.request("DELETE", href, &headers, "")?;
    match answer.status {
        200..=299 | 404 | 410 => Ok(()),
        412 => Err(WriteError::Conflict),
        s => Err(WriteError::Failed(format!(
            "{href}: the server answered {s}"
        ))),
    }
}

/// The task at `href` as the server has it now; `None` when it is gone.
pub fn get(source: &Source, href: &str) -> Result<Option<Resource>, WriteError> {
    let client = writer(source, href)?;
    let answer = client.request("GET", href, &[], "")?;
    match answer.status {
        200..=299 => Ok(Some(Resource {
            href: href.to_owned(),
            etag: answer.etag,
            ics: answer.text,
        })),
        404 | 410 => Ok(None),
        s => Err(WriteError::Failed(format!(
            "{href}: the server answered {s}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multistatus_reads_whatever_its_prefixes() {
        let xml = r#"<?xml version="1.0"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response><D:href>/dav/alex/</D:href><D:propstat><D:prop>
    <D:resourcetype><D:collection/></D:resourcetype>
    <C:calendar-home-set><D:href>/dav/calendars/alex/</D:href></C:calendar-home-set>
  </D:prop></D:propstat></D:response>
  <D:response><D:href>/dav/calendars/alex/work/</D:href><D:propstat><D:prop>
    <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>
    <D:displayname>Work</D:displayname>
    <C:calendar-data><![CDATA[BEGIN:VCALENDAR
END:VCALENDAR]]></C:calendar-data>
  </D:prop></D:propstat></D:response>
</D:multistatus>"#;
        let r = multistatus(xml);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].home.as_deref(), Some("/dav/calendars/alex/"));
        assert!(r[1].calendar && r[1].name == "Work");
        assert!(r[1].holds("VEVENT") && !r[1].holds("VTODO"));
        assert!(r[1].data.starts_with("BEGIN:VCALENDAR"));
        assert_eq!(
            join(
                "https://dav.example.org/dav/alex/",
                "/dav/calendars/alex/work/"
            ),
            "https://dav.example.org/dav/calendars/alex/work/"
        );
    }

    #[test]
    fn a_calendar_that_says_vtodo_is_a_task_list_with_its_colour() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:x1="http://apple.com/ns/ical/">
  <d:response><d:href>/remote.php/dav/calendars/alex/chores/</d:href>
    <d:propstat><d:prop>
      <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
      <d:displayname>Chores</d:displayname>
      <cal:supported-calendar-component-set><cal:comp name="VTODO"/></cal:supported-calendar-component-set>
      <x1:calendar-color>#2E7D4FFF</x1:calendar-color>
    </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:response><d:href>/remote.php/dav/calendars/alex/work/</d:href>
    <d:propstat><d:prop>
      <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
      <cal:supported-calendar-component-set><cal:comp name="VEVENT"></cal:comp><cal:comp name="VTODO"></cal:comp></cal:supported-calendar-component-set>
    </d:prop></d:propstat>
    <d:propstat><d:prop><x1:calendar-color/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
  </d:response>
  <d:response><d:href>/remote.php/dav/calendars/alex/chores/milk.ics</d:href>
    <d:propstat><d:prop><d:getetag>"3f2a"</d:getetag></d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
        let r = multistatus(xml);
        assert!(r[0].holds("VTODO") && !r[0].holds("VEVENT"));
        assert_eq!(color(&r[0].color).as_deref(), Some("#2E7D4FFF"));
        assert!(r[1].holds("VTODO") && r[1].holds("VEVENT"));
        assert_eq!(color(&r[1].color), None);
        assert_eq!(r[2].etag, "\"3f2a\"");
        assert_eq!(
            last_part("https://dav.example.com/cal/alex/chores/"),
            "chores"
        );
        assert_eq!(color("red"), None);
    }
}
