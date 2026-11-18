//! The screen light (#619, docs/design/camera.md): while an application
//! uses the camera (a call), the edges of the screen holding its window
//! glow softly and light the user's face. Any application: the light is
//! the screen's, the video is not touched.
//!
//! The shell says who uses a camera (bar.md 5.12, `ToCompositor::Camera`):
//! their ids are matched against the windows' application ids as the
//! in-use popover matches them, the focused window first, then from the
//! front; without one of theirs, the focused screen lights. The light
//! follows that screen as the call's window moves.
//!
//! Drawn as the mockup's two inset `box-shadow`s (a frame a quarter of the
//! glow's width, and the glow blurred over its width, spread over half of
//! it): four strips along the edges, computed once for the screen's size,
//! scale, warmth and strength into textures of their own, on the CPU as on
//! the GPU. Over everything but the pointer, on the screen only: never in
//! a capture, a share or a recording (their `Purpose`). It fades in and
//! out over 600 ms (at once with reduced motion), each frame of the fade
//! changing only the strips' alpha, which damages the edges alone; steady,
//! it asks for no frame and damages nothing.

use std::{cell::RefCell, collections::BTreeMap, time::Duration};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Id, memory::MemoryRenderBuffer},
    },
    output::Output,
    utils::{Physical, Rectangle, Size},
};

use crate::{
    State,
    config::{Strength, Warmth},
    decoration::{Keyed, Placed, buffer},
    model::WindowId,
    motion::{Bezier, Tween, lerp},
    render::{OutputElement, OutputElements},
};

/// The light comes and goes in this time, as the mockup's transition.
pub const FADE: Duration = Duration::from_millis(600);
/// CSS's `ease`, the mockup's transition's curve.
const EASE: Bezier = Bezier(0.25, 0.1, 0.25, 1.0);

/// The light: who uses a camera, the screens it shows on, their edges'
/// pictures.
#[derive(Default)]
pub struct ScreenLight {
    /// Who uses a camera, as the shell last said.
    users: Vec<hocus_link::CameraUser>,
    /// The light on each screen it shows on or leaves, by screen name.
    glows: BTreeMap<String, Glow>,
    /// Each screen's strips, as last drawn.
    pictures: RefCell<BTreeMap<String, Picture>>,
}

impl std::fmt::Debug for ScreenLight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenLight")
            .field("users", &self.users)
            .field("glows", &self.glows)
            .finish_non_exhaustive()
    }
}

/// The light on a screen, going from `from` to `to` (0: none, 1: full).
#[derive(Debug, Clone, Copy)]
struct Glow {
    from: f64,
    to: f64,
    tween: Tween,
}

impl Glow {
    fn level(&self, now: Duration) -> f64 {
        lerp(self.from, self.to, self.tween.at(now)).clamp(0.0, 1.0)
    }

    /// From where it is at `now` towards `to`, in the part of the fade
    /// left (at once with `fade` zero).
    fn towards(&mut self, to: f64, now: Duration, fade: Duration) {
        let from = self.level(now);
        *self = Self {
            from,
            to,
            tween: Tween::new(now, fade.mul_f64((to - from).abs()), EASE),
        };
    }
}

/// What a screen's strips were made for: its size in pixels, its scale
/// (thousandths), the warmth and the strength.
type Key = ((i32, i32), u32, Warmth, Strength);

/// A screen's four strips: their pixels, where they go, their ids (kept
/// while they do not change, so that a steady light damages nothing).
struct Picture {
    key: Key,
    strips: Vec<(MemoryRenderBuffer, Rectangle<i32, Physical>, Id)>,
}

impl State {
    /// The shell says who uses a camera now.
    pub(crate) fn camera_users(&mut self, users: Vec<hocus_link::CameraUser>) {
        if users == self.light.users {
            return;
        }
        tracing::debug!(
            apps = ?users.iter().map(|u| u.app.as_str()).collect::<Vec<_>>(),
            "who uses the camera"
        );
        self.light.users = users;
    }

    /// The window of an application using the camera: the focused one,
    /// else the frontmost, not minimised.
    fn camera_window(&self) -> Option<WindowId> {
        let users = &self.light.users;
        let theirs = |id: &WindowId| {
            !self.model.is_minimized(*id)
                && self
                    .windows
                    .get(id)
                    .and_then(|m| crate::windows::app_id(&m.window))
                    .is_some_and(|app| {
                        let app = app.to_lowercase();
                        let last = app.rsplit('.').next().unwrap_or(&app);
                        !app.is_empty()
                            && users
                                .iter()
                                .flat_map(|u| &u.ids)
                                .any(|id| *id == app || id == last)
                    })
        };
        self.model.focused().filter(|id| theirs(id)).or_else(|| {
            self.model
                .stack()
                .iter()
                .rev()
                .copied()
                .find(|id| theirs(id))
        })
    }

    /// The screen the light should be on: the camera's window's, else the
    /// focused one; none while nobody uses a camera or it is turned off.
    fn screen_light_wanted(&self) -> Option<Output> {
        if !self.config.camera.screen_light || self.light.users.is_empty() {
            return None;
        }
        self.camera_window()
            .and_then(|id| self.output_of(id))
            .or_else(|| self.bubbles_screen())
    }

    /// Once a turn of the loop: the light heads for the screen it should
    /// be on, and leaves the others; the screens it fades on draw again,
    /// they alone.
    pub(crate) fn tick_screen_light(&mut self, now: Duration) {
        let wanted = self.screen_light_wanted().map(|o| o.name());
        if wanted.is_none() && self.light.glows.is_empty() {
            return;
        }
        // A change starts now, not as of the last frame drawn.
        let at = self.now();
        let fade = if self.reduced_motion() {
            Duration::ZERO
        } else {
            FADE
        };
        // Said in the journal, with who lit it: the light is seen, a
        // surprise is to be explained.
        let apps: Vec<&str> = self.light.users.iter().map(|u| u.app.as_str()).collect();
        for (name, glow) in &mut self.light.glows {
            let to = if wanted.as_ref() == Some(name) {
                1.0
            } else {
                0.0
            };
            if glow.to != to {
                tracing::info!(screen = name, on = to > 0.0, ?apps, "the screen light");
                glow.towards(to, at, fade);
            }
        }
        if let Some(name) = wanted
            && !self.light.glows.contains_key(&name)
        {
            tracing::info!(screen = name, on = true, ?apps, "the screen light");
            let mut glow = Glow {
                from: 0.0,
                to: 0.0,
                tween: Tween::new(at, Duration::ZERO, EASE),
            };
            glow.towards(1.0, at, fade);
            self.light.glows.insert(name, glow);
        }
        let mut moving = Vec::new();
        self.light.glows.retain(|name, glow| {
            let done = glow.tween.done(now);
            if !done {
                moving.push(name.clone());
            }
            // One more frame at its end, then gone.
            !(done && glow.to == 0.0 && glow.tween.done(at))
        });
        let gone: Vec<String> = self
            .light
            .pictures
            .borrow()
            .keys()
            .filter(|name| !self.light.glows.contains_key(*name))
            .cloned()
            .collect();
        for name in gone {
            self.light.pictures.borrow_mut().remove(&name);
            // The frame without it.
            moving.push(name);
        }
        let screens: Vec<Output> = self
            .space
            .outputs()
            .filter(|o| moving.contains(&o.name()))
            .cloned()
            .collect();
        self.frames.moved_on.extend(screens);
    }

    /// The light on `output`, front first: on the screen only.
    pub(crate) fn screen_light_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let name = output.name();
        let Some(glow) = self.light.glows.get(&name) else {
            return;
        };
        let level = glow.level(self.now());
        if level <= 0.0 {
            return;
        }
        let Some(mode) = output.current_mode() else {
            return;
        };
        let size = output.current_transform().transform_size(mode.size);
        let scale = output.current_scale().fractional_scale();
        let camera = self.config.camera;
        let key: Key = (
            (size.w, size.h),
            (scale * 1000.0).round() as u32,
            camera.screen_light_warmth,
            camera.screen_light_strength,
        );
        let mut pictures = self.light.pictures.borrow_mut();
        if pictures.get(&name).is_none_or(|p| p.key != key) {
            pictures.insert(
                name.clone(),
                Picture {
                    key,
                    strips: strips(
                        size,
                        scale,
                        camera.screen_light_warmth,
                        camera.screen_light_strength,
                    ),
                },
            );
        }
        let Some(picture) = pictures.get(&name) else {
            return;
        };
        for (texture, dst, id) in &picture.strips {
            let all = Rectangle::from_size((f64::from(dst.size.w), f64::from(dst.size.h)).into());
            match Placed::new(renderer, texture, all, *dst, level as f32) {
                Ok(e) => out.push(OutputElements::Frame(Keyed::new(e, id.clone()))),
                Err(err) => tracing::warn!(?err, "could not draw the screen light"),
            }
        }
    }

    /// Tests: each screen the light shows on or leaves, and how far it is
    /// on (0 to 1).
    pub(crate) fn screen_light(&self) -> Vec<(String, f64)> {
        let now = self.now();
        self.light
            .glows
            .iter()
            .map(|(name, glow)| (name.clone(), glow.level(now)))
            .collect()
    }
}

/// The light's opacity at `d` pixels from an edge, along it: the
/// complement of the inner box's blur, which [`strips`] multiplies across
/// the two axes.
fn inside(d: f64, length: f64, spread: f64, sigma: f64) -> f64 {
    phi((d - spread) / sigma) - phi((d - (length - spread)) / sigma)
}

/// The standard normal's distribution function, by erf's approximation
/// (Abramowitz and Stegun 7.1.26, within 1.5e-7).
fn phi(x: f64) -> f64 {
    let z = x / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-z * z).exp();
    0.5 * (1.0 + erf.copysign(z))
}

/// The light's opacity of each pixel of a screen `size` pixels large at
/// `scale`, as the mockup's `box-shadow: inset 0 0 0 F c, inset 0 0 Z S
/// c` composes them (F = Z / 4, S = Z / 2, Z the glow's width), at the
/// strength's alpha.
struct Shape {
    across: Vec<f64>,
    down: Vec<f64>,
    frame: f64,
    alpha: f64,
    size: (i32, i32),
}

impl Shape {
    fn new(size: Size<i32, Physical>, scale: f64, strength: Strength) -> Self {
        let (width, alpha) = strength.glow();
        let frame = (width / 4.0).round() * scale;
        let spread = (width / 2.0).round() * scale;
        // A blur radius is twice the Gaussian's deviation.
        let sigma = width * scale / 2.0;
        let line = |n: i32| -> Vec<f64> {
            (0..n)
                .map(|i| inside(f64::from(i) + 0.5, f64::from(n), spread, sigma))
                .collect()
        };
        Self {
            across: line(size.w),
            down: line(size.h),
            frame,
            alpha,
            size: (size.w, size.h),
        }
    }

    /// The opacity at pixel (`x`, `y`).
    fn at(&self, x: i32, y: i32) -> f64 {
        let (w, h) = self.size;
        let edge = f64::from(x.min(w - 1 - x).min(y).min(h - 1 - y)) + 0.5;
        let framed = if edge < self.frame { self.alpha } else { 0.0 };
        let i = |v: &[f64], n: i32| {
            v.get(usize::try_from(n).unwrap_or(0))
                .copied()
                .unwrap_or(1.0)
        };
        let shadow = self.alpha * (1.0 - i(&self.across, x) * i(&self.down, y)).clamp(0.0, 1.0);
        framed + shadow * (1.0 - framed)
    }
}

/// How deep the strips reach in: past it the light is below a level's
/// half (three deviations past the spread), at most half the screen.
fn depth(size: Size<i32, Physical>, scale: f64, strength: Strength) -> i32 {
    let (width, _) = strength.glow();
    let reach = ((width / 2.0).round() + 1.5 * width) * scale;
    (reach.ceil() as i32 + 1)
        .min(size.w / 2)
        .min(size.h / 2)
        .max(1)
}

/// The four strips along the edges of a screen `size` pixels large at
/// `scale`: the top and the bottom across it, the sides between them.
fn strips(
    size: Size<i32, Physical>,
    scale: f64,
    warmth: Warmth,
    strength: Strength,
) -> Vec<(MemoryRenderBuffer, Rectangle<i32, Physical>, Id)> {
    let shape = Shape::new(size, scale, strength);
    let t = depth(size, scale, strength);
    let (w, h) = (size.w, size.h);
    let [r, g, b] = warmth.rgb().map(f64::from);
    let places: [Rectangle<i32, Physical>; 4] = [
        Rectangle::new((0, 0).into(), (w, t).into()),
        Rectangle::new((0, h - t).into(), (w, t).into()),
        Rectangle::new((0, t).into(), (t, h - 2 * t).into()),
        Rectangle::new((w - t, t).into(), (t, h - 2 * t).into()),
    ];
    places
        .into_iter()
        .filter(|p| p.size.w > 0 && p.size.h > 0)
        .map(|place| {
            let mut pixels = Vec::with_capacity((place.size.w * place.size.h * 4) as usize);
            for y in place.loc.y..place.loc.y + place.size.h {
                for x in place.loc.x..place.loc.x + place.size.w {
                    let a = shape.at(x, y);
                    // Premultiplied, B, G, R, A in memory.
                    let byte = |v: f64| (v * a).round().clamp(0.0, 255.0) as u8;
                    pixels.extend_from_slice(&[byte(b), byte(g), byte(r), byte(255.0)]);
                }
            }
            (
                buffer(&pixels, place.size.w, place.size.h),
                place,
                Id::new(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phi_is_the_normal_distribution() {
        assert!((phi(0.0) - 0.5).abs() < 1e-7);
        assert!((phi(1.0) - 0.841_344_7).abs() < 1e-6);
        assert!((phi(-3.0) - 0.001_349_9).abs() < 1e-6);
    }

    /// The edge is the frame's colour at the strength's alpha, the light
    /// falls off inwards over about the glow's width, and the middle of
    /// the screen has none.
    #[test]
    fn the_glow_falls_off_from_the_edge() {
        let size = Size::from((1920, 1080));
        for strength in [Strength::Soft, Strength::Medium, Strength::Strong] {
            let (width, alpha) = strength.glow();
            let shape = Shape::new(size, 1.0, strength);
            // The frame at the strength's alpha, the glow under it.
            let edge = shape.at(0, 540);
            assert!(edge >= alpha && edge <= 1.0, "{strength:?}: {edge}");
            let mut last = 1.0;
            for d in [0, (width / 4.0) as i32, width as i32, (1.5 * width) as i32] {
                let a = shape.at(d, 540);
                assert!(a <= last, "{strength:?} at {d}: {a} after {last}");
                last = a;
            }
            assert!(shape.at(d(width, 2.0), 540) < 1.0 / 255.0, "{strength:?}");
            assert!(shape.at(960, 540) < 1e-6);
            // Past the strips' depth, nothing to draw.
            let t = depth(size, 1.0, strength);
            assert!(shape.at(t, 540) * 255.0 < 0.5, "{strength:?}");
        }
    }

    fn d(width: f64, times: f64) -> i32 {
        (width * times) as i32
    }

    /// The strips cover the edges without overlapping, at any scale.
    #[test]
    fn four_strips_meet_edge_to_edge() {
        for (size, scale) in [
            ((2560, 1600), 2.0),
            ((1920, 1080), 1.0),
            ((1366, 768), 1.25),
        ] {
            let size = Size::from(size);
            let strips = strips(size, scale, Warmth::Neutral, Strength::Strong);
            assert_eq!(strips.len(), 4);
            let area: i32 = strips.iter().map(|(_, r, _)| r.size.w * r.size.h).sum();
            let t = depth(size, scale, Strength::Strong);
            assert_eq!(area, 2 * size.w * t + 2 * t * (size.h - 2 * t));
            for (i, (_, a, _)) in strips.iter().enumerate() {
                for (_, b, _) in strips.iter().skip(i + 1) {
                    assert!(!a.overlaps(*b), "{a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_glow_turned_back_midway_takes_what_is_left_of_the_fade() {
        let mut glow = Glow {
            from: 0.0,
            to: 0.0,
            tween: Tween::new(Duration::ZERO, Duration::ZERO, EASE),
        };
        glow.towards(1.0, Duration::ZERO, FADE);
        assert_eq!(glow.level(Duration::ZERO), 0.0);
        assert_eq!(glow.level(FADE), 1.0);
        let half = FADE / 2;
        let at = glow.level(half);
        glow.towards(0.0, half, FADE);
        assert!((glow.level(half) - at).abs() < 1e-9);
        assert!(glow.tween.duration < FADE);
        assert_eq!(glow.level(half + glow.tween.duration), 0.0);
        // With reduced motion, at once.
        glow.towards(1.0, FADE * 2, Duration::ZERO);
        assert_eq!(glow.level(FADE * 2), 1.0);
    }
}
