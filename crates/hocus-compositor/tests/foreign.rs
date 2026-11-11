//! Dialogs of other programs' windows through xdg-foreign (#595,
//! docs/design/windows.md §4), as the desktop portal's backend opens its
//! file chooser over the window that asked for it: above its parent, with
//! the keyboard when its parent has it or when its parent is the shell's.
//!
//! The windows are other programs' (processes of their own,
//! [`common::other_program`]): the desktop lets a window of the program
//! with the focus take it, so windows of the test's own process would.

use crate::common;

use common::{Command, Event, Shows, WAIT, other_program};
use hocus_compositor::Headless;

/// Run by [`other_program`] as a process of its own.
#[test]
fn another_program() -> anyhow::Result<()> {
    common::as_other_program()
}

/// The windows' app ids, bottom first.
fn stack(hp: &Headless) -> Vec<String> {
    hp.windows()
        .into_iter()
        .map(|w| w.app_id.unwrap_or_default())
        .collect()
}

/// The app id of the window with the keyboard.
fn keyboard(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.keyboard)
        .and_then(|w| w.app_id)
}

/// A dialog of the test's own process, the child of `handle`, once
/// mapped.
fn dialog(hp: &mut Headless, handle: &str) -> anyhow::Result<common::ClientHandle> {
    let client = common::client(hp.socket(), None)?;
    client.commands.send(Command::OpenDialog {
        app_id: "test.dialog",
        size: (300, 200),
        handle: handle.to_owned(),
    })?;
    let mapped = hp.dispatch_until(WAIT, |hp| stack(hp).iter().any(|a| a == "test.dialog"))?;
    anyhow::ensure!(mapped, "no dialog: {:?}", hp.windows());
    Ok(client)
}

/// The window with the focus exports itself; another program's dialog of
/// it opens above it and takes the keyboard.
#[test]
fn a_dialog_of_the_focused_window_comes_above_it_with_the_keyboard() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let parent = other_program(&mut hp, "test.parent", true, None)?;
    // Another program's window opens behind it.
    let _other = other_program(&mut hp, "test.other", false, None)?;
    assert_eq!(stack(&hp), ["test.other", "test.parent"]);
    assert_eq!(keyboard(&hp).as_deref(), Some("test.parent"));
    let handle = parent.handle(&mut hp)?;
    let _dialog = dialog(&mut hp, &handle)?;
    let focused = hp.dispatch_until(WAIT, |hp| keyboard(hp).as_deref() == Some("test.dialog"))?;
    assert!(focused, "{:?}", hp.windows());
    assert_eq!(stack(&hp), ["test.other", "test.parent", "test.dialog"]);
    Ok(())
}

/// A dialog of a window without the focus opens above its parent, but
/// under the window with the focus, which keeps the keyboard.
#[test]
fn a_dialog_of_a_window_behind_stays_behind_above_its_parent() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let _other = other_program(&mut hp, "test.other", false, None)?;
    let parent = other_program(&mut hp, "test.parent", true, None)?;
    assert_eq!(stack(&hp), ["test.parent", "test.other"]);
    let handle = parent.handle(&mut hp)?;
    let _dialog = dialog(&mut hp, &handle)?;
    common::wait(&mut hp, std::time::Duration::from_millis(200))?;
    assert_eq!(stack(&hp), ["test.parent", "test.dialog", "test.other"]);
    assert_eq!(keyboard(&hp).as_deref(), Some("test.other"));
    Ok(())
}

/// A dialog of the shell's surface (the capture's editor's "Enregistrer
/// sous…") comes in front with the keyboard, though another program's
/// window had the focus; asking for the focus with a token of its own,
/// made from no input, it keeps it.
#[test]
fn a_dialog_of_the_shell_comes_in_front_with_the_keyboard() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let _terminal = other_program(&mut hp, "test.terminal", false, None)?;
    assert_eq!(
        hp.export_for_shell(hocus_link::CAPTURE_EDITOR_NAMESPACE),
        None
    );
    let _editor = common::client(
        hp.socket(),
        Some(Shows::Layer {
            namespace: hocus_link::CAPTURE_EDITOR_NAMESPACE,
            size: (400, 300),
            margin: (0, 0),
            colour: 0xFF20_2030,
        }),
    )?;
    let placed = hp.dispatch_until(WAIT, |hp| {
        !hp.layer_rects(hocus_link::CAPTURE_EDITOR_NAMESPACE)
            .is_empty()
    })?;
    assert!(placed, "the shell's surface never there");
    let handle = hp
        .export_for_shell(hocus_link::CAPTURE_EDITOR_NAMESPACE)
        .expect("a handle");
    assert_eq!(handle.len(), 32);
    let _portal = other_program(&mut hp, "test.portal", false, Some(&handle))?;
    let focused = hp.dispatch_until(WAIT, |hp| keyboard(hp).as_deref() == Some("test.portal"))?;
    assert!(focused, "{:?}", hp.windows());
    assert_eq!(stack(&hp), ["test.terminal", "test.portal"]);
    // Its own token, asked for without input, changes nothing.
    common::wait(&mut hp, std::time::Duration::from_millis(200))?;
    assert_eq!(keyboard(&hp).as_deref(), Some("test.portal"));
    Ok(())
}

/// A handle that names nothing: the import is said gone, and the window
/// opens as any other program's, behind the one with the focus.
#[test]
fn a_handle_that_names_nothing_is_gone() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let _terminal = other_program(&mut hp, "test.terminal", false, None)?;
    let client = dialog(&mut hp, "nothing")?;
    let gone = hp.dispatch_until(WAIT, |_| {
        client.events.try_iter().any(|e| e == Event::ImportGone)
    })?;
    assert!(gone);
    assert_eq!(stack(&hp), ["test.dialog", "test.terminal"]);
    assert_eq!(keyboard(&hp).as_deref(), Some("test.terminal"));
    Ok(())
}
