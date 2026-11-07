//! The pointer's focus follows what shows under it when that changes
//! without the pointer moving (#737): after Alt+Tab raises another window
//! under a still pointer, the wheel goes to the window now in front (an
//! enter first), not to the one that was.

mod common;

use std::time::Duration;

use common::{
    ClientHandle, Event, KEY_LEFTALT, KEY_TAB, Pointed, Shows, compositor, open_showing, wait,
};
use hocus_compositor::{Headless, model::Rect};

fn pointed(client: &ClientHandle) -> Vec<Pointed> {
    client
        .events
        .try_iter()
        .filter_map(|e| match e {
            Event::Pointer(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn id_of(hp: &Headless, app_id: &str) -> anyhow::Result<hocus_compositor::model::WindowId> {
    hp.all_windows()
        .into_iter()
        .find(|(_, app, _, _)| app.as_deref() == Some(app_id))
        .map(|(id, _, _, _)| id)
        .ok_or_else(|| anyhow::anyhow!("no window {app_id}"))
}

#[test]
fn after_alt_tab_the_wheel_goes_to_the_window_now_under_the_pointer() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (_, back) = open_showing(
        &mut hp,
        "test.back",
        Shows::Pointing {
            app_id: "test.back",
            size: (600, 400),
        },
    )?;
    let (_, front) = open_showing(
        &mut hp,
        "test.front",
        Shows::Pointing {
            app_id: "test.front",
            size: (600, 400),
        },
    )?;
    // Overlapping, the second in front.
    let back_id = id_of(&hp, "test.back")?;
    let front_id = id_of(&hp, "test.front")?;
    hp.place_window(back_id, Rect::new(100, 100, 600, 400));
    hp.place_window(front_id, Rect::new(400, 300, 600, 400));
    wait(&mut hp, Duration::from_millis(200))?;
    // The pointer over the overlap: the front window has it.
    hp.pointer_move(550.0, 400.0);
    wait(&mut hp, Duration::from_millis(100))?;
    assert!(
        pointed(&front).contains(&Pointed::Enter),
        "the front window has no enter"
    );
    pointed(&back);

    // Alt+Tab: the other window raised and focused, the pointer still.
    hp.key(KEY_LEFTALT, true);
    hp.key(KEY_TAB, true);
    hp.key(KEY_TAB, false);
    hp.key(KEY_LEFTALT, false);
    wait(&mut hp, Duration::from_millis(200))?;
    let focused = hp
        .windows()
        .into_iter()
        .find(|w| w.focused)
        .and_then(|w| w.app_id);
    assert_eq!(focused.as_deref(), Some("test.back"));

    // The wheel: to the window now in front, after an enter; nothing to
    // the one behind but its leave.
    hp.wheel(1);
    wait(&mut hp, Duration::from_millis(200))?;
    let to_back = pointed(&back);
    let to_front = pointed(&front);
    let enter = to_back.iter().position(|p| *p == Pointed::Enter);
    let axis = to_back.iter().position(|p| *p == Pointed::Axis);
    assert!(
        matches!((enter, axis), (Some(e), Some(a)) if e < a),
        "the window in front: {to_back:?}"
    );
    assert!(
        !to_front.contains(&Pointed::Axis),
        "the window behind: {to_front:?}"
    );
    assert!(
        to_front.contains(&Pointed::Leave),
        "the window behind: {to_front:?}"
    );
    Ok(())
}
