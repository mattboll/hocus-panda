//! A headless compositor driven from the same process, on a virtual clock:
//! what scenario tests run on. Frames happen only when the harness advances
//! time, so what a test sees does not depend on the machine's speed.

use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

use hocus_api::{ClientError, Event, Permission, Reply, Request, RequestLine, ServerLine};
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
    /// The harness's own control connection, opened on first use.
    control: Option<Control>,
}

/// A control connection read without blocking, so that the harness can run
/// the compositor while it waits for a reply.
#[derive(Debug)]
struct Control {
    stream: UnixStream,
    input: Vec<u8>,
    next_id: u64,
    events: VecDeque<Event>,
}

impl Control {
    fn connect(path: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            input: Vec::new(),
            next_id: 1,
            events: VecDeque::new(),
        })
    }

    /// The next complete line, if one arrived.
    fn line(&mut self) -> Result<Option<ServerLine>, ClientError> {
        loop {
            if let Some(end) = self.input.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.input.drain(..=end).collect();
                return serde_json::from_slice(&line).map(Some).map_err(|_| {
                    ClientError::Protocol(String::from_utf8_lossy(&line).trim_end().into())
                });
            }
            let mut buf = [0u8; 64 * 1024];
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(ClientError::Closed),
                Ok(n) => self.input.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err.into()),
            }
        }
    }
}

/// How long the harness waits for a reply to its own request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

impl Headless {
    /// Starts with the test permission on the control socket.
    pub fn start(config: HeadlessConfig) -> anyhow::Result<Self> {
        Self::start_with(config, Permission::Test)
    }

    pub fn start_with(config: HeadlessConfig, api_permission: Permission) -> anyhow::Result<Self> {
        let options = Options {
            backend: Backend::Headless(config),
            runtime_dir: None,
            spawn: Vec::new(),
            api_permission,
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
            control: None,
        })
    }

    /// The Wayland socket, to connect clients with `WAYLAND_DISPLAY`.
    pub fn socket_path(&self) -> &Path {
        &self.app.state.socket_path
    }

    /// The control socket.
    pub fn api_socket(&self) -> &Path {
        self.app.state.api.path()
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

    /// Sends `request` over the control socket, as any client would, and
    /// runs the compositor until the reply arrives. The virtual clock does not
    /// move.
    pub fn request(&mut self, request: Request) -> Result<Reply, ClientError> {
        let mut control = match self.control.take() {
            Some(control) => control,
            None => Control::connect(self.api_socket())?,
        };
        let result = self.exchange(&mut control, request);
        self.control = Some(control);
        result
    }

    /// Events received since the last call; [`Request::Subscribe`] first.
    pub fn take_events(&mut self) -> Vec<Event> {
        self.control
            .as_mut()
            .map(|c| c.events.drain(..).collect())
            .unwrap_or_default()
    }

    fn exchange(&mut self, control: &mut Control, request: Request) -> Result<Reply, ClientError> {
        let id = control.next_id;
        control.next_id += 1;
        let mut line = serde_json::to_vec(&RequestLine { id, request })
            .map_err(|err| ClientError::Protocol(err.to_string()))?;
        line.push(b'\n');
        // A request line is far smaller than the socket's buffer.
        control.stream.write_all(&line)?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            while let Some(line) = control.line()? {
                match line {
                    ServerLine::Reply { id: got, ok } if got == id => return Ok(ok),
                    ServerLine::Error {
                        id: Some(got),
                        error,
                    } if got == id => return Err(ClientError::Api(error)),
                    ServerLine::Error { id: None, error } => return Err(ClientError::Api(error)),
                    ServerLine::Event { event } => control.events.push_back(event),
                    ServerLine::Reply { .. } | ServerLine::Error { .. } => {}
                }
            }
            if Instant::now() > deadline {
                return Err(io::Error::from(io::ErrorKind::TimedOut).into());
            }
            self.dispatch(Duration::from_millis(5))
                .map_err(|err| ClientError::Io(io::Error::other(format!("{err:#}"))))?;
        }
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
