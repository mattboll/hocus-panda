//! Settings › Notifications (#899): the real `hocus-settings` window, a
//! client of a headless compositor, its configuration file, home and
//! state the test's own; the applications that notified are the shell's
//! senders' file, written here as the shell writes it.
//!
//! The page lists them, the busiest this week first, each with its level
//! from the file; a level chosen in a row's menu is written; do not
//! disturb is the real shell's, which says it back. The pull
//! request's screenshots are an ignored test (`HOCUS_SHOTS`).

use crate::common;
use crate::settings_page;

use settings_page::{Floor, Wheel};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, WAIT, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";

const FILE: &str = "# Mine.\n[notifications.apps]\nslack = \"silent\"\n\
    thunderbird = \"history\"\n\"org.mozilla.firefox\" = \"none\"\n";

/// Today's number, as the shell counts days.
fn today() -> i64 {
    jiff::Zoned::now()
        .date()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .map_or(0, |z| z.timestamp().as_second().div_euclid(86_400))
}

/// The senders' file: four applications, over this week and the one
/// before (what is older is not counted).
fn senders() -> String {
    let today = today();
    let sender = |id: &str, name: &str, icon: &str, days: &[(i64, u32)]| {
        let days: serde_json::Map<String, Value> = days
            .iter()
            .map(|(ago, n)| ((today - ago).to_string(), Value::from(*n)))
            .collect();
        serde_json::json!({ "id": id, "name": name, "icon": icon, "days": days })
    };
    serde_json::json!({
        "apps": [
            sender("org.mozilla.firefox", "Firefox", "firefox", &[(1, 3), (9, 40)]),
            sender("Claude Code", "Claude Code", "", &[(0, 5), (2, 7)]),
            sender("thunderbird", "Thunderbird", "thunderbird", &[(0, 20), (3, 17)]),
            sender("slack", "Slack", "slack", &[(0, 100), (1, 90), (6, 24), (7, 300)]),
        ]
    })
    .to_string()
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
}

impl Bed {
    /// A compositor, the configuration file holding `file`, the shell's
    /// senders `senders`, the Settings' script in French; with `shell`, the
    /// real shell, its home and state the test's (it says do not disturb).
    fn start(file: &str, senders: &str, shell: bool) -> anyhow::Result<Self> {
        let program = common::settings_program();
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home",
            "config/hocuspanda",
            "cache",
            "state/hocuspanda",
            "data",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let config_file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&config_file, file)?;
        std::fs::write(
            root.join("state/hocuspanda").join(hocus_link::SENDERS_FILE),
            senders,
        )?;
        let log = root.join("settings.log");
        let script = root.join("hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 HOCUS_USER_NAME=Alex RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let shell = if shell {
            let wrapper = root.join("shell");
            std::fs::write(
                &wrapper,
                format!(
                    "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                     export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                     PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                     LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                     XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                     RUST_LOG=warn\n\
                     exec '{}' \"$@\" 2>>'{}'\n",
                    at("home"),
                    at("config"),
                    at("cache"),
                    at("state"),
                    at("data"),
                    common::shell_program().display(),
                    root.join("shell.log").display()
                ),
            )?;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))?;
            Some(wrapper)
        } else {
            None
        };
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(config_file);
        config.appearance.reduced_motion = true;
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            shell,
        )?;
        Ok(Self {
            hp,
            dir,
            log,
            script,
        })
    }

    fn file(&self) -> PathBuf {
        self.dir.path().join("config/hocuspanda/config.toml")
    }

    /// The Settings opened on the page, the senders read.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![
                self.script.display().to_string(),
                "notifications".to_owned(),
            ],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(
            open,
            "the window never opened: {}",
            common::logged(&self.log)
        );
        self.shows(|s| s["page"] == "notifications" && s["notifications"]["read"] == true)
    }

    /// What the window shows, once `done` holds of it.
    fn shows(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = common::last_json(&log, " shows {").filter(|s| done(s));
            seen.is_some()
        })?;
        seen.ok_or_else(|| {
            anyhow::anyhow!(
                "the window never showed it; it shows {:?}",
                common::last_json(&log, " shows {")
            )
        })
    }

    fn window(&self) -> anyhow::Result<hocus_compositor::model::Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    /// The middle of `id` on the screen, scrolled into view.
    fn at(&mut self, id: &str) -> anyhow::Result<(f64, f64)> {
        let wheel = Wheel {
            x: 600.0,
            notches: 2,
            top: 56.0,
            floor: Floor::Margin(90.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)?;
        let places = settings_page::places_with(&mut self.hp, &self.log, id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        let window = self.window()?;
        Ok((
            f64::from(window.x) + n(0) + n(2) / 2.0,
            f64::from(window.y) + n(1) + n(3) / 2.0,
        ))
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        let at = self.at(id)?;
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
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        anyhow::ensure!(
            seen,
            "the file never held it:\n{text}\nthe window said:\n{}",
            common::logged(&self.log)
        );
        Ok(text)
    }
}

#[test]
fn the_applications_busiest_first_and_a_level_chosen_is_written() -> anyhow::Result<()> {
    let mut bed = Bed::start(FILE, &senders(), true)?;
    bed.open()?;
    let shown = bed.shows(|s| s["notifications"]["dnd"] == false)?;
    let apps = shown["notifications"]["apps"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let said: Vec<(String, u64, String)> = apps
        .iter()
        .map(|a| {
            (
                a["name"].as_str().unwrap_or_default().to_owned(),
                a["week"].as_u64().unwrap_or_default(),
                a["level"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let want = [
        ("Slack", 214, "silent"),
        ("Thunderbird", 37, "history"),
        ("Claude Code", 12, "all"),
        ("Firefox", 3, "none"),
    ]
    .map(|(n, w, l)| (n.to_owned(), w, l.to_owned()));
    assert_eq!(said, want, "{shown}");
    // Claude Code's menu: "Rien" chosen, written by its name.
    bed.click("notifications:menu:Claude Code")?;
    bed.shows(|s| s["notifications"]["menu"] == "Claude Code")?;
    bed.click("option:3")?;
    let text = bed.written(|t| t.contains("\"Claude Code\" = \"none\""))?;
    assert!(text.starts_with("# Mine.\n"), "{text}");
    assert!(text.contains("slack = \"silent\""), "{text}");
    bed.shows(|s| {
        s["notifications"]["apps"][2]["level"] == "none" && s["notifications"]["menu"].is_null()
    })?;
    // "Ne pas déranger": the shell's, turned on, and said back.
    bed.click("notifications:dnd")?;
    bed.shows(|s| s["notifications"]["dnd"] == true)?;
    Ok(())
}

/// The window, cropped, saved as `name` in `out` once it is still.
fn shoot(bed: &mut Bed, out: &Path, name: &str) -> anyhow::Result<()> {
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..30 {
        if still >= 5 {
            break;
        }
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
    }
    let window = bed.window()?;
    let (w, h, px) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let crop = image::imageops::crop_imm(
        &image,
        window.x.max(0) as u32,
        window.y.max(0) as u32,
        window.width as u32,
        window.height as u32,
    )
    .to_image();
    let file = out.join(format!("{name}.png"));
    crop.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The pull request's screenshots, to `$HOCUS_SHOTS` when it names a
/// folder; run by hand (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let mut bed = Bed::start(FILE, &senders(), true)?;
    bed.open()?;
    bed.shows(|s| s["notifications"]["dnd"] == false)?;
    bed.hp.pointer_move(1900.0, 1060.0);
    shoot(&mut bed, &out, "settings-notifications")?;
    bed.click("notifications:menu:slack")?;
    bed.shows(|s| s["notifications"]["menu"] == "slack")?;
    bed.hp.pointer_move(1900.0, 1060.0);
    shoot(&mut bed, &out, "settings-notifications-menu")?;
    drop(bed);
    let mut bed = Bed::start("# Mine.\n", "", true)?;
    bed.open()?;
    bed.shows(|s| s["notifications"]["dnd"] == false)?;
    bed.hp.pointer_move(1900.0, 1060.0);
    shoot(&mut bed, &out, "settings-notifications-empty")
}
