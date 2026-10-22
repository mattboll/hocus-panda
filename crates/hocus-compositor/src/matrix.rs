//! The Matrix theme's window effects (docs/design/matrix-theme.md, §1 and
//! §2): a window materializes from a column of falling glyphs that fills
//! its rectangle row by row and dissolves into it; a closing window
//! shifts to green, decomposes into glyphs and drains off as rain.
//!
//! They are drawn with the elements every renderer has, pixman included,
//! so that scenarios sample them: rows and columns cut from a sheet of
//! glyphs (`hocus_shell::matrix`, drawn once per scale) and plain colours.
//! Each effect is a closed form of the time since it started, evaluated
//! when the scene is drawn: nothing is queued per frame.

use std::{collections::BTreeMap, sync::Mutex, time::Duration};

use hocus_shell::matrix::{BLACK, CELL, GLOW, GREEN, HEAD, Rng, Sheet, hash};
use hocus_ui::{Color, motion::Easing};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Color32F, ImportAll, ImportMem, Renderer,
            element::{
                Id, Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::SolidColorRenderElement,
            },
            utils::CommitCounter,
        },
    },
    utils::{Logical, Point, Rectangle, Size, Transform},
};

use crate::{State, scene::SceneElement};

/// The sheet's cells across and down; its last row holds the heads.
const SHEET_COLS: u32 = 48;
const SHEET_ROWS: u32 = 33;

/// A window opening: 400 ms, and glyphs that scatter 100 ms longer.
const OPEN_MS: f32 = 500.0;
/// A window closing: 350 ms, and its last sparks 50 ms longer.
const CLOSE_MS: f32 = 400.0;
/// The green over a window as it shows, at its strongest (180 of 255).
const TINT: f32 = 180.0 / 255.0;

fn in_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t
}

fn phase(t: f32, from: f32, to: f32) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

fn millis(since: Duration, now: Duration) -> f32 {
    now.saturating_sub(since).as_secs_f32() * 1000.0
}

/// A window materializing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Open {
    since: Duration,
    seed: u64,
}

impl Open {
    pub fn new(now: Duration) -> Self {
        Self {
            since: now,
            seed: hash(&[now.as_nanos() as u64]),
        }
    }

    pub fn is_running(&self, now: Duration) -> bool {
        millis(self.since, now) < OPEN_MS
    }

    /// How much of the window shows: nothing until its glyphs have filled
    /// it, then all of it, quickly, under their green.
    pub fn content_alpha(&self, now: Duration) -> f32 {
        phase(millis(self.since, now), 250.0, 300.0)
    }
}

/// A window decomposing: its cells in the order they turn to glyphs.
#[derive(Debug, Clone, PartialEq)]
pub struct Close {
    since: Duration,
    seed: u64,
    cols: u32,
    rows: u32,
    /// Per cell, row by row: the coverage, 0 to 1, from which it is a
    /// glyph.
    turn: Vec<f32>,
    /// Cells by the order they turn.
    order: Vec<u32>,
}

impl Close {
    /// For a window of `size`: glyphs spread from a few seed points, the
    /// edges of what they cover a little ragged.
    pub fn new(now: Duration, size: Size<f64, Logical>) -> Self {
        let (cw, ch) = (f64::from(CELL.0), f64::from(CELL.1));
        let cols = (size.w / cw).ceil().clamp(1.0, 1024.0) as u32;
        let rows = (size.h / ch).ceil().clamp(1.0, 1024.0) as u32;
        let seed = hash(&[now.as_nanos() as u64, cols.into(), rows.into()]);
        let mut rng = Rng::new(seed);
        let seeds: Vec<(f64, f64)> = (0..(cols * rows / 300).clamp(3, 9))
            .map(|_| {
                (
                    f64::from(rng.unit()) * size.w,
                    f64::from(rng.unit()) * size.h,
                )
            })
            .collect();
        let near: Vec<f64> = (0..cols * rows)
            .map(|n| {
                let (x, y) = (
                    (f64::from(n % cols) + 0.5) * cw,
                    (f64::from(n / cols) + 0.5) * ch,
                );
                let d = seeds
                    .iter()
                    .map(|(sx, sy)| (x - sx).hypot(y - sy))
                    .fold(f64::MAX, f64::min);
                let jitter = (hash(&[seed, n.into()]) % 1000) as f64 / 1000.0;
                d + jitter * 0.12 * size.w.max(size.h)
            })
            .collect();
        let mut order: Vec<u32> = (0..cols * rows).collect();
        order.sort_by(|a, b| near[*a as usize].total_cmp(&near[*b as usize]));
        let mut turn = vec![0.0; order.len()];
        let n = order.len() as f32;
        for (rank, cell) in order.iter().enumerate() {
            turn[*cell as usize] = rank as f32 / n;
        }
        Self {
            since: now,
            seed,
            cols,
            rows,
            turn,
            order,
        }
    }

    pub fn is_running(&self, now: Duration) -> bool {
        millis(self.since, now) < CLOSE_MS
    }

    /// The window's picture: there until it rains away.
    pub fn picture_alpha(&self, now: Duration) -> f32 {
        if millis(self.since, now) < 250.0 {
            1.0
        } else {
            0.0
        }
    }

    /// Its shadow goes as glyphs take it.
    pub fn shadow_alpha(&self, now: Duration) -> f32 {
        1.0 - phase(millis(self.since, now), 120.0, 250.0)
    }

    /// The glitch at first: bands of the picture shifted sideways, as
    /// fractions of its height and logical pixels; none after.
    pub fn bands(&self, now: Duration) -> Option<Vec<(f64, f64, f64)>> {
        let t = millis(self.since, now);
        if t >= 120.0 {
            return None;
        }
        // A new glitch every 30 ms, stronger as it goes.
        let mut rng = Rng::new(hash(&[self.seed, (t / 30.0) as u64]));
        let strength = 2.0 + 6.0 * f64::from(t / 120.0);
        let mut cuts: Vec<f64> = (0..3).map(|_| f64::from(rng.unit())).collect();
        cuts.extend([0.0, 1.0]);
        cuts.sort_by(f64::total_cmp);
        Some(
            cuts.windows(2)
                .map(|c| (c[0], c[1], f64::from(rng.range(-1.0, 1.0)) * strength))
                .collect(),
        )
    }

    /// How much of the window is glyphs, 0 to 1.
    fn coverage(t: f32) -> f32 {
        if t < 120.0 {
            0.0
        } else {
            0.1 + 0.8 * in_quad(phase(t, 120.0, 250.0))
        }
    }
}

/// The sheet of glyphs at one scale, on the graphics card once drawn.
#[derive(Debug, Clone)]
struct SheetBuffer {
    buffer: MemoryRenderBuffer,
    /// A cell, physical pixels.
    cell: (f64, f64),
}

/// Sheets drawn once per scale.
#[derive(Debug, Default)]
pub struct Sheets(Mutex<BTreeMap<u32, SheetBuffer>>);

impl Sheets {
    fn get(&self, scale: f64) -> SheetBuffer {
        let draw = || {
            let sheet = Sheet::new(scale, SHEET_COLS, SHEET_ROWS);
            SheetBuffer {
                buffer: MemoryRenderBuffer::from_slice(
                    &sheet.pixels,
                    Fourcc::Argb8888,
                    (sheet.width as i32, sheet.height as i32),
                    1,
                    Transform::Normal,
                    None,
                ),
                cell: (f64::from(sheet.cell.0), f64::from(sheet.cell.1)),
            }
        };
        let key = (scale * 1000.0).round() as u32;
        match self.0.lock() {
            Ok(mut cache) => cache.entry(key).or_insert_with(draw).clone(),
            Err(_) => draw(),
        }
    }
}

/// Collects an effect's elements, front first.
struct Brush<'a, R: Renderer + ImportAll + ImportMem> {
    renderer: &'a mut R,
    sheet: SheetBuffer,
    scale: f64,
    out: Vec<SceneElement<R>>,
}

impl<R> Brush<'_, R>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    /// Part of the sheet, `cells` across and down from cell `(col, row)`,
    /// drawn at `at` (output-local, logical) at the cells' own size, cut
    /// to `size`.
    fn cut(
        &mut self,
        at: Point<f64, Logical>,
        size: Size<f64, Logical>,
        cell: (u32, u32),
        alpha: f32,
    ) {
        let (w, h) = (size.w.round() as i32, size.h.round() as i32);
        if w <= 0 || h <= 0 || alpha <= 0.0 {
            return;
        }
        let (cw, ch) = self.sheet.cell;
        let (kx, ky) = (cw / f64::from(CELL.0), ch / f64::from(CELL.1));
        let src = Rectangle::<f64, Logical>::new(
            (f64::from(cell.0) * cw, f64::from(cell.1) * ch).into(),
            (f64::from(w) * kx, f64::from(h) * ky).into(),
        );
        match MemoryRenderBufferRenderElement::from_buffer(
            self.renderer,
            at.to_physical(self.scale),
            &self.sheet.buffer,
            (alpha < 1.0).then_some(alpha.clamp(0.0, 1.0)),
            Some(src),
            Some((w, h).into()),
            Kind::Unspecified,
        ) {
            Ok(element) => self.out.push(SceneElement::Frame(element)),
            Err(err) => tracing::warn!(?err, "could not draw the rain"),
        }
    }

    /// A row of glyphs `width` wide (and `height` tall, a row at most),
    /// from the sheet's row `row` and its column `col`, wrapping around.
    fn row(
        &mut self,
        at: Point<f64, Logical>,
        width: f64,
        height: f64,
        row: u32,
        col: u32,
        alpha: f32,
    ) {
        let cw = f64::from(CELL.0);
        let (mut x, mut col) = (0.0, col % SHEET_COLS);
        while x < width {
            let fits = f64::from(SHEET_COLS - col) * cw;
            let w = fits.min(width - x);
            let size = Size::from((w, height.min(f64::from(CELL.1))));
            self.cut(Point::from((at.x + x, at.y)), size, (col, row), alpha);
            x += w;
            col = 0;
        }
    }

    /// A column of glyphs `height` tall from the sheet's column `col` and
    /// its row `row`, wrapping around its green rows.
    fn column(&mut self, at: Point<f64, Logical>, height: f64, col: u32, row: u32, alpha: f32) {
        let ch = f64::from(CELL.1);
        let green = SHEET_ROWS - 1;
        let (mut y, mut row) = (0.0, row % green);
        while y < height {
            let fits = f64::from(green - row) * ch;
            let h = fits.min(height - y);
            let size = Size::from((f64::from(CELL.0), h));
            self.cut(
                Point::from((at.x, at.y + y)),
                size,
                (col % SHEET_COLS, row),
                alpha,
            );
            y += h;
            row = 0;
        }
    }

    /// One glyph; a head from the sheet's last row when `head`.
    fn glyph(&mut self, at: Point<f64, Logical>, pick: u64, head: bool, alpha: f32) {
        let col = (pick % u64::from(SHEET_COLS)) as u32;
        let row = if head {
            SHEET_ROWS - 1
        } else {
            ((pick >> 16) % u64::from(SHEET_ROWS - 1)) as u32
        };
        let size = Size::from((f64::from(CELL.0), f64::from(CELL.1)));
        self.cut(at, size, (col, row), alpha);
    }

    fn solid(&mut self, rect: Rectangle<f64, Logical>, color: Color, alpha: f32) {
        let geometry = rect.to_physical_precise_round(self.scale);
        if geometry.size.w <= 0 || geometry.size.h <= 0 || alpha <= 0.0 {
            return;
        }
        let a = alpha.clamp(0.0, 1.0);
        let c = |v: u8| f32::from(v) / 255.0 * a;
        self.out
            .push(SceneElement::Solid(SolidColorRenderElement::new(
                Id::new(),
                geometry,
                CommitCounter::default(),
                Color32F::new(c(color.r), c(color.g), c(color.b), a),
                Kind::Unspecified,
            )));
    }

    /// A glowing line `width` wide just outside `rect`.
    fn outline(&mut self, rect: Rectangle<f64, Logical>, width: f64, color: Color, alpha: f32) {
        let (x, y, w, h) = (rect.loc.x, rect.loc.y, rect.size.w, rect.size.h);
        for (rx, ry, rw, rh) in [
            (x - width, y - width, w + 2.0 * width, width),
            (x - width, y + h, w + 2.0 * width, width),
            (x - width, y, width, h),
            (x + w, y, width, h),
        ] {
            self.solid(
                Rectangle::new((rx, ry).into(), (rw, rh).into()),
                color,
                alpha,
            );
        }
    }
}

/// Which of the sheet's green rows and columns a row of the window takes.
fn pick(seed: u64, n: u32, salt: u64) -> u64 {
    hash(&[seed, n.into(), salt])
}

impl State {
    fn brush<'a, R>(&self, renderer: &'a mut R, scale: f64) -> Brush<'a, R>
    where
        R: Renderer + ImportAll + ImportMem,
    {
        Brush {
            renderer,
            sheet: self.matrix_sheets.get(scale),
            scale,
            out: Vec::new(),
        }
    }

    /// A window materializing in `frame` (output-local, logical), over
    /// its content, front first.
    pub fn matrix_open_elements<R>(
        &self,
        renderer: &mut R,
        open: &Open,
        frame: Rectangle<f64, Logical>,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let now = self.clock.now();
        let t = millis(open.since, now);
        let (cw, ch) = (f64::from(CELL.0), f64::from(CELL.1));
        let cols = (frame.size.w / cw).ceil().max(1.0) as u32;
        let rows = (frame.size.h / ch).ceil().max(1.0) as u32;
        let centre = cols / 2;
        let seed = open.seed;
        let green = SHEET_ROWS - 1;
        let row_of = |j: u32| (pick(seed, j, 1) % u64::from(green)) as u32;
        let col_of = |j: u32| (pick(seed, j, 2) % u64::from(SHEET_COLS)) as u32;
        // The glyphs' strength: whole while they fill the window, then
        // fading from 180 of 255 as the window shows (in-quad).
        let overlay = if t < 250.0 {
            1.0
        } else {
            TINT * (1.0 - in_quad(phase(t, 250.0, 400.0)))
        };
        let content = open.content_alpha(now);
        let mut glyphs = self.brush(renderer, scale);
        let mut behind: Vec<(Rectangle<f64, Logical>, f32)> = Vec::new();
        let column_x = frame.loc.x + f64::from(centre) * cw;
        if t < 100.0 {
            // A single column falls from the top, at the rain's pace.
            let shown = ((t / 100.0) * rows as f32).ceil().max(1.0) as u32;
            let shown = shown.min(rows);
            for j in 0..shown {
                let head = j + 1 == shown;
                let alpha = 0.35 + 0.65 * (j + 1) as f32 / shown as f32;
                let at = Point::from((column_x, frame.loc.y + f64::from(j) * ch));
                let h = (frame.size.h - f64::from(j) * ch).min(ch);
                if head {
                    glyphs.glyph(at, pick(seed, j, 3), true, 1.0);
                } else {
                    glyphs.row(at, cw, h, row_of(j), col_of(j) + centre, alpha);
                }
            }
        } else if t < 400.0 {
            // Rows rush out from the column to the edges, top to bottom.
            let stagger = (100.0 / rows as f32).min(2.0);
            let duration = 200.0 - stagger * rows as f32;
            let reach = f64::from(centre.max(cols - centre));
            for j in 0..rows {
                let start = 100.0 + stagger * j as f32;
                let p = Easing::OutCubic.apply(phase(t, start, start + duration));
                let half = 0.5 + f64::from(p) * reach;
                let first = (f64::from(centre) + 0.5 - half).floor().max(0.0) as u32;
                let last = ((f64::from(centre) + 0.5 + half).ceil() as u32).min(cols);
                let x = frame.loc.x + f64::from(first) * cw;
                let width = (f64::from(last - first) * cw).min(frame.loc.x + frame.size.w - x);
                let y = frame.loc.y + f64::from(j) * ch;
                let h = (frame.loc.y + frame.size.h - y).min(ch);
                glyphs.row(
                    Point::from((x, y)),
                    width,
                    h,
                    row_of(j),
                    col_of(j) + first,
                    overlay,
                );
                behind.push((Rectangle::new((x, y).into(), (width, h).into()), 0.85));
            }
        }
        // Glyphs scatter from the edges once the rows are out, falling.
        if (300.0..500.0).contains(&t) {
            let mut rng = Rng::new(seed ^ 0x9A27);
            let age = (t - 300.0) / 1000.0;
            let count = 8 + rng.below(5);
            for _ in 0..count {
                let (side, u) = (rng.below(4), f64::from(rng.unit()));
                let (x, y, nx, ny) = match side {
                    0 => (frame.loc.x + u * frame.size.w, frame.loc.y, 0.0, -1.0),
                    1 => (
                        frame.loc.x + u * frame.size.w,
                        frame.loc.y + frame.size.h,
                        0.0,
                        1.0,
                    ),
                    2 => (frame.loc.x, frame.loc.y + u * frame.size.h, -1.0, 0.0),
                    _ => (
                        frame.loc.x + frame.size.w,
                        frame.loc.y + u * frame.size.h,
                        1.0,
                        0.0,
                    ),
                };
                let speed = f64::from(rng.range(60.0, 140.0));
                let drift = f64::from(rng.range(-40.0, 40.0));
                let glyph = rng.next_u64();
                let a = f64::from(age);
                let at = Point::from((
                    x + (nx * speed - ny * drift) * a - cw / 2.0,
                    y + (ny * speed + nx * drift) * a + 250.0 * a * a - ch / 2.0,
                ));
                glyphs.glyph(at, glyph, false, 1.0 - phase(t, 300.0, 500.0));
            }
        }
        let mut out = std::mem::take(&mut glyphs.out);
        // The green over the window as it shows, and the phosphor glow
        // around it while the glyphs fill it.
        if (100.0..400.0).contains(&t) {
            let mut tint = self.brush(glyphs.renderer, scale);
            let glow = 0.35 * phase(t, 100.0, 150.0) * if t < 250.0 { 1.0 } else { overlay / TINT };
            tint.outline(frame, 2.0, GLOW, glow);
            if content > 0.0 {
                tint.solid(frame, GREEN, 0.3 * overlay * content);
            }
            for (rect, alpha) in behind {
                tint.solid(rect, BLACK, alpha * (1.0 - content));
            }
            out.append(&mut tint.out);
        }
        out
    }

    /// A window decomposing in `frame` (output-local, logical), over its
    /// picture, front first.
    pub fn matrix_close_elements<R>(
        &self,
        renderer: &mut R,
        close: &Close,
        frame: Rectangle<f64, Logical>,
        scale: f64,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let now = self.clock.now();
        let t = millis(close.since, now);
        let (cw, ch) = (f64::from(CELL.0), f64::from(CELL.1));
        // The window's size may differ from the one the cells were cut
        // for (a window closing as it opens): cells keep their size.
        let (kx, ky) = (
            frame.size.w / (f64::from(close.cols) * cw),
            frame.size.h / (f64::from(close.rows) * ch),
        );
        let (cw, ch) = (cw * kx.min(1.0), ch * ky.min(1.0));
        let seed = close.seed;
        let green = SHEET_ROWS - 1;
        let mut front = self.brush(renderer, scale);
        let mut behind: Vec<(Rectangle<f64, Logical>, f32)> = Vec::new();
        // The picture shifts to green at first, and stays so.
        let tint = if t < 250.0 {
            0.4 * phase(t, 0.0, 120.0)
        } else {
            0.0
        };
        // Sparks from the edges of what turns, each for 150 ms.
        let mut rng = Rng::new(seed ^ 0x51A7);
        let sparks = 20 + rng.below(11);
        let cells = close.order.len().max(1);
        for _ in 0..sparks {
            let born = rng.range(120.0, 250.0);
            let at_rank = (Close::coverage(born) * cells as f32) as usize;
            let cell = close.order[at_rank.min(cells - 1)];
            let (dx, dy) = (rng.range(-1.0, 1.0), rng.range(-1.0, 1.0));
            let speed = f64::from(rng.range(30.0, 120.0));
            let size = if rng.unit() < 0.5 { 1.0 } else { 2.0 };
            let age = t - born;
            if !(0.0..150.0).contains(&age) {
                continue;
            }
            let a = f64::from(age) / 1000.0;
            let norm = f64::from(dx.hypot(dy)).max(0.1);
            let x = frame.loc.x
                + (f64::from(cell % close.cols) + 0.5) * cw
                + f64::from(dx) / norm * speed * a;
            let y = frame.loc.y
                + (f64::from(cell / close.cols) + 0.5) * ch
                + f64::from(dy) / norm * speed * a
                + 200.0 * a * a;
            let color = if size > 1.0 { GLOW } else { HEAD };
            front.solid(
                Rectangle::new((x, y).into(), (size, size).into()),
                color,
                1.0 - age / 150.0,
            );
        }
        if t < 250.0 {
            // Glyphs take the window over, cell by cell.
            let coverage = Close::coverage(t);
            for j in 0..close.rows {
                let y = frame.loc.y + f64::from(j) * ch;
                let h = (frame.loc.y + frame.size.h - y).min(ch);
                let mut i = 0;
                while i < close.cols {
                    let covered = |i: u32| close.turn[(j * close.cols + i) as usize] < coverage;
                    if !covered(i) {
                        i += 1;
                        continue;
                    }
                    let first = i;
                    while i < close.cols && covered(i) {
                        i += 1;
                    }
                    let x = frame.loc.x + f64::from(first) * cw;
                    let width = (f64::from(i - first) * cw).min(frame.loc.x + frame.size.w - x);
                    let row = (pick(seed, j, 1) % u64::from(green)) as u32;
                    let col = (pick(seed, j, 2) % u64::from(SHEET_COLS)) as u32 + first;
                    front.row(Point::from((x, y)), width, h, row, col, 1.0);
                    behind.push((Rectangle::new((x, y).into(), (width, h).into()), 1.0));
                }
            }
        } else {
            // What is left falls as rain, accelerating, gone at the bottom.
            let p = phase(t, 250.0, 350.0);
            let fall = f64::from(Easing::InCubic.apply(p));
            let alpha = 1.0 - p;
            for i in 0..close.cols {
                let speed = 0.7 + 0.6 * (pick(seed, i, 4) % 1000) as f64 / 1000.0;
                let dy = speed * fall * frame.size.h;
                let x = frame.loc.x + f64::from(i) * cw;
                let w = cw.min(frame.loc.x + frame.size.w - x);
                let height = frame.size.h - dy;
                if height <= 0.0 || w <= 0.0 {
                    continue;
                }
                let at = Point::from((x, frame.loc.y + dy));
                front.column(
                    at,
                    height,
                    (pick(seed, i, 5) % u64::from(SHEET_COLS)) as u32,
                    i,
                    alpha,
                );
                behind.push((Rectangle::new(at, (w, height).into()), 0.9 * alpha));
            }
        }
        // Glyphs over black, over the green picture.
        let mut out = std::mem::take(&mut front.out);
        let mut back = self.brush(front.renderer, scale);
        for (rect, alpha) in behind {
            back.solid(rect, BLACK, alpha);
        }
        back.solid(frame, GREEN, tint);
        out.append(&mut back.out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_window_shows_once_its_glyphs_have_filled_it() {
        let open = Open::new(ms(1000));
        assert_eq!(open.content_alpha(ms(1100)), 0.0);
        assert_eq!(open.content_alpha(ms(1249)), 0.0);
        assert!(open.content_alpha(ms(1275)) > 0.0);
        assert_eq!(open.content_alpha(ms(1300)), 1.0);
        assert!(open.is_running(ms(1450)), "glyphs still scatter");
        assert!(!open.is_running(ms(1500)));
    }

    #[test]
    fn glyphs_cover_more_and_more_of_a_closing_window() {
        let close = Close::new(ms(0), Size::from((400.0, 280.0)));
        assert_eq!((close.cols, close.rows), (50, 20));
        let covered = |t: f32| {
            let c = Close::coverage(t);
            close.turn.iter().filter(|v| **v < c).count() as f32 / close.turn.len() as f32
        };
        assert_eq!(covered(100.0), 0.0, "only a glitch at first");
        assert!((covered(121.0) - 0.1).abs() < 0.02, "{}", covered(121.0));
        assert!(covered(200.0) > covered(150.0));
        assert!((covered(250.0) - 0.9).abs() < 0.02, "{}", covered(250.0));
        assert_eq!(close.picture_alpha(ms(249)), 1.0);
        assert_eq!(close.picture_alpha(ms(250)), 0.0);
        assert!(!close.is_running(ms(400)));
    }

    #[test]
    fn the_glitch_bands_cover_the_picture_then_stop() {
        let close = Close::new(ms(0), Size::from((400.0, 280.0)));
        let bands = close.bands(ms(60)).unwrap_or_default();
        assert_eq!(bands.len(), 4);
        assert_eq!(bands.first().map(|b| b.0), Some(0.0));
        assert_eq!(bands.last().map(|b| b.1), Some(1.0));
        assert!(bands.iter().all(|b| b.2.abs() <= 8.0));
        assert_eq!(close.bands(ms(120)), None);
        assert_eq!(
            close.bands(ms(60)),
            close.bands(ms(60)),
            "the same each time"
        );
    }

    #[test]
    fn the_same_moment_gives_the_same_dissolve() {
        let size = Size::from((300.0, 200.0));
        assert_eq!(Close::new(ms(5), size), Close::new(ms(5), size));
        assert_ne!(Close::new(ms(5), size), Close::new(ms(6), size));
    }
}
