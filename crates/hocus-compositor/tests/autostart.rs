//! The session's start-up applications (6.15, issue #449): once the bar
//! shows, once per session, the compositor starts the XDG autostart
//! entries that start here; hidden ones, those for other desktops and
//! those whose `TryExec` is missing do not. On a fake XDG tree: never the
//! machine's autostart entries.

mod common;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use common::{Shows, WAIT, client, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};

/// An autostart entry named `name` whose command appends a line to
/// `marks/name`, with `extra` keys.
fn entry(dir: &Path, name: &str, marks: &Path, extra: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mark = marks.join(name);
    // `\\$` is a `$` once the value's escapes and `Exec`'s quoting are
    // undone: the variables are the started program's.
    let text = format!(
        "[Desktop Entry]\nType=Application\nName={name}\n\
         Exec=sh -c \"echo \\\\$(pwd) \\\\$XDG_CURRENT_DESKTOP >> '{}'\"\n{extra}",
        mark.display()
    );
    std::fs::write(dir.join(format!("{name}.desktop")), text)?;
    Ok(())
}

/// The lines `marks/name` has, each ended.
fn lines(marks: &Path, name: &str) -> Vec<String> {
    std::fs::read_to_string(marks.join(name))
        .unwrap_or_default()
        .split_inclusive('\n')
        .filter(|l| l.ends_with('\n'))
        .map(|l| l.trim_end().to_owned())
        .collect()
}

#[test]
fn the_session_starts_the_autostart_entries_once_after_the_bar() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let user = dir.path().join("config/autostart");
    let system = dir.path().join("xdg/autostart");
    let marks = dir.path().join("marks");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&marks)?;
    std::fs::create_dir_all(&work)?;
    // The user's own.
    entry(&user, "mine", &marks, "")?;
    // Installed, in its working directory, out of the launcher only.
    entry(
        &system,
        "installed",
        &marks,
        &format!(
            "Path={}\nNoDisplay=true\nOnlyShowIn=GNOME;Hocuspanda;\n",
            work.display()
        ),
    )?;
    // Installed, switched off by the user's hidden copy.
    entry(&system, "off", &marks, "")?;
    entry(&user, "off", &marks, "Hidden=true\n")?;
    entry(&system, "hidden", &marks, "Hidden=true\n")?;
    entry(&system, "other", &marks, "OnlyShowIn=OtherDesktop;\n")?;
    entry(&system, "not-here", &marks, "NotShowIn=Hocuspanda;\n")?;
    entry(&system, "missing", &marks, "TryExec=/nonexistent/missing\n")?;

    let mut hp = Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
    )?;
    hp.autostart(vec![
        user.clone(),
        system.clone(),
        PathBuf::from("/nonexistent/xdg"),
    ]);

    // No bar yet: nothing starts.
    wait(&mut hp, Duration::from_millis(600))?;
    assert!(
        std::fs::read_dir(&marks)?.next().is_none(),
        "started before the bar showed"
    );

    let _bar = client(hp.socket(), Some(Shows::ClearBar))?;
    let started = hp.dispatch_until(WAIT, |_| {
        !lines(&marks, "mine").is_empty() && !lines(&marks, "installed").is_empty()
    })?;
    assert!(started, "the start-up applications did not start");
    assert_eq!(
        lines(&marks, "mine"),
        [format!("{} Hocuspanda", dir_of_compositor())]
    );
    assert_eq!(
        lines(&marks, "installed"),
        [format!("{} Hocuspanda", work.display())]
    );

    // Another bar, as a shell started again draws: nothing starts twice.
    let _again = client(hp.socket(), Some(Shows::ClearBar))?;
    wait(&mut hp, Duration::from_millis(800))?;
    assert_eq!(lines(&marks, "mine").len(), 1);
    assert_eq!(lines(&marks, "installed").len(), 1);
    let mut started: Vec<String> = std::fs::read_dir(&marks)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    started.sort();
    assert_eq!(started, ["installed", "mine"]);
    Ok(())
}

/// The working directory the compositor's programs start in, without a
/// `Path`: the test's own.
fn dir_of_compositor() -> String {
    std::env::current_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_default()
}
