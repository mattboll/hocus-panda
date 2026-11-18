//! The Hocuspanda camera, as the shell shows it (#619, docs/design/camera.md):
//! what the compositor says of it and of `[camera]` ([`shown`]), the one
//! list of its effects ([`effects`]), its window's preview ([`preview`]),
//! and the choices the window makes, written into the configuration
//! (`[camera]`) as the Settings write it, on a thread of their own.
//!
//! The window is the popover of the camera's tile in the in-use sign
//! (`popover::camera`); the violet dot on that tile says an effect is on.
//!
//! « Je reviens » (lot 3): while the compositor says to hush (the screen
//! locked during a call), the session's default source is muted, if it
//! was not; and unmuted when that ends, if this muted it ([`hush`]).

pub mod effects;
pub mod preview;

use std::sync::{Mutex, PoisonError};

use hocus_link::{CameraShown, IN_USE_NAMESPACE, ToCompositor};

use crate::levels::{
    self,
    sound::{Ask, Set},
};

static SHOWN: Mutex<Option<CameraShown>> = Mutex::new(None);
/// The microphone « Je reviens » muted, while it holds it.
static HUSHED: Mutex<Option<u32>> = Mutex::new(None);

/// What the compositor says, as the shell connects and as it changes.
pub fn set(shown: CameraShown) {
    let was = SHOWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(shown.clone());
    if was.as_ref() == Some(&shown) {
        return;
    }
    tracing::debug!(?shown, "the camera");
    if was.as_ref().is_none_or(|w| w.hush != shown.hush) {
        hush(shown.hush);
    }
    changed(was.is_none_or(|w| w.dot() != shown.dot()));
}

/// The session's default source muted (`true`) if it was not, or
/// unmuted if this muted it.
fn hush(on: bool) {
    let mixer = levels::sound::mixer();
    let mut hushed = HUSHED.lock().unwrap_or_else(PoisonError::into_inner);
    if on {
        let Some(mic) = mixer.sources.iter().find(|s| s.default) else {
            tracing::info!("the screen locked during a call: no microphone to mute");
            return;
        };
        if mic.sound.is_some_and(|s| s.muted) || hushed.is_some() {
            return;
        }
        tracing::info!(
            id = mic.id,
            "the screen locked during a call: the microphone muted"
        );
        *hushed = Some(mic.id);
        levels::ask(Ask::Node(mic.id, Set::Mute(true)));
    } else if let Some(id) = hushed.take() {
        tracing::info!(id, "back: the microphone unmuted");
        levels::ask(Ask::Node(id, Set::Mute(false)));
    }
}

/// What is shown now; the configuration's defaults until said.
pub fn shown() -> CameraShown {
    SHOWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| CameraShown {
            lighting: true,
            banner: false,
            screen_light: true,
            strength: "auto".to_owned(),
            ready: false,
            read: false,
            source: None,
            background: "room".to_owned(),
            away: true,
            presenter: false,
            hush: false,
        })
}

/// The window draws again; the in-use sign too when its dot changed.
fn changed(dot: bool) {
    crate::popover::changed(crate::popover::Item::Camera);
    if dot {
        crate::popover::tell(ToCompositor::Wake(IN_USE_NAMESPACE.to_owned()));
    }
}

/// `[camera] key` set to `value`: shown at once, written into the
/// configuration on a thread of its own (the compositor follows the file
/// and says it back).
pub fn choose(key: &'static str, value: bool) {
    let dot = {
        let mut shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner);
        let mut now = shown.clone().unwrap_or_else(self::shown);
        let before = now.dot();
        match key {
            "lighting" => now.lighting = value,
            "banner" => now.banner = value,
            "screen-light" => now.screen_light = value,
            "away" => now.away = value,
            "presenter" => now.presenter = value,
            _ => {}
        }
        let dot = before != now.dot();
        *shown = Some(now);
        dot
    };
    changed(dot);
    write(key, hocus_config::toml_edit::value(value));
}

/// `[camera] key` set to the word `value` (one of a tab's: Fond's
/// `background`), as [`choose`].
pub fn choose_word(key: &'static str, value: &'static str) {
    let dot = {
        let mut shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner);
        let mut now = shown.clone().unwrap_or_else(self::shown);
        let before = now.dot();
        if key == "background" {
            value.clone_into(&mut now.background);
        }
        let dot = before != now.dot();
        *shown = Some(now);
        dot
    };
    changed(dot);
    write(key, hocus_config::toml_edit::value(value));
}

fn write(key: &'static str, value: hocus_config::toml_edit::Item) {
    let Some(file) = hocus_config::file() else {
        tracing::warn!(
            key,
            "no configuration file: the camera's choice is not kept"
        );
        return;
    };
    let wrote = std::thread::Builder::new()
        .name("camera's choice".to_owned())
        .spawn(move || {
            let edited = hocus_config::edit(
                &file,
                |doc| {
                    let table = doc
                        .entry("camera")
                        .or_insert_with(hocus_config::toml_edit::table)
                        .as_table_mut()
                        .ok_or_else(|| "[camera] is not a table".to_owned())?;
                    table.insert(key, value.clone());
                    Ok(())
                },
                |_| Ok(()),
            );
            match edited {
                Ok(_) => tracing::info!(key, "the camera's choice written"),
                Err(err) => tracing::warn!(%err, key, "the camera's choice is not written"),
            }
        });
    if let Err(err) = wrote {
        tracing::warn!(%err, "cannot write the camera's choice");
    }
}
