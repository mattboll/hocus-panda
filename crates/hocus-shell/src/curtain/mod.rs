//! The curtain's pill (7.2, variant A, docs/design/idle.md): while the
//! compositor dims the screens before the lock (`curtain …`), a pill on
//! the screen it names says how long is left, and how to stay; a panda
//! falls asleep in it. It goes at `curtain-end`.
//!
//! The compositor draws the dim, the pill's shadow and its motion in and
//! out; this draws the pill itself, its countdown at most ten times a
//! second, nothing once it is gone.

pub mod draw;

use hocus_words::shell::curtain as txt;
use std::time::{Duration, Instant};

use hocus_link::{CURTAIN_NAMESPACE, CurtainShown};
use iced::{Element, Length, widget::canvas, window};

use crate::{
    clock::time::Lang,
    popover::Effect,
    surfaces::{Context, Surface},
};

/// The pill as the shell shows it.
#[derive(Debug, Clone)]
pub struct Pill {
    pub shown: CurtainShown,
    /// When the compositor said it: the countdown runs from there.
    pub told: Instant,
    pub surface: window::Id,
}

impl Pill {
    /// How long it has been up, at `now`.
    pub fn elapsed(&self, now: Instant) -> Duration {
        Duration::from_millis(u64::from(self.shown.elapsed))
            + now.saturating_duration_since(self.told)
    }

    pub fn length(&self) -> Duration {
        Duration::from_millis(u64::from(self.shown.length))
    }

    /// Its share gone, from 0 to 1.
    pub fn progress(&self, now: Instant) -> f32 {
        let length = self.length().as_secs_f32();
        if length <= 0.0 {
            return 1.0;
        }
        (self.elapsed(now).as_secs_f32() / length).clamp(0.0, 1.0)
    }

    /// The whole seconds left, at least 1.
    pub fn seconds_left(&self, now: Instant) -> u32 {
        let left = self.length().saturating_sub(self.elapsed(now));
        let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
        u32::try_from(seconds.max(1)).unwrap_or(u32::MAX)
    }
}

/// The curtain's pill, while one shows: the shell's own (#773).
#[derive(Debug, Default)]
pub struct Curtain {
    pill: Option<Pill>,
}

/// What the compositor says of the curtain.
#[derive(Debug, Clone)]
pub enum Said {
    /// `curtain …`: the pill opens on the screen it names (a new one if
    /// that changed), its countdown from what it says.
    Shown(CurtainShown),
    /// `curtain-end`: the pill goes.
    End,
}

impl Curtain {
    /// The pill on surface `id`, if it is the pill's.
    pub fn pill_on(&self, id: window::Id) -> Option<&Pill> {
        self.pill.as_ref().filter(|p| p.surface == id)
    }

    /// What the pill's canvas draws on surface `id`.
    pub fn program(&self, id: window::Id, glass: bool) -> Option<draw::View> {
        Some(draw::View {
            pill: self.pill_on(id)?.clone(),
            words: words(),
            glass,
        })
    }

    fn shown(&mut self, shown: CurtainShown, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        let same = self
            .pill
            .as_ref()
            .is_some_and(|p| p.shown.output == shown.output && p.shown.top == shown.top);
        if !same {
            if let Some(old) = self.pill.take() {
                effects.push(Effect::Remove(old.surface));
            }
            let surface = window::Id::unique();
            tracing::debug!(output = shown.output, "the curtain's pill");
            effects.push(Effect::CurtainSurface {
                id: surface,
                output: shown.output.clone(),
                top: shown.top,
            });
            self.pill = Some(Pill {
                shown,
                told: now,
                surface,
            });
        } else if let Some(pill) = self.pill.as_mut() {
            pill.shown = shown;
            pill.told = now;
        }
        effects.push(Effect::Wake(CURTAIN_NAMESPACE));
        effects
    }
}

impl Surface for Curtain {
    type Message = Said;

    fn update(&mut self, message: Said, now: Instant) -> Vec<Effect> {
        match message {
            Said::Shown(shown) => self.shown(shown, now),
            Said::End => self
                .pill
                .take()
                .map(|pill| vec![Effect::Remove(pill.surface)])
                .unwrap_or_default(),
        }
    }

    fn view(&self, id: window::Id, context: &Context) -> Option<Element<'_, crate::Message>> {
        Some(
            canvas(self.program(id, context.look.glass)?)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }
}

/// The pill's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    /// "Verrouillage dans 7 s".
    pub fn locking_in(self, seconds: u32) -> String {
        txt::locking_in_s(self.lang, seconds)
    }

    pub fn stay(self) -> &'static str {
        txt::MOVE_THE_MOUSE_TO_STAY.get(self.lang)
    }

    /// Its name for a screen reader, said once as it shows (11,
    /// accessibility).
    #[allow(dead_code)]
    pub fn name(self, seconds: u32) -> String {
        txt::locking_in_seconds_move_the_mouse(self.lang, seconds)
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(elapsed: u32) -> CurtainShown {
        CurtainShown {
            elapsed,
            length: 10_000,
            top: 1010,
            motion: true,
            output: "eDP-1".into(),
        }
    }

    #[test]
    fn its_words() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.locking_in(7), "Verrouillage dans 7 s");
        assert_eq!(en.locking_in(1), "Locking in 1 s");
        assert_eq!(fr.stay(), "Bouge la souris pour rester");
        assert_eq!(en.stay(), "Move the mouse to stay");
        assert_eq!(en.name(7), "Locking in 7 seconds. Move the mouse to stay.");
    }

    #[test]
    fn the_countdown_from_what_the_compositor_said() {
        let told = Instant::now();
        let pill = Pill {
            shown: shown(2_500),
            told,
            surface: window::Id::unique(),
        };
        assert_eq!(pill.seconds_left(told), 8);
        assert_eq!(pill.seconds_left(told + Duration::from_millis(500)), 7);
        assert_eq!(pill.seconds_left(told + Duration::from_secs(30)), 1);
        assert!((pill.progress(told) - 0.25).abs() < 1e-6);
    }

    /// The surface alone (#773): told, it opens its pill, keeps it while
    /// the screen is the same, draws it on its surface only, and lets it go.
    #[test]
    fn the_pill_alone_opens_counts_and_goes() {
        let mut curtain = Curtain::default();
        let now = Instant::now();
        let effects = curtain.update(Said::Shown(shown(0)), now);
        let Some(Effect::CurtainSurface { id, output, top }) = effects.first() else {
            panic!("a surface opened first: {effects:?}");
        };
        let id = *id;
        assert_eq!((output.as_str(), *top), ("eDP-1", 1010));
        assert!(matches!(
            effects.last(),
            Some(Effect::Wake(CURTAIN_NAMESPACE))
        ));
        let drawn = curtain.program(id, true).expect("drawn on its surface");
        assert_eq!(drawn.pill.seconds_left(now), 10);
        assert!(curtain.program(window::Id::unique(), true).is_none());
        // Told again on the same screen: the same surface, the countdown
        // from the new word.
        let later = now + Duration::from_secs(1);
        let effects = curtain.update(Said::Shown(shown(4_000)), later);
        assert!(matches!(effects.as_slice(), [Effect::Wake(_)]));
        assert_eq!(curtain.pill_on(id).map(|p| p.seconds_left(later)), Some(6));
        let effects = curtain.update(Said::End, later);
        assert!(matches!(effects.as_slice(), [Effect::Remove(gone)] if *gone == id));
        assert!(curtain.pill_on(id).is_none());
        assert!(curtain.update(Said::End, later).is_empty());
    }
}
