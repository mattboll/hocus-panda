//! Project desktops, lot A (docs/design/projects.md §12, tests 1 to 3 and
//! 12; #789): a desktop made a project by the line the overview's form
//! sends (`project-make SCREEN DESKTOP NAME FOLDER`, on the Settings'
//! line) is named everywhere a desktop's name shows: the bar's `desktops`
//! line, its list's title, Alt+Tab's suffix, what screen readers read; a
//! name taken, a folder missing are refused. A window sent to its desktop
//! belongs to it, one sent away does not, one on every desktop never does;
//! switching away and back leaves its windows as they were, never asked
//! to close; open and shown, nothing draws nor is read at rest.
//!
//! The real shell (built beside the tests, `cargo programs`) where what it
//! shows is checked, on the CPU, away from the machine's services; the
//! compositor alone otherwise. Each test has a home and a state folder of
//! its own.

use crate::common;

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use anyhow::Context;
use common::{
    KEY_2, KEY_LEFTALT, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB, WAIT, chord, open, open_with, wait,
};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::WindowId};

const SCREEN: &str = "HEADLESS-1";
const KEY_1: u32 = 2;
const KEY_3: u32 = 4;

/// A test's folders: its home, its state, the project's folder.
struct Places {
    _dir: tempfile::TempDir,
    root: PathBuf,
    folder: PathBuf,
}

fn places() -> anyhow::Result<Places> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().to_path_buf();
    let folder = root.join("home/github/hocus-site");
    std::fs::create_dir_all(folder.join(".git"))?;
    Ok(Places {
        _dir: dir,
        root,
        folder,
    })
}

fn config(places: &Places) -> Config {
    Config {
        state_dir: Some(places.root.join("state/hocuspanda")),
        ..Config::default()
    }
}

/// The compositor alone, on one screen.
fn alone(places: &Places) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config(places),
    )?;
    hp.pointer_move(600.0, 500.0);
    Ok(hp)
}

/// The compositor with the real shell, in French, its log in `shell.log`.
fn with_shell(places: &Places) -> anyhow::Result<(Headless, PathBuf)> {
    let shell = common::shell_program();
    let at = |sub: &str| -> anyhow::Result<String> {
        let path = places.root.join(sub);
        std::fs::create_dir_all(&path)?;
        Ok(path.display().to_string())
    };
    let log = places.root.join("shell.log");
    let env = [
        ("HOME", at("home")?),
        ("XDG_CONFIG_HOME", at("config")?),
        ("XDG_CACHE_HOME", at("cache")?),
        ("XDG_STATE_HOME", at("state")?),
        ("XDG_DATA_HOME", at("data")?),
        ("LC_ALL", "fr_FR.UTF-8".to_owned()),
        ("LANG", "fr_FR.UTF-8".to_owned()),
        ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
    ];
    let script = shell_script(&places.root, &shell, &env, &log)?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config(places),
        Some(script),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE).len() == 1
    })?;
    anyhow::ensure!(up, "no desktops' tiles");
    hp.pointer_move(600.0, 500.0);
    Ok((hp, log))
}

/// A script starting `shell` away from the machine's services, as
/// [`common::shell_script`] does, what it says written to `log`.
fn shell_script(
    dir: &Path,
    shell: &Path,
    env: &[(&str, String)],
    log: &Path,
) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let mut text = String::from(
        "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
         export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
         PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent\n",
    );
    for (name, value) in env {
        text.push_str(&format!("export {name}='{value}'\n"));
    }
    text.push_str(&format!(
        "exec '{}' \"$@\" 2>>'{}'\n",
        shell.display(),
        log.display()
    ));
    let script = dir.join("shell");
    std::fs::write(&script, text)?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    Ok(script)
}

/// The Settings' line (`msg screens`), on which projects are made.
struct Line {
    stream: UnixStream,
    heard: mpsc::Receiver<serde_json::Value>,
}

impl Line {
    /// Opened, once the compositor serves it: what is written on the
    /// control socket's first read with `msg screens` would be lost.
    fn open(hp: &mut Headless) -> anyhow::Result<Self> {
        let path = hp.control_socket().context("no control socket")?;
        let mut stream = UnixStream::connect(path)?;
        stream.write_all(b"msg screens\n")?;
        let reader = stream.try_clone()?;
        let (said, heard) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else {
                    return;
                };
                if let Ok(value) = serde_json::from_str(&line)
                    && said.send(value).is_err()
                {
                    return;
                }
            }
        });
        let mut first = None;
        hp.dispatch_until(WAIT, |_| {
            first = first.take().or_else(|| heard.try_recv().ok());
            first.is_some()
        })?;
        anyhow::ensure!(first.is_some(), "the line never opened");
        Ok(Self { stream, heard })
    }

    /// `project-make`, its parts a tab apart; its answer: what was made, or
    /// why not.
    fn make(
        &mut self,
        hp: &mut Headless,
        desktop: usize,
        name: &str,
        folder: &Path,
    ) -> anyhow::Result<Result<serde_json::Value, String>> {
        let line = format!(
            "project-make\t{SCREEN}\t{desktop}\t{name}\t{}\n",
            folder.display()
        );
        self.stream.write_all(line.as_bytes())?;
        let mut answer = None;
        hp.dispatch_until(WAIT, |_| {
            while let Ok(value) = self.heard.try_recv() {
                if let Some(made) = value.get("project-made") {
                    answer = Some(Ok(made.clone()));
                } else if let Some(why) = value.get("refused") {
                    answer = Some(Err(why.as_str().unwrap_or_default().to_owned()));
                }
            }
            answer.is_some()
        })?;
        answer.with_context(|| {
            format!(
                "no answer: jobs {}, projects {:?}",
                hp.project_jobs(),
                hp.projects()
            )
        })
    }
}

fn id(hp: &Headless, app_id: &str) -> anyhow::Result<WindowId> {
    hp.all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some(app_id))
        .map(|w| w.0)
        .with_context(|| format!("no {app_id}"))
}

/// `app_id` opened on the screen's current desktop, then sent to desktop
/// `key`'s (`Super+Shift+N`).
fn open_on(hp: &mut Headless, app_id: &'static str, key: Option<u32>) -> anyhow::Result<WindowId> {
    open(hp, app_id, (700, 500))?;
    if let Some(key) = key {
        chord(hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, key]);
    }
    id(hp, app_id)
}

/// Runs the compositor until the screen shows desktop `index`, still.
fn on(hp: &mut Headless, index: usize) -> anyhow::Result<()> {
    let still = hp.dispatch_until(WAIT, |hp| {
        let (current, at) = hp.desktop();
        current == index && (at - index as f64).abs() < 1e-9
    })?;
    anyhow::ensure!(still, "never on desktop {}: {:?}", index + 1, hp.desktop());
    Ok(())
}

/// `hocus msg a11y`'s answer, from a thread while the loop runs.
fn a11y(hp: &mut Headless) -> anyhow::Result<String> {
    let path = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let asked = (|| -> std::io::Result<String> {
            let mut stream = UnixStream::connect(&path)?;
            stream.set_read_timeout(Some(WAIT))?;
            stream.write_all(b"msg a11y\n")?;
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

/// Runs until the shell has logged `text`.
fn logged(hp: &mut Headless, log: &Path, text: &str) -> anyhow::Result<()> {
    let done = hp.dispatch_until(WAIT, |_| common::logged(log).contains(text))?;
    anyhow::ensure!(
        done,
        "the shell never logged {text:?}; it logged:\n{}",
        common::logged(log)
            .lines()
            .filter(|l| {
                l.contains("desktop")
                    || l.contains("project")
                    || l.contains("panic")
                    || l.contains("ERROR")
            })
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(())
}

#[test]
fn a_desktop_made_a_project_is_named_everywhere_and_a_name_taken_is_refused() -> anyhow::Result<()>
{
    let places = places()?;
    let (mut hp, log) = with_shell(&places)?;
    open_on(&mut hp, "test.here", None)?;
    open_on(&mut hp, "test.site", Some(KEY_2))?;
    let mut line = Line::open(&mut hp)?;

    let made = line
        .make(&mut hp, 2, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    assert_eq!(made["name"], "hocus-site");
    assert_eq!(made["desktop"], 2);
    assert_eq!(made["id"], "hocus-site");
    // The `desktops` line names it.
    let view = hp.desktops_view(SCREEN).context("no desktops")?;
    assert_eq!(
        view.projects,
        [hocus_link::DeskProject {
            desktop: 1,
            name: "hocus-site".into(),
            folder: places.folder.display().to_string(),
        }]
    );
    assert_eq!(
        hp.desktops_view(SCREEN).map(|v| v.project(0).is_none()),
        Some(true)
    );
    // Its file: the list, for its owner alone.
    let file = places.root.join("state/hocuspanda/projects");
    let written = hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&file).is_ok_and(|t| t.contains("hocus-site"))
    })?;
    anyhow::ensure!(written, "no projects' file");
    let text = std::fs::read_to_string(&file)?;
    assert!(text.contains("desktop = 2"), "{text}");

    // Alt+Tab: the window on it says its name.
    hp.key(KEY_LEFTALT, true);
    hp.key(KEY_TAB, true);
    hp.key(KEY_TAB, false);
    let site = id(&hp, "test.site")?;
    let item = hp
        .switcher()
        .and_then(|(items, ..)| items.iter().position(|i| *i == site))
        .context("no Alt+Tab")?;
    assert_eq!(hp.switcher_suffix(item).as_deref(), Some(" · hocus-site"));
    hp.key(KEY_LEFTALT, false);
    wait(&mut hp, Duration::from_millis(100))?;

    // The bar's tiles say it to screen readers, and the list's title is
    // its name.
    logged(&mut hp, &log, "Bureau 2, projet hocus-site, 1 fenêtre")?;
    let said = a11y(&mut hp)?;
    assert!(
        said.contains("Bureau 2, projet hocus-site"),
        "screen readers read:\n{said}"
    );
    let told = hp.dispatch_until(WAIT, |hp| hp.desk_tile_middles().len() == 4)?;
    anyhow::ensure!(told, "tiles: {:?}", hp.desk_tile_middles());
    let (x, y) = hp
        .desk_tile_middles()
        .into_iter()
        .find(|(_, _, d, _)| *d == 1)
        .map(|(.., m)| m)
        .context("no second tile")?;
    hp.pointer_move(x - 1.0, y);
    hp.pointer_move(x, y);
    logged(&mut hp, &log, "title=hocus-site count=1 fenêtre")?;
    hp.pointer_move(600.0, 500.0);

    // A name taken (case and accents aside) is refused; so are a folder
    // that is not there and a desktop that is a project already.
    let taken = line.make(&mut hp, 3, "Hocus-Sité", &places.folder)?;
    assert!(
        taken
            .as_ref()
            .is_err_and(|why| why.contains("already named")),
        "{taken:?}"
    );
    let missing = line.make(&mut hp, 3, "ailleurs", &places.root.join("nowhere"))?;
    assert_eq!(missing, Err("no such folder".to_owned()));
    let twice = line.make(&mut hp, 2, "autre", &places.folder)?;
    assert!(twice.is_err(), "{twice:?}");
    assert_eq!(hp.projects().len(), 1);
    Ok(())
}

#[test]
fn a_window_belongs_to_the_project_of_its_desktop_never_one_on_every_desktop() -> anyhow::Result<()>
{
    let places = places()?;
    let mut hp = alone(&places)?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 2, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    assert!(hp.project_windows("hocus-site").is_empty());
    // Sent to its desktop: it belongs to it; the one left here does not.
    let here = open_on(&mut hp, "test.here", None)?;
    let site = open_on(&mut hp, "test.site", Some(KEY_2))?;
    assert_eq!(hp.project_windows("hocus-site"), [site]);
    // Its name, accents and case aside, finds it too.
    assert_eq!(hp.project_windows("HOCUS-SITE"), [site]);
    // Sent away: no longer.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    on(&mut hp, 1)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_3]);
    assert!(hp.project_windows("hocus-site").is_empty());
    // A window on every desktop never belongs to it, even shown there.
    hp.window_action(here, hocus_link::WindowAction::AllDesktops);
    assert_eq!(hp.everywhere(), [here]);
    assert!(hp.project_windows("hocus-site").is_empty());
    // Back on its own desktop, the window sent away belongs to it again.
    hp.window_action(site, hocus_link::WindowAction::PreviousDesktop);
    assert_eq!(hp.project_windows("hocus-site"), [site]);
    Ok(())
}

#[test]
fn switching_away_and_back_keeps_the_projects_windows_never_asked_to_close() -> anyhow::Result<()> {
    let places = places()?;
    let mut hp = alone(&places)?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-panda", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let (_, editor) = open_with(&mut hp, "test.editor", (900, 600), false)?;
    let (_, terminal) = open_with(&mut hp, "test.terminal", (600, 400), false)?;
    let before = hp.project_windows("hocus-panda");
    assert_eq!(before.len(), 2);
    // Away to desktop 3 and back, twice.
    for _ in 0..2 {
        chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
        on(&mut hp, 2)?;
        assert_eq!(hp.project_windows("hocus-panda"), before, "away");
        chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
        on(&mut hp, 0)?;
    }
    assert_eq!(hp.project_windows("hocus-panda"), before);
    // Its windows show again where they were.
    assert!(
        hp.all_windows()
            .iter()
            .filter(|w| before.contains(&w.0))
            .all(|w| w.2 == 0 && w.3),
        "{:?}",
        hp.all_windows()
    );
    wait(&mut hp, Duration::from_millis(100))?;
    for client in [&editor, &terminal] {
        let asked = client
            .events
            .try_iter()
            .any(|e| matches!(e, common::Event::CloseRequested));
        assert!(!asked, "a window asked to close");
    }
    let open = hp.projects();
    assert_eq!(open[0].2, Some((SCREEN.to_owned(), 0)), "still open");
    Ok(())
}

#[test]
fn a_project_open_and_shown_draws_and_reads_nothing_at_rest() -> anyhow::Result<()> {
    let places = places()?;
    let (mut hp, log) = with_shell(&places)?;
    open_on(&mut hp, "test.site", None)?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    logged(&mut hp, &log, "said=Bureau 1, projet hocus-site, 1 fenêtre")?;
    // Settled: the shell's first draws done (an icon looked for, the
    // launcher's list), away from the clock's minute.
    common::clear_of_the_minute(&mut hp, Duration::from_secs(4))?;
    // The bar's surfaces but the clock (which draws its own time), the
    // windows', and the projects' thread.
    let counts = |hp: &Headless| {
        [
            hp.bar_commits(),
            hp.desktops_commits(),
            hp.levels_commits(),
            hp.connections_commits(),
            hp.popover_commits(),
            hp.window_commits(),
            hp.project_jobs(),
        ]
    };
    let start = std::time::Instant::now();
    loop {
        let before = counts(&hp);
        wait(&mut hp, Duration::from_millis(600))?;
        if counts(&hp) == before || start.elapsed() > WAIT {
            break;
        }
    }
    common::at_rest(&mut hp)?;
    let (before, frames, clock) = (counts(&hp), hp.frames(), hp.clock_commits());
    wait(&mut hp, Duration::from_millis(500))?;
    // Nothing committed, read nor written; no frame but for the clock's.
    assert_eq!(counts(&hp), before, "at rest");
    assert!(
        hp.frames() - frames <= hp.clock_commits() - clock,
        "{} frames at rest, the clock drew {} times",
        hp.frames() - frames,
        hp.clock_commits() - clock
    );
    assert!(
        hp.wake()
            .is_none_or(|at| at > hp.now() + Duration::from_millis(400))
    );
    Ok(())
}

// --- Lot B: closing and reopening, terminals, log out and log in ----------------
//
// Tests 4, 5, 8 to 11 (docs/design/projects.md §12). The applications
// started again are this test's own program ([`app_program`],
// [`terminal_program`]) through desktop entries of a fake XDG tree; a
// terminal stand-in's shell is a `sleep` on a terminal of its own (a
// pty), in a folder of the test's.

const APP_ID: &str = "HOCUS_TEST_PROJECT_APP";
const APP_STUBBORN: &str = "HOCUS_TEST_PROJECT_STUBBORN";
const TERMINAL_APP: &str = "HOCUS_TEST_PROJECT_TERMINAL";
const TERMINAL_RUN: &str = "HOCUS_TEST_PROJECT_RUN";
const TERMINAL_TITLE: &str = "HOCUS_TEST_PROJECT_TITLE";

/// The Wayland socket a program started by the compositor (or the test)
/// connects to.
fn display_socket() -> PathBuf {
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").unwrap_or_default());
    if display.is_absolute() {
        display
    } else {
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default()).join(display)
    }
}

/// An application started by its entry: a window of [`APP_ID`] that closes
/// when asked, unless [`APP_STUBBORN`] is set. Run alone, it does nothing.
#[test]
fn app_program() -> anyhow::Result<()> {
    let Ok(app) = std::env::var(APP_ID) else {
        return Ok(());
    };
    let app: &'static str = app.leak();
    let stubborn = std::env::var_os(APP_STUBBORN).is_some();
    let client = common::client(
        &display_socket(),
        Some(common::Shows::Window {
            app_id: app,
            size: (500, 400),
            popup: false,
        }),
    )?;
    while let Ok(event) = client.events.recv() {
        if event == common::Event::CloseRequested && !stubborn {
            break;
        }
    }
    Ok(())
}

/// A terminal started by its entry (through [`terminal_wrapper`]): a
/// window of [`TERMINAL_APP`], its shell a `sleep` on a pty of its own in
/// the folder it was started in (with [`TERMINAL_RUN`], a program in the
/// shell's foreground); it closes when asked, its shell with it. Run
/// alone, it does nothing.
#[test]
fn terminal_program() -> anyhow::Result<()> {
    let Ok(app) = std::env::var(TERMINAL_APP) else {
        return Ok(());
    };
    let app: &'static str = app.leak();
    let folder = std::env::current_dir()?;
    let run = std::env::var(TERMINAL_RUN).ok();
    let mut shell = pty_shell(&folder, run.as_deref())?;
    let client = common::client(
        &display_socket(),
        Some(common::Shows::Window {
            app_id: app,
            size: (600, 400),
            popup: false,
        }),
    )?;
    if let Ok(title) = std::env::var(TERMINAL_TITLE) {
        client.commands.send(common::Command::Title(title.leak()))?;
    }
    while let Ok(event) = client.events.recv() {
        if event == common::Event::CloseRequested {
            break;
        }
    }
    // Its own shell, by its own handle.
    let _ = shell.kill();
    let _ = shell.wait();
    Ok(())
}

/// A shell on a pty of its own, its controlling terminal, in `folder`:
/// `sleep`, or `run` in its foreground (job control on) then `sleep`.
#[allow(unsafe_code)]
fn pty_shell(folder: &Path, run: Option<&str>) -> anyhow::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    // SAFETY: a new pty's master, this function's own descriptor.
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    anyhow::ensure!(master >= 0, "no pty: {}", std::io::Error::last_os_error());
    // SAFETY: `master` is the pty's master just opened.
    let ready = unsafe { libc::grantpt(master) == 0 && libc::unlockpt(master) == 0 };
    anyhow::ensure!(ready, "the pty: {}", std::io::Error::last_os_error());
    let mut name = [0 as libc::c_char; 128];
    // SAFETY: the buffer is as long as said.
    let named = unsafe { libc::ptsname_r(master, name.as_mut_ptr(), name.len()) };
    anyhow::ensure!(named == 0, "the pty's name");
    // SAFETY: ptsname_r wrote a string ending with a zero into `name`.
    let slave = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_owned();
    let script = match run {
        Some(run) => format!("set -m; {run}; sleep 600"),
        None => "exec sleep 600".to_owned(),
    };
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg(script).current_dir(folder);
    // SAFETY: between fork and exec, only calls safe there: a session of
    // its own whose controlling terminal is the pty, on its standard
    // streams. `slave` was made before the fork.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let fd = libc::open(slave.as_ptr(), libc::O_RDWR);
            if fd < 0 || libc::ioctl(fd, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            libc::dup2(fd, 0);
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
            Ok(())
        });
    }
    // The master stays open as long as this process: its shell keeps its
    // terminal.
    Ok(command.spawn()?)
}

/// A fake XDG tree's applications for `places`: `id` started as
/// [`app_program`] (stubborn: it never closes when asked).
fn app_entry(places: &Places, id: &str, name: &str, stubborn: bool) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let dir = places.root.join("applications");
    std::fs::create_dir_all(&dir)?;
    let stubborn = if stubborn {
        format!(" {APP_STUBBORN}=1")
    } else {
        String::new()
    };
    std::fs::write(
        dir.join(format!("{id}.desktop")),
        format!(
            "[Desktop Entry]\nType=Application\nName={name}\n\
             Exec=env {APP_ID}={id}{stubborn} \"{}\" --exact projects::app_program --nocapture\n",
            exe.display()
        ),
    )?;
    Ok(())
}

/// The terminal stand-in's entry, `foot` (`TerminalEmulator`): a wrapper
/// that writes its arguments and its folder into `marks/foot`, then runs
/// [`terminal_program`] there.
fn terminal_wrapper(
    places: &Places,
    run: Option<&str>,
    title: Option<&str>,
) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let exe = std::env::current_exe()?;
    let dir = places.root.join("applications");
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(places.root.join("marks"))?;
    let wrapper = places.root.join("foot-wrapper");
    let mut env = format!("{TERMINAL_APP}=foot");
    if let Some(run) = run {
        env.push_str(&format!(" {TERMINAL_RUN}='{run}'"));
    }
    if let Some(title) = title {
        env.push_str(&format!(" {TERMINAL_TITLE}='{title}'"));
    }
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\t%s\\n' \"$*\" \"$(pwd)\" >> '{}'\n\
             exec env {env} '{}' --exact projects::terminal_program --nocapture\n",
            places.root.join("marks/foot").display(),
            exe.display()
        ),
    )?;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(
        dir.join("foot.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Foot\nCategories=System;TerminalEmulator;\n\
             Exec=sh {}\n",
            wrapper.display()
        ),
    )?;
    Ok(wrapper)
}

/// What the terminal stand-in's wrapper was started with: its arguments
/// and its folder, a line each.
fn terminal_marks(places: &Places) -> Vec<(String, PathBuf)> {
    std::fs::read_to_string(places.root.join("marks/foot"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(args, dir)| (args.to_owned(), PathBuf::from(dir)))
        .collect()
}

/// The configuration of a test with applications of its own.
fn config_with_apps(places: &Places) -> Config {
    Config {
        applications: vec![places.root.join("applications")],
        lang: hocus_words::Lang::French,
        ..config(places)
    }
}

/// The compositor alone, on one screen, with `config`.
fn alone_with(config: Config) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.pointer_move(600.0, 500.0);
    Ok(hp)
}

/// Runs until project `name` is closed.
fn closed(hp: &mut Headless, name: &str) -> anyhow::Result<()> {
    let done = hp.dispatch_until(WAIT, |hp| {
        !hp.project_closing() && hp.projects().iter().any(|p| p.0 == name && p.2.is_none())
    })?;
    anyhow::ensure!(done, "{name} never closed: {:?}", hp.projects());
    Ok(())
}

/// Runs until a window of `app_id` is there; its id.
fn window_of(hp: &mut Headless, app_id: &str) -> anyhow::Result<WindowId> {
    let there = hp.dispatch_until(WAIT, |hp| {
        hp.all_windows()
            .iter()
            .any(|w| w.1.as_deref() == Some(app_id))
    })?;
    anyhow::ensure!(there, "no {app_id}: {:?}", hp.all_windows());
    id(hp, app_id)
}

/// Test 4: closing asks each window to close; one that closes is saved
/// with its place and state, one that does not is left on what is then a
/// plain desktop.
#[test]
fn closing_saves_the_windows_that_close_and_leaves_a_plain_desktop() -> anyhow::Result<()> {
    let places = places()?;
    app_entry(&places, "test.closes", "Closes", false)?;
    app_entry(&places, "test.stays", "Stays", true)?;
    let mut hp = alone_with(config_with_apps(&places))?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let (_, closes) = open_with(&mut hp, "test.closes", (700, 500), false)?;
    let (_, stays) = open_with(&mut hp, "test.stays", (600, 400), false)?;
    let shut = id(&hp, "test.closes")?;
    hp.window_action(shut, hocus_link::WindowAction::Maximize);
    wait(&mut hp, Duration::from_millis(200))?;

    hp.project_close("hocus-site")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    // Each window is asked to close, as Super+Q asks it.
    let mut asked = [false, false];
    let both = hp.dispatch_until(WAIT, |_| {
        for (i, client) in [&closes, &stays].into_iter().enumerate() {
            asked[i] |= client
                .events
                .try_iter()
                .any(|e| e == common::Event::CloseRequested);
        }
        asked == [true, true]
    })?;
    anyhow::ensure!(both, "asked to close: {asked:?}");
    closes.commands.send(common::Command::Close)?;
    let gone = hp.dispatch_until(WAIT, |hp| id(hp, "test.closes").is_err())?;
    anyhow::ensure!(gone, "test.closes still there");
    // The other one asks something: the project waits for it.
    wait(&mut hp, Duration::from_millis(300))?;
    assert!(hp.project_closing(), "the project waits for its windows");
    hp.project_close_patience_over();
    closed(&mut hp, "hocus-site")?;

    // Saved: the window that closed, its place and state; not the one
    // that stayed, which is still there, on a plain desktop.
    let contents = hp.project_contents("hocus-site");
    assert_eq!(
        contents,
        [("test.closes".to_owned(), "Maximized".to_owned(), None)]
    );
    let file = places
        .root
        .join("state/hocuspanda/projects.d/hocus-site.toml");
    let written = hp.dispatch_until(WAIT, |_| file.exists())?;
    anyhow::ensure!(written, "no contents' file");
    let text = std::fs::read_to_string(&file)?;
    assert!(text.contains("app-id = \"test.closes\""), "{text}");
    assert!(text.contains("state = \"maximized\""), "{text}");
    assert!(text.contains("place = ["), "{text}");
    assert!(!text.contains("test.stays"), "{text}");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&file)?.permissions().mode() & 0o777,
        0o600
    );
    assert!(id(&hp, "test.stays").is_ok());
    let view = hp.desktops_view(SCREEN).context("no desktops")?;
    assert!(view.projects.is_empty(), "{:?}", view.projects);
    Ok(())
}

/// Test 5: a terminal is saved with its shell's folder and comes back
/// there, given as its own option and as its working directory.
#[test]
fn a_terminal_comes_back_in_its_shells_folder() -> anyhow::Result<()> {
    let places = places()?;
    let wrapper = terminal_wrapper(&places, None, None)?;
    let mut hp = alone_with(config_with_apps(&places))?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let inside = places.folder.join("src");
    std::fs::create_dir_all(&inside)?;
    let inside = inside.canonicalize()?;
    // The terminal started in a folder of the project's, as by hand.
    let mut started = std::process::Command::new("sh")
        .arg(&wrapper)
        .current_dir(&inside)
        .env("WAYLAND_DISPLAY", hp.socket())
        .spawn()?;
    window_of(&mut hp, "foot")?;

    hp.project_close("hocus-site")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    closed(&mut hp, "hocus-site")?;
    let _ = started.wait();
    assert_eq!(
        hp.project_contents("hocus-site"),
        [(
            "foot".to_owned(),
            "Floating".to_owned(),
            Some(inside.clone())
        )]
    );

    // Reopened: started from its entry in that folder.
    let desktop = hp
        .project_open("hocus-site", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    assert_eq!(desktop, 0);
    let back = hp.dispatch_until(WAIT, |hp| !hp.project_coming_back("hocus-site"))?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    let marks = terminal_marks(&places);
    assert_eq!(marks.len(), 2, "{marks:?}");
    assert_eq!(
        marks[1],
        (
            format!("--working-directory={}", inside.display()),
            inside.clone()
        )
    );
    assert_eq!(hp.project_windows("hocus-site").len(), 1);
    assert!(hp.project_not_back().is_none());
    Ok(())
}

/// Test 8: a project reopens on the current desktop when it is empty, else
/// the first empty one, else one more (taken out as it closes); with nine
/// full, it does not, and says so.
#[test]
fn reopening_takes_the_current_empty_desktop_else_the_first_else_one_more() -> anyhow::Result<()> {
    let places = places()?;
    let mut config = config(&places);
    config.desktops.count = 4;
    let mut hp = alone_with(config.clone())?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 2, "blog", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let close = |hp: &mut Headless| -> anyhow::Result<()> {
        hp.project_close("blog")
            .map_err(|why| anyhow::anyhow!("{why}"))?;
        closed(hp, "blog")
    };
    let reopen = |hp: &mut Headless| -> anyhow::Result<Result<usize, String>> {
        let opened = hp.project_open("blog", SCREEN);
        wait(hp, Duration::from_millis(100))?;
        Ok(opened)
    };
    close(&mut hp)?;
    // Desktop 1 shows, empty: there.
    assert_eq!(reopen(&mut hp)?, Ok(0));
    close(&mut hp)?;
    // A window on it: the first empty one, desktop 2.
    open_on(&mut hp, "test.one", None)?;
    assert_eq!(reopen(&mut hp)?, Ok(1));
    close(&mut hp)?;
    // The four taken: a fifth, the session's.
    for (app, key) in [("test.two", KEY_2), ("test.three", KEY_3), ("test.four", 5)] {
        chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
        on(&mut hp, 0)?;
        open_on(&mut hp, app, Some(key))?;
    }
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    on(&mut hp, 0)?;
    assert_eq!(reopen(&mut hp)?, Ok(4));
    assert_eq!(hp.desktops_count(), 5);
    // Closed, the desktop added goes.
    close(&mut hp)?;
    assert_eq!(hp.desktops_count(), 4);
    // Nine, all taken: it does not open, and says so.
    config.desktops.count = 9;
    hp.reconfigure(config);
    wait(&mut hp, Duration::from_millis(200))?;
    for (app, key) in [
        ("test.five", 6),
        ("test.six", 7),
        ("test.seven", 8),
        ("test.eight", 9),
        ("test.nine", 10),
    ] {
        chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
        on(&mut hp, 0)?;
        open_on(&mut hp, app, Some(key))?;
    }
    let refused = hp.project_open("blog", SCREEN);
    assert_eq!(refused, Err("no free desktop".to_owned()));
    assert_eq!(hp.project_no_room().as_deref(), Some("blog"));
    assert_eq!(hp.desktops_count(), 9);
    assert!(hp.projects().iter().all(|p| p.2.is_none()));
    Ok(())
}

/// A closed project `name` of folder `folder`, its windows `contents`
/// (TOML), written as the compositor would before it starts.
fn closed_project(
    places: &Places,
    name: &str,
    folder: &Path,
    contents: &str,
) -> anyhow::Result<()> {
    let state = places.root.join("state/hocuspanda");
    std::fs::create_dir_all(state.join("projects.d"))?;
    let list = state.join("projects");
    let mut text = std::fs::read_to_string(&list).unwrap_or_default();
    text.push_str(&format!(
        "[[project]]\nid = \"{name}\"\nname = \"{name}\"\nfolder = \"{}\"\n\
         made = \"2026-11-13T09:00:00+01:00\"\nclosed = \"2026-11-13T18:40:00+01:00\"\n\n",
        folder.display()
    ));
    std::fs::write(&list, text)?;
    std::fs::write(state.join(format!("projects.d/{name}.toml")), contents)?;
    Ok(())
}

/// A saved window of `app` (TOML).
fn saved(app: &str) -> String {
    format!(
        "[[window]]\nentry = \"{app}\"\napp-id = \"{app}\"\nscreen = \"{SCREEN}\"\n\
         screen-size = [1920, 1080]\narea = [0, 0, 1920, 1080]\ndesktop = 1\n\
         place = [100, 100, 500, 400]\nstate = \"floating\"\n"
    )
}

/// Test 9: what did not come back is named in one notification (its
/// monogram its image); everything back, none.
#[test]
fn what_did_not_come_back_is_named_in_one_notification() -> anyhow::Result<()> {
    let places = places()?;
    app_entry(&places, "test.back", "Back", false)?;
    let apps = places.root.join("applications");
    std::fs::write(
        apps.join("test.missing.desktop"),
        "[Desktop Entry]\nType=Application\nName=Missing\nExec=/nonexistent/program\n",
    )?;
    closed_project(&places, "site", &places.folder, &saved("test.back"))?;
    let gone = places.root.join("home/code/blog");
    closed_project(
        &places,
        "blog",
        &gone,
        &format!("{}{}", saved("test.back"), saved("test.missing")),
    )?;
    let notifications =
        common::Notifications::start(&places.root.join("bus"))?.context("no notifications")?;
    let mut hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config_with_apps(&places),
        None,
        Some(notifications.address.clone()),
    )?;
    hp.pointer_move(600.0, 500.0);
    assert_eq!(hp.projects().len(), 2);

    // Everything back: no notification.
    hp.project_open("site", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let back = hp.dispatch_until(WAIT, |hp| !hp.project_coming_back("site"))?;
    anyhow::ensure!(back, "site not back: {:?}", hp.all_windows());
    assert_eq!(hp.project_windows("site").len(), 1);
    wait(&mut hp, Duration::from_millis(300))?;
    assert!(hp.project_not_back().is_none());
    assert!(notifications.notes().is_empty());
    // Its file went once everything came back.
    let site = places.root.join("state/hocuspanda/projects.d/site.toml");
    let removed = hp.dispatch_until(WAIT, |_| !site.exists())?;
    anyhow::ensure!(removed, "site's file stays");

    // One that cannot start, and the folder gone: one notification.
    hp.project_open("blog", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let told = hp.dispatch_until(WAIT, |_| !notifications.notes().is_empty())?;
    anyhow::ensure!(told, "no notification: {:?}", hp.project_not_back());
    wait(&mut hp, Duration::from_millis(300))?;
    let notes = notifications.notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].summary, "blog est revenu, sauf Missing");
    assert!(
        notes[0].body.contains("Missing n’a pas pu être rouverte."),
        "{notes:?}"
    );
    assert!(notes[0].body.contains("n’existe plus."), "{notes:?}");
    assert!(notes[0].image, "its monogram");
    assert_eq!(hp.project_windows("blog").len(), 1);
    Ok(())
}

/// Two windows of the session, each on a project's desktop (1 and 2),
/// saved as the session ends.
fn session_with_two_projects(places: &Places) -> anyhow::Result<()> {
    app_entry(places, "test.one", "One", false)?;
    app_entry(places, "test.two", "Two", false)?;
    let mut hp = alone_with(config_with_apps(places))?;
    hp.restore_session();
    let bar = common::client(hp.socket(), Some(common::Shows::ClearBar))?;
    std::mem::forget(bar);
    open_on(&mut hp, "test.one", None)?;
    open_on(&mut hp, "test.two", Some(KEY_2))?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "un", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let two = places.root.join("home/github/deux");
    std::fs::create_dir_all(&two)?;
    line.make(&mut hp, 2, "deux", &two)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    wait(&mut hp, Duration::from_millis(200))?;
    hp.end_session();
    let saved = std::fs::read_to_string(places.root.join("state/hocuspanda/session"))?;
    assert_eq!(saved.matches("[[project]]").count(), 2, "{saved}");
    assert_eq!(saved.matches("[[window]]").count(), 2, "{saved}");
    Ok(())
}

/// Test 10: log out with two projects open, log in: both on their
/// desktops, their windows back; with `[session] restore = false`, both
/// closed, listed with their windows.
#[test]
fn log_out_and_in_brings_the_open_projects_back_or_closes_them() -> anyhow::Result<()> {
    let places = places()?;
    session_with_two_projects(&places)?;
    let mut hp = alone_with(config_with_apps(&places))?;
    hp.restore_session();
    let bar = common::client(hp.socket(), Some(common::Shows::ClearBar))?;
    std::mem::forget(bar);
    let back = hp.dispatch_until(WAIT, |hp| {
        !hp.session_restoring() && hp.all_windows().len() == 2
    })?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    let open: Vec<_> = hp.projects().into_iter().map(|p| (p.0, p.2)).collect();
    assert_eq!(
        open,
        [
            ("un".to_owned(), Some((SCREEN.to_owned(), 0))),
            ("deux".to_owned(), Some((SCREEN.to_owned(), 1))),
        ]
    );
    assert_eq!(hp.project_windows("un"), [id(&hp, "test.one")?]);
    assert_eq!(hp.project_windows("deux"), [id(&hp, "test.two")?]);
    drop(hp);

    // The same, not restored: both closed, their windows kept.
    let places = self::places()?;
    session_with_two_projects(&places)?;
    let mut config = config_with_apps(&places);
    config.session.restore = false;
    let mut hp = alone_with(config)?;
    hp.restore_session();
    wait(&mut hp, Duration::from_millis(500))?;
    assert!(hp.all_windows().is_empty());
    assert!(
        hp.projects().iter().all(|p| p.2.is_none()),
        "{:?}",
        hp.projects()
    );
    assert_eq!(
        hp.project_contents("un"),
        [("test.one".to_owned(), "Floating".to_owned(), None)]
    );
    assert_eq!(
        hp.project_contents("deux"),
        [("test.two".to_owned(), "Floating".to_owned(), None)]
    );
    let file = places.root.join("state/hocuspanda/projects.d/deux.toml");
    let written = hp.dispatch_until(WAIT, |_| file.exists())?;
    anyhow::ensure!(written, "no contents' file");
    Ok(())
}

/// Types `text` (lower case letters, `-` and spaces) on a US keyboard.
fn type_text(hp: &mut Headless, text: &str) -> anyhow::Result<()> {
    for c in text.chars() {
        let code = match c {
            'a' => 30,
            'b' => 48,
            'c' => 46,
            'd' => 32,
            'e' => 18,
            'h' => 35,
            'i' => 23,
            'j' => 36,
            'l' => 38,
            'n' => 49,
            'o' => 24,
            'p' => 25,
            'r' => 19,
            's' => 31,
            't' => 20,
            'u' => 22,
            'z' => 44,
            '-' => 12,
            ' ' => 57,
            other => anyhow::bail!("no key for {other:?}"),
        };
        hp.key(code, true);
        hp.key(code, false);
    }
    wait(hp, Duration::from_millis(50))
}

/// A git repository at `home/path`, its index changed `ago` ago.
fn repository(home: &Path, path: &str, ago: Duration) -> anyhow::Result<PathBuf> {
    let top = home.join(path);
    std::fs::create_dir_all(top.join(".git"))?;
    let index = top.join(".git/index");
    std::fs::write(&index, "x")?;
    std::fs::File::options()
        .write(true)
        .open(&index)?
        .set_modified(std::time::SystemTime::now() - ago)?;
    Ok(top)
}

/// What the launcher last said it shows with `query`, once it says
/// `needs`.
fn launcher_rows(
    hp: &mut Headless,
    log: &Path,
    query: &str,
    needs: &str,
) -> anyhow::Result<String> {
    let wanted = format!("query={query:?}");
    let mut found = String::new();
    let shown = hp.dispatch_until(WAIT, |_| {
        found = common::logged(log)
            .lines()
            .rev()
            .find(|l| l.contains("the launcher shows it") && l.contains(&wanted))
            .unwrap_or_default()
            .to_owned();
        found.contains(needs)
    })?;
    anyhow::ensure!(
        shown,
        "the launcher never showed {needs:?} for {query:?}: {found}\n{}",
        common::logged(log)
            .lines()
            .filter(|l| l.contains("launcher")
                || l.contains("project")
                || l.contains("ERROR")
                || l.contains("panic"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(found)
}

const KEY_SPACE: u32 = 57;
const KEY_ENTER: u32 = 28;

/// `Super+Space`: the launcher opens, holding the keyboard.
fn launcher(hp: &mut Headless) -> anyhow::Result<()> {
    let gone = hp.dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())?;
    anyhow::ensure!(gone, "the launcher stays");
    chord(hp, &[KEY_LEFTMETA, KEY_SPACE]);
    let up = hp.dispatch_until(WAIT, |hp| hp.launcher_surfaces().len() == 1)?;
    anyhow::ensure!(up, "no launcher");
    let still = hp.dispatch_until(WAIT, Headless::launcher_at_rest)?;
    anyhow::ensure!(still, "the launcher still moves");
    Ok(())
}

/// Test 11: "projet hoc" lists the open project first, the closed one,
/// then the recent git repositories of the home, the newest first, none in
/// `node_modules`; `Enter` goes to the open one, reopens the closed one,
/// makes a project of a repository (a terminal opening in it, its desktop
/// empty); a project is found by its name alone too.
#[test]
fn the_launcher_lists_projects_then_repositories_and_enter_does_each() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    let hour = Duration::from_secs(3600);
    let panda = repository(&home, "github/hocus-panda", 3 * hour)?;
    let site = repository(&home, "github/hocus-site", 5 * hour)?;
    let cli = repository(&home, "github/hocus-cli", hour / 2)?;
    repository(&home, "github/hocus-old", 48 * hour)?;
    repository(&home, "node_modules/hocus-hidden", hour / 4)?;
    let (mut hp, log) = with_shell(&places)?;
    let mark = places.root.join("terminal");
    let mut config = config(&places);
    config.programs.terminal = format!("sh -c 'pwd >> {}'", mark.display());
    hp.reconfigure(config);
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 2, "hocus-panda", &panda)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    line.make(&mut hp, 3, "hocus-site", &site)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    hp.project_close("hocus-site")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    closed(&mut hp, "hocus-site")?;
    // Desktop 1 has a window: it is not free.
    open_on(&mut hp, "test.here", None)?;

    // "projet hoc": the open project, the closed one, the repositories.
    launcher(&mut hp)?;
    type_text(&mut hp, "projet hoc")?;
    let shown = launcher_rows(&mut hp, &log, "projet hoc", "hocus-old")?;
    let rows = shown
        .split("projects=")
        .nth(1)
        .context("no projects' rows")?
        .to_owned();
    let at = |what: &str| rows.find(what);
    let order = [
        at("*hocus-panda | ~/github/hocus-panda · sur le bureau 2 · 0 fenêtre | Entrée y va"),
        at("hocus-site | ~/github/hocus-site · fermé à"),
        at("Créer le projet « hocus-cli »"),
        at("Créer le projet « hocus-old »"),
    ];
    assert!(order.iter().all(Option::is_some), "{rows}");
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{rows}");
    assert!(rows.contains("Entrée le rouvre") || rows.contains("| Entrée le crée"));
    assert!(!rows.contains("hocus-hidden"), "{rows}");
    assert!(
        !rows.contains("« hocus-panda »"),
        "a project's folder: {rows}"
    );
    assert!(
        !shown.contains("cards=[\"*web") && !shown.contains("web:"),
        "{shown}"
    );

    // Enter on the open one: its desktop.
    hp.key(KEY_ENTER, true);
    hp.key(KEY_ENTER, false);
    on(&mut hp, 1)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    on(&mut hp, 0)?;

    // Enter on the closed one: it reopens, on the first free desktop (3).
    launcher(&mut hp)?;
    type_text(&mut hp, "projet hocus-s")?;
    let shown = launcher_rows(&mut hp, &log, "projet hocus-s", "hocus-site |")?;
    assert!(shown.contains("*hocus-site |"), "{shown}");
    hp.key(KEY_ENTER, true);
    hp.key(KEY_ENTER, false);
    let reopened = hp.dispatch_until(WAIT, |hp| {
        hp.projects()
            .iter()
            .any(|p| p.0 == "hocus-site" && p.2 == Some((SCREEN.to_owned(), 2)))
    })?;
    anyhow::ensure!(reopened, "{:?}", hp.projects());
    on(&mut hp, 2)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    on(&mut hp, 0)?;

    // Enter on a repository: a project on the first free desktop (4), a
    // terminal opening in its folder.
    launcher(&mut hp)?;
    type_text(&mut hp, "projet hocus-c")?;
    let shown = launcher_rows(&mut hp, &log, "projet hocus-c", "hocus-cli")?;
    assert!(shown.contains("*Créer le projet « hocus-cli »"), "{shown}");
    hp.key(KEY_ENTER, true);
    hp.key(KEY_ENTER, false);
    let made = hp.dispatch_until(WAIT, |hp| {
        hp.projects()
            .iter()
            .any(|p| p.0 == "hocus-cli" && p.2 == Some((SCREEN.to_owned(), 3)))
    })?;
    anyhow::ensure!(made, "{:?}", hp.projects());
    on(&mut hp, 3)?;
    let opened = hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&mark).is_ok_and(|t| t.contains("hocus-cli"))
    })?;
    anyhow::ensure!(opened, "no terminal in the project's folder");
    assert_eq!(
        std::fs::read_to_string(&mark)?.trim(),
        cli.canonicalize()?.display().to_string()
    );

    // By its name alone (§13, I): before the applications.
    launcher(&mut hp)?;
    type_text(&mut hp, "hocus-p")?;
    let shown = launcher_rows(&mut hp, &log, "hocus-p", "hocus-panda |")?;
    assert!(shown.contains("PROJETS"), "{shown}");
    assert!(shown.contains("*hocus-panda |"), "{shown}");
    // And nothing found says so.
    for _ in 0.."hocus-p".len() {
        hp.key(14, true);
        hp.key(14, false);
    }
    type_text(&mut hp, "projet zut")?;
    let shown = launcher_rows(&mut hp, &log, "projet zut", "notice=\"Aucun")?;
    assert!(
        shown.contains("notice=\"Aucun projet ni dépôt git pour « zut »."),
        "{shown}"
    );
    Ok(())
}

/// A project's screen unplugged (§1): it follows its windows to the screen
/// they go to, on the same-numbered desktop, and goes back with them.
#[test]
fn a_project_follows_its_windows_to_another_screen_and_back() -> anyhow::Result<()> {
    let places = places()?;
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?, "1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config(&places),
    )?;
    hp.pointer_move(600.0, 500.0);
    let mut line = Line::open(&mut hp)?;
    let made = line.make(&mut hp, 2, "hocus-site", &places.folder)?;
    anyhow::ensure!(made.is_ok(), "{made:?}");
    let site = open_on(&mut hp, "test.site", Some(KEY_2))?;
    assert_eq!(hp.project_windows("hocus-site"), [site]);
    hp.unplug(SCREEN)?;
    wait(&mut hp, Duration::from_millis(200))?;
    let there = hp.projects();
    assert_eq!(there[0].2, Some(("HEADLESS-2".to_owned(), 1)), "{there:?}");
    assert_eq!(hp.project_windows("hocus-site"), [site]);
    hp.plug(SCREEN)?;
    let back = hp.dispatch_until(WAIT, |hp| {
        hp.projects()[0].2 == Some((SCREEN.to_owned(), 1))
    })?;
    anyhow::ensure!(back, "{:?}", hp.projects());
    assert_eq!(hp.project_windows("hocus-site"), [site]);
    Ok(())
}
