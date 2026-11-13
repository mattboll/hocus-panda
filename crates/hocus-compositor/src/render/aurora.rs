//! The aurora following the music drawn (#801): in the wallpaper's place,
//! which it covers, under the layers of the bottom and background, the
//! other effects, the windows and the bar; drawn into a picture of its own
//! again only as it moves, at most 60 times a second, the whole of it then
//! changed. Behind the glass stands its picture as it last stood still. On
//! the screen alone: never in a capture, a share's or a recording's
//! frames.

use smithay::{
    backend::renderer::element::Id,
    utils::{Physical, Rectangle},
};

use super::Frame;
use crate::{
    State,
    effects::{
        FireElement,
        aurora::{Picture, picture_size},
        draw::AuroraLook,
    },
    going::Shaders,
};

/// The aurora on a screen this frame: as it stood still, standing for it
/// behind the glass, and as it moves.
pub(super) struct AuroraLayer {
    pub stand_in: FireElement,
    pub live: FireElement,
}

/// `picture` over `screen`, drawn again when `look` or `size` changed.
fn element<R: Shaders>(
    renderer: &mut R,
    picture: &Picture,
    size: (i32, i32),
    look: AuroraLook,
    screen: Rectangle<i32, Physical>,
) -> Option<FireElement> {
    let key = (look.key(), size);
    let fresh = picture.drawn.get() != Some(key);
    let texture = renderer.aurora(&picture.kept, size, fresh, look)?;
    if fresh {
        picture.drawn.set(Some(key));
        let mut commit = picture.commit.get();
        commit.increment();
        picture.commit.set(commit);
    }
    Some(FireElement::picture(
        picture.id.clone(),
        picture.commit.get(),
        screen,
        texture,
    ))
}

impl State {
    /// The aurora on `frame`'s output, when it is its wallpaper.
    pub(super) fn aurora_layer<R>(&self, renderer: &mut R, frame: &Frame) -> Option<AuroraLayer>
    where
        R: Shaders,
    {
        let _measure = crate::effects::timing::build(crate::effects::timing::Fx::Aurora);
        if !frame.purpose.screen() || !self.aurora_shows_on(frame.output) {
            return None;
        }
        let aurora = self.fx.aurora.as_ref()?;
        let pane = aurora.panes.get(&frame.output.name())?;
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let logical = frame.area.size.to_f64();
        let size = picture_size((logical.w, logical.h));
        let at = [logical.w as f32, logical.h as f32];
        let live = element(renderer, &pane.live, size, aurora.look(at), screen)?;
        let stand_in = element(renderer, &pane.still, size, aurora.still_look(at), screen)?;
        Some(AuroraLayer { stand_in, live })
    }

    /// The id of the picture standing for the aurora behind the glass on
    /// `frame`'s output.
    pub(super) fn aurora_stand_in(&self, frame: &Frame) -> Option<Id> {
        let pane = self.fx.aurora.as_ref()?.panes.get(&frame.output.name())?;
        Some(pane.still.id.clone())
    }
}
