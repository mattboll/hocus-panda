//! Questions to a language model (#44), answered as a stream of text:
//! ollama's API, the chat completions OpenAI defined (which llama.cpp's
//! server and most local servers speak too), and Claude's Messages API.
//! Blocking: run on a worker thread, which hears each piece of the answer
//! as it comes and can be told to stop between pieces.

use std::{
    fmt,
    io::{BufRead, BufReader, Read},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use serde_json::{Value, json};

/// Claude's API version, sent with every request.
const ANTHROPIC_VERSION: &str = "2026-10-21";
/// Claude's model when the configuration names none.
pub const CLAUDE_MODEL: &str = "claude-opus-5-5";
/// What the answer may take, at most; the field shows the start of a long
/// one and Copy takes it all.
const MAX_TOKENS: u32 = 16_000;
/// A line of a stream longer than this is not a provider's.
const MAX_LINE: usize = 1 << 20;

/// Who answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Ollama,
    /// llama.cpp's server, through its OpenAI-like API.
    LlamaCpp,
    /// OpenAI, or any server speaking its chat completions.
    OpenAi,
    Claude,
}

impl Provider {
    /// From the configuration's `ai.provider`.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "ollama" => Self::Ollama,
            "llamacpp" => Self::LlamaCpp,
            "openai" => Self::OpenAi,
            "claude" => Self::Claude,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LlamaCpp => "llamacpp",
            Self::OpenAi => "openai",
            Self::Claude => "claude",
        }
    }

    /// Where it usually listens.
    pub fn default_url(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434",
            Self::LlamaCpp => "http://127.0.0.1:8080",
            Self::OpenAi => "https://api.openai.com",
            Self::Claude => "https://api.anthropic.com",
        }
    }

    /// It refuses a question without a key.
    pub fn needs_key(self) -> bool {
        matches!(self, Self::OpenAi | Self::Claude)
    }
}

/// A question, and where it goes.
#[derive(Clone)]
pub struct Question {
    pub provider: Provider,
    /// The provider's address, without a path.
    pub url: String,
    /// Empty: the provider's own choice.
    pub model: String,
    pub key: Option<String>,
    /// How to answer: briefly, in the user's language.
    pub system: String,
    pub text: String,
}

impl fmt::Debug for Question {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the key, nor the question, in a log.
        f.debug_struct("Question")
            .field("provider", &self.provider)
            .field("url", &self.url)
            .field("model", &self.model)
            .field("key", &self.key.as_ref().map(|_| "…"))
            .finish_non_exhaustive()
    }
}

/// Why no answer came, or not all of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskError {
    /// Nothing listens there, or the network is down.
    Unreachable(String),
    /// The key is missing or wrong.
    Refused(String),
    /// The provider has no model to answer with, or needs one named.
    NoModel,
    /// The model declined to answer.
    Declined,
    Failed(String),
}

impl fmt::Display for AskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "no answer: {why}"),
            Self::Refused(why) => write!(f, "refused: {why}"),
            Self::NoModel => f.write_str("no model to answer with"),
            Self::Declined => f.write_str("the model declined to answer"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for AskError {}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        // A local model may take a while to load before its first word.
        .timeout_recv_response(Some(Duration::from_secs(120)))
        .timeout_recv_body(Some(Duration::from_secs(300)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Asks `question`, giving each piece of the answer to `piece` as it
/// comes; returns once it is all there, or early once `stop` is set.
pub fn ask(
    question: &Question,
    stop: &AtomicBool,
    mut piece: impl FnMut(&str),
) -> Result<(), AskError> {
    let agent = agent();
    let base = question.url.trim_end_matches('/');
    match question.provider {
        Provider::Ollama => ollama(&agent, base, question, stop, &mut piece),
        Provider::LlamaCpp | Provider::OpenAi => {
            chat_completions(&agent, base, question, stop, &mut piece)
        }
        Provider::Claude => claude(&agent, base, question, stop, &mut piece),
    }
}

/// Sends `body` (JSON) or nothing, with `headers`; a refusal or an error
/// status becomes an error, with what the provider said.
fn send(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> Result<ureq::http::Response<ureq::Body>, AskError> {
    let mut request = ureq::http::Request::builder().method(method).uri(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = match body {
        Some(body) => {
            let request = request
                .header("Content-Type", "application/json")
                .body(body.to_string())
                .map_err(|e| AskError::Failed(e.to_string()))?;
            agent.run(request)
        }
        None => {
            let request = request
                .body(())
                .map_err(|e| AskError::Failed(e.to_string()))?;
            agent.run(request)
        }
    }
    .map_err(|e| AskError::Unreachable(format!("{url}: {e}")))?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(response);
    }
    let mut text = String::new();
    let _ = response
        .into_body()
        .into_with_config()
        .limit(64 * 1024)
        .reader()
        .read_to_string(&mut text);
    let said = error_message(&text).unwrap_or_else(|| format!("{url} answered {status}"));
    Err(match status {
        401 | 403 => AskError::Refused(said),
        404 if said.contains("model") => AskError::NoModel,
        _ => AskError::Failed(said),
    })
}

/// What a provider says went wrong: `{"error": "…"}` or `{"error":
/// {"message": "…"}}`.
fn error_message(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    let error = value.get("error")?;
    error
        .as_str()
        .or_else(|| error.get("message")?.as_str())
        .map(str::to_owned)
}

/// The response's lines as they come; `false` from `line` ends it.
fn lines(
    response: ureq::http::Response<ureq::Body>,
    stop: &AtomicBool,
    mut line: impl FnMut(&str) -> Result<bool, AskError>,
) -> Result<(), AskError> {
    let mut reader = BufReader::new(response.into_body().into_reader());
    let mut buf = String::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        buf.clear();
        let read = (&mut reader)
            .take(MAX_LINE as u64)
            .read_line(&mut buf)
            .map_err(|e| AskError::Failed(format!("the answer broke off: {e}")))?;
        if read == 0 {
            return Ok(());
        }
        let text = buf.trim_end_matches(['\r', '\n']);
        if !line(text)? {
            return Ok(());
        }
    }
}

fn messages(question: &Question) -> Value {
    json!([
        { "role": "system", "content": question.system },
        { "role": "user", "content": question.text },
    ])
}

/// ollama: `/api/chat`, a JSON object per line. Without a model named,
/// the first it has.
fn ollama(
    agent: &ureq::Agent,
    base: &str,
    question: &Question,
    stop: &AtomicBool,
    piece: &mut dyn FnMut(&str),
) -> Result<(), AskError> {
    let model = if question.model.is_empty() {
        let mut tags = send(agent, "GET", &format!("{base}/api/tags"), &[], None)?;
        let text = tags
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map_err(|e| AskError::Failed(e.to_string()))?;
        let value: Value =
            serde_json::from_str(&text).map_err(|e| AskError::Failed(e.to_string()))?;
        value["models"][0]["name"]
            .as_str()
            .ok_or(AskError::NoModel)?
            .to_owned()
    } else {
        question.model.clone()
    };
    let body = json!({ "model": model, "messages": messages(question), "stream": true });
    let response = send(agent, "POST", &format!("{base}/api/chat"), &[], Some(&body))?;
    lines(response, stop, |line| {
        if line.trim().is_empty() {
            return Ok(true);
        }
        let value: Value =
            serde_json::from_str(line).map_err(|e| AskError::Failed(e.to_string()))?;
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(AskError::Failed(error.to_owned()));
        }
        if let Some(text) = value["message"]["content"].as_str()
            && !text.is_empty()
        {
            piece(text);
        }
        Ok(!value["done"].as_bool().unwrap_or(false))
    })
}

/// The `data:` of a server-sent event's line, if it is one.
fn sse_data(line: &str) -> Option<&str> {
    line.strip_prefix("data:").map(str::trim_start)
}

/// OpenAI's chat completions, which llama.cpp's server speaks: server-sent
/// events, `[DONE]` at the end.
fn chat_completions(
    agent: &ureq::Agent,
    base: &str,
    question: &Question,
    stop: &AtomicBool,
    piece: &mut dyn FnMut(&str),
) -> Result<(), AskError> {
    let mut body = json!({ "messages": messages(question), "stream": true });
    if !question.model.is_empty() {
        body["model"] = json!(question.model);
    } else if question.provider == Provider::OpenAi {
        return Err(AskError::NoModel);
    }
    let auth = question.key.as_ref().map(|k| format!("Bearer {k}"));
    let mut headers = Vec::new();
    if let Some(auth) = &auth {
        headers.push(("Authorization", auth.as_str()));
    } else if question.provider.needs_key() {
        return Err(AskError::Refused("no key in the keyring".into()));
    }
    // An address given with its version keeps it.
    let url = if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    };
    let response = send(agent, "POST", &url, &headers, Some(&body))?;
    lines(response, stop, |line| {
        let Some(data) = sse_data(line) else {
            return Ok(true);
        };
        if data == "[DONE]" {
            return Ok(false);
        }
        let value: Value =
            serde_json::from_str(data).map_err(|e| AskError::Failed(e.to_string()))?;
        if value.get("error").is_some() {
            return Err(AskError::Failed(
                error_message(data).unwrap_or_else(|| "the provider failed".into()),
            ));
        }
        if let Some(text) = value["choices"][0]["delta"]["content"].as_str()
            && !text.is_empty()
        {
            piece(text);
        }
        Ok(true)
    })
}

/// Claude's Messages API, streamed: text comes in `content_block_delta`
/// events. A decline is retried on another model by the server
/// (`fallbacks`), else said.
fn claude(
    agent: &ureq::Agent,
    base: &str,
    question: &Question,
    stop: &AtomicBool,
    piece: &mut dyn FnMut(&str),
) -> Result<(), AskError> {
    let Some(key) = &question.key else {
        return Err(AskError::Refused("no key in the keyring".into()));
    };
    let model = if question.model.is_empty() {
        CLAUDE_MODEL
    } else {
        &question.model
    };
    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": question.system,
        "messages": [{ "role": "user", "content": question.text }],
        // A field's answer: quick rather than deep.
        "output_config": { "effort": "low" },
        "fallbacks": "default",
    });
    let headers = [
        ("x-api-key", key.as_str()),
        ("anthropic-version", ANTHROPIC_VERSION),
        ("anthropic-beta", "server-side-fallback-2026-07-01"),
    ];
    let response = send(
        agent,
        "POST",
        &format!("{base}/v1/messages"),
        &headers,
        Some(&body),
    )?;
    lines(response, stop, |line| {
        let Some(data) = sse_data(line) else {
            return Ok(true);
        };
        let value: Value =
            serde_json::from_str(data).map_err(|e| AskError::Failed(e.to_string()))?;
        match value["type"].as_str() {
            Some("content_block_delta") => {
                if value["delta"]["type"] == "text_delta"
                    && let Some(text) = value["delta"]["text"].as_str()
                {
                    piece(text);
                }
                Ok(true)
            }
            Some("message_delta") if value["delta"]["stop_reason"] == "refusal" => {
                Err(AskError::Declined)
            }
            Some("message_stop") => Ok(false),
            Some("error") => Err(AskError::Failed(
                error_message(data).unwrap_or_else(|| "Claude failed".into()),
            )),
            _ => Ok(true),
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        net::TcpListener,
        sync::{Arc, Mutex},
    };

    use super::*;

    /// Serves one request with `reply` (status line, headers and body),
    /// keeping what was asked.
    fn serve(replies: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}", listener.local_addr().expect("an address"));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let kept = asked.clone();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream.try_clone().expect("a stream"));
                let mut head = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap_or(0);
                    }
                    head.push_str(&line);
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                head.push_str(&String::from_utf8_lossy(&body));
                kept.lock().expect("a lock").push(head);
                let mut stream = stream;
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (url, asked)
    }

    fn reply(content_type: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn question(provider: Provider, url: String, key: Option<&str>) -> Question {
        Question {
            provider,
            url,
            model: String::new(),
            key: key.map(str::to_owned),
            system: "Be brief.".into(),
            text: "Why?".into(),
        }
    }

    fn answer(q: &Question) -> Result<String, AskError> {
        let mut text = String::new();
        ask(q, &AtomicBool::new(false), |p| text.push_str(p))?;
        Ok(text)
    }

    #[test]
    fn ollama_answers_with_its_first_model() {
        let (url, asked) = serve(vec![
            reply(
                "application/json",
                r#"{"models":[{"name":"llama3.2:latest"}]}"#,
            ),
            reply(
                "application/x-ndjson",
                "{\"message\":{\"content\":\"Because \"},\"done\":false}\n\
                 {\"message\":{\"content\":\"of the cache.\"},\"done\":false}\n\
                 {\"done\":true}\n",
            ),
        ]);
        let q = question(Provider::Ollama, url, None);
        assert_eq!(answer(&q).as_deref(), Ok("Because of the cache."));
        let asked = asked.lock().expect("a lock");
        assert!(
            asked[1].contains("\"model\":\"llama3.2:latest\""),
            "{asked:?}"
        );
        assert!(asked[1].starts_with("POST /api/chat"), "{asked:?}");
    }

    #[test]
    fn chat_completions_stream_events_until_done() {
        let (url, asked) = serve(vec![reply(
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"Forty\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"-two.\"}}]}\n\n\
             data: [DONE]\n\n",
        )]);
        let q = question(Provider::LlamaCpp, format!("{url}/v1"), None);
        assert_eq!(answer(&q).as_deref(), Ok("Forty-two."));
        assert!(asked.lock().expect("a lock")[0].starts_with("POST /v1/chat/completions"));
        let mut openai = question(Provider::OpenAi, "http://127.0.0.1:9".into(), Some("k"));
        assert_eq!(answer(&openai), Err(AskError::NoModel), "OpenAI needs one");
        openai.model = "gpt".into();
        openai.key = None;
        assert!(matches!(answer(&openai), Err(AskError::Refused(_))));
    }

    #[test]
    fn claude_sends_its_key_and_streams_text_deltas() {
        let (url, asked) = serve(vec![reply(
            "text/event-stream",
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n\
             event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
             event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" there\"}}\n\n\
             event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        )]);
        let q = question(Provider::Claude, url, Some("sk-test"));
        assert_eq!(answer(&q).as_deref(), Ok("Hello there"));
        let asked = asked.lock().expect("a lock");
        let head = asked[0].to_lowercase();
        assert!(head.contains("x-api-key: sk-test"), "{head}");
        assert!(head.contains("anthropic-version: 2026-10-21"), "{head}");
        assert!(asked[0].contains(CLAUDE_MODEL), "{asked:?}");
        assert!(format!("{q:?}").find("sk-test").is_none(), "never logged");
    }

    #[test]
    fn errors_say_what_the_provider_said() {
        let (url, _) = serve(vec![
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 47\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"invalid x-api-key\",\"t\":1}}"
                .into(),
        ]);
        let q = question(Provider::Claude, url, Some("wrong"));
        assert_eq!(
            answer(&q),
            Err(AskError::Refused("invalid x-api-key".into()))
        );
        let nobody = question(Provider::Ollama, "http://127.0.0.1:9".into(), None);
        assert!(matches!(answer(&nobody), Err(AskError::Unreachable(_))));
    }
}
