//! The Matrix theme's window motions (docs/design/matrix-theme.md, §3 to
//! §8), after its opening and closing (`matrix`):
//!
//! - Minimizing switches the window off as a CRT does, crushed to a bright
//!   line that dashes into its dock icon and flashes there; restoring
//!   powers it back on from a dot, the line growing into the window.
//! - Maximizing races circuit traces from its corners to where it goes,
//!   springs it there and flashes the new edges.
//! - Snapping shows the grid line it snaps to while it springs into place.
//! - A moved window leaves a short phosphor trail, and the guides of the
//!   zones it is dragged into flash.
//! - A resized window glows, reads out its size beside the pointer, and
//!   flashes once let go.
//!
//! Drawn with the elements every renderer has, pixman included, so that
//! scenarios sample them: the window's picture stretched, plain colours
//! and the readout. Each is a closed form of the time since it started,
//! evaluated when the scene is drawn: nothing is queued per frame.

use std::{collections::BTreeMap, time::Duration};

use hocus_core::{WindowId, Zone};
use hocus_shell::{
    matrix::{GLOW, GREEN, HEAD, Rng},
    readout::{self, Readout},
};
use hocus_ui::{
    Color, Theme, ThemeName,
    motion::{Easing, LARGE, Spring},
};
use smithay::{
    backend::renderer::{Color32F, ImportAll, ImportMem, Renderer, Texture},
    desktop::Window,
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    State,
    decoration::Rounded,
    desktops::rect,
    matrix::{Brush, millis, phase},
    matrix_layers::Layers,
    motion::Look,
    scene::SceneElement,
    shell::Panel,
    snapshot::Snapshot,
};

/// Minimizing and restoring.
pub const SWITCH: Duration = LARGE;
/// The ghosts a moved window leaves: how long ago it was there, and how
/// strong each still is, three frames at 60 Hz decaying.
const TRAIL: [(f32, f32); 3] = [(17.0, 0.5), (33.0, 0.28), (50.0, 0.14)];
/// A zone's guide flashes this long as a window is dragged into it.
const FLASH_MS: f32 = 60.0;
/// A resized window flashes this long once let go.
const RESIZED_MS: f32 = 50.0;
/// The springs of a window moving into place: a slight overshoot.
const SPRING: Spring = Spring {
    stiffness: 1600.0,
    damping_ratio: 0.7,
};

pub(crate) fn out_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

pub(crate) fn lerp(a: f64, b: f64, t: f32) -> f64 {
    a + (b - a) * f64::from(t)
}

/// A spring from 0 to 1 that has settled by `p` = 1, over `seconds`.
pub(crate) fn damped(p: f32, seconds: f32) -> f32 {
    if p >= 1.0 {
        return 1.0;
    }
    1.0 - SPRING.state(1.0, 0.0, p.max(0.0) * seconds).0
}

/// How a window changes place in the Matrix theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    /// Circuit traces race to where it goes (maximize, fullscreen, back).
    Maximize,
    /// It springs into a zone, under the grid line drawn apart.
    Snap,
}

impl Shift {
    pub fn duration(self) -> Duration {
        Duration::from_millis(self.total() as u64)
    }

    fn total(self) -> f32 {
        match self {
            Self::Maximize => 350.0,
            Self::Snap => 380.0,
        }
    }

    /// When the window moves, milliseconds from the start.
    fn moves(self) -> (f32, f32) {
        match self {
            Self::Maximize => (100.0, 300.0),
            Self::Snap => (80.0, 300.0),
        }
    }

    /// How far along its way the window is at `t` of `progress` (the
    /// share of the whole motion gone by), 0 to 1, overshooting a little.
    pub fn geometry(self, progress: f32) -> f32 {
        let t = progress * self.total();
        let (from, to) = self.moves();
        damped(phase(t, from, to), (to - from) / 1000.0)
    }

    /// The old picture over the window as it moves: it fades on the way.
    pub fn picture_alpha(self, progress: f32) -> f32 {
        let t = progress * self.total();
        let (from, to) = self.moves();
        1.0 - Easing::OutCubic.apply(phase(t, from, to))
    }
}

/// Lines of a zone's grid, global: what it is snapped apart from.
#[derive(Debug, Clone, PartialEq)]
struct Guide {
    output: String,
    /// Each one logical pixel wide, dashed along its length.
    lines: Vec<Rectangle<f64, Logical>>,
    since: Duration,
    kind: GuideKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideKind {
    /// Dragged into the zone: a flash.
    Flash,
    /// Snapped: it shows, pulses and fades as the window springs in.
    Snap,
}

impl Guide {
    fn alpha(&self, now: Duration) -> (f32, f32) {
        let t = millis(self.since, now);
        match self.kind {
            GuideKind::Flash => {
                let line = 1.0 - Easing::OutCubic.apply(t / FLASH_MS);
                (line, 0.3 * line)
            }
            GuideKind::Snap => {
                let line = if t < 80.0 {
                    t / 80.0
                } else {
                    1.0 - Easing::OutCubic.apply(phase(t, 250.0, 380.0))
                };
                let pulse = (std::f32::consts::PI * phase(t, 0.0, 250.0)).sin();
                (line, 0.35 * pulse)
            }
        }
    }

    fn is_running(&self, now: Duration) -> bool {
        millis(self.since, now)
            < match self.kind {
                GuideKind::Flash => FLASH_MS,
                GuideKind::Snap => 380.0,
            }
    }
}

/// Where a dragged window was lately, global, oldest first.
#[derive(Debug, Clone, Default)]
struct Trail {
    samples: Vec<(Duration, Point<f64, Logical>)>,
    /// Still held: kept while it rests under the pointer.
    held: bool,
}

impl Trail {
    /// Where it was drawn at `when`: its last place by then.
    fn at(&self, when: Duration) -> Option<Point<f64, Logical>> {
        self.samples
            .iter()
            .rev()
            .find(|(t, _)| *t <= when)
            .or(self.samples.first())
            .map(|(_, p)| *p)
    }

    fn last(&self) -> Option<Duration> {
        self.samples.last().map(|(t, _)| *t)
    }
}

/// A window being resized: its readout, drawn again when its size changes.
struct Resizing {
    id: WindowId,
    pointer: Point<f64, Logical>,
    readout: Option<Panel<Readout>>,
}

#[derive(Default)]
pub struct Motions {
    guides: Vec<Guide>,
    trails: BTreeMap<WindowId, Trail>,
    resizing: Option<Resizing>,
    /// Windows just resized, and when they were let go.
    resized: Vec<(WindowId, Duration)>,
}

impl std::fmt::Debug for Motions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Motions")
            .field("guides", &self.guides.len())
            .field("trails", &self.trails.len())
            .field("resizing", &self.resizing.as_ref().map(|r| r.id))
            .finish_non_exhaustive()
    }
}

/// The lines a zone of `area` is snapped apart from, with `gap` between
/// zones: the middle for halves, the middle both ways for quarters, the
/// area's edges for the whole of it.
fn zone_lines(zone: Zone, area: hocus_core::Rect, gap: u32) -> Vec<Rectangle<f64, Logical>> {
    let gap = i32::try_from(gap).unwrap_or(0);
    let (x, y, w, h) = (
        f64::from(area.x),
        f64::from(area.y),
        f64::from(area.width),
        f64::from(area.height),
    );
    // In the middle of the gap, as `Zone::rect` shares it out.
    let middle_x = f64::from(area.x + (area.width - gap) / 2) + f64::from(gap) / 2.0;
    let middle_y = f64::from(area.y + (area.height - gap) / 2) + f64::from(gap) / 2.0;
    let vertical = |at: f64| Rectangle::new((at.floor(), y).into(), (1.0, h).into());
    let horizontal = |at: f64| Rectangle::new((x, at.floor()).into(), (w, 1.0).into());
    match zone {
        Zone::Left | Zone::Right => vec![vertical(middle_x)],
        Zone::TopLeft | Zone::TopRight | Zone::BottomLeft | Zone::BottomRight => {
            vec![vertical(middle_x), horizontal(middle_y)]
        }
        Zone::Top => vec![
            horizontal(y),
            horizontal(y + h - 1.0),
            vertical(x),
            vertical(x + w - 1.0),
        ],
    }
}

/// A plain colour in `rect` (output-local), its corners cut as a window's:
/// the same element from frame to frame for the same place in the code.
#[track_caller]
pub(crate) fn tint<R>(
    layers: &Layers,
    rect: Rectangle<f64, Logical>,
    color: Color,
    alpha: f32,
    radius: f64,
    scale: f64,
) -> Option<SceneElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let geometry = rect.to_physical_precise_round(scale);
    if geometry.size.w <= 0 || geometry.size.h <= 0 || alpha <= 0.0 {
        return None;
    }
    let a = alpha.clamp(0.0, 1.0);
    let c = |v: u8| f32::from(v) / 255.0 * a;
    let solid = layers.solid(
        std::panic::Location::caller(),
        geometry,
        Color32F::new(c(color.r), c(color.g), c(color.b), a),
        scale,
    );
    let radius = (radius.min(rect.size.h / 2.0).min(rect.size.w / 2.0) * scale).round() as i32;
    Some(SceneElement::Tint(Rounded::new(solid, geometry, radius)))
}

impl<R> Brush<'_, R>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    /// A disc of `radius` about `centre`, in rows two pixels tall.
    pub(crate) fn disc(
        &mut self,
        centre: Point<f64, Logical>,
        radius: f64,
        color: Color,
        alpha: f32,
    ) {
        if radius < 0.5 {
            return;
        }
        let mut y = -radius;
        while y < radius {
            let mid = y + 1.0;
            let half = (radius * radius - mid * mid).max(0.0).sqrt();
            self.solid(
                Rectangle::new(
                    (centre.x - half, centre.y + y).into(),
                    (2.0 * half, 2.0_f64.min(radius - y)).into(),
                ),
                color,
                alpha,
            );
            y += 2.0;
        }
    }

    /// A line one pixel wide, in dashes four pixels long and four apart,
    /// over a phosphor bloom on the GPU, a faint glow three pixels wide
    /// otherwise.
    fn dashed(&mut self, line: Rectangle<f64, Logical>, color: Color, alpha: f32, glow: f32) {
        let vertical = line.size.h > line.size.w;
        let length = if vertical { line.size.h } else { line.size.w };
        let mut at = 0.0;
        while at < length {
            let dash = 4.0_f64.min(length - at);
            let rect = if vertical {
                Rectangle::new((line.loc.x, line.loc.y + at).into(), (1.0, dash).into())
            } else {
                Rectangle::new((line.loc.x + at, line.loc.y).into(), (dash, 1.0).into())
            };
            self.solid(rect, color, alpha);
            at += 8.0;
        }
        if self.bloom(line, glow) {
            return;
        }
        let halo = if vertical {
            Rectangle::new((line.loc.x - 1.0, line.loc.y).into(), (3.0, length).into())
        } else {
            Rectangle::new((line.loc.x, line.loc.y - 1.0).into(), (length, 3.0).into())
        };
        self.solid(halo, GLOW, glow);
    }

    /// A bright line two pixels tall from `left` to `right` at `y`, glowing:
    /// blooming on the GPU.
    fn beam(&mut self, left: f64, right: f64, y: f64, alpha: f32) {
        let w = (right - left).max(1.0);
        let line = Rectangle::new((left, y).into(), (w, 2.0).into());
        self.solid(line, HEAD, alpha);
        if self.bloom(line, 0.3 * alpha) {
            return;
        }
        self.solid(
            Rectangle::new((left - 2.0, y - 2.0).into(), (w + 4.0, 6.0).into()),
            GLOW,
            0.3 * alpha,
        );
    }
}

/// Where an effect is drawn: with the windows, or over the panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Picture,
    Light,
}

/// A window going into, or coming out of, its dock icon in the Matrix
/// theme: its picture, where it is (global) and where it goes.
pub(crate) struct Switch<'a> {
    pub picture: &'a Snapshot,
    pub window: Rectangle<f64, Logical>,
    pub target: Rectangle<f64, Logical>,
    /// Its corners, logical pixels: 0 when undecorated.
    pub radius: f64,
}

impl State {
    /// Whether windows move in the Matrix theme's own way: in it, without
    /// reduced motion, and not in tests that ask for instant motion.
    pub(crate) fn matrix_moves(&self) -> bool {
        !self.instant_motion
            && !self.config.appearance.reduced_motion
            && self.look() == Look::Matrix
    }

    /// The CRT going off into the dock (`restoring`: coming back on), `t`
    /// milliseconds in; `origin` is the output's corner, global. The
    /// window's `Layer::Picture` goes with the windows, its `Layer::Light`
    /// over the dock it dashes into. Front first.
    pub(crate) fn matrix_switch_elements<R>(
        &self,
        renderer: &mut R,
        switch: &Switch<'_>,
        (t, restoring): (f32, bool),
        layer: Layer,
        origin: Point<f64, Logical>,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let w = Rectangle::new(switch.window.loc - origin, switch.window.size);
        let icon = switch.target.loc - origin + switch.target.size.to_point().downscale(2.0);
        let (left, right, bottom) = (w.loc.x, w.loc.x + w.size.w, w.loc.y + w.size.h);
        let mut out = Vec::new();
        // The picture in `rect`, under green, its corners cut.
        let picture = |out: &mut Vec<SceneElement<R>>,
                       renderer: &R,
                       rect: Rectangle<f64, Logical>,
                       green: f32,
                       white: f32| {
            out.extend(tint(
                &self.matrix_layers,
                rect,
                HEAD,
                white,
                switch.radius,
                scale,
            ));
            out.extend(tint(
                &self.matrix_layers,
                rect,
                GREEN,
                green,
                switch.radius,
                scale,
            ));
            let radius = (switch.radius.min(rect.size.h / 2.0) * scale).round() as i32;
            let clip = rect.to_physical_precise_round(scale);
            out.extend(
                switch
                    .picture
                    .stretched(renderer, rect, scale, 1.0)
                    .into_iter()
                    .map(|e| SceneElement::Snapshot(Rounded::new(e, clip, radius))),
            );
        };
        let mut brush = self.brush(renderer, scale);
        let light = layer == Layer::Light;
        if restoring {
            let middle = w.loc.y + w.size.h / 2.0;
            if t < 150.0 && light {
                // A dot at the icon grows into a line racing to the window.
                let q = Easing::OutCubic.apply(t / 150.0);
                brush.beam(
                    lerp(icon.x - 1.0, left, q),
                    lerp(icon.x + 1.0, right, q),
                    lerp(icon.y - 1.0, middle - 1.0, q),
                    1.0,
                );
                brush.disc(icon, 6.0 * f64::from(1.0 - q), HEAD, 1.0 - q);
                out.extend(brush.take());
            } else if t >= 150.0 && !light {
                // The line opens up and down into the window, a scanline
                // sweeping it once as it warms up; its green settles.
                let p = phase(t, 150.0, 350.0);
                let h = (w.size.h * f64::from(damped(p, 0.2))).max(2.0);
                let rect = Rectangle::new((left, middle - h / 2.0).into(), (w.size.w, h).into());
                if t < 350.0 {
                    let y = rect.loc.y + f64::from(p) * rect.size.h;
                    brush.beam(left, right, y - 1.0, 0.9);
                }
                out.extend(brush.take());
                let green = if t < 350.0 {
                    0.15 + 0.45 * (1.0 - p)
                } else {
                    0.15 * (1.0 - phase(t, 350.0, 400.0))
                };
                picture(&mut out, brush.renderer, rect, green, 0.0);
            }
            return out;
        }
        if !light {
            if t < 200.0 {
                // Crushed to its bottom edge, brightening, greener.
                let p = Easing::InCubic.apply(t / 200.0);
                let h = lerp(w.size.h, 2.0, p);
                let rect = Rectangle::new((left, bottom - h).into(), (w.size.w, h).into());
                picture(&mut out, brush.renderer, rect, 0.6 * p, 0.6 * p.powi(3));
            }
            return out;
        }
        // Afterimages of the dashing line, fainter the older.
        let line = |t: f32| {
            let q = out_quad(phase(t, 200.0, 350.0));
            (
                lerp(left, icon.x - 1.0, q),
                lerp(right, icon.x + 1.0, q),
                lerp(bottom - 2.0, icon.y - 1.0, q),
            )
        };
        for (ago, alpha) in [(25.0, 0.5), (50.0, 0.25), (75.0, 0.12), (100.0, 0.06)] {
            let then = t - ago;
            if !(200.0..350.0).contains(&then) {
                continue;
            }
            let (l, r, y) = line(then);
            brush.solid(
                Rectangle::new((l, y + 0.5).into(), ((r - l).max(1.0), 1.0).into()),
                GLOW,
                alpha,
            );
        }
        if t >= 350.0 {
            // A flash where it went.
            let fade = 1.0 - phase(t, 350.0, 400.0);
            brush.disc(icon, 8.0, HEAD, fade);
            brush.disc(icon, 20.0, GLOW, 0.5 * fade);
        } else if t >= 200.0 {
            let (l, r, y) = line(t);
            brush.beam(l, r, y, 1.0);
        }
        out.extend(brush.take());
        out
    }

    /// The circuit traces of a window going from `from` to `to` (global),
    /// `progress` of the way through, and the edges flashing as it
    /// arrives. Front first.
    pub fn matrix_shift_elements<R>(
        &self,
        renderer: &mut R,
        (from, to): (Rectangle<f64, Logical>, Rectangle<f64, Logical>),
        progress: f32,
        seed: u64,
        origin: Point<f64, Logical>,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let t = progress * Shift::Maximize.total();
        let corners = |r: Rectangle<f64, Logical>| {
            let (x0, y0) = (r.loc.x - origin.x, r.loc.y - origin.y);
            let (x1, y1) = (x0 + r.size.w - 1.0, y0 + r.size.h - 1.0);
            [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
        };
        let mut brush = self.brush(renderer, scale);
        // Sparks where the traces reach, each for 100 ms.
        if (200.0..300.0).contains(&t) {
            let age = f64::from(t - 200.0) / 1000.0;
            let mut rng = Rng::new(seed);
            for (x, y) in corners(to) {
                for _ in 0..4 + rng.below(3) {
                    let angle = f64::from(rng.range(0.0, std::f32::consts::TAU));
                    let speed = f64::from(rng.range(60.0, 160.0));
                    brush.solid(
                        Rectangle::new(
                            (x + angle.cos() * speed * age, y + angle.sin() * speed * age).into(),
                            (2.0, 2.0).into(),
                        ),
                        if rng.unit() < 0.5 { HEAD } else { GLOW },
                        1.0 - phase(t, 200.0, 300.0),
                    );
                }
            }
        }
        // The new edges flash, four pixels wide.
        if (250.0..350.0).contains(&t) {
            let flash = 1.0 - Easing::OutCubic.apply(phase(t, 250.0, 350.0));
            let r = Rectangle::new(to.loc - origin, to.size);
            let (x, y, w, h) = (r.loc.x, r.loc.y, r.size.w, r.size.h);
            for (rx, ry, rw, rh) in [
                (x, y, w, 4.0),
                (x, y + h - 4.0, w, 4.0),
                (x, y + 4.0, 4.0, h - 8.0),
                (x + w - 4.0, y + 4.0, 4.0, h - 8.0),
            ] {
                brush.solid(
                    Rectangle::new((rx, ry).into(), (rw, rh).into()),
                    GLOW,
                    flash,
                );
            }
        }
        // From each corner to where it goes, across then along, blooming
        // on the GPU.
        let alpha = 1.0 - phase(t, 200.0, 350.0);
        if alpha > 0.0 {
            let reach = f64::from(out_quad(t / 200.0));
            for ((ax, ay), (bx, by)) in corners(from).into_iter().zip(corners(to)) {
                let (dx, dy) = (bx - ax, by - ay);
                let length = (dx.abs() + dy.abs()) * reach;
                let across = length.min(dx.abs());
                let along = (length - dx.abs()).max(0.0);
                let x_end = ax + across * dx.signum();
                let y_end = ay + along * dy.signum();
                let first =
                    Rectangle::new((ax.min(x_end), ay).into(), (across.max(1.0), 1.0).into());
                brush.solid(first, GLOW, alpha);
                brush.bloom(first, 0.3 * alpha);
                if along > 0.0 {
                    let then = Rectangle::new((bx, ay.min(y_end)).into(), (1.0, along).into());
                    brush.solid(then, GLOW, alpha);
                    brush.bloom(then, 0.3 * alpha);
                }
                if t < 200.0 {
                    let tip = if along > 0.0 {
                        (bx, y_end)
                    } else {
                        (x_end, ay)
                    };
                    brush.solid(
                        Rectangle::new((tip.0 - 1.0, tip.1 - 1.0).into(), (3.0, 3.0).into()),
                        HEAD,
                        1.0,
                    );
                }
            }
        }
        brush.finish()
    }

    /// The guide lines of `zone` on `output` show: dragged into it, or
    /// snapped to it.
    pub fn matrix_guide(&mut self, output: &Output, zone: Zone, kind: GuideKind) {
        if !self.matrix_moves() {
            return;
        }
        let area = rect(self.work_area(output));
        let lines = zone_lines(zone, area, self.config.windows.gaps.inner);
        let name = output.name();
        self.matrix_motions
            .guides
            .retain(|g| g.output != name || g.kind != kind);
        self.matrix_motions.guides.push(Guide {
            output: name,
            lines,
            since: self.clock.now(),
            kind,
        });
        self.needs_redraw = true;
    }

    /// A drag begins: the window's trail starts where it is.
    pub fn matrix_held(&mut self, id: WindowId, at: Point<i32, Logical>) {
        self.matrix_motions.trails.remove(&id);
        if self.matrix_moves() {
            let trail = Trail {
                samples: vec![(self.clock.now(), at.to_f64())],
                held: true,
            };
            self.matrix_motions.trails.insert(id, trail);
        }
    }

    /// The drag ends: the trail goes once it has faded, a glide included.
    pub fn matrix_let_go(&mut self, id: WindowId) {
        if let Some(trail) = self.matrix_motions.trails.get_mut(&id) {
            trail.held = false;
        }
    }

    /// The dragged window is now at `at`, global.
    pub fn matrix_moved(&mut self, id: WindowId, at: Point<i32, Logical>) {
        let now = self.clock.now();
        let Some(trail) = self.matrix_motions.trails.get_mut(&id) else {
            return;
        };
        // What the oldest ghost may still need, and no more.
        let keep = now.saturating_sub(Duration::from_millis(60));
        let older = trail.samples.iter().rposition(|(t, _)| *t < keep);
        if let Some(i) = older {
            trail.samples.drain(..i);
        }
        trail.samples.push((now, at.to_f64()));
        self.needs_redraw = true;
    }

    /// The ghosts of a dragged window at its last places, behind it, front
    /// first; `origin` is the output's corner, global.
    pub fn matrix_trail_elements<R>(
        &self,
        renderer: &R,
        id: WindowId,
        window: &Window,
        origin: Point<f64, Logical>,
        radius: f64,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let (Some(trail), Some(here)) = (
            self.matrix_motions.trails.get(&id),
            self.space.element_location(window),
        ) else {
            return Vec::new();
        };
        let now = self.clock.now();
        let size = window.geometry().size.to_f64();
        // Where it was drawn lately and is no more: none at rest.
        let ghosts: Vec<(Rectangle<f64, Logical>, f32)> = TRAIL
            .iter()
            .filter_map(|(ago, alpha)| {
                let at = trail.at(now.saturating_sub(Duration::from_secs_f32(ago / 1000.0)))?;
                let off = at - here.to_f64();
                (off.x.hypot(off.y) >= 1.0).then(|| (Rectangle::new(at - origin, size), *alpha))
            })
            .collect();
        if ghosts.is_empty() {
            return Vec::new();
        }
        let Some(picture) = self.picture_of(window) else {
            return Vec::new();
        };
        let r = (radius * scale).round() as i32;
        let mut out = Vec::new();
        for (rect, alpha) in ghosts {
            out.extend(tint(
                &self.matrix_layers,
                rect,
                GREEN,
                0.7 * alpha,
                radius,
                scale,
            ));
            let clip = rect.to_physical_precise_round(scale);
            out.extend(
                picture
                    .stretched(renderer, rect, scale, alpha)
                    .into_iter()
                    .map(|e| SceneElement::Snapshot(Rounded::new(e, clip, r))),
            );
        }
        out
    }

    /// A resize goes on, the pointer at `pointer`: the window glows and
    /// its size reads out beside the pointer.
    pub fn matrix_resizing(&mut self, id: WindowId, pointer: Point<f64, Logical>) {
        if self.look() != Look::Matrix {
            return;
        }
        match &mut self.matrix_motions.resizing {
            Some(r) if r.id == id => r.pointer = pointer,
            _ => {
                self.matrix_motions.resizing = Some(Resizing {
                    id,
                    pointer,
                    readout: None,
                });
            }
        }
        self.needs_redraw = true;
    }

    /// The resize ends: the glow flashes once.
    pub fn matrix_resized(&mut self, id: WindowId) {
        if self
            .matrix_motions
            .resizing
            .take_if(|r| r.id == id)
            .is_none()
        {
            return;
        }
        if self.matrix_moves() {
            let now = self.clock.now();
            self.matrix_motions.resized.retain(|(w, _)| *w != id);
            self.matrix_motions.resized.push((id, now));
        }
        self.needs_redraw = true;
    }

    /// The ring a window has while resized and just after, in place of its
    /// focus ring: the glow, then a bright flash fading.
    pub fn matrix_ring(&self, id: WindowId) -> Option<([u8; 3], f32)> {
        if self
            .matrix_motions
            .resizing
            .as_ref()
            .is_some_and(|r| r.id == id)
        {
            return Some(([GLOW.r, GLOW.g, GLOW.b], 1.0));
        }
        let now = self.clock.now();
        let (_, since) = self.matrix_motions.resized.iter().find(|(w, _)| *w == id)?;
        let t = millis(*since, now);
        (t < RESIZED_MS).then(|| {
            (
                [HEAD.r, HEAD.g, HEAD.b],
                1.0 - Easing::OutCubic.apply(t / RESIZED_MS),
            )
        })
    }

    /// Whether a Matrix motion of its own moves at `now`.
    pub fn matrix_motions_moving(&self, now: Duration) -> bool {
        let m = &self.matrix_motions;
        m.guides.iter().any(|g| g.is_running(now))
            || m.trails.values().any(|t| {
                t.last()
                    .is_some_and(|last| millis(last, now) <= TRAIL[TRAIL.len() - 1].0)
            })
            || m.resized.iter().any(|(_, t)| millis(*t, now) < RESIZED_MS)
    }

    /// After each dispatch: motions done go; the readout says the resized
    /// window's size, drawn again only when it changes.
    pub fn refresh_matrix_motions(&mut self) {
        let now = self.clock.now();
        let windows = &self.windows;
        let m = &mut self.matrix_motions;
        m.guides.retain(|g| g.is_running(now));
        m.resized
            .retain(|(id, t)| millis(*t, now) < RESIZED_MS && windows.contains_key(id));
        m.trails.retain(|id, t| {
            windows.contains_key(id)
                && (t.held
                    || t.last()
                        .is_some_and(|last| millis(last, now) <= TRAIL[TRAIL.len() - 1].0 + 20.0))
        });
        let Some(resizing) = &self.matrix_motions.resizing else {
            return;
        };
        let Some(window) = self.windows.get(&resizing.id).map(|m| m.window.clone()) else {
            self.matrix_motions.resizing = None;
            return;
        };
        let size = window.geometry().size;
        let text = readout::text(size.w, size.h);
        let scale = self
            .output_of(&window)
            .map_or(1.0, |o| o.current_scale().fractional_scale());
        let Some(resizing) = &mut self.matrix_motions.resizing else {
            return;
        };
        let panel = resizing.readout.get_or_insert_with(|| {
            Panel::new(Readout::new(
                Theme::builtin(ThemeName::Matrix),
                scale as f32,
            ))
        });
        if panel.view.set(&text) {
            let drawn = panel.view.draw(now);
            panel.upload(&drawn);
            self.needs_redraw = true;
        }
    }

    /// The guides and the readout on `output`, over the windows, front
    /// first.
    pub fn matrix_overlay_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let m = &self.matrix_motions;
        if m.guides.is_empty() && m.resizing.is_none() {
            return Vec::new();
        }
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let origin = area.loc.to_f64();
        let scale = output.current_scale().fractional_scale();
        let mut out = Vec::new();
        if let Some(r) = &m.resizing
            && let Some(panel) = &r.readout
            && area.to_f64().contains(r.pointer)
        {
            // Beside the pointer, inside the screen.
            let size = readout::SIZE;
            let (w, h) = (f64::from(size.w), f64::from(size.h));
            let local = r.pointer - origin;
            let x = (local.x + 18.0)
                .min(f64::from(area.size.w) - w - 4.0)
                .max(0.0);
            let y = (local.y + 18.0)
                .min(f64::from(area.size.h) - h - 4.0)
                .max(0.0);
            let at = Point::<f64, Logical>::from((x, y)).to_i32_round();
            out.extend(panel.element(renderer, output, at).map(SceneElement::Frame));
        }
        let now = self.clock.now();
        let mut brush = self.brush(renderer, scale);
        for guide in m.guides.iter().filter(|g| g.output == output.name()) {
            let (line, glow) = guide.alpha(now);
            // A flash is brighter, to read over the zone's own outline.
            let color = match guide.kind {
                GuideKind::Flash => HEAD,
                GuideKind::Snap => GLOW,
            };
            for l in &guide.lines {
                brush.dashed(Rectangle::new(l.loc - origin, l.size), color, line, glow);
            }
        }
        out.extend(brush.take());
        out
    }
}

/// How much of a title the bar shows as a window materializes: none until
/// the window shows, then typed in, quickly enough to be done with it.
pub fn typed(len: usize, t: f32) -> usize {
    let t = t - 250.0;
    if t < 0.0 {
        return 0;
    }
    let per = (240.0 / len.max(1) as f32).min(8.0);
    ((t / per) as usize + 1).min(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_of(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn a_shift_springs_past_its_place_and_settles_there() {
        let shift = Shift::Maximize;
        assert_eq!(shift.geometry(0.0), 0.0);
        assert_eq!(shift.geometry(90.0 / 350.0), 0.0, "the traces race first");
        let peak = (0..=100)
            .map(|i| shift.geometry(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.01 && peak < 1.08, "a slight overshoot: {peak}");
        assert_eq!(shift.geometry(1.0), 1.0);
        assert_eq!(shift.picture_alpha(0.0), 1.0);
        assert_eq!(shift.picture_alpha(1.0), 0.0);
        assert_eq!(Shift::Snap.duration(), Duration::from_millis(380));
    }

    #[test]
    fn halves_snap_apart_at_the_middle_of_the_gap_quarters_both_ways() {
        let area = hocus_core::Rect {
            x: 8,
            y: 8,
            width: 1264,
            height: 784,
        };
        let left = zone_lines(Zone::Left, area, 8);
        assert_eq!(left, vec![rect_of(640.0, 8.0, 1.0, 784.0)]);
        // The left half ends where the gap starts.
        let half = Zone::Left.rect(area, 8);
        assert_eq!(half.x + half.width + 4, 640);
        assert_eq!(zone_lines(Zone::BottomRight, area, 8).len(), 2);
        assert_eq!(zone_lines(Zone::Top, area, 8).len(), 4);
    }

    #[test]
    fn a_trail_keeps_where_the_window_was_drawn() {
        let ms = Duration::from_millis;
        let p = |x: f64| Point::from((x, 0.0));
        let trail = Trail {
            samples: vec![(ms(0), p(0.0)), (ms(20), p(10.0)), (ms(40), p(20.0))],
            held: true,
        };
        assert_eq!(trail.at(ms(39)), Some(p(10.0)));
        assert_eq!(trail.at(ms(40)), Some(p(20.0)));
        assert_eq!(trail.at(ms(100)), Some(p(20.0)));
    }

    #[test]
    fn a_title_is_typed_in_once_the_window_shows() {
        assert_eq!(typed(10, 100.0), 0);
        assert_eq!(typed(10, 250.0), 1);
        assert_eq!(typed(10, 290.0), 6);
        assert_eq!(typed(10, 400.0), 10);
        assert_eq!(typed(200, 489.0), 200, "a long one types faster");
    }
}
