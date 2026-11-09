//! Her way of being in the drawer (#753): the real shell and her real
//! process, on a clock of her own, with the scripted fake model.
//!
//! - "Vouvoie-moi", then "tutoie-moi": "Retenu : …" with "Annuler", and
//!   what it replaces said;
//! - "envoie sans me demander": refused in a plain sentence;
//! - "Ce qu'elle sait" › "Toi": the section « SA FAÇON D'ÊTRE »;
//! - "plus court" after her answers three times, then the night: her
//!   observation asked, "Oui · Non · Plus tard".
//!
//! Pixman, as in CI. With `HOCUS_DRAWER_SHOTS=DIR`, PNGs are left there
//! (for review, never compared).

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{KEY_LEFTMETA, WAIT, chord, wait};
use hocus_agents::tools::Places;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, SecretaryMode};
use hocus_link::ToShell;
use hocus_secretary_link::{Client, Connection, Status, ToSecretary};

const KEY_S: u32 = 31;

const MEMORY: &str = r#"
[[node]]
name = "Nadia Ferreira"
said = "2026-03-02T09:00:00Z"
"#;

const CALENDAR: &str = r#"
name = "fixture-calendar"
notebook = "pro"
[[items]]
id = "c1"
at = "2026-11-09T06:00:00Z"
until = "2026-11-09T07:00:00Z"
kind = "event"
digest = "Point d’équipe"
calendar = "Travail"
"#;

const MODEL: &str = r#"
name = "Claude Sonnet 5.5"
[default]
text = "Demain, tu as le point d’équipe à 9 h, puis la revue du site à 14 h avec Nadia, et rien d’autre jusqu’au soir."
"#;

fn ready() -> Option<(PathBuf, PathBuf)> {
    Some((common::shell_program()?, common::secretary_program()?))
}

struct Bed {
    dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    her: Connection,
}

fn start() -> anyhow::Result<Option<Bed>> {
    let Some((shell, secretary)) = ready() else {
        eprintln!("the shell or hocus-secretary is not built: skipped");
        return Ok(None);
    };
    let dir = tempfile::tempdir()?;
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".local/state/hocuspanda"))?;
    std::fs::write(home.join(".local/state/hocuspanda/day-sources"), "later\n")?;
    std::fs::create_dir_all(home.join("bin"))?;
    let write = |name: &str, text: &str| -> anyhow::Result<PathBuf> {
        let path = dir.path().join(name);
        std::fs::write(&path, text)?;
        Ok(path)
    };
    let model = write("model.toml", MODEL)?;
    let memory = write("memory.toml", MEMORY)?;
    let fixtures: Vec<PathBuf> = [("calendar.toml", CALENDAR)]
        .iter()
        .map(|(n, t)| write(n, t))
        .collect::<anyhow::Result<_>>()?;
    let power = dir.path().join("power");
    std::fs::create_dir_all(&power)?;
    let fixture_args: String = fixtures
        .iter()
        .map(|f| format!(" --fixture '{}'", f.display()))
        .collect();
    let her = dir.path().join("secretary");
    std::fs::write(
        &her,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\nexport XDG_DATA_HOME='{}' \
             HOCUS_POWER_SUPPLY_DIR='{}' HOCUS_THERMAL_DIR='{}' \
             HOCUS_HWMON_DIR=/nonexistent HOCUS_NVIDIA_SMI=/nonexistent\n\
             exec '{}' --virtual-clock 2026-11-09T08:00:00Z --fake-model '{}' \
             --memory-fixture '{}'{fixture_args} \"$@\" 2>>'{}'\n",
            dir.path().join("data").display(),
            power.display(),
            power.display(),
            secretary.display(),
            model.display(),
            memory.display(),
            dir.path().join("secretary.log").display(),
        ),
    )?;
    std::fs::set_permissions(&her, std::fs::Permissions::from_mode(0o755))?;
    let log = dir.path().join("shell.log");
    let script = dir.path().join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' \
             RUST_LOG=warn,hocuspanda_shell=debug\nexec '{}' \"$@\" 2>>'{}'\n",
            home.display(),
            shell.display(),
            log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut config = Config {
        secretary_program: Some(her),
        agent_places: Some(Places {
            home: home.clone(),
            config_home: home.join(".config"),
            codex_home: home.join(".codex"),
            path: vec![home.join("bin")],
            herdr_socket: home.join(".config/herdr/herdr.sock"),
            processes: home.join("proc"),
            hocus: home.join("bin/hocus"),
            state_dir: Some(home.join("state")),
        }),
        ..Config::default()
    };
    config.secretary.mode = SecretaryMode::Assistant;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    common::drawn(&mut hp, hocus_link::NAMESPACE)?;
    let socket = hp
        .secretary_socket()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("no socket"))?;
    let started = Instant::now();
    let mut her = loop {
        if let Ok(c) = Connection::open(&socket, Client::Cli, Duration::from_secs(5)) {
            break c;
        }
        anyhow::ensure!(started.elapsed() < WAIT, "she never listened");
        hp.dispatch(Duration::from_millis(20))?;
    };
    status_until(&mut hp, &mut her, |s| {
        s.connections
            .iter()
            .any(|c| c.client == Some(Client::Shell))
            && s.sources.iter().all(|s| !s.busy && s.synced_at.is_some())
    })?;
    Ok(Some(Bed { dir, hp, log, her }))
}

fn status_until(
    hp: &mut Headless,
    her: &mut Connection,
    mut done: impl FnMut(&Status) -> bool,
) -> anyhow::Result<Status> {
    let start = Instant::now();
    loop {
        let status = her.status().map_err(anyhow::Error::msg)?;
        if done(&status) {
            return Ok(status);
        }
        anyhow::ensure!(start.elapsed() < WAIT, "never: {status:?}");
        hp.dispatch(Duration::from_millis(20))?;
    }
}

impl Bed {
    fn drawer(&self) -> Option<hocus_compositor::model::Rect> {
        self.hp.drawer_surfaces().first().map(|(_, r)| *r)
    }

    fn opened(&mut self) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            !hp.drawn_layers(hocus_link::SECRETARY_NAMESPACE).is_empty()
        })?;
        anyhow::ensure!(done, "no drawer");
        self.shows(|s| s["height"].as_f64().is_some_and(|h| h > 0.0))?;
        Ok(())
    }

    fn shows(
        &mut self,
        mut done: impl FnMut(&serde_json::Value) -> bool,
    ) -> anyhow::Result<serde_json::Value> {
        let log = self.log.clone();
        let found = self.hp.dispatch_until(WAIT, |_| {
            common::last_json(&log, "the drawer shows {").is_some_and(|s| done(&s))
        })?;
        let last = common::last_json(&self.log, "the drawer shows {");
        if !found {
            let hers =
                std::fs::read_to_string(self.dir.path().join("secretary.log")).unwrap_or_default();
            let tail: Vec<&str> = hers.lines().rev().take(15).collect();
            let shell = common::logged(&self.log);
            let acts: Vec<&str> = shell
                .lines()
                .filter(|l| {
                    l.contains("her memory") || l.contains("her toast") || l.contains("ToastUndo@")
                })
                .map(|l| &l[l.len().saturating_sub(300)..])
                .collect();
            anyhow::bail!("the drawer never showed it; she said: {tail:#?}; the shell: {acts:#?}");
        }
        Ok(last.unwrap_or_default())
    }

    /// The drawer's last places' item named `name`, global.
    fn place(&mut self, name: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!("{name}@");
        let log = self.log.clone();
        let found = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the drawer's items"))
                .and_then(|l| {
                    let at = l.split(&wanted).nth(1)?;
                    let at = at.split_whitespace().next()?;
                    let (x, y) = at.split_once(',')?;
                    Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
                })
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| found(&common::logged(&log)).is_some())?;
        anyhow::ensure!(done, "no {name} in the drawer");
        let (x, y) = found(&common::logged(&self.log)).unwrap_or_default();
        let r = self.drawer().ok_or_else(|| anyhow::anyhow!("no drawer"))?;
        Ok((f64::from(r.x) + x, f64::from(r.y) + y))
    }

    fn click(&mut self, name: &str) -> anyhow::Result<()> {
        let at = self.place(name)?;
        common::click_on(&mut self.hp, at, hocus_link::SECRETARY_NAMESPACE)
    }

    fn shot(&mut self, name: &str) -> anyhow::Result<()> {
        let Some(dir) = std::env::var_os("HOCUS_DRAWER_SHOTS") else {
            return Ok(());
        };
        common::at_rest(&mut self.hp)?;
        wait(&mut self.hp, Duration::from_millis(800))?;
        common::at_rest(&mut self.hp)?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let Some(r) = self.drawer() else {
            return Ok(());
        };
        let x0 = (r.x - 8).max(0);
        let mut out = image::RgbaImage::new((width - x0) as u32, height as u32);
        for y in 0..height {
            for x in x0..width {
                let at = ((y * width + x) * 4) as usize;
                if let Some(p) = bytes.get(at..at + 4) {
                    out.put_pixel(
                        (x - x0) as u32,
                        y as u32,
                        image::Rgba([p[2], p[1], p[0], 255]),
                    );
                }
            }
        }
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        out.save(dir.join(format!("{name}.png")))?;
        Ok(())
    }
}

/// Her way of being by the clicks and her sentences; its pictures.
#[test]
fn her_way_of_being_is_kept_refused_listed_and_proposed() -> anyhow::Result<()> {
    let Some(mut bed) = start()? else {
        return Ok(());
    };
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_S]);
    bed.opened()?;
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("Vouvoie-moi".to_owned()));
    bed.shows(|s| s["items"].to_string().contains("Retenu\u{a0}: vouvoiement"))?;
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("Finalement, tutoie-moi".to_owned()));
    let shown = bed.shows(|s| s["items"].to_string().contains("Retenu\u{a0}: tutoiement"))?;
    assert!(
        shown["items"].to_string().contains("Ça remplace"),
        "{}",
        shown["items"]
    );
    bed.shot("style-kept")?;
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("pas de listes".to_owned()));
    bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("Retenu\u{a0}: pas de listes")
    })?;
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("envoie sans me demander".to_owned()));
    bed.shows(|s| s["items"].to_string().contains("sans ton clic"))?;
    bed.shot("style-refused")?;

    // "Ce qu'elle sait" › "Toi".
    bed.click("TabKnows")?;
    bed.shows(|s| s["tab"] == "knows")?;
    let shown = bed.shows(|s| s["items"].to_string().contains("section:SA FAÇON D’ÊTRE"))?;
    let items = shown["items"].to_string();
    for words in ["Tutoiement", "Pas de listes", "tu me l’as dit"] {
        assert!(items.contains(words), "{words}: {items}");
    }
    assert!(!items.contains("Vouvoiement"), "replaced: {items}");
    bed.shot("knows-style")?;

    // "Plus court" three times, then the night: her observation.
    bed.click("TabTalk")?;
    bed.shows(|s| s["tab"] == "talk")?;
    for question in ["qu’ai-je demain ?", "et jeudi ?", "et vendredi ?"] {
        bed.hp
            .tell_shell(ToShell::SecretaryAsk(question.to_owned()));
        bed.shows(|s| s["items"].to_string().matches("rien d’autre").count() >= 1)?;
        bed.hp
            .tell_shell(ToShell::SecretaryAsk("plus court".to_owned()));
        wait(&mut bed.hp, Duration::from_millis(300))?;
    }
    bed.her
        .send(&ToSecretary::Clock { advance: 24 * 3600 })
        .map_err(anyhow::Error::msg)?;
    bed.shows(|s| s["items"].to_string().contains("plus court d’office"))?;
    bed.shot("style-observation")?;
    Ok(())
}
