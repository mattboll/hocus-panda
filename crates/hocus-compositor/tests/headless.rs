//! The compositor's base, headless: real Wayland clients (on threads of
//! their own) connect to an in-process compositor, which the test drives.

mod common;

use std::time::Duration;

use anyhow::Context;
use common::{BTN_LEFT, Event, KEY_LEFTMETA, Shows, WAIT, client, compositor, open};
use hocus_compositor::{Headless, model::Rect};

#[test]
fn a_new_window_opens_at_the_centre_of_the_work_area() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    // 1920×1080: 8 px in on every side, and the full bar's place (196 px,
    // 8 px from the edge) kept on the right.
    assert_eq!(hp.work_area(), Some(Rect::new(8, 8, 1700, 1064)));
    let rect = open(&mut hp, "test.centre", (400, 300))?;
    assert_eq!(rect, Rect::new(8 + 650, 8 + 382, 400, 300));
    // A new window has the focus, and its surface the keyboard.
    let window = &hp.windows()[0];
    assert!(window.focused && window.keyboard, "{window:?}");
    // The frame was drawn once something changed.
    let drawn = hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
    assert!(drawn, "no frame drawn");
    Ok(())
}

#[test]
fn a_click_focuses_and_raises_the_window_under_the_pointer() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let first = open(&mut hp, "test.first", (400, 300))?;
    let second = open(&mut hp, "test.second", (200, 100))?;
    let focused = |hp: &Headless| {
        hp.windows()
            .into_iter()
            .find(|w| w.focused)
            .and_then(|w| w.app_id)
    };
    assert_eq!(focused(&hp).as_deref(), Some("test.second"));
    // Within the first, clear of the second (centred over it).
    let (x, y) = (f64::from(first.x + 10), f64::from(first.y + 10));
    assert!(!(second.x..second.x + second.width).contains(&(first.x + 10)));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    hp.dispatch(Duration::from_millis(10))?;
    assert_eq!(focused(&hp).as_deref(), Some("test.first"));
    let windows = hp.windows();
    let top = windows.last().context("no window")?;
    assert_eq!(top.app_id.as_deref(), Some("test.first"), "raised");
    assert!(top.keyboard, "the keyboard follows the click");
    // A click where no window is changes nothing.
    hp.pointer_move(2.0, 2.0);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    assert_eq!(focused(&hp).as_deref(), Some("test.first"));
    Ok(())
}

#[test]
fn super_and_drag_moves_a_window() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.moved", (400, 300))?;
    let (x, y) = (f64::from(start.x + 200), f64::from(start.y + 150));
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 60.0, y + 40.0);
    hp.pointer_move(x + 100.0, y + 50.0);
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    let rect = hp.windows()[0].rect;
    assert_eq!(
        rect,
        Rect::new(start.x + 100, start.y + 50, 400, 300),
        "the window follows the pointer"
    );
    // Without Super, a drag is the application's.
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 300.0, y + 50.0);
    hp.button(BTN_LEFT, false);
    assert_eq!(hp.windows()[0].rect, rect);
    Ok(())
}

#[test]
fn a_bars_exclusive_zone_comes_off_the_work_area() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let _bar = client(hp.socket(), Some(Shows::Bar { height: 40 }))?;
    let shrunk = hp.dispatch_until(WAIT, |hp| {
        hp.work_area() == Some(Rect::new(8, 48, 1700, 1024))
    })?;
    assert!(shrunk, "work area: {:?}", hp.work_area());
    // New windows open at the centre of what is left.
    let rect = open(&mut hp, "test.under-bar", (400, 300))?;
    assert_eq!(rect, Rect::new(8 + 650, 48 + 362, 400, 300));
    Ok(())
}

#[test]
fn the_protocols_of_the_base_are_offered() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let globals = client(hp.socket(), None)?;
    let mut seen = None;
    hp.dispatch_until(WAIT, |_| {
        if seen.is_none()
            && let Ok(Event::Globals(names)) = globals.events.try_recv()
        {
            seen = Some(names);
        }
        seen.is_some()
    })?;
    let seen = seen.context("the client never listed the globals")?;
    let has = |name: &str| seen.iter().any(|g| g.split(' ').next() == Some(name));
    for name in [
        "wl_compositor",
        "wl_shm",
        "wl_seat",
        "wl_output",
        "zxdg_output_manager_v1",
        "xdg_wm_base",
        "zxdg_decoration_manager_v1",
        "zwlr_layer_shell_v1",
        "zwp_linux_dmabuf_v1",
        "wp_viewporter",
        "wp_fractional_scale_manager_v1",
        "wp_presentation",
        "xdg_activation_v1",
        "wp_cursor_shape_manager_v1",
        "wl_data_device_manager",
        "zwp_primary_selection_device_manager_v1",
        "zwp_pointer_gestures_v1",
        "zwp_relative_pointer_manager_v1",
    ] {
        assert!(has(name), "{name} is not offered: {seen:?}");
    }
    assert!(hp.has_dmabuf());
    // Headless with pixman, no device to give feedback for: version 3.
    // Nested and on screens, the GPU's render node gives version 4 or more.
    assert!(
        seen.iter().any(|g| g == "zwp_linux_dmabuf_v1 v3"),
        "{seen:?}"
    );
    Ok(())
}
