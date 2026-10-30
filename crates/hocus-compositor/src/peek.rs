//! The panda peeking out of the bar's edge (docs/design/bar.md §7 C, 4.5):
//! when an agent starts waiting, or finishes, its panda (96 px) comes out
//! from under the bar's edge, at its row's height, with a bubble saying
//! who and why; it stays still for 8 s, then goes back in. It comes out
//! once more with a waiting agent's 5-minute reminder (the halo's timer,
//! [`crate::halo`]), never for a stuck agent. A click on it goes to the
//! agent (4.3), and it goes back at once. The latest replaces the one out.
//!
//! The halo says what began ([`crate::halo::Halo::take_news`]); the
//! compositor tells the shell (`peek`), which draws the panda and its
//! bubble once on a layer surface of its own beside each bar
//! ([`PEEK_NAMESPACE`]), taking the pointer only where it draws. The
//! compositor moves it, as it does the popovers: from that surface's first
//! frame it slides out in 320 ms and fades in over 260 ms, from under the
//! bar (cut at the bar's edge, under the bar's surfaces and its halo), and
//! back in 260 ms, fading out over 220 ms; with reduced motion, or a shell
//! on the CPU, it only fades in and out in place. Its two moments are a
//! timer of the loop each, never a poll; then the shell is told that it is
//! back (`no-peek`) and removes its surfaces: nothing is drawn at rest.

use std::time::Duration;

use hocus_link::{PeekKind, ToShell};
use smithay::{
    backend::renderer::utils::with_renderer_surface_state,
    desktop::LayerSurface,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
};

use crate::{
    State,
    halo::Concern,
    motion::{Bezier, Tween},
};

/// It comes out in this time along this curve (a slight overshoot),
/// fading in over [`FADE_IN`], CSS's `ease-out`.
pub const OUT: Duration = Duration::from_millis(320);
pub const OUT_CURVE: Bezier = Bezier(0.2, 0.9, 0.25, 1.15);
pub const FADE_IN: Duration = Duration::from_millis(260);
/// It stays out this long, still.
pub const STAYS: Duration = Duration::from_secs(8);
/// It goes back in this time along this curve, fading out over
/// [`FADE_OUT`], CSS's `ease-in`.
pub const BACK: Duration = Duration::from_millis(260);
pub const BACK_CURVE: Bezier = Bezier(0.5, 0.0, 0.75, 0.3);
pub const FADE_OUT: Duration = Duration::from_millis(220);
/// Where it is when in: this far toward the bar, logical pixels.
pub const SLIDE: f64 = 150.0;
/// On the CPU, its surface is a card (no alpha): its corners' radius, the
/// bubble's.
pub const CARD_RADIUS: f64 = 12.0;

/// The panda out, or on its way.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Out {
    id: u64,
    kind: PeekKind,
    /// From its surface's first frame, on the compositor's clock.
    since: Option<Duration>,
    /// When it goes back.
    stays: Option<Duration>,
    /// Since when it goes back.
    back: Option<Duration>,
}

/// The peek's state, on the loop.
#[derive(Debug, Default)]
pub struct Peeks {
    out: Option<Out>,
    /// The shell's peek surfaces drawn so far, and still there.
    layers: Vec<LayerSurface>,
    /// The next moment's timer, and when it fires.
    timer: Option<(Duration, RegistrationToken)>,
    /// Peeks told the shell so far, for tests.
    pub told: u64,
}

/// How the peek is drawn at a moment: how opaque, and how far toward the
/// bar from where it is (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub alpha: f32,
    pub shift: f64,
}

/// How a peek out since `since` (and going back since `back`) is drawn at
/// `now`. Going back, it starts from where it is: clicked on its way out,
/// it does not jump.
pub fn look(since: Duration, back: Option<Duration>, now: Duration, motion: bool) -> Look {
    let out = |at: Duration| {
        let place = if motion {
            Tween::new(since, OUT, OUT_CURVE).at(at)
        } else {
            1.0
        };
        (place, Tween::new(since, FADE_IN, Bezier::EASE_OUT).at(at))
    };
    let (place, alpha) = match back {
        None => out(now),
        Some(back) => {
            let (place, alpha) = out(back);
            let place = if motion {
                place * (1.0 - Tween::new(back, BACK, BACK_CURVE).at(now))
            } else {
                place
            };
            let alpha = alpha * (1.0 - Tween::new(back, FADE_OUT, Bezier::EASE_IN).at(now));
            (place, alpha)
        }
    };
    Look {
        alpha: alpha.clamp(0.0, 1.0) as f32,
        shift: SLIDE * (1.0 - place),
    }
}

impl Out {
    fn moving(&self, now: Duration) -> bool {
        match (self.since, self.back) {
            (_, Some(back)) => now < back + BACK.max(FADE_OUT),
            (Some(since), None) => now < since + OUT.max(FADE_IN),
            (None, None) => false,
        }
    }

    fn look(&self, now: Duration, motion: bool) -> Option<Look> {
        let look = look(self.since?, self.back, now, motion);
        (look.alpha > 0.0).then_some(look)
    }
}

impl Peeks {
    /// Which agent's panda is out, and why.
    pub fn out(&self) -> Option<(u64, PeekKind)> {
        self.out.map(|o| (o.id, o.kind))
    }

    /// Whether it is going back.
    pub fn going_back(&self) -> bool {
        self.out.is_some_and(|o| o.back.is_some())
    }
}

/// The halo's concern for why a panda peeks out.
fn concern(kind: PeekKind) -> Concern {
    match kind {
        PeekKind::Waiting => Concern::Waiting,
        PeekKind::Done => Concern::ToSee,
    }
}

impl State {
    /// Whether the peek slides (else it fades in place): neither reduced
    /// motion nor a shell on the CPU, as the popovers.
    pub fn peek_motion(&self) -> bool {
        self.popover_motion()
    }

    /// What the halo says began: the latest of it peeks out.
    pub(crate) fn peek_news(&mut self) {
        let news = self.halo.take_news();
        let latest = news
            .into_iter()
            .rev()
            .find_map(|(id, concern)| match concern {
                Concern::Waiting => Some((id, PeekKind::Waiting)),
                Concern::ToSee => Some((id, PeekKind::Done)),
                _ => None,
            });
        if let Some((id, kind)) = latest {
            self.peek_out(id, kind);
        }
    }

    /// Agent `id`'s panda peeks out for `kind`: the shell draws it, in
    /// place of the one out if one is.
    fn peek_out(&mut self, id: u64, kind: PeekKind) {
        if !self.shell_linked() {
            return;
        }
        let Some(agent) = self
            .agents
            .told
            .as_ref()
            .and_then(|told| told.iter().find(|a| a.id == id))
            .cloned()
        else {
            return;
        };
        let now = self.now();
        // One out and staying: the new one takes its place as it is, for
        // another 8 s; else it comes out from its surface's next frame.
        let showing = self
            .peek
            .out
            .and_then(|o| o.since.filter(|_| o.back.is_none()));
        tracing::debug!(id, kind = kind.name(), "the panda peeks out");
        self.peek.out = Some(Out {
            id,
            kind,
            since: showing,
            stays: showing.map(|_| now + STAYS),
            back: None,
        });
        self.peek.told += 1;
        self.tell_shell(ToShell::Peek(hocus_link::Peek {
            id,
            kind,
            since: agent.since,
            name: agent.name,
        }));
        self.arm_peek();
        self.redraw = true;
    }

    /// A commit of the shell's peek surface `layer`: its first frame
    /// starts the peek's motion, and its 8 s.
    pub fn peek_committed(&mut self, layer: &LayerSurface) {
        let drawn = with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some())
            .unwrap_or(false);
        if !drawn {
            return;
        }
        if !self.peek.layers.contains(layer) {
            self.peek.layers.push(layer.clone());
        }
        let now = self.now();
        if let Some(out) = &mut self.peek.out
            && out.since.is_none()
            && out.back.is_none()
        {
            out.since = Some(now);
            out.stays = Some(now + STAYS);
            self.arm_peek();
        }
        self.redraw = true;
    }

    /// A layer surface went: if it was a peek's, it is no longer drawn.
    pub fn peek_gone(&mut self, layer: &LayerSurface) {
        self.peek.layers.retain(|l| l != layer);
    }

    /// The user went to agent `id` (its row, the peek itself): its panda
    /// goes back at once (§8 step 5).
    pub(crate) fn peek_gone_to(&mut self, id: u64) {
        if self.peek.out.is_some_and(|o| o.id == id) {
            self.peek_back();
        }
    }

    /// After the agents changed: a panda whose agent no longer waits, or
    /// was seen, or that the user acted on, goes back.
    pub(crate) fn peek_follows(&mut self) {
        if let Some(out) = self.peek.out
            && out.back.is_none()
            && !self.halo.counts(out.id, concern(out.kind))
        {
            tracing::debug!(id = out.id, "the agent was dealt with: the panda goes back");
            self.peek_back();
        }
    }

    /// The panda goes back in now; not shown yet, it is gone at once.
    fn peek_back(&mut self) {
        let now = self.now();
        let Some(out) = self.peek.out.filter(|o| o.back.is_none()) else {
            return;
        };
        match out.since {
            Some(since) => {
                self.peek.out = Some(Out {
                    back: Some(now.max(since)),
                    ..out
                });
                self.arm_peek();
                self.redraw = true;
            }
            None => self.peek_end(),
        }
    }

    /// The panda is back: the shell removes its surfaces.
    fn peek_end(&mut self) {
        if self.peek.out.take().is_some() {
            tracing::debug!("the panda is back in the bar");
            self.tell_shell(ToShell::NoPeek);
            self.redraw = true;
        }
        self.arm_peek();
    }

    /// The shell went: its surfaces with it.
    pub(crate) fn peek_shell_gone(&mut self) {
        self.peek.out = None;
        self.peek.layers.clear();
        self.arm_peek();
    }

    /// When the peek's next moment is: going back, then back.
    fn peek_next(&self) -> Option<Duration> {
        let out = self.peek.out?;
        match out.back {
            Some(back) => Some(back + BACK.max(FADE_OUT)),
            None => out.stays,
        }
    }

    /// The timer for the peek's next moment, if it is not the one set.
    fn arm_peek(&mut self) {
        let next = self.peek_next();
        if self.peek.timer.as_ref().map(|(at, _)| *at) == next {
            return;
        }
        if let Some((_, token)) = self.peek.timer.take() {
            self.loop_handle.remove(token);
        }
        let Some(at) = next else {
            return;
        };
        let timer = Timer::from_duration(at.saturating_sub(self.now()));
        let inserted = self.loop_handle.insert_source(timer, |_, _, app| {
            app.state.peek.timer = None;
            app.state.peek_due();
            TimeoutAction::Drop
        });
        match inserted {
            Ok(token) => self.peek.timer = Some((at, token)),
            Err(err) => tracing::warn!(%err.error, "no timer for the peek"),
        }
    }

    /// The peek's moment came: it goes back, or it is back.
    fn peek_due(&mut self) {
        // A timer may fire a hair early.
        let by = self.now() + Duration::from_millis(1);
        match self.peek.out {
            Some(out) if out.back.is_none() && out.stays.is_some_and(|at| at <= by) => {
                self.peek_back();
            }
            Some(out) if out.back.is_some_and(|b| b + BACK.max(FADE_OUT) <= by) => {
                self.peek_end();
            }
            _ => self.arm_peek(),
        }
    }

    /// The peek moves on; whether it still does.
    pub fn tick_peek(&self, now: Duration) -> bool {
        self.peek.out.is_some_and(|o| o.moving(now))
    }

    /// How the peek surface `layer` is drawn at `now`; `None`: not.
    pub fn peek_look(&self, layer: &LayerSurface, now: Duration) -> Option<Look> {
        if !self.peek.layers.contains(layer) {
            return None;
        }
        self.peek.out?.look(now, self.peek_motion())
    }

    /// How the peek is drawn at `now`, wherever it is; `None`: not drawn.
    pub fn peek_at(&self, now: Duration) -> Option<Look> {
        if self.peek.layers.is_empty() {
            return None;
        }
        self.peek.out?.look(now, self.peek_motion())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn it_slides_out_from_under_the_bar_then_back_in() {
        let at = |now, back| look(ms(1000), back, ms(now), true);
        // In at its first frame: transparent, under the bar.
        assert_eq!(
            at(1000, None),
            Look {
                alpha: 0.0,
                shift: SLIDE
            }
        );
        // Quick out, overshooting a little before it lands at 320 ms.
        let early = at(1080, None);
        assert!(early.shift < SLIDE / 2.0, "{early:?}");
        let over = (1100..1320)
            .map(|t| at(t, None).shift)
            .fold(f64::INFINITY, f64::min);
        assert!(over < -1.0 && over > -15.0, "{over}");
        // Faded in by 260 ms, still from 320 ms.
        assert_eq!(at(1260, None).alpha, 1.0);
        assert_eq!(
            at(1320, None),
            Look {
                alpha: 1.0,
                shift: 0.0
            }
        );
        // Back: slow first, under the bar by 260 ms, gone by 220 ms.
        let back = Some(ms(9320));
        let leaving = at(9400, back);
        assert!(
            leaving.shift < SLIDE / 4.0 && leaving.alpha > 0.3,
            "{leaving:?}"
        );
        assert_eq!(at(9540, back).alpha, 0.0);
        assert!((at(9580, back).shift - SLIDE).abs() < 1e-9);
    }

    #[test]
    fn clicked_on_its_way_out_it_goes_back_from_where_it_is() {
        let back = Some(ms(1100));
        let then = look(ms(1000), None, ms(1100), true);
        let start = look(ms(1000), back, ms(1100), true);
        assert!((start.shift - then.shift).abs() < 1e-9);
        assert!((start.alpha - then.alpha).abs() < 1e-6);
        assert!((look(ms(1000), back, ms(1360), true).shift - SLIDE).abs() < 1e-9);
    }

    #[test]
    fn without_motion_it_fades_in_and_out_in_place() {
        let at = |now, back| look(ms(0), back, ms(now), false);
        for t in [0, 100, 260, 5000] {
            assert_eq!(at(t, None).shift, 0.0);
        }
        assert_eq!(at(0, None).alpha, 0.0);
        let half = at(130, None).alpha;
        assert!(half > 0.5 && half < 1.0, "{half}");
        assert_eq!(at(260, None).alpha, 1.0);
        let back = Some(ms(8000));
        assert_eq!(at(8100, back).shift, 0.0);
        assert!(at(8100, back).alpha > 0.5);
        assert_eq!(at(8220, back).alpha, 0.0);
    }

    #[test]
    fn it_moves_only_on_its_way_out_and_back() {
        let out = Out {
            id: 1,
            kind: PeekKind::Waiting,
            since: Some(ms(0)),
            stays: Some(STAYS),
            back: None,
        };
        assert!(out.moving(ms(319)));
        assert!(!out.moving(ms(320)));
        assert!(!out.moving(ms(7999)));
        let back = Out {
            back: Some(STAYS),
            ..out
        };
        assert!(back.moving(STAYS + ms(259)));
        assert!(!back.moving(STAYS + ms(260)));
        let waiting = Out { since: None, ..out };
        assert!(!waiting.moving(ms(10)));
        assert_eq!(waiting.look(ms(10), true), None);
    }
}
