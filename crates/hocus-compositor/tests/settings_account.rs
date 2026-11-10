//! The Settings' Compte page (6.11, issue #411): the real `hocus-settings`
//! window, a client of a headless compositor, against an AccountsService
//! faked on a private bus (`levels/accounts.rs`) and a fake `passwd` on the
//! test's `PATH` (it reads its three answers from its terminal and answers
//! as PAM does), never the machine's. Its configuration, state, home and
//! runtime directories are the test's own; it draws with tiny-skia.
//!
//! One test per point of the approved spec: the name, login, type and
//! avatar; a panda chosen and taken back; a picture cut square before it
//! is set; the display name saved on `Enter`, put back by `Esc`; the
//! current password refused on its field; the new one refused with the
//! system's reason; a password changed, never in the log; AccountsService
//! refusing; no `passwd`; the link to Énergie and the search; at rest,
//! nothing drawn. What the window shows is read from its log, never on a
//! time.

use crate::accounts;
use crate::common;
use crate::fakes;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use accounts::{AccountsService, Call, User};
use common::{BTN_LEFT, KEY_ESC, KEY_LEFTSHIFT, KEY_RIGHT, WAIT, wait};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_ENTER: u32 = 28;
const KEY_BACKSPACE: u32 = 14;
const KEY_END: u32 = 107;
const KEY_MINUS: u32 = 12;

/// The fake's current password, a new one it takes, one it refuses.
const CURRENT: &str = "panda-1234";
const NEW: &str = "bambou-5678";
const SHORT: &str = "bam";

/// A fake `passwd`: answers as PAM does at a terminal.
const PASSWD: &str = r#"#!/bin/sh
printf 'Changing password for alex.\n'
printf 'Current password: '
IFS= read -r current || exit 1
if [ "$current" != "panda-1234" ]; then
  printf 'passwd: Authentication token manipulation error\npasswd: password unchanged\n'
  exit 10
fi
printf 'New password: '
IFS= read -r new || exit 1
if [ ${#new} -lt 8 ]; then
  printf 'BAD PASSWORD: The password is shorter than 8 characters\n'
  printf 'New password: '
  IFS= read -r new || exit 1
fi
printf 'Retype new password: '
IFS= read -r again || exit 1
[ "$new" = "$again" ] || { printf 'Sorry, passwords do not match.\n'; exit 10; }
printf 'passwd: password updated successfully\n'
: > "$(dirname "$0")/../passwd-changed"
"#;

/// The keys of a US keyboard that type `c`, with Shift or not.
fn key_of(c: char) -> Option<(u32, bool)> {
    let letters = "qwertyuiop";
    let middle = "asdfghjkl";
    let bottom = "zxcvbnm";
    if let Some(i) = letters.find(c) {
        return Some((16 + i as u32, false));
    }
    if let Some(i) = middle.find(c) {
        return Some((30 + i as u32, false));
    }
    if let Some(i) = bottom.find(c) {
        return Some((44 + i as u32, false));
    }
    if c.is_ascii_uppercase() {
        return key_of(c.to_ascii_lowercase()).map(|(k, _)| (k, true));
    }
    Some(match c {
        '1'..='9' => (2 + (c as u32 - '1' as u32), false),
        '0' => (11, false),
        '-' => (KEY_MINUS, false),
        ' ' => (57, false),
        _ => return None,
    })
}

struct Bed {
    hp: Headless,
    accounts: AccountsService,
    log: PathBuf,
    script: PathBuf,
    dir: tempfile::TempDir,
    // Dropped after AccountsService: its connection goes before the bus.
    fakes: Fakes,
}

impl Bed {
    /// The compositor, a bus of its own with AccountsService holding
    /// `user`, the fake `passwd` (or no `passwd` at all), and the
    /// Settings' script; `None` when the Settings or `dbus-daemon` are not
    /// there.
    fn start(user: User, passwd: bool) -> anyhow::Result<Option<Self>> {
        Self::start_on(user, passwd, "1920x1080")
    }

    fn start_on(user: User, passwd: bool, output: &str) -> anyhow::Result<Option<Self>> {
        let program = common::settings_program();
        let bus = Command::new("sh")
            .args(["-c", "command -v dbus-daemon"])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !bus {
            eprintln!("dbus-daemon missing: skipped");
            return Ok(None);
        }
        let mut fakes = Fakes::empty()?;
        fakes.start_bus()?;
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home/Images",
            "config/hocuspanda",
            "cache",
            "state",
            "data",
            "bin",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let accounts =
            AccountsService::start(fakes.runtime(), &fakes.address, user, root.join("icons"))?;
        // Never the system's `passwd`: the fake first on the PATH, or no
        // PATH where the system's is.
        let path = if passwd {
            let fake = root.join("bin/passwd");
            std::fs::write(&fake, PASSWD)?;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))?;
            format!("{}:/usr/bin:/bin", root.join("bin").display())
        } else {
            root.join("bin").display().to_string()
        };
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, "# Mine.\n")?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='{bus}' DBUS_SESSION_BUS_ADDRESS='{bus}' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 TZ=UTC HOME='{home}' XDG_CONFIG_HOME='{config}' \
                 XDG_CACHE_HOME='{cache}' XDG_STATE_HOME='{state}' XDG_DATA_HOME='{data}' \
                 XDG_PICTURES_DIR='{pictures}' XDG_DATA_DIRS=/nonexistent \
                 PATH='{path}' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{program}' \"$@\" 2>>'{log}'\n",
                bus = fakes.address,
                home = at("home"),
                config = at("config"),
                cache = at("cache"),
                state = at("state"),
                data = at("data"),
                pictures = at("home/Images"),
                program = program.display(),
                log = log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.state_dir = Some(root.join("state/hocuspanda"));
        config.appearance.reduced_motion = true;
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
            accounts,
            log,
            script,
            dir,
            fakes,
        }))
    }

    /// The Settings launched on `section`, their window open with the
    /// keyboard.
    fn open_on(&mut self, section: &str) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), section.to_owned()],
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
        self.shows(|s| s["file"] == "saved" && s["page"] == section)
    }

    /// The Settings on Compte, AccountsService heard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.open_on("account")?;
        self.shows(|s| s["account"]["heard"] == true && s["account"]["pandas"] == 8)
    }

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
                .filter(|l| {
                    l.contains("WARN")
                        || l.contains("ERROR")
                        || l.contains("panicked")
                        || l.contains("passwd")
                        || l.contains("Accounts")
                })
                .collect();
            anyhow::anyhow!(
                "the window never showed it; it shows {}\n{}",
                read(&log).map_or_else(|| "nothing".to_owned(), |s| s.to_string()),
                trouble.join("\n")
            )
        })
    }

    /// Waits until AccountsService was asked `call`.
    fn asked(&mut self, call: &Call) -> anyhow::Result<()> {
        let done = {
            let accounts = &self.accounts;
            self.hp
                .dispatch_until(WAIT, |_| accounts.calls().contains(call))?
        };
        anyhow::ensure!(
            done,
            "AccountsService was never asked {call:?}: {:?}",
            self.accounts.calls()
        );
        Ok(())
    }

    /// Waits until AccountsService was asked a picture.
    fn asked_icon(&mut self, count: usize) -> anyhow::Result<Vec<String>> {
        let icons = |accounts: &AccountsService| -> Vec<String> {
            accounts
                .calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::Icon(path) => Some(path),
                    Call::Name(_) => None,
                })
                .collect()
        };
        let done = {
            let accounts = &self.accounts;
            self.hp
                .dispatch_until(WAIT, |_| icons(accounts).len() >= count)?
        };
        anyhow::ensure!(done, "no picture asked: {:?}", self.accounts.calls());
        Ok(icons(&self.accounts))
    }

    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = common::last_json(&log, " places {").filter(|p| p.get(id).is_some());
            seen.is_some()
        })?;
        seen.ok_or_else(|| {
            anyhow::anyhow!(
                "{id} is nowhere: {:?}",
                common::last_json(&log, " places {")
            )
        })
    }

    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        let window = self.window()?;
        let at = (
            f64::from(window.x) + n(0) + n(2) / 2.0,
            f64::from(window.y) + n(1) + n(3) / 2.0,
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

    /// The password's sheet, `current`, `new` and `again` typed in it,
    /// each field clicked first.
    fn sheet(&mut self, current: &str, new: &str, again: &str) -> anyhow::Result<Value> {
        self.click("control:account.password")?;
        self.shows(|s| s["account"]["sheet"].is_object())?;
        for (i, (field, text)) in [
            ("box:password-current", current),
            ("box:password-new", new),
            ("box:password-again", again),
        ]
        .into_iter()
        .enumerate()
        {
            self.click(field)?;
            self.type_text(text)?;
            self.shows(|s| s["account"]["sheet"]["filled"][i] == true)?;
        }
        self.shows(|s| s["account"]["sheet"]["filled"] == serde_json::json!([true, true, true]))
    }
}

#[test]
fn the_account_page_shows_the_name_login_type_and_avatar() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let account = &shown["account"];
    assert_eq!(account["service"], true);
    assert_eq!(account["name"], "Alex Martin");
    assert_eq!(account["login"], "alex");
    assert_eq!(account["admin"], true);
    // No picture yet: the initial.
    assert_eq!(account["picture"], false);
    assert_eq!(account["passwd"], true);
    bed.places_with("row:account.avatar")?;
    bed.places_with("link:account.lock")?;
    drop(bed);
    // A standard account, a picture already there (the third panda).
    let holder = tempfile::tempdir()?;
    let panda = holder.path().join("panda.png");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../hocus-settings/data/avatars/panda-3.png"),
        &panda,
    )?;
    let Some(mut bed) = Bed::start(
        User {
            admin: false,
            icon: panda.display().to_string(),
            ..User::default()
        },
        true,
    )?
    else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["account"]["admin"], false);
    assert_eq!(shown["account"]["picture"], true);
    assert_eq!(shown["account"]["panda"], 2);
    Ok(())
}

#[test]
fn choosing_a_panda_sets_the_icon_file_and_annuler_takes_it_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("control:account.avatar")?;
    bed.shows(|s| s["account"]["popover"] == 0)?;
    bed.places_with("panda:7")?;
    // The keyboard moves to the second and chooses it.
    bed.key(KEY_RIGHT)?;
    bed.shows(|s| s["account"]["popover"] == 1)?;
    bed.key(KEY_ENTER)?;
    bed.asked_icon(1)?;
    let shown = bed.shows(|s| s["account"]["panda"] == 1 && s["toast"].is_string())?;
    assert_eq!(shown["toast"], "Avatar changé.");
    assert_eq!(shown["toast_action"], "undo");
    assert!(shown["account"]["popover"].is_null());
    let panda2 = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../hocus-settings/data/avatars/panda-2.png"),
    )?;
    assert_eq!(bed.accounts.picture().as_deref(), Some(&panda2[..]));
    // Then a click on the last.
    bed.click("toast-close")?;
    bed.shows(|s| s["toast"].is_null())?;
    bed.click("control:account.avatar")?;
    bed.shows(|s| s["account"]["popover"] == 1)?;
    bed.click("panda:7")?;
    bed.asked_icon(2)?;
    bed.shows(|s| s["account"]["panda"] == 7 && s["toast"] == "Avatar changé.")?;
    // "Annuler": the second panda again.
    bed.click("toast-action")?;
    bed.asked_icon(3)?;
    let shown = bed.shows(|s| {
        s["account"]["panda"] == 1 && s["toast"].as_str().is_some_and(|t| t.starts_with("Annulé"))
    })?;
    assert_eq!(shown["toast"], "Annulé : l’avatar revient.");
    assert_eq!(bed.accounts.picture().as_deref(), Some(&panda2[..]));
    Ok(())
}

/// The desktop portal's file chooser on the test's bus, answering with
/// `chosen` a moment after, as a portal does once the user chooses.
struct Chooser {
    chosen: PathBuf,
}

#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl Chooser {
    async fn open_file(
        &self,
        _parent_window: String,
        _title: String,
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
        let uri = format!("file://{}", self.chosen.display());
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
fn a_picture_is_cropped_square_before_it_is_set() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    // 900 × 600: its middle 600 green, its sides red.
    let chosen = bed.dir.path().join("home/Images/Lac.png");
    let mut picture = image::RgbaImage::from_pixel(900, 600, image::Rgba([220, 40, 40, 255]));
    for y in 0..600 {
        for x in 150..750 {
            picture.put_pixel(x, y, image::Rgba([40, 200, 60, 255]));
        }
    }
    picture.save(&chosen)?;
    let address = bed.fakes.address.clone();
    let _portal = bed.fakes.runtime().block_on(
        zbus::connection::Builder::address(address.as_str())
            .name("org.freedesktop.portal.Desktop")
            .serve_at(
                "/org/freedesktop/portal/desktop",
                Chooser {
                    chosen: chosen.clone(),
                },
            )
            .build(),
    )?;
    bed.open()?;
    bed.click("control:account.avatar")?;
    bed.shows(|s| s["account"]["popover"] == 0)?;
    bed.click("avatar:choose")?;
    let given = bed.asked_icon(1)?;
    bed.shows(|s| s["account"]["picture"] == true && s["toast"] == "Avatar changé.")?;
    // Set from a file in the runtime directory, removed once copied.
    assert!(given[0].contains("hocus-settings-avatar-"), "{given:?}");
    assert!(!Path::new(&given[0]).exists());
    let set = image::load_from_memory(&bed.accounts.picture().unwrap_or_default())?.to_rgba8();
    assert_eq!((set.width(), set.height()), (512, 512));
    assert!(
        set.pixels().all(|p| p.0[1] > 150 && p.0[0] < 100),
        "only the green middle"
    );
    Ok(())
}

#[test]
fn the_display_name_is_saved_on_enter() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("box:account-name")?;
    wait(&mut bed.hp, Duration::from_millis(50))?;
    for _ in 0.."Martin".len() {
        bed.key(KEY_BACKSPACE)?;
    }
    bed.type_text("Dupont")?;
    bed.shows(|s| s["account"]["draft"] == "Alex Dupont")?;
    bed.key(KEY_ENTER)?;
    bed.asked(&Call::Name("Alex Dupont".to_owned()))?;
    let shown = bed.shows(|s| s["account"]["name"] == "Alex Dupont" && s["toast"].is_string())?;
    assert_eq!(shown["toast"], "Nom affiché : Alex Dupont");
    assert_eq!(shown["toast_action"], "undo");
    // Esc puts back what was there.
    bed.click("box:account-name")?;
    wait(&mut bed.hp, Duration::from_millis(50))?;
    // At its end (a second click there chose a word).
    bed.key(KEY_END)?;
    bed.type_text("xyz")?;
    bed.shows(|s| s["account"]["draft"] == "Alex Dupontxyz")?;
    bed.key(KEY_ESC)?;
    let shown = bed.shows(|s| s["account"]["draft"].is_null())?;
    assert_eq!(shown["page"], "account", "Esc does not close the window");
    // "Annuler" takes the name back.
    bed.click("toast-action")?;
    bed.asked(&Call::Name("Alex Martin".to_owned()))?;
    bed.shows(|s| s["account"]["name"] == "Alex Martin")?;
    Ok(())
}

#[test]
fn a_wrong_current_password_is_said_on_its_field() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    bed.open()?;
    bed.sheet("pas-le-bon", NEW, NEW)?;
    // Both typed the same: "Changer" is ready.
    bed.shows(|s| {
        s["account"]["sheet"]["matching"] == true && s["account"]["sheet"]["ready"] == true
    })?;
    bed.click("password:change")?;
    let shown = bed.shows(|s| s["account"]["sheet"]["wrong_current"] == true)?;
    // What was typed stays; nothing changed.
    assert_eq!(
        shown["account"]["sheet"]["filled"],
        serde_json::json!([true, true, true])
    );
    bed.places_with("under:password-current")?;
    assert!(!bed.dir.path().join("passwd-changed").exists());
    Ok(())
}

#[test]
fn a_new_password_refused_by_the_system_shows_its_reason() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    bed.open()?;
    bed.sheet(CURRENT, SHORT, SHORT)?;
    bed.shows(|s| s["account"]["sheet"]["ready"] == true)?;
    bed.key(KEY_ENTER)?;
    let shown = bed.shows(|s| s["account"]["sheet"]["refused"].is_string())?;
    assert_eq!(
        shown["account"]["sheet"]["refused"],
        "The password is shorter than 8 characters"
    );
    assert_eq!(shown["account"]["sheet"]["running"], false);
    assert!(!bed.dir.path().join("passwd-changed").exists());
    Ok(())
}

#[test]
fn the_password_is_changed_and_never_reaches_the_log() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    bed.open()?;
    // Two that differ first: said, and "Changer" waits.
    bed.sheet(CURRENT, NEW, "bambou-9")?;
    let shown = bed.shows(|s| s["account"]["sheet"]["matching"] == false)?;
    assert_eq!(shown["account"]["sheet"]["ready"], false);
    bed.key(KEY_ESC)?;
    bed.shows(|s| s["account"]["sheet"].is_null())?;
    bed.sheet(CURRENT, NEW, NEW)?;
    bed.shows(|s| s["account"]["sheet"]["ready"] == true)?;
    bed.click("password:change")?;
    let shown = bed.shows(|s| s["toast"] == "Mot de passe changé.")?;
    assert!(shown["account"]["sheet"].is_null());
    assert!(shown["toast_action"].is_null(), "no Annuler");
    assert!(bed.dir.path().join("passwd-changed").exists());
    let said = common::logged(&bed.log);
    for secret in [CURRENT, NEW, "bambou-9"] {
        assert!(!said.contains(secret), "{secret} is in the log");
    }
    Ok(())
}

#[test]
fn a_refusal_by_polkit_leaves_the_values_unchanged() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    bed.open()?;
    bed.accounts.refuse(true);
    bed.click("control:account.avatar")?;
    bed.shows(|s| s["account"]["popover"] == 0)?;
    bed.click("panda:4")?;
    bed.asked_icon(1)?;
    let shown = bed.shows(|s| s["toast"] == "Le système a refusé.")?;
    assert_eq!(shown["account"]["picture"], false);
    assert_eq!(shown["undo"], 0);
    assert_eq!(bed.accounts.now().icon, "");
    Ok(())
}

#[test]
fn without_passwd_the_button_is_inert() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), false)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["account"]["passwd"], false);
    bed.click("control:account.password")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let shown = bed.shows(|s| s["account"]["heard"] == true)?;
    assert!(shown["account"]["sheet"].is_null());
    Ok(())
}

#[test]
fn the_lock_link_and_the_search_light_their_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("link:account.lock")?;
    bed.shows(|s| s["page"] == "power" && s["lit"] == "power.lock-on-idle")?;
    bed.type_text("avatar")?;
    bed.shows(|s| s["results"].as_array().is_some_and(|r| !r.is_empty()))?;
    bed.key(KEY_ENTER)?;
    bed.shows(|s| s["page"] == "account" && s["lit"] == "account.avatar")?;
    Ok(())
}

#[test]
fn at_rest_the_page_commits_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(User::default(), true)? else {
        return Ok(());
    };
    bed.open()?;
    bed.places_with("row:account.avatar")?;
    bed.hp.pointer_move(5.0, 5.0);
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

/// The pull request's screenshots, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start_on(User::default(), true, "2880x1800@2")? else {
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
            (window.x.max(0) * 2) as u32,
            (window.y.max(0) * 2) as u32,
            (window.width * 2) as u32,
            (window.height * 2) as u32,
        )
        .to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    bed.open()?;
    save(&mut bed, "account-page.png")?;
    bed.click("control:account.avatar")?;
    bed.shows(|s| s["account"]["popover"] == 0)?;
    save(&mut bed, "account-pandas.png")?;
    bed.click("panda:0")?;
    bed.shows(|s| s["account"]["panda"] == 0 && s["toast"] == "Avatar changé.")?;
    save(&mut bed, "account-panda-set.png")?;
    bed.click("toast-close")?;
    bed.shows(|s| s["toast"].is_null())?;
    bed.sheet("pas-le-bon", NEW, NEW)?;
    bed.shows(|s| s["account"]["sheet"]["ready"] == true)?;
    bed.click("password:change")?;
    bed.shows(|s| s["account"]["sheet"]["wrong_current"] == true)?;
    save(&mut bed, "account-password-wrong.png")?;
    bed.key(KEY_ESC)?;
    bed.shows(|s| s["account"]["sheet"].is_null())?;
    bed.sheet(CURRENT, SHORT, SHORT)?;
    bed.shows(|s| s["account"]["sheet"]["ready"] == true)?;
    bed.key(KEY_ENTER)?;
    bed.shows(|s| s["account"]["sheet"]["refused"].is_string())?;
    save(&mut bed, "account-password-refused.png")?;
    Ok(())
}
