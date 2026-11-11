//! The bar over a wallpaper that moves, in Givre and Matrix (bar.md §13,
//! #433): the real shell drawing on wgpu, as on a GPU (over Mesa's
//! software Vulkan driver, lavapipe; failing without it or without the
//! shell built), the compositor on GLES (Mesa's software device), the
//! screen as the maintainer's laptop's, 2560×1600 at 1.25. Frame after
//! frame on the held clock, the bar's empty middle (its glass and tint,
//! nothing of the shell's there) stays as it was across the wallpaper's
//! frames, a window passing over the bar and desktop switches there and
//! back, and no bright speck of the wallpaper shows through it, a theme
//! switch included.
//!
//! Between two of its frames the wallpaper was drawn again over the bounds
//! of whatever changed elsewhere (the desktops' dots, a row of the bar),
//! and the bar vanished there until its next frame.

use crate::common;

use std::{path::Path, time::Duration};

use hocus_compositor::{Config, Headless, HeadlessConfig, model::Rect};

const STEP: Duration = Duration::from_micros(16_667);
const OUTPUT: &str = "HEADLESS-1";
const SCALE: f64 = 1.25;
/// The bar's empty middle, logical: under the day's card, over the
/// windows' row.
const MIDDLE: (i32, i32) = (330, 1090);
/// Brighter than glass and tint ever let through: the stars, the rain's
/// heads.
const BRIGHT: u8 = 150;

/// A pixel's place (logical) and colour (R, G, B).
type Pixel = ((i32, i32), [u8; 3]);

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

/// A theme's file, with what choosing it sets (settings.md).
fn file(theme: &str) -> String {
    let (accent, picture, animation) = match theme {
        "matrix" => ("phosphor", "rain", "rain"),
        _ => ("ice", "aurora", "frost"),
    };
    format!(
        "[keyboard]\nlayouts = [\"us\"]\n\n[appearance]\ntheme = \"{theme}\"\n\
         accent = \"{accent}\"\n\n[wallpaper]\npicture = \"{picture}\"\n\n\
         [windows]\nanimation = \"{animation}\"\n"
    )
}

/// The bright specks among `pixels`.
fn bright(pixels: &[Pixel]) -> Vec<&Pixel> {
    pixels
        .iter()
        .filter(|(_, c)| c.iter().any(|v| *v > BRIGHT))
        .take(3)
        .collect()
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    file: std::path::PathBuf,
    /// The bar's place, logical.
    bar: Rect,
}

impl Bed {
    /// The compositor and the real shell on wgpu in `theme`; `None` when
    /// either is missing.
    fn start(theme: &str) -> anyhow::Result<Option<Self>> {
        let shell = common::shell_program();
        let Some(manifest) = lavapipe() else {
            common::missing("Mesa's software Vulkan driver, lavapipe (mesa-vulkan-drivers)");
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "state", "cache"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, self::file(theme))?;
        let at = |sub: &str| root.join(sub).display().to_string();
        let script = common::shell_script(
            root,
            &shell,
            &[
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("config")),
                ("XDG_CACHE_HOME", at("cache")),
                ("XDG_STATE_HOME", at("state")),
                ("TZ", "UTC".to_owned()),
                ("ICED_BACKEND", "wgpu".to_owned()),
                ("WGPU_BACKEND", "vulkan".to_owned()),
                ("VK_DRIVER_FILES", manifest.to_owned()),
                ("VK_ICD_FILENAMES", manifest.to_owned()),
            ],
        )?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file.clone());
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec!["2560x1600@1.25".parse()?],
                ..common::gles()
            },
            config,
            Some(script),
        )?;
        if hp.renderer() != "gles" {
            common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
        }
        let up = hp.dispatch_until(Duration::from_secs(60), |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        // Its surfaces drawn, the glass under them.
        common::wait(&mut hp, Duration::from_secs(2))?;
        anyhow::ensure!(hp.glass_on(), "the shell draws on the CPU: no glass");
        let bar = hp.bar_surfaces()[0].1;
        hp.hold_clock();
        hp.pointer_move(300.0, 300.0);
        for _ in 0..10 {
            hp.step(STEP)?;
        }
        Ok(Some(Self { hp, dir, file, bar }))
    }

    /// The bar's empty middle now, clear of its rims and of Matrix's code
    /// edge on its left; none of it within reach of `hidden`'s shadow (a
    /// window held casts one 48 px blurred, 18 px down) or its wobble.
    fn middle(&mut self, hidden: Option<Rect>) -> anyhow::Result<Vec<Pixel>> {
        let (width, _, shot) = self.hp.screenshot(0)?;
        let bar = self.bar;
        let away = 160;
        let mut out = Vec::new();
        for y in (MIDDLE.0..MIDDLE.1).step_by(3) {
            for x in (bar.x + 14..bar.x + bar.width - 8).step_by(3) {
                if hidden.is_some_and(|r| {
                    x > r.x - away
                        && x < r.x + r.width + away
                        && y > r.y - away
                        && y < r.y + r.height + away
                }) {
                    continue;
                }
                let (px, py) = ((f64::from(x) * SCALE) as i32, (f64::from(y) * SCALE) as i32);
                let at = ((py * width + px) * 4) as usize;
                out.push(((x, y), [shot[at + 2], shot[at + 1], shot[at]]));
            }
        }
        Ok(out)
    }

    /// A frame stepped: the bar's middle as `before` (where both have a
    /// pixel), and no bright speck of the wallpaper through it.
    fn frame(&mut self, what: &str, before: &[Pixel], hidden: Option<Rect>) -> anyhow::Result<()> {
        self.hp.step(STEP)?;
        let now = self.middle(hidden)?;
        let specks = bright(&now);
        anyhow::ensure!(
            specks.is_empty(),
            "{what}: the wallpaper shows through the bar at {specks:?}"
        );
        // Both in the same order: rows, then columns.
        let mut was = before.iter().peekable();
        let mut changed = Vec::new();
        for (at, c) in &now {
            while was.next_if(|(p, _)| (p.1, p.0) < (at.1, at.0)).is_some() {}
            if let Some((_, b)) = was.peek().filter(|(p, _)| p == at)
                && c.iter().zip(b).any(|(x, y)| x.abs_diff(*y) > 6)
            {
                changed.push((*at, *b, *c));
            }
        }
        anyhow::ensure!(
            changed.is_empty(),
            "{what}: {} pixels of the bar's middle changed, as {:?}",
            changed.len(),
            &changed[..changed.len().min(3)]
        );
        Ok(())
    }

    /// The wallpaper's frames: the bar's middle stays.
    fn wallpaper_frames(&mut self) -> anyhow::Result<()> {
        let before = self.middle(None)?;
        let ticks = self.hp.wall_ticks(OUTPUT);
        for _ in 0..40 {
            self.frame("the wallpaper's frames", &before, None)?;
        }
        anyhow::ensure!(self.hp.wall_ticks(OUTPUT) > ticks, "it did not move");
        Ok(())
    }

    /// A window dragged over the bar and back: beside it, the bar's
    /// middle stays.
    fn window_over(&mut self) -> anyhow::Result<()> {
        let window = common::open(&mut self.hp, "over", (500, 400))?;
        // Rained or thawed in.
        for _ in 0..60 {
            self.hp.step(STEP)?;
        }
        let before = self.middle(None)?;
        let held = (f64::from(window.x) + 100.0, f64::from(window.y) + 100.0);
        self.hp.pointer_move(held.0, held.1);
        self.hp.key(common::KEY_LEFTALT, true);
        self.hp.button(common::BTN_LEFT, true);
        let mut dx = 0.0;
        for i in 0..80 {
            dx += if i < 40 { 30.0 } else { -30.0 };
            self.hp.pointer_move(held.0 + dx, held.1);
            let hidden = common::rect_of(&self.hp, "over");
            self.frame("a window over the bar", &before, hidden)?;
        }
        self.hp.button(common::BTN_LEFT, false);
        self.hp.key(common::KEY_LEFTALT, false);
        for _ in 0..30 {
            let hidden = common::rect_of(&self.hp, "over");
            self.frame("the window let go", &before, hidden)?;
        }
        Ok(())
    }

    /// To the next desktop and back: the bar's middle stays, but for the
    /// rows a window sliding in and out may cross.
    fn desktop_switches(&mut self) -> anyhow::Result<()> {
        let hidden =
            common::rect_of(&self.hp, "over").map(|r| Rect::new(-10_000, r.y, 20_000, r.height));
        let before = self.middle(None)?;
        for key in [common::KEY_RIGHT, common::KEY_LEFT] {
            common::chord(
                &mut self.hp,
                &[common::KEY_LEFTMETA, common::KEY_LEFTCTRL, key],
            );
            for _ in 0..50 {
                self.frame("a desktop switch", &before, hidden)?;
            }
        }
        Ok(())
    }

    /// Written as the Settings write it: beside it, renamed over it.
    fn write(&self, text: &str) -> anyhow::Result<()> {
        let beside = self.dir.path().join("config/hocuspanda/.config.toml.new");
        std::fs::write(&beside, text)?;
        std::fs::rename(&beside, &self.file)?;
        Ok(())
    }
}

/// Givre over the aurora (10 frames a second).
#[test]
fn givres_bar_stays_whole_over_the_aurora() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("frost")? else {
        return Ok(());
    };
    bed.wallpaper_frames()?;
    bed.window_over()?;
    bed.desktop_switches()?;
    Ok(())
}

/// Matrix over the rain (15 frames a second), switched to from Givre:
/// through the switch, nothing of either wallpaper shows through the bar.
#[test]
fn matrixs_bar_stays_whole_over_the_rain() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("frost")? else {
        return Ok(());
    };
    let commits = bed.hp.bar_commits();
    bed.write(&file("matrix"))?;
    // Each frame until the shell draws in Matrix, and a second after.
    let start = std::time::Instant::now();
    let mut after = 0;
    while after < 60 {
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(30),
            "the shell never drew in Matrix"
        );
        bed.hp.step(STEP)?;
        if bed.hp.bar_commits() > commits {
            after += 1;
        }
        let now = bed.middle(None)?;
        let specks = bright(&now);
        anyhow::ensure!(
            specks.is_empty(),
            "the switch: the wallpaper shows through the bar at {specks:?}"
        );
    }
    // Matrix's near black over the rain.
    let middle = bed.middle(None)?;
    let dark = middle
        .iter()
        .filter(|(_, c)| c.iter().all(|v| *v < 40))
        .count();
    assert!(
        dark * 10 > middle.len() * 9,
        "the bar is not Matrix's: {dark} of {} dark",
        middle.len()
    );
    bed.wallpaper_frames()?;
    bed.window_over()?;
    bed.desktop_switches()?;
    Ok(())
}
