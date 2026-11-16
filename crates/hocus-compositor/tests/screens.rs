//! Two screens, headless (docs/design/windows.md §9): a snap, by dragging
//! or by the keys, lands on the screen it was made on, and stays there as
//! its work area changes, through fullscreen and back, and as a snapped
//! pair is resized; a window's popup opens on the window's screen. A
//! window's drawing builds the scenes of its own screen alone (#826); a
//! window committing nothing new builds none, its frame callbacks still
//! answered at each refresh (#840).

use crate::common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::Duration,
};

use common::{
    BTN_LEFT, Command, Event, KEY_LEFT, KEY_LEFTALT, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_RIGHT,
    KEY_UP, Shows, WAIT, chord, client, open, open_showing, open_with, rect_of, two_screens, wait,
    wait_for_rect,
};
use hocus_compositor::{
    Headless,
    model::{GAP, Mode, Rect, Zone},
};

fn id_of(hp: &Headless, app_id: &str) -> Option<hocus_compositor::model::WindowId> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.id)
}

/// The second screen's work area once it has one.
fn second_area(hp: &Headless) -> anyhow::Result<Rect> {
    let areas = hp.work_areas();
    anyhow::ensure!(areas.len() == 2, "two work areas: {areas:?}");
    Ok(areas[1])
}

/// A bar on the second screen: its work area shrinks; the new one.
fn bar_on_second(hp: &mut Headless, before: Rect) -> anyhow::Result<Rect> {
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    let bar = client(hp.socket(), Some(Shows::Bar { height: 40 }))?;
    std::mem::forget(bar);
    let shrunk = hp.dispatch_until(WAIT, |hp| {
        hp.work_areas().get(1).is_some_and(|a| a.y == before.y + 40)
    })?;
    anyhow::ensure!(shrunk, "work areas: {:?}", hp.work_areas());
    second_area(hp)
}

#[test]
fn a_drag_snaps_on_the_screen_under_the_pointer_and_stays_there() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let area = second_area(&hp)?;
    // Opened on the first screen, held near its right edge.
    hp.pointer_move(960.0, 540.0);
    let (start, window) = open_with(&mut hp, "test.drag", (400, 300), false)?;
    assert!(start.right() < 1920, "on the first screen: {start:?}");
    let (x, y) = (f64::from(start.right() - 20), f64::from(start.y + 150));
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
    // The second screen's top-left corner: most of the window, and its
    // centre, still on the first.
    hp.pointer_move(1921.0, 2.0);
    let moved = rect_of(&hp, "test.drag").expect("mapped");
    assert!(
        moved.centre().0 < 1920.0,
        "its centre on the first: {moved:?}"
    );
    wait(&mut hp, Duration::from_millis(120))?;
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTALT, false);
    wait_for_rect(&mut hp, "test.drag", Zone::TopLeft.rect(area))?;
    let id = id_of(&hp, "test.drag").expect("mapped");
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::TopLeft));
    // Its screen's work area changes: it follows, on that screen.
    let area = bar_on_second(&mut hp, area)?;
    wait_for_rect(&mut hp, "test.drag", Zone::TopLeft.rect(area))?;
    // Fullscreen over its screen, and back in its quarter there.
    window.commands.send(Command::Fullscreen(true))?;
    wait_for_rect(&mut hp, "test.drag", Rect::new(1920, 0, 2560, 1440))?;
    window.commands.send(Command::Fullscreen(false))?;
    wait_for_rect(&mut hp, "test.drag", Zone::TopLeft.rect(area))?;
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::TopLeft));
    Ok(())
}

#[test]
fn the_keys_snap_on_the_windows_screen_and_a_pair_stays_there() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let area = second_area(&hp)?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    open(&mut hp, "test.left", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.left", Zone::Left.rect(area))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_UP]);
    wait_for_rect(&mut hp, "test.left", Zone::TopLeft.rect(area))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    wait_for_rect(&mut hp, "test.left", area)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.left", Zone::Left.rect(area))?;
    open(&mut hp, "test.right", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait_for_rect(&mut hp, "test.right", Zone::Right.rect(area))?;
    // The pair's handle, in the middle of the seam: both resize, and stay.
    let left = Zone::Left.rect(area);
    let (x, y) = (
        f64::from(left.right() + GAP / 2),
        f64::from(left.y) + f64::from(left.height) / 2.0,
    );
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x - 200.0, y);
    hp.button(BTN_LEFT, false);
    let narrower = Rect::new(left.x, left.y, left.width - 200, left.height);
    wait_for_rect(&mut hp, "test.left", narrower)?;
    let right = Zone::Right.rect(area);
    let wider = Rect::new(
        narrower.right() + GAP,
        right.y,
        right.right() - narrower.right() - GAP,
        right.height,
    );
    wait_for_rect(&mut hp, "test.right", wider)?;
    for app in ["test.left", "test.right"] {
        let id = id_of(&hp, app).expect("mapped");
        assert!(matches!(hp.mode(id), Mode::Snapped(_)), "{app} snapped");
    }
    // A re-layout of that screen puts them back in their halves there.
    let area = bar_on_second(&mut hp, area)?;
    wait_for_rect(&mut hp, "test.left", Zone::Left.rect(area))?;
    wait_for_rect(&mut hp, "test.right", Zone::Right.rect(area))?;
    Ok(())
}

/// A window's popup opens on the window's screen, at its anchor, even when
/// the window's own shadow reaches onto the screen beside it (a browser's
/// tab preview).
#[test]
fn a_popup_opens_on_its_windows_screen() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let (_, window) = open_showing(
        &mut hp,
        "test.tabs",
        Shows::Shadowed {
            app_id: "test.tabs",
            size: (800, 600),
            margin: 30,
        },
    )?;
    let id = id_of(&hp, "test.tabs").ok_or_else(|| anyhow::anyhow!("no window"))?;
    // 10 px into the second screen: its shadow 20 px onto the first.
    let rect = Rect::new(1920 + 10, 200, 800, 600);
    hp.place_window(id, rect);
    wait_for_rect(&mut hp, "test.tabs", rect)?;
    window.commands.send(Command::Popup {
        anchor: (200, 0, 150, 30),
        size: (300, 200),
    })?;
    let mut at = None;
    hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = window.events.try_recv() {
            if let Event::PopupAt(x, y) = event {
                at = Some((x, y));
            }
        }
        at.is_some()
    })?;
    assert_eq!(
        at,
        Some((200, 30)),
        "under its anchor, on the second screen"
    );
    Ok(())
}

/// A window drawing on and on (a video, a terminal scrolling) asks for
/// the scenes of its screen alone: the other screen, where nothing
/// changes, builds none (#826).
#[test]
fn a_windows_drawing_builds_its_own_screens_scenes_alone() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let (_, window) = open_with(&mut hp, "test.video", (800, 600), false)?;
    let id = id_of(&hp, "test.video").ok_or_else(|| anyhow::anyhow!("no window"))?;
    let rect = Rect::new(400, 200, 800, 600);
    hp.place_window(id, rect);
    wait_for_rect(&mut hp, "test.video", rect)?;
    common::at_rest(&mut hp)?;
    let (first, second) = (hp.scenes_on(0), hp.scenes_on(1));
    // A second of frames at 60 a second, each another colour.
    for i in 0..60u32 {
        let colour = 0xff00_0000 | (i * 4) << 16 | (255 - i * 4);
        window.commands.send(Command::Header(colour))?;
        wait(&mut hp, Duration::from_micros(16_667))?;
    }
    let (first, second) = (hp.scenes_on(0) - first, hp.scenes_on(1) - second);
    assert!(first >= 20, "its screen drew it: {first} scenes");
    assert!(
        second <= 2,
        "the other screen, where nothing changed, built {second} scenes (its own: {first})"
    );
    Ok(())
}

/// A window on the first screen, then committing nothing new at each of
/// its frame callbacks for a second (`again`: its buffer attached again
/// without damage, else its frame callback alone): the scenes and frames
/// its screen built and drew, the refreshes that told it of a frame
/// without a scene, and the frame callbacks it was answered.
fn idle_for_a_second(
    again: bool,
) -> anyhow::Result<(Headless, common::ClientHandle, Arc<AtomicU64>, [u64; 4])> {
    let mut hp = two_screens()?;
    let (_, window) = open_with(&mut hp, "test.idle", (800, 600), false)?;
    let id = id_of(&hp, "test.idle").ok_or_else(|| anyhow::anyhow!("no window"))?;
    let rect = Rect::new(400, 200, 800, 600);
    hp.place_window(id, rect);
    wait_for_rect(&mut hp, "test.idle", rect)?;
    common::at_rest(&mut hp)?;
    let done = Arc::new(AtomicU64::new(0));
    window.commands.send(Command::Idle {
        again,
        done: done.clone(),
    })?;
    // Its first commits are looked at once: from then on, a second.
    wait(&mut hp, Duration::from_millis(100))?;
    let (scenes, frames, answered) = (hp.scenes_on(0), hp.frames_on(0), done.load(Relaxed));
    hp.take_frame_cost(0);
    wait(&mut hp, Duration::from_secs(1))?;
    let counts = [
        hp.scenes_on(0) - scenes,
        hp.frames_on(0) - frames,
        hp.take_frame_cost(0).unchanged,
        done.load(Relaxed) - answered,
    ];
    eprintln!(
        "a second idle (again: {again}): {} scenes, {} frames, {} refreshes without a scene, \
         {} frame callbacks answered",
        counts[0], counts[1], counts[2], counts[3]
    );
    Ok((hp, window, done, counts))
}

/// A window committing its frame callbacks alone, at each one, as an
/// application idling (#840): its screen builds no scene for it, and
/// answers them at each refresh (60 a second headless), not faster.
#[test]
fn a_window_committing_its_frame_callbacks_alone_builds_no_scenes() -> anyhow::Result<()> {
    let (_hp, _window, _, [scenes, frames, unchanged, answered]) = idle_for_a_second(false)?;
    assert!(scenes <= 2, "{scenes} scenes for nothing new");
    assert_eq!(frames, 0, "{frames} frames drawn for nothing new");
    assert!(
        (20..=62).contains(&answered),
        "{answered} frame callbacks in a second"
    );
    // Told by refreshes without a scene.
    assert!(unchanged >= 20, "{unchanged} refreshes without a scene");
    Ok(())
}

/// The same with its buffer attached again at each commit, without
/// damage (#840).
#[test]
fn a_window_attaching_its_buffer_again_builds_no_scenes() -> anyhow::Result<()> {
    let (_hp, _window, _, [scenes, frames, unchanged, answered]) = idle_for_a_second(true)?;
    assert!(scenes <= 2, "{scenes} scenes for nothing new");
    assert_eq!(frames, 0, "{frames} frames drawn for nothing new");
    assert!(
        (20..=62).contains(&answered),
        "{answered} frame callbacks in a second"
    );
    // Told by refreshes without a scene.
    assert!(unchanged >= 20, "{unchanged} refreshes without a scene");
    Ok(())
}

/// A window idling so that draws something new between: its screen
/// draws it, each time (#840).
#[test]
fn an_idle_window_drawing_is_drawn() -> anyhow::Result<()> {
    let (mut hp, window, done, _) = idle_for_a_second(true)?;
    let (scenes, frames) = (hp.scenes_on(0), hp.frames_on(0));
    for i in 0..20u32 {
        let colour = 0xff00_0000 | (i * 12) << 16 | (255 - i * 12);
        window.commands.send(Command::Header(colour))?;
        wait(&mut hp, Duration::from_millis(50))?;
    }
    let (scenes, frames) = (hp.scenes_on(0) - scenes, hp.frames_on(0) - frames);
    assert!(
        frames >= 15,
        "{frames} frames for 20 drawings ({scenes} scenes)"
    );
    // It went on idling between.
    let answered = done.load(Relaxed);
    wait(&mut hp, Duration::from_millis(200))?;
    assert!(done.load(Relaxed) > answered, "its frame callbacks stopped");
    Ok(())
}

/// The pointer as a session starts: the display manager's place is not
/// handed over, so as the screens light up it goes to the primary
/// screen's middle, never left in a screen's corner, until the user moves
/// it; screens lit after leave it where the user put it. A headless
/// compositor keeps it at 0, 0 for its tests, as unplaced here.
#[test]
fn the_pointer_starts_in_the_primary_screens_middle_until_moved() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    assert_eq!(hp.pointer(), (0.0, 0.0));
    hp.pointer_unplaced();
    hp.plug_screen(
        "HEADLESS-2",
        hocus_compositor::OutputSpec::new(2560, 1440),
        None,
    )?;
    // The first screen is the primary one (its bar's).
    assert_eq!(hp.pointer(), (960.0, 540.0));
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.overview(), None);
    // Moved by the user: a screen lit after leaves it there.
    hp.pointer_move(100.0, 120.0);
    hp.unplug("HEADLESS-2")?;
    hp.plug("HEADLESS-2")?;
    assert_eq!(hp.pointer(), (100.0, 120.0));
    Ok(())
}
