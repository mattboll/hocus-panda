//! OpenAI-compatible chat completions (`POST {url}/chat/completions`):
//! the local triage model (Ollama, llama.cpp), and later a big model on
//! another machine of the user's (§11.1). Not streamed: the triage's
//! answer is one small JSON object, constrained to its schema
//! (`response_format: json_schema`). Ollama also unloads a model on
//! `POST /api/generate {"model", "keep_alive": 0}`.

use std::time::Duration;

use serde_json::{Value, json};

use super::Secret;
use crate::model::{Reply, Request};

/// A local model's cold start and an 8B model's answer fit in this.
const TIMEOUT: Duration = Duration::from_secs(180);
const MOST: u64 = 4 * 1024 * 1024;

/// One endpoint.
#[derive(Debug, Clone, Copy)]
pub struct Endpoint<'a> {
    /// The API's base, `…/v1`.
    pub url: &'a str,
    pub key: Option<&'a Secret>,
}

impl Endpoint<'_> {
    fn post(&self, path: &str, body: &Value) -> Result<(u16, String), String> {
        let url = format!("{}{path}", self.url.trim_end_matches('/'));
        if !crate::http::allowed(&url) {
            return Err(format!("{url}: only HTTPS, or this machine in clear"));
        }
        let text = body.to_string();
        if let Some(key) = self.key
            && text.contains(key.expose())
        {
            return Err("the request held a key: not sent".to_owned());
        }
        let agent = crate::http::agent(TIMEOUT);
        let mut request = agent.post(&url).header("content-type", "application/json");
        if let Some(key) = self.key {
            request = request.header("authorization", &format!("Bearer {}", key.expose()));
        }
        let mut answer = request
            .send(text.as_bytes())
            .map_err(|e| format!("{url}: {e}"))?;
        let status = answer.status().as_u16();
        let text = answer
            .body_mut()
            .with_config()
            .limit(MOST)
            .read_to_string()
            .map_err(|e| format!("{url}: {e}"))?;
        Ok((status, text))
    }

    /// A chat: the answer's content and the tokens in and out. With a
    /// schema, the answer is constrained to it.
    pub fn chat(
        &self,
        model: &str,
        messages: &[(&str, String)],
        schema: Option<(&str, Value)>,
    ) -> Result<(String, u64, u64), String> {
        let mut body = json!({
            "model": model,
            "messages": messages
                .iter()
                .map(|(role, text)| json!({"role": role, "content": text}))
                .collect::<Vec<_>>(),
            "stream": false,
        });
        if let Some((name, schema)) = schema {
            body["temperature"] = json!(0);
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {"name": name, "strict": true, "schema": schema},
            });
        }
        let (status, text) = self.post("/chat/completions", &body)?;
        if status != 200 {
            return Err(format!("the model's server answered {status}"));
        }
        let answer: Value =
            serde_json::from_str(&text).map_err(|e| format!("the model's answer: {e}"))?;
        let content = answer["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("the model's answer has no content")?
            .to_owned();
        let usage = &answer["usage"];
        Ok((
            content,
            usage["prompt_tokens"].as_u64().unwrap_or_default(),
            usage["completion_tokens"].as_u64().unwrap_or_default(),
        ))
    }

    /// A conversation's request, as plain chat: the system and its
    /// context as the system message (content stays in its `<source>`
    /// blocks), the turns' texts. Tools are not offered here yet.
    pub fn converse(&self, model: &str, request: &Request) -> Result<Reply, String> {
        let mut system = request.system.clone();
        for part in &request.context {
            system.push_str("\n\n");
            system.push_str(&part.text);
        }
        let mut messages = vec![("system", system)];
        for m in &request.messages {
            let role = if m.from == "assistant" {
                "assistant"
            } else {
                "user"
            };
            messages.push((role, m.text.clone()));
        }
        let (text, tokens_in, tokens_out) = self.chat(model, &messages, None)?;
        Ok(Reply {
            text,
            tokens_in,
            tokens_out,
            stop: "end_turn".to_owned(),
            ..Reply::default()
        })
    }

    /// Ollama unloads `model` now (`keep_alive` 0), on its native API
    /// beside the OpenAI-compatible one.
    pub fn unload(&self, model: &str) -> Result<(), String> {
        let native = Endpoint {
            url: self.url.trim_end_matches('/').trim_end_matches("/v1"),
            key: None,
        };
        let (status, _) =
            native.post("/api/generate", &json!({"model": model, "keep_alive": 0}))?;
        if status == 200 {
            Ok(())
        } else {
            Err(format!(
                "the model's server answered {status} to the unload"
            ))
        }
    }
}
