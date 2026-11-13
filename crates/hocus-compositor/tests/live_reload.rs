//! The shell restarted alone in the running session (issue #781;
//! docs/workflow.md, "Development loop"): `hocus msg shell restart
//! [--program PATH]`, asked on the control socket as `hocus` asks it.
//!
//! - A restart ends the program and starts it again at once: the windows
//!   stay, the bar's space stays kept, the bar comes back.
//! - A program given runs in place of the installed one, the status says
//!   so, and `installed` lets go of it; one that cannot run is refused.
//! - A program is given only in developer mode (`[developer] live-reload`,
//!   #783): off, as by default, it is refused and the installed one runs on.
//! - A given program that keeps ending, or does not start, gives way to
//!   the installed one: the status says it, and the shell says it in a
//!   notification.

use crate::common;

use std::{
    io::{Read as _, Write as _},
    os::unix::{fs::PermissionsExt as _, net::UnixStream},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use hocus_compositor::{Config, Developer, Headless, HeadlessConfig, RendererChoice};

const WAIT: Duration = Duration::from_secs(30);

fn pixman() -> HeadlessConfig {
    HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    }
}

/// The developer mode on: a program can be given.
fn developer() -> Config {
    Config {
        developer: Developer {
            live_reload: true,
            ..Developer::default()
        },
        ..Config::default()
    }
}

/// A line asked on the control socket, from a thread as `hocus msg` asks
/// it, while the loop runs: the answer.
fn ask(hp: &mut Headless, line: &str) -> anyhow::Result<String> {
    let path = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let line = format!("{line}\n");
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let asked = (|| -> std::io::Result<String> {
            let mut stream = UnixStream::connect(&path)?;
            stream.set_read_timeout(Some(WAIT))?;
            stream.write_all(line.as_bytes())?;
            let mut answer = String::new();
            stream.read_to_string(&mut answer)?;
            Ok(answer)
        })();
        let _ = said.send(asked);
    });
    let mut answer = None;
    hp.dispatch_until(WAIT, |_| {
        if answer.is_none() {
            answer = heard.try_recv().ok();
        }
        answer.is_some()
    })?;
    Ok(answer.context("no answer")??)
}

/// What a status says after `word`.
fn said<'a>(status: &'a str, word: &str) -> Option<&'a str> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(word)?.strip_prefix(' '))
}

/// The shell's status once it runs, connected, its bar drawn, as process
/// `other` than the one given.
fn shell_back(hp: &mut Headless, other: Option<i32>) -> anyhow::Result<String> {
    let back = hp.dispatch_until(WAIT, |hp| {
        hp.shell_connected()
            && hp.shell_pid().is_some()
            && hp.shell_pid() != other
            && !hp.drawn_layers(hocus_link::NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(
        back,
        "the shell never came back:\n{}",
        ask(hp, "msg shell status")?
    );
    ask(hp, "msg shell status")
}

/// The file a process runs.
fn exe(pid: Option<i32>) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{}/exe", pid?)).ok()
}

/// An executable script at `path`.
fn script_at(path: &Path, text: &str) -> anyhow::Result<()> {
    std::fs::write(path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// A restart ends the shell and starts it again at once, as many times as
/// asked: the window stays where it is, the bar's space stays kept all
/// along, the bar is drawn again by another process.
#[test]
fn a_restart_keeps_the_windows_and_the_bar_comes_back() -> anyhow::Result<()> {
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let shell = common::shell_script(dir.path(), &shell, &[])?;
    let mut hp = Headless::start_with_shell(pixman(), Config::default(), Some(shell.clone()))?;
    let bar = common::drawn(&mut hp, hocus_link::NAMESPACE)?;
    let window = common::open(&mut hp, "test.kept", (640, 480))?;
    let area = hp.work_area();
    let first = hp.shell_pid();
    let status = shell_back(&mut hp, None)?;
    assert_eq!(said(&status, "state"), Some("running"), "{status}");
    assert_eq!(said(&status, "program"), shell.to_str(), "{status}");
    assert_eq!(said(&status, "installed"), shell.to_str(), "{status}");
    assert_eq!(said(&status, "given"), Some("no"), "{status}");
    assert_eq!(said(&status, "starts"), Some("1"), "{status}");
    assert_eq!(said(&status, "bars"), Some("1 of 1"), "{status}");
    assert_eq!(
        said(&status, "pid").and_then(|pid| pid.parse().ok()),
        first,
        "{status}"
    );
    assert!(said(&status, "since").is_some(), "{status}");

    let asked = Instant::now();
    assert_eq!(
        ask(&mut hp, "msg shell restart")?,
        format!("restarting\nprogram {}\n", shell.display())
    );
    // All along: the window where it was, the bar's space kept.
    let mut moved = false;
    let back = hp.dispatch_until(WAIT, |hp| {
        moved |= hp.work_area() != area || common::rect_of(hp, "test.kept") != Some(window);
        hp.shell_connected()
            && hp.shell_pid().is_some()
            && hp.shell_pid() != first
            && !hp.drawn_layers(hocus_link::NAMESPACE).is_empty()
    })?;
    let took = asked.elapsed();
    assert!(back, "the bar never came back");
    assert!(!moved, "a window or the work area moved meanwhile");
    eprintln!("shell restart: the bar is back {took:.2?} after the restart was asked");
    assert_eq!(hp.drawn_layers(hocus_link::NAMESPACE), bar, "the same bar");
    let status = ask(&mut hp, "msg shell status")?;
    assert_eq!(said(&status, "state"), Some("running"), "{status}");
    assert_eq!(said(&status, "starts"), Some("2"), "{status}");
    assert_eq!(said(&status, "bars"), Some("1 of 1"), "{status}");

    // Asked twice in a row, whether the second finds it ending still or
    // back already: it comes back.
    let second = hp.shell_pid();
    ask(&mut hp, "msg shell restart")?;
    ask(&mut hp, "msg shell restart")?;
    let status = shell_back(&mut hp, second)?;
    let starts: Option<u32> = said(&status, "starts").and_then(|n| n.parse().ok());
    assert!(matches!(starts, Some(3 | 4)), "{status}");
    common::at_rest(&mut hp)?;
    let status = ask(&mut hp, "msg shell status")?;
    assert_eq!(said(&status, "state"), Some("running"), "{status}");
    assert_eq!(common::rect_of(&hp, "test.kept"), Some(window));
    assert_eq!(hp.work_area(), area);
    Ok(())
}

/// A program given runs in place of the installed one, and the status
/// says so; `installed` lets go of it. One that cannot run is refused,
/// and the shell that runs is left alone.
#[test]
fn a_program_given_runs_and_the_status_says_so() -> anyhow::Result<()> {
    let built = common::shell_program();
    let dir = tempfile::tempdir()?;
    let installed = common::shell_script(dir.path(), &built, &[])?;
    // The same shell at another path, as a developer's build is.
    let dev = dir.path().join("dev");
    std::fs::create_dir(&dev)?;
    let copy = dev.join("hocuspanda-shell");
    std::fs::copy(&built, &copy)?;
    let given = common::shell_script(&dev, &copy, &[])?;
    let mut hp = Headless::start_with_shell(pixman(), developer(), Some(installed.clone()))?;
    shell_back(&mut hp, None)?;
    let first = hp.shell_pid();
    assert_eq!(exe(first), Some(built.clone()));

    // Refused: not absolute, not there, not executable.
    let plain = dev.join("plain");
    std::fs::write(&plain, "#!/bin/sh\n")?;
    for (program, why) in [
        ("dev/shell".to_owned(), "not an absolute path"),
        (dev.join("none").display().to_string(), "No such file"),
        (plain.display().to_string(), "not executable"),
        (dev.display().to_string(), "not a file"),
    ] {
        let answer = ask(&mut hp, &format!("msg shell restart program {program}"))?;
        assert!(
            answer.starts_with("refused: ") && answer.contains(why),
            "{program}: {answer}"
        );
    }
    common::wait(&mut hp, Duration::from_millis(200))?;
    assert_eq!(hp.shell_pid(), first, "the shell was left alone");
    let status = ask(&mut hp, "msg shell status")?;
    assert_eq!(said(&status, "given"), Some("no"), "{status}");
    assert_eq!(said(&status, "starts"), Some("1"), "{status}");

    assert_eq!(
        ask(
            &mut hp,
            &format!("msg shell restart program {}", given.display())
        )?,
        format!("restarting\nprogram {}\n", given.display())
    );
    let status = shell_back(&mut hp, first)?;
    assert_eq!(said(&status, "state"), Some("running"), "{status}");
    assert_eq!(said(&status, "program"), given.to_str(), "{status}");
    assert_eq!(said(&status, "installed"), installed.to_str(), "{status}");
    assert_eq!(said(&status, "given"), Some("yes"), "{status}");
    let second = hp.shell_pid();
    assert_eq!(exe(second), Some(copy.clone()), "the program given runs");

    // A plain restart keeps the program given.
    ask(&mut hp, "msg shell restart")?;
    let status = shell_back(&mut hp, second)?;
    assert_eq!(said(&status, "program"), given.to_str(), "{status}");
    let third = hp.shell_pid();
    assert_eq!(exe(third), Some(copy));

    assert_eq!(
        ask(&mut hp, "msg shell restart installed")?,
        format!("restarting\nprogram {}\n", installed.display())
    );
    let status = shell_back(&mut hp, third)?;
    assert_eq!(said(&status, "program"), installed.to_str(), "{status}");
    assert_eq!(said(&status, "given"), Some("no"), "{status}");
    assert_eq!(said(&status, "fell-back"), None, "{status}");
    assert_eq!(exe(hp.shell_pid()), Some(built));
    Ok(())
}

/// Without the developer mode, as by default, a program given is refused
/// with how to turn it on, and the installed shell keeps running; a plain
/// restart and `installed` are still taken.
#[test]
fn a_program_given_is_refused_without_the_developer_mode() -> anyhow::Result<()> {
    let built = common::shell_program();
    let dir = tempfile::tempdir()?;
    let installed = common::shell_script(dir.path(), &built, &[])?;
    let dev = dir.path().join("dev");
    std::fs::create_dir(&dev)?;
    let given = common::shell_script(&dev, &built, &[])?;
    let mut hp = Headless::start_with_shell(pixman(), Config::default(), Some(installed.clone()))?;
    let status = shell_back(&mut hp, None)?;
    assert_eq!(said(&status, "live-reload"), Some("off"), "{status}");
    let first = hp.shell_pid();

    let answer = ask(
        &mut hp,
        &format!("msg shell restart program {}", given.display()),
    )?;
    assert!(
        answer.starts_with("refused: developer mode is off")
            && answer.contains("`live-reload = true` under `[developer]`"),
        "{answer}"
    );
    common::wait(&mut hp, Duration::from_millis(200))?;
    assert_eq!(hp.shell_pid(), first, "the shell was left alone");
    let status = ask(&mut hp, "msg shell status")?;
    assert_eq!(said(&status, "state"), Some("running"), "{status}");
    assert_eq!(said(&status, "program"), installed.to_str(), "{status}");
    assert_eq!(said(&status, "given"), Some("no"), "{status}");
    assert_eq!(said(&status, "starts"), Some("1"), "{status}");

    assert_eq!(
        ask(&mut hp, "msg shell restart installed")?,
        format!("restarting\nprogram {}\n", installed.display())
    );
    let status = shell_back(&mut hp, first)?;
    assert_eq!(said(&status, "program"), installed.to_str(), "{status}");
    Ok(())
}

/// A given program that keeps ending gives way to the installed one, and
/// so does one that does not start: the status says which and why, and
/// the installed shell, back, says it in a notification.
#[test]
fn a_bad_program_falls_back_to_the_installed_one_and_it_is_said() -> anyhow::Result<()> {
    let built = common::shell_program();
    let dir = tempfile::tempdir()?;
    let installed = common::shell_script(dir.path(), &built, &[])?;
    let ending = dir.path().join("ending");
    script_at(&ending, "#!/bin/sh\nexit 1\n")?;
    let unstartable = dir.path().join("unstartable");
    script_at(&unstartable, "#!/nonexistent/interpreter\n")?;
    let mut hp = Headless::start_with_shell(pixman(), developer(), Some(installed.clone()))?;
    common::drawn(&mut hp, hocus_link::NAMESPACE)?;
    let window = common::open(&mut hp, "test.kept", (640, 480))?;
    let area = hp.work_area();
    assert!(hp.bubble_surfaces().is_empty());

    for (bad, why) in [(&ending, "kept ending"), (&unstartable, "did not start")] {
        let before = hp.shell_pid();
        ask(
            &mut hp,
            &format!("msg shell restart program {}", bad.display()),
        )?;
        let status = shell_back(&mut hp, before)?;
        assert_eq!(said(&status, "state"), Some("running"), "{status}");
        assert_eq!(said(&status, "program"), installed.to_str(), "{status}");
        assert_eq!(said(&status, "given"), Some("no"), "{status}");
        assert_eq!(
            said(&status, "fell-back"),
            Some(format!("{} ({why})", bad.display()).as_str()),
            "{status}"
        );
        assert_eq!(exe(hp.shell_pid()), Some(built.clone()));
        // Said: a bubble of the shell that came back.
        let told = hp.dispatch_until(WAIT, |hp| !hp.bubble_surfaces().is_empty())?;
        assert!(told, "no notification says it ({why})");
        assert!(hp.state().shell.notices.is_empty(), "said once");
        assert_eq!(common::rect_of(&hp, "test.kept"), Some(window));
        assert_eq!(hp.work_area(), area);
    }
    // A restart asked for forgets what was said of the last one.
    let before = hp.shell_pid();
    ask(&mut hp, "msg shell restart")?;
    let status = shell_back(&mut hp, before)?;
    assert_eq!(said(&status, "fell-back"), None, "{status}");
    Ok(())
}
