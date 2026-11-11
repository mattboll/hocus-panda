//! Dropping a window on a desktop's tile in the bar, headless (docs/design/
//! windows.md §12, bar.md §4.4; 8.5 B, issue #490): while a window is
//! moved over the tiles the bar comes in front at once, the tile is the
//! one told, the window shrinks to 30 %; let go on a tile it goes to that
//! desktop of that tile's screen at its place before the drag, the
//! screens keeping the desktops they show; on its own desktop's tile it
//! goes back; `Esc` puts it back too.
//!
//! A plain layer surface of the test's stands for the shell's desktops'
//! surface, and the tiles are said as the shell says them.

use crate::common;

use std::time::Duration;

use common::{
    BTN_LEFT, KEY_ESC, KEY_LEFTALT, Shows, WAIT, client, open, open_showing, rect_of, two_screens,
    wait, wait_for_rect,
};
use hocus_compositor::{
    Headless,
    model::{self, Rect},
};
use hocus_link::DeskTile;

const FIRST: &str = "HEADLESS-1";
const SECOND: &str = "HEADLESS-2";

/// The tiles' surface: 194 × 70, 600 px from the top, 8 from the right.
const SURFACE: (u32, u32) = (194, 70);

/// The two screens, the desktops' surface on the first's bar, its tiles
/// said: the first screen's four 30 px high, the second's 20 px under
/// them. Where the surface is.
fn start() -> anyhow::Result<(Headless, Rect)> {
    with_tiles(two_screens()?)
}

/// `hp`'s first screen's bar has the desktops' surface and its tiles, as
/// `start` says.
fn with_tiles(mut hp: Headless) -> anyhow::Result<(Headless, Rect)> {
    std::mem::forget(client(
        hp.socket(),
        Some(Shows::Layer {
            namespace: hocus_link::DESKTOPS_NAMESPACE,
            size: SURFACE,
            margin: (600, 8),
            colour: 0xff30_3040,
        }),
    )?);
    let placed = hp.dispatch_until(WAIT, |hp| {
        !hp.layer_rects(hocus_link::DESKTOPS_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(placed, "no desktops' surface");
    let surface = hp.layer_rects(hocus_link::DESKTOPS_NAMESPACE)[0].1;
    let mut tiles = Vec::new();
    for (output, y, height) in [(FIRST, 5, 30), (SECOND, 45, 20)] {
        for desktop in 0..4u8 {
            tiles.push(DeskTile {
                output: output.into(),
                desktop,
                rect: [11 + i32::from(desktop) * 45, y, 39, height],
            });
        }
    }
    hp.desk_tiles(FIRST, tiles);
    Ok((hp, surface))
}

/// The middle of `output`'s tile `desktop`, global.
fn tile(surface: Rect, output: &str, desktop: i32) -> (f64, f64) {
    let (y, height) = if output == FIRST { (5, 30) } else { (45, 20) };
    (
        f64::from(surface.x + 11 + desktop * 45) + 19.5,
        f64::from(surface.y + y) + f64::from(height) / 2.0,
    )
}

/// A window's desktop (from 0) and whether it shows.
fn where_is(hp: &Headless, app_id: &str) -> Option<(usize, bool)> {
    hp.all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some(app_id))
        .map(|w| (w.2, w.3))
}

/// The desktop each screen shows.
fn shown(hp: &Headless) -> Vec<usize> {
    hp.strips().into_iter().map(|(_, _, c)| c).collect()
}

/// Presses `Alt` and the button on `window`'s middle and moves to `to`
/// in steps, holding them.
fn drag(hp: &mut Headless, window: Rect, to: (f64, f64)) -> anyhow::Result<()> {
    let (x, y) = window.centre();
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
    for step in 1..=12 {
        let t = f64::from(step) / 12.0;
        hp.pointer_move(x + (to.0 - x) * t, y + (to.1 - y) * t);
    }
    wait(hp, Duration::from_millis(150))
}

fn release(hp: &mut Headless) -> anyhow::Result<()> {
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTALT, false);
    wait(hp, Duration::from_millis(100))
}

#[test]
fn let_go_on_the_second_tile_the_window_goes_to_desktop_2_at_its_place() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    hp.pointer_move(960.0, 540.0);
    let window = open(&mut hp, "test.dropped", (400, 300))?;
    drag(&mut hp, window, tile(surface, FIRST, 1))?;
    // Over the tile: told, the bar in front, the window small.
    assert_eq!(
        hp.drop_over(),
        Some((FIRST.to_owned(), FIRST.to_owned(), 1))
    );
    assert_eq!(hp.bar_in_front().as_deref(), Some(FIRST));
    assert!(
        (hp.moved_scale() - 0.3).abs() < 1e-9,
        "{}",
        hp.moved_scale()
    );
    release(&mut hp)?;
    // On desktop 2, out of sight, where it was before the drag; the
    // screens show what they showed; the bar back under the windows.
    assert_eq!(where_is(&hp, "test.dropped"), Some((1, false)));
    assert_eq!(shown(&hp), [0, 0]);
    assert_eq!(hp.drop_over(), None);
    assert_eq!(hp.bar_in_front(), None);
    common::chord(&mut hp, &[common::KEY_LEFTMETA, common::KEY_2]);
    wait_for_rect(&mut hp, "test.dropped", window)?;
    Ok(())
}

#[test]
fn let_go_on_its_own_desktops_tile_it_goes_back() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    hp.pointer_move(960.0, 540.0);
    let window = open(&mut hp, "test.back", (400, 300))?;
    drag(&mut hp, window, tile(surface, FIRST, 0))?;
    release(&mut hp)?;
    wait_for_rect(&mut hp, "test.back", window)?;
    assert_eq!(where_is(&hp, "test.back"), Some((0, true)));
    Ok(())
}

#[test]
fn let_go_on_the_other_screens_tile_it_goes_to_that_screen() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    let areas = hp.work_areas();
    hp.pointer_move(960.0, 540.0);
    let window = open(&mut hp, "test.across", (400, 300))?;
    // The second screen's first desktop, the one it shows: there, shown.
    drag(&mut hp, window, tile(surface, SECOND, 0))?;
    assert_eq!(
        hp.drop_over(),
        Some((FIRST.to_owned(), SECOND.to_owned(), 0))
    );
    release(&mut hp)?;
    let there = model::carried(window, areas[0], areas[1]);
    wait_for_rect(&mut hp, "test.across", there)?;
    assert_eq!(where_is(&hp, "test.across"), Some((0, true)));
    // Its third desktop: out of sight there.
    drag(&mut hp, there, tile(surface, SECOND, 2))?;
    release(&mut hp)?;
    assert_eq!(where_is(&hp, "test.across"), Some((2, false)));
    assert_eq!(shown(&hp), [0, 0]);
    Ok(())
}

#[test]
fn leaving_the_tiles_gives_back_its_size_and_esc_puts_it_back() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    hp.pointer_move(960.0, 540.0);
    let window = open(&mut hp, "test.esc", (400, 300))?;
    drag(&mut hp, window, tile(surface, FIRST, 2))?;
    assert!(hp.moved_scale() < 1.0);
    // Off the tiles, back in the screen's middle: full size, nothing told,
    // the bar still in front until the move ends.
    hp.pointer_move(700.0, 400.0);
    wait(&mut hp, Duration::from_millis(150))?;
    assert_eq!(hp.drop_over(), None);
    assert_eq!(hp.moved_scale(), 1.0);
    assert_eq!(hp.bar_in_front().as_deref(), Some(FIRST));
    assert_ne!(rect_of(&hp, "test.esc"), Some(window));
    // Esc: back where the drag began, the move over.
    hp.key(KEY_ESC, true);
    hp.key(KEY_ESC, false);
    wait_for_rect(&mut hp, "test.esc", window)?;
    assert_eq!(hp.bar_in_front(), None);
    release(&mut hp)?;
    assert_eq!(rect_of(&hp, "test.esc"), Some(window));
    assert_eq!(where_is(&hp, "test.esc"), Some((0, true)));
    Ok(())
}

/// Presses the button on `window`'s middle, no key held, and moves to
/// `to` in steps, the client seeing each: the client asks for the move
/// itself at the first motion, as an application dragged by its own
/// header does (#729).
fn client_drag(hp: &mut Headless, window: Rect, to: (f64, f64)) -> anyhow::Result<()> {
    let (x, y) = window.centre();
    hp.pointer_move(x, y);
    wait(hp, Duration::from_millis(30))?;
    hp.button(BTN_LEFT, true);
    wait(hp, Duration::from_millis(30))?;
    for step in 1..=12 {
        let t = f64::from(step) / 12.0;
        hp.pointer_move(x + (to.0 - x) * t, y + (to.1 - y) * t);
        wait(hp, Duration::from_millis(20))?;
    }
    wait(hp, Duration::from_millis(150))
}

/// A window the client moves itself (`xdg_toplevel.move`, its own header
/// dragged, #729), let go on a tile: there as when moved by `Alt`+drag,
/// the tile told, the bar in front, the window small on the way.
#[test]
fn a_window_moved_by_its_client_let_go_on_a_tile_goes_there() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    hp.pointer_move(960.0, 540.0);
    let (window, _client) = open_showing(
        &mut hp,
        "test.self",
        Shows::Moving {
            app_id: "test.self",
            size: (400, 300),
        },
    )?;
    client_drag(&mut hp, window, tile(surface, FIRST, 2))?;
    assert!(hp.moving().is_some(), "the client's move never began");
    assert_eq!(
        hp.drop_over(),
        Some((FIRST.to_owned(), FIRST.to_owned(), 2))
    );
    assert_eq!(hp.bar_in_front().as_deref(), Some(FIRST));
    assert!(
        (hp.moved_scale() - 0.3).abs() < 1e-9,
        "{}",
        hp.moved_scale()
    );
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(where_is(&hp, "test.self"), Some((2, false)));
    assert_eq!(shown(&hp), [0, 0]);
    assert_eq!(hp.drop_over(), None);
    assert_eq!(hp.bar_in_front(), None);
    Ok(())
}

/// An X11 window (XWayland) that moves itself as Chromium or GTK on X11
/// do from their own header: at a left press, `_NET_WM_MOVERESIZE` (move)
/// to the root window, the button held. Its connection lives as long as
/// XWayland does.
fn x11_moving(display: u32, class: &'static str) -> anyhow::Result<()> {
    use x11rb::{
        COPY_DEPTH_FROM_PARENT,
        connection::Connection as _,
        protocol::{
            Event as XEvent,
            xproto::{
                AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask,
                PropMode, WindowClass,
            },
        },
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
                &CreateWindowAux::new()
                    .background_pixel(0x003a_7bd5)
                    .event_mask(EventMask::BUTTON_PRESS),
            )?;
            let names = format!("{class}\0{class}\0");
            conn.change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                names.as_bytes(),
            )?;
            let moveresize = conn
                .intern_atom(false, b"_NET_WM_MOVERESIZE")?
                .reply()?
                .atom;
            conn.map_window(window)?;
            conn.flush()?;
            Ok((conn, root, window, moveresize))
        })();
        match made {
            Ok((conn, root, window, moveresize)) => {
                let _ = said.send(Ok(()));
                while let Ok(event) = conn.wait_for_event() {
                    let XEvent::ButtonPress(press) = event else {
                        continue;
                    };
                    // x_root, y_root, _NET_WM_MOVERESIZE_MOVE, the button,
                    // a normal application.
                    let data = [
                        press.root_x as u32,
                        press.root_y as u32,
                        8,
                        u32::from(press.detail),
                        1,
                    ];
                    let message = ClientMessageEvent::new(32, window, moveresize, data);
                    let _ = conn.ungrab_pointer(x11rb::CURRENT_TIME);
                    let _ = conn.send_event(
                        false,
                        root,
                        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                        message,
                    );
                    let _ = conn.flush();
                }
            }
            Err(err) => {
                let _ = said.send(Err(err));
            }
        }
    });
    heard.recv()??;
    Ok(())
}

/// An X11 window moving itself (`_NET_WM_MOVERESIZE`, XWayland, #729),
/// let go on a tile: there, as any window moved by the pointer.
#[test]
fn an_x11_window_moved_by_its_client_let_go_on_a_tile_goes_there() -> anyhow::Result<()> {
    let has = std::process::Command::new("sh")
        .args(["-c", "command -v Xwayland"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !has {
        common::missing("Xwayland");
    }
    let hp = Headless::start_with_xwayland(hocus_compositor::HeadlessConfig {
        outputs: vec!["1920x1080".parse()?, "2560x1440".parse()?],
        renderer: hocus_compositor::RendererChoice::Pixman,
        ..hocus_compositor::HeadlessConfig::default()
    })?;
    let (mut hp, surface) = with_tiles(hp)?;
    let ready = hp.dispatch_until(WAIT, |hp| hp.xdisplay().is_some())?;
    anyhow::ensure!(ready, "XWayland never got ready");
    let display = hp.xdisplay().ok_or_else(|| anyhow::anyhow!("no display"))?;
    hp.pointer_move(960.0, 540.0);
    x11_moving(display, "test.x11-self")?;
    let shown = hp.dispatch_until(WAIT, |hp| rect_of(hp, "test.x11-self").is_some())?;
    anyhow::ensure!(shown, "the X11 window never showed: {:?}", hp.windows());
    let window = rect_of(&hp, "test.x11-self").ok_or_else(|| anyhow::anyhow!("no window"))?;
    wait(&mut hp, Duration::from_millis(200))?;
    client_drag(&mut hp, window, tile(surface, FIRST, 3))?;
    assert!(hp.moving().is_some(), "the client's move never began");
    assert_eq!(
        hp.drop_over(),
        Some((FIRST.to_owned(), FIRST.to_owned(), 3))
    );
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(where_is(&hp, "test.x11-self"), Some((3, false)));
    assert_eq!(hp.drop_over(), None);
    Ok(())
}
