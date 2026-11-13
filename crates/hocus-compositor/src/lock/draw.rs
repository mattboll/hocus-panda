//! What a screen shows while the session is locked, and as it opens.
//!
//! Locked, front first: the cursor, the lock's surface on that screen (the
//! shell's, or the external locker's), fading in over the desktop's last
//! frame (a picture taken by [`freeze`] as it locked, never a live
//! window), then black. A capture, a share or a recording sees the same
//! without the fade nor the picture: the lock, or black. Opening, the
//! lock's surface in two halves sliding apart over the desktop, live
//! again.

use std::time::Duration;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
            element::{
                AsRenderElements, Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::SolidColorRenderElement,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
                utils::{CropRenderElement, Relocate, RelocateRenderElement},
            },
        },
    },
    desktop::layer_map_for_output,
    output::Output,
    utils::{Physical, Point, Rectangle, Scale, Size, Transform},
};

use super::{Frozen, Holder};
use crate::{
    State,
    decoration::Keyed,
    going::Shaders,
    motion::{Bezier, Tween},
    render::{OutputElement, OutputElements, Purpose},
    wobble::Bend,
};

/// out-cubic (motion's `STANDARD`) for the fade in, in-out cubic
/// (`LARGE`) for the parting.
const OUT_CUBIC: Bezier = Bezier(0.33, 1.0, 0.68, 1.0);
const IN_OUT_CUBIC: Bezier = Bezier(0.65, 0.0, 0.35, 1.0);

impl State {
    /// What `output` shows for `purpose` while the session is locked,
    /// front first after the cursor; `None` when it is not locked.
    pub(crate) fn lock_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        purpose: Purpose,
        now: Duration,
    ) -> Option<Vec<OutputElement<R>>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let locked = self.lock.locked_state()?;
        // The wallpaper live under the desktop's own lock (#807), which
        // leaves its background clear.
        let live = self.lock_shows_wall();
        let mut fade = 1.0;
        let area = self.space.output_geometry(output)?;
        let scale = output.current_scale().fractional_scale();
        let sc = Scale::from(scale);
        let mut out: Vec<OutputElement<R>> = Vec::new();
        // The lock itself shows, not only black: its surface fully in, or
        // none to wait for (7.2 waits for it before the machine sleeps).
        let mut full = true;
        match &locked.holder {
            Holder::Own => {
                full = !self.shell_linked();
                if let Some(layer) = self.shell_lock_layer(output) {
                    let geometry = layer_map_for_output(output).layer_geometry(&layer);
                    let alpha = if purpose.screen() {
                        locked.shown_at.get(&output.name()).map_or(0.0, |at| {
                            let fade = Tween::new(*at, self.lock_fade(), OUT_CUBIC);
                            fade.at(now) as f32
                        })
                    } else {
                        1.0
                    };
                    full = alpha >= 1.0;
                    fade = alpha;
                    if let Some(geometry) = geometry.filter(|_| alpha > 0.0) {
                        let loc = geometry.loc.to_physical_precise_round(scale);
                        let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                            AsRenderElements::<R>::render_elements(
                                &layer, renderer, loc, sc, alpha,
                            );
                        out.extend(surfaces.into_iter().map(OutputElements::Surface));
                    }
                }
            }
            Holder::External(_) => {
                if let Some(surface) = self.external_surface_on(output) {
                    let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                        render_elements_from_surface_tree(
                            renderer,
                            &surface,
                            Point::<i32, Physical>::from((0, 0)),
                            sc,
                            1.0,
                            Kind::Unspecified,
                        );
                    out.extend(surfaces.into_iter().map(OutputElements::Surface));
                }
            }
        }
        // The desktop's last frame under the lock fading in: on the screen
        // only, never in a capture. Under a live lock, it fades out over
        // the wallpaper as the lock fades in.
        if purpose.screen()
            && (!live || fade < 1.0)
            && let Frozen::Taken(pictures) = &locked.frozen
            && let Some(picture) = pictures.get(&output.name())
        {
            let size = area.size;
            let alpha = live.then_some(1.0 - fade);
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                (0.0, 0.0),
                picture,
                alpha,
                None,
                Some(size),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(OutputElements::Backdrop(element)),
                Err(err) => tracing::debug!(%err, "the frozen frame could not be drawn"),
            }
        }
        if live {
            out.append(&mut self.live_wall(renderer, output, purpose, now));
        }
        out.push(OutputElements::Solid(SolidColorRenderElement::new(
            self.lock.black.clone(),
            Rectangle::from_size(area.size).to_physical_precise_round(scale),
            0,
            super::BLACK,
            Kind::Unspecified,
        )));
        if purpose.screen() {
            self.lock_drew_on(output, full);
        }
        Some(out)
    }

    /// The lock opening on `output`: its halves sliding apart, front
    /// first, over the desktop.
    pub(crate) fn lock_parting_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(until) = self.lock.opening_until() else {
            return Vec::new();
        };
        let Some(layer) = self.shell_lock_layer(output) else {
            return Vec::new();
        };
        let Some(geometry) = layer_map_for_output(output).layer_geometry(&layer) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let sc = Scale::from(scale);
        let start = until.saturating_sub(hocus_link::LOCK_PART);
        let t = Tween::new(start, hocus_link::LOCK_PART, IN_OUT_CUBIC).at(now);
        let full: Rectangle<i32, Physical> = Rectangle::new(
            geometry.loc.to_physical_precise_round(scale),
            geometry.size.to_physical_precise_round(scale),
        );
        let half = full.size.w / 2;
        let shift = (f64::from(half) * t).round() as i32;
        let halves = [
            (
                Rectangle::new(full.loc, Size::from((half, full.size.h))),
                -shift,
                &self.lock.halves[0],
            ),
            (
                Rectangle::new(
                    Point::from((full.loc.x + half, full.loc.y)),
                    Size::from((full.size.w - half, full.size.h)),
                ),
                shift,
                &self.lock.halves[1],
            ),
        ];
        let mut out = Vec::new();
        for (crop, dx, id) in halves {
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                AsRenderElements::<R>::render_elements(&layer, renderer, full.loc, sc, 1.0);
            for (i, surface) in surfaces.into_iter().enumerate() {
                let Some(cropped) = CropRenderElement::from_element(surface, sc, crop) else {
                    continue;
                };
                let moved =
                    RelocateRenderElement::from_element(cropped, (dx, 0), Relocate::Relative);
                // An id of its own for each half (and each of its
                // surfaces): they come from the one surface.
                let id = if i == 0 {
                    id.clone()
                } else {
                    smithay::backend::renderer::element::Id::new()
                };
                out.push(OutputElements::Part(Keyed::new(moved, id)));
            }
        }
        out
    }
}

/// Takes the desktop's last frame on every screen as the session locks,
/// before the first locked frame: what the lock fades in over. Drawn as
/// a capture is, read back once.
pub fn freeze<R, T>(renderer: &mut R, state: &mut State)
where
    R: Renderer
        + ImportAll
        + ImportMem
        + Offscreen<T>
        + Bind<T>
        + ExportMem
        + Bend
        + Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    if !state.lock.wants_freeze() {
        return;
    }
    let outputs: Vec<Output> = state.space.outputs().cloned().collect();
    let mut pictures = std::collections::HashMap::new();
    for output in &outputs {
        match picture(renderer, state, output) {
            Ok(picture) => {
                pictures.insert(output.name(), picture);
            }
            Err(err) => tracing::debug!(output = output.name(), "no frozen frame: {err:#}"),
        }
    }
    if let Some(locked) = state.lock.locked_mut() {
        locked.frozen = Frozen::Taken(pictures);
    }
}

/// Nothing to take the frozen frame with (a terminal in the background):
/// the lock comes over black.
pub fn no_freeze(state: &mut State) {
    if let Some(locked) = state.lock.locked_mut()
        && matches!(locked.frozen, Frozen::Wanted)
    {
        locked.frozen = Frozen::Gone;
    }
}

fn picture<R, T>(
    renderer: &mut R,
    state: &State,
    output: &Output,
) -> anyhow::Result<MemoryRenderBuffer>
where
    R: Renderer
        + ImportAll
        + ImportMem
        + Offscreen<T>
        + Bind<T>
        + ExportMem
        + Bend
        + Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    use anyhow::Context;
    let mode = output.current_mode().context("a screen without a mode")?;
    let size = output.current_transform().transform_size(mode.size);
    let scale = output.current_scale().fractional_scale();
    let elements = state.elements_for(renderer, output, Purpose::Freeze);
    let pixels = crate::capture::shoot::draw::<R, T>(
        renderer,
        size,
        scale,
        &elements,
        crate::render::BACKGROUND,
    )?;
    Ok(MemoryRenderBuffer::from_slice(
        &pixels,
        Fourcc::Argb8888,
        (size.w, size.h),
        1,
        Transform::Normal,
        None,
    ))
}
