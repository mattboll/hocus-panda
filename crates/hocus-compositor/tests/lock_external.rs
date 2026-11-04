//! ext-session-lock-v1 (7.1, docs/design/lock.md, "Another locker"): a
//! locker of the test's own (a client on a thread) locks the session with
//! its own surfaces, as swaylock does, and nothing goes round it: a second
//! locker is refused, an unlock from a client that does not hold the lock
//! opens nothing, and the locker's crash leaves the session locked under
//! the desktop's own lock (the tests' authenticator here, never PAM).

mod common;

use std::{os::fd::AsFd, path::Path, sync::mpsc, time::Duration};

use common::{Shows, WAIT, WINDOW_COLOUR, chord, open_showing};
use hocus_compositor::{
    Authenticator, Config, Headless, HeadlessConfig, LockState, RendererChoice,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_output::WlOutput,
        wl_registry::WlRegistry, wl_shm, wl_shm::WlShm, wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::ExtSessionLockManagerV1,
    ext_session_lock_surface_v1::{self, ExtSessionLockSurfaceV1},
    ext_session_lock_v1::{self, ExtSessionLockV1},
};

const KEY_ENTER: u32 = 28;
/// The locker's surfaces' colour, ARGB.
const LOCKER: u32 = 0xff12_8a4e;

/// What a locker hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Heard {
    Locked,
    Finished,
}

/// What the test asks of a locker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// `unlock_and_destroy`, held lock or not.
    Unlock,
    /// It ends without unlocking, as a crash would.
    Crash,
}

struct Locker {
    heard: mpsc::Receiver<Heard>,
    asks: mpsc::Sender<Ask>,
}

/// A locker on a thread: it locks and, with `surfaces`, draws one lock
/// surface on each screen in [`LOCKER`].
fn locker(socket: &Path, surfaces: bool) -> anyhow::Result<Locker> {
    let stream = std::os::unix::net::UnixStream::connect(socket)?;
    stream.set_nonblocking(true)?;
    let conn = Connection::from_socket(stream)?;
    let (said, heard) = mpsc::channel();
    let (asks, asked) = mpsc::channel();
    std::thread::Builder::new()
        .name("test locker".into())
        .spawn(move || {
            if let Err(err) = run(&conn, surfaces, &said, &asked) {
                eprintln!("test locker: {err:#}");
            }
        })?;
    Ok(Locker { heard, asks })
}

struct State {
    said: mpsc::Sender<Heard>,
    shm: WlShm,
    surfaces: Vec<(WlSurface, ExtSessionLockSurfaceV1)>,
    buffers: Vec<WlBuffer>,
    files: Vec<std::fs::File>,
}

fn run(
    conn: &Connection,
    with_surfaces: bool,
    said: &mpsc::Sender<Heard>,
    asked: &mpsc::Receiver<Ask>,
) -> anyhow::Result<()> {
    let (globals, mut queue) = registry_queue_init::<State>(conn)?;
    let qh = queue.handle();
    let compositor: WlCompositor = globals.bind(&qh, 4..=6, ())?;
    let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
    let manager: ExtSessionLockManagerV1 = globals.bind(&qh, 1..=1, ())?;
    let outputs: Vec<WlOutput> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|g| g.interface == "wl_output")
            .map(|g| {
                globals
                    .registry()
                    .bind::<WlOutput, _, _>(g.name, 4, &qh, ())
            })
            .collect()
    });
    let mut state = State {
        said: said.clone(),
        shm,
        surfaces: Vec::new(),
        buffers: Vec::new(),
        files: Vec::new(),
    };
    let lock = manager.lock(&qh, ());
    if with_surfaces {
        for output in &outputs {
            let surface = compositor.create_surface(&qh, ());
            let lock_surface = lock.get_lock_surface(&surface, output, &qh, ());
            state.surfaces.push((surface, lock_surface));
        }
    }
    loop {
        queue.dispatch_pending(&mut state)?;
        while let Ok(ask) = asked.try_recv() {
            match ask {
                Ask::Unlock => lock.unlock_and_destroy(),
                Ask::Crash => return Ok(()),
            }
        }
        if let Err(err) = queue.flush() {
            // A protocol error ends it, as it ends a real locker.
            return Err(err.into());
        }
        if let Some(guard) = queue.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(err))
                    if err.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! ignore {
    ($($ty:ty),*) => {$(
        impl Dispatch<$ty, ()> for State {
            fn event(
                _: &mut Self,
                _: &$ty,
                _: <$ty as wayland_client::Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}
ignore!(
    WlCompositor,
    WlShm,
    WlShmPool,
    WlBuffer,
    WlSurface,
    WlOutput,
    ExtSessionLockManagerV1
);

impl Dispatch<ExtSessionLockV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let heard = match event {
            ext_session_lock_v1::Event::Locked => Heard::Locked,
            ext_session_lock_v1::Event::Finished => Heard::Finished,
            _ => return,
        };
        let _ = state.said.send(heard);
    }
}

impl Dispatch<ExtSessionLockSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        lock_surface: &ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        else {
            return;
        };
        lock_surface.ack_configure(serial);
        let Some(surface) = state
            .surfaces
            .iter()
            .find(|(_, l)| l == lock_surface)
            .map(|(s, _)| s.clone())
        else {
            return;
        };
        // A buffer of its colour, from a file of its own.
        let (w, h) = (width.max(1) as i32, height.max(1) as i32);
        let Ok(mut file) = tempfile::tempfile() else {
            return;
        };
        let pixels: Vec<u8> = (0..w * h).flat_map(|_| LOCKER.to_le_bytes()).collect();
        if std::io::Write::write_all(&mut file, &pixels).is_err() {
            return;
        }
        let pool = state.shm.create_pool(file.as_fd(), w * h * 4, qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, w, h);
        surface.commit();
        state.buffers.push(buffer);
        state.files.push(file);
    }
}

fn compositor() -> anyhow::Result<Headless> {
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    config.plain_background = true;
    config.lock_auth = Authenticator::test("panda");
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1280x720".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
    Ok(hp)
}

fn heard(hp: &mut Headless, locker: &Locker, what: Heard) -> anyhow::Result<()> {
    let mut seen = Vec::new();
    let came = hp.dispatch_until(WAIT, |_| {
        seen.extend(locker.heard.try_iter());
        seen.contains(&what)
    })?;
    anyhow::ensure!(came, "{what:?} never came: {seen:?}");
    Ok(())
}

/// Whether the screen shows a pixel of `argb`.
fn shows(hp: &mut Headless, argb: u32) -> anyhow::Result<bool> {
    let [b, g, r, _] = argb.to_le_bytes();
    let (_, _, px) = hp.screenshot(0)?;
    Ok(px.as_chunks::<4>().0.iter().any(|p| p[..3] == [b, g, r]))
}

/// A locker's lock: `locked` once the screens drew it, its surfaces alone
/// shown, no window; its own unlock opens it.
#[test]
fn a_locker_locks_with_its_surfaces_and_its_unlock_opens() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let app_id = "test.window";
    let (_, window) = open_showing(
        &mut hp,
        app_id,
        Shows::Window {
            app_id,
            size: (600, 400),
            popup: false,
        },
    )?;
    common::at_rest(&mut hp)?;
    let swaylock = locker(hp.socket(), true)?;
    heard(&mut hp, &swaylock, Heard::Locked)?;
    assert_eq!(hp.lock_state(), LockState::Locked);
    common::at_rest(&mut hp)?;
    assert!(shows(&mut hp, LOCKER)?, "its surface shows");
    assert!(!shows(&mut hp, WINDOW_COLOUR)?, "no window under it");
    swaylock.asks.send(Ask::Unlock)?;
    let open = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Unlocked)?;
    assert!(open, "its unlock opens");
    common::at_rest(&mut hp)?;
    assert!(shows(&mut hp, WINDOW_COLOUR)?, "the window shows again");
    drop(window);
    Ok(())
}

/// Locked by a locker: a second one is refused; its unlock, which it may
/// not send, opens nothing; the locker's crash leaves the session locked
/// under the desktop's own lock, which only the password opens.
#[test]
fn nothing_goes_round_a_lockers_lock() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let swaylock = locker(hp.socket(), true)?;
    heard(&mut hp, &swaylock, Heard::Locked)?;
    // Another locker, refused; its unlock opens nothing.
    let rogue = locker(hp.socket(), false)?;
    heard(&mut hp, &rogue, Heard::Finished)?;
    rogue.asks.send(Ask::Unlock)?;
    common::wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(
        hp.lock_state(),
        LockState::Locked,
        "a rogue unlock opened it"
    );
    // The locker crashes: still locked, black, the desktop's lock holding.
    swaylock.asks.send(Ask::Crash)?;
    let own = hp.dispatch_until(WAIT, |hp| !hp.lock_external())?;
    anyhow::ensure!(own, "the locker's lock still holds");
    common::at_rest(&mut hp)?;
    assert!(!shows(&mut hp, LOCKER)?, "its surface went");
    assert_eq!(hp.lock_state(), LockState::Locked, "the crash opened it");
    // The desktop's own lock: the password opens it.
    for key in [25, 30, 49, 32, 30] {
        chord(&mut hp, &[key]);
    }
    assert_eq!(hp.lock_typed(), Some(5), "typed into the desktop's lock");
    chord(&mut hp, &[KEY_ENTER]);
    let open = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Unlocked)?;
    assert!(open, "the password opens it");
    Ok(())
}
