//! Bureaux et fenêtres (6.14, issue #448, variant A): the real
//! `hocus-settings` window, a client of a headless compositor (one
//! 2560 × 1600 screen), its configuration file, home, state and runtime
//! directories the test's own, no bus, no GPU.
//!
//! One test per point of the spec: the page's groups and rows; a desktop
//! added with + (the compositor appends it) and the last taken out with −;
//! a named desktop with windows taken out by its ✕ (its windows go to the
//! one before it, the names move up, the toast says so) and "Annuler"
//! bringing it back with them; a name changed in its field (`Enter`
//! writes, `Esc` keeps the old); Super to move turning the ink off; the
//! wobble with reduced motion; the search; the link to the keyboard's
//! shortcuts. What the window shows is read from its log, never on a
//! time.

mod common;
mod settings_page;

use settings_page::{Floor, Wheel};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFTCTRL, KEY_LEFTMETA, KEY_LEFTSHIFT, WAIT, chord, open, wait};
use hocus_compositor::{Config, DragKey, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_F: u32 = 33;
const KEY_ENTER: u32 = 28;
const KEY_2: u32 = 3;
const KEY_3: u32 = 4;

const FILE: &str = "# My desktop, by hand.\n[keyboard]\nlayouts = [\"us\"]\n\n\
                    [desktops]\ncount = 4\nnames = [\"Travail\", \"Courrier\", \"Musique\"]\n";

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
}

/// A letter's key code (US).
fn letter(c: char) -> Option<u32> {
    const ROWS: [(&str, u32); 3] = [("qwertyuiop", 16), ("asdfghjkl", 30), ("zxcvbnm", 44)];
    if c == '-' {
        return Some(12);
    }
    ROWS.iter()
        .find_map(|(keys, first)| keys.find(c).map(|i| first + u32::try_from(i).unwrap_or(0)))
}

impl Bed {
    /// The compositor and the Settings' script, the file holding `text`;
    /// `None` when the Settings are not built.
    fn start(text: &str) -> anyhow::Result<Option<Self>> {
        let Some(program) = common::settings_program() else {
            eprintln!("hocus-settings is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home",
            "config/hocuspanda",
            "cache",
            "state",
            "data/applications",
            "system",
            "bin",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        // The applications installed: the test's own.
        for (id, name) in [
            ("org.example.Mail", "Name=Mail\nName[fr]=Courrier"),
            ("test.music", "Name=Musique"),
            ("test.editor", "Name=Éditeur"),
        ] {
            std::fs::write(
                root.join(format!("data/applications/{id}.desktop")),
                format!("[Desktop Entry]\nType=Application\n{name}\n"),
            )?;
        }
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, text)?;
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
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' XDG_DATA_DIRS='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
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

    fn file(&self) -> PathBuf {
        self.dir.path().join("config/hocuspanda/config.toml")
    }

    /// Launched on Bureaux et fenêtres, its window open with the keyboard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "desktops".to_owned()],
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
            s["file"] == "saved" && s["page"] == "desktops" && s["desktops"]["windows"].is_array()
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

    /// The window's current places, once they hold `id`.
    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        settings_page::places_with(&mut self.hp, &self.log, id)
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
        let wheel = Wheel {
            x: 500.0,
            notches: 2,
            top: 56.0,
            floor: Floor::Margin(90.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        if !id.starts_with("option:") && id != "toast-action" {
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

    fn choose(&mut self, setting: &str, n: usize) -> anyhow::Result<()> {
        self.click(&format!("control:{setting}"))?;
        self.shows(|s| s["menu"].as_str() == Some(setting))?;
        self.click(&format!("option:{n}"))?;
        Ok(())
    }

    fn written(&mut self, done: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let file = self.file();
        let seen = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| done(&t))
        })?;
        let text = std::fs::read_to_string(self.file()).unwrap_or_default();
        anyhow::ensure!(seen, "the file never held it:\n{text}");
        Ok(text)
    }

    fn key(&mut self, code: u32) -> anyhow::Result<()> {
        self.hp.key(code, true);
        self.hp.key(code, false);
        wait(&mut self.hp, Duration::from_millis(10))
    }

    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            let lower = c.to_ascii_lowercase();
            let code = letter(lower).ok_or_else(|| anyhow::anyhow!("no key for {c}"))?;
            if c.is_ascii_uppercase() {
                self.hp.key(KEY_LEFTSHIFT, true);
            }
            self.key(code)?;
            if c.is_ascii_uppercase() {
                self.hp.key(KEY_LEFTSHIFT, false);
            }
        }
        Ok(())
    }

    /// `word` typed in the search, once the field has the keyboard: its
    /// first letter, until the field shows it (on a loaded machine the
    /// field takes the keyboard a while after Ctrl+F), then the rest.
    fn search_for(&mut self, word: &str) -> anyhow::Result<()> {
        let first: String = word.chars().take(1).collect();
        for _ in 0..10 {
            chord(&mut self.hp, &[KEY_LEFTCTRL, KEY_F]);
            self.shows(|s| s["focus"] == "search")?;
            wait(&mut self.hp, Duration::from_millis(100))?;
            self.type_text(&first)?;
            let log = self.log.clone();
            let typed = self.hp.dispatch_until(Duration::from_secs(2), |_| {
                common::last_json(&log, " shows {").is_some_and(|s| s["query"] == first.as_str())
            })?;
            if typed {
                return self.type_text(&word[first.len()..]);
            }
            // Gone elsewhere: the search emptied, and again.
            chord(&mut self.hp, &[common::KEY_ESC]);
            self.shows(|s| s["query"] == "")?;
        }
        anyhow::bail!("the search never took {word:?}")
    }

    /// The compositor's desktops (its only screen's count), once `count`.
    fn desktops_are(&mut self, count: usize) -> anyhow::Result<()> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.strips().iter().all(|s| s.1 == count))?;
        anyhow::ensure!(done, "the compositor's desktops: {:?}", self.hp.strips());
        Ok(())
    }

    /// `app_id`'s desktop (from 0).
    fn on(&self, app_id: &str) -> Option<usize> {
        self.hp
            .all_windows()
            .into_iter()
            .find(|w| w.1.as_deref() == Some(app_id))
            .map(|w| w.2)
    }

    /// A window `app_id` opened, then sent to desktop `key`'s.
    fn window_on(&mut self, app_id: &'static str, key: Option<u32>) -> anyhow::Result<()> {
        self.hp.pointer_move(1280.0, 800.0);
        open(&mut self.hp, app_id, (400, 300))?;
        if let Some(key) = key {
            chord(&mut self.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, key]);
        }
        Ok(())
    }
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

#[test]
fn the_page_shows_its_groups_and_rows_and_each_desktops_windows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.window_on("test.mail", Some(KEY_2))?;
    bed.window_on("test.mail2", Some(KEY_2))?;
    let shown = bed.open()?;
    let rows: Vec<&str> = shown["rows"]
        .as_array()
        .map(|r| r.iter().filter_map(|r| r["id"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        rows,
        [
            "desktops.count",
            "desktops.names",
            "windows.move-modifier",
            "windows.resize-modifier",
            "windows.magnet",
            "windows.snap-by-drag",
            "windows.wobble",
            "windows.alt-tab",
            "ink.enabled"
        ]
    );
    let desktops = &shown["desktops"];
    assert_eq!(desktops["count"], 4);
    assert_eq!(
        strings(&desktops["chips"]),
        ["Travail", "Courrier", "Musique", "Bureau 4"]
    );
    // The Settings' window is on the first desktop too.
    let shown = bed.shows(|s| s["desktops"]["windows"] == serde_json::json!([1, 2, 0, 0]))?;
    assert_eq!(shown["rows"][2]["value"], "Alt + glisser");
    bed.places_with("chip:3")?;
    bed.places_with("link:keyboard.shortcuts")?;
    Ok(())
}

#[test]
fn plus_adds_a_desktop_on_every_screen_and_minus_takes_out_the_last() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("count:plus")?;
    bed.written(|t| t.contains("count = 5"))?;
    bed.desktops_are(5)?;
    bed.shows(|s| {
        s["toast"] == "Bureau 5 ajouté, sur chaque écran" && s["desktops"]["count"] == 5
    })?;
    // −: the last taken out by the compositor, then written.
    bed.click("count:minus")?;
    bed.written(|t| t.contains("count = 4"))?;
    bed.desktops_are(4)?;
    bed.shows(|s| s["toast"] == "« Bureau 5 » retiré" && s["desktops"]["count"] == 4)?;
    Ok(())
}

#[test]
fn a_desktop_taken_out_gives_its_windows_to_the_one_before_and_annuler_brings_them_back()
-> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.window_on("test.mail", Some(KEY_2))?;
    bed.window_on("test.mail2", Some(KEY_2))?;
    bed.window_on("test.music", Some(KEY_3))?;
    bed.open()?;
    bed.shows(|s| s["desktops"]["windows"] == serde_json::json!([1, 2, 1, 0]))?;
    // "Courrier"'s ✕.
    bed.click("remove:1")?;
    let text = bed.written(|t| t.contains("count = 3"))?;
    assert!(
        text.contains("names = [\"Travail\", \"Musique\"]"),
        "{text}"
    );
    assert!(text.starts_with("# My desktop, by hand."), "{text}");
    bed.desktops_are(3)?;
    assert_eq!(bed.on("test.mail"), Some(0));
    assert_eq!(bed.on("test.mail2"), Some(0));
    assert_eq!(bed.on("test.music"), Some(1), "one down");
    bed.shows(|s| {
        s["toast"] == "« Courrier » retiré : ses 2 fenêtres passent sur « Travail »"
            && strings(&s["desktops"]["chips"]) == ["Travail", "Musique", "Bureau 3"]
            && s["desktops"]["windows"] == serde_json::json!([3, 1, 0])
    })?;
    // "Annuler": back, with its windows.
    bed.click("toast-action")?;
    let text = bed.written(|t| t.contains("count = 4"))?;
    assert!(
        text.contains("names = [\"Travail\", \"Courrier\", \"Musique\"]"),
        "{text}"
    );
    bed.desktops_are(4)?;
    assert_eq!(bed.on("test.mail"), Some(1));
    assert_eq!(bed.on("test.mail2"), Some(1));
    assert_eq!(bed.on("test.music"), Some(2));
    bed.shows(|s| s["desktops"]["windows"] == serde_json::json!([1, 2, 1, 0]))?;
    Ok(())
}

#[test]
fn a_name_changes_in_its_field_enter_writes_and_escape_keeps_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    // The fourth, unnamed: "Bureau 4" becomes "Projet".
    bed.click("chip:3")?;
    bed.shows(|s| s["desktops"]["editing"] == serde_json::json!([3, ""]))?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("Projet")?;
    bed.shows(|s| s["desktops"]["editing"][1] == "Projet")?;
    bed.key(KEY_ENTER)?;
    let text = bed.written(|t| t.contains("\"Projet\""))?;
    assert!(
        text.contains("names = [\"Travail\", \"Courrier\", \"Musique\", \"Projet\"]"),
        "{text}"
    );
    bed.shows(|s| {
        s["toast"] == "Nom du bureau 4 : Projet" && strings(&s["desktops"]["chips"])[3] == "Projet"
    })?;
    let read = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.desktop_names().len() == 4)?;
    anyhow::ensure!(read, "{:?}", bed.hp.desktop_names());
    // Esc: the name stays.
    bed.click("chip:0")?;
    bed.shows(|s| s["desktops"]["editing"] == serde_json::json!([0, "Travail"]))?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("Autre")?;
    bed.key(common::KEY_ESC)?;
    let shown = bed.shows(|s| s["desktops"]["editing"].is_null())?;
    assert_eq!(strings(&shown["desktops"]["chips"])[0], "Travail");
    assert!(std::fs::read_to_string(bed.file())?.contains("\"Travail\""));
    Ok(())
}

#[test]
fn super_to_move_turns_the_ink_off_and_says_so() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.choose("windows.move-modifier", 1)?;
    bed.written(|t| t.contains("[windows]\nmove-modifier = \"super\""))?;
    let shown = bed.shows(|s| {
        s["toast"] == "Déplacer : Super + glisser · l’encre ne se lance plus"
            && s["desktops"]["ink_off"].is_string()
    })?;
    let ink = shown["rows"]
        .as_array()
        .and_then(|r| r.iter().find(|r| r["id"] == "ink.enabled"))
        .cloned()
        .unwrap_or_default();
    assert_eq!(ink["inert"], true);
    let read = bed.hp.dispatch_until(WAIT, |hp| {
        hp.state().config.windows.move_modifier == DragKey::Super
    })?;
    anyhow::ensure!(read, "the compositor did not read it");
    Ok(())
}

#[test]
fn with_reduced_motion_the_wobble_is_off_and_links_to_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&format!("{FILE}\n[appearance]\nreduced-motion = true\n"))?
    else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(
        shown["desktops"]["wobble_off"],
        "Coupées tant que Mouvements réduits est activé."
    );
    bed.click("link:appearance.reduced-motion")?;
    bed.shows(|s| s["page"] == "appearance" && s["lit"] == "appearance.reduced-motion")?;
    Ok(())
}

#[test]
fn searching_aimant_ondule_and_move_modifier_finds_their_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    for (word, found, lit) in [
        ("aimant", "Bords aimantés", "windows.magnet"),
        ("ondule", "Fenêtres ondulantes", "windows.wobble"),
        (
            "move-modifier",
            "Déplacer de n’importe où",
            "windows.move-modifier",
        ),
        ("workspaces", "Nombre de bureaux", "desktops.count"),
    ] {
        bed.search_for(word)?;
        let shown = bed.shows(|s| {
            s["query"] == word && s["results"].as_array().is_some_and(|r| !r.is_empty())
        })?;
        let names: Vec<&str> = shown["results"]
            .as_array()
            .map(|a| a.iter().filter_map(|r| r["name"].as_str()).collect())
            .unwrap_or_default();
        assert!(names.contains(&found), "{word}: {names:?}");
        let i = names.iter().position(|n| *n == found).unwrap_or(0);
        bed.click(&format!("result:{i}"))?;
        bed.shows(|s| s["page"] == "desktops" && s["lit"] == lit)?;
        chord(&mut bed.hp, &[common::KEY_ESC]);
        bed.shows(|s| s["query"] == "")?;
    }
    Ok(())
}

#[test]
fn the_shortcuts_link_opens_the_keyboard_with_its_row_lit() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("link:keyboard.shortcuts")?;
    bed.shows(|s| s["page"] == "keyboard" && s["lit"] == "keyboard.shortcuts")?;
    Ok(())
}

/// The trial area's notes window, its place in the trial's screen.
const NOTES: (f64, f64) = (318.0, 64.0);
/// The trial's Terminal's right edge, and the gap.
const TERMINAL_RIGHT: f64 = 24.0 + 210.0;

impl Bed {
    /// A point of the trial's screen, in the compositor's space, once it
    /// shows.
    fn trial_point(&mut self, (x, y): (f64, f64)) -> anyhow::Result<(f64, f64)> {
        self.reveal("desktops:trial")?;
        // Where it stays: not while the page still moves (a scroll, a row
        // growing).
        let mut place = self.place_of("desktops:trial")?;
        for _ in 0..20 {
            wait(&mut self.hp, Duration::from_millis(150))?;
            let now = self.place_of("desktops:trial")?;
            if now == place {
                break;
            }
            place = now;
        }
        let [tx, ty, _, _] = place;
        let window = self.window()?;
        Ok((f64::from(window.x) + tx + x, f64::from(window.y) + ty + y))
    }

    /// A drag in the trial's screen from `from` to `to`, in steps.
    fn trial_drag(&mut self, from: (f64, f64), to: (f64, f64)) -> anyhow::Result<()> {
        let a = self.trial_point(from)?;
        let b = self.trial_point(to)?;
        self.hp.pointer_move(a.0 - 1.0, a.1);
        self.hp.pointer_move(a.0, a.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        wait(&mut self.hp, Duration::from_millis(30))?;
        for i in 1..=10 {
            let t = f64::from(i) / 10.0;
            self.hp
                .pointer_move(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            wait(&mut self.hp, Duration::from_millis(16))?;
        }
        Ok(())
    }

    fn trial_release(&mut self) -> anyhow::Result<()> {
        self.hp.button(BTN_LEFT, false);
        wait(&mut self.hp, Duration::from_millis(30))
    }
}

#[test]
fn in_the_trial_notes_sticks_to_the_terminal_and_nothing_is_written() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    let before = std::fs::read_to_string(bed.file())?;
    // By its title bar, to 10 px short of the gap right of the Terminal.
    let grab = (NOTES.0 + 40.0, NOTES.1 + 10.0);
    let to = (grab.0 - (NOTES.0 - TERMINAL_RIGHT - 8.0 - 10.0), grab.1);
    bed.trial_drag(grab, to)?;
    let shown = bed.shows(|s| {
        s["desktops"]["trial"]["said"]
            == "Collée au Terminal, à 8 px : glisse encore pour la décoller."
    })?;
    assert_eq!(shown["desktops"]["trial"]["notes"][0], 242);
    bed.trial_release()?;
    // The magnet off: it stops where it is let go.
    bed.click("control:windows.magnet")?;
    bed.written(|t| t.contains("magnet = false"))?;
    let grab = (242.0 + 40.0, NOTES.1 + 10.0);
    bed.trial_drag(grab, (grab.0 + 10.0, grab.1))?;
    bed.trial_release()?;
    bed.shows(|s| s["desktops"]["trial"]["notes"][0] == 252)?;
    // Only the magnet's switch was written.
    let after = std::fs::read_to_string(bed.file())?;
    assert_eq!(after.replace("\n[windows]\nmagnet = false\n", ""), before);
    Ok(())
}

#[test]
fn in_the_trial_the_content_needs_the_key_and_its_button_holds_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    let content = (NOTES.0 + 60.0, NOTES.1 + 70.0);
    bed.trial_drag(content, (content.0 + 30.0, content.1))?;
    bed.trial_release()?;
    bed.shows(|s| {
        s["desktops"]["trial"]["said"]
            .as_str()
            .is_some_and(|t| t.starts_with("Sans Alt, le clic irait à l’application."))
            && s["desktops"]["trial"]["notes"][0] == 318
    })?;
    bed.click("trial:hold")?;
    bed.shows(|s| s["desktops"]["trial"]["held"] == true)?;
    bed.trial_drag(content, (content.0 + 30.0, content.1 + 4.0))?;
    bed.trial_release()?;
    bed.shows(|s| {
        s["desktops"]["trial"]["notes"][0] == 348 && s["desktops"]["trial"]["notes"][1] == 68
    })?;
    // Super to move: the button follows the key.
    bed.choose("windows.move-modifier", 1)?;
    bed.written(|t| t.contains("move-modifier = \"super\""))?;
    bed.shows(|s| s["desktops"]["trial"]["hold"] == "Super tenue")?;
    Ok(())
}

#[test]
fn in_the_trial_a_side_takes_the_half_unless_snapping_is_off() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    let grab = (NOTES.0 + 40.0, NOTES.1 + 10.0);
    bed.trial_drag(grab, (3.0, 120.0))?;
    bed.shows(|s| s["desktops"]["trial"]["said"] == "Lâche-la ici : la moitié gauche.")?;
    bed.trial_release()?;
    // The left half of the screen's work area (640 × 230, 8 px in).
    let shown = bed.shows(|s| s["desktops"]["trial"]["snapped"] == "Left")?;
    assert_eq!(
        shown["desktops"]["trial"]["notes"],
        serde_json::json!([8, 8, 308, 214])
    );
    // Snapping off: against the right side, it stays a window.
    bed.click("control:windows.snap-by-drag")?;
    bed.written(|t| t.contains("snap-by-drag = false"))?;
    bed.trial_drag((100.0, 15.0), (637.0, 120.0))?;
    bed.shows(|s| {
        s["desktops"]["trial"]["preview"].is_null() && s["desktops"]["trial"]["notes"][2] == 184
    })?;
    bed.trial_release()?;
    bed.shows(|s| s["desktops"]["trial"]["snapped"].is_null())?;
    Ok(())
}

/// The file with a rule of Courrier's.
const RULED: &str = "# My desktop, by hand.\n[keyboard]\nlayouts = [\"us\"]\n\n\
                     [desktops]\ncount = 4\nnames = [\"Travail\", \"Courrier\", \"Musique\"]\n\n\
                     [[windows.rules]]\napp-id = \"org.example.Mail\"\n";

fn rules(shown: &Value) -> Vec<(String, String)> {
    shown["desktops"]["rules"]["rules"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    (
                        r["name"].as_str().unwrap_or_default().to_owned(),
                        r["said"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_rule_added_from_the_menu_writes_its_table_and_opens_its_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(RULED)? else {
        return Ok(());
    };
    // Musique has a window open: it comes first.
    bed.window_on("test.music", None)?;
    bed.open()?;
    bed.click("rules:add")?;
    let shown = bed.shows(|s| {
        s["desktops"]["rules"]["choices"]
            .as_array()
            .is_some_and(|c| c.first().and_then(Value::as_str) == Some("Musique"))
    })?;
    assert_eq!(
        strings(&shown["desktops"]["rules"]["choices"]),
        ["Musique", "Éditeur"],
        "Courrier has its rule"
    );
    bed.click("option:0")?;
    let text = bed.written(|t| t.contains("app-id = \"test.music\""))?;
    assert!(
        text.contains("[[windows.rules]]\napp-id = \"org.example.Mail\"\n\n[[windows.rules]]\napp-id = \"test.music\"\n"),
        "{text}"
    );
    let shown = bed.shows(|s| {
        s["toast"] == "Règle ajoutée : Musique" && s["desktops"]["rules"]["open"] == "test.music"
    })?;
    assert_eq!(
        rules(&shown)[1],
        (
            "Musique".to_owned(),
            "Aucune règle encore : elle s’ouvre comme les autres.".to_owned()
        )
    );
    bed.places_with("rule:1:desktop")?;
    Ok(())
}

#[test]
fn an_open_rules_desktop_size_and_above_are_written_and_the_compositor_follows()
-> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(RULED)? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| !rules(s).is_empty())?;
    assert_eq!(rules(&shown)[0].0, "Courrier");
    bed.click("rule:0:edit")?;
    bed.shows(|s| s["desktops"]["rules"]["open"] == "org.example.Mail")?;
    // "2 · Courrier".
    bed.click("rule:0:desktop")?;
    bed.shows(|s| s["desktops"]["rules"]["menu"] == "rule:0:desktop")?;
    bed.click("option:2")?;
    bed.written(|t| t.contains("desktop = 2"))?;
    // "Maximisée".
    bed.click("rule:0:size")?;
    bed.shows(|s| s["desktops"]["rules"]["menu"] == "rule:0:size")?;
    bed.click("option:1")?;
    bed.written(|t| t.contains("size = \"maximized\""))?;
    bed.click("rule:0:above")?;
    let text = bed.written(|t| t.contains("above = true"))?;
    assert!(
        text.contains("[[windows.rules]]\napp-id = \"org.example.Mail\"\ndesktop = 2\nsize = \"maximized\"\nabove = true\n"),
        "{text}"
    );
    let shown = bed.shows(|s| {
        s["toast"]
            .as_str()
            .is_some_and(|t| t.starts_with("Règle de Courrier : "))
            && rules(s)[0].1.contains("au-dessus")
    })?;
    assert_eq!(
        rules(&shown)[0].1,
        "Toujours sur « Courrier » · s’ouvre maximisée · au-dessus des autres"
    );
    let read = bed.hp.dispatch_until(WAIT, |hp| {
        hp.state()
            .config
            .windows
            .rules
            .first()
            .is_some_and(|r| r.desktop == Some(2) && r.above)
    })?;
    anyhow::ensure!(read, "the compositor did not read the rule");
    Ok(())
}

#[test]
fn a_rule_taken_out_and_annuler_brings_it_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(RULED)? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| !rules(s).is_empty())?;
    bed.click("rule:0:remove")?;
    let text = bed.written(|t| !t.contains("rules"))?;
    assert!(text.starts_with("# My desktop, by hand."), "{text}");
    bed.shows(|s| s["toast"] == "Règle de Courrier retirée" && rules(s).is_empty())?;
    bed.click("toast-action")?;
    bed.written(|t| t.contains("[[windows.rules]]\napp-id = \"org.example.Mail\""))?;
    bed.shows(|s| rules(s).len() == 1)?;
    Ok(())
}

#[test]
fn a_desktop_taken_out_moves_the_rules_with_its_windows() -> anyhow::Result<()> {
    let text =
        format!("{RULED}desktop = 2\n\n[[windows.rules]]\napp-id = \"test.music\"\ndesktop = 3\n");
    let Some(mut bed) = Bed::start(&text)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("remove:1")?;
    let text = bed.written(|t| t.contains("count = 3"))?;
    assert!(
        text.contains("app-id = \"org.example.Mail\"\ndesktop = 1\n"),
        "Courrier's rule to Travail, where its windows went:\n{text}"
    );
    assert!(
        text.contains("app-id = \"test.music\"\ndesktop = 2\n"),
        "{text}"
    );
    // Annuler: back where they were.
    bed.click("toast-action")?;
    let text = bed.written(|t| t.contains("count = 4"))?;
    assert!(text.contains("desktop = 3"), "{text}");
    Ok(())
}

#[test]
fn searching_regle_finds_the_rules() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(RULED)? else {
        return Ok(());
    };
    bed.open()?;
    bed.search_for("app-id")?;
    let shown = bed.shows(|s| {
        s["query"] == "app-id" && s["results"].as_array().is_some_and(|r| !r.is_empty())
    })?;
    let names: Vec<&str> = shown["results"]
        .as_array()
        .map(|a| a.iter().filter_map(|r| r["name"].as_str()).collect())
        .unwrap_or_default();
    let i = names
        .iter()
        .position(|n| *n == "Règles par application")
        .ok_or_else(|| anyhow::anyhow!("{names:?}"))?;
    bed.click(&format!("result:{i}"))?;
    bed.shows(|s| s["page"] == "desktops" && s["lit"] == "desktops.rules")?;
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
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.window_on("test.mail", Some(KEY_2))?;
    bed.window_on("test.mail2", Some(KEY_2))?;
    bed.open()?;
    bed.shows(|s| s["desktops"]["windows"] == serde_json::json!([1, 2, 0, 0]))?;
    save(&mut bed, &out, "desktops-page.png", true)?;
    // The pointer on "Courrier": its tooltip.
    let [x, y, w, h] = bed.place_of("chip:1")?;
    let window = bed.window()?;
    bed.hp.pointer_move(
        f64::from(window.x) + x + w / 2.0,
        f64::from(window.y) + y + h / 2.0,
    );
    save(&mut bed, &out, "desktops-tooltip.png", false)?;
    // The trial: Notes stuck to the Terminal, then over the left half.
    let grab = (NOTES.0 + 40.0, NOTES.1 + 10.0);
    let to = (grab.0 - (NOTES.0 - TERMINAL_RIGHT - 8.0 - 10.0), grab.1);
    bed.trial_drag(grab, to)?;
    bed.shows(|s| s["desktops"]["trial"]["notes"][0] == 242)?;
    save(&mut bed, &out, "desktops-trial-stuck.png", false)?;
    let at = (242.0 + 40.0, NOTES.1 + 10.0);
    let a = bed.trial_point(at)?;
    let b = bed.trial_point((3.0, 120.0))?;
    for i in 1..=10 {
        let t = f64::from(i) / 10.0;
        bed.hp
            .pointer_move(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        wait(&mut bed.hp, Duration::from_millis(16))?;
    }
    bed.shows(|s| s["desktops"]["trial"]["preview"].is_array())?;
    save(&mut bed, &out, "desktops-trial-preview.png", false)?;
    bed.trial_release()?;
    bed.reveal("row:desktops.names")?;
    bed.click("chip:3")?;
    bed.shows(|s| s["desktops"]["editing"].is_array())?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("Projet")?;
    bed.shows(|s| s["desktops"]["editing"][1] == "Projet")?;
    save(&mut bed, &out, "desktops-rename.png", true)?;
    bed.key(KEY_ENTER)?;
    bed.written(|t| t.contains("\"Projet\""))?;
    bed.click("remove:1")?;
    bed.shows(|s| {
        s["toast"]
            .as_str()
            .is_some_and(|t| t.starts_with("« Courrier » retiré"))
    })?;
    save(&mut bed, &out, "desktops-removed.png", true)?;
    bed.choose("windows.move-modifier", 1)?;
    bed.shows(|s| s["desktops"]["ink_off"].is_string())?;
    bed.reveal("row:ink.enabled")?;
    save(&mut bed, &out, "desktops-super-ink-off.png", true)?;
    Ok(())
}

/// The rules' screenshots, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn rules_screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let text = format!(
        "{RULED}desktop = 2\nabove = true\n\n[[windows.rules]]\napp-id = \"test.editor\"\nsize = \"right\"\n"
    );
    let Some(mut bed) = Bed::start(&text)? else {
        return Ok(());
    };
    bed.window_on("test.music", None)?;
    bed.open()?;
    bed.shows(|s| rules(s).len() == 2)?;
    bed.click("rule:0:edit")?;
    bed.shows(|s| s["desktops"]["rules"]["open"] == "org.example.Mail")?;
    bed.reveal("rules:add")?;
    save(&mut bed, &out, "desktops-rules.png", true)?;
    bed.click("rule:0:desktop")?;
    bed.shows(|s| s["desktops"]["rules"]["menu"] == "rule:0:desktop")?;
    save(&mut bed, &out, "desktops-rule-desktop-menu.png", true)?;
    chord(&mut bed.hp, &[common::KEY_ESC]);
    bed.shows(|s| s["desktops"]["rules"]["menu"].is_null())?;
    bed.click("rules:add")?;
    bed.shows(|s| {
        s["desktops"]["rules"]["choices"]
            .as_array()
            .is_some_and(|c| c.first().and_then(Value::as_str) == Some("Musique"))
    })?;
    save(&mut bed, &out, "desktops-rule-add-menu.png", true)?;
    Ok(())
}
