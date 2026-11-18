//! The Hocuspanda camera, as the shell shows it (#619, docs/design/camera.md):
//! what the compositor says of it and of `[camera]` ([`shown`]), the one
//! list of its effects ([`effects`]), its window's preview ([`preview`]),
//! and the choices the window makes, written into the configuration
//! (`[camera]`) as the Settings write it, on a thread of their own.
//!
//! The window is the popover of the camera's tile in the in-use sign
//! (`popover::camera`); the violet dot on that tile says an effect is on.

pub mod effects;
pub mod preview;

use std::sync::{Mutex, PoisonError};

use hocus_link::{CameraShown, IN_USE_NAMESPACE, ToCompositor};

static SHOWN: Mutex<Option<CameraShown>> = Mutex::new(None);

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
    changed(was.is_none_or(|w| w.dot() != shown.dot()));
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
            _ => {}
        }
        let dot = before != now.dot();
        *shown = Some(now);
        dot
    };
    changed(dot);
    write(key, value);
}

fn write(key: &'static str, value: bool) {
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
                    table.insert(key, hocus_config::toml_edit::value(value));
                    Ok(())
                },
                |_| Ok(()),
            );
            match edited {
                Ok(_) => tracing::info!(key, value, "the camera's choice written"),
                Err(err) => tracing::warn!(%err, key, "the camera's choice is not written"),
            }
        });
    if let Err(err) = wrote {
        tracing::warn!(%err, "cannot write the camera's choice");
    }
}
