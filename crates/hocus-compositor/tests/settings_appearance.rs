//! Apparence et langue (6.2, issue #402, variant A): the real
//! `hocus-settings` window and the real shell, clients of a headless
//! compositor, their configuration file, home, state and runtime
//! directories the test's own, no bus, no GPU. The shell's clock reads
//! Thursday 5 November 2026, 14:25, in UTC.
//!
//! One test per decision of the page: its five groups; choosing light
//! writes `[appearance] theme` and the whole desktop follows (the window,
//! the bar); a swatch writes `[appearance] accent`; 12 hours shows "2:25
//! PM" in the bar, the calendar and the lock; the language switch turns
//! the shell's and the window's words at once; at rest nothing draws; and
//! Givre (#740) sets its accent with it, in one toast that undoes both,
//! the window staying dark.
//! What the programs show is read from their logs, never on a time.

use crate::common;
use crate::settings_page;

use settings_page::{Floor, Wheel};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, WAIT, wait};
use hocus_compositor::{Authenticator, Config, Headless, HeadlessConfig, RendererChoice};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const SCREEN: (i32, i32) = (1920, 1080);
const BAR_LEFT: i32 = 1920 - 8 - 196;
/// The date under the clock: a click opens the calendar.
const DATE: (f64, f64) = (1814.0, 82.0);

const FILE: &str = "# My desktop, by hand.\n[keyboard]\nlayouts = [\"us\"]\n\n\
                    [[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";
/// One calendar, a meeting at 15:00.
const DAY: &str = "[[calendars]]\nid = \"work\"\nname = \"Travail\"\ncolor = \"#8FB3FF\"\n\n\
                   [[events]]\ntitle = \"Point produit\"\nstart = \"today 15:00\"\nend = \"today 15:30\"\n";

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    shell_log: PathBuf,
    log: PathBuf,
    script: PathBuf,
}

/// A log without its colours' escape sequences.
fn plain(log: &Path) -> String {
    common::logged(log)
}

impl Bed {
    /// The compositor, the shell and the Settings' script, the file
    /// holding `text`; `None` when they are not built.
    fn start(text: &str) -> anyhow::Result<Option<Self>> {
        let (shell, settings) = (common::shell_program(), common::settings_program());
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "state", "cache", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, text)?;
        std::fs::write(root.join("config/hocuspanda/day.toml"), DAY)?;
        let at = |sub: &str| root.join(sub).display().to_string();
        let shell_log = root.join("shell.log");
        let shell_script = root.join("shell");
        std::fs::write(
            &shell_script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 HOME='{}' XDG_CONFIG_HOME='{}' XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' \
                 XDG_DATA_HOME='{}' TZ=UTC HOCUS_CLOCK_AT='2026-11-05T14:25:00' \
                 HOCUS_USER_NAME=Alex LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
                 PATH='{}:/usr/bin:/bin' RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("bin"),
                shell.display(),
                shell_log.display()
            ),
        )?;
        std::fs::set_permissions(&shell_script, std::fs::Permissions::from_mode(0o755))?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 XDG_DATA_DIRS='{}' XCURSOR_PATH='{}' PATH='{}:/usr/bin:/bin' \
                 HOCUS_USER_NAME=Alex RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("data"),
                at("data"),
                at("bin"),
                settings.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.plain_background = true;
        config.lang = hocus_words::Lang::French;
        config.lock_auth = Authenticator::test("not a password");
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec![format!("{}x{}", SCREEN.0, SCREEN.1).parse()?],
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            config,
            Some(shell_script),
        )?;
        let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        hp.pointer_move(400.0, 600.0);
        Ok(Some(Self {
            hp,
            dir,
            shell_log,
            log,
            script,
        }))
    }

    fn file(&self) -> PathBuf {
        self.dir.path().join("config/hocuspanda/config.toml")
    }

    /// Writes the file as an editor does: beside it, renamed over it.
    fn write(&self, text: &str) -> anyhow::Result<()> {
        let beside = self.dir.path().join("config/hocuspanda/.config.toml.new");
        std::fs::write(&beside, text)?;
        std::fs::rename(&beside, self.file())?;
        Ok(())
    }

    /// Launched on the appearance page, its window open with the keyboard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.open_on("appearance")
    }

    /// Launched on page `page`, its window open with the keyboard.
    fn open_on(&mut self, page: &str) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), page.to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", plain(&self.log));
        self.shows(|s| s["file"] == "saved" && s["page"] == page)
    }

    /// What the window shows, once `done` holds of it.
    fn shows(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> { common::last_json(log, " shows {") };
        // What held, not what is read again after: the window may have
        // said more meanwhile.
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = read(&log).filter(|s| done(s));
            seen.is_some()
        })?;
        seen.ok_or_else(|| {
            anyhow::anyhow!(
                "the window never showed it; it shows {}",
                read(&log).map_or_else(|| "nothing".to_owned(), |s| s.to_string())
            )
        })
    }

    /// The window's current places, once they hold `id`.
    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        settings_page::places_with(&mut self.hp, &self.log, id)
    }

    fn window(&self) -> anyhow::Result<hocus_compositor::model::Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    /// `id` scrolled into the window's upper part with the wheel, over the
    /// page: a menu under it opens inside the window.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        let wheel = Wheel {
            x: 900.0,
            notches: 2,
            top: 56.0,
            floor: Floor::Margin(16.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        if !id.starts_with("option:") && id != "toast-action" {
            self.reveal(id)?;
        }
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        let window = self.window()?;
        let at = (
            f64::from(window.x) + n(0) + n(2) / 2.0,
            f64::from(window.y) + n(1) + n(3) / 2.0,
        );
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    /// Waits for the file to hold `done`.
    fn written(&mut self, done: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let file = self.file();
        let seen = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| done(&t))
        })?;
        let text = std::fs::read_to_string(self.file()).unwrap_or_default();
        anyhow::ensure!(seen, "the file never held it:\n{text}");
        Ok(text)
    }

    /// Waits until the shell's last line holding `what` holds all of
    /// `holds`: that line.
    fn shell_said(&mut self, what: &str, holds: &[&str]) -> anyhow::Result<String> {
        let log = self.shell_log.clone();
        let last = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains(what))
                .filter(|l| holds.iter().all(|h| l.contains(h)))
                .map(str::to_owned)
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| last(&plain(&log)).is_some())?;
        let text = plain(&self.shell_log);
        anyhow::ensure!(
            done,
            "the shell's last {what:?} never held {holds:?}: {:?}",
            text.lines().rev().find(|l| l.contains(what))
        );
        Ok(last(&text).unwrap_or_default())
    }

    /// The calendar, opened from the date once the day is read: the clock's
    /// group lit under the pointer, then a click.
    fn open_calendar(&mut self) -> anyhow::Result<()> {
        let log = self.shell_log.clone();
        let read = self
            .hp
            .dispatch_until(WAIT, |_| plain(&log).contains("the day holds"))?;
        anyhow::ensure!(read, "the day was never read");
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
            wait(&mut self.hp, Duration::from_millis(50))?;
            common::point(&mut self.hp, DATE)?;
            wait(&mut self.hp, Duration::from_millis(100))?;
            if lit(&plain(&log), from) {
                hovered = true;
                break;
            }
        }
        anyhow::ensure!(hovered, "the clock's group never lit under the pointer");
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn pixel(&mut self, (x, y): (i32, i32)) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let [b, g, r, _] = bytes
            .get(at..at + 4)
            .and_then(|p| <[u8; 4]>::try_from(p).ok())
            .ok_or_else(|| anyhow::anyhow!("no pixel at {x}, {y}"))?;
        Ok([r, g, b])
    }

    /// Waits until the screen at `at` is `like`.
    fn until(&mut self, at: (i32, i32), like: impl Fn([u8; 3]) -> bool) -> anyhow::Result<()> {
        let mut seen = [0; 3];
        for _ in 0..400 {
            seen = self.pixel(at)?;
            if like(seen) {
                return Ok(());
            }
            self.hp.dispatch(Duration::from_millis(25))?;
        }
        anyhow::bail!("{at:?} shows {seen:?}")
    }
}

fn row(shown: &Value, id: &str) -> Value {
    shown["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

fn light([r, g, b]: [u8; 3]) -> bool {
    r > 215 && g > 215 && b > 215
}

#[test]
fn the_page_shows_its_five_groups_and_choosing_light_turns_the_desktop_light() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let ids: Vec<&str> = shown["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "appearance.theme",
            "appearance.accent",
            "appearance.text-scale",
            "appearance.cursor-theme",
            "appearance.cursor-size",
            "appearance.accessibility",
            "locale.language",
            "locale.date",
            "locale.time",
            "appearance.bar-clock",
        ]
    );
    assert_eq!(shown["theme"], "dark");
    assert_eq!(row(&shown, "appearance.theme")["value"], "Sombre");
    // Clair: written at once, the comments and the rest kept; the window
    // and the bar turn light.
    bed.click("segment:appearance.theme:1")?;
    let text = bed.written(|t| t.contains("theme = \"light\""))?;
    assert!(
        text.starts_with("# My desktop, by hand.\n[keyboard]"),
        "{text}"
    );
    let shown = bed.shows(|s| s["theme"] == "light")?;
    assert_eq!(row(&shown, "appearance.theme")["value"], "Clair");
    bed.shell_said("the theme theme=", &["theme=light"])?;
    bed.until((BAR_LEFT + 5, 540), light)?;
    let window = bed.window()?;
    bed.until((window.x + 600, window.bottom() - 12), light)?;
    Ok(())
}

/// The Accessibility page (#422–#424): the magnifier, the view and the
/// keyboard's groups; high contrast written at once, the desktop drawing
/// it, and reduced transparency shown on and inert while it is, the file
/// keeping its own value.
#[test]
fn the_accessibility_page_holds_the_view_and_high_contrast_holds_transparency_on()
-> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open_on("accessibility")?;
    let ids: Vec<&str> = shown["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "magnifier.enabled",
            "magnifier.zoom",
            "magnifier.follow-pointer",
            "magnifier.follow-keyboard",
            "magnifier.smooth",
            "appearance.high-contrast",
            "appearance.reduce-transparency",
            "appearance.reduced-motion",
            "accessibility.text-and-cursor",
            "accessibility.screen-reader",
            "accessibility.focus-bar",
        ]
    );
    assert_eq!(
        row(&shown, "appearance.reduce-transparency")["inert"],
        false
    );
    bed.click("control:appearance.high-contrast")?;
    let text = bed.written(|t| t.contains("high-contrast = true"))?;
    assert!(!text.contains("reduce-transparency"), "{text}");
    let shown = bed.shows(|s| row(s, "appearance.high-contrast")["value"] == "oui")?;
    let transparency = row(&shown, "appearance.reduce-transparency");
    assert_eq!(transparency["inert"], true, "{transparency}");
    assert_eq!(transparency["implied"], true, "{transparency}");
    // The bar draws it.
    bed.shell_said("the theme theme=", &[" contrast "])?;
    bed.until((BAR_LEFT + 5, 540), |[r, g, b]| r < 12 && g < 12 && b < 20)?;
    // The screen reader's switch writes its key (#734); nested, the
    // compositor starts nothing.
    bed.click("control:accessibility.screen-reader")?;
    let text = bed.written(|t| t.contains("screen-reader = true"))?;
    assert!(text.contains("[accessibility]"), "{text}");
    bed.shows(|s| row(s, "accessibility.screen-reader")["value"] == "oui")?;
    Ok(())
}

#[test]
fn a_swatch_writes_the_accent_and_the_row_says_its_name() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("swatch:violet")?;
    bed.written(|t| t.contains("accent = \"violet\""))?;
    let shown = bed.shows(|s| s["accent"] == "violet")?;
    assert_eq!(row(&shown, "appearance.accent")["value"], "Violet");
    bed.shell_said("the theme theme=", &["accent=violet"])?;
    Ok(())
}

/// Givre (#740): its segment writes `theme = "frost"`, its accent (the
/// ice blue), its windows' motion and the aurora as one change; one toast
/// says them all, and its "Annuler" takes them all back, the violet
/// chosen before with them. The window stays
/// dark, in the ice blue; the bar draws Givre.
#[test]
fn choosing_givre_sets_its_accent_motion_and_aurora_in_one_toast() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&format!("{FILE}\n[appearance]\naccent = \"violet\"\n"))? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["accent"], "violet");
    let window = bed.window()?;
    // The sidebar's foot, under its last section, away from the toast.
    let corner = (window.x + 20, window.bottom() - 40);
    bed.click("segment:appearance.theme:2")?;
    let text = bed.written(|t| {
        t.contains("theme = \"frost\"")
            && t.contains("accent = \"ice\"")
            && t.contains("animation = \"frost\"")
            && t.contains("picture = \"aurora\"")
    })?;
    assert!(!text.contains("violet"), "{text}");
    let shown = bed.shows(|s| s["theme"] == "frost" && s["accent"] == "ice")?;
    assert_eq!(row(&shown, "appearance.theme")["value"], "Givre");
    let shown = bed.shows(|s| {
        s["toast"]
            == "Thème : Givre · Couleur d’accent : Bleu glacé · Ouvrir et fermer : Givre · Fond d’écran : Aurore"
            && s["toast_action"] == "undo"
    })?;
    assert_eq!(row(&shown, "appearance.accent")["value"], "Bleu glacé");
    bed.shell_said("the theme theme=", &["theme=frost", "accent=ice"])?;
    // The window keeps the dark look: its sidebar the dark one's.
    let sidebar = [0x0F, 0x13, 0x1C];
    bed.until(corner, |c| c == sidebar)?;
    wait(&mut bed.hp, Duration::from_millis(200))?;
    assert_eq!(bed.pixel(corner)?, sidebar, "the window left the dark look");
    // For the maintainer's eyes, with `HOCUS_SHOTS`: the window and the
    // bar, the toast showing.
    if let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) {
        common::at_rest(&mut bed.hp)?;
        let (w, h, bytes) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        image.save(out.join("givre-settings.png"))?;
    }
    // Annuler: all back.
    bed.click("toast-action")?;
    let text = bed.written(|t| !t.contains("frost") && t.contains("accent = \"violet\""))?;
    assert!(
        !text.contains("animation") && !text.contains("aurora"),
        "{text}"
    );
    assert!(!text.contains("\"ice\""), "{text}");
    bed.shows(|s| s["theme"] == "dark" && s["accent"] == "violet")?;
    bed.shell_said("the theme theme=", &["theme=dark", "accent=violet"])?;
    Ok(())
}

/// Matrix (#431): its segment writes the theme, its accent, its windows'
/// rain and the rain as the wallpaper (over a built-in one) as one
/// change, one toast saying them all; "Annuler" takes all four back.
/// Choosing Sombre afterwards gives back what Matrix replaced.
#[test]
fn choosing_matrix_sets_its_accent_rain_and_wallpaper_in_one_toast() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&format!(
        "{FILE}\n[appearance]\naccent = \"teal\"\n\n[wallpaper]\npicture = \"dawn\"\n"
    ))?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.click("segment:appearance.theme:3")?;
    let all = |t: &str| {
        t.contains("theme = \"matrix\"")
            && t.contains("accent = \"phosphor\"")
            && t.contains("animation = \"rain\"")
            && t.contains("picture = \"rain\"")
    };
    bed.written(all)?;
    let shown = bed.shows(|s| s["theme"] == "matrix" && s["accent"] == "phosphor")?;
    assert_eq!(row(&shown, "appearance.theme")["value"], "Matrix");
    bed.shows(|s| {
        s["toast"]
            == "Thème : Matrix · Couleur d’accent : Vert phosphore · Ouvrir et fermer : Pluie · Fond d’écran : Pluie"
            && s["toast_action"] == "undo"
    })?;
    bed.shell_said("the theme theme=", &["theme=matrix", "accent=phosphor"])?;
    // The window keeps the dark look: its sidebar the dark one's.
    let window = bed.window()?;
    let corner = (window.x + 20, window.bottom() - 40);
    bed.until(corner, |c| c == [0x0F, 0x13, 0x1C])?;
    if let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) {
        common::at_rest(&mut bed.hp)?;
        wait(&mut bed.hp, Duration::from_millis(500))?;
        let (w, h, bytes) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        image.save(out.join("matrix-settings.png"))?;
    }
    // Annuler: all four back.
    bed.click("toast-action")?;
    let text = bed.written(|t| {
        !t.contains("matrix") && t.contains("accent = \"teal\"") && t.contains("\"dawn\"")
    })?;
    assert!(!text.contains("animation"), "{text}");
    bed.shows(|s| s["theme"] == "dark" && s["accent"] == "teal")?;
    // Matrix again, then Sombre: what it replaced comes back.
    wait(&mut bed.hp, Duration::from_millis(200))?;
    bed.click("segment:appearance.theme:3")?;
    bed.written(all)?;
    bed.shows(|s| s["theme"] == "matrix")?;
    bed.click("segment:appearance.theme:0")?;
    let text = bed.written(|t| {
        !t.contains("matrix") && t.contains("accent = \"teal\"") && t.contains("\"dawn\"")
    })?;
    assert!(!text.contains("animation"), "{text}");
    bed.shows(|s| {
        s["toast"]
            == "Thème : Sombre · Couleur d’accent : Sarcelle · Ouvrir et fermer : Standard · Fond d’écran : Aube"
    })?;
    Ok(())
}

#[test]
fn twelve_hours_shows_2_25_pm_in_the_bar_the_calendar_and_the_lock() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.shell_said("the clock shows", &["time=\"14:25\""])?;
    // The page's segment, as a user does.
    bed.open()?;
    bed.click("segment:locale.time:1")?;
    bed.written(|t| t.contains("[locale]") && t.contains("time = \"12h\""))?;
    bed.shell_said("the clock shows", &["time=\"2:25 PM\""])?;
    // The calendar's meeting at 3:00 PM.
    bed.open_calendar()?;
    let line = bed.shell_said("the calendar shows it", &["Point produit"])?;
    assert!(line.contains("3:00 PM"), "{line}");
    assert!(!line.contains("15:00"), "{line}");
    // The lock's clock.
    bed.hp.key(1, true);
    bed.hp.key(1, false);
    bed.hp.lock();
    bed.shell_said("the lock is drawn", &["hm: \"2:25 PM\""])?;
    Ok(())
}

#[test]
fn the_language_switch_turns_the_shell_and_the_window_to_english_at_once() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.shell_said("the clock shows", &["date=\"jeudi 5 novembre\""])?;
    let shown = bed.open()?;
    assert_eq!(shown["lang"], "fr");
    assert_eq!(row(&shown, "locale.language")["value"], "Celle du système");
    // English, from the menu.
    bed.click("control:locale.language")?;
    bed.shows(|s| s["menu"] == "locale.language")?;
    bed.click("option:2")?;
    bed.written(|t| t.contains("language = \"en\""))?;
    let shown = bed.shows(|s| s["lang"] == "en")?;
    assert_eq!(row(&shown, "locale.language")["value"], "English");
    assert_eq!(row(&shown, "appearance.theme")["value"], "Dark");
    bed.shell_said("the clock shows", &["date=\"Thursday 5 November\""])?;
    // And the date's format, by hand in the file.
    let text = std::fs::read_to_string(bed.file())?;
    bed.write(&text.replace("language = \"en\"", "language = \"en\"\ndate = \"iso\""))?;
    bed.shell_said("the clock shows", &["date=\"2026-11-05\""])?;
    bed.shows(|s| row(s, "locale.date")["value"] == "Year first")?;
    Ok(())
}

#[test]
fn at_rest_the_appearance_page_and_the_bar_commit_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("segment:appearance.theme:1")?;
    bed.shows(|s| s["theme"] == "light")?;
    bed.shell_said("the theme theme=", &["theme=light"])?;
    bed.hp.pointer_move(400.0, 1000.0);
    common::at_rest(&mut bed.hp)?;
    bed.hp.hold_clock();
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let counts = |hp: &Headless| (hp.window_commits(), hp.bar_commits(), hp.levels_commits());
    let before = counts(&bed.hp);
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert_eq!(counts(&bed.hp), before, "something drew at rest");
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`: the bar and its
/// calendar in dark and light, the page, an accent change.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let save =
        |bed: &mut Bed, name: &str, crop: Option<(i32, i32, i32, i32)>| -> anyhow::Result<()> {
            common::at_rest(&mut bed.hp)?;
            // The window drawn, and still for half a second.
            let mut last = bed.hp.window_commits();
            let mut still = 0;
            for _ in 0..40 {
                if still >= 5 {
                    break;
                }
                wait(&mut bed.hp, Duration::from_millis(100))?;
                let now = bed.hp.window_commits();
                still = if now == last { still + 1 } else { 0 };
                last = now;
            }
            let (w, h, bytes) = bed.hp.screenshot(0)?;
            let (x0, y0, cw, ch) = crop.unwrap_or((0, 0, w, h));
            let rgba: Vec<u8> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], 255])
                .collect();
            let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
                .ok_or_else(|| anyhow::anyhow!("no image"))?;
            let image =
                image::imageops::crop_imm(&image, x0 as u32, y0 as u32, cw as u32, ch as u32)
                    .to_image();
            let path = out.join(name);
            image.save(&path)?;
            eprintln!("{}", path.display());
            Ok(())
        };
    let bar_crop = Some((1920 - 8 - 196 - 8 - 320 - 12, 0, 196 + 8 + 320 + 28, 1080));
    bed.open_calendar()?;
    bed.shell_said("the calendar shows it", &["Point produit"])?;
    save(&mut bed, "bar-calendar-dark.png", bar_crop)?;
    bed.write(&format!("{FILE}\n[appearance]\ntheme = \"light\"\n"))?;
    bed.shell_said("the theme theme=", &["theme=light"])?;
    bed.until((BAR_LEFT + 5, 540), light)?;
    save(&mut bed, "bar-calendar-light.png", bar_crop)?;
    // The page, in a session of its own.
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    save(&mut bed, "page-dark.png", None)?;
    bed.click("swatch:blue")?;
    bed.shows(|s| s["accent"] == "blue")?;
    bed.hp.pointer_move(400.0, 1060.0);
    save(&mut bed, "page-dark-blue.png", None)?;
    bed.click("segment:appearance.theme:1")?;
    bed.shows(|s| s["theme"] == "light")?;
    bed.hp.pointer_move(400.0, 1060.0);
    save(&mut bed, "page-light-blue.png", None)?;
    // Lower down: the formats, over 12 hours, in English.
    bed.click("segment:locale.time:1")?;
    bed.shell_said("the clock shows", &["time=\"2:25 PM\""])?;
    bed.click("control:locale.date")?;
    bed.shows(|s| s["menu"] == "locale.date")?;
    bed.hp.pointer_move(400.0, 1060.0);
    save(&mut bed, "page-light-date-menu.png", None)?;
    bed.click("option:1")?;
    bed.shell_said("the clock shows", &["date=\"jeu. 5 nov.\""])?;
    bed.hp.pointer_move(400.0, 1060.0);
    save(&mut bed, "page-light-12h-short.png", None)?;
    Ok(())
}

/// The maintainer's screenshots of #422 and #423, into `HOCUS_SHOTS`: the
/// keyboard's ring in the bar (on the agent waiting, then on the clock)
/// and in a popover opened by `Enter`; then high contrast, dark and
/// light: the bar with a popover, and the Accessibility page.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_of_the_keyboard_and_high_contrast() -> anyhow::Result<()> {
    const KEY_B: u32 = 48;
    const KEY_ENTER: u32 = 28;
    const KEY_HOME: u32 = 102;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let save = |bed: &mut Bed, name: &str, crop: Option<(i32, i32, i32, i32)>| {
        common::at_rest(&mut bed.hp)?;
        wait(&mut bed.hp, Duration::from_millis(400))?;
        let (w, h, bytes) = bed.hp.screenshot(0)?;
        let (x0, y0, cw, ch) = crop.unwrap_or((0, 0, w, h));
        let rgba: Vec<u8> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        let image = image::imageops::crop_imm(&image, x0 as u32, y0 as u32, cw as u32, ch as u32)
            .to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        anyhow::Ok(())
    };
    let agents = || {
        let agent = |id: u64, name: &str, state: &str| hocus_link::AgentShown {
            id,
            state: state.to_owned(),
            since: id,
            source: "claude-code".to_owned(),
            window: 0,
            name: name.to_owned(),
            note: None,
        };
        hocus_link::ToShell::Agents(vec![
            agent(1, "site-web", "waiting"),
            agent(2, "infra", "failed"),
            agent(3, "docs", "working"),
            agent(4, "api", "idle"),
        ])
    };
    let bar_crop = Some((1920 - 8 - 196 - 8 - 320 - 12, 0, 196 + 8 + 320 + 28, 1080));
    let keys = |bed: &mut Bed, codes: &[u32]| {
        common::chord(&mut bed.hp, codes);
        wait(&mut bed.hp, Duration::from_millis(250))
    };
    for (theme, contrast) in [("dark", false), ("dark", true), ("light", true)] {
        let text =
            format!("{FILE}\n[appearance]\ntheme = \"{theme}\"\nhigh-contrast = {contrast}\n");
        let Some(mut bed) = Bed::start(&text)? else {
            return Ok(());
        };
        let tag = if contrast {
            format!("contrast-{theme}")
        } else {
            format!("ring-{theme}")
        };
        bed.hp.tell_shell(agents());
        bed.shell_said("the agents group shows it", &[])?;
        wait(&mut bed.hp, Duration::from_millis(500))?;
        // Super+B: on the agent waiting.
        keys(&mut bed, &[common::KEY_LEFTMETA, KEY_B])?;
        save(&mut bed, &format!("{tag}-bar-agent.png"), bar_crop)?;
        // Home: the clock; Enter: the calendar, its first control ringed.
        keys(&mut bed, &[KEY_HOME])?;
        save(&mut bed, &format!("{tag}-bar-clock.png"), bar_crop)?;
        keys(&mut bed, &[KEY_ENTER])?;
        save(&mut bed, &format!("{tag}-popover.png"), bar_crop)?;
        keys(&mut bed, &[common::KEY_ESC])?;
        keys(&mut bed, &[common::KEY_ESC])?;
        if contrast {
            bed.open_on("accessibility")?;
            bed.hp.pointer_move(400.0, 1060.0);
            save(&mut bed, &format!("{tag}-settings.png"), None)?;
        }
    }
    Ok(())
}
