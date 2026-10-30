//! Windows on screen, headless (docs/design/windows.md §4–§10): snapping by
//! the keys and by dragging, resizing from the frame, a snapped pair's
//! shared edge resizing one and its handle both, fullscreen and back, minimising, menus that grab
//! the keyboard, a window opened in the background pulsing once, and no
//! frame drawn once nothing moves.

mod common;

use std::time::Duration;

use common::{
    BTN_LEFT, ClientHandle, Command, Event, KEY_DOWN, KEY_F4, KEY_LEFT, KEY_LEFTALT, KEY_LEFTMETA,
    KEY_LEFTSHIFT, KEY_Q, KEY_RIGHT, KEY_UP, Shows, WAIT, chord, compositor, open, open_showing,
    open_with, rect_of, wait, wait_for_rect,
};
use hocus_compositor::{
    Headless, HeadlessConfig, RendererChoice,
    model::{GAP, Mode, Rect, WindowId, Zone},
};
use smithay::input::pointer::CursorIcon;

/// The work area of the headless 1920×1080 output: 8 px in, and the full
/// bar's place kept on the right.
const AREA: Rect = Rect::new(8, 8, 1700, 1064);

#[test]
fn the_keys_snap_to_halves_quarters_maximise_restore_and_minimise() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let floating = open(&mut hp, "test.keys", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.keys", Zone::Left.rect(AREA))?;
    // Again: the other side.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.keys", Zone::Right.rect(AREA))?;
    // A quarter keeps the side it is on.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_UP]);
    wait_for_rect(&mut hp, "test.keys", Zone::TopRight.rect(AREA))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.keys", Zone::TopLeft.rect(AREA))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    wait_for_rect(&mut hp, "test.keys", AREA)?;
    // Restored where it floated, then minimised.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_DOWN]);
    wait_for_rect(&mut hp, "test.keys", floating)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_DOWN]);
    assert_eq!(hp.minimized(), 1);
    assert!(rect_of(&hp, "test.keys").is_none(), "not shown");
    Ok(())
}

#[test]
fn dragging_to_an_edge_snaps_once_the_preview_shows() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.drag", (400, 300))?;
    let (x, y) = (f64::from(start.x + 200), f64::from(start.y + 150));
    // Across the left edge and back, quickly: nothing.
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(1.0, y);
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(250))?;
    assert_eq!(rect_of(&hp, "test.drag"), Some(start));
    // Held at the edge past 80 ms: the left half.
    hp.button(BTN_LEFT, true);
    hp.pointer_move(1.0, y);
    wait(&mut hp, Duration::from_millis(120))?;
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    wait_for_rect(&mut hp, "test.drag", Zone::Left.rect(AREA))?;
    let id = hp.windows()[0].id;
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::Left));
    // A top corner: a quarter.
    hp.key(KEY_LEFTMETA, true);
    hp.pointer_move(400.0, 500.0);
    hp.button(BTN_LEFT, true);
    // Pulled away, it floats again at its size, under the pointer.
    hp.pointer_move(420.0, 520.0);
    // Held at 392 of its 846 px across and 492 of its 1064 px down.
    wait_for_rect(&mut hp, "test.drag", Rect::new(235, 381, 400, 300))?;
    hp.pointer_move(1700.0, 2.0);
    wait(&mut hp, Duration::from_millis(120))?;
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    wait_for_rect(&mut hp, "test.drag", Zone::TopRight.rect(AREA))?;
    Ok(())
}

#[test]
fn the_frame_resizes_from_a_corner_and_an_edge() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.resize", (400, 300))?;
    // The band outside the bottom-right corner.
    let (x, y) = (f64::from(start.right() + 3), f64::from(start.bottom() + 3));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 50.0, y + 40.0);
    hp.button(BTN_LEFT, false);
    let grown = Rect::new(start.x, start.y, 450, 340);
    wait_for_rect(&mut hp, "test.resize", grown)?;
    // The left edge: the right one stays.
    let (x, y) = (f64::from(grown.x - 4), f64::from(grown.y + 100));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 30.0, y);
    hp.button(BTN_LEFT, false);
    wait_for_rect(
        &mut hp,
        "test.resize",
        Rect::new(grown.x + 30, grown.y, 420, 340),
    )?;
    Ok(())
}

/// Two windows snapped left and right.
fn snapped_pair(hp: &mut hocus_compositor::Headless) -> anyhow::Result<(WindowId, WindowId)> {
    open(hp, "test.left", (400, 300))?;
    chord(hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(hp, "test.left", Zone::Left.rect(AREA))?;
    open(hp, "test.right", (400, 300))?;
    chord(hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait_for_rect(hp, "test.right", Zone::Right.rect(AREA))?;
    let id = |app: &str| {
        hp.windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app))
            .map(|w| w.id)
    };
    Ok((
        id("test.left").expect("left"),
        id("test.right").expect("right"),
    ))
}

/// The middle of the gap between the left and right halves.
fn seam_centre() -> (f64, f64) {
    let left = Zone::Left.rect(AREA);
    (
        f64::from(left.right() + GAP / 2),
        f64::from(left.y) + f64::from(left.height) / 2.0,
    )
}

#[test]
fn the_shared_edge_resizes_only_the_window_grabbed_which_floats() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (left_id, right_id) = snapped_pair(&mut hp)?;
    // In the gap, nearer the left window, away from the handle: its edge.
    let (cx, _) = seam_centre();
    let (x, y) = (cx - 3.0, 300.0);
    hp.pointer_move(x, y);
    assert_eq!(hp.state().frame_cursor, Some(CursorIcon::EwResize));
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 100.0, y);
    hp.button(BTN_LEFT, false);
    let left = Zone::Left.rect(AREA);
    wait_for_rect(
        &mut hp,
        "test.left",
        Rect::new(left.x, left.y, left.width + 100, left.height),
    )?;
    assert_eq!(hp.mode(left_id), Mode::Floating, "it left its zone");
    assert_eq!(rect_of(&hp, "test.right"), Some(Zone::Right.rect(AREA)));
    assert_eq!(
        hp.mode(right_id),
        Mode::Snapped(Zone::Right),
        "the other stays"
    );
    Ok(())
}

#[test]
fn a_half_alone_resized_by_its_edge_floats() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    // Its inner edge, with no window across it: no seam there.
    open(&mut hp, "test.half", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    let left = Zone::Left.rect(AREA);
    wait_for_rect(&mut hp, "test.half", left)?;
    let id = hp.windows()[0].id;
    let (x, y) = (f64::from(left.right() + 3), 300.0);
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x - 100.0, y);
    hp.button(BTN_LEFT, false);
    wait_for_rect(
        &mut hp,
        "test.half",
        Rect::new(left.x, left.y, left.width - 100, left.height),
    )?;
    assert_eq!(hp.mode(id), Mode::Floating, "it left its zone");
    Ok(())
}

#[test]
fn a_snapped_window_resized_by_an_outer_edge_floats_and_its_pair_stays() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (left_id, right_id) = snapped_pair(&mut hp)?;
    // The left half's bottom edge.
    let left = Zone::Left.rect(AREA);
    let (x, y) = (300.0, f64::from(left.bottom() + 3));
    hp.pointer_move(x, y);
    assert_eq!(hp.state().frame_cursor, Some(CursorIcon::NsResize));
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x, y - 100.0);
    hp.button(BTN_LEFT, false);
    wait_for_rect(
        &mut hp,
        "test.left",
        Rect::new(left.x, left.y, left.width, left.height - 100),
    )?;
    assert_eq!(hp.mode(left_id), Mode::Floating, "it left its zone");
    assert_eq!(rect_of(&hp, "test.right"), Some(Zone::Right.rect(AREA)));
    assert_eq!(hp.mode(right_id), Mode::Snapped(Zone::Right));

    // A quarter, by its outer corner.
    hp.pointer_move(1300.0, 500.0);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_UP]);
    let quarter = Zone::TopRight.rect(AREA);
    wait_for_rect(&mut hp, "test.right", quarter)?;
    let (x, y) = (f64::from(quarter.right() + 3), f64::from(quarter.y - 3));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x - 50.0, y + 40.0);
    hp.button(BTN_LEFT, false);
    wait_for_rect(
        &mut hp,
        "test.right",
        Rect::new(
            quarter.x,
            quarter.y + 40,
            quarter.width - 50,
            quarter.height - 40,
        ),
    )?;
    assert_eq!(hp.mode(right_id), Mode::Floating, "it left its zone");
    Ok(())
}

#[test]
fn the_seams_handle_resizes_both_windows() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (left_id, right_id) = snapped_pair(&mut hp)?;
    // A few pixels off the pill still take it.
    let (x, y) = seam_centre();
    let (x, y) = (x + 5.0, y + 20.0);
    hp.pointer_move(x, y);
    assert_eq!(hp.state().frame_cursor, Some(CursorIcon::EwResize));
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 100.0, y);
    hp.button(BTN_LEFT, false);
    let left = Zone::Left.rect(AREA);
    let wider = Rect::new(left.x, left.y, left.width + 100, left.height);
    wait_for_rect(&mut hp, "test.left", wider)?;
    let right = Zone::Right.rect(AREA);
    wait_for_rect(
        &mut hp,
        "test.right",
        Rect::new(
            wider.right() + GAP,
            right.y,
            right.right() - wider.right() - GAP,
            right.height,
        ),
    )?;
    assert_eq!(hp.mode(left_id), Mode::Snapped(Zone::Left));
    assert_eq!(hp.mode(right_id), Mode::Snapped(Zone::Right));
    Ok(())
}

#[test]
fn the_handle_shows_near_a_snapped_pairs_seam_only_and_then_rests() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    // One snapped window alone: no seam, no handle, its own edge.
    open(&mut hp, "test.left", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.left", Zone::Left.rect(AREA))?;
    let (x, y) = seam_centre();
    hp.pointer_move(x, y);
    assert!(hp.state().seam_handle.is_none(), "no pair, no handle");
    // With its pair: near the seam, anywhere along it, the handle shows.
    open(&mut hp, "test.right", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait_for_rect(&mut hp, "test.right", Zone::Right.rect(AREA))?;
    hp.pointer_move(x + 10.0, 200.0);
    let (seam, _) = hp.state().seam_handle.expect("the handle shows");
    assert!(seam.vertical);
    assert_eq!(
        hp.state().frame_cursor,
        None,
        "over the window: the client's"
    );
    // Away: it fades, and nothing more is drawn once it has.
    hp.pointer_move(300.0, 300.0);
    wait(&mut hp, Duration::from_millis(700))?;
    assert!(hp.state().seam_handle.is_none(), "faded");
    let frames = hp.frames();
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames(), frames, "no frame at rest");
    Ok(())
}

#[test]
fn fullscreen_covers_the_output_and_comes_back() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (start, client) = open_with(&mut hp, "test.full", (400, 300), false)?;
    client.commands.send(Command::Fullscreen(true))?;
    wait_for_rect(&mut hp, "test.full", Rect::new(0, 0, 1920, 1080))?;
    let id = hp.windows()[0].id;
    assert_eq!(hp.mode(id), Mode::Fullscreen);
    client.commands.send(Command::Fullscreen(false))?;
    wait_for_rect(&mut hp, "test.full", start)?;
    assert_eq!(hp.mode(id), Mode::Floating);
    Ok(())
}

#[test]
fn a_window_asking_for_fullscreen_before_its_first_frame_opens_fullscreen() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let shows = Shows::Asking {
        app_id: "test.asks",
        size: (400, 300),
        fullscreen: true,
    };
    let (rect, client) = open_showing(&mut hp, "test.asks", shows)?;
    // Its first frame already covers the screen.
    assert_eq!(rect, Rect::new(0, 0, 1920, 1080));
    let id = hp.windows()[0].id;
    assert_eq!(hp.mode(id), Mode::Fullscreen);
    // Back, it floats in the middle of the work area.
    client.commands.send(Command::Fullscreen(false))?;
    let (w, h) = (AREA.width * 2 / 3, AREA.height * 2 / 3);
    let floating = Rect::new(
        AREA.x + (AREA.width - w) / 2,
        AREA.y + (AREA.height - h) / 2,
        w,
        h,
    );
    wait_for_rect(&mut hp, "test.asks", floating)?;
    assert_eq!(hp.mode(id), Mode::Floating);
    Ok(())
}

#[test]
fn a_window_asking_to_be_maximised_before_its_first_frame_opens_maximised() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let shows = Shows::Asking {
        app_id: "test.asks",
        size: (400, 300),
        fullscreen: false,
    };
    let (rect, _client) = open_showing(&mut hp, "test.asks", shows)?;
    assert_eq!(rect, AREA);
    let id = hp.windows()[0].id;
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::Maximized));
    Ok(())
}

/// Presses `keys`, and tells whether `client`'s window was asked to close
/// within `time`.
fn close_asked(
    hp: &mut Headless,
    client: &ClientHandle,
    keys: &[u32],
    time: Duration,
) -> anyhow::Result<bool> {
    chord(hp, keys);
    let mut asked = false;
    hp.dispatch_until(time, |_| {
        asked |= client.events.try_recv().ok() == Some(Event::CloseRequested);
        asked
    })?;
    Ok(asked)
}

#[test]
fn super_q_and_alt_f4_ask_the_focused_window_to_close() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (_, client) = open_with(&mut hp, "test.close", (400, 300), false)?;
    assert!(hp.windows()[0].focused);
    let quiet = Duration::from_millis(300);
    assert!(
        !close_asked(&mut hp, &client, &[KEY_F4], quiet)?,
        "F4 alone"
    );
    assert!(
        !close_asked(&mut hp, &client, &[KEY_LEFTMETA, KEY_F4], quiet)?,
        "Super+F4"
    );
    assert!(
        close_asked(&mut hp, &client, &[KEY_LEFTALT, KEY_F4], WAIT)?,
        "Alt+F4 did not ask"
    );
    // The application keeps its window, as one asking to save would.
    assert_eq!(hp.windows().len(), 1);
    assert!(
        close_asked(&mut hp, &client, &[KEY_LEFTMETA, KEY_Q], WAIT)?,
        "Super+Q did not ask"
    );
    Ok(())
}

#[test]
fn a_menu_takes_the_keyboard_and_a_press_elsewhere_closes_it() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (rect, client) = open_with(&mut hp, "test.menu", (400, 300), true)?;
    hp.pointer_move(f64::from(rect.x + 20), f64::from(rect.y + 20));
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let mut shown = false;
    hp.dispatch_until(WAIT, |hp| {
        shown |= client.events.try_recv().ok() == Some(Event::PopupShown);
        shown && hp.keyboard_on_popup()
    })?;
    assert!(shown && hp.keyboard_on_popup(), "the menu has the keyboard");
    assert!(!hp.windows()[0].keyboard);
    // A press where nothing of its application is.
    hp.pointer_move(2.0, 2.0);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let mut done = false;
    hp.dispatch_until(WAIT, |hp| {
        done |= client.events.try_recv().ok() == Some(Event::PopupDone);
        done && !hp.keyboard_on_popup()
    })?;
    assert!(done, "the menu was told to close");
    let window = &hp.windows()[0];
    assert!(window.keyboard && window.focused, "the keyboard is back");
    Ok(())
}

#[test]
fn nothing_is_drawn_once_the_motions_end() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.rest", (400, 300))?;
    // The opening (220 ms) and the focus rim (120 ms) run, then stop.
    wait(&mut hp, Duration::from_millis(500))?;
    let frames = hp.frames();
    assert!(frames > 2, "the opening was drawn: {frames} frames");
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames(), frames, "no frame at rest");
    Ok(())
}

#[test]
fn super_and_right_drag_resizes_from_the_nearest_corner() -> anyhow::Result<()> {
    const BTN_RIGHT: u32 = 0x111;
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.corner", (400, 300))?;
    // Nearer the top-left corner: it moves, the bottom-right one stays.
    let (x, y) = (f64::from(start.x + 50), f64::from(start.y + 40));
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_RIGHT, true);
    hp.pointer_move(x - 40.0, y - 30.0);
    hp.button(BTN_RIGHT, false);
    hp.key(KEY_LEFTMETA, false);
    wait_for_rect(
        &mut hp,
        "test.corner",
        Rect::new(start.x - 40, start.y - 30, 440, 330),
    )?;
    Ok(())
}

#[test]
fn a_terminal_resized_by_its_left_edge_keeps_its_cells_and_stays_where_it_is_moved()
-> anyhow::Result<()> {
    let mut hp = compositor()?;
    let shows = Shows::Cells {
        app_id: "test.cells",
        size: (400, 300),
        cell: 10,
    };
    let (start, _client) = open_showing(&mut hp, "test.cells", shows)?;
    // 33 px wider asked, 30 taken: its right edge stays.
    let (x, y) = (f64::from(start.x - 3), f64::from(start.y + 100));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x - 33.0, y);
    hp.button(BTN_LEFT, false);
    let wider = Rect::new(start.x - 30, start.y, 430, 300);
    wait_for_rect(&mut hp, "test.cells", wider)?;
    // Moved, it stays there when it draws again (another window takes
    // the focus).
    let (cx, cy) = wider.centre();
    hp.pointer_move(cx, cy);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(cx + 100.0, cy + 50.0);
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    let moved = Rect::new(wider.x + 100, wider.y + 50, 430, 300);
    wait_for_rect(&mut hp, "test.cells", moved)?;
    open(&mut hp, "test.other", (200, 150))?;
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(rect_of(&hp, "test.cells"), Some(moved));
    Ok(())
}

// --- A window opening without the keyboard (§4; issue #495) -------------------

/// Run by the test below as a process of its own, given the compositor's
/// socket in `HOCUS_TEST_FOREIGN`: a window of another program, until the
/// compositor goes. Run alone, it does nothing.
#[test]
fn a_window_of_another_program() -> anyhow::Result<()> {
    let Some(socket) = std::env::var_os("HOCUS_TEST_FOREIGN") else {
        return Ok(());
    };
    let shows = Shows::Window {
        app_id: "test.foreign",
        size: (500, 400),
        popup: false,
    };
    let client = common::client(std::path::Path::new(&socket), Some(shows))?;
    // Its events end with its connection, as the compositor goes.
    while client.events.recv().is_ok() {}
    Ok(())
}

/// The other program, stopped by its own PID when the test ends.
struct Foreign(std::process::Child);

impl Drop for Foreign {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the other program's window, and waits until it has the keyboard.
fn foreign_window(hp: &mut Headless) -> anyhow::Result<Foreign> {
    let child = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "a_window_of_another_program", "--nocapture"])
        .env("HOCUS_TEST_FOREIGN", hp.socket())
        .stdout(std::process::Stdio::null())
        .spawn()?;
    let foreign = Foreign(child);
    let shown = hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some("test.foreign") && w.keyboard)
    })?;
    anyhow::ensure!(shown, "the other program's window: {:?}", hp.windows());
    Ok(foreign)
}

/// A window of this program opening while the user works in another's:
/// it opens behind without the keyboard, and its rim pulses once as a
/// window gone to from an agent's row does (bar.md §8): on a GPU, the
/// 2 px rim and the ring growing to 14 px in 600 ms; with reduced motion,
/// or on the CPU, the rim lights and fades without its ring. Then nothing
/// is drawn.
#[test]
fn a_window_opened_in_the_background_pulses_once_then_nothing_draws() -> anyhow::Result<()> {
    let machines = [
        common::render_node().map(|node| (Some(node), false)),
        common::render_node().map(|node| (Some(node), true)),
        Some((None, false)),
    ];
    for (gpu, reduced_motion) in machines.into_iter().flatten() {
        let mut config = hocus_compositor::Config::default();
        config.appearance.reduced_motion = reduced_motion;
        let mut hp = Headless::start_with(
            HeadlessConfig {
                renderer: if gpu.is_some() {
                    RendererChoice::Gles
                } else {
                    RendererChoice::Pixman
                },
                render_node: gpu,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        let ring = hp.renderer() == "gles" && !reduced_motion;
        let machine = format!("{}, reduced motion {reduced_motion}", hp.renderer());
        let _foreign = foreign_window(&mut hp)?;
        wait(&mut hp, Duration::from_millis(500))?;
        let (_, _client) = open_with(&mut hp, "test.behind", (400, 300), false)?;
        let start = std::time::Instant::now();
        let mut frames = hp.frames();
        let mut samples = Vec::new();
        while start.elapsed() < Duration::from_millis(1200) {
            hp.dispatch(Duration::from_millis(2))?;
            if hp.frames() != frames {
                frames = hp.frames();
                samples.push((start.elapsed(), hp.pulse("test.behind")));
            }
        }
        // Behind, the keyboard where it was.
        let windows = hp.windows();
        let at = |app: &str| {
            windows
                .iter()
                .position(|w| w.app_id.as_deref() == Some(app))
        };
        assert!(
            at("test.behind") < at("test.foreign"),
            "{machine}: {windows:?}"
        );
        assert!(
            windows
                .iter()
                .any(|w| w.app_id.as_deref() == Some("test.foreign") && w.keyboard),
            "{machine}: {windows:?}"
        );
        // Its rim lights, once: up to its peak, then only down.
        let rims: Vec<f32> = samples
            .iter()
            .map(|(_, p)| p.map_or(0.0, |(rim, _)| rim))
            .collect();
        let peak = rims.iter().copied().fold(0.0, f32::max);
        assert!(peak > 0.8, "{machine}: {samples:?}");
        let top = rims.iter().position(|r| *r == peak).unwrap_or_default();
        assert!(
            rims[top..].windows(2).all(|w| w[1] <= w[0]),
            "{machine}, more than once: {samples:?}"
        );
        let widest = samples
            .iter()
            .filter_map(|(_, p)| p.and_then(|(_, ring)| ring))
            .fold(0.0, f64::max);
        if ring {
            assert!(widest > 8.0 && widest <= 14.0, "{machine}: {samples:?}");
        } else {
            assert_eq!(widest, 0.0, "{machine}: {samples:?}");
        }
        // Over by its 600 ms, and nothing drawn after.
        let end = samples
            .iter()
            .rfind(|(_, p)| p.is_some())
            .map_or(Duration::ZERO, |(at, _)| *at);
        assert!(end < Duration::from_millis(700), "{machine}: {samples:?}");
        let frames = hp.frames();
        wait(&mut hp, Duration::from_millis(500))?;
        assert_eq!(hp.frames(), frames, "{machine}: drawn at rest");
    }
    Ok(())
}
