//! The theme and the accent (6.2, issue #402): `[appearance] theme` and
//! `accent` followed live, the whole desktop drawing in them. The real
//! shell and the real Settings on the CPU, headless, in French, their
//! configuration file, home, state and runtime directories the test's own;
//! the settings portal on a D-Bus daemon of the test's own, called as
//! xdg-desktop-portal calls it. No session bus otherwise, no GPU.
//!
//! One test per decision: switching the file to light draws the bar and
//! an open popover in light colours, live; the portal answers
//! `color-scheme` and signals its change; the accent changes the bar's
//! accent colour; the Settings window draws in the theme and follows the
//! file; and once the switch is drawn, nothing commits at rest. Givre
//! (#740): the bar in its night blue, its accent, nothing at rest; and a
//! theme switch draws one frame per screen. Each waits for what the
//! programs said or what the screen shows, never for a time.

use crate::common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Child,
    time::Duration,
};

use common::WAIT;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::TASKS_NAMESPACE;
use serde_json::Value;

/// The screen; the full bar 196 px wide, 8 px from its right edge.
const SCREEN: (i32, i32) = (1920, 1080);
const BAR_LEFT: i32 = 1920 - 8 - 196;
/// The date under the clock: a click opens the calendar.
const DATE: (f64, f64) = (1814.0, 82.0);

const FILE: &str = "# The test's own.\n[keyboard]\nlayouts = [\"us\"]\n";

/// A day of a fixture's, for the calendar: one calendar, today's meeting.
const DAY: &str = "[[calendars]]\nid = \"work\"\nname = \"Travail\"\ncolor = \"#8FB3FF\"\n\n\
                   [[events]]\ntitle = \"Point produit\"\nstart = \"today 15:00\"\nend = \"today 15:30\"\n";
const WITH_DAY: &str = "# The test's own.\n[keyboard]\nlayouts = [\"us\"]\n\n\
                        [[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    file: PathBuf,
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
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

/// One pixel of the screen, as red, green, blue.
fn pixel(hp: &mut Headless, (x, y): (i32, i32)) -> anyhow::Result<[u8; 3]> {
    let (width, _, bytes) = hp.screenshot(0)?;
    let at = usize::try_from(y * width + x)? * 4;
    let [b, g, r, _] = bytes
        .get(at..at + 4)
        .and_then(|p| <[u8; 4]>::try_from(p).ok())
        .ok_or_else(|| anyhow::anyhow!("no pixel at {x}, {y}"))?;
    Ok([r, g, b])
}

fn dark([r, g, b]: [u8; 3]) -> bool {
    r < 60 && g < 60 && b < 80
}

fn light([r, g, b]: [u8; 3]) -> bool {
    r > 215 && g > 215 && b > 215
}

/// Near `want`, channel by channel.
fn near(seen: [u8; 3], want: [u8; 3]) -> bool {
    seen.iter().zip(want).all(|(s, w)| s.abs_diff(w) <= 12)
}

impl Bed {
    /// The compositor with the real shell, its file holding `text`; `None`
    /// when the shell is not built.
    fn start(text: &str) -> anyhow::Result<Option<Self>> {
        Self::start_on(text, &[&format!("{}x{}", SCREEN.0, SCREEN.1)])
    }

    /// The same on the screens `outputs` (the first the bar's).
    fn start_on(text: &str, outputs: &[&str]) -> anyhow::Result<Option<Self>> {
        let shell = common::shell_program();
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "state", "cache", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, text)?;
        std::fs::write(root.join("config/hocuspanda/day.toml"), DAY)?;
        let log = root.join("shell.log");
        let script = root.join("shell");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 HOME='{}' XDG_CONFIG_HOME='{}' XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' \
                 TZ=UTC HOCUS_CLOCK_AT='2026-11-05T14:25:00' \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
                 PATH='{}:/usr/bin:/bin' RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("bin"),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file.clone());
        config.plain_background = true;
        config.lang = hocus_words::Lang::French;
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: outputs
                    .iter()
                    .map(|o| o.parse())
                    .collect::<Result<_, _>>()?,
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            config,
            Some(script),
        )?;
        let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        hp.pointer_move(400.0, 600.0);
        Ok(Some(Self { hp, dir, log, file }))
    }

    /// Writes the file as the Settings do: beside it, renamed over it.
    fn write(&self, text: &str) -> anyhow::Result<()> {
        let beside = self.dir.path().join("config/hocuspanda/.config.toml.new");
        std::fs::write(&beside, text)?;
        std::fs::rename(&beside, &self.file)?;
        Ok(())
    }

    /// Waits until the shell said it draws in `theme` with `accent`.
    fn themed(&mut self, theme: &str, accent: &str) -> anyhow::Result<()> {
        let log = self.log.clone();
        let said = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the theme theme="))
                .is_some_and(|l| {
                    l.contains(&format!("theme={theme} "))
                        && l.ends_with(&format!("accent={accent}"))
                })
        };
        let done = self.hp.dispatch_until(WAIT, |_| said(&plain(&log)))?;
        anyhow::ensure!(done, "the shell never drew in {theme} {accent}");
        Ok(())
    }

    /// Waits until the screen at `at` is `like`.
    fn until(&mut self, at: (i32, i32), like: impl Fn([u8; 3]) -> bool) -> anyhow::Result<()> {
        let mut seen = [0; 3];
        for _ in 0..400 {
            seen = pixel(&mut self.hp, at)?;
            if like(seen) {
                return Ok(());
            }
            self.hp.dispatch(Duration::from_millis(25))?;
        }
        anyhow::bail!("{at:?} shows {seen:?}")
    }

    /// The calendar, opened from the date once the day is read, drawn:
    /// its place.
    fn calendar(&mut self) -> anyhow::Result<Rect> {
        let log = self.log.clone();
        let read = self
            .hp
            .dispatch_until(WAIT, |_| plain(&log).contains("the day holds"))?;
        anyhow::ensure!(read, "the day was never read");
        // The clock's group lit under the pointer, then a click.
        let lit = |text: &str, from: usize| {
            text.get(from..).is_some_and(|t| {
                t.lines()
                    .any(|l| l.contains("the clock's group is drawn") && l.contains("hover: true"))
            })
        };
        let mut hovered = false;
        for _ in 0..50 {
            let from = plain(&log).len();
            common::point(&mut self.hp, (DATE.0, 500.0))?;
            common::wait(&mut self.hp, Duration::from_millis(50))?;
            common::point(&mut self.hp, DATE)?;
            common::wait(&mut self.hp, Duration::from_millis(100))?;
            if lit(&plain(&log), from) {
                hovered = true;
                break;
            }
        }
        anyhow::ensure!(hovered, "the clock's group never lit under the pointer");
        self.hp.button(common::BTN_LEFT, true);
        self.hp.button(common::BTN_LEFT, false);
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).len() == 1)?;
        anyhow::ensure!(
            open,
            "no calendar: {:?}\n{}",
            self.hp.popover_surfaces(),
            plain(&self.log)
        );
        common::at_rest(&mut self.hp)?;
        Ok(self.hp.popover_surfaces()[0].1)
    }

    /// Where the first-run card's `item` is on the screen, as last logged.
    fn card_item(&mut self, item: &str) -> anyhow::Result<(i32, i32)> {
        let wanted = format!(" {item}@");
        let log = self.log.clone();
        let found = |text: &str| {
            let line = text
                .lines()
                .rev()
                .find(|l| l.contains("the tasks' items"))?
                .replace("items=", "items= ");
            let at = line.split(&wanted).nth(1)?.split_whitespace().next()?;
            let (x, y) = at.split_once(',')?;
            Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| found(&plain(&log)).is_some())?;
        anyhow::ensure!(done, "no {item} on the card");
        let (x, y) = found(&plain(&log)).unwrap_or_default();
        let at = (f64::from(SCREEN.0) - 8.0 + x, 8.0 + y);
        let reached = self.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(TASKS_NAMESPACE)
                .first()
                .is_some_and(|(_, r)| {
                    at.0 >= f64::from(r.x) && at.0 < f64::from(r.right()) && at.1 >= f64::from(r.y)
                })
        })?;
        anyhow::ensure!(reached, "the card's surface is not there yet");
        Ok((at.0 as i32, at.1 as i32))
    }
}

/// A place of the bar's own background: inside its rim, left of its
/// groups' 12 px margin, half way down.
const BAR_BACKGROUND: (i32, i32) = (BAR_LEFT + 5, 540);

#[test]
fn switching_to_light_redraws_the_bar_and_the_calendar_in_light_live() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(WITH_DAY)? else {
        return Ok(());
    };
    bed.themed("dark", "green")?;
    bed.until(BAR_BACKGROUND, dark)?;
    let calendar = bed.calendar()?;
    let inside = (calendar.x + 6, calendar.y + calendar.height / 2);
    bed.until(inside, dark)?;
    // The file says light: the bar and the popover still open turn light,
    // without the session starting again.
    bed.write(&format!("{WITH_DAY}\n[appearance]\ntheme = \"light\"\n"))?;
    bed.themed("light", "green")?;
    bed.until(BAR_BACKGROUND, light)?;
    bed.until(inside, light)?;
    assert_eq!(
        common::drawn_popovers(&bed.hp).len(),
        1,
        "the calendar closed"
    );
    // Its text is dark on it: some pixel of the clock's digits is.
    let (width, _, shot) = bed.hp.screenshot(0)?;
    let darkest = (BAR_LEFT + 20..BAR_LEFT + 176)
        .flat_map(|x| (20..60).map(move |y| (x, y)))
        .map(|(x, y)| {
            let at = ((y * width + x) * 4) as usize;
            u32::from(shot[at]) + u32::from(shot[at + 1]) + u32::from(shot[at + 2])
        })
        .min()
        .unwrap_or(u32::MAX);
    assert!(darkest < 200, "no dark digit in the clock: {darkest}");
    // And back to dark.
    bed.write(WITH_DAY)?;
    bed.themed("dark", "green")?;
    bed.until(BAR_BACKGROUND, dark)?;
    bed.until(inside, dark)?;
    Ok(())
}

#[test]
fn the_accent_changes_the_bars_accent_colour() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    // The first-run card's "Choisir…" is the bar's accent at rest.
    let choose = bed.card_item("DayChoose")?;
    let at = (choose.0, choose.1 - 8);
    bed.until(at, |c| near(c, [0x6C, 0xC2, 0x87]))?;
    bed.write(&format!("{FILE}\n[appearance]\naccent = \"blue\"\n"))?;
    bed.themed("dark", "blue")?;
    bed.until(at, |c| near(c, [0x5B, 0x8D, 0xEF]))?;
    bed.write(&format!(
        "{FILE}\n[appearance]\naccent = \"rose\"\ntheme = \"light\"\n"
    ))?;
    bed.themed("light", "rose")?;
    bed.until(at, |c| near(c, [0xE0, 0x7A, 0xB5]))?;
    Ok(())
}

#[test]
fn once_the_switch_is_drawn_nothing_commits_at_rest() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.write(&format!("{FILE}\n[appearance]\ntheme = \"light\"\n"))?;
    bed.themed("light", "green")?;
    bed.until(BAR_BACKGROUND, light)?;
    common::at_rest(&mut bed.hp)?;
    // The clock draws its second; nothing else does.
    bed.hp.hold_clock();
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    let before = (
        bed.hp.bar_commits(),
        bed.hp.levels_commits(),
        bed.hp.desktops_commits(),
        bed.hp.tasks_commits(),
        bed.hp.popover_commits(),
    );
    common::wait(&mut bed.hp, Duration::from_millis(1500))?;
    let after = (
        bed.hp.bar_commits(),
        bed.hp.levels_commits(),
        bed.hp.desktops_commits(),
        bed.hp.tasks_commits(),
        bed.hp.popover_commits(),
    );
    assert_eq!(before, after, "the bar's surfaces drew at rest");
    Ok(())
}

/// Givre (#740): the bar's background its night blue (opaque on the CPU:
/// `#0E1A30` to `#08101F`), the accent its ice blue, the shell told; once
/// drawn, nothing commits at rest, each surface of the bar having drawn
/// once for the switch; back to dark, the dark bar.
#[test]
fn givre_draws_the_bar_in_night_blue_and_rests() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    // The first-run card's "Choisir…" is the bar's accent at rest.
    let choose = bed.card_item("DayChoose")?;
    let accent = (choose.0, choose.1 - 8);
    bed.themed("dark", "green")?;
    bed.until(BAR_BACKGROUND, dark)?;
    common::at_rest(&mut bed.hp)?;
    // Past the clock's next second, which draws on its own.
    bed.hp.hold_clock();
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    let commits = |hp: &Headless| {
        [
            hp.bar_commits(),
            hp.levels_commits(),
            hp.desktops_commits(),
            hp.tasks_commits(),
        ]
    };
    let before = commits(&bed.hp);
    bed.write(&format!(
        "{FILE}\n[appearance]\ntheme = \"frost\"\naccent = \"ice\"\n"
    ))?;
    bed.themed("frost", "ice")?;
    // Night blue: blue well over green, green over red (the dark theme's
    // slate is nearly grey).
    let night = |[r, g, b]: [u8; 3]| g >= r + 7 && b >= g + 12 && b < 60;
    bed.until(BAR_BACKGROUND, night)?;
    bed.until(accent, |c| near(c, [0x7F, 0xD8, 0xFF]))?;
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    let switched = commits(&bed.hp);
    for (i, (b, a)) in before.iter().zip(switched).enumerate() {
        assert_eq!(a - b, 1, "surface {i} drew {} times for the switch", a - b);
    }
    common::wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert_eq!(
        commits(&bed.hp),
        switched,
        "the bar's surfaces drew at rest"
    );
    bed.hp.release_clock();
    bed.write(FILE)?;
    bed.themed("dark", "green")?;
    bed.until(BAR_BACKGROUND, |c| dark(c) && !night(c))?;
    Ok(())
}

/// Matrix (#431, variant C): the bar's background near black with a
/// night-blue cast (opaque on the CPU, `#0C1218` to `#060A12`), never
/// green; the accent the phosphor green; its code edge down the bar's
/// left side, green glyphs; once drawn, nothing commits at rest.
#[test]
fn matrix_draws_the_bar_near_black_with_its_code_edge_and_rests() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let choose = bed.card_item("DayChoose")?;
    let accent = (choose.0, choose.1 - 8);
    bed.themed("dark", "green")?;
    bed.write(&format!(
        "{FILE}\n[appearance]\ntheme = \"matrix\"\naccent = \"phosphor\"\n"
    ))?;
    bed.themed("matrix", "phosphor")?;
    // Beside the code edge, the bar's own background.
    let background = (BAR_LEFT + 11, 540);
    let black = |[r, g, b]: [u8; 3]| b > g && g > r && b < 30 && g < 22;
    bed.until(background, black)?;
    bed.until(accent, |c| near(c, [0x00, 0xF0, 0x60]))?;
    common::at_rest(&mut bed.hp)?;
    // Green glyphs in the code edge, below the clock.
    let (width, _, shot) = bed.hp.screenshot(0)?;
    let green = (BAR_LEFT + 1..BAR_LEFT + 10)
        .flat_map(|x| (200..1000).map(move |y| (x, y)))
        .filter(|(x, y)| {
            let at = ((y * width + x) * 4) as usize;
            let [b, g, r] = [shot[at], shot[at + 1], shot[at + 2]];
            g > r.saturating_add(40) && g > b.saturating_add(20)
        })
        .count();
    if let Some(dir) = std::env::var_os("HOCUS_SHOTS") {
        let rgba: Vec<u8> = shot
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(
            width as u32,
            (shot.len() / 4 / width as usize) as u32,
            rgba,
        )
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
        let bar = image::imageops::crop_imm(&image, BAR_LEFT as u32 - 8, 0, 212, SCREEN.1 as u32);
        bar.to_image()
            .save(Path::new(&dir).join("matrix-bar-opaque.png"))?;
    }
    assert!(green > 20, "no code edge: {green} green pixels");
    bed.hp.hold_clock();
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    let commits = |hp: &Headless| [hp.bar_commits(), hp.levels_commits(), hp.tasks_commits()];
    let before = commits(&bed.hp);
    common::wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert_eq!(commits(&bed.hp), before, "the bar's surfaces drew at rest");
    Ok(())
}

/// A theme switch draws only where something is in the theme, and stops
/// (#740, the sprint's budgets): two screens, the bar on the first, to
/// Givre, light and back to dark. The second shows nothing themed (the
/// wallpaper alone): no frame there. The first draws as the shell's
/// surfaces commit, each once ([`givre_draws_the_bar_in_night_blue_and_rests`]):
/// on the CPU they land within one or two frames, not yet in exactly one
/// (that would take the compositor holding the screen's frame until the
/// shell's surfaces have all committed). How many frames those commits
/// fall into depends on the machine's load (4 on a busy CI runner): the
/// bound is one frame per surface of the bar, which a theme animating, or
/// drawing again and again, would pass.
/// The bar's surfaces: itself, the clock, the levels, the connections,
/// the desktops, the agents, the tasks and the footer.
const BAR_SURFACES: u64 = 8;

#[test]
fn a_theme_switch_draws_one_frame_per_screen() -> anyhow::Result<()> {
    // Without the clock's seconds, which draw on their own.
    let file = format!("{FILE}\n[panel.clock]\nseconds = false\n");
    let Some(mut bed) = Bed::start_on(&file, &["1920x1080", "1280x800"])? else {
        return Ok(());
    };
    // The first-run card set aside: the bar at rest.
    bed.themed("dark", "green")?;
    common::at_rest(&mut bed.hp)?;
    bed.hp.hold_clock();
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    for (theme, accent) in [
        ("frost", "ice"),
        ("matrix", "phosphor"),
        ("light", "green"),
        ("dark", "green"),
    ] {
        let before = [bed.hp.frames_on(0), bed.hp.frames_on(1)];
        bed.write(&format!(
            "{file}\n[appearance]\ntheme = \"{theme}\"\naccent = \"{accent}\"\n"
        ))?;
        bed.themed(theme, accent)?;
        let drawn = bed
            .hp
            .dispatch_until(WAIT, |hp| hp.frames_on(0) > before[0])?;
        anyhow::ensure!(drawn, "{theme}: the bar's screen not drawn");
        common::at_rest(&mut bed.hp)?;
        common::wait(&mut bed.hp, Duration::from_millis(500))?;
        let after = [bed.hp.frames_on(0), bed.hp.frames_on(1)];
        let drew: Vec<u64> = after.iter().zip(&before).map(|(a, b)| a - b).collect();
        assert_eq!(drew[1], 0, "{theme}: frames on the screen without the bar");
        assert!(
            (1..=BAR_SURFACES).contains(&drew[0]),
            "{theme}: {} frames on the bar's screen",
            drew[0]
        );
        eprintln!("{theme}: {drew:?} frames per screen");
    }
    Ok(())
}

/// High contrast (#423) reinforces the theme chosen: the bar's background
/// opaque and darker (lighter on light), no glass, the shell told with
/// the theme; switched off, the theme as it was.
#[test]
fn high_contrast_reinforces_the_theme_and_drops_the_glass() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&format!(
        "{FILE}\n[appearance]\nhigh-contrast = true\nreduce-transparency = false\n"
    ))?
    else {
        return Ok(());
    };
    bed.themed("dark", "green")?;
    let log = bed.log.clone();
    let told = |text: &str| {
        text.lines()
            .rev()
            .find(|l| l.contains("the theme theme="))
            .is_some_and(|l| l.contains(" contrast "))
    };
    assert!(told(&plain(&log)), "the shell was not told high contrast");
    assert!(!bed.hp.glass_on());
    bed.until(BAR_BACKGROUND, |c| near(c, [0x05, 0x07, 0x0C]))?;
    // A hover is a 1 px border in `text.dim` (#E2E6EE), not a fill: the
    // clock's group under the pointer, its edges in the row of the date.
    let row = |hp: &mut Headless| -> anyhow::Result<Vec<[u8; 3]>> {
        (BAR_LEFT + 2..BAR_LEFT + 194)
            .map(|x| pixel(hp, (x, DATE.1 as i32)))
            .collect()
    };
    let rims = |px: &[[u8; 3]]| px.iter().filter(|p| near(**p, [0xE2, 0xE6, 0xEE])).count();
    let before = row(&mut bed.hp)?;
    let mut seen = before.clone();
    for _ in 0..50 {
        common::point(&mut bed.hp, (DATE.0, 500.0))?;
        common::wait(&mut bed.hp, Duration::from_millis(50))?;
        common::point(&mut bed.hp, DATE)?;
        common::wait(&mut bed.hp, Duration::from_millis(150))?;
        seen = row(&mut bed.hp)?;
        if rims(&seen) >= rims(&before) + 2 {
            break;
        }
    }
    assert!(
        rims(&seen) >= rims(&before) + 2,
        "no hover's border: {seen:?}"
    );
    if let Some(dir) = std::env::var_os("HOCUS_SHOTS") {
        let (w, h, bytes) = bed.hp.screenshot(0)?;
        let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
        for p in bytes.chunks(4) {
            ppm.extend([p[2], p[1], p[0]]);
        }
        std::fs::write(Path::new(&dir).join("hover-dark-contrast.ppm"), ppm)?;
    }
    // Beside its left edge, inside, the background still: no fill.
    let edge = seen
        .iter()
        .position(|p| near(*p, [0xE2, 0xE6, 0xEE]))
        .unwrap_or_default();
    let inside = seen.get(edge + 8).copied().unwrap_or_default();
    assert!(near(inside, [0x05, 0x07, 0x0C]), "a fill: {inside:?}");
    bed.hp.pointer_move(400.0, 600.0);
    // Light, high contrast: white.
    bed.write(&format!(
        "{FILE}\n[appearance]\ntheme = \"light\"\nhigh-contrast = true\n"
    ))?;
    bed.themed("light", "green")?;
    bed.until(BAR_BACKGROUND, |c| near(c, [0xFF, 0xFF, 0xFF]))?;
    // Off: the light theme's own background.
    bed.write(&format!("{FILE}\n[appearance]\ntheme = \"light\"\n"))?;
    let off = bed.hp.dispatch_until(WAIT, |_| {
        plain(&log)
            .lines()
            .rev()
            .find(|l| l.contains("the theme theme="))
            .is_some_and(|l| !l.contains(" contrast "))
    })?;
    assert!(off, "high contrast never went");
    bed.until(BAR_BACKGROUND, |c| near(c, [0xF0, 0xF2, 0xF6]))?;
    Ok(())
}

// --- The Settings window ------------------------------------------------------

const APP_ID: &str = "hocus-settings";

/// The real Settings in `bed`, their log: `None` when they are not built.
fn settings(bed: &mut Bed) -> anyhow::Result<Option<PathBuf>> {
    let program = common::settings_program();
    let root = bed.dir.path();
    let log = root.join("settings.log");
    let script = root.join("bin/hocus-settings");
    let at = |sub: &str| root.join(sub).display().to_string();
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             HOCUS_POWER_SUPPLY_DIR=/nonexistent ICED_BACKEND=tiny-skia \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
             XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' PATH='{}:/usr/bin:/bin' \
             HOCUS_USER_NAME=Alex RUST_LOG=warn,hocus_settings=debug\n\
             exec '{}' \"$@\" 2>>'{}'\n",
            at("home"),
            at("config"),
            at("cache"),
            at("state"),
            at("bin"),
            program.display(),
            log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    bed.hp.launch(hocus_link::Launch {
        id: APP_ID.to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec![script.display().to_string(), "appearance".to_owned()],
    });
    let open = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
    })?;
    anyhow::ensure!(open, "the window never opened: {}", plain(&log));
    Ok(Some(log))
}

/// What the window shows, once `done` holds of it.
fn shows(bed: &mut Bed, log: &Path, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
    let read = |log: &Path| -> Option<Value> {
        let text = plain(log);
        let line = text.lines().rev().find(|l| l.contains(" shows {"))?;
        let at = line.find(" shows {")? + " shows ".len();
        serde_json::from_str(&line[at..]).ok()
    };
    let seen = bed
        .hp
        .dispatch_until(WAIT, |_| read(log).is_some_and(|s| done(&s)))?;
    let last = read(log);
    anyhow::ensure!(seen, "the window never showed it; it shows {last:?}");
    last.ok_or_else(|| anyhow::anyhow!("nothing shown"))
}

#[test]
fn the_settings_window_draws_in_the_theme_and_follows_the_file() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let Some(log) = settings(&mut bed)? else {
        return Ok(());
    };
    shows(&mut bed, &log, |s| {
        s["file"] == "saved" && s["theme"] == "dark"
    })?;
    let window = bed
        .hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(APP_ID))
        .map(|w| w.rect)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    // The sidebar and the content, low in the window where nothing is.
    let sidebar = (window.x + 120, window.bottom() - 12);
    let content = (window.x + 600, window.bottom() - 12);
    bed.until(sidebar, |c| near(c, [0x0F, 0x13, 0x1C]))?;
    bed.until(content, |c| near(c, [0x14, 0x19, 0x26]))?;
    bed.write(&format!("{FILE}\n[appearance]\ntheme = \"light\"\n"))?;
    shows(&mut bed, &log, |s| s["theme"] == "light")?;
    bed.until(sidebar, |c| near(c, [0xE9, 0xEC, 0xF2]))?;
    bed.until(content, |c| near(c, [0xF5, 0xF6, 0xF9]))?;
    // And the bar with it.
    bed.themed("light", "green")?;
    bed.until(BAR_BACKGROUND, light)?;
    Ok(())
}

// --- The settings portal ------------------------------------------------------

const PORTAL: &str = "org.freedesktop.impl.portal.desktop.hocuspanda";
const PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.impl.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";

struct Portal {
    hp: Headless,
    file: PathBuf,
    runtime: tokio::runtime::Runtime,
    client: zbus::Connection,
    /// The `SettingChanged` heard: namespace, key, value.
    heard: std::sync::Arc<std::sync::Mutex<Vec<(String, String, zbus::OwnedVariant)>>>,
    /// Last, so stopped once the compositor and its connections are.
    bus: Child,
    _dir: tempfile::TempDir,
}

impl Drop for Portal {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

impl Portal {
    /// A compositor serving the portal on a bus of the test's own; `None`
    /// without dbus-daemon.
    fn start() -> anyhow::Result<Option<Self>> {
        let has = std::process::Command::new("sh")
            .args(["-c", "command -v dbus-daemon"])
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !has {
            common::missing("dbus-daemon");
        }
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("config.toml");
        std::fs::write(&file, FILE)?;
        let (bus, address) = common::private_bus(dir.path())?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file.clone());
        let hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            config,
            None,
            Some(address.clone()),
        )?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let client =
            runtime.block_on(zbus::connection::Builder::address(address.as_str()).build())?;
        let heard = std::sync::Arc::default();
        runtime.block_on(listen(&client, std::sync::Arc::clone(&heard)))?;
        let mut portal = Self {
            hp,
            file,
            runtime,
            client,
            heard,
            bus,
            _dir: dir,
        };
        portal.served()?;
        Ok(Some(portal))
    }

    fn served(&mut self) -> anyhow::Result<()> {
        for _ in 0..400 {
            let client = self.client.clone();
            let owned = self.runtime.block_on(async move {
                let dbus = zbus::fdo::DBusProxy::new(&client).await?;
                anyhow::Ok(dbus.name_has_owner(PORTAL.try_into()?).await?)
            })?;
            if owned {
                return Ok(());
            }
            self.hp.dispatch(Duration::from_millis(25))?;
        }
        anyhow::bail!("the portal is not served")
    }

    fn read_all(&mut self) -> anyhow::Result<HashMap<String, HashMap<String, zbus::OwnedVariant>>> {
        let client = self.client.clone();
        self.runtime.block_on(async move {
            let reply = client
                .call_method(
                    Some(PORTAL),
                    PATH,
                    Some(SETTINGS),
                    "ReadAll",
                    &(vec!["org.freedesktop.*"],),
                )
                .await?;
            anyhow::Ok(reply.body().deserialize()?)
        })
    }

    fn read(&mut self, key: &str) -> anyhow::Result<zbus::OwnedVariant> {
        let client = self.client.clone();
        let key = key.to_owned();
        self.runtime.block_on(async move {
            let reply = client
                .call_method(
                    Some(PORTAL),
                    PATH,
                    Some(SETTINGS),
                    "Read",
                    &(APPEARANCE, key),
                )
                .await?;
            anyhow::Ok(reply.body().deserialize()?)
        })
    }

    /// Waits until `SettingChanged` said `key` is `value`.
    fn changed(&mut self, key: &str, value: u32) -> anyhow::Result<()> {
        let heard = std::sync::Arc::clone(&self.heard);
        let said = self.hp.dispatch_until(WAIT, |_| {
            heard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .any(|(n, k, v)| {
                    n == APPEARANCE && k == key && u32::try_from(&**v).ok() == Some(value)
                })
        })?;
        anyhow::ensure!(said, "no SettingChanged {key} = {value}: {:?}", self.heard);
        Ok(())
    }
}

async fn listen(
    client: &zbus::Connection,
    heard: std::sync::Arc<std::sync::Mutex<Vec<(String, String, zbus::OwnedVariant)>>>,
) -> anyhow::Result<()> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(SETTINGS)
        .build()?;
    let stream = zbus::MessageStream::for_match_rule(rule, client, None).await?;
    tokio::spawn(async move {
        use zbus::export::futures_core::Stream;
        let mut stream = Box::pin(stream);
        while let Some(Ok(message)) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
        {
            if let Ok(said) = message
                .body()
                .deserialize::<(String, String, zbus::OwnedVariant)>()
            {
                heard
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(said);
            }
        }
    });
    Ok(())
}

#[test]
fn the_portal_answers_color_scheme_and_signals_its_change() -> anyhow::Result<()> {
    let Some(mut portal) = Portal::start()? else {
        return Ok(());
    };
    // Dark, the pandas' green, no contrast.
    let all = portal.read_all()?;
    let appearance = all
        .get(APPEARANCE)
        .ok_or_else(|| anyhow::anyhow!("no {APPEARANCE}: {all:?}"))?;
    assert_eq!(u32::try_from(&*appearance["color-scheme"]).ok(), Some(1));
    assert_eq!(u32::try_from(&*appearance["contrast"]).ok(), Some(0));
    let accent = <(f64, f64, f64)>::try_from(portal.read("accent-color")?)?;
    assert_eq!(accent, (0.424, 0.761, 0.529));
    assert!(portal.read("no-such-key").is_err());
    // The file says light, blue and high contrast: each is signalled, and
    // read back.
    std::fs::write(
        &portal.file,
        format!(
            "{FILE}\n[appearance]\ntheme = \"light\"\naccent = \"blue\"\nhigh-contrast = true\n"
        ),
    )?;
    portal.changed("color-scheme", 2)?;
    portal.changed("contrast", 1)?;
    assert_eq!(
        u32::try_from(portal.read("color-scheme")?).ok(),
        Some(2),
        "Read says the new value"
    );
    let accent = <(f64, f64, f64)>::try_from(portal.read("accent-color")?)?;
    assert_eq!(accent, (0.357, 0.553, 0.937));
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`: the bar with its
/// calendar open, in dark then light, and the bar's accent in blue.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let save = |bed: &mut Bed, name: &str, (x, y, w, h): (i32, i32, i32, i32)| {
        common::at_rest(&mut bed.hp)?;
        let (width, height, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(width as u32, height as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        let image =
            image::imageops::crop_imm(&image, x as u32, y as u32, w as u32, h as u32).to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        anyhow::Ok(())
    };
    // The bar and its calendar beside it.
    let crop = (BAR_LEFT - 8 - 320 - 16, 0, 196 + 8 + 320 + 24, 720);
    let Some(mut bed) = Bed::start(WITH_DAY)? else {
        return Ok(());
    };
    bed.themed("dark", "green")?;
    bed.calendar()?;
    save(&mut bed, "bar-calendar-dark.png", crop)?;
    bed.write(&format!("{WITH_DAY}\n[appearance]\ntheme = \"light\"\n"))?;
    bed.themed("light", "green")?;
    bed.until(BAR_BACKGROUND, light)?;
    save(&mut bed, "bar-calendar-light.png", crop)?;
    // The first-run card's "Choisir…", green then blue, on light.
    let Some(mut bed) = Bed::start(&format!("{FILE}\n[appearance]\ntheme = \"light\"\n"))? else {
        return Ok(());
    };
    let choose = bed.card_item("DayChoose")?;
    let card = (BAR_LEFT - 8, 0, 196 + 16, choose.1 + 40);
    bed.themed("light", "green")?;
    save(&mut bed, "card-light-green.png", card)?;
    bed.write(&format!(
        "{FILE}\n[appearance]\ntheme = \"light\"\naccent = \"blue\"\n"
    ))?;
    bed.themed("light", "blue")?;
    bed.until((choose.0, choose.1 - 8), |c| near(c, [0x5B, 0x8D, 0xEF]))?;
    save(&mut bed, "card-light-blue.png", card)?;
    bed.write(&format!("{FILE}\n[appearance]\naccent = \"blue\"\n"))?;
    bed.themed("dark", "blue")?;
    save(&mut bed, "card-dark-blue.png", card)?;
    // The lock screen, light.
    bed.write(&format!("{FILE}\n[appearance]\ntheme = \"light\"\n"))?;
    bed.themed("light", "green")?;
    bed.hp.lock();
    let log = bed.log.clone();
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |_| plain(&log).contains("the lock is drawn"))?;
    anyhow::ensure!(drawn, "no lock");
    save(&mut bed, "lock-light.png", (0, 0, SCREEN.0, SCREEN.1))?;
    Ok(())
}
