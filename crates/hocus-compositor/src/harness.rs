//! A headless compositor driven from the same process, on a virtual clock:
//! what scenario tests run on. Frames happen only when the harness advances
//! time, so what a test sees does not depend on the machine's speed.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use smithay::reexports::calloop::EventLoop;

use crate::{
    App, Backend, BackendState, Clock, HeadlessConfig, Options, RuntimeDir, Started,
    headless::{FRAME, HeadlessBackend, OutputInfo, RendererKind},
};

/// A headless compositor in a private runtime directory, on a virtual clock.
#[derive(Debug)]
pub struct Headless {
    event_loop: EventLoop<'static, App>,
    app: App,
    runtime_dir: RuntimeDir,
}

impl Headless {
    pub fn start(config: HeadlessConfig) -> anyhow::Result<Self> {
        let options = Options {
            backend: Backend::Headless(config),
            runtime_dir: None,
            spawn: Vec::new(),
        };
        let Started {
            event_loop,
            app,
            runtime_dir,
        } = crate::start(
            &options,
            Clock::Virtual {
                now: Duration::ZERO,
            },
        )?;
        Ok(Self {
            event_loop,
            app,
            runtime_dir,
        })
    }

    /// The Wayland socket, to connect clients with `WAYLAND_DISPLAY`.
    pub fn socket_path(&self) -> &Path {
        &self.app.state.socket_path
    }

    pub fn runtime_dir(&self) -> &Path {
        self.runtime_dir.path()
    }

    pub fn renderer(&self) -> Option<RendererKind> {
        self.backend().map(HeadlessBackend::renderer)
    }

    pub fn outputs(&self) -> Vec<OutputInfo> {
        self.backend()
            .map(HeadlessBackend::outputs)
            .unwrap_or_default()
    }

    /// Time on the virtual clock since the start.
    pub fn now(&self) -> Duration {
        self.app.state.clock.now()
    }

    /// Windows shown on the outputs.
    pub fn window_count(&self) -> usize {
        self.app.state.space.elements().count()
    }

    /// Handles what clients sent, waiting up to `timeout` of real time for
    /// something to arrive. The virtual clock does not move.
    pub fn dispatch(&mut self, timeout: Duration) -> anyhow::Result<()> {
        self.event_loop.dispatch(Some(timeout), &mut self.app)?;
        self.app.state.display_handle.flush_clients()?;
        Ok(())
    }

    /// Dispatches until `done` holds, for at most `timeout` of real time:
    /// clients run in their own threads or processes.
    pub fn dispatch_until(
        &mut self,
        timeout: Duration,
        mut done: impl FnMut(&Self) -> bool,
    ) -> anyhow::Result<()> {
        let deadline = Instant::now() + timeout;
        while !done(self) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                anyhow::bail!("condition not met after {timeout:?}");
            }
            self.dispatch(left.min(Duration::from_millis(10)))?;
        }
        Ok(())
    }

    /// Moves the virtual clock forward by `by`, drawing a frame at each frame
    /// boundary it crosses if something changed.
    pub fn advance(&mut self, by: Duration) -> anyhow::Result<()> {
        let end = self.now() + by;
        loop {
            let next = next_frame_after(self.now());
            if next > end {
                break;
            }
            self.dispatch(Duration::ZERO)?;
            self.app.state.clock.advance_to(next);
            let App { state, backend } = &mut self.app;
            if let BackendState::Headless(headless) = backend {
                headless.frame(state);
            }
            self.app.state.display_handle.flush_clients()?;
        }
        self.app.state.clock.advance_to(end);
        Ok(())
    }

    fn backend(&self) -> Option<&HeadlessBackend> {
        match &self.app.backend {
            BackendState::Headless(headless) => Some(headless),
            BackendState::Winit(_) => None,
        }
    }
}

/// Frames fall on multiples of [`FRAME`].
fn next_frame_after(now: Duration) -> Duration {
    let frames = now.as_nanos() / FRAME.as_nanos() + 1;
    FRAME * u32::try_from(frames).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_fall_on_the_frame_grid() {
        assert_eq!(next_frame_after(Duration::ZERO), FRAME);
        assert_eq!(next_frame_after(FRAME / 2), FRAME);
        assert_eq!(next_frame_after(FRAME), FRAME * 2);
    }
}
