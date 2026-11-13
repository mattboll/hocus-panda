//! The windows an output shows, front first, with their frames, a pair's
//! seam's handle, a shared window's ring and the actions palette's target.

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Kind, solid::SolidColorRenderElement},
    },
    utils::{Point, Rectangle},
};

use super::{Frame, Look, OutputElement, OutputElements, PROTECTED};
use crate::{
    State,
    decoration::{self, ShadowStyle, handle_element},
    going::Shaders,
    model::{Mode, WindowId},
    wobble::Bend,
};

/// The windows drawn, and the actions palette's target apart: it goes
/// over the palette's dim.
type Stack<R> = (Vec<OutputElement<R>>, Vec<OutputElement<R>>);

impl State {
    /// The windows on `frame`'s output, front first, where their desktop
    /// is on the strip, `cover` of them under the overview.
    pub(super) fn window_looks(&self, frame: &Frame, cover: Option<f32>) -> Vec<(WindowId, Look)> {
        let Frame { now, area, .. } = *frame;
        // The daylight's sun (#805): the shadows it casts, the edges it
        // lights; on the screen alone.
        let sun = frame
            .purpose
            .screen()
            .then(|| self.daylight_on(frame.output))
            .flatten();
        let hour = self.fx.daylight.as_ref().map_or(0, |d| d.generation);
        let width = f64::from(area.size.w);
        // Desktops in depth (#806): the windows sliding with the strip
        // pull back about the screen's middle with the camera.
        let pull = if frame.purpose.screen() {
            self.depth_pull(frame.output, now)
        } else {
            1.0
        };
        let middle = area.to_f64().loc + area.to_f64().size.downscale(2.0).to_point();
        self.model
            .stack()
            .iter()
            .rev()
            .copied()
            .filter(|id| !self.model.is_minimized(*id))
            .filter_map(|id| {
                let (offset, alpha) = self.strip_offset(id, now)?;
                let mut look = self.look(id, now)?;
                look.visual.loc.x += offset;
                look.alpha *= alpha;
                if pull < 1.0 && self.slides_with_strip(id) {
                    look.visual.loc = middle + (look.visual.loc - middle).upscale(pull);
                    look.visual.size = look.visual.size.upscale(pull);
                }
                if let Some(sky) = &sun
                    && self.model.mode(id) != Mode::Fullscreen
                {
                    look.shadow = sky.shadow(width, look.shadow);
                    look.sunlit = sky.lit_edges(width).map(|lit| (lit, hour));
                }
                if let Some(cover) = cover {
                    look.alpha *= cover;
                    if look.alpha <= 0.0 {
                        return None;
                    }
                }
                Some((id, look))
            })
            .filter(|(_, look)| {
                let shadow = look.shadow;
                let reach = 2.0 * ShadowStyle::MOVING.blur.max(shadow.blur)
                    + shadow.dx.abs().max(shadow.dy.abs());
                let around = Rectangle::new(
                    look.visual.loc - Point::from((reach, reach)),
                    look.visual.size + (2.0 * reach, 2.0 * reach).into(),
                );
                around.overlaps(area.to_f64())
            })
            .collect()
    }

    /// The windows of `looks` drawn; `palette`: where the actions palette
    /// is among the elements, its target and its alpha.
    pub(super) fn window_elements<R>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        looks: &[(WindowId, Look)],
        palette: Option<(usize, Option<WindowId>, f32)>,
    ) -> Stack<R>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Frame {
            output,
            purpose,
            now,
            area,
            origin,
            scale,
        } = *frame;
        let palette_target = palette.and_then(|(_, window, _)| window);
        let mut target = Vec::new();
        // The handle of a pair's seam, where the pair is now: right above
        // the front one of the two, so that a window over the seam covers
        // it (§8).
        let mut handle = self.snap.seam_handle.and_then(|(seam, handle)| {
            let live = self.live_seam(seam.first, seam.second)?;
            let (x, y) = live.centre();
            area.to_f64()
                .contains((x, y))
                .then(|| (live, handle.alpha(now)))
        });
        let mut windows = Vec::new();
        for (id, look) in looks {
            if let Some((seam, alpha)) = handle.take_if(|(s, _)| *id == s.first || *id == s.second)
            {
                let (x, y) = seam.centre();
                windows.extend(
                    handle_element(
                        renderer,
                        &self.textures,
                        &self.snap.seam_handle_id,
                        Point::from((x, y)) - origin,
                        seam.vertical,
                        scale,
                        alpha,
                    )
                    .map(OutputElements::Frame),
                );
            }
            if !purpose.screen() && self.protected(*id) {
                let r = Rectangle::new(look.visual.loc - origin, look.visual.size);
                windows.push(OutputElements::Solid(SolidColorRenderElement::new(
                    // A capture's buffer is drawn whole, once: any id.
                    smithay::backend::renderer::element::Id::new(),
                    r.to_physical_precise_round(scale),
                    0,
                    PROTECTED,
                    Kind::Unspecified,
                )));
                continue;
            }
            // A shared window's ring, right over it (5.11).
            if purpose.screen() {
                let visual = Rectangle::new(look.visual.loc - origin, look.visual.size);
                windows.extend(self.window_ring(renderer, *id, &output.name(), visual, scale, now));
                // An MCP agent's action, shown before it is done.
                windows.extend(self.mcp_ring(renderer, *id, visual, scale));
            }
            // The palette's target over its dim, ringed.
            let out = if palette_target == Some(*id) {
                &mut target
            } else {
                &mut windows
            };
            // Its creatures (#796), right in front of it.
            self.creature_elements(renderer, frame, *id, look, out);
            // Its snow (#795), right in front of it.
            if look.decorated {
                let cap = self.snow_cap(
                    renderer,
                    frame,
                    &crate::effects::snow::Holder::Window(*id),
                    look.visual,
                    look.alpha,
                );
                out.extend(cap.into_iter().map(OutputElements::Fire));
            }
            // Its garland (#799), right in front of it.
            if let Some(garland) = self.garland_front(renderer, frame, *id, look, looks) {
                out.push(OutputElements::Fire(garland));
            }
            // Raining (#432): through the rain, else fading over its time.
            let rain = look.rain.map(|fade| {
                (
                    fade,
                    self.rain_window(renderer, *id, look, origin, scale, now),
                )
            });
            match rain {
                Some((_, Some(rain))) => out.push(OutputElements::Rain(rain)),
                Some((fade, None)) => {
                    let mut faded = look.clone();
                    faded.alpha *= fade;
                    self.framed_window(renderer, *id, &faded, origin, scale, out);
                }
                None => match self.bent_window(renderer, *id, look, frame) {
                    Some(bent) => out.push(OutputElements::Bent(bent)),
                    None => self.framed_window(renderer, *id, look, origin, scale, out),
                },
            }
            if palette_target == Some(*id)
                && let Some((_, _, alpha)) = palette
            {
                let visual = Rectangle::new(look.visual.loc - origin, look.visual.size);
                let radius = if self.model.mode(*id) == Mode::Fullscreen {
                    0.0
                } else {
                    decoration::RADIUS
                };
                let mut ring = Vec::new();
                for ((style, around), ids) in State::actions_ring(visual, radius)
                    .into_iter()
                    .zip([&self.actions.glow, &self.actions.ring])
                {
                    ring.extend(
                        decoration::box_elements(
                            renderer,
                            &self.textures,
                            ids,
                            style,
                            around,
                            scale,
                            alpha,
                        )
                        .into_iter()
                        .map(OutputElements::Frame),
                    );
                }
                // Front first: the ring, then the glow under it.
                ring.reverse();
                target.splice(0..0, ring);
            }
        }
        (windows, target)
    }
}
