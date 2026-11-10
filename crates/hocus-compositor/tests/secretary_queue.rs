//! Her triage queue in the drawer (#754): the real shell and her real
//! process, on a clock of her own, fixture mails, and the scripted local
//! triage taking 39 s an item.
//!
//! - Her proposal in the thread: GitHub's automatic notifications, with
//!   their count, the same as the view's ("Oui, ne pas les trier · Non ·
//!   Plus tard").
//! - Her header's « N mails à trier › » clicked (#758): the queue unfolds
//!   in place of the thread, under its own bar (no tabs, no field): the
//!   mail read now, one card per sender grouped by what sends them (a
//!   person's without an action), the pace once measured; never an item's
//!   text, no header's name.
//! - « Ne pas trier les mails de GitHub », inside its card: its mails move
//!   to PAS TRIÉS, À TA DEMANDE; « Trier à nouveau les mails de GitHub »
//!   brings them back.
//! - Too hot: the pause, one reason, with the real threshold.
//! - "‹ Retour à la discussion": the thread again, and nothing of the
//!   queue is sent any more.
//!
//! Pixman, as in CI. With `HOCUS_DRAWER_SHOTS=DIR`, PNGs of the drawer are
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
use hocus_secretary_link::{Client, Connection, Status, ToSecretary};

const KEY_S: u32 = 31;

const MODEL: &str = r#"
name = "Claude Sonnet 5.5"
[default]
text = "Bien noté."
[[triage]]
delay = 39
verdict = { for_me = false, needs_reply = false, urgency = "low", digest = "trié" }
"#;

/// Mails as a morning's inbox has them: a forge's notifications, a
/// newsletter, people.
fn mails() -> String {
    let mut out = String::from("name = \"fixture-mail\"\nnotebook = \"pro\"\nevery = 3600\n");
    let mut minute = 0;
    let mut item = |id: String, sender: &str, subject: &str, body: &str, headers: &str| {
        minute += 1;
        out.push_str(&format!(
            "[[items]]\nid = \"{id}\"\nat = \"2026-11-09T06:{:02}:00Z\"\nkind = \"mail\"\n\
             digest = \"{subject}\"\nsender = \"{sender}\"\nbody = \"{body}\"\n{headers}",
            minute % 60
        ));
    };
    item(
        "lea1".to_owned(),
        "Léa Martin <lea@atelier-lune.fr>",
        "Le devis de Chapitô",
        "Tu peux relire le devis avant jeudi ?",
        "",
    );
    for n in 0..12 {
        item(
            format!("gh{n}"),
            "GitHub <notifications@github.com>",
            &format!("[mattboll/hocus-panda] PR #{} : le tiroir du tri", 740 + n),
            "mattboll commented on this pull request.",
            "mail = { headers = { \"X-GitHub-Reason\" = \"subscribed\", \"List-Unsubscribe\" = \"<https://github.com/notifications/unsubscribe/x>\" } }\n",
        );
    }
    for n in 0..5 {
        item(
            format!("gl{n}"),
            "GitLab <gitlab@git.atelier-lune.fr>",
            &format!("chapito | Pipeline #{} a échoué", 9120 + n),
            "Job deploy failed.",
            "mail = { headers = { \"X-GitLab-Project\" = \"chapito\" } }\n",
        );
    }
    for n in 0..3 {
        item(
            format!("news{n}"),
            "La Lettre du lundi <lettre@exemple.fr>",
            "Au sommaire cette semaine",
            "Les nouvelles de la semaine.",
            "mail = { headers = { \"List-Unsubscribe\" = \"<mailto:unsub@exemple.fr>\" } }\n",
        );
    }
    item(
        "lea2".to_owned(),
        "Léa Martin <lea@atelier-lune.fr>",
        "Re: le devis",
        "Merci, c'est bon pour moi.",
        "",
    );
    item(
        "paul".to_owned(),
        "Paul Durand <paul@cabinet-morel.fr>",
        "Rendez-vous de mardi",
        "On se voit mardi à 10 h ?",
        "",
    );
    out
}

fn ready() -> Option<(PathBuf, PathBuf)> {
    Some((common::shell_program()?, common::secretary_program()?))
}

struct Bed {
    dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    her: Connection,
    power: PathBuf,
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
    let model = dir.path().join("model.toml");
    std::fs::write(&model, MODEL)?;
    let fixture = dir.path().join("mail.toml");
    std::fs::write(&fixture, mails())?;
    let power = dir.path().join("power");
    std::fs::create_dir_all(&power)?;
    let her = dir.path().join("secretary");
    std::fs::write(
        &her,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\nexport XDG_DATA_HOME='{}' \
             HOCUS_POWER_SUPPLY_DIR='{}' HOCUS_THERMAL_DIR='{}' \
             HOCUS_HWMON_DIR=/nonexistent HOCUS_NVIDIA_SMI=/nonexistent TZ=Europe/Paris\n\
             exec '{}' --virtual-clock 2026-11-09T08:00:00Z --fake-model '{}' \
             --fixture '{}' \"$@\" 2>>'{}'\n",
            dir.path().join("data").display(),
            power.display(),
            power.display(),
            secretary.display(),
            model.display(),
            fixture.display(),
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
    Ok(Some(Bed {
        dir,
        hp,
        log,
        her,
        power,
    }))
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
            anyhow::bail!("the drawer never showed it: {last:#?}; she said: {tail:#?}");
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

    /// The drawer's places as it last logged them.
    fn places(&self) -> String {
        common::logged(&self.log)
            .lines()
            .rev()
            .find(|l| l.contains("the drawer's items"))
            .unwrap_or_default()
            .to_owned()
    }

    fn click(&mut self, name: &str) -> anyhow::Result<()> {
        let at = self.place(name)?;
        common::click_on(&mut self.hp, at, hocus_link::SECRETARY_NAMESPACE)
    }

    fn advance(&mut self, seconds: u64) -> anyhow::Result<()> {
        self.her
            .send(&ToSecretary::Clock { advance: seconds })
            .map_err(anyhow::Error::msg)
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

fn items(shown: &serde_json::Value) -> String {
    shown["items"].to_string()
}

#[test]
fn her_queue_unfolds_from_her_status_and_a_sender_is_set_aside_by_a_click() -> anyhow::Result<()> {
    let Some(mut bed) = start()? else {
        return Ok(());
    };
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_S]);
    bed.opened()?;
    // Her proposal, in the thread.
    let shown = bed.shows(|s| items(s).contains("GitHub t’a envoyé"))?;
    assert!(
        items(&shown)
            .contains("GitHub t’a envoyé 12 notifications automatiques. Je les laisse sans tri")
            && items(&shown).contains("[Oui, ne pas les trier | Non | Plus tard]"),
        "{}",
        items(&shown)
    );
    assert!(
        !items(&shown).contains("X-GitHub-Reason"),
        "no header's name"
    );
    bed.shot("queue-proposal")?;

    // Her header's button: the queue, under its own bar.
    assert!(
        shown["to_sort"]
            .as_str()
            .is_some_and(|b| b.ends_with("mails à trier ›")),
        "{shown}"
    );
    bed.click("Status")?;
    let shown = bed.shows(|s| s["queue"] == true && items(s).contains("group:"))?;
    assert_eq!(shown["subview"], "Le tri de tes mails");
    let places = bed.places();
    assert!(
        !places.contains("TabTalk") && !places.contains(" Field@") && places.contains("Back@"),
        "no tabs, no field: {places}"
    );
    let listed = items(&shown);
    assert!(listed.contains("now:Léa Martin"), "{listed}");
    assert!(
        listed.contains("group:GitHub · 12 · Machine · aside"),
        "{listed}"
    );
    assert!(
        listed.contains("group:GitLab · 5 · Machine · aside"),
        "{listed}"
    );
    assert!(
        listed.contains("group:La Lettre du lundi · 3 · Letter · aside"),
        "{listed}"
    );
    // A person's mails: never offered to be left unsorted.
    assert!(
        listed.contains("group:Léa Martin · 2 · Person\""),
        "{listed}"
    );
    assert!(
        listed.contains("group:Paul Durand · 1 · Person\""),
        "{listed}"
    );
    let github = listed.find("group:GitHub").unwrap_or(usize::MAX);
    let gitlab = listed.find("group:GitLab").unwrap_or(0);
    assert!(github < gitlab, "the most first: {listed}");
    for body in [
        "relire le devis",
        "commented",
        "Job deploy",
        "10 h ?",
        "X-GitHub-Reason",
        "qwen",
        "fake/",
    ] {
        assert!(!listed.contains(body), "{body}: {listed}");
    }
    // 39 s: the pace measured.
    bed.advance(39)?;
    bed.shows(|s| items(s).contains("(41 s par mail sur cet ordinateur)."))?;
    bed.advance(2)?;
    bed.shows(|s| items(s).contains("now:GitHub"))?;
    bed.shot("3-queue")?;

    // « Ne pas trier les mails de GitHub », in its card.
    bed.click("Aside(0)")?;
    let shown = bed.shows(|s| items(s).contains("aside:GitHub"))?;
    assert!(!items(&shown).contains("group:GitHub"), "{}", items(&shown));
    bed.shot("queue-set-aside")?;
    // « Trier à nouveau les mails de GitHub »: back.
    bed.click("Unaside(0)")?;
    bed.shows(|s| {
        let i = items(s);
        i.contains("group:GitHub") && !i.contains("aside:")
    })?;

    // Steadily too hot (her sampler reads every five seconds of her clock,
    // and averages the last minute): the pause and why.
    let zone = bed.power.join("thermal_zone0");
    std::fs::create_dir_all(&zone)?;
    std::fs::write(zone.join("type"), "x86_pkg_temp")?;
    std::fs::write(zone.join("temp"), "100000")?;
    bed.advance(45)?;
    bed.shows(|s| items(s).contains("state:Je souffle entre deux"))?;
    bed.advance(3)?;
    // One reason, said once (#758): her status sums it up, the view says
    // why with her real threshold.
    let shown = bed.shows(|s| {
        items(s).contains(
            "pause:L’ordinateur chauffe (100 °C). Je reprends quand il redescend sous 90 °C",
        )
    })?;
    assert_eq!(shown["status"], "Tri en pause : l’ordinateur chauffe");
    assert_eq!(shown["hint"], "", "no band besides");
    bed.shot("4-queue-paused")?;

    // Back to the thread.
    bed.click("Back")?;
    bed.shows(|s| s["queue"] == false)?;
    Ok(())
}
