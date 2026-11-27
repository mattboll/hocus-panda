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
    // The desktops' surface before the project: its name will take a line
    // above the tiles.
    let plain = hp
        .drawn_layers(hocus_link::DESKTOPS_NAMESPACE)
        .first()
        .map(|(_, r)| r.height)
        .context("no desktops' tiles")?;

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
    // The tiles where the user sees them: the surface drawn at its new
    // size, the project's line above the tiles it said. The shell says the
    // project as soon as it knows it, and on a loaded machine draws it a
    // while later: hovered from the tiles it said before, the pointer
    // lands on the project's line once drawn, not on a tile (#876).
    let told = hp.dispatch_until(WAIT, |hp| {
        let Some((_, surface)) = hp
            .drawn_layers(hocus_link::DESKTOPS_NAMESPACE)
            .into_iter()
            .next()
        else {
            return false;
        };
        let below = f64::from(surface.y + surface.height - plain);
        let tiles = hp.desk_tile_middles();
        surface.height > plain && tiles.len() == 4 && tiles.iter().all(|(.., (_, y))| *y > below)
    })?;
    anyhow::ensure!(
        told,
        "tiles: {:?} on {:?}",
        hp.desk_tile_middles(),
        hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE)
    );
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
/// when asked, unless [`APP_STUBBORN`] is set, titled [`TERMINAL_TITLE`]
/// when it is set. Run alone, it does nothing.
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
    if let Ok(title) = std::env::var(TERMINAL_TITLE) {
        client.commands.send(common::Command::Title(title.leak()))?;
    }
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

// --- Lot C: browsers' tabs ----------------------------------------------------------------
//
// Test 6 (docs/design/projects.md §12). A fake Firefox profile and a fake
// Chromium profile in the test's own home; the browsers are stand-ins (the
// test's own program) that write the arguments they are given.

/// A browser stand-in's entry `app` (its id and its windows' app id): a
/// wrapper writing its arguments into `marks/APP`, a line each start,
/// then running [`app_program`].
fn browser_wrapper(places: &Places, app: &str, name: &str) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let exe = std::env::current_exe()?;
    let dir = places.root.join("applications");
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(places.root.join("marks"))?;
    let wrapper = places.root.join(format!("{app}-wrapper"));
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n\
             exec env {APP_ID}={app} '{}' --exact projects::app_program --nocapture\n",
            places.root.join("marks").join(app).display(),
            exe.display()
        ),
    )?;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(
        dir.join(format!("{app}.desktop")),
        format!(
            "[Desktop Entry]\nType=Application\nName={name}\nCategories=Network;WebBrowser;\n\
             Exec=sh {}\n",
            wrapper.display()
        ),
    )?;
    Ok(wrapper)
}

/// The lines of arguments browser `app`'s stand-in was started with.
fn browser_marks(places: &Places, app: &str) -> Vec<String> {
    std::fs::read_to_string(places.root.join("marks").join(app))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// mozLz4 of `data`: its header and size, one LZ4 block of literals.
fn mozlz4(data: &[u8]) -> Vec<u8> {
    let mut out = b"mozLz40\0".to_vec();
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_le_bytes());
    let n = data.len();
    if n >= 15 {
        out.push(0xF0);
        let mut rest = n - 15;
        while rest >= 255 {
            out.push(255);
            rest -= 255;
        }
        out.push(u8::try_from(rest).unwrap_or(0));
    } else {
        out.push(u8::try_from(n).unwrap_or(0) << 4);
    }
    out.extend_from_slice(data);
    out
}

/// A Firefox profile in `home`, running (its lock names this process),
/// its session holding a window shown as "hocus-site · Accueil" and a
/// private one; and a Chromium profile whose session is the committed SNSS
/// file.
fn browser_profiles(home: &Path) -> anyhow::Result<()> {
    let root = home.join(".mozilla/firefox");
    let profile = root.join("t1.default-release");
    std::fs::create_dir_all(profile.join("sessionstore-backups"))?;
    std::fs::write(
        root.join("profiles.ini"),
        "[Profile0]\nName=default-release\nIsRelative=1\nPath=t1.default-release\nDefault=1\n",
    )?;
    std::os::unix::fs::symlink(
        format!("127.0.1.1:+{}", std::process::id()),
        profile.join("lock"),
    )?;
    let session = r#"{"version":["sessionrestore",1],"windows":[
        {"tabs":[
          {"entries":[{"url":"http://localhost:4321/","title":"hocus-site · Accueil"}],"index":1},
          {"entries":[{"url":"https://example.org/","title":"x"},{"url":"https://docs.astro.build/","title":"Astro Docs"}],"index":2,"pinned":true}],
         "selected":1,"width":500,"height":400},
        {"isPrivate":true,"tabs":[{"entries":[{"url":"https://secret.example/","title":"Secret"}],"index":1}],"selected":1}],
      "_closedWindows":[]}"#;
    std::fs::write(
        profile.join("sessionstore-backups/recovery.jsonlz4"),
        mozlz4(session.as_bytes()),
    )?;
    let sessions = home.join(".config/chromium/Default/Sessions");
    std::fs::create_dir_all(&sessions)?;
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/Session_13400000000000000"),
        sessions.join("Session_13400000000000000"),
    )?;
    Ok(())
}

/// Test 6: a project's Firefox and Chromium windows are matched by their
/// titles to their browsers' files, their tabs saved in order with the
/// one shown, a private window's never; reopened, each comes back as one
/// new window holding its addresses; one not found comes back without its
/// tabs, said.
#[test]
fn a_projects_browsers_come_back_with_their_tabs() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    browser_profiles(&home)?;
    let firefox = browser_wrapper(&places, "firefox", "Firefox")?;
    let chromium = browser_wrapper(&places, "chromium", "Chromium")?;
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        ..config_with_apps(&places)
    })?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let mut started = Vec::new();
    for (wrapper, title) in [
        (&firefox, "hocus-site · Accueil — Mozilla Firefox"),
        (&firefox, "Secret — Mozilla Firefox Private Browsing"),
        (&chromium, "smithay · docs.rs - Chromium"),
        (&chromium, "Not in its file - Chromium"),
    ] {
        let before = hp.all_windows().len();
        started.push(
            std::process::Command::new("sh")
                .arg(wrapper)
                .env("WAYLAND_DISPLAY", hp.socket())
                .env(TERMINAL_TITLE, title)
                .spawn()?,
        );
        let shown = hp.dispatch_until(WAIT, |hp| hp.all_windows().len() > before)?;
        anyhow::ensure!(shown, "{title} never showed");
    }
    wait(&mut hp, Duration::from_millis(300))?;
    let opened = |app: &str| browser_marks(&places, app).len();
    let (firefox_before, chromium_before) = (opened("firefox"), opened("chromium"));

    hp.project_close("hocus-site")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    closed(&mut hp, "hocus-site")?;
    for mut child in started {
        let _ = child.wait();
    }
    // Saved: each window found by its title, its tabs in order and the one
    // shown; the private window's none; the one not found, none read.
    let mut tabs = hp.project_tabs("hocus-site");
    tabs.sort();
    let urls = |u: &[&str]| u.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        tabs,
        [
            ("chromium".to_owned(), Some((0, Vec::new()))),
            (
                "chromium".to_owned(),
                Some((
                    2,
                    urls(&["https://github.com/a/b/pull/1", "https://docs.rs/smithay"])
                ))
            ),
            ("firefox".to_owned(), None),
            (
                "firefox".to_owned(),
                Some((
                    1,
                    urls(&["http://localhost:4321/", "https://docs.astro.build/"])
                ))
            ),
        ]
    );
    let file = places
        .root
        .join("state/hocuspanda/projects.d/hocus-site.toml");
    let written = hp.dispatch_until(WAIT, |_| file.exists())?;
    anyhow::ensure!(written, "no contents' file");
    let text = std::fs::read_to_string(&file)?;
    assert!(text.contains("[[window.browser.tab]]"), "{text}");
    assert!(!text.contains("secret"), "a private window's tab: {text}");

    // Reopened: one new window each, holding its addresses.
    hp.project_open("hocus-site", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let back = hp.dispatch_until(WAIT, |hp| !hp.project_coming_back("hocus-site"))?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    let mut firefox_now = browser_marks(&places, "firefox").split_off(firefox_before);
    firefox_now.sort();
    assert_eq!(
        firefox_now,
        ["", "http://localhost:4321/ https://docs.astro.build/"]
    );
    let mut chromium_now = browser_marks(&places, "chromium").split_off(chromium_before);
    chromium_now.sort();
    assert_eq!(
        chromium_now,
        [
            "",
            "--new-window https://github.com/a/b/pull/1 https://docs.rs/smithay"
        ]
    );
    assert_eq!(hp.project_windows("hocus-site").len(), 4);
    let (summary, body) = hp
        .project_not_back()
        .context("the window without its tabs")?;
    assert_eq!(
        summary,
        "hocus-site est revenu, sauf les onglets de Chromium"
    );
    assert_eq!(
        body,
        "Les onglets d’une fenêtre de Chromium n’ont pas pu être lus."
    );
    Ok(())
}

/// A Firefox profile in `home`, running, whose recovery file is `session`
/// (its JSON).
fn firefox_session(home: &Path, session: &str) -> anyhow::Result<()> {
    let root = home.join(".mozilla/firefox");
    let profile = root.join("t1.default-release");
    std::fs::create_dir_all(profile.join("sessionstore-backups"))?;
    std::fs::write(
        root.join("profiles.ini"),
        "[Profile0]\nName=default-release\nIsRelative=1\nPath=t1.default-release\nDefault=1\n",
    )?;
    std::os::unix::fs::symlink(
        format!("127.0.1.1:+{}", std::process::id()),
        profile.join("lock"),
    )?;
    std::fs::write(
        profile.join("sessionstore-backups/recovery.jsonlz4"),
        mozlz4(session.as_bytes()),
    )?;
    Ok(())
}

/// Starts browser stand-in `wrapper`'s window titled `title`, and runs
/// until it shows.
fn browser_window(
    hp: &mut Headless,
    wrapper: &Path,
    title: &str,
) -> anyhow::Result<std::process::Child> {
    let before = hp.all_windows().len();
    let child = std::process::Command::new("sh")
        .arg(wrapper)
        .env("WAYLAND_DISPLAY", hp.socket())
        .env(TERMINAL_TITLE, title)
        .spawn()?;
    let shown = hp.dispatch_until(WAIT, |hp| hp.all_windows().len() > before)?;
    anyhow::ensure!(shown, "{title} never showed");
    Ok(child)
}

/// #913: Firefox writes its file every 15 s, so as a project closes, the
/// window it closed is not in it yet, while an earlier copy of it (the
/// same tab shown, one tab: a window a reopening left) is among the
/// closed ones. The project keeps the tabs the window has open, never
/// those of a closed copy.
#[test]
fn a_browser_window_keeps_its_open_tabs_never_an_earlier_closed_copys() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    firefox_session(
        &home,
        r#"{"version":["sessionrestore",1],"windows":[
        {"tabs":[
          {"entries":[{"url":"https://a.example/","title":"A"}],"index":1},
          {"entries":[{"url":"https://board.example/","title":"chapito · board"}],"index":1},
          {"entries":[{"url":"https://c.example/","title":"C"}],"index":1}],
         "selected":2,"width":500,"height":400}],
      "_closedWindows":[
        {"tabs":[{"entries":[{"url":"https://board.example/","title":"chapito · board"}],"index":1}],
         "selected":1,"width":500,"height":400,"closedAt":1}]}"#,
    )?;
    let firefox = browser_wrapper(&places, "firefox", "Firefox")?;
    let mut hp = alone_with(Config {
        home: Some(home),
        ..config_with_apps(&places)
    })?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "chapito", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let mut child = browser_window(&mut hp, &firefox, "chapito · board — Mozilla Firefox")?;
    wait(&mut hp, Duration::from_millis(300))?;
    hp.project_close("chapito")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    closed(&mut hp, "chapito")?;
    let _ = child.wait();
    // What the closing read is kept a moment later still.
    wait(&mut hp, Duration::from_millis(1500))?;
    assert_eq!(
        hp.project_tabs("chapito"),
        [(
            "firefox".to_owned(),
            Some((
                2,
                [
                    "https://a.example/",
                    "https://board.example/",
                    "https://c.example/"
                ]
                .map(str::to_owned)
                .to_vec()
            ))
        )]
    );
    Ok(())
}

/// #913: Chrome writes a page's title only as it rebuilds its file (at
/// its start), so its file, as Chrome 154 wrote it (`Session_chrome_untitled`:
/// two windows, Alpha Beta Gamma then Delta), has none: a project's
/// window is found by elimination among the browser's windows, the one on
/// another desktop too, in the order they opened.
#[test]
fn a_chrome_window_whose_titles_its_file_lacks_keeps_its_tabs() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    let profile = home.join(".config/google-chrome/Default");
    std::fs::create_dir_all(profile.join("Sessions"))?;
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/Session_chrome_untitled"),
        profile.join("Sessions/Session_13439650394959575"),
    )?;
    std::fs::write(
        home.join(".config/google-chrome/Local State"),
        r#"{"profile":{"last_used":"Default"}}"#,
    )?;
    let chrome = browser_wrapper(&places, "google-chrome", "Google Chrome")?;
    let mut hp = alone_with(Config {
        home: Some(home),
        ..config_with_apps(&places)
    })?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "chapito", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let mut alpha = browser_window(&mut hp, &chrome, "Alpha page - Google Chrome")?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    on(&mut hp, 1)?;
    let mut delta = browser_window(&mut hp, &chrome, "Delta page - Google Chrome")?;
    wait(&mut hp, Duration::from_millis(300))?;
    hp.project_close("chapito")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let done = closed(&mut hp, "chapito");
    if done.is_err() {
        alpha.kill()?;
    }
    alpha.wait()?;
    // The window on the other desktop is not the project's: it stays.
    delta.kill()?;
    delta.wait()?;
    done?;
    assert_eq!(
        hp.project_tabs("chapito"),
        [(
            "google-chrome".to_owned(),
            Some((
                1,
                [
                    "http://127.0.0.1:8737/Alpha.html",
                    "http://127.0.0.1:8737/Beta.html",
                    "http://127.0.0.1:8737/Gamma.html"
                ]
                .map(str::to_owned)
                .to_vec()
            ))
        )]
    );
    Ok(())
}

// --- Lot D: agents' conversations ---------------------------------------------------------
//
// Test 7 (docs/design/projects.md §12). The agent's terminal is the
// terminal stand-in, which runs what follows its `--`; `claude` is a fake
// on a `PATH` of the test's that writes its arguments and its folder, then
// says `SessionStart` (`source` `resume`) through `hocus agent-hook`.

const SESSION: &str = "3f6c2a1e-9b0d-4c55-8e21-7d0a94c1b2e6";

/// The agent's terminal stand-in, `foot`: it writes its arguments and its
/// folder into `marks/foot`, runs what follows `--` (its `PATH` the
/// test's `bin` first, its `HOME` the test's, its `SHELL` one that ends at
/// once), then shows its window.
fn agent_terminal(places: &Places, bin: &Path, home: &Path) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let exe = std::env::current_exe()?;
    let dir = places.root.join("applications");
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(places.root.join("marks"))?;
    let wrapper = places.root.join("foot-wrapper");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\t%s\\n' \"$*\" \"$(pwd)\" >> '{marks}'\n\
             while [ $# -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\n\
             if [ \"$1\" = \"--\" ]; then shift; \
             ( PATH='{bin}':\"$PATH\" HOME='{home}' SHELL=/bin/true \"$@\" </dev/null >/dev/null 2>&1 & ); fi\n\
             exec env {TERMINAL_APP}=foot '{exe}' --exact projects::terminal_program --nocapture\n",
            marks = places.root.join("marks/foot").display(),
            bin = bin.display(),
            home = home.display(),
            exe = exe.display()
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

/// A fake `claude` in `bin`: it writes its arguments and its folder into
/// `marks/claude`, then its hook says the session started again.
fn fake_claude(places: &Places, bin: &Path, socket: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(bin)?;
    let claude = bin.join("claude");
    std::fs::write(
        &claude,
        format!(
            "#!/bin/sh\nprintf '%s\\t%s\\n' \"$*\" \"$(pwd)\" >> '{marks}'\n\
             printf '{{\"session_id\":\"%s\",\"hook_event_name\":\"SessionStart\",\"source\":\"resume\",\"cwd\":\"%s\"}}' \"$2\" \"$(pwd)\" \
             | '{hocus}' --socket '{socket}' agent-hook claude-code\n",
            marks = places.root.join("marks/claude").display(),
            hocus = common::hocus_program().display(),
            socket = socket.display()
        ),
    )?;
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// The lines `app`'s stand-in wrote: its arguments and its folder.
fn marks_of(places: &Places, app: &str) -> Vec<(String, PathBuf)> {
    std::fs::read_to_string(places.root.join("marks").join(app))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(args, dir)| (args.to_owned(), PathBuf::from(dir)))
        .collect()
}

/// Test 7: an agent's hooks' lines with its folder and transcript; closing
/// saves its conversation with its terminal; reopening starts its terminal
/// in its folder resuming it (`$SHELL -c 'claude --resume ID; exec
/// $SHELL'`), heard back; its transcript gone, its terminal opens in the
/// folder alone and the notification says so.
#[test]
fn an_agents_conversation_resumes_in_its_terminal() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    let bin = home.join("bin");
    let folder = places.folder.canonicalize()?;
    let transcript = home.join(format!(".claude/projects/x/{SESSION}.jsonl"));
    std::fs::create_dir_all(transcript.parent().context("its folder")?)?;
    std::fs::write(&transcript, "{}\n")?;
    let wrapper = agent_terminal(&places, &bin, &home)?;
    let agent_places = hocus_agents::tools::Places {
        home: home.clone(),
        config_home: home.join(".config"),
        codex_home: home.join(".codex"),
        path: vec![bin.clone()],
        herdr_socket: home.join(".config/herdr/herdr.sock"),
        processes: home.join("proc"),
        hocus: common::hocus_program(),
        state_dir: None,
    };
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        agent_places: Some(agent_places),
        ..config_with_apps(&places)
    })?;
    let socket = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    fake_claude(&places, &bin, &socket)?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-site", &folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let mut terminal = std::process::Command::new("sh")
        .arg(&wrapper)
        .current_dir(&folder)
        .env("WAYLAND_DISPLAY", hp.socket())
        .spawn()?;
    window_of(&mut hp, "foot")?;
    // Its hooks, as `hocus agent-hook` writes them: the agent's process is
    // its terminal's.
    let said = format!(
        "agent claude-code {SESSION} working {} hocus-site\tcwd {}\ttranscript {}\t\
         prompt refais la section du haut de l'accueil, plus courte\n",
        terminal.id(),
        folder.display(),
        transcript.display()
    );
    UnixStream::connect(&socket)?.write_all(said.as_bytes())?;
    let mut heard = false;
    for _ in 0..50 {
        wait(&mut hp, Duration::from_millis(100))?;
        if hp.agents().iter().any(|a| a.window != 0) {
            heard = true;
            break;
        }
    }
    anyhow::ensure!(heard, "the agent: {:?}", hp.agents());

    hp.project_close("hocus-site")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    closed(&mut hp, "hocus-site")?;
    let _ = terminal.wait();
    assert_eq!(
        hp.project_agents("hocus-site"),
        [[
            "claude-code".to_owned(),
            SESSION.to_owned(),
            folder.display().to_string(),
            transcript.display().to_string()
        ]]
    );
    let file = places
        .root
        .join("state/hocuspanda/projects.d/hocus-site.toml");
    let written = hp.dispatch_until(WAIT, |_| file.exists())?;
    anyhow::ensure!(written, "no contents' file");
    let text = std::fs::read_to_string(&file)?;
    assert!(text.contains("[window.agent]"), "{text}");
    assert!(
        text.contains("first-prompt = \"refais la section"),
        "{text}"
    );
    assert!(
        !text.contains("[window.terminal]"),
        "the agent's terminal: {text}"
    );
    // Its folder is one the form and the launcher propose.
    let folders = std::fs::read_to_string(places.root.join("state/hocuspanda/folders"))?;
    assert!(
        folders.starts_with(&format!("{}\t", folder.display())),
        "{folders}"
    );

    // Reopened: its terminal in its folder, resuming it, heard back.
    hp.project_open("hocus-site", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let back = hp.dispatch_until(WAIT, |hp| !hp.project_coming_back("hocus-site"))?;
    anyhow::ensure!(back, "not back: {:?}", hp.agents());
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/'))
        .unwrap_or_else(|| "/bin/sh".to_owned());
    let marks = marks_of(&places, "foot");
    assert_eq!(
        marks.last(),
        Some(&(
            format!(
                "--working-directory={} -- {shell} -c claude --resume {SESSION}; exec $SHELL",
                folder.display()
            ),
            folder.clone()
        )),
        "{marks:?}"
    );
    assert_eq!(
        marks_of(&places, "claude"),
        [(format!("--resume {SESSION}"), folder.clone())]
    );
    assert!(
        hp.project_not_back().is_none(),
        "{:?}",
        hp.project_not_back()
    );

    // Its transcript gone: its terminal in its folder alone, said.
    drop(hp);
    let gone = home.join("github/blog");
    std::fs::create_dir_all(&gone)?;
    closed_project(
        &places,
        "blog",
        &gone,
        &format!(
            "{}[window.agent]\ntool = \"claude-code\"\nsession = \"{SESSION}\"\n\
             folder = \"{}\"\ntranscript = \"{}\"\n",
            saved("foot"),
            gone.display(),
            home.join("nowhere.jsonl").display()
        ),
    )?;
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        ..config_with_apps(&places)
    })?;
    hp.project_open("blog", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let told = hp.dispatch_until(WAIT, |hp| hp.project_not_back().is_some())?;
    anyhow::ensure!(told, "nothing said");
    let marks = marks_of(&places, "foot");
    assert_eq!(
        marks.last(),
        Some(&(
            format!("--working-directory={}", gone.display()),
            gone.clone()
        )),
        "{marks:?}"
    );
    let (summary, body) = hp.project_not_back().context("said")?;
    assert_eq!(summary, "blog est revenu, sauf une conversation");
    assert_eq!(
        body,
        "La conversation de Claude Code n’a pas pu reprendre : elle n’est plus sur le disque. \
         Son terminal est ouvert dans blog."
    );
    Ok(())
}

// --- The last session's agents and browsers across a log out -------------------------------
//
// A session started by a display manager has a `PATH` of its own, without
// the folders the user's shell adds (`~/.local/bin`, where Claude Code's
// installer puts `claude`); a browser asked to close its windows one by
// one keeps all but the last as windows the user closed, which its own
// restore does not reopen.

/// An agent whose program is in the home's `~/.local/bin` alone, not on
/// the session's `PATH`, resumes its conversation at log in all the same,
/// that folder first on its command's `PATH`.
#[test]
fn an_agent_whose_program_is_only_in_the_homes_bin_resumes_at_log_in() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    let local = home.join(".local/bin");
    let session_bin = places.root.join("usr/bin");
    std::fs::create_dir_all(&session_bin)?;
    let folder = places.folder.canonicalize()?;
    let transcript = home.join(format!(".claude/projects/x/{SESSION}.jsonl"));
    std::fs::create_dir_all(transcript.parent().context("its folder")?)?;
    std::fs::write(&transcript, "{}\n")?;
    // The terminal's commands find no `claude` on its `PATH` either.
    agent_terminal(&places, &session_bin, &home)?;
    let state = places.root.join("state/hocuspanda");
    std::fs::create_dir_all(&state)?;
    std::fs::write(
        state.join("session"),
        format!(
            "{}[window.agent]\ntool = \"claude-code\"\nsession = \"{SESSION}\"\n\
             folder = \"{}\"\ntranscript = \"{}\"\n",
            saved("foot"),
            folder.display(),
            transcript.display()
        ),
    )?;
    let agent_places = hocus_agents::tools::Places {
        home: home.clone(),
        config_home: home.join(".config"),
        codex_home: home.join(".codex"),
        path: vec![session_bin.clone()],
        herdr_socket: home.join(".config/herdr/herdr.sock"),
        processes: home.join("proc"),
        hocus: common::hocus_program(),
        state_dir: None,
    };
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        agent_places: Some(agent_places),
        ..config_with_apps(&places)
    })?;
    hp.restore_session();
    let socket = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    fake_claude(&places, &local, &socket)?;
    let bar = common::client(hp.socket(), Some(common::Shows::ClearBar))?;
    std::mem::forget(bar);
    let back = hp.dispatch_until(WAIT, |hp| !hp.session_restoring())?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    assert_eq!(
        marks_of(&places, "claude"),
        [(format!("--resume {SESSION}"), folder.clone())]
    );
    let marks = marks_of(&places, "foot");
    let (args, _) = marks.last().context("no terminal")?;
    assert!(
        args.contains(&format!(
            "PATH='{}':\"$PATH\" claude --resume {SESSION}; exec $SHELL",
            local.display()
        )),
        "{args}"
    );
    assert!(
        hp.session_not_back().is_empty(),
        "{:?}",
        hp.session_not_back()
    );
    Ok(())
}

const BROWSER_WINDOWS: &str = "HOCUS_TEST_BROWSER_WINDOWS";
const BROWSER_CLOSED: &str = "HOCUS_TEST_BROWSER_CLOSED";
const BROWSER_CHILDREN: &str = "HOCUS_TEST_BROWSER_CHILDREN";

/// An application with several windows in one process, as Firefox or
/// kitty: a window of [`APP_ID`] for each title of [`BROWSER_WINDOWS`]
/// (`|` between them); a window asked to close closes alone, written into
/// [`BROWSER_CLOSED`]; with [`BROWSER_CHILDREN`], a child process for each
/// window (a terminal's shells), their ids written there a line each. It
/// ends with its last window, its connections gone, or on a signal, its
/// children with it. Run alone, it does nothing.
#[test]
fn browser_program() -> anyhow::Result<()> {
    let (Ok(app), Ok(titles)) = (std::env::var(APP_ID), std::env::var(BROWSER_WINDOWS)) else {
        return Ok(());
    };
    let app: &'static str = app.leak();
    let closed = std::env::var_os(BROWSER_CLOSED).map(PathBuf::from);
    let mut children = Vec::new();
    if let Some(file) = std::env::var_os(BROWSER_CHILDREN) {
        let mut ids = String::new();
        for _ in titles.split('|') {
            let child = std::process::Command::new("sleep").arg("600").spawn()?;
            ids.push_str(&format!("{}\n", child.id()));
            children.push(child);
        }
        std::fs::write(file, ids)?;
    }
    let mut windows = Vec::new();
    for title in titles.split('|') {
        let client = common::client(
            &display_socket(),
            Some(common::Shows::Window {
                app_id: app,
                size: (500, 400),
                popup: false,
            }),
        )?;
        client
            .commands
            .send(common::Command::Title(title.to_owned().leak()))?;
        windows.push(client);
    }
    while !windows.is_empty() {
        std::thread::sleep(Duration::from_millis(20));
        windows.retain(|client| {
            let mut asked = false;
            loop {
                match client.events.try_recv() {
                    Ok(e) => asked |= e == common::Event::CloseRequested,
                    Err(mpsc::TryRecvError::Empty) => break,
                    // The compositor went.
                    Err(mpsc::TryRecvError::Disconnected) => return false,
                }
            }
            if asked && let Some(file) = &closed {
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file)
                    .and_then(|mut f| f.write_all(b"closed\n"));
            }
            !asked
        });
    }
    // Its own children, by their handles.
    for mut child in children {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

/// Logging out asks a browser to quit as a whole (`SIGTERM`), never its
/// windows to close one by one, which it would keep as closed by the
/// user; the session saved as it ends has every browser window's tabs,
/// not only a project's.
#[test]
fn logging_out_asks_a_browser_to_quit_never_its_windows_one_by_one() -> anyhow::Result<()> {
    use std::os::unix::process::ExitStatusExt;
    let places = places()?;
    let home = places.root.join("home");
    browser_profiles(&home)?;
    // Its entry, for its windows to be saved.
    browser_wrapper(&places, "firefox", "Firefox")?;
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        ..config_with_apps(&places)
    })?;
    hp.restore_session();
    let closed = places.root.join("closed");
    let mut browser = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "projects::browser_program", "--nocapture"])
        .env("WAYLAND_DISPLAY", hp.socket())
        .env(APP_ID, "firefox")
        .env(
            BROWSER_WINDOWS,
            "hocus-site · Accueil — Mozilla Firefox|Ailleurs — Mozilla Firefox",
        )
        .env(BROWSER_CLOSED, &closed)
        .spawn()?;
    let shown = hp.dispatch_until(WAIT, |hp| hp.all_windows().len() == 2)?;
    anyhow::ensure!(shown, "the browser's windows: {:?}", hp.all_windows());
    wait(&mut hp, Duration::from_millis(300))?;

    hp.leave(hocus_link::LeaveAsk::Ask {
        action: hocus_link::LeaveAction::LogOut,
        output: SCREEN.to_owned(),
    });
    hp.leave(hocus_link::LeaveAsk::Now);
    let ended = hp.dispatch_until(WAIT, |hp| hp.ending().is_some())?;
    anyhow::ensure!(ended, "the session did not end: {:?}", hp.all_windows());
    let status = browser.wait()?;
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    assert!(
        !closed.exists(),
        "a window closed by itself: {:?}",
        std::fs::read_to_string(&closed)
    );
    // Saved as it ended, off a project's desktop: the window found in
    // Firefox's file with its tabs.
    hp.end_session();
    let session = places.root.join("state/hocuspanda/session");
    let text = std::fs::read_to_string(&session)?;
    assert_eq!(text.matches("[[window]]").count(), 2, "{text}");
    assert!(text.contains("url = \"http://localhost:4321/\""), "{text}");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&session)?.permissions().mode() & 0o777,
        0o600,
        "its addresses are the user's alone"
    );
    Ok(())
}

/// A saved browser window (TOML) of `app`, at `x`, with its tabs `urls`.
fn saved_browser(app: &str, x: i32, urls: &[&str]) -> String {
    let mut out = format!(
        "[[window]]\nentry = \"{app}\"\napp-id = \"{app}\"\nscreen = \"{SCREEN}\"\n\
         screen-size = [1920, 1080]\narea = [0, 0, 1920, 1080]\ndesktop = 1\n\
         place = [{x}, 100, 500, 400]\nstate = \"floating\"\n\
         [window.browser]\nkind = \"firefox\"\nselected = 1\n"
    );
    for url in urls {
        out.push_str(&format!("[[window.browser.tab]]\nurl = \"{url}\"\n"));
    }
    out
}

/// At log in, a browser that reopens its own windows (Firefox's start
/// page 3) is started once; a saved window of it that it did not reopen
/// (it was closed by itself at log out) comes back from its saved tabs, a
/// new window, and the browser counts as back.
#[test]
fn a_window_its_browser_does_not_reopen_comes_back_with_its_tabs() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    browser_profiles(&home)?;
    std::fs::write(
        home.join(".mozilla/firefox/t1.default-release/prefs.js"),
        "user_pref(\"browser.startup.page\", 3);\n",
    )?;
    browser_wrapper(&places, "firefox", "Firefox")?;
    let state = places.root.join("state/hocuspanda");
    std::fs::create_dir_all(&state)?;
    std::fs::write(
        state.join("session"),
        format!(
            "{}{}",
            saved_browser("firefox", 100, &["https://a.example/"]),
            saved_browser(
                "firefox",
                700,
                &["https://b.example/", "https://c.example/"]
            )
        ),
    )?;
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        ..config_with_apps(&places)
    })?;
    hp.restore_session();
    let bar = common::client(hp.socket(), Some(common::Shows::ClearBar))?;
    std::mem::forget(bar);
    // Started once, without addresses: it reopens one window of the two.
    let one = hp.dispatch_until(WAIT, |hp| hp.all_windows().len() == 1)?;
    anyhow::ensure!(one, "{:?}", hp.all_windows());
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(browser_marks(&places, "firefox"), [""]);
    assert!(hp.session_restoring());

    hp.session_browsers_waited();
    let back = hp.dispatch_until(WAIT, |hp| {
        !hp.session_restoring() && hp.all_windows().len() == 2
    })?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    assert_eq!(
        browser_marks(&places, "firefox"),
        ["", "https://b.example/ https://c.example/"]
    );
    let mut xs: Vec<i32> = hp
        .placements()
        .iter()
        .filter(|p| p.app_id.as_deref() == Some("firefox"))
        .map(|p| p.rect.x)
        .collect();
    xs.sort_unstable();
    assert_eq!(xs, [100, 700]);
    assert!(
        hp.session_not_back().is_empty(),
        "{:?}",
        hp.session_not_back()
    );
    Ok(())
}

/// #915: a Firefox whose log out's `SIGTERM` ended it before it wrote that
/// a project's window closed, and which reopens its own windows (here its
/// start page 3), at log in: the last session's window (showing "Inbox")
/// and, a moment later, `extra`, each a process of the stand-in, closing
/// when asked (said in `closed`); started with addresses, one window, as
/// [`browser_wrapper`]'s. The closed project "chapito" keeps a Firefox
/// window showing "chapito · board" among its three tabs.
fn log_in_with_a_restoring_firefox(
    places: &Places,
    extra: &str,
) -> anyhow::Result<(Headless, PathBuf)> {
    use std::os::unix::fs::PermissionsExt;
    let home = places.root.join("home");
    browser_profiles(&home)?;
    std::fs::write(
        home.join(".mozilla/firefox/t1.default-release/prefs.js"),
        "user_pref(\"browser.startup.page\", 3);\n",
    )?;
    let exe = std::env::current_exe()?;
    let closed = places.root.join("closed");
    let marks = places.root.join("marks");
    std::fs::create_dir_all(&marks)?;
    let wrapper = places.root.join("firefox-wrapper");
    let window = |title: &str| {
        format!(
            "env {APP_ID}=firefox {BROWSER_WINDOWS}='{title}' {BROWSER_CLOSED}='{}' \
             '{}' --exact projects::browser_program --nocapture",
            closed.display(),
            exe.display()
        )
    };
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n\
             if [ $# -gt 0 ]; then\n  \
               exec env {APP_ID}=firefox '{}' --exact projects::app_program --nocapture\n\
             fi\n\
             {} &\nsleep 0.5\nexec {}\n",
            marks.join("firefox").display(),
            exe.display(),
            window("Inbox — Mozilla Firefox"),
            window(extra),
        ),
    )?;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))?;
    let apps = places.root.join("applications");
    std::fs::create_dir_all(&apps)?;
    std::fs::write(
        apps.join("firefox.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Firefox\n\
             Categories=Network;WebBrowser;\nExec=sh {}\n",
            wrapper.display()
        ),
    )?;
    let tabs = |titles: &[(&str, &str)]| -> String {
        titles
            .iter()
            .map(|(url, title)| {
                format!("[[window.browser.tab]]\nurl = \"{url}\"\ntitle = \"{title}\"\n")
            })
            .collect()
    };
    let state = places.root.join("state/hocuspanda");
    std::fs::create_dir_all(&state)?;
    std::fs::write(
        state.join("session"),
        format!(
            "{}[window.browser]\nkind = \"firefox\"\nselected = 1\n{}",
            saved("firefox"),
            tabs(&[("https://mail.example/", "Inbox")])
        ),
    )?;
    closed_project(
        places,
        "chapito",
        &places.folder,
        &format!(
            "{}[window.browser]\nkind = \"firefox\"\nselected = 2\n{}",
            saved("firefox").replace("desktop = 1", "desktop = 2"),
            tabs(&[
                ("https://a.example/", "A"),
                ("https://board.example/", "chapito · board"),
                ("https://c.example/", "C"),
            ])
        ),
    )?;
    let mut hp = alone_with(Config {
        home: Some(home),
        ..config_with_apps(places)
    })?;
    hp.restore_session();
    let bar = common::client(hp.socket(), Some(common::Shows::ClearBar))?;
    std::mem::forget(bar);
    Ok((hp, closed))
}

/// The titles of the windows of `app`, sorted.
fn titles_of(hp: &Headless, app: &str) -> Vec<String> {
    let mut out: Vec<String> = hp
        .all_windows()
        .iter()
        .filter(|w| w.1.as_deref() == Some(app))
        .map(|w| hp.window_title(w.0))
        .collect();
    out.sort();
    out
}

/// #915: at log in, the copy of a closed project's window that Firefox
/// reopens by itself, besides the session's, is asked to close at once;
/// the session's stays in its place. Reopening the project then brings
/// its window back with its tabs, once.
#[test]
fn a_closed_projects_window_its_browser_reopens_at_log_in_is_closed() -> anyhow::Result<()> {
    let places = places()?;
    let (mut hp, closed) =
        log_in_with_a_restoring_firefox(&places, "chapito · board — Mozilla Firefox")?;
    let gone = hp.dispatch_until(WAIT, |hp| {
        closed.exists() && hp.all_windows().len() == 1 && !hp.session_restoring()
    })?;
    anyhow::ensure!(
        gone,
        "{:?} {:?}",
        titles_of(&hp, "firefox"),
        std::fs::read_to_string(&closed)
    );
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(titles_of(&hp, "firefox"), ["Inbox — Mozilla Firefox"]);
    assert_eq!(std::fs::read_to_string(&closed)?, "closed\n");
    assert_eq!(browser_marks(&places, "firefox"), [""]);
    let xs: Vec<i32> = hp.placements().iter().map(|p| p.rect.x).collect();
    assert_eq!(xs, [100], "the session's window in its place");

    hp.project_open("chapito", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let back = hp.dispatch_until(WAIT, |hp| !hp.project_coming_back("chapito"))?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(
        browser_marks(&places, "firefox"),
        [
            "",
            "https://a.example/ https://board.example/ https://c.example/"
        ]
    );
    assert_eq!(hp.project_windows("chapito").len(), 1);
    assert_eq!(hp.all_windows().len(), 2);
    assert_eq!(std::fs::read_to_string(&closed)?, "closed\n");
    Ok(())
}

/// #915: a window Firefox reopens by itself at log in that shows no
/// closed project's tab stays, even unplaced.
#[test]
fn a_window_its_browser_reopens_showing_no_closed_projects_tab_stays() -> anyhow::Result<()> {
    let places = places()?;
    let (mut hp, closed) = log_in_with_a_restoring_firefox(&places, "Ailleurs — Mozilla Firefox")?;
    let both = hp.dispatch_until(WAIT, |hp| {
        titles_of(hp, "firefox") == ["Ailleurs — Mozilla Firefox", "Inbox — Mozilla Firefox"]
    })?;
    anyhow::ensure!(both, "{:?}", titles_of(&hp, "firefox"));
    wait(&mut hp, Duration::from_secs(1))?;
    assert_eq!(hp.all_windows().len(), 2);
    assert!(
        !closed.exists(),
        "asked to close: {:?}",
        std::fs::read_to_string(&closed)
    );
    Ok(())
}

/// One terminal process serving two windows (kitty's, foot's server), an
/// agent in each: each agent's window is the one that had the keyboard
/// as its prompt was sent, the session saves each conversation with its
/// own window, and a prompt sent while another application's window has
/// the keyboard changes nothing.
#[test]
fn each_agent_of_a_terminal_with_two_windows_is_in_the_window_its_prompt_was_typed_in()
-> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    terminal_wrapper(&places, None, None)?;
    let folder = places.folder.canonicalize()?;
    let transcript = home.join("t.jsonl");
    std::fs::create_dir_all(&home)?;
    std::fs::write(&transcript, "{}\n")?;
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        ..config_with_apps(&places)
    })?;
    hp.restore_session();
    let socket = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-site", &folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let shells = places.root.join("shells");
    let mut terminal = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "projects::browser_program", "--nocapture"])
        .env("WAYLAND_DISPLAY", hp.socket())
        .env(APP_ID, "foot")
        .env(BROWSER_WINDOWS, "one|two")
        .env(BROWSER_CHILDREN, &shells)
        .spawn()?;
    let shown = hp.dispatch_until(WAIT, |hp| hp.all_windows().len() == 2 && shells.exists())?;
    anyhow::ensure!(shown, "the terminal's windows: {:?}", hp.all_windows());
    let mut ids: Vec<WindowId> = hp.all_windows().iter().map(|w| w.0).collect();
    ids.sort();
    let (a, b) = (ids[0], ids[1]);
    hp.place_window(a, hocus_compositor::model::Rect::new(100, 100, 500, 400));
    hp.place_window(b, hocus_compositor::model::Rect::new(900, 100, 500, 400));
    let pids: Vec<u32> = std::fs::read_to_string(&shells)?
        .lines()
        .filter_map(|l| l.parse().ok())
        .collect();
    anyhow::ensure!(pids.len() == 2, "its shells: {pids:?}");
    let say = |hp: &mut Headless, session: &str, pid: u32, name: &str| -> anyhow::Result<()> {
        let said = format!(
            "agent claude-code {session} working {pid} {name}\tcwd {}\ttranscript {}\t\
             prompt une question\n",
            folder.display(),
            transcript.display()
        );
        UnixStream::connect(&socket)?.write_all(said.as_bytes())?;
        wait(hp, Duration::from_millis(400))
    };
    let window_of = |hp: &mut Headless, name: &str| {
        hp.agents()
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.window)
    };
    let (one, two) = ("aaaa-1111", "bbbb-2222");
    hp.focus_window(a);
    say(&mut hp, one, pids[0], "un")?;
    hp.focus_window(b);
    say(&mut hp, two, pids[1], "deux")?;
    assert_eq!(window_of(&mut hp, "un"), Some(a.0));
    assert_eq!(window_of(&mut hp, "deux"), Some(b.0));

    // A prompt sent while another application's window has the keyboard:
    // its window stays.
    let (_, other) = open_with(&mut hp, "test.other", (300, 200), false)?;
    let other_id = id(&hp, "test.other")?;
    hp.focus_window(other_id);
    say(&mut hp, one, pids[0], "un")?;
    assert_eq!(window_of(&mut hp, "un"), Some(a.0));
    drop(other);
    wait(&mut hp, Duration::from_millis(200))?;

    // Saved as the session ends: each conversation with its own window.
    hp.end_session();
    let text = std::fs::read_to_string(places.root.join("state/hocuspanda/session"))?;
    let placed = |session: &str| {
        text.split("[[window]]")
            .find(|w| w.contains(&format!("session = \"{session}\"")))
            .and_then(|w| w.lines().find(|l| l.starts_with("place = ")))
            .map(str::to_owned)
    };
    assert_eq!(
        placed(one).as_deref(),
        Some("place = [100, 100, 500, 400]"),
        "{text}"
    );
    assert_eq!(
        placed(two).as_deref(),
        Some("place = [900, 100, 500, 400]"),
        "{text}"
    );
    drop(hp);
    let _ = terminal.wait();
    Ok(())
}

// --- One herdr session per project (#887) ----------------------------------------------
//
// herdr's named sessions, each a fake herdr on its own socket
// (`sessions/NAME/herdr.sock` in the test's home), their servers, panes
// and clients in the test's own `/proc`; each session's terminal a
// stand-in of its own process, its client's command line naming the
// session.

/// The processes of the test's own `/proc`, above the kernel's largest pid
/// (`PID_MAX_LIMIT`, 2^22): none is the machine's.
const ALPHA_SERVER: u32 = 4_300_001;
const ALPHA_PANE: u32 = 4_300_002;
const ALPHA_SHELL: u32 = 4_300_003;
const ALPHA_CLIENT: u32 = 4_300_004;
const BETA_SERVER: u32 = 4_300_011;
const BETA_PANE: u32 = 4_300_012;
const BETA_SHELL: u32 = 4_300_013;
const BETA_CLIENT: u32 = 4_300_014;

/// Process `pid` in `home`'s own `/proc`: its `stat` and its `cmdline`.
fn fake_process(
    home: &Path,
    pid: u32,
    name: &str,
    parent: u32,
    argv: &[&str],
) -> anyhow::Result<()> {
    let dir = home.join("proc").join(pid.to_string());
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("stat"),
        format!("{pid} ({name}) S {parent} {pid} {pid} 0 -1"),
    )?;
    let mut cmdline = Vec::new();
    for arg in argv {
        cmdline.extend_from_slice(arg.as_bytes());
        cmdline.push(0);
    }
    std::fs::write(dir.join("cmdline"), cmdline)?;
    Ok(())
}

/// The agents' places of a test whose home is `home`, its programs in
/// `bin`: herdr's sockets and its clients there, never the user's.
fn herdr_places(home: &Path, bin: &Path) -> hocus_agents::tools::Places {
    hocus_agents::tools::Places {
        home: home.to_path_buf(),
        config_home: home.join(".config"),
        codex_home: home.join(".codex"),
        path: vec![bin.to_path_buf()],
        herdr_socket: home.join(".config/herdr/herdr.sock"),
        processes: home.join("proc"),
        hocus: common::hocus_program(),
        state_dir: None,
    }
}

/// A herdr session, `name`, faked: its server and its pane's Claude Code
/// (`pane`) in the test's `/proc`, one agent in workspace `label`.
fn herdr_session(
    home: &Path,
    name: &str,
    label: &str,
    (server, pane): (u32, u32),
) -> anyhow::Result<common::herdr::Heard> {
    fake_process(home, server, "herdr", 1, &["herdr", "server"])?;
    fake_process(home, pane, "claude", server, &["claude"])?;
    let heard = common::herdr::Heard::default();
    let held: common::herdr::Herdr = std::sync::Arc::new(std::sync::Mutex::new(
        common::herdr::FakeHerdr::with(label, &["1"], None),
    ));
    // Its thread waits on its socket for the test's life.
    let _thread = common::herdr::fake_herdr(
        &home
            .join(".config/herdr/sessions")
            .join(name)
            .join("herdr.sock"),
        pane,
        held,
        heard.clone(),
    )?;
    Ok(heard)
}

/// A terminal stand-in of its own process, a window of `foot` titled
/// `title`, its herdr client (`client`, its command line `argv`) under a
/// shell of its (`shell`) in the test's `/proc`; its window's id.
fn herdr_terminal(
    hp: &mut Headless,
    home: &Path,
    title: &str,
    (shell, client): (u32, u32),
    argv: &[&str],
) -> anyhow::Result<(std::process::Child, WindowId)> {
    let before: Vec<WindowId> = hp.all_windows().iter().map(|w| w.0).collect();
    let terminal = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "projects::browser_program", "--nocapture"])
        .env("WAYLAND_DISPLAY", hp.socket())
        .env(APP_ID, "foot")
        .env(BROWSER_WINDOWS, title)
        .spawn()?;
    fake_process(home, shell, "zsh", terminal.id(), &["/usr/bin/zsh"])?;
    fake_process(home, client, "herdr", shell, argv)?;
    let shown = hp.dispatch_until(WAIT, |hp| {
        hp.all_windows().iter().any(|w| !before.contains(&w.0))
    })?;
    anyhow::ensure!(shown, "no terminal: {:?}", hp.all_windows());
    let id = hp
        .all_windows()
        .iter()
        .map(|w| w.0)
        .find(|w| !before.contains(w))
        .context("its window")?;
    Ok((terminal, id))
}

/// A fake `herdr` program in `bin`: it writes its arguments into
/// `marks/herdr`.
fn fake_herdr_program(places: &Places, bin: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(bin)?;
    let program = bin.join("herdr");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
            places.root.join("marks/herdr").display()
        ),
    )?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// Runs until agent `name`'s window is `window`; its id.
fn agent_in(hp: &mut Headless, name: &str, window: WindowId) -> anyhow::Result<u64> {
    for _ in 0..100 {
        if let Some(agent) = hp
            .agents()
            .iter()
            .find(|a| a.name == name && a.window == window.0)
        {
            return Ok(agent.id);
        }
        wait(hp, Duration::from_millis(100))?;
    }
    anyhow::bail!("{name} not in window {}: {:?}", window.0, hp.agents())
}

/// Two named herdr sessions, each in a terminal of its own: each
/// session's agents are in its own terminal's window, told apart by its
/// client's command line (`herdr --session alpha`, `herdr session attach
/// beta`), and going to one selects its pane in its own session only.
#[test]
fn each_herdr_sessions_agents_are_in_its_own_terminals_window() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin)?;
    let alpha = herdr_session(&home, "alpha", "site", (ALPHA_SERVER, ALPHA_PANE))?;
    let beta = herdr_session(&home, "beta", "blog", (BETA_SERVER, BETA_PANE))?;
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        agent_places: Some(herdr_places(&home, &bin)),
        ..config_with_apps(&places)
    })?;
    let (mut one, a) = herdr_terminal(
        &mut hp,
        &home,
        "herdr --session alpha",
        (ALPHA_SHELL, ALPHA_CLIENT),
        &["herdr", "--session", "alpha"],
    )?;
    let (mut two, b) = herdr_terminal(
        &mut hp,
        &home,
        "herdr session attach beta",
        (BETA_SHELL, BETA_CLIENT),
        &["/home/alex/.local/bin/herdr", "session", "attach", "beta"],
    )?;
    hp.place_window(a, hocus_compositor::model::Rect::new(100, 100, 500, 400));
    hp.place_window(b, hocus_compositor::model::Rect::new(900, 100, 500, 400));
    // The keyboard moving looks for herdr's clients again.
    hp.focus_window(a);
    hp.focus_window(b);
    agent_in(&mut hp, "site", a)?;
    let blog = agent_in(&mut hp, "blog", b)?;
    // From the other window, its row: its own window, its own session's
    // pane.
    hp.focus_window(a);
    hp.go_to_agent(blog);
    let wanted = r#"pane.focus {"pane_id":"w1:p1"}"#;
    let asked = hp.dispatch_until(WAIT, |_| {
        beta.lock().is_ok_and(|h| h.iter().any(|l| l == wanted))
    })?;
    anyhow::ensure!(asked, "beta was asked {:?}", beta.lock().map(|h| h.clone()));
    let focused = hp.dispatch_until(WAIT, |hp| hp.focused_window() == Some(b))?;
    anyhow::ensure!(focused, "the keyboard on {:?}", hp.focused_window());
    assert!(
        alpha
            .lock()
            .is_ok_and(|h| !h.iter().any(|l| l.starts_with("pane.focus"))),
        "alpha was asked {:?}",
        alpha.lock().map(|h| h.clone())
    );
    drop(hp);
    let _ = one.wait();
    let _ = two.wait();
    Ok(())
}

/// A project whose desktop holds herdr session alpha's terminal owns that
/// session and its agents, not session beta's on another desktop; it
/// closes keeping one herdr session, not a terminal per agent, and
/// reopens one terminal running `herdr session attach alpha` at its
/// place, its agents still running in herdr's server.
#[test]
fn a_project_keeps_its_herdr_session_and_attaches_to_it_again() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    let bin = home.join("bin");
    let folder = places.folder.canonicalize()?;
    agent_terminal(&places, &bin, &home)?;
    fake_herdr_program(&places, &bin)?;
    let transcript = home.join("t.jsonl");
    std::fs::write(&transcript, "{}\n")?;
    herdr_session(&home, "alpha", "site", (ALPHA_SERVER, ALPHA_PANE))?;
    herdr_session(&home, "beta", "blog", (BETA_SERVER, BETA_PANE))?;
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        agent_places: Some(herdr_places(&home, &bin)),
        ..config_with_apps(&places)
    })?;
    let socket = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-site", &folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    let (mut one, a) = herdr_terminal(
        &mut hp,
        &home,
        "herdr --session alpha",
        (ALPHA_SHELL, ALPHA_CLIENT),
        &["herdr", "--session", "alpha"],
    )?;
    let (mut two, b) = herdr_terminal(
        &mut hp,
        &home,
        "herdr session attach beta",
        (BETA_SHELL, BETA_CLIENT),
        &["herdr", "session", "attach", "beta"],
    )?;
    hp.place_window(a, hocus_compositor::model::Rect::new(100, 100, 500, 400));
    // Beta's terminal to the second desktop.
    hp.focus_window(b);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    let away = hp.dispatch_until(WAIT, |hp| {
        hp.all_windows().iter().any(|w| w.0 == b && w.2 == 1)
    })?;
    anyhow::ensure!(away, "{:?}", hp.all_windows());
    hp.focus_window(a);
    // Each session's Claude Code, as its hooks say it: its process is its
    // pane's.
    for (session, pid, name) in [
        ("aaaa-1111", ALPHA_PANE, "site"),
        ("bbbb-2222", BETA_PANE, "blog"),
    ] {
        let said = format!(
            "agent claude-code {session} working {pid} {name}\tcwd {}\ttranscript {}\n",
            folder.display(),
            transcript.display()
        );
        UnixStream::connect(&socket)?.write_all(said.as_bytes())?;
    }
    agent_in(&mut hp, "site", a)?;
    agent_in(&mut hp, "blog", b)?;
    // Each one agent, its hooks' and its pane's.
    let mut merged = 0;
    for _ in 0..100 {
        merged = hp
            .agents()
            .iter()
            .filter(|a| a.source == "claude-code+herdr")
            .count();
        if merged == 2 {
            break;
        }
        wait(&mut hp, Duration::from_millis(100))?;
    }
    assert_eq!(merged, 2, "{:?}", hp.agents());

    hp.project_close("hocus-site")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    closed(&mut hp, "hocus-site")?;
    let _ = one.wait();
    assert_eq!(
        hp.project_herdr("hocus-site"),
        [("alpha".to_owned(), vec!["aaaa-1111".to_owned()])]
    );
    assert!(
        hp.project_agents("hocus-site").is_empty(),
        "{:?}",
        hp.project_agents("hocus-site")
    );
    let file = places
        .root
        .join("state/hocuspanda/projects.d/hocus-site.toml");
    let written = hp.dispatch_until(WAIT, |_| file.exists())?;
    anyhow::ensure!(written, "no contents' file");
    let text = std::fs::read_to_string(&file)?;
    assert!(text.contains("[window.herdr]"), "{text}");
    assert!(text.contains("session = \"alpha\""), "{text}");
    assert!(!text.contains("bbbb-2222"), "{text}");
    assert!(!text.contains("[window.agent]"), "{text}");

    // Reopened: one terminal, attaching to alpha again.
    let before = marks_of(&places, "foot").len();
    hp.project_open("hocus-site", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let back = hp.dispatch_until(WAIT, |hp| !hp.project_coming_back("hocus-site"))?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    let marks = marks_of(&places, "foot");
    assert_eq!(marks.len(), before + 1, "{marks:?}");
    let (args, _) = marks.last().context("no terminal")?;
    assert!(
        args.ends_with("-c herdr session attach alpha; exec $SHELL"),
        "{args}"
    );
    let attached = hp.dispatch_until(WAIT, |_| places.root.join("marks/herdr").exists())?;
    anyhow::ensure!(attached, "herdr never ran");
    assert_eq!(
        std::fs::read_to_string(places.root.join("marks/herdr"))?,
        "session attach alpha\n"
    );
    assert!(
        hp.project_not_back().is_none(),
        "{:?}",
        hp.project_not_back()
    );
    drop(hp);
    let _ = two.wait();
    Ok(())
}

/// A project's herdr session gone as it reopens (its server stopped): its
/// agents' conversations resume each in a terminal of its own, as before
/// herdr's sessions were kept.
#[test]
fn a_herdr_session_gone_resumes_its_agents_each_in_a_terminal() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    let bin = home.join("bin");
    let folder = places.folder.canonicalize()?;
    let transcript = home.join(format!(".claude/projects/x/{SESSION}.jsonl"));
    std::fs::create_dir_all(transcript.parent().context("its folder")?)?;
    std::fs::write(&transcript, "{}\n")?;
    agent_terminal(&places, &bin, &home)?;
    closed_project(
        &places,
        "hocus-site",
        &folder,
        &format!(
            "{}[window.herdr]\nsession = \"alpha\"\n\n[[window.herdr.agents]]\n\
             tool = \"claude-code\"\nsession = \"{SESSION}\"\nfolder = \"{}\"\n\
             transcript = \"{}\"\n",
            saved("foot"),
            folder.display(),
            transcript.display()
        ),
    )?;
    let mut hp = alone_with(Config {
        home: Some(home.clone()),
        agent_places: Some(herdr_places(&home, &bin)),
        ..config_with_apps(&places)
    })?;
    let socket = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    fake_claude(&places, &bin, &socket)?;
    hp.project_open("hocus-site", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let back = hp.dispatch_until(WAIT, |hp| !hp.project_coming_back("hocus-site"))?;
    anyhow::ensure!(back, "not back: {:?}", hp.agents());
    let marks = marks_of(&places, "foot");
    let (args, dir) = marks.last().context("no terminal")?;
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert!(
        args.ends_with(&format!("-c claude --resume {SESSION}; exec $SHELL")),
        "{args}"
    );
    assert_eq!(dir, &folder);
    assert_eq!(
        marks_of(&places, "claude"),
        [(format!("--resume {SESSION}"), folder.clone())]
    );
    assert!(
        hp.project_not_back().is_none(),
        "{:?}",
        hp.project_not_back()
    );
    Ok(())
}

// --- Log out and log in, off any project (#967) ------------------------------------------

/// A terminal running herdr and one running Claude Code, on a desktop of
/// no project, are saved at log out with herdr's session and the agent's
/// conversation, and come back at log in running them: `herdr session
/// attach alpha`, `claude --resume ID` in its folder, as a project's.
#[test]
fn a_terminal_off_a_project_comes_back_running_its_herdr_or_its_agent() -> anyhow::Result<()> {
    let places = places()?;
    let home = places.root.join("home");
    let bin = home.join("bin");
    let folder = places.folder.canonicalize()?;
    let transcript = home.join(format!(".claude/projects/x/{SESSION}.jsonl"));
    std::fs::create_dir_all(transcript.parent().context("its folder")?)?;
    std::fs::write(&transcript, "{}\n")?;
    let wrapper = agent_terminal(&places, &bin, &home)?;
    fake_herdr_program(&places, &bin)?;
    herdr_session(&home, "alpha", "site", (ALPHA_SERVER, ALPHA_PANE))?;
    let config = Config {
        home: Some(home.clone()),
        agent_places: Some(herdr_places(&home, &bin)),
        ..config_with_apps(&places)
    };
    let mut hp = alone_with(config.clone())?;
    hp.restore_session();
    let socket = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let (mut one, herdr) = herdr_terminal(
        &mut hp,
        &home,
        "herdr session attach alpha",
        (ALPHA_SHELL, ALPHA_CLIENT),
        &["herdr", "session", "attach", "alpha"],
    )?;
    agent_in(&mut hp, "site", herdr)?;
    let mut two = std::process::Command::new("sh")
        .arg(&wrapper)
        .current_dir(&folder)
        .env("WAYLAND_DISPLAY", hp.socket())
        .spawn()?;
    let said = format!(
        "agent claude-code {SESSION} working {} hocus-site\tcwd {}\ttranscript {}\n",
        two.id(),
        folder.display(),
        transcript.display()
    );
    UnixStream::connect(&socket)?.write_all(said.as_bytes())?;
    let other = |hp: &Headless| hp.all_windows().iter().map(|w| w.0).find(|w| *w != herdr);
    let shown = hp.dispatch_until(WAIT, |hp| other(hp).is_some())?;
    anyhow::ensure!(shown, "no agent's terminal: {:?}", hp.all_windows());
    let window = other(&hp).context("its window")?;
    agent_in(&mut hp, "hocus-site", window)?;
    hp.end_session();
    let saved = std::fs::read_to_string(places.root.join("state/hocuspanda/session"))?;
    assert!(
        saved.contains("[window.herdr]\nsession = \"alpha\""),
        "{saved}"
    );
    assert!(
        saved.contains(&format!(
            "[window.agent]\ntool = \"claude-code\"\nsession = \"{SESSION}\""
        )),
        "{saved}"
    );
    drop(hp);
    let _ = one.wait();
    let _ = two.wait();

    // Log in: each terminal running what it ran.
    let mut hp = alone_with(config)?;
    hp.restore_session();
    let socket = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    fake_claude(&places, &bin, &socket)?;
    let bar = common::client(hp.socket(), Some(common::Shows::ClearBar))?;
    std::mem::forget(bar);
    let back = hp.dispatch_until(WAIT, |hp| !hp.session_restoring())?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    assert_eq!(
        marks_of(&places, "claude"),
        [(format!("--resume {SESSION}"), folder.clone())]
    );
    let attached = hp.dispatch_until(WAIT, |_| places.root.join("marks/herdr").exists())?;
    anyhow::ensure!(attached, "herdr never started");
    assert_eq!(
        std::fs::read_to_string(places.root.join("marks/herdr"))?,
        "session attach alpha\n"
    );
    assert!(
        hp.session_not_back().is_empty(),
        "{:?}",
        hp.session_not_back()
    );
    Ok(())
}

/// A shell that went into another folder to run a program there (a
/// subshell: the shell itself stays where it was) is saved at log out in
/// that program's folder, and its terminal comes back there (#967).
#[test]
fn a_terminal_comes_back_in_the_folder_of_the_program_it_ran() -> anyhow::Result<()> {
    let places = places()?;
    let folder = places.folder.canonicalize()?;
    let inside = folder.join("src");
    std::fs::create_dir_all(&inside)?;
    let wrapper = terminal_wrapper(
        &places,
        Some(&format!("(cd {} && exec sleep 601)", inside.display())),
        None,
    )?;
    let mut hp = alone_with(config_with_apps(&places))?;
    hp.restore_session();
    let mut started = std::process::Command::new("sh")
        .arg(&wrapper)
        .current_dir(&folder)
        .env("WAYLAND_DISPLAY", hp.socket())
        .spawn()?;
    window_of(&mut hp, "foot")?;
    let file = places.root.join("state/hocuspanda/session");
    let wanted = format!(
        "[window.terminal]\nfolder = {:?}",
        inside.display().to_string()
    );
    // Its program takes a moment to be in its shell's foreground.
    let mut saved = String::new();
    for _ in 0..50 {
        hp.end_session();
        saved = std::fs::read_to_string(&file).unwrap_or_default();
        if saved.contains(&wanted) {
            break;
        }
        wait(&mut hp, Duration::from_millis(100))?;
    }
    assert!(saved.contains(&wanted), "{saved}");
    drop(hp);
    let _ = started.wait();

    let mut hp = alone_with(config_with_apps(&places))?;
    hp.restore_session();
    let bar = common::client(hp.socket(), Some(common::Shows::ClearBar))?;
    std::mem::forget(bar);
    let back = hp.dispatch_until(WAIT, |hp| !hp.session_restoring())?;
    anyhow::ensure!(back, "not back: {:?}", hp.all_windows());
    let marks = terminal_marks(&places);
    assert_eq!(
        marks.last(),
        Some(&(
            format!("--working-directory={}", inside.display()),
            inside.clone()
        )),
        "{marks:?}"
    );
    Ok(())
}

// --- The screens linked (#947) -------------------------------------------------------------
//
// Two screens switching their desktops together (the default): a project
// is desktop N on every screen.

const OTHER: &str = "HEADLESS-2";
const ON_FIRST: (f64, f64) = (600.0, 500.0);
const ON_OTHER: (f64, f64) = (1920.0 + 600.0, 500.0);

/// The compositor alone on two screens, `config` its configuration.
fn two_with(config: Config) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?, "1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.pointer_move(ON_FIRST.0, ON_FIRST.1);
    Ok(hp)
}

/// `app_id` opened on the desktop the screen under `at` shows; its client.
fn open_at(
    hp: &mut Headless,
    at: (f64, f64),
    app_id: &'static str,
) -> anyhow::Result<common::ClientHandle> {
    hp.pointer_move(at.0, at.1);
    let (_, client) = open_with(hp, app_id, (600, 400), false)?;
    Ok(client)
}

/// The screen a window is on, its desktop (from 0), whether it shows.
fn placed(hp: &Headless, app_id: &str) -> Option<(String, usize, bool)> {
    hp.placements()
        .into_iter()
        .find(|p| p.app_id.as_deref() == Some(app_id))
        .and_then(|p| Some((p.output?, p.desktop, p.shown)))
}

/// The desktop each screen shows (from 0).
fn shown_on(hp: &Headless) -> Vec<usize> {
    hp.strips().into_iter().map(|(_, _, c)| c).collect()
}

/// #947, scenarios 21 and 22: desktop 2 made the project "hocus-site" is
/// desktop 2 of both screens: named so on both, its windows both
/// screens'; closing closes them all and saves each with its screen;
/// reopened, each comes back on its own screen's desktop 2, both screens
/// showing it.
#[test]
fn linked_a_project_spans_every_screen_closes_and_reopens_on_each() -> anyhow::Result<()> {
    let places = places()?;
    app_entry(&places, "test.laptop", "Laptop", false)?;
    app_entry(&places, "test.wide", "Wide", false)?;
    let mut hp = two_with(config_with_apps(&places))?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 2, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    let laptop = open_at(&mut hp, ON_FIRST, "test.laptop")?;
    let wide = open_at(&mut hp, ON_OTHER, "test.wide")?;
    let outputs: Vec<smithay::output::Output> = hp.state().space.outputs().cloned().collect();
    for output in &outputs {
        assert_eq!(
            hp.state().desktop_name(output, 1).as_deref(),
            Some("hocus-site")
        );
    }
    assert_eq!(hp.project_windows("hocus-site").len(), 2);

    hp.project_close("hocus-site")
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    let mut asked = [false, false];
    let both = hp.dispatch_until(WAIT, |_| {
        for (i, client) in [&laptop, &wide].into_iter().enumerate() {
            asked[i] |= client
                .events
                .try_iter()
                .any(|e| e == common::Event::CloseRequested);
        }
        asked == [true, true]
    })?;
    anyhow::ensure!(both, "asked to close: {asked:?}");
    laptop.commands.send(common::Command::Close)?;
    wide.commands.send(common::Command::Close)?;
    closed(&mut hp, "hocus-site")?;
    let file = places
        .root
        .join("state/hocuspanda/projects.d/hocus-site.toml");
    let written = hp.dispatch_until(WAIT, |_| file.exists())?;
    anyhow::ensure!(written, "no contents' file");
    let text = std::fs::read_to_string(&file)?;
    for (app, screen) in [("test.laptop", SCREEN), ("test.wide", OTHER)] {
        let key = hp.screen_key(screen);
        let saved = text
            .split("[[window]]")
            .find(|w| w.contains(&format!("app-id = \"{app}\"")))
            .with_context(|| format!("{app} not saved: {text}"))?;
        assert!(
            saved.contains(&format!("screen = \"{key}\"")),
            "{app}: {saved}"
        );
    }

    // Reopened from the first screen, desktop 2 shown and empty on both:
    // each window on its own screen's desktop 2.
    let desktop = hp
        .project_open("hocus-site", SCREEN)
        .map_err(|why| anyhow::anyhow!("{why}"))?;
    assert_eq!(desktop, 1);
    let back = hp.dispatch_until(WAIT, |hp| {
        !hp.project_coming_back("hocus-site")
            && placed(hp, "test.laptop").is_some()
            && placed(hp, "test.wide").is_some()
    })?;
    anyhow::ensure!(back, "not back: {:?}", hp.placements());
    assert_eq!(
        placed(&hp, "test.laptop"),
        Some((SCREEN.to_owned(), 1, true))
    );
    assert_eq!(placed(&hp, "test.wide"), Some((OTHER.to_owned(), 1, true)));
    assert_eq!(shown_on(&hp), [1, 1]);
    Ok(())
}

/// #947, scenario 23: `Super+2` brings the project: its pill shows under
/// the dots on both screens, and its monogram on tile 2 of both rows.
#[test]
fn linked_a_projects_pill_and_monogram_show_on_every_screen() -> anyhow::Result<()> {
    let places = places()?;
    let mut hp = two_with(config(&places))?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 2, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    hp.hold_clock();
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    hp.step(Duration::from_millis(100))?;
    let looks = hp.strip_looks();
    assert!(
        looks
            .iter()
            .all(|s| s.dots > 0.0 && s.project.as_deref() == Some("hocus-site")),
        "{looks:?}"
    );
    for screen in [SCREEN, OTHER] {
        let view = hp.desktops_view(screen).context("no desktops")?;
        assert_eq!(view.projects.len(), 1, "{screen}: {:?}", view.projects);
        assert_eq!(view.projects[0].desktop, 1);
        assert_eq!(view.projects[0].name, "hocus-site");
    }
    Ok(())
}

/// #947, scenario 24: the project on desktop 2 with one window on the
/// first screen and two on the other; unlinked, it is the other screen's
/// desktop 2, the first screen's window still open there, out of it.
#[test]
fn unlinked_a_project_stays_on_the_screen_holding_most_of_its_windows() -> anyhow::Result<()> {
    let places = places()?;
    let mut hp = two_with(config(&places))?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 2, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    let _first = open_at(&mut hp, ON_FIRST, "test.first")?;
    let _one = open_at(&mut hp, ON_OTHER, "test.one")?;
    let _two = open_at(&mut hp, ON_OTHER, "test.two")?;
    assert_eq!(hp.project_windows("hocus-site").len(), 3);
    let mut config = config(&places);
    config.desktops.switching = hocus_compositor::DesktopSwitching::PerScreen;
    hp.reconfigure(config);
    let there = hp.projects();
    assert_eq!(there[0].2, Some((OTHER.to_owned(), 1)), "{there:?}");
    let mut windows = hp.project_windows("hocus-site");
    windows.sort_unstable_by_key(|w| w.0);
    let mut theirs = vec![id(&hp, "test.one")?, id(&hp, "test.two")?];
    theirs.sort_unstable_by_key(|w| w.0);
    assert_eq!(windows, theirs);
    assert_eq!(
        placed(&hp, "test.first"),
        Some((SCREEN.to_owned(), 1, true))
    );
    assert!(hp.state().project_of(id(&hp, "test.first")?).is_none());
    Ok(())
}

/// The compositor with the real shell on the two screens, in French, the
/// bar's full form on the first (a row of tiles per screen), its log in
/// `shell.log`.
fn two_with_shell(places: &Places) -> anyhow::Result<(Headless, PathBuf)> {
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
    let mut config = config(places);
    config.panel.form = hocus_compositor::FormChoice::Full;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?, "1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.desk_tile_rects().len() == 8
    })?;
    anyhow::ensure!(up, "tiles: {:?}", hp.desk_tile_rects());
    hp.pointer_move(ON_FIRST.0, ON_FIRST.1);
    Ok((hp, log))
}

/// The pointer resting on `output`'s tile `desktop` (from 0) in the first
/// screen's bar, once the tiles take it there.
fn rest_on_tile(hp: &mut Headless, output: &str, desktop: usize) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    loop {
        let at = hp
            .desk_tile_rects()
            .into_iter()
            .find(|(bar, o, d, _)| bar == SCREEN && o == output && *d == desktop)
            .map(|(.., (x, y, w, h))| (x + w / 2.0, y + h / 2.0));
        if let Some(at) = at {
            hp.pointer_move(at.0 - 1.0, at.1);
            hp.pointer_move(at.0, at.1);
            wait(hp, Duration::from_millis(30))?;
            if hp.pointer_layer().as_deref() == Some(hocus_link::DESKTOPS_NAMESPACE)
                && hp.undrawn_at(at.0, at.1).is_empty()
            {
                return Ok(());
            }
        } else {
            wait(hp, Duration::from_millis(30))?;
        }
        anyhow::ensure!(
            start.elapsed() < WAIT,
            "the tiles never took the pointer over tile {desktop} of {output}"
        );
    }
}

/// #947: the project on desktop 2 of both screens, one window on the
/// first and two on the other. Its list, from the other screen's tile,
/// lists them all grouped by screen, the bar's own first, each under its
/// screen's name and no line saying which screen; its card, from the
/// other screen's thumbnail, groups them too, its own screen first, each
/// window told with its screen.
#[test]
fn linked_a_projects_list_and_card_group_its_windows_by_screen() -> anyhow::Result<()> {
    const KEY_W: u32 = 17;
    let places = places()?;
    let (mut hp, log) = two_with_shell(&places)?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 2, "hocus-site", &places.folder)?
        .map_err(|why| anyhow::anyhow!("refused: {why}"))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    let mut clients = Vec::new();
    for (at, app, title) in [
        (ON_FIRST, "test.first", "Terminal"),
        (ON_OTHER, "test.one", "Navigateur"),
        (ON_OTHER, "test.two", "Éditeur"),
    ] {
        let client = open_at(&mut hp, at, app)?;
        client.commands.send(common::Command::Title(title))?;
        clients.push(client);
    }
    wait(&mut hp, Duration::from_millis(300))?;

    // The list, resting on the other screen's tile 2.
    rest_on_tile(&mut hp, OTHER, 1)?;
    logged(
        &mut hp,
        &log,
        "count=3 fenêtres empty=None rows=[\"Terminal\", \"Éditeur\", \"Navigateur\"] \
         minimized=0 screen=None screens=[(0, \"ÉCRAN 1\"), (1, \"ÉCRAN 2\")]",
    )?;
    hp.pointer_move(ON_FIRST.0, ON_FIRST.1);
    let closed = hp.dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the list stays");

    // The card, from the other screen's thumbnail in the overview.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    let open = hp.dispatch_until(WAIT, |hp| hp.overview_project_name(OTHER, 1).is_some())?;
    anyhow::ensure!(open, "no project's name in the overview");
    wait(&mut hp, Duration::from_millis(400))?;
    let name = hp.overview_project_name(OTHER, 1).context("no name")?;
    hp.pointer_move(name.0, name.1);
    wait(&mut hp, Duration::from_millis(100))?;
    hp.button(common::BTN_LEFT, true);
    hp.button(common::BTN_LEFT, false);
    logged(
        &mut hp,
        &log,
        "screens=[(0, \"ÉCRAN 2\"), (2, \"ÉCRAN 1\")]",
    )?;
    let said = common::logged(&log)
        .lines()
        .rfind(|l| l.contains("the project's card shows it"))
        .context("no card")?
        .to_owned();
    // The other screen's two, then the first screen's.
    let at = |t: &str| said.find(t).with_context(|| format!("no {t}: {said}"));
    let terminal = at("Terminal (")?;
    assert!(
        at("Éditeur (")? < terminal && at("Navigateur (")? < terminal,
        "{said}"
    );
    drop(clients);
    Ok(())
}
