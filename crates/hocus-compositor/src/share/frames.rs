//! What the backend draws for sharing, with its renderer, at each turn of
//! the loop before its own frames: the chooser's thumbnails when they are
//! due, and each share's frame when something changed and its consumer's
//! pace allows.
//!
//! A share's frame is drawn into a buffer of its own, which keeps its
//! pixels from one frame to the next (only what changed is drawn again,
//! and nothing is sent when nothing did). The loop then hands it over
//! without copying it (16 MB a frame at 2560 × 1600: a hard rule's
//! matter, [`super::lease`]):
//!
//! - **On the GPU** (GLES), it asks for the frame's pixels into a pixel
//!   buffer and a fence after it, and goes on; the read-back thread waits
//!   for the GPU, maps the buffer (where Mesa may copy), lends its memory
//!   to the streams' thread and unmaps it once given back
//!   ([`super::glread`]). The share's buffer can be drawn into again at
//!   once.
//! - **On the CPU** (pixman), the share's buffer itself is lent to the
//!   streams' thread: it is not drawn into again until given back (a
//!   frame owed waits for that).
//!
//! A window shared on another desktop, or minimised, is told when its
//! frame was drawn, so that it keeps drawing.

use std::{collections::HashMap, time::Duration};

use hocus_link::ShareSource;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
            gles::{GlesRenderer, GlesTexture},
            pixman::PixmanRenderer,
        },
    },
    reexports::pixman,
    utils::{Buffer as BufferCoords, Physical, Size, Transform},
};

use super::{
    glread,
    lease::Lease,
    stream::{StreamsSender, ToStreams},
    thumbs::Job,
};
use crate::{
    State,
    going::Shaders,
    model::WindowId,
    render::{BACKGROUND, OutputElement, OutputElements, Purpose},
    wobble::Bend,
};

/// What came of handing a frame over.
#[derive(Debug)]
pub enum Handed {
    /// The read-back thread has it.
    Sent,
    /// The share's buffer is lent, until this lease is back.
    Lent(Lease),
    /// Nothing free to read it into: owed.
    Busy,
}

/// How a renderer hands a frame over without the loop copying it.
pub trait Lends<T>: Renderer + ExportMem {
    /// What it keeps for that (the GPU's pixel buffers and thread).
    type Reader: Default + std::fmt::Debug;

    /// Share `id`'s frame, just drawn into `buffer` (`size` pixels), to
    /// the stream `to`.
    fn hand_over(
        &mut self,
        reader: &mut Self::Reader,
        id: u64,
        buffer: &T,
        size: (u32, u32),
        to: &StreamsSender,
    ) -> anyhow::Result<Handed>;

    /// What it keeps for the shares that `keep` says are gone goes.
    fn forget(&mut self, reader: &mut Self::Reader, keep: &dyn Fn(u64) -> bool);
}

impl Lends<GlesTexture> for GlesRenderer {
    type Reader = glread::Reader;

    fn hand_over(
        &mut self,
        reader: &mut Self::Reader,
        id: u64,
        buffer: &GlesTexture,
        size: (u32, u32),
        to: &StreamsSender,
    ) -> anyhow::Result<Handed> {
        Ok(match reader.read(self, id, buffer, size, to)? {
            glread::Asked::Sent => Handed::Sent,
            glread::Asked::Busy => Handed::Busy,
        })
    }

    fn forget(&mut self, reader: &mut Self::Reader, keep: &dyn Fn(u64) -> bool) {
        reader.forget(self, keep);
    }
}

impl Lends<pixman::Image<'static, 'static>> for PixmanRenderer {
    type Reader = ();

    fn hand_over(
        &mut self,
        _reader: &mut (),
        id: u64,
        buffer: &pixman::Image<'static, 'static>,
        size: (u32, u32),
        to: &StreamsSender,
    ) -> anyhow::Result<Handed> {
        let len = buffer.stride() * buffer.height();
        // SAFETY: pixman's image owns `stride × height` bytes at `data`
        // (pixman's own pointer, read here, never kept apart from it),
        // alive as long as `buffer`; the renderer drew into it
        // synchronously, and the loop neither writes nor frees it while
        // the lease made of the slice is out (`Target::lent`).
        #[allow(unsafe_code)]
        let bytes = unsafe {
            let data = buffer.data();
            anyhow::ensure!(!data.is_null() && len > 0, "no pixels to lend");
            std::slice::from_raw_parts(data as *const u8, len)
        };
        let lease = Lease::new(bytes);
        to.send(ToStreams::Frame {
            id,
            lease: lease.clone(),
            size,
            stride: u32::try_from(buffer.stride()).unwrap_or(u32::MAX),
        });
        Ok(Handed::Lent(lease))
    }

    fn forget(&mut self, _reader: &mut (), _keep: &dyn Fn(u64) -> bool) {}
}

/// A share's buffer, what it last drew, and the lease of its pixels while
/// they are lent (the CPU's).
struct Target<T> {
    buffer: T,
    damage: OutputDamageTracker,
    size: Size<i32, Physical>,
    lent: Option<Lease>,
}

/// The backend's part of sharing: each share's buffer, and what the
/// renderer keeps to hand frames over.
pub struct Frames<R: Lends<T>, T> {
    targets: HashMap<u64, Target<T>>,
    /// Buffers of shares gone (or of another size), still lent.
    retired: Vec<Target<T>>,
    reader: R::Reader,
}

impl<R: Lends<T>, T> Default for Frames<R, T> {
    fn default() -> Self {
        Self {
            targets: HashMap::new(),
            retired: Vec::new(),
            reader: R::Reader::default(),
        }
    }
}

impl<R: Lends<T>, T> Drop for Frames<R, T> {
    /// The memory lent goes with them: taken back first.
    fn drop(&mut self) {
        for target in self.targets.values().chain(&self.retired) {
            if let Some(lease) = &target.lent {
                lease.revoke();
            }
        }
    }
}

impl<R: Lends<T>, T> std::fmt::Debug for Frames<R, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frames")
            .field("targets", &self.targets.len())
            .field("retired", &self.retired.len())
            .field("reader", &self.reader)
            .finish()
    }
}

impl<R: Lends<T>, T> Frames<R, T> {
    /// What was given back may be drawn into again, or freed.
    fn settle(
        &mut self,
        renderer: &mut R,
        shares: &std::collections::BTreeMap<u64, super::Active>,
    ) {
        self.retired
            .retain(|t| t.lent.as_ref().is_some_and(|l| !l.is_back()));
        for target in self.targets.values_mut() {
            if target.lent.as_ref().is_some_and(Lease::is_back) {
                target.lent = None;
            }
        }
        let gone: Vec<u64> = self
            .targets
            .keys()
            .copied()
            .filter(|id| !shares.contains_key(id))
            .collect();
        for id in gone {
            if let Some(target) = self.targets.remove(&id)
                && target.lent.is_some()
            {
                self.retired.push(target);
            }
        }
        renderer.forget(&mut self.reader, &|id| shares.contains_key(&id));
    }
}

/// What came of a share's frame.
enum Drawn {
    /// Handed over.
    Went,
    /// Nothing changed.
    Same,
    /// Owed: nothing free to hand it over with.
    Owed,
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
        + Lends<T>
        + Bend
        + Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    frames.settle(renderer, &state.share.shares);
    thumbnails(renderer, state);
    if state.share.shares.is_empty() {
        return;
    }
    let Some(to) = state.share.streams.as_ref().map(|s| s.sender().clone()) else {
        return;
    };
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
        // Its buffer lent still (the CPU's): drawn once it is back.
        if frames.targets.get(&id).is_some_and(|t| t.lent.is_some()) {
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
        match frame::<R, T>(
            renderer, state, frames, &to, id, &source, pointer, agreed, fresh,
        ) {
            Ok(Drawn::Went) => {
                if let Some(share) = state.share.shares.get_mut(&id) {
                    share.last = Some(now);
                    share.fresh = false;
                }
            }
            Ok(Drawn::Same) => {}
            // Drawn already: sent once something is free (`Returned`).
            Ok(Drawn::Owed) => {
                if let Some(share) = state.share.shares.get_mut(&id) {
                    share.fresh = true;
                }
            }
            // Once, and the share ends: it would fail at every frame.
            Err(err) => state.share_failed(id, &format!("{err:#}")),
        }
        if let ShareSource::Window(w) = source {
            window_drawn(state, WindowId(w));
        }
    }
}

/// Share `id`'s frame drawn into its buffer, `agreed` pixels large, and
/// handed over to `to` if something changed (or `fresh`: a frame is owed).
#[allow(clippy::too_many_arguments)]
fn frame<R, T>(
    renderer: &mut R,
    state: &mut State,
    frames: &mut Frames<R, T>,
    to: &StreamsSender,
    id: u64,
    source: &ShareSource,
    pointer: bool,
    agreed: (u32, u32),
    fresh: bool,
) -> anyhow::Result<Drawn>
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
    let Frames {
        targets,
        retired,
        reader,
    } = frames;
    if targets.get(&id).is_none_or(|t| t.size != size) {
        let buffer_size: Size<i32, BufferCoords> = (size.w, size.h).into();
        // B, G, R, A in memory: what the CPU's buffer is lent as.
        let buffer = renderer
            .create_buffer(Fourcc::Argb8888, buffer_size)
            .or_else(|_| renderer.create_buffer(Fourcc::Abgr8888, buffer_size))
            .context("no buffer to draw the share into")?;
        if let Some(old) = targets.insert(
            id,
            Target {
                buffer,
                damage: OutputDamageTracker::new(size, scale, Transform::Normal),
                size,
                lent: None,
            },
        ) && old.lent.is_some()
        {
            retired.push(old);
        }
    }
    let target = targets.get_mut(&id).context("the share's buffer")?;
    let elements: &[OutputElement<R>] = &elements;
    {
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
            return Ok(Drawn::Same);
        }
    }
    let since = std::time::Instant::now();
    let handed = renderer.hand_over(reader, id, &target.buffer, agreed, to)?;
    let drawn = match handed {
        Handed::Sent => Drawn::Went,
        Handed::Lent(lease) => {
            target.lent = Some(lease);
            Drawn::Went
        }
        Handed::Busy => Drawn::Owed,
    };
    if matches!(drawn, Drawn::Went) {
        state.frame_handed(since);
    }
    Ok(drawn)
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
