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
//!
//! A folder's pictures and day and night's (#894, [`changing`]) are
//! drawn as any picture, the one they show now: the next one of a folder
//! drawn ahead by the thread, kept aside until it shows; the screen's
//! previous background fades out under it ([`Fade`]), at once with
//! reduced motion or on the CPU.

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

mod changing;
mod day_night;
mod round;

pub use changing::Changing;

/// What a screen shows, at its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub look: Look,
    pub size: (i32, i32),
    /// The screen's scale in thousandths: a moving wallpaper's still
    /// picture draws its cells at their logical size.
    pub scale: u32,
}

impl Key {
    fn scale(&self) -> f64 {
        f64::from(self.scale.max(1)) / 1000.0
    }
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
    /// The next one, drawn ahead (#894): kept aside until it shows.
    ahead: bool,
}

/// A background the thread drew.
#[derive(Debug)]
pub struct Drawn {
    output: String,
    key: Key,
    rendered: Rendered,
    ahead: bool,
}

/// A screen's previous background, fading out under its new one (#894).
#[derive(Debug)]
pub struct Fade {
    pub from: Shown,
    /// When it began, on the motions' clock, and how long it lasts.
    pub start: std::time::Duration,
    pub length: std::time::Duration,
}

impl Fade {
    /// The new background's opacity over it at `now`.
    pub fn alpha(&self, now: std::time::Duration) -> f32 {
        let t = (now.saturating_sub(self.start).as_secs_f32()
            / self.length.as_secs_f32().max(0.001))
        .clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    pub fn done(&self, now: std::time::Duration) -> bool {
        now.saturating_sub(self.start) >= self.length
    }
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
    /// The next backgrounds, drawn ahead (#894), by screen.
    ahead: HashMap<String, Shown>,
    ahead_asked: HashMap<String, Key>,
    /// Screens whose next background fades in, over this long (#894),
    /// and when that was asked.
    pub fade_next: HashMap<String, (std::time::Duration, std::time::Duration)>,
    /// Screens fading from their previous background (#894).
    pub fading: HashMap<String, Fade>,
    /// Screens that took their next background drawn ahead: the one
    /// after it is to be drawn ahead in its turn.
    pub took_ahead: Vec<String>,
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
        Ok(_) => state.graphics.backgrounds.get_mut().jobs = Some(jobs),
        Err(err) => tracing::warn!(%err, "no wallpaper thread: pictures are not drawn"),
    }
    // A folder's pictures, day and night's (#894).
    changing::start(handle, state);
}

/// The thread: each screen's last job only, drawn, sent back.
fn draw(taken: &Receiver<Job>, sender: &channel::Sender<Drawn>) {
    while let Ok(first) = taken.recv() {
        let mut jobs = vec![first];
        jobs.extend(taken.try_iter());
        let mut last: Vec<Job> = Vec::new();
        for job in jobs {
            last.retain(|j| j.output != job.output || j.ahead != job.ahead);
            last.push(job);
        }
        // What shows now before what shows next.
        last.sort_by_key(|j| j.ahead);
        for Job { output, key, ahead } in last {
            let rendered = hocus_link::wallpaper::render_at(&key.look, key.size, key.scale());
            if sender
                .send(Drawn {
                    output,
                    key,
                    rendered,
                    ahead,
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
        self.ahead.remove(output);
        self.ahead_asked.remove(output);
        self.fading.remove(output);
        self.fade_next.remove(output);
    }

    /// `shown` shows on `output` from `now`: over its previous one fading
    /// out when its change asked for a fade.
    fn install(&mut self, output: &str, shown: Shown, now: std::time::Duration) {
        let fade = self.fade_next.remove(output).map(|(length, _)| length);
        let before = self.shown.insert(output.to_owned(), shown);
        match (fade, before) {
            (Some(length), Some(from)) => {
                self.fading.insert(
                    output.to_owned(),
                    Fade {
                        from,
                        start: now,
                        length,
                    },
                );
            }
            _ => {
                self.fading.remove(output);
            }
        }
    }

    /// `output`'s next background, `key`, drawn ahead (#894) unless it is
    /// or is asked already.
    pub fn want_ahead(&mut self, output: &str, key: &Key) {
        if self.ahead.get(output).is_some_and(|s| &s.key == key)
            || self.ahead_asked.get(output) == Some(key)
            || self.shown.get(output).is_some_and(|s| &s.key == key)
        {
            return;
        }
        let asked = self.jobs.as_ref().is_some_and(|jobs| {
            jobs.send(Job {
                output: output.to_owned(),
                key: key.clone(),
                ahead: true,
            })
            .is_ok()
        });
        if asked {
            self.ahead_asked.insert(output.to_owned(), key.clone());
        }
    }

    /// `output` should show `key` from `now`: asked of the thread unless
    /// it shows it or waits for it already, or it was drawn ahead. A
    /// screen that shows nothing yet gets a built-in at once.
    pub fn want(&mut self, output: &str, key: &Key, now: std::time::Duration) {
        if self.shown.get(output).is_some_and(|s| &s.key == key)
            || self.asked.get(output) == Some(key)
        {
            return;
        }
        if self.ahead.get(output).is_some_and(|s| &s.key == key)
            && let Some(ready) = self.ahead.remove(output)
        {
            self.draws += 1;
            self.asked.remove(output);
            self.install(output, ready, now);
            self.took_ahead.push(output.to_owned());
            return;
        }
        let first = !self.shown.contains_key(output);
        // A still picture of a moving one is drawn at the screen's pixels:
        // on the thread, as a picture is.
        let builtin = matches!(key.look.picture, Picture::Builtin(_) | Picture::Plain);
        if (first && builtin) || self.jobs.is_none() {
            let look = if builtin {
                key.look.clone()
            } else {
                Look::default()
            };
            let rendered = hocus_link::wallpaper::render(&look, key.size);
            self.draws += 1;
            self.asked.remove(output);
            let shown = Shown {
                key: key.clone(),
                buffer: buffer_of(&rendered),
                picture: (rendered.width, rendered.height),
            };
            self.install(output, shown, now);
            return;
        }
        let asked = self.jobs.as_ref().is_some_and(|jobs| {
            jobs.send(Job {
                output: output.to_owned(),
                key: key.clone(),
                ahead: false,
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
    /// What screen `output` shows, as the file says now: a folder's or
    /// day and night's picture of the moment (#894); while a folder is
    /// not listed yet, what it shows already.
    pub fn wallpaper_look(&self, output: &str) -> Look {
        let wallpaper = &self.config.wallpaper;
        let picture = self.wallpaper_now(Some(output)).unwrap_or_else(|| {
            self.graphics
                .backgrounds
                .borrow()
                .shown
                .get(output)
                .map_or_else(Picture::default, |s| s.key.look.picture.clone())
        });
        Look {
            picture: self.expanded(picture),
            fit: wallpaper.fit,
            color: wallpaper.color,
        }
    }

    /// A picture's `~/` read here: the thread draws absolute paths.
    fn expanded(&self, picture: Picture) -> Picture {
        match picture {
            Picture::File(text) => match (text.strip_prefix("~/"), &self.config.home) {
                (Some(rest), Some(home)) => Picture::File(home.join(rest).display().to_string()),
                _ => Picture::File(text),
            },
            picture => picture,
        }
    }

    /// The wallpapers as the shell is told them: the pictures they show
    /// now, a folder's or day and night's among them (#894).
    pub fn wallpapers(&self) -> Wallpapers {
        let wallpaper = &self.config.wallpaper;
        let now = |output: Option<&str>| self.wallpaper_now(output).unwrap_or(Picture::Folder);
        let outputs = if wallpaper.per_screen {
            self.config
                .outputs
                .iter()
                .filter(|(_, o)| o.wallpaper.is_some())
                .map(|(name, _)| (name.clone(), now(Some(name))))
                .collect()
        } else {
            Vec::new()
        };
        let lock_picture = match &wallpaper.lock_picture {
            p if p.changes() => now(None),
            p => p.clone(),
        };
        Wallpapers {
            lock: wallpaper.lock,
            lock_picture,
            picture: now(None),
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
            .theme_choice(self.graphics.backgrounds.borrow().accent)
    }

    /// The file's `[wallpaper]` and screens' wallpapers are now `new`'s:
    /// whether they changed. Each screen asks for its background as it is
    /// drawn next.
    pub fn wallpaper_changed(&mut self, new: &crate::Config) -> bool {
        let wallpapers =
            |outputs: &std::collections::BTreeMap<String, crate::config::OutputSettings>| {
                outputs
                    .iter()
                    .map(|(name, o)| (name.clone(), o.wallpaper_part()))
                    .filter(|(_, part)| !part.is_empty())
                    .collect::<Vec<_>>()
            };
        if new.wallpaper == self.config.wallpaper
            && wallpapers(&new.outputs) == wallpapers(&self.config.outputs)
        {
            return false;
        }
        self.config.wallpaper = new.wallpaper.clone();
        for settings in self.config.outputs.values_mut() {
            settings.set_wallpaper_part(crate::config::WallpaperPart::default());
        }
        for (name, part) in wallpapers(&new.outputs) {
            self.config
                .outputs
                .entry(name)
                .or_default()
                .set_wallpaper_part(part);
        }
        // The folders followed, day or night looked at (#894).
        self.wallpaper_follow();
        let wallpapers = self.wallpapers();
        tracing::info!(wallpapers = %wallpapers, "the wallpaper changed");
        self.wallpaper_told(wallpapers);
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
            ahead,
        } = drawn;
        if ahead {
            let mut backgrounds = self.graphics.backgrounds.borrow_mut();
            if backgrounds.ahead_asked.get(&output) != Some(&key) {
                return;
            }
            backgrounds.ahead_asked.remove(&output);
            backgrounds.decodes += 1;
            tracing::debug!(output, picture = %key.look.picture, "the next wallpaper is ready");
            let ready = Shown {
                buffer: buffer_of(&rendered),
                picture: (rendered.width, rendered.height),
                key,
            };
            backgrounds.ahead.insert(output, ready);
            return;
        }
        let now = self.now();
        let accent = {
            let mut backgrounds = self.graphics.backgrounds.borrow_mut();
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
            let shown = Shown {
                key,
                buffer: buffer_of(&rendered),
                picture: (rendered.width, rendered.height),
            };
            backgrounds.install(&output, shown, now);
            rendered.accent
        };
        self.redraw = true;
        // A folder's next picture drawn ahead (#894).
        self.wallpaper_prepare(&output);
        if self.panel_primary().as_deref() == Some(output.as_str()) {
            self.wallpaper_accent(accent);
        }
    }

    /// The primary screen's wallpaper gives `accent`: told while the
    /// accent is the wallpaper's.
    fn wallpaper_accent(&mut self, accent: AccentColours) {
        let before = self.theme_choice();
        self.graphics.backgrounds.get_mut().accent = accent;
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
            Picture::Moving(moving) => moving.accent(),
            Picture::Plain => AccentColours::lifted(look.color),
            Picture::File(_) | Picture::Folder | Picture::DayNight => return,
        };
        self.wallpaper_accent(accent);
    }
}
