//! What comes after a capture (5.9, issue #399, docs/design/screenshots.md):
//! its thumbnail, its editor, its place in the notifications' history. One
//! scenario per point of the approved spec, with the real shell on a
//! D-Bus daemon of the test's own (the history is served there, never the
//! session's), its pictures', state and home folders the test's.
//!
//! Pixman here, so the shell draws on the CPU, as in CI (and, for #594, on
//! wgpu over Mesa's software Vulkan when it is installed). The capture is
//! an area over a window of black and white squares of 3 pixels, as fine
//! as small text; the editor shows it a pixel a pixel, so that the marks
//! drawn on the screen land on the image's pixels under them.

use crate::common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::Context;
use common::{BTN_LEFT, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTSHIFT, Shows, WAIT, chord, open_showing};
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
    /// The checkered window, where it is.
    window: Rect,
    _window: common::ClientHandle,
}

/// How the shell draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Draws {
    /// tiny-skia, as in CI.
    Cpu,
    /// wgpu, on Mesa's software Vulkan driver (lavapipe), its manifest:
    /// iced's GPU renderer without a GPU.
    SoftwareVulkan(&'static str),
}

/// Mesa's software Vulkan driver's manifest, where distributions put it.
fn lavapipe() -> Option<&'static str> {
    [
        "/usr/share/vulkan/icd.d/lvp_icd.json",
        "/usr/share/vulkan/icd.d/lvp_icd.x86_64.json",
        "/usr/share/vulkan/icd.d/lvp_icd.aarch64.json",
    ]
    .into_iter()
    .find(|p| Path::new(p).exists())
}

impl Drop for Bed {
    fn drop(&mut self) {
        // A test failed: what the shell said of the capture, to see why.
        if std::thread::panicking() {
            for line in self.log().lines().filter(|l| l.contains("capture")) {
                eprintln!("shell: {line}");
            }
        }
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

fn ready() -> Option<PathBuf> {
    let shell = common::shell_program();
    let daemon = Command::new("sh")
        .args(["-c", "command -v dbus-daemon"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !daemon {
        common::missing("dbus-daemon");
    }
    Some(shell)
}

impl Bed {
    /// The compositor on a 1920×1080 screen (the full bar), the real shell
    /// in French on the test's bus, a checkered window under the area.
    fn start(shell: &Path) -> anyhow::Result<Self> {
        Self::start_drawing(shell, Draws::Cpu)
    }

    /// The same, the shell drawing as `draws` says.
    fn start_drawing(shell: &Path, draws: Draws) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "state", "config", "cache"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let (bus, address) = common::private_bus(root)?;
        let log = root.join("shell.log");
        let script = root.join("shell");
        let p = |s: &str| root.join(s).display().to_string();
        // On the software Vulkan driver alone, never a GPU's.
        let renderer = match draws {
            Draws::Cpu => String::new(),
            Draws::SoftwareVulkan(manifest) => format!(
                "export ICED_BACKEND=wgpu WGPU_BACKEND=vulkan VK_DRIVER_FILES='{manifest}' \
                 VK_ICD_FILENAMES='{manifest}'\n"
            ),
        };
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' TZ=UTC \
                 RUST_LOG=warn,hocuspanda_shell=debug\n{renderer}exec '{}' \"$@\" 2>>'{}'\n",
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
        let (window_rect, window) = open_showing(
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
            window: window_rect,
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
        // Once the shell's surfaces there are drawn: else the press goes
        // to what is under them.
        common::point(&mut self.hp, (x, y))?;
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
        self.capture_area(AREA)
    }

    /// The same, of area `area`.
    fn capture_area(
        &mut self,
        area: (f64, f64, f64, f64),
    ) -> anyhow::Result<(PathBuf, Vec<u8>, Rect)> {
        let before = self.hp.captures().0;
        self.hp.pointer_move(300.0, 150.0);
        chord(&mut self.hp, &[KEY_PRINT]);
        self.idle(Duration::from_millis(100))?;
        let (x, y, w, h) = area;
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
        let image = (
            f64::from(thumb.x + thumb.width / 2),
            f64::from(thumb.y + 100),
        );
        self.over(image, hocus_link::CAPTURE_THUMB_NAMESPACE)?;
        self.click(image)?;
        self.editor(mark)
    }

    /// The pointer at `at`, once the shell's surface of `namespace` takes
    /// it ([`common::over`]).
    fn over(&mut self, at: (f64, f64), namespace: &str) -> anyhow::Result<()> {
        common::over(&mut self.hp, at, namespace)
    }

    /// The editor once it shows, laid out after `mark`.
    fn editor(&mut self, mark: usize) -> anyhow::Result<Places> {
        let line = self.logged_since(mark, "the capture's editor is laid out")?;
        let open = self.hp.dispatch_until(Duration::from_secs(5), |hp| {
            hp.capture_editor_surfaces().len() == 1
        })?;
        anyhow::ensure!(open, "no editor");
        let places = Places::read(&line)?;
        self.over(places.at("copy"), hocus_link::CAPTURE_EDITOR_NAMESPACE)?;
        self.idle(Duration::from_millis(100))?;
        Ok(places)
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
        let shown = self.hp.dispatch_until(Duration::from_secs(10), |hp| {
            hp.drawn_layers(hocus_link::BELL_NAMESPACE).len() == 1
        })?;
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

    /// The image's place on the screen: x, y, width, height.
    fn rect(&self, name: &str) -> (f64, f64, f64, f64) {
        let n = &self
            .0
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1;
        (n[0], n[1], n[2], n[3])
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
    bed.over(
        (f64::from(thumb.x + 40), f64::from(thumb.y + 240)),
        hocus_link::CAPTURE_THUMB_NAMESPACE,
    )?;
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
    let action = (f64::from(popover.x) + at.0, f64::from(popover.y) + at.1);
    bed.over(action, hocus_link::POPOVER_NAMESPACE)?;
    bed.click(action)?;
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
    bed.over(Bed::close_spot(thumb), hocus_link::CAPTURE_THUMB_NAMESPACE)?;
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

/// Run by [`common::other_program`] as a process of its own: the portal's
/// backend's dialog.
#[test]
fn another_program() -> anyhow::Result<()> {
    common::as_other_program()
}

/// What the desktop portal's file chooser was asked: the dialog's parent,
/// the request's path, who asked.
type Asked = std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>;

/// The desktop portal's file chooser, as the shell sees it on the test's
/// bus: it says what it was asked, and answers when the test says.
struct Chooser(Asked);

#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl Chooser {
    fn save_file(
        &self,
        parent_window: String,
        _title: String,
        options: std::collections::HashMap<String, zbus::OwnedVariant>,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<zbus::OwnedObjectPath> {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let token = options
            .get("handle_token")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        let path = format!(
            "/org/freedesktop/portal/desktop/request/{}/{token}",
            sender.trim_start_matches(':').replace('.', "_")
        );
        self.0
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("poisoned".into()))?
            .push((parent_window, path.clone(), sender));
        zbus::OwnedObjectPath::try_from(path).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

/// "Enregistrer sous…" (#595): the portal's file chooser is the editor's
/// child (`parent_window`, an xdg-foreign handle); its backend, another
/// program, maps its dialog as that handle's child and asks for the focus
/// with a token of its own, made from no input: the dialog comes in front
/// with the keyboard, though another program's window (a terminal, as
/// the maintainer had) had the focus. Chosen, the annotated copy is
/// written there. The programs are processes of the test's: the desktop
/// lets a program's window take the focus from the window of the program
/// that started it, so the terminal is the dialog's sibling.
#[test]
fn save_as_shows_the_chooser_in_front_with_the_keyboard() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let asked = Asked::default();
    let address = format!("unix:path={}", bed.dir.path().join("bus").display());
    let portal = runtime.block_on(
        zbus::connection::Builder::address(address.as_str())
            .name("org.freedesktop.portal.Desktop")
            .serve_at("/org/freedesktop/portal/desktop", Chooser(asked.clone()))
            .build(),
    )?;
    let _terminal = common::other_program(&mut bed.hp, "test.terminal", false, None)?;
    let (_, _, thumb) = bed.capture()?;
    let places = bed.edit(thumb)?;
    bed.drag(places.image(40.0, 60.0), places.image(260.0, 60.0))?;
    assert!(
        bed.hp
            .windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some("test.terminal") && w.focused),
        "{:?}",
        bed.hp.windows()
    );
    bed.click(places.at("save-as"))?;
    let came = bed
        .hp
        .dispatch_until(WAIT, |_| asked.lock().is_ok_and(|a| !a.is_empty()))?;
    anyhow::ensure!(came, "the file chooser was never asked");
    let (parent, path, sender) = asked
        .lock()
        .map_err(|_| anyhow::anyhow!("poisoned"))?
        .remove(0);
    let handle = parent
        .strip_prefix("wayland:")
        .with_context(|| format!("no parent: {parent:?}"))?;
    let _dialog = common::other_program(&mut bed.hp, "test.chooser", false, Some(handle))?;
    let front = bed.hp.dispatch_until(WAIT, |hp| {
        let windows = hp.windows();
        windows
            .last()
            .is_some_and(|w| w.app_id.as_deref() == Some("test.chooser") && w.focused && w.keyboard)
    })?;
    assert!(front, "{:?}", bed.hp.windows());
    // Chosen.
    let chosen = bed.dir.path().join("chosen.png");
    let mut results = std::collections::HashMap::new();
    results.insert(
        "uris",
        zbus::Variant::from(vec![format!("file://{}", chosen.display())]),
    );
    let mark = bed.mark();
    runtime.block_on(portal.emit_signal(
        Some(sender.as_str()),
        path.as_str(),
        "org.freedesktop.portal.Request",
        "Response",
        &(0u32, results),
    ))?;
    bed.logged_since(mark, "the capture is saved as")?;
    let saved = png(&std::fs::read(&chosen)?)?;
    assert_eq!(saved.dimensions(), (AREA.2 as u32, AREA.3 as u32));
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
    // Gone once its fade out is over.
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.capture_editor_surfaces().is_empty())?;
    assert!(gone, "{:?}", bed.hp.capture_editor_surfaces());
    assert_eq!(std::fs::read(&file)?, before);
    assert_eq!(std::fs::metadata(&file)?.modified()?, modified);
    assert_eq!(bed.hp.clipboard_bytes(), Some(before));
    Ok(())
}

/// Watches every frame the compositor draws: whether the editor's image
/// shows the capture in it, its squares' grey, not the dark of its empty
/// place.
struct Frames {
    /// The image's place inside, the screen's pixels (scale 1).
    inside: (i32, i32, i32, i32),
    frames: u64,
    checked: u64,
}

impl Frames {
    fn new(hp: &Headless, image: (f64, f64, f64, f64)) -> Self {
        let (x, y, w, h) = image;
        let inset = 24.0;
        Self {
            inside: (
                (x + inset) as i32,
                (y + inset) as i32,
                (w - 2.0 * inset) as i32,
                (h - 2.0 * inset) as i32,
            ),
            frames: hp.frames(),
            checked: 0,
        }
    }

    /// The picture is in the last frame drawn: the mean of its brightness.
    fn check(&mut self, hp: &mut Headless, when: &str) -> anyhow::Result<()> {
        let (width, _, pixels) = hp.screenshot(0)?;
        let (x0, y0, w, h) = self.inside;
        let (mut sum, mut n) = (0.0, 0.0);
        for y in (y0..y0 + h).step_by(5) {
            for x in (x0..x0 + w).step_by(5) {
                let i = ((y * width + x) * 4) as usize;
                let p = &pixels[i..i + 3];
                sum += (f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2])) / 3.0;
                n += 1.0;
            }
        }
        let mean = sum / n;
        self.checked += 1;
        // The squares, scaled down, are a grey around 128; the empty place
        // is the panel's dark, under 30.
        anyhow::ensure!(
            mean > 70.0,
            "frame {} ({when}): the picture is gone, its place {mean:.0} bright",
            hp.frames()
        );
        Ok(())
    }

    /// Runs the compositor until `done` holds, every frame drawn checked.
    fn until(
        &mut self,
        hp: &mut Headless,
        when: &str,
        mut done: impl FnMut(&Headless) -> bool,
    ) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            if hp.frames() != self.frames {
                self.frames = hp.frames();
                self.check(hp, when)?;
            }
            if done(hp) {
                return Ok(());
            }
            anyhow::ensure!(start.elapsed() < WAIT, "{when}: waited in vain");
            hp.dispatch(Duration::from_millis(5))?;
        }
    }

    /// Runs the compositor until the editor's surface has committed since
    /// `commits` and the compositor drew it, every frame checked.
    fn drawn_after(&mut self, hp: &mut Headless, commits: u64, when: &str) -> anyhow::Result<()> {
        self.until(hp, when, |hp| hp.capture_editor_commits() > commits)?;
        let frames = hp.frames();
        self.until(hp, when, |hp| hp.frames() > frames)
    }
}

/// The picture stays on the screen while an arrow is drawn on it, and once
/// it is (#594): on every frame from the pointer's press to its release
/// and after. The image is large enough (over 2 MiB of pixels) that iced's
/// GPU renderer would upload it on a thread of its own.
fn the_picture_stays_while_an_arrow_is_drawn(draws: Draws) -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start_drawing(&shell, draws)?;
    let w = bed.window;
    let area = (
        f64::from(w.x + 25),
        f64::from(w.y + 25),
        f64::from(w.width - 50),
        f64::from(w.height - 50),
    );
    let (_, _, thumb) = bed.capture_area(area)?;
    let places = bed.edit(thumb)?;
    let image = places.rect("image");
    // Shown smaller than captured, and more than 2 MiB of pixels all the
    // same.
    assert!(image.2 < area.2 && image.2 * image.3 * 4.0 > 2.0 * 1024.0 * 1024.0);
    let mut frames = Frames::new(&bed.hp, image);
    frames.check(&mut bed.hp, "the editor open")?;
    let at = |x: f64, y: f64| (image.0 + image.2 * x, image.1 + image.3 * y);
    let (from, to) = (at(0.2, 0.3), at(0.7, 0.6));
    let mark = bed.mark();
    let commits = bed.hp.capture_editor_commits();
    bed.hp.pointer_move(from.0, from.1);
    frames.drawn_after(&mut bed.hp, commits, "the pointer on the image")?;
    let commits = bed.hp.capture_editor_commits();
    bed.hp.button(BTN_LEFT, true);
    frames.drawn_after(&mut bed.hp, commits, "pressed")?;
    for i in 1..=5 {
        let t = f64::from(i) / 5.0;
        let commits = bed.hp.capture_editor_commits();
        bed.hp
            .pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        frames.drawn_after(&mut bed.hp, commits, "dragged")?;
    }
    let commits = bed.hp.capture_editor_commits();
    bed.hp.button(BTN_LEFT, false);
    frames.drawn_after(&mut bed.hp, commits, "released")?;
    bed.logged_since(mark, "a mark on the capture")?;
    // After: the pointer onto a button, the editor drawn again.
    let commits = bed.hp.capture_editor_commits();
    let copy = places.at("copy");
    bed.hp.pointer_move(copy.0, copy.1);
    frames.drawn_after(&mut bed.hp, commits, "after")?;
    frames.check(&mut bed.hp, "at the end")?;
    // The arrow is on it, red, half-way along its line.
    let (width, _, pixels) = bed.hp.screenshot(0)?;
    let middle = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    let i = ((middle.1 as i32 * width + middle.0 as i32) * 4) as usize;
    let (b, g, r) = (pixels[i], pixels[i + 1], pixels[i + 2]);
    assert!(near([r, g, b, 255], RED), "{:?}", (r, g, b));
    eprintln!("{} frames checked", frames.checked);
    Ok(())
}

#[test]
fn the_picture_stays_while_an_arrow_is_drawn_on_the_cpu() -> anyhow::Result<()> {
    the_picture_stays_while_an_arrow_is_drawn(Draws::Cpu)
}

/// The same with iced's GPU renderer, wgpu, on Mesa's software Vulkan
/// driver: the one that lost the picture (#594). Fails without it.
#[test]
fn the_picture_stays_while_an_arrow_is_drawn_on_wgpu() -> anyhow::Result<()> {
    let Some(manifest) = lavapipe() else {
        common::missing("Mesa's software Vulkan driver, lavapipe (mesa-vulkan-drivers)");
    };
    the_picture_stays_while_an_arrow_is_drawn(Draws::SoftwareVulkan(manifest))
}
