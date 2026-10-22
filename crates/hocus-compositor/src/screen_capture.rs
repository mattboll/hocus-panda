//! Screen capture for screenshot tools and screen sharing (#114):
//! ext-image-copy-capture with output and window sources, and
//! wlr-screencopy for older tools. Requests are kept on the loop and served
//! after the dispatch, where the renderer is: the output (or window) is
//! drawn again offscreen and copied into the client's shared-memory buffer.
//! A capture that waits for damage is served once the output has drawn a
//! new frame. Sandboxed clients do not see any of it.
//!
//! While the screen is shared (a session that captured more than once, a
//! damage-driven copy waiting, or one in the last two seconds), toasts are
//! held and summarised afterwards.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use smithay::{
    output::Output,
    reexports::{
        wayland_protocols::ext::{
            image_capture_source::v1::server::{
                ext_foreign_toplevel_image_capture_source_manager_v1::{
                    self as toplevel_sources, ExtForeignToplevelImageCaptureSourceManagerV1,
                },
                ext_image_capture_source_v1::{self as source, ExtImageCaptureSourceV1},
                ext_output_image_capture_source_manager_v1::{
                    self as output_sources, ExtOutputImageCaptureSourceManagerV1,
                },
            },
            image_copy_capture::v1::server::{
                ext_image_copy_capture_cursor_session_v1::{
                    self as cursor_session, ExtImageCopyCaptureCursorSessionV1,
                },
                ext_image_copy_capture_frame_v1::{
                    self as frame, ExtImageCopyCaptureFrameV1, FailureReason,
                },
                ext_image_copy_capture_manager_v1::{
                    self as manager, ExtImageCopyCaptureManagerV1,
                },
                ext_image_copy_capture_session_v1::{
                    self as session, ExtImageCopyCaptureSessionV1,
                },
            },
        },
        wayland_protocols_wlr::screencopy::v1::server::{
            zwlr_screencopy_frame_v1::{self as wlr_frame, ZwlrScreencopyFrameV1},
            zwlr_screencopy_manager_v1::{self as wlr_manager, ZwlrScreencopyManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            protocol::{wl_buffer::WlBuffer, wl_output::Transform, wl_shm},
        },
    },
    utils::{Physical, Rectangle},
    wayland::foreign_toplevel_list::ForeignToplevelHandle,
};

use crate::{App, State, capture::Pixels, handlers::protocols::trusted};

/// How long damage-driven copies count as sharing after the last one.
const SHARING_LINGER: Duration = Duration::from_secs(2);

/// What is captured.
#[derive(Debug, Clone)]
enum Target {
    Output(Output),
    /// A window, by its foreign-toplevel identifier.
    Window(String),
    /// A source whose output or window is gone.
    Gone,
}

/// Where the pixels go once copied.
#[derive(Debug)]
enum Reply {
    Ext(ExtImageCopyCaptureFrameV1),
    Wlr(ZwlrScreencopyFrameV1, bool),
}

#[derive(Debug)]
struct Pending {
    target: Target,
    /// Physical, within the target.
    region: Option<Rectangle<i32, Physical>>,
    cursor: bool,
    buffer: WlBuffer,
    /// Waits until this output has drawn past this frame.
    after: Option<(String, u64)>,
    reply: Reply,
}

#[derive(Debug, Default)]
pub struct ScreenCapture {
    pending: Vec<Pending>,
    /// Frames drawn per output, for captures that wait for damage.
    frames: HashMap<String, u64>,
    /// Sessions that captured more than once: sharing.
    sharing_sessions: usize,
    last_damage_copy: Option<Duration>,
    /// Notifications held while sharing.
    pub held: usize,
    pub was_sharing: bool,
}

/// An ext source's target.
#[derive(Debug)]
struct SourceData(Target);

#[derive(Debug)]
struct SessionData {
    target: Target,
    cursor: bool,
    /// The output frame of its last capture.
    last: Mutex<Option<u64>>,
    captures: Mutex<u32>,
}

#[derive(Debug)]
struct FrameData {
    session: ExtImageCopyCaptureSessionV1,
    buffer: Mutex<Option<WlBuffer>>,
}

#[derive(Debug)]
struct WlrFrameData {
    /// `None` when the output was already gone.
    output: Option<Output>,
    region: Option<Rectangle<i32, Physical>>,
    cursor: bool,
    used: Mutex<bool>,
}

/// The globals; sandboxed clients do not see them (`can_view`).
pub fn init(dh: &DisplayHandle) {
    dh.create_global::<State, ExtOutputImageCaptureSourceManagerV1, _>(1, ());
    dh.create_global::<State, ExtForeignToplevelImageCaptureSourceManagerV1, _>(1, ());
    dh.create_global::<State, ExtImageCopyCaptureManagerV1, _>(1, ());
    dh.create_global::<State, ZwlrScreencopyManagerV1, _>(3, ());
}

/// The size of a target's buffer, physical pixels.
fn size_of(state: &State, target: &Target) -> Option<(i32, i32)> {
    match target {
        Target::Output(output) => output.current_mode().map(|m| (m.size.w, m.size.h)),
        Target::Window(id) => {
            let managed = state
                .windows
                .values()
                .find(|m| m.foreign.identifier() == *id)?;
            let scale = state
                .output_of(&managed.window)
                .map_or(1.0, |o| o.current_scale().fractional_scale());
            let size = managed
                .window
                .geometry()
                .size
                .to_f64()
                .to_physical(scale)
                .to_i32_round();
            Some((size.w, size.h))
        }
        Target::Gone => None,
    }
}

impl State {
    /// An output drew a frame: captures waiting for damage may go.
    pub fn capture_frame_drawn(&mut self, output: &Output) {
        *self.screen_capture.frames.entry(output.name()).or_default() += 1;
    }

    fn output_frame(&self, output: &Output) -> u64 {
        self.screen_capture
            .frames
            .get(&output.name())
            .copied()
            .unwrap_or_default()
    }

    /// Whether the screen is being shared now.
    pub fn sharing(&self) -> bool {
        let now = self.clock.now();
        // A tool waiting for the screen to change still shares it.
        let waiting = self
            .screen_capture
            .pending
            .iter()
            .any(|p| matches!(p.reply, Reply::Wlr(_, true)));
        waiting
            || self.screen_capture.sharing_sessions > 0
            || self
                .screen_capture
                .last_damage_copy
                .is_some_and(|t| now.saturating_sub(t) < SHARING_LINGER)
    }

    pub fn has_pending_captures(&self) -> bool {
        !self.screen_capture.pending.is_empty()
    }
}

impl App {
    /// After a dispatch: copies what can be copied now.
    pub fn serve_captures(&mut self) {
        if self.state.screen_capture.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.state.screen_capture.pending);
        let mut waiting = Vec::new();
        for p in pending {
            let ready = p.after.as_ref().is_none_or(|(name, n)| {
                self.state
                    .space
                    .outputs()
                    .find(|o| o.name() == *name)
                    .is_none_or(|o| self.state.output_frame(o) > *n)
            });
            if !ready {
                waiting.push(p);
                continue;
            }
            let pixels = self.capture_target(&p.target, p.cursor);
            let now = self.state.clock.now();
            match pixels.and_then(|px| copy_into(&p.buffer, &px, p.region)) {
                Ok((w, h)) => match p.reply {
                    Reply::Ext(frame) => {
                        frame.transform(Transform::Normal);
                        frame.damage(0, 0, w, h);
                        let (hi, lo, ns) = timestamp(now);
                        frame.presentation_time(hi, lo, ns);
                        frame.ready();
                    }
                    Reply::Wlr(frame, damage) => {
                        frame.flags(wlr_frame::Flags::empty());
                        if damage && frame.version() >= 2 {
                            frame.damage(0, 0, w as u32, h as u32);
                        }
                        let (hi, lo, ns) = timestamp(now);
                        frame.ready(hi, lo, ns);
                    }
                },
                Err(err) => {
                    tracing::debug!(%err, "a capture failed");
                    match p.reply {
                        Reply::Ext(frame) => frame.failed(match err {
                            CopyError::Buffer => FailureReason::BufferConstraints,
                            CopyError::Gone => FailureReason::Stopped,
                            CopyError::Render => FailureReason::Unknown,
                        }),
                        Reply::Wlr(frame, _) => frame.failed(),
                    }
                }
            }
        }
        self.state.screen_capture.pending.extend(waiting);
    }

    fn capture_target(&mut self, target: &Target, cursor: bool) -> Result<Pixels, CopyError> {
        use crate::BackendState;
        match target {
            Target::Output(output) => match &mut self.backend {
                BackendState::Winit(winit) => winit.capture_output(output, &self.state, cursor),
                BackendState::Headless(headless) => {
                    headless.capture_output(output, &self.state, cursor)
                }
            }
            .map_err(|_| CopyError::Render),
            Target::Window(id) => {
                let window = self
                    .state
                    .windows
                    .values()
                    .find(|m| m.foreign.identifier() == *id)
                    .map(|m| m.window.clone())
                    .ok_or(CopyError::Gone)?;
                let scale = self
                    .state
                    .output_of(&window)
                    .map_or(1.0, |o| o.current_scale().fractional_scale());
                match &mut self.backend {
                    BackendState::Winit(winit) => winit.capture_window(&window, scale),
                    BackendState::Headless(headless) => headless.capture_window(&window, scale),
                }
                .map_err(|_| CopyError::Render)
            }
            Target::Gone => Err(CopyError::Gone),
        }
    }
}

#[derive(Debug)]
enum CopyError {
    /// Not a shared-memory buffer of the size and format offered.
    Buffer,
    Gone,
    Render,
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buffer => write!(f, "the buffer is not the one offered"),
            Self::Gone => write!(f, "what was captured is gone"),
            Self::Render => write!(f, "it could not be drawn"),
        }
    }
}

/// `tv_sec` high and low, and `tv_nsec`.
fn timestamp(t: Duration) -> (u32, u32, u32) {
    let secs = t.as_secs();
    ((secs >> 32) as u32, secs as u32, t.subsec_nanos())
}

/// Copies `pixels` (cut to `region`) into `buffer`: XRGB8888 or ARGB8888,
/// little endian. The size copied.
fn copy_into(
    buffer: &WlBuffer,
    pixels: &Pixels,
    region: Option<Rectangle<i32, Physical>>,
) -> Result<(i32, i32), CopyError> {
    let region = region.unwrap_or_else(|| {
        Rectangle::from_size((pixels.width as i32, pixels.height as i32).into())
    });
    let (w, h) = (region.size.w, region.size.h);
    let result = smithay::wayland::shm::with_buffer_contents_mut(buffer, |ptr, len, data| {
        let format_ok = matches!(
            data.format,
            wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888
        );
        let stride = data.stride as usize;
        let needed = data.offset as usize + stride * h as usize;
        if !format_ok
            || data.width != w
            || data.height != h
            || stride < 4 * w as usize
            || len < needed
        {
            return false;
        }
        // SAFETY: smithay hands us the client's shared-memory pool mapped
        // for `len` bytes and keeps it mapped during this call; the slice
        // does not outlive it and nothing else writes to it meanwhile.
        #[allow(unsafe_code)]
        let memory = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
        let src_stride = pixels.width as usize * 4;
        for row in 0..h as usize {
            let sy = region.loc.y as usize + row;
            let src = &pixels.rgba[sy * src_stride + region.loc.x as usize * 4..][..w as usize * 4];
            let dst = &mut memory[data.offset as usize + row * stride..][..w as usize * 4];
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
            }
        }
        true
    });
    match result {
        Ok(true) => Ok((w, h)),
        _ => Err(CopyError::Buffer),
    }
}

// ext-image-capture-source: what to capture.

impl GlobalDispatch<ExtOutputImageCaptureSourceManagerV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtOutputImageCaptureSourceManagerV1>,
        _: &(),
        data: &mut DataInit<'_, Self>,
    ) {
        data.init(resource, ());
    }

    fn can_view(client: Client, _: &()) -> bool {
        trusted(&client)
    }
}

impl Dispatch<ExtOutputImageCaptureSourceManagerV1, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtOutputImageCaptureSourceManagerV1,
        request: output_sources::Request,
        _: &(),
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        if let output_sources::Request::CreateSource { source, output } = request {
            let target = Output::from_resource(&output).map_or(Target::Gone, Target::Output);
            data.init(source, SourceData(target));
        }
    }
}

impl GlobalDispatch<ExtForeignToplevelImageCaptureSourceManagerV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtForeignToplevelImageCaptureSourceManagerV1>,
        _: &(),
        data: &mut DataInit<'_, Self>,
    ) {
        data.init(resource, ());
    }

    fn can_view(client: Client, _: &()) -> bool {
        trusted(&client)
    }
}

impl Dispatch<ExtForeignToplevelImageCaptureSourceManagerV1, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtForeignToplevelImageCaptureSourceManagerV1,
        request: toplevel_sources::Request,
        _: &(),
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        if let toplevel_sources::Request::CreateSource {
            source,
            toplevel_handle,
        } = request
        {
            let target = ForeignToplevelHandle::from_resource(&toplevel_handle)
                .map_or(Target::Gone, |h| Target::Window(h.identifier()));
            data.init(source, SourceData(target));
        }
    }
}

impl Dispatch<ExtImageCaptureSourceV1, SourceData> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtImageCaptureSourceV1,
        _: source::Request,
        _: &SourceData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

// ext-image-copy-capture: sessions and frames.

impl GlobalDispatch<ExtImageCopyCaptureManagerV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtImageCopyCaptureManagerV1>,
        _: &(),
        data: &mut DataInit<'_, Self>,
    ) {
        data.init(resource, ());
    }

    fn can_view(client: Client, _: &()) -> bool {
        trusted(&client)
    }
}

impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ExtImageCopyCaptureManagerV1,
        request: manager::Request,
        _: &(),
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        match request {
            manager::Request::CreateSession {
                session,
                source,
                options,
            } => {
                let target = source
                    .data::<SourceData>()
                    .map_or(Target::Gone, |s| s.0.clone());
                let cursor = options
                    .into_result()
                    .is_ok_and(|o| o.contains(manager::Options::PaintCursors));
                let size = size_of(state, &target);
                let session = data.init(
                    session,
                    SessionData {
                        target,
                        cursor,
                        last: Mutex::new(None),
                        captures: Mutex::new(0),
                    },
                );
                match size {
                    Some((w, h)) => {
                        session.buffer_size(w as u32, h as u32);
                        session.shm_format(wl_shm::Format::Xrgb8888);
                        session.shm_format(wl_shm::Format::Argb8888);
                        session.done();
                    }
                    None => session.stopped(),
                }
            }
            // Cursor images separately are not offered: the session stops.
            manager::Request::CreatePointerCursorSession { session, .. } => {
                data.init(session, ());
            }
            manager::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureCursorSessionV1, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtImageCopyCaptureCursorSessionV1,
        request: cursor_session::Request,
        _: &(),
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        if let cursor_session::Request::GetCaptureSession { session } = request {
            let session = data.init(
                session,
                SessionData {
                    target: Target::Gone,
                    cursor: false,
                    last: Mutex::new(None),
                    captures: Mutex::new(0),
                },
            );
            session.stopped();
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, SessionData> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        session: &ExtImageCopyCaptureSessionV1,
        request: session::Request,
        _: &SessionData,
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        if let session::Request::CreateFrame { frame } = request {
            data.init(
                frame,
                FrameData {
                    session: session.clone(),
                    buffer: Mutex::new(None),
                },
            );
        }
    }

    fn destroyed(
        state: &mut Self,
        _: smithay::reexports::wayland_server::backend::ClientId,
        _: &ExtImageCopyCaptureSessionV1,
        data: &SessionData,
    ) {
        if data.captures.lock().map(|c| *c > 1).unwrap_or(false) {
            state.screen_capture.sharing_sessions =
                state.screen_capture.sharing_sessions.saturating_sub(1);
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, FrameData> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        frame: &ExtImageCopyCaptureFrameV1,
        request: frame::Request,
        data: &FrameData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            frame::Request::AttachBuffer { buffer } => {
                if let Ok(mut slot) = data.buffer.lock() {
                    *slot = Some(buffer);
                }
            }
            frame::Request::DamageBuffer { .. } => {}
            frame::Request::Capture => {
                let Some(buffer) = data.buffer.lock().ok().and_then(|mut b| b.take()) else {
                    frame.post_error(frame::Error::NoBuffer, "no buffer attached");
                    return;
                };
                let Some(session) = data.session.data::<SessionData>() else {
                    frame.failed(FailureReason::Stopped);
                    return;
                };
                // The first frame now, the next ones once something
                // changed.
                let after = match &session.target {
                    Target::Output(output) => {
                        let last = session.last.lock().ok().and_then(|l| *l);
                        let now = state.output_frame(output);
                        if let Ok(mut l) = session.last.lock() {
                            *l = Some(now);
                        }
                        last.filter(|l| *l >= now).map(|l| (output.name(), l))
                    }
                    _ => None,
                };
                if let Ok(mut captures) = session.captures.lock() {
                    *captures += 1;
                    if *captures == 2 {
                        state.screen_capture.sharing_sessions += 1;
                    }
                }
                state.screen_capture.pending.push(Pending {
                    target: session.target.clone(),
                    region: None,
                    cursor: session.cursor,
                    buffer,
                    after,
                    reply: Reply::Ext(frame.clone()),
                });
            }
            frame::Request::Destroy => {}
            _ => {}
        }
    }
}

// wlr-screencopy.

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _: &(),
        data: &mut DataInit<'_, Self>,
    ) {
        data.init(resource, ());
    }

    fn can_view(client: Client, _: &()) -> bool {
        trusted(&client)
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ZwlrScreencopyManagerV1,
        request: wlr_manager::Request,
        _: &(),
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        let (frame, cursor, output, region) = match request {
            wlr_manager::Request::CaptureOutput {
                frame,
                overlay_cursor,
                output,
            } => (frame, overlay_cursor != 0, output, None),
            wlr_manager::Request::CaptureOutputRegion {
                frame,
                overlay_cursor,
                output,
                x,
                y,
                width,
                height,
            } => (
                frame,
                overlay_cursor != 0,
                output,
                Some(Rectangle::<i32, smithay::utils::Logical>::new(
                    (x, y).into(),
                    (width, height).into(),
                )),
            ),
            _ => return,
        };
        let Some(output) = Output::from_resource(&output) else {
            let frame = data.init(
                frame,
                WlrFrameData {
                    output: None,
                    region: None,
                    cursor,
                    used: Mutex::new(true),
                },
            );
            frame.failed();
            return;
        };
        let Some(mode) = output.current_mode() else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        // A logical region of the output, in its pixels, within it.
        let region = region.and_then(|r| {
            r.to_f64()
                .to_physical(scale)
                .to_i32_round()
                .intersection(Rectangle::from_size(mode.size))
        });
        let (w, h) = region.map_or((mode.size.w, mode.size.h), |r| (r.size.w, r.size.h));
        let frame = data.init(
            frame,
            WlrFrameData {
                output: Some(output),
                region,
                cursor,
                used: Mutex::new(false),
            },
        );
        frame.buffer(wl_shm::Format::Xrgb8888, w as u32, h as u32, 4 * w as u32);
        if frame.version() >= 3 {
            frame.buffer_done();
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, WlrFrameData> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        frame: &ZwlrScreencopyFrameV1,
        request: wlr_frame::Request,
        data: &WlrFrameData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let (buffer, damage) = match request {
            wlr_frame::Request::Copy { buffer } => (buffer, false),
            wlr_frame::Request::CopyWithDamage { buffer } => (buffer, true),
            _ => return,
        };
        let used = data
            .used
            .lock()
            .map(|mut u| std::mem::replace(&mut *u, true))
            .unwrap_or(true);
        let Some(output) = &data.output else {
            frame.failed();
            return;
        };
        if used {
            frame.post_error(
                wlr_frame::Error::AlreadyUsed,
                "the frame was already copied",
            );
            return;
        }
        let after = damage.then(|| {
            state.screen_capture.last_damage_copy = Some(state.clock.now());
            (output.name(), state.output_frame(output))
        });
        state.screen_capture.pending.push(Pending {
            target: Target::Output(output.clone()),
            region: data.region,
            cursor: data.cursor,
            buffer,
            after,
            reply: Reply::Wlr(frame.clone(), damage),
        });
    }
}
