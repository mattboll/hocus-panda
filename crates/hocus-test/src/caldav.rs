//! A CalDAV server for tests, on 127.0.0.1: one calendar, the `.ics` files
//! of a directory, behind a name and password. Enough of RFC 4791 for the
//! session's client: PROPFIND says it is a calendar, REPORT gives every
//! event, GET gives them as one subscription.
//!
//! A directory of folders is an account's home instead (#256): each folder
//! a calendar, named by its `displayname` file, coloured by its `color`
//! file, holding what its `components` file says (`VEVENT` without one).
//! Its files are served from a copy, which PUT and DELETE change on their
//! ETags (a hash of the text) as a server would, 412 when they changed.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use base64::Engine;

#[derive(Debug)]
pub struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// A home's copy, which its clients change.
    copy: Option<tempfile::TempDir>,
}

/// What the server answers: a status, a text, and its ETag.
pub(crate) type Reply = (&'static str, String, Option<String>);

/// The account's home, in home mode.
const HOME: &str = "/dav/home/";

impl Server {
    /// Serves the `.ics` files of `dir` to `user` with `password`; a
    /// directory of folders, as a home of calendars.
    pub fn start(dir: PathBuf, user: &str, password: &str) -> std::io::Result<Self> {
        let auth = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        );
        let folders = std::fs::read_dir(&dir)?
            .flatten()
            .any(|e| e.path().is_dir());
        if folders {
            return Self::home(&dir, auth);
        }
        Self::serve("caldav", "/dav/work/", move |request| {
            if request.authorization.as_deref() != Some(auth.as_str()) && request.method != "GET" {
                return ("401 Unauthorized", String::new());
            }
            match request.method.as_str() {
                "PROPFIND" => ("207 Multi-Status", propfind()),
                "REPORT" => ("207 Multi-Status", report("/dav/work/", &events(&dir))),
                "GET" => ("200 OK", events(&dir).concat()),
                _ => ("405 Method Not Allowed", String::new()),
            }
        })
    }

    /// A home of calendars, the folders of a copy of `dir`.
    fn home(dir: &Path, auth: String) -> std::io::Result<Self> {
        let copy = tempfile::tempdir()?;
        copy_dir(dir, copy.path())?;
        let root = copy.path().to_path_buf();
        let mut server = Self::serve_with("caldav home", HOME, move |request| {
            if request.authorization.as_deref() != Some(auth.as_str()) {
                return ("401 Unauthorized", String::new(), None);
            }
            home_answer(&root, request)
        })?;
        server.copy = Some(copy);
        Ok(server)
    }

    /// Where a home's files are served from, which its clients change.
    pub fn files(&self) -> Option<&Path> {
        self.copy.as_ref().map(|c| c.path())
    }

    /// Answers each request on 127.0.0.1 with `answer`, on a thread of its
    /// own; the server's address is `path` there.
    pub(crate) fn serve(
        name: &str,
        path: &str,
        answer: impl Fn(&HttpRequest) -> (&'static str, String) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::serve_with(name, path, move |request| {
            let (status, text) = answer(request);
            (status, text, None)
        })
    }

    /// The same, answers with their ETag.
    fn serve_with(
        name: &str,
        path: &str,
        answer: impl Fn(&HttpRequest) -> Reply + Send + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}{path}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if let Err(err) = serve(stream, &answer) {
                                tracing::debug!(%err, "a CalDAV request failed");
                            }
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
            })?;
        Ok(Self {
            url,
            stop,
            thread: Some(thread),
            copy: None,
        })
    }

    /// The calendar's address.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What a request asked, as far as the stand-ins look.
#[derive(Debug, Default)]
pub(crate) struct HttpRequest {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub depth: String,
    pub if_match: Option<String>,
    pub if_none_match: Option<String>,
    pub body: String,
}

fn serve(stream: TcpStream, answer: &impl Fn(&HttpRequest) -> Reply) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut words = line.split_whitespace();
    let mut request = HttpRequest {
        method: words.next().unwrap_or("").to_owned(),
        path: words.next().unwrap_or("").to_owned(),
        ..HttpRequest::default()
    };
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        match name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.trim().parse().unwrap_or(0),
            "authorization" => request.authorization = Some(value.trim().to_owned()),
            "depth" => request.depth = value.trim().to_owned(),
            "if-match" => request.if_match = Some(value.trim().to_owned()),
            "if-none-match" => request.if_none_match = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    request.body = String::from_utf8_lossy(&body).into_owned();
    let (status, text, etag) = answer(&request);
    let etag = etag.map(|e| format!("ETag: {e}\r\n")).unwrap_or_default();
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/xml; charset=utf-8\r\n{etag}Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// A text's ETag: a hash of it, quoted.
fn etag(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("\"{hash:016x}\"")
}

/// A home's answer: its calendars, their objects, written as asked.
fn home_answer(root: &Path, request: &HttpRequest) -> Reply {
    let path = request.path.as_str();
    let Some(rest) = path.strip_prefix(HOME) else {
        return ("404 Not Found", String::new(), None);
    };
    let (folder, file) = match rest.split_once('/') {
        Some((folder, file)) => (folder, file),
        None => (rest, ""),
    };
    let found = |body: String| ("207 Multi-Status", body, None);
    match (request.method.as_str(), folder, file) {
        ("PROPFIND", "", _) => {
            let mut body = multistatus_start();
            body.push_str(&format!(
                "<d:response><d:href>{HOME}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype><cal:calendar-home-set><d:href>{HOME}</d:href></cal:calendar-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            ));
            if request.depth == "1" {
                for name in folders(root) {
                    body.push_str(&calendar_response(root, &name));
                }
            }
            body.push_str("</d:multistatus>");
            found(body)
        }
        ("PROPFIND", folder, "") if root.join(folder).is_dir() => {
            let mut body = multistatus_start();
            body.push_str(&calendar_response(root, folder));
            body.push_str("</d:multistatus>");
            found(body)
        }
        ("REPORT", folder, "") if root.join(folder).is_dir() => {
            let wanted = if request.body.contains("\"VTODO\"") {
                "BEGIN:VTODO"
            } else {
                "BEGIN:VEVENT"
            };
            let mut body = multistatus_start();
            for (name, text) in objects(&root.join(folder)) {
                if !text.contains(wanted) {
                    continue;
                }
                body.push_str(&format!(
                    "<d:response><d:href>{HOME}{folder}/{name}</d:href><d:propstat><d:prop><d:getetag>{}</d:getetag><cal:calendar-data>{}</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                    xml_escape(&etag(&text)),
                    xml_escape(&text)
                ));
            }
            body.push_str("</d:multistatus>");
            found(body)
        }
        (method, folder, file)
            if !file.is_empty() && !file.contains('/') && !file.starts_with('.') =>
        {
            let at = root.join(folder).join(file);
            let now = std::fs::read_to_string(&at).ok();
            let current = now.as_deref().map(etag);
            let refused = ("412 Precondition Failed", String::new(), None);
            match method {
                "GET" => match now {
                    Some(text) => ("200 OK", text, current),
                    None => ("404 Not Found", String::new(), None),
                },
                "PUT" => {
                    if request.if_none_match.as_deref() == Some("*") && now.is_some() {
                        return refused;
                    }
                    if request.if_match.is_some() && request.if_match != current {
                        return refused;
                    }
                    if !root.join(folder).is_dir() || std::fs::write(&at, &request.body).is_err() {
                        return ("409 Conflict", String::new(), None);
                    }
                    let status = if now.is_some() {
                        "204 No Content"
                    } else {
                        "201 Created"
                    };
                    (status, String::new(), Some(etag(&request.body)))
                }
                "DELETE" => {
                    if now.is_none() {
                        return ("404 Not Found", String::new(), None);
                    }
                    if request.if_match.is_some() && request.if_match != current {
                        return refused;
                    }
                    match std::fs::remove_file(&at) {
                        Ok(()) => ("204 No Content", String::new(), None),
                        Err(_) => ("500 Internal Server Error", String::new(), None),
                    }
                }
                _ => ("405 Method Not Allowed", String::new(), None),
            }
        }
        _ => ("404 Not Found", String::new(), None),
    }
}

fn multistatus_start() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:ical="http://apple.com/ns/ical/">"#
        .to_owned()
}

/// The calendars of a home, by folder.
fn folders(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A calendar's objects: its `.ics` files, by name.
fn objects(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "ics"))
                .filter_map(|p| {
                    let name = p.file_name()?.to_string_lossy().into_owned();
                    Some((name, std::fs::read_to_string(&p).ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// A calendar of a home, as PROPFIND says it.
fn calendar_response(root: &Path, folder: &str) -> String {
    let read = |name: &str| {
        std::fs::read_to_string(root.join(folder).join(name))
            .ok()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
    };
    let name = read("displayname").unwrap_or_else(|| folder.to_owned());
    let components: String = read("components")
        .unwrap_or_else(|| "VEVENT".into())
        .split(',')
        .map(|c| format!("<cal:comp name=\"{}\"/>", c.trim()))
        .collect();
    let color = read("color")
        .map(|c| format!("<ical:calendar-color>{c}</ical:calendar-color>"))
        .unwrap_or_default();
    format!(
        "<d:response><d:href>{HOME}{folder}/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><d:displayname>{}</d:displayname><cal:supported-calendar-component-set>{components}</cal:supported-calendar-component-set>{color}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
        xml_escape(&name)
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn events(dir: &PathBuf) -> Vec<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "ics"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

fn propfind() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
<d:response><d:href>/dav/work/</d:href><d:propstat><d:prop>
<d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
<d:displayname>Work</d:displayname>
</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#
        .to_owned()
}

/// The events as a REPORT's answer, each a resource below `at`.
pub(crate) fn report(at: &str, events: &[String]) -> String {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">"#,
    );
    for (i, e) in events.iter().enumerate() {
        out.push_str(&format!(
            "<d:response><d:href>{at}{i}.ics</d:href><d:propstat><d:prop><cal:calendar-data>{}</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
            escape(e)
        ));
    }
    out.push_str("</d:multistatus>");
    out
}
