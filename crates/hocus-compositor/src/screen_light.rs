//! The screen light (#619, docs/design/camera.md): while an application
//! uses the camera (a call), the screen holding its window lights the
//! user's face. Any application: the light is the screen's, the video is
//! not touched.
//!
//! The shell says who uses a camera (bar.md 5.12, `ToCompositor::Camera`):
//! their ids are matched against the windows' application ids as the
//! in-use popover matches them, the focused window first, then from the
//! front; without one of theirs, the focused screen lights. The light
//! follows that screen as the call's window moves.
//!
//! A laptop's screen is a weak light (the maintainer's try, 2026-11-18):
//! the light takes a wide part of the screen, in one of two shapes
//! (`[camera] screen-light-shape`). The frame: four strips along the
//! edges, nearly opaque at the edge and glowing inward over a part of the
//! screen's shorter side, computed once for the screen's size, the bar's
//! room, the warmth and the strength into textures of their own, on the
//! CPU as on the GPU. Around the call: all of the screen but the call's
//! window, drawn right behind it (the windows in front of it stay in
//! front), flat fills on a fixed grid and a soft edge around the window
//! (nine pieces of one small picture, the window's place among them): as
//! the window moves, only the cells along its edge change, and only they
//! are damaged. Neither covers
//! the bar's column; both are over the windows, under the shell's
//! surfaces (its popovers, the notifications), on the screen only: never
//! in a capture, a share or a recording (their `Purpose`). Locked, the
//! frame shows over the lock. The light fades in and out over 600 ms (at
//! once with reduced motion), each frame of the fade changing only the
//! pieces' alpha; steady, it asks for no frame and damages nothing.

use std::{cell::RefCell, collections::BTreeMap, time::Duration};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Id, Kind, memory::MemoryRenderBuffer, solid::SolidColorRenderElement},
    },
    output::Output,
    utils::{Buffer as BufferCoords, Logical, Physical, Rectangle, Size},
};

use crate::{
    State,
    config::{LightShape, Warmth},
    decoration::{Keyed, Placed, buffer},
    model::WindowId,
    motion::{Bezier, Tween, lerp},
    render::{OutputElement, OutputElements},
};

/// The light comes and goes in this time, as the mockup's transition.
pub const FADE: Duration = Duration::from_millis(600);
/// CSS's `ease`, the mockup's transition's curve.
const EASE: Bezier = Bezier(0.25, 0.1, 0.25, 1.0);
/// Around the call, the light rises from a part of itself at the window's
/// edge ([`SOFT_FLOOR`]) to full over this many logical pixels: the
/// window does not look cut out.
const SOFT_EDGE: f64 = 48.0;
/// How much of the light there is at the call's edge: from nothing, what
/// is behind (the wallpaper, another window's edge) would show around it
/// as a dark ring.
const SOFT_FLOOR: f64 = 0.55;
/// Around the call, the fills' grid, physical pixels: a window moving
/// changes the cells along its edge alone.
const CELL: i32 = 256;

/// The light: who uses a camera, the screens it shows on, their pictures.
#[derive(Default)]
pub struct ScreenLight {
    /// Who uses a camera, as the shell last said.
    users: Vec<hocus_link::CameraUser>,
    /// The light on each screen it shows on or leaves, by screen name.
    glows: BTreeMap<String, Glow>,
    /// Each screen's frame, as last drawn.
    pictures: RefCell<BTreeMap<String, Picture>>,
    /// Each screen's light around the call, as last drawn.
    around: RefCell<BTreeMap<String, Around>>,
}

impl std::fmt::Debug for ScreenLight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenLight")
            .field("users", &self.users)
            .field("glows", &self.glows)
            .finish_non_exhaustive()
    }
}

/// The light on a screen, going from `from` to `to` (0: none, 1: full);
/// `window`: the call's window there, as last known (it is kept as the
/// light fades out, the call gone).
#[derive(Debug, Clone, Copy)]
struct Glow {
    from: f64,
    to: f64,
    tween: Tween,
    window: Option<WindowId>,
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
            window: self.window,
        };
    }
}

/// What a screen's frame was made for: the part of it lit (its place and
/// size in pixels), the frame's width (sixteenths of a pixel), the warmth,
/// its alpha (thousandths).
type Key = ((i32, i32, i32, i32), u32, Warmth, u32);

/// A screen's four strips: their pixels, where they go, their ids (kept
/// while they do not change, so that a steady light damages nothing).
struct Picture {
    key: Key,
    strips: Vec<(MemoryRenderBuffer, Rectangle<i32, Physical>, Id)>,
}

/// What a screen's light around the call was made for: the soft edge's
/// width (pixels), the warmth, the alpha (thousandths), the part of the
/// screen lit.
type AroundKey = (i32, Warmth, u32, (i32, i32, i32, i32));

/// A screen's light around the call: the soft edge's picture and its
/// nine pieces' ids; the fills' ids by cell and part, kept so that a
/// cell the window does not touch is the same element from frame to
/// frame.
struct Around {
    key: AroundKey,
    edge: MemoryRenderBuffer,
    pieces: [Id; 9],
    fills: BTreeMap<(i32, i32, u8), Id>,
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

    /// The screen the light should be on, and the call's window when it
    /// is there: the camera's window's screen, else the focused one; none
    /// while nobody uses a camera or it is turned off.
    fn screen_light_wanted(&self) -> Option<(Output, Option<WindowId>)> {
        if !self.config.camera.screen_light || self.light.users.is_empty() {
            return None;
        }
        self.camera_window()
            .and_then(|id| Some((self.output_of(id)?, Some(id))))
            .or_else(|| self.bubbles_screen().map(|o| (o, None)))
    }

    /// Once a turn of the loop: the light heads for the screen it should
    /// be on, and leaves the others; the screens it fades on draw again,
    /// they alone.
    pub(crate) fn tick_screen_light(&mut self, now: Duration) {
        let wanted = self.screen_light_wanted();
        if wanted.is_none() && self.light.glows.is_empty() {
            return;
        }
        let (wanted, window) = match wanted {
            Some((output, window)) => (Some(output.name()), window),
            None => (None, None),
        };
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
        let mut moving = Vec::new();
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
            // The call's window there, kept as the light leaves.
            if to > 0.0 && glow.window != window {
                glow.window = window;
                moving.push(name.clone());
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
                window,
            };
            glow.towards(1.0, at, fade);
            self.light.glows.insert(name, glow);
        }
        self.light.glows.retain(|name, glow| {
            let done = glow.tween.done(now);
            if !done {
                moving.push(name.clone());
            }
            // One more frame at its end, then gone.
            !(done && glow.to == 0.0 && glow.tween.done(at))
        });
        let glows = &self.light.glows;
        let gone: Vec<String> = self
            .light
            .pictures
            .borrow()
            .keys()
            .filter(|name| !glows.contains_key(*name))
            .cloned()
            .collect();
        for name in gone {
            self.light.pictures.borrow_mut().remove(&name);
            // The frame without it.
            moving.push(name);
        }
        self.light
            .around
            .borrow_mut()
            .retain(|name, _| glows.contains_key(name));
        let screens: Vec<Output> = self
            .space
            .outputs()
            .filter(|o| moving.contains(&o.name()))
            .cloned()
            .collect();
        self.frames.moved_on.extend(screens);
    }

    /// How far the light is on `output` now, when it is.
    fn light_level(&self, output: &Output) -> Option<f64> {
        let level = self.light.glows.get(&output.name())?.level(self.now());
        (level > 0.0).then_some(level)
    }

    /// The screen's size and scale, and the part of it the light may
    /// cover, output-local pixels: all of it but the bar's column, which
    /// stays as it is.
    fn light_area(&self, output: &Output) -> Option<(Size<i32, Physical>, f64, Rect)> {
        let mode = output.current_mode()?;
        let size = output.current_transform().transform_size(mode.size);
        let scale = output.current_scale().fractional_scale();
        let mut area = Rect::new(0, 0, size.w, size.h);
        // The bar's room as it settles, not as a form's switch draws it:
        // the frame is made again once, not at each frame of the motion.
        if let Some(width) = self.bar_width(&output.name()) {
            let column = (f64::from(width + self.bar_edge()) * scale).round() as i32;
            let column = column.clamp(0, size.w);
            match self.bar_side() {
                hocus_link::Side::Right => area.w -= column,
                hocus_link::Side::Left => {
                    area.x = column;
                    area.w -= column;
                }
            }
        }
        Some((size, scale, area))
    }

    /// The call's window the light goes around on `output`: with the
    /// shape around the call, the light on there, its window shown there;
    /// not while locked or in the overview (the frame then).
    pub(crate) fn light_around(&self, output: &Output) -> Option<WindowId> {
        if self.config.camera.screen_light_shape != LightShape::AroundCall
            || self.locked()
            || self.overview_cover(self.now()).is_some()
        {
            return None;
        }
        let id = self.light.glows.get(&output.name())?.window?;
        (self.windows.contains_key(&id)
            && !self.model.is_minimized(id)
            && self.output_of(id).as_ref() == Some(output)
            && self.strip_offset(id, self.now()).is_some())
        .then_some(id)
    }

    /// The light's frame on `output`, front first: on the screen only,
    /// unless it goes around the call there.
    pub(crate) fn screen_light_frame<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(level) = self.light_level(output) else {
            return;
        };
        if self.light_around(output).is_some() {
            return;
        }
        let Some((size, _, area)) = self.light_area(output) else {
            return;
        };
        let name = output.name();
        let warmth = self.config.camera.screen_light_warmth;
        let (part, alpha) = self.screen_light_glow();
        let width = part * f64::from(size.w.min(size.h));
        let key: Key = (
            area.tuple(),
            (width * 16.0).round() as u32,
            warmth,
            (alpha * 1000.0).round() as u32,
        );
        let mut pictures = self.light.pictures.borrow_mut();
        if pictures.get(&name).is_none_or(|p| p.key != key) {
            pictures.insert(
                name.clone(),
                Picture {
                    key,
                    strips: strips(area, width, alpha, warmth),
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

    /// The light around the call's window on `output`, `visual` where the
    /// window is drawn (output-local), front first: to go right behind
    /// it.
    pub(crate) fn screen_light_around<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        visual: Rectangle<f64, Logical>,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(level) = self.light_level(output) else {
            return;
        };
        let Some((_, scale, area)) = self.light_area(output) else {
            return;
        };
        let warmth = self.config.camera.screen_light_warmth;
        let alpha = self.screen_light_glow().1;
        let edge = ((SOFT_EDGE * scale).round() as i32).max(1);
        let hole = visual.to_physical_precise_round(scale);
        let hole = Rect::new(hole.loc.x, hole.loc.y, hole.size.w, hole.size.h);
        let key: AroundKey = (edge, warmth, (alpha * 1000.0).round() as u32, area.tuple());
        let mut all = self.light.around.borrow_mut();
        let around = match all.entry(output.name()) {
            std::collections::btree_map::Entry::Occupied(mut e) => {
                if e.get().key != key {
                    e.insert(Around::new(key, edge, alpha, warmth));
                }
                e.into_mut()
            }
            std::collections::btree_map::Entry::Vacant(e) => {
                e.insert(Around::new(key, edge, alpha, warmth))
            }
        };
        // The soft edge, from the window out, and under the window (its
        // round corners show the light, not what is behind).
        for ((src, dst), id) in soft_pieces(hole, edge).into_iter().zip(&around.pieces) {
            let Some((src, dst)) = clip(src, dst, area) else {
                continue;
            };
            match Placed::new(renderer, &around.edge, src, dst.rect(), level as f32) {
                Ok(e) => out.push(OutputElements::Frame(Keyed::new(e, id.clone()))),
                Err(err) => tracing::warn!(?err, "could not draw the screen light"),
            }
        }
        // The rest of the screen, flat; its colour premultiplied, and
        // said in the commit so that a change of it draws again.
        let a = alpha * level;
        let [r, g, b] = warmth.rgb().map(|c| f64::from(c) / 255.0 * a);
        let color = [r, g, b, a].map(|c| c as f32);
        let commit = color
            .iter()
            .fold(0usize, |n, c| (n << 8) | (c * 255.0).round() as usize);
        let grown = hole.grown(edge);
        for (cell, rect) in fills(area, grown) {
            let id = around.fills.entry(cell).or_insert_with(Id::new).clone();
            out.push(OutputElements::Solid(SolidColorRenderElement::new(
                id,
                rect.rect(),
                commit,
                color,
                Kind::Unspecified,
            )));
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

impl Around {
    fn new(key: AroundKey, edge: i32, alpha: f64, warmth: Warmth) -> Self {
        Self {
            key,
            edge: soft_edge(edge, alpha, warmth),
            pieces: std::array::from_fn(|_| Id::new()),
            fills: BTreeMap::new(),
        }
    }
}

/// A rectangle of whole pixels, output-local: the light's arithmetic,
/// edges inclusive at the start and exclusive at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Rect {
    const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    const fn from_edges(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self::new(x0, y0, x1 - x0, y1 - y0)
    }

    const fn right(self) -> i32 {
        self.x + self.w
    }

    const fn bottom(self) -> i32 {
        self.y + self.h
    }

    const fn empty(self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    const fn tuple(self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.w, self.h)
    }

    const fn grown(self, by: i32) -> Self {
        Self::new(self.x - by, self.y - by, self.w + 2 * by, self.h + 2 * by)
    }

    fn overlaps(self, other: Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    fn intersection(self, other: Self) -> Option<Self> {
        self.overlaps(other).then(|| {
            Self::from_edges(
                self.x.max(other.x),
                self.y.max(other.y),
                self.right().min(other.right()),
                self.bottom().min(other.bottom()),
            )
        })
    }

    fn rect(self) -> Rectangle<i32, Physical> {
        Rectangle::new((self.x, self.y).into(), (self.w, self.h).into())
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

/// The frame's opacity of each pixel of a part of the screen `size`
/// pixels large, as an inset `box-shadow: inset 0 0 Z W c` composes it
/// (W the frame's width, spread over all of it, blurred over two thirds
/// of it, Z = 2 W / 3), at the strength's alpha: nearly the alpha at the
/// edge, half of it at W, nothing past twice W.
struct Shape {
    across: Vec<f64>,
    down: Vec<f64>,
    alpha: f64,
}

impl Shape {
    fn new(size: (i32, i32), width: f64, alpha: f64) -> Self {
        // A blur radius is twice the Gaussian's deviation.
        let sigma = (width / 3.0).max(0.5);
        let line = |n: i32| -> Vec<f64> {
            (0..n)
                .map(|i| inside(f64::from(i) + 0.5, f64::from(n), width, sigma))
                .collect()
        };
        Self {
            across: line(size.0),
            down: line(size.1),
            alpha,
        }
    }

    /// The opacity at pixel (`x`, `y`) of the part lit.
    fn at(&self, x: i32, y: i32) -> f64 {
        let i = |v: &[f64], n: i32| {
            v.get(usize::try_from(n).unwrap_or(0))
                .copied()
                .unwrap_or(1.0)
        };
        self.alpha * (1.0 - i(&self.across, x) * i(&self.down, y)).clamp(0.0, 1.0)
    }
}

/// How deep the strips reach in: past it the light is below a level's
/// half (three deviations past the width), at most half the part lit.
fn depth(size: (i32, i32), width: f64) -> i32 {
    ((2.0 * width).ceil() as i32 + 1)
        .min(size.0 / 2)
        .min(size.1 / 2)
        .max(1)
}

/// The four strips along the edges of `area` (output-local pixels) for a
/// frame `width` pixels wide at `alpha`: the top and the bottom across it,
/// the sides between them.
fn strips(
    area: Rect,
    width: f64,
    alpha: f64,
    warmth: Warmth,
) -> Vec<(MemoryRenderBuffer, Rectangle<i32, Physical>, Id)> {
    let size = (area.w, area.h);
    let shape = Shape::new(size, width, alpha);
    let t = depth(size, width);
    let (w, h) = size;
    let [r, g, b] = warmth.rgb().map(f64::from);
    let places: [Rect; 4] = [
        Rect::new(0, 0, w, t),
        Rect::new(0, h - t, w, t),
        Rect::new(0, t, t, h - 2 * t),
        Rect::new(w - t, t, t, h - 2 * t),
    ];
    places
        .into_iter()
        .filter(|p| !p.empty())
        .map(|place| {
            let mut pixels = Vec::with_capacity((place.w * place.h * 4) as usize);
            for y in place.y..place.bottom() {
                for x in place.x..place.right() {
                    let a = shape.at(x, y);
                    // Premultiplied, B, G, R, A in memory.
                    let byte = |v: f64| (v * a).round().clamp(0.0, 255.0) as u8;
                    pixels.extend_from_slice(&[byte(b), byte(g), byte(r), byte(255.0)]);
                }
            }
            let on_screen = Rect::new(area.x + place.x, area.y + place.y, place.w, place.h);
            (
                buffer(&pixels, place.w, place.h),
                on_screen.rect(),
                Id::new(),
            )
        })
        .collect()
}

/// The soft edge's opacity at `d` pixels from the window, `edge` pixels
/// wide: from [`SOFT_FLOOR`] to full, smoothly.
fn soft(d: f64, edge: f64) -> f64 {
    let t = (d / edge).clamp(0.0, 1.0);
    SOFT_FLOOR + (1.0 - SOFT_FLOOR) * t * t * (3.0 - 2.0 * t)
}

/// The soft edge's picture: a window three pixels wide in its middle,
/// `edge` pixels of light rising around it at `alpha`; its corners drawn
/// as they are, its sides stretched along the window's.
fn soft_edge(edge: i32, alpha: f64, warmth: Warmth) -> MemoryRenderBuffer {
    let n = 2 * edge + 3;
    let far = edge + 3;
    let from = |i: i32| -> f64 {
        if i < edge {
            f64::from(edge - i) - 0.5
        } else if i >= far {
            f64::from(i - far) + 0.5
        } else {
            0.0
        }
    };
    let [r, g, b] = warmth.rgb().map(f64::from);
    let mut pixels = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let a = alpha * soft(from(x).hypot(from(y)), f64::from(edge));
            let byte = |v: f64| (v * a).round().clamp(0.0, 255.0) as u8;
            pixels.extend_from_slice(&[byte(b), byte(g), byte(r), byte(255.0)]);
        }
    }
    buffer(&pixels, n, n)
}

/// The soft edge's nine pieces around `hole` (the window, output-local
/// pixels), `edge` wide: each a part of [`soft_edge`]'s picture and where
/// it goes. The corners, the sides, then the window's own place.
fn soft_pieces(hole: Rect, edge: i32) -> [(Rectangle<f64, BufferCoords>, Rect); 9] {
    let e = edge;
    let far = e + 3;
    let src = |x: i32, y: i32, w: i32, h: i32| -> Rectangle<f64, BufferCoords> {
        Rectangle::new(
            (f64::from(x), f64::from(y)).into(),
            (f64::from(w), f64::from(h)).into(),
        )
    };
    let (x0, y0, x1, y1) = (hole.x, hole.y, hole.right(), hole.bottom());
    [
        (src(0, 0, e, e), Rect::new(x0 - e, y0 - e, e, e)),
        (src(far, 0, e, e), Rect::new(x1, y0 - e, e, e)),
        (src(0, far, e, e), Rect::new(x0 - e, y1, e, e)),
        (src(far, far, e, e), Rect::new(x1, y1, e, e)),
        (src(e + 1, 0, 1, e), Rect::new(x0, y0 - e, hole.w, e)),
        (src(e + 1, far, 1, e), Rect::new(x0, y1, hole.w, e)),
        (src(0, e + 1, e, 1), Rect::new(x0 - e, y0, e, hole.h)),
        (src(far, e + 1, e, 1), Rect::new(x1, y0, e, hole.h)),
        (src(e + 1, e + 1, 1, 1), hole),
    ]
}

/// `src` drawn on `dst`, what of it falls in `area`.
fn clip(
    src: Rectangle<f64, BufferCoords>,
    dst: Rect,
    area: Rect,
) -> Option<(Rectangle<f64, BufferCoords>, Rect)> {
    let kept = dst.intersection(area)?;
    let kx = src.size.w / f64::from(dst.w);
    let ky = src.size.h / f64::from(dst.h);
    let src = Rectangle::new(
        (
            src.loc.x + f64::from(kept.x - dst.x) * kx,
            src.loc.y + f64::from(kept.y - dst.y) * ky,
        )
            .into(),
        (f64::from(kept.w) * kx, f64::from(kept.h) * ky).into(),
    );
    Some((src, kept))
}

/// The flat light: `area` but `grown` (the window and its soft edge), cut
/// along a fixed grid, each piece with its cell and part (0: the whole
/// cell; 1 to 4: above, below, left of and right of `grown`).
fn fills(area: Rect, grown: Rect) -> Vec<((i32, i32, u8), Rect)> {
    let mut out = Vec::new();
    let columns = (area.w + CELL - 1) / CELL;
    let rows = (area.h + CELL - 1) / CELL;
    for cy in 0..rows {
        for cx in 0..columns {
            let Some(cell) =
                Rect::new(area.x + cx * CELL, area.y + cy * CELL, CELL, CELL).intersection(area)
            else {
                continue;
            };
            if !cell.overlaps(grown) {
                out.push(((cx, cy, 0), cell));
                continue;
            }
            let (top, bottom) = (grown.y.max(cell.y), grown.bottom().min(cell.bottom()));
            let parts = [
                Rect::from_edges(cell.x, cell.y, cell.right(), top),
                Rect::from_edges(cell.x, bottom, cell.right(), cell.bottom()),
                Rect::from_edges(cell.x, top, grown.x.min(cell.right()), bottom),
                Rect::from_edges(grown.right().max(cell.x), top, cell.right(), bottom),
            ];
            for (part, rect) in (1..).zip(parts) {
                if !rect.empty() {
                    out.push(((cx, cy, part), rect));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Strength;
    use anyhow::Context;

    #[test]
    fn phi_is_the_normal_distribution() {
        assert!((phi(0.0) - 0.5).abs() < 1e-7);
        assert!((phi(1.0) - 0.841_344_7).abs() < 1e-6);
        assert!((phi(-3.0) - 0.001_349_9).abs() < 1e-6);
    }

    /// The edge is nearly the strength's alpha, the light holds over half
    /// the frame's width, is half at its width and falls off inwards; the
    /// middle of the screen has none. Strong is an eighth of the shorter
    /// side, its light reaching about 200 px into a 1080 p screen.
    #[test]
    fn the_frame_is_wide_and_glows_inward() {
        let size = (1920, 1080);
        let mut widths = Vec::new();
        for strength in [Strength::Soft, Strength::Medium, Strength::Strong] {
            let (part, alpha) = strength.glow();
            let width = part * 1080.0;
            widths.push(width);
            let shape = Shape::new(size, width, alpha);
            let at = |d: f64| shape.at(d as i32, 540);
            assert!(at(0.0) > 0.99 * alpha, "{strength:?}: {}", at(0.0));
            assert!(at(width / 2.0) > 0.9 * alpha, "{strength:?}");
            assert!((at(width) - alpha / 2.0).abs() < 0.03, "{strength:?}");
            let mut last = 1.0;
            for d in [0.0, width / 2.0, width, 1.5 * width, 2.0 * width] {
                assert!(at(d) <= last, "{strength:?} at {d}: {} after {last}", at(d));
                last = at(d);
            }
            assert!(shape.at(960, 540) < 1e-6);
            // Past the strips' depth, nothing to draw.
            let t = depth(size, width);
            assert!(shape.at(t, 540) * 255.0 < 0.5, "{strength:?}");
        }
        assert_eq!(Strength::Strong.glow().1, 0.97);
        assert!((widths[2] - 135.0).abs() < 1e-9);
        let shape = Shape::new(size, widths[2], 0.97);
        assert!(shape.at(190, 540) > 0.1 && shape.at(230, 540) < 0.02);
        assert!(widths[0] < widths[1] && widths[1] < widths[2]);
    }

    /// The strips cover the edges of the part lit without overlapping, at
    /// any size, the bar's column left out.
    #[test]
    fn four_strips_meet_edge_to_edge() {
        for (area, width) in [
            (Rect::new(0, 0, 2560, 1600), 200.0),
            (Rect::new(0, 0, 1920 - 80, 1080), 135.0),
            (Rect::new(100, 0, 1266, 768), 48.0),
        ] {
            let strips = strips(area, width, 0.97, Warmth::Neutral);
            assert_eq!(strips.len(), 4);
            let sum: i32 = strips.iter().map(|(_, r, _)| r.size.w * r.size.h).sum();
            let t = depth((area.w, area.h), width);
            assert_eq!(sum, 2 * area.w * t + 2 * t * (area.h - 2 * t));
            for (i, (_, a, _)) in strips.iter().enumerate() {
                assert!(area.rect().contains_rect(*a), "{a:?} out of {area:?}");
                for (_, b, _) in strips.iter().skip(i + 1) {
                    assert!(!a.overlaps(*b), "{a:?} {b:?}");
                }
            }
        }
    }

    /// Around the call: the fills and the soft edge tile the part lit,
    /// the window's place last, without overlapping; a cell away from
    /// the window stays the same piece as it moves.
    #[test]
    fn around_the_call_the_light_is_everywhere_but_the_window() -> anyhow::Result<()> {
        let area = Rect::new(0, 0, 1840, 1080);
        let edge = 48;
        for hole in [
            Rect::new(400, 200, 800, 600),
            Rect::new(1500, -20, 600, 400),
            Rect::new(0, 0, 1840, 1080),
        ] {
            let grown = hole.grown(edge);
            let mut pieces: Vec<Rect> = fills(area, grown).into_iter().map(|(_, r)| r).collect();
            pieces.extend(
                soft_pieces(hole, edge)
                    .into_iter()
                    .filter_map(|(src, dst)| clip(src, dst, area).map(|(_, d)| d)),
            );
            let under = pieces.pop().context("no piece under the window")?;
            assert_eq!(Some(under), hole.intersection(area));
            for (i, a) in pieces.iter().enumerate() {
                assert!(!a.empty() && a.intersection(area) == Some(*a), "{a:?}");
                assert!(!a.overlaps(hole), "{a:?} on the window {hole:?}");
                for b in pieces.iter().skip(i + 1) {
                    assert!(!a.overlaps(*b), "{a:?} {b:?}");
                }
            }
            let covered: i64 = pieces.iter().map(|r| i64::from(r.w) * i64::from(r.h)).sum();
            let window = i64::from(under.w) * i64::from(under.h);
            assert_eq!(covered + window, i64::from(area.w) * i64::from(area.h));
        }
        let before = fills(area, Rect::new(400, 200, 800, 600).grown(edge));
        let after = fills(area, Rect::new(410, 200, 800, 600).grown(edge));
        let far = |f: &[((i32, i32, u8), Rect)]| f.iter().find(|(k, _)| *k == (7, 4, 0)).copied();
        assert!(far(&before).is_some() && far(&before) == far(&after));
        Ok(())
    }

    /// The soft edge: a part of the light at the window, full `edge`
    /// pixels out; a stretched side's part clipped in proportion.
    #[test]
    fn the_soft_edge_rises_from_the_window() {
        assert_eq!(soft(0.0, 48.0), SOFT_FLOOR);
        assert!((soft(24.0, 48.0) - (1.0 + SOFT_FLOOR) / 2.0).abs() < 1e-9);
        assert_eq!(soft(48.0, 48.0), 1.0);
        assert_eq!(soft(100.0, 48.0), 1.0);
        let src = Rectangle::new((49.0, 0.0).into(), (1.0, 48.0).into());
        let (s, d) = clip(
            src,
            Rect::new(1700, 100, 400, 48),
            Rect::new(0, 0, 1840, 1080),
        )
        .expect("in");
        assert_eq!(d, Rect::new(1700, 100, 140, 48));
        assert!((s.size.w - 0.35).abs() < 1e-9 && (s.size.h - 48.0).abs() < 1e-9);
    }

    #[test]
    fn a_glow_turned_back_midway_takes_what_is_left_of_the_fade() {
        let mut glow = Glow {
            from: 0.0,
            to: 0.0,
            tween: Tween::new(Duration::ZERO, Duration::ZERO, EASE),
            window: None,
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
