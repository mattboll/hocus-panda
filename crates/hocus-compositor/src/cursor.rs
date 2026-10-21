//! The pointer's cursor: the client's own surface when it sets one, our
//! arrow otherwise, drawn at each output's scale.
//!
//! The arrow is drawn here rather than loaded from a cursor theme: it is
//! sharp at any scale, and the same on every machine, which keeps reference
//! screenshots stable. Themes come with the theme work.

use std::{collections::BTreeMap, sync::Mutex};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportAll, ImportMem, Renderer, Texture,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                render_elements,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
        },
    },
    input::pointer::{CursorImageStatus, CursorImageSurfaceData},
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
    wayland::compositor::with_states,
};

use crate::State;

/// Logical size of the arrow.
const SIZE: i32 = 24;
/// The arrow's outline, in its own 24×24 units; the tip is the hotspot.
const ARROW: [(f64, f64); 7] = [
    (1.0, 1.0),
    (1.0, 18.5),
    (5.2, 14.6),
    (8.2, 21.5),
    (11.4, 20.1),
    (8.5, 13.3),
    (14.2, 13.3),
];
const HOTSPOT: (f64, f64) = (1.0, 1.0);
/// The black edge around the white body, in units.
const EDGE: f64 = 1.2;

render_elements! {
    pub CursorElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Arrow=MemoryRenderBufferRenderElement<R>,
}

/// Arrow images already drawn, by output scale (in thousandths).
#[derive(Debug, Default)]
pub struct Arrows(Mutex<BTreeMap<u32, MemoryRenderBuffer>>);

impl Arrows {
    /// The arrow for `scale`, and its width in pixels.
    fn at_scale(&self, scale: f64) -> (MemoryRenderBuffer, i32) {
        let key = (scale * 1000.0).round() as u32;
        let buffer = match self.0.lock() {
            Ok(mut cache) => cache.entry(key).or_insert_with(|| arrow(scale)).clone(),
            Err(_) => arrow(scale),
        };
        (buffer, pixels(scale))
    }
}

fn pixels(scale: f64) -> i32 {
    (f64::from(SIZE) * scale).round().max(1.0) as i32
}

/// The arrow as ARGB pixels (white body, black edge), `SIZE × scale` wide.
fn arrow(scale: f64) -> MemoryRenderBuffer {
    let px = pixels(scale);
    let mut pixels = vec![0u8; (px * px * 4) as usize];
    for y in 0..px {
        for x in 0..px {
            // The pixel's centre, in arrow units.
            let p = ((f64::from(x) + 0.5) / scale, (f64::from(y) + 0.5) / scale);
            if !inside(p) {
                continue;
            }
            let near_edge = edges().any(|(a, b)| distance(p, a, b) < EDGE);
            let value = if near_edge { 0x00 } else { 0xff };
            let i = ((y * px + x) * 4) as usize;
            // Fourcc ARGB8888 is B, G, R, A in memory; opaque, so it is
            // premultiplied as it is.
            pixels[i..i + 4].copy_from_slice(&[value, value, value, 0xff]);
        }
    }
    MemoryRenderBuffer::from_slice(
        &pixels,
        Fourcc::Argb8888,
        (px, px),
        1,
        Transform::Normal,
        None,
    )
}

fn edges() -> impl Iterator<Item = ((f64, f64), (f64, f64))> {
    (0..ARROW.len()).map(|i| (ARROW[i], ARROW[(i + 1) % ARROW.len()]))
}

/// Even-odd rule.
fn inside((x, y): (f64, f64)) -> bool {
    edges()
        .filter(|&((x1, y1), (x2, y2))| {
            (y1 > y) != (y2 > y) && x < x1 + (y - y1) * (x2 - x1) / (y2 - y1)
        })
        .count()
        % 2
        == 1
}

fn distance((px, py): (f64, f64), (x1, y1): (f64, f64), (x2, y2): (f64, f64)) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let t = (((px - x1) * dx + (py - y1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - x1 - t * dx).powi(2) + (py - y1 - t * dy).powi(2)).sqrt()
}

impl State {
    /// The cursor's elements on `output`, front first; none when the pointer
    /// is elsewhere or the client hid it.
    pub fn cursor_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<CursorElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let Some(pointer) = self.seat.get_pointer().map(|p| p.current_location()) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let near = Rectangle::<f64, Logical>::new(
            pointer - Point::from((f64::from(SIZE), f64::from(SIZE))),
            Size::from((f64::from(SIZE) * 2.0, f64::from(SIZE) * 2.0)),
        );
        if !near.overlaps(area.to_f64()) {
            return Vec::new();
        }
        let on_output = pointer - area.loc.to_f64();
        match &self.cursor {
            CursorImageStatus::Hidden => Vec::new(),
            CursorImageStatus::Surface(surface) => {
                let hotspot = with_states(surface, |states| {
                    states
                        .data_map
                        .get::<CursorImageSurfaceData>()
                        .and_then(|data| data.lock().ok().map(|d| d.hotspot))
                        .unwrap_or_default()
                });
                let at: Point<i32, Physical> = (on_output - hotspot.to_f64())
                    .to_physical(scale)
                    .to_i32_round();
                render_elements_from_surface_tree(renderer, surface, at, scale, 1.0, Kind::Cursor)
            }
            CursorImageStatus::Named(_) => {
                let (buffer, px) = self.arrows.at_scale(scale);
                let at = (on_output - Point::from(HOTSPOT)).to_physical(scale);
                // All of the buffer, shown SIZE logical pixels wide.
                let all = Rectangle::from_size(Size::from((f64::from(px), f64::from(px))));
                MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    at,
                    &buffer,
                    None,
                    Some(all),
                    Some(Size::from((SIZE, SIZE))),
                    Kind::Cursor,
                )
                .inspect_err(|err| tracing::warn!(?err, "could not draw the cursor"))
                .map(|element| vec![CursorElement::Arrow(element)])
                .unwrap_or_default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_has_a_white_body_and_a_black_edge() {
        assert!(inside((3.0, 6.0)));
        assert!(!inside((10.0, 3.0)));
        // Near the left edge: black; well inside: white.
        assert!(distance((1.5, 8.0), ARROW[0], ARROW[1]) < EDGE);
        assert!(edges().all(|(a, b)| distance((4.0, 10.0), a, b) >= EDGE));
    }
}
