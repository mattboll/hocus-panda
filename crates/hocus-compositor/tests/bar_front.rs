//! The bar under the windows (docs/design/bar.md §2, issue #519): a window
//! the user puts over the bar covers it and takes the pointer there; the
//! pointer pushed against the screen's edge over the bar, still there
//! 300 ms later, brings the bar in front until it leaves the bar; the
//! windows the desktop places still keep off the bar; its popovers, its
//! halo and the bubbles stay above a window covering it; a fullscreen
//! window covers it, pushed or not; and nothing is drawn at rest.
//!
//! Plain layer surfaces of the test's stand for the shell's, each of its
//! own colour, in the bar's, a popover's and a bubble's namespaces: what
//! shows at a point says what is in front there. Pixman, as in CI; the
//! halo on the GPU (its shaders), skipped without one.

use crate::common;

use std::time::Duration;

use common::{
    BTN_LEFT, Command, KEY_LEFTALT, KEY_LEFTMETA, KEY_UP, Shows, WAIT, WINDOW_COLOUR, chord,
    client, open, open_with, rect_of, shell_program, wait, wait_for_rect,
};
use hocus_compositor::{
    BarForm, Config, Headless, HeadlessConfig, RendererChoice, halo::Meeting, model::Rect,
};

/// The test's surfaces' colours, ARGB.
const BAR: u32 = 0xffe0_a020;
const POPOVER: u32 = 0xff20_c060;
const BUBBLE: u32 = 0xffc0_20c0;

/// A 1920×1080 screen: the full bar, 8 px from its edges.
const SCREEN: (i32, i32) = (1920, 1080);
const BAR_RECT: Rect = Rect::new(1920 - 8 - 196, 8, 196, 1080 - 16);
/// Where the popover and the bubble stand for the shell's: beside the bar,
/// 8 px from it.
const BESIDE: i32 = 8 + 196 + 8;
const POPOVER_RECT: Rect = Rect::new(1920 - BESIDE - 280, 300, 280, 200);
const BUBBLE_RECT: Rect = Rect::new(1920 - BESIDE - 280, 600, 280, 100);
/// The bubble's namespace, as the shell names its first.
const BUBBLE_NAMESPACE: &str = "hocus-bubble-1";
/// The covering window's size: over the popover's and the bubble's rows.
const WINDOW: (u32, u32) = (600, 700);
/// A row through the window, the popover's and the bar's.
const ROW: i32 = 400;

fn reduced() -> Config {
    let mut config = Config::default();
    config.appearance.reduced_motion = true;
    config
}

fn start(renderer: RendererChoice, node: Option<std::path::PathBuf>) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![format!("{}x{}", SCREEN.0, SCREEN.1).parse()?],
            renderer,
            render_node: node,
        },
        reduced(),
    )?;
    layer(&mut hp, hocus_link::NAMESPACE, (196, 0), (8, 8), BAR)?;
    Ok(hp)
}

/// A layer surface of the test's in `namespace`, `size` large, `margin`
/// (top, right) from the screen's top right corner, filled with `colour`.
fn layer(
    hp: &mut Headless,
    namespace: &'static str,
    size: (u32, u32),
    margin: (i32, i32),
    colour: u32,
) -> anyhow::Result<()> {
    std::mem::forget(client(
        hp.socket(),
        Some(Shows::Layer {
            namespace,
            size,
            margin,
            colour,
        }),
    )?);
    let placed = hp.dispatch_until(WAIT, |hp| !hp.layer_rects(namespace).is_empty())?;
    anyhow::ensure!(placed, "no {namespace} surface");
    wait(hp, Duration::from_millis(200))
}

/// One pixel of the screen, as R, G, B.
fn pixel(hp: &mut Headless, (x, y): (i32, i32)) -> anyhow::Result<[i32; 3]> {
    let shot = hp.screenshot(0)?;
    let i = ((y * shot.0 + x) * 4) as usize;
    let p = &shot.2[i..i + 4];
    Ok([i32::from(p[2]), i32::from(p[1]), i32::from(p[0])])
}

fn rgb(colour: u32) -> [i32; 3] {
    [
        ((colour >> 16) & 0xff) as i32,
        ((colour >> 8) & 0xff) as i32,
        (colour & 0xff) as i32,
    ]
}

/// Whether `got` is `colour`, give or take what a renderer rounds.
fn is(got: [i32; 3], colour: u32) -> bool {
    got.iter().zip(rgb(colour)).all(|(a, b)| (a - b).abs() <= 3)
}

/// Waits for a frame or two, then the pixel at `at` is `colour`.
fn shows(hp: &mut Headless, at: (i32, i32), colour: u32, what: &str) -> anyhow::Result<()> {
    wait(hp, Duration::from_millis(100))?;
    let got = pixel(hp, at)?;
    anyhow::ensure!(
        is(got, colour),
        "{what}: at {at:?} the screen shows {got:?}, not {:?}",
        rgb(colour)
    );
    Ok(())
}

/// Moves `app_id`'s window with `Alt`+drag, held at its left part, until
/// its left edge is at `x`; then lets go.
fn drag_to(hp: &mut Headless, app_id: &str, x: i32) -> anyhow::Result<Rect> {
    let start = rect_of(hp, app_id).ok_or_else(|| anyhow::anyhow!("no {app_id}"))?;
    let held = (100, start.height / 2);
    let y = f64::from(start.y + held.1);
    hp.pointer_move(f64::from(start.x + held.0), y);
    hp.key(KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
    let mut at = start.x;
    while at != x {
        at = if at < x {
            (at + 40).min(x)
        } else {
            (at - 40).max(x)
        };
        hp.pointer_move(f64::from(at + held.0), y);
        wait(hp, Duration::from_millis(10))?;
    }
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTALT, false);
    let want = Rect::new(x, start.y, start.width, start.height);
    wait_for_rect(hp, app_id, want)?;
    wait(hp, Duration::from_millis(200))?;
    Ok(want)
}

/// The window `over`, moved so that its right edge is at the middle of the
/// bar: the bar's left half covered.
fn half_over(hp: &mut Headless) -> anyhow::Result<Rect> {
    open(hp, "over", WINDOW)?;
    drag_to(
        hp,
        "over",
        BAR_RECT.x + BAR_RECT.width / 2 - WINDOW.0 as i32,
    )
}

/// The covered half and the other, on [`ROW`].
const COVERED: (i32, i32) = (BAR_RECT.x + 40, ROW);
const UNCOVERED: (i32, i32) = (BAR_RECT.x + 150, ROW);

fn pointer_at(hp: &mut Headless, (x, y): (i32, i32)) {
    hp.pointer_move(f64::from(x), f64::from(y));
}

fn focused(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.focused)
        .and_then(|w| w.app_id)
}

/// A window dragged half over the bar covers that half and takes the
/// pointer and the clicks there; the other half is still the bar's.
/// Nothing is drawn at rest.
#[test]
fn a_window_half_over_the_bar_covers_it_and_takes_the_clicks_there() -> anyhow::Result<()> {
    let mut hp = start(RendererChoice::Pixman, None)?;
    let over = half_over(&mut hp)?;
    assert!(over.right() > BAR_RECT.x && over.right() < BAR_RECT.right());
    // Another window takes the focus, away from the bar.
    open(&mut hp, "other", (300, 200))?;
    assert_eq!(focused(&hp).as_deref(), Some("other"));
    shows(&mut hp, COVERED, WINDOW_COLOUR, "the covered half")?;
    shows(&mut hp, UNCOVERED, BAR, "the other half")?;
    // The window takes the pointer over the bar, and the click.
    pointer_at(&mut hp, COVERED);
    assert_eq!(hp.pointer_window().as_deref(), Some("over"));
    assert_eq!(hp.pointer_layer(), None);
    hp.button(BTN_LEFT, true);
    assert!(hp.pointer_grabbed(), "the press reached no client");
    hp.button(BTN_LEFT, false);
    assert_eq!(focused(&hp).as_deref(), Some("over"));
    // Beside it, the bar's.
    pointer_at(&mut hp, UNCOVERED);
    assert_eq!(hp.pointer_window(), None);
    assert_eq!(hp.pointer_layer().as_deref(), Some(hocus_link::NAMESPACE));
    // Nothing draws at rest, with the window over the bar: once the
    // motions are over, however late a loaded machine draws their end.
    common::at_rest(&mut hp)?;
    let frames = hp.frames();
    wait(&mut hp, Duration::from_secs(1))?;
    assert_eq!(hp.frames(), frames, "frames drawn at rest");
    assert_eq!(hp.bar_in_front(), None);
    Ok(())
}

/// The pointer pushed against the screen's edge over the bar brings it in
/// front after 300 ms, where the window covered it too, and the bar takes
/// the pointer there; leaving the bar lets the window cover it again. Off
/// the edge before 300 ms, nothing. The click on the edge still switches
/// the bar's form. Nothing is drawn at rest, in front or not.
#[test]
fn pushing_against_the_edge_brings_the_bar_in_front_until_the_pointer_leaves_it()
-> anyhow::Result<()> {
    let mut hp = start(RendererChoice::Pixman, None)?;
    half_over(&mut hp)?;
    let edge = (SCREEN.0 - 1, ROW);
    // Against the edge, from the middle of the screen in one move.
    pointer_at(&mut hp, (900, ROW));
    pointer_at(&mut hp, edge);
    wait(&mut hp, Duration::from_millis(150))?;
    assert_eq!(hp.bar_in_front(), None, "in front before 300 ms");
    // Off the edge before its time: nothing.
    pointer_at(&mut hp, UNCOVERED);
    wait(&mut hp, Duration::from_millis(400))?;
    assert_eq!(hp.bar_in_front(), None, "in front without the push");
    shows(&mut hp, COVERED, WINDOW_COLOUR, "not pushed")?;
    // Pushed, and resting there.
    pointer_at(&mut hp, edge);
    wait(&mut hp, Duration::from_millis(250))?;
    assert_eq!(hp.bar_in_front(), None, "in front before 300 ms");
    // Its time on the compositor's clock; a loaded machine's loop only
    // turns later.
    let raised = hp.dispatch_until(WAIT, |hp| hp.bar_in_front().is_some())?;
    assert!(raised, "the bar never came in front");
    assert_eq!(hp.bar_in_front().as_deref(), Some("HEADLESS-1"));
    shows(&mut hp, COVERED, BAR, "the bar in front")?;
    common::at_rest(&mut hp)?;
    let frames = hp.frames();
    wait(&mut hp, Duration::from_secs(1))?;
    assert_eq!(
        hp.frames(),
        frames,
        "frames drawn at rest, the bar in front"
    );
    // Across the bar, still in it: the bar's, where the window is.
    pointer_at(&mut hp, COVERED);
    assert_eq!(hp.bar_in_front().as_deref(), Some("HEADLESS-1"));
    assert_eq!(hp.pointer_window(), None);
    assert_eq!(hp.pointer_layer().as_deref(), Some(hocus_link::NAMESPACE));
    // Out of the bar: the window covers it again, and takes the pointer.
    pointer_at(&mut hp, (BAR_RECT.x - 20, ROW));
    assert_eq!(hp.bar_in_front(), None);
    assert_eq!(hp.pointer_window().as_deref(), Some("over"));
    shows(&mut hp, COVERED, WINDOW_COLOUR, "the bar back under")?;
    pointer_at(&mut hp, COVERED);
    assert_eq!(hp.pointer_window().as_deref(), Some("over"));
    common::at_rest(&mut hp)?;
    let frames = hp.frames();
    wait(&mut hp, Duration::from_secs(1))?;
    assert_eq!(hp.frames(), frames, "frames drawn at rest, the bar back");
    // The click on the edge switches the form, pushed or not.
    pointer_at(&mut hp, edge);
    hp.button(BTN_LEFT, true);
    assert!(!hp.pointer_grabbed(), "the press reached a client");
    hp.button(BTN_LEFT, false);
    assert_eq!(hp.bars()[0].1, BarForm::Mini);
    Ok(())
}

/// The windows the desktop places keep off the bar: a new one, and one
/// maximised, which stops 8 px from it; the bar shows beside them.
#[test]
fn a_maximised_window_still_stops_at_the_bar() -> anyhow::Result<()> {
    let mut hp = start(RendererChoice::Pixman, None)?;
    let area = hp
        .work_area()
        .ok_or_else(|| anyhow::anyhow!("no work area"))?;
    assert_eq!(area.right(), BAR_RECT.x - 8);
    let new = open(&mut hp, "max", (800, 600))?;
    assert!(new.right() <= area.right(), "{new:?} over the bar");
    pointer_at(&mut hp, (400, 400));
    chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    wait_for_rect(&mut hp, "max", area)?;
    // Past the window's shadow, which falls on the bar under it.
    shows(
        &mut hp,
        (BAR_RECT.x + 100, ROW),
        BAR,
        "beside the maximised window",
    )?;
    shows(
        &mut hp,
        (BAR_RECT.x - 12, ROW),
        WINDOW_COLOUR,
        "the maximised window",
    )?;
    Ok(())
}

/// The bar's popovers and the bubbles stay above a window covering the
/// bar and them, and take the pointer there.
#[test]
fn popovers_and_bubbles_stay_above_a_window_over_the_bar() -> anyhow::Result<()> {
    let mut hp = start(RendererChoice::Pixman, None)?;
    let over = half_over(&mut hp)?;
    assert!(hocus_link::is_bubble(BUBBLE_NAMESPACE));
    let surfaces = [
        (
            hocus_link::POPOVER_NAMESPACE,
            POPOVER_RECT,
            POPOVER,
            "the popover",
        ),
        (BUBBLE_NAMESPACE, BUBBLE_RECT, BUBBLE, "the bubble"),
    ];
    for (namespace, rect, colour, what) in surfaces {
        let size = (rect.width as u32, rect.height as u32);
        layer(&mut hp, namespace, size, (rect.y, BESIDE), colour)?;
        assert_eq!(
            hp.layer_rects(namespace),
            vec![("HEADLESS-1".to_owned(), rect)],
            "{what}"
        );
    }
    for (namespace, rect, colour, what) in surfaces {
        let middle = (rect.x + rect.width / 2, rect.y + rect.height / 2);
        assert!(
            over.contains(f64::from(middle.0), f64::from(middle.1)),
            "{what} not under the window"
        );
        shows(&mut hp, middle, colour, what)?;
        pointer_at(&mut hp, middle);
        assert_eq!(hp.pointer_layer().as_deref(), Some(namespace), "{what}");
        assert_eq!(hp.pointer_window(), None, "{what}");
    }
    // The window still covers the bar beside them.
    shows(&mut hp, COVERED, WINDOW_COLOUR, "the covered half")?;
    Ok(())
}

/// On GLES (the GPU `HOCUS_TEST_GPU` names, else Mesa's software device),
/// under the real shell's bar, the halo stays above a window covering the
/// bar: lit, it shows over the window there; out, the window shows as it
/// is.
#[test]
fn the_halo_stays_above_a_window_over_the_bar() -> anyhow::Result<()> {
    let shell = shell_program();
    let dir = tempfile::tempdir()?;
    let shell = common::shell_script(dir.path(), &shell, &[])?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![format!("{}x{}", SCREEN.0, SCREEN.1).parse()?],
            ..common::gles()
        },
        reduced(),
        Some(shell),
    )?;
    if hp.renderer() != "gles" {
        eprintln!("no GLES here: no halo to see");
        return Ok(());
    }
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.shell_connected()
            && hp
                .bar_surfaces()
                .first()
                .is_some_and(|(_, r)| *r == BAR_RECT)
    })?;
    anyhow::ensure!(drawn, "the shell drew no bar: {:?}", hp.bar_surfaces());
    half_over(&mut hp)?;
    pointer_at(&mut hp, (400, 400));
    // Along the bar's left edge, under the window.
    let along: Vec<(i32, i32)> = (BAR_RECT.x - 2..BAR_RECT.x + 8).map(|x| (x, ROW)).collect();
    let before: Vec<[i32; 3]> = along
        .iter()
        .map(|at| pixel(&mut hp, *at))
        .collect::<Result<_, _>>()?;
    assert!(
        before.iter().all(|p| is(*p, WINDOW_COLOUR)),
        "the window does not cover the bar's edge: {before:?}"
    );
    hp.meeting_near(Some(Meeting::InTwo));
    wait(&mut hp, Duration::from_millis(1500))?;
    assert!(hp.halo().is_some(), "no halo drawn");
    let lit: Vec<[i32; 3]> = along
        .iter()
        .map(|at| pixel(&mut hp, *at))
        .collect::<Result<_, _>>()?;
    let most = lit
        .iter()
        .zip(&before)
        .map(|(a, b)| {
            a.iter()
                .zip(b)
                .map(|(x, y)| (x - y).abs())
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);
    assert!(most > 30, "the halo does not show over the window: {lit:?}");
    hp.meeting_near(None);
    wait(&mut hp, Duration::from_millis(1000))?;
    let out: Vec<[i32; 3]> = along
        .iter()
        .map(|at| pixel(&mut hp, *at))
        .collect::<Result<_, _>>()?;
    assert!(out.iter().all(|p| is(*p, WINDOW_COLOUR)), "{out:?}");
    Ok(())
}

/// A fullscreen window covers the bar as before; pushing against the edge
/// does not bring the bar over it.
#[test]
fn a_fullscreen_window_covers_the_bar_pushed_or_not() -> anyhow::Result<()> {
    let mut hp = start(RendererChoice::Pixman, None)?;
    let (_, handle) = open_with(&mut hp, "full", (600, 400), false)?;
    handle.commands.send(Command::Fullscreen(true))?;
    wait_for_rect(&mut hp, "full", Rect::new(0, 0, SCREEN.0, SCREEN.1))?;
    // Its way there over.
    wait(&mut hp, Duration::from_millis(500))?;
    shows(&mut hp, UNCOVERED, WINDOW_COLOUR, "fullscreen")?;
    pointer_at(&mut hp, (900, ROW));
    pointer_at(&mut hp, (SCREEN.0 - 1, ROW));
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(hp.bar_in_front(), None);
    shows(&mut hp, UNCOVERED, WINDOW_COLOUR, "fullscreen, pushed")?;
    std::mem::forget(handle);
    Ok(())
}
