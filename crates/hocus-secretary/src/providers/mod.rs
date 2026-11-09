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
}

impl Router {
    pub fn new(settings: Arc<settings::File>, keys: Arc<dyn Keys>) -> Self {
        Self {
            settings,
            keys,
            retry: Duration::from_secs(1),
        }
    }

    fn key(&self, provider: &str, kind: Kind) -> Result<Option<Secret>, String> {
        let key = self.keys.key(provider)?;
        if key.is_none() && kind == Kind::Anthropic {
            return Err(format!(
                "no key for {provider}: hocus-secretary key {provider} (or Settings)"
            ));
        }
        Ok(key)
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
        _clock: &Clock,
        part: &mut dyn FnMut(&str),
    ) -> Result<Reply, String> {
        if request.role == Role::Triage {
            return Err("the triage goes to the local queue, not a conversation".to_owned());
        }
        let settings = self.settings.current();
        let (choice, provider) = settings.provider(request.role)?;
        if !crate::http::allowed(&provider.url) {
            return Err(format!(
                "{}: only HTTPS, or this machine in clear",
                choice.provider
            ));
        }
        let key = self.key(&choice.provider, provider.kind)?;
        let mut reply = match provider.kind {
            Kind::Anthropic => {
                let key = key.ok_or("no key")?;
                anthropic::Call {
                    url: &provider.url,
                    model: &choice.model,
                    key: &key,
                    retry: self.retry,
                }
                .run(request, part)?
            }
            Kind::Openai | Kind::Ollama => {
                let reply = openai::Endpoint {
                    url: &provider.url,
                    key: key.as_ref(),
                }
                .converse(&choice.model, request)?;
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
        let (content, tokens_in, tokens_out) = endpoint.chat(
            &choice.model,
            &[
                ("system", triage::PROMPT.to_owned()),
                ("user", input.to_owned()),
            ],
            Some(("triage", triage::schema())),
        )?;
        let verdict = serde_json::from_str(&content)
            .map_err(|e| format!("the triage's answer is not its schema: {e}"))?;
        Ok((verdict, tokens_in, tokens_out))
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
