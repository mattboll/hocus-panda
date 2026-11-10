//! OpenAI-compatible chat completions (`POST {url}/chat/completions`):
//! the local triage model (Ollama, llama.cpp), and later a big model on
//! another machine of the user's (§11.1). Not streamed: the triage's
//! answer is one small JSON object, constrained to its schema
//! (`response_format: json_schema`). Ollama's own API serves the triage
//! instead (#752): `POST /api/chat` takes `options.num_ctx`, which `/v1`
//! leaves out (Ollama's default context, 40k tokens, takes an 8B model
//! past an 8 GB card), and `options.num_thread` (#756: with the model
//! whole on the card, its runner need not heat the processor), the schema
//! in `format`; it unloads a model on
//! `POST /api/generate {"model", "keep_alive": 0}` and says where the
//! models it holds are on `GET /api/ps`.

use std::time::Duration;

use serde_json::{Value, json};

use super::Secret;
use crate::model::{Reply, Request};

/// A local model's cold start and an 8B model's answer fit in this.
const TIMEOUT: Duration = Duration::from_secs(180);
const MOST: u64 = 4 * 1024 * 1024;

/// Where a local model is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// On the graphics card, whole.
    Gpu,
    /// Partly in the processor's memory: slower.
    Split,
    /// On the processor only.
    Cpu,
}

impl Placement {
    /// As her status says it.
    pub fn words(self, fr: bool) -> &'static str {
        match (self, fr) {
            (Self::Gpu, true) => "tri : sur la carte graphique",
            (Self::Gpu, false) => "sorting: on the graphics card",
            (Self::Split, true) => "tri : en partie sur le processeur",
            (Self::Split, false) => "sorting: partly on the processor",
            (Self::Cpu, true) => "tri : sur le processeur",
            (Self::Cpu, false) => "sorting: on the processor",
        }
    }
}

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
            // Written once, with its room from the start, and wiped once
            // the request has it.
            let mut bearer =
                zeroize::Zeroizing::new(String::with_capacity(7 + key.expose().len()));
            bearer.push_str("Bearer ");
            bearer.push_str(key.expose());
            request = request.header("authorization", bearer.as_str());
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

    /// Ollama's native API, beside the OpenAI-compatible one.
    fn native(&self) -> Endpoint<'_> {
        Endpoint {
            url: self.url.trim_end_matches('/').trim_end_matches("/v1"),
            key: self.key,
        }
    }

    /// A chat on Ollama's own API: its context `num_ctx` tokens, its
    /// runner on `num_thread` threads when given, the answer constrained
    /// to `schema`, at temperature 0.
    pub fn ollama_chat(
        &self,
        model: &str,
        messages: &[(&str, String)],
        schema: Value,
        num_ctx: u32,
        num_thread: Option<u32>,
    ) -> Result<(String, u64, u64), String> {
        let mut options = json!({"num_ctx": num_ctx, "temperature": 0});
        if let Some(threads) = num_thread.filter(|t| *t > 0) {
            options["num_thread"] = json!(threads);
        }
        let body = json!({
            "model": model,
            "messages": messages
                .iter()
                .map(|(role, text)| json!({"role": role, "content": text}))
                .collect::<Vec<_>>(),
            "stream": false,
            "format": schema,
            "options": options,
        });
        let (status, text) = self.native().post("/api/chat", &body)?;
        if status != 200 {
            return Err(format!("the model's server answered {status}"));
        }
        let answer: Value =
            serde_json::from_str(&text).map_err(|e| format!("the model's answer: {e}"))?;
        let content = answer["message"]["content"]
            .as_str()
            .ok_or("the model's answer has no content")?
            .to_owned();
        Ok((
            content,
            answer["prompt_eval_count"].as_u64().unwrap_or_default(),
            answer["eval_count"].as_u64().unwrap_or_default(),
        ))
    }

    /// Where Ollama holds `model` now (`GET /api/ps`): on the graphics
    /// card whole, or partly in the processor's memory; none when it does
    /// not say.
    pub fn ollama_placement(&self, model: &str) -> Option<Placement> {
        let url = format!("{}/api/ps", self.native().url);
        if !crate::http::local(&url) {
            return None;
        }
        let mut answer = crate::http::agent(Duration::from_secs(5))
            .get(&url)
            .call()
            .ok()?;
        let text = answer
            .body_mut()
            .with_config()
            .limit(MOST)
            .read_to_string()
            .ok()?;
        let ps: Value = serde_json::from_str(&text).ok()?;
        let held = ps["models"]
            .as_array()?
            .iter()
            .find(|m| m["name"] == model || m["model"] == model)?;
        let (size, vram) = (held["size"].as_u64()?, held["size_vram"].as_u64()?);
        Some(if vram >= size {
            Placement::Gpu
        } else if vram == 0 {
            Placement::Cpu
        } else {
            Placement::Split
        })
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
        let (status, _) = self
            .native()
            .post("/api/generate", &json!({"model": model, "keep_alive": 0}))?;
        if status == 200 {
            Ok(())
        } else {
            Err(format!(
                "the model's server answered {status} to the unload"
            ))
        }
    }
}
