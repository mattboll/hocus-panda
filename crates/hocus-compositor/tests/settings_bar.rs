//! La barre (6.4, issue #404, variant A): the real `hocus-settings`
//! window, a client of a headless compositor of two screens (a laptop's
//! 2560 × 1600 at 1, a 3440 × 1440 beside it), its configuration file,
//! home, state and runtime directories the test's own, no bus, no GPU.
//!
//! One test per point of the spec: the page's groups, a row per screen
//! and the preview; a screen's form written at once (the bar follows) and
//! taken back; the click's form forgotten with "Revenir à la forme de sa
//! largeur"; a group moved up, the keyboard following its button; a group
//! hidden, and the amber line of one with nothing to show; the clock's
//! rows and a colour of the file's own; the search; the links to the
//! other pages with their rows lit; at rest only the preview's clock
//! draws. What the window shows is read from its log, never on a time.

mod common;
mod settings_page;

use settings_page::{Floor, Wheel};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFTCTRL, KEY_LEFTMETA, WAIT, chord, wait};
use hocus_compositor::{BarForm, Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const LAPTOP: &str = "HEADLESS-1";
const EXTERNAL: &str = "HEADLESS-2";
const KEY_B: u32 = 48;
const KEY_F: u32 = 33;
const KEY_TAB: u32 = 15;
const KEY_SPACE: u32 = 57;

/// With a calendar and a task board: nothing to warn about.
const FILE: &str = "# My desktop, by hand.\n[keyboard]\nlayouts = [\"us\"]\n\n\
                    [[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\n\
                    [[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
}

fn letter(c: char) -> Option<u32> {
    Some(match c {
        'a' => 30,
        'b' => 48,
        'c' => 46,
        'd' => 32,
        'e' => 18,
        'g' => 34,
        'h' => 35,
        'n' => 49,
        'o' => 24,
        'p' => 25,
        'r' => 19,
        's' => 31,
        't' => 20,
        'u' => 22,
        _ => return None,
    })
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
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, text)?;
        std::fs::write(root.join("config/hocuspanda/day.toml"), "")?;
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
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.state_dir = Some(root.join("state/hocuspanda"));
        config.appearance.reduced_motion = true;
        let mut hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["2560x1600@1".parse()?, "3440x1440".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        hp.set_internal(LAPTOP);
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

    fn remembered(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("state/hocuspanda/panel.toml"))
            .unwrap_or_default()
    }

    /// Launched on La barre, its window open with the keyboard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "bar".to_owned()],
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
                && s["page"] == "bar"
                && s["bar"]["forms"].as_array().is_some_and(|f| f.len() == 2)
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
        if !id.starts_with("option:") && !id.starts_with("toast-") {
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

    /// A menu's choice `n` taken.
    fn choose(&mut self, setting: &str, n: usize) -> anyhow::Result<()> {
        self.click(&format!("control:{setting}"))?;
        self.shows(|s| s["menu"].as_str() == Some(setting))?;
        self.click(&format!("option:{n}"))?;
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

    fn key(&mut self, code: u32) -> anyhow::Result<()> {
        self.hp.key(code, true);
        self.hp.key(code, false);
        wait(&mut self.hp, Duration::from_millis(10))
    }

    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            let code = letter(c).ok_or_else(|| anyhow::anyhow!("no key for {c}"))?;
            self.key(code)?;
        }
        Ok(())
    }

    /// The laptop's bar's form, once it is `form`.
    fn bar_is(&mut self, form: BarForm) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.bars()
                .iter()
                .any(|(name, f, _)| name == LAPTOP && *f == form)
        })?;
        anyhow::ensure!(done, "the laptop's bar: {:?}", self.hp.bars());
        Ok(())
    }
}

fn form_row<'a>(shown: &'a Value, name: &str) -> &'a Value {
    shown["bar"]["forms"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["name"] == name))
        .unwrap_or(&Value::Null)
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

#[test]
fn the_page_shows_its_groups_a_row_per_screen_and_the_preview() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let rows: Vec<&str> = shown["rows"]
        .as_array()
        .map(|r| r.iter().filter_map(|r| r["id"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        rows,
        [
            "bar.side",
            "bar.screens",
            "bar.meeting",
            "bar.today",
            "bar.tasks",
            "bar.agents",
            "bar.windows",
            "bar.seconds",
            "bar.transition",
            "bar.font",
            "bar.color"
        ]
    );
    let bar = &shown["bar"];
    assert_eq!(
        bar["screens_line"],
        "Aujourd’hui sur l’écran du portable ; elle passe sur l’autre s’il s’éteint."
    );
    assert_eq!(form_row(&shown, LAPTOP)["title"], "Écran du portable");
    assert_eq!(form_row(&shown, LAPTOP)["form"], "auto");
    assert_eq!(
        form_row(&shown, LAPTOP)["line"],
        "Automatique : pleine, l’écran fait 2560 px de large (pleine dès 1796)."
    );
    assert!(
        form_row(&shown, EXTERNAL)["line"]
            .as_str()
            .is_some_and(|l| l.ends_with("La barre n’y vient que si l’écran principal s’éteint.")),
        "{shown}"
    );
    assert_eq!(strings(&bar["order"]), ["today", "tasks", "agents"]);
    assert!(strings(&bar["nothing"]).is_empty());
    assert_eq!(bar["colour"], "Blanc doux");
    let preview = &bar["preview"];
    assert_eq!(preview["caption"], "Portable · pleine");
    assert_eq!(
        strings(&preview["groups"]),
        [
            "bar.seconds",
            "bar.meeting",
            "bar.today",
            "bar.tasks",
            "bar.agents",
            "bar.windows"
        ]
    );
    assert_eq!(strings(&preview["screens"]).len(), 2);
    bed.places_with("bar:preview")?;
    Ok(())
}

#[test]
fn a_screens_form_is_written_at_once_and_annuler_takes_it_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.bar_is(BarForm::Full)?;
    bed.click(&format!("form:{LAPTOP}:mini"))?;
    let text = bed.written(|t| t.contains("[outputs.HEADLESS-1]\npanel-form = \"mini\""))?;
    assert!(text.starts_with("# My desktop, by hand."), "{text}");
    bed.bar_is(BarForm::Mini)?;
    let shown = bed.shows(|s| {
        s["toast"] == "Forme de la barre, écran du portable : Mini"
            && form_row(s, LAPTOP)["form"] == "mini"
            && s["bar"]["preview"]["caption"] == "Portable · mini"
    })?;
    assert_eq!(
        form_row(&shown, LAPTOP)["line"],
        "Toujours mini ; un clic sur son bord change pour la session."
    );
    bed.click("toast-action")?;
    bed.written(|t| !t.contains("panel-form"))?;
    bed.bar_is(BarForm::Full)?;
    Ok(())
}

#[test]
fn back_to_the_form_of_its_width_forgets_the_click() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    // Super+B on the laptop's screen: mini, remembered in automatic.
    bed.hp.pointer_move(1280.0, 800.0);
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_B]);
    bed.bar_is(BarForm::Mini)?;
    let state = bed.dir.path().join("state/hocuspanda/panel.toml");
    let kept = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&state).is_ok_and(|t| t.contains("HEADLESS-1 = \"mini\""))
    })?;
    anyhow::ensure!(kept, "the click's form is not remembered");
    bed.open()?;
    let shown = bed.shows(|s| form_row(s, LAPTOP)["back"] == true)?;
    assert_eq!(
        form_row(&shown, LAPTOP)["line"],
        "Automatique : mini, choisie d’un clic sur son bord."
    );
    bed.click(&format!("auto:{LAPTOP}"))?;
    bed.bar_is(BarForm::Full)?;
    let shown = bed.shows(|s| form_row(s, LAPTOP)["back"] == false)?;
    assert_eq!(
        form_row(&shown, LAPTOP)["line"],
        "Automatique : pleine, l’écran fait 2560 px de large (pleine dès 1796)."
    );
    let forgot = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&state).is_ok_and(|t| !t.contains("HEADLESS-1"))
    })?;
    anyhow::ensure!(forgot, "still remembered: {}", bed.remembered());
    // Nothing in config.toml.
    assert!(!std::fs::read_to_string(bed.file())?.contains("panel-form"));
    Ok(())
}

#[test]
fn moving_to_do_up_writes_the_order_and_the_keyboard_follows_its_button() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("move:tasks:up")?;
    bed.written(|t| t.contains("order = [\"tasks\", \"today\", \"agents\"]"))?;
    let shown = bed.shows(|s| {
        s["toast"] == "Ordre de la barre : À faire, Aujourd’hui, Les agents"
            && strings(&s["bar"]["order"]) == ["tasks", "today", "agents"]
    })?;
    assert_eq!(
        strings(&shown["bar"]["preview"]["groups"])[2..5],
        ["bar.tasks", "bar.today", "bar.agents"]
    );
    // Annuler: the order as it was, the line taken out.
    bed.click("toast-action")?;
    bed.written(|t| !t.contains("order"))?;
    // The keyboard: Tab to the agents' ↑, Space twice; it stays on the
    // agents' buttons, ↓ once ↑ is at the top.
    // The agents' ↑ among the controls: the link, the side, the screens,
    // the link, two screens' forms, the link, the meeting, Today's and To
    // do's three each.
    let target = 4 + 2 + 1 + 1 + 3 + 3;
    for _ in 0..80 {
        let now = bed.shows(|_| true)?["focus"].clone();
        if now == format!("control:{target}") {
            break;
        }
        bed.key(KEY_TAB)?;
        bed.shows(|s| s["focus"] != now)?;
    }
    bed.shows(|s| s["focus"] == format!("control:{target}"))?;
    bed.key(KEY_SPACE)?;
    bed.written(|t| t.contains("order = [\"today\", \"agents\", \"tasks\"]"))?;
    // The agents' ↑ is now two places earlier.
    bed.shows(|s| {
        strings(&s["bar"]["order"]) == ["today", "agents", "tasks"]
            && s["focus"] == format!("control:{}", target - 3)
    })?;
    bed.key(KEY_SPACE)?;
    bed.written(|t| t.contains("order = [\"agents\", \"today\", \"tasks\"]"))?;
    // At the top, ↑ is inert: the keyboard is on ↓.
    bed.shows(|s| {
        strings(&s["bar"]["order"]) == ["agents", "today", "tasks"]
            && s["focus"] == format!("control:{}", target - 6 + 1)
    })?;
    Ok(())
}

#[test]
fn gauche_writes_panel_side_and_the_bar_and_the_preview_move_to_the_left() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["bar"]["preview"]["side"], "right");
    // The room kept for the full bar on the right of the laptop's screen.
    assert_eq!(bed.hp.work_area_of(LAPTOP).map(|a| a.x), Some(8));
    bed.click("segment:bar.side:1")?;
    bed.written(|t| t.contains("[panel]\nside = \"left\""))?;
    bed.shows(|s| {
        s["bar"]["preview"]["side"] == "left"
            && s["toast"] == "Côté de l’écran : Gauche"
            && s["rows"][0]["value"] == "Gauche"
    })?;
    // The compositor follows: the bar's room on the left.
    let left = bed.hp.dispatch_until(WAIT, |hp| {
        hp.work_area_of(LAPTOP).map(|a| a.x) == Some(8 + 196 + 8)
    })?;
    anyhow::ensure!(left, "the work area: {:?}", bed.hp.work_area_of(LAPTOP));
    // Annuler: back on the right.
    bed.click("toast-action")?;
    bed.written(|t| !t.contains("side"))?;
    bed.shows(|s| s["bar"]["preview"]["side"] == "right")?;
    let right = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.work_area_of(LAPTOP).map(|a| a.x) == Some(8))?;
    anyhow::ensure!(right, "the work area: {:?}", bed.hp.work_area_of(LAPTOP));
    Ok(())
}

/// The window on top of the stack, and whether it has the keyboard.
fn in_front(hp: &Headless) -> (Option<String>, bool) {
    hp.windows()
        .last()
        .map_or((None, false), |w| (w.app_id.clone(), w.keyboard))
}

/// After Gauche, the maximised window behind the Settings follows the bar
/// to its new room; the next clicks on the Settings are still theirs, ✕
/// closing them at the first (#647).
#[test]
fn after_gauche_the_next_clicks_on_the_window_are_its_own() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    common::open(&mut bed.hp, "behind", (600, 400))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, common::KEY_UP]);
    let maximised = bed.hp.dispatch_until(WAIT, |hp| {
        common::rect_of(hp, "behind").is_some_and(|r| r.x == 8 && r.width > 2000)
    })?;
    anyhow::ensure!(
        maximised,
        "behind: {:?}",
        common::rect_of(&bed.hp, "behind")
    );
    bed.open()?;
    bed.click("segment:bar.side:1")?;
    bed.written(|t| t.contains("[panel]\nside = \"left\""))?;
    let followed = bed.hp.dispatch_until(WAIT, |hp| {
        common::rect_of(hp, "behind").is_some_and(|r| r.x == 8 + 196 + 8)
    })?;
    anyhow::ensure!(followed, "behind: {:?}", common::rect_of(&bed.hp, "behind"));
    // The next click: Apparence et langue opens, the Settings in front.
    bed.click("nav:appearance")?;
    bed.shows(|s| s["page"] == "appearance")?;
    assert_eq!(in_front(&bed.hp), (Some(APP_ID.to_owned()), true));
    // ✕, at the first click.
    bed.click("close")?;
    let closed = bed.hp.dispatch_until(WAIT, |hp| {
        !hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(APP_ID))
    })?;
    anyhow::ensure!(
        closed,
        "✕ did not close the window: {:?}",
        in_front(&bed.hp)
    );
    assert_eq!(in_front(&bed.hp), (Some("behind".to_owned()), true));
    Ok(())
}

#[test]
fn hiding_today_writes_panel_today_and_a_group_with_nothing_to_show_says_so() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start("[keyboard]\nlayouts = [\"us\"]\n")? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(
        strings(&shown["bar"]["nothing"]),
        ["bar.meeting", "bar.today", "bar.tasks"]
    );
    bed.click("switch:bar.today")?;
    bed.written(|t| t.contains("[panel]\ntoday = false"))?;
    bed.shows(|s| {
        !strings(&s["bar"]["preview"]["groups"]).contains(&"bar.today")
            && s["toast"]
                .as_str()
                .is_some_and(|t| t.starts_with("Aujourd’hui"))
    })?;
    Ok(())
}

#[test]
fn the_clock_rows_write_panel_clock_and_a_colour_of_the_files_own_stays() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&format!("{FILE}\n[panel.clock]\ncolor = \"#3DFFA2\"\n"))?
    else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["bar"]["colour"], "Personnalisée · #3DFFA2");
    bed.click("control:bar.seconds")?;
    bed.written(|t| t.contains("seconds = false"))?;
    bed.shows(|s| s["bar"]["preview"]["seconds"] == false)?;
    // "Volet": written, and played once on the preview's minutes.
    bed.choose("bar.transition", 2)?;
    bed.written(|t| t.contains("transition = \"flip\""))?;
    // Green: the file's colour replaced.
    bed.choose("bar.color", 1)?;
    let text = bed.written(|t| t.contains("color = \"#7ee0a0\""))?;
    assert!(!text.contains("3DFFA2"), "{text}");
    bed.shows(|s| s["bar"]["colour"] == "Vert")?;
    Ok(())
}

#[test]
fn searching_super_b_ordre_and_secondes_finds_their_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    for (word, found, lit) in [
        ("ordre", "Ordre des groupes", "bar.order"),
        ("secondes", "Les secondes", "bar.seconds"),
        // "gauche" finds eight rows before it now (the overview's "Coin en
        // haut à gauche", #727): its own name.
        ("cote", "Côté de l’écran", "bar.side"),
        ("bord", "Changer de forme à la main", "bar.by-hand"),
    ] {
        chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_F]);
        bed.shows(|s| s["focus"] == "search")?;
        // The field takes the keyboard a moment after it is focused, as
        // settings_screens.rs waits.
        wait(&mut bed.hp, Duration::from_millis(100))?;
        bed.type_text(word)?;
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
        bed.shows(|s| s["page"] == "bar" && s["lit"] == lit)?;
        chord(&mut bed.hp, &[common::KEY_ESC]);
        bed.shows(|s| s["query"] == "")?;
    }
    Ok(())
}

#[test]
fn the_links_open_their_pages_with_their_row_lit() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    for (link, page, lit) in [
        (
            "link:appearance.reduce-transparency",
            "appearance",
            "appearance.reduce-transparency",
        ),
        ("link:screens.primary", "screens", "screens.primary"),
        (
            "link:shortcuts.toggle-bar",
            "keyboard",
            "shortcuts.toggle-bar",
        ),
    ] {
        bed.click(link)?;
        bed.shows(|s| s["page"] == page && s["lit"] == lit)?;
        bed.click("nav:bar")?;
        bed.shows(|s| s["page"] == "bar")?;
    }
    Ok(())
}

/// A row's light goes out at a frame, nothing else happening: the window
/// says its places again after it, as after anything it shows, so that a
/// test waiting for places as recent as what it shows does not wait for
/// ever (#684).
#[test]
fn the_places_follow_a_light_going_out() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("link:screens.primary")?;
    bed.shows(|s| s["page"] == "screens" && s["lit"] == "screens.primary")?;
    bed.shows(|s| s["page"] == "screens" && s["lit"].is_null())?;
    bed.places_with("nav:bar")?;
    Ok(())
}

#[test]
fn at_rest_the_bar_page_draws_only_its_clock() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&format!("{FILE}\n[panel.clock]\nseconds = false\n"))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["bar"]["preview"]["seconds"] == false)?;
    bed.hp.pointer_move(5.0, 5.0);
    // Still once the window settled: without the seconds the preview's
    // clock draws once a minute, so a quiet few seconds draw nothing but
    // perhaps that minute.
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..200 {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 30 {
            break;
        }
    }
    let before = bed.hp.window_commits();
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert!(
        bed.hp.window_commits() <= before + 1,
        "the page drew at rest: {} commits",
        bed.hp.window_commits() - before
    );
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
    let save = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
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
    };
    bed.open()?;
    save(&mut bed, "bar-page.png")?;
    bed.click(&format!("form:{LAPTOP}:mini"))?;
    bed.bar_is(BarForm::Mini)?;
    bed.shows(|s| s["bar"]["preview"]["caption"] == "Portable · mini")?;
    save(&mut bed, "bar-page-mini.png")?;
    bed.click("move:tasks:up")?;
    bed.shows(|s| strings(&s["bar"]["order"]) == ["tasks", "today", "agents"])?;
    bed.click(&format!("form:{LAPTOP}:auto"))?;
    bed.bar_is(BarForm::Full)?;
    bed.reveal("row:bar.agents")?;
    save(&mut bed, "bar-page-order.png")?;
    bed.reveal("row:bar.color")?;
    save(&mut bed, "bar-page-clock.png")?;
    // Gauche, the page at its top.
    bed.click("segment:bar.side:1")?;
    bed.shows(|s| s["bar"]["preview"]["side"] == "left")?;
    bed.click("toast-close")?;
    bed.shows(|s| s["toast"].is_null())?;
    bed.reveal("row:bar.side")?;
    save(&mut bed, "bar-page-left.png")?;
    Ok(())
}
