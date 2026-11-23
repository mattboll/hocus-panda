//! Settings › Calendar and tasks (#926): the real `hocus-settings` window,
//! a client of a headless compositor, its configuration file, home and
//! state the test's own; GNOME Online Accounts faked on a private bus, a
//! Calendar API and a Planka server faked on 127.0.0.1: never the
//! network, the session's bus or the user's files.
//!
//! What the page writes, as Day.1 writes it: a calendar unticked, its
//! star put out and its colour changed rewrite its account's
//! `[[day.calendars]]`, the file's other lines kept; an account of Online
//! Accounts connected, removed, and back with the toast's "Annuler"; the
//! board's columns, adding following the tasks' until chosen apart. The
//! pull request's screenshots are an ignored test (`HOCUS_SHOTS`).

use crate::common;
use crate::day_fakes as fakes;
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
const GOOD: &str = "tok-good";

/// A configuration of the user's: Alex's work account in the bar.
const MINE: &str = "# Mine, by hand.\n\
                    [recording]\n\
                    format = \"mp4\"   # the small files\n\
                    \n\
                    [[day.calendars]]\n\
                    kind = \"google\"\n\
                    account = \"alex@example.org\"\n\
                    shown = [\"Travail\", \"Famille\"]\n\
                    featured = [\"Travail\", \"Famille\"]\n\
                    colors = { \"Travail\" = \"#8FB3FF\", \"Famille\" = \"#F28FB1\" }\n\
                    \n\
                    [shortcuts]\n\
                    terminal = \"Super+Return\"\n";

/// Online Accounts on a private bus with Alex's accounts, and a Calendar
/// API with three calendars.
struct Google {
    // Dropped in this order: the fakes, then the bus.
    goa: fakes::FakeGoa,
    api: fakes::FakeApi,
    bus: fakes::PrivateBus,
}

impl Google {
    fn start(dir: &Path, accounts: &[&str]) -> anyhow::Result<Self> {
        let bus = fakes::PrivateBus::start(dir)?;
        let goa = fakes::FakeGoa::start(&bus.address)?;
        for (i, account) in accounts.iter().enumerate() {
            goa.add(&format!("account_{i}"), "google", account)?;
        }
        let api = fakes::FakeApi::start()?;
        api.with(|api| {
            let mut work = fakes::calendar(
                "alex@example.org",
                "alex@example.org",
                "#16a765",
                "owner",
                true,
            );
            work["summaryOverride"] = serde_json::json!("Travail");
            let family = fakes::calendar("family", "Famille", "#f691b2", "writer", true);
            let birthdays =
                fakes::calendar("birthdays", "Anniversaires", "#9fe1e7", "reader", false);
            api.calendars = vec![work, family, birthdays];
        });
        Ok(Self { goa, api, bus })
    }
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
}

impl Bed {
    /// A compositor, the configuration file holding `file`, the Settings'
    /// script in French with `google`'s bus and API.
    fn start(dir: tempfile::TempDir, file: &str, google: Option<&Google>) -> anyhow::Result<Self> {
        let program = common::settings_program();
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let config_file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&config_file, file)?;
        let panel = root.join("bin/gnome-control-center");
        std::fs::write(
            &panel,
            format!(
                "#!/bin/sh\necho \"$@\" >> '{}'\n",
                root.join("panel.args").display()
            ),
        )?;
        std::fs::set_permissions(&panel, std::fs::Permissions::from_mode(0o755))?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        let google_env = google.map_or_else(
            || "unset DBUS_SESSION_BUS_ADDRESS\n".to_owned(),
            |g| {
                format!(
                    "export DBUS_SESSION_BUS_ADDRESS='{}' HOCUS_TEST_GOOGLE_API='{}'\n",
                    g.bus.address, g.api.address
                )
            },
        );
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n{google_env}\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
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
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(config_file);
        config.appearance.reduced_motion = true;
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
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

    /// The Settings opened on Calendar and tasks, once `done` holds of it.
    fn open(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "agenda".to_owned()],
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
        let shown =
            self.shows(|s| s["page"] == "agenda" && s["file"] == "saved" && done(&s["agenda"]))?;
        Ok(shown["agenda"].clone())
    }

    /// What the page shows, once `done` holds of the whole line.
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

    /// The page's part, once `done` holds of it.
    fn agenda(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        Ok(self.shows(|s| done(&s["agenda"]))?["agenda"].clone())
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

/// The calendars of the page's first account, once read.
fn read(agenda: &Value) -> bool {
    agenda["accounts"][0]["calendars"]
        .as_array()
        .is_some_and(|c| c.len() == 3)
}

/// The first account's calendar `name` as shown.
fn calendar(agenda: &Value, name: &str) -> Value {
    agenda["accounts"][0]["calendars"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["name"] == name))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Each line of `mine` still in `text`, in its order.
fn kept(mine: &str, text: &str, but: &[&str]) {
    let mut from = 0;
    for line in mine
        .lines()
        .filter(|l| !but.iter().any(|b| l.starts_with(b)))
    {
        let at = text[from..]
            .find(line)
            .unwrap_or_else(|| panic!("{line:?} lost:\n{text}"));
        from += at + line.len();
    }
}

#[test]
fn a_calendar_unticked_featured_and_recoloured_rewrites_its_accounts_entry() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let google = Google::start(dir.path(), &["alex@example.org"])?;
    let mut bed = Bed::start(dir, MINE, Some(&google))?;
    let agenda = bed.open(read)?;
    assert_eq!(agenda["accounts"][0]["account"], "alex@example.org");
    let birthdays = calendar(&agenda, "Anniversaires");
    assert_eq!(birthdays["shown"], false, "{agenda}");
    assert_eq!(birthdays["read_only"], true, "{agenda}");
    assert_eq!(calendar(&agenda, "Famille")["colour"], 4, "{agenda}");
    // Famille unticked: shown and featured lose it, the rest stays.
    bed.click("agenda:box:alex@example.org:1")?;
    let text = bed.written(|t| t.contains("shown = [\"Travail\"]"))?;
    assert!(text.contains("featured = [\"Travail\"]"), "{text}");
    kept(MINE, &text, &["shown", "featured", "colors"]);
    // Travail's star put out.
    bed.agenda(|a| calendar(a, "Famille")["shown"] == false)?;
    bed.click("agenda:star:alex@example.org:0")?;
    let text = bed.written(|t| t.contains("featured = []"))?;
    assert!(text.contains("shown = [\"Travail\"]"), "{text}");
    // Travail in green, the second of the eight.
    bed.agenda(|a| calendar(a, "Travail")["featured"] == false)?;
    bed.click("agenda:swatch:alex@example.org:0")?;
    bed.agenda(|a| a["unfolded"] == "alex@example.org:0")?;
    bed.click("agenda:colour:alex@example.org:0:1")?;
    let text = bed.written(|t| t.contains("Travail = \"#6CC287\""))?;
    kept(MINE, &text, &["shown", "featured", "colors"]);
    assert_eq!(text.matches("[[day.calendars]]").count(), 1, "{text}");
    let agenda = bed.agenda(|a| calendar(a, "Travail")["colour"] == 1)?;
    assert_eq!(agenda["unfolded"], Value::Null);
    Ok(())
}

#[test]
fn an_account_is_connected_then_removed_and_back_with_annuler() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let google = Google::start(dir.path(), &["alex@example.org"])?;
    let mine = "# Mine.\n[recording]\nformat = \"mp4\"\n";
    let mut bed = Bed::start(dir, mine, Some(&google))?;
    let agenda = bed.open(|a| a["others"][0] == "alex@example.org")?;
    assert_eq!(agenda["accounts"], serde_json::json!([]));
    // Connected: ticked as the account marks them, every star lit.
    bed.click("agenda:plug:alex@example.org")?;
    let text = bed.written(|t| t.contains("account = \"alex@example.org\""))?;
    assert!(text.starts_with(mine), "{text}");
    assert!(
        text.contains("shown = [\"Travail\", \"Famille\"]"),
        "{text}"
    );
    assert!(
        text.contains("featured = [\"Travail\", \"Famille\"]"),
        "{text}"
    );
    let agenda = bed.agenda(read)?;
    assert_eq!(agenda["others"], serde_json::json!([]));
    // Removed from the bar, not from Online Accounts.
    bed.click("agenda:remove:alex@example.org")?;
    let text = bed.written(|t| !t.contains("alex@example.org"))?;
    assert_eq!(text, mine);
    bed.agenda(|a| a["others"][0] == "alex@example.org")?;
    // "Annuler": back as it was.
    bed.click("toast-action")?;
    let text = bed.written(|t| t.contains("account = \"alex@example.org\""))?;
    assert!(
        text.contains("shown = [\"Travail\", \"Famille\"]"),
        "{text}"
    );
    bed.agenda(|a| a["accounts"][0]["account"] == "alex@example.org")?;
    Ok(())
}

#[test]
fn the_boards_columns_are_menus_adding_following_the_tasks() -> anyhow::Result<()> {
    let planka = fakes::FakePlanka::start(GOOD)?;
    let address = format!("{}/boards/1", planka.address);
    let mine = format!(
        "# Mine.\n[recording]\nformat = \"mp4\"\n\n[[day.tasks]]\nkind = \"planka\"\n\
         address = \"{address}\"\ntasks = \"TODO\"\ndone = \"Done\"\nadd = \"TODO\"\n"
    );
    let dir = tempfile::tempdir()?;
    // Its token, as Day.1 kept it without a keyring.
    let host = planka
        .address
        .trim_start_matches("http://")
        .replace(':', "-");
    let accounts = dir.path().join("state/hocuspanda/accounts");
    std::fs::create_dir_all(&accounts)?;
    std::fs::set_permissions(&accounts, std::fs::Permissions::from_mode(0o700))?;
    std::fs::write(accounts.join(format!("planka-tasks-{host}-1")), GOOD)?;
    let mut bed = Bed::start(dir, &mine, None)?;
    let agenda = bed.open(|a| a["board"]["name"] == "Sprint")?;
    assert_eq!(
        agenda["board"]["columns"],
        serde_json::json!(["TODO", "Done", "TODO"])
    );
    // The tasks' column: adding follows it.
    bed.click("agenda:menu:0")?;
    bed.click("agenda:column:0:0")?;
    let text = bed.written(|t| t.contains("tasks = \"Backlog\""))?;
    assert!(text.contains("add = \"Backlog\""), "{text}");
    assert!(text.contains("done = \"Done\""), "{text}");
    assert!(!text.contains(GOOD), "{text}");
    // Adding chosen apart, then the tasks' column again: adding stays.
    bed.agenda(|a| a["board"]["columns"][0] == "Backlog")?;
    bed.click("agenda:menu:2")?;
    bed.click("agenda:column:2:1")?;
    bed.written(|t| t.contains("add = \"TODO\""))?;
    bed.agenda(|a| a["board"]["columns"][2] == "TODO")?;
    bed.click("agenda:menu:0")?;
    bed.click("agenda:column:0:2")?;
    let text = bed.written(|t| t.contains("tasks = \"Done\""))?;
    assert!(text.contains("add = \"TODO\""), "{text}");
    // Taken out: the local list.
    bed.agenda(|a| a["board"]["columns"][0] == "Done")?;
    bed.click("agenda:remove-board")?;
    let text = bed.written(|t| !t.contains("planka"))?;
    assert_eq!(text, "# Mine.\n[recording]\nformat = \"mp4\"\n");
    bed.agenda(|a| a["board_set"].is_null())?;
    Ok(())
}

/// The window, cropped, saved as `name` in `out` once it is still.
fn shoot(bed: &mut Bed, out: &Path, name: &str) -> anyhow::Result<()> {
    bed.hp.pointer_move(1900.0, 1060.0);
    // A field holding the keyboard blinks its caret: never still.
    if settings_page::still(&mut bed.hp).is_err() {
        wait(&mut bed.hp, Duration::from_millis(300))?;
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
    // An account and its calendars (one swatch unfolded), another to
    // connect, the board.
    let planka = fakes::FakePlanka::start(GOOD)?;
    let host = planka
        .address
        .trim_start_matches("http://")
        .replace(':', "-");
    let dir = tempfile::tempdir()?;
    let accounts = dir.path().join("state/hocuspanda/accounts");
    std::fs::create_dir_all(&accounts)?;
    std::fs::set_permissions(&accounts, std::fs::Permissions::from_mode(0o700))?;
    std::fs::write(accounts.join(format!("planka-tasks-{host}-1")), GOOD)?;
    let google = Google::start(dir.path(), &["alex@example.org", "alex.perso@example.org"])?;
    let file = format!(
        "{MINE}\n[[day.tasks]]\nkind = \"planka\"\naddress = \"{}/boards/1\"\n\
         tasks = \"TODO\"\ndone = \"Done\"\nadd = \"TODO\"\n",
        planka.address
    );
    let mut bed = Bed::start(dir, &file, Some(&google))?;
    bed.open(|a| read(a) && a["board"]["name"] == "Sprint")?;
    bed.click("agenda:swatch:alex@example.org:1")?;
    bed.agenda(|a| a["unfolded"] == "alex@example.org:1")?;
    shoot(&mut bed, &out, "settings-agenda")?;
    bed.click("agenda:swatch:alex@example.org:1")?;
    bed.agenda(|a| a["unfolded"].is_null())?;
    bed.click("agenda:menu:0")?;
    bed.agenda(|a| a["menu"] == 0)?;
    bed.at("agenda:column:0:2")?;
    shoot(&mut bed, &out, "settings-agenda-board")?;
    bed.click("agenda:menu:0")?;
    bed.agenda(|a| a["menu"].is_null())?;
    bed.click("agenda:change-board")?;
    bed.agenda(|a| !a["form"].is_null())?;
    bed.at("agenda:sign-in")?;
    shoot(&mut bed, &out, "settings-agenda-change-board")?;
    drop(bed);
    // An account that needs signing in again.
    let dir = tempfile::tempdir()?;
    let google = Google::start(dir.path(), &["alex@example.org"])?;
    google.goa.set_attention("account_0", true)?;
    let mut bed = Bed::start(dir, MINE, Some(&google))?;
    bed.open(|a| {
        a["online"] == "listed" && a["accounts"][0]["state"] != "Google Agenda · lecture…"
    })?;
    shoot(&mut bed, &out, "settings-agenda-expired")?;
    drop(bed);
    // Nothing in the bar: an account to connect, the local list.
    let dir = tempfile::tempdir()?;
    let google = Google::start(dir.path(), &["alex@example.org"])?;
    let mut bed = Bed::start(dir, "# Mine.\n", Some(&google))?;
    bed.open(|a| a["others"][0] == "alex@example.org" && !a["local"].is_null())?;
    shoot(&mut bed, &out, "settings-agenda-none")?;
    drop(bed);
    // Not even an account in Online Accounts.
    let dir = tempfile::tempdir()?;
    let google = Google::start(dir.path(), &[])?;
    let mut bed = Bed::start(dir, "# Mine.\n", Some(&google))?;
    bed.open(|a| a["online"] == "listed" && !a["local"].is_null())?;
    shoot(&mut bed, &out, "settings-agenda-no-account")?;
    drop(bed);
    // The home's cards.
    let dir = tempfile::tempdir()?;
    let mut bed = Bed::start(dir, "# Mine.\n[appearance]\ntheme = \"light\"\n", None)?;
    bed.hp.launch(hocus_link::Launch {
        id: APP_ID.to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec![bed.script.display().to_string()],
    });
    bed.shows(|s| s["page"] == "home" && s["file"] == "saved")?;
    shoot(&mut bed, &out, "settings-home-cards")?;
    Ok(())
}
