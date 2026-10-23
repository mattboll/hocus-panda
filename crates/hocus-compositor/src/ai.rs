//! Questions asked in the command field (#44), or about a circled area
//! (#46): each goes to the provider the configuration names on a worker
//! thread of its own, which reads the key from the keyring and sends the
//! answer back piece by piece through a channel; the loop only adds the
//! pieces to the card. Typing again, closing the field or the circle, or
//! Stop tells the worker to stop at the next piece.
//!
//! Without a model named, the worker asks the provider which it has at
//! the first question (the newest Claude Opus, ollama's first…), and the
//! session keeps that one: models change too often to be written here.
//!
//! Nested or in tests, a provider's usual address is never asked (the
//! developer's own model is not ours): only the address the configuration
//! gives, and only the session's own keyring.
//!
//! The model is told which actions it may propose (#197); once an answer
//! is all there, what it proposes is offered under it (`ai_actions`).

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use hocus_platform::{
    ai::{self, AskError, Provider, Question},
    secrets::Secrets,
};
use hocus_shell::{
    Lang,
    answer::{self, News as AnswerNews, Source},
    proposals,
};
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

/// Who asked, and gets the answer: the command field, or a circle (#46).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    Launcher,
    Circle,
}

/// What the workers send back.
#[derive(Debug)]
pub(crate) enum News {
    Piece(Asker, u64, String),
    Done(Asker, u64),
    Failed(Asker, u64, AskError),
    /// The model a provider at an address picked, kept for the session.
    Model(Provider, String, String),
    /// A key was stored, or not.
    Stored(Result<String, String>),
    /// The providers with a key kept, as read.
    Keys(Result<Vec<String>, String>),
    /// A configuration change an answer proposed was written, as said, or
    /// not (#197).
    Written(Result<String, String>),
}

/// The question being answered.
#[derive(Debug)]
struct Asking {
    asker: Asker,
    id: u64,
    stop: Arc<AtomicBool>,
    since: Duration,
}

#[derive(Debug, Default)]
pub struct Ai {
    pub(crate) news: Option<channel::Sender<News>>,
    asking: Option<Asking>,
    secrets: Option<Secrets>,
    /// The model picked for a provider at an address, for the session.
    model: Option<(Provider, String, String)>,
    /// The providers with a key kept, for the settings; `None` until read.
    keys: Option<Vec<String>>,
}

pub fn start(
    handle: &LoopHandle<'static, App>,
    state: &mut State,
    dir: Option<PathBuf>,
    session_bus: Option<&str>,
) {
    let (news, received) = channel::channel::<News>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(news) = event {
            app.timed("ai", |app| app.state.ai_news(news));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "questions will get no answers");
        return;
    }
    state.ai.news = Some(news);
    state.ai.secrets = dir.map(|dir| Secrets {
        // The session's own keyring only: nested, the developer's is not ours.
        bus: state
            .is_session
            .then(|| session_bus.map(str::to_owned))
            .flatten(),
        fallback: dir.join("ai-secrets"),
        attribute: "ai-provider",
    });
    state.read_ai_keys();
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%err, "could not start a thread");
    }
}

/// What the user reads when no answer came.
fn said(err: &AskError, url: &str, lang: Lang) -> String {
    match err {
        AskError::Unreachable(_) => lang.ai_unreachable(url),
        AskError::Refused(why) => lang.ai_refused(why),
        AskError::NoModel => lang.ai_no_model().into(),
        AskError::Declined => lang.ai_declined().into(),
        AskError::Invalid(why) | AskError::Failed(why) => lang.ai_failed(why),
    }
}

impl State {
    /// Where the AI keeps its keys, for the document search's vectors.
    pub(crate) fn ai_secrets(&self) -> Option<Secrets> {
        self.ai.secrets.clone()
    }

    /// The provider, and its address; `None` when nothing may be asked.
    pub(crate) fn ai_provider(&self) -> Option<(Provider, String)> {
        let config = &self.config.ai;
        if !config.enabled {
            return None;
        }
        let provider = Provider::from_name(&config.provider)?;
        let url = if !config.url.is_empty() {
            config.url.clone()
        } else if self.is_session {
            provider.default_url().to_owned()
        } else {
            return None;
        };
        Some((provider, url))
    }

    /// Who answers, as the command field says it; `None` when nothing may
    /// be asked.
    pub fn ai_source(&self) -> Option<Source> {
        let (provider, _) = self.ai_provider()?;
        // Claude is Claude, wherever its address leads.
        Some(
            if self.config.ai.is_local() && provider != Provider::Claude {
                Source::Local
            } else {
                Source::Named(
                    match provider {
                        Provider::Claude => "Claude",
                        Provider::OpenAi => "OpenAI",
                        Provider::Ollama => "ollama",
                        Provider::LlamaCpp => "llama.cpp",
                    }
                    .into(),
                )
            },
        )
    }

    /// Asks `question` (the command field's question `id`) on a worker.
    pub fn ask_ai(&mut self, id: u64, question: String) {
        let system = answer::instructions(self.shell_lang()) + &proposals::instructions();
        self.ask_ai_for(Asker::Launcher, id, system, question);
    }

    /// Asks `question` for `asker`, the model told `system` first; the
    /// answer goes back to it by `id`.
    pub fn ask_ai_for(&mut self, asker: Asker, id: u64, system: String, question: String) {
        self.stop_asking();
        let lang = self.shell_lang();
        let Some(news) = self.ai.news.clone() else {
            return;
        };
        let Some((provider, url)) = self.ai_provider() else {
            self.answer_news(asker, id, AnswerNews::Failed(lang.ai_no_address().into()));
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        self.ai.asking = Some(Asking {
            asker,
            id,
            stop: stop.clone(),
            since: self.clock.now(),
        });
        let secrets = self.ai.secrets.clone();
        let model = match (&self.config.ai.model, &self.ai.model) {
            (named, _) if !named.is_empty() => named.clone(),
            (_, Some((p, u, picked))) if *p == provider && *u == url => picked.clone(),
            _ => String::new(),
        };
        let mut question = Question {
            provider,
            url,
            model,
            key: None,
            system,
            text: question,
        };
        tracing::info!(provider = provider.name(), "asking");
        spawn("ai", move || {
            // A local server may want a key too, if one was kept.
            if provider != Provider::Ollama
                && let Some(secrets) = &secrets
            {
                match secrets.lookup(provider.name()) {
                    Ok(key) => question.key = key,
                    Err(err) => tracing::warn!(err, "the keyring did not give the key"),
                }
            }
            if question.model.is_empty() {
                match ai::pick_model(&question) {
                    Ok(Some(model)) => {
                        let at = question.url.clone();
                        let _ = news.send(News::Model(provider, at, model.clone()));
                        question.model = model;
                    }
                    // The server answers with its own.
                    Ok(None) => {}
                    Err(err) => {
                        let _ = news.send(News::Failed(asker, id, err));
                        return;
                    }
                }
            }
            let sender = news.clone();
            let asked = ai::ask(&question, &stop, |piece| {
                let _ = sender.send(News::Piece(asker, id, piece.to_owned()));
            });
            let _ = news.send(match asked {
                Ok(()) => News::Done(asker, id),
                Err(err) => News::Failed(asker, id, err),
            });
        });
    }

    pub(crate) fn stop_asking(&mut self) {
        if let Some(asking) = self.ai.asking.take() {
            asking.stop.store(true, Ordering::Relaxed);
        }
    }

    fn answer_news(&mut self, asker: Asker, id: u64, news: AnswerNews) {
        match asker {
            Asker::Launcher => {
                if let Some(panel) = self.launcher.as_mut() {
                    panel.launcher.answer_news(id, news);
                }
            }
            Asker::Circle => self.circle_answer_news(id, news),
        }
        self.needs_redraw = true;
    }

    fn ai_news(&mut self, news: News) {
        let lang = self.shell_lang();
        match news {
            News::Piece(asker, id, text) => self.answer_news(asker, id, AnswerNews::Text(text)),
            News::Done(asker, id) => {
                let took = match self.ai.asking.take() {
                    Some(a) if a.asker == asker && a.id == id => {
                        self.clock.now().saturating_sub(a.since)
                    }
                    other => {
                        self.ai.asking = other;
                        Duration::ZERO
                    }
                };
                self.answer_news(asker, id, AnswerNews::Done(took));
                match asker {
                    Asker::Launcher => self.offer_launcher_proposals(id),
                    Asker::Circle => self.offer_circle_proposals(id),
                }
            }
            News::Failed(asker, id, err) => {
                tracing::warn!(%err, "no answer");
                if self
                    .ai
                    .asking
                    .as_ref()
                    .is_some_and(|a| a.asker == asker && a.id == id)
                {
                    self.ai.asking = None;
                }
                let url = self.ai_provider().map(|(_, u)| u).unwrap_or_default();
                self.answer_news(asker, id, AnswerNews::Failed(said(&err, &url, lang)));
            }
            News::Model(provider, url, model) => {
                tracing::info!(provider = provider.name(), model, "AI model picked");
                self.ai.model = Some((provider, url, model));
            }
            News::Stored(Ok(provider)) => {
                tracing::info!(provider, "AI key kept or forgotten");
                self.read_ai_keys();
            }
            News::Stored(Err(err)) => tracing::warn!(err, "the AI key was not kept"),
            News::Keys(Ok(keys)) => {
                if self.ai.keys.as_ref() != Some(&keys) {
                    self.ai.keys = Some(keys);
                    self.emit(|_| hocus_api::Event::Ai);
                }
            }
            News::Keys(Err(err)) => tracing::warn!(err, "the AI keys kept were not read"),
            News::Written(written) => self.config_written(written),
        }
    }

    /// Reads which providers have a key, on a worker: the settings say it.
    fn read_ai_keys(&self) {
        let (Some(secrets), Some(news)) = (self.ai.secrets.clone(), self.ai.news.clone()) else {
            return;
        };
        spawn("ai keys", move || {
            let _ = news.send(News::Keys(secrets.ids()));
        });
    }

    /// Which providers have a key kept, never the keys.
    pub fn ai_info(&self) -> hocus_api::AiInfo {
        hocus_api::AiInfo {
            keys: self.ai.keys.clone(),
        }
    }

    /// Keeps `secret` as `provider`'s key; an empty one forgets it.
    pub fn ai_secret(&mut self, provider: &str, secret: String) -> Result<(), String> {
        let Some(provider) = Provider::from_name(provider) else {
            return Err(format!("unknown provider {provider:?}"));
        };
        let (Some(secrets), Some(news)) = (self.ai.secrets.clone(), self.ai.news.clone()) else {
            return Err("no place to keep secrets".into());
        };
        spawn("ai secrets", move || {
            let name = provider.name();
            let done = if secret.is_empty() {
                // Forgets this one, keeps the others.
                let others: Vec<String> = ["ollama", "openai", "llamacpp", "claude"]
                    .iter()
                    .filter(|p| **p != name)
                    .map(|p| (*p).to_owned())
                    .collect();
                secrets.keep_only(&others)
            } else {
                let label = format!("Hocuspanda: {name} key");
                secrets.store(name, &label, &secret)
            };
            let _ = news.send(News::Stored(done.map(|()| name.to_owned())));
        });
        Ok(())
    }

    /// After each dispatch: a question no longer awaited is stopped.
    pub fn refresh_ai(&mut self) {
        let Some(asker) = self.ai.asking.as_ref().map(|a| a.asker) else {
            return;
        };
        let awaited = match asker {
            Asker::Launcher => self.launcher.as_ref().and_then(|p| p.launcher.awaited()),
            Asker::Circle => self.circle_awaited(),
        };
        if self
            .ai
            .asking
            .as_ref()
            .is_some_and(|a| Some(a.id) != awaited)
        {
            self.stop_asking();
        }
    }
}
