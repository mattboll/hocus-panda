//! A head in three dimensions, and what draws on it (lot 4): Visage's
//! accessories and the panda that takes the person's place.
//!
//! - **The rig** ([`Rig`]): head-local units (the head's half width is
//!   30, its middle at the origin, y down, z towards the camera), turned
//!   by the head's yaw and pitch, seen in perspective, then turned by its
//!   roll in the picture, scaled and placed: the approved panda mockup's
//!   own rig, used for the person's head too.
//! - **Paths through it** ([`Cmd`], [`trace`]): a drawing's outline, its
//!   points each taken through the rig, so that a flat drawing on the head
//!   turns with it.
//! - **Paint** ([`rgba`], [`linear`], [`radial`]): tiny-skia's, with the
//!   mockup's colours and gradients.

use resvg::tiny_skia::{
    self, Color, FillRule, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap,
    Point, RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

/// The head turned and placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rig {
    cy: f32,
    sy: f32,
    cp: f32,
    sp: f32,
    pub cr: f32,
    pub sr: f32,
    /// The camera's distance, head-local units.
    d: f32,
    /// Frame pixels a unit, and where the origin is.
    pub k: f32,
    pub ox: f32,
    pub oy: f32,
}

impl Rig {
    pub fn new(yaw: f32, pitch: f32, roll: f32, k: f32, origin: (f32, f32), d: f32) -> Self {
        let (sy, cy) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let (sr, cr) = roll.sin_cos();
        Self {
            cy,
            sy,
            cp,
            sp,
            cr,
            sr,
            d,
            k,
            ox: origin.0,
            oy: origin.1,
        }
    }

    /// A head-local point turned and seen (still head-local units, before
    /// the roll), and its depth after the turn.
    pub fn turn(&self, x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        let x1 = x * self.cy + z * self.sy;
        let z1 = -x * self.sy + z * self.cy;
        let y2 = y * self.cp - z1 * self.sp;
        let z2 = y * self.sp + z1 * self.cp;
        let f = self.d / (self.d - z2).max(1.0);
        (x1 * f, y2 * f, z2)
    }

    /// A point seen from the camera (head-local units) to the frame.
    pub fn place(&self, x: f32, y: f32) -> (f32, f32) {
        let (rx, ry) = (x * self.cr - y * self.sr, x * self.sr + y * self.cr);
        (self.ox + rx * self.k, self.oy + ry * self.k)
    }

    /// A head-local point in the frame, and its depth.
    pub fn p(&self, x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        let (a, b, z) = self.turn(x, y, z);
        let (fx, fy) = self.place(a, b);
        (fx, fy, z)
    }

    /// Without its depth.
    pub fn xy(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
        let (a, b, _) = self.p(x, y, z);
        (a, b)
    }
}

/// A path's command, absolute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

/// An SVG path's `d` (M, L, Q, C, Z, absolute), its points moved by
/// (`-ox`, `-oy`).
pub fn parse(d: &str, ox: f32, oy: f32) -> Vec<Cmd> {
    let mut out = Vec::new();
    let mut numbers: Vec<f32> = Vec::new();
    let mut cmd = 'M';
    let flush = |cmd: &mut char, numbers: &mut Vec<f32>, out: &mut Vec<Cmd>| {
        let n = match *cmd {
            'M' | 'L' => 2,
            'Q' => 4,
            'C' => 6,
            _ => return,
        };
        for c in numbers.chunks_exact(n) {
            let p = |i: usize| {
                c.get(i).copied().unwrap_or(0.0) - if i.is_multiple_of(2) { ox } else { oy }
            };
            out.push(match *cmd {
                'M' => Cmd::M(p(0), p(1)),
                'L' => Cmd::L(p(0), p(1)),
                'Q' => Cmd::Q(p(0), p(1), p(2), p(3)),
                _ => Cmd::C(p(0), p(1), p(2), p(3), p(4), p(5)),
            });
            if *cmd == 'M' {
                *cmd = 'L';
            }
        }
        numbers.clear();
    };
    let mut token = String::new();
    let push_token = |token: &mut String, numbers: &mut Vec<f32>| {
        if let Ok(v) = token.parse::<f32>() {
            numbers.push(v);
        }
        token.clear();
    };
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() {
            push_token(&mut token, &mut numbers);
            flush(&mut cmd, &mut numbers, &mut out);
            cmd = ch.to_ascii_uppercase();
            if cmd == 'Z' {
                out.push(Cmd::Z);
            }
        } else if ch == '-' && !token.is_empty() {
            push_token(&mut token, &mut numbers);
            token.push(ch);
        } else if ch.is_ascii_digit() || ch == '.' || ch == '-' {
            if ch == '.' && token.contains('.') {
                push_token(&mut token, &mut numbers);
            }
            token.push(ch);
        } else {
            push_token(&mut token, &mut numbers);
        }
    }
    push_token(&mut token, &mut numbers);
    flush(&mut cmd, &mut numbers, &mut out);
    out
}

/// `cmds`, each point through `map`.
pub fn trace(cmds: &[Cmd], map: impl Fn(f32, f32) -> (f32, f32)) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for c in cmds {
        match *c {
            Cmd::M(x, y) => {
                let (a, b) = map(x, y);
                pb.move_to(a, b);
            }
            Cmd::L(x, y) => {
                let (a, b) = map(x, y);
                pb.line_to(a, b);
            }
            Cmd::Q(x1, y1, x, y) => {
                let ((a, b), (c, d)) = (map(x1, y1), map(x, y));
                pb.quad_to(a, b, c, d);
            }
            Cmd::C(x1, y1, x2, y2, x, y) => {
                let ((a, b), (c, d), (e, f)) = (map(x1, y1), map(x2, y2), map(x, y));
                pb.cubic_to(a, b, c, d, e, f);
            }
            Cmd::Z => pb.close(),
        }
    }
    pb.finish()
}

/// A closed polygon.
pub fn poly(points: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    pb.finish()
}

/// An open polyline.
pub fn line(points: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.finish()
}

/// An ellipse's outline (`n` points), centred at (`cx`, `cy`), turned by
/// `rot`, each point through `map`.
pub fn ring(
    map: impl Fn(f32, f32) -> (f32, f32),
    (cx, cy): (f32, f32),
    (rx, ry): (f32, f32),
    n: usize,
    rot: f32,
) -> Vec<(f32, f32)> {
    let (s, c) = rot.sin_cos();
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let (x, y) = (rx * a.cos(), ry * a.sin());
            map(cx + x * c - y * s, cy + x * s + y * c)
        })
        .collect()
}

/// An ellipse in the picture.
pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, rot: f32) -> Option<Path> {
    poly(&ring(|x, y| (x, y), (cx, cy), (rx, ry), 40, rot))
}

/// A rounded rectangle in the picture.
pub fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// A colour, `0xRRGGBB` at `alpha`.
pub fn rgba(hex: u32, alpha: f32) -> Color {
    let [_, r, g, b] = hex.to_be_bytes();
    Color::from_rgba8(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn stops(stops: &[(f32, Color)]) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|&(at, c)| GradientStop::new(at.clamp(0.0, 1.0), c))
        .collect()
}

/// A linear gradient from `a` to `b`.
pub fn linear(a: (f32, f32), b: (f32, f32), at: &[(f32, Color)]) -> Shader<'static> {
    LinearGradient::new(
        Point::from_xy(a.0, a.1),
        Point::from_xy(b.0 + 1e-3, b.1),
        stops(at),
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(
        at.first().map_or(Color::TRANSPARENT, |s| s.1),
    ))
}

/// A radial gradient, from `focus` (canvas's inner circle, taken as a
/// point) to the circle at `centre` of radius `r`.
pub fn radial(
    focus: (f32, f32),
    centre: (f32, f32),
    r: f32,
    at: &[(f32, Color)],
) -> Shader<'static> {
    RadialGradient::new(
        Point::from_xy(focus.0, focus.1),
        Point::from_xy(centre.0, centre.1),
        r.max(0.01),
        stops(at),
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(
        at.first().map_or(Color::TRANSPARENT, |s| s.1),
    ))
}

/// Paint of a shader, anti-aliased.
pub fn paint(shader: Shader<'static>) -> Paint<'static> {
    Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    }
}

pub fn solid(c: Color) -> Paint<'static> {
    paint(Shader::SolidColor(c))
}

/// `path` filled with `paint` on `pixmap`, within `clip`.
pub fn fill(pixmap: &mut Pixmap, path: &Path, paint: &Paint<'_>, clip: Option<&Mask>) {
    pixmap.fill_path(path, paint, FillRule::Winding, Transform::identity(), clip);
}

/// `path` stroked `width` wide.
pub fn stroke(
    pixmap: &mut Pixmap,
    path: &Path,
    paint: &Paint<'_>,
    width: f32,
    clip: Option<&Mask>,
) {
    let stroke = Stroke {
        width,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, paint, &stroke, Transform::identity(), clip);
}

/// A mask of `path` on a pixmap's size.
pub fn mask(pixmap: &Pixmap, path: &Path) -> Option<Mask> {
    let mut m = Mask::new(pixmap.width(), pixmap.height())?;
    m.fill_path(path, FillRule::Winding, true, Transform::identity());
    Some(m)
}

/// The convex hull of `points`, round: a convex shape's outline from
/// its points seen.
pub fn hull(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut pts: Vec<(f32, f32)> = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let turn = |o: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut lower: Vec<(f32, f32)> = Vec::new();
    for &p in &pts {
        while let [.., a, b] = lower.as_slice()
            && turn(*a, *b, p) <= 0.0
        {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f32, f32)> = Vec::new();
    for &p in pts.iter().rev() {
        while let [.., a, b] = upper.as_slice()
            && turn(*a, *b, p) <= 0.0
        {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Of a closed ring of `n` points, those `seen` says face the camera, as
/// one run from its first to its last (empty when none does).
pub fn front_run(n: usize, seen: impl Fn(usize) -> bool) -> Vec<usize> {
    let shown: Vec<bool> = (0..n).map(&seen).collect();
    if shown.iter().all(|&s| s) {
        return (0..n).collect();
    }
    // Start just after a hidden one.
    let Some(start) = (0..n).find(|&i| !shown.get(i).copied().unwrap_or(false)) else {
        return Vec::new();
    };
    (1..=n)
        .map(|k| (start + k) % n)
        .filter(|&i| shown.get(i).copied().unwrap_or(false))
        .collect()
}

/// Smoothstep from `a` to `b`.
pub fn sm(a: f32, b: f32, x: f32) -> f32 {
    let u = ((x - a) / (b - a)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

pub fn lerp(a: f32, b: f32, u: f32) -> f32 {
    a + (b - a) * u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_reads_and_a_turn_turns() {
        let cmds = parse("M39 47 C43 40 54 42 55.5 50 L1-2 Z", 60.0, 54.0);
        assert_eq!(
            cmds,
            [
                Cmd::M(-21.0, -7.0),
                Cmd::C(-17.0, -14.0, -6.0, -12.0, -4.5, -4.0),
                Cmd::L(-59.0, -56.0),
                Cmd::Z
            ]
        );
        // Turned a quarter aside, a point in front goes to the side.
        let rig = Rig::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0, 1.0, (0.0, 0.0), 1e6);
        let (x, _, z) = rig.p(0.0, 0.0, 10.0);
        assert!((x - 10.0).abs() < 1e-3 && z.abs() < 1e-3, "{x} {z}");
    }
}
