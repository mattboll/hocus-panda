//! Her providers (§11.1): one internal request shape ([`crate::model`]),
//! adapters for the Anthropic Messages API ([`anthropic`]) and
//! OpenAI-compatible servers ([`openai`]), and the [`Router`] that gives
//! each role the provider and model the settings name, read again at each
//! call (AC22).
//!
//! Keys are in the keyring of the bus she is given, never in the
//! configuration; each is read on a call's worker thread and kept in
//! memory for the next calls. A request body that would hold a key is
//! never sent.

pub mod anthropic;
pub mod openai;

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use crate::{
    clock::Clock,
    model::{Model, Reply, Request, Role},
    settings::{self, Kind},
    triage::{self, Triager, Verdict},
};

/// A secret: never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(…)")
    }
}

/// Where the providers' keys are.
pub trait Keys: Send + Sync + fmt::Debug {
    /// `provider`'s key; `Ok(None)`: it has none.
    fn key(&self, provider: &str) -> Result<Option<Secret>, String>;

    /// `key` kept as `provider`'s, from then on (her drawer, the
    /// Settings, #752).
    fn keep(&self, _provider: &str, _key: &Secret) -> Result<(), String> {
        Err("these keys are fixed".to_owned())
    }

    /// `provider`'s key taken out.
    fn erase(&self, _provider: &str) -> Result<(), String> {
        Err("these keys are fixed".to_owned())
    }
}

/// The keyring on the bus she was given (§11.3); without a bus, no key.
#[derive(Debug)]
pub struct Keyring {
    bus: Option<String>,
    read: Mutex<BTreeMap<String, Secret>>,
}

impl Keyring {
    pub fn new(bus: Option<String>) -> Self {
        Self {
            bus,
            read: Mutex::new(BTreeMap::new()),
        }
    }
}

impl Keys for Keyring {
    fn key(&self, provider: &str) -> Result<Option<Secret>, String> {
        if let Some(key) = self
            .read
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(provider)
        {
            return Ok(Some(key.clone()));
        }
        let Some(bus) = &self.bus else {
            return Ok(None);
        };
        let found = crate::key::provider_key(bus, provider)?.map(Secret);
        if let Some(key) = &found {
            self.read
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(provider.to_owned(), key.clone());
        }
        Ok(found)
    }

    fn keep(&self, provider: &str, key: &Secret) -> Result<(), String> {
        let bus = self.bus.as_deref().ok_or("no keyring known")?;
        if !crate::key::keep_provider_key(bus, provider, key.expose()) {
            return Err("the keyring did not keep it".to_owned());
        }
        self.read
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(provider.to_owned(), key.clone());
        Ok(())
    }

    fn erase(&self, provider: &str) -> Result<(), String> {
        let bus = self.bus.as_deref().ok_or("no keyring known")?;
        self.read
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(provider);
        if crate::key::erase_provider_key(bus, provider) {
            Ok(())
        } else {
            Err("the keyring does not answer".to_owned())
        }
    }
}

/// Keys kept in memory only (tests): given, kept and erased.
#[derive(Debug, Default)]
pub struct Held(pub Mutex<BTreeMap<String, Secret>>);

impl Keys for Held {
    fn key(&self, provider: &str) -> Result<Option<Secret>, String> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(provider)
            .cloned())
    }

    fn keep(&self, provider: &str, key: &Secret) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(provider.to_owned(), key.clone());
        Ok(())
    }

    fn erase(&self, provider: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(provider);
        Ok(())
    }
}

/// Keys given as they are (tests).
#[derive(Debug, Default)]
pub struct Given(pub BTreeMap<String, Secret>);

impl Keys for Given {
    fn key(&self, provider: &str) -> Result<Option<Secret>, String> {
        Ok(self.0.get(provider).cloned())
    }
}

/// Each role to its provider and model, as the settings say now.
#[derive(Debug)]
pub struct Router {
    settings: Arc<settings::File>,
    keys: Arc<dyn Keys>,
    /// The first wait before a retry (doubled each time), when the
    /// provider gives none.
    pub retry: Duration,
    /// Where Ollama held the triage's model at its last answer.
    placed: Mutex<Option<openai::Placement>>,
}

impl Router {
    pub fn new(settings: Arc<settings::File>, keys: Arc<dyn Keys>) -> Self {
        Self {
            settings,
            keys,
            retry: Duration::from_secs(1),
            placed: Mutex::new(None),
        }
    }
}

impl Router {
    /// The provider and model `role` goes to now, fixed.
    pub fn pinned(&self, role: Role) -> Result<Pinned, String> {
        if role == Role::Triage {
            return Err("the triage goes to the local queue, not a conversation".to_owned());
        }
        let settings = self.settings.current();
        let (choice, provider) = settings.provider(role)?;
        Ok(Pinned {
            choice: choice.clone(),
            provider: provider.clone(),
            keys: self.keys.clone(),
            retry: self.retry,
        })
    }
}

impl Model for Router {
    fn provider(&self) -> &str {
        "settings"
    }

    fn model(&self) -> &str {
        "by role"
    }

    fn call(&self, request: &Request, clock: &Clock) -> Result<Reply, String> {
        self.stream(request, clock, &mut |_| {})
    }

    fn stream(
        &self,
        request: &Request,
        clock: &Clock,
        part: &mut dyn FnMut(&str),
    ) -> Result<Reply, String> {
        self.pinned(request.role)?.stream(request, clock, part)
    }

    fn pin(&self, role: Role) -> Result<Option<Arc<dyn Model>>, String> {
        Ok(Some(Arc::new(self.pinned(role)?)))
    }

    /// The model that answers now, as the drawer says it.
    fn shown(&self) -> String {
        self.pinned(Role::Answer)
            .map(|p| p.shown())
            .unwrap_or_default()
    }

    fn needs_key(&self) -> bool {
        true
    }

    fn key_provider(&self) -> Option<String> {
        let pinned = self.pinned(Role::Answer).ok()?;
        (pinned.provider.kind == Kind::Anthropic).then_some(pinned.choice.provider)
    }

    fn keep_key(
        &self,
        provider: &str,
        key: Option<&hocus_secretary_link::Secret>,
    ) -> hocus_secretary_link::KeyOutcome {
        use hocus_secretary_link::KeyOutcome;
        let settings = self.settings.current();
        let Some(known) = settings.providers.get(provider) else {
            return KeyOutcome::Failed;
        };
        let Some(key) = key else {
            return match self.keys.erase(provider) {
                Ok(()) => KeyOutcome::Erased,
                Err(err) => {
                    tracing::info!(provider, %err, "the key is not erased");
                    KeyOutcome::NoKeyring
                }
            };
        };
        let text = key.expose().trim();
        let anthropic = known.kind == Kind::Anthropic;
        if text.is_empty()
            || text.chars().any(|c| c.is_whitespace() || c.is_control())
            || (anthropic && !text.starts_with("sk-ant-"))
        {
            return KeyOutcome::Malformed;
        }
        if anthropic && let Err(outcome) = anthropic::check(&known.url, text) {
            return outcome;
        }
        match self.keys.keep(provider, &Secret::new(text)) {
            Ok(()) => KeyOutcome::Kept,
            Err(err) => {
                tracing::info!(provider, %err, "the key is not kept");
                KeyOutcome::NoKeyring
            }
        }
    }

    fn missing_key(&self) -> Option<String> {
        let pinned = self.pinned(Role::Answer).ok()?;
        if pinned.provider.kind != Kind::Anthropic {
            return None;
        }
        match self.keys.key(&pinned.choice.provider) {
            Ok(Some(_)) => None,
            Ok(None) => Some(pinned.choice.provider),
            Err(err) => {
                tracing::info!(%err, "the keyring does not give her key");
                Some(pinned.choice.provider)
            }
        }
    }
}

/// One provider and model, as the settings named them when it was made:
/// a turn's, whatever the settings say meanwhile (#750).
#[derive(Debug, Clone)]
pub struct Pinned {
    choice: settings::Choice,
    provider: settings::Provider,
    keys: Arc<dyn Keys>,
    retry: Duration,
}

impl Pinned {
    fn key(&self) -> Result<Option<Secret>, String> {
        let key = self.keys.key(&self.choice.provider)?;
        if key.is_none() && self.provider.kind == Kind::Anthropic {
            return Err(format!(
                "no key for {0}: hocus-secretary key {0} (or Settings)",
                self.choice.provider
            ));
        }
        Ok(key)
    }
}

impl Model for Pinned {
    fn provider(&self) -> &str {
        &self.choice.provider
    }

    fn model(&self) -> &str {
        &self.choice.model
    }

    fn call(&self, request: &Request, clock: &Clock) -> Result<Reply, String> {
        self.stream(request, clock, &mut |_| {})
    }

    fn stream(
        &self,
        request: &Request,
        _clock: &Clock,
        part: &mut dyn FnMut(&str),
    ) -> Result<Reply, String> {
        if request.role == Role::Triage {
            return Err("the triage goes to the local queue, not a conversation".to_owned());
        }
        let (choice, provider) = (&self.choice, &self.provider);
        if !crate::http::allowed(&provider.url) {
            return Err(format!(
                "{}: only HTTPS, or this machine in clear",
                choice.provider
            ));
        }
        // Another model's own blocks never reach this one: its turns go
        // as their text and tool calls.
        let by = format!("{}/{}", choice.provider, choice.model);
        let mut request = request.clone();
        for message in &mut request.messages {
            if message.native.is_some() && message.by != by {
                tracing::info!(from = %message.by, to = %by, "another model's blocks: plain text");
                message.native = None;
            }
        }
        let key = self.key()?;
        let mut reply = match provider.kind {
            Kind::Anthropic => {
                let key = key.ok_or("no key")?;
                anthropic::Call {
                    url: &provider.url,
                    model: &choice.model,
                    key: &key,
                    retry: self.retry,
                }
                .run(&request, part)?
            }
            Kind::Openai | Kind::Ollama => {
                let reply = openai::Endpoint {
                    url: &provider.url,
                    key: key.as_ref(),
                }
                .converse(&choice.model, &request)?;
                if !reply.text.is_empty() {
                    part(&reply.text);
                }
                reply
            }
        };
        reply.provider.clone_from(&choice.provider);
        reply.model.clone_from(&choice.model);
        Ok(reply)
    }

    fn shown(&self) -> String {
        shown(&self.choice.model)
    }
}

/// A model's name as people say it: `claude-sonnet-5-5` is "Claude
/// Sonnet 5.5"; another stays as it is (`qwen3:8b`).
pub fn shown(model: &str) -> String {
    let Some(rest) = model.strip_prefix("claude-") else {
        return model.to_owned();
    };
    let (words, numbers): (Vec<&str>, Vec<&str>) = rest
        .split('-')
        .partition(|w| !w.chars().all(|c| c.is_ascii_digit()));
    let mut out = String::from("Claude");
    for word in words {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(' ');
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if !numbers.is_empty() {
        out.push(' ');
        out.push_str(&numbers.join("."));
    }
    out
}

impl Triager for Router {
    fn endpoint(&self) -> (String, String) {
        let settings = self.settings.current();
        let choice = settings.models.of(Role::Triage);
        (choice.provider.clone(), choice.model.clone())
    }

    fn triage(&self, input: &str) -> Result<(Verdict, u64, u64), String> {
        let settings = self.settings.current();
        let (choice, provider) = settings.provider(Role::Triage)?;
        if provider.kind == Kind::Anthropic || !crate::http::local(&provider.url) {
            // What it reads has been through the first pass only.
            return Err(format!(
                "the triage runs on a model of this machine only, not {}",
                choice.provider
            ));
        }
        let key = self.keys.key(&choice.provider)?;
        let endpoint = openai::Endpoint {
            url: &provider.url,
            key: key.as_ref(),
        };
        let context = settings.triage.context.max(1024);
        let messages = [
            ("system", triage::PROMPT.to_owned()),
            ("user", triage::fitted(input, context)),
        ];
        let (content, tokens_in, tokens_out) = if provider.kind == Kind::Ollama {
            let answer =
                endpoint.ollama_chat(&choice.model, &messages, triage::schema(), context)?;
            // Where the model is held, now that it is loaded: cheap, and
            // only said in her status.
            *self
                .placed
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = endpoint.ollama_placement(&choice.model);
            answer
        } else {
            endpoint.chat(&choice.model, &messages, Some(("triage", triage::schema())))?
        };
        let verdict = serde_json::from_str(&content)
            .map_err(|e| format!("the triage's answer is not its schema: {e}"))?;
        Ok((verdict, tokens_in, tokens_out))
    }

    fn placement(&self) -> Option<openai::Placement> {
        *self.placed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn unload(&self) -> Result<(), String> {
        let settings = self.settings.current();
        let (choice, provider) = settings.provider(Role::Triage)?;
        if provider.kind != Kind::Ollama || !crate::http::local(&provider.url) {
            return Ok(());
        }
        openai::Endpoint {
            url: &provider.url,
            key: None,
        }
        .unload(&choice.model)
    }
}
