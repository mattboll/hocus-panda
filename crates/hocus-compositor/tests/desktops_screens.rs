//! Desktops across several screens, headless (docs/design/windows.md §12,
//! issue #420): each screen keeps its own desktops, switching alone; the
//! bar on the primary screen shows every screen's, its own first, then the
//! others left to right; `Super+Alt+←/→` sends the focused window to the
//! screen on that side, onto the desktop it shows, at the same share of
//! its work area, focused, and with no screen there does nothing.

use crate::common;

use common::{
    BTN_LEFT, KEY_2, KEY_LEFT, KEY_LEFTALT, KEY_LEFTMETA, KEY_RIGHT, chord, open, rect_of,
    two_screens, wait_for_rect,
};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec, PanelScreens, RendererChoice,
    model::{self, Mode, WindowId, Zone},
};

const FIRST: &str = "HEADLESS-1";
const SECOND: &str = "HEADLESS-2";

fn id_of(hp: &Headless, app_id: &str) -> Option<WindowId> {
    hp.all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some(app_id))
        .map(|w| w.0)
}

/// A window's desktop (from 0) and whether it shows.
fn where_is(hp: &Headless, app_id: &str) -> Option<(usize, bool)> {
    hp.all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some(app_id))
        .map(|w| (w.2, w.3))
}

fn focused(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.focused && w.keyboard)
        .and_then(|w| w.app_id)
}

/// The desktop each screen shows, by name.
fn shown(hp: &Headless) -> Vec<(String, usize)> {
    hp.strips()
        .into_iter()
        .map(|(name, _, current)| (name, current))
        .collect()
}

/// The screens whose desktops bar `bar` shows, in its rows' order.
fn rows(hp: &Headless, bar: &str) -> Vec<String> {
    hp.state()
        .desk_rows(bar)
        .iter()
        .map(smithay::output::Output::name)
        .collect()
}

#[test]
fn each_screen_switches_alone_and_the_primary_bar_shows_every_screens_desktops()
-> anyhow::Result<()> {
    let mut hp = two_screens()?;
    // The bar on the first screen alone shows both rows, its own first.
    assert_eq!(rows(&hp, FIRST), [FIRST, SECOND]);
    hp.pointer_move(960.0, 540.0);
    open(&mut hp, "test.left", (400, 300))?;
    // The pointer on the second screen: Super+2 switches it alone.
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    assert_eq!(shown(&hp), [(FIRST.to_owned(), 0), (SECOND.to_owned(), 1)]);
    assert_eq!(where_is(&hp, "test.left"), Some((0, true)));
    Ok(())
}

#[test]
fn with_a_bar_on_every_screen_each_shows_its_own_row() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.panel.screens = PanelScreens::All;
    let hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    assert_eq!(rows(&hp, FIRST), [FIRST]);
    assert_eq!(rows(&hp, SECOND), [SECOND]);
    Ok(())
}

#[test]
fn a_screen_unplugged_takes_its_row_away_and_plugged_back_brings_it() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    hp.unplug(SECOND)?;
    assert_eq!(rows(&hp, FIRST), [FIRST]);
    hp.plug(SECOND)?;
    assert_eq!(rows(&hp, FIRST), [FIRST, SECOND]);
    Ok(())
}

#[test]
fn super_alt_right_sends_the_window_to_the_screens_shown_desktop_at_the_same_share()
-> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let areas = hp.work_areas();
    anyhow::ensure!(areas.len() == 2, "two work areas: {areas:?}");
    // The second screen shows its second desktop.
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    hp.pointer_move(960.0, 540.0);
    let start = open(&mut hp, "test.moved", (400, 300))?;
    let pointer = hp.pointer();
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTALT, KEY_RIGHT]);
    let there = model::carried(start, areas[0], areas[1]);
    wait_for_rect(&mut hp, "test.moved", there)?;
    // On the desktop that screen shows, focused; the first screen and the
    // pointer stay.
    assert_eq!(where_is(&hp, "test.moved"), Some((1, true)));
    assert_eq!(focused(&hp).as_deref(), Some("test.moved"));
    assert_eq!(shown(&hp), [(FIRST.to_owned(), 0), (SECOND.to_owned(), 1)]);
    assert_eq!(hp.pointer(), pointer);
    // And back.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTALT, KEY_LEFT]);
    wait_for_rect(
        &mut hp,
        "test.moved",
        model::carried(there, areas[1], areas[0]),
    )?;
    assert_eq!(where_is(&hp, "test.moved"), Some((0, true)));
    Ok(())
}

#[test]
fn with_no_screen_on_that_side_nothing_happens() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    let start = open(&mut hp, "test.right", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTALT, KEY_RIGHT]);
    common::wait(&mut hp, std::time::Duration::from_millis(400))?;
    assert_eq!(rect_of(&hp, "test.right"), Some(start));
    assert_eq!(focused(&hp).as_deref(), Some("test.right"));
    Ok(())
}

#[test]
fn a_snapped_window_takes_the_same_zone_there() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let areas = hp.work_areas();
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    open(&mut hp, "test.half", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.half", Zone::Left.rect(areas[1]))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTALT, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.half", Zone::Left.rect(areas[0]))?;
    let id = id_of(&hp, "test.half").expect("mapped");
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::Left));
    Ok(())
}

#[test]
fn a_window_dragged_onto_another_screen_goes_on_the_desktop_it_shows() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    hp.pointer_move(960.0, 540.0);
    let start = open(&mut hp, "test.dragged", (400, 300))?;
    // Alt+drag from its middle to the second screen's.
    let (x, y) = start.centre();
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
    for step in 1..=10 {
        let t = f64::from(step) / 10.0;
        hp.pointer_move(x + (1920.0 + 1280.0 - x) * t, y + (720.0 - y) * t);
    }
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTALT, false);
    common::wait(&mut hp, std::time::Duration::from_millis(100))?;
    assert_eq!(where_is(&hp, "test.dragged"), Some((1, true)));
    assert_eq!(shown(&hp), [(FIRST.to_owned(), 0), (SECOND.to_owned(), 1)]);
    Ok(())
}
