//! Aurora following the music (#801, docs/design/effects.md): the
//! wallpaper is the board "Aurore en musique"'s night, three curtains of
//! aurora over a starry sky and two ridges of mountains, drawn by its
//! shader (`draw.rs`, `AURORA`) into a picture of 0.6 of the screen's
//! logical pixels, as the board's canvas was.
//!
//! While the bar's music block plays, the curtains ripple to the board's
//! drive: its swell (`0.35 + 0.65 ((sin 0.11 t + 1) / 2)^1.5`) and its
//! kick at 96 beats a minute. The shell reads no sound: it says whether
//! its player plays ([`hocus_link::ToCompositor::Music`]), and the beat is
//! the board's, not the song's. When the music stops the curtains calm:
//! their pace eases to nothing (their swell and kick with it), and they
//! stand still; nothing is drawn then frame after frame. The music again,
//! and they wake.
//!
//! Its time moves on with that pace, as of the effects' frames
//! ([`super::Pace`]): the picture is drawn again at most 60 times a
//! second, and only while it moves. Behind the bar's glass stands its
//! picture as it last stood still, so that the glass is not blurred again
//! at each frame.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::Duration,
};

use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    output::Output,
};

use super::Plays;
use crate::{effects::draw::AuroraLook, wobble::Kept};

/// The board's canvas: 0.6 of its pixels.
pub const RESOLUTION: f64 = 0.6;
/// Its picture's longest side at most (pixels), on the largest screens.
pub const LONGEST: f64 = 1600.0;
/// The board's tempo.
const BPM: f64 = 96.0;
/// The board's moment, where it starts (its still frame with reduced
/// motion).
const START: f64 = 12.0;
/// How long its pace takes to ease (seconds, to about two thirds): the
/// music stopping, it calms over some three and a half seconds; again, it
/// wakes in about one and a half.
const CALMING: f64 = 0.9;
const WAKING: f64 = 0.35;
/// Within this of its end, its pace is there: under it, it stands still.
const REST: f64 = 0.02;
/// Its time goes back by this much (seconds of music, some four and a half
/// hours): the shader's noise keeps its precision.
const WRAP: f64 = 16_384.0;
/// A turn of the loop longer than this moves it on by this much at most.
const LONGEST_STEP: Duration = Duration::from_millis(100);

/// What a picture shows: its look's key and its size (pixels).
type Drawn = ([u32; 3], (i32, i32));

/// A picture of the aurora, kept between frames.
#[derive(Debug)]
pub struct Picture {
    pub id: Id,
    pub kept: RefCell<Option<Kept>>,
    /// What it shows.
    pub drawn: Cell<Option<Drawn>>,
    pub commit: Cell<CommitCounter>,
}

impl Default for Picture {
    fn default() -> Self {
        Self {
            id: Id::new(),
            kept: RefCell::new(None),
            drawn: Cell::new(None),
            commit: Cell::new(CommitCounter::default()),
        }
    }
}

impl Picture {
    /// How many times it was drawn.
    pub fn draws(&self) -> usize {
        self.commit
            .get()
            .distance(Some(CommitCounter::default()))
            .unwrap_or(0)
    }
}

/// The aurora of one screen: as it moves, and as it last stood still
/// (behind the glass).
#[derive(Debug, Default)]
pub struct Pane {
    pub live: Picture,
    pub still: Picture,
}

/// Aurora following the music, while its switch is on.
#[derive(Debug)]
pub struct Aurora {
    /// The board's `t`, seconds: it moves on with the pace.
    pub time: f64,
    /// How lively it is: 1 while the music plays, easing to 0 as it stops.
    pub pace: f64,
    /// Its time as it last stood still.
    pub rest: f64,
    /// When it last moved on.
    last: Option<Duration>,
    /// By screen name.
    pub panes: HashMap<String, Pane>,
}

impl Default for Aurora {
    fn default() -> Self {
        Self {
            time: START,
            pace: 0.0,
            rest: START,
            last: None,
            panes: HashMap::new(),
        }
    }
}

/// The board's drive at `t`: its swell, and its kick on the beat.
fn drive(t: f64) -> (f64, f64) {
    let beat = (t * BPM / 60.0).fract();
    let kick = (-beat * 5.5).exp();
    let energy = 0.35 + 0.65 * ((t * 0.11).sin() * 0.5 + 0.5).powf(1.5);
    (energy, kick)
}

impl Aurora {
    /// How it looks now over a screen of `size` (logical).
    pub fn look(&self, size: [f32; 2]) -> AuroraLook {
        let (energy, kick) = drive(self.time);
        AuroraLook {
            size,
            time: self.time as f32,
            amp: (energy * self.pace) as f32,
            beat: (kick * energy * self.pace) as f32,
        }
    }

    /// How it looked as it last stood still.
    pub fn still_look(&self, size: [f32; 2]) -> AuroraLook {
        AuroraLook {
            size,
            time: self.rest as f32,
            amp: 0.0,
            beat: 0.0,
        }
    }

    /// It moves on to `now`, the music playing or not, `shown` on a
    /// screen: whether it moved (a frame is wanted). Not shown, it waits
    /// as it is.
    pub fn advance(&mut self, now: Duration, playing: bool, shown: bool) -> bool {
        let dt = self
            .last
            .map_or(Duration::ZERO, |last| now.saturating_sub(last))
            .min(LONGEST_STEP)
            .as_secs_f64();
        if !shown || (!playing && self.pace == 0.0) {
            self.last = None;
            return false;
        }
        self.last = Some(now);
        let (target, ease) = if playing {
            (1.0, WAKING)
        } else {
            (0.0, CALMING)
        };
        self.pace += (target - self.pace) * (1.0 - (-dt / ease).exp());
        if playing && self.pace > 1.0 - REST {
            self.pace = 1.0;
        }
        self.time = (self.time + dt * self.pace) % WRAP;
        if !playing && self.pace < REST {
            // Calmed: it stands still as it is, behind the glass too.
            self.pace = 0.0;
            self.rest = self.time;
            tracing::debug!(time = self.time, "the aurora calmed");
        }
        true
    }
}

/// The size (pixels) of the picture of a screen of `size` (logical).
pub fn picture_size(size: (f64, f64)) -> (i32, i32) {
    let scale = RESOLUTION.min(LONGEST / size.0.max(size.1).max(1.0));
    (
        (size.0 * scale).round().max(1.0) as i32,
        (size.1 * scale).round().max(1.0) as i32,
    )
}

impl crate::State {
    /// How the aurora shows now (#801): moving, still with reduced
    /// motion, or not at all.
    pub(crate) fn aurora_plays(&self) -> Option<Plays> {
        self.wallpaper_effect(self.config.effects.aurora)
    }

    /// Whether the aurora is the wallpaper of `output`: it plays, the
    /// wallpaper shows there, and the koi's pond does not take its place
    /// (the pond first, both on).
    pub(crate) fn aurora_shows_on(&self, output: &Output) -> bool {
        self.aurora_plays().is_some()
            && self.fx.aurora.is_some()
            && !self.wall_hidden(output)
            && !self.koi_shows_on(output)
    }

    /// The shell says whether the bar's player plays (#801).
    pub(crate) fn music_plays(&mut self, playing: bool) {
        if self.fx.music != playing {
            tracing::debug!(playing, "the music");
            self.fx.music = playing;
        }
    }

    /// The aurora follows its switch: on, a picture for each screen; off,
    /// gone. In the power saver or while a screen is shared it waits,
    /// kept. It moves on to `now` while the music plays, and as it calms
    /// after, on the screens it shows on (on, no fullscreen window in
    /// front): whether it moves.
    pub(crate) fn tick_aurora(&mut self, now: Duration) -> bool {
        let plays = self.aurora_plays();
        match (plays.is_some(), self.fx.aurora.is_some()) {
            (true, false) => {
                tracing::debug!("the aurora");
                self.fx.aurora = Some(Aurora::default());
                self.redraw = true;
            }
            (false, true) if !self.config.effects.aurora => {
                tracing::debug!("the aurora goes");
                self.fx.aurora = None;
                self.redraw = true;
            }
            _ => {}
        }
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let shown = outputs
            .iter()
            .any(|o| self.output_powered(o) && self.aurora_shows_on(o));
        let playing = self.fx.music && plays == Some(Plays::Moving);
        let Some(aurora) = self.fx.aurora.as_mut() else {
            return false;
        };
        let names: Vec<String> = outputs.iter().map(Output::name).collect();
        aurora.panes.retain(|n, _| names.contains(n));
        for name in names {
            aurora.panes.entry(name).or_default();
        }
        if plays == Some(Plays::Still) && aurora.pace > 0.0 {
            // Reduced motion: still at once.
            aurora.pace = 0.0;
            aurora.rest = aurora.time;
            aurora.last = None;
        }
        aurora.advance(now, playing, shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn the_music_wakes_it_and_it_calms_to_still_when_it_stops() {
        let mut aurora = Aurora::default();
        assert!(!aurora.advance(MS(0), false, true), "still without music");
        let mut at = 0;
        for _ in 0..120 {
            at += 16;
            assert!(aurora.advance(MS(at), true, true));
        }
        assert_eq!(aurora.pace, 1.0, "awake within two seconds");
        let look = aurora.look([1200.0, 720.0]);
        assert!(look.amp >= 0.35 && look.beat > 0.0, "{look:?}");
        // Stopped: some seconds of calming, then still.
        let mut frames = 0;
        loop {
            at += 16;
            if !aurora.advance(MS(at), false, true) {
                break;
            }
            frames += 1;
            assert!(frames < 600, "never calms");
        }
        assert!((150..450).contains(&frames), "{frames} frames calming");
        assert_eq!(aurora.pace, 0.0);
        let time = aurora.time;
        assert!(!aurora.advance(MS(at + 1000), false, true));
        assert_eq!(aurora.time, time, "still");
        assert_eq!(aurora.rest, time, "the glass's picture where it stands");
        // Not shown, it waits as it is.
        assert!(!aurora.advance(MS(at + 2000), true, false));
        assert_eq!(aurora.time, time);
    }

    #[test]
    fn the_picture_is_the_board_s_share_of_the_screen() {
        assert_eq!(picture_size((1200.0, 720.0)), (720, 432));
        assert_eq!(picture_size((3840.0, 2160.0)), (1600, 900));
    }
}
