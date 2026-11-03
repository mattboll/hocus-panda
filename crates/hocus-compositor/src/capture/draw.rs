//! What the capture draws on the screens, all of it the compositor's own
//! but the shell's toolbar: the darkening around what is chosen, the
//! area's border, its handles and its size, the window's ring, the
//! countdown and the flash. Every size is the 5.8 mockup's × the
//! launcher's zoom, as the launcher is drawn; the darkening is four solid
//! rectangles and the boxes are the frames' stretched textures, so that an
//! area dragged costs a frame and no drawing of pixels (but its size's
//! few glyphs).

use std::{
    cell::{OnceCell, RefCell},
    time::Duration,
};

use hocus_link::{CAPTURE_TOOLBAR_RADIUS, CaptureMode, LAUNCHER_ZOOM};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, Id, Kind, memory::MemoryRenderBuffer, solid::SolidColorRenderElement,
            surface::WaylandSurfaceRenderElement,
        },
    },
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale},
};

use super::{FLASH, FLASH_FROM, Phase, TICK, geometry::Area};
use crate::{
    State,
    decoration::{
        self, ACCENT, BoxStyle, FrameIds, Keyed, Outside, Placed, Rounded, ShadowStyle,
        box_elements, shadow_elements,
    },
    going::Shaders,
    motion::{Bezier, Tween, lerp},
    render::{OutputElement, OutputElements},
    wobble::Bend,
};

const Z: f64 = LAUNCHER_ZOOM;
/// The mockup's `rgba(6, 8, 14, 0.55)` outside what is chosen.
const SHADE: [f32; 4] = [6.0 / 255.0, 8.0 / 255.0, 14.0 / 255.0, 0.55];
/// The area's 1.5 px white border (whole pixels: 2 at the launcher's
/// zoom on a screen at 1), four solid sides: a box's stretched middle
/// would blur its rim inward over a large area.
const BORDER: [f32; 4] = [
    0xEE as f32 / 255.0,
    0xF1 as f32 / 255.0,
    0xF6 as f32 / 255.0,
    1.0,
];
const BORDER_WIDTH: f64 = 1.5 * Z;
/// The window's green ring, on its corners.
const RING: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: ACCENT,
    rim_width: (1.5 * Z * 1000.0) as u32,
    radius: (decoration::RADIUS * 1000.0) as u32,
};
/// The handles: 8 px white dots.
const HANDLE: f64 = 8.0 * Z;
const DOT: BoxStyle = BoxStyle {
    fill: [0xFF; 4],
    rim: [0; 4],
    rim_width: 0,
    radius: (HANDLE / 2.0 * 1000.0) as u32,
};
/// The size over the area: Martian Mono 10.5 px, 2 px by 7 px in, radius
/// 6, on `rgba(14, 17, 28, 0.92)`, its top 22 px above the area's.
const LABEL_SIZE: f64 = 10.5 * Z;
const LABEL_PAD: (f64, f64) = (7.0 * Z, 2.0 * Z);
const LABEL_ABOVE: f64 = 22.0 * Z;
const LABEL: BoxStyle = BoxStyle {
    fill: [14, 17, 28, 235],
    rim: [0; 4],
    rim_width: 0,
    radius: (6.0 * Z * 1000.0) as u32,
};
const TEXT: [u8; 4] = [0xEE, 0xF1, 0xF6, 0xFF];
/// The countdown: a 110 px disc of `rgba(14, 17, 28, 0.85)`, its number
/// in Martian Mono 48 px, coming in from 1.25 times its size and 0.3 of
/// its opacity (the mockup's `tick`).
const DISC: f64 = 110.0 * Z;
const DISC_STYLE: BoxStyle = BoxStyle {
    fill: [14, 17, 28, 217],
    rim: [0; 4],
    rim_width: 0,
    radius: (DISC / 2.0 * 1000.0) as u32,
};
const NUMBER: f64 = 48.0 * Z;
/// The toolbar's shadow, the mockup's `0 14px 40px rgba(0, 0, 0, 0.45)`.
const TOOLBAR_SHADOW: ShadowStyle = ShadowStyle {
    dy: 14.0 * Z,
    blur: 40.0 * Z,
    alpha: 0.45,
    radius: CAPTURE_TOOLBAR_RADIUS,
};
/// Texts drawn kept, the last ones.
const TEXTS_KEPT: usize = 16;

/// Martian Mono, the shell's own (SIL Open Font License 1.1, its notice in
/// `crates/hocus-shell/fonts`): read once, the first time a size shows.
static MARTIAN_MONO: &[u8] = include_bytes!("../../../hocus-shell/fonts/MartianMono-Regular.ttf");

/// The capture's element ids and its texts' textures.
pub struct Drawn {
    font: OnceCell<Option<fontdue::Font>>,
    texts: RefCell<Vec<Text>>,
    shade: [Id; 4],
    flash: Id,
    border: [Id; 4],
    ring: [Id; 9],
    handles: [[Id; 9]; 8],
    label: [Id; 9],
    disc: [Id; 9],
    shadow: FrameIds,
}

impl std::fmt::Debug for Drawn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drawn")
            .field("texts", &self.texts.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Default for Drawn {
    fn default() -> Self {
        let nine = || std::array::from_fn(|_| Id::new());
        Self {
            font: OnceCell::new(),
            texts: RefCell::default(),
            shade: std::array::from_fn(|_| Id::new()),
            flash: Id::new(),
            border: std::array::from_fn(|_| Id::new()),
            ring: nine(),
            handles: std::array::from_fn(|_| nine()),
            label: nine(),
            disc: nine(),
            shadow: FrameIds::default(),
        }
    }
}

/// A text drawn: what it says at what size (hundredths of a pixel), its
/// texture, its size in pixels, and an id of its own (a texture of
/// another text is other pixels).
#[derive(Clone)]
struct Text {
    said: String,
    px: u32,
    texture: MemoryRenderBuffer,
    size: (i32, i32),
    id: Id,
}

/// Where the toolbar's glass goes among the elements, and over what.
#[derive(Debug, Clone, Copy)]
pub struct ToolbarGlass {
    pub at: usize,
    pub rect: Rectangle<f64, Logical>,
    pub alpha: f32,
}

impl Drawn {
    /// `said` in Martian Mono `px` pixels high, white, as premultiplied
    /// ARGB.
    fn text(&self, said: &str, px: f64) -> Option<Text> {
        let key = (px * 100.0).round() as u32;
        if let Some(text) = self
            .texts
            .borrow()
            .iter()
            .find(|t| t.px == key && t.said == said)
        {
            return Some(text.clone());
        }
        let font = self
            .font
            .get_or_init(|| {
                fontdue::Font::from_bytes(MARTIAN_MONO, fontdue::FontSettings::default())
                    .map_err(|err| tracing::warn!(err, "the capture's font does not read"))
                    .ok()
            })
            .as_ref()?;
        let (pixels, w, h) = rasterize(font, said, px as f32)?;
        let text = Text {
            said: said.to_owned(),
            px: key,
            texture: decoration::buffer(&pixels, w, h),
            size: (w, h),
            id: Id::new(),
        };
        let mut texts = self.texts.borrow_mut();
        if texts.len() >= TEXTS_KEPT {
            texts.remove(0);
        }
        texts.push(text.clone());
        Some(text)
    }
}

/// `said` drawn in `font` at `px`, in [`TEXT`]'s colour: its pixels and
/// their size.
fn rasterize(font: &fontdue::Font, said: &str, px: f32) -> Option<(Vec<u8>, i32, i32)> {
    let line = font.horizontal_line_metrics(px)?;
    let width: f32 = said
        .chars()
        .map(|c| font.metrics(c, px).advance_width)
        .sum();
    let (w, h) = (
        width.ceil() as i32,
        (line.ascent - line.descent).ceil() as i32,
    );
    if w <= 0 || h <= 0 {
        return None;
    }
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let mut pen = 0.0f32;
    for c in said.chars() {
        let (metrics, coverage) = font.rasterize(c, px);
        let gx = (pen + metrics.xmin as f32).round() as i32;
        let gy = (line.ascent - (metrics.height as f32 + metrics.ymin as f32)).round() as i32;
        for (row, run) in coverage.chunks(metrics.width.max(1)).enumerate() {
            for (col, &a) in run.iter().enumerate() {
                let (x, y) = (gx + col as i32, gy + row as i32);
                if a == 0 || x < 0 || y < 0 || x >= w || y >= h {
                    continue;
                }
                let at = ((y * w + x) * 4) as usize;
                let alpha = u32::from(a) * u32::from(TEXT[3]) / 255;
                let premultiply = |v: u8| (u32::from(v) * alpha / 255) as u8;
                let px = [
                    premultiply(TEXT[2]),
                    premultiply(TEXT[1]),
                    premultiply(TEXT[0]),
                    alpha as u8,
                ];
                for k in 0..4 {
                    pixels[at + k] = pixels[at + k].max(px[k]);
                }
            }
        }
        pen += metrics.advance_width;
    }
    Some((pixels, w, h))
}

/// `rect` (logical, output-local) in the output's pixels, its edges
/// rounded each: rectangles side by side meet without a gap.
fn pixels(rect: Rectangle<f64, Logical>, scale: f64) -> Rectangle<i32, Physical> {
    let a: Point<i32, Physical> = rect.loc.to_physical_precise_round(scale);
    let b: Point<i32, Physical> =
        (rect.loc + rect.size.to_point()).to_physical_precise_round(scale);
    Rectangle::new(a, (b.x - a.x, b.y - a.y).into())
}

/// The four sides of a border inside `edge` (pixels), as wide as
/// [`BORDER_WIDTH`] at `scale`, in whole pixels.
fn border(edge: Rectangle<i32, Physical>, scale: f64) -> [Rectangle<i32, Physical>; 4] {
    let w = ((BORDER_WIDTH * scale).round() as i32).clamp(1, edge.size.w.min(edge.size.h).max(1));
    let (x, y, ew, eh) = (edge.loc.x, edge.loc.y, edge.size.w, edge.size.h);
    [
        Rectangle::new((x, y).into(), (ew, w).into()),
        Rectangle::new((x, y + eh - w).into(), (ew, w).into()),
        Rectangle::new((x, y + w).into(), (w, eh - 2 * w).into()),
        Rectangle::new((x + ew - w, y + w).into(), (w, eh - 2 * w).into()),
    ]
}

fn rect(area: Area) -> Rectangle<f64, Logical> {
    Rectangle::new((area.x, area.y).into(), (area.w, area.h).into())
}

/// A solid rectangle, premultiplied `rgba`; `commit` says when its colour
/// changed (the flash fading).
fn solid(
    id: &Id,
    at: Rectangle<i32, Physical>,
    rgba: [f32; 4],
    commit: usize,
) -> SolidColorRenderElement {
    SolidColorRenderElement::new(id.clone(), at, commit, rgba, Kind::Unspecified)
}

impl State {
    /// The capture on `output` at `now`, front first; where the toolbar's
    /// glass goes, when it shows there.
    pub(crate) fn capture_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) -> Option<ToolbarGlass>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let name = output.name();
        let geometry = self.space.output_geometry(output)?;
        let size = (f64::from(geometry.size.w), f64::from(geometry.size.h));
        let origin = (f64::from(geometry.loc.x), f64::from(geometry.loc.y));
        let scale = output.current_scale().fractional_scale();
        let drawn = &self.capture.drawn;
        let screen = Rectangle::<f64, Logical>::from_size(size.into());
        // The flash over what was taken.
        if let Some((parts, since)) = &self.capture.flash {
            let t = now.saturating_sub(*since).as_secs_f64() / FLASH.as_secs_f64();
            if t < 1.0 {
                let alpha = FLASH_FROM * (1.0 - Bezier::EASE_OUT.at(t)) as f32;
                for (on, area) in parts.iter().filter(|(on, _)| *on == name) {
                    let _ = on;
                    out.push(OutputElements::Solid(solid(
                        &drawn.flash,
                        pixels(rect(*area), scale),
                        [alpha, alpha, alpha, alpha],
                        (alpha * 1000.0) as usize,
                    )));
                }
            }
        }
        match &self.capture.phase {
            Phase::Closed => None,
            Phase::Counting(counting) => {
                if counting.output == name {
                    self.countdown(
                        renderer,
                        counting.left,
                        counting.since,
                        now,
                        size,
                        scale,
                        out,
                    );
                }
                None
            }
            Phase::Choosing(choosing) => {
                let glass = self.toolbar_elements(renderer, output, now, scale, out);
                // What is chosen on this screen, output-local.
                let chosen = match self.capture.mode {
                    CaptureMode::Area => self
                        .capture
                        .area
                        .as_ref()
                        .filter(|(on, _)| *on == name)
                        .map(|(_, a)| Some(rect(*a))),
                    CaptureMode::Window => choosing.hovered.and_then(|id| {
                        let r = self.rect_of(id)?;
                        let r = Rectangle::<f64, Logical>::new(
                            (f64::from(r.x) - origin.0, f64::from(r.y) - origin.1).into(),
                            (f64::from(r.width), f64::from(r.height)).into(),
                        );
                        Some(Some(r))
                    }),
                    CaptureMode::Screen => {
                        let under = self.output_under_pointer().is_some_and(|o| o == *output);
                        under.then_some(None)
                    }
                    CaptureMode::All => Some(None),
                };
                if let Some(Some(r)) = chosen {
                    let whole = Area {
                        x: r.loc.x,
                        y: r.loc.y,
                        w: r.size.w,
                        h: r.size.h,
                    };
                    let real = (
                        (whole.w * scale).round() as i64,
                        (whole.h * scale).round() as i64,
                    );
                    self.size_label(renderer, whole, real, scale, out);
                    if self.capture.mode == CaptureMode::Area {
                        let drawing = matches!(choosing.drag, Some(super::Drag::Draw { .. }));
                        if !drawing && whole.drawn() {
                            for (i, (x, y)) in whole.handles().into_iter().enumerate() {
                                let dot = Rectangle::new(
                                    (x - HANDLE / 2.0, y - HANDLE / 2.0).into(),
                                    (HANDLE, HANDLE).into(),
                                );
                                out.extend(
                                    box_elements(
                                        renderer,
                                        &self.textures,
                                        &drawn.handles[i],
                                        DOT,
                                        dot,
                                        scale,
                                        1.0,
                                    )
                                    .into_iter()
                                    .map(OutputElements::Frame),
                                );
                            }
                        }
                        let edge = pixels(r, scale);
                        for (id, part) in drawn.border.iter().zip(border(edge, scale)) {
                            out.push(OutputElements::Solid(solid(id, part, BORDER, 0)));
                        }
                    } else {
                        out.extend(
                            box_elements(
                                renderer,
                                &self.textures,
                                &drawn.ring,
                                RING,
                                r,
                                scale,
                                1.0,
                            )
                            .into_iter()
                            .map(OutputElements::Frame),
                        );
                    }
                }
                // The darkening: around what is chosen, or all of the
                // screen when nothing of it is.
                let parts: Vec<Rectangle<f64, Logical>> = match chosen {
                    Some(Some(r)) => {
                        let inside = r.intersection(screen).unwrap_or_default();
                        let area = Area {
                            x: inside.loc.x,
                            y: inside.loc.y,
                            w: inside.size.w,
                            h: inside.size.h,
                        };
                        area.around(size).into_iter().map(rect).collect()
                    }
                    Some(None) => Vec::new(),
                    None => vec![screen],
                };
                for (part, id) in parts.into_iter().zip(&drawn.shade) {
                    let at = pixels(part, scale);
                    if at.size.w > 0 && at.size.h > 0 {
                        out.push(OutputElements::Solid(solid(id, at, SHADE, 0)));
                    }
                }
                glass
            }
        }
    }

    /// The toolbar's surface on `output`, fading in from its first frame,
    /// and its shadow; where its glass goes.
    fn toolbar_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        scale: f64,
        out: &mut Vec<OutputElement<R>>,
    ) -> Option<ToolbarGlass>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let (layer, opened) = self.capture.toolbar.as_ref()?;
        let map = smithay::desktop::layer_map_for_output(output);
        if !map.layers().any(|l| l == layer) {
            return None;
        }
        let geometry = map.layer_geometry(layer)?;
        drop(map);
        let alpha = Tween::new(*opened, hocus_link::CAPTURE_TOOLBAR_IN, Bezier::EASE_OUT)
            .at(now)
            .clamp(0.0, 1.0) as f32;
        if alpha <= 0.0 {
            return None;
        }
        let loc = geometry.loc.to_physical_precise_round(scale);
        let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
            AsRenderElements::<R>::render_elements(layer, renderer, loc, Scale::from(scale), alpha);
        if self.shell.software {
            let clip = Rectangle::new(loc, geometry.size.to_physical_precise_round(scale));
            let radius = (CAPTURE_TOOLBAR_RADIUS * scale).round() as i32;
            out.extend(surfaces.into_iter().map(|e| {
                OutputElements::Bar(Rounded::new(e, clip, radius, Outside::Cropped, scale))
            }));
        } else {
            out.extend(surfaces.into_iter().map(OutputElements::Surface));
        }
        out.extend(
            shadow_elements(
                renderer,
                &self.textures,
                &self.capture.drawn.shadow,
                TOOLBAR_SHADOW,
                geometry.to_f64(),
                scale,
                alpha,
            )
            .into_iter()
            .map(OutputElements::Shadow),
        );
        Some(ToolbarGlass {
            at: out.len(),
            rect: geometry.to_f64(),
            alpha,
        })
    }

    /// "600 × 380", the size chosen in the screen's pixels, over its top
    /// left corner (inside it when there is no room above).
    fn size_label<R>(
        &self,
        renderer: &mut R,
        area: Area,
        real: (i64, i64),
        scale: f64,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let drawn = &self.capture.drawn;
        let said = format!("{} × {}", real.0, real.1);
        let Some(text) = drawn.text(&said, LABEL_SIZE * scale) else {
            return;
        };
        let (w, h) = (
            f64::from(text.size.0) / scale,
            f64::from(text.size.1) / scale,
        );
        let box_size = (w + 2.0 * LABEL_PAD.0, h + 2.0 * LABEL_PAD.1);
        let top = if area.y - LABEL_ABOVE >= 0.0 {
            area.y - LABEL_ABOVE
        } else {
            area.y + LABEL_PAD.1 * 2.0
        };
        let at = Point::<f64, Logical>::from((area.x + LABEL_PAD.0, top + LABEL_PAD.1));
        let dst = Rectangle::new(at.to_physical_precise_round(scale), text.size.into());
        let all = Rectangle::from_size((f64::from(text.size.0), f64::from(text.size.1)).into());
        match Placed::new(renderer, &text.texture, all, dst, 1.0) {
            Ok(e) => out.push(OutputElements::Frame(Keyed::new(e, text.id.clone()))),
            Err(err) => tracing::warn!(?err, "could not draw the capture's size"),
        }
        let backing = Rectangle::new((area.x, top).into(), box_size.into());
        out.extend(
            box_elements(
                renderer,
                &self.textures,
                &drawn.label,
                LABEL,
                backing,
                scale,
                1.0,
            )
            .into_iter()
            .map(OutputElements::Frame),
        );
    }

    /// The countdown's disc and its number `left`, shown since `since`, in
    /// the middle of a screen `size` large.
    #[allow(clippy::too_many_arguments)]
    fn countdown<R>(
        &self,
        renderer: &mut R,
        left: u8,
        since: Duration,
        now: Duration,
        size: (f64, f64),
        scale: f64,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let drawn = &self.capture.drawn;
        let t = if self.reduced_motion() {
            1.0
        } else {
            Tween::new(since, TICK, Bezier::EASE_OUT)
                .at(now)
                .clamp(0.0, 1.0)
        };
        let alpha = lerp(0.3, 1.0, t) as f32;
        let k = lerp(1.25, 1.0, t);
        let (cx, cy) = (size.0 / 2.0, size.1 / 2.0);
        if let Some(text) = drawn.text(&left.to_string(), NUMBER * scale) {
            let (w, h) = (f64::from(text.size.0) * k, f64::from(text.size.1) * k);
            let centre = Point::<f64, Logical>::from((cx, cy)).to_physical(scale);
            let dst = Rectangle::new(
                (
                    (centre.x - w / 2.0).round() as i32,
                    (centre.y - h / 2.0).round() as i32,
                )
                    .into(),
                (w.round() as i32, h.round() as i32).into(),
            );
            let all = Rectangle::from_size((f64::from(text.size.0), f64::from(text.size.1)).into());
            match Placed::new(renderer, &text.texture, all, dst, alpha) {
                Ok(e) => out.push(OutputElements::Frame(Keyed::new(e, text.id.clone()))),
                Err(err) => tracing::warn!(?err, "could not draw the countdown"),
            }
        }
        let disc = Rectangle::new(
            (cx - DISC / 2.0, cy - DISC / 2.0).into(),
            (DISC, DISC).into(),
        );
        out.extend(
            box_elements(
                renderer,
                &self.textures,
                &drawn.disc,
                DISC_STYLE,
                disc,
                scale,
                alpha,
            )
            .into_iter()
            .map(OutputElements::Frame),
        );
    }

    /// The glass under the toolbar on `output`: what is `behind` it,
    /// blurred, cut to its corners, as under the popovers. None without
    /// glass.
    pub(crate) fn capture_glass<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        glass: ToolbarGlass,
        behind: &[OutputElement<R>],
    ) -> Option<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if !self.glass_on() {
            return None;
        }
        let size = self.space.output_geometry(output)?.size;
        let scale = output.current_scale().fractional_scale();
        let reach = crate::glass::REACH;
        let screen = Rectangle::<f64, Logical>::from_size(size.to_f64());
        let r = glass.rect;
        let around = Rectangle::<f64, Logical>::new(
            (r.loc.x - reach, r.loc.y - reach).into(),
            (r.size.w + 2.0 * reach, r.size.h + 2.0 * reach).into(),
        )
        .intersection(screen)?;
        let blurred = renderer.frost(
            &mut self.panes.borrow_mut(),
            &crate::glass::capture_pane(&output.name()),
            around.to_physical_precise_round(scale),
            scale,
            crate::glass::Frost::BAR,
            behind,
        )?;
        crate::render::glass_piece(
            renderer,
            blurred,
            r.to_physical_precise_round(scale),
            scale,
            glass.alpha,
            CAPTURE_TOOLBAR_RADIUS,
        )
    }
}
