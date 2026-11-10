//! The secretary's briefing and restraint (docs/design/secretary.md §6,
//! lot 5): the real shell and her real process, her clock of her own, the
//! scripted fake model (its triage too), fixture sources; the shell's
//! clock set alike (`HOCUS_CLOCK_AT`), both in UTC.
//!
//! - AC9: a login before 07:00 makes no peek; the first unlock after it
//!   makes exactly one, « Ton point du matin est prêt. » with what is in
//!   it, « Voir le point · Plus tard ▾ » and no « Fait » (#758), and the
//!   drawer does not open; its bubble opens the briefing in the drawer,
//!   under its own bar, its free slot named, each mail to answer with
//!   « Ouvrir le mail ↗ » and « Rappelle-le-moi »; a second unlock makes
//!   none.
//! - AC44: after 14 h asleep, one "Pendant ton absence" peek after the
//!   unlock, the gap's mails in the briefing.
//! - AC12, AC38: during a meeting (the shell's featured calendar), her
//!   peek is held and the drawer shows no perso title, only how many;
//!   "Montrer" shows them.
//! - Her first effector: "Mettre dans À faire" on a promise's card adds
//!   the task to the local To do list.
//!
//! Pixman, so the shell draws on the CPU, as in CI. With
//! `HOCUS_DRAWER_SHOTS=DIR`, PNGs of her peek, the briefing and "Pendant
//! ton absence" are left there (for review, never compared).

use crate::common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{KEY_LEFTMETA, WAIT, chord, wait};
use hocus_agents::tools::Places;
use hocus_compositor::{
    Authenticator, Config, Headless, HeadlessConfig, LockState, RendererChoice, SecretaryMode,
    model::Rect,
};
use hocus_link::{DayAsk, ToShell};
use hocus_secretary_link::{Client, Connection, Status, ToSecretary};

const KEY_S: u32 = 31;
const KEY_ENTER: u32 = 28;
/// "panda", the tests' password.
const PANDA: [u32; 5] = [25, 30, 49, 32, 30];

const MODEL: &str = r#"
name = "Claude Sonnet 5.5"

[[reply]]
role = "briefing"
text = """Une journée chargée, mais un bon créneau ce matin.
Deux PR à relire sur Chapitô.
Demain, rien de prévu : le 11 novembre est férié."""

[default]
text = "D'accord."

[[triage]]
contains = ["[m1]"]
verdict = { for_me = true, needs_reply = true, urgency = "high", digest = "il manque 2 justificatifs, avant jeudi" }

[[triage]]
contains = ["[m2]"]
verdict = { for_me = true, needs_reply = true, digest = "bloquée sans accès depuis son arrivée" }

[[triage]]
contains = ["[g1]"]
verdict = { for_me = true, needs_reply = true, urgency = "high", digest = "elle attend ta validation avant midi" }

[[triage]]
contains = ["[s1]"]
verdict = { digest = "Le devis à Paul", commitments = ["envoyer le devis à Paul avant 16 h"] }

[[triage]]
contains = ["[p1]"]
verdict = { digest = "Les places pour Claire", commitments = ["réserver les places pour Claire"] }
"#;

/// Her calendar and mail on Monday 9 November (UTC), and a perso mail.
fn fixtures(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let calendar = r#"
name = "fixture-calendar"
notebook = "pro"
[[items]]
id = "daily"
at = "2026-11-09T09:30:00Z"
kind = "event"
digest = "Daily Chapitô"
event = { end = "2026-11-09T09:45:00Z", calendar = "Travail", attendees = ["Léa Martin", "Karim Benali"] }
[[items]]
id = "site"
at = "2026-11-09T11:00:00Z"
kind = "event"
digest = "Point site web avec Léa"
event = { end = "2026-11-09T11:30:00Z", calendar = "Travail", attendees = ["Léa Martin"] }
[[items]]
id = "review"
at = "2026-11-09T14:00:00Z"
kind = "event"
digest = "Revue de sprint Chapitô"
event = { end = "2026-11-09T15:00:00Z", calendar = "Travail", attendees = ["Sophie Marchand", "Karim Benali"], presenter = "Sophie Marchand" }
"#;
    let mail = r#"
name = "fixture-mail"
notebook = "pro"
[[items]]
id = "m1"
at = "2026-11-09T06:10:00Z"
kind = "mail"
digest = "Notes de frais d’octobre"
sender = "Cabinet Morel <morel@cabinet.fr>"
body = "Pouvez-vous valider avant jeudi ? [m1]"
mail = { to = ["Alex <alex@atelier-lune.fr>"], message_id = "<m1@mail>" }
[[items]]
id = "m2"
at = "2026-11-09T06:20:00Z"
kind = "mail"
digest = "Accès GitLab et Planka"
sender = "Julie Berthier <julie@atelier-lune.fr>"
body = "Je n'ai toujours pas les accès. [m2]"
mail = { to = ["Alex <alex@atelier-lune.fr>"], message_id = "<m2@mail>" }
[[items]]
id = "m3"
at = "2026-11-09T06:25:00Z"
kind = "mail"
digest = "La lettre de Scaleway"
sender = "Scaleway <news@scaleway.com>"
body = "Nouveautés du mois. [m3]"
mail = { to = ["Alex <alex@atelier-lune.fr>"] }
[[items]]
id = "s1"
at = "2026-11-09T06:28:00Z"
kind = "mail"
digest = "Le devis"
sender = "Alex <alex@atelier-lune.fr>"
body = "Je t'envoie le devis avant 16 h. [s1]"
mail = { to = ["Paul Durand <paul@client.fr>"], sent = true }
[[items]]
id = "g1"
at = "2026-11-09T22:00:00Z"
kind = "mail"
digest = "La maquette de la home"
sender = "Léa Martin <lea@atelier-lune.fr>"
body = "Tu peux valider la maquette ? [g1]"
mail = { to = ["Alex <alex@atelier-lune.fr>"], message_id = "<g1@mail>" }
[[items]]
id = "g2"
at = "2026-11-10T03:00:00Z"
kind = "mail"
digest = "Facture OVH"
sender = "OVH <noreply@ovh.com>"
body = "Votre facture. [g2]"
mail = { to = ["Alex <alex@atelier-lune.fr>"] }
"#;
    let perso = r#"
name = "fixture-perso"
notebook = "perso"
[[items]]
id = "dinner"
at = "2026-11-09T19:30:00Z"
kind = "event"
digest = "Dîner chez Claire"
event = { end = "2026-11-09T22:00:00Z", calendar = "Famille" }
[[items]]
id = "p1"
at = "2026-11-09T06:29:00Z"
kind = "mail"
digest = "Le concert"
sender = "Alex <alex.perso@gmail.com>"
body = "Je réserve les places pour Claire. [p1]"
mail = { to = ["Claire <claire@gmail.com>"], sent = true }
"#;
    let mut files = Vec::new();
    for (name, text) in [
        ("calendar.toml", calendar),
        ("mail.toml", mail),
        ("perso.toml", perso),
    ] {
        let file = dir.join(name);
        std::fs::write(&file, text)?;
        files.push(file);
    }
    Ok(files)
}

fn ready() -> Option<(PathBuf, PathBuf)> {
    Some((common::shell_program(), common::secretary_program()))
}

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    her_log: PathBuf,
    home: PathBuf,
    her: Connection,
}

/// The compositor, the shell and her: her clock at `her_clock`, the
/// shell's at `shell_clock` (UTC), the shell's day a fixture with
/// `day_events` (`[[events]]` of its featured calendar).
fn start(her_clock: &str, shell_clock: &str, day_events: &str) -> anyhow::Result<Option<Bed>> {
    let Some((shell, secretary)) = ready() else {
        eprintln!("the shell or hocus-secretary is not built: skipped");
        return Ok(None);
    };
    let dir = tempfile::tempdir()?;
    let home = dir.path().join("home");
    let config = home.join(".config/hocuspanda");
    std::fs::create_dir_all(home.join(".local/state/hocuspanda"))?;
    std::fs::create_dir_all(&config)?;
    std::fs::create_dir_all(home.join("bin"))?;
    std::fs::write(home.join(".local/state/hocuspanda/day-sources"), "later\n")?;
    let config_file = config.join("config.toml");
    std::fs::write(
        &config_file,
        "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\n\
         [secretary]\nmode = \"assistant\"\n\n[secretary.triage]\npace = 0\n",
    )?;
    std::fs::write(
        config.join("day.toml"),
        format!(
            "[[calendars]]\nid = \"work\"\nname = \"Travail\"\ncolor = \"#6F9BFF\"\n{day_events}"
        ),
    )?;
    let model = dir.path().join("model.toml");
    std::fs::write(&model, MODEL)?;
    let fixtures: Vec<String> = fixtures(dir.path())?
        .iter()
        .map(|f| format!("--fixture '{}'", f.display()))
        .collect();
    let her_log = dir.path().join("secretary.log");
    let her_script = dir.path().join("secretary");
    std::fs::write(
        &her_script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\nexport XDG_DATA_HOME='{data}' TZ=UTC \
             HOCUS_POWER_SUPPLY_DIR=/nonexistent HOCUS_THERMAL_DIR=/nonexistent \
             HOCUS_HWMON_DIR=/nonexistent HOCUS_NVIDIA_SMI=/nonexistent \
             RUST_LOG=hocus_secretary=info\n\
             exec '{secretary}' --virtual-clock {her_clock} --fake-model '{model}' \
             --recorder '{calls}' --config '{config}' {fixtures} \"$@\" 2>>'{log}'\n",
            data = dir.path().join("data").display(),
            secretary = secretary.display(),
            model = model.display(),
            calls = dir.path().join("calls.jsonl").display(),
            config = config_file.display(),
            fixtures = fixtures.join(" "),
            log = her_log.display(),
        ),
    )?;
    std::fs::set_permissions(&her_script, std::fs::Permissions::from_mode(0o755))?;
    let log = dir.path().join("shell.log");
    let script = dir.path().join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{home}' TZ=UTC \
             XDG_CONFIG_HOME='{home}/.config' HOCUS_CLOCK_AT='{shell_clock}' \
             RUST_LOG=warn,hocuspanda_shell=debug\nexec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut config = Config {
        file: Some(config_file),
        secretary_program: Some(her_script),
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
        plain_background: true,
        ..Config::default()
    };
    config.secretary.mode = SecretaryMode::Assistant;
    config.keyboard.layouts = vec!["us".to_owned()];
    config.lock_auth = Authenticator::test("panda");
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
    let mut her = connect(&mut hp)?;
    status_until(&mut hp, &mut her, |s| {
        s.connections
            .iter()
            .any(|c| c.client == Some(Client::Shell))
            && s.sources.iter().all(|s| !s.busy && s.synced_at.is_some())
    })?;
    Ok(Some(Bed {
        _dir: dir,
        hp,
        log,
        her_log,
        home,
        her,
    }))
}

fn connect(hp: &mut Headless) -> anyhow::Result<Connection> {
    let socket = hp
        .secretary_socket()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("no socket"))?;
    let start = Instant::now();
    loop {
        if let Ok(c) = Connection::open(&socket, Client::Cli, Duration::from_secs(5)) {
            return Ok(c);
        }
        anyhow::ensure!(start.elapsed() < WAIT, "she never listened");
        hp.dispatch(Duration::from_millis(20))?;
    }
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
    fn advance(&mut self, seconds: u64) -> anyhow::Result<()> {
        self.her
            .send(&ToSecretary::Clock { advance: seconds })
            .map_err(anyhow::Error::msg)
    }

    fn lock(&mut self) -> anyhow::Result<()> {
        self.hp.lock();
        let locked = self
            .hp
            .dispatch_until(WAIT, |hp| hp.lock_state() != LockState::Unlocked)?;
        anyhow::ensure!(locked, "never locked");
        wait(&mut self.hp, Duration::from_millis(300))
    }

    fn unlock(&mut self) -> anyhow::Result<()> {
        for key in PANDA {
            chord(&mut self.hp, &[key]);
        }
        chord(&mut self.hp, &[KEY_ENTER]);
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Unlocked)?;
        anyhow::ensure!(open, "the lock did not open");
        Ok(())
    }

    /// Her peek, once out and drawn: its request, its surface.
    fn peek(&mut self) -> anyhow::Result<(u64, Rect)> {
        let out = self.hp.dispatch_until(WAIT, |hp| {
            hp.secretary_peek_out().is_some()
                && !hp.drawn_layers(hocus_link::PEEK_NAMESPACE).is_empty()
        })?;
        anyhow::ensure!(
            out,
            "her peek never came out; the shell said:\n{}",
            self.said("her peek")
        );
        let id = self.hp.secretary_peek_out().unwrap_or_default();
        let rect = self
            .hp
            .peek_surfaces()
            .first()
            .map(|(_, r)| *r)
            .ok_or_else(|| anyhow::anyhow!("no peek surface"))?;
        Ok((id, rect))
    }

    /// The shell's log lines holding `what`.
    fn said(&self, what: &str) -> String {
        common::logged(&self.log)
            .lines()
            .filter(|l| l.contains(what))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Where her peek's `item` is, global.
    fn peek_place(&mut self, item: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!("{item}@");
        let log = self.log.clone();
        let found = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("her peek's items"))
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
        anyhow::ensure!(done, "no {item} on her peek");
        let (x, y) = found(&common::logged(&self.log)).unwrap_or_default();
        let (_, r) = self.peek()?;
        Ok((f64::from(r.x) + x, f64::from(r.y) + y))
    }

    /// What the drawer last said it shows, once `done` holds for it.
    fn shows(
        &mut self,
        mut done: impl FnMut(&serde_json::Value) -> bool,
    ) -> anyhow::Result<serde_json::Value> {
        let log = self.log.clone();
        let found = self.hp.dispatch_until(WAIT, |_| {
            common::last_json(&log, "the drawer shows {").is_some_and(|s| done(&s))
        })?;
        let last = common::last_json(&self.log, "the drawer shows {");
        anyhow::ensure!(found, "the drawer never showed it; last: {last:#?}");
        Ok(last.unwrap_or_default())
    }

    /// Where the drawer's `item` is, global.
    fn drawer_place(&mut self, item: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!("{item}@");
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
        anyhow::ensure!(done, "no {item} in the drawer");
        let (x, y) = found(&common::logged(&self.log)).unwrap_or_default();
        let r = self.drawer().ok_or_else(|| anyhow::anyhow!("no drawer"))?;
        Ok((f64::from(r.x) + x, f64::from(r.y) + y))
    }

    /// The drawer's place starting with `prefix`.
    fn drawer_item(&mut self, prefix: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let find = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the drawer's items"))
                .and_then(|l| {
                    l.split_whitespace()
                        .find(|w| w.starts_with(prefix))
                        .and_then(|w| w.split('@').next())
                        .map(str::to_owned)
                })
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| find(&common::logged(&log)).is_some())?;
        anyhow::ensure!(done, "no {prefix}… in the drawer");
        Ok(find(&common::logged(&self.log)).unwrap_or_default())
    }

    fn drawer(&self) -> Option<Rect> {
        self.hp.drawer_surfaces().first().map(|(_, r)| *r)
    }

    fn opened(&mut self) -> anyhow::Result<Rect> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            !hp.drawn_layers(hocus_link::SECRETARY_NAMESPACE).is_empty()
        })?;
        anyhow::ensure!(done, "no drawer");
        self.drawer().ok_or_else(|| anyhow::anyhow!("no drawer"))
    }

    /// A PNG of `rect` widened to the screen's right edge, when the run
    /// asks for screenshots (`HOCUS_DRAWER_SHOTS`).
    fn shot(&mut self, name: &str, rect: Rect) -> anyhow::Result<()> {
        let Some(dir) = std::env::var_os("HOCUS_DRAWER_SHOTS") else {
            return Ok(());
        };
        common::at_rest(&mut self.hp)?;
        wait(&mut self.hp, Duration::from_millis(300))?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let x0 = (rect.x - 16).max(0);
        let y0 = (rect.y - 16).max(0);
        let y1 = (rect.y + rect.height + 16).min(height);
        let mut out = image::RgbaImage::new((width - x0) as u32, (y1 - y0) as u32);
        for y in y0..y1 {
            for x in x0..width {
                let at = ((y * width + x) * 4) as usize;
                if let Some(p) = bytes.get(at..at + 4) {
                    out.put_pixel(
                        (x - x0) as u32,
                        (y - y0) as u32,
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

/// AC9, and her briefing opened from her peek.
#[test]
fn the_first_unlock_after_seven_brings_her_out_once() -> anyhow::Result<()> {
    let Some(mut bed) = start("2026-11-09T06:30:00Z", "2026-11-09T08:30:00", "")? else {
        return Ok(());
    };
    // Logged in before 07:00: nothing.
    wait(&mut bed.hp, Duration::from_secs(1))?;
    anyhow::ensure!(bed.hp.peeks_told() == 0, "a peek before 07:00");
    bed.lock()?;
    bed.advance(2 * 3600)?;
    bed.unlock()?;
    let (id, rect) = bed.peek()?;
    let said = bed.said("her peek shows");
    anyhow::ensure!(said.contains("Ton point du matin est prêt."), "{said}");
    anyhow::ensure!(said.contains("Voir le point | Plus tard ▾"), "{said}");
    anyhow::ensure!(!said.contains("| Fait"), "no Fait on a briefing: {said}");
    anyhow::ensure!(bed.hp.peeks_told() == 1);
    anyhow::ensure!(
        bed.hp.drawer_surfaces().is_empty(),
        "the drawer never opens by itself"
    );
    bed.shot("8-peek", rect)?;
    // Its bubble opens the briefing in the drawer: its own bar, no tabs
    // nor field; its day and its free slot; each mail to answer, its
    // actions.
    let open = bed.peek_place("Open")?;
    common::click_on(&mut bed.hp, open, hocus_link::PEEK_NAMESPACE)?;
    let drawer = bed.opened()?;
    let shows = bed.shows(|s| s["point"].as_u64().is_some())?;
    anyhow::ensure!(
        shows["subview"]
            .as_str()
            .is_some_and(|t| t.starts_with("Ton point du lundi 9 novembre")),
        "{shows}"
    );
    let text = shows.to_string();
    anyhow::ensure!(
        !text.contains("vraiment") && !text.contains("EN TROIS LIGNES"),
        "{text}"
    );
    let remind = bed.drawer_place("Remind(0)")?;
    bed.shot("9-briefing", drawer)?;
    common::click_on(&mut bed.hp, remind, hocus_link::SECRETARY_NAMESPACE)?;
    bed.shows(|s| s.to_string().contains("Je te le rappelle à"))?;
    let shows = bed.shows(|s| {
        s["point"].as_u64().is_some()
            && s["items"].as_array().is_some_and(|i| {
                i.iter()
                    .any(|t| t.as_str().is_some_and(|t| t.starts_with("section:answer")))
            })
    })?;
    let items: Vec<String> = shows["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let section = |key: &str| {
        items
            .iter()
            .find(|i| i.starts_with(&format!("section:{key}:")))
            .cloned()
            .unwrap_or_default()
    };
    anyhow::ensure!(
        section("today").contains("Daily Chapitô") && section("today").contains("Revue de sprint"),
        "{items:?}"
    );
    anyhow::ensure!(
        section("answer").contains("Cabinet Morel · Notes de frais")
            && section("answer").contains("Julie Berthier · Accès GitLab")
            && !section("answer").contains("Scaleway"),
        "{items:?}"
    );
    anyhow::ensure!(section("rest").contains("Deux PR à relire"), "{items:?}");
    let back = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.secretary_peek_out().is_none())?;
    anyhow::ensure!(back, "her peek goes back once answered");
    bed.shot("briefing", drawer)?;
    let _ = id;
    // A second unlock that day: none.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_S]);
    bed.hp
        .dispatch_until(WAIT, |hp| hp.drawer_surfaces().is_empty())?;
    bed.lock()?;
    bed.advance(3600)?;
    bed.unlock()?;
    wait(&mut bed.hp, Duration::from_secs(2))?;
    anyhow::ensure!(bed.hp.peeks_told() == 1, "a second peek that day");
    Ok(())
}

/// AC44: 14 h asleep; one "Pendant ton absence" after the unlock.
#[test]
fn after_fourteen_hours_away_she_says_what_came_once() -> anyhow::Result<()> {
    let Some(mut bed) = start("2026-11-09T18:00:00Z", "2026-11-09T18:00:00", "")? else {
        return Ok(());
    };
    // The evening's first session: the day's briefing, seen.
    bed.peek()?;
    let open = bed.peek_place("Open")?;
    common::click_on(&mut bed.hp, open, hocus_link::PEEK_NAMESPACE)?;
    let back = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.secretary_peek_out().is_none())?;
    anyhow::ensure!(back, "\"Voir le point\" sends her back");
    bed.opened()?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_S]);
    bed.hp
        .dispatch_until(WAIT, |hp| hp.drawer_surfaces().is_empty())?;
    let told = bed.hp.peeks_told();
    bed.lock()?;
    bed.hp.tell_shell(ToShell::Day(DayAsk::Slept(14 * 3600)));
    status_until(&mut bed.hp, &mut bed.her, |s| {
        s.catch_up.as_ref().is_some_and(|c| c.away_briefing)
            && s.triage.as_ref().is_some_and(|t| t.pending == 0)
    })?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    anyhow::ensure!(bed.hp.peeks_told() == told, "nothing while locked");
    bed.unlock()?;
    let (_, rect) = bed.peek()?;
    let said = bed.said("her peek shows");
    anyhow::ensure!(
        said.lines()
            .last()
            .is_some_and(|l| l.contains("Pendant ton absence")),
        "{said}"
    );
    anyhow::ensure!(bed.hp.peeks_told() == told + 1);
    bed.shot("absence-peek", rect)?;
    let open = bed.peek_place("Open")?;
    common::click_on(&mut bed.hp, open, hocus_link::PEEK_NAMESPACE)?;
    let drawer = bed.opened()?;
    let shows = bed.shows(|s| {
        s["items"].as_array().is_some_and(|i| {
            i.iter()
                .any(|t| t.as_str().is_some_and(|t| t.starts_with("section:away:")))
        })
    })?;
    let away = shows["items"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .find(|t| t.starts_with("section:away:"))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    anyhow::ensure!(
        away.contains("2 mails arrivés, dont 1 pour toi")
            && away.contains("La maquette de la home"),
        "{away}"
    );
    bed.shot("absence", drawer)?;
    wait(&mut bed.hp, Duration::from_secs(1))?;
    anyhow::ensure!(bed.hp.peeks_told() == told + 1, "one only");
    let _ = &bed.her_log;
    Ok(())
}

/// AC12 and AC38 during a meeting, and the To do list as her effector.
#[test]
fn in_a_meeting_perso_hides_and_a_promise_goes_to_to_do() -> anyhow::Result<()> {
    let meeting = "[[events]]\nid = \"call\"\ncalendar = \"work\"\ntitle = \"Appel client\"\n\
                   start = \"today 08:00\"\nend = \"today 10:00\"\n";
    let Some(mut bed) = start("2026-11-09T08:30:00Z", "2026-11-09T08:30:00", meeting)? else {
        return Ok(());
    };
    // Her briefing is ready, but a meeting takes time: no peek.
    let shell_log = bed.log.clone();
    let held = bed.hp.dispatch_until(WAIT, |_| {
        common::logged(&shell_log).contains("her peek is held")
    })?;
    anyhow::ensure!(held, "her peek is held: {}", bed.said("her peek"));
    anyhow::ensure!(bed.said("her peek is held").contains("why=\"meeting\""));
    anyhow::ensure!(bed.hp.peeks_told() == 0);
    // The drawer: the briefing, a promise, and the perso hidden.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_S]);
    let drawer = bed.opened()?;
    let shows = bed.shows(|s| {
        s["moment"] == "meeting"
            && s["items"].as_array().is_some_and(|i| {
                i.iter()
                    .any(|t| t.as_str().is_some_and(|t| t.starts_with("hidden:")))
                    && i.iter().any(|t| {
                        t.as_str()
                            .is_some_and(|t| t.starts_with("card:Plan:Ta promesse"))
                    })
            })
    })?;
    let text = shows.to_string();
    anyhow::ensure!(
        !text.contains("Claire") && !text.contains("Dîner"),
        "a perso title in the drawer: {text}"
    );
    bed.shot("perso-hidden", drawer)?;
    // The briefing too: its perso part counted only.
    let point = bed.drawer_item("Point(")?;
    let at = bed.drawer_place(&point)?;
    common::click_on(&mut bed.hp, at, hocus_link::SECRETARY_NAMESPACE)?;
    let shows = bed.shows(|s| s["point"].as_u64().is_some())?;
    let text = shows.to_string();
    anyhow::ensure!(
        !text.contains("Claire")
            && !text.contains("Dîner")
            && text.contains("section:perso: (+1 hidden)"),
        "{text}"
    );
    // "Montrer": by a deliberate click, they show.
    let reveal = bed.drawer_place("Reveal")?;
    common::click_on(&mut bed.hp, reveal, hocus_link::SECRETARY_NAMESPACE)?;
    let shows = bed.shows(|s| s.to_string().contains("Dîner chez Claire"))?;
    anyhow::ensure!(shows["point"].as_u64().is_some());
    // Back to the thread: the promise's card into To do.
    let back = bed.drawer_place("Back")?;
    common::click_on(&mut bed.hp, back, hocus_link::SECRETARY_NAMESPACE)?;
    bed.shows(|s| s["point"].is_null())?;
    let promise = match bed
        .her
        .request(|id| ToSecretary::Thread { id })
        .map_err(anyhow::Error::msg)?
    {
        hocus_secretary_link::FromSecretary::Thread { items, .. } => items
            .into_iter()
            .find_map(|i| match i.body {
                hocus_secretary_link::Body::Card { card } if card.text.contains("Paul") => {
                    Some(i.id)
                }
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("no promise to Paul"))?,
        other => anyhow::bail!("not a thread: {other:?}"),
    };
    let at = bed.drawer_place(&format!("Button({promise},Accept)"))?;
    common::click_on(&mut bed.hp, at, hocus_link::SECRETARY_NAMESPACE)?;
    bed.shows(|s| s.to_string().contains("Mis dans À faire."))?;
    let tasks = bed.home.join(".local/state/hocuspanda/tasks.json");
    let added = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&tasks)
            .is_ok_and(|t| t.contains("envoyer le devis à Paul avant 16 h"))
    })?;
    anyhow::ensure!(
        added,
        "the task is not in To do: {:?}",
        std::fs::read_to_string(&tasks)
    );
    Ok(())
}
