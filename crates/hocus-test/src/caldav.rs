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
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}/dav/work/", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let auth = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        );
        let stopped = stop.clone();
        let thread = std::thread::Builder::new()
            .name("caldav".into())
            .spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if let Err(err) = serve(stream, &dir, &auth) {
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

fn serve(stream: TcpStream, dir: &PathBuf, auth: &str) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let method = line.split_whitespace().next().unwrap_or("").to_owned();
    let mut length = 0;
    let mut authorized = false;
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
            "authorization" => authorized = value.trim() == auth,
            _ => {}
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let (status, text) = if !authorized && method != "GET" {
        ("401 Unauthorized", String::new())
    } else {
        match method.as_str() {
            "PROPFIND" => ("207 Multi-Status", propfind()),
            "REPORT" => ("207 Multi-Status", report(&events(dir))),
            "GET" => ("200 OK", events(dir).concat()),
            _ => ("405 Method Not Allowed", String::new()),
        }
    };
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/xml; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

fn events(dir: &PathBuf) -> Vec<String> {
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

fn report(events: &[String]) -> String {
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
            "<d:response><d:href>/dav/work/{i}.ics</d:href><d:propstat><d:prop><cal:calendar-data>{}</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
            escape(e)
        ));
    }
    out.push_str("</d:multistatus>");
    out
}
