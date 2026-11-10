//! Souris et pavé tactile (6.12, issue #446): the real `hocus-settings`
//! window, a client of a headless compositor, its configuration file the
//! test's own, its state, home and runtime directories too, no bus, no
//! GPU. Headless has no libinput: the touchpad and the mouse are the
//! harness's, which keep what the compositor gives them of `[pointer]` and
//! `[touchpad]` (what `hocus msg input-devices` says).
//!
//! One test per point of the spec: the page lists the touchpad and the
//! mouse, by kind and bus; unplugging the mouse leaves one line in its
//! group, and plugging one brings its rows back with the file's values;
//! natural scrolling on the touchpad writes `[touchpad] natural-scroll`,
//! which the compositor gives the touchpad at once; the test area says
//! which device scrolled; tap and drag is inert while tap is off; Annuler
//! takes a touchpad setting back; searching "tap" finds tap to click. And
//! the compositor's side: tap is on by default, a hand edit of the file
//! reaches every device, and `swipe-desktops` off keeps three fingers off
//! the desktops.
//!
//! Then the gestures' group (6.13, issue #447): it says how many fingers
//! the touchpad senses, its rows have their diagrams and 68 px, the last
//! one a note never focused; turning three fingers off writes
//! `[touchpad] swipe-desktops`, which the compositor follows; a touchpad
//! that senses three fingers greys the four-finger row; the diagrams move
//! only while the group shows and the window has the focus, and never
//! with reduced motion.
//!
//! What the window shows is read from its log ("shows" and "places"
//! lines, JSON), never on a time.

use crate::common;
use crate::settings_page;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFTCTRL, WAIT, chord, wait};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, RendererChoice, input_devices::FakeDevice, model::Rect,
};
use hocus_link::input::{Bus, Can, DeviceKind, InputDevice, RightClick};
use serde_json::Value;
use settings_page::{Floor, Wheel};

const APP_ID: &str = "hocus-settings";
const KEY_F: u32 = 33;
const BTN_RIGHT: u32 = 0x111;

/// A configuration of the user's own, with a mouse's setting in it.
const FILE: &str = "# My desktop, by hand.\n\
    [keyboard]\n\
    layouts = [\"us\", \"fr\"]\n\
    \n\
    [pointer]\n\
    left-handed = true # I am\n";

const TOUCHPAD: FakeDevice = FakeDevice {
    kind: DeviceKind::Touchpad,
    bus: Bus::Internal,
    can: Can {
        tap: true,
        clickfinger: true,
        button_areas: true,
        edge: true,
        two_fingers: true,
        dwt: true,
        natural: true,
        accel: true,
        flat: false,
        adaptive: true,
        left_handed: true,
        off_with_mouse: true,
        fingers: 4,
    },
    click_default: Some(RightClick::Corner),
};

const MOUSE: FakeDevice = FakeDevice {
    kind: DeviceKind::Pointer,
    bus: Bus::Usb,
    can: Can {
        tap: false,
        clickfinger: false,
        button_areas: false,
        edge: false,
        two_fingers: false,
        dwt: false,
        natural: true,
        accel: true,
        flat: true,
        adaptive: true,
        left_handed: true,
        off_with_mouse: false,
        fingers: 0,
    },
    click_default: None,
};

/// A US keyboard's letters, by their evdev codes.
fn letter(c: char) -> Option<u32> {
    Some(match c {
        'a' => 30,
        'p' => 25,
        't' => 20,
        'g' => 34,
        'e' => 18,
        's' => 31,
        _ => return None,
    })
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
}

impl Bed {
    /// A compositor, its file holding `file`; `None` when the Settings
    /// are not built.
    fn start(file: &str) -> anyhow::Result<Option<Self>> {
        Self::start_on(file, "1920x1080")
    }

    /// The same on a screen `output` (`WxH[@SCALE]`).
    fn start_on(file: &str, output: &str) -> anyhow::Result<Option<Self>> {
        let program = common::settings_program();
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let config_file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&config_file, file)?;
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
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("bin"),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        // The compositor reads the file as a session does, and follows it.
        let mut config = Config::load(&config_file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(config_file);
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec![output.parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
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

    fn text(&self) -> String {
        std::fs::read_to_string(self.file()).unwrap_or_default()
    }

    /// Launched on the mouse page, its window open with the keyboard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "mouse".to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", self.plain());
        self.shows(|s| s["file"] == "saved" && s["page"] == "mouse")
    }

    fn plain(&self) -> String {
        common::logged(&self.log)
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

    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    /// The middle of `id` on the screen.
    fn at(&mut self, id: &str) -> anyhow::Result<(f64, f64)> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        let window = self.window()?;
        Ok((
            f64::from(window.x) + n(0) + n(2) / 2.0,
            f64::from(window.y) + n(1) + n(3) / 2.0,
        ))
    }

    /// A point over the page, in the window's view whatever the page's
    /// length: where the wheel scrolls it. The page's own middle is not:
    /// at scale 2 it fell below the window, and the wheel turned over the
    /// desktop (#681).
    fn over_page(&mut self) -> anyhow::Result<(f64, f64)> {
        let window = self.window()?;
        Ok((f64::from(window.x) + 632.0, f64::from(window.y) + 400.0))
    }

    fn point(&mut self, id: &str) -> anyhow::Result<()> {
        let at = self.at(id)?;
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))
    }

    fn click_with(&mut self, id: &str, button: u32) -> anyhow::Result<()> {
        self.point(id)?;
        self.hp.button(button, true);
        self.hp.button(button, false);
        Ok(())
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        self.click_with(id, BTN_LEFT)
    }

    /// The row `id` scrolled into the page's view, then clicked.
    fn click_row(&mut self, id: &str) -> anyhow::Result<()> {
        self.click(&format!("control:{id}"))
    }

    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            let code = letter(c).ok_or_else(|| anyhow::anyhow!("no key for {c}"))?;
            self.hp.key(code, true);
            self.hp.key(code, false);
            wait(&mut self.hp, Duration::from_millis(10))?;
        }
        Ok(())
    }

    /// Waits for the file to hold `done`.
    fn written(&mut self, done: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let file = self.file();
        let seen = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| done(&t))
        })?;
        let text = self.text();
        anyhow::ensure!(
            seen,
            "the file never held it:\n{text}\nthe window said:\n{}",
            self.plain()
        );
        Ok(text)
    }

    /// Waits for the compositor's device `id` to hold `done`.
    fn given(&mut self, id: u64, done: impl Fn(&InputDevice) -> bool) -> anyhow::Result<()> {
        let seen = self.hp.dispatch_until(WAIT, |hp| {
            hp.input_devices().iter().any(|d| d.id == id && done(d))
        })?;
        anyhow::ensure!(
            seen,
            "device {id} never got it: {:?}",
            self.hp.input_devices()
        );
        Ok(())
    }
}

/// `id`'s row as shown.
fn row(shown: &Value, id: &str) -> Value {
    shown["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

fn ids(shown: &Value) -> Vec<String> {
    shown["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| r["id"].as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_page_lists_the_touchpad_and_the_mouse() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.hp.plug_device(MOUSE);
    bed.open()?;
    let shown = bed.shows(|s| s["mouse"]["mouse"].is_string())?;
    let mouse = &shown["mouse"];
    assert_eq!(mouse["touchpad"], "Intégré au portable");
    assert_eq!(mouse["mouse"], "Souris USB");
    let groups: Vec<&str> = mouse["groups"]
        .as_array()
        .map(|g| g.iter().filter_map(|g| g["id"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(groups, ["touchpad", "mouse", "gestures"]);
    assert_eq!(
        ids(&shown),
        [
            "touchpad.speed",
            "touchpad.tap-to-click",
            "touchpad.tap-and-drag",
            "touchpad.right-click",
            "touchpad.disable-while-typing",
            "touchpad.natural-scroll",
            "touchpad.scroll",
            "touchpad.off-with-mouse",
            "pointer.speed",
            "pointer.acceleration",
            "pointer.natural-scroll",
            "pointer.left-handed",
            "touchpad.swipe-desktops",
            "touchpad.swipe-launcher",
            "touchpad.left-to-applications",
        ]
    );
    // What applies: tap on, the pad's own right click, the file's mouse.
    assert_eq!(row(&shown, "touchpad.tap-to-click")["value"], "oui");
    assert_eq!(
        row(&shown, "touchpad.right-click")["value"],
        "Coin inférieur droit"
    );
    assert_eq!(row(&shown, "touchpad.speed")["value"], "moyenne");
    assert_eq!(row(&shown, "pointer.left-handed")["value"], "oui");
    // Each drawn in its group, the touchpad's first.
    let places = bed.places_with("device:mouse")?;
    let y = |id: &str| places[id][1].as_f64().unwrap_or(-1.0);
    assert!(y("device:touchpad") < y("device:mouse"), "{places}");
    Ok(())
}

#[test]
fn unplugging_the_mouse_leaves_one_line_in_its_group() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    let mouse = bed.hp.plug_device(MOUSE);
    bed.open()?;
    bed.shows(|s| s["mouse"]["mouse"] == "Souris USB")?;
    bed.hp.unplug_device(mouse);
    let shown = bed.shows(|s| s["mouse"]["mouse"].is_null())?;
    assert_eq!(shown["mouse"]["groups"][1]["empty"], true);
    assert!(
        !ids(&shown).iter().any(|id| id.starts_with("pointer.")),
        "{shown}"
    );
    bed.places_with("empty:mouse")?;
    // Its settings stay in the file.
    assert_eq!(bed.text(), FILE);
    // Another mouse, over Bluetooth: its rows, with the file's values.
    bed.hp.plug_device(FakeDevice {
        bus: Bus::Bluetooth,
        ..MOUSE
    });
    let shown = bed.shows(|s| s["mouse"]["mouse"] == "Souris Bluetooth")?;
    assert_eq!(row(&shown, "pointer.left-handed")["value"], "oui");
    assert_eq!(shown["mouse"]["groups"][1]["empty"], false);
    Ok(())
}

#[test]
fn natural_scrolling_on_the_touchpad_writes_touchpad_natural_scroll() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let pad = bed.hp.plug_device(TOUCHPAD);
    bed.given(pad, |d| d.applied.natural == Some(false))?;
    bed.open()?;
    bed.click_row("touchpad.natural-scroll")?;
    let text = bed.written(|t| t.contains("natural-scroll = true"))?;
    assert_eq!(text, format!("{FILE}\n[touchpad]\nnatural-scroll = true\n"));
    let shown = bed.shows(|s| s["toast"].is_string())?;
    assert_eq!(shown["toast"], "Défilement naturel, pavé tactile : oui");
    // The compositor read the file again and gave it to the touchpad.
    bed.given(pad, |d| d.applied.natural == Some(true))?;
    Ok(())
}

#[test]
fn the_test_area_says_which_device_scrolled() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.hp.plug_device(MOUSE);
    let shown = bed.open()?;
    assert_eq!(
        shown["mouse"]["recognised"],
        "Ce que tu fais ici n’est pas écrit"
    );
    // The test area is under the groups: scrolled to with the wheel over
    // the page, then the wheel over its list.
    scroll_to(&mut bed, "try:list")?;
    bed.point("try:list")?;
    bed.hp.wheel(1);
    let shown = bed.shows(|s| s["mouse"]["recognised"] == "Reconnu : molette de la souris")?;
    assert!(
        shown["mouse"]["direction"]
            .as_str()
            .is_some_and(|d| d.starts_with("Molette · sens classique")),
        "{shown}"
    );
    bed.hp.finger_scroll(20.0);
    let shown = bed.shows(|s| s["mouse"]["recognised"] == "Reconnu : pavé tactile")?;
    assert!(
        shown["mouse"]["direction"]
            .as_str()
            .is_some_and(|d| d.starts_with("Deux doigts · sens classique")),
        "{shown}"
    );
    // A press says its button; the file says the mouse is a left-hander's.
    bed.click_with("try:target", BTN_RIGHT)?;
    bed.shows(|s| s["mouse"]["pressed"] == "Clic droit : boutons inversés pour un gaucher")?;
    Ok(())
}

#[test]
fn tap_and_drag_is_inert_while_tap_is_off() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let pad = bed.hp.plug_device(TOUCHPAD);
    let shown = bed.open()?;
    assert_eq!(row(&shown, "touchpad.tap-and-drag")["inert"], false);
    bed.click_row("touchpad.tap-to-click")?;
    bed.written(|t| t.contains("[touchpad]\ntap-to-click = false\n"))?;
    let shown = bed.shows(|s| row(s, "touchpad.tap-and-drag")["inert"] == true)?;
    assert_eq!(row(&shown, "touchpad.tap-to-click")["value"], "non");
    bed.given(pad, |d| d.applied.tap == Some(false))?;
    // Its switch does nothing.
    bed.click_row("touchpad.tap-and-drag")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.shows(|s| s["toast"].is_string())?;
    assert!(!bed.text().contains("tap-and-drag"), "{}", bed.text());
    Ok(())
}

#[test]
fn annuler_takes_the_touchpad_setting_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.hp.plug_device(MOUSE);
    bed.open()?;
    bed.click_row("touchpad.off-with-mouse")?;
    bed.written(|t| t.contains("off-with-mouse = true"))?;
    let shown = bed.shows(|s| s["toast_action"] == "undo")?;
    assert_eq!(
        shown["toast"],
        "Coupé quand une souris est branchée, pavé tactile : oui"
    );
    // The touchpad's line says it is off, a mouse being plugged.
    assert_eq!(shown["mouse"]["touchpad_off"], true);
    bed.click("toast-action")?;
    bed.written(|t| t == FILE)?;
    let shown = bed.shows(|s| s["toast"].as_str().is_some_and(|t| t.starts_with("Annulé")))?;
    assert_eq!(shown["lit"], "touchpad.off-with-mouse");
    assert_eq!(shown["mouse"]["touchpad_off"], false);
    Ok(())
}

#[test]
fn searching_tap_finds_tap_to_click() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.open()?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_F]);
    bed.type_text("tap")?;
    let shown = bed
        .shows(|s| s["query"] == "tap" && s["results"].as_array().is_some_and(|r| !r.is_empty()))?;
    assert_eq!(shown["results"][0]["name"], "Toucher pour cliquer");
    assert_eq!(shown["results"][0]["target"], "touchpad.tap-to-click");
    Ok(())
}

#[test]
fn searching_geste_lists_the_three_gestures_rows_then_the_desktop_keys() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.open()?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_F]);
    bed.type_text("geste")?;
    let shown = bed.shows(|s| {
        s["query"] == "geste" && s["results"].as_array().is_some_and(|r| r.len() >= 5)
    })?;
    let targets: Vec<&str> = shown["results"]
        .as_array()
        .map(|r| r.iter().filter_map(|f| f["target"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        targets,
        [
            "touchpad.swipe-desktops",
            "touchpad.swipe-launcher",
            "touchpad.left-to-applications",
            "shortcuts.desktop-left",
            "shortcuts.desktop-right"
        ],
        "{shown}"
    );
    Ok(())
}

/// The compositor's side, without the window: tap on by default, which
/// libinput's own default is not, and the rest as the pad can take it.
#[test]
fn a_touchpad_gets_tap_to_click_by_default() -> anyhow::Result<()> {
    let mut hp = Headless::start(HeadlessConfig {
        outputs: vec!["1280x800".parse()?],
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })?;
    let pad = hp.plug_device(FakeDevice {
        can: Can {
            edge: false,
            ..TOUCHPAD.can
        },
        ..TOUCHPAD
    });
    let mouse = hp.plug_device(MOUSE);
    let devices = hp.input_devices();
    let pad = devices.iter().find(|d| d.id == pad).cloned();
    let pad = pad.ok_or_else(|| anyhow::anyhow!("no touchpad"))?;
    assert_eq!(pad.applied.tap, Some(true));
    assert_eq!(pad.applied.drag, Some(true));
    assert_eq!(pad.applied.dwt, Some(true));
    assert_eq!(pad.applied.click, None, "the pad's own right click");
    let mouse = devices.iter().find(|d| d.id == mouse).cloned();
    let mouse = mouse.ok_or_else(|| anyhow::anyhow!("no mouse"))?;
    assert_eq!(mouse.applied.tap, None);
    assert_eq!(mouse.applied.left_handed, Some(false));
    // As the control socket says them.
    assert!(pad.to_string().contains(" tap=on "), "{pad}");
    Ok(())
}

#[test]
fn a_hand_edit_of_the_file_reaches_every_device() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    std::fs::write(&file, "[pointer]\nspeed = 0.0\n")?;
    let mut config = Config::load(&file);
    config.file = Some(file.clone());
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1280x800".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    let mouse = hp.plug_device(MOUSE);
    let other = hp.plug_device(MOUSE);
    // As an editor saves: a new file renamed over the old.
    let beside = dir.path().join(".config.toml.swp");
    std::fs::write(
        &beside,
        "[pointer]\nspeed = -0.5\nacceleration = \"flat\"\n",
    )?;
    std::fs::rename(&beside, &file)?;
    let given = hp.dispatch_until(WAIT, |hp| {
        hp.input_devices().iter().all(|d| {
            d.applied.speed == Some(-0.5)
                && d.applied.acceleration == Some(hocus_link::input::Acceleration::Flat)
        })
    })?;
    anyhow::ensure!(given, "{:?}", hp.input_devices());
    assert_eq!(hp.input_devices().len(), 2, "{mouse} and {other}");
    // A file that does not read changes nothing.
    std::fs::write(&file, "[pointer\nspeed = 1\n")?;
    wait(&mut hp, Duration::from_millis(500))?;
    assert!(
        hp.input_devices()
            .iter()
            .all(|d| d.applied.speed == Some(-0.5))
    );
    Ok(())
}

/// The page scrolled by the wheel `times` times three notches, the
/// pointer over it each time.
fn wheel_page(bed: &mut Bed, times: usize, notches: i32) -> anyhow::Result<()> {
    for _ in 0..times {
        let page = bed.over_page()?;
        bed.hp.pointer_move(page.0, page.1);
        bed.hp.wheel(notches);
        wait(&mut bed.hp, Duration::from_millis(40))?;
    }
    Ok(())
}

/// The page scrolled until `id` shows whole, with the wheel over it,
/// each wheel taken by the window before the next (#705): a wheel still
/// on its way when the test reads where to point moved the page after.
fn scroll_to(bed: &mut Bed, id: &str) -> anyhow::Result<()> {
    let wheel = Wheel {
        x: 632.0,
        notches: 3,
        top: 56.0,
        floor: Floor::Margin(40.0),
    };
    settings_page::reveal(&mut bed.hp, &bed.log, APP_ID, id, &wheel)
}

/// The page scrolled down until the gestures' card shows whole.
fn scroll_to_gestures(bed: &mut Bed) -> anyhow::Result<()> {
    scroll_to(bed, "card:gestures")
}

#[test]
fn the_gestures_group_says_the_fingers_and_switches_three_fingers_off() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.open()?;
    let shown = bed.shows(|s| s["mouse"]["gestures"]["fingers"] == 4)?;
    assert_eq!(
        row(&shown, "touchpad.left-to-applications")["value"],
        "Toujours"
    );
    for id in ["touchpad.swipe-desktops", "touchpad.swipe-launcher"] {
        assert_eq!(row(&shown, id)["value"], "oui", "{id}");
        assert_eq!(row(&shown, id)["inert"], false, "{id}");
    }
    // Each row 68 px, a 64 × 44 diagram on its left; the line under the
    // card.
    let places = bed.places_with("diagram:touchpad.left-to-applications")?;
    let n = |id: &str, i: usize| places[id][i].as_f64().unwrap_or_default();
    for id in [
        "touchpad.swipe-desktops",
        "touchpad.swipe-launcher",
        "touchpad.left-to-applications",
    ] {
        let diagram = format!("diagram:{id}");
        assert_eq!((n(&diagram, 2), n(&diagram, 3)), (64.0, 44.0), "{id}");
        assert!(n(&format!("row:{id}"), 3) >= 68.0, "{id}");
    }
    assert!(n("gestures:note", 1) > n("card:gestures", 1) + n("card:gestures", 3) - 1.0);
    // Three fingers off: the line written, the toast, the compositor.
    scroll_to_gestures(&mut bed)?;
    bed.click_row("touchpad.swipe-desktops")?;
    let text = bed.written(|t| t.contains("swipe-desktops = false"))?;
    assert_eq!(
        text,
        format!("{FILE}\n[touchpad]\nswipe-desktops = false\n")
    );
    let shown = bed.shows(|s| s["toast"].is_string())?;
    assert_eq!(shown["toast"], "Trois doigts de côté : non");
    let followed = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.state().config.touchpad.swipe_desktops)?;
    assert!(followed, "the compositor never read it");
    Ok(())
}

#[test]
fn a_three_finger_touchpad_greys_the_four_finger_row() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(FakeDevice {
        can: Can {
            fingers: 3,
            ..TOUCHPAD.can
        },
        ..TOUCHPAD
    });
    bed.open()?;
    let shown = bed.shows(|s| s["mouse"]["gestures"]["fingers"] == 3)?;
    let launcher = row(&shown, "touchpad.swipe-launcher");
    assert_eq!(launcher["inert"], true);
    assert_eq!(
        launcher["not_here"],
        "Pas sur ce pavé tactile : il ne sent que trois doigts."
    );
    // Its value stays written for another touchpad.
    assert_eq!(launcher["value"], "oui");
    assert_eq!(row(&shown, "touchpad.swipe-desktops")["inert"], false);
    // Another touchpad, of two fingers: the largest count counts.
    bed.hp.plug_device(FakeDevice {
        can: Can {
            fingers: 2,
            ..TOUCHPAD.can
        },
        ..TOUCHPAD
    });
    let shown = bed.shows(|s| s["mouse"]["gestures"]["fingers"] == 3)?;
    assert_eq!(
        row(&shown, "touchpad.swipe-desktops")["inert"],
        false,
        "the largest count"
    );
    Ok(())
}

#[test]
fn the_diagrams_move_only_while_shown_and_focused() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.open()?;
    // Below the page's fold: still, and nothing draws.
    let shown = bed.shows(|s| s["mouse"]["gestures"]["fingers"] == 4)?;
    assert_eq!(shown["mouse"]["gestures"]["moving"], false);
    bed.hp.pointer_move(5.0, 5.0);
    wait(&mut bed.hp, Duration::from_millis(1000))?;
    let commits = bed.hp.window_commits();
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.hp.window_commits(), commits, "the window drew at rest");
    // Shown: they move, frame after frame.
    scroll_to_gestures(&mut bed)?;
    bed.shows(|s| s["mouse"]["gestures"]["moving"] == true)?;
    let commits = bed.hp.window_commits();
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.window_commits() > commits + 5)?;
    assert!(drawn, "the diagrams stand still");
    // Another window takes the focus: they stop.
    common::open(&mut bed.hp, "test.other", (300, 200))?;
    bed.shows(|s| s["mouse"]["gestures"]["moving"] == false)?;
    Ok(())
}

#[test]
fn with_reduced_motion_the_diagrams_stand_still() -> anyhow::Result<()> {
    let file = format!("{FILE}\n[appearance]\nreduced-motion = true\n");
    let Some(mut bed) = Bed::start(&file)? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.open()?;
    bed.shows(|s| s["mouse"]["gestures"]["fingers"] == 4)?;
    scroll_to_gestures(&mut bed)?;
    bed.places_with("diagram:touchpad.swipe-desktops")?;
    wait(&mut bed.hp, Duration::from_millis(400))?;
    let shown = bed.shows(|s| s["mouse"].is_object())?;
    assert_eq!(shown["mouse"]["gestures"]["moving"], false);
    let commits = bed.hp.window_commits();
    wait(&mut bed.hp, Duration::from_millis(400))?;
    assert_eq!(bed.hp.window_commits(), commits, "the window drew at rest");
    Ok(())
}

#[test]
fn swipe_desktops_off_keeps_three_fingers_off_the_desktops() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.touchpad.swipe_desktops = false;
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1280x800".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.pointer_move(640.0, 400.0);
    // What moves to the next desktop with the switch on (desktops.rs).
    hp.swipe(3, &[(-30.0, 0.0); 5], 40);
    assert_eq!(hp.desktop(), (0, 0.0), "three fingers moved the desktops");
    Ok(())
}

/// The pull request's screenshots, the window at scale 2 on the CPU:
/// written to `$HOCUS_SHOTS` when it names a folder. Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context as _;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start_on(FILE, "2880x1800@2")? else {
        return Ok(());
    };
    let shoot = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        // Drawn, and still.
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
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let pad = 24;
        let x = (window.x * 2 - pad).max(0) as u32;
        let y = (window.y * 2 - pad).max(0) as u32;
        let cw = ((window.width * 2 + 2 * pad) as u32).min(w as u32 - x);
        let ch = ((window.height * 2 + 2 * pad) as u32).min(h as u32 - y);
        let image = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    bed.hp.plug_device(TOUCHPAD);
    let mouse = bed.hp.plug_device(MOUSE);
    bed.open()?;
    bed.shows(|s| s["mouse"]["mouse"].is_string())?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "mouse-touchpad.png")?;
    // The mouse's group and the gestures: the page scrolled.
    scroll_to(&mut bed, "card:mouse")?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "mouse-mouse-and-gestures.png")?;
    // The test area: scrolled with two fingers, the target double clicked.
    scroll_to(&mut bed, "try:list")?;
    bed.point("try:list")?;
    bed.hp.finger_scroll(40.0);
    bed.shows(|s| s["mouse"]["recognised"] == "Reconnu : pavé tactile")?;
    bed.click("try:target")?;
    bed.click("try:target")?;
    bed.shows(|s| s["mouse"]["lit"] == true)?;
    shoot(&mut bed, "mouse-try-it.png")?;
    // No mouse: one line in its group.
    bed.hp.unplug_device(mouse);
    bed.shows(|s| s["mouse"]["mouse"].is_null())?;
    wheel_page(&mut bed, 12, -3)?;
    for _ in 0..2 {
        bed.hp.wheel(3);
        wait(&mut bed.hp, Duration::from_millis(20))?;
    }
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "mouse-no-mouse.png")?;
    Ok(())
}

/// The gestures' pull request's screenshots (6.13), the window at scale 2
/// on the CPU: written to `$HOCUS_SHOTS` when it names a folder. Run by
/// hand (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_of_the_gestures() -> anyhow::Result<()> {
    use anyhow::Context as _;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let shoot = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        wait(&mut bed.hp, Duration::from_millis(500))?;
        let window = bed.window()?;
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let pad = 24;
        let x = (window.x * 2 - pad).max(0) as u32;
        let y = (window.y * 2 - pad).max(0) as u32;
        let cw = ((window.width * 2 + 2 * pad) as u32).min(w as u32 - x);
        let ch = ((window.height * 2 + 2 * pad) as u32).min(h as u32 - y);
        let image = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    // A touchpad of four fingers and a mouse: the group, its diagrams
    // moving.
    let Some(mut bed) = Bed::start_on(FILE, "2880x1800@2")? else {
        return Ok(());
    };
    bed.hp.plug_device(TOUCHPAD);
    bed.hp.plug_device(MOUSE);
    bed.open()?;
    bed.shows(|s| s["mouse"]["gestures"]["fingers"] == 4)?;
    scroll_to_gestures(&mut bed)?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "gestures.png")?;
    // Three fingers off: the toast.
    bed.click_row("touchpad.swipe-desktops")?;
    bed.shows(|s| s["toast"].is_string())?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "gestures-off.png")?;
    // Searching « geste ».
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_F]);
    bed.type_text("geste")?;
    bed.shows(|s| s["query"] == "geste")?;
    shoot(&mut bed, "gestures-search.png")?;
    drop(bed);
    // A touchpad of three fingers.
    let Some(mut bed) = Bed::start_on(FILE, "2880x1800@2")? else {
        return Ok(());
    };
    bed.hp.plug_device(FakeDevice {
        can: Can {
            fingers: 3,
            ..TOUCHPAD.can
        },
        ..TOUCHPAD
    });
    bed.hp.plug_device(MOUSE);
    bed.open()?;
    bed.shows(|s| s["mouse"]["gestures"]["fingers"] == 3)?;
    scroll_to_gestures(&mut bed)?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "gestures-three-fingers.png")?;
    Ok(())
}
