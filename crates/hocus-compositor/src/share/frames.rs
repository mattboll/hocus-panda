//! What the backend draws for sharing, with its renderer, at each turn of
//! the loop before its own frames: the chooser's thumbnails when they are
//! due, and each share's frame when something changed and its consumer's
//! pace allows.
//!
//! A share's frame is drawn into a buffer of its own, which keeps its
//! pixels from one frame to the next (only what changed is drawn again,
//! and nothing is sent when nothing did), then copied out for reading
//! (the GPU does it while the loop goes on); its pixels are taken at the
//! loop's next turn, a few milliseconds later, and handed to the
//! streams' thread. A window shared on another desktop, or minimised, is
//! told when its frame was drawn, so that it keeps drawing.

use std::{collections::HashMap, time::Duration};

use hocus_link::ShareSource;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
        },
    },
    utils::{Buffer as BufferCoords, Physical, Rectangle, Size, Transform},
};

use super::thumbs::Job;
use crate::{
    State,
    going::Shaders,
    model::WindowId,
    render::{BACKGROUND, OutputElement, OutputElements, Purpose},
    wobble::Bend,
};

/// A share's buffer, and what it last drew.
struct Target<T> {
    buffer: T,
    damage: OutputDamageTracker,
    size: Size<i32, Physical>,
}

/// A frame copied out, its pixels to take.
struct Reading<M> {
    id: u64,
    mapping: M,
    size: (u32, u32),
}

/// The backend's part of sharing: each share's buffer, and the frames
/// being read back.
pub struct Frames<R: ExportMem, T> {
    targets: HashMap<u64, Target<T>>,
    reading: Vec<Reading<R::TextureMapping>>,
}

impl<R: ExportMem, T> Default for Frames<R, T> {
    fn default() -> Self {
        Self {
            targets: HashMap::new(),
            reading: Vec::new(),
        }
    }
}

impl<R: ExportMem, T> std::fmt::Debug for Frames<R, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frames")
            .field("targets", &self.targets.len())
            .field("reading", &self.reading.len())
            .finish()
    }
}

/// Draws what sharing needs now.
pub fn run<R, T>(renderer: &mut R, state: &mut State, frames: &mut Frames<R, T>)
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
    // What was copied out at the last turn: its pixels, to the stream.
    for reading in std::mem::take(&mut frames.reading) {
        let bytes = match renderer.map_texture(&reading.mapping) {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!(?err, id = reading.id, "a share's frame could not be read");
                continue;
            }
        };
        let mut pixels = state.share.spare_buffer(bytes.len());
        pixels.clear();
        pixels.extend_from_slice(bytes);
        state.send_frame(reading.id, pixels, reading.size);
    }
    frames
        .targets
        .retain(|id, _| state.share.shares.contains_key(id));
    thumbnails(renderer, state);
    if state.share.shares.is_empty() {
        return;
    }
    let due = state.redraw || std::mem::take(&mut state.share.streams_due);
    let now = state.now();
    let ids: Vec<u64> = state.share.shares.keys().copied().collect();
    for id in ids {
        let Some(share) = state.share.shares.get(&id) else {
            continue;
        };
        let (Some(_), true, Some((agreed, fps))) = (share.node, share.streaming, share.agreed)
        else {
            continue;
        };
        if !due && !share.fresh {
            continue;
        }
        let every = Duration::from_micros(1_000_000 / u64::from(fps.max(1)));
        if let Some(last) = share.last
            && now < last + every
        {
            state.pace_streams(last + every - now);
            continue;
        }
        let fresh = share.fresh;
        let source = share.source.clone();
        let pointer = share.pointer;
        match frame::<R, T>(renderer, state, frames, id, &source, pointer, agreed, fresh) {
            Ok(Some(reading)) => {
                frames.reading.push(reading);
                if let Some(share) = state.share.shares.get_mut(&id) {
                    share.last = Some(now);
                    share.fresh = false;
                }
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(id, "a share's frame could not be drawn: {err:#}"),
        }
        if let ShareSource::Window(w) = source {
            window_drawn(state, WindowId(w));
        }
    }
    if !frames.reading.is_empty() {
        state.read_soon();
    }
}

/// Share `id`'s frame drawn into its buffer, `agreed` pixels large, and
/// copied out if something changed (or `fresh`: a frame is owed).
#[allow(clippy::too_many_arguments)]
fn frame<R, T>(
    renderer: &mut R,
    state: &mut State,
    frames: &mut Frames<R, T>,
    id: u64,
    source: &ShareSource,
    pointer: bool,
    agreed: (u32, u32),
    fresh: bool,
) -> anyhow::Result<Option<Reading<R::TextureMapping>>>
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
    let (elements, scale, clear) = match source {
        ShareSource::Screen(name) => {
            let output = state
                .space
                .outputs()
                .find(|o| o.name() == *name)
                .cloned()
                .context("the screen went")?;
            let scale = output.current_scale().fractional_scale();
            let elements =
                state.elements_for(renderer, &output, Purpose::Capture { cursor: pointer });
            (elements, scale, BACKGROUND)
        }
        ShareSource::Window(w) => {
            let window = WindowId(*w);
            let output = state
                .window_output(window)
                .context("the window is on no screen")?;
            let scale = output.current_scale().fractional_scale();
            let (mut elements, rect) = state
                .window_alone(renderer, window, scale)
                .context("the window went")?;
            if pointer && state.pointer_over(window) {
                let cursor = state.cursor_elements_in(renderer, rect, scale);
                elements.splice(0..0, cursor.into_iter().map(OutputElements::Cursor));
            }
            // Its size now: the stream follows it.
            let now = (
                (rect.size.w * scale).round().max(1.0) as u32,
                (rect.size.h * scale).round().max(1.0) as u32,
            );
            state.share_resized(id, now);
            (elements, scale, [0.0; 4])
        }
    };
    let size: Size<i32, Physical> = (agreed.0 as i32, agreed.1 as i32).into();
    anyhow::ensure!(size.w > 0 && size.h > 0, "nothing to draw");
    if frames.targets.get(&id).is_none_or(|t| t.size != size) {
        let buffer_size: Size<i32, BufferCoords> = (size.w, size.h).into();
        let buffer = renderer
            .create_buffer(Fourcc::Abgr8888, buffer_size)
            .or_else(|_| renderer.create_buffer(Fourcc::Argb8888, buffer_size))
            .context("no buffer to draw the share into")?;
        frames.targets.insert(
            id,
            Target {
                buffer,
                damage: OutputDamageTracker::new(size, scale, Transform::Normal),
                size,
            },
        );
    }
    let target = frames.targets.get_mut(&id).context("the share's buffer")?;
    let elements: &[OutputElement<R>] = &elements;
    let mut framebuffer = renderer
        .bind(&mut target.buffer)
        .context("could not bind")?;
    let drawn = target
        .damage
        .render_output(renderer, &mut framebuffer, 1, elements, clear)
        .map_err(|err| anyhow::anyhow!("{err:?}"))
        .context("could not draw")?;
    let changed = drawn.damage.is_some_and(|d| !d.is_empty());
    if !changed && !fresh {
        return Ok(None);
    }
    let region = Rectangle::from_size((size.w, size.h).into());
    let mapping = renderer
        .copy_framebuffer(&framebuffer, region, Fourcc::Argb8888)
        .context("could not copy the frame out")?;
    Ok(Some(Reading {
        id,
        mapping,
        size: agreed,
    }))
}

/// Window `id`, shared, is told its frame was drawn, wherever it is.
fn window_drawn(state: &State, id: WindowId) {
    let (Some(managed), Some(output)) = (state.windows.get(&id), state.window_output(id)) else {
        return;
    };
    let now: Duration = state.clock.now().into();
    managed
        .window
        .send_frame(&output, now, Some(Duration::ZERO), |_, _| {
            Some(output.clone())
        });
}

/// The chooser's thumbnails, if due: each source drawn at its own
/// resolution and read back at once (a few, twice a second at most, while
/// the chooser shows), then made small on their thread.
fn thumbnails<R, T>(renderer: &mut R, state: &mut State)
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
    let scales: HashMap<String, f64> = state
        .space
        .outputs()
        .map(|o| (o.name(), o.current_scale().fractional_scale()))
        .collect();
    let asks = state
        .share
        .take_thumbs(|name| scales.get(name).copied().unwrap_or(1.0));
    if asks.is_empty() {
        return;
    }
    let mut jobs = Vec::with_capacity(asks.len());
    for ask in asks {
        let drawn = match &ask.source {
            ShareSource::Screen(name) => state
                .space
                .outputs()
                .find(|o| o.name() == *name)
                .cloned()
                .and_then(|o| {
                    crate::capture::shoot::screen::<R, T>(renderer, state, &o, false)
                        .map_err(|err| tracing::debug!("a screen's thumbnail: {err:#}"))
                        .ok()
                })
                .map(|piece| (piece.pixels, piece.size)),
            ShareSource::Window(w) => {
                let id = WindowId(*w);
                let scale = state
                    .window_output(id)
                    .map_or(1.0, |o| o.current_scale().fractional_scale());
                state
                    .window_alone(renderer, id, scale)
                    .and_then(|(elements, rect)| {
                        let size: Size<i32, Physical> = rect.size.to_physical_precise_round(scale);
                        crate::capture::shoot::draw::<R, T>(
                            renderer, size, scale, &elements, [0.0; 4],
                        )
                        .map_err(|err| tracing::debug!("a window's thumbnail: {err:#}"))
                        .ok()
                        .map(|pixels| (pixels, (size.w as u32, size.h as u32)))
                    })
            }
        };
        if let Some((pixels, size)) = drawn {
            jobs.push(Job {
                request: ask.request,
                source: ask.source,
                pixels,
                size,
                room: ask.room,
                radius: ask.radius,
            });
        }
    }
    if let Some(maker) = state.thumb_maker() {
        for job in jobs {
            maker.make(job);
        }
    }
}
