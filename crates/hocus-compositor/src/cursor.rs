//! The pointer's cursor: the client's own surface or shape when it sets
//! one, drawn at each output's scale.
//!
//! The shapes are drawn here rather than loaded from a cursor theme: they
//! are sharp at any scale, and the same on every machine. Themes come with
//! the theme work. They are
//! named as cursor-shape-v1 and CSS name them (`e-resize`, `move`…), which
//! are also the names of current Xcursor themes; a shape drawn here stands
//! for the names close to it, and the arrow for the others.

use std::{collections::BTreeMap, f64::consts::FRAC_PI_4, sync::Mutex};

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
    input::pointer::{CursorIcon, CursorImageStatus, CursorImageSurfaceData},
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
    wayland::compositor::with_states,
};

use crate::State;

/// Logical size of a cursor.
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
/// The middle of the other shapes, their hotspot.
const MIDDLE: (f64, f64) = (12.0, 12.0);
/// The black edge around the white body, in units.
const EDGE: f64 = 1.2;

/// A double arrow along x, around the middle: its tips and where its heads
/// start (along an edge, and longer across a corner, where the box is
/// wider), their half height, and the shaft's half thickness.
const TIP: f64 = 10.5;
const HEAD: f64 = 5.0;
const DIAGONAL_TIP: f64 = 13.0;
const DIAGONAL_HEAD: f64 = 7.5;
const HEAD_HALF: f64 = 5.5;
const SHAFT_HALF: f64 = 2.5;
/// The four-way arrow's: thinner, its heads narrower, to fit four.
const CROSS_TIP: f64 = 11.0;
const CROSS_HEAD: f64 = 6.5;
const CROSS_HEAD_HALF: f64 = 4.0;
const CROSS_SHAFT_HALF: f64 = 2.2;

render_elements! {
    pub CursorElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Arrow=MemoryRenderBufferRenderElement<R>,
}

/// The shapes the compositor draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Shape {
    Arrow,
    /// Up and down: the top and bottom edges.
    Vertical,
    /// Left and right.
    Horizontal,
    /// Up right and down left: the top-right and bottom-left corners.
    Rising,
    /// Up left and down right.
    Falling,
    /// Four ways: moving a window.
    Move,
}

impl Shape {
    fn of(icon: CursorIcon) -> Self {
        use CursorIcon as C;
        match icon {
            C::NResize | C::SResize | C::NsResize | C::RowResize => Self::Vertical,
            C::EResize | C::WResize | C::EwResize | C::ColResize => Self::Horizontal,
            C::NeResize | C::SwResize | C::NeswResize => Self::Rising,
            C::NwResize | C::SeResize | C::NwseResize => Self::Falling,
            C::Move | C::AllScroll | C::Grab | C::Grabbing => Self::Move,
            _ => Self::Arrow,
        }
    }

    /// The outline, in 24×24 units.
    fn outline(self) -> Vec<(f64, f64)> {
        match self {
            Self::Arrow => ARROW.to_vec(),
            Self::Horizontal => double_arrow(0.0, TIP, HEAD),
            Self::Vertical => double_arrow(2.0 * FRAC_PI_4, TIP, HEAD),
            // y grows downwards: up right is a turn back.
            Self::Rising => double_arrow(-FRAC_PI_4, DIAGONAL_TIP, DIAGONAL_HEAD),
            Self::Falling => double_arrow(FRAC_PI_4, DIAGONAL_TIP, DIAGONAL_HEAD),
            Self::Move => cross(),
        }
    }

    fn hotspot(self) -> (f64, f64) {
        match self {
            Self::Arrow => HOTSPOT,
            _ => MIDDLE,
        }
    }
}

/// A double arrow through the middle, its tips `tip` from it and its heads
/// from `head`, turned by `angle`.
fn double_arrow(angle: f64, tip: f64, head: f64) -> Vec<(f64, f64)> {
    let half = [
        (tip, 0.0),
        (head, HEAD_HALF),
        (head, SHAFT_HALF),
        (-head, SHAFT_HALF),
        (-head, HEAD_HALF),
    ];
    // The other half is the same, a half turn round.
    let points = half
        .iter()
        .chain(half.iter())
        .enumerate()
        .map(
            |(i, &(x, y))| {
                if i < half.len() { (x, y) } else { (-x, -y) }
            },
        );
    let (sin, cos) = angle.sin_cos();
    points
        .map(|(x, y)| (MIDDLE.0 + x * cos - y * sin, MIDDLE.1 + x * sin + y * cos))
        .collect()
}

/// A four-way arrow through the middle: one arm, turned a quarter at a
/// time.
fn cross() -> Vec<(f64, f64)> {
    let s = CROSS_SHAFT_HALF;
    let arm = [
        (s, -s),
        (CROSS_HEAD, -s),
        (CROSS_HEAD, -CROSS_HEAD_HALF),
        (CROSS_TIP, 0.0),
        (CROSS_HEAD, CROSS_HEAD_HALF),
        (CROSS_HEAD, s),
    ];
    let mut points = Vec::with_capacity(4 * arm.len());
    for turn in 0..4 {
        for &(x, y) in &arm {
            // A quarter turn, `turn` times: (x, y) to (-y, x).
            let (x, y) = (0..turn).fold((x, y), |(x, y), _| (-y, x));
            points.push((MIDDLE.0 + x, MIDDLE.1 + y));
        }
    }
    points
}

/// Cursor images already drawn, by shape and output scale (in
/// thousandths).
#[derive(Debug, Default)]
pub struct CursorImages(Mutex<BTreeMap<(Shape, u32), MemoryRenderBuffer>>);

impl CursorImages {
    /// The image of `shape` for `scale`, and its width in pixels.
    fn at_scale(&self, shape: Shape, scale: f64) -> (MemoryRenderBuffer, i32) {
        let key = (shape, (scale * 1000.0).round() as u32);
        let buffer = match self.0.lock() {
            Ok(mut cache) => cache
                .entry(key)
                .or_insert_with(|| draw(&shape.outline(), scale))
                .clone(),
            Err(_) => draw(&shape.outline(), scale),
        };
        (buffer, pixels(scale))
    }
}

fn pixels(scale: f64) -> i32 {
    (f64::from(SIZE) * scale).round().max(1.0) as i32
}

/// An outline as ARGB pixels (white body, black edge), `SIZE × scale` wide.
fn draw(outline: &[(f64, f64)], scale: f64) -> MemoryRenderBuffer {
    let px = pixels(scale);
    let mut pixels = vec![0u8; (px * px * 4) as usize];
    for y in 0..px {
        for x in 0..px {
            // The pixel's centre, in cursor units.
            let p = ((f64::from(x) + 0.5) / scale, (f64::from(y) + 0.5) / scale);
            if !inside(outline, p) {
                continue;
            }
            let near_edge = edges(outline).any(|(a, b)| distance(p, a, b) < EDGE);
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

fn edges(outline: &[(f64, f64)]) -> impl Iterator<Item = ((f64, f64), (f64, f64))> + '_ {
    (0..outline.len()).map(|i| (outline[i], outline[(i + 1) % outline.len()]))
}

/// Even-odd rule.
fn inside(outline: &[(f64, f64)], (x, y): (f64, f64)) -> bool {
    edges(outline)
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

/// What the pointer shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown<'a> {
    Hidden,
    /// A client's own image.
    Surface(&'a WlSurface),
    /// A shape by its name, the client's or the compositor's.
    Named(CursorIcon),
}

impl State {
    /// What the pointer shows: what the client under it set.
    pub fn shown_cursor(&self) -> Shown<'_> {
        if let Some(icon) = self.frame_cursor {
            return Shown::Named(icon);
        }
        match &self.cursor {
            CursorImageStatus::Hidden => Shown::Hidden,
            CursorImageStatus::Surface(surface) => Shown::Surface(surface),
            CursorImageStatus::Named(icon) => Shown::Named(*icon),
        }
    }

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
        match self.shown_cursor() {
            Shown::Hidden => Vec::new(),
            Shown::Surface(surface) => {
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
            Shown::Named(icon) => {
                let shape = Shape::of(icon);
                let (buffer, px) = self.cursor_images.at_scale(shape, scale);
                let at = (on_output - Point::from(shape.hotspot())).to_physical(scale);
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
        assert!(inside(&ARROW, (3.0, 6.0)));
        assert!(!inside(&ARROW, (10.0, 3.0)));
        // Near the left edge: black; well inside: white.
        assert!(distance((1.5, 8.0), ARROW[0], ARROW[1]) < EDGE);
        assert!(edges(&ARROW).all(|(a, b)| distance((4.0, 10.0), a, b) >= EDGE));
    }

    /// Whether `p` is white: inside, clear of the edge.
    fn white(shape: Shape, p: (f64, f64)) -> bool {
        let outline = shape.outline();
        inside(&outline, p) && edges(&outline).all(|(a, b)| distance(p, a, b) >= EDGE)
    }

    #[test]
    fn resize_arrows_point_both_ways_along_their_edge() {
        // White along the shaft and in both heads, nothing across.
        for p in [(12.0, 12.0), (5.0, 12.0), (19.0, 12.0)] {
            assert!(white(Shape::Horizontal, p), "{p:?}");
            assert!(white(Shape::Vertical, (p.1, p.0)), "{p:?}");
        }
        assert!(!inside(&Shape::Horizontal.outline(), (12.0, 5.0)));
        assert!(!inside(&Shape::Vertical.outline(), (5.0, 12.0)));
        // The bottom-right corner's arrow runs from top left to bottom
        // right; the bottom-left one's from top right to bottom left.
        assert!(white(Shape::Falling, (18.0, 18.0)));
        assert!(white(Shape::Falling, (6.0, 6.0)));
        assert!(!inside(&Shape::Falling.outline(), (18.0, 6.0)));
        assert!(white(Shape::Rising, (18.0, 6.0)));
        assert!(!inside(&Shape::Rising.outline(), (18.0, 18.0)));
    }

    #[test]
    fn the_move_cross_has_four_arms_and_fits() {
        for p in [
            (12.0, 12.0),
            (20.0, 12.0),
            (4.0, 12.0),
            (12.0, 20.0),
            (12.0, 4.0),
        ] {
            assert!(white(Shape::Move, p), "{p:?}");
        }
        assert!(!inside(&Shape::Move.outline(), (18.0, 18.0)));
        for shape in [Shape::Move, Shape::Rising, Shape::Falling, Shape::Vertical] {
            assert!(
                shape
                    .outline()
                    .iter()
                    .all(|&(x, y)| (0.0..=24.0).contains(&x) && (0.0..=24.0).contains(&y))
            );
        }
    }

    #[test]
    fn names_close_to_a_shape_share_it() {
        assert_eq!(Shape::of(CursorIcon::NsResize), Shape::Vertical);
        assert_eq!(Shape::of(CursorIcon::SeResize), Shape::Falling);
        assert_eq!(Shape::of(CursorIcon::SwResize), Shape::Rising);
        assert_eq!(Shape::of(CursorIcon::Text), Shape::Arrow);
    }
}
