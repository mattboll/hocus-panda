//! A CalDAV server for tests, on 127.0.0.1: one calendar, the `.ics` files
//! of a directory, behind a name and password. Enough of RFC 4791 for the
//! session's client: PROPFIND says it is a calendar, REPORT gives every
//! event, GET gives them as one subscription.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
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
}

impl Server {
    /// Serves the `.ics` files of `dir` to `user` with `password`.
    pub fn start(dir: PathBuf, user: &str, password: &str) -> std::io::Result<Self> {
        let auth = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        );
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

    /// Answers each request on 127.0.0.1 with `answer`, on a thread of its
    /// own; the server's address is `path` there.
    pub(crate) fn serve(
        name: &str,
        path: &str,
        answer: impl Fn(&HttpRequest) -> (&'static str, String) + Send + 'static,
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
}

fn serve(
    stream: TcpStream,
    answer: &impl Fn(&HttpRequest) -> (&'static str, String),
) -> std::io::Result<()> {
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
            _ => {}
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let (status, text) = answer(&request);
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/xml; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
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
