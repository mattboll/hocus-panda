//! The overview, headless (docs/design/windows.md §12b; 8.5 A, issue
//! #490): `Super+W` and four fingers down open it on every screen, each
//! with its desktops' thumbnails and its windows laid out without overlap;
//! a window dragged onto a thumbnail goes to that desktop, onto the other
//! screen to that screen, elsewhere it springs back; a click on a
//! thumbnail shows that desktop and keeps it open; a click on a window
//! focuses it and closes it; `Esc` closes it, nothing moved; the keys;
//! the agents' badges and the empty desktops' sleeping pandas; at rest it
//! draws nothing. With `HOCUS_SHOTS` naming a folder, the tests save the
//! screens as `overview-*.png`.

mod common;

use std::{io::Write, os::unix::net::UnixStream, path::Path, time::Duration};

use common::{
    BTN_LEFT, KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, WAIT, chord, open, two_screens, wait,
};
use hocus_compositor::{Headless, model::Rect};

const FIRST: &str = "HEADLESS-1";
const SECOND: &str = "HEADLESS-2";
const KEY_W: u32 = 17;
const KEY_ENTER: u32 = 28;
const KEY_RIGHT: u32 = 106;
const KEY_3: u32 = 4;

/// Two screens; two windows on the first, one on the second.
fn start() -> anyhow::Result<Headless> {
    let mut hp = two_screens()?;
    hp.pointer_move(960.0, 540.0);
    open(&mut hp, "test.left", (700, 500))?;
    open(&mut hp, "test.right", (600, 420))?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    open(&mut hp, "test.wide", (1200, 800))?;
    hp.pointer_move(960.0, 540.0);
    Ok(hp)
}

fn super_w(hp: &mut Headless) {
    chord(hp, &[KEY_LEFTMETA, KEY_W]);
}

/// Opens the overview and lets its windows glide in.
fn opened(hp: &mut Headless) -> anyhow::Result<()> {
    super_w(hp);
    anyhow::ensure!(hp.overview() == Some(true), "not open");
    wait(hp, Duration::from_millis(300))
}

/// Waits until the overview is gone.
fn closed(hp: &mut Headless) -> anyhow::Result<()> {
    let gone = hp.dispatch_until(WAIT, |hp| hp.overview().is_none())?;
    anyhow::ensure!(gone, "the overview stays");
    Ok(())
}

/// Where the overview lays out `app_id`: its middle.
fn slot(hp: &Headless, app_id: &str) -> anyhow::Result<(f64, f64)> {
    hp.overview_windows()
        .into_iter()
        .find(|(app, _)| app.as_deref() == Some(app_id))
        .map(|(_, (x, y, w, h))| (x + w / 2.0, y + h / 2.0))
        .ok_or_else(|| anyhow::anyhow!("{app_id} not laid out: {:?}", hp.overview_windows()))
}

/// The middle of `output`'s thumbnail `desktop` (from 0).
fn thumb(hp: &Headless, output: &str, desktop: usize) -> anyhow::Result<(f64, f64)> {
    hp.overview_thumbs()
        .into_iter()
        .find(|(o, d, _)| o == output && *d == desktop)
        .map(|(_, _, at)| at)
        .ok_or_else(|| anyhow::anyhow!("no thumbnail {desktop} on {output}"))
}

fn click(hp: &mut Headless, (x, y): (f64, f64)) -> anyhow::Result<()> {
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    wait(hp, Duration::from_millis(50))
}

/// Presses on `from`, moves to `to` in steps, holding the button.
fn drag(hp: &mut Headless, from: (f64, f64), to: (f64, f64)) -> anyhow::Result<()> {
    hp.pointer_move(from.0, from.1);
    hp.button(BTN_LEFT, true);
    for step in 1..=12 {
        let t = f64::from(step) / 12.0;
        hp.pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
    }
    wait(hp, Duration::from_millis(150))
}

fn release(hp: &mut Headless) -> anyhow::Result<()> {
    hp.button(BTN_LEFT, false);
    wait(hp, Duration::from_millis(100))
}

/// A window's screen's name, its desktop (from 0), and whether it shows.
fn where_is(hp: &Headless, app_id: &str) -> Option<(String, usize, bool)> {
    hp.placements()
        .into_iter()
        .find(|p| p.app_id.as_deref() == Some(app_id))
        .and_then(|p| Some((p.output?, p.desktop, p.shown)))
}

fn focused(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.focused)
        .and_then(|w| w.app_id)
}

/// The desktop each screen shows.
fn shown(hp: &Headless) -> Vec<usize> {
    hp.strips().into_iter().map(|(_, _, c)| c).collect()
}

/// With `HOCUS_SHOTS` naming a folder: both screens side by side, at
/// rest, as `overview-WHAT.png`.
fn shot(hp: &mut Headless, what: &str) -> anyhow::Result<()> {
    let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
        return Ok(());
    };
    if hp.overview_badges().iter().any(|b| b.1 == "waiting") {
        // A waiting agent's panda waves on: a frame of its own.
        hp.hold_clock();
        hp.step(Duration::from_millis(500))?;
    } else {
        common::at_rest(hp)?;
    }
    let screens = [hp.screenshot(0)?, hp.screenshot(1)?];
    let width: i32 = screens.iter().map(|s| s.0).sum();
    let height = screens.iter().map(|s| s.1).max().unwrap_or(0);
    let mut image =
        image::RgbaImage::from_pixel(width as u32, height as u32, image::Rgba([14, 15, 18, 255]));
    let mut x0 = 0;
    for (w, h, bytes) in &screens {
        for y in 0..*h {
            for x in 0..*w {
                let at = usize::try_from(y * w + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(
                    (x0 + x) as u32,
                    y as u32,
                    image::Rgba([p[2], p[1], p[0], 255]),
                );
            }
        }
        x0 += w;
    }
    image.save(Path::new(&folder).join(format!("overview-{what}.png")))?;
    Ok(())
}

fn overlaps(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

#[test]
fn super_w_opens_it_on_every_screen_each_with_its_desktops_and_windows() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    // Four thumbnails on each screen.
    let thumbs = hp.overview_thumbs();
    for output in [FIRST, SECOND] {
        assert_eq!(
            thumbs.iter().filter(|t| t.0 == output).count(),
            4,
            "{thumbs:?}"
        );
    }
    // Every screen's windows, none over another, each on its screen.
    let laid = hp.overview_windows();
    assert_eq!(laid.len(), 3, "{laid:?}");
    for (i, (_, a)) in laid.iter().enumerate() {
        for (_, b) in &laid[i + 1..] {
            assert!(!overlaps(*a, *b), "{a:?} over {b:?}");
        }
    }
    let on_second = |app: &str| {
        laid.iter()
            .find(|(a, _)| a.as_deref() == Some(app))
            .is_some_and(|(_, r)| r.0 >= 1920.0)
    };
    assert!(on_second("test.wide") && !on_second("test.left") && !on_second("test.right"));
    shot(&mut hp, "two-screens")?;
    // Again: it closes.
    super_w(&mut hp);
    assert_eq!(hp.overview(), Some(false));
    closed(&mut hp)?;
    Ok(())
}

#[test]
fn esc_closes_it_and_nothing_moved() -> anyhow::Result<()> {
    let mut hp = start()?;
    let before: Vec<(Option<String>, Rect)> = hp
        .windows()
        .into_iter()
        .map(|w| (w.app_id, w.rect))
        .collect();
    let focus = focused(&hp);
    opened(&mut hp)?;
    // The keys are the overview's: a client sees none.
    chord(&mut hp, &[KEY_ESC]);
    closed(&mut hp)?;
    let after: Vec<(Option<String>, Rect)> = hp
        .windows()
        .into_iter()
        .map(|w| (w.app_id, w.rect))
        .collect();
    assert_eq!(before, after);
    assert_eq!(focused(&hp), focus);
    assert_eq!(shown(&hp), [0, 0]);
    Ok(())
}

#[test]
fn a_click_on_a_window_focuses_it_and_closes_the_overview() -> anyhow::Result<()> {
    let mut hp = start()?;
    assert_eq!(focused(&hp).as_deref(), Some("test.wide"));
    opened(&mut hp)?;
    let at = slot(&hp, "test.left")?;
    click(&mut hp, at)?;
    assert_eq!(hp.overview(), Some(false));
    closed(&mut hp)?;
    assert_eq!(focused(&hp).as_deref(), Some("test.left"));
    Ok(())
}

#[test]
fn a_click_on_the_second_thumbnail_shows_desktop_2_and_keeps_it_open() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let at = thumb(&hp, FIRST, 1)?;
    click(&mut hp, at)?;
    assert_eq!(hp.overview(), Some(true), "it stays open");
    assert_eq!(shown(&hp), [1, 0]);
    // Desktop 2 is empty: only the other screen's window is laid out.
    wait(&mut hp, Duration::from_millis(400))?;
    let laid: Vec<Option<String>> = hp.overview_windows().into_iter().map(|w| w.0).collect();
    assert_eq!(laid, [Some("test.wide".to_owned())]);
    // Closed, the screen stays on it.
    chord(&mut hp, &[KEY_ESC]);
    closed(&mut hp)?;
    assert_eq!(shown(&hp), [1, 0]);
    Ok(())
}

#[test]
fn a_window_dragged_onto_thumbnail_3_goes_to_desktop_3() -> anyhow::Result<()> {
    let mut hp = start()?;
    let rect = common::rect_of(&hp, "test.left").ok_or_else(|| anyhow::anyhow!("no window"))?;
    opened(&mut hp)?;
    let from = slot(&hp, "test.left")?;
    let to = thumb(&hp, FIRST, 2)?;
    drag(&mut hp, from, to)?;
    // Lifted, over the third thumbnail.
    assert_eq!(
        hp.overview_drag(),
        Some((Some("test.left".to_owned()), Some((FIRST.to_owned(), 2))))
    );
    shot(&mut hp, "dragging")?;
    release(&mut hp)?;
    assert_eq!(
        where_is(&hp, "test.left"),
        Some((FIRST.to_owned(), 2, false))
    );
    // The screens show what they showed; the overview stays open.
    assert_eq!(shown(&hp), [0, 0]);
    assert_eq!(hp.overview(), Some(true));
    let laid: Vec<Option<String>> = hp.overview_windows().into_iter().map(|w| w.0).collect();
    assert!(!laid.contains(&Some("test.left".to_owned())), "{laid:?}");
    // On desktop 3, at its place.
    chord(&mut hp, &[KEY_ESC]);
    closed(&mut hp)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    common::wait_for_rect(&mut hp, "test.left", rect)?;
    Ok(())
}

#[test]
fn a_window_dragged_onto_the_other_screen_goes_there() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let from = slot(&hp, "test.right")?;
    // The other screen's empty part, under its window.
    drag(&mut hp, from, (1920.0 + 300.0, 1300.0))?;
    release(&mut hp)?;
    assert_eq!(
        where_is(&hp, "test.right"),
        Some((SECOND.to_owned(), 0, true))
    );
    // Onto the other screen's fourth thumbnail: its desktop 4.
    let from = slot(&hp, "test.left")?;
    let to = thumb(&hp, SECOND, 3)?;
    drag(&mut hp, from, to)?;
    release(&mut hp)?;
    assert_eq!(
        where_is(&hp, "test.left"),
        Some((SECOND.to_owned(), 3, false))
    );
    assert_eq!(shown(&hp), [0, 0]);
    Ok(())
}

#[test]
fn a_window_let_go_elsewhere_springs_back() -> anyhow::Result<()> {
    let mut hp = start()?;
    let rect = common::rect_of(&hp, "test.left").ok_or_else(|| anyhow::anyhow!("no window"))?;
    opened(&mut hp)?;
    let from = slot(&hp, "test.left")?;
    // Its own screen's empty bottom.
    drag(&mut hp, from, (200.0, 1040.0))?;
    release(&mut hp)?;
    assert_eq!(
        where_is(&hp, "test.left"),
        Some((FIRST.to_owned(), 0, true))
    );
    assert_eq!(hp.overview(), Some(true));
    wait(&mut hp, Duration::from_millis(300))?;
    assert!((slot(&hp, "test.left")?.0 - from.0).abs() < 1.0);
    // Let go on its own desktop's thumbnail: the same.
    let to = thumb(&hp, FIRST, 0)?;
    drag(&mut hp, from, to)?;
    release(&mut hp)?;
    assert_eq!(
        where_is(&hp, "test.left"),
        Some((FIRST.to_owned(), 0, true))
    );
    chord(&mut hp, &[KEY_ESC]);
    closed(&mut hp)?;
    assert_eq!(common::rect_of(&hp, "test.left"), Some(rect));
    Ok(())
}

#[test]
fn four_fingers_down_open_it_and_again_close_it() -> anyhow::Result<()> {
    let mut hp = start()?;
    let down = [(0.0, 8.0); 8];
    hp.swipe(4, &down, 10);
    assert_eq!(hp.overview(), Some(true));
    // The swipe was the desktop's, from its start.
    assert_eq!(hp.swipes_given(), (0, 0));
    wait(&mut hp, Duration::from_millis(300))?;
    hp.swipe(4, &down, 10);
    assert_eq!(hp.overview(), Some(false));
    closed(&mut hp)?;
    // Up is not the overview's.
    hp.swipe(4, &[(0.0, -8.0); 8], 10);
    assert_eq!(hp.overview(), None);
    Ok(())
}

#[test]
fn the_arrows_select_enter_focuses_super_shift_n_sends() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    // The first arrow shows the ring on the focused window; right from
    // the first screen's right window: the other screen's.
    chord(&mut hp, &[KEY_RIGHT]);
    assert_eq!(hp.overview_selected().as_deref(), Some("test.wide"));
    // Back left, onto the first screen.
    chord(&mut hp, &[common::KEY_LEFT]);
    assert_eq!(hp.overview_selected().as_deref(), Some("test.right"));
    // Super+Shift+3: to desktop 3, staying open.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_3]);
    assert_eq!(
        where_is(&hp, "test.right"),
        Some((FIRST.to_owned(), 2, false))
    );
    assert_eq!(hp.overview(), Some(true));
    // The selection went on to another window; Enter focuses it.
    let selected = hp.overview_selected();
    assert!(selected.is_some() && selected.as_deref() != Some("test.right"));
    chord(&mut hp, &[KEY_ENTER]);
    closed(&mut hp)?;
    assert_eq!(focused(&hp), selected);
    Ok(())
}

#[test]
fn a_window_hosting_waiting_agents_has_the_badge_and_empty_desktops_sleep() -> anyhow::Result<()> {
    let mut hp = start()?;
    // The agents' window: this process's last, as a terminal's is its
    // agents' ancestor.
    open(&mut hp, "test.host", (800, 500))?;
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?
        .to_path_buf();
    let pid = std::process::id();
    for (name, state) in [
        ("migration", "waiting"),
        ("payments", "waiting"),
        ("site", "working"),
    ] {
        let mut stream = UnixStream::connect(&socket)?;
        writeln!(stream, "agent command {name} {state} {pid} {name}")?;
    }
    let until = std::time::Instant::now() + WAIT;
    while hp.agents().len() < 3 && std::time::Instant::now() < until {
        hp.dispatch(Duration::from_millis(10))?;
    }
    anyhow::ensure!(hp.agents().len() == 3, "the agents: {:?}", hp.agents());
    opened(&mut hp)?;
    assert_eq!(
        hp.overview_badges(),
        [(Some("test.host".to_owned()), "waiting".to_owned(), 2)]
    );
    // Desktops 2 to 4 of each screen are empty.
    let mut asleep = hp.overview_asleep();
    asleep.sort();
    let expected: Vec<(String, usize)> = [FIRST, SECOND]
        .iter()
        .flat_map(|o| (1..4).map(move |d| ((*o).to_owned(), d)))
        .collect();
    assert_eq!(asleep, expected);
    // While one waits, its panda waves on: the overview moves.
    wait(&mut hp, Duration::from_millis(200))?;
    assert!(!hp.still());
    shot(&mut hp, "agents")?;
    hp.release_clock();
    // It stops waiting: the badge follows.
    let mut stream = UnixStream::connect(&socket)?;
    writeln!(stream, "agent command migration working {pid} migration")?;
    let mut stream = UnixStream::connect(&socket)?;
    writeln!(stream, "agent command payments working {pid} payments")?;
    let followed = hp.dispatch_until(WAIT, |hp| {
        hp.overview_badges() == [(Some("test.host".to_owned()), "working".to_owned(), 3)]
    })?;
    anyhow::ensure!(followed, "{:?}", hp.overview_badges());
    Ok(())
}

#[test]
fn at_rest_the_open_overview_draws_nothing() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    // Past the sleeping pandas' breaths.
    hp.hold_clock();
    hp.step(Duration::from_secs(7))?;
    hp.step(common::STEP)?;
    assert!(hp.still(), "something still moves");
    hp.release_clock();
    common::at_rest(&mut hp)?;
    let frames = hp.frames();
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(hp.frames(), frames, "frames drawn at rest");
    assert_eq!(hp.overview(), Some(true));
    Ok(())
}
