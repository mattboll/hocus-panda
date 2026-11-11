//! Drawing the radial menu (#723, docs/design/windows.md §10b), the
//! mockup's: a soft dark halo, the ring's disc with its shadow, eight
//! sectors with their icons, their labels outside the ring, and the hub.
//!
//! The picture is drawn on the CPU (tiny-skia, the glyphs by fontdue in
//! the shell's own fonts) at the screen's scale, once for each look the
//! menu takes: a sector lit, Close armed. Only the arc filling along
//! Close's rim and the waiting agent's panda bobbing in the hub change at
//! every frame, each on a small picture of its own; the menu's motion in
//! and out scales and fades the pictures as they are.

use std::{
    cell::{OnceCell, RefCell},
    time::Duration,
};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Id, memory::MemoryRenderBuffer},
    },
    output::Output,
    utils::{Logical, Point, Rectangle, Size},
};
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, Paint, PathBuilder, Pixmap, RadialGradient,
    SpreadMode, Stroke, Transform,
};

use crate::{
    State,
    decoration::{FrameIds, Keyed, Placed, ShadowStyle, buffer, shadow_elements},
    radial::{HUB, REACH, RIM, RING, Radial, Sector},
    render::{OutputElement, OutputElements},
};

/// The shell's fonts: the labels' and the hub's words.
const MANROPE_SEMIBOLD: &[u8] = include_bytes!("../../hocus-shell/fonts/Manrope-SemiBold.ttf");
const MANROPE_BOLD: &[u8] = include_bytes!("../../hocus-shell/fonts/Manrope-Bold.ttf");
const MARTIAN_MONO: &[u8] = include_bytes!("../../hocus-shell/fonts/MartianMono-Regular.ttf");

/// A sector's half-width, degrees: 45° less the gap between two.
const HALF_SPAN: f64 = 20.9;
/// The icons sit on this radius, 22 px of a 24-unit drawing.
const ICON_RADIUS: f64 = (HUB + RING) / 2.0;
const ICON: f64 = 22.0;
/// The labels: 12.5 px, lines 15 px apart; the hub's words 10.5 px.
const LABEL: f32 = 12.5;
const LABEL_LINE: f64 = 15.0;
const HUB_TEXT: f32 = 10.5;
/// The disc under the sectors, its rim included; the hub's circle.
const DISC: f64 = RING + RIM;
const HUB_CIRCLE: f64 = 40.0;
/// The panda in the hub (78 px since #649), and how far it bobs.
const PANDA: f64 = 78.0;
const BOB: f64 = 2.0;
const BOB_PERIOD: f64 = 1.6;

const TEXT: [u8; 4] = [0xEE, 0xF1, 0xF6, 0xFF];
const DIM: [u8; 4] = [0xA9, 0xB2, 0xC2, 0xFF];
const DIM_OFF: [u8; 4] = [0xA9, 0xB2, 0xC2, 115];
const OUTLINE: [u8; 4] = [7, 9, 15, 204];
const ACCENT: [u8; 4] = [0x6C, 0xC2, 0x87, 0xFF];
const ORANGE: [u8; 4] = [0xF2, 0x94, 0x5B, 0xFF];
const ORANGE_INK: [u8; 4] = [0xF7, 0xB9, 0x8A, 0xFF];
const ON: [u8; 4] = [0x7E, 0xE0, 0xA0, 0xFF];
const OFF_INK: [u8; 4] = [0xFF, 0xFF, 0xFF, 64];

/// The ring's shadow, the mockup's `drop-shadow(0 10px 14px)` at 50 %.
const SHADOW: ShadowStyle = ShadowStyle {
    dy: 10.0,
    blur: 28.0,
    alpha: 0.5,
    radius: DISC,
};

pub(crate) struct Fonts {
    pub semibold: fontdue::Font,
    pub bold: fontdue::Font,
    pub mono: fontdue::Font,
}

pub(crate) fn fonts() -> Option<Fonts> {
    let read = |bytes: &'static [u8]| {
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|err| tracing::warn!(err, "the radial menu's font does not read"))
            .ok()
    };
    Some(Fonts {
        semibold: read(MANROPE_SEMIBOLD)?,
        bold: read(MANROPE_BOLD)?,
        mono: read(MARTIAN_MONO)?,
    })
}

/// What the menu's picture depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LookKey {
    scale: u32,
    lit: Option<Sector>,
    picked: Option<Sector>,
    armed: bool,
    enabled: [bool; 8],
    on: [bool; 8],
    maximized: bool,
    /// The hub shows a panda.
    panda: bool,
    french: bool,
    hub: String,
    app: String,
}

/// A picture drawn for a key, its side in pixels, and the id it is drawn
/// by: a new one with each picture, so that what draws it is drawn again
/// (a picture of the same size at the same place, under the same id,
/// would show nothing changed).
type Picture<K> = RefCell<Option<(K, MemoryRenderBuffer, i32, Id)>>;

/// The menu's pictures.
pub struct RadialPaint {
    fonts: OnceCell<Option<Fonts>>,
    main: Picture<LookKey>,
    arc: Picture<(u32, u32)>,
    panda: Picture<(u32, hocus_pandas::Mood, crate::pandas::Frame)>,
    shadow: FrameIds,
}

impl std::fmt::Debug for RadialPaint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadialPaint").finish_non_exhaustive()
    }
}

impl Default for RadialPaint {
    fn default() -> Self {
        Self {
            fonts: OnceCell::new(),
            main: RefCell::default(),
            arc: RefCell::default(),
            panda: RefCell::default(),
            shadow: FrameIds::default(),
        }
    }
}

/// What the user knows screen `output` by: its model, else its
/// connector's name.
pub fn screen_label(output: &Output) -> String {
    output
        .user_data()
        .get::<crate::panel::ScreenModel>()
        .map_or_else(|| output.name(), |m| m.0.clone())
}

/// The menu's words, French or English.
pub fn label(sector: Sector, maximized: bool, french: bool) -> &'static [&'static str] {
    match (sector, french) {
        (Sector::Up, true) if maximized => &["Restaurer"],
        (Sector::Up, false) if maximized => &["Restore"],
        (Sector::Up, true) => &["Agrandir"],
        (Sector::Up, false) => &["Maximise"],
        (Sector::UpRight, true) => &["Au premier", "plan"],
        (Sector::UpRight, false) => &["Always", "on top"],
        (Sector::Right, true) => &["Bureau", "suivant"],
        (Sector::Right, false) => &["Next", "desktop"],
        (Sector::DownRight, true) => &["Autre", "écran"],
        (Sector::DownRight, false) => &["Other", "screen"],
        (Sector::Down, true) => &["Réduire"],
        (Sector::Down, false) => &["Minimise"],
        (Sector::DownLeft, true) => &["Fermer"],
        (Sector::DownLeft, false) => &["Close"],
        (Sector::Left, true) => &["Bureau", "précédent"],
        (Sector::Left, false) => &["Previous", "desktop"],
        (Sector::UpLeft, true) => &["Tous les", "bureaux"],
        (Sector::UpLeft, false) => &["All", "desktops"],
    }
}

/// Where a sector's label goes, from the centre: its anchor's `x`, how
/// its lines line up (−1 ending there, 0 centred, 1 starting there), and
/// the baseline of its block's middle line.
fn label_place(sector: Sector, lines: usize) -> (f64, i8, f64) {
    let n = lines.saturating_sub(1) as f64;
    match sector {
        Sector::Up => (0.0, 0, -132.0 - LABEL_LINE / 2.0 * n),
        Sector::Down => (0.0, 0, 142.5 + LABEL_LINE / 2.0 * n),
        Sector::Right => (128.0, 1, 4.4),
        Sector::Left => (-128.0, -1, 4.4),
        Sector::UpRight => (93.3, 1, -89.0),
        Sector::UpLeft => (-93.3, -1, -89.0),
        Sector::DownRight => (93.3, 1, 97.7),
        Sector::DownLeft => (-93.3, -1, 97.7),
    }
}

/// What the hub says while `radial`'s lit sector is lit: its shortcut,
/// the desktop it sends to, the screen; for Close once armed, a click.
pub fn hub_words(radial: &Radial, armed: bool, french: bool, names: &[String]) -> String {
    let desktop = |n: usize| {
        names
            .get(n)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                if french {
                    format!("Bureau {}", n + 1)
                } else {
                    format!("Desktop {}", n + 1)
                }
            })
    };
    match radial.lit {
        Some(Sector::Up) if radial.maximized => "Super+↓".to_owned(),
        Some(Sector::Up) => "Super+↑".to_owned(),
        Some(Sector::Down) => "Super+H".to_owned(),
        Some(Sector::DownLeft) if armed => if french { "clic" } else { "click" }.to_owned(),
        Some(Sector::DownLeft) => "Super+Q".to_owned(),
        Some(Sector::Right) => desktop(radial.desktop + 1),
        Some(Sector::Left) => desktop(radial.desktop.saturating_sub(1)),
        Some(Sector::DownRight) => radial.other_screen.clone().unwrap_or_default(),
        Some(Sector::UpRight | Sector::UpLeft) | None => String::new(),
    }
}

/// A colour drawn for the dark theme, as it reads now (high contrast,
/// `crate::contrast`).
fn colour(c: [u8; 4]) -> Color {
    let [r, g, b, a] = crate::contrast::ink(c);
    Color::from_rgba8(r, g, b, a)
}

pub(crate) fn paint(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(colour(c));
    p.anti_alias = true;
    p
}

pub(crate) fn rgba(r: u8, g: u8, b: u8, a: f32) -> [u8; 4] {
    [r, g, b, (a * 255.0).round() as u8]
}

/// An annular wedge around `(cx, cy)` from `r0` to `r1`, between the
/// angles `a0` and `a1` (degrees, `y` down).
fn wedge(cx: f64, cy: f64, r0: f64, r1: f64, a0: f64, a1: f64) -> Option<tiny_skia::Path> {
    let steps = (((a1 - a0).abs() / 0.5).ceil() as usize).max(2);
    let at = |r: f64, a: f64| {
        let (s, c) = a.to_radians().sin_cos();
        ((cx + r * c) as f32, (cy + r * s) as f32)
    };
    let mut pb = PathBuilder::new();
    let (x, y) = at(r1, a0);
    pb.move_to(x, y);
    for i in 1..=steps {
        let (x, y) = at(r1, a0 + (a1 - a0) * i as f64 / steps as f64);
        pb.line_to(x, y);
    }
    for i in (0..=steps).rev() {
        let (x, y) = at(r0, a0 + (a1 - a0) * i as f64 / steps as f64);
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish()
}

/// A rounded rectangle.
pub(crate) fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    // The circle's quarter as a cubic: 0.5523 of the radius.
    let k = r * (1.0 - 0.552_284_8);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - k, y, x + w, y + k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - k, x + w - k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + k, y + h, x, y + h - k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + k, x + k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub(crate) fn stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

/// With its alpha scaled by `k`.
fn faded([r, g, b, a]: [u8; 4], k: f32) -> [u8; 4] {
    [r, g, b, (f32::from(a) * k).round() as u8]
}

/// Sector `sector`'s icon, its 24-unit drawing placed by `t`, in `ink`
/// (its back parts at 55 %).
fn icon(pixmap: &mut Pixmap, sector: Sector, t: Transform, ink: [u8; 4]) {
    let back = faded(ink, 0.55);
    let mut rect = |x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: [u8; 4], fill: bool| {
        if let Some(path) = rounded(x, y, w, h, r) {
            if fill {
                pixmap.fill_path(&path, &paint(faded(c, 0.25)), FillRule::Winding, t, None);
            }
            pixmap.stroke_path(&path, &paint(c), &stroke(width), t, None);
        }
    };
    let mut lines: Vec<(Vec<(f32, f32)>, f32)> = Vec::new();
    match sector {
        Sector::Up => rect(5.0, 5.0, 14.0, 14.0, 2.2, 1.9, ink, false),
        Sector::UpRight => {
            rect(4.0, 10.0, 11.0, 9.0, 2.0, 1.7, back, false);
            rect(9.0, 5.0, 11.0, 9.0, 2.0, 1.9, ink, true);
        }
        Sector::Right => {
            rect(3.0, 6.0, 8.0, 12.0, 1.8, 1.7, back, false);
            lines.push((vec![(13.0, 12.0), (21.0, 12.0)], 2.0));
            lines.push((vec![(17.5, 8.5), (21.0, 12.0), (17.5, 15.5)], 2.0));
        }
        Sector::DownRight => {
            rect(2.5, 4.5, 13.0, 9.0, 1.6, 1.7, back, false);
            rect(9.0, 10.0, 12.5, 8.5, 1.6, 1.9, ink, false);
            lines.push((vec![(13.0, 21.0), (18.0, 21.0)], 1.8));
        }
        Sector::Down => lines.push((vec![(6.0, 17.0), (18.0, 17.0)], 2.1)),
        Sector::DownLeft => {
            lines.push((vec![(7.0, 7.0), (17.0, 17.0)], 2.1));
            lines.push((vec![(17.0, 7.0), (7.0, 17.0)], 2.1));
        }
        Sector::Left => {
            rect(13.0, 6.0, 8.0, 12.0, 1.8, 1.7, back, false);
            lines.push((vec![(11.0, 12.0), (3.0, 12.0)], 2.0));
            lines.push((vec![(6.5, 8.5), (3.0, 12.0), (6.5, 15.5)], 2.0));
        }
        Sector::UpLeft => {
            for x in [3.0, 9.5, 16.0] {
                rect(x, 6.0, 5.0, 12.0, 1.5, 1.7, ink, false);
            }
            lines.push((vec![(5.5, 12.0), (18.5, 12.0)], 2.2));
        }
    }
    for (points, width) in lines {
        let mut pb = PathBuilder::new();
        for (i, (x, y)) in points.into_iter().enumerate() {
            if i == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &paint(ink), &stroke(width), t, None);
        }
    }
}

/// Lays `text` in `font` at `px`: its width and its glyphs, each with its
/// pen position.
pub(crate) fn advance(font: &fontdue::Font, text: &str, px: f32) -> f32 {
    let mut width = 0.0;
    let mut previous = None;
    for c in text.chars() {
        if let Some(p) = previous {
            width += font.horizontal_kern(p, c, px).unwrap_or(0.0);
        }
        width += font.metrics(c, px).advance_width;
        previous = Some(c);
    }
    width
}

/// Blends `colour` (straight alpha) over the pixmap's pixel at `(x, y)`
/// with `coverage`.
fn blend(pixmap: &mut Pixmap, x: i32, y: i32, colour: [u8; 4], coverage: u8) {
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    if x < 0 || y < 0 || x >= w || y >= h || coverage == 0 {
        return;
    }
    let a = u32::from(colour[3]) * u32::from(coverage) / 255;
    if a == 0 {
        return;
    }
    let at = ((y * w + x) * 4) as usize;
    let Some([r, g, b, alpha]) = pixmap.data_mut().get_mut(at..at + 4) else {
        return;
    };
    let over = |src: u32, dst: u8| (src + u32::from(dst) * (255 - a) / 255).min(255) as u8;
    let [cr, cg, cb, _] = colour;
    *r = over(u32::from(cr) * a / 255, *r);
    *g = over(u32::from(cg) * a / 255, *g);
    *b = over(u32::from(cb) * a / 255, *b);
    *alpha = over(a, *alpha);
}

/// Writes `text` in `font`, `px` high, its baseline's left at `(x, y)`
/// (pixmap pixels); with a dark outline `outline` pixels wide first.
pub(crate) fn write(
    pixmap: &mut Pixmap,
    font: &fontdue::Font,
    text: &str,
    px: f32,
    (x, y): (f32, f32),
    ink: [u8; 4],
    outline: f32,
) {
    // Under high contrast, text is never faded: an inert word reads in
    // `text.dim`, solid, at 7:1.
    let ink = match (crate::contrast::now(), crate::contrast::ink(ink)) {
        (Some(_), [r, g, b, a]) if a > 0 => [r, g, b, 255],
        (_, ink) => ink,
    };
    let mut glyphs = Vec::new();
    let mut pen = x;
    let mut previous = None;
    for c in text.chars() {
        if let Some(p) = previous {
            pen += font.horizontal_kern(p, c, px).unwrap_or(0.0);
        }
        let (metrics, coverage) = font.rasterize(c, px);
        let gx = (pen + metrics.xmin as f32).round() as i32;
        let gy = (y - (metrics.height as f32 + metrics.ymin as f32)).round() as i32;
        glyphs.push((gx, gy, metrics.width, coverage));
        pen += metrics.advance_width;
        previous = Some(c);
    }
    if outline > 0.0 {
        let reach = outline.ceil() as i32;
        let outline_ink = crate::contrast::ink(OUTLINE);
        for (gx, gy, width, coverage) in &glyphs {
            for (row, line) in coverage.chunks((*width).max(1)).enumerate() {
                for (col, &a) in line.iter().enumerate() {
                    if a < 16 {
                        continue;
                    }
                    for dy in -reach..=reach {
                        for dx in -reach..=reach {
                            if ((dx * dx + dy * dy) as f32).sqrt() > outline {
                                continue;
                            }
                            blend(
                                pixmap,
                                gx + col as i32 + dx,
                                gy + row as i32 + dy,
                                outline_ink,
                                a / 3,
                            );
                        }
                    }
                }
            }
        }
    }
    for (gx, gy, width, coverage) in glyphs {
        for (row, line) in coverage.chunks(width.max(1)).enumerate() {
            for (col, &a) in line.iter().enumerate() {
                blend(pixmap, gx + col as i32, gy + row as i32, ink, a);
            }
        }
    }
}

/// The colour of an application's letter tile, from its id.
pub(crate) fn letter_colour(app: &str) -> [u8; 4] {
    const TILES: [[u8; 4]; 6] = [
        [0x4F, 0xC1, 0xC9, 0xFF],
        [0x8F, 0xB3, 0xFF, 0xFF],
        [0xB4, 0x8C, 0xF0, 0xFF],
        [0xF2, 0x94, 0x5B, 0xFF],
        [0x6C, 0xC2, 0x87, 0xFF],
        [0x6F, 0x9B, 0xFF, 0xFF],
    ];
    let sum: usize = app.bytes().map(usize::from).sum();
    TILES.get(sum % TILES.len()).copied().unwrap_or_default()
}

/// The letter an application's tile shows: its id's last part's first.
pub(crate) fn letter_of(app: &str) -> String {
    app.rsplit('.')
        .next()
        .and_then(|n| n.chars().next())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "·".to_owned())
}

/// The whole menu but the arc and the panda, `2 × REACH` logical pixels
/// square at `scale`.
fn draw_main(fonts: &Fonts, key: &LookKey, scale: f64) -> Option<(Vec<u8>, i32)> {
    let size = (2.0 * REACH * scale).ceil() as u32;
    let mut pixmap = Pixmap::new(size, size)?;
    let s = scale as f32;
    let c = REACH;
    let t = Transform::from_scale(s, s);
    // The halo, legible labels over any window.
    let halo = RadialGradient::new(
        tiny_skia::Point::from_xy(c as f32, c as f32),
        tiny_skia::Point::from_xy(c as f32, c as f32),
        REACH as f32,
        vec![
            GradientStop::new(0.0, colour([7, 9, 15, 158])),
            GradientStop::new(0.6, colour([7, 9, 15, 158])),
            GradientStop::new(1.0, colour([7, 9, 15, 0])),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    );
    if let (Some(shader), Some(path)) = (
        halo,
        PathBuilder::from_circle(c as f32, c as f32, REACH as f32),
    ) {
        let mut p = Paint {
            shader,
            ..Paint::default()
        };
        p.anti_alias = true;
        pixmap.fill_path(&path, &p, FillRule::Winding, t, None);
    }
    // The disc.
    if let Some(path) = PathBuilder::from_circle(c as f32, c as f32, DISC as f32) {
        pixmap.fill_path(
            &path,
            &paint(rgba(14, 17, 28, 0.92)),
            FillRule::Winding,
            t,
            None,
        );
        pixmap.stroke_path(
            &path,
            &paint(rgba(255, 255, 255, 0.10)),
            &stroke(1.0),
            t,
            None,
        );
    }
    for ((sector, enabled), on) in Sector::ALL.into_iter().zip(key.enabled).zip(key.on) {
        let lit = key.lit == Some(sector) || key.picked == Some(sector);
        let close = sector == Sector::DownLeft;
        let middle = sector.angle().to_degrees();
        let (fill, rim, rim_width) = match (lit, close, enabled) {
            (true, _, _) if key.picked == Some(sector) => (faded(ACCENT, 0.55), ACCENT, 1.4),
            (true, true, _) => (rgba(242, 148, 91, 0.22), ORANGE, 1.4),
            (true, false, _) => (rgba(108, 194, 135, 0.24), ACCENT, 1.4),
            (false, _, true) => (rgba(255, 255, 255, 0.05), rgba(255, 255, 255, 0.06), 1.0),
            (false, _, false) => (rgba(255, 255, 255, 0.02), rgba(255, 255, 255, 0.06), 1.0),
        };
        if let Some(path) = wedge(c, c, HUB, RING, middle - HALF_SPAN, middle + HALF_SPAN) {
            pixmap.fill_path(&path, &paint(fill), FillRule::Winding, t, None);
            pixmap.stroke_path(&path, &paint(rim), &stroke(rim_width), t, None);
        }
        // The icon at mid-ring.
        let (sin, cos) = sector.angle().sin_cos();
        let (ix, iy) = (c + ICON_RADIUS * cos, c + ICON_RADIUS * sin);
        let ink = if !enabled {
            OFF_INK
        } else if close {
            ORANGE_INK
        } else if on {
            ON
        } else {
            TEXT
        };
        let k = (ICON / 24.0) as f32;
        let place = Transform::from_row(
            k,
            0.0,
            0.0,
            k,
            (ix - ICON / 2.0) as f32,
            (iy - ICON / 2.0) as f32,
        )
        .post_scale(s, s);
        icon(&mut pixmap, sector, place, ink);
        // A toggle on: a dot at the icon's top right.
        if on
            && let Some(path) =
                PathBuilder::from_circle((ix + ICON / 2.0) as f32, (iy - ICON / 2.0) as f32, 3.2)
        {
            pixmap.fill_path(&path, &paint(ON), FillRule::Winding, t, None);
        }
        // The label outside the ring.
        let lines = label(sector, key.maximized, key.french);
        let (ax, align, middle_baseline) = label_place(sector, lines.len());
        let font = if lit { &fonts.bold } else { &fonts.semibold };
        let ink = match (enabled, close, lit) {
            (false, _, _) => DIM_OFF,
            (true, true, _) => ORANGE_INK,
            (true, false, true) => TEXT,
            (true, false, false) => DIM,
        };
        let px = LABEL * s;
        let first = middle_baseline - LABEL_LINE / 2.0 * (lines.len() as f64 - 1.0);
        for (n, line) in lines.iter().enumerate() {
            let width = advance(font, line, px);
            let x = (c + ax) as f32 * s;
            let x = match align {
                -1 => x - width,
                0 => x - width / 2.0,
                _ => x,
            };
            let y = (c + first + LABEL_LINE * n as f64) as f32 * s;
            write(&mut pixmap, font, line, px, (x, y), ink, 1.5 * s);
        }
    }
    // The hub.
    if let Some(path) = PathBuilder::from_circle(c as f32, c as f32, HUB_CIRCLE as f32) {
        pixmap.fill_path(
            &path,
            &paint(rgba(255, 255, 255, 0.04)),
            FillRule::Winding,
            t,
            None,
        );
        pixmap.stroke_path(
            &path,
            &paint(rgba(255, 255, 255, 0.08)),
            &stroke(1.0),
            t,
            None,
        );
    }
    if !key.hub.is_empty() {
        // The words, cut to the hub.
        let px = HUB_TEXT * s;
        let room = (2.0 * HUB_CIRCLE - 10.0) as f32 * s;
        let mut words = key.hub.clone();
        if advance(&fonts.mono, &words, px) > room {
            let mut cut = words;
            while !cut.is_empty() && advance(&fonts.mono, &format!("{cut}…"), px) > room {
                cut.pop();
            }
            words = format!("{}…", cut.trim_end());
        }
        let width = advance(&fonts.mono, &words, px);
        let x = c as f32 * s - width / 2.0;
        let y = (c + 4.0) as f32 * s;
        write(&mut pixmap, &fonts.mono, &words, px, (x, y), TEXT, 0.0);
    } else if !key.panda {
        // The application's letter tile.
        let tile = 28.0;
        if let Some(path) = rounded(
            (c - tile / 2.0) as f32,
            (c - tile / 2.0) as f32,
            tile as f32,
            tile as f32,
            7.0,
        ) {
            pixmap.fill_path(
                &path,
                &paint(letter_colour(&key.app)),
                FillRule::Winding,
                t,
                None,
            );
        }
        let letter = letter_of(&key.app);
        let px = 15.0 * s;
        let width = advance(&fonts.bold, &letter, px);
        let x = c as f32 * s - width / 2.0;
        let y = (c + 5.5) as f32 * s;
        write(
            &mut pixmap,
            &fonts.bold,
            &letter,
            px,
            (x, y),
            [0x0B, 0x0E, 0x16, 0xFF],
            0.0,
        );
    }
    Some((argb(pixmap), size as i32))
}

/// The arc along Close's rim, `share` of it, on a picture of the rim's
/// corner: `side` logical pixels square, its corner at
/// [`arc_corner`] from the centre.
const ARC_SIDE: f64 = 84.0;
fn arc_corner() -> (f64, f64) {
    (-RING - 2.0, RING - ARC_SIDE + 2.0)
}

fn draw_arc(share: f64, scale: f64) -> Option<(Vec<u8>, i32)> {
    let size = (ARC_SIDE * scale).ceil() as u32;
    let mut pixmap = Pixmap::new(size, size)?;
    let (ox, oy) = arc_corner();
    let middle = Sector::DownLeft.angle().to_degrees();
    // From the side nearest the bottom, round toward the left.
    let a0 = middle - HALF_SPAN;
    let a1 = a0 + 2.0 * HALF_SPAN * share;
    let path = wedge(-ox, -oy, RING - 5.0, RING, a0, a1)?;
    let s = scale as f32;
    pixmap.fill_path(
        &path,
        &paint(ORANGE),
        FillRule::Winding,
        Transform::from_scale(s, s),
        None,
    );
    Some((argb(pixmap), size as i32))
}

/// The hub's panda in `mood` at `frame`, `PANDA` logical pixels at
/// `scale`.
fn draw_panda(
    mood: hocus_pandas::Mood,
    frame: crate::pandas::Frame,
    scale: f64,
) -> Option<(Vec<u8>, i32)> {
    let size = (PANDA * scale).ceil() as u32;
    crate::pandas::draw(mood, frame, size).map(|pixels| (pixels, size as i32))
}

/// tiny-skia's premultiplied RGBA as the renderer's ARGB8888 (B, G, R, A
/// in memory).
pub(crate) fn argb(pixmap: Pixmap) -> Vec<u8> {
    argb_of(pixmap.take())
}

/// Premultiplied RGBA bytes as the renderer's ARGB8888.
pub(crate) fn argb_of(mut data: Vec<u8>) -> Vec<u8> {
    for px in data.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    data
}

impl State {
    /// Whether the menu's words are French: `[locale] language`, else the
    /// session's locale.
    pub(crate) fn radial_french(&self) -> bool {
        use hocus_link::locale::Language;
        match self.config.locale.language {
            Language::French => true,
            Language::English => false,
            Language::System => ["LC_ALL", "LC_MESSAGES", "LANG"]
                .into_iter()
                .filter_map(|name| std::env::var(name).ok())
                .find(|value| !value.is_empty())
                .is_some_and(|value| value.starts_with("fr")),
        }
    }

    /// The radial menu on `output`, front first: the panda, the arc, the
    /// menu, its shadow.
    pub(crate) fn radial_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some((radial, alpha, k, picked)) = self.radial_shown(now) else {
            return;
        };
        if radial.output != *output || alpha <= 0.0 {
            return;
        }
        let Some(geometry) = self.space.output_geometry(output) else {
            return;
        };
        let paint = &self.radial_menu.paint;
        let Some(fonts) = paint.fonts.get_or_init(fonts) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let french = self.radial_french();
        let armed = radial.armed(now);
        let names = self.config.desktops.shown_names();
        let key = LookKey {
            scale: (scale * 1000.0).round() as u32,
            lit: radial.lit,
            picked,
            armed,
            enabled: radial.enabled,
            on: radial.on,
            maximized: radial.maximized,
            panda: radial.agent.is_some(),
            french,
            hub: hub_words(radial, armed, french, &names),
            app: radial.app.clone(),
        };
        // The centre on the output, in its pixels.
        let centre = Point::<f64, Logical>::from((
            radial.centre.0 - f64::from(geometry.loc.x),
            radial.centre.1 - f64::from(geometry.loc.y),
        ));
        // A square of `side` logical pixels whose corner is `corner` from
        // the centre, scaled by `k` about the centre, on the output's
        // pixels.
        let placed = |corner: (f64, f64), side: f64| {
            let x = centre.x + corner.0 * k;
            let y = centre.y + corner.1 * k;
            let rect = Rectangle::<f64, Logical>::new((x, y).into(), (side * k, side * k).into());
            rect.to_physical_precise_round::<f64, i32>(scale)
        };
        let mut draw = |texture: &MemoryRenderBuffer, px: i32, dst, id: &Id, out: &mut Vec<_>| {
            let all = Rectangle::from_size((f64::from(px), f64::from(px)).into());
            match Placed::new(renderer, texture, all, dst, alpha) {
                Ok(e) => out.push(OutputElements::Frame(Keyed::new(e, id.clone()))),
                Err(err) => tracing::warn!(?err, "could not draw the radial menu"),
            }
        };
        // The panda, playing its mood as the menu opens, a waiting one
        // bobbing.
        if let Some(state) = radial.agent.filter(|_| key.hub.is_empty()) {
            let mood = crate::pandas::mood(state);
            let lively = self.pandas_lively();
            let frame = lively
                .then(|| crate::pandas::frame(mood.burst(), now.saturating_sub(radial.opened)))
                .flatten();
            let wanted = (key.scale, mood, frame);
            let mut cache = paint.panda.borrow_mut();
            if cache.as_ref().is_none_or(|(k, _, _, _)| *k != wanted)
                && let Some((pixels, px)) = draw_panda(mood, frame, scale)
            {
                *cache = Some((wanted, buffer(&pixels, px, px), px, Id::new()));
            }
            if let Some((_, texture, px, id)) = cache.as_ref() {
                let bob = if !lively || state != crate::overview::AgentState::Waiting {
                    0.0
                } else {
                    let t = now.saturating_sub(radial.opened).as_secs_f64();
                    BOB * (t * std::f64::consts::TAU / BOB_PERIOD).sin()
                };
                let dst = placed((-PANDA / 2.0, -PANDA / 2.0 + bob), PANDA);
                draw(texture, *px, dst, id, out);
            }
        }
        // The arc along Close's rim, resting.
        if let Some(share) = radial
            .rest_share(now)
            .filter(|s| *s > 0.0 && radial.lit == Some(Sector::DownLeft))
        {
            let step = (share * 40.0).round() as u32;
            let mut cache = paint.arc.borrow_mut();
            if cache
                .as_ref()
                .is_none_or(|(k, _, _, _)| *k != (key.scale, step))
                && let Some((pixels, px)) = draw_arc(f64::from(step) / 40.0, scale)
            {
                *cache = Some(((key.scale, step), buffer(&pixels, px, px), px, Id::new()));
            }
            if let Some((_, texture, px, id)) = cache.as_ref() {
                draw(texture, *px, placed(arc_corner(), ARC_SIDE), id, out);
            }
        }
        // The menu.
        {
            let mut cache = paint.main.borrow_mut();
            if cache.as_ref().is_none_or(|(k, _, _, _)| *k != key)
                && let Some((pixels, px)) = draw_main(fonts, &key, scale)
            {
                *cache = Some((key.clone(), buffer(&pixels, px, px), px, Id::new()));
            }
            if let Some((_, texture, px, id)) = cache.as_ref() {
                draw(texture, *px, placed((-REACH, -REACH), 2.0 * REACH), id, out);
            }
        }
        // The disc's shadow.
        let disc = Rectangle::<f64, Logical>::new(
            (centre.x - DISC * k, centre.y - DISC * k).into(),
            Size::from((2.0 * DISC * k, 2.0 * DISC * k)),
        );
        out.extend(
            shadow_elements(
                renderer,
                &self.textures,
                &paint.shadow,
                ShadowStyle {
                    radius: DISC * k,
                    ..SHADOW
                },
                disc,
                scale,
                alpha,
            )
            .into_iter()
            .map(OutputElements::Shadow),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_label_stays_inside_the_menus_square_in_both_languages() {
        let fonts = fonts().expect("the shell's fonts");
        for french in [true, false] {
            for maximized in [true, false] {
                for sector in Sector::ALL {
                    let lines = label(sector, maximized, french);
                    let (ax, align, middle) = label_place(sector, lines.len());
                    let first = middle - LABEL_LINE / 2.0 * (lines.len() as f64 - 1.0);
                    for (n, line) in lines.iter().enumerate() {
                        // Bold, as when lit: the widest.
                        let width = f64::from(advance(&fonts.bold, line, LABEL));
                        let x = match align {
                            -1 => ax - width,
                            0 => ax - width / 2.0,
                            _ => ax,
                        };
                        let baseline = first + LABEL_LINE * n as f64;
                        // Its outline included.
                        assert!(
                            x - 2.0 >= -REACH && x + width + 2.0 <= REACH,
                            "{line:?} across: {x}…{}",
                            x + width
                        );
                        assert!(
                            baseline - 14.0 >= -REACH && baseline + 5.0 <= REACH,
                            "{line:?} down: {baseline}"
                        );
                        // Clear of the disc.
                        let (dx, dy) = (x.abs().min((x + width).abs()), baseline.abs());
                        assert!(
                            align == 0 || dx > DISC * 0.6 || dy > DISC * 0.6,
                            "{line:?} over the ring"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_menu_draws_at_any_scale_and_its_pieces_too() {
        let fonts = fonts().expect("the shell's fonts");
        let key = LookKey {
            scale: 1500,
            lit: Some(Sector::Up),
            picked: None,
            armed: false,
            enabled: [true; 8],
            on: [false, true, false, false, false, false, false, false],
            maximized: false,
            panda: false,
            french: true,
            hub: "Super+↑".into(),
            app: "foot".into(),
        };
        for scale in [1.0, 1.5, 2.0] {
            let (pixels, px) = draw_main(&fonts, &key, scale).expect("a picture");
            assert_eq!(px, (2.0 * REACH * scale).ceil() as i32);
            assert_eq!(pixels.len(), (px * px * 4) as usize);
            // The centre is the hub's dark disc, the corners are clear.
            assert_eq!(pixels[3], 0);
            let mid = ((px / 2 * px + px / 2) * 4) as usize;
            assert!(pixels[mid + 3] > 200);
            assert!(draw_arc(0.5, scale).is_some());
            assert!(draw_panda(hocus_pandas::Mood::Waiting, None, scale).is_some());
        }
    }

    #[test]
    fn the_hub_says_the_lit_sectors_shortcut_or_where_it_sends() {
        let output = Output::new(
            "test".into(),
            smithay::output::PhysicalProperties {
                size: (0, 0).into(),
                subpixel: smithay::output::Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
            },
        );
        let mut radial = Radial {
            window: crate::model::WindowId(1),
            output,
            centre: (0.0, 0.0),
            origin: (0.0, 0.0),
            enabled: [true; 8],
            on: [false; 8],
            maximized: false,
            desktop: 0,
            other_screen: Some("DP-2".into()),
            app: String::new(),
            agent: None,
            lit: Some(Sector::Right),
            rest: crate::radial::Rest::Away,
            last: (0.0, 0.0),
            opened: Duration::ZERO,
        };
        assert_eq!(hub_words(&radial, false, true, &[]), "Bureau 2");
        assert_eq!(
            hub_words(&radial, false, false, &["Mail".into(), "Code".into()]),
            "Code"
        );
        radial.lit = Some(Sector::DownLeft);
        assert_eq!(hub_words(&radial, false, true, &[]), "Super+Q");
        assert_eq!(hub_words(&radial, true, true, &[]), "clic");
        radial.lit = Some(Sector::DownRight);
        assert_eq!(hub_words(&radial, false, true, &[]), "DP-2");
        radial.lit = None;
        assert_eq!(hub_words(&radial, false, true, &[]), "");
    }
}
