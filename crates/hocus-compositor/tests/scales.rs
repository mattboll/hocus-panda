//! Screens of different scales side by side (#419, docs/design/windows.md
//! §12, "Screens of different scales"): a window straddling a screen at 2
//! and one at 1 is asked to draw at 2, and reduced on the screen at 1; a
//! window too big for the screen it is dropped on shrinks to fit and takes
//! its size back where it fits; the pointer at a step between screens
//! slides onto the screen beyond; an X11 window on the screen at 2 shows
//! at its size, enlarged.

use crate::common;

use std::{
    os::{fd::AsFd, unix::net::UnixStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use common::{BTN_LEFT, KEY_LEFTALT, WAIT, rect_of, wait, wait_for_rect};
use hocus_compositor::{Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    protocol::{
        wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_registry, wl_shm, wl_shm::WlShm,
        wl_shm_pool::WlShmPool, wl_surface, wl_surface::WlSurface,
    },
};
use wayland_protocols::{
    wp::fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    xdg::shell::client::{
        xdg_surface::{self, XdgSurface},
        xdg_toplevel::{self, XdgToplevel},
        xdg_wm_base::{self, XdgWmBase},
    },
};

/// The screen at 2: 2560×1600 pixels, 1280×800 logical, on the left.
const SHARP: &str = "HEADLESS-1";
/// The screen at 1: 1920×1080, on its right, tops aligned.
const PLAIN: &str = "HEADLESS-2";
const APP_ID: &str = "test.scaled";
const OPAQUE_BLACK: u32 = 0xff00_0000;
const OPAQUE_WHITE: u32 = 0xffff_ffff;
/// The band of stripes, logical rows from the window's top.
const BAND: (i32, i32) = (20, 60);

fn screens() -> anyhow::Result<Headless> {
    Headless::start(HeadlessConfig {
        outputs: vec![
            OutputSpec::new(2560, 1600).scale(2.0),
            OutputSpec::new(1920, 1080),
        ],
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })
}

fn output(hp: &Headless, name: &str) -> anyhow::Result<Rect> {
    hp.outputs()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, r)| r)
        .with_context(|| format!("no {name}"))
}

// --- A client that says the scales it is told -----------------------------

/// What the client was told, in order, and the buffer scale it last drew
/// at.
#[derive(Debug, Default, Clone)]
struct Seen {
    /// `wl_surface.preferred_buffer_scale`.
    buffer: Vec<i32>,
    /// `wp_fractional_scale_v1.preferred_scale`, in 120ths.
    fractional: Vec<u32>,
    /// The scale of its last buffer, and its logical size.
    drawn: Option<(i32, (i32, i32))>,
}

/// A window of its own size until configured otherwise, drawn at the
/// buffer scale it is told: the window's blue, with a band of one-pixel
/// stripes, black and white, in the buffer's pixels.
struct Scaled {
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Scaled {
    fn start(socket: &Path, size: (i32, i32)) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("could not connect to {}", socket.display()))?;
        stream.set_nonblocking(true)?;
        let conn = Connection::from_socket(stream)?;
        let seen = Arc::new(Mutex::new(Seen::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new().name("scaled".into()).spawn({
            let seen = seen.clone();
            let stop = stop.clone();
            move || {
                if let Err(err) = run(&conn, &seen, &stop, size) {
                    eprintln!("scaled client: {err:#}");
                }
            }
        })?;
        Ok(Self {
            seen,
            stop,
            thread: Some(thread),
        })
    }

    fn seen(&self) -> Seen {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Until it was last told `buffer` and `fractional` (120ths), and drew
    /// at `buffer`.
    fn wait_for(&self, hp: &mut Headless, buffer: i32, fractional: u32) -> anyhow::Result<()> {
        let told = hp.dispatch_until(WAIT, |_| {
            let seen = self.seen();
            // 1 is wl_surface's own until told otherwise.
            seen.buffer.last().copied().unwrap_or(1) == buffer
                && seen.fractional.last() == Some(&fractional)
                && seen.drawn.is_some_and(|(s, _)| s == buffer)
        })?;
        anyhow::ensure!(
            told,
            "not told {buffer} and {fractional}: {:?}",
            self.seen()
        );
        Ok(())
    }
}

impl Drop for Scaled {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Client {
    seen: Arc<Mutex<Seen>>,
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    wm_base: Option<XdgWmBase>,
    fractional: Option<WpFractionalScaleManagerV1>,
    surface: Option<WlSurface>,
    own: (i32, i32),
    configured: (i32, i32),
    scale: i32,
    acked: bool,
}

impl Client {
    fn draw(&self, qh: &QueueHandle<Self>) -> anyhow::Result<()> {
        let (surface, shm) = (
            self.surface.as_ref().context("no surface")?,
            self.shm.as_ref().context("no wl_shm")?,
        );
        let size = match self.configured {
            (0, _) | (_, 0) => self.own,
            size => size,
        };
        let (w, h) = (size.0 * self.scale, size.1 * self.scale);
        let mut pixels = vec![0u8; usize::try_from(w * h * 4)?];
        for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let i = i32::try_from(i)?;
            let (x, y) = (i % w, i / w);
            let colour = if (BAND.0 * self.scale..BAND.1 * self.scale).contains(&y) {
                if x % 2 == 0 {
                    OPAQUE_BLACK
                } else {
                    OPAQUE_WHITE
                }
            } else {
                common::WINDOW_COLOUR
            };
            px.copy_from_slice(&colour.to_le_bytes());
        }
        let file = tempfile::tempfile()?;
        std::io::Write::write_all(&mut &file, &pixels)?;
        let pool = shm.create_pool(file.as_fd(), w * h * 4, qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        surface.set_buffer_scale(self.scale);
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, w, h);
        surface.commit();
        if let Ok(mut seen) = self.seen.lock() {
            seen.drawn = Some((self.scale, size));
        }
        Ok(())
    }
}

fn run(
    conn: &Connection,
    seen: &Arc<Mutex<Seen>>,
    stop: &AtomicBool,
    own: (i32, i32),
) -> anyhow::Result<()> {
    let mut queue: EventQueue<Client> = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut client = Client {
        seen: seen.clone(),
        compositor: None,
        shm: None,
        wm_base: None,
        fractional: None,
        surface: None,
        own,
        configured: (0, 0),
        scale: 1,
        acked: false,
    };
    queue.roundtrip(&mut client)?;
    let compositor = client.compositor.clone().context("no wl_compositor")?;
    let wm_base = client.wm_base.clone().context("no xdg_wm_base")?;
    let fractional = client.fractional.clone().context("no fractional scale")?;
    let surface = compositor.create_surface(&qh, ());
    fractional.get_fractional_scale(&surface, &qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_app_id(APP_ID.into());
    surface.commit();
    client.surface = Some(surface);
    while !stop.load(Ordering::Relaxed) {
        queue.dispatch_pending(&mut client)?;
        queue.flush()?;
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
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for Client {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()));
                }
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "xdg_wm_base" => state.wm_base = Some(registry.bind(name, version.min(5), qh, ())),
                "wp_fractional_scale_manager_v1" => {
                    state.fractional = Some(registry.bind(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSurface, ()> for Client {
    fn event(
        state: &mut Self,
        _: &WlSurface,
        event: wl_surface::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_surface::Event::PreferredBufferScale { factor } = event {
            if let Ok(mut seen) = state.seen.lock() {
                seen.buffer.push(factor);
            }
            if factor != state.scale {
                state.scale = factor;
                if state.acked
                    && let Err(err) = state.draw(qh)
                {
                    eprintln!("scaled client: {err:#}");
                }
            }
        }
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for Client {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event
            && let Ok(mut seen) = state.seen.lock()
        {
            seen.fractional.push(scale);
        }
    }
}

impl Dispatch<XdgWmBase, ()> for Client {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for Client {
    fn event(
        state: &mut Self,
        xdg: &XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg.ack_configure(serial);
            state.acked = true;
            if let Err(err) = state.draw(qh) {
                eprintln!("scaled client: {err:#}");
            }
        }
    }
}

impl Dispatch<XdgToplevel, ()> for Client {
    fn event(
        state: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Configure { width, height, .. } = event {
            state.configured = (width, height);
        }
    }
}

delegate_noop!(Client: ignore WlCompositor);
delegate_noop!(Client: ignore WlShm);
delegate_noop!(Client: ignore WlShmPool);
delegate_noop!(Client: ignore WlBuffer);
delegate_noop!(Client: ignore WpFractionalScaleManagerV1);

// --- Moving it --------------------------------------------------------------

/// Opens the client's window on the screen under the pointer, put on
/// `PLAIN`'s middle first.
fn open_on_plain(hp: &mut Headless, size: (i32, i32)) -> anyhow::Result<(Scaled, Rect)> {
    let plain = output(hp, PLAIN)?;
    let (cx, cy) = plain.centre();
    hp.pointer_move(cx, cy);
    let client = Scaled::start(hp.socket(), size)?;
    let mapped = hp.dispatch_until(WAIT, |hp| rect_of(hp, APP_ID).is_some())?;
    anyhow::ensure!(mapped, "{APP_ID} was never mapped");
    // Its opening over.
    wait(hp, Duration::from_millis(400))?;
    let rect = rect_of(hp, APP_ID).context("gone")?;
    Ok((client, rect))
}

/// Moves the test client's window with `Alt`+drag until its top-left
/// corner is at `x` (same height); the button kept down with `hold`.
fn drag_to(hp: &mut Headless, x: i32, hold: bool) -> anyhow::Result<Rect> {
    drag_window(hp, APP_ID, x, hold)
}

/// Moves `app_id`'s window with `Alt`+drag until its top-left corner is at
/// `x` (same height), held 100 px in; the button kept down with `hold`.
fn drag_window(hp: &mut Headless, app_id: &str, x: i32, hold: bool) -> anyhow::Result<Rect> {
    let start = rect_of(hp, app_id).context("no window")?;
    let y = f64::from(start.y + 100);
    hp.pointer_move(f64::from(start.x + 100), y);
    hp.key(KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
    let mut at = start.x;
    while at != x {
        at = if at < x {
            (at + 40).min(x)
        } else {
            (at - 40).max(x)
        };
        hp.pointer_move(f64::from(at + 100), y);
        hp.dispatch(Duration::ZERO)?;
    }
    let reached = hp.dispatch_until(WAIT, |hp| {
        rect_of(hp, app_id).is_some_and(|r| (r.x, r.y) == (x, start.y))
    })?;
    anyhow::ensure!(reached, "at {:?}, not at {x}", rect_of(hp, app_id));
    let moved = rect_of(hp, app_id).context("gone")?;
    if !hold {
        release(hp);
    }
    Ok(moved)
}

fn release(hp: &mut Headless) {
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTALT, false);
}

/// A screenshot's pixel at (`x`, `y`), ARGB.
fn pixel(shot: &(i32, i32, Vec<u8>), x: i32, y: i32) -> u32 {
    let (w, _, px) = shot;
    let at = usize::try_from((y * w + x) * 4).unwrap_or(0);
    px.get(at..at + 4)
        .map_or(0, |p| u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
}

// --- The tests --------------------------------------------------------------

#[test]
fn a_window_straddling_two_scales_is_asked_for_the_larger_and_keeps_its_size() -> anyhow::Result<()>
{
    let mut hp = screens()?;
    let (sharp, plain) = (output(&hp, SHARP)?, output(&hp, PLAIN)?);
    assert_eq!(sharp, Rect::new(0, 0, 1280, 800));
    assert_eq!(plain, Rect::new(1280, 0, 1920, 1080));
    let (client, rect) = open_on_plain(&mut hp, (1000, 640))?;
    assert_eq!((rect.width, rect.height), (1000, 640));
    client.wait_for(&mut hp, 1, 120)?;

    // Across the seam, held: asked for 2 at once, while it moves.
    let across = drag_to(&mut hp, 1000, true)?;
    client.wait_for(&mut hp, 2, 240)?;
    assert_eq!((across.width, across.height), (1000, 640), "its size");
    wait(&mut hp, Duration::from_millis(100))?;
    // Each screen shows its part at its own density: on the screen at 2
    // the stripes are one pixel each, on the one at 1 two buffer pixels
    // make one, neither black nor white.
    let row = across.y + (BAND.0 + BAND.1) / 2;
    let left = hp.screenshot(0)?;
    let stripes: Vec<u32> = (2 * 1100..2 * 1100 + 16)
        .map(|x| pixel(&left, x, 2 * row))
        .collect();
    let alternate = stripes
        .windows(2)
        .all(|p| p[0] != p[1] && [OPAQUE_BLACK, OPAQUE_WHITE].contains(&(p[0] | 0xff00_0000)));
    assert!(alternate, "not one pixel each at 2: {stripes:x?}");
    let right = hp.screenshot(1)?;
    let reduced: Vec<u32> = (1400 - 1280..1400 - 1280 + 16)
        .map(|x| pixel(&right, x, row))
        .collect();
    let grey = reduced.iter().all(|p| {
        let r = (p >> 16) & 0xff;
        (0x40..=0xc0).contains(&r)
    });
    assert!(grey, "not reduced at 1: {reduced:x?}");
    release(&mut hp);

    // Wholly on the screen at 2: 2; back wholly on the one at 1: 1.
    drag_to(&mut hp, 100, false)?;
    client.wait_for(&mut hp, 2, 240)?;
    let back = drag_to(&mut hp, 1500, false)?;
    client.wait_for(&mut hp, 1, 120)?;
    assert_eq!((back.width, back.height), (1000, 640), "its size");
    let seen = client.seen();
    assert_eq!(seen.drawn, Some((1, (1000, 640))));
    // Told each change once.
    let changes = |v: &[u32]| v.windows(2).filter(|p| p[0] != p[1]).count();
    assert_eq!(
        seen.fractional.len(),
        changes(&seen.fractional) + 1,
        "{seen:?}"
    );
    Ok(())
}

#[test]
fn a_window_too_big_shrinks_to_fit_and_takes_its_size_back_where_it_fits() -> anyhow::Result<()> {
    let mut hp = screens()?;
    let small = hp.work_area_of(SHARP).context("no work area")?;
    let large = hp.work_area_of(PLAIN).context("no work area")?;
    assert!(large.width >= 1500 && large.height >= 1000, "{large:?}");
    let (client, rect) = open_on_plain(&mut hp, (1500, 1000))?;
    assert_eq!((rect.width, rect.height), (1500, 1000));

    // Dropped on the screen at 2, its centre there: the work area's size,
    // wholly in it.
    drag_to(&mut hp, 100, false)?;
    wait_for_rect(&mut hp, APP_ID, small)?;
    client.wait_for(&mut hp, 2, 240)?;

    // Back where it fits: its size again, inside the work area.
    let back = drag_to(&mut hp, 1400, false)?;
    let want = Rect::new(
        1400.min(large.x + large.width - 1500),
        back.y.min(large.y + large.height - 1000),
        1500,
        1000,
    );
    wait_for_rect(&mut hp, APP_ID, want)?;

    // Shrunk again, then resized by the user from its right edge: dragged
    // back, it keeps the size the user gave it.
    drag_to(&mut hp, 100, false)?;
    wait_for_rect(&mut hp, APP_ID, small)?;
    let y = f64::from(small.y + small.height / 2);
    hp.pointer_move(f64::from(small.x + small.width + 3), y);
    hp.button(BTN_LEFT, true);
    for step in 1..=10 {
        hp.pointer_move(f64::from(small.x + small.width + 3 - 30 * step), y);
        hp.dispatch(Duration::ZERO)?;
    }
    hp.button(BTN_LEFT, false);
    let resized = Rect::new(small.x, small.y, small.width - 300, small.height);
    wait_for_rect(&mut hp, APP_ID, resized)?;
    let kept = drag_to(&mut hp, 1400, false)?;
    wait(&mut hp, Duration::from_millis(200))?;
    assert_eq!(
        rect_of(&hp, APP_ID).map(|r| (r.width, r.height)),
        Some((kept.width, kept.height))
    );
    assert_eq!((kept.width, kept.height), (resized.width, resized.height));
    Ok(())
}

#[test]
fn a_window_sent_by_keys_shrinks_to_fit_and_takes_its_size_back() -> anyhow::Result<()> {
    let mut hp = screens()?;
    let small = hp.work_area_of(SHARP).context("no work area")?;
    let large = hp.work_area_of(PLAIN).context("no work area")?;
    let (_client, rect) = open_on_plain(&mut hp, (1500, 1000))?;
    assert_eq!((rect.width, rect.height), (1500, 1000));
    common::click(&mut hp, (f64::from(rect.x + 200), f64::from(rect.y + 200)))?;
    // `Super+Alt+←`: onto the screen at 2, its work area's size.
    common::chord(
        &mut hp,
        &[common::KEY_LEFTMETA, KEY_LEFTALT, common::KEY_LEFT],
    );
    wait_for_rect(&mut hp, APP_ID, small)?;
    // `Super+Alt+→`: back, 1500×1000 again, inside the work area.
    common::chord(
        &mut hp,
        &[common::KEY_LEFTMETA, KEY_LEFTALT, common::KEY_RIGHT],
    );
    let back = hp.dispatch_until(WAIT, |hp| {
        rect_of(hp, APP_ID).is_some_and(|r| {
            (r.width, r.height) == (1500, 1000)
                && r.x >= large.x
                && r.y >= large.y
                && r.x + r.width <= large.x + large.width
                && r.y + r.height <= large.y + large.height
        })
    })?;
    assert!(back, "not its size back: {:?}", rect_of(&hp, APP_ID));
    Ok(())
}

#[test]
fn the_pointer_at_a_step_between_screens_slides_onto_the_screen_beyond() -> anyhow::Result<()> {
    let mut hp = screens()?;
    // On the screen at 1, below the bottom of the one at 2, going left.
    hp.pointer_move(1290.0, 1000.0);
    hp.pointer_by(-30.0, 0.0);
    assert_eq!(hp.pointer(), (1260.0, 799.0), "onto the nearest point");
    // Across the edge the screens share, it goes on as it is.
    hp.pointer_move(1290.0, 400.0);
    hp.pointer_by(-30.0, 0.0);
    assert_eq!(hp.pointer(), (1260.0, 400.0));
    // The layout's outer edges still stop it.
    hp.pointer_move(1290.0, 1075.0);
    hp.pointer_by(0.0, 20.0);
    assert_eq!(hp.pointer(), (1290.0, 1079.0));
    hp.pointer_move(600.0, 790.0);
    hp.pointer_by(0.0, 30.0);
    assert_eq!(hp.pointer(), (600.0, 799.0));
    Ok(())
}

/// An X11 window of 400×300, the window's blue, with this class, mapped;
/// its connection kept open on a thread until XWayland goes.
fn x11_window(display: u32, class: &'static str) -> anyhow::Result<()> {
    use x11rb::{
        COPY_DEPTH_FROM_PARENT,
        connection::Connection as _,
        protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, PropMode, WindowClass},
        wrapper::ConnectionExt as _,
    };
    let (said, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let made = (|| -> anyhow::Result<_> {
            let (conn, screen) = x11rb::connect(Some(&format!(":{display}")))?;
            let root = conn.setup().roots[screen].root;
            let window = conn.generate_id()?;
            conn.create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                400,
                300,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().background_pixel(0x003a_7bd5),
            )?;
            let names = format!("{class}\0{class}\0");
            conn.change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                names.as_bytes(),
            )?;
            conn.map_window(window)?;
            conn.flush()?;
            Ok(conn)
        })();
        match made {
            Ok(conn) => {
                let _ = said.send(Ok(()));
                while conn.wait_for_event().is_ok() {}
            }
            Err(err) => {
                let _ = said.send(Err(err));
            }
        }
    });
    heard.recv_timeout(WAIT)?
}

#[test]
fn an_x11_window_on_the_screen_at_2_shows_at_its_size_enlarged() -> anyhow::Result<()> {
    let has = std::process::Command::new("sh")
        .args(["-c", "command -v Xwayland"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !has {
        common::missing("Xwayland");
    }
    let mut hp = Headless::start_with_xwayland(HeadlessConfig {
        outputs: vec![
            OutputSpec::new(2560, 1600).scale(2.0),
            OutputSpec::new(1920, 1080),
        ],
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })?;
    let ready = hp.dispatch_until(WAIT, |hp| hp.xdisplay().is_some())?;
    anyhow::ensure!(ready, "XWayland never got ready");
    let display = hp.xdisplay().context("no display")?;
    hp.pointer_move(640.0, 400.0);
    const CLASS: &str = "test.x11-scaled";
    x11_window(display, CLASS)?;
    let shown = hp.dispatch_until(WAIT, |hp| rect_of(hp, CLASS).is_some())?;
    anyhow::ensure!(shown, "{CLASS} never showed: {:?}", hp.windows());
    wait(&mut hp, Duration::from_millis(500))?;
    let rect = rect_of(&hp, CLASS).context("gone")?;
    assert_eq!((rect.width, rect.height), (400, 300), "its size, logical");
    assert!(
        rect.x >= 0 && rect.x + rect.width <= 1280,
        "on the screen at 2: {rect:?}"
    );
    // Its 400×300 cover 800×600 pixels there: its blue up to its far
    // corner, inside.
    let shot = hp.screenshot(0)?;
    let blue = |x: i32, y: i32| pixel(&shot, x, y) & 0x00ff_ffff == 0x003a_7bd5;
    for (x, y) in [(4, 4), (395, 4), (4, 295), (395, 295), (200, 150)] {
        let (px, py) = (2 * (rect.x + x), 2 * (rect.y + y));
        assert!(
            blue(px, py),
            "not its blue at {x},{y}: {:x}",
            pixel(&shot, px, py)
        );
    }
    Ok(())
}

// --- Screenshots, by hand ---------------------------------------------------

/// A program started for a test, stopped by its own PID when it ends.
struct Started(std::process::Child);

impl Drop for Started {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// An ARGB screenshot as an RGBA image.
fn image_of((w, h, px): (i32, i32, Vec<u8>)) -> anyhow::Result<image::RgbaImage> {
    let mut rgba = px;
    for p in rgba.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    image::RgbaImage::from_raw(u32::try_from(w)?, u32::try_from(h)?, rgba).context("not an image")
}

/// foot straddling the screen at 2 and the one at 1, saved in the folder
/// `HOCUS_SHOTS` names: each screen (`sharp.png`, `plain.png`), both as
/// arranged, a logical pixel each (`layout.png`), and the window's part on each
/// side of the seam at the same size (`seam.png`: the screen at 1's
/// pixels doubled, as the eye sees them at that screen's size). Needs
/// foot.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR, needs foot"]
fn straddling_shots() -> anyhow::Result<()> {
    let dir = std::path::PathBuf::from(std::env::var_os("HOCUS_SHOTS").context("HOCUS_SHOTS")?);
    std::fs::create_dir_all(&dir)?;
    let mut hp = screens()?;
    hp.pointer_move(2240.0, 540.0);
    let home = tempfile::Builder::new().prefix("hp").tempdir_in("/tmp")?;
    let mut env = Vec::new();
    for (var, sub) in [
        ("HOME", "home"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_RUNTIME_DIR", "run"),
    ] {
        let place = home.path().join(sub);
        std::fs::create_dir(&place)?;
        env.push((var, place));
    }
    let source = concat!(env!("CARGO_MANIFEST_DIR"), "/src/scales.rs");
    let script = format!(
        "printf '\\033[1;32mScreens of different scales\\033[0m: one window, two densities\\n\\n'; \
         cat -n {source} | sed -n 1,30p; exec sleep 600"
    );
    let foot = std::process::Command::new("foot")
        .args(["-W", "118x36", "sh", "-c", &script])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .envs(env)
        .env("WAYLAND_DISPLAY", hp.socket())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("could not start foot")?;
    let _foot = Started(foot);
    let shown = hp.dispatch_until(WAIT, |hp| rect_of(hp, "foot").is_some())?;
    anyhow::ensure!(shown, "foot never showed");
    wait(&mut hp, Duration::from_secs(1))?;
    let rect = rect_of(&hp, "foot").context("gone")?;
    let straddling = drag_window(&mut hp, "foot", 1280 - rect.width * 2 / 5, false)?;
    hp.pointer_move(2900.0, 1000.0);
    wait(&mut hp, Duration::from_secs(1))?;
    let (sharp, plain) = (image_of(hp.screenshot(0)?)?, image_of(hp.screenshot(1)?)?);
    sharp.save(dir.join("sharp.png"))?;
    plain.save(dir.join("plain.png"))?;
    // The two screens as they are arranged, one logical pixel each: the
    // window one piece, the same size on both.
    let logical = image::imageops::resize(
        &sharp,
        sharp.width() / 2,
        sharp.height() / 2,
        image::imageops::FilterType::Triangle,
    );
    let mut layout = image::RgbaImage::from_pixel(
        logical.width() + plain.width(),
        plain.height(),
        image::Rgba([24, 24, 28, 255]),
    );
    image::imageops::overlay(&mut layout, &logical, 0, 0);
    image::imageops::overlay(&mut layout, &plain, i64::from(logical.width()), 0);
    layout.save(dir.join("layout.png"))?;
    // 300 logical pixels on each side of the seam, 360 high, at 2 pixels
    // each: the screen at 2's own, the screen at 1's doubled.
    let (top, high, side) = (straddling.y.max(0), 360, 300);
    let left = image::imageops::crop_imm(
        &sharp,
        u32::try_from(2 * (1280 - side))?,
        u32::try_from(2 * top)?,
        u32::try_from(2 * side)?,
        u32::try_from(2 * high)?,
    )
    .to_image();
    let right = image::imageops::crop_imm(
        &plain,
        0,
        u32::try_from(top)?,
        u32::try_from(side)?,
        u32::try_from(high)?,
    )
    .to_image();
    let right = image::imageops::resize(
        &right,
        right.width() * 2,
        right.height() * 2,
        image::imageops::FilterType::Nearest,
    );
    let mut seam = image::RgbaImage::from_pixel(
        left.width() + right.width() + 4,
        left.height(),
        image::Rgba([60, 60, 60, 255]),
    );
    image::imageops::overlay(&mut seam, &left, 0, 0);
    image::imageops::overlay(&mut seam, &right, i64::from(left.width() + 4), 0);
    seam.save(dir.join("seam.png"))?;
    eprintln!("saved in {}", dir.display());
    Ok(())
}

/// xterm (X11) on the screen at 2, saved as `x11.png` in the folder
/// `HOCUS_SHOTS` names: the window's part of the screen, at the right
/// size, enlarged and a little soft. Needs Xwayland and xterm.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR, needs Xwayland, xterm and unshare"]
fn x11_shots() -> anyhow::Result<()> {
    let dir = std::path::PathBuf::from(std::env::var_os("HOCUS_SHOTS").context("HOCUS_SHOTS")?);
    std::fs::create_dir_all(&dir)?;
    let mut hp = Headless::start_with_xwayland(HeadlessConfig {
        outputs: vec![
            OutputSpec::new(2560, 1600).scale(2.0),
            OutputSpec::new(1920, 1080),
        ],
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })?;
    let ready = hp.dispatch_until(WAIT, |hp| hp.xdisplay().is_some())?;
    anyhow::ensure!(ready, "XWayland never got ready");
    let display = hp.xdisplay().context("no display")?;
    hp.pointer_move(640.0, 400.0);
    let home = tempfile::Builder::new().prefix("hp").tempdir_in("/tmp")?;
    let script = "printf 'An X11 application on the screen at 2:\\nits size, enlarged, a little soft.\\n\\n'; \
                  seq -w 1 12 | paste -sd' '; exec sleep 600";
    // As a user of its own: in a namespace where the test is root (the
    // tests' `nogpu`), xterm would give its terminal to root and stop.
    let xterm = std::process::Command::new("unshare")
        .args(["--user", "--map-user=1000", "--map-group=1000", "xterm"])
        .args(["-geometry", "64x14", "-fa", "Monospace", "-fs", "11"])
        .args(["-bg", "#1e1e22", "-fg", "#d8dae0", "-e", "sh", "-c", script])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("HOME", home.path())
        .env("DISPLAY", format!(":{display}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(home.path().join("xterm.log"))?)
        .spawn()
        .context("could not start xterm")?;
    let _xterm = Started(xterm);
    let shown = hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some("XTerm"))
    })?;
    anyhow::ensure!(
        shown,
        "xterm never showed: {:?} {}",
        hp.windows(),
        std::fs::read_to_string(home.path().join("xterm.log")).unwrap_or_default()
    );
    hp.pointer_move(1000.0, 780.0);
    wait(&mut hp, Duration::from_secs(1))?;
    let rect = rect_of(&hp, "XTerm").context("gone")?;
    let sharp = image_of(hp.screenshot(0)?)?;
    let margin = 24;
    let window = image::imageops::crop_imm(
        &sharp,
        u32::try_from(2 * (rect.x - margin).max(0))?,
        u32::try_from(2 * (rect.y - margin).max(0))?,
        u32::try_from(2 * (rect.width + 2 * margin))?,
        u32::try_from(2 * (rect.height + 2 * margin))?,
    )
    .to_image();
    window.save(dir.join("x11.png"))?;
    eprintln!("xterm at {rect:?}, saved in {}", dir.display());
    Ok(())
}
