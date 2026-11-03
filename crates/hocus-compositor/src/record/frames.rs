//! What the backend draws for a recording, with its renderer, at each turn
//! of the loop: its frame when something changed, at most
//! [`RECORD_FPS`](hocus_link::RECORD_FPS) a second, while it records.
//! The same way as a share's ([`crate::share::frames`]): drawn into a
//! buffer of its own as the screen shows it but for what only the screen
//! shows (the capture's drawing, the recording's frame, the bar's item),
//! the pointer in it; then handed over without the loop copying it, its
//! pixels lent to the recorder's thread.
//!
//! An area is its screen drawn moved by the area's corner into a buffer of
//! the area's size; a window, the window alone (its surfaces and popups),
//! the pointer when it is over it, at the size it had as the recording
//! started (cut, or black around it, when it changes).

use std::time::Duration;

use smithay::{
    backend::renderer::{
        Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
        element::utils::{Relocate, RelocateRenderElement},
    },
    utils::{Physical, Point},
};

use super::{Source, frame_key};
use crate::{
    State,
    going::Shaders,
    model::WindowId,
    render::{BACKGROUND, OutputElements, Purpose},
    share::{
        frames::{Drawn, Frames, Lends, draw_and_hand},
        sink::FrameTo,
    },
    wobble::Bend,
};

/// The recording's frame, if one is due.
pub fn run<R, T>(renderer: &mut R, state: &mut State, frames: &mut Frames<R, T>)
where
    R: Renderer
        + ImportAll
        + ImportMem
        + Offscreen<T>
        + Bind<T>
        + ExportMem
        + Lends<T>
        + Bend
        + Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    let due = state.redraw || std::mem::take(&mut state.record.due);
    let now = state.now();
    let Some(live) = state.record.live() else {
        return;
    };
    if live.paused || !live.started || (!due && !live.fresh) {
        return;
    }
    let key = frame_key(live.id);
    // Its buffer lent still (the CPU's): drawn once it is back.
    if frames.lent(key) {
        state.record.due = true;
        return;
    }
    let every = Duration::from_secs(1) / hocus_link::RECORD_FPS;
    if let Some(last) = live.last
        && now < last + every
    {
        let at = last + every - now;
        state.record_pace(at);
        return;
    }
    let (source, size, fresh, to) = (
        live.source.clone(),
        live.size,
        live.fresh,
        FrameTo::Record(live.sender.clone()),
    );
    let since = std::time::Instant::now();
    let drawn = match &source {
        Source::Screen(name) | Source::Area { output: name, .. } => {
            let Some(output) = state.space.outputs().find(|o| o.name() == *name).cloned() else {
                return;
            };
            let scale = output.current_scale().fractional_scale();
            let elements = state.elements_for(renderer, &output, Purpose::Capture { cursor: true });
            let corner: Point<i32, Physical> = match &source {
                Source::Area { area, .. } => {
                    Point::<f64, smithay::utils::Logical>::from((area.x, area.y))
                        .to_physical_precise_round(scale)
                }
                _ => (0, 0).into(),
            };
            let moved: Vec<RelocateRenderElement<_>> = elements
                .into_iter()
                .map(|e| {
                    RelocateRenderElement::from_element(
                        e,
                        Point::from((-corner.x, -corner.y)),
                        Relocate::Relative,
                    )
                })
                .collect();
            draw_and_hand(
                renderer, frames, &to, key, &moved, scale, BACKGROUND, size, fresh,
            )
        }
        Source::Window(id) => {
            let window = WindowId(*id);
            let Some(output) = state.window_output(window) else {
                return;
            };
            let scale = output.current_scale().fractional_scale();
            let Some((mut elements, rect)) = state.window_alone(renderer, window, scale) else {
                return;
            };
            if state.pointer_over(window) {
                let cursor = state.cursor_elements_in(renderer, rect, scale);
                elements.splice(0..0, cursor.into_iter().map(OutputElements::Cursor));
            }
            draw_and_hand(
                renderer,
                frames,
                &to,
                key,
                &elements,
                scale,
                [0.0, 0.0, 0.0, 1.0],
                size,
                fresh,
            )
        }
    };
    match drawn {
        Ok(Drawn::Went) => state.record_frame_went(now, since),
        Ok(Drawn::Same) => {}
        Ok(Drawn::Owed) => state.record.owe(),
        Err(err) => state.record_failed(&format!("{err:#}")),
    }
    if let Source::Window(id) = source
        && let (Some(managed), Some(output)) = (
            state.windows.get(&WindowId(id)),
            state.window_output(WindowId(id)),
        )
    {
        let now: Duration = state.clock.now().into();
        managed
            .window
            .send_frame(&output, now, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
    }
}
