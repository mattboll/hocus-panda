//! The recorded area's frame while recording (5.10): a 2 px red dashed
//! line (`rgba(242, 84, 91, 0.9)`, dashes of 8 every 16), 3 px outside
//! what is recorded, the mockup's sizes × the launcher's zoom as the
//! capture's; half as opaque while paused. A whole screen's runs just
//! inside its edges. Solid rectangles, one per dash: drawn on the screen
//! only, never in the recording, a capture or a share.

use std::cell::RefCell;

use hocus_link::LAUNCHER_ZOOM;
use smithay::{
    backend::renderer::element::{Id, Kind, solid::SolidColorRenderElement},
    output::Output,
    utils::{Logical, Physical, Rectangle},
};

use super::Source;
use crate::{
    State,
    render::{OutputElement, OutputElements},
};

const Z: f64 = LAUNCHER_ZOOM;
const WIDTH: f64 = 2.0 * Z;
const OUTSIDE: f64 = 3.0 * Z;
const DASH: f64 = 8.0 * Z;
const PERIOD: f64 = 16.0 * Z;
/// `rgba(242, 84, 91, 0.9)`.
const RED: [f32; 4] = [242.0 / 255.0, 84.0 / 255.0, 91.0 / 255.0, 0.9];
const PAUSED: f32 = 0.5;

/// The dashes' ids, kept from frame to frame.
#[derive(Debug, Default)]
pub struct Drawn {
    ids: RefCell<Vec<Id>>,
}

impl Drawn {
    fn id(&self, at: usize) -> Id {
        let mut ids = self.ids.borrow_mut();
        while ids.len() <= at {
            ids.push(Id::new());
        }
        ids[at].clone()
    }
}

/// The dashes of a frame around `rect` (output-local, logical): each side
/// its own run, from its start.
pub fn dashes(rect: Rectangle<f64, Logical>) -> Vec<Rectangle<f64, Logical>> {
    let (x, y, w, h) = (rect.loc.x, rect.loc.y, rect.size.w, rect.size.h);
    let mut out = Vec::new();
    let mut run = |start: f64, length: f64, piece: &dyn Fn(f64, f64) -> Rectangle<f64, Logical>| {
        let mut at = 0.0;
        while at < length {
            let long = DASH.min(length - at);
            out.push(piece(start + at, long));
            at += PERIOD;
        }
    };
    run(x, w, &|a, l| {
        Rectangle::new((a, y).into(), (l, WIDTH).into())
    });
    run(x, w, &|a, l| {
        Rectangle::new((a, y + h - WIDTH).into(), (l, WIDTH).into())
    });
    run(y, h, &|a, l| {
        Rectangle::new((x, a).into(), (WIDTH, l).into())
    });
    run(y, h, &|a, l| {
        Rectangle::new((x + w - WIDTH, a).into(), (WIDTH, l).into())
    });
    out
}

/// `rect` in the output's pixels, its edges rounded each.
fn pixels(rect: Rectangle<f64, Logical>, scale: f64) -> Rectangle<i32, Physical> {
    let a: smithay::utils::Point<i32, Physical> = rect.loc.to_physical_precise_round(scale);
    let b: smithay::utils::Point<i32, Physical> =
        (rect.loc + rect.size.to_point()).to_physical_precise_round(scale);
    Rectangle::new(a, (b.x - a.x, b.y - a.y).into())
}

impl State {
    /// The recording's frame on `output`, if what it records is there.
    pub(crate) fn record_elements<R>(&self, output: &Output, out: &mut Vec<OutputElement<R>>)
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem
            + crate::wobble::Bend
            + crate::going::Shaders,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        let Some(live) = self.record.live() else {
            return;
        };
        let Some(geometry) = self.space.output_geometry(output) else {
            return;
        };
        let name = output.name();
        let scale = output.current_scale().fractional_scale();
        let around = match &live.source {
            Source::Screen(on) if *on == name => Rectangle::<f64, Logical>::from_size(
                (f64::from(geometry.size.w), f64::from(geometry.size.h)).into(),
            ),
            Source::Area { output: on, area } if *on == name => Rectangle::new(
                (area.x - OUTSIDE, area.y - OUTSIDE).into(),
                (area.w + 2.0 * OUTSIDE, area.h + 2.0 * OUTSIDE).into(),
            ),
            Source::Window(id) => {
                let Some(r) = self.rect_of(crate::model::WindowId(*id)) else {
                    return;
                };
                let r = Rectangle::<f64, Logical>::new(
                    (
                        f64::from(r.x - geometry.loc.x) - OUTSIDE,
                        f64::from(r.y - geometry.loc.y) - OUTSIDE,
                    )
                        .into(),
                    (
                        f64::from(r.width) + 2.0 * OUTSIDE,
                        f64::from(r.height) + 2.0 * OUTSIDE,
                    )
                        .into(),
                );
                let screen = Rectangle::<f64, Logical>::from_size(geometry.size.to_f64());
                if !r.overlaps(screen) {
                    return;
                }
                r
            }
            _ => return,
        };
        let k = if live.paused { PAUSED } else { 1.0 };
        let colour = [
            RED[0] * RED[3] * k,
            RED[1] * RED[3] * k,
            RED[2] * RED[3] * k,
            RED[3] * k,
        ];
        let commit = usize::from(live.paused);
        for (i, dash) in dashes(around).into_iter().enumerate() {
            let at = pixels(dash, scale);
            if at.size.w <= 0 || at.size.h <= 0 {
                continue;
            }
            out.push(OutputElements::Solid(SolidColorRenderElement::new(
                self.record.drawn.id(i),
                at,
                commit,
                colour,
                Kind::Unspecified,
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frames_dashes_run_along_each_side() {
        let rect = Rectangle::<f64, Logical>::new((10.0, 20.0).into(), (100.0, 50.0).into());
        let all = dashes(rect);
        // 100 / 24 → 5 on each of the long sides, 50 / 24 → 3 on the others.
        assert_eq!(all.len(), 5 + 5 + 3 + 3);
        assert_eq!(
            all[0],
            Rectangle::new((10.0, 20.0).into(), (DASH, WIDTH).into())
        );
        // The last of a side is cut at its end.
        let last_top = all[4];
        assert!(last_top.loc.x + last_top.size.w <= 110.0 + 1e-9);
        for dash in &all {
            assert!(rect.contains_rect(*dash), "{dash:?}");
        }
    }
}
