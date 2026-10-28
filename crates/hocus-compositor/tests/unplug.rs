//! A screen unplugged and plugged back, headless (docs/design/windows.md
//! §12): no window is lost, each going to the screen still there on its
//! desktop and in its state; back, they return where they were, but for
//! those the user placed meanwhile. A drag, Alt+Tab or a desktop slide on
//! the screen as it goes ends cleanly.

mod common;

use std::time::Duration;

use common::{
    BTN_LEFT, Command, KEY_2, KEY_3, KEY_H, KEY_LEFT, KEY_LEFTALT, KEY_LEFTMETA, KEY_LEFTSHIFT,
    KEY_RIGHT, KEY_TAB, WAIT, chord, open, open_with, two_screens, wait,
};
use hocus_compositor::{
    Headless, Placed,
    model::{self, Mode, Rect, Zone},
};

const FIRST: &str = "HEADLESS-1";
const SECOND: &str = "HEADLESS-2";

fn placed(hp: &Headless, app_id: &str) -> Placed {
    hp.placements()
        .into_iter()
        .find(|p| p.app_id.as_deref() == Some(app_id))
        .unwrap_or_else(|| panic!("no window {app_id}"))
}

fn geometry(hp: &Headless, name: &str) -> Rect {
    hp.outputs()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, g)| g)
        .unwrap_or_else(|| panic!("no output {name}"))
}

fn inside(rect: Rect, area: Rect) -> bool {
    rect.x >= area.x
        && rect.y >= area.y
        && rect.right() <= area.right()
        && rect.bottom() <= area.bottom()
}

/// Drags the window under (`x`, `y`) by (`dx`, `dy`) with `Super`.
fn super_drag(hp: &mut Headless, (x, y): (f64, f64), (dx, dy): (f64, f64)) {
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    for i in 1..=10 {
        let k = f64::from(i) / 10.0;
        hp.pointer_move(x + dx * k, y + dy * k);
    }
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
}

fn centre(rect: Rect) -> (f64, f64) {
    rect.centre()
}

#[test]
fn a_screen_unplugged_keeps_its_windows_and_gets_them_back() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let first = geometry(&hp, FIRST);
    let area1 = hp.work_area_of(FIRST).expect("an area");
    let area2 = hp.work_area_of(SECOND).expect("an area");
    // On the first screen, a window that stays where it is.
    hp.pointer_move(960.0, 540.0);
    open(&mut hp, "test.stays", (500, 400))?;
    // On the second: fullscreen, snapped, minimised, on other desktops...
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    let (_, full) = open_with(&mut hp, "test.full", (400, 300), false)?;
    full.commands.send(Command::Fullscreen(true))?;
    let fullscreen =
        hp.dispatch_until(WAIT, |hp| placed(hp, "test.full").mode == Mode::Fullscreen)?;
    assert!(fullscreen, "test.full went fullscreen");
    open(&mut hp, "test.snapped", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    open(&mut hp, "test.minimized", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_H]);
    open(&mut hp, "test.away", (700, 500))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    open(&mut hp, "test.away-snapped", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_3]);
    // ... and floating on top, moved off its centre.
    let opened = open(&mut hp, "test.floating", (600, 400))?;
    super_drag(&mut hp, centre(opened), (600.0, 300.0));
    wait(&mut hp, Duration::from_millis(100))?;
    let before = hp.placements();
    let moved: Vec<&Placed> = before
        .iter()
        .filter(|p| p.output.as_deref() == Some(SECOND))
        .collect();
    assert_eq!(moved.len(), 6, "{before:#?}");
    let stays = placed(&hp, "test.stays");
    assert_eq!(stays.output.as_deref(), Some(FIRST));

    // The pointer was on the second screen.
    hp.unplug(SECOND)?;
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(hp.outputs().len(), 1);
    for was in &moved {
        let app = was.app_id.as_deref().unwrap_or_default();
        let now = placed(&hp, app);
        assert_eq!(now.output.as_deref(), Some(FIRST), "{app} on the first");
        assert_eq!(now.desktop, was.desktop, "{app} on the same desktop");
        assert_eq!(now.mode, was.mode, "{app} in the same state");
        assert_eq!(now.minimized, was.minimized, "{app} minimised as it was");
        assert_eq!(
            now.shown,
            !was.minimized && was.desktop == 0,
            "{app} shows with the first screen's desktop 1"
        );
        match now.mode {
            Mode::Fullscreen => assert_eq!(now.rect, first, "{app} over the first"),
            Mode::Snapped(zone) => assert_eq!(now.rect, zone.rect(area1), "{app} in its zone"),
            Mode::Floating => {
                assert!(inside(now.rect, area1), "{app} at {:?}", now.rect);
                assert_eq!(now.rect, model::carried(was.rect, area2, area1), "{app}");
            }
        }
    }
    assert_eq!(placed(&hp, "test.stays"), stays, "the first's window stays");
    // The focus and the pointer are on the screen still there.
    let (px, py) = hp.pointer();
    assert!(first.contains(px, py), "the pointer at {px},{py}");
    let focused = hp.windows().into_iter().find(|w| w.focused);
    let focused = focused.expect("a window has the focus");
    assert!(focused.keyboard, "the keyboard follows the focus");

    // The user moves one of them meanwhile: it stays where they put it.
    let carried = placed(&hp, "test.floating").rect;
    super_drag(&mut hp, centre(carried), (-100.0, -60.0));
    let put = placed(&hp, "test.floating");
    assert_ne!(put.rect, carried, "the drag moved it");

    hp.plug(SECOND)?;
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(hp.outputs().len(), 2);
    for was in &moved {
        let app = was.app_id.as_deref().unwrap_or_default();
        let now = placed(&hp, app);
        if app == "test.floating" {
            assert_eq!(now, put, "{app} stays where the user put it");
        } else {
            assert_eq!(&now, *was, "{app} back where it was");
        }
    }
    assert_eq!(placed(&hp, "test.stays"), stays);
    Ok(())
}

#[test]
fn a_drag_alt_tab_or_a_slide_on_a_screen_unplugged_ends_there() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let first = geometry(&hp, FIRST);
    let area1 = hp.work_area_of(FIRST).expect("an area");
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    let opened = open(&mut hp, "test.dragged", (600, 400))?;
    // Held mid-drag, at the second screen's left edge, its snap preview
    // showing there.
    let (x, y) = centre(opened);
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x - 300.0, y);
    hp.pointer_move(1925.0, y);
    wait(&mut hp, Duration::from_millis(150))?;
    hp.unplug(SECOND)?;
    wait(&mut hp, Duration::from_millis(50))?;
    let (px, py) = hp.pointer();
    assert!(first.contains(px, py), "the pointer at {px},{py}");
    // Let go: nothing more happens to it.
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    wait(&mut hp, Duration::from_millis(50))?;
    let dragged = placed(&hp, "test.dragged");
    assert_eq!(dragged.output.as_deref(), Some(FIRST));
    assert_eq!(dragged.mode, Mode::Floating, "the drag did not snap it");
    assert!(inside(dragged.rect, area1), "{:?}", dragged.rect);

    // Alt+Tab held on the second screen as it goes.
    hp.plug(SECOND)?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    open(&mut hp, "test.other", (400, 300))?;
    hp.key(KEY_LEFTALT, true);
    hp.key(KEY_TAB, true);
    hp.key(KEY_TAB, false);
    let shown = hp.dispatch_until(WAIT, |hp| hp.switcher().is_some_and(|s| s.2))?;
    assert!(shown, "Alt+Tab shows");
    hp.unplug(SECOND)?;
    assert!(hp.switcher().is_none(), "Alt+Tab ends with its screen");
    hp.key(KEY_LEFTALT, false);
    wait(&mut hp, Duration::from_millis(50))?;

    // A desktop sliding on it.
    hp.plug(SECOND)?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    hp.unplug(SECOND)?;
    wait(&mut hp, Duration::from_millis(400))?;
    for p in hp.placements() {
        let app = p.app_id.unwrap_or_default();
        assert_eq!(p.output.as_deref(), Some(FIRST), "{app}");
        assert!(
            p.minimized || inside(p.rect, area1),
            "{app} at {:?}",
            p.rect
        );
    }
    // Back, it shows the desktop it showed.
    hp.plug(SECOND)?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    assert_eq!(hp.desktop_under_pointer(), 1);
    Ok(())
}

#[test]
fn a_window_snapped_or_sent_meanwhile_stays() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let area1 = hp.work_area_of(FIRST).expect("an area");
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    open(&mut hp, "test.resnapped", (400, 300))?;
    open(&mut hp, "test.sent", (400, 300))?;
    hp.unplug(SECOND)?;
    hp.pointer_move(960.0, 540.0);
    // The focused one goes to desktop 2; the other one snaps.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    let snapped = placed(&hp, "test.resnapped");
    assert_eq!(snapped.mode, Mode::Snapped(Zone::Right));
    let sent = placed(&hp, "test.sent");
    assert_eq!(sent.desktop, 1);
    hp.plug(SECOND)?;
    assert_eq!(placed(&hp, "test.resnapped").rect, Zone::Right.rect(area1));
    assert_eq!(placed(&hp, "test.resnapped").output.as_deref(), Some(FIRST));
    assert_eq!(placed(&hp, "test.sent").output.as_deref(), Some(FIRST));
    assert_eq!(placed(&hp, "test.sent").desktop, 1);
    Ok(())
}

#[test]
fn with_every_screen_gone_the_windows_wait_for_one() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let area = hp.work_area_of(FIRST).expect("an area");
    hp.pointer_move(960.0, 540.0);
    open(&mut hp, "test.floating", (500, 400))?;
    open(&mut hp, "test.snapped", (500, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    let before = hp.placements();
    hp.unplug(FIRST)?;
    wait(&mut hp, Duration::from_millis(50))?;
    assert!(hp.outputs().is_empty());
    hp.plug(FIRST)?;
    wait(&mut hp, Duration::from_millis(50))?;
    for was in &before {
        let app = was.app_id.as_deref().unwrap_or_default();
        assert_eq!(&placed(&hp, app), was, "{app} where it was");
    }
    assert_eq!(placed(&hp, "test.snapped").rect, Zone::Right.rect(area));
    // Gone again, and back elsewhere: they come to it, inside its area.
    hp.unplug(FIRST)?;
    hp.plug_at(FIRST, 5000, 200)?;
    wait(&mut hp, Duration::from_millis(50))?;
    let area = hp.work_area_of(FIRST).expect("an area");
    assert_eq!(area.x, 5000 + model::GAP);
    let floating = placed(&hp, "test.floating");
    assert!(
        inside(floating.rect, area),
        "{:?} in {area:?}",
        floating.rect
    );
    assert_eq!(placed(&hp, "test.snapped").rect, Zone::Right.rect(area));
    Ok(())
}

#[test]
fn a_screen_back_elsewhere_gets_its_windows_at_the_same_place_on_it() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    let opened = open(&mut hp, "test.floating", (600, 400))?;
    super_drag(&mut hp, centre(opened), (-500.0, 300.0));
    let was = placed(&hp, "test.floating");
    hp.unplug(SECOND)?;
    // Below the first screen now.
    hp.plug_at(SECOND, 0, 1080)?;
    let back = placed(&hp, "test.floating");
    assert_eq!(back.output.as_deref(), Some(SECOND));
    let moved = Rect::new(was.rect.x - 1920, was.rect.y + 1080, 600, 400);
    assert_eq!(back.rect, moved);
    Ok(())
}
