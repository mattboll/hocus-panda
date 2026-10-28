//! A window's look (docs/design/windows.md §3, §7, §9): rounded corners, a
//! rim just outside its content (the accent when focused), a soft shadow,
//! the grip of windows with no header of their own, and the preview of
//! where a dragged window snaps.
//!
//! Everything here works the same with every renderer, pixman included, so
//! that headless tests see it: corners are cut by leaving their outside out
//! of what the window draws, and the shadow, the rim, the grip and the
//! preview are textures drawn once per scale and stretched in nine slices.
//! Ported from the first build's `decoration.rs`, with the slice's values.

use std::{cell::RefCell, collections::BTreeMap};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportMem, Renderer,
            element::{
                Element, Id, Kind, RenderElement, UnderlyingStorage,
                memory::{MemoryBuffer, MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
            utils::{CommitCounter, DamageSet, OpaqueRegions},
        },
    },
    utils::{Buffer as BufferCoords, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

/// Corner radius, logical pixels.
pub const RADIUS: f64 = 8.0;
/// The accent (`#6CC287`) and the unfocused rim, white at 18 %.
pub const ACCENT: [u8; 4] = [0x6C, 0xC2, 0x87, 0xFF];
pub const RIM: [u8; 4] = [0xFF, 0xFF, 0xFF, 46];
/// The grip: a pill 44×4, radius 2, light at 75 %, 4 px inside the top
/// edge (§7).
pub const GRIP_SIZE: (f64, f64) = (44.0, 4.0);
pub const GRIP_INSET: f64 = 4.0;
const GRIP_COLOUR: [u8; 4] = [0xEE, 0xF1, 0xF6, 191];
/// The handle on the seam of a snapped pair (§8): the grip's pill, along
/// the seam, at its middle.
pub const HANDLE_LENGTH: f64 = GRIP_SIZE.0;
pub const HANDLE_THICKNESS: f64 = GRIP_SIZE.1;
/// The snap preview: the accent at 12 % with a 1 px accent rim.
const PREVIEW_FILL: [u8; 4] = [0x6C, 0xC2, 0x87, 31];

/// A shadow: offset down, blur (CSS's: twice the deviation), darkest
/// alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowStyle {
    pub dy: f64,
    pub blur: f64,
    pub alpha: f32,
}

impl ShadowStyle {
    pub const UNFOCUSED: Self = Self {
        dy: 12.0,
        blur: 36.0,
        alpha: 0.35,
    };
    pub const FOCUSED: Self = Self {
        dy: 12.0,
        blur: 36.0,
        alpha: 0.45,
    };
    /// Deeper while the window moves (§6); the offset is the prototype's.
    pub const MOVING: Self = Self {
        dy: 18.0,
        blur: 48.0,
        alpha: 0.5,
    };
}

/// The parts of `rect` outside its rounded corners, one rectangle per pixel
/// row of each corner.
pub fn corner_cutouts(
    rect: Rectangle<i32, Physical>,
    radius: i32,
) -> Vec<Rectangle<i32, Physical>> {
    let radius = radius.min(rect.size.w / 2).min(rect.size.h / 2);
    if radius <= 0 {
        return Vec::new();
    }
    let mut cuts = Vec::with_capacity(4 * radius as usize);
    for (row, inset) in corner_insets(radius) {
        let (left, right) = (rect.loc.x, rect.loc.x + rect.size.w - inset);
        let (top, bottom) = (rect.loc.y + row, rect.loc.y + rect.size.h - 1 - row);
        for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
            cuts.push(Rectangle::new((x, y).into(), (inset, 1).into()));
        }
    }
    cuts
}

/// Each row of a corner rounded to `radius`, from the outer one, and how
/// far in from the side its edge is; rows the curve does not reach left out.
fn corner_insets(radius: i32) -> impl Iterator<Item = (i32, i32)> {
    let r = f64::from(radius);
    (0..radius.max(0)).filter_map(move |row| {
        let dy = r - f64::from(row) - 0.5;
        let inset = (r - (r * r - dy * dy).max(0.0).sqrt()).round() as i32;
        (inset > 0).then_some((row, inset))
    })
}

/// A window's element with the outside of the window's rounded corners left
/// out: those pixels show what is behind.
#[derive(Debug)]
pub struct Rounded<E> {
    inner: E,
    /// The window's rectangle and the corner radius, physical, in the same
    /// space as the element's geometry.
    clip: Rectangle<i32, Physical>,
    radius: i32,
}

impl<E> Rounded<E> {
    pub fn new(inner: E, clip: Rectangle<i32, Physical>, radius: i32) -> Self {
        Self {
            inner,
            clip,
            radius,
        }
    }
}

impl<E: Element> Rounded<E> {
    /// The corner cutouts in the element's own coordinates.
    fn local_cutouts(&self, scale: Scale<f64>) -> Vec<Rectangle<i32, Physical>> {
        let origin = self.inner.geometry(scale).loc;
        corner_cutouts(self.clip, self.radius)
            .into_iter()
            .map(|mut r| {
                r.loc -= origin;
                r
            })
            .collect()
    }
}

impl<E: Element> Element for Rounded<E> {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.inner.src()
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        let cutouts = self.local_cutouts(scale);
        if cutouts.is_empty() {
            return self.inner.opaque_regions(scale);
        }
        let opaque: Vec<_> = self.inner.opaque_regions(scale).iter().copied().collect();
        Rectangle::subtract_rects_many(opaque, cutouts)
            .into_iter()
            .collect()
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R: Renderer, E: RenderElement<R>> RenderElement<R> for Rounded<E> {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        // Damage is relative to `dst`.
        let cutouts: Vec<_> = corner_cutouts(self.clip, self.radius)
            .into_iter()
            .map(|mut r| {
                r.loc -= dst.loc;
                r
            })
            .collect();
        if cutouts.is_empty() {
            return self.inner.draw(frame, src, dst, damage, opaque_regions);
        }
        let damage = Rectangle::subtract_rects_many(damage.iter().copied(), cutouts);
        if damage.is_empty() {
            return Ok(());
        }
        self.inner.draw(frame, src, dst, &damage, opaque_regions)
    }

    fn underlying_storage(&self, _renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        // Scanned out directly, it would lose its corners.
        None
    }
}

/// An element under an id of its own: the pieces of a window's frame
/// share one texture, and each needs its own id for the damage it leaves
/// when it moves.
#[derive(Debug)]
pub struct Keyed<E> {
    inner: E,
    id: Id,
}

impl<E> Keyed<E> {
    pub fn new(inner: E, id: Id) -> Self {
        Self { inner, id }
    }
}

impl<E: Element> Element for Keyed<E> {
    fn id(&self) -> &Id {
        &self.id
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }
    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }
    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.inner.src()
    }
    fn transform(&self) -> Transform {
        self.inner.transform()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }
    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.inner.opaque_regions(scale)
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R: Renderer, E: RenderElement<R>> RenderElement<R> for Keyed<E> {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        self.inner.draw(frame, src, dst, damage, opaque_regions)
    }
}

/// Ids for the pieces of one window's frame, made once with the window.
#[derive(Debug)]
pub struct FrameIds {
    rim: [Id; 8],
    accent: [Id; 8],
    pulse: [Id; 8],
    shadow: [Id; 9],
    grip: Id,
}

impl Default for FrameIds {
    fn default() -> Self {
        Self {
            rim: std::array::from_fn(|_| Id::new()),
            accent: std::array::from_fn(|_| Id::new()),
            pulse: std::array::from_fn(|_| Id::new()),
            shadow: std::array::from_fn(|_| Id::new()),
            grip: Id::new(),
        }
    }
}

/// Which of a window's rims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RimKind {
    /// Unfocused.
    Plain,
    /// The focused window's.
    Accent,
    /// A new window that did not take the focus, once.
    Pulse,
}

/// What a nine-slice texture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Piece {
    /// A rim of this colour and width (thousandths of a logical pixel).
    Rim([u8; 4], u32),
    Grip,
    /// The grip's pill upright, for a seam that runs down.
    Handle,
    Preview,
    Box(BoxStyle),
}

/// A rounded box: its fill and a rim inside its edge, as RGBA; widths and
/// the radius in thousandths of a logical pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BoxStyle {
    pub fill: [u8; 4],
    pub rim: [u8; 4],
    pub rim_width: u32,
    pub radius: u32,
}

/// How many shadows' textures are kept.
const SHADOWS_KEPT: usize = 16;

/// Textures drawn once per piece and scale; shadows once per scale, blur,
/// and size for windows too small for the shared one.
#[derive(Debug, Default)]
pub struct Textures {
    pieces: RefCell<BTreeMap<(Piece, u32), (MemoryRenderBuffer, i32)>>,
    shadows: RefCell<Vec<(ShadowKey, Shadow, u64)>>,
    uses: RefCell<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShadowKey {
    scale: u32,
    w: Span,
    h: Span,
    blur: u32,
}

impl Textures {
    fn get(&self, piece: Piece, scale: f64) -> (MemoryRenderBuffer, i32) {
        let key = (piece, (scale * 1000.0).round() as u32);
        let draw = || match piece {
            Piece::Rim(rgba, width) => rim_texture(rgba, f64::from(width) / 1000.0, scale),
            Piece::Grip => pill_texture(GRIP_SIZE, scale),
            Piece::Handle => pill_texture((GRIP_SIZE.1, GRIP_SIZE.0), scale),
            Piece::Preview => preview_texture(scale),
            Piece::Box(style) => box_texture(style, scale),
        };
        self.pieces
            .borrow_mut()
            .entry(key)
            .or_insert_with(draw)
            .clone()
    }

    /// The shadow of a window `size` physical pixels large.
    fn shadow(&self, blur: f64, scale: f64, size: Size<i32, Physical>) -> Shadow {
        let reach = shadow_reach(shadow_sigma(blur, scale));
        let key = ShadowKey {
            scale: (scale * 1000.0).round() as u32,
            w: Span::new(size.w, reach),
            h: Span::new(size.h, reach),
            blur: (blur.max(0.0) * 1000.0).round() as u32,
        };
        let now = {
            let mut uses = self.uses.borrow_mut();
            *uses += 1;
            *uses
        };
        let mut shadows = self.shadows.borrow_mut();
        if let Some((_, shadow, used)) = shadows.iter_mut().find(|(k, _, _)| *k == key) {
            *used = now;
            return shadow.clone();
        }
        let shadow = shadow_texture(blur, scale, key.w, key.h);
        if shadows.len() >= SHADOWS_KEPT
            && let Some(oldest) = (0..shadows.len()).min_by_key(|&i| shadows[i].2)
        {
            shadows.swap_remove(oldest);
        }
        shadows.push((key, shadow.clone(), now));
        shadow
    }
}

/// Premultiplied ARGB8888 pixels, as B, G, R, A bytes.
fn pixel(rgba: [u8; 4], coverage: f64) -> [u8; 4] {
    let a = coverage.clamp(0.0, 1.0) * f64::from(rgba[3]) / 255.0;
    let c = |v: u8| (f64::from(v) * a).round() as u8;
    [
        c(rgba[2]),
        c(rgba[1]),
        c(rgba[0]),
        (a * 255.0).round() as u8,
    ]
}

/// Coverage of a rounded square of side `side` and radius `r`, inset by
/// `inset`, at pixel centre `(x, y)`; antialiased over one pixel.
fn rounded_coverage(x: f64, y: f64, side: f64, inset: f64, r: f64) -> f64 {
    rounded_rect_coverage(x, y, (side, side), inset, r)
}

fn rounded_rect_coverage(x: f64, y: f64, size: (f64, f64), inset: f64, r: f64) -> f64 {
    let (hw, hh) = (size.0 / 2.0 - inset, size.1 / 2.0 - inset);
    let r = r.min(hw).min(hh).max(0.0);
    // Signed distance to a rounded rectangle: negative inside.
    let qx = (x - size.0 / 2.0).abs() - (hw - r);
    let qy = (y - size.1 / 2.0).abs() - (hh - r);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let d = outside + qx.max(qy).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// The shadow's deviation, physical pixels: CSS's blur radius is twice
/// the standard deviation of its Gaussian.
fn shadow_sigma(blur: f64, scale: f64) -> f64 {
    (blur * scale / 2.0).max(0.5)
}

/// How far a shadow reaches past the edge of what casts it, and as far in.
fn shadow_reach(sigma: f64) -> i32 {
    (2.5 * sigma).ceil() as i32
}

/// One way across a shadow's texture, physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    /// The window's side the texture is drawn for: its own when short, or
    /// one just long enough for the middle to be the shadow at its darkest,
    /// to stretch over any longer side.
    len: i32,
    reach: i32,
}

impl Span {
    fn new(len: i32, reach: i32) -> Self {
        Self {
            len: len.clamp(0, 2 * reach + 1),
            reach,
        }
    }

    fn corner(self) -> i32 {
        self.reach + self.len / 2
    }

    fn size(self) -> i32 {
        self.len + 2 * self.reach
    }

    fn middle(self) -> i32 {
        self.size() - 2 * self.corner()
    }

    fn half(self) -> i32 {
        self.corner() + 2 * self.middle()
    }

    /// How much of the shadow there is at each texel, 0 to 1, and how much
    /// a rounded corner's missing bit at each end takes away from it.
    fn profile(self, sigma: f64, radius: f64) -> (Vec<f64>, Vec<f64>) {
        let k = f64::from(self.reach) / sigma;
        let tail = normal_cdf(-k);
        let cdf = |z: f64| ((normal_cdf(z) - tail) / (1.0 - 2.0 * tail)).clamp(0.0, 1.0);
        let inset =
            radius * (10.0 - 3.0 * std::f64::consts::PI) / (12.0 - 3.0 * std::f64::consts::PI);
        let len = f64::from(self.len);
        let gauss = |d: f64| {
            (-d * d / (2.0 * sigma * sigma)).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt())
        };
        (0..self.half())
            .map(|t| {
                let u = f64::from(t) + 0.5 - f64::from(self.reach);
                let cover = cdf(u / sigma) - cdf((u - len) / sigma);
                let corners = gauss(u - inset) + gauss(u - (len - inset));
                (cover, corners)
            })
            .unzip()
    }
}

/// The standard normal distribution's cumulative function (Abramowitz and
/// Stegun 7.1.26, to 1.5e-7).
fn normal_cdf(z: f64) -> f64 {
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    if z < 0.0 {
        0.5 * (1.0 - erf)
    } else {
        0.5 * (1.0 + erf)
    }
}

/// A window's shadow drawn for its size at the output's pixels, at its
/// darkest fully opaque (the element's alpha sets how dark): its top left
/// quarter, the other quarters being the same pixels mirrored.
#[derive(Debug, Clone)]
struct Shadow {
    quarters: [MemoryRenderBuffer; 4],
    w: Span,
    h: Span,
}

impl Shadow {
    fn quarter(&self, mirrored_x: bool, mirrored_y: bool) -> &MemoryRenderBuffer {
        &self.quarters[usize::from(mirrored_x) + 2 * usize::from(mirrored_y)]
    }
}

fn shadow_texture(blur: f64, scale: f64, w: Span, h: Span) -> Shadow {
    let pixels = MemoryBuffer::from_slice(
        &shadow_pixels(blur, scale, w, h),
        Fourcc::Argb8888,
        (w.half(), h.half()),
    );
    let quarters = [
        Transform::Normal,
        Transform::Flipped,
        Transform::Flipped180,
        Transform::_180,
    ]
    .map(|t| MemoryRenderBuffer::from_memory(pixels.clone(), 1, t, None));
    Shadow { quarters, w, h }
}

fn shadow_pixels(blur: f64, scale: f64, w: Span, h: Span) -> Vec<u8> {
    let sigma = shadow_sigma(blur, scale);
    let radius = RADIUS * scale;
    let (cover_x, corners_x) = w.profile(sigma, radius);
    let (cover_y, corners_y) = h.profile(sigma, radius);
    let missing = radius * radius * (1.0 - std::f64::consts::FRAC_PI_4);
    let mut pixels = vec![0; cover_x.len() * cover_y.len() * 4];
    let rows = pixels.chunks_exact_mut(cover_x.len().max(1) * 4);
    for ((cy, ky), row) in cover_y.iter().zip(&corners_y).zip(rows) {
        for ((cx, kx), px) in cover_x
            .iter()
            .zip(&corners_x)
            .zip(row.as_chunks_mut::<4>().0)
        {
            let a = (cx * cy - missing * kx * ky).clamp(0.0, 1.0);
            px[3] = (a * 255.0).round() as u8;
        }
    }
    pixels
}

/// A square holding a rounded rim `width` wide hugging the window from
/// outside: the window's content keeps every pixel.
fn rim_texture(rgba: [u8; 4], width: f64, scale: f64) -> (MemoryRenderBuffer, i32) {
    let width = width * scale;
    let r = RADIUS * scale;
    let corner = (width + r).ceil() as i32 + 1;
    let side = 2 * corner + 1;
    let n = side as usize;
    // The window's edge is `corner - r` in from the sides.
    let edge = f64::from(corner) - r;
    let pixels: Vec<u8> = (0..n * n)
        .flat_map(|i| {
            let (x, y) = ((i % n) as f64 + 0.5, (i / n) as f64 + 0.5);
            let outer = rounded_coverage(x, y, f64::from(side), edge - width, r + width);
            let inner = rounded_coverage(x, y, f64::from(side), edge, r);
            pixel(rgba, outer - inner)
        })
        .collect();
    (buffer(&pixels, side, side), corner)
}

/// The grip's pill, drawn whole, `size` logical pixels.
fn pill_texture(size: (f64, f64), scale: f64) -> (MemoryRenderBuffer, i32) {
    let w = (size.0 * scale).round().max(1.0);
    let h = (size.1 * scale).round().max(1.0);
    let (nw, nh) = (w as usize, h as usize);
    let pixels: Vec<u8> = (0..nw * nh)
        .flat_map(|i| {
            let (x, y) = ((i % nw) as f64 + 0.5, (i / nw) as f64 + 0.5);
            pixel(
                GRIP_COLOUR,
                rounded_rect_coverage(x, y, (w, h), 0.0, w.min(h) / 2.0),
            )
        })
        .collect();
    (buffer(&pixels, nw as i32, nh as i32), 0)
}

/// A square holding the preview's rounded box: its fill and its rim,
/// inside its edge.
fn preview_texture(scale: f64) -> (MemoryRenderBuffer, i32) {
    let r = RADIUS * scale;
    let width = scale.max(1.0);
    let corner = r.ceil() as i32 + 1;
    let side = 2 * corner + 1;
    let n = side as usize;
    let pixels: Vec<u8> = (0..n * n)
        .flat_map(|i| {
            let (x, y) = ((i % n) as f64 + 0.5, (i / n) as f64 + 0.5);
            let outer = rounded_coverage(x, y, f64::from(side), 0.0, r);
            let inner = rounded_coverage(x, y, f64::from(side), width, (r - width).max(0.0));
            // The rim over the fill, premultiplied.
            let fill = pixel(PREVIEW_FILL, inner);
            let rim = pixel(ACCENT, (outer - inner).max(0.0));
            let mut out = [0u8; 4];
            for k in 0..4 {
                out[k] = rim[k].saturating_add(fill[k]);
            }
            out
        })
        .collect();
    (buffer(&pixels, side, side), corner)
}

/// A square holding a rounded box's corners: its fill, and its rim
/// inside its edge. Its corners are exactly the radius, so that a box as
/// small as twice its radius (a pill) keeps its shape.
fn box_texture(style: BoxStyle, scale: f64) -> (MemoryRenderBuffer, i32) {
    let r = f64::from(style.radius) / 1000.0 * scale;
    let width = f64::from(style.rim_width) / 1000.0 * scale;
    let corner = (r.ceil() as i32).max(1);
    let side = 2 * corner + 1;
    let n = side as usize;
    let pixels: Vec<u8> = (0..n * n)
        .flat_map(|i| {
            let (x, y) = ((i % n) as f64 + 0.5, (i / n) as f64 + 0.5);
            let outer = rounded_coverage(x, y, f64::from(side), 0.0, r);
            let inner = if width > 0.0 {
                rounded_coverage(x, y, f64::from(side), width, (r - width).max(0.0))
            } else {
                outer
            };
            let fill = pixel(style.fill, inner);
            let rim = pixel(style.rim, (outer - inner).max(0.0));
            let mut out = [0u8; 4];
            for k in 0..4 {
                out[k] = rim[k].saturating_add(fill[k]);
            }
            out
        })
        .collect();
    (buffer(&pixels, side, side), corner)
}

/// A rounded box at `rect` (logical, output-local), `alpha` opaque.
pub fn box_elements<R>(
    renderer: &mut R,
    textures: &Textures,
    ids: &[Id; 9],
    style: BoxStyle,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<FramePiece<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    if alpha <= 0.0 || rect.size.w <= 0.0 || rect.size.h <= 0.0 {
        return Vec::new();
    }
    let (texture, corner) = textures.get(Piece::Box(style), scale);
    nine_slice(renderer, &texture, corner, 0.0, rect, scale, alpha, true)
        .into_iter()
        .zip(ids.iter())
        .map(|(e, id)| Keyed::new(e, id.clone()))
        .collect()
}

pub(crate) fn buffer(pixels: &[u8], w: i32, h: i32) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(pixels, Fourcc::Argb8888, (w, h), 1, Transform::Normal, None)
}

/// A frame piece: a texture's part drawn on the output.
pub type FramePiece<R> = Keyed<MemoryRenderBufferRenderElement<R>>;

/// A window's rim at `rect` (logical, output-local), `alpha` opaque, its
/// width in logical pixels.
#[allow(clippy::too_many_arguments)]
pub fn rim_elements<R>(
    renderer: &mut R,
    textures: &Textures,
    ids: &FrameIds,
    kind: RimKind,
    width: f64,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<FramePiece<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    if alpha <= 0.0 {
        return Vec::new();
    }
    let (rgba, ids) = match kind {
        RimKind::Plain => (RIM, &ids.rim),
        RimKind::Accent => (ACCENT, &ids.accent),
        RimKind::Pulse => (ACCENT, &ids.pulse),
    };
    let piece = Piece::Rim(rgba, (width * 1000.0).round() as u32);
    let (texture, corner) = textures.get(piece, scale);
    let inset = f64::from(corner) / scale - RADIUS;
    nine_slice(renderer, &texture, corner, inset, rect, scale, alpha, false)
        .into_iter()
        .zip(ids.iter())
        .map(|(e, id)| Keyed::new(e, id.clone()))
        .collect()
}

/// The grip of a window at `rect` (logical, output-local), `alpha` shown.
pub fn grip_element<R>(
    renderer: &mut R,
    textures: &Textures,
    ids: &FrameIds,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Option<FramePiece<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    if alpha <= 0.0 {
        return None;
    }
    let (texture, _) = textures.get(Piece::Grip, scale);
    let at = Point::<f64, Logical>::from((
        rect.loc.x + (rect.size.w - GRIP_SIZE.0) / 2.0,
        rect.loc.y + GRIP_INSET,
    ))
    .to_physical(scale)
    .to_i32_round::<i32>()
    .to_f64();
    let size = Size::<f64, Logical>::from(GRIP_SIZE).to_i32_round();
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        at,
        &texture,
        (alpha < 1.0).then_some(alpha),
        None,
        Some(size),
        Kind::Unspecified,
    )
    .map_err(|err| tracing::warn!(?err, "could not draw a grip"))
    .ok()
    .map(|e| Keyed::new(e, ids.grip.clone()))
}

/// The handle of a seam centred on `centre` (logical, output-local),
/// upright when the seam runs down, `alpha` shown.
pub fn handle_element<R>(
    renderer: &mut R,
    textures: &Textures,
    id: &Id,
    centre: Point<f64, Logical>,
    vertical: bool,
    scale: f64,
    alpha: f32,
) -> Option<FramePiece<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    if alpha <= 0.0 {
        return None;
    }
    let (piece, size) = if vertical {
        (Piece::Handle, (HANDLE_THICKNESS, HANDLE_LENGTH))
    } else {
        (Piece::Grip, (HANDLE_LENGTH, HANDLE_THICKNESS))
    };
    let (texture, _) = textures.get(piece, scale);
    let at = Point::<f64, Logical>::from((centre.x - size.0 / 2.0, centre.y - size.1 / 2.0))
        .to_physical(scale)
        .to_i32_round::<i32>()
        .to_f64();
    let size = Size::<f64, Logical>::from(size).to_i32_round();
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        at,
        &texture,
        (alpha < 1.0).then_some(alpha),
        None,
        Some(size),
        Kind::Unspecified,
    )
    .map_err(|err| tracing::warn!(?err, "could not draw a seam's handle"))
    .ok()
    .map(|e| Keyed::new(e, id.clone()))
}

/// The preview of where a dragged window snaps, at `rect` (logical,
/// output-local).
pub fn preview_elements<R>(
    renderer: &mut R,
    textures: &Textures,
    ids: &[Id; 9],
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<FramePiece<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    if alpha <= 0.0 {
        return Vec::new();
    }
    let (texture, corner) = textures.get(Piece::Preview, scale);
    nine_slice(renderer, &texture, corner, 0.0, rect, scale, alpha, true)
        .into_iter()
        .zip(ids.iter())
        .map(|(e, id)| Keyed::new(e, id.clone()))
        .collect()
}

/// The shadow under a window at `rect` (logical, output-local), `alpha`
/// opaque on top of the style's: the window's shape moved down and
/// blurred, in nine pieces placed on the output's pixels, none of it under
/// the window itself.
pub fn shadow_elements<R>(
    renderer: &mut R,
    textures: &Textures,
    ids: &FrameIds,
    style: ShadowStyle,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<ShadowPiece<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let window: Rectangle<i32, Physical> = rect.to_physical_precise_round(scale);
    let alpha = alpha * style.alpha;
    if alpha <= 0.0 || window.size.w <= 0 || window.size.h <= 0 {
        return Vec::new();
    }
    let radius = (RADIUS * scale).round() as i32;
    let shadow = textures.shadow(style.blur, scale, window.size);
    let offset = Point::<i32, Physical>::from((0, (style.dy * scale).round() as i32));
    let thirds = |start: i32, len: i32, span: Span| {
        let (c, total) = (span.corner(), len + 2 * span.reach);
        let start = start - span.reach;
        [
            (start, c, 0, c, false),
            (start + c, total - 2 * c, c, span.middle(), false),
            (start + total - c, c, 0, c, true),
        ]
    };
    let at = window.loc + offset;
    let xs = thirds(at.x, window.size.w, shadow.w);
    let ys = thirds(at.y, window.size.h, shadow.h);
    let mut pieces = Vec::with_capacity(9);
    let mut ids = ids.shadow.iter();
    for &(y, h, sy, sh, flip_y) in &ys {
        for &(x, w, sx, sw, flip_x) in &xs {
            let Some(id) = ids.next() else {
                break;
            };
            if w <= 0 || h <= 0 || sw <= 0 || sh <= 0 {
                continue;
            }
            let dst = Rectangle::<i32, Physical>::new((x, y).into(), (w, h).into());
            if under(window, radius, dst) {
                continue;
            }
            let src = Rectangle::<f64, BufferCoords>::new(
                (f64::from(sx), f64::from(sy)).into(),
                (f64::from(sw), f64::from(sh)).into(),
            );
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                dst.loc.to_f64(),
                shadow.quarter(flip_x, flip_y),
                Some(alpha),
                None,
                None,
                Kind::Unspecified,
            ) {
                Ok(inner) => pieces.push(ShadowPiece {
                    inner,
                    id: id.clone(),
                    src,
                    dst,
                    hole: window,
                    radius,
                }),
                Err(err) => tracing::warn!(?err, "could not draw a window's shadow"),
            }
        }
    }
    pieces
}

/// Rectangles on the stack, on the heap past a few.
#[derive(Default)]
struct Rects {
    few: [Rectangle<i32, Physical>; 16],
    len: usize,
    more: Vec<Rectangle<i32, Physical>>,
}

impl Rects {
    fn push(&mut self, r: Rectangle<i32, Physical>) {
        if self.len < self.few.len() {
            self.few[self.len] = r;
            self.len += 1;
        } else {
            if self.more.is_empty() {
                self.more.extend_from_slice(&self.few);
            }
            self.more.push(r);
        }
    }

    fn as_slice(&self) -> &[Rectangle<i32, Physical>] {
        if self.more.is_empty() {
            &self.few[..self.len]
        } else {
            &self.more
        }
    }
}

/// Whether `piece` is all under the window at `window`, its corners
/// rounded to `radius`.
fn under(window: Rectangle<i32, Physical>, radius: i32, piece: Rectangle<i32, Physical>) -> bool {
    let (w, h) = (window.size.w, window.size.h);
    let r = radius.clamp(0, w.min(h) / 2);
    let across = Rectangle::new(
        (window.loc.x, window.loc.y + r).into(),
        (w, h - 2 * r).into(),
    );
    let down = Rectangle::new(
        (window.loc.x + r, window.loc.y).into(),
        (w - 2 * r, h).into(),
    );
    across.contains_rect(piece) || down.contains_rect(piece)
}

/// A piece of a window's shadow, placed on the output's pixels exactly,
/// and drawn only outside the window.
#[derive(Debug)]
pub struct ShadowPiece<R: Renderer> {
    inner: MemoryRenderBufferRenderElement<R>,
    id: Id,
    src: Rectangle<f64, BufferCoords>,
    dst: Rectangle<i32, Physical>,
    hole: Rectangle<i32, Physical>,
    radius: i32,
}

impl<R: Renderer> Element for ShadowPiece<R> {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, _scale: Scale<f64>) -> Point<i32, Physical> {
        self.dst.loc
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.src
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.dst
    }

    fn damage_since(
        &self,
        _scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        if commit == Some(self.current_commit()) {
            DamageSet::default()
        } else {
            DamageSet::from_slice(&[Rectangle::from_size(self.dst.size)])
        }
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R> RenderElement<R> for ShadowPiece<R>
where
    R: Renderer + ImportMem,
    R::TextureId: 'static,
{
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        // Damage is relative to `dst`.
        let mut hole = self.hole;
        hole.loc -= dst.loc;
        let radius = self.radius;
        if !damage.iter().any(|d| d.overlaps(hole)) {
            return self.inner.draw(frame, src, dst, damage, opaque_regions);
        }
        let mut outside = Rects::default();
        for &d in damage {
            match d.intersection(hole) {
                None => outside.push(d),
                Some(i) => {
                    let (d1, i1) = (d.loc + d.size.to_point(), i.loc + i.size.to_point());
                    for (x0, y0, x1, y1) in [
                        (d.loc.x, d.loc.y, d1.x, i.loc.y),
                        (d.loc.x, i1.y, d1.x, d1.y),
                        (d.loc.x, i.loc.y, i.loc.x, i1.y),
                        (i1.x, i.loc.y, d1.x, i1.y),
                    ] {
                        if x1 > x0 && y1 > y0 {
                            outside.push(Rectangle::from_extremities((x0, y0), (x1, y1)));
                        }
                    }
                }
            }
        }
        let r = radius.min(hole.size.w / 2).min(hole.size.h / 2).max(0);
        for (left, top) in [(true, true), (false, true), (true, false), (false, false)] {
            let x = if left {
                hole.loc.x
            } else {
                hole.loc.x + hole.size.w - r
            };
            let y = if top {
                hole.loc.y
            } else {
                hole.loc.y + hole.size.h - r
            };
            let square = Rectangle::new((x, y).into(), (r, r).into());
            if !damage.iter().any(|d| d.overlaps(square)) {
                continue;
            }
            for (row, inset) in corner_insets(r) {
                let cut = Rectangle::new(
                    (
                        if left { x } else { x + r - inset },
                        if top { y + row } else { y + r - 1 - row },
                    )
                        .into(),
                    (inset, 1).into(),
                );
                for d in damage.iter().filter_map(|d| d.intersection(cut)) {
                    outside.push(d);
                }
            }
        }
        let outside = outside.as_slice();
        if outside.is_empty() {
            return Ok(());
        }
        self.inner.draw(frame, src, dst, outside, opaque_regions)
    }

    fn underlying_storage(&self, _renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        // Scanned out directly, it would cover the window.
        None
    }
}

/// The pieces of a `2 × corner + 1` square texture around `rect`: corners
/// as they are, edges stretched; the middle only when `middle`.
/// `inset` is how far the texture's window edge is from its sides, logical.
#[allow(clippy::too_many_arguments)]
fn nine_slice<R>(
    renderer: &mut R,
    texture: &MemoryRenderBuffer,
    corner: i32,
    inset: f64,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
    middle: bool,
) -> Vec<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let c = f64::from(corner);
    let cl = c / scale;
    // The whole decorated area, logical, its edges on whole pixels.
    let outer = Rectangle::<f64, Logical>::new(
        rect.loc - Point::from((inset, inset)),
        rect.size + Size::from((2.0 * inset, 2.0 * inset)),
    );
    let mid_w = (outer.size.w - 2.0 * cl).max(0.0);
    let mid_h = (outer.size.h - 2.0 * cl).max(0.0);
    let xs = [
        (0.0, cl, 0.0, c),
        (cl, mid_w, c, 1.0),
        (cl + mid_w, cl, c + 1.0, c),
    ];
    let ys = [
        (0.0, cl, 0.0, c),
        (cl, mid_h, c, 1.0),
        (cl + mid_h, cl, c + 1.0, c),
    ];
    let mut pieces = Vec::with_capacity(9);
    for (j, &(y, h, sy, sh)) in ys.iter().enumerate() {
        for (i, &(x, w, sx, sw)) in xs.iter().enumerate() {
            if (i == 1 && j == 1 && !middle) || w <= 0.0 || h <= 0.0 {
                continue;
            }
            let at = Point::<f64, Logical>::from((outer.loc.x + x, outer.loc.y + y))
                .to_physical(scale)
                .to_i32_round::<i32>()
                .to_f64();
            let size = Size::<f64, Logical>::from((w, h)).to_i32_round();
            let src = Rectangle::<f64, Logical>::new((sx, sy).into(), (sw, sh).into());
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                at,
                texture,
                (alpha < 1.0).then_some(alpha),
                Some(src),
                Some(size),
                Kind::Unspecified,
            ) {
                Ok(element) => pieces.push(element),
                Err(err) => tracing::warn!(?err, "could not draw a window's frame"),
            }
        }
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_cut_and_the_middle_is_not() {
        let rect = Rectangle::<i32, Physical>::new((10, 20).into(), (100, 50).into());
        let cuts = corner_cutouts(rect, 8);
        assert!(cuts.iter().any(|c| c.contains((10, 20))));
        assert!(cuts.iter().any(|c| c.contains((109, 69))));
        assert!(!cuts.iter().any(|c| c.contains((60, 20))));
        assert!(!cuts.iter().any(|c| c.contains((18, 28))));
        assert!(corner_cutouts(rect, 0).is_empty());
    }

    #[test]
    fn the_rim_is_outside_the_window_only() {
        let (_, corner) = rim_texture(ACCENT, 1.0, 1.0);
        assert!(corner > RADIUS as i32);
        let side = f64::from(2 * corner + 1);
        let edge = f64::from(corner) - RADIUS;
        // Along a straight edge: the pixel just outside is the rim, the
        // window's first pixel is not.
        let at = |y: f64| {
            rounded_coverage(side / 2.0, y, side, edge - 1.0, RADIUS + 1.0)
                - rounded_coverage(side / 2.0, y, side, edge, RADIUS)
        };
        assert_eq!(at(edge - 0.5), 1.0, "the rim");
        assert_eq!(at(edge + 0.5), 0.0, "the window's first row");
    }

    #[test]
    fn the_shadow_falls_off_as_a_gaussian() {
        let sigma = shadow_sigma(36.0, 1.0);
        let reach = shadow_reach(sigma);
        let (w, h) = (Span::new(4000, reach), Span::new(4000, reach));
        let pixels = shadow_pixels(36.0, 1.0, w, h);
        let n = w.half() as usize;
        let row = (h.corner() + h.middle()) as usize - 1;
        let alpha: Vec<u8> = (0..n).map(|x| pixels[(row * n + x) * 4 + 3]).collect();
        assert_eq!(alpha[0], 0, "faded at the side");
        assert!(alpha[w.corner() as usize] >= 254, "darkest in the middle");
        let steps: Vec<i16> = alpha[..=w.corner() as usize]
            .windows(2)
            .map(|p| i16::from(p[1]) - i16::from(p[0]))
            .collect();
        assert!(steps.iter().all(|s| (0..=6).contains(s)), "{steps:?}");
    }

    #[test]
    fn the_normal_distribution_is_right() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-7);
        assert!((normal_cdf(1.0) - 0.841_344_75).abs() < 1e-6);
    }
}
