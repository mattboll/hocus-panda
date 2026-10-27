//! Document search in the command field (#45): the service runs on
//! threads of its own (hocus-platform's `search`); the loop only tells it
//! the configuration, the power source and what is typed, and hands the
//! documents found to the launcher.
//!
//! Nested or in tests, the user's own folders are never searched (the
//! developer's documents are not ours): only the folders the
//! configuration names, and the screenshots folder when it names one.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use hocus_core::config::{Meaning, PowerSource};
use hocus_platform::{
    ai::Embedder,
    search::{Folder, News, Phase, Pictures, Root, Search, Settings, Status},
};
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

/// In the session, the first look at the folders waits this long: the
/// session starts first.
const SESSION_DELAY: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Default)]
pub struct SearchState {
    service: Option<Search>,
    /// Where the index is kept.
    store: Option<PathBuf>,
    /// What the service was last told.
    sent: Option<Option<Settings>>,
    battery: Option<bool>,
    /// The configuration generation the settings follow.
    from: u64,
    pub status: Status,
    /// When the last look at the folders ended, on the compositor's clock.
    last_look: Option<std::time::Duration>,
    /// The text in pictures, read once for the indexer and the
    /// screenshots' toasts (#302).
    pictures: Arc<Pictures>,
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, dir: Option<&Path>) {
    let (sender, receiver) = channel::channel::<News>();
    let inserted = handle.insert_source(receiver, |event, _, app| {
        if let channel::Event::Msg(news) = event {
            app.timed("search", |app| app.state.search_news(news));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "documents will not be searched");
        return;
    }
    let sender = std::sync::Mutex::new(sender);
    let service = Search::start(
        state.ai_secrets(),
        move |news| {
            if let Ok(sender) = sender.lock() {
                let _ = sender.send(news);
            }
        },
        None,
        state.search.pictures.clone(),
    );
    match service {
        Ok(service) => state.search.service = Some(service),
        Err(err) => tracing::warn!(%err, "documents will not be searched"),
    }
    state.search.store = dir.map(|d| d.join("search").join("index"));
}

/// The home, from the environment.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| h.is_absolute())
}

/// A configured folder: `~/` is the home; relative ones are left out.
fn folder(configured: &str, home: Option<&Path>) -> Option<PathBuf> {
    match configured.strip_prefix("~/") {
        Some(rest) => Some(home?.join(rest)),
        None if configured == "~" => home.map(Path::to_owned),
        None => Some(PathBuf::from(configured)).filter(|p| p.is_absolute()),
    }
}

/// A document's folder, by its name.
fn folder_name(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl State {
    /// What the configuration asks of the indexer; `None`: off.
    fn search_settings(&self) -> Option<Settings> {
        let config = &self.config.search;
        if !config.documents {
            return None;
        }
        let home = home();
        let mut roots: Vec<Root> = config
            .folders
            .iter()
            .filter_map(|f| folder(f, home.as_deref()))
            .map(|path| Root {
                folder: Folder::Path(path),
                pictures: false,
            })
            .collect();
        if config.folders.is_empty() && self.is_session {
            for (key, fallback) in [
                ("DOCUMENTS", "Documents"),
                ("DESKTOP", "Desktop"),
                ("DOWNLOAD", "Downloads"),
            ] {
                roots.push(Root {
                    folder: Folder::User {
                        key,
                        fallback,
                        sub: "",
                    },
                    pictures: false,
                });
            }
        }
        let shots = &self.config.screenshots.folder;
        if config.screenshots {
            let screenshots = if !shots.is_empty() {
                folder(shots, home.as_deref()).map(Folder::Path)
            } else if self.is_session {
                Some(Folder::User {
                    key: "PICTURES",
                    fallback: "Pictures",
                    sub: "Screenshots",
                })
            } else {
                None
            };
            roots.extend(screenshots.map(|folder| Root {
                folder,
                pictures: true,
            }));
        }
        Some(Settings {
            roots,
            exclude: config.exclude.clone(),
            ocr: self.config.screenshots.ocr.clone(),
            embedder: self.search_embedder(),
            store: self.search.store.clone(),
            delay: if self.is_session {
                SESSION_DELAY
            } else {
                std::time::Duration::ZERO
            },
            home,
        })
    }

    /// Who makes vectors of the documents: the AI's provider when it is
    /// on, can, and is on this machine (or is allowed elsewhere).
    fn search_embedder(&self) -> Option<Embedder> {
        let meaning = self.config.search.meaning;
        if meaning == Meaning::Off {
            return None;
        }
        let (provider, url) = self.ai_provider()?;
        if !provider.can_embed() || (meaning == Meaning::Local && !self.config.ai.is_local()) {
            return None;
        }
        Some(Embedder {
            provider,
            url,
            model: self.config.search.embedding_model.clone(),
            key: None,
        })
    }

    /// After each dispatch: the configuration and the power source told
    /// to the indexer when they changed, and what the launcher wants
    /// looked for.
    pub fn refresh_search(&mut self) {
        if self.search.service.is_none() {
            return;
        }
        // The power first: on battery, nothing is read even once.
        let battery = self.power_source() == PowerSource::Battery;
        if self.search.battery != Some(battery) {
            self.search.battery = Some(battery);
            if let Some(service) = &self.search.service {
                service.set_battery(battery);
            }
        }
        if self.search.from != self.config_generation {
            self.search.from = self.config_generation;
            let settings = self.search_settings();
            if self.search.sent.as_ref() != Some(&settings) {
                if let Some(service) = &self.search.service {
                    match settings.clone() {
                        Some(settings) => service.configure(settings),
                        // Off, even at start: an index kept before goes.
                        None => service.turn_off(self.search.store.clone()),
                    }
                }
                self.search.sent = Some(settings);
            }
        }
        let wanted = self
            .launcher
            .as_mut()
            .and_then(|p| p.launcher.take_documents_query());
        if let (Some((id, query)), Some(service)) = (wanted, &self.search.service) {
            service.query(id, query);
        }
    }

    fn search_news(&mut self, news: News) {
        match news {
            News::Status(status) => {
                if status.phase == Phase::Ready && self.search.status.phase != Phase::Ready {
                    self.search.last_look = Some(self.clock.now());
                }
                let moved = status.phase != self.search.status.phase;
                if moved {
                    tracing::info!(
                        phase = status.phase.as_str(),
                        documents = status.documents,
                        "document search"
                    );
                }
                self.search.status = status;
                if moved {
                    let state = self.search.status.phase.as_str().to_owned();
                    self.emit(|_| hocus_api::Event::Search { state });
                }
            }
            News::Found { id, hits } => {
                let documents = hits
                    .into_iter()
                    .map(|hit| hocus_shell::launcher::Document {
                        name: hit
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        folder: folder_name(&hit.path),
                        path: hit.path.to_string_lossy().into_owned(),
                        snippet: hit.snippet,
                    })
                    .collect();
                if let Some(panel) = self.launcher.as_mut() {
                    panel.launcher.set_documents(id, documents);
                }
                self.needs_redraw = true;
            }
        }
    }

    /// The folders looked at again now (`search_again`), as the settings'
    /// button asks.
    pub fn search_again(&self) {
        if let Some(service) = &self.search.service {
            service.look_again();
        }
    }

    /// Where the text in pictures is read, for the indexer and the others
    /// reading the same pictures.
    pub(crate) fn pictures(&self) -> Arc<Pictures> {
        self.search.pictures.clone()
    }

    /// Documents can be searched.
    pub fn search_running(&self) -> bool {
        self.search.service.is_some()
    }

    /// The indexer's state, for the control API.
    pub fn search_info(&self) -> hocus_api::SearchInfo {
        let s = &self.search.status;
        hocus_api::SearchInfo {
            state: s.phase.as_str().into(),
            documents: s.documents as u64,
            embedded: s.embedded as u64,
            never_read: s.denied as u64,
            unreadable: s.unreadable as u64,
            meaning: s.meaning,
            last_look_seconds: self
                .search
                .last_look
                .map(|at| self.clock.now().saturating_sub(at).as_secs()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_folders_read_from_the_home() {
        let home = Path::new("/home/alex");
        assert_eq!(
            folder("~/Documents", Some(home)),
            Some(PathBuf::from("/home/alex/Documents"))
        );
        assert_eq!(
            folder("/srv/docs", Some(home)),
            Some(PathBuf::from("/srv/docs"))
        );
        assert_eq!(folder("docs", Some(home)), None, "relative: left out");
        assert_eq!(folder("~/Documents", None), None);
        assert_eq!(
            folder_name(Path::new("/home/alex/Documents/Work/plan.odt")),
            "Work"
        );
    }

    #[test]
    fn the_never_read_list_is_the_platform_s() {
        // The configuration has no say: nothing here takes it.
        assert!(hocus_platform::search::deny::denied(
            Path::new("/home/alex/Documents/.ssh/id_rsa"),
            None
        ));
    }
}
