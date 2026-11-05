//! The lock screen (7.1, variant A, docs/design/lock.md): one surface on
//! each screen, over everything, while the compositor says the session is
//! locked (`lock …`), until it says it opened (`unlocked`).
//!
//! The compositor keeps the keyboard and the password; this draws what it
//! says: the phase, how many dots, Caps Lock, where `Tab` is, how many
//! agents wait. The pointer's clicks on the arrow, the field and the
//! layout's button go back to it (`lock submit`, `lock focus`,
//! `next-layout`). The screen the compositor names shows the form, the
//! status row and the sleeping pandas; the others the clock alone.
//!
//! At rest a lock surface draws once a minute, as the minute changes; it
//! draws every frame only while the field shakes or the spinner turns.
//! Meanwhile the bar's clock draws nothing, the bubbles wait.

pub mod avatar;
pub mod draw;
pub mod wallpaper;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{LOCK_NAMESPACE, LockAsk, LockFocus, LockPhase, LockShown, ToCompositor};
use iced::window;

use crate::{clock::time::Lang, popover::Effect};

/// The lock as the shell shows it.
#[derive(Debug, Clone)]
pub struct Lock {
    pub shown: LockShown,
    /// Its surface on each screen, by name.
    pub surfaces: HashMap<String, window::Id>,
    /// The last wrong password's shake: its number and when it started.
    pub shake: Option<(u32, Instant)>,
    /// When the check started, for the spinner.
    pub checking: Option<Instant>,
    /// What the pointer is over, and what it pressed, on which surface.
    pub hover: Option<(window::Id, Target)>,
    pub pressed: Option<(window::Id, Target)>,
}

/// What can be clicked on the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Field,
    Button,
    Layout,
}

static LOCK: Mutex<Option<Lock>> = Mutex::new(None);

/// `with` on the lock, if one shows.
pub fn with<T>(with: impl FnOnce(&mut Option<Lock>) -> T) -> T {
    with(&mut LOCK.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Whether the session is locked: the bar's clock and the bubbles wait.
pub fn active() -> bool {
    with(|l| l.is_some())
}

/// The screen surface `id` is the lock's on, if it is one.
pub fn output_of(id: window::Id) -> Option<String> {
    with(|l| {
        l.as_ref()?
            .surfaces
            .iter()
            .find(|(_, s)| **s == id)
            .map(|(o, _)| o.clone())
    })
}

/// The compositor's `lock …`: the lock opens on every screen `screens`
/// names (or where the compositor puts it, when they cannot be known),
/// or shows its new state.
pub fn set(shown: LockShown, screens: &crate::outputs::Known) -> Vec<Effect> {
    let now = Instant::now();
    let mut opened = false;
    let effects = with(|slot| {
        let lock = slot.get_or_insert_with(|| {
            opened = true;
            Lock {
                shown: shown.clone(),
                surfaces: HashMap::new(),
                shake: None,
                checking: None,
                hover: None,
                pressed: None,
            }
        });
        // A new wrong password shakes the field again.
        if shown.phase == LockPhase::Wrong
            && shown.motion
            && (opened || shown.shake != lock.shown.shake)
        {
            lock.shake = Some((shown.shake, now));
        }
        if shown.phase == LockPhase::Checking {
            lock.checking.get_or_insert(now);
        } else {
            lock.checking = None;
        }
        lock.shown = shown;
        let mut effects = surfaces(lock, screens);
        effects.push(Effect::Wake(LOCK_NAMESPACE));
        effects
    });
    if opened {
        tracing::info!("the session is locked: the lock shows");
        // Nothing else of the shell stays open over it.
        crate::popover::event(crate::popover::Event::PressElsewhere);
        crate::launcher::close();
        crate::emoji::close();
        crate::notifications::locked(true);
    }
    effects
}

/// The screens changed while locked: a new one gets the clock, one gone
/// takes its surface with it.
pub fn screens_known(screens: &crate::outputs::Known) -> Vec<Effect> {
    with(|slot| slot.as_mut().map(|lock| surfaces(lock, screens))).unwrap_or_default()
}

/// The surfaces opened or closed for `screens`.
fn surfaces(lock: &mut Lock, screens: &crate::outputs::Known) -> Vec<Effect> {
    let mut effects = Vec::new();
    let names: Vec<String> = match screens {
        crate::outputs::Known::Names(names) => names.clone(),
        crate::outputs::Known::Unknowable => vec![crate::ALONE.to_owned()],
    };
    let gone: Vec<String> = lock
        .surfaces
        .keys()
        .filter(|n| !names.contains(n))
        .cloned()
        .collect();
    for name in gone {
        if let Some(id) = lock.surfaces.remove(&name) {
            effects.push(Effect::Remove(id));
        }
    }
    for name in names {
        if lock.surfaces.contains_key(&name) {
            continue;
        }
        let id = window::Id::unique();
        tracing::debug!(output = name, "a lock surface");
        lock.surfaces.insert(name.clone(), id);
        effects.push(Effect::LockSurface { id, output: name });
    }
    effects
}

/// The compositor's `unlocked`: the lock's surfaces go, what waited goes
/// on.
pub fn end() -> Vec<Effect> {
    let Some(lock) = with(Option::take) else {
        return Vec::new();
    };
    tracing::info!("the session is unlocked: the lock goes");
    crate::notifications::locked(false);
    let mut effects: Vec<Effect> = lock.surfaces.into_values().map(Effect::Remove).collect();
    // The bar's clock waited: it draws again.
    effects.push(Effect::Wake(hocus_link::CLOCK_NAMESPACE));
    effects
}

/// The lock's words, in the shell's language (docs/design/lock.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    pub fn password(self) -> &'static str {
        self.pick("Mot de passe", "Password")
    }

    /// The arrow's name, for a screen reader (11, accessibility).
    #[allow(dead_code)]
    pub fn unlock(self) -> &'static str {
        self.pick("Déverrouiller", "Unlock")
    }

    pub fn caps(self) -> &'static str {
        self.pick("Majuscules verrouillées", "Caps Lock is on")
    }

    pub fn checking(self) -> &'static str {
        self.pick("Vérification…", "Checking…")
    }

    /// "Mot de passe incorrect.", with Caps Lock's sentence after it when
    /// it is on.
    pub fn wrong(self, caps: bool) -> String {
        let wrong = self.pick("Mot de passe incorrect.", "Wrong password.");
        if caps {
            format!(
                "{wrong} {}",
                self.pick("Les majuscules sont verrouillées.", "Caps Lock is on.")
            )
        } else {
            wrong.to_owned()
        }
    }

    pub fn wait(self, seconds: u32) -> String {
        if self.fr {
            format!("Trop d’essais. Réessaie dans {seconds} s.")
        } else {
            format!("Too many attempts. Try again in {seconds} s.")
        }
    }

    /// The agents waiting for the user: a count, no name.
    pub fn agents(self, count: u32) -> String {
        match (count, self.fr) {
            (1, true) => "1 agent t’attend".to_owned(),
            (n, true) => format!("{n} agents t’attendent"),
            (1, false) => "1 agent is waiting for you".to_owned(),
            (n, false) => format!("{n} agents are waiting for you"),
        }
    }

    /// The layout's button's name, for a screen reader (11,
    /// accessibility).
    #[allow(dead_code)]
    pub fn layout(self, name: &str) -> String {
        if self.fr {
            format!("Disposition du clavier : {name}. Changer")
        } else {
            format!("Keyboard layout: {name}. Change")
        }
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// What the pointer did on lock surface `id`: whether to draw again.
pub fn pointer(id: window::Id, event: Pointing) -> bool {
    let (draw, ask) = with(|slot| {
        let Some(lock) = slot.as_mut() else {
            return (false, None);
        };
        match event {
            Pointing::Over(target) => {
                let over = target.map(|t| (id, t));
                if lock.hover == over {
                    return (false, None);
                }
                lock.hover = over;
                (true, None)
            }
            Pointing::Press(target) => {
                lock.pressed = target.map(|t| (id, t));
                (false, None)
            }
            Pointing::Release(target) => {
                let clicked = lock
                    .pressed
                    .take()
                    .filter(|(on, t)| *on == id && Some(*t) == target)
                    .map(|(_, t)| t);
                let ask = match clicked {
                    Some(Target::Button) => Some(ToCompositor::Lock(LockAsk::Submit)),
                    Some(Target::Field) => {
                        Some(ToCompositor::Lock(LockAsk::Focus(LockFocus::Field)))
                    }
                    Some(Target::Layout) => Some(ToCompositor::NextLayout),
                    None => None,
                };
                (false, ask)
            }
        }
    });
    if let Some(ask) = ask {
        tracing::debug!(%ask, "the lock's click");
        crate::popover::tell(ask);
    }
    draw
}

/// What the pointer does on a lock surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointing {
    Over(Option<Target>),
    Press(Option<Target>),
    Release(Option<Target>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_words() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.password(), "Mot de passe");
        assert_eq!(en.unlock(), "Unlock");
        assert_eq!(fr.caps(), "Majuscules verrouillées");
        assert_eq!(en.checking(), "Checking…");
        assert_eq!(fr.wrong(false), "Mot de passe incorrect.");
        assert_eq!(
            fr.wrong(true),
            "Mot de passe incorrect. Les majuscules sont verrouillées."
        );
        assert_eq!(en.wrong(true), "Wrong password. Caps Lock is on.");
        assert_eq!(fr.wait(30), "Trop d’essais. Réessaie dans 30 s.");
        assert_eq!(en.wait(7), "Too many attempts. Try again in 7 s.");
        assert_eq!(fr.agents(2), "2 agents t’attendent");
        assert_eq!(en.agents(1), "1 agent is waiting for you");
        assert_eq!(
            fr.layout("français"),
            "Disposition du clavier : français. Changer"
        );
    }

    #[test]
    fn a_surface_a_screen_new_ones_get_theirs() {
        let shown = LockShown {
            phase: LockPhase::Idle,
            chars: 0,
            caps: false,
            shake: 0,
            focus: LockFocus::Field,
            agents: 0,
            wallpaper: true,
            motion: false,
            output: "eDP-1".into(),
        };
        let mut lock = Lock {
            shown,
            surfaces: HashMap::new(),
            shake: None,
            checking: None,
            hover: None,
            pressed: None,
        };
        let two = crate::outputs::Known::Names(vec!["eDP-1".into(), "HDMI-A-1".into()]);
        let opened = surfaces(&mut lock, &two);
        assert_eq!(opened.len(), 2);
        assert!(surfaces(&mut lock, &two).is_empty());
        let one = crate::outputs::Known::Names(vec!["eDP-1".into()]);
        assert_eq!(surfaces(&mut lock, &one).len(), 1, "the other's goes");
        assert_eq!(lock.surfaces.len(), 1);
    }
}
