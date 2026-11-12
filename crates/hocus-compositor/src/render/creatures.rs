//! The creatures drawn (#796): each right in front of its window, as the
//! snow's caps, so that a window in front covers it and it covers no
//! window in front. A panda is its drawing's pieces laid on quads
//! (`effects::rig`), from one sheet a mood and size; its name tag a
//! picture of its own, under it. On the screen alone: never in a capture,
//! a share's or a recording's frames.

use std::rc::Rc;

use hocus_pandas::{Pose, creature};
use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    utils::{Physical, Rectangle},
};

use super::{Frame, Look, OutputElement, OutputElements};
use crate::{
    State,
    decoration::{Keyed, Placed},
    effects::{
        FireElement,
        creatures::{Drawn, Kept, TAG_HIGH, TAG_IN, draw_tag, spots, tag_buffer, tag_width},
        draw::SPRITE_FLOATS,
        rig::BOX,
    },
    going::Shaders,
    model::WindowId,
};

impl State {
    /// The creatures on window `id`'s edge, drawn where it is (`look`),
    /// front first: each panda, then its tag.
    pub(super) fn creature_elements<R>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        id: WindowId,
        look: &Look,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if !frame.purpose.screen() || self.fullscreen_in_front(frame.output) {
            return;
        }
        let Some(moving) = self.creatures_showing() else {
            return;
        };
        let live = &self.fx.creatures;
        let Some(list) = live.shown.get(&id) else {
            return;
        };
        let Frame {
            output,
            now,
            area,
            origin,
            scale,
            ..
        } = *frame;
        let fonts = list
            .iter()
            .any(|c| c.tag.is_some())
            .then(|| live.fonts.get_or_init(crate::radial_draw::fonts).as_ref())
            .flatten();
        let tags: Vec<f64> = list
            .iter()
            .map(|c| match (&c.tag, fonts) {
                (Some(words), Some(fonts)) => tag_width(fonts, words),
                _ => 0.0,
            })
            .collect();
        let edge = look.visual;
        let screen: Rectangle<i32, Physical> =
            Rectangle::from_size(area.size.to_f64().to_physical_precise_round(scale));
        for (creature, spot) in list.iter().zip(spots(list, edge.size.w, &tags)) {
            let Some(spot) = spot else {
                continue;
            };
            let (w, h) = (spot.width, spot.height());
            let left = edge.loc.x + spot.x;
            let top = edge.loc.y + spot.bottom() - h;
            // No room above the window: no free edge.
            if top < f64::from(area.loc.y) {
                continue;
            }
            let Some(rig) = live.rig(creature.mood) else {
                continue;
            };
            // Its sheet, at the screen's pixels.
            let k = (w * scale) as f32 / BOX.0;
            let key = (creature.mood, (k * 100.0).round() as u32);
            let kept = {
                let mut sheets = live.sheets.borrow_mut();
                if let std::collections::hash_map::Entry::Vacant(slot) = sheets.entry(key) {
                    let Some(sheet) = rig.sheet(key.1 as f32 / 100.0) else {
                        continue;
                    };
                    let Some(texture) =
                        renderer.sheet(&sheet.rgba, (sheet.size.0 as i32, sheet.size.1 as i32))
                    else {
                        continue;
                    };
                    slot.insert(Kept {
                        texture,
                        sheet: Rc::new(sheet),
                        used: std::cell::Cell::new(now),
                    });
                }
                let Some(kept) = sheets.get(&key) else {
                    continue;
                };
                kept.used.set(now);
                (kept.texture.clone(), kept.sheet.clone())
            };
            let (texture, sheet) = kept;
            let k = sheet.k;
            let pose =
                moving.then(|| creature::look(creature.mood, live.time(now, creature.agent)));
            let laid = rig.lay(pose.as_ref(), &sheet);
            // Its box on whole pixels, as its sheet is.
            let bx = ((left - origin.x) * scale).round() as f32;
            let by = ((top - origin.y) * scale).round() as f32;
            let (bw, bh) = (BOX.0 * k, BOX.1 * k);
            let whole = pose.as_ref().map_or(Pose::REST, |p| p.whole);
            let whole = Pose {
                dx: whole.dx * scale as f32,
                dy: whole.dy * scale as f32,
                ..whole
            }
            .affine((bx + bw / 2.0, by + bh));
            let alpha = spot.alpha * look.alpha;
            let mut vertices = Vec::with_capacity(laid.len() * 6 * SPRITE_FLOATS);
            let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
            for piece in &laid {
                let at = piece.corners.map(|(x, y)| {
                    let (x, y) = (bx + x * k, by + y * k);
                    let [a, b, c, d, e, f] = whole;
                    (a * x + c * y + e, b * x + d * y + f)
                });
                for i in [0, 1, 2, 1, 3, 2] {
                    let (Some((x, y)), Some((u, v))) = (at.get(i), piece.uv.get(i)) else {
                        continue;
                    };
                    lo = (lo.0.min(*x), lo.1.min(*y));
                    hi = (hi.0.max(*x), hi.1.max(*y));
                    vertices.extend_from_slice(&[*x, *y, *u, *v, piece.alpha * alpha]);
                }
            }
            let Some(geometry) = Rectangle::<i32, Physical>::new(
                (lo.0.floor() as i32, lo.1.floor() as i32).into(),
                (
                    (hi.0.ceil() - lo.0.floor()) as i32,
                    (hi.1.ceil() - lo.1.floor()) as i32,
                )
                    .into(),
            )
            .intersection(screen) else {
                continue;
            };
            let (element_id, tag_id, commit) = {
                let mut drawn = live.drawn.borrow_mut();
                let entry = drawn
                    .entry((id, creature.agent, output.name()))
                    .or_insert_with(|| Drawn {
                        id: smithay::backend::renderer::element::Id::new(),
                        tag: smithay::backend::renderer::element::Id::new(),
                        commit: Default::default(),
                        vertices: Vec::new(),
                        used: now,
                    });
                if entry.vertices != vertices {
                    entry.commit.increment();
                    entry.vertices.clone_from(&vertices);
                }
                entry.used = now;
                (entry.id.clone(), entry.tag.clone(), entry.commit)
            };
            live.frame_starts(now);
            live.last
                .borrow_mut()
                .1
                .push((id, creature.agent, [left, top, w, h]));
            out.push(OutputElements::Fire(FireElement::sprites(
                element_id, commit, geometry, texture, vertices,
            )));
            // Its tag, under it, on the window's edge.
            if let (Some(words), Some(fonts), Some(tag_w)) = (&creature.tag, fonts, spot.tag) {
                let milli = (scale * 1000.0).round() as u32;
                let mut tags = live.tags.borrow_mut();
                let picture = match tags.get(&(words.clone(), milli)) {
                    Some(p) => Some(p.clone()),
                    None => draw_tag(fonts, words, scale).map(|(pixels, size)| {
                        let p = (tag_buffer(&pixels, size), size);
                        tags.insert((words.clone(), milli), p.clone());
                        p
                    }),
                };
                drop(tags);
                if let Some((buffer, (pw, ph))) = picture {
                    let x = left + w / 2.0 - tag_w / 2.0;
                    let y = edge.loc.y + TAG_IN - TAG_HIGH;
                    let dst = Rectangle::<i32, Physical>::new(
                        (
                            ((x - origin.x) * scale).round() as i32,
                            ((y - origin.y) * scale).round() as i32,
                        )
                            .into(),
                        (pw, ph).into(),
                    );
                    let src = Rectangle::from_size((f64::from(pw), f64::from(ph)).into());
                    match Placed::new(renderer, &buffer, src, dst, look.alpha) {
                        Ok(e) => out.push(OutputElements::Frame(Keyed::new(e, tag_id))),
                        Err(err) => tracing::warn!(?err, "could not draw a creature's tag"),
                    }
                }
            }
        }
    }
}
