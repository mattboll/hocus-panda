//! The desktop's wallpaper (6.3, issue #403), as the compositor draws it
//! under everything: `[wallpaper]` and `[outputs.NAME] wallpaper`, followed
//! live through the one watcher of the file (`reload.rs`).
//!
//! Each screen's background is drawn by [`hocus_link::wallpaper::render`]
//! on a thread of its own, never on the loop: a picture is decoded there,
//! scaled and cropped to the screen's pixels for its fit, and sent back as
//! one owned buffer. The loop only swaps the screen's buffer; until it
//! arrives the previous background stays (no flash of black). A screen's
//! first frame draws a built-in at once, as the compositor always did (a
//! gradient at a quarter of its pixels, not a file). A change of a
//! screen's size or scale asks again for that screen. At rest nothing is
//! asked and nothing is drawn: a background damages nothing.
//!
//! The primary screen's background gives `[appearance] accent =
//! "wallpaper"` its colour; the shell is told each screen's wallpaper and
//! the lock's, which it draws under the lock itself.

use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
};

use hocus_link::{
    ToShell,
    theme::AccentColours,
    wallpaper::{Look, Picture, Rendered, Wallpapers},
};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    reexports::calloop::{
        LoopHandle,
        channel::{self, Event},
    },
    utils::Transform,
};

use crate::{App, State};

/// What a screen shows, at its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub look: Look,
    pub size: (i32, i32),
}

/// A background drawn for a screen.
#[derive(Debug)]
pub struct Shown {
    pub key: Key,
    pub buffer: MemoryRenderBuffer,
    /// The buffer's size in pixels, drawn stretched over the screen.
    pub picture: (i32, i32),
}

struct Job {
    output: String,
    key: Key,
}

/// A background the thread drew.
#[derive(Debug)]
pub struct Drawn {
    output: String,
    key: Key,
    rendered: Rendered,
}

/// The screens' backgrounds, and the thread that draws them.
#[derive(Debug, Default)]
pub struct Backgrounds {
    jobs: Option<Sender<Job>>,
    /// By screen name.
    pub shown: HashMap<String, Shown>,
    /// What each screen waits for from the thread.
    asked: HashMap<String, Key>,
    /// The primary screen's wallpaper's accent.
    pub accent: AccentColours,
    /// Pictures of one's own drawn so far (one per change, none at rest).
    pub decodes: u64,
    /// Backgrounds drawn or asked of the thread so far (none at rest).
    pub draws: u64,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job").field("output", &self.output).finish()
    }
}

/// Starts the thread that draws the backgrounds; without it, built-ins
/// are drawn on the loop and pictures are not (said in the log).
pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (sender, drawn) = channel::channel::<Drawn>();
    if let Err(err) = handle.insert_source(drawn, |event, (), app| {
        if let Event::Msg(drawn) = event {
            app.state.wallpaper_drawn(drawn);
        }
    }) {
        tracing::warn!(error = %err.error, "no wallpaper thread: pictures are not drawn");
        return;
    }
    let (jobs, taken) = mpsc::channel::<Job>();
    let started = std::thread::Builder::new()
        .name("wallpaper".to_owned())
        .spawn(move || draw(&taken, &sender));
    match started {
        Ok(_) => state.backgrounds.get_mut().jobs = Some(jobs),
        Err(err) => tracing::warn!(%err, "no wallpaper thread: pictures are not drawn"),
    }
}

/// The thread: each screen's last job only, drawn, sent back.
fn draw(taken: &Receiver<Job>, sender: &channel::Sender<Drawn>) {
    while let Ok(first) = taken.recv() {
        let mut jobs = vec![first];
        jobs.extend(taken.try_iter());
        let mut last: Vec<Job> = Vec::new();
        for job in jobs {
            last.retain(|j| j.output != job.output);
            last.push(job);
        }
        for Job { output, key } in last {
            let rendered = hocus_link::wallpaper::render(&key.look, key.size);
            if sender
                .send(Drawn {
                    output,
                    key,
                    rendered,
                })
                .is_err()
            {
                return;
            }
        }
    }
}

fn buffer_of(rendered: &Rendered) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        &rendered.pixels,
        Fourcc::Abgr8888,
        (rendered.width, rendered.height),
        1,
        Transform::Normal,
        None,
    )
}

impl Backgrounds {
    /// `output` is unplugged: its buffer goes (it is drawn again when it
    /// comes back).
    pub fn forget(&mut self, output: &str) {
        self.shown.remove(output);
        self.asked.remove(output);
    }

    /// `output` should show `key`: asked of the thread unless it shows it
    /// or waits for it already. A screen that shows nothing yet gets a
    /// built-in at once.
    pub fn want(&mut self, output: &str, key: &Key) {
        if self.shown.get(output).is_some_and(|s| &s.key == key)
            || self.asked.get(output) == Some(key)
        {
            return;
        }
        let first = !self.shown.contains_key(output);
        let builtin = !matches!(key.look.picture, Picture::File(_));
        if (first && builtin) || self.jobs.is_none() {
            let look = if builtin {
                key.look.clone()
            } else {
                Look::default()
            };
            let rendered = hocus_link::wallpaper::render(&look, key.size);
            self.draws += 1;
            self.asked.remove(output);
            self.shown.insert(
                output.to_owned(),
                Shown {
                    key: key.clone(),
                    buffer: buffer_of(&rendered),
                    picture: (rendered.width, rendered.height),
                },
            );
            return;
        }
        let asked = self.jobs.as_ref().is_some_and(|jobs| {
            jobs.send(Job {
                output: output.to_owned(),
                key: key.clone(),
            })
            .is_ok()
        });
        if asked {
            self.draws += 1;
            self.asked.insert(output.to_owned(), key.clone());
        }
    }
}

impl State {
    /// What screen `output` shows, as the file says now.
    pub fn wallpaper_look(&self, output: &str) -> Look {
        let wallpaper = &self.config.wallpaper;
        let own = wallpaper
            .per_screen
            .then(|| self.config.outputs.get(output)?.wallpaper.clone())
            .flatten();
        // A picture's `~/` read here: the thread draws absolute paths.
        let picture = match own.unwrap_or_else(|| wallpaper.picture.clone()) {
            Picture::File(text) => match (text.strip_prefix("~/"), &self.config.home) {
                (Some(rest), Some(home)) => Picture::File(home.join(rest).display().to_string()),
                _ => Picture::File(text),
            },
            picture => picture,
        };
        Look {
            picture,
            fit: wallpaper.fit,
            color: wallpaper.color,
        }
    }

    /// The wallpapers as the shell is told them.
    pub fn wallpapers(&self) -> Wallpapers {
        let wallpaper = &self.config.wallpaper;
        let outputs = if wallpaper.per_screen {
            self.config
                .outputs
                .iter()
                .filter_map(|(name, o)| Some((name.clone(), o.wallpaper.clone()?)))
                .collect()
        } else {
            Vec::new()
        };
        Wallpapers {
            lock: wallpaper.lock,
            lock_picture: wallpaper.lock_picture.clone(),
            picture: wallpaper.picture.clone(),
            fit: wallpaper.fit,
            color: wallpaper.color,
            outputs,
        }
    }

    /// The theme the shell and the applications are told: the file's,
    /// the wallpaper's accent the primary screen's.
    pub fn theme_choice(&self) -> hocus_link::theme::Choice {
        self.config
            .appearance
            .theme_choice(self.backgrounds.borrow().accent)
    }

    /// The file's `[wallpaper]` and screens' wallpapers are now `new`'s:
    /// whether they changed. Each screen asks for its background as it is
    /// drawn next.
    pub fn wallpaper_changed(&mut self, new: &crate::Config) -> bool {
        let wallpapers =
            |outputs: &std::collections::BTreeMap<String, crate::config::OutputSettings>| {
                outputs
                    .iter()
                    .filter_map(|(name, o)| Some((name.clone(), o.wallpaper.clone()?)))
                    .collect::<Vec<_>>()
            };
        if new.wallpaper == self.config.wallpaper
            && wallpapers(&new.outputs) == wallpapers(&self.config.outputs)
        {
            return false;
        }
        self.config.wallpaper = new.wallpaper.clone();
        for settings in self.config.outputs.values_mut() {
            settings.wallpaper = None;
        }
        for (name, settings) in &new.outputs {
            if let Some(picture) = &settings.wallpaper {
                self.config
                    .outputs
                    .entry(name.clone())
                    .or_default()
                    .wallpaper = Some(picture.clone());
            }
        }
        let wallpapers = self.wallpapers();
        tracing::info!(wallpapers = %wallpapers, "the wallpaper changed");
        self.tell_shell(ToShell::Wallpaper(wallpapers));
        self.wallpaper_accent_now();
        self.redraw = true;
        true
    }

    /// The thread drew `drawn`: its screen shows it, unless it waits for
    /// another by now.
    pub fn wallpaper_drawn(&mut self, drawn: Drawn) {
        let Drawn {
            output,
            key,
            rendered,
        } = drawn;
        let accent = {
            let mut backgrounds = self.backgrounds.borrow_mut();
            if backgrounds.asked.get(&output) != Some(&key) {
                return;
            }
            backgrounds.asked.remove(&output);
            if matches!(key.look.picture, Picture::File(_)) {
                backgrounds.decodes += 1;
            }
            match &rendered.failed {
                Some(why) => tracing::warn!(
                    output,
                    picture = %key.look.picture,
                    why,
                    "the wallpaper does not read: Nuit is drawn"
                ),
                None => tracing::info!(
                    output,
                    picture = %key.look.picture,
                    width = key.size.0,
                    height = key.size.1,
                    "the wallpaper is drawn"
                ),
            }
            backgrounds.shown.insert(
                output.clone(),
                Shown {
                    key,
                    buffer: buffer_of(&rendered),
                    picture: (rendered.width, rendered.height),
                },
            );
            rendered.accent
        };
        self.redraw = true;
        if self.panel_primary().as_deref() == Some(output.as_str()) {
            self.wallpaper_accent(accent);
        }
    }

    /// The primary screen's wallpaper gives `accent`: told while the
    /// accent is the wallpaper's.
    fn wallpaper_accent(&mut self, accent: AccentColours) {
        let before = self.theme_choice();
        self.backgrounds.get_mut().accent = accent;
        let now = self.theme_choice();
        if now != before {
            tracing::info!(accent = %now, "the wallpaper's accent changed");
            self.tell_shell(ToShell::Theme(now));
            self.share.portal_appearance(self.portal_appearance());
        }
    }

    /// Built-ins drawn at once give their accent at once too (the thread
    /// only says pictures'): the primary screen's, as it shows now.
    pub fn wallpaper_accent_now(&mut self) {
        let Some(primary) = self.panel_primary() else {
            return;
        };
        let look = self.wallpaper_look(&primary);
        let accent = match look.picture {
            Picture::Builtin(builtin) => builtin.accent(),
            Picture::Plain => AccentColours::lifted(look.color),
            Picture::File(_) => return,
        };
        self.wallpaper_accent(accent);
    }
}
