//! Where a dragged window would snap, and the windows leaving.

use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    utils::{Logical, Physical, Point, Rectangle},
};

use super::{Frame, OutputElement, OutputElements};
use crate::{
    State,
    decoration::{self, Outside, Rounded, preview_elements},
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
}
