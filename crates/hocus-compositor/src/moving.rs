//! The wallpapers that move (#433): `[wallpaper] picture = "rain"` or
//! `"stars"`, drawn on the GPU by [`crate::matrix`]'s shaders straight into
//! the screen's frame, at their own pace (rain 15 frames a second, stars
//! 20; half on battery), each screen deciding alone whether it runs.
//!
//! A frame of the wallpaper is a tick: a timer on the loop for the next
//! one due while it runs, never queued per tick; at each tick its element
//! changes, and the screen redraws what of it shows. It stops (the last
//! frame stays, and goes on from there) under a fullscreen window, covered
//! at 90 % or more, locked, off, under the idle curtain, after 2 minutes
//! without input, in the power saver, with reduced motion, `animate =
//! false`, and on battery with `on-battery = "still"`. Without GLES the
//! wallpaper thread draws its still picture once, as other wallpapers.
//!
//! The glass (the bar's, the popovers', the launcher's) never sees it
//! move: what is behind a pane is drawn with a second element of the
//! wallpaper, a still copy taken as it starts, as a surface with glass
//! appears and every 2 minutes while it moves ([`STILL_REFRESH`]): the
//! panes are blurred then, not at each of its frames.

use std::{collections::HashMap, time::Duration};

use hocus_link::{
    moving::{Moving, RAIN_CELL, STILL_TICK},
    wallpaper::Picture,
};
use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    output::Output,
};

use crate::{
    State,
    config::OnBattery,
    matrix::{WallElement, WallLook},
};

/// Input this long ago, and the wallpaper stops until the next.
pub const RESTS_AFTER: Duration = Duration::from_secs(120);
/// Covered this much or more, it stops.
pub const COVERED: f64 = 0.9;
/// The glass's still copy of a moving wallpaper is taken again this often
/// while it moves (one blur of each pane each time), so that what the
/// glass shows never drifts far from what moves beside it; and when a
/// surface with glass appears.
pub const STILL_REFRESH: Duration = Duration::from_secs(120);

/// The surfaces of `output` that have glass behind them: the bar, its
/// popovers, the emoji picker, the launcher.
fn glassy(output: &Output) -> usize {
    smithay::desktop::layer_map_for_output(output)
        .layers()
        .filter(|l| {
            matches!(
                l.namespace(),
                hocus_link::NAMESPACE
                    | hocus_link::POPOVER_NAMESPACE
                    | hocus_link::EMOJI_NAMESPACE
                    | hocus_link::LAUNCHER_NAMESPACE
            )
        })
        .count()
}

/// A screen's moving wallpaper.
#[derive(Debug)]
pub struct Wall {
    moving: Moving,
    size: (i32, i32),
    /// Its frames so far; the shader's time.
    ticks: u64,
    id: Id,
    commit: CommitCounter,
    /// The still one behind the glass: the frame it shows, its commit
    /// (a new one each time it is taken again), when it was taken, and the
    /// surfaces with glass over the screen then.
    still_id: Id,
    still_tick: u64,
    still_commit: CommitCounter,
    still_at: Option<Duration>,
    glassy: usize,
    /// When its next frame is due, while it runs.
    next: Option<Duration>,
    /// Whether the day's recorder was told it moves.
    said_moving: bool,
}

impl Wall {
    fn new(moving: Moving, size: (i32, i32)) -> Self {
        Self {
            moving,
            size,
            ticks: u64::from(STILL_TICK),
            id: Id::new(),
            commit: CommitCounter::default(),
            still_id: Id::new(),
            still_tick: u64::from(STILL_TICK),
            still_commit: CommitCounter::default(),
            still_at: None,
            glassy: 0,
            next: None,
            said_moving: false,
        }
    }
}

/// Every screen's moving wallpaper, and what they cost.
#[derive(Debug, Default)]
pub struct Walls {
    by_output: HashMap<String, Wall>,
    /// Frames of moving wallpapers so far, by screen.
    pub ticks: HashMap<String, u64>,
    /// Times the moving wallpapers' timer woke the loop.
    pub wakes: u64,
    /// The fastest running one's pace, and the timer waking them.
    pace: Option<Duration>,
    timer: Option<calloop::RegistrationToken>,
}

impl Walls {
    /// `output` is unplugged.
    pub fn forget(&mut self, output: &str) {
        self.by_output.remove(output);
    }

    /// Whether `output`'s moving wallpaper runs now.
    pub fn running(&self, output: &str) -> bool {
        self.by_output.get(output).is_some_and(|w| w.next.is_some())
    }
}

/// The area of the union of `rects` (x0, y0, x1, y1).
fn union_area(rects: &[(f64, f64, f64, f64)]) -> f64 {
    let mut xs: Vec<f64> = rects.iter().flat_map(|r| [r.0, r.2]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let mut area = 0.0;
    let mut spans: Vec<(f64, f64)> = Vec::with_capacity(rects.len());
    for (&a, &b) in xs.iter().zip(xs.iter().skip(1)) {
        spans.clear();
        spans.extend(
            rects
                .iter()
                .filter(|r| r.0 <= a && r.2 >= b)
                .map(|r| (r.1, r.3)),
        );
        spans.sort_by(|p, q| p.0.total_cmp(&q.0));
        let (mut covered, mut end) = (0.0, f64::MIN);
        for &(y0, y1) in &spans {
            if y1 <= end {
                continue;
            }
            covered += y1 - y0.max(end);
            end = y1;
        }
        area += covered * (b - a);
    }
    area
}

impl State {
    /// The moving wallpaper `output` shows, where the compositor draws one.
    fn moving_on(&self, output: &Output) -> Option<Moving> {
        if !self.graphics.blur || self.config.plain_background {
            return None;
        }
        // The rain on the glass refracts its still picture (#802); the
        // koi's pond takes its place (#803).
        if self.rain_shows_on(output) || self.koi_shows_on(output) {
            return None;
        }
        match self.wallpaper_look(&output.name()).picture {
            Picture::Moving(moving) => Some(moving),
            _ => None,
        }
    }

    /// How much of `output` the windows shown cover, from 0 to 1.
    pub fn covered(&self, output: &Output) -> f64 {
        let Some(screen) = self.space.output_geometry(output) else {
            return 0.0;
        };
        let rects: Vec<(f64, f64, f64, f64)> = self
            .windows
            .keys()
            .filter(|id| self.model.is_shown(**id) && !self.model.is_minimized(**id))
            .filter_map(|id| self.rect_of(*id))
            .filter_map(|r| {
                let r = crate::windows::rectangle(r).intersection(screen)?;
                Some((
                    f64::from(r.loc.x),
                    f64::from(r.loc.y),
                    f64::from(r.loc.x + r.size.w),
                    f64::from(r.loc.y + r.size.h),
                ))
            })
            .collect();
        let whole = f64::from(screen.size.w) * f64::from(screen.size.h);
        if whole <= 0.0 {
            return 0.0;
        }
        union_area(&rects) / whole
    }

    /// The time between `output`'s wallpaper frames if it runs at `now`;
    /// `None`: it stays as it is.
    pub(crate) fn wall_pace(&self, output: &Output, now: Duration) -> Option<Duration> {
        let moving = self.moving_on(output)?;
        let wallpaper = &self.config.wallpaper;
        let battery = self.on_battery();
        let runs = wallpaper.animate
            && !self.reduced_motion()
            && !self.power_saving()
            && !(battery && wallpaper.on_battery == OnBattery::Still)
            && !self.locked()
            && self.output_powered(output)
            && self.curtain_since().is_none()
            && now.saturating_sub(self.idle_input_at()) < RESTS_AFTER
            && !self.fullscreen_in_front(output)
            && self.covered(output) < COVERED;
        if !runs {
            return None;
        }
        let pace = Duration::from_secs(1) / moving.fps().max(1);
        Some(if battery && wallpaper.on_battery == OnBattery::Half {
            pace * 2
        } else {
            pace
        })
    }

    /// The wallpapers' frames due now: each running one moves on a frame
    /// when it is due. One timer on the loop wakes them while any runs,
    /// set again by itself at the fastest's pace (never a new one per
    /// frame); the day's recorder hears when one starts or stops moving.
    pub(crate) fn tick_walls(&mut self) {
        let now = self.now();
        if self.graphics.walls.get_mut().by_output.is_empty() {
            self.graphics.walls.get_mut().pace = None;
            return;
        }
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let mut fastest: Option<Duration> = None;
        for output in outputs {
            let pace = self.wall_pace(&output, now);
            let glass = glassy(&output);
            let name = output.name();
            let walls = self.graphics.walls.get_mut();
            let Some(wall) = walls.by_output.get_mut(&name) else {
                continue;
            };
            // The glass's still copy taken again: a surface with glass came
            // (it is blurred from what shows now), or it moved for 2 min.
            let taken = *wall.still_at.get_or_insert(now);
            let old = pace.is_some() && now.saturating_sub(taken) >= STILL_REFRESH;
            if (glass > wall.glassy || old) && wall.still_tick != wall.ticks {
                wall.still_tick = wall.ticks;
                wall.still_commit.increment();
                wall.still_at = Some(now);
                self.redraw = true;
            } else if old {
                wall.still_at = Some(now);
            }
            wall.glassy = glass;
            if pace.is_some() != wall.said_moving {
                wall.said_moving = pace.is_some();
                let kind = if pace.is_some() {
                    hocus_perf::EventKind::WallpaperMoving
                } else {
                    hocus_perf::EventKind::WallpaperStill
                };
                tracing::debug!(output = name, moving = pace.is_some(), "the wallpaper");
                self.perf.event(kind, wall.moving.word());
            }
            let Some(pace) = pace else {
                wall.next = None;
                continue;
            };
            fastest = Some(fastest.map_or(pace, |f| f.min(pace)));
            // Going on from where it stopped, a frame from now.
            let due = *wall.next.get_or_insert(now + pace);
            if now >= due {
                wall.ticks += 1;
                wall.commit.increment();
                // Late: the next a whole frame from now, none skipped
                // ahead.
                let next = due + pace;
                wall.next = Some(if next > now { next } else { now + pace });
                *walls.ticks.entry(name).or_default() += 1;
                self.redraw = true;
            }
        }
        let walls = self.graphics.walls.get_mut();
        walls.pace = fastest;
        if let Some(pace) = fastest
            && walls.timer.is_none()
        {
            let timer = calloop::timer::Timer::from_duration(pace);
            let inserted = self
                .loop_handle
                .insert_source(timer, |_, _, app| app.state.wall_timer_fired());
            match inserted {
                Ok(token) => self.graphics.walls.get_mut().timer = Some(token),
                Err(err) => tracing::warn!(%err.error, "no timer for the moving wallpaper"),
            }
        }
    }

    /// The wallpapers' timer: a frame is drawn, and it comes again at the
    /// fastest's pace while one runs.
    fn wall_timer_fired(&mut self) -> calloop::timer::TimeoutAction {
        let walls = self.graphics.walls.get_mut();
        walls.wakes += 1;
        self.redraw = true;
        match walls.pace {
            Some(pace) => calloop::timer::TimeoutAction::ToDuration(pace),
            None => {
                walls.timer = None;
                calloop::timer::TimeoutAction::Drop
            }
        }
    }

    /// `output`'s moving wallpaper: the element the screen draws, and the
    /// still one the glass blurs. `None`: none moves there, or the renderer
    /// draws none (the wallpaper thread's picture shows).
    pub(crate) fn wall_elements<R: crate::matrix::Matrix>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Option<(WallElement, WallElement)> {
        let moving = self.moving_on(output)?;
        if !renderer.draws_walls() {
            return None;
        }
        let mode = output.current_mode()?;
        let size = output.current_transform().transform_size(mode.size);
        let size = (size.w, size.h);
        let scale = output.current_scale().fractional_scale();
        let mut walls = self.graphics.walls.borrow_mut();
        let wall = walls
            .by_output
            .entry(output.name())
            .or_insert_with(|| Wall::new(moving, size));
        if wall.moving != moving || wall.size != size {
            *wall = Wall::new(moving, size);
        }
        let look = |tick: u64| WallLook {
            moving,
            tick: tick as f32,
            cell: ((RAIN_CELL.0 * scale) as f32, (RAIN_CELL.1 * scale) as f32),
            scale: scale as f32,
            colours: hocus_link::moving::rain_colours(self.config.appearance.theme),
        };
        Some((
            WallElement::new(wall.id.clone(), wall.commit, size, look(wall.ticks)),
            WallElement::new(
                wall.still_id.clone(),
                wall.still_commit,
                size,
                look(wall.still_tick),
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_union_of_windows_counts_each_place_once() {
        assert_eq!(union_area(&[]), 0.0);
        assert_eq!(union_area(&[(0.0, 0.0, 10.0, 10.0)]), 100.0);
        // Two halves: the whole.
        let halves = [(0.0, 0.0, 5.0, 10.0), (5.0, 0.0, 10.0, 10.0)];
        assert_eq!(union_area(&halves), 100.0);
        // Overlapping: counted once.
        let over = [(0.0, 0.0, 6.0, 10.0), (4.0, 0.0, 10.0, 10.0)];
        assert_eq!(union_area(&over), 100.0);
        let corner = [(0.0, 0.0, 10.0, 10.0), (5.0, 5.0, 15.0, 15.0)];
        assert_eq!(union_area(&corner), 175.0);
    }
}
