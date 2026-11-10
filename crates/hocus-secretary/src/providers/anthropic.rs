//! The Anthropic Messages API over HTTP (`POST /v1/messages`, API version
//! `2026-10-21`); there is no official Rust SDK.
//!
//! - **Caching** (§7.3): the frozen prefix (her persona, the tools, the
//!   core card) is the system, cached an hour; the day's part opens the
//!   first user turn, cached five minutes; what was retrieved for this
//!   question sits in the last user turn, uncached. Content stays in its
//!   `<source>` blocks, never in the system.
//! - **Tools without `strict`**: the API compiles strict tools into one
//!   grammar, and hers (two readers, eleven proposals) make one too large
//!   for it ("The compiled grammar is too large"); their schemas still
//!   guide the model, and what she reads of a call is tolerant (a field
//!   missing is a field left out).
//! - **Adaptive thinking**, its depth by the role's effort.
//! - **Streaming** (SSE): the text goes to the caller as it comes; tool
//!   calls and thinking blocks are kept whole for the next turn.
//! - **Retries** (§15): an overloaded or rate-limited API, a server error
//!   or a lost connection is tried again three times, after the wait it
//!   asks for or a doubling one, as long as nothing was streamed yet.
//! - A refusal ends the reply with `stop = "refusal"`; on the models that
//!   take it, the API's own fallback (`fallbacks: "default"`) runs first.

use std::{
    io::{BufRead, BufReader},
    time::Duration,
};

use serde_json::{Value, json};

use super::Secret;
use crate::{
    context,
    model::{Message, Reply, Request, Role, ToolCall},
    tools,
};

pub const VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const TRIES: u32 = 4;
const TIMEOUT: Duration = Duration::from_secs(600);

/// A key pasted by the user, checked with the smallest request there is
/// (the models' list, one of them): no tokens spent. Blocking.
pub fn check(url: &str, key: &str) -> Result<(), hocus_secretary_link::KeyOutcome> {
    use hocus_secretary_link::KeyOutcome;
    if !crate::http::allowed(url) {
        return Err(KeyOutcome::Failed);
    }
    let url = format!("{}/v1/models", url.trim_end_matches('/'));
    let answer = crate::http::agent(Duration::from_secs(20))
        .get(&url)
        .query("limit", "1")
        .header("x-api-key", key)
        .header("anthropic-version", VERSION)
        .call();
    match answer.map(|a| a.status().as_u16()) {
        Ok(200..=299) => Ok(()),
        Ok(401 | 403) => Err(KeyOutcome::Refused),
        Ok(status) => {
            tracing::info!(status, "the provider did not check the key");
            Err(KeyOutcome::Failed)
        }
        Err(err) => {
            tracing::info!(%err, "the provider is not reached");
            Err(KeyOutcome::Offline)
        }
    }
}

/// The models that take the API's own refusal fallback.
fn falls_back(model: &str) -> bool {
    matches!(
        model,
        "claude-sonnet-5-5" | "claude-opus-5-5" | "claude-opus-5" | "claude-fable-5-1"
    )
}

/// How deep each role thinks: a chat answers fast, a briefing is worth
/// more thought, extraction is a classification.
fn effort(role: Role) -> &'static str {
    match role {
        Role::Briefing => "high",
        Role::Extraction | Role::Triage => "low",
        Role::Answer | Role::Compaction => "medium",
    }
}

fn max_tokens(role: Role) -> u32 {
    match role {
        Role::Extraction | Role::Triage => 8_000,
        Role::Answer => 16_000,
        Role::Briefing | Role::Compaction => 32_000,
    }
}

fn text(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn cached(mut block: Value, ttl: Option<&str>) -> Value {
    block["cache_control"] = match ttl {
        Some(ttl) => json!({"type": "ephemeral", "ttl": ttl}),
        None => json!({"type": "ephemeral"}),
    };
    block
}

/// A turn's blocks.
fn blocks(message: &Message) -> Vec<Value> {
    if message.from == "assistant" {
        if let Some(Value::Array(native)) = &message.native {
            return native.clone();
        }
        let mut out = Vec::new();
        if !message.text.is_empty() {
            out.push(text(&message.text));
        }
        out.extend(
            message
                .tool_calls
                .iter()
                .map(|c| json!({"type": "tool_use", "id": c.id, "name": c.name, "input": c.input})),
        );
        return out;
    }
    let mut out: Vec<Value> = message
        .tool_results
        .iter()
        .map(|r| {
            json!({"type": "tool_result", "tool_use_id": r.id, "content": r.text, "is_error": r.error})
        })
        .collect();
    if !message.text.is_empty() {
        out.push(text(&message.text));
    }
    out
}

/// The request's body.
pub fn body(model: &str, request: &Request) -> Value {
    let mut system = vec![text(&request.system)];
    let mut day = Vec::new();
    let mut retrieved = Vec::new();
    for part in &request.context {
        match part.label.as_str() {
            context::FROZEN => system.push(text(&part.text)),
            context::DAY => day.push(text(&part.text)),
            _ => retrieved.push(text(&part.text)),
        }
    }
    if let Some(last) = system.pop() {
        system.push(cached(last, Some("1h")));
    }
    if let Some(last) = day.pop() {
        day.push(cached(last, None));
    }
    let mut messages: Vec<(String, Vec<Value>)> = request
        .messages
        .iter()
        .map(|m| {
            let role = if m.from == "assistant" {
                "assistant"
            } else {
                "user"
            };
            (role.to_owned(), blocks(m))
        })
        .collect();
    if messages.first().is_none_or(|(role, _)| role != "user") {
        messages.insert(0, ("user".to_owned(), Vec::new()));
    }
    // What was retrieved for this question comes just before its words;
    // the day opens the first user turn.
    if let Some((_, last)) = messages.iter_mut().rev().find(|(role, _)| role == "user") {
        let at = last
            .iter()
            .position(|b| b["type"] == "text")
            .unwrap_or(last.len());
        last.splice(at..at, retrieved);
    }
    if let Some((_, first)) = messages.first_mut() {
        first.splice(0..0, day);
    }
    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens(request.role),
        "stream": true,
        "thinking": {"type": "adaptive"},
        "output_config": {"effort": effort(request.role)},
        "system": system,
        "messages": messages
            .into_iter()
            .filter(|(_, content)| !content.is_empty())
            .map(|(role, content)| json!({"role": role, "content": content}))
            .collect::<Vec<_>>(),
    });
    let tools: Vec<Value> = request
        .tools
        .iter()
        .filter_map(|name| tools::spec(name))
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.schema,
            })
        })
        .collect();
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if falls_back(model) {
        body["fallbacks"] = json!("default");
    }
    body
}

/// One call.
#[derive(Debug)]
pub struct Call<'a> {
    /// The API's root.
    pub url: &'a str,
    pub model: &'a str,
    pub key: &'a Secret,
    pub retry: Duration,
}

/// Why a try failed.
enum Failed {
    /// Worth another try after this wait (none: the doubling one).
    Again(String, Option<Duration>),
    /// Not.
    Stop(String),
}

impl Call<'_> {
    pub fn run(&self, request: &Request, part: &mut dyn FnMut(&str)) -> Result<Reply, String> {
        let body = body(self.model, request).to_string();
        if self.key.expose().len() >= 8 && body.contains(self.key.expose()) {
            // AC19: a keyring secret never reaches a request's body.
            return Err("the request held a key: not sent".to_owned());
        }
        let mut wait = self.retry;
        let mut last = String::new();
        for attempt in 1..=TRIES {
            let mut streamed = false;
            match self.once(&body, &mut |t| {
                streamed = true;
                part(t);
            }) {
                Ok(reply) => return Ok(reply),
                Err(Failed::Stop(err)) => return Err(err),
                Err(Failed::Again(err, _)) if streamed => {
                    return Err(format!("{err} (after part of the answer)"));
                }
                Err(Failed::Again(err, asked)) => {
                    tracing::info!(attempt, %err, "the provider failed: again");
                    last = err;
                    if attempt < TRIES {
                        std::thread::sleep(asked.unwrap_or(wait).min(Duration::from_secs(60)));
                        wait = wait.saturating_mul(2);
                    }
                }
            }
        }
        Err(format!("the provider failed {TRIES} times: {last}"))
    }

    fn once(&self, body: &str, part: &mut dyn FnMut(&str)) -> Result<Reply, Failed> {
        let url = format!("{}/v1/messages", self.url.trim_end_matches('/'));
        let agent = crate::http::agent(TIMEOUT);
        let mut request = agent
            .post(&url)
            .header("content-type", "application/json")
            .header("x-api-key", self.key.expose())
            .header("anthropic-version", VERSION)
            .header("accept", "text/event-stream");
        if falls_back(self.model) {
            request = request.header("anthropic-beta", FALLBACK_BETA);
        }
        let answer = request
            .send(body.as_bytes())
            .map_err(|e| Failed::Again(format!("{url}: {e}"), None))?;
        let status = answer.status().as_u16();
        let id = answer
            .headers()
            .get("request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if status != 200 {
            let asked = answer
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            let mut answer = answer;
            let text = answer
                .body_mut()
                .with_config()
                .limit(64 * 1024)
                .read_to_string()
                .unwrap_or_default();
            // The API's error, as it says it (#755): its type, its message
            // and the request's id, never anything of the request.
            let error = serde_json::from_str::<Value>(&text)
                .ok()
                .map(|v| v["error"].clone())
                .unwrap_or_default();
            let kind = error["type"].as_str().unwrap_or_default();
            let message: String = error["message"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect();
            let err = format!("the API answered {status} {kind}: {message} (request {id})");
            return Err(match status {
                408 | 409 | 429 | 500..=599 => Failed::Again(err, asked),
                _ => Failed::Stop(err),
            });
        }
        let reader = BufReader::new(answer.into_body().into_reader());
        read_stream(reader, part)
    }
}

/// A content block being streamed.
struct Block {
    value: Value,
    /// A tool call's input, as it comes.
    json: String,
}

/// The SSE stream into a reply.
fn read_stream(reader: impl BufRead, part: &mut dyn FnMut(&str)) -> Result<Reply, Failed> {
    let mut reply = Reply::default();
    let mut blocks: Vec<Block> = Vec::new();
    let mut ended = false;
    for line in reader.lines() {
        let line = line.map_err(|e| Failed::Again(format!("the stream broke: {e}"), None))?;
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
            continue;
        };
        match event["type"].as_str().unwrap_or_default() {
            "message_start" => {
                let usage = &event["message"]["usage"];
                reply.tokens_in = usage["input_tokens"].as_u64().unwrap_or_default();
                reply.cache_read = usage["cache_read_input_tokens"]
                    .as_u64()
                    .unwrap_or_default();
                reply.cache_written = usage["cache_creation_input_tokens"]
                    .as_u64()
                    .unwrap_or_default();
            }
            "content_block_start" => {
                let index = event["index"].as_u64().unwrap_or_default() as usize;
                while blocks.len() <= index {
                    blocks.push(Block {
                        value: Value::Null,
                        json: String::new(),
                    });
                }
                blocks[index].value = event["content_block"].clone();
            }
            "content_block_delta" => {
                let index = event["index"].as_u64().unwrap_or_default() as usize;
                let Some(block) = blocks.get_mut(index) else {
                    continue;
                };
                let delta = &event["delta"];
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let t = delta["text"].as_str().unwrap_or_default();
                        append(&mut block.value, "text", t);
                        part(t);
                    }
                    "thinking_delta" => {
                        append(
                            &mut block.value,
                            "thinking",
                            delta["thinking"].as_str().unwrap_or_default(),
                        );
                    }
                    "signature_delta" => {
                        block.value["signature"] = delta["signature"].clone();
                    }
                    "input_json_delta" => {
                        block
                            .json
                            .push_str(delta["partial_json"].as_str().unwrap_or_default());
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = event["index"].as_u64().unwrap_or_default() as usize;
                if let Some(block) = blocks.get_mut(index)
                    && block.value["type"] == "tool_use"
                    && !block.json.is_empty()
                {
                    block.value["input"] = serde_json::from_str(&block.json).map_err(|e| {
                        Failed::Stop(format!("a tool call's input does not read: {e}"))
                    })?;
                }
            }
            "message_delta" => {
                if let Some(stop) = event["delta"]["stop_reason"].as_str() {
                    stop.clone_into(&mut reply.stop);
                }
                if let Some(out) = event["usage"]["output_tokens"].as_u64() {
                    reply.tokens_out = out;
                }
            }
            "message_stop" => ended = true,
            "error" => {
                let kind = event["error"]["type"].as_str().unwrap_or_default();
                let message: String = event["error"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(300)
                    .collect();
                let err = format!("the stream said {kind}: {message}");
                return Err(match kind {
                    "overloaded_error" | "api_error" | "rate_limit_error" => {
                        Failed::Again(err, None)
                    }
                    _ => Failed::Stop(err),
                });
            }
            _ => {}
        }
    }
    if !ended {
        return Err(Failed::Again("the stream ended early".to_owned(), None));
    }
    let native: Vec<Value> = blocks
        .into_iter()
        .map(|b| b.value)
        .filter(|v| !v.is_null())
        .collect();
    for block in &native {
        match block["type"].as_str().unwrap_or_default() {
            "text" => reply
                .text
                .push_str(block["text"].as_str().unwrap_or_default()),
            "tool_use" => reply.tool_calls.push(ToolCall {
                id: block["id"].as_str().unwrap_or_default().to_owned(),
                name: block["name"].as_str().unwrap_or_default().to_owned(),
                input: block["input"].clone(),
            }),
            _ => {}
        }
    }
    reply.native = Some(Value::Array(native));
    Ok(reply)
}

fn append(block: &mut Value, field: &str, more: &str) {
    let mut text = block[field].as_str().unwrap_or_default().to_owned();
    text.push_str(more);
    block[field] = Value::String(text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Part;

    #[test]
    fn the_body_caches_the_prefix_and_the_day_and_keeps_content_out_of_the_system() {
        let request = Request {
            role: Role::Answer,
            system: "Tu es la secrétaire.".to_owned(),
            context: vec![
                Part {
                    label: context::FROZEN.to_owned(),
                    text: "Carte : Alex".to_owned(),
                    items: vec![],
                },
                Part {
                    label: context::DAY.to_owned(),
                    text: "<today>réunion 14 h</today>".to_owned(),
                    items: vec![],
                },
                Part {
                    label: context::RETRIEVED.to_owned(),
                    text: "<source id=\"m1\">le devis</source>".to_owned(),
                    items: vec!["m1".to_owned()],
                },
            ],
            messages: vec![
                Message::user("Bonjour"),
                Message::assistant("Bonjour !"),
                Message::user("Et le devis ?"),
            ],
            tools: vec!["graph".to_owned(), "nonesuch".to_owned()],
        };
        let body = body("claude-sonnet-5-5", &request);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "medium");
        assert_eq!(body["stream"], true);
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["system"][1]["cache_control"]["ttl"], "1h");
        assert!(!body["system"].to_string().contains("source"));
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(
            messages[2]["content"][0]["text"],
            "<source id=\"m1\">le devis</source>"
        );
        let alone = super::body(
            "claude-sonnet-5-5",
            &Request {
                messages: vec![Message::user("Et le devis ?")],
                ..request.clone()
            },
        );
        let order: Vec<&str> = alone["messages"][0]["content"]
            .as_array()
            .expect("blocks")
            .iter()
            .map(|b| b["text"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            order,
            [
                "<today>réunion 14 h</today>",
                "<source id=\"m1\">le devis</source>",
                "Et le devis ?"
            ]
        );
        assert_eq!(messages[2]["content"][1]["text"], "Et le devis ?");
        let tools = body["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), 1);
        assert!(
            tools[0].get("strict").is_none(),
            "the API refuses hers as strict"
        );
        assert_eq!(tools[0]["input_schema"]["additionalProperties"], false);
    }

    #[test]
    fn a_stream_gives_its_text_as_it_comes_and_its_tool_calls_whole() {
        let stream = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":120,\"cache_read_input_tokens\":100}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Nadia \"}}\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"et Sophie.\"}}\n\
data: {\"type\":\"content_block_stop\",\"index\":1}\n\
data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"graph\",\"input\":{}}}\n\
data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"node\\\":\"}}\n\
data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Chapitô\\\"}\"}}\n\
data: {\"type\":\"content_block_stop\",\"index\":2}\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":40}}\n\
data: {\"type\":\"message_stop\"}\n";
        let mut parts = Vec::new();
        let reply = read_stream(stream.as_bytes(), &mut |t| parts.push(t.to_owned()))
            .unwrap_or_else(|_| panic!("a reply"));
        assert_eq!(parts, ["Nadia ", "et Sophie."]);
        assert_eq!(reply.text, "Nadia et Sophie.");
        assert_eq!(reply.tool_calls[0].input["node"], "Chapitô");
        assert_eq!(reply.tool_calls[0].id, "toolu_1");
        assert_eq!(
            (reply.tokens_in, reply.tokens_out, reply.cache_read),
            (120, 40, 100)
        );
        assert_eq!(reply.stop, "tool_use");
        let native = reply.native.expect("its blocks");
        assert_eq!(native[0]["signature"], "sig");
        let replayed = blocks(&Message::replied(&Reply {
            native: Some(native.clone()),
            ..Reply::default()
        }));
        assert_eq!(Value::Array(replayed), native);
    }
}
