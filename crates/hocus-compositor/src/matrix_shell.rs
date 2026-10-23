//! The Matrix theme's shell (docs/design/matrix-theme.md, §9 to §16): what
//! its rain, glyphs and lights are drawn with, and the moments of the
//! desktop switch, the toasts, the column and the launcher; the switcher,
//! the lock and the boot screen are in `matrix_screens`.
//!
//! As the window effects (`matrix`, `matrix_windows`), everything is drawn
//! with the elements every renderer has, so that pixman and the scenarios
//! show it: rows and columns cut from the sheet of glyphs, plain colours
//! and parts of the shell's own pictures. Each moment is a closed form of
//! the time since it began, evaluated when the scene is drawn: nothing is
//! queued per frame, and once it is over nothing more is drawn. Rain that
//! stays (behind the lock screen, the launcher) stands still.

use std::time::Duration;

use hocus_shell::{
    launcher::Parts,
    matrix::{BLACK, CELL, GREEN, HEAD, PADLOCK, PADLOCK_CELLS, Rng, hash, letter},
};
use hocus_ui::{Color, motion::Easing};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            surface::WaylandSurfaceRenderElement,
        },
    },
    desktop::Window,
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale, Size},
};

use crate::{
    State,
    decoration::Rounded,
    matrix::{Brush, SHEET_COLS, SHEET_ROWS, in_quad, millis, phase},
    matrix_windows::{damped, out_quad},
    scene::SceneElement,
};

pub(crate) type Rect = Rectangle<f64, Logical>;

/// A rectangle; nothing (no width or height) where it would be less.
pub(crate) fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rectangle::new((x, y).into(), (w.max(0.0), h.max(0.0)).into())
}

/// A rectangle of the shell's (`hocus_ui`), moved by `by`.
pub(crate) fn ui_rect(r: hocus_ui::Rect, by: Point<f64, Logical>) -> Rect {
    rect(
        by.x + f64::from(r.x),
        by.y + f64::from(r.y),
        f64::from(r.w),
        f64::from(r.h),
    )
}

/// What is left of `of` once `holes` are taken out of it.
pub(crate) fn subtract(of: Rect, holes: &[Rect]) -> Vec<Rect> {
    let mut parts = vec![of];
    for hole in holes {
        let mut next = Vec::new();
        for p in parts {
            let Some(cut) = p.intersection(*hole) else {
                next.push(p);
                continue;
            };
            let (px1, py1) = (p.loc.x + p.size.w, p.loc.y + p.size.h);
            let (cx1, cy1) = (cut.loc.x + cut.size.w, cut.loc.y + cut.size.h);
            for r in [
                rect(p.loc.x, p.loc.y, p.size.w, cut.loc.y - p.loc.y),
                rect(p.loc.x, cy1, p.size.w, py1 - cy1),
                rect(p.loc.x, cut.loc.y, cut.loc.x - p.loc.x, cut.size.h),
                rect(cx1, cut.loc.y, px1 - cx1, cut.size.h),
            ] {
                if r.size.w > 0.5 && r.size.h > 0.5 {
                    next.push(r);
                }
            }
        }
        parts = next;
    }
    parts
}

/// Part `part` (logical, from the picture's corner) of a picture of the
/// shell drawn at `scale` with its corner at `at` (output-local), as it is
/// there.
pub(crate) fn buffer_part<R>(
    renderer: &mut R,
    buffer: &MemoryRenderBuffer,
    scale: f64,
    at: Point<f64, Logical>,
    part: Rect,
    alpha: f32,
) -> Option<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let size = part.size.to_i32_round::<i32>();
    if size.w <= 0 || size.h <= 0 || alpha <= 0.0 {
        return None;
    }
    // In the picture's pixels.
    let src = Rectangle::new(
        (part.loc.x * scale, part.loc.y * scale).into(),
        (part.size.w * scale, part.size.h * scale).into(),
    );
    let location: Point<f64, Physical> = (at + part.loc).to_physical(scale);
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        location,
        buffer,
        (alpha < 1.0).then_some(alpha.clamp(0.0, 1.0)),
        Some(src),
        Some(size),
        Kind::Unspecified,
    )
    .inspect_err(|err| tracing::warn!(?err, "could not draw part of the shell"))
    .ok()
}

/// A window's surfaces drawn where it is (`frame`, output-local, its
/// geometry's corner), its corners cut.
pub(crate) fn window_elements<R>(
    renderer: &mut R,
    window: &Window,
    frame: Rect,
    radius: f64,
    scale: f64,
    alpha: f32,
) -> Vec<SceneElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let geometry = window.geometry();
    let origin = (frame.loc - geometry.loc.to_f64()).to_physical_precise_round(scale);
    let clip = frame.to_physical_precise_round(scale);
    let r = (radius * scale).round() as i32;
    let content: Vec<WaylandSurfaceRenderElement<R>> =
        window.render_elements(renderer, origin, Scale::from(scale), alpha);
    content
        .into_iter()
        .map(|e| SceneElement::Window(Rounded::new(e, clip, r).at_scale(Scale::from(scale))))
        .collect()
}

/// Rain falling in columns `step` apart: each column has `drops` drops,
/// each there with the chance `density`, 6 to 24 glyphs long, falling at
/// about `speed` logical pixels a second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rain {
    pub seed: u64,
    pub density: f32,
    pub speed: f32,
    pub step: f64,
    pub drops: u32,
}

impl Rain {
    /// Every column, two drops each, fast: the lock's and the boot's.
    pub const fn dense(seed: u64) -> Self {
        Self {
            seed,
            density: 1.0,
            speed: 520.0,
            step: 16.0,
            drops: 2,
        }
    }

    /// A drop in some columns, slow: behind the switcher and the launcher.
    pub const fn sparse(seed: u64, density: f32) -> Self {
        Self {
            seed,
            density,
            speed: 90.0,
            step: 28.0,
            drops: 1,
        }
    }

    /// Columns across `width`.
    fn columns(&self, width: f64) -> u32 {
        (width / self.step).ceil().max(1.0) as u32
    }
}

/// How a bitmap of glyphs is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ink {
    /// Glyphs across a cell.
    pub wide: u32,
    /// Bright heads on a green block (letters), or the rain's glyphs.
    pub bright: bool,
}

/// Strength of a drop's tail, from its top: fainter the older.
const TAIL: [f32; 3] = [0.3, 0.55, 0.85];

impl<R> Brush<'_, R>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    /// A drop of `len` glyphs from `top` at `x`, its head the last, shown
    /// between `from` and `to` only; `pick` chooses its glyphs.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn drop_at(
        &mut self,
        x: f64,
        top: f64,
        len: u32,
        pick: u64,
        (from, to): (f64, f64),
        head_color: bool,
        alpha: f32,
    ) {
        let ch = f64::from(CELL.1);
        let col = (pick % u64::from(SHEET_COLS)) as u32;
        let row = ((pick >> 8) % u64::from(SHEET_ROWS - 1)) as u32;
        let tail = len.saturating_sub(1);
        for (s, strength) in TAIL.iter().enumerate() {
            let first = tail * s as u32 / 3;
            let last = tail * (s as u32 + 1) / 3;
            if last <= first {
                continue;
            }
            // Whole glyphs from where it shows.
            let mut j = first;
            while j < last && top + f64::from(j) * ch < from {
                j += 1;
            }
            let y = top + f64::from(j) * ch;
            let end = (top + f64::from(last) * ch).min(to);
            if j < last && end > y {
                self.column(Point::from((x, y)), end - y, col, row + j, alpha * strength);
            }
        }
        let y = top + f64::from(tail) * ch;
        if y >= from && y < to {
            let h = ch.min(to - y);
            let cell = if head_color {
                (col, SHEET_ROWS - 1)
            } else {
                (col, (row + tail) % (SHEET_ROWS - 1))
            };
            self.cut(
                Point::from((x, y)),
                Size::from((f64::from(CELL.0), h)),
                cell,
                alpha,
            );
        }
    }

    /// `rain` at `t` milliseconds in `area`, each column above `bottom` of
    /// it only (global to the output, logical).
    pub(crate) fn rain(
        &mut self,
        area: Rect,
        rain: &Rain,
        t: f32,
        alpha: f32,
        bottom: impl Fn(u32) -> f64,
    ) {
        if alpha <= 0.0 {
            return;
        }
        let ch = f64::from(CELL.1);
        let inset = ((rain.step - f64::from(CELL.0)) / 2.0).max(0.0);
        for i in 0..rain.columns(area.size.w) {
            let x = area.loc.x + f64::from(i) * rain.step + inset;
            let to = bottom(i).min(area.loc.y + area.size.h);
            if to <= area.loc.y {
                continue;
            }
            for k in 0..rain.drops {
                let mut rng = Rng::new(hash(&[rain.seed, i.into(), k.into()]));
                if rng.unit() >= rain.density {
                    continue;
                }
                let len = 6 + rng.below(19) as u32;
                let speed = f64::from(rain.speed * rng.range(0.7, 1.3));
                let period = area.size.h + f64::from(len) * ch;
                let start = (f64::from(rng.unit()) + f64::from(k) / f64::from(rain.drops)) * period;
                let head = (start + speed * f64::from(t) / 1000.0).rem_euclid(period);
                let top = area.loc.y + head - f64::from(len) * ch;
                let pick = rng.next_u64();
                self.drop_at(x, top, len, pick, (area.loc.y, to), true, alpha);
            }
        }
    }

    /// Rain cascading down `area` as a front: each column's front reaches
    /// the bottom at `p` = 1 (a little sooner or later column by column),
    /// black above it, the rain falling there; the leading drops at the
    /// front. Over what is below the front, nothing.
    pub(crate) fn cascade(&mut self, area: Rect, rain: &Rain, p: f32, t: f32, alpha: f32) {
        let ch = f64::from(CELL.1);
        let inset = ((rain.step - f64::from(CELL.0)) / 2.0).max(0.0);
        let front = |i: u32| {
            let jitter = (hash(&[rain.seed, i.into(), 9]) % 1000) as f64 / 1000.0;
            area.loc.y + f64::from(p) * (area.size.h + 4.0 * ch) * (0.85 + 0.3 * jitter)
                - 2.0 * ch * jitter
        };
        let columns = rain.columns(area.size.w);
        for i in 0..columns {
            let y = front(i);
            let x = area.loc.x + f64::from(i) * rain.step + inset;
            let len = 10 + (hash(&[rain.seed, i.into(), 10]) % 10) as u32;
            let top = y - f64::from(len) * ch;
            let bottom = area.loc.y + area.size.h;
            self.drop_at(
                x,
                top,
                len,
                hash(&[rain.seed, i.into(), 11]),
                (area.loc.y, bottom),
                true,
                alpha,
            );
        }
        self.rain(area, rain, t, alpha, |i| front(i) - 12.0 * ch);
        // The black the rain falls on, column by column, behind it.
        for i in 0..columns {
            let x = area.loc.x + f64::from(i) * rain.step;
            let y = (front(i) - ch).min(area.loc.y + area.size.h);
            let w = rain.step.min(area.loc.x + area.size.w - x);
            if y > area.loc.y && w > 0.0 {
                self.solid(rect(x, area.loc.y, w, y - area.loc.y), BLACK, alpha);
            }
        }
    }

    /// Rows of glyphs over black in `area`, as far as `width` from its left
    /// edge, a bright cursor at their end while `cursor`.
    pub(crate) fn typed(&mut self, area: Rect, width: f64, seed: u64, alpha: f32, cursor: bool) {
        let ch = f64::from(CELL.1);
        let width = width.clamp(0.0, area.size.w);
        if width <= 0.0 || alpha <= 0.0 {
            return;
        }
        let green = SHEET_ROWS - 1;
        let mut behind = Vec::new();
        let mut y = area.loc.y;
        let mut j = 0u64;
        while y < area.loc.y + area.size.h {
            let h = ch.min(area.loc.y + area.size.h - y);
            let row = (hash(&[seed, j, 1]) % u64::from(green)) as u32;
            let col = (hash(&[seed, j, 2]) % u64::from(SHEET_COLS)) as u32;
            self.row(Point::from((area.loc.x, y)), width, h, row, col, alpha);
            behind.push(rect(area.loc.x, y, width, h));
            y += ch;
            j += 1;
        }
        if cursor {
            self.solid(
                rect(area.loc.x + width - 2.0, area.loc.y, 2.0, area.size.h),
                HEAD,
                alpha,
            );
        }
        for r in behind {
            self.solid(r, BLACK, alpha);
        }
    }

    /// Glyphs flying out of `area` from `count` places in it, `age` of
    /// their `life` milliseconds, accelerating, falling a little.
    pub(crate) fn scatter(&mut self, area: Rect, count: usize, seed: u64, age: f32, life: f32) {
        if !(0.0..life).contains(&age) {
            return;
        }
        let mut rng = Rng::new(seed);
        let p = age / life;
        let a = f64::from(age) / 1000.0;
        for _ in 0..count {
            let (u, v) = (f64::from(rng.unit()), f64::from(rng.unit()));
            let angle = f64::from(rng.range(0.0, std::f32::consts::TAU));
            let reach = f64::from(rng.range(40.0, 140.0));
            let glyph = rng.next_u64();
            let d = reach * f64::from(in_quad(p));
            let at = Point::from((
                area.loc.x + u * area.size.w + angle.cos() * d - f64::from(CELL.0) / 2.0,
                area.loc.y + v * area.size.h + angle.sin() * d + 160.0 * a * a
                    - f64::from(CELL.1) / 2.0,
            ));
            self.glyph(at, glyph, false, 1.0 - p);
        }
    }

    /// Glyphs falling out of each of `rows` (as letters of text would),
    /// one every 12 pixels across the first two thirds, `p` of the way.
    pub(crate) fn fall(&mut self, rows: &[Rect], seed: u64, p: f32, fall: f64) {
        let p = p.clamp(0.0, 1.0);
        if p >= 1.0 {
            return;
        }
        let drop = f64::from(in_quad(p));
        for (n, row) in rows.iter().enumerate() {
            let mut rng = Rng::new(hash(&[seed, n as u64]));
            let mut x = row.loc.x + 14.0;
            while x < row.loc.x + row.size.w * 0.66 {
                let speed = f64::from(rng.range(0.6, 1.4));
                let drift = f64::from(rng.range(-12.0, 12.0));
                let glyph = rng.next_u64();
                let at = Point::from((
                    x + drift * drop,
                    row.loc.y + (row.size.h - f64::from(CELL.1)) / 2.0 + speed * drop * fall,
                ));
                self.glyph(at, glyph, false, 1.0 - p);
                x += 12.0;
            }
        }
    }

    /// Cells set in `bits` (a row per entry, `cols` across, the highest
    /// bit on the left) as glyphs on black from `at`, to read over rain,
    /// each `ink.wide` glyphs across; letters bright on a green block.
    pub(crate) fn bitmap(
        &mut self,
        at: Point<f64, Logical>,
        (bits, cols): (&[u16], u32),
        seed: u64,
        ink: Ink,
        alpha: f32,
    ) {
        let (cw, ch) = (f64::from(CELL.0) * f64::from(ink.wide), f64::from(CELL.1));
        let green = SHEET_ROWS - 1;
        for (j, line) in bits.iter().enumerate() {
            let set = |i: u32| line >> (cols - 1 - i) & 1 == 1;
            let mut i = 0;
            while i < cols {
                if !set(i) {
                    i += 1;
                    continue;
                }
                let first = i;
                while i < cols && set(i) {
                    i += 1;
                }
                let row = if ink.bright {
                    green
                } else {
                    (hash(&[seed, j as u64, 1]) % u64::from(green)) as u32
                };
                let col =
                    (hash(&[seed, j as u64, 2]) % u64::from(SHEET_COLS)) as u32 + first * ink.wide;
                let at = Point::from((at.x + f64::from(first) * cw, at.y + j as f64 * ch));
                let width = f64::from(i - first) * cw;
                let block = rect(at.x, at.y, width, ch);
                self.row(at, width, ch, row, col, alpha);
                if ink.bright {
                    self.solid(block, GREEN, 0.22 * alpha);
                }
                self.solid(block, BLACK, alpha);
            }
        }
    }

    /// The padlock of glyphs with its corner at `at`.
    pub(crate) fn padlock(&mut self, at: Point<f64, Logical>, seed: u64, alpha: f32) {
        let ink = Ink {
            wide: 1,
            bright: false,
        };
        self.bitmap(at, (&PADLOCK, PADLOCK_CELLS.0), seed, ink, alpha);
    }

    /// `text` in large letters of glyphs, each letter 5 cells by 7 (two
    /// glyphs across a cell where `width` has room), about `centre`.
    pub(crate) fn letters(
        &mut self,
        text: &str,
        centre: Point<f64, Logical>,
        width: f64,
        seed: u64,
        a: f32,
    ) {
        let count = text.chars().count() as f64;
        let wide = if count * 6.0 * 2.0 * f64::from(CELL.0) <= width * 0.9 {
            2
        } else {
            1
        };
        let (cw, ch) = (f64::from(CELL.0) * f64::from(wide), f64::from(CELL.1));
        let advance = 6.0 * cw;
        let x0 = centre.x - (count * advance - cw) / 2.0;
        let y0 = centre.y - 3.5 * ch;
        let ink = Ink { wide, bright: true };
        for (n, c) in text.chars().enumerate() {
            let rows: Vec<u16> = letter(c).iter().map(|r| u16::from(*r)).collect();
            let at = Point::from((x0 + n as f64 * advance, y0));
            self.bitmap(at, (&rows, 5), hash(&[seed, n as u64]), ink, a);
        }
    }

    /// A ring of `radius` about `centre`, `width` wide, in rows four
    /// pixels tall.
    pub(crate) fn ring(
        &mut self,
        centre: Point<f64, Logical>,
        radius: f64,
        width: f64,
        color: Color,
        alpha: f32,
    ) {
        if radius < 1.0 || alpha <= 0.0 {
            return;
        }
        let inner = (radius - width).max(0.0);
        let mut y = -radius;
        while y < radius {
            let mid = y + 2.0;
            let outer_half = (radius * radius - mid * mid).max(0.0).sqrt();
            let inner_half = (inner * inner - mid * mid).max(0.0).sqrt();
            let w = (outer_half - inner_half).max(1.0);
            for x in [centre.x - outer_half, centre.x + inner_half] {
                self.solid(rect(x, centre.y + y, w, 4.0), color, alpha);
            }
            y += 4.0;
        }
    }
}

// The desktop switch (§9).

/// The desktop left behind, breaking into glyphs that rain away.
#[derive(Debug, Clone)]
pub(crate) struct DesktopRain {
    output: String,
    since: Duration,
    /// Where its glyphs go: -1 to the left (the next desktop), 1 right.
    dir: f64,
    /// Its windows and where they were, global.
    windows: Vec<(Window, Rect)>,
    seed: u64,
}

/// The switch, its glyphs gone after 350 ms, and its stragglers 150 ms
/// later.
const SWITCH_MS: f32 = 500.0;

impl DesktopRain {
    fn is_running(&self, now: Duration) -> bool {
        millis(self.since, now) < SWITCH_MS
    }
}

// Toasts (§13).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToastMoment {
    /// Typed in where it goes, then shown under its fading glyphs.
    Appear,
    /// Turned to glyphs where it was, which scatter.
    Gone,
}

#[derive(Debug, Clone)]
struct ToastFx {
    id: u32,
    moment: ToastMoment,
    since: Duration,
    /// Its card, from the toasts' corner, once known.
    rect: Option<Rect>,
}

impl ToastFx {
    fn total(&self) -> f32 {
        match self.moment {
            ToastMoment::Appear => 350.0,
            ToastMoment::Gone => 300.0,
        }
    }
}

// The column (§14) and the launcher (§15).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Moment {
    Open,
    Close,
}

/// How long the column's and the launcher's moments last.
fn column_ms(moment: Moment) -> f32 {
    match moment {
        Moment::Open => 350.0,
        Moment::Close => 300.0,
    }
}

fn launcher_ms(moment: Moment) -> f32 {
    match moment {
        Moment::Open => 450.0,
        Moment::Close => 300.0,
    }
}

#[derive(Debug, Clone)]
struct LauncherFx {
    moment: Moment,
    since: Duration,
    /// Where its parts were, closing.
    parts: Option<Parts>,
}

/// The Matrix theme's shell moments under way.
#[derive(Debug, Default)]
pub struct Moments {
    desktops: Vec<DesktopRain>,
    toasts: Vec<ToastFx>,
    /// The column shown last time, and its moment.
    column_shown: Option<bool>,
    column: Option<(Moment, Duration)>,
    launcher: Option<LauncherFx>,
}

/// The rain's seed on an output: the same each time.
pub(crate) fn seed_of(output: &str) -> u64 {
    hash(&[output
        .bytes()
        .fold(7_u64, |h, b| h.wrapping_mul(31) ^ u64::from(b))])
}

/// Darkness behind the launcher.
const LAUNCHER_DIM: f32 = 0.6;

impl State {
    /// Whether the shell is drawn in the Matrix theme (moving or not).
    pub(crate) fn matrix_look(&self) -> bool {
        self.look() == crate::motion::Look::Matrix
    }

    /// Whether the Matrix theme's rain is drawn: not with reduced motion.
    pub(crate) fn matrix_rains(&self) -> bool {
        self.matrix_look() && !self.config.appearance.reduced_motion
    }

    /// Whether a Matrix shell moment moves at `now`.
    pub fn matrix_shell_moving(&self, now: Duration) -> bool {
        let m = &self.matrix_shell;
        m.desktops.iter().any(|d| d.is_running(now))
            || m.toasts.iter().any(|t| millis(t.since, now) < t.total())
            || m.column
                .is_some_and(|(moment, since)| millis(since, now) < column_ms(moment))
            || m.launcher
                .as_ref()
                .is_some_and(|l| millis(l.since, now) < launcher_ms(l.moment))
    }

    /// After each dispatch: moments over go, the column's shows and goes,
    /// the toasts' cards are found.
    pub fn refresh_matrix_shell(&mut self) {
        let now = self.clock.now();
        let moves = self.matrix_moves();
        // The column comes and goes with a fullscreen window over it.
        let shown = self
            .column_output()
            .is_some_and(|o| !self.fullscreen_on(&o))
            && self.shell.as_ref().is_some_and(|s| s.is_placed());
        let was = self.matrix_shell.column_shown.replace(shown);
        if moves && was.is_some_and(|w| w != shown) {
            let moment = if shown { Moment::Open } else { Moment::Close };
            self.matrix_shell.column = Some((moment, now));
        }
        if let Some(panel) = self.toasts.as_mut() {
            for (id, rect) in panel.toasts.take_gone() {
                if moves {
                    self.matrix_shell.toasts.retain(|t| t.id != id);
                    self.matrix_shell.toasts.push(ToastFx {
                        id,
                        moment: ToastMoment::Gone,
                        since: now,
                        rect: Some(ui_rect(rect, Point::default())),
                    });
                }
            }
            for fx in &mut self.matrix_shell.toasts {
                if fx.rect.is_none() {
                    fx.rect = panel
                        .toasts
                        .card_rect(fx.id)
                        .map(|r| ui_rect(r, Point::default()));
                }
            }
        }
        let m = &mut self.matrix_shell;
        m.desktops.retain(|d| d.is_running(now));
        m.toasts.retain(|t| millis(t.since, now) < t.total());
        m.column = m
            .column
            .filter(|(moment, since)| millis(*since, now) < column_ms(*moment));
        m.launcher = m
            .launcher
            .take()
            .filter(|l| millis(l.since, now) < launcher_ms(l.moment));
        if self.matrix_shell_moving(now) {
            self.needs_redraw = true;
        }
    }

    // The desktop switch.

    /// `output` switched from desktop `from` to `to` (from 0) in the Matrix
    /// theme: the windows of the one left break into glyphs that rain away.
    pub(crate) fn matrix_switch_desktops(&mut self, output: &Output, from: usize, to: usize) {
        let now = self.clock.now();
        let name = hocus_core::OutputName(output.name());
        let Some(desk) = self
            .model
            .output(&name)
            .and_then(|o| o.desktops.get(from))
            .and_then(|d| self.model.desktop(*d))
        else {
            return;
        };
        let windows = desk
            .windows
            .iter()
            .filter(|id| !self.model.is_minimized(**id) && !self.model.is_pinned(**id))
            .filter_map(|id| {
                let managed = self.windows.get(id)?;
                let size = managed.window.geometry().size.to_f64();
                let bar = f64::from(self.bar_height(*id));
                let at = managed.location.to_f64();
                Some((
                    managed.window.clone(),
                    rect(at.x, at.y - bar, size.w, size.h + bar),
                ))
            })
            .collect();
        self.matrix_shell
            .desktops
            .retain(|d| d.output != output.name());
        self.matrix_shell.desktops.push(DesktopRain {
            output: output.name(),
            since: now,
            dir: if to > from { -1.0 } else { 1.0 },
            windows,
            seed: hash(&[now.as_nanos() as u64, from as u64, to as u64]),
        });
        self.needs_redraw = true;
    }

    /// The desktop left behind on `output`, over the windows, front first.
    pub fn matrix_desktop_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let now = self.clock.now();
        let Some(rain) = self
            .matrix_shell
            .desktops
            .iter()
            .find(|d| d.output == output.name() && d.is_running(now))
        else {
            return Vec::new();
        };
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let origin = area.loc.to_f64();
        let screen = rect(0.0, 0.0, f64::from(area.size.w), f64::from(area.size.h));
        let t = millis(rain.since, now);
        let (cw, ch) = (f64::from(CELL.0), f64::from(CELL.1));
        let green = SHEET_ROWS - 1;
        let (cx, cy) = (screen.size.w / 2.0, screen.size.h / 2.0);
        // Cells turn from the screen's edges in (in-quad), done by 150 ms.
        let coverage = 1.1 * in_quad(t / 150.0);
        let mut brush = self.brush(renderer, scale);
        let mut behind = Vec::new();
        let mut live = Vec::new();
        for (n, (window, frame)) in rain.windows.iter().enumerate() {
            let frame = rect(
                frame.loc.x - origin.x,
                frame.loc.y - origin.y,
                frame.size.w,
                frame.size.h,
            );
            let Some(shown) = frame.intersection(screen) else {
                continue;
            };
            let seed = hash(&[rain.seed, n as u64]);
            let cols = (shown.size.w / cw).ceil() as u32;
            let rows = (shown.size.h / ch).ceil() as u32;
            if t < 150.0 {
                let turn = |i: u32, j: u32| {
                    let x = shown.loc.x + (f64::from(i) + 0.5) * cw;
                    let y = shown.loc.y + (f64::from(j) + 0.5) * ch;
                    let d = ((x - cx).abs() / cx).max((y - cy).abs() / cy);
                    let jitter = (hash(&[seed, i.into(), j.into()]) % 1000) as f64 / 10_000.0;
                    (1.0 - d + jitter) as f32
                };
                for j in 0..rows {
                    let y = shown.loc.y + f64::from(j) * ch;
                    let h = ch.min(shown.loc.y + shown.size.h - y);
                    let mut i = 0;
                    while i < cols {
                        if turn(i, j) > coverage {
                            i += 1;
                            continue;
                        }
                        let first = i;
                        while i < cols && turn(i, j) <= coverage {
                            i += 1;
                        }
                        let x = shown.loc.x + f64::from(first) * cw;
                        let w = (f64::from(i - first) * cw).min(shown.loc.x + shown.size.w - x);
                        let row = (hash(&[seed, j.into(), 1]) % u64::from(green)) as u32;
                        let col = (hash(&[seed, j.into(), 2]) % u64::from(SHEET_COLS)) as u32;
                        brush.row(Point::from((x, y)), w, h, row, col + first, 1.0);
                        behind.push(rect(x, y, w, h));
                    }
                }
                live.push((window.clone(), frame));
                continue;
            }
            // Column by column, the glyphs rain off in the switch's way,
            // drifting down, each column at its own pace, then fade.
            let p = phase(t, 150.0, 300.0);
            let fade = if t < 250.0 {
                1.0
            } else {
                (-5.0 * phase(t, 250.0, 350.0)).exp() * (1.0 - phase(t, 250.0, 350.0))
            };
            if t < 350.0 {
                for i in 0..cols {
                    let mut rng = Rng::new(hash(&[seed, i.into(), 3]));
                    let speed = f64::from(rng.range(0.8, 1.2));
                    let jitter = f64::from(rng.range(-1.0, 1.0)) * 6.0 * f64::from(p);
                    let eased = f64::from(Easing::OutCubic.apply(p));
                    let dx = rain.dir * eased * speed * screen.size.w * 0.6 + jitter;
                    let dy = 0.15 * screen.size.h * f64::from(in_quad(p)) * speed;
                    let x = shown.loc.x + f64::from(i) * cw + dx;
                    let w = cw.min(shown.loc.x + shown.size.w - (shown.loc.x + f64::from(i) * cw));
                    if w <= 0.0 {
                        continue;
                    }
                    brush.column(
                        Point::from((x, shown.loc.y + dy)),
                        shown.size.h,
                        (hash(&[seed, i.into(), 4]) % u64::from(SHEET_COLS)) as u32,
                        i,
                        fade,
                    );
                }
            }
            // A few stragglers drift on, fading.
            if t >= 300.0 {
                let mut rng = Rng::new(hash(&[seed, 5]));
                let age = f64::from(t - 300.0) / 1000.0;
                for _ in 0..10 + rng.below(6) {
                    let u = f64::from(rng.unit());
                    let v = f64::from(rng.unit());
                    let speed = f64::from(rng.range(80.0, 220.0));
                    let glyph = rng.next_u64();
                    let at = Point::from((
                        shown.loc.x
                            + u * shown.size.w
                            + rain.dir * (shown.size.w * 0.3 + speed * age),
                        shown.loc.y + v * shown.size.h + 60.0 * age,
                    ));
                    brush.glyph(at, glyph, false, 1.0 - phase(t, 300.0, SWITCH_MS));
                }
            }
        }
        let mut out = std::mem::take(&mut brush.out);
        let mut back = self.brush(brush.renderer, scale);
        for r in behind {
            back.solid(r, BLACK, 1.0);
        }
        out.append(&mut back.out);
        // The windows left behind, under their glyphs, while they turn.
        for (window, frame) in live {
            let radius = if self.decorated(&window) {
                self.style.radius
            } else {
                0.0
            };
            out.extend(window_elements(
                renderer, &window, frame, radius, scale, 1.0,
            ));
        }
        out
    }

    // Toasts.

    /// A toast shows in the Matrix theme: it is typed in where it goes.
    pub(crate) fn matrix_toast_shown(&mut self, id: u32) {
        if !self.matrix_moves() {
            return;
        }
        let now = self.clock.now();
        self.matrix_shell.toasts.retain(|t| t.id != id);
        self.matrix_shell.toasts.push(ToastFx {
            id,
            moment: ToastMoment::Appear,
            since: now,
            rect: None,
        });
        self.needs_redraw = true;
    }

    /// The parts of the toasts' picture not yet typed in, from its corner.
    pub(crate) fn matrix_toasts_hidden(&self) -> Vec<Rect> {
        let now = self.clock.now();
        self.matrix_shell
            .toasts
            .iter()
            .filter(|t| t.moment == ToastMoment::Appear)
            .filter_map(|t| {
                let r = t.rect?;
                let typed = toast_typed(millis(t.since, now));
                (typed < 1.0).then(|| {
                    // Its shadow's room too, but the cards around.
                    let x = r.loc.x + r.size.w * f64::from(typed);
                    rect(
                        x,
                        r.loc.y - 4.0,
                        r.loc.x + r.size.w + 24.0 - x,
                        r.size.h + 8.0,
                    )
                })
            })
            .collect()
    }

    /// The toasts' glyphs: typed in, fading, scattering; `origin` is the
    /// toasts' corner, output-local. Front first.
    pub fn matrix_toast_elements<R>(
        &self,
        renderer: &mut R,
        origin: Point<f64, Logical>,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let now = self.clock.now();
        let mut brush = self.brush(renderer, scale);
        for fx in &self.matrix_shell.toasts {
            let Some(r) = fx.rect else {
                continue;
            };
            let r = rect(r.loc.x + origin.x, r.loc.y + origin.y, r.size.w, r.size.h);
            let t = millis(fx.since, now);
            let seed = hash(&[u64::from(fx.id), 77]);
            match fx.moment {
                ToastMoment::Appear if t < 200.0 => {
                    let w = r.size.w * f64::from(toast_typed(t));
                    brush.typed(r, w, seed, 1.0, true);
                }
                ToastMoment::Appear => {
                    // Green on black, giving way to the card (out-cubic).
                    let a = 1.0 - Easing::OutCubic.apply(phase(t, 200.0, 350.0));
                    brush.typed(r, r.size.w, seed, a, false);
                }
                ToastMoment::Gone => {
                    if t < 100.0 {
                        brush.typed(r, r.size.w, seed, 1.0 - phase(t, 50.0, 100.0), false);
                    }
                    let count = 20 + (seed % 11) as usize;
                    brush.scatter(r, count, seed, t - 50.0, 250.0);
                }
            }
        }
        brush.out
    }

    // The column.

    /// The column's moment, while it plays.
    pub(crate) fn matrix_column_moment(&self) -> Option<(Moment, f32)> {
        let (moment, since) = self.matrix_shell.column?;
        let t = millis(since, self.clock.now());
        (t < column_ms(moment)).then_some((moment, t))
    }

    /// The column coming or going in rain: `buffer` is its picture at
    /// `column` (output-local), the screen `width` wide. Front first.
    pub(crate) fn matrix_column_elements<R>(
        &self,
        renderer: &mut R,
        buffer: &MemoryRenderBuffer,
        column: Rect,
        width: f64,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some((moment, t)) = self.matrix_column_moment() else {
            return Vec::new();
        };
        let (left, right) = (column.loc.x, column.loc.x + column.size.w);
        const BAND: f64 = 40.0;
        let rain = Rain {
            speed: 1040.0,
            step: 8.0,
            drops: 5,
            ..Rain::dense(seed_of("column"))
        };
        let mut brush = self.brush(renderer, scale);
        let (front, content, band) = match moment {
            Moment::Open => {
                // From the right edge in, out-cubic; the content shows
                // behind it, fading in (out-quad).
                let x = right - f64::from(Easing::OutCubic.apply(t / 250.0)) * column.size.w;
                let band = 1.0 - phase(t, 250.0, 350.0);
                (
                    x,
                    out_quad(t / 200.0),
                    (rect(x, column.loc.y, BAND, column.size.h), band),
                )
            }
            Moment::Close => {
                // Green first, then swept away to the right, accelerating,
                // the rain going on off the screen.
                if t < 50.0 {
                    brush.solid(column, GREEN, 0.3 * t / 50.0);
                }
                let reach = width - left + BAND;
                let x = left + f64::from(Easing::InCubic.apply(phase(t, 50.0, 300.0))) * reach;
                (
                    x,
                    1.0,
                    (rect(x - BAND, column.loc.y, BAND, column.size.h), 1.0),
                )
            }
        };
        let (band_rect, band_alpha) = band;
        brush.rain(band_rect, &rain, t, band_alpha, |_| f64::MAX);
        let shown = rect(
            front.max(left),
            column.loc.y,
            right - front.max(left),
            column.size.h,
        );
        if moment == Moment::Close && t >= 50.0 {
            brush.solid(shown, GREEN, 0.3);
        }
        let mut out = std::mem::take(&mut brush.out);
        if shown.size.w > 0.0 {
            let part = rect(shown.loc.x - left, 0.0, shown.size.w, shown.size.h);
            out.extend(
                buffer_part(renderer, buffer, scale, column.loc, part, content)
                    .map(SceneElement::Frame),
            );
        }
        out
    }

    // The launcher.

    /// The launcher opens or closes in the Matrix theme; `parts` are where
    /// its parts were, closing.
    pub(crate) fn matrix_launcher(&mut self, moment: Moment, parts: Option<Parts>) {
        if !self.matrix_moves() {
            self.matrix_shell.launcher = None;
            return;
        }
        self.matrix_shell.launcher = Some(LauncherFx {
            moment,
            since: self.clock.now(),
            parts,
        });
        self.needs_redraw = true;
    }

    /// Whether the launcher's own picture is left out: it is typed in or
    /// rained away instead.
    pub(crate) fn matrix_launcher_playing(&self) -> bool {
        self.matrix_shell
            .launcher
            .as_ref()
            .is_some_and(|l| millis(l.since, self.clock.now()) < launcher_ms(l.moment))
    }

    /// The launcher in the Matrix theme on the screen it is on: the dim and
    /// the rain behind it, and its parts as they come or go. `buffer` is
    /// its picture with its corner at `at` (output-local), on a screen
    /// `screen` large; `parts` where its parts are now, when open. Front
    /// first.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn matrix_launcher_elements<R>(
        &self,
        renderer: &mut R,
        buffer: &MemoryRenderBuffer,
        at: Point<f64, Logical>,
        screen: Size<f64, Logical>,
        parts: Option<Parts>,
        open: bool,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let now = self.clock.now();
        let fx = self
            .matrix_shell
            .launcher
            .as_ref()
            .filter(|l| millis(l.since, now) < launcher_ms(l.moment));
        let t = fx.map_or(f32::MAX, |l| millis(l.since, now));
        // The dim and the rain: in with it, out with it.
        let back = match fx.map(|l| l.moment) {
            Some(Moment::Open) => phase(t, 0.0, 200.0),
            Some(Moment::Close) => 1.0 - phase(t, 0.0, 300.0),
            None if open => 1.0,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        let mut brush = self.brush(renderer, scale);
        let whole = rect(0.0, 0.0, screen.w, screen.h);
        let shift = |r: hocus_ui::Rect| ui_rect(r, at);
        match (fx, parts.or_else(|| fx.and_then(|l| l.parts.clone()))) {
            (Some(l), Some(parts)) if l.moment == Moment::Open => {
                let head = shift(parts.head);
                // A cursor blinks twice where the field will be.
                let (hx, hy) = (
                    head.loc.x + head.size.w / 2.0,
                    head.loc.y + head.size.h / 2.0,
                );
                let on = |t: f32| (0.0..50.0).contains(&t) || (100.0..150.0).contains(&t);
                if on(t) {
                    brush.solid(rect(hx - 5.0, hy - 12.0, 10.0, 24.0), HEAD, 1.0);
                }
                let mut parts_out = Vec::new();
                if t >= 200.0 {
                    // The field opens from there on a spring.
                    let p = damped(phase(t, 200.0, 300.0), 0.1);
                    let w = head.size.w * f64::from(p);
                    let r = rect(hx - w / 2.0, head.loc.y, w, head.size.h);
                    if t < 300.0 {
                        brush.solid(rect(r.loc.x + w - 2.0, hy - 12.0, 2.0, 24.0), HEAD, 1.0);
                    }
                    parts_out.push((r, 1.0));
                    // The results cascade in, typed left to right.
                    let rows: Vec<Rect> = parts.rows.iter().map(|r| shift(*r)).collect();
                    for (k, row) in rows.iter().enumerate() {
                        let start = 300.0 + 30.0 * k as f32;
                        let q = Easing::OutCubic.apply(phase(t, start, start + 120.0));
                        if q > 0.0 {
                            let w = row.size.w * f64::from(q);
                            parts_out.push((rect(row.loc.x, row.loc.y, w, row.size.h), 1.0));
                            if q < 1.0 {
                                brush.solid(
                                    rect(
                                        row.loc.x + w - 2.0,
                                        row.loc.y + 12.0,
                                        2.0,
                                        row.size.h - 24.0,
                                    ),
                                    HEAD,
                                    1.0,
                                );
                            }
                        }
                    }
                    // The rest of the card fades in with them.
                    let mut holes = vec![head];
                    holes.extend(rows.iter().copied());
                    let body = phase(t, 300.0, 450.0);
                    for r in subtract(shift(parts.card), &holes) {
                        parts_out.push((r, body));
                    }
                    // The room around the card (its shadow), as much as
                    // above and left of it.
                    let card = shift(parts.card);
                    let room = card.loc - at;
                    let whole = rect(
                        at.x,
                        at.y,
                        card.size.w + 2.0 * room.x,
                        card.size.h + 2.0 * room.y,
                    );
                    for r in subtract(whole, &[card]) {
                        parts_out.push((r, body));
                    }
                }
                out.append(&mut brush.out);
                for (r, a) in parts_out {
                    let part = rect(r.loc.x - at.x, r.loc.y - at.y, r.size.w, r.size.h);
                    out.extend(
                        buffer_part(brush.renderer, buffer, scale, at, part, a)
                            .map(SceneElement::Frame),
                    );
                }
            }
            (Some(l), Some(parts)) => {
                debug_assert_eq!(l.moment, Moment::Close);
                // Its text falls as rain; the field closes to its middle.
                let head = shift(parts.head);
                let mut rows: Vec<Rect> = parts.rows.iter().map(|r| shift(*r)).collect();
                rows.push(head);
                brush.fall(&rows, seed_of("launcher"), t / 250.0, 260.0);
                out.append(&mut brush.out);
                let q = 1.0 - Easing::OutCubic.apply(phase(t, 0.0, 200.0));
                let w = head.size.w * f64::from(q);
                let r = rect(
                    head.loc.x + (head.size.w - w) / 2.0,
                    head.loc.y,
                    w,
                    head.size.h,
                );
                let part = rect(r.loc.x - at.x, r.loc.y - at.y, r.size.w, r.size.h);
                out.extend(
                    buffer_part(brush.renderer, buffer, scale, at, part, q)
                        .map(SceneElement::Frame),
                );
            }
            _ => {}
        }
        let mut brush = self.brush(renderer, scale);
        if self.matrix_rains() {
            // Sparse and slow, standing still once it is open.
            let rain = Rain::sparse(seed_of("launcher"), 0.2);
            let t = fx.map_or(450.0, |l| match l.moment {
                Moment::Open => t,
                Moment::Close => 450.0 + t,
            });
            brush.rain(whole, &rain, t, 0.6 * back, |_| f64::MAX);
        }
        brush.solid(whole, BLACK, LAUNCHER_DIM * back);
        out.append(&mut brush.out);
        out
    }
}

/// How much of a toast is typed in, `t` milliseconds in: 200 ms, at a
/// constant rate.
fn toast_typed(t: f32) -> f32 {
    phase(t, 0.0, 200.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtracting_keeps_what_is_outside_the_holes() {
        let parts = subtract(
            rect(0.0, 0.0, 100.0, 100.0),
            &[rect(20.0, 30.0, 40.0, 20.0)],
        );
        let area: f64 = parts.iter().map(|r| r.size.w * r.size.h).sum();
        assert!((area - (10_000.0 - 800.0)).abs() < 1e-6, "{area}");
        assert!(
            parts
                .iter()
                .all(|p| p.intersection(rect(20.0, 30.0, 40.0, 20.0)).is_none())
        );
        assert_eq!(
            subtract(rect(0.0, 0.0, 10.0, 10.0), &[rect(50.0, 50.0, 5.0, 5.0)]).len(),
            1
        );
    }

    #[test]
    fn a_toast_is_typed_in_200_ms() {
        assert_eq!(toast_typed(0.0), 0.0);
        assert!((toast_typed(100.0) - 0.5).abs() < 1e-6);
        assert_eq!(toast_typed(250.0), 1.0);
    }

    #[test]
    fn every_output_has_its_rain() {
        assert_eq!(seed_of("HEADLESS-1"), seed_of("HEADLESS-1"));
        assert_ne!(seed_of("HEADLESS-1"), seed_of("HEADLESS-2"));
    }
}
