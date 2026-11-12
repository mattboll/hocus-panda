//! The garland drawn (#799): its cord and bulbs right in front of the
//! window it hangs under, as the snow's caps, so that a window in front
//! covers it; never on that window, nor on the bar. Their halos on the
//! wallpaper, under the windows and the bar (a light on the wallpaper
//! alone), in the layer's place behind the glass. On the screen alone:
//! never in a capture, a share's or a recording's frames.

use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    utils::{Logical, Physical, Rectangle},
};

use super::{Frame, Look};
use crate::{
    State,
    effects::{
        FireElement,
        draw::Clip,
        garland::{Garland, View},
        mesh::Mesh,
    },
    model::WindowId,
};

/// Far enough to keep nothing out.
const FAR: f32 = 1.0e7;

/// Its parts, for their commits.
const FRONT: usize = 0;
const HALOS: usize = 1;

impl State {
    /// The garland, when it shows on `frame`'s output.
    fn garland_on(&self, frame: &Frame) -> Option<&Garland> {
        if !frame.purpose.screen() || self.fullscreen_in_front(frame.output) {
            return None;
        }
        self.garlanding()?;
        self.fx.garland.as_ref()
    }

    /// Where `garland` is drawn on `frame`'s output, its window drawn at
    /// `visual`: moved along with it (as it opens, or is carried to a
    /// desktop's tile), `alpha` of it.
    fn garland_view(
        frame: &Frame,
        garland: &Garland,
        visual: Rectangle<f64, Logical>,
        alpha: f32,
    ) -> View {
        let bottom = |r: Rectangle<f64, Logical>| (r.loc.x + r.size.w / 2.0, r.loc.y + r.size.h);
        let ((x0, y0), (x1, y1)) = (bottom(garland.rect), bottom(visual));
        View {
            shift: (x1 - x0 - frame.origin.x, y1 - y0 - frame.origin.y),
            scale: frame.scale as f32,
            alpha,
        }
    }

    /// The garland's cord and bulbs, when it hangs under window `id`,
    /// drawn where it is (`look`): right in front of it. Jumping to it,
    /// over none of the windows `looks` shows: on the wallpaper alone.
    pub(super) fn garland_front(
        &self,
        frame: &Frame,
        id: WindowId,
        look: &Look,
        looks: &[(WindowId, Look)],
    ) -> Option<FireElement> {
        let garland = self.garland_on(frame)?;
        if garland.window != id {
            return None;
        }
        let view = Self::garland_view(frame, garland, look.visual, look.alpha);
        let (over, added) = garland.front(&view);
        let phys = |r: Rectangle<f64, Logical>| {
            let s = frame.scale;
            [
                ((r.loc.x - frame.origin.x) * s) as f32,
                ((r.loc.y - frame.origin.y) * s) as f32,
                ((r.loc.x + r.size.w - frame.origin.x) * s) as f32,
                ((r.loc.y + r.size.h - frame.origin.y) * s) as f32,
            ]
        };
        // Never on its window, nor on the bar under the windows; on its
        // way, on no window (those nearest it first).
        let window = phys(look.visual);
        let bar = self.bar_rect(frame.output, frame.now).map(|b| {
            let s = frame.scale as f32;
            [
                b.loc.x as f32 * s,
                b.loc.y as f32 * s,
                (b.loc.x + b.size.w) as f32 * s,
                (b.loc.y + b.size.h) as f32 * s,
            ]
        });
        let mut others: Vec<Rectangle<f64, Logical>> = if garland.jumping() {
            looks
                .iter()
                .filter(|(other, _)| *other != id)
                .map(|(_, l)| l.visual)
                .collect()
        } else {
            Vec::new()
        };
        let (cx, cy) = (
            look.visual.loc.x + look.visual.size.w / 2.0,
            look.visual.loc.y + look.visual.size.h,
        );
        others.sort_by(|a, b| {
            let d = |r: &Rectangle<f64, Logical>| {
                (r.loc.x + r.size.w / 2.0 - cx).hypot(r.loc.y + r.size.h / 2.0 - cy)
            };
            d(a).total_cmp(&d(b))
        });
        let clip = Clip::new(
            [-FAR, -FAR, FAR, FAR],
            std::iter::once(window)
                .chain(bar)
                .chain(others.into_iter().map(phys)),
            1.0,
        );
        let geometry = bounds(frame, [&over, &added])?;
        let commit = drawn(garland, FRONT, frame, &over, &added, &clip);
        Some(FireElement::shapes(
            garland.ids[0].clone(),
            commit,
            geometry,
            over,
            added,
            clip,
        ))
    }

    /// The garland's halos on `frame`'s output, its window drawn as
    /// `looks` say: the element standing for them behind the glass (blank,
    /// so that the glass is not blurred again at each of their frames),
    /// and themselves.
    pub(super) fn garland_layer(
        &self,
        frame: &Frame,
        looks: &[(WindowId, Look)],
    ) -> Option<(FireElement, FireElement)> {
        let garland = self.garland_on(frame)?;
        let (_, look) = looks.iter().find(|(id, _)| *id == garland.window)?;
        let view = Self::garland_view(frame, garland, look.visual, look.alpha);
        let halos = garland.halos(&view);
        let none = Mesh::default();
        let geometry = bounds(frame, [&none, &halos])?;
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
        let commit = drawn(garland, HALOS, frame, &none, &halos, &clip);
        let screen = screen(frame);
        Some((
            FireElement::blank(garland.ids[2].clone(), screen),
            FireElement::shapes(garland.ids[1].clone(), commit, geometry, none, halos, clip),
        ))
    }

    /// The id of the blank standing for the garland's halos.
    pub(super) fn garland_blank(&self) -> Option<Id> {
        Some(self.fx.garland.as_ref()?.ids[2].clone())
    }
}

fn screen(frame: &Frame) -> Rectangle<i32, Physical> {
    Rectangle::from_size(
        frame
            .area
            .size
            .to_f64()
            .to_physical_precise_round(frame.scale),
    )
}

/// What `meshes` cover on `frame`'s output, when anything.
fn bounds(frame: &Frame, meshes: [&Mesh; 2]) -> Option<Rectangle<i32, Physical>> {
    let [x0, y0, x1, y1] = meshes
        .iter()
        .filter_map(|m| m.bounds())
        .map(|[x, y, w, h]| [x, y, x + w, y + h])
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })?;
    Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into()).intersection(screen(frame))
}

/// The commit of `part` of `garland` on `frame`'s output: a new one when
/// what it draws changed since it last drew it there.
fn drawn(
    garland: &Garland,
    part: usize,
    frame: &Frame,
    over: &Mesh,
    added: &Mesh,
    clip: &Clip,
) -> CommitCounter {
    let mut now = Vec::with_capacity(over.vertices.len() + added.vertices.len() + 40);
    now.extend_from_slice(&over.vertices);
    now.push(over.vertices.len() as f32);
    now.extend_from_slice(&added.vertices);
    now.extend_from_slice(&clip.keep);
    now.extend(clip.holes.iter().flatten());
    let mut kept = garland.drawn.borrow_mut();
    let last = kept.entry((part, frame.output.name())).or_default();
    if last.vertices != now {
        last.commit.increment();
        last.vertices = now;
    }
    last.commit
}
