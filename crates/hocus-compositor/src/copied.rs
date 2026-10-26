//! A frame the primary GPU drew for a screen of another GPU, as the one
//! element that screen shows (#123): smithay's multi-GPU renderer copies
//! it across. The frame is drawn already turned as the screen is, so its
//! transform is the screen's: the two cancel and the pixels land as drawn.

use smithay::{
    backend::renderer::{
        Frame, Renderer, Texture,
        element::{Element, Id, RenderElement},
        utils::{CommitCounter, OpaqueRegions},
    },
    utils::{Buffer, Physical, Rectangle, Scale, Size, Transform},
};

/// The whole screen, drawn.
#[derive(Debug)]
pub struct CopiedFrame<T> {
    id: Id,
    commit: CommitCounter,
    texture: T,
    /// The screen's transform.
    transform: Transform,
}

impl<T: Texture> CopiedFrame<T> {
    /// `commit` moves on with each frame drawn into `texture`.
    pub fn new(id: Id, commit: CommitCounter, texture: T, transform: Transform) -> Self {
        Self {
            id,
            commit,
            texture,
            transform,
        }
    }

    fn size(&self) -> Size<i32, Physical> {
        let size = self.transform.transform_size(self.texture.size());
        (size.w, size.h).into()
    }
}

impl<T: Texture> Element for CopiedFrame<T> {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size(self.texture.size().to_f64())
    }

    fn transform(&self) -> Transform {
        self.transform
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        Rectangle::from_size(self.size())
    }

    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::from_slice(&[Rectangle::from_size(self.size())])
    }
}

impl<R> RenderElement<R> for CopiedFrame<R::TextureId>
where
    R: Renderer,
    R::TextureId: Texture,
{
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            opaque_regions,
            self.transform,
            1.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use smithay::backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, ImportMem, Offscreen, damage::OutputDamageTracker,
            pixman::PixmanRenderer,
        },
    };

    use super::*;

    /// What a 3x2 picture of distinct pixels comes out as, drawn as a
    /// screen turned `transform` shows it.
    fn through(transform: Transform) -> (Vec<u8>, Vec<u8>) {
        let mut renderer = PixmanRenderer::new().expect("pixman");
        let (w, h) = (3, 2);
        let picture: Vec<u8> = (0..w * h)
            .flat_map(|i| [i as u8 * 40, 255 - i as u8 * 40, i as u8 * 7, 255])
            .collect();
        let texture = renderer
            .import_memory(&picture, Fourcc::Abgr8888, (w, h).into(), false)
            .expect("imported");
        let element = CopiedFrame::new(Id::new(), CommitCounter::default(), texture, transform);
        let mut target: smithay::reexports::pixman::Image<'static, 'static> = renderer
            .create_buffer(Fourcc::Abgr8888, (w, h).into())
            .expect("a buffer");
        let mut framebuffer = renderer.bind(&mut target).expect("bound");
        let mut tracker = OutputDamageTracker::new((w, h), 1.0, transform);
        tracker
            .render_output(&mut renderer, &mut framebuffer, 0, &[&element], [0.0; 4])
            .expect("drawn");
        let mapping = renderer
            .copy_framebuffer(
                &framebuffer,
                Rectangle::from_size((w, h).into()),
                Fourcc::Abgr8888,
            )
            .expect("read");
        let out = renderer.map_texture(&mapping).expect("mapped").to_vec();
        (picture, out)
    }

    #[test]
    fn the_frame_lands_as_drawn_whatever_the_screens_turn() {
        for transform in [
            Transform::Normal,
            Transform::_90,
            Transform::_180,
            Transform::Flipped270,
        ] {
            let (picture, out) = through(transform);
            assert_eq!(out, picture, "turned {transform:?}");
        }
    }
}
