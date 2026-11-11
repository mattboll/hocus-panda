//! Who draws a window's header (docs/design/windows.md §7, #716): a client
//! leaving it to the desktop through KDE's server decoration protocol, as
//! GTK 3 clients do, having no xdg-decoration, gets the grip and moves by
//! it; one drawing its own through that protocol, or asking through
//! neither protocol (GTK 4), gets none. On XWayland, an X11
//! window gets it unless its Motif hints ask for no decorations.

use crate::common;

use std::time::Duration;

use common::{BTN_LEFT, ClientHandle, Command, Event, Shows, WAIT, compositor, open_showing, wait};
use hocus_compositor::{
    Headless,
    model::{Rect, WindowId},
};

/// How long a grip that must not show is watched: five times what it
/// takes to show.
const NOT_SHOWN: Duration = Duration::from_millis(600);

/// The grip's middle on the screen: 4 px inside the window's top edge, 4
/// high (windows.md §7).
fn grip_middle(window: Rect) -> (f64, f64) {
    (
        f64::from(window.x) + f64::from(window.width) / 2.0,
        f64::from(window.y) + 6.0,
    )
}

/// Off the grip's middle, still on the pill, where the cursor is not
/// drawn.
fn on_pill(grip: (f64, f64)) -> (f64, f64) {
    (grip.0 + 18.0, grip.1)
}

/// The red channel at `at` on the screen: the test windows' blue has
/// little (0x3a), the grip's light grey much.
fn red(hp: &mut Headless, at: (f64, f64)) -> anyhow::Result<u8> {
    let (w, _, px) = hp.screenshot(0)?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (at.1 as usize * w as usize + at.0 as usize) * 4;
    Ok(px[i + 2])
}

fn id_of(hp: &Headless, app_id: &str) -> anyhow::Result<WindowId> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window {app_id}"))
}

/// Runs the compositor until the client is told its KDE decoration's mode.
fn kde_mode(hp: &mut Headless, client: &ClientHandle) -> anyhow::Result<bool> {
    let mut mode = None;
    hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = client.events.try_recv() {
            if let Event::KdeMode { server } = event {
                mode = Some(server);
            }
        }
        mode.is_some()
    })?;
    mode.ok_or_else(|| anyhow::anyhow!("no mode came"))
}

/// The pointer over the grip's place, then at most `within` for it to
/// show (it does in 120 ms): whether it did, and the cursor.
fn hover_grip(
    hp: &mut Headless,
    window: Rect,
    within: Duration,
) -> anyhow::Result<(bool, Option<String>)> {
    let grip = grip_middle(window);
    hp.pointer_move(grip.0 - 1.0, grip.1);
    hp.pointer_move(grip.0, grip.1);
    let start = std::time::Instant::now();
    let mut shown = false;
    while start.elapsed() < within {
        hp.dispatch(Duration::from_millis(10))?;
        if red(hp, on_pill(grip))? > 150 {
            shown = true;
            break;
        }
    }
    Ok((shown, hp.frame_cursor()))
}

#[test]
fn a_window_leaving_its_header_to_the_desktop_through_kdes_protocol_gets_the_grip_and_moves_by_it()
-> anyhow::Result<()> {
    let mut hp = compositor()?;
    let shows = Shows::KdeDecorated {
        app_id: "test.kde",
        size: (500, 400),
        server: true,
        header: None,
    };
    let (window, client) = open_showing(&mut hp, "test.kde", shows)?;
    assert!(
        kde_mode(&mut hp, &client)?,
        "the server's mode was not given"
    );
    let id = id_of(&hp, "test.kde")?;
    assert!(!hp.state().has_own_header(id), "taken for its own header");
    // Away from the top edge, the window's blue under the grip.
    let grip = grip_middle(window);
    hp.pointer_move(grip.0, grip.1 + 200.0);
    wait(&mut hp, Duration::from_millis(400))?;
    let blue = red(&mut hp, on_pill(grip))?;
    assert!(blue < 100, "something light under the grip: {blue}");
    // Over it: the pill shows, and the cursor grabs.
    let (shown, cursor) = hover_grip(&mut hp, window, WAIT)?;
    assert!(shown, "the grip never showed");
    assert_eq!(cursor.as_deref(), Some("grab"));
    // Dragged: the window follows.
    hp.button(BTN_LEFT, true);
    for step in 1..=10 {
        let k = f64::from(step) / 10.0;
        hp.pointer_move(grip.0 - 200.0 * k, grip.1 + 100.0 * k);
        wait(&mut hp, Duration::from_millis(16))?;
    }
    hp.button(BTN_LEFT, false);
    let moved = Rect {
        x: window.x - 200,
        y: window.y + 100,
        ..window
    };
    common::wait_for_rect(&mut hp, "test.kde", moved)?;
    drop(client);
    Ok(())
}

#[test]
fn a_window_drawing_its_own_header_through_kdes_protocol_gets_no_grip() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let shows = Shows::KdeDecorated {
        app_id: "test.kde-own",
        size: (500, 400),
        server: false,
        header: None,
    };
    let (window, client) = open_showing(&mut hp, "test.kde-own", shows)?;
    // The protocol's default is the server's; asked, the client's own.
    let mut modes = Vec::new();
    hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = client.events.try_recv() {
            if let Event::KdeMode { server } = event {
                modes.push(server);
            }
        }
        modes.last() == Some(&false)
    })?;
    assert_eq!(modes.last(), Some(&false), "modes given: {modes:?}");
    let id = id_of(&hp, "test.kde-own")?;
    assert!(hp.state().has_own_header(id), "taken for headerless");
    let (shown, cursor) = hover_grip(&mut hp, window, NOT_SHOWN)?;
    assert!(!shown, "a grip over its own header");
    assert_ne!(cursor.as_deref(), Some("grab"));
    Ok(())
}

#[test]
fn a_window_binding_no_decoration_protocol_gets_no_grip() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let shows = Shows::Undecorated {
        app_id: "test.undecorated",
        size: (500, 400),
    };
    let (window, _client) = open_showing(&mut hp, "test.undecorated", shows)?;
    let id = id_of(&hp, "test.undecorated")?;
    assert!(hp.state().has_own_header(id), "taken for headerless");
    let (shown, cursor) = hover_grip(&mut hp, window, NOT_SHOWN)?;
    assert!(!shown, "a grip over its own header");
    assert_ne!(cursor.as_deref(), Some("grab"));
    Ok(())
}

// --- The pill against what is under it (#719) -----------------------------

/// KDE Connect's Kirigami header (#716), and Breeze Dark's.
const LIGHT_HEADER: u32 = 0xffde_e0e2;
const DARK_HEADER: u32 = 0xff31_363b;

/// The colour at `at` on the screen: its rows are BGRA.
fn rgb(px: &[u8], w: i32, at: (f64, f64)) -> [f64; 3] {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (at.1 as usize * w as usize + at.0 as usize) * 4;
    [px[i + 2], px[i + 1], px[i]].map(f64::from)
}

/// WCAG's contrast ratio between two sRGB colours.
fn contrast(a: [f64; 3], b: [f64; 3]) -> f64 {
    let luminance = |c: [f64; 3]| {
        let lin = |v: f64| {
            let v = v / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
    };
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// A window leaving its header to the desktop whose top is `header`
/// (ARGB), its grip shown: the pill's colour and the header's beside it.
/// With `HOCUS_SHOT` set to a directory, the window's top is saved there
/// as `<name>.png`, three times larger.
fn pill_over(
    header: u32,
    app_id: &'static str,
    name: &str,
    settled: impl Fn([f64; 3]) -> bool,
) -> anyhow::Result<([f64; 3], [f64; 3])> {
    let mut hp = compositor()?;
    let shows = Shows::KdeDecorated {
        app_id,
        size: (500, 400),
        server: true,
        header: Some(header),
    };
    let (window, client) = open_showing(&mut hp, app_id, shows)?;
    assert!(
        kde_mode(&mut hp, &client)?,
        "the server's mode was not given"
    );
    let grip = grip_middle(window);
    header_shown(&mut hp, grip, header)?;
    hp.pointer_move(grip.0 - 1.0, grip.1);
    hp.pointer_move(grip.0, grip.1);
    // Shown (120 ms), and what is under it read a frame or two later:
    // as late as a loaded machine makes it, so until the pill takes the
    // tone `settled` looks for.
    wait(&mut hp, Duration::from_millis(400))?;
    assert_eq!(hp.frame_cursor().as_deref(), Some("grab"));
    let until = std::time::Instant::now() + WAIT;
    let (w, h, px, pill) = loop {
        let (w, h, px) = hp.screenshot(0)?;
        let pill = rgb(&px, w, on_pill(grip));
        if settled(pill) || std::time::Instant::now() > until {
            break (w, h, px, pill);
        }
        wait(&mut hp, Duration::from_millis(50))?;
    };
    // On the pill's row, well clear of it.
    let beside = rgb(&px, w, (grip.0 + 60.0, grip.1));
    if let Some(dir) = std::env::var_os("HOCUS_SHOT") {
        save_top(
            &px,
            (w, h),
            window,
            &std::path::Path::new(&dir).join(format!("{name}.png")),
        )?;
    }
    drop(client);
    Ok((pill, beside))
}

/// The top 60 px of `window` on a screen `size` large (BGRA rows), three
/// times larger, as a PNG.
#[allow(clippy::cast_sign_loss)]
fn save_top(
    px: &[u8],
    size: (i32, i32),
    window: Rect,
    file: &std::path::Path,
) -> anyhow::Result<()> {
    const K: u32 = 3;
    let (x0, y0) = ((window.x - 20).max(0) as u32, (window.y - 20).max(0) as u32);
    let (cw, ch) = ((window.width + 40) as u32, 80u32);
    let (cw, ch) = (cw.min(size.0 as u32 - x0), ch.min(size.1 as u32 - y0));
    let mut out = vec![0u8; (cw * K * ch * K * 4) as usize];
    for y in 0..ch * K {
        for x in 0..cw * K {
            let i = (((y0 + y / K) * size.0 as u32 + x0 + x / K) * 4) as usize;
            let o = ((y * cw * K + x) * 4) as usize;
            out[o..o + 4].copy_from_slice(&[px[i + 2], px[i + 1], px[i], 255]);
        }
    }
    image::save_buffer(file, &out, cw * K, ch * K, image::ColorType::Rgba8)?;
    Ok(())
}

#[test]
fn on_a_light_header_the_grips_pill_turns_dark_and_stands_out() -> anyhow::Result<()> {
    let (pill, header) = pill_over(LIGHT_HEADER, "test.light-header", "light-header", |p| {
        p[0] < 100.0
    })?;
    assert!(
        header[0] > 200.0,
        "not the light header beside it: {header:?}"
    );
    assert!(pill[0] < 100.0, "the pill stayed light: {pill:?}");
    let ratio = contrast(pill, header);
    assert!(ratio >= 3.0, "the pill is {ratio:.2}:1 against the header");
    Ok(())
}

#[test]
fn on_a_dark_header_the_grips_pill_stays_light_and_stands_out() -> anyhow::Result<()> {
    let (pill, header) = pill_over(DARK_HEADER, "test.dark-header", "dark-header", |p| {
        p[0] > 150.0
    })?;
    assert!(
        header[0] < 80.0,
        "not the dark header beside it: {header:?}"
    );
    assert!(pill[0] > 150.0, "the pill turned dark: {pill:?}");
    let ratio = contrast(pill, header);
    assert!(ratio >= 3.0, "the pill is {ratio:.2}:1 against the header");
    Ok(())
}

/// Waits until the window's `header` (ARGB) shows on the grip's row, clear
/// of the pill: the window opened whole, where it stays. A window still
/// opening is drawn smaller, its top lower, and the screen's dark
/// background there passes for a dark pill.
fn header_shown(hp: &mut Headless, grip: (f64, f64), header: u32) -> anyhow::Result<()> {
    let want = [(header >> 16) & 0xff, (header >> 8) & 0xff, header & 0xff].map(f64::from);
    let until = std::time::Instant::now() + WAIT;
    loop {
        let (w, _, px) = hp.screenshot(0)?;
        let beside = rgb(&px, w, (grip.0 + 60.0, grip.1));
        if beside.iter().zip(want).all(|(a, b)| (a - b).abs() <= 2.0) {
            return Ok(());
        }
        anyhow::ensure!(
            std::time::Instant::now() < until,
            "the header never showed: {beside:?}"
        );
        wait(hp, Duration::from_millis(50))?;
    }
}

/// The pill's colour once `settled` holds of it, or after [`WAIT`].
fn pill_until(
    hp: &mut Headless,
    grip: (f64, f64),
    settled: impl Fn([f64; 3]) -> bool,
) -> anyhow::Result<[f64; 3]> {
    let until = std::time::Instant::now() + WAIT;
    loop {
        let (w, _, px) = hp.screenshot(0)?;
        let pill = rgb(&px, w, on_pill(grip));
        if settled(pill) || std::time::Instant::now() > until {
            return Ok(pill);
        }
        wait(hp, Duration::from_millis(50))?;
    }
}

/// The pill takes its new tone when what is under it changes once it is
/// still: then only the tone changes, and it must still be drawn again.
#[test]
fn the_grips_pill_turns_dark_when_its_header_turns_light_under_it() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let shows = Shows::KdeDecorated {
        app_id: "test.header-turns",
        size: (500, 400),
        server: true,
        header: Some(DARK_HEADER),
    };
    let (window, client) = open_showing(&mut hp, "test.header-turns", shows)?;
    assert!(
        kde_mode(&mut hp, &client)?,
        "the server's mode was not given"
    );
    let grip = grip_middle(window);
    header_shown(&mut hp, grip, DARK_HEADER)?;
    hp.pointer_move(grip.0 - 1.0, grip.1);
    hp.pointer_move(grip.0, grip.1);
    // Shown whole and light, then still: nothing moves on the screen
    // any more.
    let light = pill_until(&mut hp, grip, |p| p[0] > 150.0)?;
    assert!(light[0] > 150.0, "the pill never showed light: {light:?}");
    wait(&mut hp, Duration::from_millis(500))?;
    client.commands.send(Command::Header(LIGHT_HEADER))?;
    let dark = pill_until(&mut hp, grip, |p| p[0] < 100.0)?;
    assert!(
        dark[0] < 100.0,
        "the pill stayed light over the light header: {dark:?}"
    );
    drop(client);
    Ok(())
}

// --- X11 windows ------------------------------------------------------------

/// An X11 window of class `class`, 400×300, on XWayland's display
/// `display`, its Motif hints asking for no decorations when it draws its
/// own (`own`), as GTK 3's client-side decorated windows do; Qt's ask for
/// all of them, xterm's say nothing. On a thread of its own, as the
/// Wayland test clients: it holds its connection until the test ends.
fn x11_window(display: u32, class: &'static str, own: bool) -> anyhow::Result<()> {
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
            if own {
                let motif = conn.intern_atom(false, b"_MOTIF_WM_HINTS")?.reply()?.atom;
                // Flags: decorations given; decorations: none.
                conn.change_property32(PropMode::REPLACE, window, motif, motif, &[2, 0, 0, 0, 0])?;
            }
            conn.map_window(window)?;
            conn.flush()?;
            Ok(conn)
        })();
        match made {
            Ok(conn) => {
                let _ = said.send(Ok(()));
                // Until the compositor, and XWayland with it, goes.
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
fn an_x11_window_gets_the_grip_unless_its_motif_hints_ask_for_no_decorations() -> anyhow::Result<()>
{
    let has = std::process::Command::new("sh")
        .args(["-c", "command -v Xwayland"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !has {
        common::missing("Xwayland");
    }
    let mut hp = Headless::start_with_xwayland(hocus_compositor::HeadlessConfig {
        renderer: hocus_compositor::RendererChoice::Pixman,
        ..hocus_compositor::HeadlessConfig::default()
    })?;
    let ready = hp.dispatch_until(WAIT, |hp| hp.xdisplay().is_some())?;
    anyhow::ensure!(ready, "XWayland never got ready");
    let display = hp.xdisplay().ok_or_else(|| anyhow::anyhow!("no display"))?;
    for (class, own) in [("test.x11-plain", false), ("test.x11-own", true)] {
        x11_window(display, class, own)?;
        let shown = hp.dispatch_until(WAIT, |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(class))
        })?;
        anyhow::ensure!(shown, "{class} never showed: {:?}", hp.windows());
        let id = id_of(&hp, class)?;
        assert_eq!(hp.state().has_own_header(id), own, "{class}");
    }
    Ok(())
}

// --- A real KDE application -----------------------------------------------

/// A program the test started, stopped by its own PID when the test ends.
struct Started(std::process::Child);

impl Drop for Started {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// KDE Connect's window (Qt 6, Kirigami), as the maintainer saw it (#716):
/// it gets the grip on hover. With `HOCUS_SHOT` set to a file, the screen
/// is saved there as a PNG. Needs `kdeconnect-app` and `dbus-daemon`: run
/// by hand.
#[test]
#[ignore = "needs kdeconnect-app and dbus-daemon"]
fn kde_connects_window_gets_the_grip() -> anyhow::Result<()> {
    const APP_ID: &str = "org.kde.kdeconnect.app";
    let mut hp = compositor()?;
    let dir = tempfile::tempdir()?;
    let (bus, address) = common::private_bus(dir.path())?;
    let _bus = Started(bus);
    let (home, runtime) = (dir.path().join("home"), dir.path().join("run"));
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&runtime)?;
    let app = std::process::Command::new("kdeconnect-app")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("WAYLAND_DISPLAY", hp.socket())
        .env("QT_QPA_PLATFORM", "wayland")
        .env("QT_QUICK_BACKEND", "software")
        .env("DBUS_SESSION_BUS_ADDRESS", &address)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let _app = Started(app);
    let shown = hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(APP_ID))
    })?;
    anyhow::ensure!(shown, "KDE Connect's window never showed");
    let id = id_of(&hp, APP_ID)?;
    let headerless = hp.dispatch_until(WAIT, |hp| !hp.state().has_own_header(id))?;
    anyhow::ensure!(headerless, "taken for a window with its own header");
    // Drawn, then the pointer over the grip's place.
    wait(&mut hp, Duration::from_secs(2))?;
    let window = common::rect_of(&hp, APP_ID).ok_or_else(|| anyhow::anyhow!("gone"))?;
    let grip = grip_middle(window);
    hp.pointer_move(grip.0 - 1.0, grip.1);
    hp.pointer_move(grip.0, grip.1);
    wait(&mut hp, Duration::from_millis(400))?;
    assert_eq!(hp.frame_cursor().as_deref(), Some("grab"));
    if let Some(file) = std::env::var_os("HOCUS_SHOT") {
        let (w, h, mut px) = hp.screenshot(0)?;
        for p in px.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        #[allow(clippy::cast_sign_loss)]
        image::save_buffer(&file, &px, w as u32, h as u32, image::ColorType::Rgba8)?;
    }
    Ok(())
}
