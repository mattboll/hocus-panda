//! How the snow looks (#795), the board's drawing ported to triangles
//! ([`super::mesh`]): on the layer under the windows the flakes (a soft
//! disc each, its size, alpha and softness by its plane), the ground's pile
//! (white to blue) and the clumps falling; on a top edge its cap: a soft
//! shadow on the edge, the snow smoothed (a 9-tap blur, its ends thinner)
//! along quadratic curves, white to blue from its peak down, a highlight
//! along its top, and glints now and then.
//!
//! Places are given in logical pixels and drawn output-local in physical
//! ones: `origin` is the output's corner (global), `scale` its scale.

use smithay::utils::{Logical, Rectangle};

use super::{
    mesh::{Mesh, Shape},
    snow::{COL, Cap, Field, GROUND_COL},
};

/// How a cap's snow rests on its edge: its base this far below it.
pub const BASE: f32 = 2.0;
/// Its shadow on the edge: inset at the sides, deep, blurred.
const SHADOW_INSET: f32 = 4.0;
const SHADOW_DEPTH: f32 = 5.0;
const SHADOW_BLUR: f32 = 3.0;
/// Below this, what lies on the edge (the shadow's last blur) is cut:
/// nothing of the cap ever falls on what the window shows under it.
pub const REACH_BELOW: f64 = 8.0;

const BLUR: [f32; 9] = [1.0, 3.0, 6.0, 9.0, 10.0, 9.0, 6.0, 3.0, 1.0];

fn rgb(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
    ]
}

/// `c` at `alpha`, premultiplied.
fn premul(c: [f32; 3], alpha: f32) -> [f32; 4] {
    [c[0] * alpha, c[1] * alpha, c[2] * alpha, alpha]
}

const CLEAR: [f32; 4] = [0.0; 4];

/// A gradient's colour at `t` (0 to 1) between its stops (one at least).
#[expect(
    clippy::indexing_slicing,
    reason = "every gradient here has stops, and colours of three channels"
)]
fn gradient(stops: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let mut prev = stops[0];
    for &(at, c) in stops {
        if t <= at {
            let span = at - prev.0;
            let k = if span > 0.0 { (t - prev.0) / span } else { 1.0 };
            return std::array::from_fn(|i| prev.1[i] + (c[i] - prev.1[i]) * k);
        }
        prev = (at, c);
    }
    prev.1
}

/// The layer under the windows of screen `field` (its corner the output's):
/// the ground's pile, and with `falling` the flakes and the clumps.
pub fn layer(field: &Field, scale: f32, falling: bool) -> Mesh {
    let mut mesh = Mesh::default();
    if falling {
        for f in &field.flakes {
            // The board's sprites: small and sharp far off, large and soft
            // near.
            let (stop, across) = match f.z {
                z if z < 0.8 => (0.45, 4.0),
                _ => (0.25, 5.0),
            };
            let half = f.r * across / 2.0 * scale;
            let alpha = 0.25 + 0.65 * f.z;
            mesh.square(
                [f.x * scale, f.y * scale],
                [half, half],
                [1.0, 1.0],
                [alpha; 4],
                Shape::Flake,
                [stop, 0.0, 0.0],
            );
        }
    }
    ground(&mut mesh, field, scale);
    if falling {
        for k in &field.clumps {
            let (sin, cos) = k.rot.sin_cos();
            let (rx, ry) = (k.r, k.r * 0.8);
            let corner = |u: f32, v: f32| {
                let (x, y) = (u * rx, v * ry);
                [
                    (k.x + x * cos - y * sin) * scale,
                    (k.y + x * sin + y * cos) * scale,
                ]
            };
            mesh.quad(
                [
                    corner(-1.0, -1.0),
                    corner(1.0, -1.0),
                    corner(1.0, 1.0),
                    corner(-1.0, 1.0),
                ],
                [[-1.0, -0.8], [1.0, -0.8], [1.0, 0.8], [-1.0, 0.8]],
                [1.0; 4],
                Shape::Clump,
                [k.r * scale, 0.0, 0.0],
            );
        }
    }
    mesh
}

/// What the layer of a screen draws where (#815): each flake's and
/// clump's box (whole physical pixels, `x0, y0, x1, y1`), the ground's
/// columns, and the scale and the size they were drawn at.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Pieces {
    flakes: Vec<[i32; 4]>,
    clumps: Vec<[i32; 4]>,
    ground: Vec<f32>,
    at: (f32, (f32, f32)),
}

/// What [`layer`] draws where on screen `field` at `scale`.
pub fn pieces(field: &Field, scale: f32, falling: bool) -> Pieces {
    let square = |x: f32, y: f32, half: f32| {
        [
            ((x - half) * scale).floor() as i32 - 1,
            ((y - half) * scale).floor() as i32 - 1,
            ((x + half) * scale).ceil() as i32 + 1,
            ((y + half) * scale).ceil() as i32 + 1,
        ]
    };
    let (flakes, clumps) = if falling {
        (
            field
                .flakes
                .iter()
                .map(|f| {
                    let across = if f.z < 0.8 { 4.0 } else { 5.0 };
                    square(f.x, f.y, f.r * across / 2.0)
                })
                .collect(),
            // A clump turned: within its radius each way.
            field.clumps.iter().map(|k| square(k.x, k.y, k.r)).collect(),
        )
    } else {
        Default::default()
    };
    Pieces {
        flakes,
        clumps,
        ground: field.ground.clone(),
        at: (scale, field.size),
    }
}

/// Where the layer changed from `old` to `new` (physical, `x, y, w, h`);
/// `None`: all of it.
pub fn layer_changes(old: &Pieces, new: &Pieces) -> Option<Vec<[i32; 4]>> {
    if old.at != new.at
        || old.flakes.len() != new.flakes.len()
        || old.ground.len() != new.ground.len()
    {
        return None;
    }
    let mut out: Vec<[i32; 4]> = Vec::new();
    let mut add = |[x0, y0, x1, y1]: [i32; 4]| out.push([x0, y0, x1 - x0, y1 - y0]);
    for (a, b) in old.flakes.iter().zip(&new.flakes) {
        if a == b {
            continue;
        }
        // Where it was and where it is, in one when they meet.
        if a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3] {
            add([
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]);
        } else {
            add(*a);
            add(*b);
        }
    }
    for k in old.clumps.iter().chain(&new.clumps) {
        add(*k);
    }
    // A column's pile reaches its neighbours'.
    let (scale, (w, h)) = new.at;
    let n = new.ground.len();
    let mut i = 0;
    while i < n {
        if old.ground.get(i) == new.ground.get(i) {
            i += 1;
            continue;
        }
        let from = i;
        let mut top = 0.0f32;
        while i < n && old.ground.get(i) != new.ground.get(i) {
            let at = |g: &[f32]| g.get(i).copied().unwrap_or(0.0);
            top = top.max(at(&old.ground)).max(at(&new.ground));
            i += 1;
        }
        let x0 = (from.saturating_sub(1) as f32 * GROUND_COL * scale).floor() as i32 - 1;
        let x1 = ((((i + 1) as f32 * GROUND_COL).min(w)) * scale).ceil() as i32 + 1;
        let y0 = ((h - top - 1.0) * scale).floor() as i32 - 1;
        add([x0, y0, x1, (h * scale).ceil() as i32 + 1]);
    }
    Some(out)
}

/// The ground's pile: straight from column to column, white 50 px up to
/// blue at the bottom.
fn ground(mesh: &mut Mesh, field: &Field, scale: f32) {
    let (w, h) = field.size;
    let stops = [(0.0, rgb(0xF4F7FC)), (1.0, rgb(0xAFC0D8))];
    let colour = |y: f32| premul(gradient(&stops, (y - (h - 50.0)) / 50.0), 1.0);
    let half = 0.5 / scale;
    let column = |x: f32, g: f32| {
        let top = h - g;
        (
            x * scale,
            [
                ((top - half) * scale, CLEAR),
                ((top + half).min(h) * scale, colour(top)),
                (h * scale, colour(h)),
            ],
        )
    };
    let n = field.ground.len();
    let mut columns: Vec<_> = field
        .ground
        .iter()
        .enumerate()
        .map(|(i, g)| column(i as f32 * GROUND_COL, *g))
        .collect();
    if let Some(last) = field.ground.last() {
        columns.push(column(w.max((n - 1) as f32 * GROUND_COL), *last));
    }
    mesh.columns(&columns);
}

/// A cap's look this frame.
#[derive(Debug, Clone, Copy)]
pub struct CapLook {
    /// Its edge where it is drawn (global, logical): its cap stretched to
    /// it.
    pub edge: Rectangle<f64, Logical>,
    pub origin: (f64, f64),
    pub scale: f32,
    pub alpha: f32,
    /// The snow's clock for its glints; none still.
    pub glints: Option<f32>,
}

/// The cap of `cap` as `look` says, drawn over what is under it (the
/// shadow, the snow and its highlight; its glints are [`glints`]); `None`
/// when there is too little snow to show.
#[expect(
    clippy::indexing_slicing,
    reason = "a cap of four columns or more, its smoothed heights as many; the outline holds \
              the curve's points from top_from to top_to"
)]
pub fn cap(cap: &Cap, look: &CapLook) -> Option<Mesh> {
    let n = cap.h.len();
    if n < 4 {
        return None;
    }
    let s = smoothed(&cap.h);
    let peak = s.iter().copied().fold(0.0, f32::max);
    if peak < 0.5 {
        return None;
    }
    let scale = look.scale;
    let a = look.alpha;
    let x = (look.edge.loc.x - look.origin.0) as f32;
    let y = (look.edge.loc.y - look.origin.1) as f32;
    let w = look.edge.size.w as f32;
    // A column's width as drawn: the cap stretched to its edge.
    let col = COL * w / (n as f32 * COL).max(1.0);
    let px = |i: usize| x + i as f32 * col + col / 2.0;
    let p = |x: f32, y: f32| [x * scale, y * scale];
    let mut over = Mesh::default();
    // The shadow on the edge, under the snow.
    let half = [
        ((w - 2.0 * SHADOW_INSET) / 2.0).max(0.0) * scale,
        SHADOW_DEPTH / 2.0 * scale,
    ];
    let sigma = SHADOW_BLUR * scale;
    over.square(
        p(x + w / 2.0, y + SHADOW_DEPTH / 2.0),
        [half[0] + 3.0 * sigma, half[1] + 3.0 * sigma],
        [half[0] + 3.0 * sigma, half[1] + 3.0 * sigma],
        premul(rgb(0x080C16), 0.35 * a),
        Shape::Box,
        [half[0], half[1], sigma],
    );
    // The snow's outline from the left: a rounded corner up from the
    // edge, the curve through its columns, a corner down.
    let mut outline: Vec<(f32, f32)> = Vec::with_capacity(n * 4 + 16);
    let corner = |from: (f32, f32), control: (f32, f32), to: (f32, f32), out: &mut Vec<_>| {
        for k in 1..=6 {
            let t = k as f32 / 6.0;
            let u = 1.0 - t;
            out.push((
                u * u * from.0 + 2.0 * u * t * control.0 + t * t * to.0,
                u * u * from.1 + 2.0 * u * t * control.1 + t * t * to.1,
            ));
        }
    };
    let base = y + BASE;
    outline.push((x - 2.0, base));
    corner(
        (x - 2.0, base),
        (x - 2.0, y - s[0]),
        (px(0), y - s[0]),
        &mut outline,
    );
    let top_from = outline.len() - 1;
    let curve = curve(&s, px, y);
    outline.extend_from_slice(&curve);
    let top_to = outline.len();
    corner(
        (px(n - 1), y - s[n - 1]),
        (x + w + 2.0, y - s[n - 1]),
        (x + w + 2.0, base),
        &mut outline,
    );
    // Its body, white at its peak to blue at its base, a pixel's fringe
    // along its top.
    let stops = [
        (0.0, rgb(0xFFFFFF)),
        (0.55, rgb(0xEEF3FA)),
        (1.0, rgb(0xC6D2E4)),
    ];
    let (crest, foot) = ((y - peak) * scale, base * scale);
    let colour = |py: f32| premul(gradient(&stops, (py - crest) / (foot - crest)), a);
    let mid = crest + 0.55 * (foot - crest);
    let body: Vec<_> = outline
        .iter()
        .map(|&(ox, oy)| {
            let top = oy * scale;
            let inner = (top + 0.5).min(foot);
            let middle = mid.clamp(inner, foot);
            (
                ox * scale,
                [
                    (top - 0.5, CLEAR),
                    (inner, colour(inner)),
                    (middle, colour(middle)),
                    (foot, colour(foot)),
                ],
            )
        })
        .collect();
    over.columns(&body);
    // The highlight: a 1.2 px line along the curve, 0.7 px below it.
    let white = premul([1.0; 3], 0.9 * a);
    let line: Vec<_> = outline[top_from..top_to]
        .iter()
        .map(|&(lx, ly)| {
            let (t, b) = ((ly + 0.1) * scale, (ly + 1.3) * scale);
            (
                lx * scale,
                [
                    (t - 0.5, CLEAR),
                    (t + 0.5, white),
                    ((b - 0.5).max(t + 0.5), white),
                    (b + 0.5, CLEAR),
                ],
            )
        })
        .collect();
    over.columns(&line);
    Some(over)
}

/// The glints of `cap` as `look` says, added to what is under them: a
/// twinkling cross on a column in seven, where the snow lies deep enough.
#[expect(
    clippy::indexing_slicing,
    reason = "i is a column of the cap, under its count"
)]
pub fn glints(cap: &Cap, look: &CapLook) -> Mesh {
    let n = cap.h.len();
    let scale = look.scale;
    let a = look.alpha;
    let x = (look.edge.loc.x - look.origin.0) as f32;
    let y = (look.edge.loc.y - look.origin.1) as f32;
    let w = look.edge.size.w as f32;
    let col = COL * w / (n as f32 * COL).max(1.0);
    let p = |x: f32, y: f32| [x * scale, y * scale];
    let mut added = Mesh::default();
    if let Some(time) = look.glints {
        for i in (3..n.saturating_sub(3)).step_by(7) {
            let tw = (time * 2.3 + i as f32 * 12.9898).sin().max(0.0).powi(24);
            if tw < 0.05 || cap.h[i] < 3.0 {
                continue;
            }
            let at = p(x + i as f32 * col, y - cap.h[i] + 2.5);
            for half in [[0.6, 3.0], [3.0, 0.6]] {
                let half = [half[0] * scale, half[1] * scale];
                let reach = [half[0] + 1.0, half[1] + 1.0];
                added.square(
                    at,
                    reach,
                    reach,
                    [tw * a; 4],
                    Shape::Box,
                    [half[0], half[1], 0.0],
                );
            }
        }
    }
    added
}

/// A cap's columns drawn again only as they move more than this, logical
/// pixels (#815).
pub const UNSEEN: f32 = 0.05;

/// Where a cap drawn as `look` changes as its columns go from `old` to
/// `new` (as many, four or more): from and to (physical, from the look's
/// origin), nothing when it does not; `None`: all of it (its peak, which
/// colours all of it, moved; its columns are others).
pub fn cap_changes(old: &[f32], new: &[f32], look: &CapLook) -> Option<Option<[f32; 2]>> {
    let n = new.len();
    if old.len() != n || n < 4 {
        return None;
    }
    let (so, sn) = (smoothed(old), smoothed(new));
    // Less than this is less than a step of colour, or of an edge's cover.
    const LEAST: f32 = 0.001;
    let peak = |s: &[f32]| s.iter().copied().fold(0.0, f32::max);
    if (peak(&so) - peak(&sn)).abs() > LEAST {
        return None;
    }
    let differ = |i: &usize| match (so.get(*i), sn.get(*i)) {
        (Some(a), Some(b)) => (a - b).abs() > LEAST,
        _ => true,
    };
    let (Some(lo), Some(hi)) = ((0..n).find(differ), (0..n).rev().find(differ)) else {
        return Some(None);
    };
    // A column's curve reaches the middles of its neighbours; the end
    // columns their rounded corners, 2 px out.
    let x = (look.edge.loc.x - look.origin.0) as f32;
    let w = look.edge.size.w as f32;
    let col = w / n as f32;
    let from = x + lo.saturating_sub(1) as f32 * col - 3.0;
    let to = x + (hi + 2).min(n) as f32 * col + 3.0;
    Some(Some([from * look.scale, to * look.scale]))
}

/// The board's 9-tap blur of the columns (four or more), its ends
/// thinner.
#[expect(
    clippy::indexing_slicing,
    reason = "indices clamped to the columns, of which there are four or more"
)]
fn smoothed(h: &[f32]) -> Vec<f32> {
    let n = h.len();
    let mut s: Vec<f32> = (0..n)
        .map(|i| {
            BLUR.iter()
                .enumerate()
                .map(|(k, w)| h[(i + k).saturating_sub(4).min(n - 1)] * w)
                .sum::<f32>()
                / 48.0
        })
        .collect();
    s[0] *= 0.35;
    s[n - 1] *= 0.35;
    s[1] *= 0.75;
    s[n - 2] *= 0.75;
    s
}

/// The board's curve through the columns: quadratic from each column to
/// the middle of the next, each sampled four times; from the first
/// column's top to the last's.
#[expect(
    clippy::indexing_slicing,
    reason = "i - 1, i and i + 1 are columns, of which there are four or more"
)]
fn curve(s: &[f32], px: impl Fn(usize) -> f32, y: f32) -> Vec<(f32, f32)> {
    let n = s.len();
    let mut out = Vec::with_capacity(n * 4);
    let mut from = (px(0), y - s[0]);
    for i in 1..n - 1 {
        let control = (px(i), y - s[i]);
        let to = ((px(i) + px(i + 1)) / 2.0, y - (s[i] + s[i + 1]) / 2.0);
        for k in 1..=4 {
            let t = k as f32 / 4.0;
            let u = 1.0 - t;
            out.push((
                u * u * from.0 + 2.0 * u * t * control.0 + t * t * to.0,
                u * u * from.1 + 2.0 * u * t * control.1 + t * t * to.1,
            ));
        }
        from = to;
    }
    out.push((px(n - 1), y - s[n - 1]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blur_keeps_a_flat_cap_flat_but_its_ends() {
        let s = smoothed(&[10.0; 12]);
        assert!((s[5] - 10.0).abs() < 1e-4);
        assert!((s[0] - 3.5).abs() < 1e-4 && (s[1] - 7.5).abs() < 1e-4);
        let c = curve(&s, |i| i as f32 * 4.0 + 2.0, 100.0);
        // From the first column's top to the last's, left to right.
        assert!(c[0].0 > 2.0 && c[0].0 < 6.0, "{:?}", c[0]);
        assert!(c.windows(2).all(|w| w[1].0 >= w[0].0));
        // Over the flat middle, at its height.
        assert!(c.iter().any(|p| (p.1 - 90.0).abs() < 1e-3));
        assert_eq!(c.last(), Some(&(46.0, 100.0 - s[11])));
    }
}
