//! The Settings (6.1, issue #401): the real `hocus-settings` window, a
//! client of a headless compositor, its configuration file the test's
//! own, its state, home and runtime directories too; never the machine's
//! power supplies, buses or GPU (it draws with tiny-skia).
//!
//! One test per point of the issue: the window opens with its sidebar's
//! 19 sections; searching "veille" finds "Mettre en veille après", the lid
//! and the power button, and opening a result lights its row; changing the
//! delay writes `[idle] delay` at once, the file's other sections and
//! comments kept; "Annuler" and `Ctrl+Z` take it back; a hand edit of the
//! file shows in the window; a file that does not read turns it read-only
//! with the offer to take back the last version that read, which works; a
//! value not understood warns on its row; a second launch brings the first
//! forward; at rest, the window commits nothing. And of #603: the grip
//! shows on hover at the top edge and a drag on it moves the window; ✕,
//! `Esc` and `Ctrl+W` close it; a change made just before closing is on
//! disk. And of #662: a scroll, a switch, a menu and toasts leave nothing
//! on screen.
//!
//! What the window shows is read from its log, as it says it ("shows" and
//! "places" lines, JSON): the places are the window's own, the window's
//! place on the screen added.

use crate::common;
use crate::settings_page;

use settings_page::{Floor, Wheel};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTSHIFT, WAIT, chord, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_ENTER: u32 = 28;
const KEY_F: u32 = 33;
const KEY_W: u32 = 17;
const KEY_Z: u32 = 44;

/// A configuration of the user's own: comments, sections the Settings do
/// not know, an idle delay of five minutes.
const FILE: &str = "# My desktop, by hand.\n\
    [keyboard]\n\
    layouts = [\"us\", \"fr\"]   # both\n\
    \n\
    # When I step away.\n\
    [idle]\n\
    delay = 300 # five minutes\n\
    \n\
    [panel]\n\
    form = \"full\"\n";

/// A US keyboard's letters, by their evdev codes.
fn letter(c: char) -> Option<u32> {
    Some(match c {
        'a' => 30,
        'c' => 46,
        'e' => 18,
        'i' => 23,
        'l' => 38,
        'o' => 24,
        'r' => 19,
        's' => 31,
        'u' => 22,
        'v' => 47,
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
    /// A compositor, the configuration file holding `file`; `None` when
    /// the Settings are not built.
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
        // A laptop on AC, its battery at 82 % (the kernel's supplies, no
        // UPower): the Énergie page has its battery's rows and its lid.
        for (supply, files) in [
            ("BAT0", [("type", "Battery"), ("capacity", "82")]),
            ("AC", [("type", "Mains"), ("online", "1")]),
        ] {
            let dir = root.join("supplies").join(supply);
            std::fs::create_dir_all(&dir)?;
            for (name, text) in files {
                std::fs::write(dir.join(name), format!("{text}\n"))?;
            }
        }
        // What "Ouvrir le fichier" runs: nothing.
        let opener = root.join("bin/xdg-open");
        std::fs::write(&opener, "#!/bin/sh\nexit 0\n")?;
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR='{}' PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("supplies"),
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
        let mut config = Config::default();
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

    /// The Settings launched as the desktop launches applications: with an
    /// activation token.
    fn launch(&mut self, args: &[&str]) {
        let mut argv = vec![self.script.display().to_string()];
        argv.extend(args.iter().map(|a| (*a).to_owned()));
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv,
        });
    }

    /// Launched, its window open with the keyboard and drawn.
    fn open(&mut self) -> anyhow::Result<()> {
        self.launch(&[]);
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", self.plain());
        self.shows(|s| s["file"] == "saved" || s["file"] == "read-only")?;
        self.places_with("nav:power")?;
        Ok(())
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

    /// The window's place on the screen.
    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    /// `id` scrolled into view, clear of the toast, by the wheel (the
    /// Énergie page is taller than the window, 6.10).
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        let wheel = Wheel {
            x: 600.0,
            notches: 2,
            top: 56.0,
            floor: Floor::Margin(90.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)
    }

    /// The middle of `id` on the screen.
    fn at(&mut self, id: &str) -> anyhow::Result<(f64, f64)> {
        if id.starts_with("control:") || id.starts_with("row:") || id == "restore" {
            self.reveal(id)?;
        }
        let places = self.places_with(id)?;
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

    /// The power page, its rows shown.
    fn power(&mut self) -> anyhow::Result<Value> {
        self.click("nav:power")?;
        let shown = self.shows(|s| {
            s["page"] == "power" && s["rows"].as_array().is_some_and(|r| r.len() == 10)
        })?;
        self.places_with("control:power.suspend-after")?;
        Ok(shown)
    }

    /// `id`'s row as shown.
    fn row(shown: &Value, id: &str) -> Value {
        shown["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["id"] == id))
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// A menu's choice `n` taken: its button clicked, then its option.
    fn choose(&mut self, setting: &str, n: usize) -> anyhow::Result<()> {
        self.click(&format!("control:{setting}"))?;
        self.shows(|s| s["menu"].as_str().is_some_and(|m| m.starts_with(setting)))?;
        self.click(&format!("option:{n}"))?;
        Ok(())
    }
}

/// Every line of `before` is in `after`, in its order.
fn kept(before: &str, after: &str, but: &[&str]) -> bool {
    let mut from = 0;
    for line in before
        .lines()
        .filter(|l| !but.iter().any(|b| l.contains(b)))
    {
        match after[from..].find(line) {
            Some(at) => from += at + line.len(),
            None => return false,
        }
    }
    true
}

#[test]
fn the_window_opens_with_its_sidebars_19_sections() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| s["page"] == "home")?;
    let sections: Vec<&str> = shown["sections"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert_eq!(sections.len(), 19, "{sections:?}");
    assert_eq!(sections[0], "Apparence et langue");
    assert_eq!(sections[3], "Accessibilité");
    assert_eq!(sections[15], "Énergie");
    // Each drawn in the sidebar, in that order, under the home's row.
    let places = bed.places_with("nav:performance")?;
    let y = |id: &str| places[id][1].as_f64().unwrap_or(-1.0);
    let slugs = [
        "home",
        "appearance",
        "wallpaper",
        "bar",
        "accessibility",
        "agenda",
        "screens",
        "sound",
        "keyboard",
        "mouse",
        "network",
        "bluetooth",
        "desktops",
        "applications",
        "agents",
        "captures",
        "power",
        "account",
        "date-and-time",
        "performance",
    ];
    for pair in slugs.windows(2) {
        let (a, b) = (
            y(&format!("nav:{}", pair[0])),
            y(&format!("nav:{}", pair[1])),
        );
        assert!(a >= 0.0 && b > a, "{} at {a}, {} at {b}", pair[0], pair[1]);
    }
    // All of them in the window's 760 px, without scrolling.
    assert!(y("nav:performance") + 30.0 <= 760.0, "{places}");
    let window = bed.window()?;
    assert_eq!((window.width, window.height), (1100, 760));
    // The home: six cards, Énergie's from the file.
    let cards = shown["cards"].as_array().map_or(0, Vec::len);
    assert_eq!(cards, 6, "{shown}");
    Ok(())
}

#[test]
fn searching_veille_finds_the_suspend_delay_the_lid_and_the_button() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_F]);
    bed.type_text("veille")?;
    let shown = bed.shows(|s| {
        s["query"] == "veille" && s["results"].as_array().is_some_and(|r| !r.is_empty())
    })?;
    let names: Vec<&str> = shown["results"]
        .as_array()
        .map(|a| a.iter().filter_map(|r| r["name"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        names,
        [
            "Mettre en veille après",
            "Verrouiller avant la mise en veille",
            "Éteindre l’écran",
            "Couvercle fermé",
            "Bouton d’alimentation"
        ]
    );
    assert_eq!(shown["results"][0]["via"], Value::Null);
    assert_eq!(shown["results"][3]["via"], "veille");
    assert_eq!(shown["results"][4]["via"], "veille");
    // Enter opens the first: its page, its row lit.
    bed.hp.key(KEY_ENTER, true);
    bed.hp.key(KEY_ENTER, false);
    bed.shows(|s| s["page"] == "power" && s["lit"] == "power.suspend-after")?;
    // The list stays: the lid's is one click away.
    bed.click("result:3")?;
    bed.shows(|s| s["page"] == "power" && s["lit"] == "power.lid" && s["query"] == "veille")?;
    // Esc empties the field and brings the sections back.
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    bed.shows(|s| s["query"] == "")?;
    Ok(())
}

/// Keys typed faster than the window reads them (a loaded machine) reach
/// it together, in one read: `Ctrl+F` selects the query before the letters
/// typed after it, which replace it, whether the field had the keyboard or
/// not (#669).
#[test]
fn a_word_typed_at_once_after_ctrl_f_replaces_the_query() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.type_text("veille")?;
    bed.shows(|s| s["query"] == "veille" && s["focus"] == "search")?;
    let at_once = |bed: &mut Bed, word: &str| -> anyhow::Result<()> {
        chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_F]);
        for c in word.chars() {
            let code = letter(c).ok_or_else(|| anyhow::anyhow!("no key for {c}"))?;
            bed.hp.key(code, true);
            bed.hp.key(code, false);
        }
        Ok(())
    };
    // The field has the keyboard.
    at_once(&mut bed, "couleur")?;
    bed.shows(|s| s["query"] == "couleur")?;
    at_once(&mut bed, "veille")?;
    bed.shows(|s| {
        s["query"] == "veille" && s["results"].as_array().is_some_and(|r| !r.is_empty())
    })?;
    // A result opened: the field lets the keyboard go, the query stays.
    bed.click("result:0")?;
    bed.shows(|s| s["page"] == "power" && s["query"] == "veille")?;
    at_once(&mut bed, "couleur")?;
    bed.shows(|s| s["query"] == "couleur")?;
    Ok(())
}

#[test]
fn changing_the_delay_writes_idle_delay_at_once_keeping_the_rest() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.power()?;
    // On AC here: the page shows the values on AC, the idle delay's.
    assert_eq!(shown["source"], "ac");
    assert_eq!(Bed::row(&shown, "power.idle")["value"], "5 min");
    // 15 min, the menu's fifth choice.
    bed.choose("power.idle", 5)?;
    let text = bed.written(|t| t.contains("delay = 900"))?;
    assert_eq!(text, FILE.replace("delay = 300 #", "delay = 900 #"));
    assert!(kept(FILE, &text, &["delay ="]));
    let shown = bed.shows(|s| s["toast"].as_str().is_some())?;
    assert_eq!(
        shown["toast"],
        "Assombrir l’écran après, sur secteur : 15 min"
    );
    assert_eq!(shown["toast_action"], "undo");
    assert_eq!(Bed::row(&shown, "power.idle")["value"], "15 min");
    // A setting the file did not hold: its table made, the rest kept.
    bed.choose("power.lid", 1)?;
    let text = bed.written(|t| t.contains("lid = \"lock\""))?;
    assert!(
        text.starts_with(&FILE.replace("delay = 300 #", "delay = 900 #")),
        "{text}"
    );
    assert!(text.contains("[power]\nlid = \"lock\"\n"), "{text}");
    Ok(())
}

#[test]
fn annuler_and_ctrl_z_take_the_value_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.power()?;
    bed.choose("power.idle", 5)?;
    bed.written(|t| t.contains("delay = 900"))?;
    bed.shows(|s| s["toast_action"] == "undo")?;
    // The toast's "Annuler": the file as it was.
    bed.click("toast-action")?;
    bed.written(|t| t == FILE)?;
    let shown = bed.shows(|s| s["toast"].as_str().is_some_and(|t| t.starts_with("Annulé")))?;
    assert_eq!(
        shown["toast"],
        "Annulé : Assombrir l’écran après, sur secteur revient à 5 min."
    );
    assert_eq!(shown["lit"], "power.idle");
    // Ctrl+Z, the same.
    bed.choose("power.idle", 4)?;
    bed.written(|t| t.contains("delay = 600"))?;
    bed.shows(|s| s["undo"] == 1)?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_Z]);
    bed.written(|t| t == FILE)?;
    bed.shows(|s| Bed::row(s, "power.idle")["value"] == "5 min")?;
    // Ctrl+Shift+Z does it again.
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_Z]);
    bed.written(|t| t.contains("delay = 600"))?;
    Ok(())
}

#[test]
fn a_hand_edit_of_the_file_shows_in_the_window() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.power()?;
    // As an editor saves: a new file renamed over the old.
    let edited = FILE.replace("delay = 300", "delay = 600");
    let beside = bed.dir.path().join("config/hocuspanda/.config.toml.swp");
    std::fs::write(&beside, &edited)?;
    std::fs::rename(&beside, bed.file())?;
    let shown = bed.shows(|s| Bed::row(s, "power.idle")["value"] == "10 min")?;
    assert_eq!(shown["toast"], "config.toml a changé : relu.");
    assert_eq!(shown["toast_action"], "show");
    assert_eq!(shown["lit"], "power.idle");
    // Nothing written over it.
    assert_eq!(bed.text(), edited);
    Ok(())
}

#[test]
fn an_unreadable_file_turns_it_read_only_and_the_last_readable_one_comes_back() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.power()?;
    // The file that read is kept.
    let kept_file = bed
        .dir
        .path()
        .join("state/hocuspanda/config.last-good.toml");
    let seen = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&kept_file).is_ok_and(|t| t == FILE)
    })?;
    anyhow::ensure!(seen, "the last version that read is not kept");
    let broken = format!("{FILE}\n[idle\ndelay = 3\n");
    std::fs::write(bed.file(), &broken)?;
    let shown = bed.shows(|s| s["file"] == "read-only")?;
    assert_eq!(shown["banner"], "config.toml ne se lit plus, ligne 12");
    assert!(
        shown["restore"]
            .as_str()
            .is_some_and(|r| r.starts_with("Reprendre la version de ")),
        "{shown}"
    );
    let rows = shown["rows"].as_array().cloned().unwrap_or_default();
    assert!(rows.iter().all(|r| r["inert"] == true), "{rows:?}");
    // It shows the last version that read, as the desktop keeps it.
    assert_eq!(Bed::row(&shown, "power.idle")["value"], "5 min");
    // Its controls do nothing.
    bed.click("control:power.lid")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let shown = bed.shows(|s| s["file"] == "read-only")?;
    assert_eq!(shown["menu"], Value::Null);
    assert_eq!(bed.text(), broken);
    // Taken back: the broken one kept beside it.
    bed.click("restore")?;
    bed.written(|t| t == FILE)?;
    bed.shows(|s| s["file"] == "saved" && s["banner"].is_null())?;
    let aside =
        std::fs::read_to_string(bed.dir.path().join("config/hocuspanda/config.toml.broken"))?;
    assert_eq!(aside, broken);
    // One click, one restore asked of the worker (#676).
    let log = common::logged(&bed.log);
    let asked = log
        .matches("the last version that read is taken back")
        .count()
        + log.matches("no version to take back").count();
    assert_eq!(asked, 1, "{log}");
    Ok(())
}

#[test]
fn a_value_not_understood_warns_on_its_row() -> anyhow::Result<()> {
    let file = format!("{FILE}\n[power.ac]\nsuspend-after = \"3 heures\" # by hand\n");
    let Some(mut bed) = Bed::start(&file)? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.power()?;
    let row = Bed::row(&shown, "power.suspend-after");
    assert_eq!(
        row["warn"],
        "Dans le fichier : « 3 heures », non compris ; Jamais s’applique."
    );
    assert_eq!(row["value"], "Jamais");
    // Choosing a value writes over that one line.
    bed.choose("power.suspend-after", 3)?;
    let text = bed.written(|t| t.contains("suspend-after = \"1h\""))?;
    assert_eq!(text, file.replace("\"3 heures\"", "\"1h\""));
    let shown = bed.shows(|s| Bed::row(s, "power.suspend-after")["warn"].is_null())?;
    assert_eq!(Bed::row(&shown, "power.suspend-after")["value"], "1 h");
    Ok(())
}

#[test]
fn a_second_launch_brings_the_first_forward() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    // Another window takes the focus.
    let other = common::open(&mut bed.hp, "other", (500, 400))?;
    common::click(
        &mut bed.hp,
        (
            f64::from(other.x + other.width / 2),
            f64::from(other.y + other.height / 2),
        ),
    )?;
    let away = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some("other") && w.keyboard)
    })?;
    anyhow::ensure!(away, "the other window never took the keyboard");
    // Launched again, on Énergie.
    bed.launch(&["power"]);
    let back = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
    })?;
    anyhow::ensure!(back, "the Settings never came forward: {}", bed.plain());
    bed.shows(|s| s["page"] == "power")?;
    let windows = bed
        .hp
        .windows()
        .iter()
        .filter(|w| w.app_id.as_deref() == Some(APP_ID))
        .count();
    assert_eq!(windows, 1);
    assert!(bed.plain().contains("the Settings already run: told them"));
    Ok(())
}

#[test]
fn at_rest_it_commits_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.power()?;
    // Settled: a second without a commit.
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..200 {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 10 {
            break;
        }
    }
    anyhow::ensure!(still >= 10, "the window never came to rest");
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.window_commits(), last, "the window drew at rest");
    Ok(())
}

/// What a frame draws past a piece's bounds (a toast's, a switch's knob's
/// and a menu's shadows, anti-aliased edges, text moved with the page)
/// leaves nothing on screen (#662). On the Power page, a toast, a scroll
/// down and back up, a switch turned off and on and a menu opened and
/// closed; on the Appearance and language page, a toast and a scroll down
/// and back up: once the toasts are gone, the window at scale 2 on the
/// CPU is, pixel for pixel, what it was before them.
#[test]
fn after_a_scroll_round_trip_and_a_toast_the_window_is_as_before() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start_on(FILE, "2880x1800@2")? else {
        return Ok(());
    };
    bed.open()?;
    bed.power()?;
    let window = bed.window()?;
    // The pointer away from the window: nothing under it is hovered.
    bed.hp.pointer_move(5.0, 5.0);
    settings_page::still(&mut bed.hp)?;
    let before = settings_page::window_pixels(&mut bed.hp, 0, 2, window)?;
    // A toast: the file changed by hand, a comment added.
    std::fs::write(bed.file(), format!("{FILE}# Read again.\n"))?;
    bed.shows(|s| s["toast"] == "config.toml a changé : relu.")?;
    settings_page::scroll_round_trip(&mut bed.hp, &bed.log, APP_ID)?;
    // A switch turned off and on again: its knob slides, its shadow with
    // it.
    bed.click("control:power.lock-on-idle")?;
    bed.written(|t| t.contains("lock-on-idle = false"))?;
    settings_page::still(&mut bed.hp)?;
    bed.click("control:power.lock-on-idle")?;
    bed.written(|t| !t.contains("lock-on-idle = false"))?;
    settings_page::still(&mut bed.hp)?;
    // A menu opened and closed, its shadow with it.
    bed.click("control:power.idle")?;
    bed.shows(|s| {
        s["menu"]
            .as_str()
            .is_some_and(|m| m.starts_with("power.idle"))
    })?;
    bed.places_with("option:0")?;
    settings_page::still(&mut bed.hp)?;
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    bed.shows(|s| s["menu"].is_null())?;
    bed.hp.pointer_move(5.0, 5.0);
    // The toasts gone, the lit row out.
    bed.shows(|s| s["toast"].is_null() && s["lit"].is_null())?;
    settings_page::still(&mut bed.hp)?;
    let after = settings_page::window_pixels(&mut bed.hp, 0, 2, window)?;
    let power = settings_page::differ(&before, &after);
    // The Appearance and language page.
    bed.click("nav:appearance")?;
    bed.shows(|s| s["page"] == "appearance")?;
    bed.hp.pointer_move(5.0, 5.0);
    settings_page::still(&mut bed.hp)?;
    let before = settings_page::window_pixels(&mut bed.hp, 0, 2, window)?;
    std::fs::write(bed.file(), format!("{FILE}# Read again, twice.\n"))?;
    bed.shows(|s| s["toast"] == "config.toml a changé : relu.")?;
    settings_page::scroll_round_trip(&mut bed.hp, &bed.log, APP_ID)?;
    bed.shows(|s| s["toast"].is_null() && s["lit"].is_null())?;
    settings_page::still(&mut bed.hp)?;
    let after = settings_page::window_pixels(&mut bed.hp, 0, 2, window)?;
    let appearance = settings_page::differ(&before, &after);
    assert_eq!(
        (power, appearance),
        (0, 0),
        "pixels differ from before on the Power page, on the Appearance page"
    );
    Ok(())
}

/// The grip's middle on the screen: 4 px inside the window's top edge, 4
/// high (windows.md §7).
fn grip_middle(window: Rect) -> (f64, f64) {
    (
        f64::from(window.x) + f64::from(window.width) / 2.0,
        f64::from(window.y) + 6.0,
    )
}

/// How light the screen is at `at` (0 to 255, the brightest channel).
fn lightness(hp: &mut Headless, at: (f64, f64)) -> anyhow::Result<u8> {
    let (w, _, px) = hp.screenshot(0)?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (at.1 as usize * w as usize + at.0 as usize) * 4;
    Ok(px[i..i + 3].iter().copied().max().unwrap_or(0))
}

#[test]
fn the_grip_shows_on_hover_at_the_top_edge_and_a_drag_on_it_moves_the_window() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    // The window asked for the compositor's decorations: the grip is its.
    let id = bed
        .hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(APP_ID))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no settings window"))?;
    let server_side = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.state().has_own_header(id))?;
    anyhow::ensure!(server_side, "the window draws its own header");
    let window = bed.window()?;
    let grip = grip_middle(window);
    // Read off the pill's middle, where the cursor is not drawn.
    let pill = (grip.0 + 18.0, grip.1);
    // Away from the top edge, the window's own dark header there.
    bed.hp.pointer_move(grip.0, grip.1 + 300.0);
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let dark = lightness(&mut bed.hp, pill)?;
    assert!(dark < 80, "the header is light under the grip: {dark}");
    // Over it: the pill shows, light.
    bed.hp.pointer_move(grip.0 - 1.0, grip.1);
    bed.hp.pointer_move(grip.0, grip.1);
    let start = std::time::Instant::now();
    loop {
        bed.hp.dispatch(Duration::from_millis(10))?;
        if lightness(&mut bed.hp, pill)? > 150 {
            break;
        }
        anyhow::ensure!(start.elapsed() < WAIT, "the grip never showed");
    }
    assert_eq!(bed.hp.frame_cursor().as_deref(), Some("grab"));
    // Dragged: the window follows.
    bed.hp.button(BTN_LEFT, true);
    for step in 1..=10 {
        let k = f64::from(step) / 10.0;
        bed.hp.pointer_move(grip.0 - 200.0 * k, grip.1 + 100.0 * k);
        wait(&mut bed.hp, Duration::from_millis(16))?;
    }
    bed.hp.button(BTN_LEFT, false);
    let moved = Rect {
        x: window.x - 200,
        y: window.y + 100,
        ..window
    };
    common::wait_for_rect(&mut bed.hp, APP_ID, moved)?;
    Ok(())
}

/// The window closed, and its program ended.
fn closed(bed: &mut Bed, how: &str) -> anyhow::Result<()> {
    let log = bed.log.clone();
    let ended =
        |log: &Path| std::fs::read_to_string(log).is_ok_and(|t| t.contains("the Settings closed"));
    let gone = bed.hp.dispatch_until(WAIT, |hp| {
        ended(&log)
            && !hp
                .windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID))
    })?;
    anyhow::ensure!(gone, "{how} did not close the window: {}", bed.plain());
    // The next one's log starts afresh.
    std::fs::write(&bed.log, "")?;
    Ok(())
}

#[test]
fn the_close_button_esc_and_ctrl_w_close_the_window() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("close")?;
    closed(&mut bed, "✕")?;
    // Esc, once a menu and the search have each taken their own.
    bed.open()?;
    bed.power()?;
    bed.click("control:power.lid")?;
    bed.shows(|s| s["menu"] == "power.lid")?;
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    bed.shows(|s| s["menu"].is_null())?;
    bed.type_text("veille")?;
    bed.shows(|s| s["query"] == "veille")?;
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    bed.shows(|s| s["query"] == "")?;
    // The field still has the keyboard: Esc leaves it.
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    bed.shows(|s| s["focus"] == "nothing")?;
    assert!(bed.window().is_ok(), "Esc closed a window in use");
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    closed(&mut bed, "Esc")?;
    bed.open()?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_W]);
    closed(&mut bed, "Ctrl+W")?;
    Ok(())
}

#[test]
fn a_change_made_just_before_closing_is_on_disk() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.power()?;
    // The slider is written 450 ms after it stops: closed before that.
    let (x, y) = bed.at("control:power.auto-saver-below")?;
    let places = bed.places_with("control:power.auto-saver-below")?;
    let width = places["control:power.auto-saver-below"][2]
        .as_f64()
        .unwrap_or_default();
    // The slider's right end, 50 %: it is the control's first 220 px.
    let end = x - width / 2.0 + 219.0;
    bed.hp.pointer_move(end - 1.0, y);
    bed.hp.pointer_move(end, y);
    wait(&mut bed.hp, Duration::from_millis(30))?;
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_W]);
    closed(&mut bed, "Ctrl+W")?;
    let text = bed.text();
    assert!(
        text.starts_with(FILE) && text.contains("[power]\nauto-saver-below = 50\n"),
        "{text}"
    );
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
    let file = format!("{FILE}\n[power.battery]\nsuspend-after = \"30m\"\n");
    let Some(mut bed) = Bed::start_on(&file, "2880x1800@2")? else {
        return Ok(());
    };
    let shoot = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        // Drawn, and still (a text field's caret blinks: 3 s at most).
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
        // The window and a little of what is around it, ×2.
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
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "home.png")?;
    // The compositor's grip, the pointer at the top edge (#603).
    let grip = grip_middle(bed.window()?);
    bed.hp.pointer_move(grip.0, grip.1);
    let start = std::time::Instant::now();
    while lightness(&mut bed.hp, ((grip.0 + 18.0) * 2.0, grip.1 * 2.0))? <= 150 {
        anyhow::ensure!(start.elapsed() < WAIT, "the grip never showed");
        bed.hp.dispatch(Duration::from_millis(10))?;
    }
    shoot(&mut bed, "grip.png")?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_F]);
    bed.type_text("veille")?;
    bed.shows(|s| s["results"].as_array().is_some_and(|r| r.len() == 3))?;
    shoot(&mut bed, "search-veille.png")?;
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    bed.power()?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "power.png")?;
    bed.choose("power.suspend-after", 2)?;
    bed.shows(|s| s["toast_action"] == "undo")?;
    bed.hp.pointer_move(5.0, 5.0);
    // Past the toast's rise.
    wait(&mut bed.hp, Duration::from_millis(300))?;
    shoot(&mut bed, "toast.png")?;
    bed.choose("power.idle", 5)?;
    bed.shows(|s| s["menu"].is_null())?;
    bed.click("control:power.lid")?;
    bed.shows(|s| s["menu"] == "power.lid")?;
    bed.places_with("option:0")?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "menu.png")?;
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    // Closed by its Esc before the file breaks, or the Esc would close the
    // window.
    bed.shows(|s| s["menu"].is_null())?;
    std::fs::write(bed.file(), format!("{}\n[idle\ndelay = 3\n", bed.text()))?;
    bed.shows(|s| s["file"] == "read-only")?;
    wait(&mut bed.hp, Duration::from_secs(7))?;
    shoot(&mut bed, "read-only.png")?;
    Ok(())
}
