//! Windows without title bars still read as windows: rounded corners, a
//! soft shadow, a hairline just inside their edge in the theme's tint, and
//! a ring in the accent colour on the focused one
//! (docs/decisions/0006-no-title-bars.md).
//!
//! Everything here works the same with every renderer, pixman included, so
//! that scenarios can check it: corners are cut by leaving their outside out
//! of what the window draws, and the shadow, the hairline and the ring are
//! textures drawn once per scale and stretched in nine slices. The shadow
//! is drawn at the output's pixels, placed on them exactly, and left out
//! of the window's own rounded rectangle.

use std::{collections::BTreeMap, sync::Mutex};

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

/// How windows are decorated: from the configuration, the ring in the
/// theme's accent unless it has a colour of its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Corner radius, logical pixels.
    pub radius: f64,
    /// Shadow spread (blur radius) and offset, logical pixels: the light
    /// of the hour's (#161).
    pub shadow_blur: f64,
    pub shadow_dx: f64,
    pub shadow_dy: f64,
    /// Shadow opacity at its darkest, 0 to 1.
    pub shadow_alpha: f64,
    /// The focus ring: width in logical pixels, colour as RGB.
    pub ring_width: f64,
    pub accent: [u8; 3],
    /// The hairline inside the window's edge, as RGBA (the theme's).
    pub border: [u8; 4],
}

impl Default for Style {
    /// The dark theme with the bamboo accent (docs/design/identity.md).
    fn default() -> Self {
        Self {
            radius: 14.0,
            shadow_blur: 70.0,
            shadow_dx: 0.0,
            shadow_dy: 36.0,
            shadow_alpha: 0.5,
            ring_width: 2.0,
            accent: [0x7B, 0xC4, 0x7F],
            border: [0xFF, 0xFF, 0xFF, 0x14],
        }
    }
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
    /// The element's own geometry when made: drawn elsewhere (magnified),
    /// the corners follow.
    natural: Option<Rectangle<i32, Physical>>,
}

impl<E> Rounded<E> {
    pub fn new(inner: E, clip: Rectangle<i32, Physical>, radius: i32) -> Self {
        Self {
            inner,
            clip,
            radius,
            natural: None,
        }
    }
}

impl<E: Element> Rounded<E> {
    /// Remembers the element's geometry at `scale`, for corners that follow
    /// it when it is drawn larger.
    pub fn at_scale(mut self, scale: Scale<f64>) -> Self {
        self.natural = Some(self.inner.geometry(scale));
        self
    }

    /// The clip and the radius where `dst` puts the element.
    fn clip_in(&self, dst: Rectangle<i32, Physical>) -> (Rectangle<i32, Physical>, i32) {
        match self.natural {
            Some(n) if n.size != dst.size && n.size.w > 0 && n.size.h > 0 => {
                // Stretched more one way than the other (a window changing
                // place): each way by its own factor.
                let kx = f64::from(dst.size.w) / f64::from(n.size.w);
                let ky = f64::from(dst.size.h) / f64::from(n.size.h);
                let at = |v: i32, from: i32, to: i32, k: f64| {
                    to + (f64::from(v - from) * k).round() as i32
                };
                let clip = Rectangle::new(
                    (
                        at(self.clip.loc.x, n.loc.x, dst.loc.x, kx),
                        at(self.clip.loc.y, n.loc.y, dst.loc.y, ky),
                    )
                        .into(),
                    (
                        (f64::from(self.clip.size.w) * kx).round() as i32,
                        (f64::from(self.clip.size.h) * ky).round() as i32,
                    )
                        .into(),
                );
                (clip, (f64::from(self.radius) * kx.min(ky)).round() as i32)
            }
            _ => (self.clip, self.radius),
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
        let (clip, radius) = self.clip_in(dst);
        let cutouts: Vec<_> = corner_cutouts(clip, radius)
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

    fn underlying_storage(&self, renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        // Scanned out directly, it would lose its corners.
        let _ = renderer;
        None
    }
}

/// What a nine-slice texture is: the ring in a colour, or the hairline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Piece {
    Ring([u8; 3]),
    Hairline([u8; 4]),
}

/// How many shadows' textures are kept: one per scale shared by most
/// windows, one per scale and corner radius by most panels (#281), and
/// those of windows and panels too small for it.
const SHADOWS_KEPT: usize = 16;

/// Textures drawn once per piece and scale; shadows once per scale, and
/// once per size for windows too small for the shared one.
#[derive(Debug, Default)]
pub struct Textures {
    pieces: Mutex<BTreeMap<(Piece, u32), (MemoryRenderBuffer, i32)>>,
    shadows: Mutex<Shadows>,
}

/// The shadows drawn, the least recently used first out.
#[derive(Debug, Default)]
struct Shadows {
    made: Vec<(ShadowKey, Shadow, u64)>,
    uses: u64,
}

/// A shadow's texture is the same for every window whose sides are long
/// enough for its middle to stretch, and for every panel with the same
/// corners and light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShadowKey {
    scale: u32,
    w: Span,
    h: Span,
    /// Thousandths of a logical pixel, and the darkest level.
    radius: u32,
    blur: u32,
    darkest: u8,
}

impl Textures {
    /// Forgets every texture: the style changed.
    pub fn clear(&self) {
        if let Ok(mut cache) = self.pieces.lock() {
            cache.clear();
        }
        if let Ok(mut shadows) = self.shadows.lock() {
            shadows.made.clear();
        }
    }

    /// The texture and its corner size in pixels.
    fn get(&self, piece: Piece, style: &Style, scale: f64) -> (MemoryRenderBuffer, i32) {
        let key = (piece, (scale * 1000.0).round() as u32);
        let draw = || match piece {
            Piece::Ring(rgb) => ring_texture(style, rgb, scale),
            Piece::Hairline(rgba) => hairline_texture(style, rgba, scale),
        };
        match self.pieces.lock() {
            Ok(mut cache) => cache.entry(key).or_insert_with(draw).clone(),
            Err(_) => draw(),
        }
    }

    /// The shadow of a window `size` physical pixels large.
    fn shadow(&self, style: &Style, scale: f64, size: Size<i32, Physical>) -> Shadow {
        let reach = shadow_reach(shadow_sigma(style, scale));
        let key = ShadowKey {
            scale: (scale * 1000.0).round() as u32,
            w: Span::new(size.w, reach),
            h: Span::new(size.h, reach),
            radius: (style.radius.max(0.0) * 1000.0).round() as u32,
            blur: (style.shadow_blur.max(0.0) * 1000.0).round() as u32,
            darkest: (style.shadow_alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        };
        let draw = || shadow_texture(style, scale, key.w, key.h);
        let Ok(mut shadows) = self.shadows.lock() else {
            return draw();
        };
        shadows.uses += 1;
        let now = shadows.uses;
        if let Some((_, shadow, used)) = shadows.made.iter_mut().find(|(k, _, _)| *k == key) {
            *used = now;
            return shadow.clone();
        }
        let shadow = draw();
        if shadows.made.len() >= SHADOWS_KEPT
            && let Some(oldest) = (0..shadows.made.len()).min_by_key(|&i| shadows.made[i].2)
        {
            shadows.made.swap_remove(oldest);
        }
        shadows.made.push((key, shadow.clone(), now));
        shadow
    }
}

/// Premultiplied ARGB8888 pixels, as B, G, R, A bytes.
fn pixel(rgb: [u8; 3], alpha: f64) -> [u8; 4] {
    let a = alpha.clamp(0.0, 1.0);
    let c = |v: u8| (f64::from(v) * a).round() as u8;
    [c(rgb[2]), c(rgb[1]), c(rgb[0]), (a * 255.0).round() as u8]
}

/// Coverage of a rounded square of side `side` and radius `r`, inset by
/// `inset`, at pixel centre `(x, y)`; antialiased over one pixel.
fn rounded_coverage(x: f64, y: f64, side: f64, inset: f64, r: f64) -> f64 {
    let half = side / 2.0 - inset;
    let centre = side / 2.0;
    // Signed distance to a rounded rectangle: negative inside.
    let qx = (x - centre).abs() - (half - r);
    let qy = (y - centre).abs() - (half - r);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let d = outside + qx.max(qy).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// The shadow's spread, physical pixels: CSS's blur radius is twice the
/// standard deviation of its Gaussian, as the boards draw it.
fn shadow_sigma(style: &Style, scale: f64) -> f64 {
    (style.shadow_blur * scale / 2.0).max(0.5)
}

/// How far a shadow reaches past the edge of what casts it, and as far in:
/// two and a half deviations, where the Gaussian has all but faded.
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
    /// How far the shadow reaches past the window's edge, and as far in.
    reach: i32,
}

impl Span {
    fn new(len: i32, reach: i32) -> Self {
        Self {
            len: len.clamp(0, 2 * reach + 1),
            reach,
        }
    }

    /// Texels from the texture's side to its middle.
    fn corner(self) -> i32 {
        self.reach + self.len / 2
    }

    fn size(self) -> i32 {
        self.len + 2 * self.reach
    }

    /// The texels of the middle: one that stretches, or none.
    fn middle(self) -> i32 {
        self.size() - 2 * self.corner()
    }

    /// The texels drawn: a corner, the middle, and when it stretches the
    /// texel past it, which its filtering reads; the other corner is the
    /// same, mirrored.
    fn half(self) -> i32 {
        self.corner() + 2 * self.middle()
    }

    /// How much of the shadow there is at each texel, 0 to 1, and how much
    /// a rounded corner's missing bit at each end takes away from it.
    fn profile(self, sigma: f64, radius: f64) -> (Vec<f64>, Vec<f64>) {
        // The Gaussian cut at the texture's sides and spread again over
        // the rest: it fades to nothing exactly there, and the middle of a
        // long side is exactly at its darkest.
        let k = f64::from(self.reach) / sigma;
        let tail = normal_cdf(-k);
        let cdf = |z: f64| ((normal_cdf(z) - tail) / (1.0 - 2.0 * tail)).clamp(0.0, 1.0);
        // A corner rounded to `radius` lacks the bit between its square and
        // its quarter circle; blurred far wider than it is, that bit is a
        // point at its centroid.
        let inset =
            radius * (10.0 - 3.0 * std::f64::consts::PI) / (12.0 - 3.0 * std::f64::consts::PI);
        let len = f64::from(self.len);
        let gauss = |d: f64| {
            (-d * d / (2.0 * sigma * sigma)).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt())
        };
        (0..self.half())
            .map(|t| {
                // From the window's near edge, inwards.
                let u = f64::from(t) + 0.5 - f64::from(self.reach);
                let cover = cdf(u / sigma) - cdf((u - len) / sigma);
                let corners = gauss(u - inset) + gauss(u - (len - inset));
                (cover, corners)
            })
            .unzip()
    }
}

/// The standard normal distribution's cumulative function, from the error
/// function's approximation 7.1.26 of Abramowitz and Stegun (to 1.5e-7).
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

/// A window's shadow drawn for its size at the output's pixels: its top
/// left quarter, with the middle row and column (if any) that stretch. The
/// other quarters are the same pixels mirrored, so they are kept once.
#[derive(Debug, Clone)]
struct Shadow {
    /// As they are, mirrored left to right, top to bottom, and both.
    quarters: [MemoryRenderBuffer; 4],
    w: Span,
    h: Span,
}

impl Shadow {
    fn quarter(&self, mirrored_x: bool, mirrored_y: bool) -> &MemoryRenderBuffer {
        &self.quarters[usize::from(mirrored_x) + 2 * usize::from(mirrored_y)]
    }
}

/// The shadow of a rounded rectangle `w` × `h`, Gaussian-blurred as CSS's
/// `box-shadow` blurs (a standard deviation of half the blur).
///
/// A rectangle's blur is the product of its blur each way, so each pixel is
/// two numbers multiplied: no blur passes, cheap to draw again for another
/// size. The rounded corners take away what they lack.
fn shadow_texture(style: &Style, scale: f64, w: Span, h: Span) -> Shadow {
    let pixels = MemoryBuffer::from_slice(
        &shadow_pixels(style, scale, w, h),
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

/// The top left quarter's pixels, premultiplied black, row after row.
fn shadow_pixels(style: &Style, scale: f64, w: Span, h: Span) -> Vec<u8> {
    let sigma = shadow_sigma(style, scale);
    let radius = style.radius * scale;
    let (cover_x, corners_x) = w.profile(sigma, radius);
    let (cover_y, corners_y) = h.profile(sigma, radius);
    let missing = radius * radius * (1.0 - std::f64::consts::FRAC_PI_4);
    let darkest = style.shadow_alpha.clamp(0.0, 1.0) * 255.0;
    // Written in place: a panel's shadow is drawn again on the loop for
    // each new height (#281).
    let mut pixels = vec![0; cover_x.len() * cover_y.len() * 4];
    let rows = pixels.chunks_exact_mut(cover_x.len().max(1) * 4);
    for ((cy, ky), row) in cover_y.iter().zip(&corners_y).zip(rows) {
        for ((cx, kx), px) in cover_x
            .iter()
            .zip(&corners_x)
            .zip(row.as_chunks_mut::<4>().0)
        {
            let a = (cx * cy - missing * kx * ky).clamp(0.0, 1.0);
            px[3] = (a * darkest).round() as u8;
        }
    }
    pixels
}

/// A square holding a rounded ring hugging the window from outside.
fn ring_texture(style: &Style, rgb: [u8; 3], scale: f64) -> (MemoryRenderBuffer, i32) {
    let width = style.ring_width * scale;
    let r = style.radius * scale;
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
            pixel(rgb, outer - inner)
        })
        .collect();
    (buffer(&pixels, side), corner)
}

/// A square holding a hairline one logical pixel wide just inside a
/// rounded window's edge, which is the square's own edge: the window's
/// content shows through it, tinted.
fn hairline_texture(style: &Style, rgba: [u8; 4], scale: f64) -> (MemoryRenderBuffer, i32) {
    let width = scale.max(1.0);
    let r = style.radius * scale;
    let corner = (r.ceil() as i32).max(width.ceil() as i32) + 1;
    let side = 2 * corner + 1;
    let n = side as usize;
    let alpha = f64::from(rgba[3]) / 255.0;
    let rgb = [rgba[0], rgba[1], rgba[2]];
    let pixels: Vec<u8> = (0..n * n)
        .flat_map(|i| {
            let (x, y) = ((i % n) as f64 + 0.5, (i / n) as f64 + 0.5);
            let outer = rounded_coverage(x, y, f64::from(side), 0.0, r);
            let inner = rounded_coverage(x, y, f64::from(side), width, (r - width).max(0.0));
            pixel(rgb, (outer - inner).max(0.0) * alpha)
        })
        .collect();
    (buffer(&pixels, side), corner)
}

fn buffer(pixels: &[u8], side: i32) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        pixels,
        Fourcc::Argb8888,
        (side, side),
        1,
        Transform::Normal,
        None,
    )
}

/// The focus ring around a window at `rect` (logical, relative to the
/// output), `alpha` opaque.
pub fn ring_elements<R>(
    renderer: &mut R,
    textures: &Textures,
    style: &Style,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    ring_elements_in(renderer, textures, style, style.accent, rect, scale, alpha)
}

/// The same ring in another colour: the Matrix theme's glow while a window
/// is resized.
pub fn ring_elements_in<R>(
    renderer: &mut R,
    textures: &Textures,
    style: &Style,
    rgb: [u8; 3],
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let (texture, corner) = textures.get(Piece::Ring(rgb), style, scale);
    let inset = f64::from(corner) / scale - style.radius;
    nine_slice(renderer, &texture, corner, inset, rect, scale, alpha)
}

/// The hairline just inside a window at `rect`, over its content, `alpha`
/// opaque; none where the theme has none.
pub fn hairline_elements<R>(
    renderer: &mut R,
    textures: &Textures,
    style: &Style,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    if style.border[3] == 0 || alpha <= 0.0 {
        return Vec::new();
    }
    let (texture, corner) = textures.get(Piece::Hairline(style.border), style, scale);
    nine_slice(renderer, &texture, corner, 0.0, rect, scale, alpha)
}

/// The shadow under a window at `rect` (logical, relative to the output),
/// `alpha` opaque: the window's shape moved by the light's offset and
/// blurred, in nine pieces placed on the output's pixels, none of it under
/// the window itself.
pub fn shadow_elements<R>(
    renderer: &mut R,
    textures: &Textures,
    style: &Style,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<ShadowPiece<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let window: Rectangle<i32, Physical> = rect.to_physical_precise_round(scale);
    if style.shadow_alpha <= 0.0 || alpha <= 0.0 || window.size.w <= 0 || window.size.h <= 0 {
        return Vec::new();
    }
    let radius = (style.radius * scale).round() as i32;
    let shadow = textures.shadow(style, scale, window.size);
    let offset = Point::<i32, Physical>::from((
        (style.shadow_dx * scale).round() as i32,
        (style.shadow_dy * scale).round() as i32,
    ));
    // Each way: where each third starts on the output and how long it is,
    // the same in the quarter, and whether it is mirrored.
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
    for &(y, h, sy, sh, flip_y) in &ys {
        for &(x, w, sx, sw, flip_x) in &xs {
            if w <= 0 || h <= 0 || sw <= 0 || sh <= 0 {
                continue;
            }
            let dst = Rectangle::<i32, Physical>::new((x, y).into(), (w, h).into());
            // Not under the window: a see-through one shows what is behind
            // it, not its own shadow.
            if under(window, radius, dst) {
                continue;
            }
            // In the quarter's own pixels: its transform mirrors them.
            let src = Rectangle::<f64, BufferCoords>::new(
                (f64::from(sx), f64::from(sy)).into(),
                (f64::from(sw), f64::from(sh)).into(),
            );
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                dst.loc.to_f64(),
                shadow.quarter(flip_x, flip_y),
                (alpha < 1.0).then_some(alpha),
                None,
                None,
                Kind::Unspecified,
            ) {
                Ok(inner) => pieces.push(ShadowPiece {
                    inner,
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

/// A shell panel's shadow on an output (#281): as deep as windows', the
/// same Gaussian, with the panel's corners and the light of its theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caster {
    /// The panel's box, output-local, logical.
    pub rect: Rectangle<f64, Logical>,
    pub radius: f64,
    /// Its own fading, 0 to 1.
    pub alpha: f32,
    pub shadow: hocus_ui::Shadow,
}

/// The shadows a shell surface's panels cast, `origin` its corner on the
/// output, logical.
pub fn casters(origin: Point<f64, Logical>, casts: &[hocus_ui::Cast]) -> Vec<Caster> {
    casts
        .iter()
        .map(|c| Caster {
            rect: Rectangle::new(
                origin + Point::from((f64::from(c.rect.x), f64::from(c.rect.y))),
                (f64::from(c.rect.w), f64::from(c.rect.h)).into(),
            ),
            radius: f64::from(c.radius),
            alpha: c.opacity,
            shadow: c.shadow,
        })
        .collect()
}

/// The style a panel's shadow is drawn with: the windows' with the
/// panel's corners and the light of its theme.
fn caster_style(style: &Style, caster: &Caster) -> Style {
    let s = caster.shadow;
    Style {
        radius: caster.radius,
        shadow_blur: f64::from(s.blur),
        shadow_dx: f64::from(s.dx),
        shadow_dy: f64::from(s.dy),
        shadow_alpha: f64::from(s.alpha),
        ..*style
    }
}

impl Textures {
    /// Draws the texture of a panel's shadow ahead of time, as the panel
    /// is: showing it then draws nothing new.
    pub fn prepare_caster(&self, style: &Style, caster: &Caster, scale: f64) {
        let size: Rectangle<i32, Physical> = caster.rect.to_physical_precise_round(scale);
        let _ = self.shadow(&caster_style(style, caster), scale, size.size);
    }
}

/// The shadow under a panel, `alpha` opaque on top of its own fading:
/// drawn as windows' are, and none of it under the panel.
pub fn caster_elements<R>(
    renderer: &mut R,
    textures: &Textures,
    style: &Style,
    caster: &Caster,
    scale: f64,
    alpha: f32,
) -> Vec<ShadowPiece<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    shadow_elements(
        renderer,
        textures,
        &caster_style(style, caster),
        caster.rect,
        scale,
        caster.alpha * alpha,
    )
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

/// A piece of a window's shadow, placed on the output's pixels exactly (a
/// texel each, but where it stretches), and drawn only outside the window.
#[derive(Debug)]
pub struct ShadowPiece<R: Renderer> {
    inner: MemoryRenderBufferRenderElement<R>,
    src: Rectangle<f64, BufferCoords>,
    dst: Rectangle<i32, Physical>,
    /// The window's rectangle and corner radius, in the same space.
    hole: Rectangle<i32, Physical>,
    radius: i32,
}

impl<R: Renderer> ShadowPiece<R> {
    /// The window's rectangle and radius where `dst` puts the piece: drawn
    /// elsewhere (magnified), the hole follows.
    fn hole_in(&self, dst: Rectangle<i32, Physical>) -> (Rectangle<i32, Physical>, i32) {
        if dst == self.dst || self.dst.size.w <= 0 || self.dst.size.h <= 0 {
            return (self.hole, self.radius);
        }
        let kx = f64::from(dst.size.w) / f64::from(self.dst.size.w);
        let ky = f64::from(dst.size.h) / f64::from(self.dst.size.h);
        let at = |v: i32, from: i32, to: i32, k: f64| to + (f64::from(v - from) * k).round() as i32;
        let hole = Rectangle::new(
            (
                at(self.hole.loc.x, self.dst.loc.x, dst.loc.x, kx),
                at(self.hole.loc.y, self.dst.loc.y, dst.loc.y, ky),
            )
                .into(),
            (
                (f64::from(self.hole.size.w) * kx).round() as i32,
                (f64::from(self.hole.size.h) * ky).round() as i32,
            )
                .into(),
        );
        (hole, (f64::from(self.radius) * kx.min(ky)).round() as i32)
    }
}

impl<R: Renderer> Element for ShadowPiece<R> {
    fn id(&self) -> &Id {
        self.inner.id()
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
        // The texture never changes once drawn.
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
        let (mut hole, radius) = self.hole_in(dst);
        hole.loc -= dst.loc;
        if !damage.iter().any(|d| d.overlaps(hole)) {
            return self.inner.draw(frame, src, dst, damage, opaque_regions);
        }
        // Outside the window, and in the corners it leaves out; kept on the
        // stack, as this runs for every piece on every frame.
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

    fn underlying_storage(&self, renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        // Scanned out directly, it would cover the window.
        let _ = renderer;
        None
    }
}

/// Eight pieces of a `2 × corner + 1` square texture around `rect`: corners
/// as they are, edges stretched; the middle, behind the window, left out.
/// `inset` is how far the texture's window edge is from its sides, logical.
fn nine_slice<R>(
    renderer: &mut R,
    texture: &MemoryRenderBuffer,
    corner: i32,
    inset: f64,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
) -> Vec<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let c = f64::from(corner);
    let cl = c / scale;
    // The whole decorated area, logical.
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
    let mut pieces = Vec::with_capacity(8);
    for (j, &(y, h, sy, sh)) in ys.iter().enumerate() {
        for (i, &(x, w, sx, sw)) in xs.iter().enumerate() {
            if i == 1 && j == 1 || w <= 0.0 || h <= 0.0 {
                continue;
            }
            let at =
                Point::<f64, Logical>::from((outer.loc.x + x, outer.loc.y + y)).to_physical(scale);
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
        // The very corner pixel is cut, the middle of an edge is not.
        assert!(cuts.iter().any(|c| c.contains((10, 20))));
        assert!(cuts.iter().any(|c| c.contains((109, 69))));
        assert!(!cuts.iter().any(|c| c.contains((60, 20))));
        assert!(!cuts.iter().any(|c| c.contains((18, 28))));
        assert!(corner_cutouts(rect, 0).is_empty());
    }

    #[test]
    fn the_hairline_is_one_pixel_inside_the_edge() {
        let style = Style::default();
        let r = style.radius;
        let corner = f64::from(r.ceil() as i32 + 1);
        let side = 2.0 * corner + 1.0;
        let at = |y: f64| {
            rounded_coverage(side / 2.0, y, side, 0.0, r)
                - rounded_coverage(side / 2.0, y, side, 1.0, r - 1.0)
        };
        assert_eq!(at(0.5), 1.0, "the window's outermost row");
        assert_eq!(at(1.5), 0.0, "the content inside");
    }

    /// The shadow's alpha along its middle row, outside in.
    fn middle_row(scale: f64, w: i32, h: i32) -> (Vec<u8>, Span, Span) {
        let style = Style::default();
        let reach = shadow_reach(shadow_sigma(&style, scale));
        let (sw, sh) = (Span::new(w, reach), Span::new(h, reach));
        let pixels = shadow_pixels(&style, scale, sw, sh);
        // The middle row, or the corner's last; the row goes on mirrored
        // past the middle.
        let row = (sh.corner() + sh.middle()) as usize - 1;
        let n = sw.half() as usize;
        let (corner, middle) = (sw.corner() as usize, sw.middle() as usize);
        let half: Vec<u8> = (0..n).map(|x| pixels[(row * n + x) * 4 + 3]).collect();
        let alpha = half[..corner + middle]
            .iter()
            .chain(half[..corner].iter().rev())
            .copied()
            .collect();
        (alpha, sw, sh)
    }

    #[test]
    fn the_shadow_falls_off_as_a_gaussian_at_every_scale() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let style = Style::default();
            let sigma = shadow_sigma(&style, scale);
            let (alpha, w, _) = middle_row(scale, 4000, 4000);
            assert_eq!(w.middle(), 1, "a long side stretches its middle");
            let darkest = (style.shadow_alpha * 255.0).round() as u8;
            assert_eq!(alpha[0], 0, "faded to nothing at the side, at {scale}");
            let middle = alpha[w.corner() as usize];
            assert!(
                middle.abs_diff(darkest) <= 1,
                "the middle at its darkest: {middle}"
            );
            for (t, pair) in alpha[..=w.corner() as usize].windows(2).enumerate() {
                assert!(pair[0] <= pair[1], "darker inwards, at {scale}, texel {t}");
                assert!(pair[1] - pair[0] <= 2, "no step, at {scale}, texel {t}");
            }
            // As CSS draws `box-shadow`: the blur's Gaussian.
            for (t, &a) in alpha[..=w.corner() as usize].iter().enumerate() {
                let u = t as f64 + 0.5 - f64::from(w.reach);
                let ideal = normal_cdf(u / sigma) * style.shadow_alpha * 255.0;
                assert!(
                    (f64::from(a) - ideal).abs() <= 1.5,
                    "texel {t} at {scale}: {a}, not {ideal:.1}"
                );
            }
        }
    }

    #[test]
    fn a_small_window_has_a_lighter_shadow_of_its_own() {
        let (large, _, _) = middle_row(1.0, 1000, 1000);
        let (small, w, _) = middle_row(1.0, 60, 1000);
        assert_eq!(w.size(), 60 + 2 * w.reach, "drawn for its side");
        let middle = small[w.size() as usize / 2];
        assert!(middle < large[large.len() / 2], "lighter: {middle}");
        // Symmetric, and falling off as smoothly.
        assert!(small.iter().eq(small.iter().rev()));
        for pair in small[..=w.size() as usize / 2].windows(2) {
            assert!(pair[0] <= pair[1] && pair[1] - pair[0] <= 2);
        }
    }

    #[test]
    fn the_normal_distribution_is_right() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-7);
        assert!((normal_cdf(1.0) - 0.841_344_75).abs() < 1e-6);
        assert!((normal_cdf(-2.5) - 0.006_209_67).abs() < 1e-6);
    }

    #[test]
    fn the_ring_is_coloured_outside_the_window_only() {
        let style = Style::default();
        let (_, corner) = ring_texture(&style, style.accent, 1.0);
        assert!(corner > style.radius as i32);
        // Coverage: inside the window's straight edge, none.
        let side = f64::from(2 * corner + 1);
        let edge = f64::from(corner) - style.radius;
        assert_eq!(
            rounded_coverage(side / 2.0, edge + 3.0, side, edge, style.radius),
            1.0
        );
    }
}
