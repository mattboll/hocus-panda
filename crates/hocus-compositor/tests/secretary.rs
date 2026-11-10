//! The secretary's process (docs/design/secretary.md §16, §21): started
//! in assistant mode only, followed live, started again after a crash
//! (AC1); at rest she wakes neither the compositor nor the shell (AC2); a
//! question she takes thirty seconds to answer holds no frame of the
//! shell's (AC3).
//!
//! She runs on a clock of her own (`--virtual-clock`), moved on by the
//! test over her socket, with a scripted fake model and a fixture source.
//! Without her program (or the shell's) built, the tests skip.

use crate::common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, SecretaryMode};
use hocus_link::{SecretaryShown, ToShell};
use hocus_secretary_link::{Client, Connection, Status, ToSecretary};

const WAIT: Duration = Duration::from_secs(20);

/// Her program, run by a script that gives her a data folder of her own,
/// her clock, her fake model and her fixture source.
fn script(dir: &Path, secretary: &Path) -> anyhow::Result<PathBuf> {
    let model = dir.join("model.toml");
    std::fs::write(
        &model,
        "[[reply]]\nrole = \"answer\"\ncontains = [\"long\"]\ntext = \"Voilà.\"\ndelay = 30\n\n\
         [default]\ntext = \"D'accord.\"\n",
    )?;
    let fixture = dir.join("mail.toml");
    std::fs::write(
        &fixture,
        "name = \"fixture-mail\"\nnotebook = \"pro\"\nevery = 300\n\
         [[items]]\nid = \"m1\"\nat = \"2026-11-09T08:04:00Z\"\nkind = \"mail\"\ndigest = \"Paul : le devis\"\n",
    )?;
    let data = dir.join("data");
    let script = dir.join("secretary");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\nexport XDG_DATA_HOME='{}'\n\
             exec '{}' --virtual-clock 2026-11-09T08:00:00Z --fake-model '{}' \
             --recorder '{}' --fixture '{}' \"$@\" 2>>'{}'\n",
            data.display(),
            secretary.display(),
            model.display(),
            dir.join("calls.jsonl").display(),
            fixture.display(),
            dir.join("secretary.log").display(),
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    Ok(script)
}

fn config(script: &Path, mode: SecretaryMode) -> Config {
    let mut config = Config {
        secretary_program: Some(script.to_owned()),
        ..Config::default()
    };
    config.secretary.mode = mode;
    config
}

fn pixman() -> HeadlessConfig {
    HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    }
}

/// A connection to her once she listens, the compositor's loop turning
/// meanwhile.
fn connect(hp: &mut Headless) -> anyhow::Result<Connection> {
    let socket = hp
        .secretary_socket()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("no socket"))?;
    let start = Instant::now();
    loop {
        if let Ok(connection) = Connection::open(&socket, Client::Test, Duration::from_secs(5)) {
            return Ok(connection);
        }
        anyhow::ensure!(
            start.elapsed() < WAIT,
            "she never listened on {}",
            socket.display()
        );
        hp.dispatch(Duration::from_millis(20))?;
    }
}

/// Her status once `done` holds for it.
fn status_until(
    hp: &mut Headless,
    her: &mut Connection,
    mut done: impl FnMut(&Status) -> bool,
) -> anyhow::Result<Status> {
    let start = Instant::now();
    loop {
        let status = her.status().map_err(anyhow::Error::msg)?;
        if done(&status) {
            return Ok(status);
        }
        anyhow::ensure!(start.elapsed() < WAIT, "never: {status:?}");
        hp.dispatch(Duration::from_millis(20))?;
    }
}

fn gone(pid: u32) -> bool {
    !Path::new(&format!("/proc/{pid}")).exists()
}

/// AC1: classic starts nothing; assistant starts her, live; a crash
/// starts her again (said "unavailable" meanwhile), her memory kept;
/// classic again stops her; the compositor's end ends her.
#[test]
fn she_runs_in_assistant_mode_only_and_comes_back_after_a_crash() -> anyhow::Result<()> {
    let program = common::secretary_program();
    let dir = tempfile::tempdir()?;
    let script = script(dir.path(), &program)?;
    let mut hp = Headless::start_with(pixman(), config(&script, SecretaryMode::Classic))?;
    common::wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.secretary_starts(), 0, "classic: nothing starts");
    assert!(hp.secretary_pid().is_none());

    hp.reconfigure(config(&script, SecretaryMode::Assistant));
    let mut her = connect(&mut hp)?;
    let first = hp.secretary_pid().expect("her process");
    assert_eq!(
        hp.secretary_told(),
        Some(SecretaryShown::Ready(
            hp.secretary_socket().expect("a socket").to_owned()
        ))
    );
    let status = status_until(&mut hp, &mut her, |s| s.sources.iter().all(|s| !s.busy))?;
    assert!(status.virtual_clock);
    assert_eq!(status.key, "file", "no bus given: a key file");
    her.send(&ToSecretary::Clock { advance: 300 })
        .map_err(anyhow::Error::msg)?;
    status_until(&mut hp, &mut her, |s| s.counts.episodes == 1)?;
    drop(her);

    hp.kill_secretary()?;
    let mut unavailable = false;
    let back = hp.dispatch_until(WAIT, |hp| {
        unavailable |= hp.secretary_told() == Some(SecretaryShown::Unavailable);
        hp.secretary_starts() == 2 && hp.secretary_pid().is_some()
    })?;
    assert!(
        back && unavailable,
        "started again, said unavailable meanwhile"
    );
    assert_ne!(hp.secretary_pid(), Some(first));
    let mut her = connect(&mut hp)?;
    let status = status_until(&mut hp, &mut her, |s| s.catch_up.is_some())?;
    assert_eq!(
        status.counts.episodes, 1,
        "her memory kept across the crash"
    );
    drop(her);

    let second = hp.secretary_pid().expect("her process");
    hp.reconfigure(config(&script, SecretaryMode::Classic));
    let stopped = hp.dispatch_until(WAIT, |hp| hp.secretary_pid().is_none() && gone(second))?;
    assert!(stopped, "classic again: she ends");
    assert_eq!(hp.secretary_told(), Some(SecretaryShown::Off));
    common::wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.secretary_starts(), 2, "and stays ended");
    let socket = hp
        .secretary_socket()
        .map(Path::to_path_buf)
        .expect("a socket's path");
    assert!(!socket.exists(), "her socket goes with her");

    hp.reconfigure(config(&script, SecretaryMode::Assistant));
    drop(connect(&mut hp)?);
    let third = hp.secretary_pid().expect("her process");
    drop(hp);
    let start = Instant::now();
    while !gone(third) {
        anyhow::ensure!(start.elapsed() < WAIT, "she outlived the compositor");
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// AC1, the shell's side: killing her leaves the bar working, and the
/// shell is told she is unavailable.
#[test]
fn killing_her_leaves_the_bar_working() -> anyhow::Result<()> {
    let (program, shell) = (common::secretary_program(), common::shell_program());
    let dir = tempfile::tempdir()?;
    let script = script(dir.path(), &program)?;
    let shell = common::shell_script(dir.path(), &shell, &[])?;
    let mut hp = Headless::start_with_shell(
        pixman(),
        config(&script, SecretaryMode::Assistant),
        Some(shell),
    )?;
    let mut her = connect(&mut hp)?;
    status_until(&mut hp, &mut her, |s| {
        s.connections
            .iter()
            .any(|c| c.client == Some(Client::Shell))
    })?;
    let bar = common::drawn(&mut hp, hocus_link::NAMESPACE)?;
    let shell_pid = hp.shell_pid();
    drop(her);
    hp.kill_secretary()?;
    let told = hp.dispatch_until(WAIT, |hp| {
        hp.secretary_told() == Some(SecretaryShown::Unavailable)
    })?;
    assert!(told, "the shell told she is unavailable");
    assert_eq!(hp.shell_pid(), shell_pid, "the shell runs on");
    assert_eq!(
        common::drawn(&mut hp, hocus_link::NAMESPACE)?,
        bar,
        "the bar is there"
    );
    // The bar still takes what the compositor says.
    let commits: u64 = shell_commits(&hp).iter().sum();
    hp.tell_shell(ToShell::BarWindows(false));
    let redrawn = hp.dispatch_until(WAIT, |hp| shell_commits(hp).iter().sum::<u64>() > commits)?;
    assert!(redrawn, "the bar still draws");
    // And she comes back to it.
    let mut her = connect(&mut hp)?;
    status_until(&mut hp, &mut her, |s| {
        s.connections
            .iter()
            .any(|c| c.client == Some(Client::Shell))
    })?;
    Ok(())
}

/// The compositor's loop's turns over `time`.
fn turns(hp: &mut Headless, time: Duration) -> anyhow::Result<u32> {
    let until = Instant::now() + time;
    let mut turns = 0;
    while let Some(left) = until
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
    {
        hp.dispatch(left)?;
        turns += 1;
    }
    Ok(turns)
}

/// AC2, the compositor's side: ten minutes of hers (her source syncing
/// twice) wake its loop no more than rest does, and draw nothing.
#[test]
fn ten_minutes_of_hers_wake_the_compositor_zero_times() -> anyhow::Result<()> {
    let program = common::secretary_program();
    let dir = tempfile::tempdir()?;
    let script = script(dir.path(), &program)?;
    let mut hp = Headless::start_with(pixman(), config(&script, SecretaryMode::Assistant))?;
    let mut her = connect(&mut hp)?;
    status_until(&mut hp, &mut her, |s| s.sources.iter().all(|s| !s.busy))?;
    common::at_rest(&mut hp)?;
    common::wait(&mut hp, Duration::from_millis(300))?;
    let frames = hp.frames();
    let wakes = her.status().map_err(anyhow::Error::msg)?.wakes;
    her.send(&ToSecretary::Clock { advance: 600 })
        .map_err(anyhow::Error::msg)?;
    let turned = turns(&mut hp, Duration::from_secs(2))?;
    let status = her.status().map_err(anyhow::Error::msg)?;
    assert_eq!(status.counts.episodes, 1, "her source read meanwhile");
    assert!(status.wakes > wakes, "she worked");
    assert_eq!(
        (turned, hp.frames() - frames),
        (1, 0),
        "nothing woke the compositor"
    );
    Ok(())
}

/// The shell's surfaces' commits but the clock's, which draws its second
/// on its own.
fn shell_commits(hp: &Headless) -> [u64; 5] {
    [
        hp.bar_commits(),
        hp.levels_commits(),
        hp.desktops_commits(),
        hp.tasks_commits(),
        hp.popover_commits(),
    ]
}

/// The lines she sent the shell.
fn sent_to_shell(status: &Status) -> u64 {
    status
        .connections
        .iter()
        .filter(|c| c.client == Some(Client::Shell))
        .map(|c| c.sent)
        .sum()
}

/// AC2 and AC3, the shell's side: ten minutes of hers send the shell
/// nothing and draw nothing; a question she takes thirty seconds to
/// answer draws nothing meanwhile, and her answer reaches the shell after.
#[test]
fn she_holds_no_frame_of_the_shell_at_rest_nor_while_she_thinks() -> anyhow::Result<()> {
    let (program, shell) = (common::secretary_program(), common::shell_program());
    let dir = tempfile::tempdir()?;
    let script = script(dir.path(), &program)?;
    let shell = common::shell_script(dir.path(), &shell, &[])?;
    let mut hp = Headless::start_with_shell(
        pixman(),
        config(&script, SecretaryMode::Assistant),
        Some(shell),
    )?;
    let mut her = connect(&mut hp)?;
    let shell_there = |s: &Status| {
        s.connections
            .iter()
            .any(|c| c.client == Some(Client::Shell))
    };
    status_until(&mut hp, &mut her, |s| {
        shell_there(s) && s.sources.iter().all(|s| !s.busy)
    })?;
    common::drawn(&mut hp, hocus_link::NAMESPACE)?;
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    common::wait(&mut hp, Duration::from_millis(300))?;

    // AC2: ten minutes of hers.
    let before = shell_commits(&hp);
    let sent = sent_to_shell(&her.status().map_err(anyhow::Error::msg)?);
    her.send(&ToSecretary::Clock { advance: 600 })
        .map_err(anyhow::Error::msg)?;
    common::wait(&mut hp, Duration::from_millis(1500))?;
    let status = her.status().map_err(anyhow::Error::msg)?;
    assert_eq!(status.counts.episodes, 1, "her source read meanwhile");
    assert_eq!(sent_to_shell(&status), sent, "nothing sent to the shell");
    assert_eq!(shell_commits(&hp), before, "the shell drew nothing");

    // AC3: a question her model takes 30 s (of her clock) to answer.
    hp.tell_shell(ToShell::SecretaryAsk("Une question longue".to_owned()));
    status_until(&mut hp, &mut her, |s| s.calls_running == 1)?;
    common::wait(&mut hp, Duration::from_millis(300))?;
    let thinking = shell_commits(&hp);
    common::wait(&mut hp, Duration::from_millis(1500))?;
    // The idle count: none, as over the ten minutes above (the line
    // itself redrew the bar once, as any line from the compositor does).
    assert_eq!(shell_commits(&hp), thinking, "no frame while she thinks");
    her.send(&ToSecretary::Clock { advance: 30 })
        .map_err(anyhow::Error::msg)?;
    let answered = status_until(&mut hp, &mut her, |s| {
        s.calls_running == 0 && sent_to_shell(s) > sent
    })?;
    // The thread's lines (the sentence, her typing, her words, her face,
    // the end): what the drawer would show, and nothing draws for them
    // while it is closed.
    assert!(
        sent_to_shell(&answered) > sent,
        "her answer reached the shell"
    );
    common::wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(shell_commits(&hp), thinking, "her drawer closed: no frame");
    Ok(())
}
