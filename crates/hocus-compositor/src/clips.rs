//! What a screen's frame says changed, as the kernel takes it (#312).
//!
//! A window drawn scaled down (the overview, Alt+Tab, the welcome back's
//! chooser, the magnifier, mirroring) can turn a damage of one of its
//! pixels into an empty rectangle at a fractional scale. smithay passes
//! damage on to the plane's `FB_DAMAGE_CLIPS` as it is, and the DRM core
//! refuses a whole commit with an empty clip in it: the screen froze, every
//! page flip failing. Elements go to a DRM surface through [`Clipped`],
//! which leaves out empty rectangles.

use smithay::{
    backend::renderer::{
        Renderer,
        element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer as BufferCoords, Physical, Point, Rectangle, Scale, Transform},
};

/// An element whose damage and opaque regions hold no empty rectangle.
pub struct Clipped<'a, E>(pub &'a E);

/// Each of `elements`, to hand to a DRM surface.
pub fn clipped<E>(elements: &[E]) -> Vec<Clipped<'_, E>> {
    elements.iter().map(Clipped).collect()
}

fn kept(rect: &Rectangle<i32, Physical>) -> bool {
    rect.size.w > 0 && rect.size.h > 0
}

impl<E: Element> Element for Clipped<'_, E> {
    fn id(&self) -> &Id {
        self.0.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.0.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.0.location(scale)
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.0.src()
    }

    fn transform(&self) -> Transform {
        self.0.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.0.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.0
            .damage_since(scale, commit)
            .iter()
            .filter(|r| kept(r))
            .copied()
            .collect()
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.0
            .opaque_regions(scale)
            .iter()
            .filter(|r| kept(r))
            .copied()
            .collect()
    }

    fn alpha(&self) -> f32 {
        self.0.alpha()
    }

    fn kind(&self) -> Kind {
        self.0.kind()
    }
}

impl<R: Renderer, E: RenderElement<R>> RenderElement<R> for Clipped<'_, E> {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        self.0.draw(frame, src, dst, damage, opaque_regions)
    }

    fn underlying_storage(&self, renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        self.0.underlying_storage(renderer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window's picture scaled down: one pixel of its damage rounds to
    /// nothing.
    struct Scaled(Id);

    impl Element for Scaled {
        fn id(&self) -> &Id {
            &self.0
        }

        fn current_commit(&self) -> CommitCounter {
            CommitCounter::default()
        }

        fn src(&self) -> Rectangle<f64, BufferCoords> {
            Rectangle::from_size((189.0, 133.0).into())
        }

        fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
            Rectangle::new((1139, 674).into(), (283, 200).into())
        }

        fn damage_since(
            &self,
            _scale: Scale<f64>,
            _commit: Option<CommitCounter>,
        ) -> DamageSet<i32, Physical> {
            DamageSet::from_slice(&[
                Rectangle::new((0, 0).into(), (283, 0).into()),
                Rectangle::new((10, 20).into(), (4, 6).into()),
                Rectangle::new((5, 5).into(), (0, 9).into()),
            ])
        }

        fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
            OpaqueRegions::from_slice(&[
                Rectangle::new((0, 0).into(), (283, 0).into()),
                Rectangle::new((0, 1).into(), (283, 199).into()),
            ])
        }
    }

    #[test]
    fn empty_rectangles_never_reach_the_kernel() {
        let scaled = Scaled(Id::new());
        let elements = [scaled];
        let clipped = clipped(&elements);
        let scale = Scale::from(1.5);
        let damage: Vec<_> = clipped[0]
            .damage_since(scale, None)
            .iter()
            .copied()
            .collect();
        assert_eq!(damage, [Rectangle::new((10, 20).into(), (4, 6).into())]);
        let opaque: Vec<_> = clipped[0].opaque_regions(scale).iter().copied().collect();
        assert_eq!(opaque, [Rectangle::new((0, 1).into(), (283, 199).into())]);
        assert_eq!(clipped[0].geometry(scale), elements[0].geometry(scale));
    }
}
