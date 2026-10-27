//! An AI provider for tests, on 127.0.0.1: it answers every question with
//! the same pieces, a pause before each, as a slow model streams. It speaks
//! ollama (`/api/tags`, `/api/chat`), OpenAI's chat completions
//! (`/v1/chat/completions`, which llama.cpp's server speaks) and Claude's
//! Messages API (`/v1/messages`, refused without a key), lists its models
//! (`/v1/models`, newest first: `claude-sonnet-9`, `claude-opus-9-1`,
//! `claude-opus-9`, `claude-haiku-9`, then `text-embedding-test`), makes
//! embeddings (`/api/embed` as ollama, which lists `test-embed` after
//! `test-model`, and `/v1/embeddings`), and keeps what it was asked:
//! path, headers, body.
//!
//! Its vectors stand for meaning crudely: each word counts in one of 64
//! dimensions, and a few words of the same meaning, in English and
//! French, share theirs ([`SYNONYMS`]).

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use crate::gate::Gate;

/// A request it got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub path: String,
    /// `name: value`, the name in lower case.
    pub headers: Vec<String>,
    pub body: String,
}

/// How it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answers {
    pub pieces: Vec<String>,
    /// Before each piece.
    pub delay: Duration,
    /// Before the first one too, as a model loading.
    pub first: Duration,
}

/// Where an answer stops until the test lets it go on.
#[derive(Debug, Clone)]
pub struct Hold {
    /// Pieces sent before it.
    pub after: usize,
    pub gate: Gate,
}

#[derive(Debug)]
pub struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    asked: Arc<Mutex<Vec<Asked>>>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(answers: Answers) -> std::io::Result<Self> {
        Self::start_with(answers, None)
    }

    /// As [`Server::start`], each answer stopping at `hold` until its gate
    /// opens.
    pub fn start_held(answers: Answers, hold: Hold) -> std::io::Result<Self> {
        Self::start_with(answers, Some(hold))
    }

    fn start_with(answers: Answers, hold: Option<Hold>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (stopped, kept) = (stop.clone(), asked.clone());
        let thread = std::thread::Builder::new()
            .name("ai provider".into())
            .spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            // Each answer streams on its own: a question
                            // asked again does not wait for the first.
                            let (answers, hold, kept, stopped) =
                                (answers.clone(), hold.clone(), kept.clone(), stopped.clone());
                            let _ = std::thread::Builder::new().name("ai answer".into()).spawn(
                                move || {
                                    let served =
                                        serve(stream, &answers, hold.as_ref(), &kept, &stopped);
                                    if let Err(err) = served {
                                        tracing::debug!(%err, "an AI request failed");
                                    }
                                },
                            );
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
            })?;
        Ok(Self {
            url,
            stop,
            asked,
            thread: Some(thread),
        })
    }

    /// Its address, without a path.
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn asked(&self) -> Vec<Asked> {
        self.asked.lock().map(|a| a.clone()).unwrap_or_default()
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

/// Words of one meaning, the first standing for all.
pub const SYNONYMS: &[&[&str]] = &[
    &["invoice", "facture", "bill", "receipt", "recu"],
    &["holiday", "vacances", "vacation", "conges"],
    &["car", "voiture", "automobile"],
];

/// The stand-in's vector of `text`.
pub fn vector(text: &str) -> Vec<f32> {
    let mut v = vec![0.0f32; 64];
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
    {
        let word: String = word
            .to_lowercase()
            .chars()
            .map(|c| match c {
                'à' | 'â' => 'a',
                'é' | 'è' | 'ê' => 'e',
                'ç' => 'c',
                'ô' => 'o',
                c => c,
            })
            .collect();
        // A word with a meaning of its own weighs more, as a model would.
        let (word, weight) = SYNONYMS
            .iter()
            .find(|group| group.contains(&word.as_str()))
            .map_or((word.clone(), 1.0), |group| (group[0].to_owned(), 3.0));
        // FNV-1a: the same dimension for the same word, every time.
        let hash = word.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        });
        v[(hash % 64) as usize] += weight;
    }
    v
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

fn serve(
    stream: TcpStream,
    answers: &Answers,
    hold: Option<&Hold>,
    asked: &Mutex<Vec<Asked>>,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
    let mut length = 0;
    let mut headers = Vec::new();
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
        let name = name.to_ascii_lowercase();
        if name == "content-length" {
            length = value.trim().parse().unwrap_or(0);
        }
        headers.push(format!("{name}: {}", value.trim()));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let keyed = headers.iter().any(|h| h.starts_with("x-api-key: "));
    if let Ok(mut asked) = asked.lock() {
        asked.push(Asked {
            path: path.clone(),
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }
    let mut stream = stream;
    let whole = |stream: &mut TcpStream, status: &str, text: &str| {
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
            text.len()
        )?;
        stream.flush()
    };
    // Each piece in its own event, then the end.
    let (content_type, piece, end): (&str, fn(&str) -> String, &str) = match path.as_str() {
        "/api/tags" => {
            return whole(
                &mut stream,
                "200 OK",
                r#"{"models":[{"name":"test-model"},{"name":"test-embed"}]}"#,
            );
        }
        "/v1/models" => {
            return whole(
                &mut stream,
                "200 OK",
                r#"{"data":[{"id":"claude-sonnet-9","type":"model"},{"id":"claude-opus-9-1","type":"model"},{"id":"claude-opus-9","type":"model"},{"id":"claude-haiku-9","type":"model"},{"id":"text-embedding-test","type":"model"}],"has_more":false}"#,
            );
        }
        "/api/embed" | "/v1/embeddings" => {
            let asked: serde_json::Value =
                serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
            let texts: Vec<String> = match &asked["input"] {
                serde_json::Value::String(s) => vec![s.clone()],
                serde_json::Value::Array(all) => all
                    .iter()
                    .filter_map(|t| t.as_str().map(str::to_owned))
                    .collect(),
                _ => Vec::new(),
            };
            let vectors: Vec<Vec<f32>> = texts.iter().map(|t| vector(t)).collect();
            let answer = if path == "/api/embed" {
                serde_json::json!({ "model": asked["model"], "embeddings": vectors })
            } else {
                let data: Vec<serde_json::Value> = vectors
                    .iter()
                    .enumerate()
                    .map(|(i, v)| serde_json::json!({ "index": i, "embedding": v }))
                    .collect();
                serde_json::json!({ "data": data })
            };
            return whole(&mut stream, "200 OK", &answer.to_string());
        }
        "/api/chat" => (
            "application/x-ndjson",
            |p| {
                format!(
                    "{{\"message\":{{\"role\":\"assistant\",\"content\":{}}},\"done\":false}}\n",
                    json_string(p)
                )
            },
            "{\"done\":true}\n",
        ),
        "/v1/chat/completions" => (
            "text/event-stream",
            |p| {
                format!(
                    "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\n",
                    json_string(p)
                )
            },
            "data: [DONE]\n\n",
        ),
        "/v1/messages" if !keyed => {
            return whole(
                &mut stream,
                "401 Unauthorized",
                r#"{"type":"error","error":{"type":"authentication_error","message":"x-api-key header is required"}}"#,
            );
        }
        "/v1/messages" => (
            "text/event-stream",
            |p| {
                format!(
                    "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":{}}}}}\n\n",
                    json_string(p)
                )
            },
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ),
        _ => return whole(&mut stream, "404 Not Found", r#"{"error":"not found"}"#),
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;
    let chunk = |stream: &mut TcpStream, data: &str| {
        write!(stream, "{:x}\r\n{data}\r\n", data.len())?;
        stream.flush()
    };
    std::thread::sleep(answers.first);
    for (i, p) in answers.pieces.iter().enumerate() {
        if let Some(hold) = hold.filter(|h| h.after == i) {
            hold.gate.wait();
        }
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        std::thread::sleep(answers.delay);
        // A client that went away ends it.
        chunk(&mut stream, &piece(p))?;
    }
    chunk(&mut stream, end)?;
    write!(stream, "0\r\n\r\n")?;
    stream.flush()
}
