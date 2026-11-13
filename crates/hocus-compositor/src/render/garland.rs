//! The garlands drawn (#799, #813). The first's cord and bulbs right in
//! front of the window it hangs under, as the snow's caps, so that a
//! window in front covers it; never on that window, nor on the bar. Their
//! halos on the wallpaper, under the windows and the bar (a light on the
//! wallpaper alone), in the layer's place behind the glass. The second's,
//! across the top of each screen, all there: its cord and bulbs (never on
//! the bar) over its halos. On the screen alone: never in a capture, a
//! share's or a recording's frames.

use smithay::{
    backend::renderer::element::Id,
    utils::{Logical, Physical, Rectangle},
};

use super::{Frame, Look};
use crate::{
    State,
    effects::{
        Fire, FireElement,
        draw::Clip,
        garland::{Garland, View},
        kept::Shown,
        mesh::Mesh,
    },
    model::WindowId,
};

use std::{cell::RefCell, collections::HashMap};

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
    pub(super) fn garland_front<R: Fire>(
        &self,
        renderer: &mut R,
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
        let bar = self.garland_bar(frame);
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
        let made_of = made_of(garland.changes(), &view);
        show(
            renderer,
            &garland.drawn,
            &garland.ids,
            FRONT,
            frame,
            made_of,
            clip,
            || garland.front(&view),
        )
    }

    /// The garland's halos on `frame`'s output, its window drawn as
    /// `looks` say: the element standing for them behind the glass (blank,
    /// so that the glass is not blurred again at each of their frames),
    /// and themselves.
    pub(super) fn garland_layer<R: Fire>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        looks: &[(WindowId, Look)],
    ) -> Option<(FireElement, FireElement)> {
        let garland = self.garland_on(frame)?;
        let (_, look) = looks.iter().find(|(id, _)| *id == garland.window)?;
        let view = Self::garland_view(frame, garland, look.visual, look.alpha);
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
        let made_of = made_of(garland.changes(), &view);
        let layer = show(
            renderer,
            &garland.drawn,
            &garland.ids,
            HALOS,
            frame,
            made_of,
            clip,
            || (Mesh::default(), garland.halos(&view)),
        )?;
        Some((
            FireElement::blank(garland.ids[2].clone(), screen(frame)),
            layer,
        ))
    }

    /// The id of the blank standing for the garland's halos.
    pub(super) fn garland_blank(&self) -> Option<Id> {
        Some(self.fx.garland.as_ref()?.ids[2].clone())
    }
    /// The bar on `frame`'s output (output-local, physical), which the
    /// garlands' cords and bulbs keep off.
    fn garland_bar(&self, frame: &Frame) -> Option<[f32; 4]> {
        self.bar_rect(frame.output, frame.now).map(|b| {
            let s = frame.scale as f32;
            [
                b.loc.x as f32 * s,
                b.loc.y as f32 * s,
                (b.loc.x + b.size.w) as f32 * s,
                (b.loc.y + b.size.h) as f32 * s,
            ]
        })
    }

    /// The garland across the top of `frame`'s output (#813), when it
    /// shows there: the element standing for it behind the glass (blank),
    /// and what it is, front first: its cord and bulbs (off the bar), its
    /// halos.
    pub(super) fn garland_top_layer<R: Fire>(
        &self,
        renderer: &mut R,
        frame: &Frame,
    ) -> Option<(FireElement, [FireElement; 2])> {
        if !frame.purpose.screen() || self.fullscreen_in_front(frame.output) {
            return None;
        }
        self.garlanding_top()?;
        let top = self.fx.tops.get(&frame.output.name())?;
        let view = View {
            shift: (-frame.origin.x, -frame.origin.y),
            scale: frame.scale as f32,
            alpha: 1.0,
        };
        let off_bar = Clip::new([-FAR, -FAR, FAR, FAR], self.garland_bar(frame), 1.0);
        let anywhere = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
        let made_of = made_of(top.changes(), &view);
        let front = show(
            renderer,
            &top.drawn,
            &top.ids,
            FRONT,
            frame,
            made_of.clone(),
            off_bar,
            || top.front(&view),
        );
        let halos = show(
            renderer,
            &top.drawn,
            &top.ids,
            HALOS,
            frame,
            made_of,
            anywhere,
            || (Mesh::default(), top.halos(&view)),
        );
        let (front, halos) = front.zip(halos)?;
        Some((
            FireElement::blank(top.ids[2].clone(), screen(frame)),
            [front, halos],
        ))
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

/// What a garland's part is drawn from: its rope's changes and where.
fn made_of(changes: u64, view: &View) -> Vec<f64> {
    vec![
        changes as f64,
        view.shift.0,
        view.shift.1,
        f64::from(view.scale),
        f64::from(view.alpha),
    ]
}

/// `part` of a garland on `frame`'s output, over the whole screen, its
/// shapes (over and added) from `shapes`: made again only when what it
/// is `made_of` or `clip` changed, telling what changed (#815).
#[expect(
    clippy::too_many_arguments,
    reason = "a part's shapes and where they go"
)]
fn show<R: Fire>(
    renderer: &mut R,
    kept: &RefCell<HashMap<(usize, String), Shown>>,
    ids: &[Id; 3],
    part: usize,
    frame: &Frame,
    made_of: Vec<f64>,
    clip: Clip,
    shapes: impl FnOnce() -> (Mesh, Mesh),
) -> Option<FireElement> {
    let id = ids.get(part).cloned().unwrap_or_else(Id::new);
    let mut kept = kept.borrow_mut();
    let shown = kept
        .entry((part, frame.output.name()))
        .or_insert_with(|| Shown::new(id));
    let screen = screen(frame);
    if let Some(again) = shown.again_for(&made_of, screen, &clip) {
        return Some(again);
    }
    let (over, added) = shapes();
    shown.made_of = made_of;
    shown.shapes(renderer, screen, over, added, clip)
}
