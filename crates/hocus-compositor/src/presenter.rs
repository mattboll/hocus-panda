//! Présentateur (#619, lot 3, docs/design/camera.md; the board
//! « Présentateur »): while the desktop shares a screen or a window
//! (5.11), the person, cut out by `hocus-camera`, stands in a corner of
//! the share's stream like a weather presenter. Never on the screen: it
//! is drawn into the share's frames alone.
//!
//! - **Where**: on the bottom edge, 24 px of the board's 860 in from the
//!   left or the right (`[camera] presenter-side`), as tall as the
//!   board's 196, 262 or 340 px of its 520 (`presenter-size`), as wide
//!   as the cut-out is for that height; its shadow comes with it.
//! - **Out of the way**: while the pointer is under it, it stands in the
//!   other corner, sliding there over 300 ms (the board's transition), at
//!   once with reduced motion.
//! - **Its pictures** come from `hocus-camera` while the presenter is
//!   wanted (on, and a share live), each a new buffer; each makes the
//!   shares' frames due.

use std::{cell::Cell, time::Duration};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
        },
    },
    utils::{Logical, Point, Rectangle},
};

use crate::{
    State,
    camera::Silhouette,
    config::{Side, Size},
    going::Shaders,
    render::OutputElements,
    wobble::Bend,
};

/// The board's shared screen, which the presenter's measures are of.
const BOARD_W: f64 = 860.0;
const MARGIN: f64 = 24.0;
/// Its slide to the other corner.
const SLIDE: Duration = Duration::from_millis(300);

/// The person's picture, as the compositor keeps it.
#[derive(Debug)]
pub struct Shown {
    buffer: MemoryRenderBuffer,
    size: (u32, u32),
    /// Where it stands, 0 the left corner to 1 the right, and when that
    /// was last moved on.
    at: Cell<Option<(f64, Duration)>>,
}

impl Shown {
    pub fn new(cut: Silhouette) -> Self {
        let buffer = crate::decoration::buffer(&cut.bgra, cut.width as i32, cut.height as i32);
        Self {
            buffer,
            size: (cut.width, cut.height),
            at: Cell::new(None),
        }
    }

    /// It takes the place the last picture had.
    pub fn after(self, before: Option<&Self>) -> Self {
        if let Some(before) = before {
            self.at.set(before.at.get());
        }
        self
    }
}

/// The presenter's box in an area of `area` (logical), its corner `side`
/// (0 left to 1 right), for a cut-out `w` × `h`.
pub fn place(
    area: (f64, f64),
    size: Size,
    side: f64,
    (w, h): (u32, u32),
) -> Rectangle<f64, Logical> {
    let height = area.1 * size.height();
    let width = height * f64::from(w) / f64::from(h.max(1));
    let margin = area.0 * MARGIN / BOARD_W;
    let left = margin;
    let right = area.0 - margin - width;
    let x = left + (right - left) * side;
    Rectangle::new((x, area.1 - height).into(), (width, height).into())
}

impl State {
    /// The presenter over a share's frame whose corner is at `origin` on
    /// the desktop, standing in `zone` of it (logical, from that corner:
    /// a screen's beside the bar, a window's whole), drawn at `scale`, if
    /// it shows: moved to the other corner while the pointer is under it.
    pub(crate) fn presenter_element<R>(
        &self,
        renderer: &mut R,
        origin: Point<f64, Logical>,
        zone: Rectangle<f64, Logical>,
        scale: f64,
    ) -> Option<OutputElements<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if !self.presenter_wanted() {
            return None;
        }
        let shown = self.camera.silhouette.as_ref()?;
        let c = &self.config.camera;
        let preferred = match c.presenter_side {
            Side::Left => 0.0,
            Side::Right => 1.0,
        };
        let area = (zone.size.w, zone.size.h);
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location() - origin - zone.loc);
        let under = pointer
            .is_some_and(|p| place(area, c.presenter_size, preferred, shown.size).contains(p));
        let target = if under { 1.0 - preferred } else { preferred };
        let now = self.now();
        let side = match shown.at.get() {
            None => target,
            Some(_) if self.reduced_motion() => target,
            Some((at, last)) => {
                let step = now.saturating_sub(last).as_secs_f64() / SLIDE.as_secs_f64();
                if at < target {
                    (at + step).min(target)
                } else {
                    (at - step).max(target)
                }
            }
        };
        shown.at.set(Some((side, now)));
        let eased = side * side * (3.0 - 2.0 * side);
        let rect = place(area, c.presenter_size, eased, shown.size);
        let at = (rect.loc + zone.loc).to_physical_precise_round::<_, i32>(scale);
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            at.to_f64(),
            &shown.buffer,
            None,
            // The whole cut-out, scaled to its box.
            Some(Rectangle::from_size(
                (f64::from(shown.size.0), f64::from(shown.size.1)).into(),
            )),
            Some((rect.size.w.round() as i32, rect.size.h.round() as i32).into()),
            Kind::Unspecified,
        )
        .map_err(|err| tracing::debug!(%err, "the presenter not drawn"))
        .ok()
        .map(OutputElements::Backdrop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_stands_in_its_corner_at_its_size() {
        // The board's own: 860 × 520, the medium one on the right.
        let r = place((860.0, 520.0), Size::Medium, 1.0, (200, 262));
        assert_eq!(r.size.h, 262.0);
        assert!((r.size.w - 200.0).abs() < 1e-9);
        assert!((r.loc.x - (860.0 - 24.0 - 200.0)).abs() < 1e-9);
        assert_eq!(r.loc.y, 520.0 - 262.0);
        let l = place((860.0, 520.0), Size::Small, 0.0, (150, 196));
        assert_eq!((l.loc.x, l.size.h), (24.0, 196.0));
    }
}
