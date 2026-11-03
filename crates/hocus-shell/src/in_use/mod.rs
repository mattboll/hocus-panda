//! What is in use, and the bar's sign that says so (5.11): so far the
//! screen, shared through the desktop portal (`shares`, from the
//! compositor). 5.12 adds the microphone and the camera, and groups them
//! in one sign of tiles under the clock, the screen's being one kind of
//! [`Use`]: the model here is theirs to grow, the sign's place too.
//!
//! The sign shows under the clock while something is in use, in both
//! forms: a surface of its own on each bar, opened when the first use
//! starts and removed when the last one ends; the agents' group moves
//! under it ([`room`]). Hovering it (150 ms) or a click opens its popover
//! ([`crate::popover::sharing`]): who sees what, the notifications held,
//! and a button to stop each share.

pub mod draw;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{EDGE, Form, IN_USE_NAMESPACE, ShareShown};
use iced::window;

use crate::popover::{self, Effect, Item};

/// What is in use, by kind: the screen's shares, for now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Uses {
    pub shares: Vec<ShareShown>,
}

/// A kind of use, as the sign shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    Screen,
}

impl Uses {
    pub fn kinds(&self) -> Vec<Use> {
        let mut kinds = Vec::new();
        if !self.shares.is_empty() {
            kinds.push(Use::Screen);
        }
        kinds
    }

    pub fn any(&self) -> bool {
        !self.kinds().is_empty()
    }
}

static USES: Mutex<Option<Uses>> = Mutex::new(None);

/// What is in use now.
pub fn snapshot() -> Uses {
    USES.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// The room the sign takes under the clock, the 14 px after it included:
/// none when nothing is in use.
pub fn room() -> f32 {
    if snapshot().any() {
        draw::SIGN.height + draw::AFTER
    } else {
        0.0
    }
}

/// The compositor's `shares`.
pub fn set_shares(shares: Vec<ShareShown>) {
    let (before, after) = {
        let mut uses = USES.lock().unwrap_or_else(PoisonError::into_inner);
        let uses = uses.get_or_insert_with(Uses::default);
        let before = uses.clone();
        uses.shares = shares;
        (before, uses.clone())
    };
    if before == after {
        return;
    }
    tracing::debug!(
        shares = ?after.shares.iter().map(|s| (s.id, s.app.clone())).collect::<Vec<_>>(),
        "what is in use"
    );
    crate::notifications::sharing(!after.shares.is_empty());
    let mut effects = Vec::new();
    if before.any() != after.any() {
        // The sign comes or goes on every bar, the agents move.
        places(|places| {
            for (bar, place) in places.iter_mut() {
                effects.extend(place.show(bar, after.any()));
            }
        });
        crate::agents::draw::moved();
        if !after.any() && popover::with(|c| c.open_item()) == Some(Item::Sharing) {
            popover::event(popover::Event::Chosen);
        }
    }
    effects.push(Effect::Wake(IN_USE_NAMESPACE));
    popover::send(effects);
    popover::changed(Item::Sharing);
}

/// The sign's surface on a bar.
#[derive(Debug, Clone, Copy)]
struct Place {
    id: Option<window::Id>,
    form: Form,
    switching: bool,
}

impl Place {
    /// The sign shows on `bar` (`shown`), or goes: what opens or removes it.
    fn show(&mut self, bar: &str, shown: bool) -> Vec<Effect> {
        match (self.id, shown) {
            (None, true) => {
                let id = window::Id::unique();
                self.id = Some(id);
                let (size, margin) = placement(self.form, self.switching);
                popover::with(|c| c.set_in_use(bar, Some(id)));
                vec![Effect::Layer {
                    id,
                    bar: bar.to_owned(),
                    namespace: IN_USE_NAMESPACE,
                    size,
                    margin,
                }]
            }
            (Some(id), false) => {
                self.id = None;
                popover::with(|c| c.set_in_use(bar, None));
                vec![Effect::Remove(id)]
            }
            _ => Vec::new(),
        }
    }
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// The surface's size and its margins in `form` (both forms' places while
/// the bar switches).
fn placement(form: Form, switching: bool) -> ((u32, u32), (i32, i32, i32, i32)) {
    let ((w, h), top) = draw::placement(form, switching);
    ((w, h), (EDGE + top.round() as i32, EDGE + 1, 0, 0))
}

/// Bar `bar` opened in `form`: its sign, if something is in use.
pub fn bar_opened(bar: &str, form: Form) -> Vec<Effect> {
    let shown = snapshot().any();
    places(|places| {
        let place = places.entry(bar.to_owned()).or_insert(Place {
            id: None,
            form,
            switching: false,
        });
        place.form = form;
        place.show(bar, shown)
    })
}

/// Bar `bar` in `form`, switching or not: its sign's size and place.
pub fn form(bar: &str, form: Form, switching: bool) -> Vec<Effect> {
    places(|places| {
        let Some(place) = places.get_mut(bar) else {
            return Vec::new();
        };
        if place.form == form && place.switching == switching {
            return Vec::new();
        }
        place.form = form;
        place.switching = switching;
        let Some(id) = place.id else {
            return Vec::new();
        };
        let (size, margin) = placement(form, switching);
        vec![Effect::Resize {
            id,
            size,
            margin: Some(margin),
        }]
    })
}

/// Bar `bar` went: its sign's surface, to remove.
pub fn forget(bar: &str) -> Option<window::Id> {
    places(|places| places.remove(bar)).and_then(|p| p.id)
}

/// The bar whose sign's surface is `id`.
pub fn bar_of(id: window::Id) -> Option<String> {
    places(|places| {
        places
            .iter()
            .find(|(_, p)| p.id == Some(id))
            .map(|(bar, _)| bar.clone())
    })
}

/// The share's words, for the sign and its popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: crate::clock::time::Lang) -> Self {
        Self {
            fr: lang == crate::clock::time::Lang::French,
        }
    }

    pub fn sharing(self) -> &'static str {
        if self.fr { "Partage" } else { "Sharing" }
    }

    /// "Firefox voit ton écran".
    pub fn sees(self, app: Option<&str>) -> String {
        match (app, self.fr) {
            (Some(app), true) => format!("{app} voit ton écran"),
            (Some(app), false) => format!("{app} sees your screen"),
            (None, true) => "Une application voit ton écran".to_owned(),
            (None, false) => "An application sees your screen".to_owned(),
        }
    }

    /// "Tu partages la fenêtre « … »", or the screen.
    pub fn shares(self, thing: &hocus_link::SharedThing) -> String {
        use hocus_link::SharedThing;
        match (thing, self.fr) {
            (SharedThing::Window(title), true) => format!("Tu partages la fenêtre « {title} »"),
            (SharedThing::Window(title), false) => {
                format!("You share the window \u{201c}{title}\u{201d}")
            }
            (SharedThing::Screen(name), true) if name.is_empty() => {
                "Tu partages l'écran du portable".to_owned()
            }
            (SharedThing::Screen(name), false) if name.is_empty() => {
                "You share the laptop screen".to_owned()
            }
            (SharedThing::Screen(name), true) => format!("Tu partages l'écran « {name} »"),
            (SharedThing::Screen(name), false) => {
                format!("You share the screen \u{201c}{name}\u{201d}")
            }
        }
    }

    /// How many notifications wait for the end.
    pub fn held(self, n: usize) -> String {
        match (n, self.fr) {
            (0, true) => "Les notifications attendent la fin".to_owned(),
            (0, false) => "Notifications wait until the end".to_owned(),
            (1, true) => "1 notification en attente".to_owned(),
            (1, false) => "1 notification held".to_owned(),
            (n, true) => format!("{n} notifications en attente"),
            (n, false) => format!("{n} notifications held"),
        }
    }

    pub fn stop(self) -> &'static str {
        if self.fr {
            "Arrêter le partage"
        } else {
            "Stop sharing"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::time::Lang;
    use hocus_link::SharedThing;

    #[test]
    fn the_signs_words() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.sharing(), "Partage");
        assert_eq!(fr.sees(Some("Firefox")), "Firefox voit ton écran");
        assert_eq!(en.sees(Some("Firefox")), "Firefox sees your screen");
        assert_eq!(
            fr.shares(&SharedThing::Window(
                "hocus-panda — Visual Studio Code".into()
            )),
            "Tu partages la fenêtre « hocus-panda — Visual Studio Code »"
        );
        assert_eq!(
            en.shares(&SharedThing::Screen(String::new())),
            "You share the laptop screen"
        );
        assert_eq!(fr.held(4), "4 notifications en attente");
        assert_eq!(en.held(0), "Notifications wait until the end");
        assert_eq!(fr.stop(), "Arrêter le partage");
    }
}
