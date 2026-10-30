//! The windows' applications' icons, for the tiles' sketches and the
//! desktops' lists (3.9b): found as the sound's applications find theirs
//! ([`crate::levels::icons`]), by the application's id, on a thread of
//! their own, never on the one that draws, once per id.
//!
//! What draws reads what was found and asks for what was not looked for
//! yet; once found, the desktops' surfaces are woken, and the list if it
//! shows. Ids looked for together wake them once.

use std::{
    collections::HashMap,
    sync::{
        Mutex, PoisonError,
        mpsc::{self, Receiver, Sender},
    },
};

use hocus_link::DESKTOPS_NAMESPACE;

use crate::{levels::icons::Icon, popover};

/// An application's icon, as looked for.
#[derive(Debug, Clone)]
enum Looked {
    Asked,
    Found(Option<Icon>),
}

static ICONS: Mutex<Option<HashMap<String, Looked>>> = Mutex::new(None);
static ASK: Mutex<Option<Sender<String>>> = Mutex::new(None);

fn icons<T>(with: impl FnOnce(&mut HashMap<String, Looked>) -> T) -> T {
    let mut icons = ICONS.lock().unwrap_or_else(PoisonError::into_inner);
    with(icons.get_or_insert_with(HashMap::new))
}

/// Application `app`'s icon, if it was found.
pub fn of(app: &str) -> Option<Icon> {
    icons(|icons| match icons.get(app) {
        Some(Looked::Found(icon)) => icon.clone(),
        _ => None,
    })
}

/// The icons of `apps` are wanted: those not looked for yet are, off the
/// loop that draws.
pub fn want<'a>(apps: impl IntoIterator<Item = &'a str>) {
    let new: Vec<String> = icons(|icons| {
        let mut new = Vec::new();
        for app in apps {
            if !app.is_empty() && !icons.contains_key(app) {
                icons.insert(app.to_owned(), Looked::Asked);
                new.push(app.to_owned());
            }
        }
        new
    });
    if new.is_empty() {
        return;
    }
    let mut ask = ASK.lock().unwrap_or_else(PoisonError::into_inner);
    if ask.is_none() {
        let (sender, receiver) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("desktops' icons".to_owned())
            .spawn(move || look(&receiver));
        match started {
            Ok(_) => *ask = Some(sender),
            Err(err) => {
                tracing::warn!(%err, "cannot look for the windows' icons");
                return;
            }
        }
    }
    if let Some(sender) = ask.as_ref() {
        for app in new {
            let _ = sender.send(app);
        }
    }
}

/// The names an application's icon may go by, from its id, the likeliest
/// first: the id itself (a desktop file's, as Wayland's ids are meant to
/// be, or an icon's), in lower case (X11's classes are capitalised), and
/// its last part (`org.gnome.Nautilus`'s `nautilus`).
pub fn names(app: &str) -> Vec<String> {
    let lower = app.to_lowercase();
    let last = lower.rsplit('.').next().unwrap_or_default().to_owned();
    let mut names = vec![app.to_owned()];
    for name in [lower, last] {
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The icons' thread: each id asked, and those asked meanwhile, then one
/// wake for what was found.
fn look(asked: &Receiver<String>) {
    while let Ok(first) = asked.recv() {
        let mut found = false;
        for app in std::iter::once(first).chain(asked.try_iter()) {
            let icon = crate::levels::icons::find(&names(&app));
            tracing::debug!(app = %app, found = icon.is_some(), "a window's icon");
            found |= icon.is_some();
            icons(|icons| icons.insert(app, Looked::Found(icon)));
        }
        if found {
            popover::send(vec![popover::Effect::Wake(DESKTOPS_NAMESPACE)]);
            super::list_changed(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_apps_icon_goes_by_its_id_its_lower_case_and_its_last_part() {
        assert_eq!(
            names("org.gnome.Nautilus"),
            ["org.gnome.Nautilus", "org.gnome.nautilus", "nautilus"]
        );
        assert_eq!(names("foot"), ["foot"]);
        assert_eq!(names("Firefox"), ["Firefox", "firefox"]);
    }
}
