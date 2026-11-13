//! The koi's pond drawn (#803): over the wallpaper, which it covers,
//! under the layers of the bottom and background, the windows and the
//! bar; on the screens it shows on. Each frame the koi and their shadows
//! are drawn over the pond's floor into a picture (the board's `under`
//! canvas, at the screen's logical size), the water's heights given to the
//! GPU, and the water drawn from both; the lily pads over it. Behind the
//! glass of the bar stand the floor under still water and a blank for the
//! pads, which never change, so that the bar's glass is not blurred again
//! at each frame. On the screen alone: never in a capture, a share's or a
//! recording's frames.

use smithay::{
    backend::renderer::element::Id,
    utils::{Physical, Rectangle},
};

use super::Frame;
use crate::{
    State,
    effects::{
        FireElement, Plays,
        draw::{Clip, KoiLook},
        koi::{CELL, Pond},
        mesh::Mesh,
    },
    going::Shaders,
};

/// The board's stage, which the water's bending is a share of.
const BOARD: (f32, f32) = (1200.0, 720.0);

/// The koi's pond on a screen this frame: the still water standing for it
/// behind the glass, itself, the blank standing for its pads there and
/// its pads.
pub(super) struct KoiLayer {
    pub stand_in: FireElement,
    pub water: FireElement,
    pub pads_blank: FireElement,
    pub pads: FireElement,
}

impl State {
    /// The pond on `frame`'s output, when it shows there.
    fn pond_on(&self, frame: &Frame) -> Option<&Pond> {
        if !frame.purpose.screen() || !self.koi_shows_on(frame.output) {
            return None;
        }
        self.fx.koi.as_ref()?.panes.get(&frame.output.name())
    }

    /// The pond on `frame`'s output.
    pub(super) fn koi_layer<R>(&self, renderer: &mut R, frame: &Frame) -> Option<KoiLayer>
    where
        R: Shaders,
    {
        let pond = self.pond_on(frame)?;
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let (w, h) = pond.size;
        let size = (w.ceil() as i32, h.ceil() as i32);
        if let Some(pixels) = pond.floor_pixels() {
            let texture = renderer.sheet(&pixels, size)?;
            pond.floor_made(texture);
        }
        let floor = pond.floor_texture.borrow().clone()?;
        let generation = pond.generation();
        let fresh = pond.shown.get() != Some(generation);
        let under = if fresh {
            renderer.koi_under(&pond.under, size, &floor, &pond.under_mesh())?
        } else {
            pond.under.borrow().as_ref()?.1.clone()
        };
        let (gw, gh) = pond.grid;
        let grid = (gw as i32, gh as i32);
        let (heights, half) = if fresh {
            renderer.koi_heights(&pond.heights, grid, &pond.cur)?
        } else {
            let kept = pond.heights.borrow();
            let kept = kept.as_ref()?;
            (kept.texture.clone(), kept.half)
        };
        pond.shown.set(Some(generation));
        let look = KoiLook {
            screen: [screen.size.w as f32, screen.size.h as f32],
            grid: [w / (gw as f32 * CELL), h / (gh as f32 * CELL)],
            texel: [1.0 / gw as f32, 1.0 / gh as f32],
            bend: [BOARD.0 / w, BOARD.1 / h],
            zero: if half { 0.0 } else { 0.5 },
        };
        let commit = pond.commit();
        let pads = pond.pads_mesh(frame.scale as f32);
        let geometry = pads
            .bounds()
            .map(|[x, y, w, h]| Rectangle::new((x, y).into(), (w, h).into()))
            .and_then(|r| r.intersection(screen))
            .unwrap_or_default();
        let clip = Clip::new([-1.0e7, -1.0e7, 1.0e7, 1.0e7], [], 1.0);
        let moving = self.koi_plays() == Some(Plays::Moving);
        tracing::trace!(moving, "the koi's pond drawn");
        Some(KoiLayer {
            stand_in: FireElement::koi(
                pond.ids[1].clone(),
                pond.floor_commit.get(),
                screen,
                floor,
                None,
                look,
            ),
            water: FireElement::koi(
                pond.ids[0].clone(),
                commit,
                screen,
                under,
                Some(heights),
                look,
            ),
            pads_blank: FireElement::blank(pond.ids[3].clone(), screen),
            pads: FireElement::shapes(
                pond.ids[2].clone(),
                commit,
                geometry,
                pads,
                Mesh::default(),
                clip,
            ),
        })
    }

    /// The ids of the still water and of the blank standing for the pond
    /// and its pads on `frame`'s output.
    pub(super) fn koi_stand_ins(&self, frame: &Frame) -> Option<(Id, Id)> {
        let pond = self.fx.koi.as_ref()?.panes.get(&frame.output.name())?;
        Some((pond.ids[1].clone(), pond.ids[3].clone()))
    }
}
