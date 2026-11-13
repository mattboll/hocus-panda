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
