//! Clavier et raccourcis (6.8, issue #408): the real `hocus-settings`
//! window, a client of a headless compositor, its configuration file the
//! test's own, its state, home and runtime directories too, XKB's registry
//! a few layouts of the test's, no bus, no GPU.
//!
//! One test per point of the spec: the shortcuts are on a sub-page with
//! their own search; a family of nine is one folded row; recording a
//! shortcut suspends the desktop's own (keyboard-shortcuts-inhibit); a
//! combination another action holds shows inline, "Remplacer" writes both
//! lines and the toast's "Annuler" takes both back; a rebinding takes
//! effect live; "Rétablir" takes the line out of the file; Num Lock at
//! login writes `[keyboard] numlock`; a layout added writes the list. And
//! the compositor's side: Num Lock on as the session opens, the keyboard's
//! layouts and repeat followed live.
//!
//! What the window shows is read from its log ("shows" and "places"
//! lines, JSON), never on a time.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFTMETA, WAIT, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, LockState, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_L: u32 = 38;
const KEY_K: u32 = 37;

/// A configuration of the user's own.
const FILE: &str = "# My desktop, by hand.\n\
    [keyboard]\n\
    layouts = [\"us\"]\n";

/// XKB's registry, as xkeyboard-config writes it, cut short.
const REGISTRY: &str = "<?xml version=\"1.0\"?><xkbConfigRegistry version=\"1.1\"><layoutList>\
    <layout><configItem><name>us</name><description>English (US)</description></configItem>\
    <variantList><variant><configItem><name>intl</name>\
    <description>English (US, intl., with dead keys)</description></configItem></variant>\
    </variantList></layout>\
    <layout><configItem><name>de</name><description>German</description></configItem></layout>\
    <layout><configItem><name>fr</name><description>French</description></configItem>\
    <variantList><variant><configItem><name>bepo</name><description>French (BEPO)</description>\
    </configItem></variant></variantList></layout>\
    </layoutList></xkbConfigRegistry>";

/// A US keyboard's letters, by their evdev codes.
fn letter(c: char) -> Option<u32> {
    Some(match c {
        'a' => 30,
        'b' => 48,
        'd' => 32,
        'e' => 18,
        'i' => 23,
        'm' => 50,
        'p' => 25,
        'r' => 19,
        'u' => 22,
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
        let Some(program) = common::settings_program() else {
            eprintln!("hocus-settings is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let config_file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&config_file, file)?;
        let log = root.join("settings.log");
        // XKB's registry, a few layouts of the test's own.
        let registry = root.join("evdev.xml");
        std::fs::write(&registry, REGISTRY)?;
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
                 HOCUS_XKB_REGISTRY='{}' HOCUS_SYSTEM_KEYBOARD=/nonexistent \
                 HOCUS_LOCALE_DIR=/nonexistent \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("bin"),
                registry.display(),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        // The compositor reads the file as a session does, and follows it.
        let mut config = Config::load(&config_file);
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

    /// Launched on the keyboard's page, its window open with the keyboard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "keyboard".to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", self.plain());
        let shown = self.shows(|s| s["file"] == "saved" && s["page"] == "keyboard")?;
        // Laid out and drawn, and still: where its pieces are holds.
        self.places_with("page:keyboard")?;
        let mut last = self.hp.window_commits();
        let mut still = 0;
        for _ in 0..100 {
            if still >= 3 {
                break;
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
            let now = self.hp.window_commits();
            still = if now == last { still + 1 } else { 0 };
            last = now;
        }
        Ok(shown)
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

    /// The window's places, once they hold `id`.
    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> { common::last_json(log, " places {") };
        // What held, not what is read again after.
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

    /// `id` scrolled into the window's view with the wheel, over the page.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        let mut last = self.place_of(id)?;
        for _ in 0..60 {
            let window = self.window()?;
            let (top, bottom) = (last[1], last[1] + last[3]);
            let notches = if bottom > f64::from(window.height) - 16.0 {
                2
            } else if top < 56.0 {
                -2
            } else {
                // In view, and still there a moment later: the page has
                // stopped moving.
                wait(&mut self.hp, Duration::from_millis(100))?;
                let now = self.place_of(id)?;
                if now == last {
                    return Ok(());
                }
                last = now;
                continue;
            };
            self.hp.pointer_move(
                f64::from(window.x) + 900.0,
                f64::from(window.y) + f64::from(window.height) / 2.0,
            );
            self.hp.wheel(notches);
            // Until the window says it moved (or did not, at an end).
            let before = last;
            let log = self.log.clone();
            let key = id.to_owned();
            self.hp.dispatch_until(Duration::from_secs(2), |_| {
                read_place(&log, &key).is_some_and(|p| p != before)
            })?;
            last = self.place_of(id)?;
        }
        anyhow::bail!("{id} never came into view")
    }

    /// `id`'s place in the window, as the log last said it.
    fn place_of(&mut self, id: &str) -> anyhow::Result<[f64; 4]> {
        self.places_with(id)?;
        read_place(&self.log, id).ok_or_else(|| anyhow::anyhow!("{id} is nowhere"))
    }

    fn point(&mut self, id: &str) -> anyhow::Result<()> {
        self.reveal(id)?;
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
}

/// `id`'s place in the window in the log's last "places" line.
fn read_place(log: &Path, id: &str) -> Option<[f64; 4]> {
    let places = common::last_json(log, " places {")?;
    let place = places.get(id)?;
    let n = |i: usize| place[i].as_f64();
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

/// The shortcuts' row `id`, as shown.
fn shortcut(shown: &Value, id: &str) -> Value {
    shown["keys"]["shortcuts"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

fn shortcut_ids(shown: &Value) -> Vec<String> {
    shown["keys"]["shortcuts"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter_map(|r| r["id"].as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Super and `key`, pressed and let go.
fn super_and(hp: &mut Headless, key: u32) -> anyhow::Result<()> {
    hp.key(KEY_LEFTMETA, true);
    wait(hp, Duration::from_millis(20))?;
    hp.key(key, true);
    hp.key(key, false);
    wait(hp, Duration::from_millis(20))?;
    hp.key(KEY_LEFTMETA, false);
    wait(hp, Duration::from_millis(20))
}

impl Bed {
    /// The shortcuts' sub-page, opened from its row.
    fn shortcuts(&mut self) -> anyhow::Result<Value> {
        self.click("keyboard:shortcuts")?;
        self.shows(|s| {
            s["keys"]["sub"] == true
                && s["keys"]["shortcuts"]
                    .as_array()
                    .is_some_and(|r| !r.is_empty())
        })
    }

    /// `id`'s combination `index` clicked: recording, the desktop's
    /// shortcuts suspended.
    fn record(&mut self, id: &str, index: usize) -> anyhow::Result<()> {
        self.click(&format!("chord:{id}:{index}"))?;
        self.shows(|s| s["keys"]["recording"]["setting"] == id)?;
        let taken = self
            .hp
            .dispatch_until(WAIT, |hp| hp.shortcuts_inhibited())?;
        anyhow::ensure!(
            taken,
            "the desktop's shortcuts were never suspended: {}",
            self.plain()
        );
        Ok(())
    }
}

#[test]
fn the_shortcuts_are_on_a_sub_page_with_their_own_search() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["keys"]["sub"], false);
    let ids: Vec<&str> = shown["rows"]
        .as_array()
        .map(|r| r.iter().filter_map(|r| r["id"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        ids,
        [
            "keyboard.layouts",
            "keyboard.switch",
            "keyboard.repeat-delay",
            "keyboard.repeat-rate",
            "keyboard.caps",
            "keyboard.compose",
            "keyboard.numlock"
        ]
    );
    let shown = bed.shortcuts()?;
    // 64 actions: three families of nine folded, 37 others.
    assert_eq!(shortcut_ids(&shown).len(), 40, "{:?}", shortcut_ids(&shown));
    // Its own search: Print's.
    bed.click("shortcuts:search")?;
    bed.type_text("impr")?;
    let shown = bed.shows(|s| s["keys"]["query"] == "impr")?;
    assert_eq!(
        shortcut_ids(&shown),
        [
            "shortcuts.capture",
            "shortcuts.capture-screen",
            "shortcuts.capture-window"
        ]
    );
    // Back to the keyboard's page.
    bed.click("shortcuts:back")?;
    bed.shows(|s| s["keys"]["sub"] == false)?;
    Ok(())
}

#[test]
fn desktops_one_to_nine_are_one_folded_row() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shortcuts()?;
    let family = shortcut(&shown, "family:desktop-");
    assert_eq!(family["keys"], "Super + 1 … 9");
    assert_eq!(family["unfolded"], false);
    assert_eq!(shortcut(&shown, "shortcuts.desktop-3"), Value::Null);
    bed.click("fold:desktop-")?;
    let shown = bed.shows(|s| shortcut(s, "family:desktop-")["unfolded"] == true)?;
    for n in 1..=9 {
        let member = shortcut(&shown, &format!("shortcuts.desktop-{n}"));
        assert_eq!(member["keys"][0], format!("Super + {n}"));
    }
    // One of the nine changed by hand: each its own.
    let text = format!("{FILE}\n[shortcuts]\ndesktop-3 = \"Super+F3\"\n");
    std::fs::write(bed.file(), &text)?;
    let shown = bed.shows(|s| shortcut(s, "family:desktop-")["keys"].is_null())?;
    assert_eq!(shortcut(&shown, "shortcuts.desktop-3")["changed"], true);
    Ok(())
}

#[test]
fn recording_a_shortcut_suspends_the_desktops_own_and_a_conflict_shows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.shortcuts()?;
    bed.record("shortcuts.terminal", 0)?;
    // Super+L, the lock's: the session does not lock, the window hears it.
    super_and(&mut bed.hp, KEY_L)?;
    let shown = bed.shows(|s| s["keys"]["conflict"].is_object())?;
    assert_eq!(bed.hp.lock_state(), LockState::Unlocked);
    assert_eq!(shown["keys"]["conflict"]["other"], "shortcuts.lock");
    assert_eq!(shown["keys"]["conflict"]["chord"], "Super+L");
    let places = bed.places_with("conflict:text")?;
    assert!(places.get("conflict:replace").is_some(), "{places}");
    // "Remplacer" writes both lines.
    bed.click("conflict:replace")?;
    let text = bed.written(|t| t.contains("terminal = \"Super+L\""))?;
    assert!(text.contains("lock = \"\""), "{text}");
    let shown = bed.shows(|s| s["toast"].is_string())?;
    assert_eq!(
        shown["toast"],
        "Ouvrir un terminal : Super + L · « Verrouiller la session » n’en a plus"
    );
    // The desktop's shortcuts are back.
    let back = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.shortcuts_inhibited())?;
    anyhow::ensure!(back, "the desktop's shortcuts stayed suspended");
    // The toast's "Annuler" takes both back: the file as it was.
    bed.click("toast-action")?;
    bed.written(|t| t == FILE)?;
    Ok(())
}

#[test]
fn a_rebinding_takes_effect_live() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.shortcuts()?;
    bed.record("shortcuts.lock", 0)?;
    super_and(&mut bed.hp, KEY_K)?;
    let text = bed.written(|t| t.contains("lock = \"Super+K\""))?;
    assert_eq!(text, format!("{FILE}\n[shortcuts]\nlock = \"Super+K\"\n"));
    let chord: hocus_compositor::Chord = "Super+K".parse()?;
    let taken = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.state().config.shortcuts.lock == [chord])?;
    anyhow::ensure!(taken, "the compositor never took the new shortcut");
    let back = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.shortcuts_inhibited())?;
    anyhow::ensure!(back, "the desktop's shortcuts stayed suspended");
    // Super+L no longer locks; Super+K does.
    super_and(&mut bed.hp, KEY_L)?;
    wait(&mut bed.hp, Duration::from_millis(200))?;
    assert_eq!(bed.hp.lock_state(), LockState::Unlocked);
    super_and(&mut bed.hp, KEY_K)?;
    let locked = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.lock_state() != LockState::Unlocked)?;
    anyhow::ensure!(locked, "Super+K did not lock");
    Ok(())
}

#[test]
fn retablir_takes_the_line_out_of_the_file() -> anyhow::Result<()> {
    let file = format!("{FILE}\n[shortcuts]\nlock = \"Super+K\"\nclose = \"Super+W\" # mine\n");
    let Some(mut bed) = Bed::start(&file)? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shortcuts()?;
    assert_eq!(shown["keys"]["changed"], 2);
    let lock = shortcut(&shown, "shortcuts.lock");
    assert_eq!(lock["changed"], true);
    assert_eq!(lock["keys"][0], "Super + K");
    bed.click("reset:shortcuts.lock")?;
    let text = bed.written(|t| !t.contains("lock ="))?;
    assert_eq!(text, file.replace("lock = \"Super+K\"\n", ""));
    let shown = bed.shows(|s| shortcut(s, "shortcuts.lock")["changed"] == false)?;
    assert_eq!(shortcut(&shown, "shortcuts.lock")["keys"][0], "Super + L");
    // "Tout rétablir": the other one too, and its table with it.
    bed.click("shortcuts:reset-all")?;
    let text = bed.written(|t| !t.contains("close ="))?;
    // Its table, empty, taken out with it: the file as it was before.
    assert_eq!(text, FILE);
    Ok(())
}

#[test]
fn numlock_at_login_writes_keyboard_numlock() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("control:keyboard.numlock")?;
    let text = bed.written(|t| t.contains("numlock = true"))?;
    assert_eq!(text, format!("{FILE}numlock = true\n"));
    Ok(())
}

#[test]
fn a_layout_added_writes_the_list() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| s["keys"]["layouts"][0]["name"] == "English (US)")?;
    assert_eq!(shown["keys"]["layouts"][0]["chip"], "US");
    bed.click("layouts:add")?;
    bed.shows(|s| s["keys"]["adding"] == "")?;
    bed.type_text("de")?;
    let shown = bed.shows(|s| s["keys"]["adding"] == "de")?;
    assert_eq!(shown["keys"]["found"][0], "de");
    bed.click("pick:0")?;
    let text = bed.written(|t| t.contains("\"de\""))?;
    assert!(text.contains("layouts = [\"us\", \"de\"]"), "{text}");
    // The compositor's keymap takes it.
    let taken = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.state().layout_codes == ["us", "de"])?;
    anyhow::ensure!(taken, "{:?}", bed.hp.state().layout_codes);
    Ok(())
}

#[test]
fn numlock_is_on_as_the_session_opens() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    let headless = || -> anyhow::Result<HeadlessConfig> {
        Ok(HeadlessConfig {
            outputs: vec!["1280x800".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        })
    };
    let off = Headless::start_with(headless()?, config.clone())?;
    assert!(!off.num_lock());
    drop(off);
    config.keyboard.numlock = true;
    let on = Headless::start_with(headless()?, config)?;
    assert!(on.num_lock());
    Ok(())
}

#[test]
fn the_keyboards_layouts_and_repeat_follow_the_file() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    std::fs::write(&file, "[keyboard]\nlayouts = [\"us\"]\n")?;
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
    assert_eq!(hp.keyboard_repeat(), (25, 200));
    std::fs::write(
        &file,
        "[keyboard]\nlayouts = [\"us\", \"fr(bepo)\"]\nrepeat_delay = 300\nrepeat_rate = 40\n",
    )?;
    let followed = hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_repeat() == (40, 300) && hp.state().layout_codes == ["us", "fr(bepo)"]
    })?;
    anyhow::ensure!(
        followed,
        "{:?} {:?}",
        hp.keyboard_repeat(),
        hp.state().layout_codes
    );
    // Layouts XKB refuses: the keymap stays.
    std::fs::write(&file, "[keyboard]\nlayouts = [\"nowhere\"]\n")?;
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(hp.state().layout_codes, ["us", "fr(bepo)"]);
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
    let file = "[keyboard]\nlayouts = [\"us\", \"fr(bepo)\"]\noptions = \"compose:ralt\"\n\n\
                [shortcuts]\nterminal = \"Super+T\"\n";
    let Some(mut bed) = Bed::start_on(file, "2880x1800@2")? else {
        return Ok(());
    };
    let shoot = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
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
    bed.open()?;
    bed.shows(|s| s["keys"]["layouts"][1]["name"] == "French (BEPO)")?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "keyboard.png")?;
    bed.click("layouts:add")?;
    bed.type_text("de")?;
    bed.shows(|s| s["keys"]["adding"] == "de")?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "keyboard-add-layout.png")?;
    bed.hp.key(common::KEY_ESC, true);
    bed.hp.key(common::KEY_ESC, false);
    bed.shows(|s| s["keys"]["adding"].is_null())?;
    // The sub-page's 64 rows take the CPU a few seconds at scale 2.
    let settle = |bed: &mut Bed| -> anyhow::Result<()> {
        bed.hp.pointer_move(5.0, 5.0);
        wait(&mut bed.hp, Duration::from_secs(6))
    };
    bed.shortcuts()?;
    settle(&mut bed)?;
    shoot(&mut bed, "shortcuts.png")?;
    // Super+L for "Fermer la fenêtre": the lock's.
    bed.record("shortcuts.close", 0)?;
    super_and(&mut bed.hp, KEY_L)?;
    bed.shows(|s| s["keys"]["conflict"].is_object())?;
    settle(&mut bed)?;
    shoot(&mut bed, "shortcuts-conflict.png")?;
    bed.click("conflict:cancel")?;
    bed.shows(|s| s["keys"]["conflict"].is_null())?;
    // The desktops' group, its family unfolded.
    bed.click("shortcuts:search")?;
    bed.type_text("bureau")?;
    bed.shows(|s| s["keys"]["query"] == "bureau")?;
    settle(&mut bed)?;
    shoot(&mut bed, "shortcuts-search.png")?;
    bed.record("shortcuts.desktop-left", 0)?;
    settle(&mut bed)?;
    shoot(&mut bed, "shortcuts-recording.png")?;
    Ok(())
}
