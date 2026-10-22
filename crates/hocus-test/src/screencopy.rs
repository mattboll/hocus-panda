//! A wlr-screencopy client: one region of an output into a buffer, or
//! copies again and again as a screen sharing tool does.

use std::os::{fd::AsFd, unix::fs::FileExt};

use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool},
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

#[derive(Debug, Default)]
struct Copier {
    buffer: Option<(u32, u32, u32)>,
    ready: bool,
    failed: bool,
}

/// The region's pixels as `[b, g, r, x]` rows, and its size.
pub fn region(
    socket: &std::path::Path,
    output_index: usize,
    (x, y, w, h): (i32, i32, i32, i32),
) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let conn = Connection::from_socket(std::os::unix::net::UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Copier>(&conn)?;
    let qh = queue.handle();
    let manager: ZwlrScreencopyManagerV1 = globals.bind(&qh, 1..=3, ())?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
    let outputs: Vec<wl_output::WlOutput> = globals
        .contents()
        .clone_list()
        .into_iter()
        .filter(|g| g.interface == "wl_output")
        .map(|g| globals.registry().bind(g.name, 1, &qh, ()))
        .collect();
    let output = outputs
        .get(output_index)
        .ok_or_else(|| anyhow::anyhow!("no output"))?;
    let frame = manager.capture_output_region(0, output, x, y, w, h, &qh, ());
    let mut copier = Copier::default();
    while copier.buffer.is_none() && !copier.failed {
        queue.blocking_dispatch(&mut copier)?;
    }
    let (bw, bh, stride) = copier
        .buffer
        .ok_or_else(|| anyhow::anyhow!("no buffer offered"))?;
    let size = (stride * bh) as usize;
    let fd = rustix::fs::memfd_create("capture", rustix::fs::MemfdFlags::CLOEXEC)?;
    rustix::fs::ftruncate(&fd, size as u64)?;
    let pool = shm.create_pool(fd.as_fd(), size as i32, &qh, ());
    let buffer = pool.create_buffer(
        0,
        bw as i32,
        bh as i32,
        stride as i32,
        wl_shm::Format::Xrgb8888,
        &qh,
        (),
    );
    frame.copy(&buffer);
    while !copier.ready && !copier.failed {
        queue.blocking_dispatch(&mut copier)?;
    }
    anyhow::ensure!(copier.ready, "the copy failed");
    let file = std::fs::File::from(fd);
    let mut bytes = vec![0; size];
    file.read_exact_at(&mut bytes, 0)?;
    Ok((bw, bh, bytes))
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for Copier {
    fn event(
        copier: &mut Self,
        _: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                width,
                height,
                stride,
                ..
            } => copier.buffer = Some((width, height, stride)),
            zwlr_screencopy_frame_v1::Event::Ready { .. } => copier.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => copier.failed = true,
            _ => {}
        }
    }
}

macro_rules! ignore {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Copier {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    ZwlrScreencopyManagerV1,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer,
    wl_output::WlOutput
);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Copier {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Shares the first output as a screen sharing tool does: damage-driven
/// copies until `stop` is set.
pub fn share(
    socket: &std::path::Path,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> anyhow::Result<()> {
    use std::sync::atomic::Ordering;
    let conn = Connection::from_socket(std::os::unix::net::UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Copier>(&conn)?;
    let qh = queue.handle();
    let manager: ZwlrScreencopyManagerV1 = globals.bind(&qh, 2..=3, ())?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
    let output: wl_output::WlOutput = globals
        .contents()
        .clone_list()
        .into_iter()
        .find(|g| g.interface == "wl_output")
        .map(|g| globals.registry().bind(g.name, 1, &qh, ()))
        .ok_or_else(|| anyhow::anyhow!("no output"))?;
    while !stop.load(Ordering::Relaxed) {
        let frame = manager.capture_output(0, &output, &qh, ());
        let mut copier = Copier::default();
        while copier.buffer.is_none() && !copier.failed {
            queue.blocking_dispatch(&mut copier)?;
        }
        let (w, h, stride) = copier
            .buffer
            .ok_or_else(|| anyhow::anyhow!("no buffer offered"))?;
        let size = (stride * h) as usize;
        let fd = rustix::fs::memfd_create("share", rustix::fs::MemfdFlags::CLOEXEC)?;
        rustix::fs::ftruncate(&fd, size as u64)?;
        let pool = shm.create_pool(fd.as_fd(), size as i32, &qh, ());
        let buffer = pool.create_buffer(
            0,
            w as i32,
            h as i32,
            stride as i32,
            wl_shm::Format::Xrgb8888,
            &qh,
            (),
        );
        frame.copy_with_damage(&buffer);
        conn.flush()?;
        // The compositor answers once the output changed; the test may
        // stop sharing meanwhile.
        while !copier.ready && !copier.failed && !stop.load(Ordering::Relaxed) {
            queue.dispatch_pending(&mut copier)?;
            conn.flush()?;
            if let Some(guard) = queue.prepare_read() {
                let fd = guard.connection_fd();
                let mut fds = [rustix::event::PollFd::new(
                    &fd,
                    rustix::event::PollFlags::IN,
                )];
                let timeout = rustix::event::Timespec {
                    tv_sec: 0,
                    tv_nsec: 10_000_000,
                };
                if rustix::event::poll(&mut fds, Some(&timeout))? > 0 {
                    guard.read()?;
                }
            }
        }
        frame.destroy();
        buffer.destroy();
        pool.destroy();
    }
    Ok(())
}
