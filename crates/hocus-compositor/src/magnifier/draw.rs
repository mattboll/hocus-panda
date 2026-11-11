//! Drawing a screen magnified (#424): what the screen shows is drawn into
//! a picture of the screen's size, kept between frames with damage
//! tracking of its own (only what changed is drawn again), then that
//! picture's view is drawn over the whole screen, stretched; the cursor,
//! the pill and the minimap over it at their own sizes. The frame's
//! damage is the picture's damage seen through the view: at rest, with
//! the magnifier on and nothing moving, no frame is drawn.
//!
//! Screenshots, recordings and shared screens draw the screen themselves
//! (`render::Purpose::Capture`): they stay at 1×.

use hocus_words::compositor::magnifier_draw as txt;
use std::cell::{OnceCell, RefCell};

use anyhow::Context;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Frame, ImportAll, ImportMem, Offscreen, Renderer, Texture, TextureFilter,
            damage::OutputDamageTracker,
            element::{
                Element, Id, Kind, RenderElement, RenderElementStates, memory::MemoryRenderBuffer,
            },
            gles::{GlesError, GlesRenderer, GlesTarget, GlesTexture},
            pixman::{PixmanError, PixmanRenderer, PixmanTarget, PixmanTexture},
            utils::{CommitCounter, DamageBag, DamageSet, DamageSnapshot, OpaqueRegions},
        },
    },
    output::Output,
    reexports::pixman,
    utils::{Buffer as BufferCoords, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};
use tiny_skia::{PathBuilder, Pixmap};

use crate::{
    State,
    decoration::{FrameIds, Keyed, Placed, ShadowStyle, buffer, shadow_elements},
    radial_draw::{Fonts, advance, argb, paint, rounded, stroke, write},
    render::{BACKGROUND, OutputElement, OutputElements, Purpose},
};

/// The pill (the mockup's): 32 px high, 12 px from the screen's top.
pub(crate) const PILL_H: f64 = 32.0;
const PILL_TOP: f64 = 12.0;
pub(crate) const PAD_LEFT: f64 = 12.0;
const PAD_RIGHT: f64 = 6.0;
const GAP: f64 = 8.0;
pub(crate) const ICON: f64 = 15.0;
pub(crate) const ICON_GAP: f64 = 6.0;
const BUTTON: f64 = 24.0;
const HINT_PAD: f64 = 6.0;
const LABEL_PX: f32 = 12.5;
pub(crate) const ZOOM_PX: f32 = 13.0;
const HINT_PX: f32 = 11.5;
pub(crate) const PILL_FILL: [u8; 4] = [14, 17, 28, 240];
pub(crate) const PILL_RIM: [u8; 4] = [255, 255, 255, 41];
pub(crate) const TEXT: [u8; 4] = [0xD5, 0xDB, 0xE5, 0xFF];
const HINT: [u8; 4] = [0x8C, 0x96, 0xA8, 0xFF];
pub(crate) const ACCENT: [u8; 4] = [0x7E, 0xE0, 0xA0, 0xFF];
const BUTTON_FILL: [u8; 4] = [255, 255, 255, 20];
pub(crate) const PILL_SHADOW: ShadowStyle = ShadowStyle {
    dy: 8.0,
    blur: 24.0,
    alpha: 0.45,
    radius: PILL_H / 2.0,
};
/// The minimap: 64 px wide, 12 px from the screen's bottom right corner.
const MINIMAP_W: f64 = 64.0;
const MINIMAP_MARGIN: f64 = 12.0;
const MINIMAP_FILL: [u8; 4] = [7, 9, 15, 209];
const MINIMAP_RIM: [u8; 4] = [255, 255, 255, 89];
const VIEW_FILL: [u8; 4] = [126, 224, 160, 64];

/// Renderers that draw a screen into a picture of their own and that
/// picture back.
pub trait Magnify: Renderer + Sized {
    type Canvas;
    /// A picture of `size` to draw into.
    fn canvas(&mut self, size: Size<i32, Physical>) -> Result<Self::Canvas, Self::Error>;
    fn bind_canvas<'a>(
        &mut self,
        canvas: &'a mut Self::Canvas,
    ) -> Result<Self::Framebuffer<'a>, Self::Error>;
    /// The picture as a texture to draw.
    fn canvas_texture(canvas: &Self::Canvas) -> Self::TextureId;
}

impl Magnify for GlesRenderer {
    type Canvas = GlesTexture;

    fn canvas(&mut self, size: Size<i32, Physical>) -> Result<GlesTexture, GlesError> {
        Offscreen::<GlesTexture>::create_buffer(self, Fourcc::Abgr8888, (size.w, size.h).into())
    }

    fn bind_canvas<'a>(
        &mut self,
        canvas: &'a mut GlesTexture,
    ) -> Result<GlesTarget<'a>, GlesError> {
        self.bind(canvas)
    }

    fn canvas_texture(canvas: &GlesTexture) -> GlesTexture {
        canvas.clone()
    }
}

/// pixman's picture: an image to draw into, and a texture over the same
/// pixels to draw it with (smithay binds images and draws textures).
pub struct PixmanCanvas {
    /// First, so that it goes before the image owning its pixels.
    texture: PixmanTexture,
    image: pixman::Image<'static, 'static>,
}

impl Magnify for PixmanRenderer {
    type Canvas = PixmanCanvas;

    fn canvas(&mut self, size: Size<i32, Physical>) -> Result<PixmanCanvas, PixmanError> {
        let image: pixman::Image<'static, 'static> =
            Offscreen::<pixman::Image<'static, 'static>>::create_buffer(
                self,
                Fourcc::Argb8888,
                (size.w, size.h).into(),
            )?;
        // SAFETY: the second image reads the pixels pixman allocated for
        // the first, which lives as long as it (the canvas drops the
        // texture first). pixman reads and writes them only while drawing,
        // and the screen's picture is drawn before it is read, on this
        // thread alone.
        #[allow(unsafe_code)]
        let alias = unsafe {
            pixman::Image::from_raw_mut(
                image.format(),
                image.width(),
                image.height(),
                image.data(),
                image.stride(),
                false,
            )
        }
        .map_err(|_| PixmanError::Unsupported)?;
        Ok(PixmanCanvas {
            texture: PixmanTexture::from(alias),
            image,
        })
    }

    fn bind_canvas<'a>(
        &mut self,
        canvas: &'a mut PixmanCanvas,
    ) -> Result<PixmanTarget<'a>, PixmanError> {
        self.bind(&mut canvas.image)
    }

    fn canvas_texture(canvas: &PixmanCanvas) -> PixmanTexture {
        canvas.texture.clone()
    }
}

/// A screen's picture, kept between frames while it is magnified.
pub struct Lens<C> {
    canvas: Option<C>,
    tracker: Option<OutputDamageTracker>,
    /// The picture's size and scale (thousandths).
    key: Option<(Size<i32, Physical>, u32)>,
    /// Drawn whole once: from then on, only what changes.
    drawn: bool,
    /// What the frames drew over, seen through the view.
    bag: DamageBag<i32, Physical>,
    /// The zoom and view of the last frame: another redraws everything.
    last: Option<(u64, u64, u64)>,
    id: Id,
}

impl<C> Default for Lens<C> {
    fn default() -> Self {
        Self {
            canvas: None,
            tracker: None,
            key: None,
            drawn: false,
            bag: DamageBag::new(4),
            last: None,
            id: Id::new(),
        }
    }
}

impl<C> std::fmt::Debug for Lens<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lens")
            .field("canvas", &self.canvas.is_some())
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl<C> Lens<C> {
    /// Not magnified: the picture goes.
    fn rest(&mut self) {
        if self.canvas.is_some() {
            *self = Self::default();
        }
    }
}

/// The screen's picture seen through the view, stretched over the screen.
#[derive(Debug)]
pub struct ZoomElement<T> {
    id: Id,
    damage: DamageSnapshot<i32, Physical>,
    texture: T,
    src: Rectangle<f64, BufferCoords>,
    geometry: Rectangle<i32, Physical>,
}

impl<T> Element for ZoomElement<T> {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.damage.current_commit()
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.src
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.damage
            .damage_since(commit)
            .unwrap_or_else(|| DamageSet::from_slice(&[self.geometry(scale)]))
    }

    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::from_slice(&[self.geometry])
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R, T> RenderElement<R> for ZoomElement<T>
where
    R: Renderer<TextureId = T>,
    T: Texture,
{
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            opaque_regions,
            Transform::Normal,
            1.0,
        )
    }
}

/// What a screen shows this frame.
pub struct Screen<R: Renderer> {
    pub elements: Vec<OutputElement<R>>,
    /// Magnified: what the screen's picture showed, which clients are told
    /// they were seen by; else the frame's own.
    pub states: Option<RenderElementStates>,
    /// The upscale filter was set for hard pixels: [`after`] puts it back.
    hard: bool,
}

impl<R: Renderer> Screen<R> {
    fn plain(elements: Vec<OutputElement<R>>) -> Self {
        Self {
            elements,
            states: None,
            hard: false,
        }
    }
}

/// What `output` shows, magnified when the magnifier is on there:
/// instead of `State::output_elements`. `cursor`: the compositor draws
/// the cursor (nested, the host does).
pub fn screen<R>(
    renderer: &mut R,
    state: &State,
    output: &Output,
    cursor: bool,
    lens: &mut Lens<R::Canvas>,
) -> Screen<R>
where
    R: Magnify
        + ImportAll
        + ImportMem
        + crate::wobble::Bend
        + crate::going::Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let now = state.now();
    let Some((zoom, view)) = state.magnified(output, now) else {
        lens.rest();
        return Screen::plain(state.output_elements(renderer, output, cursor));
    };
    match magnified(renderer, state, output, cursor, lens, zoom, view) {
        Ok(screen) => screen,
        Err(err) => {
            tracing::warn!(output = output.name(), "could not magnify: {err:#}");
            lens.rest();
            Screen::plain(state.output_elements(renderer, output, cursor))
        }
    }
}

/// After the frame: the renderer's filter as it was.
pub fn after<R: Renderer>(renderer: &mut R, screen: &Screen<R>) {
    if screen.hard
        && let Err(err) = renderer.upscale_filter(TextureFilter::Linear)
    {
        tracing::debug!(?err, "could not set the filter back");
    }
}

fn magnified<R>(
    renderer: &mut R,
    state: &State,
    output: &Output,
    cursor: bool,
    lens: &mut Lens<R::Canvas>,
    zoom: f64,
    view: Point<f64, Logical>,
) -> anyhow::Result<Screen<R>>
where
    R: Magnify
        + ImportAll
        + ImportMem
        + crate::wobble::Bend
        + crate::going::Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let mode = output.current_mode().context("a screen without a mode")?;
    let size = output.current_transform().transform_size(mode.size);
    let scale = output.current_scale().fractional_scale();
    let key = (size, (scale * 1000.0).round() as u32);
    if lens.key != Some(key) || lens.canvas.is_none() {
        *lens = Lens::default();
        lens.canvas = Some(
            renderer
                .canvas(size)
                .map_err(|err| anyhow::anyhow!("{err:?}"))
                .context("no picture for the magnifier")?,
        );
        lens.tracker = Some(OutputDamageTracker::new(size, scale, Transform::Normal));
        lens.key = Some(key);
    }
    let inner = state.elements_for(renderer, output, Purpose::Screen { cursor: false });
    let Lens {
        canvas: Some(canvas),
        tracker: Some(tracker),
        drawn,
        bag,
        last,
        id,
        ..
    } = lens
    else {
        anyhow::bail!("no picture");
    };
    let (damage, states) = {
        let mut target = renderer
            .bind_canvas(canvas)
            .map_err(|err| anyhow::anyhow!("{err:?}"))
            .context("could not draw into the picture")?;
        // The picture keeps its pixels: an age of 1 draws what changed.
        let age = usize::from(*drawn);
        let result = tracker
            .render_output(renderer, &mut target, age, &inner, BACKGROUND)
            .map_err(|err| anyhow::anyhow!("{err:?}"))
            .context("could not draw the screen's picture")?;
        (result.damage.cloned(), result.states)
    };
    *drawn = true;
    let full = Rectangle::<i32, Physical>::from_size(size);
    let corner = view.to_physical(scale);
    let now_seen = (zoom.to_bits(), corner.x.to_bits(), corner.y.to_bits());
    if *last != Some(now_seen) {
        bag.add([full]);
        *last = Some(now_seen);
    } else if let Some(damage) = damage {
        // Through the view, a pixel more around for the filter.
        let seen: Vec<_> = damage
            .iter()
            .filter_map(|r| {
                let x0 = ((f64::from(r.loc.x) - 1.0 - corner.x) * zoom).floor() as i32;
                let y0 = ((f64::from(r.loc.y) - 1.0 - corner.y) * zoom).floor() as i32;
                let x1 = ((f64::from(r.loc.x + r.size.w) + 1.0 - corner.x) * zoom).ceil() as i32;
                let y1 = ((f64::from(r.loc.y + r.size.h) + 1.0 - corner.y) * zoom).ceil() as i32;
                Rectangle::from_extremities((x0, y0), (x1, y1)).intersection(full)
            })
            .collect();
        if !seen.is_empty() {
            bag.add(seen);
        }
    }
    let src = Rectangle::<f64, BufferCoords>::new(
        (corner.x, corner.y).into(),
        (f64::from(size.w) / zoom, f64::from(size.h) / zoom).into(),
    );
    let zoomed = ZoomElement {
        id: id.clone(),
        damage: bag.snapshot(),
        texture: R::canvas_texture(canvas),
        src,
        geometry: full,
    };
    let mut elements: Vec<OutputElement<R>> = Vec::new();
    let area = state
        .space
        .output_geometry(output)
        .context("a screen not in the space")?
        .to_f64();
    if cursor {
        let shown = Size::from((area.size.w / zoom, area.size.h / zoom));
        let seen = Rectangle::new(area.loc + view, shown);
        elements.extend(state.magnified_cursor_elements(renderer, seen, scale, zoom));
    }
    state.magnifier_over(renderer, output, area.size, view, zoom, &mut elements);
    elements.push(OutputElements::Zoom(zoomed));
    let hard = !state.config.magnifier.smooth;
    if hard && let Err(err) = renderer.upscale_filter(TextureFilter::Nearest) {
        tracing::debug!(?err, "could not draw hard pixels");
    }
    Ok(Screen {
        elements,
        states: Some(states),
        hard,
    })
}

/// The pill's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
    label: &'static str,
    zoom: String,
    hint: &'static str,
}

impl Words {
    pub fn of(zoom: f64, lang: hocus_words::Lang) -> Self {
        Self {
            label: txt::MAGNIFIER.get(lang),
            zoom: super::zoom_words(zoom, lang),
            hint: txt::SUPER_ESC_TO_EXIT.get(lang),
        }
    }
}

/// Where the pill and its buttons are on the screen (logical, from its
/// corner).
#[derive(Debug, Clone, Copy)]
pub struct Pill {
    pub rect: Rectangle<f64, Logical>,
    pub minus: Rectangle<f64, Logical>,
    pub plus: Rectangle<f64, Logical>,
}

/// The pill's parts, from its left edge.
struct Layout {
    width: f64,
    label: f64,
    zoom: f64,
    minus: f64,
    plus: f64,
    hint: f64,
}

fn layout(fonts: &Fonts, words: &Words) -> Layout {
    let mut x = PAD_LEFT + ICON + ICON_GAP;
    let label = x;
    x += f64::from(advance(&fonts.semibold, words.label, LABEL_PX)) + GAP;
    let zoom = x;
    x += f64::from(advance(&fonts.mono, &words.zoom, ZOOM_PX)) + GAP;
    let minus = x;
    x += BUTTON + GAP;
    let plus = x;
    x += BUTTON + GAP;
    let hint = x + HINT_PAD;
    x += HINT_PAD + f64::from(advance(&fonts.semibold, words.hint, HINT_PX)) + HINT_PAD;
    Layout {
        width: x + PAD_RIGHT,
        label,
        zoom,
        minus,
        plus,
        hint,
    }
}

/// A picture, the key it was drawn for, its size in pixels, and its id: a
/// new one with each picture, so that the damage tracker draws it again.
type Picture<K> = RefCell<Option<(K, MemoryRenderBuffer, (i32, i32), Id)>>;

/// What draws the pill and the minimap.
#[derive(Default)]
pub struct Paint {
    fonts: OnceCell<Option<Fonts>>,
    pill: Picture<(Words, u32)>,
    minimap: Picture<(u32, [i32; 4], (i32, i32))>,
    shadow: FrameIds,
}

impl std::fmt::Debug for Paint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Paint").finish_non_exhaustive()
    }
}

impl Paint {
    fn fonts(&self) -> Option<&Fonts> {
        self.fonts.get_or_init(crate::radial_draw::fonts).as_ref()
    }

    /// The pill with `words` on a screen of `size`.
    pub fn layout(&self, words: &Words, size: Size<f64, Logical>) -> Option<Pill> {
        let parts = layout(self.fonts()?, words);
        let x = ((size.w - parts.width) / 2.0).round();
        let button = |at: f64| {
            Rectangle::new(
                (x + at, PILL_TOP + (PILL_H - BUTTON) / 2.0).into(),
                (BUTTON, BUTTON).into(),
            )
        };
        Some(Pill {
            rect: Rectangle::new((x, PILL_TOP).into(), (parts.width, PILL_H).into()),
            minus: button(parts.minus),
            plus: button(parts.plus),
        })
    }
}

/// The pill drawn at `scale`: its pixels, width and height.
fn draw_pill(fonts: &Fonts, words: &Words, scale: f64) -> Option<(Vec<u8>, i32, i32)> {
    let parts = layout(fonts, words);
    let (w, h) = (
        (parts.width * scale).ceil() as u32,
        (PILL_H * scale).ceil() as u32,
    );
    let mut pixmap = Pixmap::new(w, h)?;
    let s = scale as f32;
    let t = tiny_skia::Transform::from_scale(s, s);
    let (pw, ph) = (parts.width as f32, PILL_H as f32);
    if let Some(path) = rounded(0.5, 0.5, pw - 1.0, ph - 1.0, ph / 2.0) {
        pixmap.fill_path(
            &path,
            &paint(PILL_FILL),
            tiny_skia::FillRule::Winding,
            t,
            None,
        );
        pixmap.stroke_path(&path, &paint(PILL_RIM), &stroke(1.0), t, None);
    }
    // The magnifying glass, a 24-unit drawing at 15 px.
    let k = (ICON / 24.0) as f32;
    let icon = t
        .pre_translate(PAD_LEFT as f32, ((PILL_H - ICON) / 2.0) as f32)
        .pre_scale(k, k);
    let mut pb = PathBuilder::new();
    pb.push_circle(10.5, 10.5, 6.5);
    pb.move_to(15.5, 15.5);
    pb.line_to(20.5, 20.5);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint(TEXT), &stroke(1.8), icon, None);
    }
    let baseline = |px: f32| ((PILL_H as f32) / 2.0 + px * 0.36) * s;
    write(
        &mut pixmap,
        &fonts.semibold,
        words.label,
        LABEL_PX * s,
        (parts.label as f32 * s, baseline(LABEL_PX)),
        TEXT,
        0.0,
    );
    write(
        &mut pixmap,
        &fonts.mono,
        &words.zoom,
        ZOOM_PX * s,
        (parts.zoom as f32 * s, baseline(ZOOM_PX)),
        ACCENT,
        0.0,
    );
    for (at, plus) in [(parts.minus, false), (parts.plus, true)] {
        let (cx, cy) = ((at + BUTTON / 2.0) as f32, (PILL_H / 2.0) as f32);
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, (BUTTON / 2.0) as f32);
        if let Some(path) = pb.finish() {
            pixmap.fill_path(
                &path,
                &paint(BUTTON_FILL),
                tiny_skia::FillRule::Winding,
                t,
                None,
            );
        }
        // − and +: a 24-unit drawing at 14 px.
        let k = 14.0 / 24.0;
        let mut pb = PathBuilder::new();
        pb.move_to(cx - 7.0 * k, cy);
        pb.line_to(cx + 7.0 * k, cy);
        if plus {
            pb.move_to(cx, cy - 7.0 * k);
            pb.line_to(cx, cy + 7.0 * k);
        }
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &paint(TEXT), &stroke(1.8 * k), t, None);
        }
    }
    write(
        &mut pixmap,
        &fonts.semibold,
        words.hint,
        HINT_PX * s,
        (parts.hint as f32 * s, baseline(HINT_PX)),
        HINT,
        0.0,
    );
    Some((argb(pixmap), w as i32, h as i32))
}

/// The minimap at `scale`, `w` × `h` pixels, the view's rectangle at
/// `rect` (its pixels).
fn draw_minimap(scale: f64, (w, h): (i32, i32), rect: [i32; 4]) -> Option<Vec<u8>> {
    let mut pixmap = Pixmap::new(w as u32, h as u32)?;
    let s = scale as f32;
    let (fw, fh) = (w as f32, h as f32);
    if let Some(path) = rounded(0.5 * s, 0.5 * s, fw - s, fh - s, 4.0 * s) {
        let t = tiny_skia::Transform::identity();
        pixmap.fill_path(
            &path,
            &paint(MINIMAP_FILL),
            tiny_skia::FillRule::Winding,
            t,
            None,
        );
        pixmap.stroke_path(&path, &paint(MINIMAP_RIM), &stroke(s), t, None);
    }
    // The view's rectangle, in quarters of a pixel.
    let [x, y, vw, vh] = rect.map(|v| v as f32 / 4.0);
    let line = 1.5 * s;
    if let Some(path) = rounded(
        x + line / 2.0,
        y + line / 2.0,
        (vw - line).max(1.0),
        (vh - line).max(1.0),
        s,
    ) {
        let t = tiny_skia::Transform::identity();
        pixmap.fill_path(
            &path,
            &paint(VIEW_FILL),
            tiny_skia::FillRule::Winding,
            t,
            None,
        );
        pixmap.stroke_path(&path, &paint(ACCENT), &stroke(line), t, None);
    }
    Some(argb(pixmap))
}

/// `texture` (`px` pixels) at `rect` (logical) on a screen at `scale`.
pub(crate) fn placed<R>(
    renderer: &mut R,
    texture: &MemoryRenderBuffer,
    px: (i32, i32),
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
    id: &Id,
) -> Option<OutputElement<R>>
where
    R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let all = Rectangle::from_size((f64::from(px.0), f64::from(px.1)).into());
    let dst = Rectangle::new(rect.loc.to_physical_precise_round(scale), Size::from(px));
    Placed::new(renderer, texture, all, dst, alpha)
        .inspect_err(|err| tracing::warn!(?err, "could not draw the magnifier's pill"))
        .ok()
        .map(|e| OutputElements::Frame(Keyed::new(e, id.clone())))
}

impl State {
    /// The pill and the minimap over the magnified screen, front first.
    fn magnifier_over<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        size: Size<f64, Logical>,
        view: Point<f64, Logical>,
        zoom: f64,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let now = self.now();
        let paint = &self.magnifier.paint;
        let scale = output.current_scale().fractional_scale();
        let milli = (scale * 1000.0).round() as u32;
        let pill_alpha = self.magnifier_fade(self.magnifier.hud_until, now);
        let zoom_to = self.magnifier.on.as_ref().map_or(zoom, |on| on.zoom);
        if pill_alpha > 0.0
            && let Some(fonts) = paint.fonts()
        {
            let words = Words::of(zoom_to, self.radial_lang());
            let key = (words.clone(), milli);
            let mut cache = paint.pill.borrow_mut();
            if cache.as_ref().is_none_or(|(k, ..)| *k != key)
                && let Some((pixels, w, h)) = draw_pill(fonts, &words, scale)
            {
                *cache = Some((key, buffer(&pixels, w, h), (w, h), Id::new()));
            }
            if let (Some((_, texture, px, id)), Some(pill)) =
                (cache.as_ref(), paint.layout(&words, size))
            {
                out.extend(placed(
                    renderer, texture, *px, pill.rect, scale, pill_alpha, id,
                ));
                out.extend(
                    shadow_elements(
                        renderer,
                        &self.textures,
                        &paint.shadow,
                        PILL_SHADOW,
                        pill.rect,
                        scale,
                        pill_alpha,
                    )
                    .into_iter()
                    .map(OutputElements::Shadow),
                );
            }
        }
        let map_alpha = self.magnifier_fade(self.magnifier.minimap_until, now);
        if map_alpha > 0.0 && size.w > 0.0 {
            let height = MINIMAP_W * size.h / size.w;
            let px = (
                (MINIMAP_W * scale).round() as i32,
                (height * scale).round() as i32,
            );
            let k = MINIMAP_W * scale / size.w * 4.0;
            let rect = [
                (view.x * k).round() as i32,
                (view.y * k).round() as i32,
                ((size.w / zoom * k).round() as i32).max(8),
                ((size.h / zoom * k).round() as i32).max(8),
            ];
            let key = (milli, rect, px);
            let mut cache = paint.minimap.borrow_mut();
            if cache.as_ref().is_none_or(|(k, ..)| *k != key)
                && let Some(pixels) = draw_minimap(scale, px, rect)
            {
                *cache = Some((key, buffer(&pixels, px.0, px.1), px, Id::new()));
            }
            if let Some((_, texture, px, id)) = cache.as_ref() {
                let at = Rectangle::new(
                    (
                        size.w - MINIMAP_MARGIN - MINIMAP_W,
                        size.h - MINIMAP_MARGIN - height,
                    )
                        .into(),
                    (MINIMAP_W, height).into(),
                );
                out.extend(placed(renderer, texture, *px, at, scale, map_alpha, id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_draws_in_both_languages_and_its_buttons_are_inside() {
        let fonts = crate::radial_draw::fonts().expect("the shell's fonts");
        for lang in hocus_words::Lang::ALL {
            let words = Words::of(8.0, lang);
            let parts = layout(&fonts, &words);
            assert!(
                parts.width > 200.0 && parts.width < 400.0,
                "{}",
                parts.width
            );
            assert!(parts.plus + BUTTON < parts.hint);
            for scale in [1.0, 1.5, 2.0] {
                let (pixels, w, h) = draw_pill(&fonts, &words, scale).expect("drawn");
                assert_eq!(pixels.len(), (w * h * 4) as usize);
            }
        }
        let paint = Paint::default();
        let pill = paint
            .layout(
                &Words::of(2.0, hocus_words::Lang::French),
                Size::from((1920.0, 1080.0)),
            )
            .expect("laid out");
        assert!(pill.rect.contains(pill.minus.loc) && pill.rect.contains(pill.plus.loc));
        assert!((pill.rect.loc.x + pill.rect.size.w / 2.0 - 960.0).abs() <= 1.0);
    }
}
