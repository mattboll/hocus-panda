//! Visage › Panda (lot 4): a panda drawn in the person's place, the
//! approved live mockup (`hocus-panda-tools/mockups/s23-webcam/
//! panda-avatar`) ported: the bar's panda (its shapes and colours) on a
//! rig in two and a half dimensions.
//!
//! - **The rig** ([`Rig`]): each feature of the face on a sphere (its
//!   cheeks and muzzle a bump on it), turned by the head's yaw and pitch,
//!   seen in perspective, then rolled; the head bobs with its breath and
//!   leans, the body turns a little with the head.
//! - **What moves it** ([`Sim`]): the face landmarker's pose and
//!   expressions (blinks, the jaw, the mouth's round, the smile, the
//!   brows, wide eyes), followed through the mockup's springs (fixed
//!   steps of 1/120 s); the ears and the magician's hat lag on springs of
//!   their own. Without a face for [`IDLE_AFTER`], it idles: it breathes,
//!   sways, glances about and blinks slowly.
//! - **The three looks** ([`PandaLook`]): Classique (both ears ink),
//!   Magicien (the green magic ear, the top hat, the bow tie), Secrétaire
//!   (gold glasses, the pencil behind the ear, the clipboard, the bow tie).
//! - **The picture** ([`draw`]): over what is behind the person (Fond's
//!   wallpaper, else the room blurred without them), the background's
//!   glow behind the panda, its soft shadow, the scene's light on it.
//!   Drawn with tiny-skia on the CPU, the GPU's path and the CPU's alike
//!   (the frames' effects are drawn over the made frame).

use std::f32::consts::TAU;

use hocus_link::camera::PandaLook;
use resvg::tiny_skia::{
    BlendMode, Color, FillRule, Mask, Paint, Path, Pixmap, Shader, Stroke, Transform,
};

use crate::{
    face::{Pose, shape},
    look::{H, W},
    rig::{Cmd, lerp, linear, parse, poly, radial, rgba, sm, trace},
};

/// A face lost this long, the panda idles.
pub const IDLE_AFTER: f32 = 2.0;
/// The simulation's step.
const DT: f32 = 1.0 / 120.0;

const INK: u32 = 0x23262E;
const FUR: u32 = 0xF7F4EC;
const MAGIC: u32 = 0x7BC47F;
const HATTOP: u32 = 0x3A3D45;
const STITCH: u32 = 0x4A4E58;
const GOLD: u32 = 0xE8B44E;

/// The mockup's pose's numbers, by name.
mod at {
    pub const YAW: usize = 0;
    pub const PITCH: usize = 1;
    pub const ROLL: usize = 2;
    pub const GX: usize = 3;
    pub const GY: usize = 4;
    pub const LID_L: usize = 5;
    pub const LID_R: usize = 6;
    pub const HAPPY_L: usize = 7;
    pub const HAPPY_R: usize = 8;
    pub const WIDE: usize = 9;
    pub const BROW_L: usize = 10;
    pub const BROW_R: usize = 11;
    pub const JAW: usize = 12;
    pub const ROUND: usize = 13;
    pub const SMILE: usize = 14;
    pub const BLUSH: usize = 15;
    pub const BOB: usize = 16;
    pub const LEAN: usize = 17;
    pub const PAWS: usize = 18;
    pub const PERK: usize = 19;
    pub const LEVEL: usize = 20;
    pub const BREATH: usize = 21;
    pub const SQ: usize = 22;
    pub const N: usize = 23;
}

/// A pose: the mockup's `DEF`'s numbers.
pub type Params = [f32; at::N];

/// The mockup's springs: [stiffness, damping ratio]; the rest is set
/// directly.
const SPRINGS: [(usize, f32, f32); 18] = [
    (at::YAW, 70.0, 0.68),
    (at::PITCH, 120.0, 0.62),
    (at::ROLL, 60.0, 0.66),
    (at::GX, 900.0, 0.95),
    (at::GY, 900.0, 0.95),
    (at::HAPPY_L, 320.0, 0.8),
    (at::HAPPY_R, 320.0, 0.8),
    (at::WIDE, 420.0, 0.55),
    (at::BROW_L, 360.0, 0.45),
    (at::BROW_R, 360.0, 0.45),
    (at::JAW, 620.0, 0.72),
    (at::ROUND, 460.0, 0.8),
    (at::SMILE, 170.0, 0.85),
    (at::BLUSH, 26.0, 1.0),
    (at::BOB, 620.0, 0.42),
    (at::LEAN, 260.0, 0.5),
    (at::PAWS, 90.0, 0.6),
    (at::PERK, 240.0, 0.4),
];

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

fn hash(n: f32) -> f32 {
    let s = (n * 127.1 + 311.7).sin() * 43_758.547;
    s - s.floor()
}

/// The ears' and the hat's springs.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Secondary {
    pub ear_lx: f32,
    pub ear_ly: f32,
    pub ear_rx: f32,
    pub ear_ry: f32,
    pub hat_x: f32,
    pub hat_y: f32,
    pub hat_r: f32,
    pub hat_yaw: f32,
    pub hat_vy: f32,
}

/// The panda's pose, now, and the springs that bring it there.
#[derive(Debug, Clone)]
pub struct Sim {
    pub p: Params,
    v: Params,
    a: Params,
    pub sec: Secondary,
    sv: Secondary,
    prev: Option<(Anchors, Anchors)>,
    /// Its clock (s), and the time left to step.
    pub t: f32,
    pending: f32,
    /// Since when no face was seen (s of its clock).
    lost_since: Option<f32>,
}

/// What the face says, for the panda.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drive {
    pub pose: Pose,
    /// The 52 expressions (MediaPipe's order).
    pub shapes: [f32; 52],
}

impl Default for Sim {
    fn default() -> Self {
        let p = rest(0.0);
        Self {
            p,
            v: [0.0; at::N],
            a: [0.0; at::N],
            sec: Secondary::default(),
            sv: Secondary::default(),
            prev: None,
            t: 0.0,
            pending: 0.0,
            lost_since: Some(-IDLE_AFTER),
        }
    }
}

/// The mockup's resting pose at clock `t`: a slow sway, glances, a small
/// smile, the breath.
fn rest(t: f32) -> Params {
    let mut o = [0.0; at::N];
    o[at::YAW] = 0.06 * (0.55 * t + 0.3).sin() + 0.025 * (1.37 * t + 1.1).sin();
    o[at::PITCH] = 0.035 * (0.43 * t + 2.0).sin() + 0.015 * (1.9 * t).sin();
    o[at::ROLL] = 0.03 * (0.31 * t + 0.5).sin();
    let si = (t / 1.25).floor();
    o[at::GX] = (hash(si * 3.1) - 0.5) * 0.5;
    o[at::GY] = (hash(si * 7.7 + 1.0) - 0.5) * 0.35;
    o[at::SMILE] = 0.18;
    o[at::BREATH] = (TAU * t / 3.9).sin();
    o[at::BOB] = 0.5 * o[at::BREATH];
    o
}

/// Idle, a slow blink every few seconds (the mockup's blink: 70 ms to
/// close, 140 ms to open).
fn idle_blink(t: f32) -> f32 {
    let period = 4.3;
    let d = t.rem_euclid(period) - 0.5;
    if (0.0..0.07).contains(&d) {
        d / 0.07
    } else if (0.07..0.21).contains(&d) {
        1.0 - (d - 0.07) / 0.14
    } else {
        0.0
    }
}

fn get(shapes: &[f32; 52], i: usize) -> f32 {
    shapes.get(i).copied().unwrap_or(0.0)
}

/// The face's pose and expressions as the panda's targets. The panda is
/// seen as the person is (not mirrored): its left in the picture is the
/// person's right.
fn from_face(o: &mut Params, d: &Drive) {
    let s = |i: usize| get(&d.shapes, i);
    o[at::YAW] = d.pose.yaw.clamp(-0.8, 0.8);
    o[at::PITCH] = d.pose.pitch.clamp(-0.5, 0.5);
    o[at::ROLL] = d.pose.roll.clamp(-0.5, 0.5);
    // MediaPipe's left and right are the person's.
    o[at::LID_L] = clamp01((s(shape::EYE_BLINK_RIGHT) - 0.25) / 0.45);
    o[at::LID_R] = clamp01((s(shape::EYE_BLINK_LEFT) - 0.25) / 0.45);
    let smile = (s(shape::MOUTH_SMILE_LEFT) + s(shape::MOUTH_SMILE_RIGHT)) / 2.0;
    let frown = (s(shape::MOUTH_FROWN_LEFT) + s(shape::MOUTH_FROWN_RIGHT)) / 2.0;
    let jaw = clamp01((s(shape::JAW_OPEN) - 0.04) / 0.5);
    o[at::JAW] = jaw;
    o[at::ROUND] = clamp01((s(shape::MOUTH_FUNNEL).max(s(shape::MOUTH_PUCKER)) - 0.1) / 0.45);
    o[at::SMILE] = (0.12 + 0.95 * smile - 0.7 * frown).clamp(-0.4, 1.0);
    let laugh = clamp01((smile - 0.55) / 0.3);
    o[at::HAPPY_L] = laugh * clamp01((s(shape::EYE_SQUINT_RIGHT) + 0.2) * 1.4);
    o[at::HAPPY_R] = laugh * clamp01((s(shape::EYE_SQUINT_LEFT) + 0.2) * 1.4);
    o[at::BLUSH] = clamp01((smile - 0.6) / 0.3);
    o[at::PAWS] = clamp01((smile - 0.7) / 0.2) * clamp01((jaw - 0.3) / 0.25);
    let inner = s(shape::BROW_INNER_UP);
    o[at::BROW_L] = (0.6 * inner + 0.8 * s(shape::BROW_OUTER_UP_RIGHT) - s(shape::BROW_DOWN_RIGHT))
        .clamp(-0.5, 1.0);
    o[at::BROW_R] = (0.6 * inner + 0.8 * s(shape::BROW_OUTER_UP_LEFT) - s(shape::BROW_DOWN_LEFT))
        .clamp(-0.5, 1.0);
    o[at::WIDE] =
        clamp01(((s(shape::EYE_WIDE_LEFT) + s(shape::EYE_WIDE_RIGHT)) / 2.0 - 0.15) / 0.45);
    o[at::PERK] = o[at::WIDE] * 0.8;
    // Where the eyes look: right in the picture is the person's left.
    let right = (s(shape::EYE_LOOK_OUT_LEFT) + s(shape::EYE_LOOK_IN_RIGHT))
        - (s(shape::EYE_LOOK_IN_LEFT) + s(shape::EYE_LOOK_OUT_RIGHT));
    let down = (s(shape::EYE_LOOK_DOWN_LEFT) + s(shape::EYE_LOOK_DOWN_RIGHT))
        - (s(shape::EYE_LOOK_UP_LEFT) + s(shape::EYE_LOOK_UP_RIGHT));
    o[at::GX] = (right * 0.8).clamp(-1.0, 1.0);
    o[at::GY] = (down * 0.7).clamp(-1.0, 1.0);
    o[at::LEVEL] = jaw;
}

/// Where the anchors are, or how fast they go.
type Anchors = [(f32, f32); 3];

/// The anchors the ears and the hat hang from.
const ANCHORS: [(f32, f32, f32); 3] = [
    (-23.0, -23.0, -3.0),
    (25.0, -21.0, -3.0),
    (-4.0, -25.0, 3.0),
];

impl Sim {
    /// `dt` seconds on, moved by `drive` (a face seen in this frame) or
    /// idle.
    pub fn advance(&mut self, dt: f32, drive: Option<&Drive>) {
        self.pending = (self.pending + dt.clamp(0.0, 0.25)).min(0.25);
        match drive {
            Some(_) => self.lost_since = None,
            None => {
                self.lost_since.get_or_insert(self.t);
            }
        }
        while self.pending >= DT {
            self.pending -= DT;
            self.t += DT;
            self.step(drive);
        }
    }

    /// Whether it idles now (no face for a while).
    pub fn idle(&self) -> bool {
        self.lost_since.is_some_and(|l| self.t - l >= IDLE_AFTER)
    }

    fn step(&mut self, drive: Option<&Drive>) {
        let t = self.t;
        let mut tg = rest(t);
        if let Some(d) = drive.filter(|_| !self.idle()) {
            from_face(&mut tg, d);
            // Blinks drive the lids at once; a little smoothing for the
            // expressions' noise.
            let k = 0.55;
            self.p[at::LID_L] += (tg[at::LID_L] - self.p[at::LID_L]) * k;
            self.p[at::LID_R] += (tg[at::LID_R] - self.p[at::LID_R]) * k;
        } else {
            let lids = idle_blink(t);
            self.p[at::LID_L] = lids;
            self.p[at::LID_R] = lids;
        }
        for &(key, stiff, damp) in &SPRINGS {
            let c = 2.0 * damp * stiff.sqrt();
            let (Some(&target), Some(p), Some(v), Some(a)) = (
                tg.get(key),
                self.p.get(key).copied(),
                self.v.get_mut(key),
                self.a.get_mut(key),
            ) else {
                continue;
            };
            let acc = stiff * (target - p) - c * *v;
            *v += acc * DT;
            *a = acc;
            let v = *v;
            if let Some(p) = self.p.get_mut(key) {
                *p += v * DT;
            }
        }
        self.p[at::LEVEL] = tg[at::LEVEL];
        self.p[at::BREATH] = tg[at::BREATH];
        self.p[at::SQ] = (-self.v[at::BOB] * 0.004).clamp(-0.06, 0.06);
        // The anchors' accelerations shake the ears and the hat.
        let rig = Rig::new(&self.p);
        let now = ANCHORS.map(|(x, y, z)| {
            let (a, b, _) = rig.p(x, y, z);
            (a, b)
        });
        let (pos, vel) = self.prev.unwrap_or((now, [(0.0, 0.0); 3]));
        let mut acc = [(0.0f32, 0.0f32); 3];
        let mut new_vel = vel;
        for ((((a, nv), n), p), old) in acc.iter_mut().zip(&mut new_vel).zip(now).zip(pos).zip(vel)
        {
            let v = ((n.0 - p.0) / DT, (n.1 - p.1) / DT);
            *a = (
                ((v.0 - old.0) / DT).clamp(-4000.0, 4000.0),
                ((v.1 - old.1) / DT).clamp(-4000.0, 4000.0),
            );
            *nv = v;
        }
        if self.prev.is_none() {
            acc = [(0.0, 0.0); 3];
        }
        self.prev = Some((now, new_vel));
        let (s, v) = (&mut self.sec, &mut self.sv);
        let spring = |x: &mut f32, vx: &mut f32, stiff: f32, damp: f32, force: f32, lim: f32| {
            let c = 2.0 * damp * stiff.sqrt();
            *vx += (-stiff * *x - c * *vx + force) * DT;
            *x = (*x + *vx * DT).clamp(-lim, lim);
        };
        spring(
            &mut s.ear_lx,
            &mut v.ear_lx,
            230.0,
            0.28,
            -acc[0].0 * 0.9,
            4.0,
        );
        spring(
            &mut s.ear_ly,
            &mut v.ear_ly,
            230.0,
            0.28,
            -acc[0].1 * 0.9,
            4.0,
        );
        spring(
            &mut s.ear_rx,
            &mut v.ear_rx,
            210.0,
            0.26,
            -acc[1].0 * 0.9,
            4.0,
        );
        spring(
            &mut s.ear_ry,
            &mut v.ear_ry,
            210.0,
            0.26,
            -acc[1].1 * 0.9,
            4.0,
        );
        spring(&mut s.hat_x, &mut v.hat_x, 170.0, 0.3, -acc[2].0, 6.0);
        spring(&mut s.hat_y, &mut v.hat_y, 170.0, 0.3, -acc[2].1, 9.0);
        spring(
            &mut s.hat_r,
            &mut v.hat_r,
            120.0,
            0.28,
            -self.a[at::ROLL] * 0.9 - acc[2].0 * 0.012,
            0.5,
        );
        spring(
            &mut s.hat_yaw,
            &mut v.hat_yaw,
            90.0,
            0.35,
            -self.a[at::YAW] * 0.6,
            0.4,
        );
        s.hat_vy = v.hat_y;
    }
}

/// The mockup's rig: a head on a sphere, in the bar drawing's units (its
/// head the ellipse of radii 30 and 27 round the middle).
#[derive(Debug, Clone, Copy)]
pub struct Rig {
    cy: f32,
    sy: f32,
    cp: f32,
    sp: f32,
    cr: f32,
    sr: f32,
    lean: f32,
    sq_x: f32,
    sq_y: f32,
    pub hx: f32,
    pub hy: f32,
    /// The head's outline's radii, as it turns.
    pub a: f32,
    pub b: f32,
}

impl Rig {
    pub fn new(p: &Params) -> Self {
        let (sy, cy) = p[at::YAW].sin_cos();
        let (sp, cp) = p[at::PITCH].sin_cos();
        let (sr, cr) = p[at::ROLL].sin_cos();
        Self {
            cy,
            sy,
            cp,
            sp,
            cr,
            sr,
            lean: 1.0 + 0.035 * p[at::LEAN],
            sq_x: 1.0 - p[at::SQ] * 0.6,
            sq_y: 1.0 + p[at::SQ],
            hx: 26.0 * sr * 0.55 + sy * 2.2,
            hy: p[at::BOB] + p[at::LEAN] * 2.6 - p[at::PAWS] * 0.6,
            a: (900.0 * cy * cy + 625.0 * sy * sy).sqrt(),
            b: (729.0 * cp * cp + 625.0 * sp * sp).sqrt(),
        }
    }

    pub fn post(&self, x: f32, y: f32) -> (f32, f32) {
        let mut xx = x * self.cr - y * self.sr;
        let mut yy = x * self.sr + y * self.cr;
        xx *= self.sq_x * self.lean;
        yy = 26.0 + (yy - 26.0) * self.sq_y;
        yy *= self.lean;
        (self.hx + xx, self.hy + yy)
    }

    fn r3(&self, x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        let x1 = x * self.cy + z * self.sy;
        let z1 = -x * self.sy + z * self.cy;
        let y2 = y * self.cp - z1 * self.sp;
        let z2 = y * self.sp + z1 * self.cp;
        let f = 380.0 / (380.0 - z2);
        (x1 * f, y2 * f, z2)
    }

    pub fn p(&self, x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        let (a, b, z) = self.r3(x, y, z);
        let (px, py) = self.post(a, b);
        (px, py, z)
    }

    fn zf(x: f32, y: f32) -> f32 {
        let u = (x / 30.0).powi(2) + (y / 27.0).powi(2);
        let mut z = if u < 1.0 {
            25.0 * (1.0 - u).sqrt()
        } else {
            0.0
        };
        z += 4.5 * (-((x / 8.0).powi(2) + ((y - 11.0) / 7.0).powi(2))).exp();
        z
    }

    /// A point of the face's surface, and its depth.
    pub fn m3(&self, x: f32, y: f32) -> (f32, f32, f32) {
        self.p(x, y, Self::zf(x, y))
    }

    pub fn m(&self, x: f32, y: f32) -> (f32, f32) {
        let (a, b, _) = self.m3(x, y);
        (a, b)
    }
}

/// How the background lights the panda (the mockup's `SCENES`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    rim_l: Color,
    rim_r: Color,
    tint: Color,
    tint2: Color,
    shadow: Color,
    sx: f32,
    sy: f32,
    sb: f32,
    glow: Color,
    eye: Color,
}

impl Light {
    /// Under the aurora (and the desktop's other living wallpapers).
    pub fn aurora() -> Self {
        Self {
            rim_l: Color::from_rgba8(110, 240, 190, 217),
            rim_r: Color::from_rgba8(165, 135, 255, 217),
            tint: Color::from_rgba8(40, 70, 140, 26),
            tint2: Color::from_rgba8(80, 230, 170, 18),
            shadow: Color::from_rgba8(0, 4, 12, 128),
            sx: 0.0,
            sy: 0.01,
            sb: 0.08,
            glow: Color::from_rgba8(90, 230, 180, 56),
            eye: Color::from_rgba8(120, 240, 200, 230),
        }
    }

    /// In the room.
    pub fn room() -> Self {
        Self {
            rim_l: Color::from_rgba8(150, 185, 255, 191),
            rim_r: Color::from_rgba8(255, 190, 120, 230),
            tint: Color::from_rgba8(90, 60, 30, 20),
            tint2: Color::from_rgba8(255, 170, 90, 20),
            shadow: Color::from_rgba8(20, 10, 4, 128),
            sx: -0.025,
            sy: 0.02,
            sb: 0.06,
            glow: Color::from_rgba8(255, 200, 140, 26),
            eye: Color::from_rgba8(255, 215, 160, 230),
        }
    }
}

/// Where the panda stands in the picture: its middle's height (of the
/// picture's) and its units' size (of the picture's height).
fn frame_of(look: PandaLook) -> (f32, f32) {
    match look {
        PandaLook::Magician => (0.615, 1.0 / 104.0),
        PandaLook::Classic | PandaLook::Secretary => (0.53, 1.0 / 90.0),
    }
}

/// The drawing surface: a pixmap, the transform from the drawing's units.
struct Canvas {
    pixmap: Pixmap,
    ts: Transform,
    alpha: f32,
}

fn with_alpha(c: Color, alpha: f32) -> Color {
    let mut c = c;
    c.apply_opacity(alpha);
    c
}

fn col(hex: u32) -> Color {
    rgba(hex, 1.0)
}

impl Canvas {
    fn paint(&self, shader: Shader<'static>, blend: BlendMode) -> Paint<'static> {
        let mut shader = shader;
        if self.alpha < 1.0 {
            shader.apply_opacity(self.alpha);
        }
        Paint {
            shader,
            anti_alias: true,
            blend_mode: blend,
            ..Paint::default()
        }
    }

    fn fill(&mut self, path: &Path, shader: Shader<'static>, clip: Option<&Mask>) {
        let paint = self.paint(shader, BlendMode::SourceOver);
        self.pixmap
            .fill_path(path, &paint, FillRule::Winding, self.ts, clip);
    }

    fn fill_c(&mut self, path: &Path, c: Color, clip: Option<&Mask>) {
        self.fill(path, Shader::SolidColor(c), clip);
    }

    fn stroke_with(
        &mut self,
        path: &Path,
        shader: Shader<'static>,
        width: f32,
        clip: Option<&Mask>,
        blend: BlendMode,
    ) {
        let paint = self.paint(shader, blend);
        let stroke = Stroke {
            width,
            line_cap: resvg::tiny_skia::LineCap::Round,
            line_join: resvg::tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(path, &paint, &stroke, self.ts, clip);
    }

    fn stroke(&mut self, path: &Path, c: Color, width: f32) {
        self.stroke_with(
            path,
            Shader::SolidColor(c),
            width,
            None,
            BlendMode::SourceOver,
        );
    }

    fn mask(&self, path: &Path) -> Option<Mask> {
        let mut m = Mask::new(self.pixmap.width(), self.pixmap.height())?;
        m.fill_path(path, FillRule::Winding, true, self.ts);
        Some(m)
    }

    /// A transform's group: `local` on top of the current one.
    fn push(&mut self, local: Transform) -> Transform {
        let old = self.ts;
        self.ts = local.post_concat(old);
        old
    }
}

fn circle(cx: f32, cy: f32, r: f32) -> Option<Path> {
    crate::rig::ellipse(cx, cy, r, r, 0.0)
}

fn ell(cx: f32, cy: f32, rx: f32, ry: f32, rot: f32) -> Option<Path> {
    crate::rig::ellipse(cx, cy, rx, ry, rot)
}

fn fur_fill(cx: f32, cy: f32, r: f32) -> Shader<'static> {
    radial(
        (cx - r * 0.32, cy - r * 0.42),
        (cx, cy),
        r * 1.08,
        &[
            (0.0, col(0xFFFFFF)),
            (0.42, col(FUR)),
            (0.82, col(0xE4E1DA)),
            (1.0, col(0xC7CAD3)),
        ],
    )
}

fn ink_fill(cx: f32, cy: f32, r: f32) -> Shader<'static> {
    radial(
        (cx - r * 0.35, cy - r * 0.45),
        (cx, cy),
        r * 1.2,
        &[
            (0.0, col(0x4B515F)),
            (0.45, col(0x2A2E37)),
            (1.0, col(0x15171C)),
        ],
    )
}

/// The mockup's `rim`: the scene's two lights along an edge, inside it.
fn rim(
    c: &mut Canvas,
    path: &Path,
    (cx, cy, r): (f32, f32, f32),
    light: &Light,
    width: f32,
    screen: bool,
) {
    let clip = c.mask(path);
    let g = linear(
        (cx - r, cy - r * 0.3),
        (cx + r, cy + r * 0.3),
        &[
            (0.0, light.rim_l),
            (0.3, Color::TRANSPARENT),
            (0.7, Color::TRANSPARENT),
            (1.0, light.rim_r),
        ],
    );
    let old = c.alpha;
    c.alpha = old * if screen { 0.9 } else { 0.42 };
    let blend = if screen {
        BlendMode::Screen
    } else {
        BlendMode::SourceOver
    };
    c.stroke_with(path, g, width, clip.as_ref(), blend);
    c.alpha = old;
}

/// A dark part with the drawings' light halo.
fn dark_part(c: &mut Canvas, path: &Path, (cx, cy, r): (f32, f32, f32), light: &Light) {
    c.stroke(path, col(FUR), 4.0);
    c.fill(path, ink_fill(cx, cy, r), None);
    rim(c, path, (cx, cy, r), light, r * 0.3, true);
}

/// A soft glow (the mockup's `shadowBlur` round a shape): a radial
/// gradient of `colour` round (`x`, `y`).
fn glow(c: &mut Canvas, (x, y): (f32, f32), r: f32, colour: Color) {
    if let Some(disc) = circle(x, y, r) {
        let g = radial(
            (x, y),
            (x, y),
            r,
            &[(0.0, colour), (1.0, Color::TRANSPARENT)],
        );
        c.fill(&disc, g, None);
    }
}

struct Shapes {
    patch_l: Vec<Cmd>,
    patch_r: Vec<Cmd>,
    nose: Vec<Cmd>,
    hat_crown: Vec<Cmd>,
    hat_crease: Vec<Cmd>,
    hat_crease2: Vec<Cmd>,
    hat_band: Vec<Cmd>,
    hat_brim: Vec<Cmd>,
    spark: Vec<Cmd>,
    bow_l: Vec<Cmd>,
    bow_r: Vec<Cmd>,
}

fn shapes() -> &'static Shapes {
    static SHAPES: std::sync::OnceLock<Shapes> = std::sync::OnceLock::new();
    SHAPES.get_or_init(|| Shapes {
        patch_l: parse(
            "M39 47 C43 40 54 42 55.5 50 C56.5 57 50 62.5 44.5 61.5 C39.5 60.5 36.5 53 39 47 Z",
            60.0,
            54.0,
        ),
        patch_r: parse(
            "M81 47 C77 40 66 42 64.5 50 C63.5 57 70 62.5 75.5 61.5 C80.5 60.5 83.5 53 81 47 Z",
            60.0,
            54.0,
        ),
        nose: parse(
            "M56.5 62.4 Q60 60.4 63.5 62.4 Q62.3 65.5 60 66.1 Q57.7 65.5 56.5 62.4 Z",
            60.0,
            54.0,
        ),
        hat_crown: parse(
            "M-11.5 0.5 C-12.5 -8 -14.5 -16 -13.5 -24 Q0 -27 13.5 -24 C14.5 -16 12.5 -8 11.5 0.5 Q0 2 -11.5 0.5 Z",
            0.0,
            0.0,
        ),
        hat_crease: parse("M-9.6 -5 C-10.6 -11 -11.6 -16 -11.2 -21", 0.0, 0.0),
        hat_crease2: parse("M-4 -22.6 Q-1 -19.4 3 -22.2", 0.0, 0.0),
        hat_band: parse(
            "M-12.1 -2.6 Q0 -1 12.1 -2.6 L12.7 -8 Q0 -6.4 -12.7 -8 Z",
            0.0,
            0.0,
        ),
        hat_brim: parse(
            "M-23 -1.5 C-21.5 5 21.5 5 23 -1.5 C17 1.6 -17 1.6 -23 -1.5 Z",
            0.0,
            0.0,
        ),
        spark: parse(
            "M0 -5.4 L1.4 -1.4 L5.4 0 L1.4 1.4 L0 5.4 L-1.4 1.4 L-5.4 0 L-1.4 -1.4 Z",
            0.0,
            0.0,
        ),
        bow_l: parse("M0 0 L-9.5 -5 Q-11.4 0 -9.5 5 Z", 0.0, 0.0),
        bow_r: parse("M0 0 L9.5 -5 Q11.4 0 9.5 5 Z", 0.0, 0.0),
    })
}

fn plain(cmds: &[Cmd]) -> Option<Path> {
    trace(cmds, |x, y| (x, y))
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    crate::rig::rounded(x, y, w, h, 0.0)
}

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    crate::rig::rounded(x, y, w, h, r)
}

fn ring_pts(
    map: impl Fn(f32, f32) -> (f32, f32),
    cx: f32,
    cy: f32,
    r: f32,
    n: usize,
) -> Vec<(f32, f32)> {
    crate::rig::ring(map, (cx, cy), (r, r), n, 0.0)
}

/// The panda, posed as `p`, its springs `sec`, in `look`, lit by `light`,
/// at clock `time`, in the drawing's units on `c`.
fn draw_panda(
    c: &mut Canvas,
    p: &Params,
    sec: &Secondary,
    look: PandaLook,
    light: &Light,
    time: f32,
) {
    let r = Rig::new(p);
    let magician = look == PandaLook::Magician;
    let secretary = look == PandaLook::Secretary;
    let classic = look == PandaLook::Classic;
    let breath = p[at::BREATH];
    let body_yaw = p[at::YAW] * 0.28;
    let (bx, by) = (r.hx * 0.28, 37.0 + breath * 0.35);
    let bshift = body_yaw.sin() * 20.0;
    let roll = p[at::ROLL];

    // The pencil behind the left ear (the secretary).
    if secretary {
        let (ax, ay, _) = r.p(-50.0, -11.5, -8.0);
        let old = c.push(
            Transform::from_rotate(-41.0 + roll.to_degrees())
                .post_translate(ax + sec.ear_lx * 0.5, ay + sec.ear_ly * 0.5),
        );
        if let Some(tip) = crate::rig::poly(&[(0.0, 0.0), (7.5, -3.4), (7.5, 3.4)]) {
            c.fill_c(&tip, col(0xF3D9A8), None);
            c.stroke(&tip, col(INK), 1.6);
        }
        if let Some(lead) = crate::rig::poly(&[(0.0, 0.0), (2.6, -1.2), (2.6, 1.2)]) {
            c.fill_c(&lead, col(INK), None);
        }
        if let Some(body) = rect(7.5, -3.4, 31.0, 6.8) {
            c.fill_c(&body, col(0xF5C84B), None);
            c.stroke(&body, col(INK), 1.8);
        }
        if let Some(l) = crate::rig::line(&[(8.5, 0.0), (37.5, 0.0)]) {
            c.stroke(&l, col(0xE0A92E), 1.4);
        }
        if let Some(ferrule) = rect(38.5, -3.4, 4.6, 6.8) {
            c.fill_c(&ferrule, col(0xC9CDD4), None);
            c.stroke(&ferrule, col(INK), 1.6);
        }
        if let Some(eraser) = trace(
            &parse(
                "M43.1 -3.4 L46.5 -3.4 Q48.7 0 46.5 3.4 L43.1 3.4 Z",
                0.0,
                0.0,
            ),
            |x, y| (x, y),
        ) {
            c.fill_c(&eraser, col(0xF08FA0), None);
            c.stroke(&eraser, col(INK), 1.6);
        }
        c.ts = old;
    }

    // The ears, behind the head, lagging on springs.
    let perk = p[at::PERK];
    {
        let (ax, ay, _) = r.p(-23.0 - perk * 1.2, -23.0 - perk * 1.6, -3.0);
        let (x, y) = (ax + sec.ear_lx, ay + sec.ear_ly);
        let st = ((sec.ear_lx.hypot(sec.ear_ly)) * 0.03).clamp(0.0, 0.12);
        let ang = sec
            .ear_ly
            .atan2(if sec.ear_lx == 0.0 { 1e-6 } else { sec.ear_lx });
        let local = Transform::from_rotate(-ang.to_degrees())
            .post_scale(1.0 + st + perk * 0.06, 1.0 - st * 0.6 + perk * 0.06)
            .post_rotate(ang.to_degrees())
            .post_translate(x, y);
        let old = c.push(local);
        if let Some(ear) = circle(0.0, 0.0, 10.5) {
            dark_part(c, &ear, (0.0, 0.0, 10.5), light);
        }
        if let Some(inner) = circle(1.5, 1.5, 5.5) {
            c.fill_c(&inner, Color::from_rgba8(255, 255, 255, 13), None);
        }
        c.ts = old;
    }
    {
        let (ax, ay, _) = r.p(25.0 + perk * 1.2, -21.0 - perk * 1.6, -3.0);
        let (x, y) = (ax + sec.ear_rx, ay + sec.ear_ry);
        let local = Transform::from_scale(1.0 + perk * 0.08, 1.0 + perk * 0.05)
            .post_rotate(
                (38.0f32.to_radians() + roll + sec.ear_rx * 0.05 - perk * 0.25).to_degrees(),
            )
            .post_translate(x, y);
        let old = c.push(local);
        if let Some(ear) = ell(0.0, 0.0, 11.5, 7.5, 0.0) {
            if classic {
                dark_part(c, &ear, (0.0, 0.0, 10.5), light);
            } else {
                let pulse = 0.5 + 0.5 * (time * 2.2).sin();
                glow(
                    c,
                    (0.0, 0.0),
                    14.0 + 5.0 * pulse,
                    Color::from_rgba8(123, 196, 127, 150),
                );
                c.fill_c(&ear, col(MAGIC), None);
                c.stroke(&ear, col(INK), 2.5);
                let g = radial(
                    (-3.0, -2.5),
                    (0.0, 0.0),
                    12.0,
                    &[
                        (0.0, Color::from_rgba8(225, 255, 215, 217)),
                        (0.45, Color::from_rgba8(123, 196, 127, 0)),
                        (1.0, Color::from_rgba8(20, 60, 30, 89)),
                    ],
                );
                c.fill(&ear, g, None);
            }
        }
        c.ts = old;
    }

    // The body, turned a little with the head.
    if let Some(body) = ell(bx, by, 27.5 * (1.0 + 0.012 * breath), 22.5, roll * 0.12) {
        c.fill(&body, fur_fill(bx, by - 4.0, 30.0), None);
        // The head's shadow on it (its gradient fades within the body,
        // which holds it).
        let ao = radial(
            (r.hx, 22.0),
            (r.hx, 22.0),
            26.0,
            &[
                (0.0, Color::from_rgba8(30, 36, 60, 107)),
                (1.0, Color::from_rgba8(30, 36, 60, 0)),
            ],
        );
        c.fill(&body, ao, None);
        rim(c, &body, (bx, by, 28.0), light, 7.0, false);
        c.stroke(&body, col(INK), 2.2);
    }

    // The secretary's clipboard, or the arms at rest.
    if secretary {
        let old = c.push(
            Transform::from_rotate((roll * 0.1).to_degrees()).post_translate(bx + bshift * 0.6, by),
        );
        if let Some(board) = rrect(-22.0, -15.0, 44.0, 36.0, 4.0) {
            c.fill_c(&board, col(0xB98352), None);
            c.stroke(&board, col(INK), 2.0);
        }
        if let Some(paper) = rrect(-18.0, -10.0, 36.0, 27.0, 2.0) {
            c.fill_c(&paper, col(FUR), None);
        }
        if let Some(clip) = rrect(-8.0, -18.5, 16.0, 7.0, 2.0) {
            c.fill_c(&clip, col(0x9AA0AA), None);
            c.stroke(&clip, col(INK), 1.6);
        }
        if let Some(tick) = crate::rig::line(&[(-14.0, -3.0), (-11.5, -0.5), (-7.5, -5.5)]) {
            c.stroke(&tick, col(0x3FA66B), 2.0);
        }
        for (x, y, w, colour) in [
            (-5.0, -4.5, 18.0, 0x8A8E98),
            (-5.0, 3.8, 15.0, 0x8A8E98),
            (-14.0, 10.5, 24.0, 0xC2C6CE),
        ] {
            if let Some(l) = rrect(x, y, w, 2.4, 1.2) {
                c.fill_c(&l, col(colour), None);
            }
        }
        if let Some(dot) = circle(-11.0, 5.0, 2.2) {
            c.fill_c(&dot, col(0x8FB3FF), None);
        }
        for s in [-1.0f32, 1.0] {
            let py = 1.0 - p[at::PAWS] * 3.0 + (time * 40.0).sin() * p[at::PAWS] * 0.4;
            if let Some(paw) = ell(s * 21.5, py, 7.0, 8.5, s * -0.21) {
                dark_part(c, &paw, (s * 21.5, 1.0, 8.0), light);
            }
        }
        c.ts = old;
    } else {
        for s in [-1.0f32, 1.0] {
            let paws = p[at::PAWS];
            let shake = paws * (time * 38.0 + s).sin() * 0.5;
            let x = bx + bshift * 0.5 + s * lerp(15.0, 11.5, paws);
            let y = by + lerp(4.0, -2.5, paws) + shake;
            if let Some(paw) = ell(x, y, 7.0, 9.0, s * lerp(-32.0, -58.0, paws).to_radians()) {
                dark_part(c, &paw, (x, y, 8.5), light);
            }
        }
    }

    // The head.
    let smile = p[at::SMILE];
    let jaw = p[at::JAW];
    let al = r.a + smile.max(0.0) * 0.9 + jaw * 0.3;
    let bl = r.b + jaw * 2.6 + smile.max(0.0) * 0.4;
    let sil: Vec<(f32, f32)> = (0..64)
        .map(|i| {
            let a = i as f32 / 64.0 * TAU;
            let s = a.sin();
            let w = sm(0.0, 0.7, s);
            r.post(lerp(r.a, al, w) * a.cos(), lerp(r.b, bl, w) * s)
        })
        .collect();
    let Some(head) = poly(&sil) else {
        return;
    };
    let hc = r.post(0.0, 0.0);
    c.fill(&head, fur_fill(hc.0, hc.1, 31.0), None);
    let head_clip = c.mask(&head);
    let hm = head_clip.as_ref();
    if magician {
        let hb = radial(
            (hc.0 - 4.0, hc.1 - 30.0),
            (hc.0 - 4.0, hc.1 - 30.0),
            24.0,
            &[
                (0.0, Color::from_rgba8(30, 36, 60, 89)),
                (1.0, Color::from_rgba8(30, 36, 60, 0)),
            ],
        );
        c.fill(&head, hb, None);
    }
    // The cheeks.
    for s in [-1.0f32, 1.0] {
        let (cx, cy, cz) = r.m3(s * 19.0, 10.5 - smile.max(0.0) * 1.3);
        let vis = clamp01(cz / 8.0);
        if vis <= 0.0 {
            continue;
        }
        let al = (0.10 + 0.45 * p[at::BLUSH] + 0.08 * smile.max(0.0)) * vis;
        if let Some(cheek) = ell(cx, cy, 7.0, 4.6, roll) {
            let g = radial(
                (cx, cy),
                (cx, cy),
                6.5,
                &[(0.0, rgba(0xF28CA0, al)), (1.0, rgba(0xF28CA0, 0.0))],
            );
            c.fill(&cheek, g, hm);
        }
    }
    // The eye patches, lifting with the brows.
    let sh = shapes();
    for s in [-1.0f32, 1.0] {
        let brow = (if s < 0.0 {
            p[at::BROW_L]
        } else {
            p[at::BROW_R]
        }) + p[at::WIDE] * 0.4;
        let happy = if s < 0.0 {
            p[at::HAPPY_L]
        } else {
            p[at::HAPPY_R]
        };
        let tweak = |x: f32, y: f32| {
            let w = clamp01((5.0 - y) / 13.0);
            let inner = clamp01(if s * x > 0.0 { 13.0 - x.abs() } else { 0.0 } / 9.0);
            r.m(x, y - brow * 2.6 * w - inner * brow * 0.6 + happy * 0.6 * w)
        };
        let Some(patch) = trace(if s < 0.0 { &sh.patch_l } else { &sh.patch_r }, tweak) else {
            continue;
        };
        let (pcx, pcy) = r.m(s * 12.0, -1.0);
        c.fill(&patch, ink_fill(pcx, pcy, 9.0), hm);
        let pclip = c.mask(&patch);
        let g = radial(
            (pcx - 2.5, pcy - 4.0),
            (pcx - 2.5, pcy - 4.0),
            6.0,
            &[
                (0.0, Color::from_rgba8(255, 255, 255, 26)),
                (1.0, Color::from_rgba8(255, 255, 255, 0)),
            ],
        );
        c.fill(&patch, g, pclip.as_ref());
    }
    // The eyes.
    for s in [-1.0f32, 1.0] {
        draw_eye(c, &r, p, s, light);
    }
    // The secretary's glasses.
    if secretary {
        for s in [-1.0f32, 1.0] {
            let Some(ring) = poly(&ring_pts(|x, y| r.m(x, y), s * 12.0, -2.0, 8.6, 40)) else {
                continue;
            };
            c.fill_c(&ring, Color::from_rgba8(190, 215, 255, 26), None);
            let rclip = c.mask(&ring);
            let (a, b) = (r.m(s * 12.0 - 6.0, -9.0), r.m(s * 12.0 + 2.0, 5.0));
            let g = linear(
                a,
                b,
                &[
                    (0.0, Color::TRANSPARENT),
                    (0.42, Color::TRANSPARENT),
                    (0.5, Color::from_rgba8(255, 255, 255, 56)),
                    (0.6, Color::TRANSPARENT),
                ],
            );
            c.fill(&ring, g, rclip.as_ref());
            c.stroke(&ring, col(GOLD), 2.2);
        }
        for d in [
            "M-3.4 -3 Q0 -5.4 3.4 -3",
            "M-20.6 -4 L-27 -6.5",
            "M20.6 -4 L27 -6.5",
        ] {
            if let Some(l) = trace(&parse(d, 0.0, 0.0), |x, y| r.m(x, y)) {
                c.stroke(&l, col(GOLD), 2.2);
            }
        }
    }
    // The nose, its light.
    if let Some(nose) = trace(&sh.nose, |x, y| r.m(x, y)) {
        c.fill_c(&nose, col(INK), hm);
    }
    let (nx, ny) = r.m(-1.0, 8.2);
    if let Some(shine) = ell(nx, ny, 1.3, 0.6, roll) {
        c.fill_c(&shine, Color::from_rgba8(255, 255, 255, 71), hm);
    }
    draw_mouth(c, &r, p, hm);
    rim(c, &head, (hc.0, hc.1, 31.0), light, 7.0, false);
    c.stroke(&head, col(INK), 2.3);

    // The bow tie, on the body, in front of the head's bottom.
    if !classic {
        let k = r.post(0.0, 0.0);
        let x = lerp(bx + bshift * 0.75, k.0, 0.35);
        let y = lerp(by - 8.3, k.1 + 29.0, 0.4) + if secretary { -2.5 } else { 0.0 };
        let old = c.push(
            Transform::from_scale((body_yaw * 1.4).cos(), 1.0)
                .post_rotate((roll * 0.35).to_degrees())
                .post_translate(x, y),
        );
        let g = linear(
            (0.0, -5.0),
            (0.0, 5.0),
            &[(0.0, col(0x9AD99D)), (1.0, col(0x5FA864))],
        );
        for side in [&sh.bow_l, &sh.bow_r] {
            if let Some(w) = plain(side) {
                c.fill(&w, g.clone(), None);
                c.stroke(&w, col(INK), 1.8);
            }
        }
        if let Some(knot) = rrect(-2.5, -2.6, 5.0, 5.2, 1.6) {
            c.fill_c(&knot, col(MAGIC), None);
            c.stroke(&knot, col(INK), 1.8);
        }
        c.ts = old;
    }

    // The hat, on its own springs.
    if magician {
        draw_hat(c, &r, p, sec, light, time);
    }
}

fn draw_eye(c: &mut Canvas, r: &Rig, p: &Params, s: f32, light: &Light) {
    let left = s < 0.0;
    let brow = if left { p[at::BROW_L] } else { p[at::BROW_R] };
    let wide = p[at::WIDE];
    let (cx, cy) = (s * 12.0, -2.5 - wide * 0.6 + brow * 0.25);
    let lid = if left { p[at::LID_L] } else { p[at::LID_R] };
    let happy = if left { p[at::HAPPY_L] } else { p[at::HAPPY_R] };
    let rr = 4.1 * (1.0 + 0.22 * wide);
    let top = clamp01(lid.max(happy * 0.42));
    let low = clamp01(p[at::SMILE].max(0.0) * 0.18 + happy * 0.8 - wide * 0.1);
    let m = |x: f32, y: f32| r.m(x, y);
    if let Some(white) = poly(&ring_pts(m, cx, cy, rr, 32))
        && top + low < 0.98
    {
        let wclip = c.mask(&white);
        let wc = wclip.as_ref();
        let (ccx, ccy) = m(cx, cy);
        let wg = radial(
            (ccx, ccy + 1.0),
            (ccx, ccy),
            rr * 1.1,
            &[
                (0.0, col(0xFFFFFF)),
                (0.75, col(0xF3F4F8)),
                (1.0, col(0xC9CEDA)),
            ],
        );
        c.fill(&white, wg, None);
        let pr = 2.35 * (1.0 - 0.18 * wide);
        let (px, py) = (cx + p[at::GX] * 1.55, cy + p[at::GY] * 1.3 + 0.2);
        if let Some(pupil) = poly(&ring_pts(m, px, py, pr, 24)) {
            let (pcx, pcy) = m(px, py);
            let pg = radial(
                (pcx, pcy + 0.8),
                (pcx, pcy),
                pr * 1.3,
                &[
                    (0.0, col(0x3A4152)),
                    (0.6, col(0x1E2129)),
                    (1.0, col(0x101217)),
                ],
            );
            c.fill(&pupil, pg, wc);
        }
        // The scene's colour in the eye, then the drawing's highlights.
        let (gx, gy) = m(px + 0.9, py + 1.1);
        if let Some(e) = ell(gx, gy, 0.75, 0.45, 0.0) {
            c.fill_c(&e, with_alpha(light.eye, 0.35), wc);
        }
        let (h1x, h1y) = m(px - 0.75, py - 0.85);
        if let Some(h) = circle(h1x, h1y, 0.9) {
            c.fill_c(&h, col(0xFFFFFF), wc);
        }
        let (h2x, h2y) = m(px + 0.95, py - 1.2);
        if let Some(h) = circle(h2x, h2y, 0.35) {
            c.fill_c(&h, col(0xFFFFFF), wc);
        }
        // The lids: the patch's ink closing from above, the cheek from
        // below.
        if top > 0.001 {
            let yl = cy - rr + 2.0 * rr * top;
            let mut pts: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let x = cx - rr - 1.0 + (2.0 * rr + 2.0) * i as f32 / 12.0;
                    let d = (x - cx) / rr;
                    m(x, yl + 0.9 * top * (1.0 - d * d))
                })
                .collect();
            pts.push(m(cx + rr + 1.0, cy - rr - 2.0));
            pts.push(m(cx - rr - 1.0, cy - rr - 2.0));
            if let Some(l) = poly(&pts) {
                c.fill_c(&l, col(0x20232B), wc);
            }
        }
        if low > 0.001 {
            let yl = cy + rr - 2.0 * rr * low;
            let mut pts: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let x = cx - rr - 1.0 + (2.0 * rr + 2.0) * i as f32 / 12.0;
                    let d = (x - cx) / rr;
                    m(x, yl - 0.8 * low * (1.0 - d * d))
                })
                .collect();
            pts.push(m(cx + rr + 1.0, cy + rr + 2.0));
            pts.push(m(cx - rr - 1.0, cy + rr + 2.0));
            if let Some(l) = poly(&pts) {
                c.fill_c(&l, col(0x20232B), wc);
            }
        }
    }
    // Closed in a blink: a calm line; happy: the wink drawing's arch.
    let shut = sm(0.78, 1.0, lid) * (1.0 - sm(0.3, 0.6, happy));
    if shut > 0.0
        && let Some(l) = trace(
            &[
                Cmd::M(cx - rr * 0.95, cy + 0.2),
                Cmd::Q(cx, cy + rr * 0.6, cx + rr * 0.95, cy + 0.2),
            ],
            m,
        )
    {
        c.stroke(&l, rgba(FUR, shut), 1.7);
    }
    let arch = sm(0.5, 0.95, happy);
    if arch > 0.0
        && let Some(l) = trace(
            &[
                Cmd::M(cx - 3.9, cy + 1.1),
                Cmd::Q(cx, cy - 3.3, cx + 3.9, cy + 1.1),
            ],
            m,
        )
    {
        c.stroke(&l, rgba(FUR, arch), 2.2);
    }
}

fn draw_mouth(c: &mut Canvas, r: &Rig, p: &Params, clip: Option<&Mask>) {
    let sml = p[at::SMILE];
    let jaw = clamp01(p[at::JAW]);
    let rnd = clamp01(p[at::ROUND]);
    let y0 = 16.2;
    let w = (3.1 + sml.max(0.0) * 3.2 + jaw * 1.3 - rnd * 1.6 - (-sml).max(0.0) * 0.6).max(1.7);
    let cy = y0 - sml * 2.1 + jaw * 1.2;
    let up = y0 + sml * 0.9 - jaw * 0.9 - rnd * jaw * 1.4;
    let dn = y0 + sml * 0.9 + jaw * (6.2 + 2.2 * sml.max(0.0)) + rnd * jaw * 0.6;
    let kx = 0.55 + 0.4 * rnd;
    let cu = cy + 4.0 / 3.0 * (up - cy);
    let cd = cy + 4.0 / 3.0 * (dn - cy);
    let cmds = [
        Cmd::M(-w, cy),
        Cmd::C(-w * kx, cu, w * kx, cu, w, cy),
        Cmd::C(w * kx, cd, -w * kx, cd, -w, cy),
        Cmd::Z,
    ];
    let Some(mouth) = trace(&cmds, |x, y| r.m(x, y)) else {
        return;
    };
    if dn - up > 0.6 {
        c.fill_c(&mouth, col(0x2B1820), clip);
        let mclip = c.mask(&mouth);
        let (tx, ty) = r.m(0.0, dn - 0.4);
        let (tw, th) = (w * 0.62, 1.2 + 2.2 * jaw);
        if let Some(tongue) = ell(tx, ty, tw, th, p[at::ROLL]) {
            let g = radial(
                (tx, ty - th * 0.6),
                (tx, ty),
                tw * 1.1,
                &[(0.0, col(0xF4A0AC)), (1.0, col(0xC9606F))],
            );
            c.fill(&tongue, g, mclip.as_ref());
        }
    }
    c.stroke(&mouth, col(INK), 1.9);
}

fn draw_hat(c: &mut Canvas, r: &Rig, p: &Params, sec: &Secondary, light: &Light, time: f32) {
    let sh = shapes();
    let (ax, ay, _) = r.p(-4.0, -25.0, 3.0);
    let yaw_h = p[at::YAW] + sec.hat_yaw;
    let st = (-sec.hat_vy * 0.0045).clamp(-0.08, 0.1);
    let old = c.push(
        Transform::from_scale(1.0 - st * 0.6, 1.0 + st)
            .post_rotate((-8.0f32.to_radians() + p[at::ROLL] + sec.hat_r).to_degrees())
            .post_translate(ax + sec.hat_x, ay + sec.hat_y),
    );
    if let Some(crown) = plain(&sh.hat_crown) {
        c.stroke(&crown, col(FUR), 3.0);
        let hl = (0.32 - yaw_h * 0.35).clamp(0.08, 0.6);
        let g = linear(
            (-13.5, 0.0),
            (13.5, 0.0),
            &[
                (0.0, col(0x16181D)),
                (hl, col(0x4A4F5C)),
                ((hl + 0.2).min(0.98), col(0x2A2E37)),
                (1.0, col(0x121318)),
            ],
        );
        c.fill(&crown, g, None);
        rim(c, &crown, (0.0, -12.0, 14.0), light, 4.0, true);
        // The top, seen more as the head bows.
        let ry = (2.6 - p[at::PITCH] * 9.0).clamp(0.9, 6.5);
        if let Some(top) = ell(0.0, -24.4, 13.2, ry, 0.0) {
            c.fill_c(&top, col(HATTOP), None);
        }
        if let Some(l) = plain(&sh.hat_crease) {
            c.stroke(&l, col(STITCH), 1.6);
        }
        if let Some(l) = plain(&sh.hat_crease2) {
            c.stroke(&l, col(STITCH), 1.3);
        }
        // The patch slides round the crown as the head turns.
        let patch_a = (4.0f32 / 13.0).asin() + yaw_h;
        if patch_a.cos() > 0.05 {
            let o2 = c.push(
                Transform::from_scale(patch_a.cos().max(0.2), 1.0)
                    .post_rotate(-8.0)
                    .post_translate(13.0 * patch_a.sin() - 4.0, 0.0),
            );
            if let Some(pt) = rrect(-3.5, -18.0, 7.0, 6.5, 1.0) {
                c.fill_c(&pt, col(HATTOP), None);
                c.stroke(&pt, rgba(FUR, 0.6), 0.7);
            }
            c.ts = o2;
        }
        if let Some(band) = plain(&sh.hat_band) {
            let g = linear(
                (-12.7, 0.0),
                (12.7, 0.0),
                &[
                    (0.0, col(0x4F8F55)),
                    (hl, col(0xA7E3A5)),
                    (1.0, col(0x4C8A52)),
                ],
            );
            c.fill(&band, g, None);
            c.stroke(&band, col(INK), 1.2);
        }
    }
    if let Some(brim) = plain(&sh.hat_brim) {
        c.stroke(&brim, col(FUR), 3.0);
        c.fill(&brim, ink_fill(-6.0, -1.0, 22.0), None);
    }
    let spark_a = 1.22 + yaw_h;
    if spark_a.cos() > -0.15 {
        let pulse = 1.0 + 0.12 * (time * 3.1).sin();
        let sx = 14.6 * spark_a.sin() - 0.5;
        glow(c, (sx, -12.6), 9.0, Color::from_rgba8(123, 196, 127, 140));
        let o2 = c.push(
            Transform::from_scale(pulse * (spark_a.cos() * 0.6 + 0.4).max(0.35), pulse)
                .post_rotate((time * 0.6).to_degrees())
                .post_translate(sx, -12.6),
        );
        if let Some(spark) = plain(&sh.spark) {
            c.fill_c(&spark, col(MAGIC), None);
            c.stroke(&spark, col(INK), 1.0);
        }
        c.ts = o2;
    }
    c.ts = old;
}

/// The panda posed as `sim` over `frame` (B, G, R, x, what is behind the
/// person already there): the background's glow behind it, its soft
/// shadow, the scene's light on it.
pub fn draw(frame: &mut [u8], sim: &Sim, look: PandaLook, light: &Light) {
    let (cy, s) = frame_of(look);
    let s = H as f32 * s;
    let (ox, oy) = (W as f32 / 2.0, H as f32 * cy);
    // The panda's own layer: its box (the pencil, the ears, the hat).
    let x0 = (ox - 64.0 * s).max(0.0).floor();
    let x1 = (ox + 64.0 * s).min(W as f32).ceil();
    let y0 = (oy - 74.0 * s).max(0.0).floor();
    // Drawn at [`LAYER`] of the frame's size, laid over it between its
    // pixels: the drawing's soft shapes stay as sharp, at two thirds of
    // the processor's time.
    let lw = ((x1 - x0) * LAYER).ceil() as u32 + 1;
    let lh = ((H as f32 - y0) * LAYER).ceil() as u32 + 1;
    let Some(pixmap) = Pixmap::new(lw, lh) else {
        return;
    };
    let k = s * LAYER;
    let mut c = Canvas {
        pixmap,
        ts: Transform::from_row(k, 0.0, 0.0, k, (ox - x0) * LAYER, (oy - y0) * LAYER),
        alpha: 1.0,
    };
    draw_panda(&mut c, &sim.p, &sim.sec, look, light, sim.t);
    compose(
        frame,
        &c.pixmap,
        (x0 as usize, y0 as usize),
        light,
        (ox, oy - s * 6.0, s * 10.0, s * 62.0),
    );
}

/// A column's glow distance across (squared), its shadow's quarter
/// column, its layer's column and how far towards the next.
type Column = (f32, Option<usize>, Option<(usize, u32)>);

/// The panda's layer's size, of the frame's.
const LAYER: f32 = 0.8;

/// The panda's layer (at `lx`, `ly` in the frame, [`LAYER`] of its size)
/// laid over `frame` in one pass, as the mockup composes it: the background's glow behind it
/// (screened, full to `inner` round `glow`, nothing at `outer`), the
/// panda's soft shadow (the mockup's `shadowBlur`: its alpha at a quarter
/// of the size, blurred, moved), then the panda, the scene's tints on it
/// (`source-atop`: one colour over it, another from both sides).
fn compose(
    frame: &mut [u8],
    layer: &Pixmap,
    (lx, ly): (usize, usize),
    light: &Light,
    (gx, gy, inner, outer): (f32, f32, f32, f32),
) {
    const Q: usize = 4;
    let (sw, sh) = (W / Q, H / Q);
    let (lw, lh) = (layer.width() as usize, layer.height() as usize);
    let data = layer.data();
    // The shadow: the layer's alpha at a quarter, blurred.
    let mut a = vec![0f32; sw * sh];
    for (i, v) in a.iter_mut().enumerate() {
        let (x, y) = ((i % sw) * Q, (i / sw) * Q);
        if x < lx || y < ly {
            continue;
        }
        let (sx, sy) = (
            ((x - lx) as f32 * LAYER) as usize,
            ((y - ly) as f32 * LAYER) as usize,
        );
        if sx + 3 > lw || sy + 3 > lh {
            continue;
        }
        let mut sum = 0u32;
        for dy in 0..3 {
            for dx in 0..3 {
                let at = ((sy + dy) * lw + sx + dx) * 4 + 3;
                sum += u32::from(data.get(at).copied().unwrap_or(0));
            }
        }
        *v = sum as f32 / (255.0 * 9.0);
    }
    // A Gaussian of `shadowBlur / 2`, as three box passes each way.
    let sigma = H as f32 * light.sb / 2.0 / Q as f32;
    let radius = ((sigma * sigma + 1.0).sqrt() as usize).max(1);
    for _ in 0..3 {
        box_pass(&mut a, sw, sh, radius, true);
        box_pass(&mut a, sw, sh, radius, false);
    }
    let shadow_k: Vec<u32> = a
        .iter()
        .map(|v| (v * light.shadow.alpha() * 256.0) as u32)
        .collect();
    let shade = [
        light.shadow.blue(),
        light.shadow.green(),
        light.shadow.red(),
    ]
    .map(|v| (v * 255.0) as u32);
    let (sdx, sdy) = (
        (H as f32 * light.sx) as isize,
        (H as f32 * light.sy) as isize,
    );
    // The glow, by squared distance.
    const STEPS: usize = 512;
    let ga = light.glow.alpha();
    let glow = [light.glow.blue(), light.glow.green(), light.glow.red()]
        .map(|v| (v * ga * 255.0).round() as u32);
    let table: Vec<u32> = (0..=STEPS)
        .map(|i| {
            let d = (i as f32 / STEPS as f32).sqrt() * outer;
            let k = if d <= inner {
                1.0
            } else {
                1.0 - (d - inner) / (outer - inner)
            };
            (k.clamp(0.0, 1.0) * 256.0) as u32
        })
        .collect();
    let r2 = outer * outer;
    // The tints, as integers: the first's colour and strength, the
    // second's colour and, for each column, its strength.
    let t = light.tint;
    let ta = (t.alpha() * 256.0) as u32;
    let tc = [t.blue(), t.green(), t.red()].map(|v| (v * 255.0) as u32);
    let t2 = light.tint2;
    let t2c = [t2.blue(), t2.green(), t2.red()].map(|v| (v * 255.0) as u32);
    let t2k: Vec<u32> = (0..W)
        .map(|x| {
            let side = (1.0 - (x as f32 / W as f32 - 0.5).abs() * 2.0).clamp(0.0, 1.0);
            (t2.alpha() * (1.0 - side) * 256.0) as u32
        })
        .collect();
    // For each column: the glow's distance across, the shadow's quarter
    // column, the layer's column and how far towards the next.
    let cols: Vec<Column> = (0..W)
        .map(|x| {
            let dxg = x as f32 + 0.5 - gx;
            let sx = x as isize - sdx;
            let q = (sx >= 0 && (sx as usize) < sw * Q).then_some(sx as usize / Q);
            let fx = (x as f32 - lx as f32 + 0.5) * LAYER - 0.5;
            let l = (x >= lx && fx >= 0.0 && (fx as usize) + 1 < lw)
                .then(|| (fx as usize, ((fx - fx.floor()) * 256.0) as u32));
            (dxg * dxg, q, l)
        })
        .collect();
    let shadow_rows = ly.saturating_sub(64)..H;
    for y in 0..H {
        let dyg = y as f32 + 0.5 - gy;
        let dy2 = dyg * dyg;
        let in_glow = dy2 < r2;
        let sy = y as isize - sdy;
        let qrow = (shadow_rows.contains(&y) && sy >= 0 && (sy as usize) < sh * Q)
            .then(|| (sy as usize / Q) * sw);
        // The layer's two rows this one lies between, and how far.
        let fy = (y as f32 - ly as f32 + 0.5) * LAYER - 0.5;
        let lrow = (y >= ly && fy >= 0.0 && (fy as usize) + 1 < lh).then(|| {
            let r0 = fy as usize;
            (r0 * lw, (r0 + 1) * lw, ((fy - r0 as f32) * 256.0) as u32)
        });
        if !in_glow && qrow.is_none() && lrow.is_none() {
            continue;
        }
        let Some(row) = frame.get_mut(y * W * 4..(y + 1) * W * 4) else {
            continue;
        };
        for ((x, px), &(dx2, qcol, lcol)) in
            row.as_chunks_mut::<4>().0.iter_mut().enumerate().zip(&cols)
        {
            // The panda, tinted, first: where it is opaque, nothing under
            // it shows.
            let mut over = None;
            if let (Some((r0, r1, ty)), Some((c0, tx))) = (lrow, lcol)
                && let Some([r, g, bl, la]) = bilinear(data, (r0 + c0, r1 + c0), tx, ty)
                && la > 0
            {
                let la = la.min(255);
                let k2 = t2k.get(x).copied().unwrap_or(0);
                let mut src = [bl, g, r];
                for (s, (c1, c2)) in src.iter_mut().zip(tc.into_iter().zip(t2c)) {
                    let v = (*s * (256 - ta) + ((c1 * la) >> 8) * ta) >> 8;
                    *s = (v * (256 - k2) + ((c2 * la) >> 8) * k2) >> 8;
                }
                if la == 255 {
                    px[0] = src[0].min(255) as u8;
                    px[1] = src[1].min(255) as u8;
                    px[2] = src[2].min(255) as u8;
                    continue;
                }
                over = Some((src, la));
            }
            let d2 = dx2 + dy2;
            if over.is_none() && d2 >= r2 && qcol.is_none() {
                continue;
            }
            let mut b = [u32::from(px[0]), u32::from(px[1]), u32::from(px[2])];
            // The glow.
            if d2 < r2 {
                let k = table
                    .get((d2 / r2 * STEPS as f32) as usize)
                    .copied()
                    .unwrap_or(0);
                for (v, g) in b.iter_mut().zip(glow) {
                    let g = (g * k) >> 8;
                    *v += g * (255 - *v) / 255;
                }
            }
            // The shadow.
            if let (Some(q), Some(qc)) = (qrow, qcol) {
                let k = shadow_k.get(q + qc).copied().unwrap_or(0).min(256);
                if k > 0 {
                    for (v, c) in b.iter_mut().zip(shade) {
                        *v = (*v * (256 - k) + c * k) >> 8;
                    }
                }
            }
            if let Some((src, la)) = over {
                for (v, s) in b.iter_mut().zip(src) {
                    *v = (s + *v * (255 - la) / 255).min(255);
                }
            }
            px[0] = b[0] as u8;
            px[1] = b[1] as u8;
            px[2] = b[2] as u8;
        }
    }
}

/// The layer's premultiplied pixel between four: rows `r0` and `r1` at
/// `at.0` and `at.1`, `tx` and `ty` of 256 towards the second.
fn bilinear(data: &[u8], at: (usize, usize), tx: u32, ty: u32) -> Option<[u32; 4]> {
    let px = |i: usize| -> Option<[u32; 4]> {
        let p = data.get(i * 4..i * 4 + 4)?;
        Some([p.first()?, p.get(1)?, p.get(2)?, p.get(3)?].map(|&v| u32::from(v)))
    };
    let (a, b, c, d) = (px(at.0)?, px(at.0 + 1)?, px(at.1)?, px(at.1 + 1)?);
    if a[3] == 0 && b[3] == 0 && c[3] == 0 && d[3] == 0 {
        return Some([0; 4]);
    }
    let mut out = [0u32; 4];
    for (i, o) in out.iter_mut().enumerate() {
        let (a, b, c, d) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
            c.get(i).copied().unwrap_or(0),
            d.get(i).copied().unwrap_or(0),
        );
        let top = a * (256 - tx) + b * tx;
        let bottom = c * (256 - tx) + d * tx;
        *o = (top * (256 - ty) + bottom * ty) >> 16;
    }
    Some(out)
}

fn box_pass(a: &mut [f32], w: usize, h: usize, r: usize, rows: bool) {
    let (n, lines, step, stride) = if rows { (w, h, 1, w) } else { (h, w, w, 1) };
    let mut line = vec![0f32; n];
    for l in 0..lines {
        let start = l * stride;
        for (i, v) in line.iter_mut().enumerate() {
            *v = a.get(start + i * step).copied().unwrap_or(0.0);
        }
        let mut sum: f32 = 0.0;
        let get = |i: isize| {
            line.get(i.clamp(0, n as isize - 1) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        for i in -(r as isize)..=(r as isize) {
            sum += get(i);
        }
        let k = 1.0 / (2 * r + 1) as f32;
        for i in 0..n {
            if let Some(v) = a.get_mut(start + i * step) {
                *v = sum * k;
            }
            sum += get(i as isize + r as isize + 1) - get(i as isize - r as isize);
        }
    }
}

/// The look's still picture, `size` pixels square (the window's
/// « Allure » tiles, the mockup's `drawStill`): the panda smiling a
/// little, zoomed 1.25, on the aurora's still colours. RGBA.
pub fn portrait(look: PandaLook, size: u32) -> Vec<u8> {
    let Some(mut pixmap) = Pixmap::new(size, size) else {
        return Vec::new();
    };
    let n = size as f32;
    if let Some(all) = resvg::tiny_skia::Rect::from_xywh(0.0, 0.0, n, n) {
        let sky = linear(
            (0.0, 0.0),
            (0.0, n),
            &[(0.0, col(0x060B18)), (1.0, col(0x0D1E2E))],
        );
        pixmap.fill_rect(all, &crate::rig::paint(sky), Transform::identity(), None);
        for (x, y, r, c) in [
            (0.25, 0.35, 0.55, Color::from_rgba8(84, 230, 170, 115)),
            (0.85, 0.2, 0.5, Color::from_rgba8(130, 120, 255, 102)),
        ] {
            let g = radial(
                (n * x, n * y),
                (n * x, n * y),
                n * r,
                &[(0.0, c), (1.0, Color::TRANSPARENT)],
            );
            pixmap.fill_rect(all, &crate::rig::paint(g), Transform::identity(), None);
        }
    }
    let (cy, s) = frame_of(look);
    let shift = if look == PandaLook::Magician {
        0.1
    } else {
        0.06
    };
    let k = n * s * 1.25;
    let mut c = Canvas {
        pixmap,
        ts: Transform::from_row(k, 0.0, 0.0, k, n / 2.0, n * (cy + shift)),
        alpha: 1.0,
    };
    let p = posed(&[(at::SMILE, 0.3)]);
    draw_panda(
        &mut c,
        &p,
        &Secondary::default(),
        look,
        &Light::aurora(),
        0.6,
    );
    let mut out = c.pixmap.take();
    // Opaque already; unpremultiplied all the same.
    for px in out.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    out
}

/// The look's picture for the window's tiles and Settings: the panda at
/// rest, smiling a little.
pub fn posed(p: &[(usize, f32)]) -> Params {
    let mut o = rest(0.0);
    o[at::BREATH] = 0.0;
    o[at::BOB] = 0.0;
    o[at::GX] = 0.0;
    o[at::GY] = 0.0;
    o[at::YAW] = 0.0;
    o[at::PITCH] = 0.0;
    o[at::ROLL] = 0.0;
    for &(k, v) in p {
        if let Some(x) = o.get_mut(k) {
            *x = v;
        }
    }
    o
}

/// Names of the pose's numbers, for the tests and screenshots.
pub mod key {
    pub use super::at::*;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{face::FaceTracker, look::BYTES, track::Frame};

    fn grey() -> Vec<u8> {
        vec![90u8; BYTES]
    }

    /// The face's pose on the portrait, and expressions at rest but
    /// `shapes` (index, value).
    fn drive(shapes: &[(usize, f32)]) -> anyhow::Result<Drive> {
        let picture = crate::face::test_face();
        let mut tracker = FaceTracker::new()?;
        let frame = Frame {
            pixels: &picture,
            width: W,
            height: H,
            order: [0, 1, 2],
        };
        let face = tracker.track(&frame, 1.0 / 30.0, false)?.expect("a face");
        let mut all = [0.0f32; 52];
        for &(i, v) in shapes {
            if let Some(x) = all.get_mut(i) {
                *x = v;
            }
        }
        Ok(Drive {
            pose: face.pose(),
            shapes: all,
        })
    }

    /// The panda after half a second of `d`, drawn on grey.
    fn drawn(d: &Drive) -> (Sim, Vec<u8>) {
        let mut sim = Sim::default();
        for _ in 0..15 {
            sim.advance(1.0 / 30.0, Some(d));
        }
        let mut frame = grey();
        draw(&mut frame, &sim, PandaLook::Classic, &Light::room());
        (sim, frame)
    }

    /// The share of the picture round a point of the panda's head that
    /// `pass` takes (by brightness).
    fn around(
        frame: &[u8],
        sim: &Sim,
        (x, y): (f32, f32),
        r: f32,
        pass: impl Fn(f32) -> bool,
    ) -> f32 {
        let (cy, s) = frame_of(PandaLook::Classic);
        let s = H as f32 * s;
        let rig = Rig::new(&sim.p);
        let (ux, uy) = rig.m(x, y);
        let (px, py) = (W as f32 / 2.0 + ux * s, H as f32 * cy + uy * s);
        let mut sum = 0.0;
        let mut n = 0.0;
        let rr = (r * s) as i32;
        for dy in -rr..=rr {
            for dx in -rr..=rr {
                let (fx, fy) = (px as i32 + dx, py as i32 + dy);
                if let Some(p) = frame.get(
                    ((fy as usize) * W + fx as usize) * 4
                        ..((fy as usize) * W + fx as usize) * 4 + 3,
                ) {
                    let v = p.iter().map(|&v| f32::from(v)).sum::<f32>() / 3.0;
                    sum += if pass(v) { 1.0 } else { 0.0 };
                    n += 1.0;
                }
            }
        }
        sum / n
    }

    /// A blink closes the eyes (their whites go); `jawOpen` opens the
    /// mouth (dark where the lips met).
    #[test]
    fn a_blink_closes_the_eyes_and_the_jaw_opens_the_mouth() -> anyhow::Result<()> {
        let white = |v: f32| v > 200.0;
        let dark = |v: f32| v < 70.0;
        let (rest_sim, rest) = drawn(&drive(&[])?);
        // The eye's upper half: its white, or the closed lid.
        let eye_open = around(&rest, &rest_sim, (-12.0, -4.8), 1.4, white);
        let (sim, blink) = drawn(&drive(&[
            (crate::face::shape::EYE_BLINK_LEFT, 0.95),
            (crate::face::shape::EYE_BLINK_RIGHT, 0.95),
        ])?);
        assert!(
            sim.p[at::LID_L] > 0.9 && sim.p[at::LID_R] > 0.9,
            "{:?}",
            sim.p
        );
        let eye_shut = around(&blink, &sim, (-12.0, -4.8), 1.4, white);
        assert!(
            eye_open > 0.4 && eye_shut < 0.1,
            "the eye's white: {eye_open} open, {eye_shut} shut"
        );
        let mouth_shut = around(&rest, &rest_sim, (0.0, 19.5), 1.5, dark);
        let (sim, open) = drawn(&drive(&[(crate::face::shape::JAW_OPEN, 0.8)])?);
        assert!(sim.p[at::JAW] > 0.8, "{:?}", sim.p);
        let mouth_open = around(&open, &sim, (0.0, 19.5), 1.5, dark);
        assert!(
            mouth_open > 0.3 && mouth_open > mouth_shut + 0.25,
            "the mouth's dark: {mouth_shut} shut, {mouth_open} open"
        );
        Ok(())
    }

    /// Without a face for two seconds it idles, and blinks by itself.
    #[test]
    fn without_a_face_it_idles_and_blinks() {
        let mut sim = Sim::default();
        let mut blinked = false;
        for _ in 0..(30 * 6) {
            sim.advance(1.0 / 30.0, None);
            blinked |= sim.p[at::LID_L] > 0.9;
        }
        assert!(sim.idle());
        assert!(blinked);
    }

    /// Each look draws at 720p within a frame's time at 30 a second on
    /// one core (the median of a few, in the thread's own time), held to
    /// it with `HOCUS_TEST_BUDGETS=1` only (`track::within_budget`).
    #[test]
    fn the_panda_draws_within_a_frame_s_budget() -> anyhow::Result<()> {
        let d = drive(&[(crate::face::shape::JAW_OPEN, 0.4)])?;
        let mut sim = Sim::default();
        sim.advance(0.5, Some(&d));
        let mut frame = grey();
        for look in PandaLook::ALL {
            draw(&mut frame, &sim, look, &Light::aurora());
            let times: Vec<Duration> = (0..7)
                .map(|_| {
                    let start = crate::track::thread_time();
                    sim.advance(1.0 / 30.0, Some(&d));
                    draw(&mut frame, &sim, look, &Light::aurora());
                    crate::track::thread_time() - start
                })
                .collect();
            // Held to a frame's time on a real machine only: CI's shared
            // runners drew a look in 47 ms, over the 33 of a frame
            // (docs/performance.md).
            crate::track::within_budget(
                &format!("the panda's {look:?}"),
                times,
                Duration::from_millis(33),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod shots {
    use super::*;
    use crate::{
        backdrop::{self, Alpha},
        segment::{Mask, Segmenter},
    };

    /// The pull request's pictures of the panda: each look on the aurora
    /// and on the room blurred without the person; a blink, a laugh, a
    /// head turned. Written to `$HOCUS_SHOTS` when it names a folder, on
    /// lavapipe (the aurora). Run by hand (`--run-ignored only`).
    #[test]
    #[ignore = "writes the pull request's screenshots"]
    fn screenshots_of_the_panda() -> anyhow::Result<()> {
        let Ok(dir) = std::env::var("HOCUS_SHOTS") else {
            return Ok(());
        };
        if !crate::scenes::tests::on_lavapipe("panda::shots::screenshots_of_the_panda") {
            return Ok(());
        }
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        let save = |name: &str, bgrx: &[u8]| -> anyhow::Result<()> {
            let rgba: Vec<u8> = bgrx
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[b, g, r, _]| [r, g, b, 255])
                .collect();
            image::save_buffer(
                dir.join(name),
                &rgba,
                W as u32,
                H as u32,
                image::ColorType::Rgba8,
            )?;
            Ok(())
        };
        let rgba = crate::segment::test_person();
        let person: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [b, g, r, 255])
            .collect();
        let mut mask = Mask::default();
        Segmenter::new()?.run(&rgba, W, H, [0, 1, 2], &mut mask)?;
        let mut alpha = Alpha::default();
        alpha.from_mask(&mask);
        let mut room = person.clone();
        backdrop::emptied(&mut room, &alpha);
        let mut painter = crate::scenes::Painter::new(true).expect("lavapipe");
        let aurora = painter
            .draw(hocus_link::camera::Scene::Aurora, 6.0)
            .expect("a picture");
        let mut sky = person.clone();
        backdrop::scene(&mut sky, &Alpha::default(), &aurora);
        let posed_sim = |pose: &[(usize, f32)]| Sim {
            p: posed(pose),
            t: 0.6,
            ..Sim::default()
        };
        let smile = posed_sim(&[(at::SMILE, 0.3)]);
        for look in PandaLook::ALL {
            let mut made = sky.clone();
            draw(&mut made, &smile, look, &Light::aurora());
            save(&format!("panda-{}-aurore.png", look.word()), &made)?;
            let mut made = room.clone();
            draw(&mut made, &smile, look, &Light::room());
            save(&format!("panda-{}-piece-floue.png", look.word()), &made)?;
        }
        for (name, pose) in [
            (
                "cligne",
                vec![(at::LID_L, 1.0), (at::LID_R, 1.0), (at::SMILE, 0.2)],
            ),
            (
                "rit",
                vec![
                    (at::JAW, 0.85),
                    (at::SMILE, 1.0),
                    (at::HAPPY_L, 1.0),
                    (at::HAPPY_R, 1.0),
                    (at::BLUSH, 1.0),
                    (at::PITCH, 0.16),
                    (at::ROLL, 0.08),
                    (at::PAWS, 1.0),
                    (at::BROW_L, 0.45),
                    (at::BROW_R, 0.45),
                ],
            ),
            (
                "tourne",
                vec![
                    (at::YAW, 0.58),
                    (at::GX, 1.0),
                    (at::GY, 0.1),
                    (at::PITCH, -0.05),
                    (at::ROLL, -0.06),
                    (at::SMILE, 0.08),
                ],
            ),
        ] {
            let mut made = sky.clone();
            draw(
                &mut made,
                &posed_sim(&pose),
                PandaLook::Magician,
                &Light::aurora(),
            );
            save(&format!("panda-{name}.png"), &made)?;
        }
        let tile = portrait(PandaLook::Secretary, 200);
        image::save_buffer(
            dir.join("panda-allure-tile.png"),
            &tile,
            200,
            200,
            image::ColorType::Rgba8,
        )?;
        Ok(())
    }
}
