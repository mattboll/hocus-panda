//! The bar's opaque colour behind one of its surfaces (bar.md §11): on
//! the CPU, and on the GPU with reduced transparency or high contrast.
//!
//! It is a geometry of its own, under the surface's drawing. iced's GPU
//! renderer (iced_wgpu 0.14) keeps a frame's shapes in two lists: those
//! drawn in a clip (`Frame::with_clip`) join it as the clip ends, those
//! drawn on the frame itself only as it becomes a geometry, after them. A
//! backdrop filled on the frame the surface draws in then covered every
//! shape drawn in a clip: icons, gauges, check boxes; only text and
//! images, which come after shapes, showed on it. The CPU's renderer keeps
//! the order things are drawn in, so its bars never showed it.
//!
//! The same goes for any shape drawn on a frame before a clip it would be
//! under (an item's hover, its ring, a field's fill): it is drawn in a
//! clip of its own ([`layer`]), and clips keep their order on both
//! renderers.

use iced::{
    Color, Point, Rectangle, Renderer, Size,
    widget::canvas::{Frame, Geometry},
};

/// `draw` in a clip of its own, `region`: on the GPU too it stays under
/// the clips drawn after it on `frame`, as it does on the CPU.
pub fn layer<R>(frame: &mut Frame, region: Rectangle, draw: impl FnOnce(&mut Frame) -> R) -> R {
    frame.with_clip(region, draw)
}

/// The backdrop's geometry, `size` filled with `colour`, if there is one.
pub fn under(renderer: &Renderer, size: Size, colour: Option<Color>) -> Option<Geometry> {
    colour.map(|colour| {
        let mut frame = Frame::new(renderer, size);
        frame.fill_rectangle(Point::ORIGIN, size, colour);
        frame.into_geometry()
    })
}

/// What a surface returns: its backdrop first, then its drawing.
pub fn over(under: Option<Geometry>, frame: Frame) -> Vec<Geometry> {
    under.into_iter().chain([frame.into_geometry()]).collect()
}

#[cfg(test)]
pub mod tests {
    use iced::{
        Color, Font, Pixels, Rectangle, Renderer, Size, Theme,
        advanced::{graphics::geometry::Renderer as _, renderer::Headless},
        mouse,
        widget::canvas::{Geometry, Program},
    };

    /// A CPU renderer, as the shell's tests have no GPU.
    fn renderer() -> Renderer {
        let new = <Renderer as Headless>::new(Font::DEFAULT, Pixels(14.0), Some("tiny-skia"));
        block_on(new).expect("the CPU renderer")
    }

    /// The futures the renderer's constructor returns are ready at once.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::{
            pin::pin,
            task::{Context, Poll, Waker},
        };
        let mut future = pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("the renderer's constructor waits"),
        }
    }

    /// `geometries` drawn on a transparent surface of `size`, RGBA.
    fn pixels(renderer: &mut Renderer, size: Size<u32>, geometries: Vec<Geometry>) -> Vec<u8> {
        let bounds = Size::new(size.width as f32, size.height as f32);
        iced::advanced::Renderer::reset(renderer, Rectangle::with_size(bounds));
        for geometry in geometries {
            renderer.draw_geometry(geometry);
        }
        renderer.screenshot(size, 1.0, Color::TRANSPARENT)
    }

    /// What `program` draws on a transparent surface of `size`, RGBA.
    pub fn pixels_of<P: Program<crate::Message>>(
        program: &P,
        state: &P::State,
        size: Size<u32>,
    ) -> Vec<u8> {
        let mut renderer = renderer();
        let bounds = Rectangle::with_size(Size::new(size.width as f32, size.height as f32));
        let drawn = program.draw(
            state,
            &renderer,
            &Theme::Dark,
            bounds,
            mouse::Cursor::Unavailable,
        );
        pixels(&mut renderer, size, drawn)
    }

    /// The pixels `program` draws on a surface of `size`, made with
    /// `backdrop` or without (`make`): drawn without a backdrop, and,
    /// with one, its backdrop alone and its drawing alone.
    pub struct Drawn {
        pub glass: Vec<u8>,
        pub backdrop: Vec<u8>,
        pub drawing: Vec<u8>,
    }

    pub fn drawn<P: Program<crate::Message>>(
        make: impl Fn(Option<Color>) -> P,
        state: &P::State,
        size: Size<u32>,
        colour: Color,
    ) -> Drawn {
        let mut renderer = renderer();
        let bounds = Rectangle::with_size(Size::new(size.width as f32, size.height as f32));
        let draw = |program: &P, renderer: &Renderer| {
            program.draw(
                state,
                renderer,
                &Theme::Dark,
                bounds,
                mouse::Cursor::Unavailable,
            )
        };
        let glass = draw(&make(None), &renderer);
        let glass = pixels(&mut renderer, size, glass);
        let mut opaque = draw(&make(Some(colour)), &renderer);
        assert!(
            opaque.len() >= 2,
            "the backdrop is a geometry of its own, under the drawing"
        );
        let drawing = opaque.split_off(1);
        let backdrop = pixels(&mut renderer, size, opaque);
        let drawing = pixels(&mut renderer, size, drawing);
        Drawn {
            glass,
            backdrop,
            drawing,
        }
    }

    /// The backdrop alone is flat, and the drawing over it the same,
    /// pixel for pixel, as without it: whatever order a renderer puts a
    /// frame's shapes in, none of them can go under the backdrop.
    pub fn assert_drawn_over(drawn: &Drawn, what: &str) {
        let first = &drawn.backdrop[..4];
        assert!(
            first[3] == 255 && drawn.backdrop.chunks(4).all(|p| p == first),
            "{what}: the backdrop alone is flat and opaque"
        );
        assert!(
            drawn.glass.chunks(4).filter(|p| p[3] > 0).count() > 100,
            "{what}: something is drawn"
        );
        assert!(
            drawn.drawing == drawn.glass,
            "{what}: drawn the same over the backdrop as on glass"
        );
    }
}
