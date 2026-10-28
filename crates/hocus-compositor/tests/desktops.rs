//! Desktops and Alt+Tab, headless (docs/design/windows.md §11, §12):
//! which windows are on which desktop, switching by the keys and three
//! fingers, sending a window and taking it along, the strip at rest once
//! it has slid, and Alt+Tab's order, selection and minimised windows.

mod common;

use std::time::Duration;

use common::{
    BTN_LEFT, KEY_1, KEY_2, KEY_3, KEY_ESC, KEY_H, KEY_LEFT, KEY_LEFTALT, KEY_LEFTCTRL,
    KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_RIGHT, KEY_TAB, WAIT, chord, compositor, open, wait,
};
use hocus_compositor::{Headless, model::WindowId};

/// A window's desktop (from 0) and whether it shows, by app id.
fn where_is(hp: &Headless, app_id: &str) -> Option<(usize, bool)> {
    hp.all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some(app_id))
        .map(|w| (w.2, w.3))
}

fn id_of(hp: &Headless, app_id: &str) -> Option<WindowId> {
    hp.all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some(app_id))
        .map(|w| w.0)
}

/// The focused window's app id, with the keyboard on it.
fn focused(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.focused && w.keyboard)
        .and_then(|w| w.app_id)
}

/// Runs the compositor until the strip is still on `desktop`.
fn settle_on(hp: &mut Headless, desktop: usize) -> anyhow::Result<()> {
    let still = hp.dispatch_until(WAIT, |hp| hp.desktop() == (desktop, desktop as f64))?;
    anyhow::ensure!(still, "the strip is at {:?}, not {desktop}", hp.desktop());
    Ok(())
}

#[test]
fn each_desktop_shows_its_own_windows_and_the_focus_follows() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.one", (400, 300))?;
    assert_eq!(where_is(&hp, "test.one"), Some((0, true)));
    // Super+2: an empty desktop, nothing focused.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    assert_eq!(hp.desktop().0, 1);
    assert_eq!(where_is(&hp, "test.one"), Some((0, false)));
    assert!(hp.windows().is_empty(), "nothing shows");
    assert_eq!(focused(&hp), None);
    // A window opens on the desktop shown.
    open(&mut hp, "test.two", (400, 300))?;
    assert_eq!(where_is(&hp, "test.two"), Some((1, true)));
    // Super+1: back, the first window focused again.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    assert_eq!(where_is(&hp, "test.two"), Some((1, false)));
    assert_eq!(where_is(&hp, "test.one"), Some((0, true)));
    assert_eq!(focused(&hp).as_deref(), Some("test.one"));
    // Super+Ctrl+→ and ←, and not past the first.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_RIGHT]);
    assert_eq!(hp.desktop().0, 1);
    assert_eq!(focused(&hp).as_deref(), Some("test.two"));
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFT]);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFT]);
    assert_eq!(hp.desktop().0, 0);
    settle_on(&mut hp, 0)?;
    Ok(())
}

#[test]
fn a_window_sent_away_stays_there_and_one_taken_goes_along() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.stays", (400, 300))?;
    let sent = open(&mut hp, "test.sent", (500, 300))?;
    assert_eq!(focused(&hp).as_deref(), Some("test.sent"));
    // Super+Shift+3: to the third desktop, without going there.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_3]);
    assert_eq!(hp.desktop().0, 0);
    assert_eq!(where_is(&hp, "test.sent"), Some((2, false)));
    assert_eq!(focused(&hp).as_deref(), Some("test.stays"));
    // There, it is where it was.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    let rect = hp.windows().into_iter().find_map(|w| {
        (w.app_id.as_deref() == Some("test.sent")).then_some(w.rect)
    });
    assert_eq!(rect, Some(sent));
    // Super+Ctrl+Shift+1: taken to the first desktop, going with it.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_1]);
    assert_eq!(hp.desktop().0, 0);
    assert_eq!(where_is(&hp, "test.sent"), Some((0, true)));
    assert_eq!(focused(&hp).as_deref(), Some("test.sent"));
    assert_eq!(hp.windows().len(), 2);
    Ok(())
}

#[test]
fn the_strip_slides_then_nothing_is_drawn() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.slide", (400, 300))?;
    wait(&mut hp, Duration::from_millis(400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    // On its way, past the second desktop: from 1 to 3 the strip slides
    // past 2.
    let passing = hp.dispatch_until(WAIT, |hp| {
        let (_, at) = hp.desktop();
        at > 0.9 && at < 1.9
    })?;
    assert!(passing, "the strip went straight to {:?}", hp.desktop());
    settle_on(&mut hp, 2)?;
    // The dots fade, then no frame is drawn.
    wait(&mut hp, Duration::from_millis(500))?;
    let frames = hp.frames();
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames(), frames, "frames drawn at rest");
    Ok(())
}

#[test]
fn three_fingers_switch_desktops_and_a_short_swipe_springs_back() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.swipe", (400, 300))?;
    // Fingers to the left, past a quarter of a desktop: the next one.
    hp.swipe(3, &[(-30.0, 0.0); 5], 40);
    assert_eq!(hp.desktop().0, 1);
    settle_on(&mut hp, 1)?;
    assert_eq!(where_is(&hp, "test.swipe"), Some((0, false)));
    // A little, slowly: back where it was.
    hp.swipe(3, &[(10.0, 0.0); 4], 100);
    assert_eq!(hp.desktop().0, 1);
    settle_on(&mut hp, 1)?;
    // Fingers to the right: the first desktop again.
    hp.swipe(3, &[(40.0, 0.0); 5], 40);
    settle_on(&mut hp, 0)?;
    assert_eq!(where_is(&hp, "test.swipe"), Some((0, true)));
    // Four fingers, or three going down, are not the desktops'.
    hp.swipe(4, &[(-60.0, 0.0); 5], 40);
    hp.swipe(3, &[(0.0, -60.0); 5], 40);
    assert_eq!(hp.desktop(), (0, 0.0));
    Ok(())
}

/// Alt held, then Tab `tabs` times.
fn alt_tab(hp: &mut Headless, tabs: usize) {
    hp.key(KEY_LEFTALT, true);
    for _ in 0..tabs {
        hp.key(KEY_TAB, true);
        hp.key(KEY_TAB, false);
    }
}

#[test]
fn a_quick_alt_tab_goes_back_to_the_previous_window_unseen() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.first", (400, 300))?;
    open(&mut hp, "test.second", (400, 300))?;
    alt_tab(&mut hp, 1);
    let (items, selected, shown) = hp.switcher().ok_or_else(|| anyhow::anyhow!("no switcher"))?;
    assert_eq!(items.len(), 2);
    assert_eq!(selected, 1);
    assert!(!shown, "shown before 100 ms");
    hp.key(KEY_LEFTALT, false);
    assert!(hp.switcher().is_none());
    assert_eq!(focused(&hp).as_deref(), Some("test.first"));
    // And back again.
    alt_tab(&mut hp, 1);
    hp.key(KEY_LEFTALT, false);
    assert_eq!(focused(&hp).as_deref(), Some("test.second"));
    Ok(())
}

#[test]
fn alt_tab_held_lists_this_desktop_then_minimised_and_other_desktops() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.away", (400, 300))?;
    // To the second desktop.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    open(&mut hp, "test.minimised", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_H]);
    open(&mut hp, "test.older", (400, 300))?;
    open(&mut hp, "test.newest", (400, 300))?;
    let ids = |hp: &Headless, apps: &[&str]| -> Vec<Option<WindowId>> {
        apps.iter().map(|a| id_of(hp, a)).collect()
    };
    alt_tab(&mut hp, 1);
    let shown = hp.dispatch_until(WAIT, |hp| hp.switcher().is_some_and(|s| s.2))?;
    assert!(shown, "the panel never showed");
    let (items, selected, _) = hp.switcher().ok_or_else(|| anyhow::anyhow!("closed"))?;
    let expected = ids(
        &hp,
        &["test.newest", "test.older", "test.minimised", "test.away"],
    );
    assert_eq!(items.into_iter().map(Some).collect::<Vec<_>>(), expected);
    assert_eq!(selected, 1);
    // Tab twice, Shift+Tab once: the minimised one.
    hp.key(KEY_TAB, true);
    hp.key(KEY_TAB, false);
    hp.key(KEY_TAB, true);
    hp.key(KEY_TAB, false);
    hp.key(KEY_LEFTSHIFT, true);
    hp.key(KEY_TAB, true);
    hp.key(KEY_TAB, false);
    hp.key(KEY_LEFTSHIFT, false);
    assert_eq!(hp.switcher().map(|s| s.1), Some(2));
    hp.key(KEY_LEFTALT, false);
    assert_eq!(hp.minimized(), 0);
    assert_eq!(focused(&hp).as_deref(), Some("test.minimised"));
    // To the window of the other desktop: the desktop comes with it.
    alt_tab(&mut hp, 3);
    let (items, selected, _) = hp.switcher().ok_or_else(|| anyhow::anyhow!("closed"))?;
    assert_eq!(Some(items[selected]), id_of(&hp, "test.away"));
    hp.key(KEY_LEFTALT, false);
    assert_eq!(hp.desktop().0, 1);
    assert_eq!(focused(&hp).as_deref(), Some("test.away"));
    Ok(())
}

#[test]
fn escape_cancels_alt_tab_and_a_click_picks_a_window() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.a", (400, 300))?;
    open(&mut hp, "test.b", (400, 300))?;
    open(&mut hp, "test.c", (400, 300))?;
    alt_tab(&mut hp, 2);
    hp.key(KEY_ESC, true);
    hp.key(KEY_ESC, false);
    assert!(hp.switcher().is_none());
    hp.key(KEY_LEFTALT, false);
    assert_eq!(focused(&hp).as_deref(), Some("test.c"));
    // Held until it shows, then a click on the last thumbnail.
    alt_tab(&mut hp, 1);
    let shown = hp.dispatch_until(WAIT, |hp| hp.switcher().is_some_and(|s| s.2))?;
    assert!(shown);
    let (x, y) = hp
        .switcher_item(2)
        .ok_or_else(|| anyhow::anyhow!("no third item"))?;
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    assert!(hp.switcher().is_none());
    hp.key(KEY_LEFTALT, false);
    assert_eq!(focused(&hp).as_deref(), Some("test.a"));
    Ok(())
}
