//! Fond d'écran (6.3, issue #403, variant A, the gallery): the real
//! `hocus-settings` window and the real shell, clients of a headless
//! compositor of two screens, their configuration file, home, data, state
//! and runtime directories the test's own, no session bus (but the test's
//! own, with a fake file chooser, where one is chosen), no GPU.
//!
//! One test per decision: choosing Brume redraws the desktop's
//! background; a picture chosen in the portal's file chooser is copied
//! into `~/.local/share/hocuspanda/wallpapers` and drawn; each screen takes
//! its own; the lock shows the desktop's wallpaper dimmed, or a picture of
//! its own; a swatch colours the plain background; the "wallpaper" accent
//! takes the wallpaper's colour; at rest nothing draws. What the programs
//! show is read from their logs and the screens' pixels, never on a time.

mod common;
mod settings_page;

use settings_page::{Floor, Wheel};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, WAIT, wait};
use hocus_compositor::{Authenticator, Config, Headless, HeadlessConfig, RendererChoice};
use hocus_link::backdrop::Backdrop;
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
/// The screens: the first has the bar and the window.
const SCREENS: [(i32, i32); 2] = [(1920, 1080), (1280, 800)];
/// A corner of each that nothing covers.
const BARE: [(i32, i32); 2] = [(40, 1060), (20, 780)];

const FILE: &str = "# My desktop, by hand.\n[keyboard]\nlayouts = [\"us\"]\n";

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    shell_log: PathBuf,
    log: PathBuf,
    script: PathBuf,
    _bus: Option<Bus>,
}

/// The test's own D-Bus daemon, stopped with the test.
struct Bus(std::process::Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A log without its colours' escape sequences.
fn plain(log: &Path) -> String {
    common::logged(log)
}

fn has(program: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

impl Bed {
    /// The compositor, the shell and the Settings' script, the file
    /// holding `text`; the Settings on the bus at `bus` when given. `None`
    /// when they are not built.
    fn start(text: &str) -> anyhow::Result<Option<Self>> {
        Self::start_with_bus(text, false)
    }

    fn start_with_bus(text: &str, with_bus: bool) -> anyhow::Result<Option<Self>> {
        let (Some(shell), Some(settings)) = (common::shell_program(), common::settings_program())
        else {
            eprintln!(
                "the shell or the Settings are not built: build the workspace to run this test"
            );
            return Ok(None);
        };
        if with_bus && !has("dbus-daemon") {
            eprintln!("no dbus-daemon: skipped");
            return Ok(None);
        }
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home/Images",
            "home/.local/share",
            "config/hocuspanda",
            "state",
            "cache",
            "bin",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let (bus, address) = if with_bus {
            let (child, address) = common::private_bus(root)?;
            (Some(Bus(child)), address)
        } else {
            (None, String::new())
        };
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, text)?;
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
                 XDG_DATA_HOME='{}' TZ=UTC HOCUS_CLOCK_AT='2026-11-05T14:02:00' \
                 HOCUS_USER_NAME=Alex LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
                 PATH='{}:/usr/bin:/bin' RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("home/.local/share"),
                at("bin"),
                shell.display(),
                shell_log.display()
            ),
        )?;
        std::fs::set_permissions(&shell_script, std::fs::Permissions::from_mode(0o755))?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let session_bus = if with_bus {
            format!("export DBUS_SESSION_BUS_ADDRESS='{address}'\n")
        } else {
            "unset DBUS_SESSION_BUS_ADDRESS\n".to_owned()
        };
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n{session_bus}\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 XDG_PICTURES_DIR='{}' XDG_DATA_DIRS='{}' PATH='{}:/usr/bin:/bin' \
                 HOCUS_USER_NAME=Alex RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("home/.local/share"),
                at("home/Images"),
                at("home/.local/share"),
                at("bin"),
                settings.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.home = Some(root.join("home"));
        config.french = true;
        config.lock_auth = Authenticator::test("not a password");
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: SCREENS
                    .iter()
                    .map(|(w, h)| format!("{w}x{h}").parse())
                    .collect::<Result<_, _>>()?,
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
            _bus: bus,
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

    /// Launched on the wallpaper's page, its window open with the keyboard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "wallpaper".to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", plain(&self.log));
        self.shows(|s| s["file"] == "saved" && s["page"] == "wallpaper")
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

    fn place_of(&mut self, id: &str) -> anyhow::Result<[f64; 4]> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        Ok([n(0), n(1), n(2), n(3)])
    }

    /// `id` scrolled into the window's upper part with the wheel.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        let wheel = Wheel {
            x: 900.0,
            notches: 2,
            top: 56.0,
            floor: Floor::AboveToast(16.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        self.reveal(id)?;
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

    fn pixel(&mut self, screen: usize, (x, y): (i32, i32)) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(screen)?;
        let at = usize::try_from(y * width + x)? * 4;
        let [b, g, r, _] = bytes
            .get(at..at + 4)
            .and_then(|p| <[u8; 4]>::try_from(p).ok())
            .ok_or_else(|| anyhow::anyhow!("no pixel at {x}, {y}"))?;
        Ok([r, g, b])
    }

    /// Waits until screen `screen` shows about `like` at its bare corner.
    fn until(&mut self, screen: usize, like: [u8; 3]) -> anyhow::Result<()> {
        let at = BARE[screen];
        let mut seen = [0; 3];
        for _ in 0..400 {
            seen = self.pixel(screen, at)?;
            if near(seen, like) {
                return Ok(());
            }
            self.hp.dispatch(Duration::from_millis(25))?;
        }
        anyhow::bail!("screen {screen} shows {seen:?} at {at:?}, not about {like:?}")
    }
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 12)
}

/// What `builtin` shows on screen `screen` at its bare corner: the
/// gradient a quarter as large, drawn stretched.
fn builtin_at(builtin: Backdrop, screen: usize) -> [u8; 3] {
    let (w, _, pixels) = hocus_link::backdrop::pixels(SCREENS[screen], builtin);
    let (x, y) = BARE[screen];
    let at = ((y / 4) * w + x / 4) as usize * 4;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

fn tile(shown: &Value, id: &str) -> Value {
    shown["wallpaper"]["tiles"]
        .as_array()
        .and_then(|tiles| tiles.iter().find(|t| t["id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn choosing_brume_redraws_the_desktops_background() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.until(1, builtin_at(Backdrop::Night, 1))?;
    let shown = bed.open()?;
    assert_eq!(tile(&shown, "tile:night")["chosen"], true, "{shown}");
    bed.click("tile:mist")?;
    let text = bed.written(|t| t.contains("[wallpaper]\npicture = \"mist\"\n"))?;
    assert!(text.starts_with(FILE), "{text}");
    bed.shows(|s| tile(s, "tile:mist")["chosen"] == true)?;
    // Both screens, at once.
    bed.until(1, builtin_at(Backdrop::Mist, 1))?;
    bed.until(0, builtin_at(Backdrop::Mist, 0))?;
    // And the lock is told.
    bed.shell_said("the wallpapers", &["mist"])?;
    Ok(())
}

/// The desktop portal's file chooser, as the Settings see it on the test's
/// bus: it says what it was asked, and answers with `chosen` a moment
/// after, as a portal does once the user chooses.
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
        let path = format!(
            "/org/freedesktop/portal/desktop/request/{}/{token}",
            sender.trim_start_matches(':').replace('.', "_")
        );
        let filters = options.contains_key("filters");
        self.asked
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("poisoned".into()))?
            .push(format!("{title} filters={filters}"));
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
fn a_picture_chosen_in_the_file_chooser_is_copied_and_drawn() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start_with_bus(FILE, true)? else {
        return Ok(());
    };
    // A red picture in the user's pictures.
    let chosen = bed.dir.path().join("home/Images/Montagne.png");
    image::RgbaImage::from_pixel(64, 40, image::Rgba([220, 40, 40, 255])).save(&chosen)?;
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
    bed.click("tile:choose")?;
    let text = bed.written(|t| {
        t.contains("picture = \"~/.local/share/hocuspanda/wallpapers/Montagne.png\"")
    })?;
    assert!(text.starts_with(FILE), "{text}");
    let asked = asked
        .lock()
        .map_err(|_| anyhow::anyhow!("poisoned"))?
        .clone();
    assert_eq!(asked, ["Choisir une image pour le fond filters=true"]);
    // Copied, the original left where it is.
    let copy = bed
        .dir
        .path()
        .join("home/.local/share/hocuspanda/wallpapers/Montagne.png");
    assert_eq!(std::fs::read(&copy)?, std::fs::read(&chosen)?);
    let shown = bed.shows(|s| tile(s, "tile:mine:Montagne.png")["chosen"] == true)?;
    assert_eq!(
        shown["wallpaper"]["copied"],
        "Copié dans ~/.local/share/hocuspanda/wallpapers/Montagne.png"
    );
    // Drawn covering both screens.
    bed.until(1, [220, 40, 40])?;
    bed.until(0, [220, 40, 40])?;
    Ok(())
}

#[test]
fn each_screen_takes_its_own_background() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("wallpaper-scope:each")?;
    // The first time, each screen's own written from the background.
    bed.written(|t| {
        t.contains("per-screen = true")
            && t.contains("[outputs.HEADLESS-1]\nwallpaper = \"night\"")
            && t.contains("[outputs.HEADLESS-2]\nwallpaper = \"night\"")
    })?;
    bed.shows(|s| s["wallpaper"]["per_screen"] == true)?;
    bed.click("wallpaper-screen:HEADLESS-2")?;
    bed.shows(|s| s["wallpaper"]["target"] == "HEADLESS-2")?;
    bed.click("tile:forest")?;
    bed.written(|t| t.contains("[outputs.HEADLESS-2]\nwallpaper = \"forest\""))?;
    bed.until(1, builtin_at(Backdrop::Forest, 1))?;
    // The other keeps Nuit.
    let first = bed.pixel(0, BARE[0])?;
    assert!(near(first, builtin_at(Backdrop::Night, 0)), "{first:?}");
    let shown = bed.shows(|s| tile(s, "tile:forest")["chosen"] == true)?;
    let captions: Vec<&str> = shown["wallpaper"]["screens"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["picture"].as_str())
        .collect();
    assert_eq!(captions, ["night", "forest"], "{shown}");
    // All screens again: the screens' own kept for later, every screen
    // Nuit.
    bed.click("wallpaper-scope:all")?;
    let text = bed.written(|t| t.contains("per-screen = false"))?;
    assert!(text.contains("wallpaper = \"forest\""), "{text}");
    bed.until(1, builtin_at(Backdrop::Night, 1))?;
    Ok(())
}

#[test]
fn the_lock_shows_the_desktops_wallpaper_dimmed() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    // By hand: Aube; the lock by default: the desktop's, dimmed.
    bed.write(&format!("{FILE}\n[wallpaper]\npicture = \"dawn\"\n"))?;
    bed.until(1, builtin_at(Backdrop::Dawn, 1))?;
    bed.shell_said("the wallpapers", &["dawn"])?;
    bed.hp.lock();
    bed.shell_said(
        "the lock's wallpaper is drawn",
        &["picture=dawn", "blurred=false"],
    )?;
    // Dimmed by the veil: darker than the desktop's.
    let desktop = builtin_at(Backdrop::Dawn, 1);
    let mut darker = false;
    for _ in 0..400 {
        let [r, g, b] = bed.pixel(1, BARE[1])?;
        if !bed.hp.lock_surfaces().is_empty()
            && u32::from(r) + u32::from(g) + u32::from(b) + 20
                < u32::from(desktop[0]) + u32::from(desktop[1]) + u32::from(desktop[2])
            && u32::from(r) + u32::from(g) + u32::from(b) > 0
        {
            darker = true;
            break;
        }
        bed.hp.dispatch(Duration::from_millis(25))?;
    }
    assert!(darker, "the lock never showed Aube dimmed");
    Ok(())
}

#[test]
fn the_lock_takes_a_picture_of_its_own() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["wallpaper"]["lock"], "dimmed");
    bed.click("lock:blurred")?;
    bed.written(|t| t.contains("lock = \"blurred\""))?;
    bed.click("lock:picture")?;
    bed.written(|t| t.contains("lock = \"picture\""))?;
    bed.click("control:wallpaper.lock-picture")?;
    bed.shows(|s| s["wallpaper"]["lock_menu"] == true)?;
    bed.click("lock-option:forest")?;
    bed.written(|t| t.contains("lock-picture = \"forest\""))?;
    bed.shows(|s| {
        s["wallpaper"]["lock_picture"] == "forest" && s["wallpaper"]["lock_drawn"] == true
    })?;
    // The desktop keeps Nuit; the lock shows Forêt.
    bed.shell_said("the wallpapers", &["picture\tforest"])?;
    bed.hp.key(1, true);
    bed.hp.key(1, false);
    bed.hp.lock();
    bed.shell_said(
        "the lock's wallpaper is drawn",
        &["picture=forest", "blurred=false"],
    )?;
    Ok(())
}

#[test]
fn a_swatch_colours_the_plain_background() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["wallpaper"]["colour_row"], false);
    bed.click("tile:plain")?;
    bed.written(|t| t.contains("picture = \"plain\""))?;
    // Ardoise, the default.
    bed.until(1, [0x1B, 0x22, 0x33])?;
    bed.shows(|s| s["wallpaper"]["colour_row"] == true)?;
    bed.click("colour:#1d3a2e")?;
    bed.written(|t| t.contains("color = \"#1d3a2e\""))?;
    let shown = bed.shows(|s| {
        s["rows"].as_array().is_some_and(|r| {
            r.iter()
                .any(|r| r["id"] == "wallpaper.color" && r["value"] == "Sapin")
        })
    })?;
    assert_eq!(shown["toast"], "Couleur unie : Sapin", "{shown}");
    bed.until(1, [0x1D, 0x3A, 0x2E])?;
    bed.until(0, [0x1D, 0x3A, 0x2E])?;
    Ok(())
}

#[test]
fn the_wallpaper_accent_takes_the_wallpapers_colour() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&format!("{FILE}\n[appearance]\naccent = \"wallpaper\"\n"))?
    else {
        return Ok(());
    };
    let hex = |rgb: [u8; 3]| format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    let night = hex(Backdrop::Night.accent().base);
    let forest = hex(Backdrop::Forest.accent().base);
    bed.shell_said("the theme theme=", &["accent=wallpaper", &night])?;
    let shown = bed.open()?;
    assert_eq!(shown["wallpaper"]["accent"], night.as_str());
    bed.click("tile:forest")?;
    bed.written(|t| t.contains("picture = \"forest\""))?;
    // The shell and the window both take Forêt's green.
    bed.shell_said("the theme theme=", &["accent=wallpaper", &forest])?;
    bed.shows(|s| s["wallpaper"]["accent"] == forest.as_str())?;
    // A picture's own hue: a red picture, by hand.
    let red = bed.dir.path().join("home/Images/Rouge.png");
    image::RgbaImage::from_pixel(32, 20, image::Rgba([200, 30, 40, 255])).save(&red)?;
    let text = std::fs::read_to_string(bed.file())?;
    bed.write(&text.replace("\"forest\"", "\"~/Images/Rouge.png\""))?;
    let red_base = hex(hocus_link::theme::AccentColours::lifted([200, 30, 40]).base);
    bed.shell_said("the theme theme=", &["accent=wallpaper", &red_base])?;
    bed.shows(|s| s["wallpaper"]["accent"] == red_base.as_str())?;
    Ok(())
}

#[test]
fn at_rest_the_wallpaper_page_commits_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("tile:mist")?;
    bed.until(1, builtin_at(Backdrop::Mist, 1))?;
    bed.shows(|s| {
        s["wallpaper"]["tiles"].as_array().is_some_and(|t| {
            t.iter()
                .all(|t| t["drawn"] == true || t["id"] == "tile:choose")
        }) && s["wallpaper"]["lock_drawn"] == true
    })?;
    bed.hp.pointer_move(400.0, 1070.0);
    common::at_rest(&mut bed.hp)?;
    bed.hp.hold_clock();
    wait(&mut bed.hp, Duration::from_millis(500))?;
    // The window, the bar, and the backgrounds drawn: none asked again.
    let counts = |hp: &Headless| {
        (
            hp.window_commits(),
            hp.bar_commits(),
            hp.state().backgrounds.borrow().draws,
        )
    };
    let before = counts(&bed.hp);
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert_eq!(counts(&bed.hp), before, "something drew at rest");
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`: the page in its
/// states, and the desktop behind it.
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
    let save = |bed: &mut Bed, name: &str, screen: usize| -> anyhow::Result<()> {
        common::at_rest(&mut bed.hp)?;
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
        let (w, h, bytes) = bed.hp.screenshot(screen)?;
        let rgba: Vec<u8> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    let picture = bed
        .dir
        .path()
        .join("home/.local/share/hocuspanda/wallpapers");
    std::fs::create_dir_all(&picture)?;
    sunset(&picture.join("Montagne.png"))?;
    bed.open()?;
    bed.shows(|s| {
        s["wallpaper"]["tiles"].as_array().is_some_and(|t| {
            t.iter()
                .all(|t| t["drawn"] == true || t["id"] == "tile:choose")
        })
    })?;
    bed.hp.pointer_move(400.0, 1070.0);
    save(&mut bed, "page-night.png", 0)?;
    bed.click("tile:dawn")?;
    bed.until(1, builtin_at(Backdrop::Dawn, 1))?;
    bed.hp.pointer_move(400.0, 1070.0);
    save(&mut bed, "page-dawn.png", 0)?;
    bed.click("wallpaper-scope:each")?;
    bed.shows(|s| s["wallpaper"]["per_screen"] == true)?;
    bed.click("wallpaper-screen:HEADLESS-2")?;
    bed.shows(|s| s["wallpaper"]["target"] == "HEADLESS-2")?;
    bed.click("tile:mine:Montagne.png")?;
    bed.written(|t| t.contains("[outputs.HEADLESS-2]\nwallpaper = \"~/.local/share"))?;
    bed.until(1, sunset_at_bare())?;
    bed.hp.pointer_move(400.0, 1070.0);
    save(&mut bed, "page-each-screen.png", 0)?;
    save(&mut bed, "second-screen-picture.png", 1)?;
    bed.click("lock:picture")?;
    bed.shows(|s| s["wallpaper"]["lock"] == "picture")?;
    bed.click("control:wallpaper.lock-picture")?;
    bed.shows(|s| s["wallpaper"]["lock_menu"] == true)?;
    bed.hp.pointer_move(400.0, 1070.0);
    save(&mut bed, "page-lock-menu.png", 0)?;
    bed.click("lock-option:forest")?;
    bed.shows(|s| {
        s["wallpaper"]["lock_drawn"] == true && s["wallpaper"]["lock_picture"] == "forest"
    })?;
    bed.click("tile:plain")?;
    bed.shows(|s| s["wallpaper"]["colour_row"] == true)?;
    bed.click("colour:#22324d")?;
    bed.until(1, [0x22, 0x32, 0x4D])?;
    bed.hp.pointer_move(400.0, 1070.0);
    save(&mut bed, "page-plain.png", 0)?;
    bed.hp.key(1, true);
    bed.hp.key(1, false);
    bed.hp.lock();
    bed.shell_said("the lock's wallpaper is drawn", &["picture=forest"])?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    save(&mut bed, "lock-forest.png", 1)?;
    Ok(())
}

/// A picture of a sunset, for the screenshots: warm sky, dark hills.
fn sunset(path: &Path) -> anyhow::Result<()> {
    let image = image::RgbaImage::from_fn(640, 400, |x, y| {
        let t = y as f32 / 400.0;
        let hill = 260.0 + 40.0 * ((x as f32) / 90.0).sin();
        if (y as f32) > hill {
            image::Rgba([35, 31, 56, 255])
        } else {
            let sky = |a: f32, b: f32| (a + (b - a) * t) as u8;
            image::Rgba([sky(246.0, 154.0), sky(195.0, 95.0), sky(139.0, 134.0), 255])
        }
    });
    image.save(path)?;
    Ok(())
}

fn sunset_at_bare() -> [u8; 3] {
    // The bottom-left corner is a hill.
    [35, 31, 56]
}
