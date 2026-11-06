//! Applications (6.15, issue #449, variant A): the real `hocus-settings`
//! window, a client of a headless compositor (one 2560 × 1600 screen), its
//! configuration file, `mimeapps.list`, autostart folders, applications,
//! home, state and runtime directories the test's own, no bus, no GPU.
//!
//! One test per point of the spec: a browser chosen writes its four types
//! in `mimeapps.list`, the rest of the file kept, and "Annuler" puts it
//! back byte for byte; a row no application opens is inert and says so;
//! "Autre commande…" refuses a program not in `PATH`, writes `[programs]
//! terminal` otherwise, and `Super+Return` starts it; an installed
//! start-up entry turned off is a hidden copy, on again none; an
//! application added is its entry copied, its ✕ removes it and "Annuler"
//! puts it back. What the window shows is read from its log, never on a
//! time.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFTCTRL, KEY_LEFTMETA, KEY_LEFTSHIFT, WAIT, chord, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_ENTER: u32 = 28;
const KEY_A: u32 = 30;
const KEY_SPACE: u32 = 57;

const FILE: &str = "# My desktop, by hand.\n[keyboard]\nlayouts = [\"us\"]\n";
const MIMEAPPS: &str = "# Mine.\n[Added Associations]\nimage/png=imv.desktop;\n\n\
                        [Default Applications]\nx-scheme-handler/http=firefox.desktop;\n\
                        text/html=firefox.desktop;\napplication/pdf=evince.desktop;\n";

/// The applications installed: id, name, and the rest of the entry.
const APPS: [(&str, &str, &str); 8] = [
    (
        "firefox",
        "Firefox",
        "Exec=firefox %u\nMimeType=text/html;application/xhtml+xml;x-scheme-handler/http;x-scheme-handler/https;\n",
    ),
    (
        "chromium",
        "Chromium",
        "Exec=chromium %U\nMimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;\n",
    ),
    (
        "mpv",
        "mpv",
        "Exec=mpv %U\nMimeType=video/mp4;video/webm;\n",
    ),
    (
        "vlc",
        "VLC",
        "Exec=vlc %U\nMimeType=video/webm;video/mp4;\n",
    ),
    (
        "imv",
        "imv",
        "Exec=imv %F\nMimeType=image/png;image/jpeg;\n",
    ),
    (
        "foot",
        "Foot",
        "Exec=foot\nCategories=System;TerminalEmulator;\n",
    ),
    (
        "kitty",
        "Kitty",
        "Exec=kitty\nCategories=System;TerminalEmulator;\n",
    ),
    (
        "org.keepassxc.KeePassXC",
        "KeePassXC",
        "Exec=keepassxc %f\nIcon=keepassxc\n",
    ),
];

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
}

/// A character's key code (US) and whether it takes Shift.
fn key_of(c: char) -> Option<(u32, bool)> {
    const ROWS: [(&str, u32); 3] = [("qwertyuiop", 16), ("asdfghjkl", 30), ("zxcvbnm", 44)];
    let lower = c.to_ascii_lowercase();
    if let Some(code) = ROWS
        .iter()
        .find_map(|(keys, first)| keys.find(lower).map(|i| first + i as u32))
    {
        return Some((code, c.is_ascii_uppercase()));
    }
    Some(match c {
        '1'..='9' => (2 + c as u32 - '1' as u32, false),
        '0' => (11, false),
        '-' => (12, false),
        '_' => (12, true),
        '.' => (52, false),
        '/' => (53, false),
        ' ' => (KEY_SPACE, false),
        _ => return None,
    })
}

impl Bed {
    /// The compositor and the Settings' script; `None` when the Settings
    /// are not built.
    fn start() -> anyhow::Result<Option<Self>> {
        let Some(program) = common::settings_program() else {
            eprintln!("hocus-settings is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home",
            "config/hocuspanda",
            "config/autostart",
            "xdg/autostart",
            "cache",
            "state",
            "data/applications",
            "system",
            "bin",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        for (id, name, rest) in APPS {
            std::fs::write(
                root.join(format!("data/applications/{id}.desktop")),
                format!("[Desktop Entry]\nType=Application\nName={name}\n{rest}"),
            )?;
        }
        std::fs::write(root.join("config/mimeapps.list"), MIMEAPPS)?;
        // Installed start-up entries: one here, three for other desktops.
        let entry = |name: &str, extra: &str| {
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec={}\n{extra}",
                name.to_lowercase()
            )
        };
        std::fs::write(
            root.join("xdg/autostart/slack.desktop"),
            entry("Slack", "Comment=Team chat\n"),
        )?;
        for (id, desktop) in [
            ("tracker", "GNOME"),
            ("baloo", "KDE"),
            ("polkit-gnome", "GNOME"),
        ] {
            std::fs::write(
                root.join(format!("xdg/autostart/{id}.desktop")),
                entry(id, &format!("OnlyShowIn={desktop};\n")),
            )?;
        }
        // The user's own.
        std::fs::write(
            root.join("config/autostart/syncthing-start.desktop"),
            "[Desktop Entry]\nType=Application\nName=Syncthing\nExec=syncthing serve --no-browser\n",
        )?;
        // `foot` is in the Settings' PATH: "Automatique" finds it.
        for program in ["foot", "myterm"] {
            let file = root.join("bin").join(program);
            std::fs::write(&file, "#!/bin/sh\nexit 0\n")?;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
        }
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, FILE)?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 TZ=UTC HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CONFIG_DIRS='{}' XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 XDG_DATA_DIRS='{}' PATH='{}' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("xdg"),
                at("cache"),
                at("state"),
                at("data"),
                at("system"),
                at("bin"),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.state_dir = Some(root.join("state/hocuspanda"));
        config.appearance.reduced_motion = true;
        let mut hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["2560x1600@1".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        hp.pointer_move(1280.0, 800.0);
        Ok(Some(Self {
            hp,
            dir,
            log,
            script,
        }))
    }

    fn path(&self, sub: &str) -> PathBuf {
        self.dir.path().join(sub)
    }

    /// Launched on Applications, once its thread has read.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "applications".to_owned()],
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
        self.shows(|s| {
            s["file"] == "saved"
                && s["page"] == "applications"
                && s["applications"]["heard"] == true
        })
    }

    /// What the window shows, once `done` holds of it.
    fn shows(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> { common::last_json(log, " shows {") };
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = read(&log).filter(|s| done(s));
            seen.is_some()
        })?;
        seen.ok_or_else(|| {
            let said = common::logged(&log);
            let trouble: Vec<&str> = said
                .lines()
                .filter(|l| l.contains("WARN") || l.contains("ERROR") || l.contains("panicked"))
                .collect();
            anyhow::anyhow!(
                "the window never showed it; it shows {}\n{}",
                read(&log).map_or_else(|| "nothing".to_owned(), |s| s.to_string()),
                trouble.join("\n")
            )
        })
    }

    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> { common::last_json(log, " places {") };
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = read(&log).filter(|p| p.get(id).is_some());
            seen.is_some()
        })?;
        seen.ok_or_else(|| anyhow::anyhow!("{id} is nowhere: {:?}", read(&log)))
    }

    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    fn place_of(&mut self, id: &str) -> anyhow::Result<[f64; 4]> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        Ok([n(0), n(1), n(2), n(3)])
    }

    /// `id` scrolled into the window with the wheel over the page.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        for _ in 0..30 {
            let [_, y, _, h] = self.place_of(id)?;
            let window = self.window()?;
            let notches = if y + h > f64::from(window.height) - 90.0 {
                2
            } else if y < 56.0 {
                -2
            } else {
                return Ok(());
            };
            self.hp.pointer_move(
                f64::from(window.x) + 500.0,
                f64::from(window.y) + f64::from(window.height) / 2.0,
            );
            wait(&mut self.hp, Duration::from_millis(50))?;
            self.hp.wheel(notches);
            let before = [y, h];
            let log = self.log.clone();
            let key = id.to_owned();
            let moved = self.hp.dispatch_until(Duration::from_secs(2), |_| {
                read_place(&log, &key).is_some_and(|p| [p[1], p[3]] != before)
            })?;
            // At the page's end: as far as it goes.
            if !moved {
                return Ok(());
            }
        }
        anyhow::bail!("{id} never came into view")
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        if !id.starts_with("option:") && !id.starts_with("sheet:") && id != "toast-action" {
            self.reveal(id)?;
        }
        let [x, y, w, h] = self.place_of(id)?;
        let window = self.window()?;
        let at = (
            f64::from(window.x) + x + w / 2.0,
            f64::from(window.y) + y + h / 2.0,
        );
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn key(&mut self, code: u32) -> anyhow::Result<()> {
        self.hp.key(code, true);
        self.hp.key(code, false);
        wait(&mut self.hp, Duration::from_millis(10))
    }

    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            let (code, shift) = key_of(c).ok_or_else(|| anyhow::anyhow!("no key for {c}"))?;
            if shift {
                self.hp.key(KEY_LEFTSHIFT, true);
            }
            self.key(code)?;
            if shift {
                self.hp.key(KEY_LEFTSHIFT, false);
            }
        }
        Ok(())
    }

    /// `file` once `done` holds of its text (`None`: not there).
    fn file_is(
        &mut self,
        file: &Path,
        done: impl Fn(Option<&str>) -> bool,
    ) -> anyhow::Result<Option<String>> {
        let read = |f: &Path| std::fs::read_to_string(f).ok();
        let seen = self
            .hp
            .dispatch_until(WAIT, |_| done(read(file).as_deref()))?;
        let text = read(file);
        anyhow::ensure!(seen, "{} never held it: {text:?}", file.display());
        Ok(text)
    }
}

fn read_place(log: &Path, id: &str) -> Option<[f64; 4]> {
    let places = common::last_json(log, " places {")?;
    let place = places.get(id)?;
    let n = |i: usize| place[i].as_f64();
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn default_row<'a>(shown: &'a Value, id: &str) -> &'a Value {
    shown["applications"]["defaults"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id))
        .unwrap_or(&Value::Null)
}

fn startup<'a>(shown: &'a Value, id: &str) -> &'a Value {
    shown["applications"]["startup"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id))
        .unwrap_or(&Value::Null)
}

const BROWSER_TYPES: [&str; 4] = [
    "x-scheme-handler/http",
    "x-scheme-handler/https",
    "text/html",
    "application/xhtml+xml",
];

/// The browser's menu opened and Chromium chosen.
fn choose_chromium(bed: &mut Bed) -> anyhow::Result<()> {
    let shown = bed.shows(|s| default_row(s, "applications.browser")["app"] == "firefox")?;
    assert_eq!(
        strings(&default_row(&shown, "applications.browser")["choices"]),
        ["Chromium", "Firefox"]
    );
    bed.click("control:applications.browser")?;
    bed.shows(|s| s["applications"]["menu"]["id"] == "applications.browser")?;
    bed.places_with("option:0")?;
    bed.click("option:0")?;
    Ok(())
}

#[test]
fn choosing_a_browser_writes_its_four_types_in_mimeapps_list_keeping_the_rest() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    bed.open()?;
    choose_chromium(&mut bed)?;
    let list = bed.path("config/mimeapps.list");
    let text = bed
        .file_is(&list, |t| {
            t.is_some_and(|t| {
                BROWSER_TYPES
                    .iter()
                    .all(|m| t.contains(&format!("{m}=chromium.desktop;")))
            })
        })?
        .unwrap_or_default();
    assert_eq!(
        text,
        "# Mine.\n[Added Associations]\nimage/png=imv.desktop;\n\n\
         [Default Applications]\nx-scheme-handler/http=chromium.desktop;\n\
         text/html=chromium.desktop;\napplication/pdf=evince.desktop;\n\
         x-scheme-handler/https=chromium.desktop;\napplication/xhtml+xml=chromium.desktop;\n"
    );
    let shown = bed.shows(|s| {
        s["toast"] == "Navigateur web : Chromium"
            && default_row(s, "applications.browser")["app"] == "chromium"
    })?;
    assert_eq!(shown["toast_action"], "undo");
    Ok(())
}

#[test]
fn annuler_puts_mimeapps_list_back_byte_for_byte() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    bed.open()?;
    choose_chromium(&mut bed)?;
    bed.shows(|s| s["toast"] == "Navigateur web : Chromium")?;
    bed.click("toast-action")?;
    let list = bed.path("config/mimeapps.list");
    bed.file_is(&list, |t| t == Some(MIMEAPPS))?;
    bed.shows(|s| {
        s["toast"] == "Annulé : Navigateur web : Firefox"
            && default_row(s, "applications.browser")["app"] == "firefox"
    })?;
    Ok(())
}

#[test]
fn a_row_with_no_application_is_inert_and_says_so() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let mail = default_row(&shown, "applications.mail");
    assert_eq!(mail["inert"], true);
    assert_eq!(mail["app"], Value::Null);
    assert_eq!(
        mail["line"],
        "Aucune application installée n’ouvre les liens « mailto: »."
    );
    bed.click("control:applications.mail")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let shown = bed.shows(|s| s["applications"]["heard"] == true)?;
    assert_eq!(
        shown["applications"]["menu"],
        Value::Null,
        "an inert menu opened"
    );
    // The videos: mpv for MP4, VLC for WebM (by hand): the row says so.
    std::fs::write(
        bed.path("config/mimeapps.list"),
        format!("{MIMEAPPS}video/mp4=mpv.desktop;\nvideo/webm=vlc.desktop;\n"),
    )?;
    bed.shows(|s| {
        default_row(s, "applications.videos")["differs"] == "Les vidéos WebM s’ouvrent avec VLC"
    })?;
    Ok(())
}

#[test]
fn other_command_writes_programs_terminal_and_super_return_uses_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["applications"]["terminal"]["found"], "foot");
    assert_eq!(shown["applications"]["terminal"]["chosen"], "");
    bed.click("control:programs.terminal")?;
    let shown = bed.shows(|s| s["applications"]["menu"]["id"] == "programs.terminal")?;
    assert_eq!(
        strings(&shown["applications"]["menu"]["choices"]),
        ["auto", "Foot", "Kitty", "other"]
    );
    bed.places_with("option:3")?;
    bed.click("option:3")?;
    bed.shows(|s| s["applications"]["terminal"]["typing"] == "")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("kitty-nightly")?;
    bed.shows(|s| s["applications"]["terminal"]["typing"] == "kitty-nightly")?;
    bed.key(KEY_ENTER)?;
    bed.shows(|s| s["applications"]["terminal"]["missing"] == "kitty-nightly")?;
    let config = bed.path("config/hocuspanda/config.toml");
    assert!(!std::fs::read_to_string(&config)?.contains("terminal"));
    // A program that is there: written, and Super+Return starts it.
    let marker = bed.path("started");
    let program = bed.path("bin/myterm");
    std::fs::write(
        &program,
        format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display()),
    )?;
    let command = format!("{} --hold", program.display());
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_A]);
    wait(&mut bed.hp, Duration::from_millis(50))?;
    bed.type_text(&command)?;
    bed.shows(|s| s["applications"]["terminal"]["typing"] == command.as_str())?;
    bed.key(KEY_ENTER)?;
    bed.file_is(&config, |t| {
        t.is_some_and(|t| t.contains(&format!("[programs]\nterminal = \"{command}\"")))
    })?;
    bed.shows(|s| {
        s["applications"]["terminal"]["value"] == command.as_str()
            && s["toast"] == format!("Terminal : {command}").as_str()
    })?;
    // The compositor follows the file: then Super+Return runs the command.
    let taken = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.state().config.programs.terminal == command)?;
    anyhow::ensure!(taken, "the compositor did not take the new terminal");
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_ENTER]);
    let ran = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&marker).is_ok_and(|t| t.trim() == "--hold")
    })?;
    anyhow::ensure!(ran, "Super+Return did not start {command}");
    Ok(())
}

#[test]
fn turning_off_an_installed_autostart_entry_writes_a_hidden_copy() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let names: Vec<&str> = shown["applications"]["startup"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| e["name"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(names, ["Slack", "Syncthing"]);
    assert_eq!(startup(&shown, "slack")["own"], false);
    assert_eq!(startup(&shown, "syncthing-start")["own"], true);
    assert_eq!(strings(&shown["applications"]["others"]).len(), 3);
    bed.click("switch:slack")?;
    let copy = bed.path("config/autostart/slack.desktop");
    let text = bed.file_is(&copy, |t| t.is_some())?.unwrap_or_default();
    assert_eq!(
        text,
        "[Desktop Entry]\nType=Application\nName=Slack\nExec=slack\nComment=Team chat\nHidden=true\n"
    );
    bed.shows(|s| s["toast"] == "Slack au démarrage : non" && startup(s, "slack")["on"] == false)?;
    // On again: the copy, which holds nothing else, goes.
    bed.click("switch:slack")?;
    bed.file_is(&copy, |t| t.is_none())?;
    bed.shows(|s| s["toast"] == "Slack au démarrage : oui" && startup(s, "slack")["on"] == true)?;
    // The installed file is never touched.
    assert!(!std::fs::read_to_string(bed.path("xdg/autostart/slack.desktop"))?.contains("Hidden"));
    // "Les voir": the other desktops' entries.
    bed.click("applications-others")?;
    bed.shows(|s| s["applications"]["others_shown"] == true)?;
    bed.places_with("applications-others-list")?;
    Ok(())
}

#[test]
fn adding_an_application_copies_its_entry_into_autostart() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("control:applications.add")?;
    let shown = bed.shows(|s| s["applications"]["sheet"]["query"] == "")?;
    // Those already there are not listed.
    let list = strings(&shown["applications"]["sheet"]["list"]);
    assert!(
        list.contains(&"KeePassXC") && !list.contains(&"Syncthing"),
        "{list:?}"
    );
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("kee")?;
    bed.shows(|s| strings(&s["applications"]["sheet"]["list"]) == ["KeePassXC"])?;
    bed.key(KEY_ENTER)?;
    let added = bed.path("config/autostart/org.keepassxc.KeePassXC.desktop");
    let text = bed.file_is(&added, |t| t.is_some())?.unwrap_or_default();
    assert_eq!(
        text,
        std::fs::read_to_string(bed.path("data/applications/org.keepassxc.KeePassXC.desktop"))?
    );
    let shown = bed.shows(|s| {
        s["toast"] == "KeePassXC ajoutée au démarrage"
            && s["applications"]["sheet"].is_null()
            && startup(s, "org.keepassxc.KeePassXC")["own"] == true
    })?;
    // The new row comes last.
    let last = shown["applications"]["startup"]
        .as_array()
        .and_then(|a| a.last())
        .map(|e| e["id"].clone());
    assert_eq!(last, Some(Value::from("org.keepassxc.KeePassXC")));
    // Its ✕ removes it, "Annuler" puts it back byte for byte.
    bed.click("remove:org.keepassxc.KeePassXC")?;
    bed.file_is(&added, |t| t.is_none())?;
    bed.shows(|s| s["toast"] == "KeePassXC enlevée du démarrage")?;
    bed.click("toast-action")?;
    let back = bed.file_is(&added, |t| t.is_some())?.unwrap_or_default();
    assert_eq!(back, text);
    Ok(())
}

/// The window as it shows, once still, into `out`/`name`.
fn save(bed: &mut Bed, out: &Path, name: &str, rest: bool) -> anyhow::Result<()> {
    if rest {
        bed.hp.pointer_move(5.0, 5.0);
    }
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
    let window = bed.window()?;
    let (w, h, bytes) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let image = image::imageops::crop_imm(
        &image,
        window.x.max(0) as u32,
        window.y.max(0) as u32,
        window.width as u32,
        window.height as u32,
    )
    .to_image();
    let path = out.join(name);
    image.save(&path)?;
    eprintln!("{}", path.display());
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    std::fs::write(
        bed.path("config/mimeapps.list"),
        format!("{MIMEAPPS}video/mp4=mpv.desktop;\nvideo/webm=vlc.desktop;\n"),
    )?;
    // Icons in the theme (hicolor), as applications install them.
    for (id, _, _) in APPS {
        let name = id.rsplit('.').next().unwrap_or(id).to_lowercase();
        let colour = format!(
            "#{:06x}",
            id.bytes()
                .fold(0x40_80c0u32, |a, b| a.wrapping_mul(33) ^ u32::from(b))
                & 0x7f_7f7f
                | 0x80_8080
        );
        let dir = bed.path("data/icons/hicolor/scalable/apps");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("{name}.svg")),
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"48\" height=\"48\">\
                 <rect width=\"48\" height=\"48\" rx=\"12\" fill=\"{colour}\"/>\
                 <circle cx=\"24\" cy=\"24\" r=\"11\" fill=\"#ffffff\" fill-opacity=\"0.85\"/></svg>"
            ),
        )?;
        let entry = bed.path(&format!("data/applications/{id}.desktop"));
        let text = std::fs::read_to_string(&entry)?;
        if !text.contains("Icon=") {
            std::fs::write(&entry, format!("{text}Icon={name}\n"))?;
        }
    }
    bed.open()?;
    bed.shows(|s| {
        default_row(s, "applications.videos")["differs"].is_string()
            && s["applications"]["icons"].as_u64().is_some_and(|n| n >= 8)
    })?;
    save(&mut bed, &out, "applications-page.png", true)?;
    bed.click("control:applications.browser")?;
    bed.shows(|s| s["applications"]["menu"]["id"] == "applications.browser")?;
    save(&mut bed, &out, "applications-browser-menu.png", true)?;
    chord(&mut bed.hp, &[common::KEY_ESC]);
    bed.shows(|s| s["applications"]["menu"].is_null())?;
    bed.click("control:programs.terminal")?;
    bed.shows(|s| s["applications"]["menu"]["id"] == "programs.terminal")?;
    save(&mut bed, &out, "applications-terminal-menu.png", true)?;
    bed.click("option:3")?;
    bed.shows(|s| s["applications"]["terminal"]["typing"] == "")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("kitty-nightly")?;
    bed.key(KEY_ENTER)?;
    bed.shows(|s| s["applications"]["terminal"]["missing"] == "kitty-nightly")?;
    save(&mut bed, &out, "applications-terminal-missing.png", true)?;
    chord(&mut bed.hp, &[common::KEY_ESC]);
    bed.click("switch:slack")?;
    bed.shows(|s| s["toast"] == "Slack au démarrage : non")?;
    bed.click("applications-others")?;
    bed.shows(|s| s["applications"]["others_shown"] == true)?;
    bed.reveal("applications-others-list")?;
    save(&mut bed, &out, "applications-startup.png", true)?;
    bed.click("control:applications.add")?;
    bed.shows(|s| s["applications"]["sheet"]["query"] == "")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("k")?;
    bed.shows(|s| s["applications"]["sheet"]["query"] == "k")?;
    save(&mut bed, &out, "applications-add-sheet.png", true)?;
    Ok(())
}
