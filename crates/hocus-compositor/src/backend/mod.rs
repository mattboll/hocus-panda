//! Where the compositor draws and reads input from: a window of the
//! current session (winit), the real screens (tty), or offscreen buffers
//! for tests (headless).

pub mod clips;
pub mod flip;
pub mod headless;
pub mod tty;
pub mod winit;

use smithay::{
    backend::{
        allocator::format::FormatSet,
        egl::{EGLDevice, EGLDisplay},
    },
    output::{Output, Scale},
    reexports::calloop::LoopHandle,
    wayland::dmabuf::DmabufFeedbackBuilder,
};

use crate::{App, BackendKind, State};

#[derive(Debug)]
pub enum Backend {
    // A GLES renderer is large; there is one per compositor.
    Winit(Box<winit::WinitBackend>),
    Tty(Box<tty::TtyBackend>),
    Headless(headless::HeadlessBackend),
}

impl Backend {
    pub fn init(
        kind: &BackendKind,
        handle: &LoopHandle<'static, App>,
        state: &mut State,
    ) -> anyhow::Result<Self> {
        Ok(match kind {
            BackendKind::Winit => Self::Winit(Box::new(winit::init(handle, state)?)),
            BackendKind::Tty => Self::Tty(Box::new(tty::init(handle, state)?)),
            BackendKind::Headless(config) => Self::Headless(headless::init(config, handle, state)?),
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Winit(_) => "winit",
            Self::Tty(_) => "tty",
            Self::Headless(_) => "headless",
        }
    }

    /// After a turn of the loop: draws the outputs that changed.
    pub fn after_dispatch(&mut self, state: &mut State) {
        match self {
            Self::Winit(winit) => winit.after_dispatch(state),
            Self::Tty(tty) => tty.after_dispatch(state),
            Self::Headless(headless) => headless.after_dispatch(state),
        }
    }

    pub fn headless(&self) -> Option<&headless::HeadlessBackend> {
        match self {
            Self::Headless(headless) => Some(headless),
            _ => None,
        }
    }
}

/// An output's scale as smithay takes it: whole when it is, and otherwise
/// to the nearest 120th, which fractional-scale gives clients.
pub fn scale(scale: f64) -> Scale {
    let scale = (scale * 120.0).round() / 120.0;
    if scale.fract() == 0.0 {
        Scale::Integer(scale as i32)
    } else {
        Scale::Fractional(scale)
    }
}

/// The linux-dmabuf global, version 4 with feedback when the renderer's
/// device is known (`main_device`), version 3 otherwise.
pub fn dmabuf_global(state: &mut State, formats: FormatSet, main_device: Option<DevId>) {
    let display = state.display_handle.clone();
    let global = match main_device {
        Some(device) => match DmabufFeedbackBuilder::new(device, formats.clone()).build() {
            Ok(feedback) => state
                .dmabuf_state
                .create_global_with_default_feedback::<State>(&display, &feedback),
            Err(err) => {
                tracing::warn!(%err, "no dmabuf feedback; linux-dmabuf version 3");
                state.dmabuf_state.create_global::<State>(&display, formats)
            }
        },
        None => state.dmabuf_state.create_global::<State>(&display, formats),
    };
    state.dmabuf_global = Some(global);
}

/// The device number of an EGL display's render node, which dmabuf
/// feedback names as the main device.
pub fn render_node_of(display: &EGLDisplay) -> Option<DevId> {
    let device = EGLDevice::device_for_display(display).ok()?;
    let node = device.try_get_render_node().ok().flatten()?;
    Some(node.dev_id())
}

/// `dev_t`, as smithay's DRM nodes give it.
pub type DevId = smithay::reexports::rustix::fs::Dev;

/// Makes `output` known to clients and maps it in the space at `position`.
pub fn add_output(state: &mut State, output: &Output, position: (i32, i32)) {
    state.space.map_output(output, position);
    state.redraw = true;
}
