//! The lock screen (7.1, variant A, docs/design/lock.md): one surface on
//! each screen, over everything, while the compositor says the session is
//! locked (`lock …`), until it says it opened (`unlocked`).
//!
//! The compositor keeps the keyboard and the password; this draws what it
//! says: the phase, how many dots, Caps Lock, where `Tab` is, how many
//! agents wait. The pointer's clicks on the arrow, the field and the
//! layout's button, the kit's controls ([`view`]), go back to it (`lock
//! submit`, `lock focus`, `next-layout`). The screen the compositor names shows the form, the
//! status row and the sleeping pandas; the others the clock alone.
//!
//! At rest a lock surface draws once a minute, as the minute changes; it
//! draws every frame only while the field shakes or the spinner turns.
//! Meanwhile the bar's clock draws nothing, the bubbles wait.

pub mod avatar;
pub mod draw;
pub mod view;
pub mod wallpaper;

use hocus_words::shell::lock as txt;
use std::{collections::HashMap, time::Instant};

use hocus_link::{LOCK_NAMESPACE, LockPhase, LockShown};
use iced::{Element, Size, window};

use crate::{
    clock::time::Lang,
    outputs::Known,
    popover::Effect,
    surfaces::{Context, Lent, Surface},
};

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
}

/// The lock while it shows, the shell's own (#773), and what its
/// surfaces keep from a view to the next.
#[derive(Debug, Default)]
pub struct Locked {
    lock: Option<Lock>,
    /// Each lock surface's size, as it was laid out: its controls are
    /// placed from it.
    sizes: HashMap<window::Id, Size>,
    /// What each screen's lock last showed, as the log says it.
    said: HashMap<String, draw::Said>,
}

/// What changes the lock.
#[derive(Debug, Clone)]
pub enum Told {
    /// The compositor's `lock …`: the lock opens on every screen `screens`
    /// names (or where the compositor puts it, when they cannot be
    /// known), or shows its new state.
    Shown(LockShown, Known),
    /// The screens changed while locked: a new one gets the clock, one
    /// gone takes its surface with it.
    Screens(Known),
    /// The compositor's `unlocked`: the lock's surfaces go, what waited
    /// goes on.
    End,
}

impl Locked {
    /// Whether the session is locked: the bar's clock and the bubbles wait.
    pub fn active(&self) -> bool {
        self.lock.is_some()
    }

    /// The screen surface `id` is the lock's on, if it is one.
    pub fn output_of(&self, id: window::Id) -> Option<String> {
        self.lock
            .as_ref()?
            .surfaces
            .iter()
            .find(|(_, s)| **s == id)
            .map(|(o, _)| o.clone())
    }

    /// `told` taken in at `now`: what the shell then does.
    pub fn update(&mut self, told: Told, now: Instant) -> Vec<Effect> {
        let effects = match told {
            Told::Shown(shown, screens) => self.shown(shown, &screens, now),
            Told::Screens(screens) => self
                .lock
                .as_mut()
                .map(|lock| surfaces(lock, &screens))
                .unwrap_or_default(),
            Told::End => self.end(),
        };
        crate::a11y::shown(crate::a11y::Shown::Lock(self.lock.clone()));
        effects
    }

    fn shown(&mut self, shown: LockShown, screens: &Known, now: Instant) -> Vec<Effect> {
        let mut opened = false;
        let lock = self.lock.get_or_insert_with(|| {
            opened = true;
            Lock {
                shown: shown.clone(),
                surfaces: HashMap::new(),
                shake: None,
                checking: None,
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
        // Its controls are built again for what it now shows.
        crate::ui::touch();
        let mut effects = surfaces(lock, screens);
        effects.push(Effect::Wake(LOCK_NAMESPACE));
        let surfaces: Vec<window::Id> = lock.surfaces.values().copied().collect();
        self.sizes.retain(|id, _| surfaces.contains(id));
        if opened {
            tracing::info!("the session is locked: the lock shows");
            // Nothing else of the shell stays open over it.
            crate::popover::event(crate::popover::Event::PressElsewhere);
            crate::launcher::close();
            crate::emoji::close();
            crate::actions::close();
            crate::notifications::locked(true);
        }
        effects
    }

    fn end(&mut self) -> Vec<Effect> {
        let Some(lock) = self.lock.take() else {
            return Vec::new();
        };
        self.sizes.clear();
        tracing::info!("the session is unlocked: the lock goes");
        crate::notifications::locked(false);
        let mut effects: Vec<Effect> = lock.surfaces.into_values().map(Effect::Remove).collect();
        // The bar's clock waited: it draws again.
        effects.push(Effect::Wake(hocus_link::CLOCK_NAMESPACE));
        effects
    }
}

impl Surface for Lent<Locked> {
    type Message = Told;

    fn update(&mut self, told: Told, now: Instant) -> Vec<Effect> {
        self.get_mut().update(told, now)
    }

    fn view(&self, id: window::Id, context: &Context) -> Option<Element<'_, crate::Message>> {
        let output = self.read(|l| l.output_of(id))??;
        Some(view::view(
            self,
            id,
            output,
            context.wall.clone(),
            context.lang,
            words(),
        ))
    }
}

/// The surfaces opened or closed for `screens`.
fn surfaces(lock: &mut Lock, screens: &Known) -> Vec<Effect> {
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

/// The lock's words, in the shell's language (docs/design/lock.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    pub fn password(self) -> &'static str {
        self.pick(txt::PASSWORD)
    }

    /// The arrow's name, for a screen reader (11, accessibility).
    pub fn unlock(self) -> &'static str {
        self.pick(txt::UNLOCK)
    }

    pub fn caps(self) -> &'static str {
        self.pick(txt::CAPS_LOCK_IS_ON)
    }

    pub fn checking(self) -> &'static str {
        self.pick(txt::CHECKING)
    }

    /// "Mot de passe incorrect.", with Caps Lock's sentence after it when
    /// it is on.
    pub fn wrong(self, caps: bool) -> String {
        let wrong = self.pick(txt::WRONG_PASSWORD);
        if caps {
            format!("{wrong} {}", self.pick(txt::CAPS_LOCK_IS_ON_2))
        } else {
            wrong.to_owned()
        }
    }

    pub fn wait(self, seconds: u32) -> String {
        txt::too_many_attempts_try_again_in(self.lang, seconds)
    }

    /// The agents waiting for the user: a count, no name.
    pub fn agents(self, count: u32) -> String {
        match count {
            1 => txt::N_1_AGENT_IS_WAITING_FOR_YOU.get(self.lang).to_owned(),
            n => txt::agents_are_waiting_for_you(self.lang, n),
        }
    }

    /// The layout's button's name, for a screen reader (11,
    /// accessibility).
    pub fn layout(self, name: &str) -> String {
        txt::keyboard_layout_change(self.lang, name)
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_link::LockFocus;

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

    fn shown(phase: LockPhase, shake: u32) -> LockShown {
        LockShown {
            phase,
            chars: 3,
            caps: false,
            shake,
            focus: LockFocus::Field,
            agents: 0,
            wallpaper: true,
            motion: true,
            output: "eDP-1".into(),
        }
    }

    /// The lock alone (#773): told, it opens a surface a screen; a wrong
    /// password shakes it and its words say so; a screen gone takes its
    /// surface; unlocked, they all go and the bar's clock draws again.
    #[test]
    fn the_lock_alone_opens_shakes_follows_the_screens_and_goes() {
        let mut locked = Locked::default();
        let now = Instant::now();
        let two = Known::Names(vec!["eDP-1".into(), "HDMI-A-1".into()]);
        let effects = locked.update(Told::Shown(shown(LockPhase::Idle, 0), two), now);
        let opened: Vec<window::Id> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::LockSurface { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(opened.len(), 2);
        assert!(effects.contains(&Effect::Wake(LOCK_NAMESPACE)));
        assert!(locked.active());
        let on: Vec<String> = opened
            .iter()
            .filter_map(|id| locked.output_of(*id))
            .collect();
        assert!(on.contains(&"eDP-1".to_owned()) && on.contains(&"HDMI-A-1".to_owned()));
        let words = Words::new(Lang::English);
        let one = Known::Names(vec!["eDP-1".into()]);
        locked.update(Told::Shown(shown(LockPhase::Wrong, 1), one.clone()), now);
        let lock = locked.lock.as_ref().expect("locked");
        assert_eq!(lock.shake.map(|(n, _)| n), Some(1));
        assert_eq!(draw::message(lock, words).0, "Wrong password.");
        assert_eq!(lock.surfaces.len(), 1, "the screen gone took its surface");
        let effects = locked.update(Told::End, now);
        assert!(matches!(
            effects.as_slice(),
            [Effect::Remove(_), Effect::Wake(hocus_link::CLOCK_NAMESPACE)]
        ));
        assert!(!locked.active());
        assert!(locked.update(Told::Screens(one), now).is_empty());
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
