//! A lock client: locks the session (ext-session-lock), shows a solid
//! surface on every output, and can unlock, or vanish without unlocking as
//! a crashed locker would.

use std::{
    os::unix::net::UnixStream,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread::JoinHandle,
};

use anyhow::Context;
use rustix::event::{PollFd, PollFlags, Timespec};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_registry, delegate_session_lock, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
};

/// What the lock client asks of its thread.
const RUN: u8 = 0;
const UNLOCK: u8 = 1;
const VANISH: u8 = 2;

#[derive(Debug)]
pub struct LockClient {
    locked: Arc<AtomicBool>,
    order: Arc<AtomicU8>,
    thread: JoinHandle<anyhow::Result<()>>,
}

impl LockClient {
    pub fn spawn(socket: &Path, argb: u32) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("could not connect to {}", socket.display()))?;
        let conn = Connection::from_socket(stream)?;
        let locked = Arc::new(AtomicBool::new(false));
        let order = Arc::new(AtomicU8::new(RUN));
        let thread = std::thread::Builder::new()
            .name("lock client".into())
            .spawn({
                let locked = locked.clone();
                let order = order.clone();
                move || run(conn, argb, locked, order)
            })?;
        Ok(Self {
            locked,
            order,
            thread,
        })
    }

    /// Whether the compositor said the session is locked.
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    pub fn unlock(&self) {
        self.order.store(UNLOCK, Ordering::SeqCst);
    }

    /// Goes away without unlocking.
    pub fn vanish(&self) {
        self.order.store(VANISH, Ordering::SeqCst);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }
}

fn run(
    conn: Connection,
    argb: u32,
    locked: Arc<AtomicBool>,
    order: Arc<AtomicU8>,
) -> anyhow::Result<()> {
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let mut client = Client {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).context("no wl_compositor")?,
        shm: Shm::bind(&globals, &qh).context("no wl_shm")?,
        pool: None,
        argb,
        lock: None,
        surfaces: Vec::new(),
        locked,
        finished: false,
    };
    let state = SessionLockState::new(&globals, &qh);
    client.lock = Some(state.lock(&qh).context("no session lock")?);
    loop {
        match order.load(Ordering::SeqCst) {
            UNLOCK => {
                if let Some(lock) = client.lock.take() {
                    lock.unlock();
                }
                queue.roundtrip(&mut client)?;
                return Ok(());
            }
            // Dropped connection, no unlock: what a crash looks like.
            VANISH => return Ok(()),
            _ => {}
        }
        if client.finished {
            return Ok(());
        }
        queue.flush()?;
        queue.dispatch_pending(&mut client)?;
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut fds = [PollFd::new(&fd, PollFlags::IN)];
            let timeout = Timespec {
                tv_sec: 0,
                tv_nsec: 10_000_000,
            };
            if rustix::event::poll(&mut fds, Some(&timeout))? > 0 {
                guard.read()?;
            }
        }
    }
}

struct Client {
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    shm: Shm,
    pool: Option<SlotPool>,
    argb: u32,
    lock: Option<SessionLock>,
    surfaces: Vec<SessionLockSurface>,
    locked: Arc<AtomicBool>,
    finished: bool,
}

impl SessionLockHandler for Client {
    fn locked(&mut self, _: &Connection, qh: &QueueHandle<Self>, lock: SessionLock) {
        self.locked.store(true, Ordering::SeqCst);
        let outputs: Vec<wl_output::WlOutput> = self.outputs.outputs().collect();
        for output in outputs {
            let surface = self.compositor.create_surface(qh);
            self.surfaces
                .push(lock.create_lock_surface(surface, &output, qh));
        }
    }

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        self.finished = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        let drawn = (|| -> anyhow::Result<()> {
            if self.pool.is_none() {
                self.pool = Some(SlotPool::new((w * h * 4) as usize, &self.shm)?);
            }
            let Some(pool) = self.pool.as_mut() else {
                return Ok(());
            };
            let (buffer, canvas) =
                pool.create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888)?;
            for pixel in canvas.as_chunks_mut::<4>().0 {
                *pixel = self.argb.to_le_bytes();
            }
            let s = surface.wl_surface();
            s.damage_buffer(0, 0, w as i32, h as i32);
            buffer.attach_to(s)?;
            s.commit();
            Ok(())
        })();
        if let Err(err) = drawn {
            eprintln!("lock client could not draw: {err:#}");
        }
    }
}

impl CompositorHandler for Client {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Client {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Client {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Client {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState];
}

delegate_compositor!(Client);
delegate_output!(Client);
delegate_shm!(Client);
delegate_session_lock!(Client);
delegate_registry!(Client);
