//! The desktops' count and names followed live (6.14, issue #448;
//! docs/design/windows.md §12): `[desktops] count` raised or lowered in
//! the file on every screen, the last ones' windows going to the last that
//! stays; a desktop taken out by the Settings' line wherever it is, its
//! windows going to the one before it (the first's to the next one), and
//! put back with them; the file then written by the Settings moving
//! nothing twice; the names told to the shell, whose tiles say them.
//!
//! The file is written by hand, as the Settings' worker would. Each test
//! waits for what it checks, never for a time.

use crate::common;

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::mpsc,
};

use anyhow::Context;
use common::{KEY_1, KEY_2, KEY_3, KEY_LEFTMETA, KEY_LEFTSHIFT, WAIT, chord, open};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::WindowId};

const KEY_4: u32 = 5;
const KEY_6: u32 = 7;

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    file: PathBuf,
}

/// The compositor on two screens, its file saying `desktops` (the
/// `[desktops]` section's lines).
fn start(desktops: &str) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    let text = format!("[desktops]\n{desktops}");
    std::fs::write(&file, &text)?;
    let mut config: Config = toml::from_str(&text)?;
    config.file = Some(file.clone());
    let hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?, "1280x720".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    Ok(Bed {
        _dir: dir,
        hp,
        file,
    })
}

impl Bed {
    /// The file now says `desktops`.
    fn write(&self, desktops: &str) -> anyhow::Result<()> {
        // As the Settings write it: a new file renamed over the old.
        let new = self.file.with_extension("new");
        std::fs::write(&new, format!("[desktops]\n{desktops}"))?;
        std::fs::rename(&new, &self.file)?;
        Ok(())
    }

    /// Runs the compositor until every screen has `count` desktops.
    fn counts(&mut self, count: usize) -> anyhow::Result<()> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.strips().iter().all(|(_, n, _)| *n == count))?;
        anyhow::ensure!(done, "the screens' desktops: {:?}", self.hp.strips());
        Ok(())
    }

    /// `app_id`'s desktop (from 0) and whether it shows.
    fn on(&self, app_id: &str) -> Option<(usize, bool)> {
        self.hp
            .all_windows()
            .into_iter()
            .find(|w| w.1.as_deref() == Some(app_id))
            .map(|w| (w.2, w.3))
    }

    fn id(&self, app_id: &str) -> anyhow::Result<WindowId> {
        self.hp
            .all_windows()
            .into_iter()
            .find(|w| w.1.as_deref() == Some(app_id))
            .map(|w| w.0)
            .with_context(|| format!("no {app_id}"))
    }

    /// The focused window's app id.
    fn focused(&self) -> Option<String> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.focused && w.keyboard)
            .and_then(|w| w.app_id)
    }

    /// The first screen's current desktop (from 0).
    fn shows(&self) -> usize {
        self.hp.strips().first().map_or(0, |s| s.2)
    }

    /// A window `app_id` opened on the first screen's desktop `desktop`
    /// (from 0): opened on the first, then sent there.
    fn open_on(&mut self, app_id: &'static str, desktop: usize) -> anyhow::Result<()> {
        self.hp.pointer_move(400.0, 400.0);
        open(&mut self.hp, app_id, (400, 300))?;
        let key = [KEY_1, KEY_2, KEY_3, KEY_4][desktop];
        if desktop > 0 {
            chord(&mut self.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, key]);
        }
        anyhow::ensure!(
            self.on(app_id).map(|o| o.0) == Some(desktop),
            "{app_id} on {:?}",
            self.on(app_id)
        );
        Ok(())
    }
}

/// The Settings' line (`msg screens`): what the compositor says on it, as
/// JSON, and what is asked on it.
struct Line {
    stream: UnixStream,
    heard: mpsc::Receiver<serde_json::Value>,
    /// The desktops' windows as last said.
    desktops: std::cell::RefCell<Option<serde_json::Value>>,
}

impl Line {
    fn open(hp: &Headless) -> anyhow::Result<Self> {
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
        Ok(Self {
            stream,
            heard,
            desktops: std::cell::RefCell::new(None),
        })
    }

    fn ask(&mut self, line: &str) -> anyhow::Result<()> {
        self.stream.write_all(format!("{line}\n").as_bytes())?;
        Ok(())
    }

    /// Runs the compositor until the line says something under `key` (or
    /// refuses what was asked): the last such thing said by then.
    fn heard(&self, hp: &mut Headless, key: &str) -> anyhow::Result<serde_json::Value> {
        let mut found = None;
        let mut refused = None;
        hp.dispatch_until(WAIT, |_| {
            while let Ok(value) = self.heard.try_recv() {
                if let Some(v) = value.get("desktops") {
                    *self.desktops.borrow_mut() = Some(v.clone());
                }
                if let Some(v) = value.get(key) {
                    found = Some(v.clone());
                }
                if key != "refused" && value.get("refused").is_some() {
                    refused = Some(value);
                }
            }
            found.is_some() || refused.is_some()
        })?;
        if let Some(refused) = refused {
            anyhow::bail!("refused: {refused}");
        }
        found.with_context(|| format!("nothing said of {key}"))
    }

    /// The desktops' windows as said, once they are `windows`.
    fn says(&self, hp: &mut Headless, windows: &[u64]) -> anyhow::Result<()> {
        let done = hp.dispatch_until(WAIT, |_| {
            let mut last = self.desktops.borrow_mut();
            while let Ok(value) = self.heard.try_recv() {
                if let Some(v) = value.get("desktops") {
                    *last = Some(v.clone());
                }
            }
            last.as_ref().is_some_and(|v| {
                v["count"] == windows.len() && v["windows"] == serde_json::json!(windows)
            })
        })?;
        let last = self.desktops.borrow();
        anyhow::ensure!(done, "the line said {last:?}, not {windows:?}");
        Ok(())
    }
}

#[test]
fn more_desktops_in_the_file_are_added_empty_on_every_screen() -> anyhow::Result<()> {
    let mut bed = start("count = 4\n")?;
    bed.open_on("test.kept", 1)?;
    bed.write("count = 6\n")?;
    bed.counts(6)?;
    assert_eq!(bed.on("test.kept"), Some((1, false)), "nothing moved");
    // The sixth is there to go to, empty.
    bed.hp.pointer_move(400.0, 400.0);
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_6]);
    assert_eq!(bed.shows(), 5);
    assert!(bed.hp.windows().is_empty());
    Ok(())
}

#[test]
fn fewer_desktops_in_the_file_give_the_last_ones_windows_to_the_last_that_stays()
-> anyhow::Result<()> {
    let mut bed = start("count = 4\n")?;
    bed.open_on("test.first", 0)?;
    bed.open_on("test.third", 2)?;
    bed.open_on("test.fourth", 3)?;
    // The first screen shows the fourth desktop.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_4]);
    assert_eq!(bed.focused().as_deref(), Some("test.fourth"));
    bed.write("count = 2\n")?;
    bed.counts(2)?;
    // Taken out on every screen; the screen shows the last that stays,
    // where the windows went, at once.
    assert_eq!(bed.shows(), 1);
    assert!(
        bed.hp.strips().iter().all(|s| s.2 < 2),
        "{:?}",
        bed.hp.strips()
    );
    assert_eq!(bed.on("test.third"), Some((1, true)));
    assert_eq!(bed.on("test.fourth"), Some((1, true)));
    assert_eq!(bed.on("test.first"), Some((0, false)));
    assert_eq!(bed.focused().as_deref(), Some("test.fourth"));
    assert_eq!(bed.hp.desktop(), (1, 1.0), "nothing slides");
    Ok(())
}

#[test]
fn the_settings_take_a_desktop_out_its_windows_to_the_one_before() -> anyhow::Result<()> {
    let names = "names = [\"Travail\", \"Courrier\", \"Musique\"]\n";
    let mut bed = start(&format!("count = 4\n{names}"))?;
    bed.open_on("test.work", 0)?;
    bed.open_on("test.mail", 1)?;
    bed.open_on("test.mail2", 1)?;
    bed.open_on("test.music", 2)?;
    let mail = [bed.id("test.mail")?.0, bed.id("test.mail2")?.0];
    let mut line = Line::open(&bed.hp)?;
    line.says(&mut bed.hp, &[1, 2, 1, 0])?;
    // "Courrier" taken out: its two windows to "Travail", "Musique" one
    // down, its name with it.
    line.ask("desktop-remove 2")?;
    let removed = line.heard(&mut bed.hp, "desktop-removed")?;
    assert_eq!(removed["desktop"], 2);
    assert_eq!(removed["to"], 1);
    let mut moved: Vec<u64> = serde_json::from_value(removed["windows"].clone())?;
    moved.sort_unstable();
    assert_eq!(moved, mail);
    line.says(&mut bed.hp, &[3, 1, 0])?;
    assert!(bed.hp.strips().iter().all(|s| s.1 == 3));
    assert_eq!(bed.on("test.mail").map(|o| o.0), Some(0));
    assert_eq!(bed.on("test.music").map(|o| o.0), Some(1));
    assert_eq!(bed.hp.desktop_names(), ["Travail", "Musique"]);
    // Then the Settings write the file: what it says is already so.
    bed.write("count = 3\nnames = [\"Travail\", \"Musique\", \"Radio\"]\n")?;
    let read = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.desktop_names().len() == 3)?;
    anyhow::ensure!(read, "the file is not read: {:?}", bed.hp.desktop_names());
    assert!(bed.hp.strips().iter().all(|s| s.1 == 3));
    assert_eq!(bed.on("test.music").map(|o| o.0), Some(1), "moved once");
    assert_eq!(bed.on("test.mail").map(|o| o.0), Some(0));
    Ok(())
}

#[test]
fn the_first_desktop_taken_out_goes_to_the_next_and_comes_back_with_its_windows()
-> anyhow::Result<()> {
    let mut bed = start("count = 3\n")?;
    bed.open_on("test.first", 0)?;
    bed.open_on("test.second", 1)?;
    assert_eq!(bed.shows(), 0);
    let first = bed.id("test.first")?.0;
    let mut line = Line::open(&bed.hp)?;
    line.says(&mut bed.hp, &[1, 1, 0])?;
    line.ask("desktop-remove 1")?;
    let removed = line.heard(&mut bed.hp, "desktop-removed")?;
    assert_eq!(removed["to"], 1, "the next one, now first");
    line.says(&mut bed.hp, &[2, 0])?;
    // The screen showed it: it shows where its window went, focused.
    assert_eq!(bed.shows(), 0);
    assert_eq!(bed.on("test.first"), Some((0, true)));
    assert_eq!(bed.on("test.second"), Some((0, true)));
    // "Annuler": put back first, with its window.
    line.ask(&format!("desktop-restore 1 {first}"))?;
    line.heard(&mut bed.hp, "desktop-restored")?;
    line.says(&mut bed.hp, &[1, 1, 0])?;
    assert_eq!(bed.on("test.first").map(|o| o.0), Some(0));
    assert_eq!(bed.on("test.second").map(|o| o.0), Some(1));
    // The last desktop stays.
    let mut alone = start("count = 1\n")?;
    let mut line = Line::open(&alone.hp)?;
    line.says(&mut alone.hp, &[0])?;
    line.ask("desktop-remove 1")?;
    let refused = line.heard(&mut alone.hp, "refused")?;
    assert_eq!(refused, "the last desktop stays");
    alone.counts(1)?;
    Ok(())
}

/// The shell, logging what its tiles say.
fn shell_script(dir: &Path, log: &Path) -> anyhow::Result<Option<PathBuf>> {
    let shell = common::shell_program();
    let home = dir.join("home");
    std::fs::create_dir_all(&home)?;
    let script = dir.join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_STATE_HOME='{home}/.state' XDG_DATA_HOME='{home}/.local/share' \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    Ok(Some(script))
}

#[test]
fn the_bars_tiles_say_the_desktops_names_as_the_file_changes() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let log = dir.path().join("shell.log");
    let Some(script) = shell_script(dir.path(), &log)? else {
        return Ok(());
    };
    let file = dir.path().join("config.toml");
    let text = "[desktops]\ncount = 3\nnames = [\"Travail\"]\n";
    std::fs::write(&file, text)?;
    let mut config: Config = toml::from_str(text)?;
    config.file = Some(file.clone());
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let said = |hp: &mut Headless, what: &str| -> anyhow::Result<()> {
        let found = hp.dispatch_until(WAIT, |_| common::logged(&log).contains(what))?;
        anyhow::ensure!(found, "the shell did not say {what:?}");
        Ok(())
    };
    said(
        &mut hp,
        "said=Travail, aucune fenêtre · Bureau 2, aucune fenêtre · Bureau 3, aucune fenêtre",
    )?;
    // Renamed in the file: said again, without a window changing.
    let new = file.with_extension("new");
    std::fs::write(
        &new,
        "[desktops]\ncount = 3\nnames = [\"Projet\", \"\", \"Courrier\"]\n",
    )?;
    std::fs::rename(&new, &file)?;
    said(
        &mut hp,
        "said=Projet, aucune fenêtre · Bureau 2, aucune fenêtre · Courrier, aucune fenêtre",
    )?;
    // Resting on the third tile: its list's title is its name. Four tiles
    // 38.5 px wide to a row, 6 px apart, 11 px in, 5 px down (bar.md).
    let surface = hp
        .desktops_surfaces()
        .first()
        .map(|(_, r)| *r)
        .context("no tiles")?;
    let (x, y) = (
        f64::from(surface.x) + 11.0 + 44.5 * 2.0 + 19.0,
        f64::from(surface.y) + 20.0,
    );
    hp.pointer_move(x - 1.0, y);
    hp.pointer_move(x, y);
    said(
        &mut hp,
        "title=Courrier count= empty=Some(\"Aucune fenêtre.\")",
    )?;
    Ok(())
}
