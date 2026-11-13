//! The desktop's effects, for the fun of it (#434, the foundation #793,
//! docs/design/effects.md): each off by default, its own switch in
//! `[effects]` and in Settings › Appearance › Effects.
//!
//! What they share: when they may play ([`State::effects_still`]), the
//! window pass that draws a window's picture through a shader of its own
//! ([`draw::Fire`]), the particles ([`particles`]) and the shapes drawn in
//! one call ([`mesh`]), the layer under the windows and the decorations on
//! their edges (`crate::render::snow`), and the windows' motion
//! ([`motion`]). Built with its first effects: closing a window in flames
//! ([`flames`], #794), snow that settles ([`snow`], #795); then the agents
//! as creatures on their windows ([`creatures`], #796), windows in jelly
//! ([`jelly`], #797), shattering into glass as they close
//! ([`shatter`], #798), and a garland under the active window
//! ([`garland`], #799), another across the top of the screens (#813);
//! on the wallpaper, rain on the glass ([`raindrops`], #802), a koi pond
//! ([`koi`], #803), evening fireflies ([`fireflies`], #804) and the
//! daylight ([`daylight`], #805), following the sun ([`sun`]);
//! desktops in depth ([`depth`], #806); and an aurora following the music
//! ([`aurora`], #801). The wallpaper's effects play behind the lock too,
//! and in the overview (#807). The moments ([`moments`]): fireworks as an
//! agent's tests go green ([`fireworks`], #810), confetti for its good
//! news ([`confetti`], #811).
//!
//! Every effect follows the same rules: none with reduced motion or in
//! the power saver, over a fullscreen window, while a screen or a window
//! is shared or the screen recorded; never in a capture, a share's or a
//! recording's frames; never on the CPU (pixman draws none: what would
//! have played plays as it does without the effect); nothing drawn per
//! frame once it is over.
//!
//! And none costs more than it shows (#815): those that play on their own
//! (the snow, the garlands, the creatures) move on at most 60 times a
//! second whatever the screens' refresh ([`Pace`]), what they draw
//! staying the same between; and each tells the screen only what of it
//! changed, so that what is still around it is not drawn again.

pub mod aurora;
pub mod confetti;
pub mod creatures;
pub mod daylight;
pub mod depth;
pub mod draw;
pub mod fireflies;
pub mod fireworks;
pub mod flames;
pub mod garland;
pub mod jelly;
pub mod kept;
pub mod koi;
pub mod mesh;
pub mod moments;
pub mod motion;
pub mod particles;
pub mod raindrops;
pub mod rig;
pub mod shatter;
pub mod snow;
pub mod snow_look;
pub mod snowing;
pub mod sun;
pub mod timing;

pub use draw::{Fire, FireElement};

use std::time::Duration;

/// What the effects playing now keep from frame to frame.
#[derive(Debug, Default)]
pub struct Live {
    /// The windows' motions, followed while an effect reacts to them.
    pub motions: motion::Motions,
    /// The snow, while it is on (#795).
    pub snow: Option<snow::Snow>,
    /// The snows started so far, for their seeds.
    snows: u64,
    /// The agents as creatures on their windows (#796).
    pub creatures: creatures::Creatures,
    /// The garland under the active window, while it shows (#799).
    pub garland: Option<garland::Garland>,
    /// The garlands across the tops of the screens, by screen, while
    /// they show (#813).
    pub tops: std::collections::HashMap<String, garland::Top>,
    /// The garlands hung so far, for their seeds.
    garlands: u64,
    /// The rain on the glass, while it is on (#802).
    pub raindrops: Option<raindrops::Rain>,
    /// The rains started so far, for their seeds.
    rains: u64,
    /// The fireflies, while they are out (#804).
    pub fireflies: Option<fireflies::Fireflies>,
    /// The swarms come out so far, for their seeds.
    swarms: u64,
    /// Whether it is evening, while the fireflies are on.
    pub dusk: fireflies::Dusk,
    /// The daylight, while it is on (#805).
    pub daylight: Option<daylight::Daylight>,
    /// Desktops in depth, while it is on (#806).
    pub depth: Option<depth::Depth>,
    /// The aurora, while it is on (#801).
    pub aurora: Option<aurora::Aurora>,
    /// Whether the bar's player plays, as the shell says (#801).
    pub music: bool,
    /// The koi's pond, while it is on (#803).
    pub koi: Option<koi::Ponds>,
    /// The ponds dug so far, for their seeds.
    ponds: u64,
    /// The fireworks, while they play (#810).
    pub fireworks: Option<fireworks::Show>,
    /// The confetti, while they fly or lie (#811).
    pub confetti: Option<confetti::Confetti>,
    /// The moments played so far, for their seeds.
    moments: u64,
    /// The sun looked at every minute, while an effect follows it.
    pub sun_timer: Option<smithay::reexports::calloop::RegistrationToken>,
    /// Tests: the sun as it is at this time, whatever the clock says.
    pub sun_held: Option<jiff::Timestamp>,
    /// When they move on next (#815).
    pub pace: Pace,
}

/// The effects' frames: at most this many a second.
pub const FRAMES: u32 = 60;
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / FRAMES as u64);
/// An effect's frame comes this much early at most, to fall on the
/// screen's frame next to it: at 60 Hz, one a refresh whatever its jitter.
const EARLY: Duration = Duration::from_millis(4);

/// When the effects that play on their own move on (#815): on a 60 Hz
/// grid, whatever the screens' refresh, its frames falling on theirs.
/// Between two, what they draw stays as it is (their elements keep their
/// commits): a screen at 165 Hz draws them again only where something
/// else changed.
#[derive(Debug, Default)]
pub struct Pace {
    /// The next due, while one moves; `None`: at the next turn.
    next: Option<Duration>,
    /// Their time: what they show is as of it.
    pub at: Duration,
    /// The clock's when they last moved on (`at` is the last frame's).
    pub clock: Duration,
    /// Their frames so far, for the tests.
    pub frames: u64,
}

impl Pace {
    /// Whether they move on at `now` (the clock's, not the last frame's).
    pub fn due(&self, now: Duration) -> bool {
        self.next.is_none_or(|next| now + EARLY >= next)
    }

    /// They moved on to `at` at `now`; `moving`: they go on moving, the
    /// next due a frame on, else at the next turn.
    pub fn moved(&mut self, now: Duration, at: Duration, moving: bool) {
        self.at = at;
        self.clock = now;
        self.next = moving.then(|| {
            // On the grid, but never so late that two come at once.
            let on = self.next.unwrap_or(now) + FRAME;
            on.max(now + FRAME - EARLY)
        });
        self.frames += u64::from(moving);
    }

    /// When they move on next, while they move.
    pub fn next(&self) -> Option<Duration> {
        self.next
    }
}

use crate::{State, model::Mode, model::WindowId};

/// Effect `fx`'s step, `step`, measured while their cost is (#815):
/// whether it moves.
pub(crate) fn timed(fx: timing::Fx, step: impl FnOnce() -> bool) -> bool {
    let measure = timing::tick(fx);
    let moving = step();
    drop(measure);
    timing::stepped(fx, moving);
    moving
}

/// Why the effects do not play now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Still {
    ReducedMotion,
    PowerSaver,
    /// A screen or a window is shared, or the screen recorded.
    Shown,
    /// The compositor draws on the CPU.
    Cpu,
}

/// How an effect that lasts shows now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plays {
    Moving,
    /// Drawn still (reduced motion): nothing drawn frame after frame.
    Still,
}

impl State {
    /// How an effect on the wallpaper that lasts, its switch `on`, shows
    /// now: moving, still with reduced motion; `None`, not at all (in the
    /// power saver, while a screen is shared or recorded, it waits; on the
    /// CPU there is none).
    pub(crate) fn wallpaper_effect(&self, on: bool) -> Option<Plays> {
        if !on {
            return None;
        }
        match self.effects_still() {
            None => Some(Plays::Moving),
            Some(Still::ReducedMotion) => Some(Plays::Still),
            Some(Still::PowerSaver | Still::Shown | Still::Cpu) => None,
        }
    }

    /// Whether the desktop's own lock shows the wallpaper live, drawn by
    /// the compositor under it with the wallpaper's effects (#807): one
    /// of them is on, the lock shows the wallpaper dimmed (not blurred,
    /// nor a picture of its own), on the GPU, and the shell draws on the
    /// GPU too (on the CPU its surfaces are opaque). Not the transient
    /// rules (the power saver, a share): then the still wallpaper is
    /// drawn.
    pub(crate) fn lock_live(&self) -> bool {
        let e = &self.config.effects;
        let any =
            e.snow || e.aurora || e.rain_on_glass || e.koi || e.fireflies || e.daylight || e.depth;
        any && self.config.wallpaper.lock == hocus_link::wallpaper::LockLook::Dimmed
            && !self.config.plain_background
            && self.graphics.blur
            && !self.shell.software
    }

    /// Whether the desktop's own lock shows now, the wallpaper live
    /// under it.
    pub(crate) fn lock_shows_wall(&self) -> bool {
        self.lock
            .locked_state()
            .is_some_and(|l| matches!(l.holder, crate::lock::Holder::Own))
            && self.lock_live()
    }

    /// The wallpaper's effects step on at `now`: the snow, the rain on the
    /// glass, the fireflies, the koi, the aurora, the daylight, desktops in
    /// depth; whether one moves.
    pub(crate) fn tick_wallpaper_effects(&mut self, now: std::time::Duration) -> bool {
        use timing::Fx;
        let mut moving = timed(Fx::Snow, || self.tick_snow(now));
        moving |= timed(Fx::Rain, || self.tick_raindrops(now));
        moving |= timed(Fx::Fireflies, || self.tick_fireflies(now));
        moving |= timed(Fx::Koi, || self.tick_koi(now));
        moving |= timed(Fx::Aurora, || self.tick_aurora(now));
        timed(Fx::Daylight, || {
            self.tick_daylight();
            false
        });
        moving |= timed(Fx::Depth, || self.tick_depth(now));
        moving
    }

    /// The effects that play on their own step on by `step` at `now`,
    /// when [`Pace`] says they are due (#815): whether they move.
    pub(crate) fn tick_paced(
        &mut self,
        now: Duration,
        step: impl FnOnce(&mut Self, Duration) -> bool,
    ) -> bool {
        let clock = self.now();
        if !self.fx.pace.due(clock) {
            return false;
        }
        let moving = step(self, now);
        self.fx.pace.moved(clock, now, moving);
        moving
    }

    /// Whether the wallpaper of `output` is as good as unseen: a
    /// fullscreen window over it, or windows and the bar covering 90 % of
    /// it or more (a maximised window), as a wallpaper that moves stops
    /// (#433); the rain on the glass, the koi, the fireflies and the
    /// aurora then wait (#815).
    pub(crate) fn wall_unseen(&self, output: &smithay::output::Output) -> bool {
        self.wall_hidden(output) || self.covered_with_bar(output) >= crate::moving::COVERED
    }

    /// Why no effect plays now; `None`: they may.
    pub(crate) fn effects_still(&self) -> Option<Still> {
        if self.reduced_motion() {
            Some(Still::ReducedMotion)
        } else if self.power_saving() {
            Some(Still::PowerSaver)
        } else if self.share.shares.values().any(|s| s.node.is_some()) || self.record.recording() {
            Some(Still::Shown)
        } else if !self.graphics.blur {
            Some(Still::Cpu)
        } else {
            None
        }
    }

    /// How window `id`, closing now, goes (#794, #798): in flames or in
    /// glass when one is on, the effects may play, and it is an
    /// application's window (not a dialog) that is not fullscreen nor
    /// under a fullscreen one. Both on, they take turns.
    pub(crate) fn close_effect(&mut self, id: WindowId) -> Option<Closing> {
        let (flames, shatter) = (self.config.effects.flames, self.config.effects.shatter);
        if !flames && !shatter {
            return None;
        }
        if let Some(still) = self.effects_still() {
            tracing::debug!(window = id.0, ?still, "closes as standard");
            return None;
        }
        let fullscreen = self.model.mode(id) == Mode::Fullscreen
            || self
                .output_of(id)
                .is_some_and(|o| self.fullscreen_in_front(&o));
        let dialog = self.model.parent(id).is_some();
        if fullscreen || dialog {
            tracing::debug!(window = id.0, fullscreen, dialog, "closes as standard");
            return None;
        }
        Some(if flames && shatter {
            self.departures.turns = self.departures.turns.wrapping_add(1);
            if self.departures.turns % 2 == 1 {
                Closing::Flames
            } else {
                Closing::Shatter
            }
        } else if flames {
            Closing::Flames
        } else {
            Closing::Shatter
        })
    }

    /// Whether window `id`, dragged now, goes in jelly (#797): the jelly
    /// is on, the effects may play, and no fullscreen window is in front
    /// on its screen.
    pub(crate) fn jellies(&self, id: WindowId) -> bool {
        self.config.effects.jelly
            && self.effects_still().is_none()
            && !self
                .output_of(id)
                .is_some_and(|o| self.fullscreen_in_front(&o))
    }
}

/// How a window closing goes, by an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    /// It burns (#794).
    Flames,
    /// It shatters into glass (#798).
    Shatter,
}
