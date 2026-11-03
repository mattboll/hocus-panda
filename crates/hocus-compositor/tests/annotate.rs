//! What comes after a capture (5.9, issue #399, docs/design/screenshots.md):
//! its thumbnail, its editor, its place in the notifications' history. One
//! scenario per point of the approved spec, with the real shell on a
//! D-Bus daemon of the test's own (the history is served there, never the
//! session's), its pictures', state and home folders the test's.
//!
//! Pixman here, so the shell draws on the CPU, as in CI. The capture is an
//! area over a window of black and white squares of 3 pixels, as fine as
//! small text; the editor shows it a pixel a pixel, so that the marks
//! drawn on the screen land on the image's pixels under them.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::Context;
use common::{BTN_LEFT, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTSHIFT, Shows, chord, open_showing};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_PRINT: u32 = 99;
const KEY_ENTER: u32 = 28;
const KEY_O: u32 = 24;
const KEY_K: u32 = 37;
const KEY_Z: u32 = 44;
/// The full bar's left edge, 8 px from the screen's right one.
const BAR_LEFT: i32 = 1920 - 8 - 196;
/// The area captured, logical pixels of the screen: over the checkered
/// window.
const AREA: (f64, f64, f64, f64) = (400.0, 200.0, 600.0, 380.0);

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    /// The test's own D-Bus daemon, stopped by its process id.
    bus: Child,
    _window: common::ClientHandle,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

/// A D-Bus daemon of the test's own in `dir`, and its address.
fn private_bus(dir: &Path) -> anyhow::Result<(Child, String)> {
    let socket = dir.join("bus");
    let config = dir.join("bus.conf");
    std::fs::write(
        &config,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig><type>session</type><listen>unix:path={}</listen>\
             <policy context=\"default\"><allow send_destination=\"*\"/>\
             <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
            socket.display()
        ),
    )?;
    let bus = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .arg("--nofork")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    while !socket.exists() {
        anyhow::ensure!(start.elapsed() < Duration::from_secs(10), "no bus");
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok((bus, format!("unix:path={}", socket.display())))
}

fn ready() -> Option<PathBuf> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    let daemon = Command::new("sh")
        .args(["-c", "command -v dbus-daemon"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !daemon {
        eprintln!("no dbus-daemon: skipped");
        return None;
    }
    Some(shell)
}

impl Bed {
    /// The compositor on a 1920×1080 screen (the full bar), the real shell
    /// in French on the test's bus, a checkered window under the area.
    fn start(shell: &Path) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "state", "config", "cache"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let (bus, address) = private_bus(root)?;
        let log = root.join("shell.log");
        let script = root.join("shell");
        let p = |s: &str| root.join(s).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' TZ=UTC \
                 RUST_LOG=warn,hocuspanda_shell=debug\nexec '{}' \"$@\" 2>>'{}'\n",
                p("home"),
                p("config"),
                p("cache"),
                p("state"),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.plain_background = true;
        config.pictures = Some(root.join("Images"));
        config.state_dir = Some(root.join("state/hocuspanda"));
        config.french = true;
        let mut hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            config,
            Some(script),
            Some(address),
        )?;
        let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        let (_, window) = open_showing(
            &mut hp,
            "squares",
            Shows::Checkered {
                app_id: "squares",
                size: (1300, 900),
            },
        )?;
        let mut bed = Self {
            hp,
            dir,
            log,
            bus,
            _window: window,
        };
        bed.idle(Duration::from_millis(300))?;
        Ok(bed)
    }

    fn idle(&mut self, time: Duration) -> anyhow::Result<()> {
        let _ = self.hp.dispatch_until(time, |_| false)?;
        Ok(())
    }

    /// What the shell logged, its colours taken out.
    fn log(&self) -> String {
        let text =
            String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn mark(&self) -> usize {
        self.log().len()
    }

    /// The first line logged since `mark` saying `what`, waited for.
    fn logged_since(&mut self, mark: usize, what: &str) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            let log = self.log();
            if let Some(line) = log
                .get(mark..)
                .and_then(|l| l.lines().find(|l| l.contains(what)))
            {
                return Ok(line.to_owned());
            }
            self.idle(Duration::from_millis(30))?;
        }
        anyhow::bail!("the shell never logged {what:?}")
    }

    fn click(&mut self, (x, y): (f64, f64)) -> anyhow::Result<()> {
        self.hp.pointer_move(x - 1.0, y);
        self.hp.pointer_move(x, y);
        self.idle(Duration::from_millis(60))?;
        self.hp.button(BTN_LEFT, true);
        self.idle(Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, false);
        self.idle(Duration::from_millis(80))
    }

    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) -> anyhow::Result<()> {
        self.hp.pointer_move(from.0, from.1);
        self.idle(Duration::from_millis(40))?;
        self.hp.button(BTN_LEFT, true);
        self.idle(Duration::from_millis(30))?;
        for i in 1..=5 {
            let t = f64::from(i) / 5.0;
            self.hp
                .pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            self.idle(Duration::from_millis(20))?;
        }
        self.hp.button(BTN_LEFT, false);
        self.idle(Duration::from_millis(80))
    }

    fn key(&mut self, keys: &[u32]) -> anyhow::Result<()> {
        chord(&mut self.hp, keys);
        self.idle(Duration::from_millis(60))
    }

    /// `Print`, the area drawn over the window, `Enter`: the capture's file
    /// and its bytes, once its thumbnail shows.
    fn capture(&mut self) -> anyhow::Result<(PathBuf, Vec<u8>, Rect)> {
        let before = self.hp.captures().0;
        self.hp.pointer_move(300.0, 150.0);
        chord(&mut self.hp, &[KEY_PRINT]);
        self.idle(Duration::from_millis(100))?;
        let (x, y, w, h) = AREA;
        self.drag((x, y), (x + w, y + h))?;
        chord(&mut self.hp, &[KEY_ENTER]);
        let taken = self
            .hp
            .dispatch_until(Duration::from_secs(10), |hp| hp.captures().0 > before)?;
        anyhow::ensure!(taken, "nothing captured");
        let file = self.hp.captures().1.context("no file")?;
        let shown = self.hp.dispatch_until(Duration::from_secs(10), |hp| {
            hp.capture_thumb_surfaces()
                .first()
                .is_some_and(|(_, r)| r.height > 0)
        })?;
        anyhow::ensure!(shown, "no thumbnail");
        self.idle(Duration::from_millis(300))?;
        let thumb = self.hp.capture_thumb_surfaces()[0].1;
        Ok((file.clone(), std::fs::read(&file)?, thumb))
    }

    /// A click on the thumbnail's image: the editor, and where its pieces
    /// are, as it logs them.
    fn edit(&mut self, thumb: Rect) -> anyhow::Result<Places> {
        let mark = self.mark();
        self.click((
            f64::from(thumb.x + thumb.width / 2),
            f64::from(thumb.y + 100),
        ))?;
        self.editor(mark)
    }

    /// The editor once it shows, laid out after `mark`.
    fn editor(&mut self, mark: usize) -> anyhow::Result<Places> {
        let line = self.logged_since(mark, "the capture's editor is laid out")?;
        let open = self.hp.dispatch_until(Duration::from_secs(5), |hp| {
            hp.capture_editor_surfaces().len() == 1
        })?;
        anyhow::ensure!(open, "no editor");
        self.idle(Duration::from_millis(200))?;
        Places::read(&line)
    }

    /// The thumbnail's ✕.
    fn close_spot(thumb: Rect) -> (f64, f64) {
        // 8 in from its right and bottom, 26 square, zoomed.
        let side = 26.0 * 1.5;
        let pad = 8.0 * 1.5;
        (
            f64::from(thumb.x + thumb.width) - pad - side / 2.0,
            f64::from(thumb.y + thumb.height) - pad - side / 2.0,
        )
    }

    /// Opens the history with a click on the bell: what it shows.
    fn open_history(&mut self) -> anyhow::Result<String> {
        let shown = self
            .hp
            .dispatch_until(Duration::from_secs(10), |hp| hp.bell_surfaces().len() == 1)?;
        anyhow::ensure!(shown, "no bell");
        let (_, at) = self.hp.bell_surfaces()[0];
        let bell = (f64::from(at.x + 11 + 44 + 20), f64::from(at.y + 6 + 17));
        let mark = self.mark();
        self.click(bell)?;
        let shows = self.logged_since(mark, "the history shows")?;
        Ok(shows)
    }
}

/// Where the editor's pieces are, as it logged them.
struct Places(Vec<(String, Vec<f64>)>);

impl Places {
    fn read(line: &str) -> anyhow::Result<Self> {
        let places = line
            .split_whitespace()
            .filter_map(|word| {
                let word = word.strip_prefix("places=").unwrap_or(word);
                let (name, numbers) = word.split_once('@')?;
                let numbers = numbers
                    .split(',')
                    .map(|n| n.parse::<f64>())
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
                Some((name.to_owned(), numbers))
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(!places.is_empty(), "no places in {line}");
        Ok(Self(places))
    }

    fn at(&self, name: &str) -> (f64, f64) {
        let n = &self
            .0
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1;
        (n[0], n[1])
    }

    /// Point (`x`, `y`) of the image on the screen: shown a pixel a pixel.
    fn image(&self, x: f64, y: f64) -> (f64, f64) {
        let n = &self
            .0
            .iter()
            .find(|(n, _)| n == "image")
            .expect("an image")
            .1;
        assert_eq!(
            (n[2], n[3]),
            (AREA.2, AREA.3),
            "the image is shown at its size"
        );
        (n[0] + x, n[1] + y)
    }
}

fn png(bytes: &[u8]) -> anyhow::Result<image::RgbaImage> {
    Ok(image::load_from_memory(bytes)?.to_rgba8())
}

/// The variance of the pixels' brightness in a rectangle.
fn variance(image: &image::RgbaImage, (x, y, w, h): (u32, u32, u32, u32)) -> f64 {
    let mut values = Vec::new();
    for py in y..y + h {
        for px in x..x + w {
            let p = image.get_pixel(px, py).0;
            values.push((f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2])) / 3.0);
        }
    }
    let n = values.len().max(1) as f64;
    let mean = values.iter().sum::<f64>() / n;
    values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n
}

fn near(p: [u8; 4], colour: [u8; 3]) -> bool {
    p[..3].iter().zip(colour).all(|(a, b)| a.abs_diff(b) <= 6)
}

const RED: [u8; 3] = [0xF2, 0x54, 0x5B];
const GREEN: [u8; 3] = [0x6C, 0xC2, 0x87];

/// The thumbnail shows at the bottom right of the captured screen,
/// against the bar, 8 px from it and from the bottom, the mockup's 220
/// wide × 1.5.
#[test]
fn the_thumbnail_shows_beside_the_bar() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let mark = bed.mark();
    let (file, _, thumb) = bed.capture()?;
    assert_eq!(thumb.x + thumb.width, BAR_LEFT - 8, "{thumb:?}");
    assert_eq!(thumb.y + thumb.height, 1080 - 8, "{thumb:?}");
    assert_eq!((thumb.width, thumb.height), (330, 267));
    let line = bed.logged_since(mark, "the capture's thumbnail shows")?;
    assert!(line.contains(&file.display().to_string()), "{line}");
    // In the French folder, under its French name; no toast in its place.
    assert!(file.starts_with(bed.dir.path().join("Images/Captures d'écran")));
    assert!(bed.hp.launcher_toast_surfaces().is_empty());
    Ok(())
}

/// The pointer on it holds it past its 6 s; away, it goes after what was
/// left.
#[test]
fn the_pointer_on_the_thumbnail_holds_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let (_, _, thumb) = bed.capture()?;
    bed.hp
        .pointer_move(f64::from(thumb.x + 40), f64::from(thumb.y + 240));
    bed.hp
        .pointer_move(f64::from(thumb.x + 41), f64::from(thumb.y + 240));
    bed.idle(Duration::from_millis(7000))?;
    assert_eq!(
        bed.hp.capture_thumb_surfaces().len(),
        1,
        "it went while held"
    );
    bed.hp.pointer_move(300.0, 300.0);
    let start = Instant::now();
    let gone = bed.hp.dispatch_until(Duration::from_secs(10), |hp| {
        hp.capture_thumb_surfaces().is_empty()
    })?;
    assert!(gone, "it stayed");
    // What was left of it: about 6 s less the moment it took to hold it.
    assert!(
        start.elapsed() > Duration::from_secs(4),
        "{:?}",
        start.elapsed()
    );
    Ok(())
}

/// Its time up, it goes, and the capture is in the history with its
/// image and its actions: "Annoter" opens the editor from there.
#[test]
fn after_6_s_the_capture_goes_to_the_history() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let mark = bed.mark();
    let (file, _, _) = bed.capture()?;
    bed.hp.pointer_move(300.0, 300.0);
    let gone = bed.hp.dispatch_until(Duration::from_secs(9), |hp| {
        hp.capture_thumb_surfaces().is_empty()
    })?;
    assert!(gone, "the thumbnail stayed");
    bed.logged_since(mark, "the capture goes to the history")?;
    let shows = bed.open_history()?;
    let name = file
        .file_name()
        .context("a name")?
        .to_string_lossy()
        .into_owned();
    assert!(
        shows.contains(&format!("Capture d'écran «{name}»")) && shows.contains("[3 actions]"),
        "{shows}"
    );
    // Its first action, "Annoter".
    let items = bed.logged_since(mark, "the history's items")?;
    let popover = bed.hp.popover_surfaces()[0].1;
    let at = items
        .split_whitespace()
        .find_map(|w| {
            let w = w.strip_prefix("items=").unwrap_or(w);
            let (who, xy) = w.rsplit_once('@')?;
            who.starts_with("Action(").then_some(())?;
            who.ends_with(",0)").then_some(())?;
            let (x, y) = xy.split_once(',')?;
            Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
        })
        .context("no action")?;
    let mark = bed.mark();
    bed.click((f64::from(popover.x) + at.0, f64::from(popover.y) + at.1))?;
    bed.editor(mark)?;
    Ok(())
}

/// ✕ deletes the capture: its file goes, and the clipboard, which held
/// it, is empty.
#[test]
fn the_cross_deletes_the_file_and_empties_the_clipboard() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let (file, bytes, thumb) = bed.capture()?;
    assert_eq!(bed.hp.clipboard_bytes(), Some(bytes));
    bed.click(Bed::close_spot(thumb))?;
    let gone = bed.hp.dispatch_until(Duration::from_secs(5), |hp| {
        !file.exists() && hp.capture_thumb_surfaces().is_empty()
    })?;
    assert!(gone, "the file or the thumbnail stayed");
    assert_eq!(bed.hp.clipboard_bytes(), None);
    Ok(())
}

/// An arrow, a box, a text and a blur, each where it was drawn on the
/// image, in the file written by "Copier et fermer".
#[test]
fn the_marks_are_in_the_saved_image_where_they_were_drawn() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let (file, before, thumb) = bed.capture()?;
    let places = bed.edit(thumb)?;
    // An arrow in red, the colour it starts with.
    bed.drag(places.image(40.0, 60.0), places.image(260.0, 60.0))?;
    // A box in green.
    bed.click(places.at("box"))?;
    bed.click(places.at("colour2"))?;
    bed.drag(places.image(320.0, 30.0), places.image(560.0, 150.0))?;
    // A text in white.
    bed.click(places.at("text"))?;
    bed.click(places.at("colour4"))?;
    bed.click(places.image(40.0, 300.0))?;
    bed.key(&[KEY_O])?;
    bed.key(&[KEY_K])?;
    bed.key(&[KEY_ENTER])?;
    // A blur.
    bed.click(places.at("blur"))?;
    bed.drag(places.image(320.0, 200.0), places.image(560.0, 340.0))?;
    let mark = bed.mark();
    bed.click(places.at("copy"))?;
    bed.logged_since(mark, "the annotated capture is written")?;
    let after = std::fs::read(&file)?;
    assert_ne!(after, before);
    let (old, new) = (png(&before)?, png(&after)?);
    assert_eq!(new.dimensions(), old.dimensions());
    // On the arrow's line, red; under it, the squares as they were.
    assert!(
        near(new.get_pixel(150, 60).0, RED),
        "{:?}",
        new.get_pixel(150, 60)
    );
    assert_eq!(new.get_pixel(150, 90), old.get_pixel(150, 90));
    // On the box's frame, green; inside it, as it was.
    assert!(
        near(new.get_pixel(440, 32).0, GREEN),
        "{:?}",
        new.get_pixel(440, 32)
    );
    assert_eq!(new.get_pixel(440, 90), old.get_pixel(440, 90));
    // The text: what changed is near where it was clicked.
    let changed = |x0: u32, y0: u32, x1: u32, y1: u32| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| new.get_pixel(x, y) != old.get_pixel(x, y))
            .count()
    };
    assert!(changed(40, 280, 120, 320) > 100, "no text");
    assert_eq!(changed(150, 260, 300, 340), 0, "a text too long");
    // The blur: blurred.
    let region = (330, 210, 220, 120);
    assert!(variance(&new, region) < variance(&old, region) * 0.05);
    Ok(())
}

/// A blur makes what is under it unreadable in the saved image: the
/// squares' contrast all but gone, not covered by a colour.
#[test]
fn a_blur_makes_the_region_unreadable() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let (file, before, thumb) = bed.capture()?;
    let places = bed.edit(thumb)?;
    bed.click(places.at("blur"))?;
    bed.drag(places.image(100.0, 100.0), places.image(400.0, 300.0))?;
    let mark = bed.mark();
    bed.click(places.at("copy"))?;
    bed.logged_since(mark, "the annotated capture is written")?;
    let (old, new) = (png(&before)?, png(&std::fs::read(&file)?)?);
    let region = (110, 110, 280, 180);
    let (was, is) = (variance(&old, region), variance(&new, region));
    assert!(was > 5000.0, "{was}");
    assert!(is < was * 0.02, "{was} → {is}");
    // Grey, as black and white squares mixed: blurred, not hidden.
    let middle = new.get_pixel(250, 200).0;
    assert!((90..=165).contains(&middle[0]), "{middle:?}");
    // Outside it, as it was.
    assert_eq!(new.get_pixel(50, 50), old.get_pixel(50, 50));
    Ok(())
}

/// `Ctrl+Z` undoes the last mark and `Ctrl+Shift+Z` redoes it; the undo
/// button too; the saved image has what is left.
#[test]
fn undo_and_redo() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let (file, before, thumb) = bed.capture()?;
    let places = bed.edit(thumb)?;
    // Two arrows: the second undone, redone, undone by the button.
    bed.drag(places.image(40.0, 60.0), places.image(260.0, 60.0))?;
    bed.drag(places.image(40.0, 200.0), places.image(260.0, 200.0))?;
    bed.key(&[KEY_LEFTCTRL, KEY_Z])?;
    bed.key(&[KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_Z])?;
    bed.click(places.at("undo"))?;
    // A text begun, then left with `Escape`: nothing.
    bed.click(places.at("text"))?;
    bed.click(places.image(40.0, 300.0))?;
    bed.key(&[KEY_O])?;
    bed.key(&[KEY_ESC])?;
    assert_eq!(
        bed.hp.capture_editor_surfaces().len(),
        1,
        "Escape closed it"
    );
    let mark = bed.mark();
    bed.click(places.at("copy"))?;
    bed.logged_since(mark, "the annotated capture is written")?;
    let (old, new) = (png(&before)?, png(&std::fs::read(&file)?)?);
    assert!(near(new.get_pixel(150, 60).0, RED));
    assert_eq!(
        new.get_pixel(150, 200),
        old.get_pixel(150, 200),
        "undone, still there"
    );
    assert_eq!(
        new.get_pixel(60, 300),
        old.get_pixel(60, 300),
        "the text left is there"
    );
    Ok(())
}

/// "Copier et fermer" writes the annotated image over the file and on the
/// clipboard, then the editor closes.
#[test]
fn copy_and_close_updates_the_file_and_the_clipboard() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let (file, before, thumb) = bed.capture()?;
    assert_eq!(bed.hp.clipboard_bytes().as_deref(), Some(&before[..]));
    let places = bed.edit(thumb)?;
    bed.drag(places.image(40.0, 60.0), places.image(260.0, 60.0))?;
    bed.click(places.at("copy"))?;
    let closed = bed.hp.dispatch_until(Duration::from_secs(5), |hp| {
        hp.capture_editor_surfaces().is_empty()
    })?;
    assert!(closed, "the editor stayed");
    let file_ = file.clone();
    let before_ = before.clone();
    let copied = bed.hp.dispatch_until(Duration::from_secs(10), move |hp| {
        let now = std::fs::read(&file_).unwrap_or_default();
        now != before_ && hp.clipboard_bytes() == Some(now)
    })?;
    assert!(copied, "the clipboard does not hold the annotated image");
    // Still one capture, in its place.
    let names: Vec<_> = std::fs::read_dir(file.parent().context("a folder")?)?
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(names.len(), 1);
    Ok(())
}

/// Closing with no marks changes nothing: the file as it was, the
/// clipboard too. Undone back to nothing is no marks.
#[test]
fn closing_with_no_marks_changes_nothing() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let (file, before, thumb) = bed.capture()?;
    let modified = std::fs::metadata(&file)?.modified()?;
    let places = bed.edit(thumb)?;
    bed.drag(places.image(40.0, 60.0), places.image(260.0, 60.0))?;
    bed.key(&[KEY_LEFTCTRL, KEY_Z])?;
    let mark = bed.mark();
    bed.click(places.at("copy"))?;
    bed.logged_since(mark, "the capture's editor closes")?;
    bed.idle(Duration::from_millis(500))?;
    assert!(bed.hp.capture_editor_surfaces().is_empty());
    assert_eq!(std::fs::read(&file)?, before);
    assert_eq!(std::fs::metadata(&file)?.modified()?, modified);
    assert_eq!(bed.hp.clipboard_bytes(), Some(before));
    Ok(())
}
