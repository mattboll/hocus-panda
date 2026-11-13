//! The rain on the glass drawn (#802): over the wallpaper, under the
//! layers of the bottom and background, the windows and the bar; on the
//! screens it shows on. The wallpaper's picture is blurred once (as the
//! glass of the bar is, in a pane of its own, again only with a new
//! wallpaper); each frame the drops' map is drawn, then the glass from it
//! over what changed. Opaque: the wallpaper under it is not drawn. Behind
//! the glass of the bar stands the fogged glass alone, which never
//! changes, so that the bar's glass is not blurred again at each frame.
//! On the screen alone: never in a capture, a share's or a recording's
//! frames.

use std::{any::Any, rc::Rc};

use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    utils::{Physical, Rectangle},
};

use super::Frame;
use crate::{
    State,
    effects::{FireElement, draw::RainLook, raindrops::Pane},
    glass::{Frost, Glass, rain_pane},
    going::Shaders,
    wobble::Bend,
};

/// The board's blur: CSS's `blur(9px)` on its half-sized picture, 18
/// logical pixels of standard deviation; a reach is twice it.
const FOG: Frost = Frost {
    reach: 36.0,
    saturation: 1.0,
};
/// The board's blurred picture is drawn 20 of its pixels (40 logical)
/// beyond its edges: what it shows is scaled about its middle.
const FOG_MARGIN: f32 = 40.0;
/// The board's stage, which the drops' bending is a share of.
const BOARD: (f32, f32) = (1200.0, 720.0);

impl State {
    /// The rain's pane on `frame`'s output, when it shows there.
    fn raindrops_on(&self, frame: &Frame) -> Option<&Pane> {
        if !frame.purpose.screen() || !self.rain_shows_on(frame.output) {
            return None;
        }
        self.fx.raindrops.as_ref()?.panes.get(&frame.output.name())
    }

    /// The rain on `frame`'s output: the fogged glass standing for it
    /// behind the glass of the bar, and itself.
    pub(super) fn raindrops_layer<R>(
        &self,
        renderer: &mut R,
        frame: &Frame,
    ) -> Option<(FireElement, FireElement)>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let pane = self.raindrops_on(frame)?;
        let backdrop = self.backdrop_element(renderer, frame.output)?;
        let scale = frame.scale;
        let screen: Rectangle<i32, Physical> =
            Rectangle::from_size(frame.area.size.to_f64().to_physical_precise_round(scale));
        let blurred = renderer.frost(
            &mut self.graphics.panes.borrow_mut(),
            &rain_pane(&frame.output.name()),
            screen,
            scale,
            FOG,
            std::slice::from_ref(&backdrop),
        )?;
        let drawn = pane.frame(&blurred.id);
        let map = renderer.drop_map(&pane.map, pane.map_size(), drawn.fresh_map, || {
            pane.map_mesh()
        })?;
        let (w, h) = pane.size;
        let m = FOG_MARGIN;
        let look = RainLook {
            screen: [screen.size.w as f32, screen.size.h as f32],
            zoom: [w / (w + 2.0 * m), h / (h + 2.0 * m)],
            offset: [m / (w + 2.0 * m), m / (h + 2.0 * m)],
            bend: [BOARD.0 / w, BOARD.1 / h],
        };
        let textures: [Rc<dyn Any>; 2] = [Rc::new(blurred.behind), Rc::new(blurred.texture)];
        let fog = FireElement::rain(
            pane.ids[1].clone(),
            drawn.fog,
            screen,
            textures.clone(),
            None,
            look,
        );
        let mut rain = FireElement::rain(
            pane.ids[0].clone(),
            drawn.commit,
            screen,
            textures,
            Some(map),
            look,
        );
        if let Some((prev, rects)) = drawn.since {
            let s = scale as f32;
            let rects = rects
                .iter()
                .filter_map(|[x0, y0, x1, y1]| {
                    let (x, y) = ((x0 * s).floor() as i32, (y0 * s).floor() as i32);
                    let (r, b) = ((x1 * s).ceil() as i32, (y1 * s).ceil() as i32);
                    Rectangle::new((x, y).into(), (r - x, b - y).into()).intersection(screen)
                })
                .collect();
            rain = rain.changed_since(prev, rects);
        }
        Some((fog, rain))
    }

    /// The id of the fogged glass standing for the rain on `frame`'s
    /// output.
    pub(super) fn raindrops_stand_in(
        &self,
        frame: &Frame,
    ) -> Option<smithay::backend::renderer::element::Id> {
        let rain = self.fx.raindrops.as_ref()?;
        Some(rain.panes.get(&frame.output.name())?.ids[1].clone())
    }
}
