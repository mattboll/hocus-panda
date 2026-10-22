//! Windows without title bars still read as windows: rounded corners, a
//! soft shadow, a hairline just inside their edge in the theme's tint, and
//! a ring in the accent colour on the focused one
//! (docs/decisions/0006-no-title-bars.md).
//!
//! Everything here works the same with every renderer, pixman included, so
//! that scenarios can check it: corners are cut by leaving their outside out
//! of what the window draws, and the shadow, the hairline and the ring are
//! textures drawn once per scale and stretched in nine slices.

use std::{collections::BTreeMap, sync::Mutex};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportMem, Renderer,
            element::{
                Element, Id, Kind, RenderElement, UnderlyingStorage,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
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
    let r = f64::from(radius);
    let mut cuts = Vec::with_capacity(4 * radius as usize);
    for row in 0..radius {
        // How far in from the side this row's edge is.
        let dy = r - f64::from(row) - 0.5;
        let inset = (r - (r * r - dy * dy).max(0.0).sqrt()).round() as i32;
        if inset == 0 {
            continue;
        }
        let (left, right) = (rect.loc.x, rect.loc.x + rect.size.w - inset);
        let (top, bottom) = (rect.loc.y + row, rect.loc.y + rect.size.h - 1 - row);
        for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
            cuts.push(Rectangle::new((x, y).into(), (inset, 1).into()));
        }
    }
    cuts
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

/// What a nine-slice texture is: the shadow, or the ring in a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Piece {
    Shadow,
    Ring([u8; 3]),
    Hairline([u8; 4]),
}

/// Textures drawn once per piece and scale.
#[derive(Debug, Default)]
pub struct Textures(Mutex<BTreeMap<(Piece, u32), (MemoryRenderBuffer, i32)>>);

impl Textures {
    /// Forgets every texture: the style changed.
    pub fn clear(&self) {
        if let Ok(mut cache) = self.0.lock() {
            cache.clear();
        }
    }

    /// The texture and its corner size in pixels.
    fn get(&self, piece: Piece, style: &Style, scale: f64) -> (MemoryRenderBuffer, i32) {
        let key = (piece, (scale * 1000.0).round() as u32);
        let draw = || match piece {
            Piece::Shadow => shadow_texture(style, scale),
            Piece::Ring(rgb) => ring_texture(style, rgb, scale),
            Piece::Hairline(rgba) => hairline_texture(style, rgba, scale),
        };
        match self.0.lock() {
            Ok(mut cache) => cache.entry(key).or_insert_with(draw).clone(),
            Err(_) => draw(),
        }
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

/// A square holding a rounded rectangle's shadow: the window's corner sits
/// at `corner` pixels from each side; the middle row and column stretch.
///
/// The blur is done on a shape wide enough for the blur not to reach across
/// it, then the corners and the middle row and column are taken from it:
/// blurring a shape only one corner wide would wash the edges out.
fn shadow_texture(style: &Style, scale: f64) -> (MemoryRenderBuffer, i32) {
    let blur = style.shadow_blur * scale;
    let r = style.radius * scale;
    let corner = (blur + r).ceil() as i32;
    let side = 2 * corner + 1;
    let k = ((blur / 2.0).round() as usize).max(1);
    // Three box blurs reach 3k on each side.
    let big = (side as usize) + 6 * k;
    let mut alpha: Vec<f64> = (0..big * big)
        .map(|i| {
            let (x, y) = ((i % big) as f64 + 0.5, (i / big) as f64 + 0.5);
            rounded_coverage(x, y, big as f64, blur, r)
        })
        .collect();
    for _ in 0..3 {
        alpha = box_blur(&alpha, big, k, true);
        alpha = box_blur(&alpha, big, k, false);
    }
    let c = corner as usize;
    let n = side as usize;
    let source = |t: usize| match t.cmp(&c) {
        std::cmp::Ordering::Less => t,
        std::cmp::Ordering::Equal => big / 2,
        std::cmp::Ordering::Greater => big - n + t,
    };
    let pixels: Vec<u8> = (0..n * n)
        .flat_map(|i| {
            let a = alpha[source(i / n) * big + source(i % n)];
            pixel([0, 0, 0], a * style.shadow_alpha)
        })
        .collect();
    (buffer(&pixels, side), corner)
}

fn box_blur(src: &[f64], n: usize, k: usize, horizontal: bool) -> Vec<f64> {
    let mut out = vec![0.0; src.len()];
    let width = (2 * k + 1) as f64;
    for line in 0..n {
        let at = |i: usize| {
            if horizontal {
                src[line * n + i]
            } else {
                src[i * n + line]
            }
        };
        let mut sum: f64 = (0..=k.min(n - 1)).map(at).sum();
        for i in 0..n {
            let v = sum / width;
            if horizontal {
                out[line * n + i] = v;
            } else {
                out[i * n + line] = v;
            }
            if i + k + 1 < n {
                sum += at(i + k + 1);
            }
            if i >= k {
                sum -= at(i - k);
            }
        }
    }
    out
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

/// The shadow under a window at `rect`, `alpha` opaque.
pub fn shadow_elements<R>(
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
    let (texture, corner) = textures.get(Piece::Shadow, style, scale);
    let inset = f64::from(corner) / scale - style.radius;
    let mut below = rect;
    below.loc.x += style.shadow_dx;
    below.loc.y += style.shadow_dy;
    nine_slice(renderer, &texture, corner, inset, below, scale, alpha)
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
