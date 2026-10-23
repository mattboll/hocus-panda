//! Fetching calendars: a CalDAV account (RFC 4791), found from its server,
//! its principal or a calendar's own address, or a subscription (an ICS
//! link). Blocking: the compositor calls it on worker threads.

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

const PROPFIND: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><d:resourcetype/><d:displayname/><d:current-user-principal/><c:calendar-home-set/></d:prop>
</d:propfind>"#;

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
                path.push(name);
            }
            Ok(Xml::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if let (Some(r), Some(parent)) = (current.as_mut(), path.last())
                    && parent == "resourcetype"
                {
                    match name.as_str() {
                        "calendar" => r.calendar = true,
                        "collection" => r.collection = true,
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

struct Client {
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

    fn bearer(token: &Token, url: &str) -> Self {
        Self {
            agent: agent(),
            auth: Some(format!("Bearer {}", token.0)),
            only: Some(origin(url).to_owned()),
        }
    }

    fn send(&self, method: &str, url: &str, depth: &str, body: &str) -> Result<String, FetchError> {
        let failed = |e: &dyn std::fmt::Display| FetchError::Failed(format!("{url}: {e}"));
        if self.only.as_deref().is_some_and(|o| o != origin(url)) {
            return Err(failed(&"elsewhere than the account's server"));
        }
        let mut request = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("Depth", depth)
            .header("Content-Type", "application/xml; charset=utf-8");
        if let Some(auth) = &self.auth {
            request = request.header("Authorization", auth);
        }
        let request = request.body(body.to_owned()).map_err(|e| failed(&e))?;
        let mut response = self.agent.run(request).map_err(|e| failed(&e))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| failed(&e))?;
        match status {
            200..=299 => Ok(text),
            401 | 403 => Err(FetchError::Refused(format!(
                "{url}: the server refused the {}",
                if self.only.is_some() {
                    "access token"
                } else {
                    "name or password"
                }
            ))),
            s => Err(failed(&format!("the server answered {s}"))),
        }
    }
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
/// else those of its home, found through the principal if needed.
fn calendars(client: &Client, url: &str) -> Result<Vec<(String, String)>, FetchError> {
    let nothing = |at: &str| FetchError::Failed(format!("{at}: no calendars here"));
    let here = multistatus(&client.send("PROPFIND", url, "0", PROPFIND)?);
    let first = here.first().cloned().unwrap_or_default();
    if first.calendar {
        return Ok(vec![(url.to_owned(), first.name)]);
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
    let found: Vec<(String, String)> = listed
        .into_iter()
        .filter(|r| r.calendar)
        .map(|r| (join(url, &r.href), r.name))
        .collect();
    if found.is_empty() {
        return Err(nothing(url));
    }
    Ok(found)
}

/// Every calendar of `source`, with the events that overlap `from`..`to`
/// (a subscription gives them all).
pub fn fetch(source: &Source, from: Timestamp, to: Timestamp) -> Result<Vec<Fetched>, FetchError> {
    match source {
        Source::Subscription { url } => {
            let url = url.replacen("webcal://", "https://", 1);
            let client = Client::new(None);
            let ics = client.send("GET", &url, "0", "")?;
            Ok(vec![Fetched {
                calendar: url,
                ics: vec![ics],
            }])
        }
        Source::CalDav {
            url,
            user,
            password,
        } => account(&Client::new(Some((user, password))), url, from, to),
        Source::Bearer { url, token } => account(&Client::bearer(token, url), url, from, to),
    }
}

/// Every calendar of the account at `url`, with the events of `from`..`to`.
fn account(
    client: &Client,
    url: &str,
    from: Timestamp,
    to: Timestamp,
) -> Result<Vec<Fetched>, FetchError> {
    let body = report(from, to);
    let mut out = Vec::new();
    for (calendar, name) in calendars(client, url)? {
        let answer = multistatus(&client.send("REPORT", &calendar, "1", &body)?);
        out.push(Fetched {
            calendar: if name.trim().is_empty() {
                calendar
            } else {
                name
            },
            ics: answer
                .into_iter()
                .map(|r| r.data)
                .filter(|d| !d.is_empty())
                .collect(),
        });
    }
    Ok(out)
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
        assert!(r[1].data.starts_with("BEGIN:VCALENDAR"));
        assert_eq!(
            join(
                "https://dav.example.org/dav/alex/",
                "/dav/calendars/alex/work/"
            ),
            "https://dav.example.org/dav/calendars/alex/work/"
        );
    }
}
