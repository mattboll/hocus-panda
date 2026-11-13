//! Daylight (#805, the board "La lumière du jour", its JS ported): the
//! wallpaper is the board's sky at the hour the real sun stands for
//! ([`super::sun::board_hour`]): its gradient, the stars by night, the sun
//! (its halo, its disc and its rays) by day, else the moon, two ridges;
//! the hour's light graded over it. The windows' shadows lengthen and turn
//! away from the sun, and their edge facing it lights up. Drawn by
//! `crate::render::daylight`.
//!
//! The board ran its hour on (its slider); here it is the sun's, looked at
//! every minute on the wall clock: the sky is drawn again then, never
//! frame after frame.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use smithay::backend::renderer::{element::Id, utils::CommitCounter};

use crate::{decoration::ShadowStyle, wobble::Kept};

/// The board's `KEYS`: an hour, the sky's top and bottom colours, the
/// light's, and how much of the light is graded over everything.
type Key = (f64, [f64; 3], [f64; 3], [f64; 3], f64);
const KEYS: [Key; 9] = [
    (
        5.0,
        [10.0, 14.0, 34.0],
        [40.0, 36.0, 70.0],
        [0.0, 0.0, 0.0],
        0.0,
    ),
    (
        6.5,
        [60.0, 70.0, 120.0],
        [240.0, 150.0, 140.0],
        [255.0, 170.0, 150.0],
        0.25,
    ),
    (
        8.0,
        [110.0, 160.0, 220.0],
        [230.0, 200.0, 190.0],
        [255.0, 210.0, 180.0],
        0.15,
    ),
    (
        12.0,
        [90.0, 150.0, 225.0],
        [180.0, 210.0, 240.0],
        [255.0, 255.0, 245.0],
        0.05,
    ),
    (
        16.5,
        [100.0, 150.0, 210.0],
        [230.0, 205.0, 170.0],
        [255.0, 225.0, 180.0],
        0.12,
    ),
    (
        18.5,
        [70.0, 80.0, 140.0],
        [250.0, 150.0, 80.0],
        [255.0, 150.0, 70.0],
        0.35,
    ),
    (
        19.5,
        [40.0, 40.0, 90.0],
        [190.0, 90.0, 110.0],
        [210.0, 110.0, 120.0],
        0.3,
    ),
    (
        21.0,
        [14.0, 18.0, 44.0],
        [40.0, 40.0, 80.0],
        [80.0, 90.0, 160.0],
        0.25,
    ),
    (
        23.0,
        [8.0, 10.0, 26.0],
        [20.0, 22.0, 48.0],
        [60.0, 70.0, 140.0],
        0.25,
    ),
];

fn mix(p: [f64; 3], q: [f64; 3], t: f64) -> [f64; 3] {
    let mut out = p;
    for (o, b) in out.iter_mut().zip(q) {
        *o += (b - *o) * t;
    }
    out
}

/// The board's colours at an hour: the sky's top and bottom, the light,
/// its grade (0 to 255, the grade 0 to 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colours {
    pub top: [f64; 3],
    pub bottom: [f64; 3],
    pub light: [f64; 3],
    pub grade: f64,
}

/// The board's `at(h)`. Past its last key (23) the night goes on to its
/// first (5) the next day, which the board, its slider ending at 23, never
/// showed.
#[expect(
    clippy::indexing_slicing,
    reason = "the keys are a constant nine, looked at from 0 to their last"
)]
pub fn colours(hour: f64) -> Colours {
    let last = KEYS.len() - 1;
    let (k0, k1, k) = if hour >= KEYS[last].0 || hour < KEYS[0].0 {
        let h = if hour < KEYS[0].0 { hour + 24.0 } else { hour };
        (
            KEYS[last],
            KEYS[0],
            (h - KEYS[last].0) / (KEYS[0].0 + 24.0 - KEYS[last].0),
        )
    } else {
        let mut i = 0;
        while i < KEYS.len() - 2 && KEYS[i + 1].0 < hour {
            i += 1;
        }
        let (a, b) = (KEYS[i], KEYS[i + 1]);
        (a, b, (hour - a.0) / (b.0 - a.0))
    };
    let k = k.clamp(0.0, 1.0);
    Colours {
        top: mix(k0.1, k1.1, k),
        bottom: mix(k0.2, k1.2, k),
        light: mix(k0.3, k1.3, k),
        grade: k0.4 + (k1.4 - k0.4) * k,
    }
}

/// The sky at an hour on a screen `w` by `h` (logical), as the board
/// places it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sky {
    pub hour: f64,
    pub colours: Colours,
    pub sun_up: bool,
    /// The sun's height, 0 on the horizon to 1 at noon.
    pub elev: f64,
    /// The sun's place (logical, from the screen's corner).
    pub sun: (f64, f64),
}

impl Sky {
    pub fn at(hour: f64, w: f64, h: f64) -> Self {
        let day_t = (hour - 6.0) / 14.0;
        let sun_up = hour > 6.0 && hour < 20.0;
        let t = day_t.clamp(0.0, 1.0);
        let elev = (t * std::f64::consts::PI).sin();
        Self {
            hour,
            colours: colours(hour),
            sun_up,
            elev,
            sun: (60.0 + (w - 320.0) * t, h * 0.78 - elev * h * 0.62),
        }
    }

    /// The windows' shadow under this sun on a screen `w` wide, from
    /// `style` (the standard one of a window, focused or moving): the
    /// board's offset, blur and darkness, what the window's focus or its
    /// motion adds kept.
    pub fn shadow(&self, w: f64, style: ShadowStyle) -> ShadowStyle {
        let base = ShadowStyle::UNFOCUSED;
        let lx = self.lx(w);
        let len = if self.sun_up {
            18.0 + (1.0 - self.elev) * 70.0
        } else {
            14.0
        };
        let alpha = if self.sun_up {
            0.35 + 0.25 * self.elev
        } else {
            0.45
        };
        ShadowStyle {
            dx: lx * len * 2.2,
            dy: len * (0.4 + self.elev * 0.6) + (style.dy - base.dy),
            blur: 26.0 + len * 0.6 + (style.blur - base.blur),
            alpha: alpha as f32 + (style.alpha - base.alpha),
            radius: style.radius,
        }
    }

    /// The board's `lx`: how far the sun is from the screen's middle,
    /// left positive, as a share of its width; by night, a little.
    pub fn lx(&self, w: f64) -> f64 {
        if self.sun_up {
            (w / 2.0 - self.sun.0) / w
        } else {
            0.1
        }
    }

    /// The edges a window's lit by this sun: by day, its top over a pixel,
    /// and the side facing the sun (`-1` left, `1` right) as wide as the
    /// board's `lx * 3` pixels; their colour (premultiplied, 0 to 1).
    pub fn lit_edges(&self, w: f64) -> Option<LitEdges> {
        if !self.sun_up {
            return None;
        }
        let lx = self.lx(w);
        let a = 0.25;
        let [r, g, b] = self.colours.light.map(|c| (c / 255.0 * a) as f32);
        Some(LitEdges {
            side: if lx > 0.0 { -1 } else { 1 },
            width: (lx.abs() * 3.0) as f32,
            colour: [r, g, b, a as f32],
        })
    }
}

/// The edges of a window a sun lights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LitEdges {
    /// The side facing the sun: -1 left, 1 right.
    pub side: i32,
    /// That side's width, logical pixels (under one: as opaque as it is
    /// wide).
    pub width: f32,
    pub colour: [f32; 4],
}

/// The sky of one screen, drawn once a minute.
#[derive(Debug)]
pub struct Pane {
    pub id: Id,
    /// Its picture, kept between frames.
    pub kept: RefCell<Option<Kept>>,
    /// What its picture shows: the hour, and its size (physical).
    pub drawn: Cell<Option<(u64, (i32, i32))>>,
    pub commit: Cell<CommitCounter>,
}

impl Default for Pane {
    fn default() -> Self {
        Self {
            id: Id::new(),
            kept: RefCell::new(None),
            drawn: Cell::new(None),
            commit: Cell::new(CommitCounter::default()),
        }
    }
}

/// The daylight, while it is on.
#[derive(Debug, Default)]
pub struct Daylight {
    /// The board's hour now.
    pub hour: f64,
    /// A new one each time the hour changes.
    pub generation: u64,
    /// The skies, by screen.
    pub panes: HashMap<String, Pane>,
}

impl Daylight {
    pub fn new(hour: f64) -> Self {
        Self {
            hour,
            generation: 1,
            panes: HashMap::new(),
        }
    }

    /// The hour is `hour`: whether it changed.
    pub fn set(&mut self, hour: f64) -> bool {
        if (hour - self.hour).abs() < 1.0e-6 {
            return false;
        }
        self.hour = hour;
        self.generation += 1;
        true
    }
}

impl crate::State {
    /// How the daylight shows now (#805). With reduced motion it shows
    /// as it does without: it never moves, the hour changing once a
    /// minute.
    pub(crate) fn daylight_plays(&self) -> Option<super::Plays> {
        self.wallpaper_effect(self.config.effects.daylight)
    }

    /// The daylight's sky over `output` now: it shows there (not with a
    /// fullscreen window in front), the screen's size (logical).
    pub(crate) fn daylight_on(&self, output: &smithay::output::Output) -> Option<Sky> {
        self.daylight_plays()?;
        if self.wall_hidden(output) {
            return None;
        }
        let daylight = self.fx.daylight.as_ref()?;
        let area = self.space.output_geometry(output)?;
        Some(Sky::at(
            daylight.hour,
            f64::from(area.size.w),
            f64::from(area.size.h),
        ))
    }

    /// Whether the daylight's sky is the wallpaper of `output`: the koi's
    /// pond, the aurora (#801) or the landscape in depth (#806) takes its
    /// place when on too.
    pub(crate) fn sky_shows_on(&self, output: &smithay::output::Output) -> bool {
        self.daylight_on(output).is_some()
            && !self.koi_shows_on(output)
            && !self.aurora_shows_on(output)
            && !self.depth_shows_on(output)
    }

    /// The daylight follows its switch (#805): on, the hour is the sun's,
    /// looked at every minute ([`Self::watch_sun`]); off, gone.
    pub(crate) fn tick_daylight(&mut self) {
        self.watch_sun();
        if let Some(daylight) = self.fx.daylight.as_mut() {
            let names: Vec<String> = self.space.outputs().map(|o| o.name()).collect();
            daylight.panes.retain(|n, _| names.contains(n));
            for name in names {
                daylight.panes.entry(name).or_default();
            }
        }
        match (self.daylight_plays(), self.fx.daylight.is_some()) {
            (Some(_), false) => {
                let hour = self.sun_hour();
                tracing::debug!(hour, "the daylight comes");
                self.fx.daylight = Some(Daylight::new(hour));
                self.redraw = true;
            }
            (None, true) if !self.config.effects.daylight => {
                tracing::debug!("the daylight goes");
                self.fx.daylight = None;
                self.redraw = true;
            }
            // Waiting (the power saver, a share): kept, not drawn; a
            // frame drawn as it starts waiting, without it.
            _ => {}
        }
    }

    /// The board's hour the sun stands for now, or the one the tests hold.
    pub(crate) fn sun_hour(&self) -> f64 {
        match self.fx.sun_held {
            Some(at) => super::sun::board_hour_at(at),
            None => super::sun::board_hour_now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_keys_morning_noon_sunset_night() {
        let c = colours(18.5);
        assert_eq!(c.light, [255.0, 150.0, 70.0]);
        assert!((c.grade - 0.35).abs() < 1e-9);
        let c = colours(10.0);
        assert_eq!(c.top, [100.0, 155.0, 222.5]);
        // Past 23 the night goes on to 5's.
        let c = colours(2.0);
        assert!(c.top[2] > 26.0 && c.top[2] < 34.0, "{:?}", c.top);
        let noon = Sky::at(13.0, 1200.0, 720.0);
        assert!(noon.sun_up && (noon.elev - 1.0).abs() < 1e-9);
        assert!((noon.sun.0 - 500.0).abs() < 1e-9);
        // The evening's sun on the right: shadows long, to the left; the
        // right edge lit.
        let evening = Sky::at(18.5, 1200.0, 720.0);
        let s = evening.shadow(1200.0, ShadowStyle::UNFOCUSED);
        assert!(s.dx < -10.0 && s.dy > 20.0 && s.blur > 60.0, "{s:?}");
        assert_eq!(evening.lit_edges(1200.0).map(|e| e.side), Some(1));
        assert!(Sky::at(22.0, 1200.0, 720.0).lit_edges(1200.0).is_none());
    }
}
