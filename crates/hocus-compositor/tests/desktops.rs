//! Desktops and Alt+Tab, headless (docs/design/windows.md §11, §12):
//! which windows are on which desktop, switching by the keys and three
//! fingers, sending a window and taking it along, the strip at rest once
//! it has slid, and Alt+Tab's order, selection and minimised windows.

mod common;

use std::time::Duration;

use common::{
    BTN_LEFT, KEY_1, KEY_2, KEY_3, KEY_DOWN, KEY_ESC, KEY_H, KEY_LEFT, KEY_LEFTALT, KEY_LEFTCTRL,
    KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_RIGHT, KEY_TAB, KEY_UP, Shows, WAIT, WINDOW_COLOUR, chord,
    compositor, open, open_showing, wait,
};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::WindowId,
};

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
    let rect = hp
        .windows()
        .into_iter()
        .find_map(|w| (w.app_id.as_deref() == Some("test.sent")).then_some(w.rect));
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
    let (items, selected, shown) = hp
        .switcher()
        .ok_or_else(|| anyhow::anyhow!("no switcher"))?;
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

/// Runs the compositor until Alt+Tab's panel has appeared and nothing
/// more is drawn: a loaded machine draws its motion late.
fn appeared(hp: &mut Headless) -> anyhow::Result<()> {
    wait(hp, Duration::from_millis(150))?;
    for _ in 0..40 {
        let frames = hp.frames();
        wait(hp, Duration::from_millis(150))?;
        if hp.frames() == frames {
            return Ok(());
        }
    }
    anyhow::bail!("the panel never came to rest")
}

/// The pixel at (`x`, `y`) of a screenshot, ARGB.
fn pixel((width, _, bytes): &(i32, i32, Vec<u8>), x: f64, y: f64) -> Option<u32> {
    let at = usize::try_from((y as i32) * width + x as i32).ok()? * 4;
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[test]
fn a_thumbnail_shows_the_window_without_its_client_side_shadow() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let plain = open(&mut hp, "test.plain", (400, 300))?;
    let shadowed = Shows::Shadowed {
        app_id: "test.shadowed",
        size: (400, 300),
        margin: 40,
    };
    let (rect, _client) = open_showing(&mut hp, "test.shadowed", shadowed)?;
    assert_eq!((rect.width, rect.height), (400, 300), "its geometry");
    alt_tab(&mut hp, 1);
    let shown = hp.dispatch_until(WAIT, |hp| hp.switcher().is_some_and(|s| s.2))?;
    assert!(shown, "the panel never showed");
    appeared(&mut hp)?;
    let thumb = |at| {
        hp.switcher_thumb(at)
            .ok_or_else(|| anyhow::anyhow!("no thumbnail {at}"))
    };
    let (with_shadow, without) = (thumb(0)?, thumb(1)?);
    assert_eq!(
        (with_shadow.2, with_shadow.3),
        (without.2, without.3),
        "the same size as the same window without a shadow ({plain:?})"
    );
    let shot = hp.screenshot(0)?;
    for (x, y, w, h) in [with_shadow, without] {
        // Inside its rounded corners and along its edges, the window fills
        // its thumbnail: no shadow around a smaller picture.
        let inset = 8.0;
        for (px, py) in [
            (x + inset, y + inset),
            (x + w - inset, y + inset),
            (x + inset, y + h - inset),
            (x + w - inset, y + h - inset),
            (x + w / 2.0, y + 2.0),
            (x + 2.0, y + h / 2.0),
        ] {
            assert_eq!(
                pixel(&shot, px, py),
                Some(WINDOW_COLOUR),
                "at ({px}, {py}) in the thumbnail at ({x}, {y}) {w}×{h}"
            );
        }
    }
    hp.key(KEY_LEFTALT, false);
    Ok(())
}

#[test]
fn holding_tab_steps_through_the_windows_at_the_keyboards_repeat() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keyboard.repeat_delay = 300;
    config.keyboard.repeat_rate = 10;
    let headless = HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    };
    let mut hp = Headless::start_with(headless, config)?;
    let apps = ["test.1", "test.2", "test.3", "test.4", "test.5"];
    for app in apps {
        open(&mut hp, app, (400, 300))?;
    }
    let selected = |hp: &Headless| hp.switcher().map(|s| s.1);
    // Most recently used first.
    let name = |at: usize| apps[apps.len() - 1 - at].to_string();
    hp.key(KEY_LEFTALT, true);
    hp.key(KEY_TAB, true);
    assert_eq!(selected(&hp), Some(1));
    // Held, Tab waits for the repeat delay...
    wait(&mut hp, Duration::from_millis(200))?;
    assert_eq!(selected(&hp), Some(1), "stepped before the delay");
    // ...then steps on, a window at a time.
    let stepped = hp.dispatch_until(WAIT, |hp| selected(hp) == Some(2))?;
    assert!(stepped, "held, Tab did not step on: {:?}", selected(&hp));
    let stepped = hp.dispatch_until(WAIT, |hp| selected(hp) == Some(3))?;
    assert!(stepped, "held, Tab stepped only once: {:?}", selected(&hp));
    // Let go, it stays; releasing Alt goes there.
    hp.key(KEY_TAB, false);
    let at = selected(&hp).ok_or_else(|| anyhow::anyhow!("closed"))?;
    wait(&mut hp, Duration::from_millis(400))?;
    assert_eq!(selected(&hp), Some(at), "stepped on once Tab was let go");
    hp.key(KEY_LEFTALT, false);
    assert!(hp.switcher().is_none());
    assert_eq!(focused(&hp), Some(name(at)));
    // Alt let go first, Tab still held: it switches, and nothing steps on.
    hp.key(KEY_LEFTALT, true);
    hp.key(KEY_TAB, true);
    hp.key(KEY_LEFTALT, false);
    let switched = focused(&hp);
    wait(&mut hp, Duration::from_millis(600))?;
    assert!(hp.switcher().is_none());
    assert_eq!(focused(&hp), switched);
    hp.key(KEY_TAB, false);
    // A shortcut held does not repeat: Super+Down held on a maximised
    // window restores it, and does not go on to minimise it.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    hp.key(KEY_LEFTMETA, true);
    hp.key(KEY_DOWN, true);
    wait(&mut hp, Duration::from_millis(600))?;
    hp.key(KEY_DOWN, false);
    hp.key(KEY_LEFTMETA, false);
    assert_eq!(hp.minimized(), 0);
    Ok(())
}

/// What Alt+Tab draws of its first item's title on a 1280×800 screen
/// (logical) at `scale`: how many strokes of ink stand apart along the
/// line, its height and its box (physical pixels, x0, y0, x1, y1), and
/// the panel's box, physical too.
fn title_ink(scale: f64) -> anyhow::Result<(usize, i32, [i32; 4], [i32; 4])> {
    let size = |logical: f64| (logical * scale).round() as i32;
    let mut screen = OutputSpec::new(size(1280.0), size(800.0));
    screen.scale = scale;
    let mut hp = Headless::start(HeadlessConfig {
        outputs: vec![screen],
        renderer: RendererChoice::Pixman,
    })?;
    // A title of upright strokes apart: easy to count.
    open(&mut hp, "test.other", (400, 300))?;
    open(&mut hp, "I I I I I I I I", (400, 300))?;
    alt_tab(&mut hp, 1);
    let shown = hp.dispatch_until(WAIT, |hp| {
        hp.switcher().is_some_and(|s| s.2) && hp.has_titles_font()
    })?;
    anyhow::ensure!(shown, "the panel or its font never came");
    appeared(&mut hp)?;
    let [panel, label] = hp
        .switcher_label(0)
        .ok_or_else(|| anyhow::anyhow!("no first item"))?;
    let shot = hp.screenshot(0)?;
    let physical = |(x, y, w, h): (f64, f64, f64, f64)| {
        [
            (x * scale).floor() as i32,
            (y * scale).floor() as i32,
            ((x + w) * scale).ceil() as i32,
            ((y + h) * scale).ceil() as i32,
        ]
    };
    let (panel, label) = (physical(panel), physical(label));
    // From the gap above the title's line (8 px, §11) to the screen's
    // bottom: the title and whatever would spill past it.
    let ink = |x: i32, y: i32| {
        pixel(&shot, f64::from(x), f64::from(y)).is_some_and(|argb| {
            let [b, g, r, _] = argb.to_le_bytes();
            r.min(g).min(b) > 0x90
        })
    };
    let top = label[1] - size(8.0);
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    let mut strokes = 0;
    let mut in_stroke = false;
    for x in label[0]..label[2] {
        let mut column = false;
        for y in top..shot.1 {
            if ink(x, y) {
                column = true;
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
        strokes += usize::from(column && !in_stroke);
        in_stroke = column;
    }
    hp.key(KEY_LEFTALT, false);
    anyhow::ensure!(strokes > 0, "no title drawn at scale {scale}");
    Ok((strokes, y1 - y0, [x0, y0, x1, y1], panel))
}

#[test]
fn alt_tab_titles_keep_their_size_on_a_scaled_screen() -> anyhow::Result<()> {
    let (strokes, height, _, _) = title_ink(1.0)?;
    assert_eq!(strokes, 8, "the eight strokes at scale 1");
    for scale in [1.5, 2.0] {
        let (n, h, ink, panel) = title_ink(scale)?;
        assert_eq!(n, strokes, "the whole title at scale {scale}");
        let ratio = f64::from(h) / f64::from(height);
        assert!(
            (ratio - scale).abs() <= 0.25,
            "{h} px high at scale {scale}, {height} at 1"
        );
        assert!(
            ink[0] >= panel[0] && ink[1] >= panel[1] && ink[2] <= panel[2] && ink[3] <= panel[3],
            "the title {ink:?} spills out of the panel {panel:?} at scale {scale}"
        );
    }
    Ok(())
}

/// The pixels along Alt+Tab's panel at `scale`, with nothing behind it
/// (its windows minimised): a row in its top padding and a column in its
/// left one, physical, away from its rounded corners.
fn panel_lines(scale: f64) -> anyhow::Result<(Vec<u32>, Vec<u32>)> {
    // Odd sizes, so that the panel's edges fall between pixels.
    let mut screen = OutputSpec::new(1919, 1201);
    screen.scale = scale;
    let mut hp = Headless::start(HeadlessConfig {
        outputs: vec![screen],
        renderer: RendererChoice::Pixman,
    })?;
    open(&mut hp, "test.a", (400, 300))?;
    open(&mut hp, "test.b", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_H]);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_H]);
    anyhow::ensure!(hp.minimized() == 2, "not both minimised");
    alt_tab(&mut hp, 1);
    let shown = hp.dispatch_until(WAIT, |hp| hp.switcher().is_some_and(|s| s.2))?;
    anyhow::ensure!(shown, "the panel never showed");
    appeared(&mut hp)?;
    let [(x, y, w, h), _] = hp
        .switcher_label(0)
        .ok_or_else(|| anyhow::anyhow!("no first item"))?;
    let shot = hp.screenshot(0)?;
    // Clear of the rim where it curves at the corners (radius 18).
    let corner = 8.0;
    let (x0, x1) = ((x + corner) * scale, (x + w - corner) * scale);
    let (y0, y1) = ((y + corner) * scale, (y + h - corner) * scale);
    let row_y = (y + 6.0) * scale;
    let column_x = (x + 6.0) * scale;
    let row = (x0 as i32..x1 as i32)
        .filter_map(|px| pixel(&shot, f64::from(px), row_y))
        .collect();
    let column = (y0 as i32..y1 as i32)
        .filter_map(|py| pixel(&shot, column_x, f64::from(py)))
        .collect();
    hp.key(KEY_LEFTALT, false);
    Ok((row, column))
}

#[test]
fn alt_tabs_panel_shows_no_seam_at_fractional_scales() -> anyhow::Result<()> {
    for scale in [1.25, 1.5, 1.75] {
        let (row, column) = panel_lines(scale)?;
        for (name, line) in [("row", row), ("column", column)] {
            anyhow::ensure!(line.len() > 100, "{name} too short at {scale}");
            let odd: Vec<(usize, u32)> = line
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, p)| *p != line[0])
                .collect();
            assert!(
                odd.is_empty(),
                "a seam across the panel's {name} at scale {scale}: {:#x} then {odd:x?}",
                line[0]
            );
        }
    }
    Ok(())
}
