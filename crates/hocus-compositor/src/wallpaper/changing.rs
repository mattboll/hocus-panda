//! The wallpapers that change by themselves (#894): `"folder"`, a
//! folder's pictures in turn, and `"day-night"`, a picture by day and
//! another by night, every screen's or each screen's own.
//!
//! A folder is listed on a thread of its own (its subfolders too), which
//! also reads and writes what is kept of the rounds between sessions
//! (`$XDG_STATE_HOME/hocuspanda/wallpaper.json`) and adds the folders to
//! an inotify instance another thread reads: a change in a folder lists
//! it again, a moment after the last ([`SETTLE`]), and the round takes
//! the pictures added and lets go of those removed. Nothing of it on the
//! loop but the rounds themselves.
//!
//! One timer on the loop wakes it at the next change: a folder's interval
//! on the monotonic clock, day and night's switch on the wall clock
//! (looked at again every ten minutes at least, should the clock jump).
//! A change fades the screens it changes ([`FOLDER_FADE`],
//! [`DAY_NIGHT_FADE`]), at once with reduced motion or on the CPU; the
//! shell (for the lock) and the Settings are told the pictures shown.

use std::{
    collections::HashMap,
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
};

use hocus_link::{
    ToShell,
    wallpaper::{Look, Picture, Wallpapers},
};
use smithay::reexports::calloop::{
    LoopHandle, RegistrationToken,
    channel::{self, Event},
    timer::{TimeoutAction, Timer},
};

use super::{
    Key,
    day_night::{Phase, Switches, phase_at},
    round::{Kept, Round, first_change},
};
use crate::{App, State};

/// A folder's picture fades into the next over this long.
pub const FOLDER_FADE: Duration = Duration::from_secs(2);
/// Day's picture into night's, and back.
pub const DAY_NIGHT_FADE: Duration = Duration::from_secs(30);
/// A folder that changed is listed again this long after its last
/// change: many pictures copied in are listed once.
const SETTLE: Duration = Duration::from_millis(400);
/// Day and night are looked at again this often at least: the wall
/// clock may jump (a suspend, another time zone).
const LOOK_AGAIN: Duration = Duration::from_secs(10 * 60);
/// What is kept of the rounds, in the state's folder.
const KEPT: &str = "wallpaper.json";
/// So many rounds kept at most, the last changed.
const KEPT_MOST: usize = 16;

/// What the loop knows of the wallpapers that change.
#[derive(Debug, Default)]
pub struct Changing {
    /// The folders followed, by path.
    folders: HashMap<PathBuf, Folder>,
    jobs: Option<mpsc::Sender<Job>>,
    timer: Option<RegistrationToken>,
    settle: Option<RegistrationToken>,
    /// Day or night now, while a screen shows them.
    phase: Option<Phase>,
    /// When day and night next switch.
    switch_at: Option<jiff::Timestamp>,
    /// Pictures changed by themselves or asked so far (tests).
    pub changes: u64,
    /// What the shell was last told.
    told: Option<Wallpapers>,
    /// What the Settings were last told: each screen's picture now.
    told_settings: Option<String>,
}

#[derive(Debug)]
struct Folder {
    /// None until it is listed.
    round: Option<Round>,
    /// When it changes next; none: only when asked ("session").
    due: Option<Instant>,
    /// When it last changed, seconds since 1970 (what is kept).
    changed: u64,
}

enum Job {
    List(PathBuf),
    Keep(Vec<(PathBuf, Kept)>),
}

enum Heard {
    Listed {
        folder: PathBuf,
        pictures: Vec<PathBuf>,
        kept: Option<Kept>,
    },
    /// Something changed in a folder watched.
    Changed,
}

/// The wall clock, in seconds since 1970.
fn wall_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A seed for a round's shuffle.
fn seed() -> u64 {
    let mut bytes = [0u8; 8];
    match rustix::rand::getrandom(&mut bytes, rustix::rand::GetRandomFlags::NONBLOCK) {
        Ok(8) => u64::from_ne_bytes(bytes),
        _ => SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64),
    }
}

/// Starts the folders' thread and their watch; then follows the file.
pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (sender, heard) = channel::channel::<Heard>();
    if let Err(err) = handle.insert_source(heard, |event, (), app| {
        if let Event::Msg(heard) = event {
            app.state.wallpaper_heard(heard);
        }
    }) {
        tracing::warn!(error = %err.error, "no thread for the wallpapers' folders");
        return;
    }
    let notify = watch(sender.clone());
    let kept = state.config.state_dir.as_ref().map(|d| d.join(KEPT));
    let (jobs, taken) = mpsc::channel::<Job>();
    let started = std::thread::Builder::new()
        .name("wallpaper: folders".to_owned())
        .spawn(move || folders(&taken, &sender, kept, notify));
    match started {
        Ok(_) => state.changing.jobs = Some(jobs),
        Err(err) => tracing::warn!(%err, "no thread for the wallpapers' folders"),
    }
    state.wallpaper_follow();
}

/// The folders' changes, watched by a thread of its own blocked in
/// inotify's read: what it returns adds the folders to the watch.
fn watch(sender: channel::Sender<Heard>) -> Option<OwnedFd> {
    use rustix::{
        fs::inotify::{self, CreateFlags},
        io::Errno,
    };
    let fd = match inotify::init(CreateFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(err) => {
            tracing::warn!(%err, "the wallpapers' folders are not watched");
            return None;
        }
    };
    let adder = match rustix::io::dup(&fd) {
        Ok(adder) => adder,
        Err(err) => {
            tracing::warn!(%err, "the wallpapers' folders are not watched");
            return None;
        }
    };
    let started = std::thread::Builder::new()
        .name("wallpaper: watch".to_owned())
        .spawn(move || {
            let mut buffer = [std::mem::MaybeUninit::uninit(); 4096];
            let mut events = inotify::Reader::new(&fd, &mut buffer);
            loop {
                match events.next() {
                    Ok(_) => {
                        if sender.send(Heard::Changed).is_err() {
                            return;
                        }
                    }
                    Err(Errno::INTR) => {}
                    Err(err) => {
                        tracing::warn!(%err, "stopped watching the wallpapers' folders");
                        return;
                    }
                }
            }
        });
    match started {
        Ok(_) => Some(adder),
        Err(err) => {
            tracing::warn!(%err, "the wallpapers' folders are not watched");
            None
        }
    }
}

/// The folders' thread: lists, adds to the watch, reads and writes what
/// is kept.
fn folders(
    taken: &mpsc::Receiver<Job>,
    sender: &channel::Sender<Heard>,
    file: Option<PathBuf>,
    notify: Option<OwnedFd>,
) {
    use rustix::fs::inotify::{self, WatchFlags};
    let mut kept: HashMap<PathBuf, Kept> = file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let flags = WatchFlags::CREATE
        | WatchFlags::DELETE
        | WatchFlags::MOVED_TO
        | WatchFlags::MOVED_FROM
        | WatchFlags::CLOSE_WRITE
        | WatchFlags::DELETE_SELF;
    while let Ok(job) = taken.recv() {
        match job {
            Job::List(folder) => {
                let listed = hocus_link::wallpaper::list(&folder);
                if let Some(fd) = &notify {
                    for dir in &listed.folders {
                        if let Err(err) = inotify::add_watch(fd, dir, flags) {
                            tracing::debug!(%err, dir = %dir.display(), "a folder is not watched");
                        }
                    }
                }
                tracing::info!(
                    folder = %folder.display(),
                    pictures = listed.pictures.len(),
                    "the wallpaper's folder is listed"
                );
                let heard = Heard::Listed {
                    kept: kept.get(&folder).cloned(),
                    folder,
                    pictures: listed.pictures,
                };
                if sender.send(heard).is_err() {
                    return;
                }
            }
            Job::Keep(rounds) => {
                let Some(file) = &file else {
                    continue;
                };
                kept.extend(rounds);
                if kept.len() > KEPT_MOST {
                    let mut by_age: Vec<(u64, PathBuf)> =
                        kept.iter().map(|(p, k)| (k.changed, p.clone())).collect();
                    by_age.sort();
                    for (_, old) in by_age.iter().take(kept.len() - KEPT_MOST) {
                        kept.remove(old);
                    }
                }
                if let Err(err) = write(file, &kept) {
                    tracing::warn!(%err, file = %file.display(), "the wallpaper's round is not kept");
                }
            }
        }
    }
}

/// `kept` written beside `file`, then renamed over it.
fn write(file: &Path, kept: &HashMap<PathBuf, Kept>) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string(kept).map_err(std::io::Error::other)?;
    let beside = file.with_extension("json.new");
    std::fs::write(&beside, text)?;
    std::fs::rename(&beside, file)
}

/// What a screen's wallpaper is made of, as the file says.
struct Source<'a> {
    picture: &'a Picture,
    folder: &'a str,
    day: &'a Picture,
    night: &'a Picture,
}

impl State {
    /// `output`'s wallpaper in the file (every screen's with `None`), its
    /// own with per-screen.
    fn wallpaper_source(&self, output: Option<&str>) -> Source<'_> {
        let wallpaper = &self.config.wallpaper;
        let own = output
            .filter(|_| wallpaper.per_screen)
            .and_then(|o| self.config.outputs.get(o))
            .filter(|o| o.wallpaper.is_some());
        Source {
            picture: own
                .and_then(|o| o.wallpaper.as_ref())
                .unwrap_or(&wallpaper.picture),
            folder: own
                .and_then(|o| o.wallpaper_folder.as_deref())
                .unwrap_or(&wallpaper.folder),
            day: own
                .and_then(|o| o.wallpaper_day.as_ref())
                .unwrap_or(&wallpaper.day),
            night: own
                .and_then(|o| o.wallpaper_night.as_ref())
                .unwrap_or(&wallpaper.night),
        }
    }

    fn folder_of(&self, source: &Source<'_>) -> Option<PathBuf> {
        (*source.picture == Picture::Folder)
            .then(|| self.config.chosen_folder(source.folder))
            .flatten()
    }

    /// The picture `output` (every screen with `None`) shows now: a
    /// folder's or day and night's resolved; `None` while its folder is
    /// not listed yet.
    pub(crate) fn wallpaper_now(&self, output: Option<&str>) -> Option<Picture> {
        let source = self.wallpaper_source(output);
        match source.picture {
            Picture::Folder => {
                let Some(folder) = self.folder_of(&source) else {
                    // No folder chosen: Nuit.
                    return Some(Picture::default());
                };
                let round = self.changing.folders.get(&folder)?.round.as_ref()?;
                Some(
                    round
                        .current()
                        .map_or_else(Picture::default, |p| Picture::File(p.display().to_string())),
                )
            }
            Picture::DayNight => Some(match self.changing.phase {
                Some(Phase::Night) => source.night.clone(),
                _ => source.day.clone(),
            }),
            picture => Some(picture.clone()),
        }
    }

    /// The folders the file names, and whether day and night show.
    fn wallpaper_wanted(&self) -> (Vec<PathBuf>, bool) {
        let mut outputs: Vec<Option<&str>> = vec![None];
        if self.config.wallpaper.per_screen {
            outputs.extend(self.config.outputs.keys().map(|o| Some(o.as_str())));
        }
        let mut folders = Vec::new();
        let mut day_night = false;
        for output in outputs {
            let source = self.wallpaper_source(output);
            day_night |= *source.picture == Picture::DayNight;
            if let Some(folder) = self.folder_of(&source)
                && !folders.contains(&folder)
            {
                folders.push(folder);
            }
        }
        (folders, day_night)
    }

    /// How often a folder's picture changes; none: as each session opens.
    fn wallpaper_every(&self) -> Option<Duration> {
        self.config
            .wallpaper_every
            .or_else(|| self.config.wallpaper.every.length())
    }

    /// Day or night now, and when they next switch.
    fn wallpaper_phase(&self) -> (Phase, jiff::Timestamp) {
        let weather = &self.config.weather;
        let by = if weather.has_place() {
            Switches::Sun(weather.latitude, weather.longitude)
        } else {
            let w = &self.config.wallpaper;
            Switches::Hours(w.day_starts, w.night_starts)
        };
        phase_at(&jiff::Zoned::now(), by)
    }

    /// The file changed, or the desktop starts: the folders it names
    /// followed (listed when new), the order and the interval taken, day
    /// and night looked at. What shows changes at once, without a fade.
    pub(crate) fn wallpaper_follow(&mut self) {
        let before = self.wallpaper_shows();
        let (folders, day_night) = self.wallpaper_wanted();
        let order = self.config.wallpaper.order;
        let every = self.wallpaper_every();
        self.changing.folders.retain(|f, _| folders.contains(f));
        for folder in folders {
            match self.changing.folders.get_mut(&folder) {
                Some(followed) => {
                    if let Some(round) = &mut followed.round {
                        round.reorder(order);
                        // A new interval counts from now.
                        let due = every.map(|e| Instant::now() + e);
                        if followed.due.is_none() != due.is_none()
                            || followed.due.zip(due).is_some_and(|(a, b)| a > b)
                        {
                            followed.due = due;
                        }
                    }
                }
                None => {
                    self.changing.folders.insert(
                        folder.clone(),
                        Folder {
                            round: None,
                            due: None,
                            changed: wall_now(),
                        },
                    );
                    if let Some(jobs) = &self.changing.jobs {
                        let _ = jobs.send(Job::List(folder));
                    }
                }
            }
        }
        if day_night {
            let (phase, at) = self.wallpaper_phase();
            self.changing.phase = Some(phase);
            self.changing.switch_at = Some(at);
        } else {
            self.changing.phase = None;
            self.changing.switch_at = None;
        }
        self.wallpaper_arm();
        self.wallpaper_moved(before, false);
    }

    /// What each screen shows now, and whether it is day and night's.
    fn wallpaper_shows(&self) -> Vec<(String, Option<Picture>, bool)> {
        self.space
            .outputs()
            .map(|o| {
                let name = o.name();
                let day_night = *self.wallpaper_source(Some(&name)).picture == Picture::DayNight;
                (name.clone(), self.wallpaper_now(Some(&name)), day_night)
            })
            .collect()
    }

    /// After a change: the screens whose picture changed fade into it
    /// (`fade`, unless reduced motion or the CPU), draw again, and the
    /// shell and the Settings are told.
    fn wallpaper_moved(&mut self, before: Vec<(String, Option<Picture>, bool)>, fade: bool) {
        let fades = fade && !self.config.appearance.reduced_motion && self.graphics.blur;
        let mut moved = Vec::new();
        for (name, was, day_night) in before {
            let now = self.wallpaper_now(Some(&name));
            if now == was || now.is_none() {
                continue;
            }
            if fades && was.is_some() {
                let length = if day_night {
                    DAY_NIGHT_FADE
                } else {
                    FOLDER_FADE
                };
                let asked = self.now();
                self.graphics
                    .backgrounds
                    .get_mut()
                    .fade_next
                    .insert(name.clone(), (length, asked));
            }
            moved.push((name, now));
        }
        if !moved.is_empty() {
            tracing::info!(?moved, fade = fades, "the wallpaper's picture changes");
            self.redraw = true;
        }
        self.wallpaper_told(self.wallpapers());
        if !moved.is_empty() {
            self.wallpaper_accent_now();
        }
    }

    /// The shell told the wallpapers, the Settings each screen's picture
    /// now, unless they know them already.
    pub(crate) fn wallpaper_told(&mut self, wallpapers: Wallpapers) {
        if self.changing.told.as_ref() != Some(&wallpapers) {
            self.changing.told = Some(wallpapers.clone());
            self.tell_shell(ToShell::Wallpaper(wallpapers));
        }
        let line = self.wallpaper_now_line();
        if self.changing.told_settings.as_ref() != Some(&line) {
            self.changing.told_settings = Some(line.clone());
            self.displays_send(None, &line);
        }
    }

    /// What the Settings are told: each screen's picture now, by name.
    pub(crate) fn wallpaper_now_line(&self) -> String {
        let now: serde_json::Map<String, serde_json::Value> = self
            .space
            .outputs()
            .filter_map(|o| {
                let name = o.name();
                let picture = self.wallpaper_now(Some(&name))?;
                Some((name, serde_json::Value::from(picture.word())))
            })
            .collect();
        serde_json::json!({ "wallpaper-now": now }).to_string()
    }

    /// The thread listed `folder`, or a folder changed.
    fn wallpaper_heard(&mut self, heard: Heard) {
        match heard {
            Heard::Changed => {
                if let Some(token) = self.changing.settle.take() {
                    self.loop_handle.remove(token);
                }
                let timer = Timer::from_duration(SETTLE);
                let inserted = self.loop_handle.insert_source(timer, |_, _, app| {
                    app.state.changing.settle = None;
                    let listed: Vec<PathBuf> = app
                        .state
                        .changing
                        .folders
                        .iter()
                        .filter(|(_, f)| f.round.is_some())
                        .map(|(p, _)| p.clone())
                        .collect();
                    if let Some(jobs) = &app.state.changing.jobs {
                        for folder in listed {
                            let _ = jobs.send(Job::List(folder));
                        }
                    }
                    TimeoutAction::Drop
                });
                match inserted {
                    Ok(token) => self.changing.settle = Some(token),
                    Err(err) => tracing::warn!(%err.error, "a folder's change is not listed"),
                }
            }
            Heard::Listed {
                folder,
                pictures,
                kept,
            } => self.wallpaper_listed(&folder, pictures, kept.as_ref()),
        }
    }

    fn wallpaper_listed(&mut self, folder: &Path, pictures: Vec<PathBuf>, kept: Option<&Kept>) {
        let before = self.wallpaper_shows();
        let order = self.config.wallpaper.order;
        let every = self.wallpaper_every();
        let Some(followed) = self.changing.folders.get_mut(folder) else {
            return;
        };
        let fade = match &mut followed.round {
            // Listed again: its pictures added and removed.
            Some(round) => {
                if round.update(pictures) {
                    followed.changed = wall_now();
                    self.changing.changes += 1;
                }
                true
            }
            // Listed first: the kept round goes on where it was.
            None => {
                let mut round = match kept {
                    Some(kept) => Round::again(kept, order, pictures, seed()),
                    None => Round::new(pictures, order, seed()),
                };
                let since = kept.map(|k| Duration::from_secs(wall_now().saturating_sub(k.changed)));
                let (at_once, next) = first_change(every, since);
                if at_once {
                    round.advance();
                    followed.changed = wall_now();
                    self.changing.changes += 1;
                } else if let Some(kept) = kept {
                    followed.changed = kept.changed;
                }
                followed.due = next.map(|n| Instant::now() + n);
                followed.round = Some(round);
                false
            }
        };
        self.wallpaper_keep();
        self.wallpaper_arm();
        self.wallpaper_moved(before, fade);
        for (name, _, _) in self.wallpaper_shows() {
            self.wallpaper_prepare(&name);
        }
    }

    /// The rounds kept, by the folders' thread.
    fn wallpaper_keep(&self) {
        if self.config.state_dir.is_none() {
            return;
        }
        let rounds: Vec<(PathBuf, Kept)> = self
            .changing
            .folders
            .iter()
            .filter_map(|(p, f)| Some((p.clone(), f.round.as_ref()?.kept(f.changed))))
            .collect();
        if let Some(jobs) = &self.changing.jobs {
            let _ = jobs.send(Job::Keep(rounds));
        }
    }

    /// The timer at the next change, if any.
    fn wallpaper_arm(&mut self) {
        if let Some(token) = self.changing.timer.take() {
            self.loop_handle.remove(token);
        }
        let now = Instant::now();
        let mut next: Option<Duration> = self
            .changing
            .folders
            .values()
            .filter_map(|f| f.due)
            .map(|due| due.saturating_duration_since(now))
            .min();
        if let Some(at) = self.changing.switch_at {
            let left = jiff::Timestamp::now().duration_until(at);
            let left = if left.is_negative() {
                Duration::ZERO
            } else {
                left.unsigned_abs().min(LOOK_AGAIN)
            };
            next = Some(next.map_or(left, |n| n.min(left)));
        }
        let Some(next) = next else {
            return;
        };
        let timer = Timer::from_duration(next);
        let inserted = self.loop_handle.insert_source(timer, |_, _, app| {
            app.state.changing.timer = None;
            app.state.wallpaper_due();
            TimeoutAction::Drop
        });
        match inserted {
            Ok(token) => self.changing.timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "the wallpaper will not change by itself"),
        }
    }

    /// The timer: the folders due change, day or night looked at.
    fn wallpaper_due(&mut self) {
        let before = self.wallpaper_shows();
        let now = Instant::now();
        let every = self.wallpaper_every();
        let mut changed = false;
        for followed in self.changing.folders.values_mut() {
            if followed.due.is_some_and(|due| due <= now)
                && let Some(round) = &mut followed.round
            {
                round.advance();
                followed.changed = wall_now();
                followed.due = every.map(|e| now + e);
                changed = true;
                self.changing.changes += 1;
            }
        }
        if self.changing.phase.is_some() {
            let (phase, at) = self.wallpaper_phase();
            if self.changing.phase != Some(phase) {
                self.changing.changes += 1;
            }
            self.changing.phase = Some(phase);
            self.changing.switch_at = Some(at);
        }
        if changed {
            self.wallpaper_keep();
        }
        self.wallpaper_arm();
        self.wallpaper_moved(before, true);
    }

    /// "Image suivante" (#894), on `output` (every screen's with
    /// `None`): its folder's next picture now, the interval counted again
    /// from it. Whether its wallpaper is a folder's.
    pub fn wallpaper_next(&mut self, output: Option<&str>) -> bool {
        let source = self.wallpaper_source(output);
        let Some(folder) = self.folder_of(&source) else {
            tracing::info!(output, "the wallpaper is no folder's: no next picture");
            return false;
        };
        let before = self.wallpaper_shows();
        let every = self.wallpaper_every();
        let Some(followed) = self.changing.folders.get_mut(&folder) else {
            return false;
        };
        let Some(round) = &mut followed.round else {
            return false;
        };
        round.advance();
        followed.changed = wall_now();
        followed.due = every.map(|e| Instant::now() + e);
        self.changing.changes += 1;
        self.wallpaper_keep();
        self.wallpaper_arm();
        self.wallpaper_moved(before, true);
        true
    }

    /// Whether `output`'s wallpaper is a folder's: "Image suivante" runs.
    pub(crate) fn wallpaper_has_next(&self, output: &str) -> bool {
        self.folder_of(&self.wallpaper_source(Some(output)))
            .is_some()
    }

    /// `output`'s next picture drawn ahead, when its wallpaper is a
    /// folder's (#894): at the size it shows now.
    pub(crate) fn wallpaper_prepare(&mut self, output: &str) {
        let source = self.wallpaper_source(Some(output));
        let Some(folder) = self.folder_of(&source) else {
            return;
        };
        let Some(next) = self
            .changing
            .folders
            .get(&folder)
            .and_then(|f| f.round.as_ref()?.upcoming())
        else {
            return;
        };
        let backgrounds = self.graphics.backgrounds.get_mut();
        let Some(shown) = backgrounds.shown.get(output) else {
            return;
        };
        let key = Key {
            look: Look {
                picture: Picture::File(next.display().to_string()),
                ..shown.key.look.clone()
            },
            size: shown.key.size,
            scale: shown.key.scale,
        };
        backgrounds.want_ahead(output, &key);
    }

    /// The fades under way move on (#894): their screens draw again, an
    /// ended one a last time.
    pub(crate) fn tick_wallpaper_fades(&mut self, now: Duration) {
        let backgrounds = self.graphics.backgrounds.get_mut();
        let taken = std::mem::take(&mut backgrounds.took_ahead);
        if backgrounds.fading.is_empty() && backgrounds.fade_next.is_empty() && taken.is_empty() {
            return;
        }
        // A fade asked starts as its picture is drawn: its screen draws
        // until then (a moment, the picture drawn ahead), not forever.
        backgrounds
            .fade_next
            .retain(|_, (_, asked)| now.saturating_sub(*asked) < Duration::from_secs(10));
        let names: Vec<String> = backgrounds
            .fading
            .keys()
            .chain(backgrounds.fade_next.keys())
            .cloned()
            .collect();
        backgrounds.fading.retain(|_, fade| !fade.done(now));
        let screens: Vec<smithay::output::Output> = self
            .space
            .outputs()
            .filter(|o| names.contains(&o.name()))
            .cloned()
            .collect();
        if !screens.is_empty() {
            self.moved_on_screens(screens);
        }
        for output in taken {
            self.wallpaper_prepare(&output);
        }
    }
}
