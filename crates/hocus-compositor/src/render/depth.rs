//! Desktops in depth drawn (#806): the landscape in the wallpaper's place,
//! which it covers, under the layers of the bottom and background, the
//! other effects, the windows and the bar; drawn into a picture of its
//! own again only as the camera moves. On the screen alone: never in a
//! capture, a share's or a recording's frames.

use smithay::utils::{Physical, Rectangle};

use super::Frame;
use crate::{
    State,
    effects::{FireElement, depth::phases, draw::DepthLook},
    going::Shaders,
};

impl State {
    /// The landscape on `frame`'s output, when it is its wallpaper.
    pub(super) fn depth_layer<R>(&self, renderer: &mut R, frame: &Frame) -> Option<FireElement>
    where
        R: Shaders,
    {
        if !frame.purpose.screen() || !self.depth_shows_on(frame.output) {
            return None;
        }
        let depth = self.fx.depth.as_ref()?;
        let pane = depth.panes.get(&frame.output.name())?;
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let size = (screen.size.w, screen.size.h);
        let camera = self.depth_camera(frame.output, frame.now);
        let key = [
            (camera.x as f32).to_bits(),
            (camera.zoom as f32).to_bits(),
            (camera.drift as f32).to_bits(),
        ];
        let fresh = pane.drawn.get() != Some((key, size));
        let look = DepthLook {
            size: [frame.area.size.w as f32, frame.area.size.h as f32],
            scale: frame.scale as f32,
            cam: camera.x as f32,
            zoom: camera.zoom as f32,
            drift: camera.drift as f32,
            phases: phases(),
        };
        let texture = renderer.depth(&pane.kept, size, fresh, look)?;
        if fresh {
            pane.drawn.set(Some((key, size)));
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
