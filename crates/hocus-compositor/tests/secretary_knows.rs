//! "Ce qu'elle sait" in the drawer (docs/design/secretary.md §8, lot 6):
//! the real shell and her real process, on a clock of her own, with a
//! memory fixture, fixture sources and the scripted fake model.
//!
//! - The tab at 720 px lists what she knows by section, each item with
//!   "d'où je le sais"; a click unfolds an item's provenance (its quoted
//!   words, its source to open).
//! - AC29 by the clicks: "Oublier" with its toast "Oublié · Annuler";
//!   "Annuler" brings it back.
//! - AC40 by the clicks: "Oublier tout le perso…" asks to confirm, then the
//!   perso notebook is gone and the pro one stays.
//! - AC27 and AC30 by the clicks: an arrival's card in the thread, kept by
//!   "Ajouter"; her observation of a board barely used, a proposal, kept by
//!   "Oui".
//!
//! Pixman, as in CI. With `HOCUS_DRAWER_SHOTS=DIR`, PNGs of the tab are
//! left there (for review, never compared).

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
aliases = ["Nad"]
said = "2026-03-02T09:00:00Z"
[[node]]
name = "Sophie Marchand"
said = "2026-10-02T09:00:00Z"
[[node]]
name = "Marc Delaunay"
said = "2026-10-02T09:00:00Z"
[[node]]
name = "Chapitô"
kind = "project"
aliases = ["chapito", "le cirque", "CHAP-"]
summary = "billetterie pour le spectacle vivant"
said = "2026-10-02T09:00:00Z"
[[node]]
name = "Le site web"
kind = "project"
aliases = ["le site", "le blog"]
summary = "le site et le blog d’Atelier Lune"
said = "2026-10-02T09:00:00Z"
[[node]]
name = "Claire"
notebook = "perso"
said = "2026-06-14T09:00:00Z"

[[edge]]
src = "Nadia Ferreira"
type = "arrived"
value = "2022-03-01"
from = "2022-03-01"
said = "2026-10-12T09:00:00Z"
[[edge]]
src = "Nadia Ferreira"
type = "role"
value = "lead dev"
from = "2022-03-01"
said = "2026-10-12T09:00:00Z"
[[edge]]
src = "Nadia Ferreira"
type = "leads"
dst = "Chapitô"
from = "2026-03-01"
said = "2026-10-12T09:00:00Z"
[[edge]]
src = "Sophie Marchand"
type = "arrived"
value = "2023-09-01"
from = "2023-09-01"
said = "2026-10-02T09:00:00Z"
[[edge]]
src = "Sophie Marchand"
type = "role"
value = "dev back"
from = "2023-09-01"
said = "2026-10-02T09:00:00Z"
[[edge]]
src = "Sophie Marchand"
type = "works_on"
dst = "Chapitô"
from = "2023-09-01"
said = "2026-10-02T09:00:00Z"
[[edge]]
src = "Sophie Marchand"
type = "reviews"
dst = "Le site web"
provenance = "inferred"
pointer = "fixture-board:SW-88"
[[edge]]
src = "Marc Delaunay"
type = "arrived"
value = "2023-01-09"
from = "2023-01-09"
said = "2026-10-02T09:00:00Z"
[[edge]]
src = "Marc Delaunay"
type = "works_on"
dst = "Chapitô"
from = "2023-01-09"
to = "2026-11-06"
said = "2026-10-02T09:00:00Z"
[[edge]]
src = "Marc Delaunay"
type = "left"
value = "2026-11-06"
from = "2026-11-06"
said = "2026-11-06T17:00:00Z"
[[edge]]
src = "Claire"
type = "likes"
value = "la céramique"
provenance = "extracted"
source = "fixture-mail-perso"
item = "p1"
quote = "je me suis mise à la céramique"
accepted = true
notebook = "perso"
said = "2026-06-14T09:00:00Z"

[[rule]]
sentence = "jamais de rappel pendant le daily, même s’il déborde"
compiled = { kind = "restraint", event = "daily" }
said = "2026-10-14T09:00:00Z"
[[rule]]
sentence = "les mails du cabinet Morel : me les signaler le jour même"
compiled = { kind = "urgent", sender = "cabinet Morel" }
said = "2026-11-03T09:00:00Z"
"#;

const SLACK: &str = r##"
name = "fixture-slack"
notebook = "pro"
[[items]]
id = "n1"
at = "2026-11-09T07:30:00Z"
kind = "message"
digest = "#général : Julie Berthier a rejoint le canal · Développeuse front"
open = "https://atelier-lune.slack.com/archives/C01/p1"
[[items]]
id = "n2"
at = "2026-11-09T07:40:00Z"
kind = "message"
digest = "#site-web : Julie Berthier @julie.b : bonjour, je commence sur le site"
"##;

const CALENDAR: &str = r#"
name = "fixture-calendar"
notebook = "pro"
[[items]]
id = "c1"
at = "2026-11-09T06:00:00Z"
until = "2026-11-16T22:00:00Z"
kind = "event"
digest = "Congés Sophie"
calendar = "Congés"
"#;

const BOARD: &str = r#"
name = "fixture-board"
notebook = "pro"
[[items]]
id = "P-1"
at = "2026-09-20T07:00:00Z"
kind = "card"
digest = "Planka : ranger les factures"
"#;

const PERSO: &str = r#"
name = "fixture-mail-perso"
notebook = "perso"
[[items]]
id = "p1"
at = "2026-11-08T18:00:00Z"
kind = "mail"
digest = "Claire : des nouvelles"
"#;

const MODEL: &str = r#"
name = "Claude Sonnet 5.5"
[default]
text = "Bien noté."
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
    let fixtures: Vec<PathBuf> = [
        ("slack.toml", SLACK),
        ("calendar.toml", CALENDAR),
        ("board.toml", BOARD),
        ("perso.toml", PERSO),
    ]
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
             HOCUS_POWER_SUPPLY_DIR='{}' HOCUS_THERMAL_DIR='{}'\n\
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

    /// The index of the shown item whose line holds `words`.
    fn index_of(&mut self, words: &str) -> anyhow::Result<u32> {
        let shown = self.shows(|s| s["items"].to_string().contains(words))?;
        let items = shown["items"].as_array().cloned().unwrap_or_default();
        items
            .iter()
            .filter_map(|i| i.as_str())
            .find(|i| {
                i.contains(words)
                    && i.split(':')
                        .next()
                        .is_some_and(|n| n.parse::<u32>().is_ok())
            })
            .and_then(|i| i.split(':').next()?.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("no item with {words:?}"))
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

/// The tab: 720 px, its sections, "d'où je le sais"; an item unfolds its
/// provenance; "Oublier" and its "Annuler"; "Oublier tout le perso…"
/// confirmed; her observation accepted.
#[test]
fn what_she_knows_shows_says_where_from_and_forgets_on_a_click() -> anyhow::Result<()> {
    let Some(mut bed) = start()? else {
        return Ok(());
    };
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_S]);
    bed.opened()?;
    // An arrival said, its card, "Ajouter" (AC27 by the clicks); a rule.
    bed.hp.tell_shell(ToShell::SecretaryAsk(
        "les personnes en congés sont notées dans le calendrier Congés".to_owned(),
    ));
    bed.shows(|s| s["items"].to_string().contains("learned:Retenu · règle n"))?;
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("Julie est arrivée lundi".to_owned()));
    bed.shows(|s| s["items"].to_string().contains("Nouvelle personne"))?;
    bed.shot("talk-arrival")?;
    let button = {
        let log = common::logged(&bed.log);
        log.lines()
            .rev()
            .find(|l| l.contains("the drawer's items"))
            .and_then(|l| {
                l.split_whitespace()
                    .rfind(|w| w.starts_with("Button(") && w.contains(",Accept)@"))
                    .and_then(|w| w.split('@').next())
                    .map(str::to_owned)
            })
            .ok_or_else(|| anyhow::anyhow!("no Ajouter"))?
    };
    bed.click(&button)?;
    bed.shows(|s| s["items"].to_string().contains("ajouté·e à l’équipe"))?;

    // The tab.
    bed.click("TabKnows")?;
    let shown =
        bed.shows(|s| s["tab"] == "knows" && s["items"].to_string().contains("section:PERSONNES"))?;
    let wide = bed.hp.dispatch_until(WAIT, |hp| {
        hp.drawer_surfaces().first().map(|(_, r)| r.width) == Some(720)
    })?;
    assert!(wide, "720 px: {:?}", bed.drawer());
    let items = shown["items"].to_string();
    for words in [
        "section:APPRIS CETTE SEMAINE",
        "section:PROJETS ET LEURS NOMS",
        "section:RÈGLES",
        "section:SES OUTILS",
        "section:PROCHES",
        "Nadia Ferreira",
        "tu me l’as dit",
        "gardé pour l’histoire",
    ] {
        assert!(items.contains(words), "{words}: {items}");
    }
    assert_eq!(shown["status"], "Tout ce qu’elle sait, d’où elle le tient");
    bed.shot("knows-list")?;

    // An item's provenance: Julie's role, read in Slack.
    let julie = bed.index_of("Julie Berthier · Développeuse front")?;
    bed.click(&format!("Expand({julie})"))?;
    bed.shows(|s| s["items"].to_string().contains("| unfolded"))?;
    let shown = bed.shows(|s| s["items"].to_string().contains("lu dans Slack"))?;
    assert!(shown["items"].to_string().contains("Julie Berthier"));
    bed.place(&format!("Open({julie})"))?;
    bed.shot("knows-provenance")?;

    // "Oublier" a rule, its toast, "Annuler" (within its five seconds).
    let rule = bed.index_of("cabinet Morel")?;
    bed.click(&format!("Known({rule},Forget)"))?;
    let gone = bed.shows(|s| s["items"].to_string().contains("toast:Oublié · Annuler"))?;
    assert!(!gone["items"].to_string().contains("cabinet Morel"));
    bed.click("ToastUndo")?;
    bed.shows(|s| {
        let items = s["items"].to_string();
        items.contains("cabinet Morel") && !items.contains("toast:")
    })?;
    // Again, for its picture: the toast stays its five seconds.
    let rule = bed.index_of("cabinet Morel")?;
    bed.click(&format!("Known({rule},Forget)"))?;
    bed.shows(|s| s["items"].to_string().contains("toast:Oublié · Annuler"))?;
    bed.shot("knows-forgotten")?;

    // "Oublier tout le perso…", confirmed.
    bed.click("ForgetPerso")?;
    bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("confirm:Oublier tout le perso ?")
    })?;
    bed.shot("knows-forget-perso")?;
    bed.click("Confirm(true)")?;
    let after = bed.shows(|s| {
        let items = s["items"].to_string();
        !items.contains("confirm:") && !items.contains("Claire") && items.contains("Nadia Ferreira")
    })?;
    assert!(!after["items"].to_string().contains("section:PROCHES"));

    // The night: her observation of the board, a proposal (AC30).
    bed.her
        .send(&ToSecretary::Clock { advance: 24 * 3600 })
        .map_err(anyhow::Error::msg)?;
    bed.shows(|s| s["items"].to_string().contains("Planka"))?;
    let seen = bed.index_of("Je n’en parle que si tu demandes")?;
    bed.shows(|s| s["items"].to_string().contains("Oui/Non/Plus tard"))?;
    bed.shot("knows-proposal")?;
    bed.click(&format!("Known({seen},Accept)"))?;
    bed.shows(|s| {
        let items = s["items"].to_string();
        !items.contains("Je n’en parle que si tu demandes") && items.contains("Planka")
    })?;
    Ok(())
}
