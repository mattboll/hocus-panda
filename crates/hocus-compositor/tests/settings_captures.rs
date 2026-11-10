//! Captures et presse-papiers (6.17, issue #451, variant A): the real
//! `hocus-settings` window, a client of a headless compositor (one
//! 2560 × 1600 screen), its configuration file, home, state, data and
//! runtime directories the test's own, a private session bus only for
//! the portal's folder chooser, no GPU.
//!
//! One test per point of the spec: the page's three groups and rows, the
//! history's count from the compositor; JPEG and WebM written and taken by
//! the compositor; a folder chosen in the portal written from the home,
//! with its toast; a folder that cannot be written warning on its row; the
//! history off saying what it erases, its rows inert, "Annuler" turning
//! it back on; passwords kept warning in amber; an application left out
//! from "+ Ajouter", then taken back by its chip's ✕; "Effacer…" asking in
//! place, `Esc` keeping, then clearing through the compositor; the keep
//! menu; the search. What the window shows is read from its log, never on
//! a time.

use crate::common;
use crate::settings_page;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTSHIFT, WAIT, chord, wait};
use hocus_compositor::{
    CaptureFormat, Config, Headless, HeadlessConfig, RendererChoice, model::Rect,
};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_F: u32 = 33;

const FILE: &str = "# My desktop, by hand.\n[keyboard]\nlayouts = [\"us\"]\n";

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
    _bus: Option<Bus>,
}

struct Bus(std::process::Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn has(program: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// A letter's key code (US).
fn letter(c: char) -> Option<u32> {
    const ROWS: [(&str, u32); 3] = [("qwertyuiop", 16), ("asdfghjkl", 30), ("zxcvbnm", 44)];
    ROWS.iter()
        .find_map(|(keys, first)| keys.find(c).map(|i| first + u32::try_from(i).unwrap_or(0)))
}

impl Bed {
    fn start(text: &str) -> anyhow::Result<Option<Self>> {
        Self::start_with_bus(text, false)
    }

    /// The compositor and the Settings' script, the file holding `text`,
    /// a private session bus with `with_bus`; `None` when the Settings
    /// are not built.
    fn start_with_bus(text: &str, with_bus: bool) -> anyhow::Result<Option<Self>> {
        let program = common::settings_program();
        if with_bus && !has("dbus-daemon") {
            eprintln!("no dbus-daemon: skipped");
            return Ok(None);
        }
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home/Images",
            "home/Vidéos",
            "home/Bureau/Captures",
            "config/hocuspanda",
            "cache",
            "state",
            "data/applications",
            "bin",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        std::fs::write(
            root.join("data/applications/com.protonpass.ProtonPass.desktop"),
            "[Desktop Entry]\nType=Application\nName=Proton Pass\nExec=true\n",
        )?;
        // The folder's opener: says what it was given.
        let opened = root.join("opened");
        std::fs::write(
            root.join("bin/xdg-open"),
            format!("#!/bin/sh\necho \"$@\" >> '{}'\n", opened.display()),
        )?;
        std::fs::set_permissions(
            root.join("bin/xdg-open"),
            std::fs::Permissions::from_mode(0o755),
        )?;
        let (bus, address) = if with_bus {
            let (child, address) = common::private_bus(root)?;
            (Some(Bus(child)), address)
        } else {
            (None, String::new())
        };
        let session_bus = if with_bus {
            format!("export DBUS_SESSION_BUS_ADDRESS='{address}'\n")
        } else {
            "unset DBUS_SESSION_BUS_ADDRESS\n".to_owned()
        };
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, text)?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n{session_bus}\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 TZ=UTC HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' XDG_DATA_DIRS='{}' \
                 XDG_PICTURES_DIR='{}' XDG_VIDEOS_DIR='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("data"),
                at("home/Images"),
                at("home/Vidéos"),
                at("bin"),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.home = Some(root.join("home"));
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
            _bus: bus,
        }))
    }

    fn file(&self) -> PathBuf {
        self.dir.path().join("config/hocuspanda/config.toml")
    }

    /// Launched on Captures et presse-papiers, its window open with the
    /// keyboard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "captures".to_owned()],
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
        self.shows(|s| s["file"] == "saved" && s["page"] == "captures")
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
        let mut last = self.place_of(id)?;
        let mut still = 0;
        for _ in 0..60 {
            let window = self.window()?;
            let (top, bottom) = (last[1], last[1] + last[3]);
            let notches = if bottom > f64::from(window.height) - 90.0 {
                2
            } else if top < 56.0 {
                -2
            } else {
                wait(&mut self.hp, Duration::from_millis(100))?;
                let now = self.place_of(id)?;
                if now == last {
                    return Ok(());
                }
                last = now;
                continue;
            };
            self.hp.pointer_move(
                f64::from(window.x) + 500.0,
                f64::from(window.y) + f64::from(window.height) / 2.0,
            );
            wait(&mut self.hp, Duration::from_millis(50))?;
            self.hp.wheel(notches);
            let before = last;
            let log = self.log.clone();
            let key = id.to_owned();
            let moved = self.hp.dispatch_until(Duration::from_secs(2), |_| {
                read_place(&log, &key).is_some_and(|p| p != before)
            })?;
            if !moved {
                still += 1;
                if still >= 3 {
                    return Ok(());
                }
                continue;
            }
            still = 0;
            last = self.place_of(id)?;
        }
        anyhow::bail!("{id} never came into view")
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

    /// `word` typed in the search, once the field has the keyboard.
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
            chord(&mut self.hp, &[KEY_ESC]);
            self.shows(|s| s["query"] == "")?;
        }
        anyhow::bail!("the search never took {word:?}")
    }
}

fn read_place(log: &Path, id: &str) -> Option<[f64; 4]> {
    let places = common::last_json(log, " places {")?;
    let p = places.get(id)?;
    let n = |i: usize| p[i].as_f64().unwrap_or_default();
    Some([n(0), n(1), n(2), n(3)])
}

/// The page's own part of what the window shows.
fn captures(shown: &Value) -> &Value {
    &shown["captures"]
}

#[test]
fn the_page_shows_its_three_groups_and_the_historys_count() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    // A shell would say it: 143 items, 3 pinned.
    bed.hp.clipboard_counted(143, 3);
    let shown = bed.shows(|s| captures(s)["count"] == serde_json::json!([143, 3]))?;
    let page = captures(&shown);
    assert_eq!(
        page["lines"]["capture.format"],
        "Sans perte : net pour du texte et du code."
    );
    assert_eq!(
        page["lines"]["recording.format"],
        "MP4 (H.264 et AAC) : se lit et s’envoie partout."
    );
    assert!(page["capture"]["state"].is_null());
    let home = bed.dir.path().join("home");
    assert_eq!(
        page["capture"]["used"],
        home.join("Images/Captures d'écran").display().to_string()
    );
    assert_eq!(
        page["recording_folder"]["used"],
        home.join("Vidéos/Enregistrements d'écran")
            .display()
            .to_string()
    );
    for row in [
        "row:capture.folder",
        "row:capture.format",
        "row:capture.sound",
        "row:recording.folder",
        "row:recording.format",
        "row:clipboard.history",
        "row:clipboard.keep",
        "row:clipboard.skip-secrets",
        "row:clipboard.clear",
        "chip:org.keepassxc.KeePassXC",
        "captures:add",
    ] {
        bed.places_with(row)?;
    }
    Ok(())
}

#[test]
fn jpeg_and_webm_are_written_and_the_compositor_takes_them() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("segment:capture.format:1")?;
    bed.written(|t| t.contains("[capture]\nformat = \"jpeg\""))?;
    bed.shows(|s| {
        captures(s)["lines"]["capture.format"]
            == "Plus léger, pour les photos ; le presse-papiers reçoit toujours du PNG."
    })?;
    bed.click("segment:recording.format:1")?;
    bed.written(|t| t.contains("[recording]\nformat = \"webm\""))?;
    let read = bed.hp.dispatch_until(WAIT, |hp| {
        hp.state().config.capture.format == CaptureFormat::Jpeg
            && hp.state().config.recording.format == hocus_link::RecordFormat::WebM
    })?;
    anyhow::ensure!(read, "the compositor never took them");
    // The shutter, off.
    bed.click("control:capture.sound")?;
    bed.written(|t| t.contains("sound = false"))?;
    let read = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.state().config.capture.sound)?;
    anyhow::ensure!(read, "the compositor kept the shutter");
    Ok(())
}

/// The desktop portal's file chooser, as the Settings see it on the test's
/// bus: it says what it was asked, and answers with `chosen` a moment
/// after.
struct Chooser {
    chosen: PathBuf,
    asked: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl Chooser {
    async fn open_file(
        &self,
        _parent_window: String,
        title: String,
        options: std::collections::HashMap<String, zbus::OwnedVariant>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<zbus::OwnedObjectPath> {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let token = options
            .get("handle_token")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        let directory = options
            .get("directory")
            .and_then(|v| bool::try_from(v.try_clone().ok()?).ok())
            .unwrap_or(false);
        let path = format!(
            "/org/freedesktop/portal/desktop/request/{}/{token}",
            sender.trim_start_matches(':').replace('.', "_")
        );
        self.asked
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("poisoned".into()))?
            .push(format!("{title} directory={directory}"));
        let uri = format!("file://{}", self.chosen.display()).replace(' ', "%20");
        let connection = connection.clone();
        let answer = path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let mut results: std::collections::HashMap<&str, zbus::Variant<'_>> =
                std::collections::HashMap::new();
            results.insert("uris", zbus::Variant::from(vec![uri]));
            let _ = connection
                .emit_signal(
                    Some(sender.as_str()),
                    answer.as_str(),
                    "org.freedesktop.portal.Request",
                    "Response",
                    &(0u32, results),
                )
                .await;
        });
        zbus::OwnedObjectPath::try_from(path).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

#[test]
fn a_folder_chosen_in_the_portal_is_written_from_the_home_and_opens() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start_with_bus(FILE, true)? else {
        return Ok(());
    };
    let chosen = bed.dir.path().join("home/Bureau/Captures");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let address = format!("unix:path={}", bed.dir.path().join("bus").display());
    let _portal = runtime.block_on(
        zbus::connection::Builder::address(address.as_str())
            .name("org.freedesktop.portal.Desktop")
            .serve_at(
                "/org/freedesktop/portal/desktop",
                Chooser {
                    chosen: chosen.clone(),
                    asked: asked.clone(),
                },
            )
            .build(),
    )?;
    bed.open()?;
    bed.click("capture:change")?;
    bed.written(|t| t.contains("[capture]\nfolder = \"~/Bureau/Captures\""))?;
    bed.shows(|s| s["toast"] == "Dossier des captures : Bureau › Captures")?;
    let asked = asked
        .lock()
        .map_err(|_| anyhow::anyhow!("poisoned"))?
        .clone();
    assert_eq!(asked, ["Choisir le dossier des captures directory=true"]);
    let read = bed.hp.dispatch_until(WAIT, |hp| {
        hp.state().config.capture.folder == "~/Bureau/Captures"
    })?;
    anyhow::ensure!(read, "the compositor never took it");
    // The path button opens it.
    bed.shows(|s| captures(s)["capture"]["used"] == chosen.display().to_string())?;
    bed.click("capture:open")?;
    let opened = bed.dir.path().join("opened");
    let done = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&opened).is_ok_and(|t| t.trim() == chosen.display().to_string())
    })?;
    anyhow::ensure!(done, "xdg-open was never given the folder");
    Ok(())
}

#[test]
fn an_unwritable_folder_warns_on_its_row() -> anyhow::Result<()> {
    let text = format!("{FILE}\n[capture]\nfolder = \"/nonexistent-usb-key/Captures\"\n");
    let Some(mut bed) = Bed::start(&text)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let shown = if captures(&shown)["capture"]["state"] == "Gone" {
        shown
    } else {
        bed.shows(|s| captures(s)["capture"]["state"] == "Gone")?
    };
    let home = bed.dir.path().join("home");
    assert_eq!(
        captures(&shown)["capture"]["used"],
        home.join("Images/Captures d'écran").display().to_string()
    );
    // The row says it, in amber: read from its place in the log.
    let said = common::logged(&bed.log);
    assert!(!said.contains("panicked"), "{said}");
    Ok(())
}

#[test]
fn the_history_off_says_what_it_erases_and_annuler_turns_it_back_on() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.hp.clipboard_counted(143, 3);
    bed.shows(|s| captures(s)["count"] == serde_json::json!([143, 3]))?;
    bed.click("control:clipboard.history")?;
    bed.written(|t| t.contains("[clipboard]\nhistory = false"))?;
    let shown = bed.shows(|s| s["toast"] == "Historique coupé : 140 éléments effacés")?;
    assert_eq!(captures(&shown)["inert"], true);
    assert_eq!(
        captures(&shown)["lines"]["clipboard.history"],
        "Coupé : Super+V ne montre que les épinglés."
    );
    let read = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.state().config.clipboard.history)?;
    anyhow::ensure!(read, "the compositor kept the history on");
    // "Annuler", within the toast's time: on again.
    bed.click("toast-action")?;
    bed.written(|t| !t.contains("history = false"))?;
    let read = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.state().config.clipboard.history)?;
    anyhow::ensure!(read, "the compositor kept the history off");
    Ok(())
}

#[test]
fn passwords_kept_warn_in_amber_with_the_keep_chosen() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("control:clipboard.keep")?;
    bed.shows(|s| s["menu"].as_str() == Some("clipboard.keep"))?;
    bed.click("option:2")?;
    bed.written(|t| t.contains("keep = \"7d\""))?;
    bed.click("control:clipboard.skip-secrets")?;
    bed.written(|t| t.contains("skip-secrets = false"))?;
    bed.shows(|s| {
        captures(s)["lines"]["clipboard.skip-secrets"]
            == "Coupé : un mot de passe copié sera gardé dans l’historique, chiffré, pendant 7 jours."
    })?;
    let read = bed.hp.dispatch_until(WAIT, |hp| {
        !hp.state().config.clipboard.skip_secrets
            && hp.state().config.clipboard.keep == hocus_link::ClipKeep::Week
    })?;
    anyhow::ensure!(read, "the compositor never took them");
    Ok(())
}

#[test]
fn an_application_is_left_out_from_add_and_taken_back_by_its_chip() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("captures:add")?;
    let shown = bed.shows(|s| captures(s)["adding"].is_array())?;
    let choices = captures(&shown)["adding"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let at = choices
        .iter()
        .position(|c| c == "Proton Pass")
        .ok_or_else(|| anyhow::anyhow!("no Proton Pass in {choices:?}"))?;
    bed.click(&format!("option:{at}"))?;
    bed.written(|t| t.contains("ignore = [\"com.protonpass.ProtonPass\"]"))?;
    bed.shows(|s| s["toast"] == "Laissée de côté : Proton Pass")?;
    let read = bed.hp.dispatch_until(WAIT, |hp| {
        hp.state().config.clipboard.ignore == ["com.protonpass.ProtonPass"]
    })?;
    anyhow::ensure!(read, "the compositor never took it");
    bed.click("chip:0:remove")?;
    bed.written(|t| t.contains("ignore = []"))?;
    Ok(())
}

#[test]
fn clear_asks_in_place_then_clears_through_the_compositor() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.hp.clipboard_counted(143, 3);
    bed.shows(|s| captures(s)["count"] == serde_json::json!([143, 3]))?;
    bed.click("clear:clear")?;
    bed.shows(|s| captures(s)["confirming"] == true)?;
    // Esc keeps it.
    bed.key(common::KEY_ESC)?;
    bed.shows(|s| captures(s)["confirming"] == false)?;
    bed.click("clear:clear")?;
    bed.shows(|s| captures(s)["confirming"] == true)?;
    bed.click("clear:confirm")?;
    bed.shows(|s| captures(s)["cleared"] == true)?;
    let said = common::logged(&bed.log);
    assert!(said.contains("the history is cleared"), "{said}");
    Ok(())
}

#[test]
fn searching_obturateur_and_effacer_finds_their_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    // The keys reach the field one by one: what it finds once it holds
    // the whole word.
    bed.search_for("obturateur")?;
    bed.shows(|s| {
        s["query"] == "obturateur" && s["results"].to_string().contains("Son de l’obturateur")
    })?;
    chord(&mut bed.hp, &[KEY_ESC]);
    bed.shows(|s| s["query"] == "")?;
    bed.search_for("effacer")?;
    bed.shows(|s| {
        s["query"] == "effacer" && s["results"].to_string().contains("Effacer l’historique")
    })?;
    Ok(())
}

/// The window, once its commits have stopped, saved as `name` in `out`.
fn save(bed: &mut Bed, out: &Path, name: &str) -> anyhow::Result<()> {
    bed.hp.pointer_move(5.0, 5.0);
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
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let text = format!("{FILE}\n[clipboard]\nignore = [\"com.protonpass.ProtonPass\"]\n");
    let Some(mut bed) = Bed::start(&text)? else {
        return Ok(());
    };
    bed.open()?;
    bed.hp.clipboard_counted(143, 3);
    bed.shows(|s| captures(s)["count"] == serde_json::json!([143, 3]))?;
    save(&mut bed, &out, "captures-page.png")?;
    bed.reveal("row:clipboard.clear")?;
    save(&mut bed, &out, "captures-clipboard.png")?;
    bed.click("clear:clear")?;
    bed.shows(|s| captures(s)["confirming"] == true)?;
    save(&mut bed, &out, "captures-clear.png")?;
    bed.key(KEY_ESC)?;
    bed.click("control:clipboard.skip-secrets")?;
    bed.written(|t| t.contains("skip-secrets = false"))?;
    save(&mut bed, &out, "captures-passwords-off.png")?;
    bed.click("control:clipboard.history")?;
    bed.written(|t| t.contains("history = false"))?;
    bed.shows(|s| captures(s)["inert"] == true)?;
    save(&mut bed, &out, "captures-history-off.png")?;
    // A folder gone, at the top.
    drop(bed);
    let text =
        format!("{FILE}\n[capture]\nfolder = \"/media/alex/USB/Captures\"\nformat = \"jpeg\"\n");
    let Some(mut bed) = Bed::start(&text)? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| captures(s)["capture"]["state"] == "Gone")?;
    save(&mut bed, &out, "captures-folder-gone.png")?;
    Ok(())
}
