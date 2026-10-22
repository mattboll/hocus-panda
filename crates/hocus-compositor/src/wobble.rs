//! Wobbly windows and flicks (#35). While a window is dragged it bends
//! like jelly: a grid of points on springs, the ones far from where it is
//! held lagging behind; let go, they settle with a small bounce. Let go
//! with speed, it keeps its momentum, glides and lands, on the screen it
//! was thrown at when it was aimed at another one.
//!
//! Drawn in strips of its picture, as the genie is, so that pixman and the
//! scenarios see it too. Nothing moves in tests that do not sample motion,
//! nor with reduced motion: there a flick lands at once. In the Matrix
//! theme a dragged window stays rigid and leaves a phosphor trail instead
//! (`matrix_windows`); it still glides when thrown.

use std::{collections::BTreeMap, time::Duration};

use hocus_core::WindowId;
use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    desktop::Window,
    output::Output,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
};

use crate::{
    State,
    decoration::Rounded,
    scene::SceneElement,
    snapshot::{bent, unclipped},
};

/// Points per side of the grid.
const GRID: usize = 4;
/// The springs: an angular frequency of about 16 rad/s, damped to 0.62 of
/// critical, so that it settles with a small bounce well within 600 ms.
const STIFFNESS: f64 = 260.0;
const DAMPING: f64 = 20.0;
/// How far a point may lag, logical pixels.
const MAX_LAG: f64 = 90.0;
/// Below these, a point has settled.
const REST: f64 = 0.3;
const REST_SPEED: f64 = 4.0;
/// Integration steps, at most this long.
const STEP: f64 = 0.004;
/// A flick: let go faster than this, logical pixels per second.
const FLICK_SPEED: f64 = 1100.0;
/// The glide's time constant: it travels its speed times this.
const GLIDE: f64 = 0.22;
/// How long the glide lasts.
const GLIDE_FOR: f64 = 0.6;
/// How far back the speed at release is measured.
const SPEED_WINDOW: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, Copy, Default)]
struct Node {
    /// From where the rigid window has it.
    lag: (f64, f64),
    speed: (f64, f64),
}

/// A window bending as it is dragged.
#[derive(Debug, Clone)]
struct Jelly {
    nodes: [[Node; GRID]; GRID],
    /// Where it is held, from 0 to 1 across and down; none once let go.
    held: Option<(f64, f64)>,
    last: Duration,
}

/// A window thrown: from where, to where, since when.
#[derive(Debug, Clone, Copy)]
struct Flick {
    from: Point<f64, Logical>,
    to: Point<f64, Logical>,
    since: Duration,
}

#[derive(Debug, Default)]
pub struct Wobbles {
    jellies: BTreeMap<WindowId, Jelly>,
    flicks: BTreeMap<WindowId, Flick>,
}

impl Jelly {
    fn new(held: (f64, f64), now: Duration) -> Self {
        Self {
            nodes: [[Node::default(); GRID]; GRID],
            held: Some(held),
            last: now,
        }
    }

    fn uv(i: usize) -> f64 {
        i as f64 / (GRID - 1) as f64
    }

    /// The window moved by `delta`: points lag the more the farther they
    /// are from where it is held.
    fn moved(&mut self, delta: (f64, f64)) {
        let Some((hu, hv)) = self.held else {
            return;
        };
        for (j, row) in self.nodes.iter_mut().enumerate() {
            for (i, node) in row.iter_mut().enumerate() {
                let far = ((Self::uv(i) - hu).hypot(Self::uv(j) - hv) / std::f64::consts::SQRT_2)
                    .clamp(0.0, 1.0);
                let lag = 0.9 * far.powf(1.2);
                node.lag.0 = (node.lag.0 - delta.0 * lag).clamp(-MAX_LAG, MAX_LAG);
                node.lag.1 = (node.lag.1 - delta.1 * lag).clamp(-MAX_LAG, MAX_LAG);
            }
        }
    }

    /// The springs pull every point back to the rigid window, up to `now`.
    fn advance(&mut self, now: Duration) {
        let mut left = now.saturating_sub(self.last).as_secs_f64().min(0.5);
        self.last = now;
        while left > 0.0 {
            let dt = left.min(STEP);
            left -= dt;
            for node in self.nodes.iter_mut().flatten() {
                for (lag, speed) in [
                    (&mut node.lag.0, &mut node.speed.0),
                    (&mut node.lag.1, &mut node.speed.1),
                ] {
                    *speed += (-STIFFNESS * *lag - DAMPING * *speed) * dt;
                    *lag += *speed * dt;
                }
            }
        }
    }

    fn settled(&self) -> bool {
        self.held.is_none()
            && self
                .nodes
                .iter()
                .flatten()
                .all(|n| n.lag.0.hypot(n.lag.1) < REST && n.speed.0.hypot(n.speed.1) < REST_SPEED)
    }

    /// The lag at (`u`, `v`), between the grid's points.
    fn lag_at(&self, u: f64, v: f64) -> (f64, f64) {
        let last = (GRID - 1) as f64;
        let (x, y) = ((u * last).clamp(0.0, last), (v * last).clamp(0.0, last));
        let (i, j) = ((x as usize).min(GRID - 2), (y as usize).min(GRID - 2));
        let (fx, fy) = (x - i as f64, y - j as f64);
        let n = |a: usize, b: usize| self.nodes[b][a].lag;
        let mix =
            |a: (f64, f64), b: (f64, f64), t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let top = mix(n(i, j), n(i + 1, j), fx);
        let bottom = mix(n(i, j + 1), n(i + 1, j + 1), fx);
        mix(top, bottom, fy)
    }
}

impl Flick {
    /// Where it is `t` seconds after it was let go: fast, then slowing, at
    /// its landing place by the end.
    fn at(&self, t: f64) -> Point<f64, Logical> {
        let t = t.clamp(0.0, GLIDE_FOR);
        let p = (1.0 - (-t / GLIDE).exp()) / (1.0 - (-GLIDE_FOR / GLIDE).exp());
        self.from + (self.to - self.from).upscale(p)
    }
}

/// The speed of the pointer over the last moments, logical pixels per
/// second; `None` when they all came at once.
pub fn speed(samples: &[(Duration, Point<f64, Logical>)]) -> Option<(f64, f64)> {
    let (last_t, last_p) = *samples.last()?;
    let (first_t, first_p) = *samples
        .iter()
        .find(|(t, _)| last_t.saturating_sub(*t) <= SPEED_WINDOW)?;
    let dt = last_t.saturating_sub(first_t).as_secs_f64();
    (dt > 0.0).then(|| ((last_p.x - first_p.x) / dt, (last_p.y - first_p.y) / dt))
}

impl State {
    fn wobbles_move(&self) -> bool {
        !self.instant_motion && !self.config.appearance.reduced_motion
    }

    /// A drag begins, held at `at` (global).
    pub fn wobble_held(&mut self, id: WindowId, window: &Window, at: Point<f64, Logical>) {
        if !self.wobbles_move() || self.look() == crate::motion::Look::Matrix {
            return;
        }
        self.wobbles.flicks.remove(&id);
        let Some(loc) = self.space.element_location(window) else {
            return;
        };
        let size = window.geometry().size.to_f64();
        let held = (
            ((at.x - f64::from(loc.x)) / size.w.max(1.0)).clamp(0.0, 1.0),
            ((at.y - f64::from(loc.y)) / size.h.max(1.0)).clamp(0.0, 1.0),
        );
        let now = self.clock.now();
        let jelly = self
            .wobbles
            .jellies
            .entry(id)
            .or_insert_with(|| Jelly::new(held, now));
        jelly.held = Some(held);
    }

    /// The dragged window moved by `delta`.
    pub fn wobble_moved(&mut self, id: WindowId, delta: Point<i32, Logical>) {
        let now = self.clock.now();
        if let Some(jelly) = self.wobbles.jellies.get_mut(&id) {
            jelly.advance(now);
            jelly.moved((f64::from(delta.x), f64::from(delta.y)));
            self.needs_redraw = true;
        }
    }

    /// The drag ends: the jelly settles; let go with speed, the window
    /// glides and lands, on the screen aimed at. Whether it was thrown.
    pub fn wobble_let_go(
        &mut self,
        id: WindowId,
        window: &Window,
        speed: Option<(f64, f64)>,
    ) -> bool {
        if let Some(jelly) = self.wobbles.jellies.get_mut(&id) {
            jelly.held = None;
        }
        let Some((vx, vy)) = speed.filter(|(x, y)| x.hypot(*y) > FLICK_SPEED) else {
            return false;
        };
        let Some(from) = self.space.element_location(window).map(|p| p.to_f64()) else {
            return false;
        };
        let size = window.geometry().size;
        let thrown = from + Point::from((vx * GLIDE, vy * GLIDE));
        // Where its middle lands says which screen; it lands whole inside
        // that screen's work area.
        let middle = thrown + size.to_f64().to_point().downscale(2.0);
        let output = self
            .space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| g.to_f64().contains(middle))
            })
            .or_else(|| self.output_of(window))
            .cloned();
        let Some(output) = output else {
            return false;
        };
        let area = self.work_area(&output).to_f64();
        let to = Point::from((
            thrown.x.clamp(
                area.loc.x,
                (area.loc.x + area.size.w - f64::from(size.w)).max(area.loc.x),
            ),
            thrown.y.clamp(
                area.loc.y,
                (area.loc.y + area.size.h - f64::from(size.h)).max(area.loc.y),
            ),
        ));
        if self.wobbles_move() {
            let since = self.clock.now();
            self.wobbles.flicks.insert(id, Flick { from, to, since });
        } else {
            self.land(id, window, to);
        }
        self.needs_redraw = true;
        true
    }

    /// Where it ends up: on the screen it is over, its shown desktop.
    fn land(&mut self, id: WindowId, window: &Window, at: Point<f64, Logical>) {
        let at = at.to_i32_round();
        let size = window.geometry().size;
        let middle = at + Point::from((size.w / 2, size.h / 2));
        let over = self
            .space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| g.contains(middle))
            })
            .cloned();
        if let Some(output) = over
            && self.output_of(window).map(Output::name) != Some(output.name())
            && let Err(err) = self.send_window(id, &output, None, SERIAL_COUNTER.next_serial())
        {
            tracing::warn!(%err, "a thrown window could not land on its screen");
        }
        self.space.map_element(window.clone(), at, true);
        self.sync_desktops();
    }

    /// After each dispatch: jellies spring back, thrown windows glide;
    /// those done go.
    pub fn refresh_wobbles(&mut self) {
        if self.wobbles.jellies.is_empty() && self.wobbles.flicks.is_empty() {
            return;
        }
        let now = self.clock.now();
        for jelly in self.wobbles.jellies.values_mut() {
            jelly.advance(now);
        }
        let windows = &self.windows;
        self.wobbles
            .jellies
            .retain(|id, j| !j.settled() && windows.contains_key(id));
        let flicks: Vec<(WindowId, Flick)> = self
            .wobbles
            .flicks
            .iter()
            .map(|(id, f)| (*id, *f))
            .collect();
        for (id, flick) in flicks {
            let Some(window) = self.windows.get(&id).map(|m| m.window.clone()) else {
                self.wobbles.flicks.remove(&id);
                continue;
            };
            let t = now.saturating_sub(flick.since).as_secs_f64();
            let at = flick.at(t);
            if t >= GLIDE_FOR {
                self.wobbles.flicks.remove(&id);
                self.land(id, &window, at);
            } else {
                let before = self.space.element_location(&window);
                let now_at = at.to_i32_round();
                self.space.map_element(window, now_at, false);
                if let Some(before) = before {
                    self.wobble_moved(id, now_at - before);
                }
                self.matrix_moved(id, now_at);
            }
        }
        self.needs_redraw = true;
    }

    /// Whether a window bends now: it is drawn by [`Self::wobbly_elements`].
    pub fn wobbles(&self, id: WindowId) -> bool {
        self.wobbles.jellies.contains_key(&id)
    }

    /// The bending window at `rect` (global), in strips of its picture.
    pub fn wobbly_elements<R>(
        &self,
        renderer: &R,
        id: WindowId,
        window: &Window,
        rect: Rectangle<f64, Logical>,
        output: &Output,
        alpha: f32,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let (Some(jelly), Some(picture), Some(area)) = (
            self.wobbles.jellies.get(&id),
            self.picture_of(window),
            self.space.output_geometry(output),
        ) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let row = |v: f64| {
            let (_, top) = jelly.lag_at(0.5, v);
            let (left, _) = jelly.lag_at(0.0, v);
            let (right, _) = jelly.lag_at(1.0, v);
            (
                rect.loc.y + v * rect.size.h + top,
                rect.loc.x + left,
                rect.loc.x + rect.size.w + right,
            )
        };
        bent(renderer, &picture, row, area.loc.to_f64(), scale, alpha)
            .into_iter()
            .map(|e| SceneElement::Snapshot(Rounded::new(e, unclipped(), 0)))
            .collect()
    }

    /// Whether jellies or glides move: frames keep coming.
    pub fn wobbles_moving(&self) -> bool {
        !self.wobbles.jellies.is_empty() || !self.wobbles.flicks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_window_s_far_side_lags_and_its_held_point_does_not() {
        let mut jelly = Jelly::new((0.0, 0.0), Duration::ZERO);
        jelly.moved((40.0, 0.0));
        assert_eq!(jelly.lag_at(0.0, 0.0), (0.0, 0.0));
        let (far, _) = jelly.lag_at(1.0, 1.0);
        assert!(far < -30.0, "{far}");
    }

    #[test]
    fn let_go_it_settles_with_a_bounce_within_600_ms() {
        let mut jelly = Jelly::new((0.5, 0.0), Duration::ZERO);
        jelly.moved((80.0, 30.0));
        jelly.held = None;
        let mut overshot = false;
        for ms in (16..=600).step_by(16) {
            jelly.advance(Duration::from_millis(ms));
            overshot |= jelly.lag_at(0.0, 1.0).0 > 0.5;
        }
        jelly.advance(Duration::from_millis(600));
        assert!(overshot, "a small bounce");
        assert!(jelly.settled());
    }

    #[test]
    fn the_speed_is_measured_over_the_last_moments() {
        let p = |x: f64| Point::from((x, 0.0));
        let ms = Duration::from_millis;
        let samples = [
            (ms(0), p(0.0)),
            (ms(100), p(10.0)),
            (ms(140), p(50.0)),
            (ms(180), p(90.0)),
        ];
        let (vx, _) = speed(&samples).unwrap_or_default();
        assert!((vx - 1000.0).abs() < 1.0, "{vx}");
        assert_eq!(speed(&[(ms(5), p(0.0)), (ms(5), p(500.0))]), None);
    }

    #[test]
    fn a_glide_lands_where_it_was_aimed_and_slows_down() {
        let flick = Flick {
            from: Point::from((0.0, 0.0)),
            to: Point::from((500.0, 0.0)),
            since: Duration::ZERO,
        };
        assert_eq!(flick.at(GLIDE_FOR), Point::from((500.0, 0.0)));
        let first = flick.at(0.1).x;
        let second = flick.at(0.2).x - first;
        assert!(first > second, "{first} then {second}");
    }
}
