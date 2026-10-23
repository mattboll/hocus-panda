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

    /// It makes embeddings (#45): Claude does not.
    pub fn can_embed(self) -> bool {
        self != Self::Claude
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
    /// The provider found the request itself wrong (a 400).
    Invalid(String),
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
            Self::Invalid(why) => write!(f, "invalid request: {why}"),
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

/// The model to ask when the configuration names none, as the provider
/// lists them: ollama's first; the newest Claude Opus (models change
/// often: none is written here), else Claude's newest; an OpenAI-like
/// server's first. `None`: the llama.cpp server answers with its own.
/// Blocking, a request of its own: the caller keeps the result.
pub fn pick_model(question: &Question) -> Result<Option<String>, AskError> {
    let agent = agent();
    let base = question.url.trim_end_matches('/');
    let listed = |url: &str, headers: &[(&str, &str)]| -> Result<Value, AskError> {
        let mut response = send(&agent, "GET", url, headers, None)?;
        let text = response
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map_err(|e| AskError::Failed(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| AskError::Failed(e.to_string()))
    };
    let ids = |value: &Value| -> Vec<String> {
        value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect()
    };
    match question.provider {
        Provider::Ollama => {
            let tags = listed(&format!("{base}/api/tags"), &[])?;
            tags["models"][0]["name"]
                .as_str()
                .map(|name| Some(name.to_owned()))
                .ok_or(AskError::NoModel)
        }
        Provider::LlamaCpp | Provider::OpenAi => {
            let auth = question.key.as_ref().map(|k| format!("Bearer {k}"));
            let headers: Vec<(&str, &str)> = auth
                .as_deref()
                .map(|a| ("Authorization", a))
                .into_iter()
                .collect();
            match listed(&v1(base, "models"), &headers) {
                Ok(value) => match ids(&value).into_iter().next() {
                    Some(id) => Ok(Some(id)),
                    None if question.provider == Provider::LlamaCpp => Ok(None),
                    None => Err(AskError::NoModel),
                },
                // llama.cpp's server needs no model named.
                Err(_) if question.provider == Provider::LlamaCpp => Ok(None),
                Err(err) => Err(err),
            }
        }
        Provider::Claude => {
            let Some(key) = &question.key else {
                return Err(AskError::Refused("no key in the keyring".into()));
            };
            let headers = [
                ("x-api-key", key.as_str()),
                ("anthropic-version", ANTHROPIC_VERSION),
            ];
            // Listed newest first.
            let all = ids(&listed(&format!("{base}/v1/models"), &headers)?);
            all.iter()
                .find(|id| id.starts_with("claude-opus"))
                .or(all.first())
                .map(|id| Some(id.clone()))
                .ok_or(AskError::NoModel)
        }
    }
}

/// `path` under the OpenAI-like API's `/v1`: an address given with its
/// version keeps it.
fn v1(base: &str, path: &str) -> String {
    if base.ends_with("/v1") {
        format!("{base}/{path}")
    } else {
        format!("{base}/v1/{path}")
    }
}

/// Asks `question`, giving each piece of the answer to `piece` as it
/// comes; returns once it is all there, or early once `stop` is set. The
/// model is the question's: see [`pick_model`] for one the configuration
/// does not name.
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
    let invalid = serde_json::from_str::<Value>(&text)
        .is_ok_and(|v| v["error"]["type"] == "invalid_request_error");
    Err(match status {
        400 if invalid => AskError::Invalid(said),
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

/// ollama: `/api/chat`, a JSON object per line.
fn ollama(
    agent: &ureq::Agent,
    base: &str,
    question: &Question,
    stop: &AtomicBool,
    piece: &mut dyn FnMut(&str),
) -> Result<(), AskError> {
    if question.model.is_empty() {
        return Err(AskError::NoModel);
    }
    let body = json!({ "model": question.model, "messages": messages(question), "stream": true });
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
    let url = v1(base, "chat/completions");
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
/// (`fallbacks`), else said. A model that refuses the request's options
/// (effort, fallbacks: a newer one may) is asked once more without them.
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
    if question.model.is_empty() {
        return Err(AskError::NoModel);
    }
    let plain = json!({
        "model": question.model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": question.system,
        "messages": [{ "role": "user", "content": question.text }],
    });
    let mut body = plain.clone();
    // A field's answer: quick rather than deep.
    body["output_config"] = json!({ "effort": "low" });
    body["fallbacks"] = json!("default");
    let headers = [
        ("x-api-key", key.as_str()),
        ("anthropic-version", ANTHROPIC_VERSION),
    ];
    let with_beta = [
        headers[0],
        headers[1],
        ("anthropic-beta", "server-side-fallback-2026-07-01"),
    ];
    let url = format!("{base}/v1/messages");
    let response = match send(agent, "POST", &url, &with_beta, Some(&body)) {
        Err(AskError::Invalid(why)) => {
            tracing::debug!(
                why,
                "Claude refused the request's options; asking without them"
            );
            send(agent, "POST", &url, &headers, Some(&plain))?
        }
        sent => sent?,
    };
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

/// A model that makes embeddings (#45), and where: ollama's
/// `/api/embed`, or the `/v1/embeddings` OpenAI defined (which llama.cpp's
/// server speaks when started with `--embeddings`).
#[derive(Clone, PartialEq, Eq)]
pub struct Embedder {
    pub provider: Provider,
    /// The provider's address, without a path.
    pub url: String,
    /// Empty: see [`pick_embedding_model`].
    pub model: String,
    pub key: Option<String>,
}

impl fmt::Debug for Embedder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Embedder")
            .field("provider", &self.provider)
            .field("url", &self.url)
            .field("model", &self.model)
            .field("key", &self.key.as_ref().map(|_| "…"))
            .finish()
    }
}

impl Embedder {
    fn auth(&self) -> Option<String> {
        self.key.as_ref().map(|k| format!("Bearer {k}"))
    }
}

/// The model to embed with when the configuration names none: the first
/// the provider lists that reads as one (`embed` in its name, or a known
/// family). Blocking.
pub fn pick_embedding_model(embedder: &Embedder) -> Result<String, AskError> {
    let agent = agent();
    let base = embedder.url.trim_end_matches('/');
    let auth = embedder.auth();
    let headers: Vec<(&str, &str)> = auth
        .as_deref()
        .map(|a| ("Authorization", a))
        .into_iter()
        .collect();
    let (url, list, field) = match embedder.provider {
        Provider::Ollama => (format!("{base}/api/tags"), "models", "name"),
        Provider::LlamaCpp | Provider::OpenAi => (v1(base, "models"), "data", "id"),
        Provider::Claude => return Err(AskError::NoModel),
    };
    let mut response = send(&agent, "GET", &url, &headers, None)?;
    let text = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .map_err(|e| AskError::Failed(e.to_string()))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| AskError::Failed(e.to_string()))?;
    let embeds = |name: &str| {
        let name = name.to_lowercase();
        ["embed", "bge", "minilm", "gte-", "e5-"]
            .iter()
            .any(|w| name.contains(w))
    };
    value[list]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m[field].as_str())
        .find(|name| embeds(name))
        .map(str::to_owned)
        .ok_or(AskError::NoModel)
}

/// The vectors of `texts`, in their order, by the embedder's model.
/// Blocking.
pub fn embed(embedder: &Embedder, texts: &[String]) -> Result<Vec<Vec<f32>>, AskError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let agent = agent();
    let base = embedder.url.trim_end_matches('/');
    let auth = embedder.auth();
    let headers: Vec<(&str, &str)> = auth
        .as_deref()
        .map(|a| ("Authorization", a))
        .into_iter()
        .collect();
    let body = json!({ "model": embedder.model, "input": texts });
    let url = match embedder.provider {
        Provider::Ollama => format!("{base}/api/embed"),
        Provider::LlamaCpp | Provider::OpenAi => v1(base, "embeddings"),
        Provider::Claude => return Err(AskError::NoModel),
    };
    let mut response = send(&agent, "POST", &url, &headers, Some(&body))?;
    let text = response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| AskError::Failed(e.to_string()))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| AskError::Failed(e.to_string()))?;
    let vector = |v: &Value| -> Option<Vec<f32>> {
        v.as_array()?
            .iter()
            .map(|x| x.as_f64().map(|x| x as f32))
            .collect()
    };
    let vectors: Option<Vec<Vec<f32>>> = match embedder.provider {
        Provider::Ollama => value["embeddings"]
            .as_array()
            .and_then(|all| all.iter().map(vector).collect::<Option<Vec<_>>>()),
        _ => {
            let mut data: Vec<&Value> = value["data"].as_array().into_iter().flatten().collect();
            data.sort_by_key(|d| d["index"].as_u64().unwrap_or(0));
            data.iter().map(|d| vector(&d["embedding"])).collect()
        }
    };
    match vectors {
        Some(v) if v.len() == texts.len() => Ok(v),
        _ => Err(AskError::Failed(format!("{url}: no vectors in the answer"))),
    }
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

    /// As the compositor asks: the model picked first when none is named.
    fn picked_answer(q: &Question) -> Result<String, AskError> {
        let mut q = q.clone();
        if q.model.is_empty() {
            q.model = pick_model(&q)?.unwrap_or_default();
        }
        answer(&q)
    }

    const CLAUDE_STREAM: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n\
         event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
         event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" there\"}}\n\n\
         event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

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
        assert_eq!(picked_answer(&q).as_deref(), Ok("Because of the cache."));
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
    fn claude_sends_its_key_and_the_model_named_as_is() {
        let (url, asked) = serve(vec![reply("text/event-stream", CLAUDE_STREAM)]);
        let mut q = question(Provider::Claude, url, Some("sk-test"));
        q.model = "claude-sonnet-7".into();
        assert_eq!(picked_answer(&q).as_deref(), Ok("Hello there"));
        let asked = asked.lock().expect("a lock");
        assert_eq!(asked.len(), 1, "no listing for a model named: {asked:?}");
        let head = asked[0].to_lowercase();
        assert!(head.contains("x-api-key: sk-test"), "{head}");
        assert!(head.contains("anthropic-version: 2026-10-21"), "{head}");
        assert!(
            asked[0].contains("\"model\":\"claude-sonnet-7\""),
            "{asked:?}"
        );
        assert!(format!("{q:?}").find("sk-test").is_none(), "never logged");
    }

    #[test]
    fn without_a_model_claude_takes_the_newest_opus_it_lists() {
        let (url, asked) = serve(vec![
            reply(
                "application/json",
                r#"{"data":[{"id":"claude-sonnet-9"},{"id":"claude-opus-8-5"},{"id":"claude-opus-8"}],"has_more":false}"#,
            ),
            reply("text/event-stream", CLAUDE_STREAM),
        ]);
        let q = question(Provider::Claude, url, Some("sk-test"));
        assert_eq!(pick_model(&q), Ok(Some("claude-opus-8-5".into())));
        // Without an Opus, the newest.
        let (url, _) = serve(vec![reply(
            "application/json",
            r#"{"data":[{"id":"claude-sonnet-9"},{"id":"claude-haiku-9"}]}"#,
        )]);
        let other = question(Provider::Claude, url, Some("sk-test"));
        assert_eq!(pick_model(&other), Ok(Some("claude-sonnet-9".into())));
        let asked = asked.lock().expect("a lock");
        assert!(asked[0].starts_with("GET /v1/models"), "{asked:?}");
        assert!(asked[0].to_lowercase().contains("x-api-key: sk-test"));
    }

    #[test]
    fn a_request_claude_finds_wrong_is_asked_again_without_its_options() {
        let refused = r#"{"type":"error","error":{"type":"invalid_request_error","message":"output_config: unknown field"}}"#;
        let (url, asked) = serve(vec![
            format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{refused}",
                refused.len()
            ),
            reply("text/event-stream", CLAUDE_STREAM),
        ]);
        let mut q = question(Provider::Claude, url, Some("sk-test"));
        q.model = "claude-opus-10".into();
        assert_eq!(answer(&q).as_deref(), Ok("Hello there"));
        let asked = asked.lock().expect("a lock");
        assert_eq!(asked.len(), 2, "once more, only once");
        assert!(asked[0].contains("effort") && asked[0].contains("fallbacks"));
        assert!(asked[0].to_lowercase().contains("anthropic-beta:"));
        for left_out in ["effort", "fallbacks", "anthropic-beta"] {
            assert!(!asked[1].to_lowercase().contains(left_out), "{left_out}");
        }
    }

    #[test]
    fn openai_like_servers_take_their_first_listed_model() {
        let (url, asked) = serve(vec![reply(
            "application/json",
            r#"{"object":"list","data":[{"id":"qwen3-8b"},{"id":"other"}]}"#,
        )]);
        let q = question(Provider::OpenAi, format!("{url}/v1"), Some("k"));
        assert_eq!(pick_model(&q), Ok(Some("qwen3-8b".into())));
        assert!(asked.lock().expect("a lock")[0].starts_with("GET /v1/models"));
        // llama.cpp's server answers without one.
        let nobody = question(Provider::LlamaCpp, "http://127.0.0.1:9".into(), None);
        assert_eq!(pick_model(&nobody), Ok(None));
    }

    #[test]
    fn embeddings_come_in_order_from_ollama_and_openai_like_servers() {
        let (url, asked) = serve(vec![
            reply(
                "application/json",
                r#"{"models":[{"name":"llama3.2:latest"},{"name":"nomic-embed-text:latest"}]}"#,
            ),
            reply(
                "application/json",
                r#"{"embeddings":[[0.1,0.2],[0.3,0.4]]}"#,
            ),
        ]);
        let mut e = Embedder {
            provider: Provider::Ollama,
            url,
            model: String::new(),
            key: None,
        };
        e.model = pick_embedding_model(&e).expect("a model");
        assert_eq!(e.model, "nomic-embed-text:latest");
        let v = embed(&e, &["a".into(), "b".into()]).expect("vectors");
        assert_eq!(v, vec![vec![0.1, 0.2], vec![0.3, 0.4]]);
        let asked = asked.lock().expect("a lock");
        assert!(asked[1].starts_with("POST /api/embed"), "{asked:?}");
        assert!(asked[1].contains(r#""input":["a","b"]"#), "{asked:?}");
        let (url, asked) = serve(vec![reply(
            "application/json",
            r#"{"data":[{"index":1,"embedding":[2.0]},{"index":0,"embedding":[1.0]}]}"#,
        )]);
        let e = Embedder {
            provider: Provider::OpenAi,
            url: format!("{url}/v1"),
            model: "text-embedding-3-small".into(),
            key: Some("k".into()),
        };
        assert_eq!(
            embed(&e, &["a".into(), "b".into()]),
            Ok(vec![vec![1.0], vec![2.0]])
        );
        let asked = asked.lock().expect("a lock");
        assert!(asked[0].starts_with("POST /v1/embeddings"), "{asked:?}");
        assert!(asked[0].to_lowercase().contains("authorization: bearer k"));
        assert!(!format!("{e:?}").contains("\"k\""), "never logged");
        assert!(!Provider::Claude.can_embed());
    }

    #[test]
    fn errors_say_what_the_provider_said() {
        let (url, _) = serve(vec![
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 47\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"invalid x-api-key\",\"t\":1}}"
                .into(),
        ]);
        let mut q = question(Provider::Claude, url, Some("wrong"));
        q.model = "claude-opus-10".into();
        assert_eq!(
            answer(&q),
            Err(AskError::Refused("invalid x-api-key".into()))
        );
        let nobody = question(Provider::Ollama, "http://127.0.0.1:9".into(), None);
        assert!(matches!(
            picked_answer(&nobody),
            Err(AskError::Unreachable(_))
        ));
    }
}
