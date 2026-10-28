//! The session's shortcuts, headless: a key chord pressed as a keyboard
//! would send it runs its action (docs/design/windows.md §13).

use std::{path::Path, time::Duration};

use hocus_compositor::{Config, Ending, Headless, HeadlessConfig, RendererChoice};

const WAIT: Duration = Duration::from_secs(10);
const KEY_LEFTMETA: u32 = 125;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTALT: u32 = 56;
const KEY_ENTER: u32 = 28;
const KEY_E: u32 = 18;
const KEY_T: u32 = 20;

fn compositor(config: Config) -> anyhow::Result<Headless> {
    Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )
}

/// Presses the keys in order, then releases them in reverse.
fn chord(hp: &mut Headless, keys: &[u32]) {
    for key in keys {
        hp.key(*key, true);
    }
    for key in keys.iter().rev() {
        hp.key(*key, false);
    }
}

/// A terminal command that writes what it was told about the session into
/// `marker`, all at once.
fn terminal_writing(marker: &Path) -> String {
    let tmp = marker.with_extension("tmp");
    format!(
        "printf '%s %s %s' \"$WAYLAND_DISPLAY\" \"$XDG_SESSION_TYPE\" \"$XDG_CURRENT_DESKTOP\" \
         > '{}' && mv '{}' '{}'",
        tmp.display(),
        tmp.display(),
        marker.display()
    )
}

#[test]
fn super_return_opens_the_terminal_in_the_session() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let marker = dir.path().join("terminal");
    let mut config = Config::default();
    config.programs.terminal = terminal_writing(&marker);
    let mut hp = compositor(config)?;
    // Return alone, or Super alone, is the client's.
    chord(&mut hp, &[KEY_ENTER]);
    chord(&mut hp, &[KEY_LEFTMETA]);
    hp.dispatch_until(Duration::from_millis(300), |_| marker.exists())?;
    assert!(!marker.exists(), "started without its shortcut");

    chord(&mut hp, &[KEY_LEFTMETA, KEY_ENTER]);
    let started = hp.dispatch_until(WAIT, |_| marker.exists())?;
    assert!(started, "Super+Return started nothing");
    let socket = hp
        .socket()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    assert_eq!(
        std::fs::read_to_string(&marker)?,
        format!("{} wayland Hocuspanda", socket.unwrap_or_default())
    );
    assert_eq!(hp.ending(), None);
    Ok(())
}

#[test]
fn super_shift_e_logs_out() -> anyhow::Result<()> {
    let mut hp = compositor(Config::default())?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_E]);
    hp.dispatch(Duration::from_millis(10))?;
    assert_eq!(hp.ending(), None, "Super+E is not the shortcut");
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_E]);
    hp.dispatch(Duration::from_millis(10))?;
    assert_eq!(hp.ending(), Some(Ending::LoggedOut));
    Ok(())
}

#[test]
fn shortcuts_follow_the_configuration() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let marker = dir.path().join("terminal");
    let mut config = Config::default();
    config.programs.terminal = terminal_writing(&marker);
    config.shortcuts.terminal = vec!["Ctrl+Alt+T".parse()?];
    config.shortcuts.log_out = Vec::new();
    let mut hp = compositor(config)?;

    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_E]);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_ENTER]);
    hp.dispatch_until(Duration::from_millis(300), |_| marker.exists())?;
    assert_eq!(hp.ending(), None, "log-out is turned off");
    assert!(!marker.exists(), "Super+Return is no longer the terminal's");

    chord(&mut hp, &[KEY_LEFTCTRL, KEY_LEFTALT, KEY_T]);
    let started = hp.dispatch_until(WAIT, |_| marker.exists())?;
    assert!(started, "Ctrl+Alt+T started nothing");
    Ok(())
}
