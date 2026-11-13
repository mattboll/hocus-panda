//! What sharing draws on the screens: the chooser (the shell's surface,
//! popping in, its shadow, the darkening under it, and the thumbnails
//! over it where the shell says they go), and the ring around what is
//! shared. None of it is ever in a stream or a capture.

use std::{cell::RefCell, collections::HashMap, time::Duration};

use hocus_link::{
    LAUNCHER_ZOOM, SHARE_CHOOSER_FROM, SHARE_CHOOSER_IN, SHARE_CHOOSER_RADIUS, ShareSource,
};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, Id, Kind, memory::MemoryRenderBufferRenderElement,
            solid::SolidColorRenderElement, surface::WaylandSurfaceRenderElement,
            utils::RescaleRenderElement,
        },
    },
    desktop::layer_map_for_output,
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale},
};

use super::ring::{self, Breath};
use crate::{
    State,
    decoration::{
        self, BoxStyle, FrameIds, Outside, Rounded, ShadowStyle, box_elements, shadow_elements,
    },
    going::{ShaderElement, Shaders},
    motion::Bezier,
    render::{OutputElement, OutputElements},
    wobble::Bend,
};

/// The mockup's `rgba(6, 8, 14, 0.55)` under the chooser.
const SHADE: [f32; 4] = [6.0 / 255.0, 8.0 / 255.0, 14.0 / 255.0, 0.55];
/// The chooser's `0 22px 60px rgba(0, 0, 0, 0.5)`, zoomed.
const SHADOW: ShadowStyle = ShadowStyle {
    dx: 0.0,
    dy: 22.0 * LAUNCHER_ZOOM,
    blur: 60.0 * LAUNCHER_ZOOM,
    alpha: 0.5,
    radius: SHARE_CHOOSER_RADIUS,
};
/// Its pop's curve, `cubic-bezier(.2, .8, .2, 1)`.
const POP: Bezier = Bezier(0.2, 0.8, 0.2, 1.0);
/// A screen's ring: on a box 2 px in from its edges, radius 6 (the
/// mockup's `inset: 2px; border-radius: 6px`).
const SCREEN_INSET: f64 = 2.0;
const SCREEN_RADIUS: f64 = 6.0;

/// The elements' ids sharing keeps.
#[derive(Debug)]
pub struct Drawn {
    shade: Id,
    shadow: FrameIds,
    /// Each share's ring as last drawn, by share and output.
    rings: RefCell<HashMap<(u64, String), (ring::Key, ShaderElement)>>,
}

impl Default for Drawn {
    fn default() -> Self {
        Self {
            shade: Id::new(),
            shadow: FrameIds::default(),
            rings: RefCell::default(),
        }
    }
}

/// The chooser's look at `t` since its first frame: opaque, its scale.
fn pop(t: Duration, motion: bool) -> (f32, f64) {
    if !motion {
        return (1.0, 1.0);
    }
    let k = (t.as_secs_f64() / SHARE_CHOOSER_IN.as_secs_f64()).clamp(0.0, 1.0);
    let p = POP.at(k);
    (
        p as f32,
        SHARE_CHOOSER_FROM + (1.0 - SHARE_CHOOSER_FROM) * p,
    )
}

impl State {
    /// What sharing shows on `output` at `now`, in front of everything but
    /// the cursor, front first: the chooser (its thumbnails, its surface,
    /// its shadow, the darkening), then the ring around the screen when it
    /// is shared.
    pub(crate) fn share_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = output.current_scale().fractional_scale();
        self.chooser_elements(renderer, output, now, scale, out);
        let name = output.name();
        let Some(size) = self.space.output_geometry(output).map(|g| g.size) else {
            return;
        };
        let shared: Vec<(u64, Duration)> = self
            .share
            .shares
            .values()
            .filter(|s| s.node.is_some())
            .filter(|s| matches!(&s.source, ShareSource::Screen(n) if *n == name))
            .map(|s| (s.id, s.since))
            .collect();
        for (id, since) in shared {
            let around = Rectangle::<f64, Logical>::new(
                (SCREEN_INSET, SCREEN_INSET).into(),
                (
                    f64::from(size.w) - 2.0 * SCREEN_INSET,
                    f64::from(size.h) - 2.0 * SCREEN_INSET,
                )
                    .into(),
            );
            out.extend(self.ring_elements(
                renderer,
                id,
                &name,
                around,
                SCREEN_RADIUS,
                scale,
                now,
                since,
            ));
        }
    }

    fn chooser_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        scale: f64,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(choosing) = &self.share.choosing else {
            return;
        };
        if choosing.output != output.name() {
            return;
        }
        let Some(size) = self.space.output_geometry(output).map(|g| g.size) else {
            return;
        };
        let surface = choosing.surface.as_ref().and_then(|(layer, since)| {
            let geometry = layer_map_for_output(output).layer_geometry(layer)?;
            Some((layer.clone(), *since, geometry))
        });
        let motion = !self.reduced_motion() && !self.shell.software;
        let (alpha, k) = match &surface {
            Some((_, since, _)) => pop(now.saturating_sub(*since), motion),
            None => (0.0, SHARE_CHOOSER_FROM),
        };
        if let Some((layer, _, geometry)) = &surface
            && alpha > 0.0
        {
            // The thumbnails, once it popped in.
            if k >= 1.0 {
                for (source, rect) in &choosing.rects {
                    let Some((buffer, (w, h))) = self.share.pictures.get(source) else {
                        continue;
                    };
                    let (lw, lh) = (f64::from(*w) / scale, f64::from(*h) / scale);
                    let at = Point::<f64, Logical>::from((
                        f64::from(geometry.loc.x + rect.x) + (f64::from(rect.width) - lw) / 2.0,
                        f64::from(geometry.loc.y + rect.y) + (f64::from(rect.height) - lh) / 2.0,
                    ));
                    let at: Point<i32, Physical> = at.to_physical_precise_round(scale);
                    match MemoryRenderBufferRenderElement::from_buffer(
                        renderer,
                        at.to_f64(),
                        buffer,
                        Some(alpha),
                        None,
                        Some((lw.round() as i32, lh.round() as i32).into()),
                        Kind::Unspecified,
                    ) {
                        Ok(element) => out.push(OutputElements::Backdrop(element)),
                        Err(err) => tracing::debug!(%err, "a chooser's thumbnail not drawn"),
                    }
                }
            }
            let loc: Point<i32, Physical> = geometry.loc.to_physical_precise_round(scale);
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                AsRenderElements::<R>::render_elements(
                    layer,
                    renderer,
                    loc,
                    Scale::from(scale),
                    alpha,
                );
            if self.shell.software {
                // The CPU's buffers have no alpha: cut to its corners.
                let clip = Rectangle::new(loc, geometry.size.to_physical_precise_round(scale));
                let radius = (SHARE_CHOOSER_RADIUS * scale).round() as i32;
                out.extend(surfaces.into_iter().map(|e| {
                    OutputElements::Bar(Rounded::new(e, clip, radius, Outside::Cropped, scale))
                }));
            } else if k < 1.0 {
                let middle = Point::<f64, Logical>::from((
                    f64::from(geometry.loc.x) + f64::from(geometry.size.w) / 2.0,
                    f64::from(geometry.loc.y) + f64::from(geometry.size.h) / 2.0,
                ))
                .to_physical_precise_round(scale);
                out.extend(surfaces.into_iter().map(|e| {
                    OutputElements::Scaled(RescaleRenderElement::from_element(e, middle, k))
                }));
            } else {
                out.extend(surfaces.into_iter().map(OutputElements::Surface));
            }
            if !self.shell.software {
                out.extend(
                    shadow_elements(
                        renderer,
                        &self.textures,
                        &self.share.drawn.shadow,
                        SHADOW,
                        geometry.to_f64(),
                        scale,
                        alpha,
                    )
                    .into_iter()
                    .map(OutputElements::Shadow),
                );
            }
        }
        // The darkening under the chooser, fading in with it.
        if alpha <= 0.0 {
            return;
        }
        let a = SHADE[3] * alpha;
        let whole =
            Rectangle::<f64, Logical>::from_size(size.to_f64()).to_physical_precise_round(scale);
        out.push(OutputElements::Solid(SolidColorRenderElement::new(
            self.share.drawn.shade.clone(),
            whole,
            (a * 1000.0) as usize,
            [SHADE[0] * a, SHADE[1] * a, SHADE[2] * a, a],
            Kind::Unspecified,
        )));
    }

    /// The ring of share `id` around `around` (output-local, logical, the
    /// thing's edge), its corners `radius` round, at `now`; its breath
    /// from `since`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ring_elements<R>(
        &self,
        renderer: &mut R,
        id: u64,
        output: &str,
        around: Rectangle<f64, Logical>,
        radius: f64,
        scale: f64,
        now: Duration,
        since: Duration,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let still = self.reduced_motion();
        let (breath, _) = Breath::at(now.saturating_sub(since), still);
        if let Some(program) = renderer.ring_program() {
            let key = ring::Key {
                around,
                radius,
                breath,
                scale,
            };
            let mut rings = self.share.drawn.rings.borrow_mut();
            let slot = (id, output.to_owned());
            let element = match rings.get(&slot) {
                Some((k, element)) if *k == key => element.clone(),
                _ => {
                    let element = ring::element(&program, &key);
                    rings.insert(slot, (key, element.clone()));
                    element
                }
            };
            return vec![OutputElements::Shader(element)];
        }
        // Without shaders: the ring alone, on its corners.
        let Some(share) = self.share.shares.get(&id) else {
            return Vec::new();
        };
        let [r, g, b] = ring::BLUE;
        let style = BoxStyle {
            fill: [0; 4],
            rim: [r, g, b, (Breath::FULL.ring * 255.0) as u8],
            rim_width: (ring::WIDTH * 1000.0) as u32,
            radius: ((radius + ring::WIDTH) * 1000.0) as u32,
        };
        let outside = Rectangle::new(
            around.loc - Point::from((ring::WIDTH, ring::WIDTH)),
            around.size + (2.0 * ring::WIDTH, 2.0 * ring::WIDTH).into(),
        );
        box_elements(
            renderer,
            &self.textures,
            &share.ring_ids,
            style,
            outside,
            scale,
            1.0,
        )
        .into_iter()
        .map(OutputElements::Frame)
        .collect()
    }

    /// The ring of a shared window `window`, drawn where it is now
    /// (`visual`, output-local), if it is shared.
    pub(crate) fn window_ring<R>(
        &self,
        renderer: &mut R,
        window: crate::model::WindowId,
        output: &str,
        visual: Rectangle<f64, Logical>,
        scale: f64,
        now: Duration,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(share) = self
            .share
            .shares
            .values()
            .find(|s| s.node.is_some() && s.source == ShareSource::Window(window.0))
        else {
            return Vec::new();
        };
        let radius = if self.model.mode(window) == crate::model::Mode::Fullscreen {
            0.0
        } else {
            decoration::RADIUS
        };
        self.ring_elements(
            renderer,
            share.id,
            output,
            visual,
            radius,
            scale,
            now,
            share.since,
        )
    }

    /// Share `id` ended: its rings go.
    pub(crate) fn drawn_ring_forget(&self, id: u64) {
        self.share
            .drawn
            .rings
            .borrow_mut()
            .retain(|(share, _), _| *share != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chooser_pops_in_and_stays() {
        let (a, k) = pop(Duration::ZERO, true);
        assert_eq!(a, 0.0);
        assert!((k - SHARE_CHOOSER_FROM).abs() < 1e-9);
        assert_eq!(pop(SHARE_CHOOSER_IN, true), (1.0, 1.0));
        assert_eq!(pop(Duration::ZERO, false), (1.0, 1.0));
    }
}
