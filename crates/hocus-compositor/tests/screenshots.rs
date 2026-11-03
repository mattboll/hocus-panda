//! The screen capture (5.8, issue #398, docs/design/screenshots.md):
//! one scenario per point of the approved spec. The compositor owns the
//! capture, so most run without a shell: the toolbar's clicks are given
//! as the shell would send them (`capture mode|delay|shoot`); one runs the
//! real shell for its toolbar and what follows the capture. The pictures' folder is the
//! test's own, never the machine's.
//!
//! Pixman, a flat background: what is darkened, and what is not, is told
//! by its pixels.

mod common;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use common::{
    BTN_LEFT, KEY_DOWN, KEY_ESC, KEY_LEFT, KEY_LEFTALT, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_RIGHT,
    WAIT, WINDOW_COLOUR, chord, open, wait,
};
use hocus_compositor::{
    CaptureArea, CaptureShown, Config, Headless, HeadlessConfig, OutputSpec, RendererChoice,
};
use hocus_link::{CaptureAsk, CaptureMode};

/// Print (`KEY_SYSRQ`), Enter.
const KEY_PRINT: u32 = 99;
const KEY_ENTER: u32 = 28;

/// The window's colour as a PNG has it: red, green, blue, opaque.
const WINDOW: [u8; 4] = [
    (WINDOW_COLOUR >> 16) as u8,
    (WINDOW_COLOUR >> 8) as u8,
    WINDOW_COLOUR as u8,
    0xFF,
];

/// A compositor on `outputs`, its pictures' and state folders the test's.
struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
}

impl Bed {
    fn start(outputs: &[&str]) -> anyhow::Result<Self> {
        Self::start_in(outputs, tempfile::tempdir()?)
    }

    fn start_in(outputs: &[&str], dir: tempfile::TempDir) -> anyhow::Result<Self> {
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.plain_background = true;
        config.pictures = Some(dir.path().join("Pictures"));
        config.state_dir = Some(dir.path().join("state"));
        let outputs = outputs
            .iter()
            .map(|o| o.parse())
            .collect::<Result<Vec<OutputSpec>, _>>()?;
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs,
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        let mut bed = Self { hp, dir };
        let drawn = bed.hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
        anyhow::ensure!(drawn, "nothing drawn");
        Ok(bed)
    }

    fn shots(&self) -> PathBuf {
        self.dir.path().join("Pictures/Screenshots")
    }

    /// `Print`, and the capture open.
    fn open(&mut self) -> anyhow::Result<()> {
        chord(&mut self.hp, &[KEY_PRINT]);
        anyhow::ensure!(self.choosing().is_some(), "the capture did not open");
        wait(&mut self.hp, Duration::from_millis(50))
    }

    /// What the capture has chosen, while it is open: its mode and area.
    fn choosing(&self) -> Option<(CaptureMode, Option<(String, CaptureArea)>)> {
        match self.hp.capture() {
            CaptureShown::Choosing { mode, area, .. } => Some((mode, area)),
            _ => None,
        }
    }

    fn area(&self) -> Option<CaptureArea> {
        self.choosing().and_then(|(_, a)| a).map(|(_, a)| a)
    }

    /// A drag of the left button from `from` to `to`, in steps.
    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) -> anyhow::Result<()> {
        self.hp.pointer_move(from.0, from.1);
        self.hp.button(BTN_LEFT, true);
        for i in 1..=4 {
            let t = f64::from(i) / 4.0;
            self.hp
                .pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        }
        self.hp.button(BTN_LEFT, false);
        wait(&mut self.hp, Duration::from_millis(30))
    }

    /// The `n`th capture made, read back: its file and its image.
    fn taken(&mut self, n: u64) -> anyhow::Result<(PathBuf, image::RgbaImage)> {
        let done = self.hp.dispatch_until(WAIT, |hp| hp.captures().0 >= n)?;
        anyhow::ensure!(done, "no capture {n} made");
        let file = self.hp.captures().1.context("no file written")?;
        let image = image::open(&file)?.to_rgba8();
        Ok((file, image))
    }

    /// The pixel at (`x`, `y`) of the first screen as it shows: red, green,
    /// blue.
    fn shown(&mut self, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let p = bytes.get(at..at + 4).context("off the screen")?;
        Ok([p[2], p[1], p[0]])
    }
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A drag outside the area draws a new one, shown with its size; `Enter`
/// takes it.
#[test]
fn an_area_drawn_is_taken() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    open(&mut bed.hp, "editor", (1000, 700))?;
    bed.open()?;
    bed.drag((300.0, 200.0), (900.0, 580.0))?;
    assert_eq!(
        bed.area(),
        Some(CaptureArea {
            x: 300.0,
            y: 200.0,
            w: 600.0,
            h: 380.0
        })
    );
    // Darkened outside, not inside.
    let outside = bed.shown(100, 100)?;
    let inside = bed.shown(600, 400)?;
    assert!(
        outside.iter().zip(inside).all(|(o, i)| *o < i),
        "{outside:?} {inside:?}"
    );
    chord(&mut bed.hp, &[KEY_ENTER]);
    let (_, image) = bed.taken(1)?;
    assert_eq!(image.dimensions(), (600, 380));
    assert_eq!(bed.hp.capture(), CaptureShown::Closed);
    Ok(())
}

/// A press inside the area moves it, held inside its screen.
#[test]
fn the_area_moves_from_inside_and_stays_on_its_screen() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    bed.open()?;
    // The first area: a third of the screen, in its middle.
    let first = bed.area().context("no area")?;
    assert_eq!(
        (first.x, first.y, first.w, first.h),
        (640.0, 360.0, 640.0, 360.0)
    );
    assert_eq!(bed.hp.frame_cursor().as_deref(), Some("crosshair"));
    bed.hp.pointer_move(900.0, 500.0);
    assert_eq!(bed.hp.frame_cursor().as_deref(), Some("move"));
    bed.drag((900.0, 500.0), (1000.0, 450.0))?;
    let moved = bed.area().context("no area")?;
    assert_eq!(
        (moved.x, moved.y, moved.w, moved.h),
        (740.0, 310.0, 640.0, 360.0)
    );
    bed.drag((900.0, 500.0), (5000.0, 5000.0))?;
    let held = bed.area().context("no area")?;
    assert_eq!((held.x + held.w, held.y + held.h), (1920.0, 1080.0));
    // The arrows move it by a pixel; with Shift they resize it.
    chord(&mut bed.hp, &[KEY_LEFT]);
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_DOWN]);
    let nudged = bed.area().context("no area")?;
    assert_eq!((nudged.x, nudged.h), (held.x - 1.0, held.h));
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_LEFT]);
    let narrower = bed.area().context("no area")?;
    assert_eq!(narrower.w, nudged.w - 1.0);
    Ok(())
}

/// A corner's handle resizes both its sides, an edge its own; the cursor
/// says which.
#[test]
fn the_area_is_resized_by_a_corner_and_by_an_edge() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    bed.open()?;
    // Its bottom right corner, at (1280, 720).
    bed.hp.pointer_move(1281.0, 721.0);
    assert_eq!(bed.hp.frame_cursor().as_deref(), Some("nwse-resize"));
    bed.drag((1281.0, 721.0), (1381.0, 771.0))?;
    let corner = bed.area().context("no area")?;
    assert_eq!(
        (corner.x, corner.y, corner.w, corner.h),
        (640.0, 360.0, 740.0, 410.0)
    );
    // Its left edge, halfway down.
    bed.hp.pointer_move(640.0, 600.0);
    assert_eq!(bed.hp.frame_cursor().as_deref(), Some("ew-resize"));
    bed.drag((640.0, 600.0), (540.0, 100.0))?;
    let edge = bed.area().context("no area")?;
    assert_eq!(
        (edge.x, edge.y, edge.w, edge.h),
        (540.0, 360.0, 840.0, 410.0)
    );
    Ok(())
}

/// The area is kept for the next capture, and for the next session.
#[test]
fn the_area_is_kept_for_next_time() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    bed.open()?;
    bed.drag((100.0, 100.0), (500.0, 400.0))?;
    chord(&mut bed.hp, &[KEY_ESC]);
    bed.open()?;
    let again = bed.area().context("no area")?;
    assert_eq!(
        (again.x, again.y, again.w, again.h),
        (100.0, 100.0, 400.0, 300.0)
    );
    chord(&mut bed.hp, &[KEY_ESC]);
    // Written off the loop: there once the thread is done.
    let file = bed.dir.path().join("state/capture-area");
    let written = bed.hp.dispatch_until(WAIT, |_| file.exists())?;
    anyhow::ensure!(written, "the area was not kept");
    let Bed { hp, dir } = bed;
    drop(hp);
    let mut bed = Bed::start_in(&["1920x1080"], dir)?;
    bed.open()?;
    let kept = bed.area().context("no area")?;
    assert_eq!(
        (kept.x, kept.y, kept.w, kept.h),
        (100.0, 100.0, 400.0, 300.0)
    );
    Ok(())
}

/// Window mode: the window under the pointer is chosen, and a click
/// takes it alone, at its size, without its shadow.
#[test]
fn window_mode_takes_the_window_under_the_pointer() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    let left = open(&mut bed.hp, "left", (700, 500))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFT]);
    let _ = left;
    open(&mut bed.hp, "right", (640, 480))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    // Both drawn where they went, their motions over.
    common::at_rest(&mut bed.hp)?;
    let left = common::rect_of(&bed.hp, "left").context("no left window")?;
    bed.open()?;
    bed.hp.capture_ask(CaptureAsk::Mode(CaptureMode::Window));
    let (cx, cy) = (
        f64::from(left.x + left.width / 2),
        f64::from(left.y + left.height / 2),
    );
    bed.hp.pointer_move(cx, cy);
    let hovered = match bed.hp.capture() {
        CaptureShown::Choosing { hovered, .. } => hovered,
        _ => None,
    };
    let left_id = bed
        .hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("left"))
        .map(|w| w.id);
    assert_eq!(hovered, left_id);
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    let (_, image) = bed.taken(1)?;
    assert_eq!(
        image.dimensions(),
        (u32::try_from(left.width)?, u32::try_from(left.height)?)
    );
    // Its middle, the window's own colour; its corner, rounded off.
    let (w, h) = image.dimensions();
    assert_eq!(image.get_pixel(w / 2, h / 2).0, WINDOW);
    assert_eq!(image.get_pixel(0, 0).0[3], 0);
    Ok(())
}

/// "Include the pointer" in window mode (5.8b, issue #528): on, the
/// window's capture shows the pointer where it is on the window; off, it
/// does not.
#[test]
fn a_window_capture_shows_the_pointer_when_asked() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    open(&mut bed.hp, "editor", (800, 600))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let editor = common::rect_of(&bed.hp, "editor").context("no window")?;
    // The pointer a third of the way into the window, away from its
    // middle and its corners.
    let at = (editor.width / 3, editor.height / 3);
    let pointer = (
        f64::from(editor.x + at.0) + 0.5,
        f64::from(editor.y + at.1) + 0.5,
    );
    // The pixels of a 16 px square under the pointer's tip that are not
    // the window's colour.
    let under_the_tip = |image: &image::RgbaImage| -> usize {
        let (x0, y0) = (
            u32::try_from(at.0).unwrap_or(0),
            u32::try_from(at.1).unwrap_or(0),
        );
        (y0..y0 + 16)
            .flat_map(|y| (x0..x0 + 16).map(move |x| (x, y)))
            .filter(|(x, y)| image.get_pixel(*x, *y).0 != WINDOW)
            .count()
    };
    let take = |bed: &mut Bed, pointer_on: bool, n: u64| -> anyhow::Result<image::RgbaImage> {
        bed.open()?;
        bed.hp.capture_ask(CaptureAsk::Mode(CaptureMode::Window));
        let shown = match bed.hp.capture() {
            CaptureShown::Choosing { pointer, .. } => pointer,
            _ => false,
        };
        if shown != pointer_on {
            bed.hp.capture_ask(CaptureAsk::Pointer);
        }
        bed.hp.pointer_move(pointer.0 - 1.0, pointer.1);
        bed.hp.pointer_move(pointer.0, pointer.1);
        bed.hp.button(BTN_LEFT, true);
        bed.hp.button(BTN_LEFT, false);
        let (_, image) = bed.taken(n)?;
        anyhow::ensure!(
            image.dimensions() == (u32::try_from(editor.width)?, u32::try_from(editor.height)?),
            "not the window: {:?}",
            image.dimensions()
        );
        Ok(image)
    };
    let with = take(&mut bed, true, 1)?;
    assert!(
        under_the_tip(&with) > 20,
        "no pointer drawn on the window: {} pixels",
        under_the_tip(&with)
    );
    // Away from it, the window as it is.
    let (w, h) = with.dimensions();
    assert_eq!(with.get_pixel(w * 3 / 4, h * 3 / 4).0, WINDOW);
    let without = take(&mut bed, false, 2)?;
    assert_eq!(
        under_the_tip(&without),
        0,
        "a pointer drawn though not asked"
    );
    Ok(())
}

/// Screen mode takes the screen under the pointer; all screens, one image
/// of them as they are laid out.
#[test]
fn the_screen_and_all_the_screens() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080", "2560x1440"])?;
    bed.open()?;
    bed.hp.capture_ask(CaptureAsk::Mode(CaptureMode::Screen));
    bed.hp.pointer_move(2500.0, 700.0);
    chord(&mut bed.hp, &[KEY_ENTER]);
    let (_, screen) = bed.taken(1)?;
    assert_eq!(screen.dimensions(), (2560, 1440));
    bed.open()?;
    bed.hp.capture_ask(CaptureAsk::Mode(CaptureMode::All));
    bed.hp.capture_ask(CaptureAsk::Shoot);
    let (_, all) = bed.taken(2)?;
    assert_eq!(all.dimensions(), (1920 + 2560, 1440));
    // Under the smaller screen, nothing.
    assert_eq!(all.get_pixel(10, 1400).0[3], 0);
    assert_eq!(all.get_pixel(10, 10).0[3], 0xFF);
    Ok(())
}

/// A delay counts down, the darkening and the toolbar gone meanwhile, and
/// none of the capture is in the image.
#[test]
fn the_delay_counts_down_and_the_capture_is_not_in_the_image() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    open(&mut bed.hp, "editor", (1000, 700))?;
    let editor = common::rect_of(&bed.hp, "editor").context("no window")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    let shown_before = bed.shown(1900, 1060)?;
    bed.open()?;
    // The area over the window, inside it.
    let (x, y) = (f64::from(editor.x + 100), f64::from(editor.y + 100));
    bed.drag((x, y), (x + 300.0, y + 200.0))?;
    bed.hp.capture_ask(CaptureAsk::Delay);
    bed.hp.capture_ask(CaptureAsk::Shoot);
    assert_eq!(
        bed.hp.capture(),
        CaptureShown::Counting {
            left: 3,
            record: false
        }
    );
    wait(&mut bed.hp, Duration::from_millis(100))?;
    // Nothing darkened while it counts.
    assert_eq!(bed.shown(1900, 1060)?, shown_before);
    let counted = bed.hp.dispatch_until(WAIT, |hp| {
        hp.capture()
            == CaptureShown::Counting {
                left: 2,
                record: false,
            }
    })?;
    assert!(counted, "the countdown did not go on");
    assert_eq!(bed.hp.captures().0, 0);
    let (_, image) = bed.taken(1)?;
    assert_eq!(image.dimensions(), (300, 200));
    // The window's colour, neither darkened nor framed.
    for (px, py) in [(0, 0), (150, 100), (299, 199)] {
        assert_eq!(image.get_pixel(px, py).0, WINDOW, "({px}, {py})");
    }
    Ok(())
}

/// The PNG is written in the pictures' folder, named for when it was
/// taken, and the clipboard holds the same image.
#[test]
fn the_png_is_saved_and_copied() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    bed.open()?;
    chord(&mut bed.hp, &[KEY_ENTER]);
    let (file, image) = bed.taken(1)?;
    assert_eq!(image.dimensions(), (640, 360));
    assert_eq!(file.parent(), Some(bed.shots().as_path()));
    let name = files(&bed.shots());
    assert_eq!(name.len(), 1);
    let name = &name[0];
    // "Screenshot from 2026-11-03 14-32-05.png".
    let stamp = name
        .strip_prefix("Screenshot from ")
        .and_then(|n| n.strip_suffix(".png"))
        .context("not a capture's name")?;
    let bytes = stamp.as_bytes();
    assert_eq!(bytes.len(), 19, "{name}");
    assert_eq!(
        (bytes[4], bytes[7], bytes[10], bytes[13], bytes[16]),
        (b'-', b'-', b' ', b'-', b'-')
    );
    assert_eq!(bed.hp.clipboard_bytes(), Some(std::fs::read(&file)?));
    Ok(())
}

/// `Shift+Print` takes the screen under the pointer, `Alt+Print` the
/// focused window, at once: nothing to choose.
#[test]
fn shift_print_and_alt_print_take_at_once() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080", "1280x720"])?;
    open(&mut bed.hp, "editor", (800, 600))?;
    let editor = common::rect_of(&bed.hp, "editor").context("no window")?;
    bed.hp.pointer_move(2500.0, 300.0);
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_PRINT]);
    assert_eq!(bed.hp.capture(), CaptureShown::Closed);
    let (_, screen) = bed.taken(1)?;
    assert_eq!(screen.dimensions(), (1280, 720));
    chord(&mut bed.hp, &[KEY_LEFTALT, KEY_PRINT]);
    assert_eq!(bed.hp.capture(), CaptureShown::Closed);
    let (_, window) = bed.taken(2)?;
    assert_eq!(
        window.dimensions(),
        (u32::try_from(editor.width)?, u32::try_from(editor.height)?)
    );
    Ok(())
}

/// `Escape` cancels: nothing is taken, nothing is darkened any more.
#[test]
fn escape_cancels() -> anyhow::Result<()> {
    let mut bed = Bed::start(&["1920x1080"])?;
    let before = bed.shown(1900, 1060)?;
    bed.open()?;
    wait(&mut bed.hp, Duration::from_millis(50))?;
    assert_ne!(bed.shown(1900, 1060)?, before, "not darkened");
    chord(&mut bed.hp, &[KEY_ESC]);
    assert_eq!(bed.hp.capture(), CaptureShown::Closed);
    wait(&mut bed.hp, Duration::from_millis(100))?;
    assert_eq!(bed.shown(1900, 1060)?, before);
    assert_eq!(bed.hp.captures().0, 0);
    assert!(files(&bed.shots()).is_empty());
    assert_eq!(bed.hp.frame_cursor(), None);
    Ok(())
}

/// With the real shell: the toolbar at the bottom of the screen under the
/// pointer, its modes and its green button clicked, and the capture's
/// thumbnail once taken (5.9), in its French folder.
#[test]
fn the_toolbar_and_the_toast() -> anyhow::Result<()> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return Ok(());
    };
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "config", "cache", "state"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let log = root.join("shell.log");
    let home = |sub: &str| root.join(sub).display().to_string();
    let script = common::shell_script(
        root,
        &shell,
        &[
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
            ("HOME", home("home")),
            ("XDG_CONFIG_HOME", home("config")),
            ("XDG_CACHE_HOME", home("cache")),
            ("XDG_STATE_HOME", home("state")),
            ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
        ],
    )?;
    // Its log, beside it.
    let text = std::fs::read_to_string(&script)?
        .replace("\"$@\"\n", &format!("\"$@\" 2>>'{}'\n", log.display()));
    std::fs::write(&script, text)?;
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    config.pictures = Some(root.join("Images"));
    config.french = true;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    hp.pointer_move(400.0, 300.0);
    chord(&mut hp, &[KEY_PRINT]);
    let shown = hp.dispatch_until(WAIT, |hp| {
        hp.capture_toolbar_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height > 0)
    })?;
    anyhow::ensure!(shown, "no toolbar");
    let (output, bar) = hp.capture_toolbar_surfaces()[0].clone();
    assert_eq!(output, "HEADLESS-1");
    // At the bottom, in the middle, 33 px up (22 × 1.5).
    assert_eq!(bar.y + bar.height, 1080 - 33);
    assert!((bar.x + bar.width / 2 - 960).abs() <= 1, "{bar:?}");
    wait(&mut hp, Duration::from_millis(250))?;
    let click = |hp: &mut Headless, x: i32| -> anyhow::Result<()> {
        let y = f64::from(bar.y + bar.height / 2);
        // Once the toolbar there is drawn at its size: it changes with
        // the mode, a moment after the compositor's.
        common::point(hp, (f64::from(x), y))?;
        wait(hp, Duration::from_millis(100))?;
        hp.button(BTN_LEFT, true);
        hp.button(BTN_LEFT, false);
        Ok(())
    };
    // The second mode, "Fenêtre", after "Zone", both after the Photo /
    // Vidéo switch (5.10).
    click(&mut hp, bar.x + 380)?;
    let window = hp.dispatch_until(WAIT, |hp| {
        matches!(
            hp.capture(),
            CaptureShown::Choosing {
                mode: CaptureMode::Window,
                ..
            }
        )
    })?;
    assert!(
        window,
        "the toolbar's mode was not chosen: {:?}",
        hp.capture()
    );
    // Back to the area, then the green button, at its right end.
    click(&mut hp, bar.x + 250)?;
    let area = hp.dispatch_until(WAIT, |hp| {
        matches!(
            hp.capture(),
            CaptureShown::Choosing {
                mode: CaptureMode::Area,
                ..
            }
        )
    })?;
    assert!(area, "back to the area: {:?}", hp.capture());
    // Where the toolbar is now drawn, back in its area's size.
    let back = hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::CAPTURE_TOOLBAR_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| *r == bar)
    })?;
    assert!(
        back,
        "{:?}, not {bar:?}",
        hp.layer_rects(hocus_link::CAPTURE_TOOLBAR_NAMESPACE)
    );
    click(&mut hp, bar.x + bar.width - 30)?;
    let taken = hp.dispatch_until(WAIT, |hp| hp.captures().0 == 1)?;
    assert!(taken, "nothing captured: {:?}", hp.capture());
    let gone = hp.dispatch_until(WAIT, |hp| hp.capture_toolbar_surfaces().is_empty())?;
    assert!(gone, "the toolbar stayed");
    // Saved: its thumbnail (5.9) in place of 5.8's toast.
    let thumb = hp.dispatch_until(WAIT, |hp| hp.capture_thumb_surfaces().len() == 1)?;
    assert!(thumb, "no thumbnail");
    assert!(hp.launcher_toast_surfaces().is_empty());
    let said = String::from_utf8_lossy(&std::fs::read(&log)?).into_owned();
    let line = said
        .lines()
        .rfind(|l| l.contains("the capture's thumbnail shows"))
        .unwrap_or_default()
        .to_owned();
    assert!(
        line.contains("Images/Captures d'écran/Capture du "),
        "{line}"
    );
    let saved = files(&root.join("Images/Captures d'écran"));
    assert_eq!(saved.len(), 1);
    assert!(
        saved[0].starts_with("Capture du ") && saved[0].contains(" à "),
        "{saved:?}"
    );
    Ok(())
}
