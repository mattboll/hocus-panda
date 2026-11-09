//! A fake HTTP server on this machine, never the network: it records
//! every request whole (method, path, headers, body) and answers each as
//! its test says. It stands for the Anthropic API and for a local
//! OpenAI-compatible model server; no real API is ever called.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

/// A request as it arrived.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_default()
    }
}

/// An answer.
#[derive(Debug, Clone)]
pub struct Answer {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Answer {
    pub fn json(value: &serde_json::Value) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: value.to_string(),
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_owned(),
        }
    }

    /// The Messages API's stream for a text answer.
    pub fn stream(text: &str) -> Self {
        let event =
            |kind: &str, data: serde_json::Value| format!("event: {kind}\ndata: {data}\n\n");
        let mut body = event(
            "message_start",
            serde_json::json!({"type": "message_start", "message": {"id": "msg_1", "usage": {"input_tokens": 1000, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 900}}}),
        );
        body.push_str(&event(
            "content_block_start",
            serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        ));
        for word in text.split_inclusive(' ') {
            body.push_str(&event(
                "content_block_delta",
                serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": word}}),
            ));
        }
        body.push_str(&event(
            "content_block_stop",
            serde_json::json!({"type": "content_block_stop", "index": 0}),
        ));
        body.push_str(&event(
            "message_delta",
            serde_json::json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 12}}),
        ));
        body.push_str(&event(
            "message_stop",
            serde_json::json!({"type": "message_stop"}),
        ));
        Self {
            status: 200,
            content_type: "text/event-stream",
            body,
        }
    }
}

type Script = dyn Fn(&Seen, usize) -> Answer + Send + Sync;

/// The server: answers by `script` (the request, and how many came
/// before it).
pub struct Fake {
    pub url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Fake {
    pub fn start(script: impl Fn(&Seen, usize) -> Answer + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}", listener.local_addr().expect("its address"));
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let script: Arc<Script> = Arc::new(script);
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (kept, script) = (kept.clone(), script.clone());
                std::thread::spawn(move || serve(stream, &kept, script.as_ref()));
            }
        });
        Self { url, seen }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn serve(stream: TcpStream, kept: &Mutex<Vec<Seen>>, script: &Script) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(stream.try_clone().expect("a stream"));
    let mut first = String::new();
    if reader.read_line(&mut first).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let length = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let seen = Seen {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    let before = {
        let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.push(seen.clone());
        kept.len() - 1
    };
    let answer = script(&seen, before);
    let mut out = stream;
    let _ = write!(
        out,
        "HTTP/1.1 {} X\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        answer.status,
        answer.content_type,
        answer.body.len(),
        answer.body
    );
    let _ = out.flush();
}
