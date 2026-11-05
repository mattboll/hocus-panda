//! The lock's background (6.3): each screen's wallpaper, or the lock's own
//! picture, as the compositor says them (`wallpaper` on the line), dimmed
//! by the veil, or blurred once (never a live blur of the screen: the lock
//! never shows windows).
//!
//! Drawn on a thread of its own with the compositor's function
//! ([`hocus_link::wallpaper::render`]), at twice the lock's logical size
//! (smooth either way); until a screen's is drawn, its previous stays, and
//! the plain dark before the first. Drawn, the lock's surfaces are woken.

use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock, PoisonError,
        mpsc::{self, Sender},
    },
};

use hocus_link::wallpaper::{LockLook, Look, Wallpapers};
use iced::advanced::image::Handle;

/// What a screen's lock shows, at its size.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    look: Look,
    blurred: bool,
    size: (u32, u32),
}

#[derive(Debug, Default)]
struct Made {
    /// By screen: what it shows now.
    shown: HashMap<String, (Key, Handle)>,
    /// What each screen waits for.
    asked: HashMap<String, Key>,
}

static WALLPAPERS: Mutex<Option<Wallpapers>> = Mutex::new(None);
static MADE: Mutex<Option<Made>> = Mutex::new(None);
/// A screen's background asked of the thread.
type Job = (String, Key);

static JOBS: OnceLock<Option<Mutex<Sender<Job>>>> = OnceLock::new();

/// The compositor says the wallpapers now: whether they changed.
pub fn set(wallpapers: Wallpapers) -> bool {
    let mut now = WALLPAPERS.lock().unwrap_or_else(PoisonError::into_inner);
    let changed = now.as_ref() != Some(&wallpapers);
    *now = Some(wallpapers);
    changed
}

/// The lock's background on `output`, `size` logical pixels large: its
/// picture once drawn (the previous meanwhile); `None` before any.
pub fn background(output: &str, size: (u32, u32)) -> Option<Handle> {
    let wallpapers = WALLPAPERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default();
    let key = Key {
        look: wallpapers.lock_look(output),
        blurred: wallpapers.lock == LockLook::Blurred,
        size: (size.0 * 2, size.1 * 2),
    };
    let mut made = MADE.lock().unwrap_or_else(PoisonError::into_inner);
    let made = made.get_or_insert_with(Made::default);
    let shown = made.shown.get(output);
    if shown.is_none_or(|(k, _)| *k != key)
        && made.asked.get(output) != Some(&key)
        && let Some(jobs) = jobs()
    {
        let sent = jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send((output.to_owned(), key.clone()))
            .is_ok();
        if sent {
            made.asked.insert(output.to_owned(), key);
        }
    }
    shown.map(|(_, handle)| handle.clone())
}

/// The thread, started with the first background asked.
fn jobs() -> Option<&'static Mutex<Sender<Job>>> {
    JOBS.get_or_init(|| {
        let (sender, taken) = mpsc::channel::<Job>();
        let started = std::thread::Builder::new()
            .name("lock: wallpaper".to_owned())
            .spawn(move || {
                while let Ok(first) = taken.recv() {
                    // Each screen's last ask only.
                    let mut jobs = vec![first];
                    jobs.extend(taken.try_iter());
                    let mut last: Vec<Job> = Vec::new();
                    for job in jobs {
                        last.retain(|(o, _)| *o != job.0);
                        last.push(job);
                    }
                    for (output, key) in last {
                        draw(output, key);
                    }
                }
            });
        match started {
            Ok(_) => Some(Mutex::new(sender)),
            Err(err) => {
                tracing::warn!(%err, "no thread for the lock's wallpaper: it stays plain");
                None
            }
        }
    })
    .as_ref()
}

fn draw(output: String, key: Key) {
    let mut rendered =
        hocus_link::wallpaper::render(&key.look, (key.size.0 as i32, key.size.1 as i32));
    if key.blurred {
        rendered = hocus_link::wallpaper::soften(&rendered);
    }
    if let Some(why) = &rendered.failed {
        tracing::warn!(
            output,
            why,
            "the lock's wallpaper does not read: Nuit is drawn"
        );
    }
    tracing::info!(
        output,
        picture = %key.look.picture,
        blurred = key.blurred,
        "the lock's wallpaper is drawn"
    );
    let handle = Handle::from_rgba(
        rendered.width as u32,
        rendered.height as u32,
        rendered.pixels,
    );
    {
        let mut made = MADE.lock().unwrap_or_else(PoisonError::into_inner);
        let made = made.get_or_insert_with(Made::default);
        if made.asked.get(&output) != Some(&key) {
            return;
        }
        made.asked.remove(&output);
        made.shown.insert(output, (key, handle));
    }
    crate::popover::tell(hocus_link::ToCompositor::Wake(
        hocus_link::LOCK_NAMESPACE.to_owned(),
    ));
}
