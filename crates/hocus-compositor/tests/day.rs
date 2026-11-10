//! "Your day"'s data (issue #563, docs/design/day.md): the real shell
//! starts the day's workers from the configuration the compositor read,
//! and says what the day holds in its log ("the day holds …"), which is
//! what these tests read. No network: a fixture's file, and the local
//! list, in folders of the test's own.
//!
//! One test per point of the acceptance: a fixture's day reaches the
//! shell, its times relative to the shell's clock (`HOCUS_CLOCK_AT`,
//! 4 November 2026, 14:00 UTC); the local list survives a restart of the
//! shell; changing the fixture's file shows the change; at rest, nothing
//! wakes, neither the workers nor the shell's surfaces, a change of the
//! day included. And a Google account (#567): Online Accounts of the
//! test's own on the bus the compositor names, a Calendar API of its own
//! on 127.0.0.1 (`HOCUS_TEST_GOOGLE_API`).

use crate::common;
use crate::day_fakes as fakes;

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use hocus_link::{DayAsk, ToShell};

struct Bed {
    // The compositor (its shell) before the bus it is given.
    hp: Headless,
    _dir: tempfile::TempDir,
    log: PathBuf,
    /// The shell's process id, as its script writes it.
    pid: PathBuf,
    /// The fixture's file.
    fixture: PathBuf,
    /// `$XDG_STATE_HOME`.
    state: PathBuf,
}

const FIXTURE: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#8FB3FF"

[[calendars]]
name = "Perso"
shown = false

[[events]]
title = "Point produit"
start = "now+25m"
end = "now+55m"
video = "https://meet.example.org/abc-defg"

[[events]]
title = "Revue de sprint"
start = "today 16:00"
end = "today 17:00"

[[events]]
calendar = "Perso"
title = "Piscine"
start = "today 18:30"

[[events]]
title = "Congés de Lou"
date = "tomorrow"

[tasks]
board = "Sprint"

[[tasks.cards]]
id = "spec"
title = "Relire la spec"

[[tasks.cards]]
id = "ticket"
title = "Écrire le ticket"
"##;

const WITH_FIXTURE: &str = "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\
                            [[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

/// The shell's program: the test fails when it is not built.
fn ready() -> Option<PathBuf> {
    Some(common::shell_program())
}

/// The compositor on a 1920×1080 screen with the real shell, its
/// configuration `config` (its `[day]`) and, beside it, `day.toml`
/// holding `fixture`.
fn start(shell: &Path, config: &str, fixture: &str) -> anyhow::Result<Bed> {
    start_in(tempfile::tempdir()?, shell, config, fixture, None)
}

/// A session bus and the Calendar API's address, for the shell.
struct Google<'a> {
    bus: &'a str,
    api: &'a str,
}

fn start_in(
    dir: tempfile::TempDir,
    shell: &Path,
    config: &str,
    fixture: &str,
    google: Option<Google<'_>>,
) -> anyhow::Result<Bed> {
    use std::os::unix::fs::PermissionsExt;
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    let settings_dir = dir.path().join("config");
    for sub in [&home, &state, &settings_dir] {
        std::fs::create_dir_all(sub)?;
    }
    let file = settings_dir.join("config.toml");
    std::fs::write(&file, config)?;
    let fixture_file = settings_dir.join("day.toml");
    std::fs::write(&fixture_file, fixture)?;
    let log = dir.path().join("shell.log");
    let pid = dir.path().join("shell.pid");
    let script = dir.path().join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_STATE_HOME='{state}' TZ=UTC HOCUS_CLOCK_AT='2026-11-04T14:00:00' \
             RUST_LOG=warn,hocuspanda_shell=debug,hocus_day=trace {api}\necho $$ > '{pid}'\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            api = google
                .as_ref()
                .map(|g| format!("HOCUS_TEST_GOOGLE_API='{}'", g.api))
                .unwrap_or_default(),
            home = home.display(),
            state = state.display(),
            pid = pid.display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let settings = Config {
        file: Some(file),
        ..Config::default()
    };
    let mut hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        settings,
        Some(script),
        google.map(|g| g.bus.to_owned()),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(drawn, "no bar");
    Ok(Bed {
        _dir: dir,
        hp,
        log,
        pid,
        fixture: fixture_file,
        state,
    })
}

impl Bed {
    /// The shell's log, without its colours.
    fn log(&self) -> String {
        let text =
            String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn mark(&self) -> usize {
        self.log().len()
    }

    /// The lines logged after `mark` that hold `what`.
    fn lines_since(&self, mark: usize, what: &str) -> Vec<String> {
        self.log()
            .get(mark..)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains(what))
            .map(str::to_owned)
            .collect()
    }

    /// Waits until the shell says, after `mark`, that the day holds all of
    /// `holds` and none of `not`; that line.
    fn day_since(&mut self, mark: usize, holds: &[&str], not: &[&str]) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < common::WAIT {
            let found = self
                .lines_since(mark, "the day holds")
                .into_iter()
                .rev()
                .find(|l| {
                    holds.iter().all(|h| l.contains(h)) && !not.iter().any(|n| l.contains(n))
                });
            if let Some(line) = found {
                return Ok(line);
            }
            self.hp
                .dispatch_until(Duration::from_millis(50), |_| false)?;
        }
        anyhow::bail!(
            "the day never held {holds:?} without {not:?}:\n{}",
            self.log()
        )
    }

    /// The shell's surfaces' commits, the clock's aside (it draws every
    /// second).
    fn commits(&self) -> [u64; 5] {
        [
            self.hp.bar_commits(),
            self.hp.levels_commits(),
            self.hp.connections_commits(),
            self.hp.desktops_commits(),
            self.hp.agents_commits(),
        ]
    }
}

/// The id a task has in a line of the day: `ID «TITLE»`.
fn task_id<'a>(line: &'a str, title: &str) -> Option<&'a str> {
    let at = line.find(&format!(" «{title}»"))?;
    line[..at].rsplit([' ', '[']).next()
}

#[test]
fn a_fixture_day_reaches_the_shell_its_times_relative_to_its_clock() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE)?;
    let line = bed.day_since(
        0,
        &[
            "fixture-calendars-day fresh",
            "fixture-tasks-day fresh",
            // Today's, by start, the hidden calendar's left out; tomorrow's
            // all-day event is not today's.
            "today=2=[14:25 «Point produit» (Travail); 16:00 «Revue de sprint» (Travail)]",
            // The fixture's board is the bar's tasks, in its column's order.
            "tasks=2=[spec «Relire la spec»; ticket «Écrire le ticket»]",
        ],
        &[],
    )?;
    assert!(
        !line.contains("Piscine") && !line.contains("Congés"),
        "{line}"
    );
    Ok(())
}

#[test]
fn the_local_list_survives_a_restart_of_the_shell() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "", "")?;
    bed.day_since(0, &["local fresh", "tasks=0=[]"], &[])?;
    let mark = bed.mark();
    bed.hp
        .tell_shell(ToShell::Day(DayAsk::Add("Acheter du pain".to_owned())));
    bed.hp
        .tell_shell(ToShell::Day(DayAsk::Add("Appeler Inès".to_owned())));
    // Each added at the top.
    let line = bed.day_since(mark, &["tasks=2=[", "pending=0"], &[])?;
    let at = |t: &str| line.find(t).unwrap_or(usize::MAX);
    assert!(at("«Appeler Inès»") < at("«Acheter du pain»"), "{line}");
    let bread = task_id(&line, "Acheter du pain")
        .ok_or_else(|| anyhow::anyhow!("no id: {line}"))?
        .to_owned();
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Day(DayAsk::Tick(bread.clone())));
    bed.day_since(mark, &["tasks=1=[", "«Appeler Inès»"], &["Acheter du pain"])?;
    assert!(bed.state.join("hocuspanda/tasks.json").exists());
    // The shell is told to end (SIGTERM, by its process id, its script's):
    // the compositor starts it again, which reads the list back.
    let pid = std::fs::read_to_string(&bed.pid)?.trim().to_owned();
    let mark = bed.mark();
    let killed = Command::new("kill").arg(&pid).status()?;
    assert!(killed.success());
    let line = bed.day_since(mark, &["local fresh", "tasks=1=[", "«Appeler Inès»"], &[])?;
    assert!(!line.contains("Acheter du pain"), "{line}");
    let again = std::fs::read_to_string(&bed.pid)?.trim().to_owned();
    assert_ne!(again, pid);
    // The new shell's day may be read before it is linked to the
    // compositor, which drops what it tells no shell: wait for the link.
    let linked = bed
        .hp
        .dispatch_until(common::WAIT, |hp| hp.shell_connected())?;
    anyhow::ensure!(linked, "the shell started again never linked");
    // Ticked before the restart, it is in the done column still: unticked,
    // it comes back (at the top: where it was went with the shell).
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Day(DayAsk::Untick(bread)));
    bed.day_since(mark, &["tasks=2=[", "«Acheter du pain»; "], &[])?;
    Ok(())
}

#[test]
fn changing_the_fixtures_file_shows_the_change() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE)?;
    bed.day_since(0, &["«Point produit»", "«Relire la spec»"], &[])?;
    let mark = bed.mark();
    // Written anew, as an editor saves it: beside it, then over it.
    let changed = FIXTURE
        .replace("Point produit", "Point d'équipe")
        .replace("now+25m", "now+5m")
        .replace("Relire la spec", "Relire la maquette");
    let new = bed.fixture.with_extension("new");
    std::fs::write(&new, changed)?;
    std::fs::rename(&new, &bed.fixture)?;
    bed.day_since(
        mark,
        &[
            "14:05 «Point d'équipe» (Travail)",
            "spec «Relire la maquette»",
        ],
        &["Point produit", "Relire la spec"],
    )?;
    Ok(())
}

#[test]
fn at_rest_nothing_wakes_and_a_change_of_the_day_draws_nothing() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE)?;
    bed.day_since(
        0,
        &[
            "fixture-calendars-day fresh",
            "fixture-tasks-day fresh",
            "local fresh",
        ],
        &[],
    )?;
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let mark = bed.mark();
    let commits = bed.commits();
    common::wait(&mut bed.hp, Duration::from_secs(3))?;
    // The workers sleep: none woke, none read, the day did not change.
    assert!(
        bed.lines_since(mark, "the day's worker wakes").is_empty(),
        "{}",
        bed.log()
    );
    assert!(bed.lines_since(mark, "the day").is_empty(), "{}", bed.log());
    assert_eq!(bed.commits(), commits, "the shell drew at rest");
    // The day changes: said, and nothing drawn for it on these surfaces
    // (the next meeting's card and Today have their own, Day.2 and Day.3,
    // measured in their tests).
    std::fs::write(&bed.fixture, FIXTURE.replace("Revue de sprint", "Démo"))?;
    bed.day_since(mark, &["16:00 «Démo»"], &[])?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.commits(), commits, "the shell drew for the day");
    // Woken once for it, then asleep again.
    let woke = bed.lines_since(mark, "the day's worker wakes").len();
    assert!(woke >= 1, "{}", bed.log());
    let mark = bed.mark();
    common::wait(&mut bed.hp, Duration::from_secs(2))?;
    assert!(bed.lines_since(mark, "the day").is_empty(), "{}", bed.log());
    Ok(())
}

#[test]
fn a_google_account_of_online_accounts_reaches_the_shell() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let dir = tempfile::tempdir()?;
    // Dropped after the compositor (declared before it).
    let bus = fakes::PrivateBus::start(dir.path())?;
    let goa = fakes::FakeGoa::start(&bus.address)?;
    goa.add("account_1", "google", "alex@example.org")?;
    let api = fakes::FakeApi::start()?;
    api.with(|api| {
        let mut work = fakes::calendar(
            "alex@example.org",
            "alex@example.org",
            "#6CC287",
            "owner",
            true,
        );
        work["summaryOverride"] = serde_json::json!("Travail");
        api.calendars = vec![work];
        api.events.insert(
            "alex@example.org".to_owned(),
            (
                vec![
                    fakes::event("e1", "Point produit", "14:25", "14:55"),
                    fakes::event("e2", "Revue de sprint", "16:00", "17:00"),
                ],
                "s1".to_owned(),
            ),
        );
    });
    let mut bed = start_in(
        dir,
        &shell,
        "[[day.calendars]]\nkind = \"google\"\n",
        "",
        Some(Google {
            bus: &bus.address,
            api: &api.address,
        }),
    )?;
    bed.day_since(
        0,
        &[
            "google-calendars-alex@example.org fresh",
            "today=2=[14:25 «Point produit» (Travail); 16:00 «Revue de sprint» (Travail)]",
        ],
        &[],
    )?;
    // Asked with Online Accounts' token, never another address.
    let asked = api.asked();
    assert!(
        !asked.is_empty() && asked.iter().all(|a| a.token == "tok-1"),
        "{asked:?}"
    );
    Ok(())
}
