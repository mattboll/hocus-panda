//! The desktop's background ([`hocus_link::backdrop`]), as the compositor
//! draws it under everything.

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::Transform,
};

pub use hocus_link::backdrop::SHRINK;

/// The background of a screen `size` physical pixels large: a picture to
/// draw stretched over it.
pub fn picture(size: (i32, i32)) -> MemoryRenderBuffer {
    let (w, h, pixels) = hocus_link::backdrop::pixels(size);
    MemoryRenderBuffer::from_slice(
        &pixels,
        Fourcc::Abgr8888,
        (w, h),
        1,
        Transform::Normal,
        None,
    )
}
