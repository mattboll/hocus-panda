//! The clipboard history and the emoji picker (#71). Every text copied, by
//! a Wayland or an X11 client, is read on a worker thread and kept, newest
//! first; a copy its application marks secret (password managers) is never
//! read nor kept. Only the session itself keeps it across sessions,
//! encrypted (`hocus_platform::sealed`).
//!
//! The history (Super+V) and the picker (Super+Period) show in the
//! launcher's card. What is chosen is typed into the focused application
//! through text-input, once it has the keyboard back; an application
//! without it gets the text on the clipboard instead.

use std::{io::Read, os::fd::OwnedFd, path::PathBuf, time::Duration};

use hocus_core::clipboard::{Entry, History, Limits};
use hocus_platform::sealed::Sealed;
use smithay::{
    reexports::calloop::{LoopHandle, channel},
    wayland::text_input::TextInputSeat,
};

use crate::{App, State};

/// How long the text waits for the application to take the keyboard back.
const INSERT_PATIENCE: Duration = Duration::from_secs(1);
/// What marks a copy secret (KeePassXC, KDE's password managers).
pub const SECRET_HINT: &str = "x-kde-passwordManagerHint";
const TEXT_MIMES: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

/// What workers send back.
#[derive(Debug)]
enum News {
    /// A copy read, by the order it was made in.
    Copied(u64, String),
    Loaded(Vec<Entry>),
    Recents(Vec<String>),
}

#[derive(Debug, Default)]
pub struct Clipboard {
    pub history: History,
    /// Emoji chosen lately, newest first.
    pub recents: Vec<String>,
    /// Text waiting for the application's text-input, and since when.
    pending: Option<(String, Duration)>,
    /// Copies are numbered as they are made; reads may end in any order.
    made: u64,
    /// The number of each copy read, to place one read late.
    read: Vec<(u64, String)>,
    /// Clipboard selections seen: a read asked for one is dropped when a
    /// newer one came meanwhile, since it would read that one.
    selections: u64,
    news: Option<channel::Sender<News>>,
    /// Where it is kept, for the session itself.
    store: Option<PathBuf>,
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, dir: Option<PathBuf>) {
    let (news, received) = channel::channel::<News>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(news) = event {
            app.timed("clipboard", |app| app.state.clipboard_news(news));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no clipboard history");
        return;
    }
    state.clipboard.news = Some(news.clone());
    let Some(dir) = dir else {
        return;
    };
    let store = dir.join("clipboard");
    state.clipboard.store = Some(store.clone());
    spawn("clipboard load", move || {
        match Sealed::new(&store).read() {
            Ok(Some(bytes)) => match serde_json::from_slice::<Vec<Entry>>(&bytes) {
                Ok(entries) => {
                    let _ = news.send(News::Loaded(entries));
                }
                Err(err) => tracing::warn!(%err, "the clipboard history does not read"),
            },
            Ok(None) => {}
            Err(err) => tracing::warn!(err, "the clipboard history does not open"),
        }
        let recents = std::fs::read_to_string(store.with_file_name("emoji-recents"))
            .map(|t| t.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        let _ = news.send(News::Recents(recents));
    });
}

impl State {
    fn limits(&self) -> Limits {
        Limits {
            entries: self.config.clipboard.entries as usize,
            ..Limits::default()
        }
    }

    fn clipboard_news(&mut self, news: News) {
        match news {
            News::Copied(n, text) => {
                let clip = &mut self.clipboard;
                clip.read.retain(|(_, t)| *t != text);
                let newer = clip.read.iter().filter(|(m, _)| *m > n).count();
                clip.read.push((n, text.clone()));
                if clip.read.len() > 256 {
                    clip.read.remove(0);
                }
                let limits = self.limits();
                if self.config.clipboard.history
                    && self.clipboard.history.insert(&text, newer, limits)
                {
                    self.history_changed();
                }
            }
            News::Loaded(entries) => {
                // What was copied before it loaded stays on top.
                let mut history = History::from_entries(entries, self.limits());
                for e in self.clipboard.history.entries().into_iter().rev() {
                    history.push(&e.text, self.limits());
                }
                self.clipboard.history = history;
                self.show_history();
            }
            News::Recents(recents) => {
                self.clipboard.recents = recents;
                if let Some(panel) = self.launcher.as_mut() {
                    panel.launcher.set_recents(self.clipboard.recents.clone());
                }
            }
        }
    }

    /// A new clipboard selection: its number.
    pub fn selection_changed(&mut self) -> u64 {
        self.clipboard.selections += 1;
        self.clipboard.selections
    }

    /// Whether selection `n` is still the clipboard's.
    pub fn selection_is(&self, n: u64) -> bool {
        self.clipboard.selections == n
    }

    /// A client offered the clipboard: where to ask for its text, read off
    /// the loop; `None` when it is marked secret or is not text.
    pub fn clipboard_reader(&mut self, mimes: &[String]) -> Option<(&'static str, OwnedFd)> {
        if !self.config.clipboard.history || mimes.iter().any(|m| m == SECRET_HINT) {
            return None;
        }
        let mime = TEXT_MIMES.iter().find(|t| mimes.iter().any(|m| m == *t))?;
        let news = self.clipboard.news.clone()?;
        let (reader, writer) = std::io::pipe().ok()?;
        self.clipboard.made += 1;
        let n = self.clipboard.made;
        let limit = Limits::default().entry_bytes as u64;
        spawn("clipboard read", move || {
            let mut text = String::new();
            // Past the limit, it is not kept: reading a byte more says so.
            if reader.take(limit + 1).read_to_string(&mut text).is_ok()
                && !text.is_empty()
                && text.len() as u64 <= limit
            {
                let _ = news.send(News::Copied(n, text));
            }
        });
        Some((mime, OwnedFd::from(writer)))
    }

    /// The shell copied text itself (a calculator result, a paste).
    pub fn shell_copied(&mut self, text: &str) {
        self.clipboard.made += 1;
        let n = self.clipboard.made;
        self.clipboard_news(News::Copied(n, text.to_owned()));
    }

    fn history_changed(&mut self) {
        self.show_history();
        if !self.config.clipboard.keep {
            return;
        }
        let Some(store) = self.clipboard.store.clone() else {
            return;
        };
        let entries: Vec<Entry> = self
            .clipboard
            .history
            .entries()
            .into_iter()
            .cloned()
            .collect();
        spawn("clipboard save", move || {
            let saved = serde_json::to_vec(&entries)
                .map_err(|e| e.to_string())
                .and_then(|bytes| Sealed::new(&store).write(&bytes));
            if let Err(err) = saved {
                tracing::warn!(err, "the clipboard history was not saved");
            }
        });
    }

    fn show_history(&mut self) {
        let shown: Vec<(String, bool)> = self
            .clipboard
            .history
            .entries()
            .into_iter()
            .map(|e| (e.text.clone(), e.pinned))
            .collect();
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.set_history(shown);
        }
        self.needs_redraw = true;
    }

    pub fn history_pin(&mut self, text: &str) {
        self.clipboard.history.toggle_pin(text);
        self.history_changed();
    }

    pub fn history_forget(&mut self, text: &str) {
        self.clipboard.history.remove(text);
        self.history_changed();
    }

    pub fn history_clear(&mut self) {
        self.clipboard.history.clear();
        self.history_changed();
    }

    /// Types `text` into the focused application once it has the keyboard
    /// back; with the clipboard too when `copy`.
    pub fn insert_text(&mut self, text: &str, copy: bool) {
        if copy {
            self.copy_text(text);
        }
        self.clipboard.pending = Some((text.to_owned(), self.clock.now()));
    }

    pub fn emoji_chosen(&mut self, glyph: &str) {
        let recents = &mut self.clipboard.recents;
        recents.retain(|g| g != glyph);
        recents.insert(0, glyph.to_owned());
        recents.truncate(24);
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.set_recents(self.clipboard.recents.clone());
        }
        if let Some(store) = self.clipboard.store.clone() {
            let text = self.clipboard.recents.join("\n");
            spawn("emoji recents", move || {
                if let Err(err) = std::fs::write(store.with_file_name("emoji-recents"), text) {
                    tracing::warn!(%err, "the recent emoji were not saved");
                }
            });
        }
        self.insert_text(glyph, false);
    }

    /// After each dispatch: text waiting for the application goes once
    /// its text-input is on again; after a second, it goes to the
    /// clipboard instead.
    pub fn refresh_clipboard(&mut self) {
        let Some((text, since)) = self.clipboard.pending.clone() else {
            return;
        };
        // Once the application has the keyboard back, its text-input has
        // been told (`enter`); what follows comes after on its connection.
        // No input method runs, so the done's serial is not its count:
        // the protocol has the client apply the text all the same.
        let text_input = self.seat.text_input();
        let mut typed = false;
        if text_input.focus().is_some() {
            text_input.with_focused_text_input(|ti, _| {
                ti.commit_string(Some(text.clone()));
                ti.done(0);
                typed = true;
            });
        }
        if typed {
            self.clipboard.pending = None;
            return;
        }
        if self.clock.now().saturating_sub(since) > INSERT_PATIENCE {
            self.clipboard.pending = None;
            self.copy_text(&text);
            let lang = self.shell_lang();
            self.shell_toast(lang.copied_paste(), "");
        }
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%err, "could not start a thread");
    }
}
