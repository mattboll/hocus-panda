//! Visage (lot 4): an accessory that follows the head, one at a time:
//! Oreilles, Bonnet, Couronne, Lunettes.
//!
//! Each is the mockup's own drawing (the window's preview, its figure
//! 110 × 144 at scale 1: the ears, the beanie and the crown over its head,
//! the glasses over its eyes), in the figure's pixels, where it sits on
//! the figure, laid on the face found ([`crate::face`]) by the head's
//! pose, turned as the eyes' line is (roll):
//!
//! - **The glasses** on the eyes: the figure's eyes' middle, (55, 55), on
//!   theirs, the figure's eyes' span, 17.6 px, scaled to theirs.
//! - **The others** on the head: the figure's forehead's top, at its
//!   hair, (55, 34), on the face's (where the hair starts), the figure's
//!   face's width, 46.2 px, scaled to the face's (temple to temple) and
//!   [`HAIR`] more, a real head being wider than its face with its hair.
//!
//! The same on the GPU's path and the CPU's: drawn over the made frame.

use std::sync::OnceLock;

use hocus_link::camera::Wear;
use resvg::{tiny_skia::Transform, usvg};

use crate::{face::Pose, paint};

/// The mockup's figure's eyes: their middle and their span.
const EYES: (f32, f32) = (55.0, 55.0);
const SPAN: f32 = 17.6;
/// Its forehead's top, at the hair, and its face's width.
const FOREHEAD: (f32, f32) = (55.0, 34.0);
const FACE_WIDTH: f32 = 46.2;
/// A head with its hair is this much wider than its face.
pub const HAIR: f32 = 1.15;

/// The ears: two discs, `#3A3D46` round a `#1A1C22` middle (the mockup's
/// `box-shadow: inset 0 0 0 9px` on 24 px discs).
const EARS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="110" height="144">
<circle cx="37" cy="16" r="12" fill="#3A3D46"/><circle cx="37" cy="16" r="3" fill="#1A1C22"/>
<circle cx="75" cy="16" r="12" fill="#3A3D46"/><circle cx="75" cy="16" r="3" fill="#1A1C22"/>
</svg>"##;

/// The beanie: its dome, its brim, its pompom.
const BEANIE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="110" height="144">
<path d="M30 32 A25 24 0 0 1 80 32 Q80 36 76 36 L34 36 Q30 36 30 32 Z" fill="#D9534F"/>
<rect x="28" y="30" width="54" height="9" rx="4" fill="#B33F3B"/>
<circle cx="55.5" cy="6.5" r="5.5" fill="#F4F1EA"/>
</svg>"##;

/// The crown: the mockup's path, 44 × 28 at (33, 2).
const CROWN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="110" height="144">
<g transform="translate(33 2) scale(1.1 1.0769)">
<path d="M2 24 L4 6 L13 15 L20 2 L27 15 L36 6 L38 24 Z" fill="#F5C542" stroke="#C9971E" stroke-width="1.5" stroke-linejoin="round"/>
</g></svg>"##;

/// The glasses: in the figure's own units (its 100 × 131 drawn 110 × 144).
const GLASSES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="110" height="144">
<g transform="scale(1.1)" stroke="#14161C" stroke-width="2.4" fill="rgba(143,179,255,0.25)">
<rect x="33" y="44" width="15" height="11" rx="4"/><rect x="52" y="44" width="15" height="11" rx="4"/>
<path d="M48 48 h4" fill="none"/>
</g></svg>"##;

fn drawing(wear: Wear) -> Option<&'static usvg::Tree> {
    static TREES: OnceLock<[Option<usvg::Tree>; 4]> = OnceLock::new();
    let trees = TREES.get_or_init(|| [EARS, BEANIE, CROWN, GLASSES].map(paint::tree));
    let i = match wear {
        Wear::Ears => 0,
        Wear::Beanie => 1,
        Wear::Crown => 2,
        Wear::Glasses => 3,
    };
    trees.get(i)?.as_ref()
}

/// The figure's pixels to the frame's, for `wear` on a head at `pose`.
pub fn transform(wear: Wear, pose: &Pose) -> Transform {
    let (from, k, to) = match wear {
        Wear::Glasses => (EYES, pose.span / SPAN, pose.eyes),
        Wear::Ears | Wear::Beanie | Wear::Crown => {
            (FOREHEAD, HAIR * pose.width / FACE_WIDTH, pose.forehead)
        }
    };
    Transform::from_translate(-from.0, -from.1)
        .post_scale(k, k)
        .post_concat(Transform::from_rotate(pose.roll.to_degrees()))
        .post_translate(to.0, to.1)
}

/// `wear` on the head at `pose`, over `frame` (B, G, R, x), at `opacity`.
pub fn draw(frame: &mut [u8], wear: Wear, pose: &Pose, opacity: f32) {
    if let Some(tree) = drawing(wear) {
        paint::svg_over(frame, tree, transform(wear, pose), opacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        face::FaceTracker,
        look::{BYTES, H, W},
        track::Frame,
    };

    /// Each accessory changes the pixels around the head, where it goes
    /// (above the eyes, or on them for the glasses), and none elsewhere:
    /// not the frame's corners, not the shoulders.
    #[test]
    fn an_accessory_changes_the_head_and_nothing_else() -> anyhow::Result<()> {
        let picture = crate::face::test_face();
        let mut tracker = FaceTracker::new()?;
        let frame = Frame {
            pixels: &picture,
            width: W,
            height: H,
            order: [0, 1, 2],
        };
        let face = tracker.track(&frame, 1.0 / 30.0, false)?.expect("a face");
        let pose = face.pose();
        let bgrx: Vec<u8> = picture
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [b, g, r, 255])
            .collect();
        assert_eq!(bgrx.len(), BYTES);
        let (ex, ey) = pose.eyes;
        for wear in [Wear::Ears, Wear::Beanie, Wear::Crown, Wear::Glasses] {
            let mut drawn = bgrx.clone();
            draw(&mut drawn, wear, &pose, 1.0);
            let changed: Vec<(usize, usize)> = drawn
                .as_chunks::<4>()
                .0
                .iter()
                .zip(bgrx.as_chunks::<4>().0)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, _)| (i % W, i / W))
                .collect();
            assert!(changed.len() > 2_000, "{wear:?}: {} pixels", changed.len());
            // All of it within the head's reach: three spans aside, four
            // above, one below the eyes.
            let reach = pose.span;
            for &(x, y) in &changed {
                let (dx, dy) = (x as f32 - ex, y as f32 - ey);
                assert!(
                    dx.abs() < 3.0 * reach && dy > -4.0 * reach && dy < reach,
                    "{wear:?} at ({x}, {y}), the eyes at ({ex}, {ey})"
                );
            }
            // Above the eyes, but the glasses: on them.
            let mean_y = changed.iter().map(|&(_, y)| y as f32).sum::<f32>() / changed.len() as f32;
            if wear == Wear::Glasses {
                assert!((mean_y - ey).abs() < 0.3 * reach, "{wear:?}: {mean_y}");
            } else {
                assert!(mean_y < ey - 1.0 * reach, "{wear:?}: {mean_y}");
            }
        }
        Ok(())
    }
}
