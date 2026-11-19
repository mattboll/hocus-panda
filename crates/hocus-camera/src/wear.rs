//! Visage (lot 4): an accessory that follows the head, one at a time:
//! Oreilles, Bonnet, Couronne, Lunettes.
//!
//! Each is a shape in three dimensions on the head ([`crate::rig`]: the
//! head's half width is 30 units, its middle between the temples), turned
//! by the head's yaw, pitch and roll and seen in perspective, so that it
//! turns with the head rather than lying flat on the picture:
//!
//! - **On the hair** (the ears, the beanie, the crown): the top of the
//!   head is where the person's mask ends above the forehead, along the
//!   face's axis (lot 3's segmentation; [`crate::sight::Sight::hair`]),
//!   else a typical head's; the beanie's fold sits on the forehead's top
//!   and its dome covers the hair to its top; the crown sits a little
//!   into the hair; the ears stand on it.
//! - **On the face** (the glasses): their lenses in front of the eyes,
//!   curving with the face; their arms go back towards the ears, fading
//!   into the hair, the far one hidden behind the head as it turns.
//!
//! Each is shaded as lit from above on the left (a soft gradient), with a
//! light rim on its edge and a soft contact shadow on the head under it;
//! the colours are the mockup's (the ears the panda's ink, `#23262E`).
//! Drawn apart in the head's box, then laid over the made frame: the same
//! on the GPU's path and the CPU's.

use std::f32::consts::{PI, TAU};

use hocus_link::camera::Wear;
use resvg::tiny_skia::{Path, Pixmap};

use crate::{
    face::Pose,
    look::{H, W},
    paint,
    rig::{self, Rig, fill, front_run, hull, linear, poly, radial, rgba, solid, stroke},
};

/// The camera's distance, head-local units: a face some 60 cm away.
pub const DISTANCE: f32 = 260.0;
/// The top of a head without its mask: this far above the forehead's top.
pub const TOP_ABOVE_FOREHEAD: f32 = 27.0;

type Ring = Vec<([f32; 3], [f32; 3])>;

/// Where on the head things go, head-local.
#[derive(Debug, Clone, Copy)]
struct Head {
    rig: Rig,
    /// The forehead's top (y, z), the hair's top (y), the eyes (half
    /// span, y, z), the skull's middle's depth.
    forehead: (f32, f32),
    top: f32,
    eyes: (f32, f32, f32),
    centre_z: f32,
}

impl Head {
    fn p(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
        self.rig.xy(x, y, z)
    }

    /// How much a direction from the head faces the camera, after the
    /// turn (positive: towards it).
    fn facing(&self, nx: f32, ny: f32, nz: f32) -> f32 {
        let (_, _, a) = self.rig.turn(nx, ny, nz);
        let (_, _, o) = self.rig.turn(0.0, 0.0, 0.0);
        a - o
    }
}

/// `wear` on the head at `pose`, over `frame` (B, G, R, x), at
/// `opacity`; `top` the hair's top (head-local y), if the mask found it.
pub fn draw(frame: &mut [u8], wear: Wear, pose: &Pose, top: Option<f32>, opacity: f32) {
    let rig =
        |origin: (f32, f32)| Rig::new(pose.yaw, pose.pitch, pose.roll, pose.unit, origin, DISTANCE);
    let forehead = pose.forehead_at;
    // The head placed so that what the accessory sits on (the eyes, the
    // forehead's top) is exactly where the face's points are: the turn's
    // small errors then move it about that point only.
    let (local, seen) = match wear {
        Wear::Glasses => ((0.0, pose.eyes_at.1, pose.eyes_at.2), pose.eyes),
        _ => ((0.0, forehead.0, forehead.1), pose.forehead),
    };
    let (px, py) = rig(pose.head).xy(local.0, local.1, local.2);
    let origin = (pose.head.0 + seen.0 - px, pose.head.1 + seen.1 - py);
    let full = rig(origin);
    let top = top
        .unwrap_or(forehead.0 - TOP_ABOVE_FOREHEAD)
        .min(forehead.0 - 8.0);
    // The head's box, from above its hair to under its eyes.
    let mut corners = Vec::new();
    for x in [-50.0f32, 50.0] {
        for y in [top - 26.0, pose.eyes_at.1 + 22.0] {
            for z in [30.0f32, -30.0] {
                corners.push(full.xy(x, y, z));
            }
        }
    }
    let x0 = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::MAX, f32::min)
        .max(0.0)
        .floor();
    let y0 = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::MAX, f32::min)
        .max(0.0)
        .floor();
    let x1 = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::MIN, f32::max)
        .min(W as f32)
        .ceil();
    let y1 = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::MIN, f32::max)
        .min(H as f32)
        .ceil();
    if x1 - x0 < 2.0 || y1 - y0 < 2.0 {
        return;
    }
    let Some(mut pixmap) = Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32) else {
        return;
    };
    let head = Head {
        rig: rig((origin.0 - x0, origin.1 - y0)),
        forehead,
        top,
        eyes: pose.eyes_at,
        centre_z: forehead.1 - 31.0,
    };
    match wear {
        Wear::Ears => ears(&mut pixmap, &head),
        Wear::Beanie => beanie(&mut pixmap, &head),
        Wear::Crown => crown(&mut pixmap, &head),
        Wear::Glasses => glasses(&mut pixmap, &head),
        // The panda takes the person's place: drawn by `crate::panda`.
        Wear::Panda => {}
    }
    paint::over(frame, &pixmap, x0 as i32, y0 as i32, opacity);
}

/// A ring round the head at height `y`: `n` points, its radii across and
/// in depth, its middle's depth; with each point's outward direction.
fn ring_at(y: f32, rx: f32, rz: f32, cz: f32, n: usize) -> Ring {
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * TAU;
            let (s, c) = a.sin_cos();
            ([rx * c, y, cz + rz * s], [c, 0.0, s])
        })
        .collect()
}

/// The band between two rings (`low` and `high`, the same angles), its
/// part facing the camera.
fn band(head: &Head, low: &Ring, high: &Ring) -> Option<Path> {
    let run = front_run(low.len(), |i| {
        low.get(i)
            .is_some_and(|(_, n)| head.facing(n[0], n[1], n[2]) > -0.05)
    });
    if run.len() < 2 {
        return None;
    }
    let mut pts: Vec<(f32, f32)> = run
        .iter()
        .filter_map(|&i| low.get(i))
        .map(|(p, _)| head.p(p[0], p[1], p[2]))
        .collect();
    pts.extend(
        run.iter()
            .rev()
            .filter_map(|&i| high.get(i))
            .map(|(p, _)| head.p(p[0], p[1], p[2])),
    );
    poly(&pts)
}

/// A soft contact shadow on the head: the front of the band from `y` to
/// `y + depth`, darkest at its top.
fn contact(pixmap: &mut Pixmap, head: &Head, y: f32, depth: f32, radii: (f32, f32), alpha: f32) {
    let (rx, rz) = radii;
    let cz = head.centre_z;
    let low = ring_at(y + depth, rx, rz, cz, 48);
    let high = ring_at(y, rx, rz, cz, 48);
    if let Some(path) = band(head, &low, &high) {
        let a = head.p(0.0, y, cz + rz);
        let b = head.p(0.0, y + depth, cz + rz);
        let shade = linear(
            a,
            b,
            &[
                (0.0, rgba(0x140C08, alpha)),
                (0.45, rgba(0x140C08, alpha * 0.4)),
                (1.0, rgba(0x140C08, 0.0)),
            ],
        );
        fill(pixmap, &path, &rig::paint(shade), None);
    }
}

/// A light rim along `path`'s edge, from `from`, inside it.
fn rim(pixmap: &mut Pixmap, path: &Path, from: (f32, f32), to: (f32, f32), width: f32, alpha: f32) {
    let shine = linear(
        from,
        to,
        &[
            (0.0, rgba(0xFFFFFF, alpha)),
            (0.45, rgba(0xFFFFFF, 0.0)),
            (1.0, rgba(0xFFFFFF, 0.0)),
        ],
    );
    let clip = rig::mask(pixmap, path);
    stroke(pixmap, path, &rig::paint(shine), width, clip.as_ref());
}

/// A disc of `r` pixels at (`x`, `y`), lit from its upper left.
fn ball(pixmap: &mut Pixmap, (x, y): (f32, f32), r: f32, light: u32, mid: u32, dark: u32) {
    if let Some(disc) = rig::ellipse(x, y, r, r, 0.0) {
        let shade = radial(
            (x - r * 0.38, y - r * 0.42),
            (x, y),
            r * 1.1,
            &[
                (0.0, rgba(light, 1.0)),
                (0.5, rgba(mid, 1.0)),
                (1.0, rgba(dark, 1.0)),
            ],
        );
        fill(pixmap, &disc, &rig::paint(shade), None);
    }
}

/// The beanie: its fold on the forehead's top, its dome over the hair,
/// its pompom; knitted ribs on the fold.
fn beanie(pixmap: &mut Pixmap, head: &Head) {
    let k = head.rig.k;
    let cz = head.centre_z;
    let (fy, fz) = head.forehead;
    let (rx, rz) = (37.0, (fz - cz) + 1.5);
    let brim = fy + 5.0;
    let fold = brim - 9.0;
    let apex = head.top - 3.0;
    let h = (fold - apex).max(6.0);
    contact(pixmap, head, brim, 6.0, (rx, rz), 0.38);
    // The dome: an ellipsoid's cap from the fold to above the hair.
    let mut cap = Vec::new();
    for j in 0..=9 {
        let (s, c) = (j as f32 / 9.0 * PI / 2.0).sin_cos();
        for i in 0..36 {
            let a = i as f32 / 36.0 * TAU;
            cap.push(head.p(
                rx * 0.97 * c * a.cos(),
                fold - h * s,
                cz + rz * 0.97 * c * a.sin(),
            ));
        }
    }
    let Some(dome) = poly(&hull(&cap)) else {
        return;
    };
    let lit = head.p(-12.0, fold - h * 0.75, cz + rz * 0.6);
    let mid = head.p(0.0, fold - h * 0.45, cz + rz * 0.5);
    let shade = radial(
        lit,
        mid,
        rx * k * 1.25,
        &[
            (0.0, rgba(0xEF7A70, 1.0)),
            (0.45, rgba(0xD9534F, 1.0)),
            (1.0, rgba(0x9E3330, 1.0)),
        ],
    );
    fill(pixmap, &dome, &rig::paint(shade), None);
    rim(
        pixmap,
        &dome,
        head.p(-rx, apex, cz),
        head.p(rx, fold, cz),
        2.4 * k,
        0.45,
    );
    // The fold, its ribs.
    let n = 72;
    let low = ring_at(brim, rx, rz, cz, n);
    let high = ring_at(fold, rx * 1.01, rz * 1.01, cz, n);
    if let Some(path) = band(head, &low, &high) {
        let a = head.p(-rx, fold, cz + rz * 0.3);
        let b = head.p(rx, brim, cz + rz * 0.3);
        let shade = linear(
            a,
            b,
            &[
                (0.0, rgba(0xC84D48, 1.0)),
                (0.5, rgba(0xB33F3B, 1.0)),
                (1.0, rgba(0x8A2C29, 1.0)),
            ],
        );
        fill(pixmap, &path, &rig::paint(shade), None);
        let clip = rig::mask(pixmap, &path);
        for i in (0..n).step_by(2) {
            let (Some((p, m)), Some((q, _))) = (low.get(i), high.get(i)) else {
                continue;
            };
            if head.facing(m[0], m[1], m[2]) < 0.05 {
                continue;
            }
            if let Some(rib) = rig::line(&[head.p(p[0], p[1], p[2]), head.p(q[0], q[1], q[2])]) {
                stroke(
                    pixmap,
                    &rib,
                    &solid(rgba(0x5A1816, 0.28)),
                    0.55 * k,
                    clip.as_ref(),
                );
            }
        }
        // Its upper edge catches the light.
        let edge: Vec<(f32, f32)> = front_run(n, |i| {
            high.get(i)
                .is_some_and(|(_, m)| head.facing(m[0], m[1], m[2]) > 0.0)
        })
        .iter()
        .filter_map(|&i| high.get(i))
        .map(|(p, _)| head.p(p[0], p[1], p[2]))
        .collect();
        if let Some(edge) = rig::line(&edge) {
            stroke(
                pixmap,
                &edge,
                &solid(rgba(0xFFD0C8, 0.35)),
                0.8 * k,
                clip.as_ref(),
            );
        }
    }
    // The pompom, on top: its wool a soft, broken edge.
    let (px, py) = head.p(0.0, apex - 4.0, cz + 2.0);
    let r = 6.5 * k;
    for i in 0..14 {
        let (s, c) = (i as f32 / 14.0 * TAU).sin_cos();
        if let Some(tuft) =
            rig::ellipse(px + c * r * 0.9, py + s * r * 0.9, r * 0.24, r * 0.24, 0.0)
        {
            fill(pixmap, &tuft, &solid(rgba(0xDCD5C8, 1.0)), None);
        }
    }
    ball(pixmap, (px, py), r, 0xFFFFFF, 0xF1ECE2, 0xBDB6A8);
}

/// The crown: a gold ring a little into the hair, five points, its inner
/// side seen between them, jewels on its front.
fn crown(pixmap: &mut Pixmap, head: &Head) {
    let k = head.rig.k;
    let cz = head.centre_z + 3.0;
    let (rx, rz) = (23.0, 22.0);
    let base = head.top + 14.0;
    let n = 100;
    let height = |a: f32| {
        // Five points, one in front (a = π/2).
        let u = ((a - PI / 2.0) / (TAU / 5.0)).rem_euclid(1.0);
        let tri = 1.0 - (u * 2.0 - 1.0).abs();
        8.0 + 8.5 * tri.powf(1.6)
    };
    contact(pixmap, head, base - 1.0, 5.0, (rx + 1.0, rz + 1.0), 0.32);
    let at = |a: f32, y: f32| {
        let (s, c) = a.sin_cos();
        head.p(rx * c, y, cz + rz * s)
    };
    let angle = |i: usize| i as f32 / n as f32 * TAU;
    for front in [false, true] {
        let run = front_run(n, |i| {
            let (s, c) = angle(i).sin_cos();
            (head.facing(c, 0.0, s) > 0.0) == front
        });
        if run.len() < 2 {
            continue;
        }
        let mut outline: Vec<(f32, f32)> = run.iter().map(|&i| at(angle(i), base)).collect();
        outline.extend(
            run.iter()
                .rev()
                .map(|&i| at(angle(i), base - height(angle(i)))),
        );
        let Some(path) = poly(&outline) else {
            continue;
        };
        let (l, r) = (at(PI, base), at(0.0, base));
        let shade = if front {
            linear(
                l,
                r,
                &[
                    (0.0, rgba(0xB8860B, 1.0)),
                    (0.3, rgba(0xFBE08A, 1.0)),
                    (0.55, rgba(0xF5C542, 1.0)),
                    (1.0, rgba(0xA87A12, 1.0)),
                ],
            )
        } else {
            linear(
                l,
                r,
                &[(0.0, rgba(0x6E4E0C, 1.0)), (1.0, rgba(0x9A7116, 1.0))],
            )
        };
        fill(pixmap, &path, &rig::paint(shade), None);
        stroke(pixmap, &path, &solid(rgba(0xC9971E, 1.0)), 0.6 * k, None);
        if !front {
            continue;
        }
        rim(
            pixmap,
            &path,
            at(PI * 0.75, base - 16.0),
            at(PI / 4.0, base),
            1.6 * k,
            0.55,
        );
        // Its band, the jewels on its front, a gold ball on each point.
        let band_pts: Vec<(f32, f32)> = run.iter().map(|&i| at(angle(i), base - 3.2)).collect();
        if let Some(line) = rig::line(&band_pts) {
            stroke(pixmap, &line, &solid(rgba(0x9C7012, 0.8)), 0.7 * k, None);
        }
        for (a, colour) in [
            (PI / 2.0, 0xD9534F),
            (PI / 2.0 - 0.75, 0x8FB3FF),
            (PI / 2.0 + 0.75, 0x7BC47F),
        ] {
            let (s, c) = a.sin_cos();
            if head.facing(c, 0.0, s) < 0.15 {
                continue;
            }
            let gem = at(a, base - 6.0);
            ball(pixmap, gem, 2.0 * k, 0xFFFFFF, colour, colour);
            if let Some(edge) = rig::ellipse(gem.0, gem.1, 2.0 * k, 2.0 * k, 0.0) {
                stroke(pixmap, &edge, &solid(rgba(0x8A6410, 1.0)), 0.5 * k, None);
            }
        }
        for j in 0..5 {
            let a = PI / 2.0 + j as f32 * TAU / 5.0;
            let (s, c) = a.sin_cos();
            if head.facing(c, 0.0, s) < 0.0 {
                continue;
            }
            ball(
                pixmap,
                at(a, base - height(a) - 1.0),
                1.5 * k,
                0xFFF4C8,
                0xF5C542,
                0xC9971E,
            );
        }
    }
}

/// The panda's ears on the hair: the ink's discs, a soft highlight, the
/// far one first.
fn ears(pixmap: &mut Pixmap, head: &Head) {
    let k = head.rig.k;
    let cz = head.centre_z + 4.0;
    let y = head.top + 6.0;
    let r = 12.5;
    let mut sides = [-1.0f32, 1.0];
    sides.sort_by(|a, b| {
        head.facing(*a, 0.0, 0.0)
            .total_cmp(&head.facing(*b, 0.0, 0.0))
    });
    let roll = head.rig.sr.atan2(head.rig.cr);
    for s in sides {
        let cx = s * 22.0;
        // Its base's shadow on the hair.
        let (bx, by) = head.p(cx, y + r * 0.8, cz);
        if let Some(sh) = rig::ellipse(bx, by, r * 0.9 * k, r * 0.35 * k, roll) {
            let shade = radial(
                (bx, by),
                (bx, by),
                r * 0.9 * k,
                &[(0.0, rgba(0x0C0D10, 0.42)), (1.0, rgba(0x0C0D10, 0.0))],
            );
            fill(pixmap, &sh, &rig::paint(shade), None);
        }
        // The disc, tilted a little outwards.
        let tilt = s * 0.35;
        let outline: Vec<(f32, f32)> = (0..48)
            .map(|i| {
                let (sa, ca) = (i as f32 / 48.0 * TAU).sin_cos();
                let (u, v) = (r * ca, r * sa);
                head.p(cx + u * tilt.cos(), y + v, cz + u * tilt.sin())
            })
            .collect();
        let Some(disc) = poly(&outline) else {
            continue;
        };
        let (ex, ey) = head.p(cx, y, cz);
        let shade = radial(
            (ex - r * 0.35 * k, ey - r * 0.45 * k),
            (ex, ey),
            r * 1.2 * k,
            &[
                (0.0, rgba(0x4A505E, 1.0)),
                (0.45, rgba(0x23262E, 1.0)),
                (1.0, rgba(0x121419, 1.0)),
            ],
        );
        fill(pixmap, &disc, &rig::paint(shade), None);
        let clip = rig::mask(pixmap, &disc);
        // The inner ear, and the soft highlight.
        if let Some(inner) = rig::ellipse(
            ex + s * 1.2 * k,
            ey + 1.5 * k,
            r * 0.5 * k,
            r * 0.45 * k,
            roll,
        ) {
            fill(pixmap, &inner, &solid(rgba(0x2E323C, 0.75)), clip.as_ref());
        }
        let spot = (ex - r * 0.4 * k, ey - r * 0.5 * k);
        let shine = radial(
            spot,
            spot,
            r * 0.55 * k,
            &[(0.0, rgba(0xFFFFFF, 0.22)), (1.0, rgba(0xFFFFFF, 0.0))],
        );
        fill(pixmap, &disc, &rig::paint(shine), clip.as_ref());
        rim(
            pixmap,
            &disc,
            (ex - r * k, ey - r * k),
            (ex + r * k, ey + r * k),
            1.6 * k,
            0.3,
        );
    }
}

/// A rounded rectangle's outline in head-local x, y (`n` points a
/// corner), one way round or the other.
fn rounded(c: (f32, f32), w: f32, h: f32, r: f32, back: bool) -> Vec<(f32, f32)> {
    let (cx, cy) = c;
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let corners = [
        (cx + w / 2.0 - r, cy - h / 2.0 + r, -PI / 2.0),
        (cx + w / 2.0 - r, cy + h / 2.0 - r, 0.0),
        (cx - w / 2.0 + r, cy + h / 2.0 - r, PI / 2.0),
        (cx - w / 2.0 + r, cy - h / 2.0 + r, PI),
    ];
    let mut out: Vec<(f32, f32)> = corners
        .iter()
        .flat_map(|&(x, y, a0)| {
            (0..=8).map(move |i| {
                let a = a0 + i as f32 / 8.0 * PI / 2.0;
                (x + r * a.cos(), y + r * a.sin())
            })
        })
        .collect();
    if back {
        out.reverse();
    }
    out
}

/// The glasses: two lenses in front of the eyes, curving with the face,
/// thick frames with a light edge on top, their soft shadow on the face,
/// the arms going back into the hair on the side that shows.
fn glasses(pixmap: &mut Pixmap, head: &Head) {
    let k = head.rig.k;
    let (e, ey, ez) = head.eyes;
    let plane = ez + 4.0;
    let depth = |x: f32| plane - 0.0045 * x * x;
    let at = |x: f32, y: f32| head.p(x, y, depth(x));
    // The mockup's proportions, of the eyes' half span.
    let (lens_x, lens_w, lens_h, radius, t) = (1.19 * e, 1.88 * e, 1.38 * e, 0.5 * e, 0.3 * e);
    let lens_y = ey - 0.06 * e;
    // Their shadow on the face, below them.
    for (width, alpha, drop) in [(2.4 * t, 0.07, 2.2), (1.3 * t, 0.12, 1.4)] {
        for s in [-1.0f32, 1.0] {
            let pts: Vec<(f32, f32)> =
                rounded((s * lens_x, lens_y + drop), lens_w, lens_h, radius, false)
                    .into_iter()
                    .map(|(x, y)| head.p(x, y, depth(x) - 2.0))
                    .collect();
            if let Some(path) = poly(&pts) {
                stroke(
                    pixmap,
                    &path,
                    &solid(rgba(0x000000, alpha)),
                    width * k,
                    None,
                );
            }
        }
    }
    // The arms, from the hinges back to the ears, on the side that shows,
    // fading into the hair.
    for s in [-1.0f32, 1.0] {
        if head.facing(s, 0.0, 0.2) < -0.1 {
            continue;
        }
        let hinge = (
            s * (lens_x + lens_w / 2.0 + t * 0.3),
            lens_y - 0.25 * lens_h,
        );
        let a = head.p(hinge.0, hinge.1, depth(hinge.0));
        let b = head.p(s * 31.0, ey - 1.0, head.centre_z + 6.0);
        if let Some(arm) = rig::line(&[a, b]) {
            let fade = linear(
                a,
                b,
                &[
                    (0.0, rgba(0x1A1C22, 1.0)),
                    (0.55, rgba(0x1A1C22, 0.8)),
                    (1.0, rgba(0x1A1C22, 0.0)),
                ],
            );
            stroke(pixmap, &arm, &rig::paint(fade), 0.75 * t * k, None);
        }
    }
    for s in [-1.0f32, 1.0] {
        let c = (s * lens_x, lens_y);
        let inner: Vec<(f32, f32)> = rounded(c, lens_w - t, lens_h - t, radius - t / 2.0, false)
            .into_iter()
            .map(|(x, y)| at(x, y))
            .collect();
        let Some(lens) = poly(&inner) else {
            continue;
        };
        // The lens: a light tint and a reflection.
        fill(pixmap, &lens, &solid(rgba(0x8FB3FF, 0.14)), None);
        let a = at(c.0 - lens_w * 0.5, lens_y - lens_h * 0.5);
        let b = at(c.0 + lens_w * 0.2, lens_y + lens_h * 0.5);
        let streak = linear(
            a,
            b,
            &[
                (0.0, rgba(0xFFFFFF, 0.0)),
                (0.34, rgba(0xFFFFFF, 0.0)),
                (0.42, rgba(0xFFFFFF, 0.28)),
                (0.5, rgba(0xFFFFFF, 0.04)),
                (0.58, rgba(0xFFFFFF, 0.14)),
                (0.64, rgba(0xFFFFFF, 0.0)),
                (1.0, rgba(0xFFFFFF, 0.0)),
            ],
        );
        let clip = rig::mask(pixmap, &lens);
        fill(pixmap, &lens, &rig::paint(streak), clip.as_ref());
        // The frame: the ring between the outer and the inner outline.
        let mut ring: Vec<(f32, f32)> = rounded(c, lens_w + t, lens_h + t, radius + t / 2.0, false)
            .into_iter()
            .map(|(x, y)| at(x, y))
            .collect();
        let hole: Vec<(f32, f32)> = rounded(c, lens_w - t, lens_h - t, radius - t / 2.0, true)
            .into_iter()
            .map(|(x, y)| at(x, y))
            .collect();
        if let Some(&first) = ring.first() {
            ring.push(first);
        }
        ring.extend(hole.iter().copied());
        if let Some(&h0) = hole.first() {
            ring.push(h0);
        }
        let Some(frame) = poly(&ring) else {
            continue;
        };
        let shade = linear(
            at(c.0, lens_y - lens_h / 2.0 - t),
            at(c.0, lens_y + lens_h / 2.0 + t),
            &[
                (0.0, rgba(0x464B57, 1.0)),
                (0.3, rgba(0x1E2027, 1.0)),
                (1.0, rgba(0x0C0D11, 1.0)),
            ],
        );
        fill(pixmap, &frame, &rig::paint(shade), None);
        // Its top edge catches the light.
        let edge: Vec<(f32, f32)> = (0..=12)
            .map(|i| {
                let x = c.0 - lens_w * 0.42 + lens_w * 0.84 * i as f32 / 12.0;
                at(x, lens_y - lens_h / 2.0 - t * 0.3)
            })
            .collect();
        if let Some(edge) = rig::line(&edge) {
            stroke(
                pixmap,
                &edge,
                &solid(rgba(0xFFFFFF, 0.3)),
                0.25 * t * k,
                None,
            );
        }
    }
    // The bridge.
    let inner_x = lens_x - lens_w / 2.0;
    let bridge: Vec<(f32, f32)> = (0..=10)
        .map(|i| {
            let u = i as f32 / 10.0;
            let x = -inner_x + 2.0 * inner_x * u;
            let y = lens_y - 0.18 * lens_h - 0.12 * e * (1.0 - (2.0 * u - 1.0).powi(2));
            at(x, y)
        })
        .collect();
    if let Some(bridge) = rig::line(&bridge) {
        stroke(
            pixmap,
            &bridge,
            &solid(rgba(0x1A1C22, 1.0)),
            0.7 * t * k,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{face::FaceTracker, look::BYTES, track::Frame};

    /// Each accessory changes the pixels around the head, where it goes
    /// (above the eyes, or on them for the glasses), and none elsewhere:
    /// not the frame's corners, not the shoulders.
    #[test]
    fn an_accessory_changes_the_head_and_nothing_else() -> anyhow::Result<()> {
        let picture = crate::face::test_face();
        let mut tracker = FaceTracker::new()?;
        let frame = Frame {
            pixels: &picture,
            width: W,
            height: H,
            order: [0, 1, 2],
        };
        let face = tracker.track(&frame, 1.0 / 30.0, false)?.expect("a face");
        let pose = face.pose();
        let bgrx: Vec<u8> = picture
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [b, g, r, 255])
            .collect();
        assert_eq!(bgrx.len(), BYTES);
        let (ex, ey) = pose.eyes;
        for wear in [Wear::Ears, Wear::Beanie, Wear::Crown, Wear::Glasses] {
            let mut drawn = bgrx.clone();
            draw(&mut drawn, wear, &pose, None, 1.0);
            let changed: Vec<(usize, usize)> = drawn
                .as_chunks::<4>()
                .0
                .iter()
                .zip(bgrx.as_chunks::<4>().0)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, _)| (i % W, i / W))
                .collect();
            assert!(changed.len() > 2_000, "{wear:?}: {} pixels", changed.len());
            // All of it within the head's reach: three spans aside, four
            // above, one below the eyes.
            let reach = pose.span;
            for &(x, y) in &changed {
                let (dx, dy) = (x as f32 - ex, y as f32 - ey);
                assert!(
                    dx.abs() < 3.0 * reach && dy > -4.0 * reach && dy < reach,
                    "{wear:?} at ({x}, {y}), the eyes at ({ex}, {ey})"
                );
            }
            // Above the eyes, but the glasses: on them.
            let mean_y = changed.iter().map(|&(_, y)| y as f32).sum::<f32>() / changed.len() as f32;
            if wear == Wear::Glasses {
                assert!((mean_y - ey).abs() < 0.3 * reach, "{wear:?}: {mean_y}");
            } else {
                assert!(mean_y < ey - 1.0 * reach, "{wear:?}: {mean_y}");
            }
        }
        Ok(())
    }
}
