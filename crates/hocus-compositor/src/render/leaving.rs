//! Where a dragged window would snap, and the windows leaving: fading,
//! through their rain (#432), or burning (#794).

use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    utils::{Logical, Physical, Point, Rectangle},
};

use super::{Frame, OutputElement, OutputElements};
use crate::{
    State,
    decoration::{self, Outside, Rounded, preview_elements},
    effects::{
        FireElement,
        draw::{BurnLook, LightLook},
        flames::{Burn, LIGHT, MARGIN, MARGIN_ABOVE},
        particles::vertices,
    },
    going::Shaders,
    motion::lerp,
    wobble::{Bend, Picture},
};

impl State {
    /// Where a dragged window would snap, above the windows.
    pub(super) fn snap_elements<R>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        elements: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Frame {
            now, origin, scale, ..
        } = *frame;
        let preview = self
            .snap
            .preview
            .as_ref()
            .filter(|p| p.shown(now))
            .map(|p| (p.rect, p.alpha(now)))
            .or_else(|| {
                self.snap
                    .preview_out
                    .as_ref()
                    .map(|(rect, t)| (*rect, 1.0 - t.at(now) as f32))
            });
        if let Some((rect, alpha)) = preview {
            let r = crate::windows::rectangle(rect).to_f64();
            let r = Rectangle::new(r.loc - origin, r.size);
            elements.extend(
                preview_elements(
                    renderer,
                    &self.textures,
                    &self.snap.preview_ids,
                    r,
                    scale,
                    alpha,
                )
                .into_iter()
                .map(OutputElements::Frame),
            );
        }
    }

    /// Windows leaving, from their last frame.
    pub(super) fn leaving_elements<R>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        elements: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Frame {
            now,
            area,
            origin,
            scale,
            ..
        } = *frame;
        for leaving in self.departures.leaving.iter().rev() {
            // Burning (#794): on the screen alone, never in a capture, a
            // share's or a recording's frames.
            if let Some(burn) = &leaving.burn {
                if frame.purpose.screen() {
                    self.burn_elements(renderer, frame, leaving, burn, elements);
                }
                continue;
            }
            let p = leaving.tween.at(now);
            let rect = Rectangle::<f64, Logical>::new(
                (
                    lerp(leaving.from.loc.x, leaving.to.loc.x, p),
                    lerp(leaving.from.loc.y, leaving.to.loc.y, p),
                )
                    .into(),
                (
                    lerp(leaving.from.size.w, leaving.to.size.w, p),
                    lerp(leaving.from.size.h, leaving.to.size.h, p),
                )
                    .into(),
            );
            if !rect.overlaps(area.to_f64()) {
                continue;
            }
            let local = Rectangle::new(rect.loc - origin, rect.size);
            // Through its rain (#432), its last frame drawn once; a margin
            // around it for the thawing pane (#740).
            if let Some(rain) = &leaving.rain {
                let margin = match rain.style {
                    crate::rain::Style::Rain => 0.0,
                    crate::rain::Style::Frost => 8.0,
                };
                let picture = Picture {
                    window: local,
                    extent: Rectangle::new(
                        (-margin, -margin).into(),
                        (local.size.w + 2.0 * margin, local.size.h + 2.0 * margin).into(),
                    ),
                    scale,
                };
                let first = rain
                    .picture
                    .borrow()
                    .as_ref()
                    .is_none_or(|(s, _)| *s != picture.buffer_size());
                let mut parts = Vec::new();
                if first {
                    let at = Point::<f64, Logical>::from((margin, margin));
                    let clip = Rectangle::new(
                        at.to_physical_precise_round(scale),
                        local.size.to_physical_precise_round(scale),
                    );
                    let radius = (decoration::RADIUS * scale).round() as i32;
                    parts.extend(
                        leaving
                            .picture
                            .stretched(renderer, Rectangle::new(at, local.size), scale, 1.0)
                            .into_iter()
                            .map(|e| Rounded::new(e, clip, radius, Outside::Shown, scale)),
                    );
                }
                let look = self.rain_look(rain, now, &picture, 1.0);
                if let Some(element) = renderer.rain(rain, first, &picture, &parts, look) {
                    elements.push(OutputElements::Rain(element));
                    continue;
                }
            }
            let alpha = match &leaving.rain {
                Some(rain) => rain.fade(now),
                None => (1.0 - leaving.fade.linear(now)) as f32,
            };
            let clip: Rectangle<i32, Physical> = Rectangle::new(
                local.loc.to_physical_precise_round(scale),
                local.size.to_physical_precise_round(scale),
            );
            let k = (local.size.w / leaving.picture.size.w.max(1.0))
                .min(local.size.h / leaving.picture.size.h.max(1.0));
            let radius = (decoration::RADIUS * scale * k).round() as i32;
            elements.extend(
                leaving
                    .picture
                    .stretched(renderer, local, scale, alpha)
                    .into_iter()
                    .map(|e| {
                        OutputElements::Picture(Rounded::new(
                            e,
                            clip,
                            radius,
                            Outside::Shown,
                            scale,
                        ))
                    }),
            );
        }
    }

    /// A window burning (#794), front first: its sparks, its picture
    /// through the flames while the paper lasts, the warm light on what is
    /// under it while it burns. Where the renderer has no flames, its
    /// picture fades over the burn instead.
    fn burn_elements<R>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        leaving: &crate::windows::Leaving,
        burn: &Burn,
        elements: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Frame {
            now,
            area,
            origin,
            scale,
            ..
        } = *frame;
        burn.advance(now);
        let screen: Rectangle<i32, Physical> =
            Rectangle::from_size(area.size.to_f64().to_physical_precise_round(scale));
        let local = Rectangle::<f64, Logical>::new(burn.rect.loc - origin, burn.rect.size);
        let (mut smoke, mut embers) = (Vec::new(), Vec::new());
        let bounds = vertices(
            &burn.sparks.borrow().list,
            (origin.x, origin.y),
            scale,
            &mut smoke,
            &mut embers,
        );
        if let Some((x, y, w, h)) = bounds {
            let geometry = Rectangle::new((x, y).into(), (w, h).into());
            if geometry.overlaps(screen) {
                elements.push(OutputElements::Fire(FireElement::sparks(
                    burn.ids[1].clone(),
                    burn.next_commit(),
                    geometry,
                    smoke,
                    embers,
                )));
            }
        }
        if burn.burning(now) {
            let around = Rectangle::<f64, Logical>::new(
                (local.loc.x - MARGIN, local.loc.y - MARGIN_ABOVE).into(),
                (
                    local.size.w + 2.0 * MARGIN,
                    local.size.h + MARGIN_ABOVE + MARGIN,
                )
                    .into(),
            );
            let lo = around.loc.to_physical(scale);
            let hi = (around.loc + around.size.to_point()).to_physical(scale);
            let (x0, y0) = (lo.x.floor() as i32, lo.y.floor() as i32);
            let geometry: Rectangle<i32, Physical> = Rectangle::new(
                (x0, y0).into(),
                (hi.x.ceil() as i32 - x0, hi.y.ceil() as i32 - y0).into(),
            );
            if geometry.overlaps(screen) {
                let picture = Picture {
                    window: local,
                    extent: Rectangle::from_size(local.size),
                    scale,
                };
                let first = burn
                    .picture
                    .borrow()
                    .as_ref()
                    .is_none_or(|(s, _)| *s != picture.buffer_size());
                let clip = Rectangle::from_size(local.size.to_physical_precise_round(scale));
                let radius = (decoration::RADIUS * scale).round() as i32;
                let parts: Vec<_> = if first {
                    leaving
                        .picture
                        .stretched(renderer, Rectangle::from_size(local.size), scale, 1.0)
                        .into_iter()
                        .map(|e| Rounded::new(e, clip, radius, Outside::Shown, scale))
                        .collect()
                } else {
                    Vec::new()
                };
                let look = BurnLook {
                    size: [local.size.w as f32, local.size.h as f32],
                    corner: [
                        (f64::from(x0) / scale - local.loc.x) as f32,
                        (f64::from(y0) / scale - local.loc.y) as f32,
                    ],
                    scale: scale as f32,
                    radius: decoration::RADIUS as f32,
                    front: burn.front(now),
                    time: burn.time(now),
                };
                match renderer.burn(burn, &picture, &parts, geometry, look) {
                    Some(element) => elements.push(OutputElements::Fire(element)),
                    None => {
                        let alpha = 1.0 - burn.progress(now);
                        let clip = Rectangle::new(
                            local.loc.to_physical_precise_round(scale),
                            local.size.to_physical_precise_round(scale),
                        );
                        elements.extend(
                            leaving
                                .picture
                                .stretched(renderer, local, scale, alpha)
                                .into_iter()
                                .map(|e| {
                                    OutputElements::Picture(Rounded::new(
                                        e,
                                        clip,
                                        radius,
                                        Outside::Shown,
                                        scale,
                                    ))
                                }),
                        );
                    }
                }
            }
        }
        let fire = burn.fire(now);
        if fire > 0.01 {
            let cx = local.loc.x + local.size.w / 2.0;
            let cy = local.loc.y.max(burn.front_y(now) - origin.y);
            let reach = Rectangle::<f64, Logical>::new(
                (cx - LIGHT, cy - LIGHT).into(),
                (2.0 * LIGHT, 2.0 * LIGHT).into(),
            )
            .to_physical_precise_round(scale);
            if let Some(geometry) = reach.intersection(screen) {
                let look = LightLook {
                    centre: [
                        (cx * scale) as f32 - geometry.loc.x as f32,
                        (cy * scale) as f32 - geometry.loc.y as f32,
                    ],
                    inner: (10.0 * scale) as f32,
                    outer: (LIGHT * scale) as f32,
                    strength: fire * burn.flicker(now),
                };
                elements.push(OutputElements::Fire(FireElement::light(
                    burn.ids[2].clone(),
                    burn.next_commit(),
                    geometry,
                    look,
                )));
            }
        }
    }
}
