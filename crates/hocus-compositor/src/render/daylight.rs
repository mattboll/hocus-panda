//! The daylight drawn (#805): its sky over the wallpaper, which it covers,
//! under the layers of the bottom and background, the windows and the
//! bar, drawn once a minute into a picture of its own; and a window's
//! edges lit by its sun, right in front of its frame, out of what it
//! shows. On the screen alone: never in a capture, a share's or a
//! recording's frames.

use smithay::{
    backend::renderer::{
        Color32F,
        element::{Kind, solid::SolidColorRenderElement},
    },
    utils::{Logical, Physical, Rectangle},
};

use super::Frame;
use crate::{
    State,
    decoration::RADIUS,
    effects::{FireElement, daylight::LitEdges, draw::SkyLook},
    going::Shaders,
};

/// The board's stars over its 1200 × 720.
const STARS_A_PIXEL: f64 = 160.0 / (1200.0 * 720.0);

impl State {
    /// The daylight's sky on `frame`'s output, when it is its wallpaper.
    pub(super) fn daylight_layer<R>(&self, renderer: &mut R, frame: &Frame) -> Option<FireElement>
    where
        R: Shaders,
    {
        let _measure = crate::effects::timing::build(crate::effects::timing::Fx::Daylight);
        if !frame.purpose.screen() || !self.sky_shows_on(frame.output) {
            return None;
        }
        let sky = self.daylight_on(frame.output)?;
        let daylight = self.fx.daylight.as_ref()?;
        let pane = daylight.panes.get(&frame.output.name())?;
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let size = (screen.size.w, screen.size.h);
        let now = (daylight.generation, size);
        let fresh = pane.drawn.get() != Some(now);
        let (w, h) = (f64::from(frame.area.size.w), f64::from(frame.area.size.h));
        let c = sky.colours;
        let unit = |v: [f64; 3]| v.map(|c| (c / 255.0) as f32);
        let night = if !sky.sun_up {
            1.0
        } else if sky.elev < 0.25 {
            1.0 - sky.elev / 0.25
        } else {
            0.0
        };
        let look = SkyLook {
            size: [w as f32, h as f32],
            scale: frame.scale as f32,
            top: unit(c.top),
            bottom: unit(c.bottom),
            light: unit(c.light),
            grade: c.grade as f32,
            sun: [sky.sun.0 as f32, sky.sun.1 as f32],
            up: sky.sun_up,
            elev: sky.elev as f32,
            night: night as f32,
            stars: (w * h * STARS_A_PIXEL).round().min(1024.0) as f32,
        };
        let texture = renderer.sky(&pane.kept, size, fresh, look)?;
        if fresh {
            pane.drawn.set(Some(now));
            let mut commit = pane.commit.get();
            commit.increment();
            pane.commit.set(commit);
        }
        Some(FireElement::picture(
            pane.id.clone(),
            pane.commit.get(),
            screen,
            texture,
        ))
    }
}

/// The edges of a window at `rect` (logical, output-local) its sun lights
/// (#805): a line along its top and one down the side facing the sun,
/// just out of what it shows, clear of its round corners; `generation`
/// the daylight's hour, `ids` theirs.
pub(super) fn lit_edges(
    lit: &LitEdges,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
    ids: &[smithay::backend::renderer::element::Id; 2],
    generation: u64,
) -> Vec<SolidColorRenderElement> {
    let [r, g, b, a] = lit.colour.map(|c| c * alpha);
    let side_alpha = lit.width.min(1.0);
    let width = f64::from(lit.width.max(1.0));
    let (x, y, w, h) = (rect.loc.x, rect.loc.y, rect.size.w, rect.size.h);
    let top = Rectangle::<f64, Logical>::new(
        (x + RADIUS, y - 1.0).into(),
        (w - 2.0 * RADIUS, 1.0).into(),
    );
    let side_x = if lit.side < 0 { x - width } else { x + w };
    let side = Rectangle::<f64, Logical>::new(
        (side_x, y + RADIUS).into(),
        (width, h - 2.0 * RADIUS).into(),
    );
    let commit = generation as usize;
    [
        (top, Color32F::new(r, g, b, a)),
        (
            side,
            Color32F::new(
                r * side_alpha,
                g * side_alpha,
                b * side_alpha,
                a * side_alpha,
            ),
        ),
    ]
    .into_iter()
    .zip(ids)
    .filter(|((rect, colour), _)| rect.size.w > 0.0 && rect.size.h > 0.0 && colour.a() > 0.0)
    .map(|((rect, colour), id)| {
        SolidColorRenderElement::new(
            id.clone(),
            rect.to_physical_precise_round(scale),
            commit,
            colour,
            Kind::Unspecified,
        )
    })
    .collect()
}
