//! The calendar popover (issue #429, Day.5, variant A): the real shell on
//! the CPU, headless, its day a fixture's file (docs/design/day.md) read
//! against its clock (`HOCUS_CLOCK_AT`, Wednesday 4 November 2026, 09:47,
//! in UTC), in French. The shell says what the calendar shows in its log
//! ("the calendar shows it", "the calendar's targets", "the clock's group
//! is drawn"), which is what these tests read, with the popover's surface,
//! the screen's pixels and the addresses its `xdg-open` (the test's own)
//! is given.
//!
//! One test per point of the spec that can be measured: opening by a
//! click on the date or the clock, a second click closing it; in the mini
//! form by a click, never by resting on it; today chosen, its all-day
//! events first, then by start, those over greyed; another day, one of
//! the months around; the arrows and "Aujourd'hui"; a busy day's list
//! scrolling; an event's call, its page, its details unfolding; `Esc` and
//! a click outside; the keyboard; the entry point the next meeting's card
//! opens it by; no calendar; a day outside the window; at rest, nothing
//! drawn.

mod common;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{BTN_LEFT, KEY_DOWN, KEY_ESC, KEY_RIGHT, KEY_TAB};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::{DayAsk, ToShell};

const KEY_ENTER: u32 = 28;
const KEY_HOME: u32 = 102;
const KEY_END: u32 = 107;
const KEY_PAGEDOWN: u32 = 109;

/// The calendars: one hidden; and the day, from Monday 26 October to
/// Thursday 3 December.
const FIXTURE: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#8FB3FF"

[[calendars]]
id = "perso"
name = "Perso"
color = "#6CC287"

[[calendars]]
id = "famille"
name = "Famille"
color = "#F5A35B"

[[calendars]]
id = "equipe"
name = "Équipe produit"
color = "#C89BFF"

[[calendars]]
id = "hidden"
name = "Caché"
shown = false

[[events]]
calendar = "perso"
title = "Télétravail"
date = "today"

[[events]]
id = "daily"
title = "Point d'équipe quotidien"
start = "today 09:30"
end = "today 09:45"
video = "https://meet.google.com/abc-mnop-xyz"

[[events]]
id = "sprint"
title = "Revue de sprint"
start = "today 10:00"
end = "today 11:00"
video = "https://meet.google.com/xvr-pqat-kmd"

[[events]]
id = "lunch"
calendar = "equipe"
title = "Déjeuner d'équipe"
start = "today 12:30"
end = "today 13:30"
place = "Brasserie du Marché"

[[events]]
id = "atelier"
title = "Atelier conception du tableau de bord"
start = "today 14:00"
end = "today 15:30"
place = "Salle Jupiter · 3e étage"
description = "Revoir les maquettes du tableau de bord avant la démo."

[[events]]
id = "dentist"
calendar = "perso"
title = "Rendez-vous chez le dentiste"
start = "today 17:15"
end = "today 17:45"
place = "12 rue des Lilas"
page = "https://calendar.example.org/event/dentist"

[[events]]
calendar = "hidden"
title = "Secret"
start = "today 11:00"

[[events]]
title = "Annulée"
start = "today 16:00"
status = "cancelled"

[[events]]
title = "Démo pour le client"
start = "today-1 11:00"
video = "https://meet.google.com/qwe-rtyu-iop"

[[events]]
calendar = "equipe"
title = "Point produit"
start = "today+1 11:00"
end = "today+1 11:30"

[[events]]
calendar = "famille"
title = "Week-end à la montagne"
date = "today+3"
days = 2

[[events]]
title = "Mise en production 2.4"
date = "today+6"

[[events]]
calendar = "equipe"
title = "Petit-déjeuner d'accueil"
start = "today+6 08:30"
end = "today+6 09:15"
place = "Cafétéria · 2e étage"

[[events]]
title = "Point d'équipe quotidien"
start = "today+6 09:30"
end = "today+6 09:45"
video = "https://meet.google.com/abc-mnop-xyz"

[[events]]
title = "Revue d'architecture"
start = "today+6 10:00"
end = "today+6 11:15"
place = "Salle Jupiter · 3e étage"

[[events]]
title = "Entretien de recrutement"
start = "today+6 11:30"
end = "today+6 12:15"
video = "https://visio.example.fr/entretien"

[[events]]
calendar = "equipe"
title = "Déjeuner avec le service client"
start = "today+6 12:30"
end = "today+6 13:30"
place = "Cantine de l'immeuble"

[[events]]
title = "Rétrospective"
start = "today+6 14:00"
end = "today+6 15:00"
video = "https://meet.google.com/rty-uiop-qsd"

[[events]]
title = "Point sécurité"
start = "today+6 15:30"
end = "today+6 16:00"
place = "Salle Saturne"

[[events]]
calendar = "equipe"
title = "Démo interne de la 2.4"
start = "today+6 16:30"
end = "today+6 17:30"
place = "Auditorium"

[[events]]
id = "yoga"
calendar = "perso"
title = "Cours de yoga"
start = "today+6 18:30"
end = "today+6 19:30"
place = "Studio du parc"

[[events]]
title = "Formation accessibilité"
start = "today+8 10:00"
end = "today+8 12:00"
video = "https://visio.example.fr/formation-a11y"

[[events]]
calendar = "famille"
title = "Réunion parents-professeurs"
start = "today+8 17:30"
end = "today+8 18:30"
place = "École des Tilleuls"

[[events]]
calendar = "famille"
title = "Spectacle de l'école"
start = "today+29 18:00"
end = "today+29 19:30"
place = "École des Tilleuls"
"##;

const WITH_FIXTURE: &str = "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

/// The screen.
const SCREEN: (i32, i32) = (1920, 1080);

/// The full form's calendar: 320 wide, 8 px left of the 196 px bar, its
/// top on the clock's group's, 22 px from the screen's top.
const POPOVER: (i32, i32) = (1920 - 8 - 196 - 8 - 320, 22);
/// The date under the clock, and the clock.
const DATE: (f64, f64) = (1814.0, 82.0);
const CLOCK: (f64, f64) = (1790.0, 44.0);
/// The mini form's clock, and its calendar.
const MINI_CLOCK: (f64, f64) = (1884.0, 40.0);
const MINI_POPOVER: (i32, i32) = (1920 - 8 - 56 - 8 - 320, 20);

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    fixture: PathBuf,
    /// What the shell's `xdg-open` was given, a line each.
    launched: PathBuf,
}

/// The shell's program, or `None` (the test skips) when it is not built.
fn ready() -> Option<PathBuf> {
    let shell = common::shell_program();
    if shell.is_none() {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
    }
    shell
}

/// The compositor with the real shell on the CPU, its day `fixture` (with
/// the configuration `config`), its bar in `form`.
fn start(shell: &Path, config: &str, fixture: &str, form: FormChoice) -> anyhow::Result<Bed> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "state", "config", "bin", "launched"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let file = root.join("config/config.toml");
    std::fs::write(&file, config)?;
    let fixture_file = root.join("config/day.toml");
    std::fs::write(&fixture_file, fixture)?;
    let launched = root.join("launched/xdg-open");
    let opener = root.join("bin/xdg-open");
    std::fs::write(
        &opener,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n",
            launched.display()
        ),
    )?;
    std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))?;
    let log = root.join("shell.log");
    let script = root.join("shell");
    let home = root.join("home");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_STATE_HOME='{state}' TZ=UTC HOCUS_CLOCK_AT='2026-11-04T09:47:00' \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             PATH='{bin}:/usr/bin:/bin' RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            state = root.join("state").display(),
            bin = root.join("bin").display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut settings = Config {
        file: Some(file),
        plain_background: true,
        french: true,
        ..Config::default()
    };
    settings.keyboard.layouts = vec!["us".to_owned()];
    settings.panel.form = form;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![format!("{}x{}", SCREEN.0, SCREEN.1).parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        settings,
        Some(script),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(drawn, "no bar");
    hp.pointer_move(400.0, 600.0);
    let mut bed = Bed {
        _dir: dir,
        hp,
        log,
        fixture: fixture_file,
        launched,
    };
    // The day read (or none to read) before a test opens the calendar.
    if config.is_empty() {
        bed.logged_since(0, "the day holds", &["local fresh"])?;
    } else {
        bed.logged_since(0, "the day holds", &["fixture-calendars-day fresh"])?;
    }
    Ok(bed)
}

/// The calendar's line `field=` value: up to the next ` field=`.
fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let at = line.find(&format!(" {name}="))? + name.len() + 2;
    let rest = &line[at..];
    let end = rest
        .match_indices(' ')
        .find(|(i, _)| {
            rest[i + 1..].split_once('=').is_some_and(|(k, _)| {
                !k.is_empty() && k.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
        })
        .map_or(rest.len(), |(i, _)| i);
    Some(&rest[..end])
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

    /// Waits until a line logged after `mark` holds `what` and all of
    /// `holds`; that line.
    fn logged_since(&mut self, mark: usize, what: &str, holds: &[&str]) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < common::WAIT * 2 {
            let found = self
                .lines_since(mark, what)
                .into_iter()
                .rev()
                .find(|l| holds.iter().all(|h| l.contains(h)));
            if let Some(line) = found {
                return Ok(line);
            }
            self.hp
                .dispatch_until(Duration::from_millis(50), |_| false)?;
        }
        anyhow::bail!("never logged {what} with {holds:?}:\n{}", self.log())
    }

    /// Waits until the calendar shows all of `holds` (after `mark`).
    fn shows(&mut self, mark: usize, holds: &[&str]) -> anyhow::Result<String> {
        self.logged_since(mark, "the calendar shows it", holds)
    }

    /// The calendar's popover, once drawn.
    fn popover(&mut self) -> anyhow::Result<Rect> {
        let shown = self.hp.dispatch_until(common::WAIT, |hp| {
            common::drawn_popovers(hp).iter().any(|p| p.width == 320)
        })?;
        anyhow::ensure!(
            shown,
            "no calendar drawn: {:?}\n{}",
            self.hp.popover_surfaces(),
            self.log()
                .lines()
                .filter(|l| !l.contains("applications are"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        Ok(common::drawn_popovers(&self.hp)
            .into_iter()
            .find(|p| p.width == 320)
            .unwrap_or(Rect::new(0, 0, 0, 0)))
    }

    /// Waits until no popover shows.
    fn closed(&mut self) -> anyhow::Result<()> {
        let gone = self
            .hp
            .dispatch_until(common::WAIT, |hp| hp.popover_surfaces().is_empty())?;
        anyhow::ensure!(gone, "still open: {:?}", self.hp.popover_surfaces());
        Ok(())
    }

    /// The pointer on the clock's group at `at`, once the shell draws the
    /// group lit under it: a bar just drawn on a loaded machine may not
    /// hear the pointer at once (the clock's surface lets it through to
    /// the bar's).
    fn hover_clock(&mut self, at: (f64, f64)) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let mark = self.mark();
            common::point(&mut self.hp, (at.0, 500.0))?;
            common::wait(&mut self.hp, Duration::from_millis(50))?;
            common::point(&mut self.hp, at)?;
            common::wait(&mut self.hp, Duration::from_millis(100))?;
            if self
                .lines_since(mark, "the clock's group is drawn")
                .iter()
                .any(|l| l.contains("hover: true"))
            {
                return Ok(());
            }
            anyhow::ensure!(
                start.elapsed() < common::WAIT,
                "the clock's group never lit under the pointer:\n{}",
                self.log()
            );
        }
    }

    /// A click on the clock's group at `at`.
    fn click_clock(&mut self, at: (f64, f64)) -> anyhow::Result<()> {
        self.hover_clock(at)?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    /// A click at `at` (on the popover, from its top left).
    fn click_in(&mut self, popover: Rect, (x, y): (f32, f32)) -> anyhow::Result<()> {
        let at = (
            f64::from(popover.x) + f64::from(x),
            f64::from(popover.y) + f64::from(y),
        );
        common::click(&mut self.hp, at)
    }

    /// Where the last targets logged after `mark` put `target`, on the
    /// popover.
    fn target(&mut self, mark: usize, target: &str) -> anyhow::Result<(f32, f32)> {
        let line = self.logged_since(mark, "the calendar's targets", &[&format!("{target}@")])?;
        let at = line
            .split(&format!("{target}@"))
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|xy| xy.split_once(','))
            .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))
            .ok_or_else(|| anyhow::anyhow!("no {target} in {line}"))?;
        Ok(at)
    }

    fn press(&mut self, key: u32) -> anyhow::Result<()> {
        self.hp.key(key, true);
        self.hp.key(key, false);
        common::wait(&mut self.hp, Duration::from_millis(30))
    }

    /// The pixel at `(x, y)` of the screen, RGB, once at rest.
    fn pixel(&mut self, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
        common::at_rest(&mut self.hp)?;
        let (w, _, px) = self.hp.screenshot(0)?;
        let at = ((y * w + x) * 4) as usize;
        Ok([px[at + 2], px[at + 1], px[at]])
    }

    /// The addresses the shell opened, once `n` are there.
    fn opened(&mut self, n: usize) -> anyhow::Result<Vec<String>> {
        let file = self.launched.clone();
        let read = move || -> Vec<String> {
            std::fs::read_to_string(&file)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        };
        let done = self
            .hp
            .dispatch_until(common::WAIT, |_| read().len() >= n)?;
        anyhow::ensure!(done, "opened {:?}, not {n}:\n{}", read(), self.log());
        Ok(read())
    }

    /// The calendar opened by a click on the date: its popover, and what
    /// it showed then.
    fn open(&mut self) -> anyhow::Result<(Rect, String)> {
        let mark = self.mark();
        self.click_clock(DATE)?;
        let popover = self.popover()?;
        let shown = self.shows(mark, &[])?;
        Ok((popover, shown))
    }
}

/// The middle of day `col` (Monday 0) of week `row` (from 0) on the
/// popover: a 28 px column, then seven equal ones across its 286 px; six
/// rows of 32 px, 2 px apart, under the header (14 px in, 28 px, 10 px)
/// and the letters' 18 px row.
fn day_at(row: u32, col: u32) -> (f32, f32) {
    let column = (286.0 - 28.0) / 7.0;
    (
        17.0 + 28.0 + (col as f32 + 0.5) * column,
        1.0 + 14.0 + 28.0 + 10.0 + 18.0 + 2.0 + 16.0 + row as f32 * 34.0,
    )
}

#[test]
fn a_click_on_the_date_or_the_clock_opens_it_on_today_and_a_second_click_closes_it()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    // The group lit under the pointer.
    let mark = bed.mark();
    bed.hover_clock(DATE)?;
    let line = bed.logged_since(
        mark,
        "the clock's group is drawn",
        &["hover: true, open: false"],
    )?;
    // 172 wide, from the clock's cells (22 px down) to 6 px under the date.
    assert!(line.contains("at=(-184.0, 14.0, 172.0, 77.2"), "{line}");
    // A click on the date: the calendar, beside the bar, its top on the
    // group's; the group lit while it is open.
    let (popover, shown) = bed.open()?;
    assert_eq!(
        (popover.x, popover.y, popover.width),
        (POPOVER.0, POPOVER.1, 320)
    );
    assert!(popover.height <= 580, "{popover:?}");
    assert!(
        shown
            .contains("month=2026-11 title=«Novembre 2026» today_button=faint selected=2026-11-04"),
        "{shown}"
    );
    bed.logged_since(mark, "the clock's group is drawn", &["open: true"])?;
    // Lit 0.08 over the bar's colour: lighter than the bar beside it.
    // (The pointer away: its cursor shows in the screenshot.)
    bed.hp.pointer_move(400.0, 600.0);
    let lit = bed.pixel(1814, 96)?;
    let bar = bed.pixel(1814, 103)?;
    assert!(
        lit[0] > bar[0] + 10 && lit[2] > bar[2] + 10,
        "{lit:?} over {bar:?}"
    );
    // A second click, on the clock: closed.
    bed.click_clock(CLOCK)?;
    bed.closed()?;
    // On the clock: open again, on today.
    let mark = bed.mark();
    bed.click_clock(CLOCK)?;
    bed.popover()?;
    bed.shows(mark, &["selected=2026-11-04"])?;
    Ok(())
}

#[test]
fn in_the_mini_form_a_click_on_the_clock_opens_it_and_resting_on_it_does_not() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Mini)?;
    let mark = bed.mark();
    bed.hover_clock(MINI_CLOCK)?;
    let line = bed.logged_since(mark, "the clock's group is drawn", &["hover: true"])?;
    assert!(line.contains("form=\"mini\""), "{line}");
    // Resting 600 ms: nothing opens (question 3).
    common::wait(&mut bed.hp, Duration::from_millis(600))?;
    assert!(bed.hp.popover_surfaces().is_empty());
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    let popover = bed.popover()?;
    assert_eq!(
        (popover.x, popover.y, popover.width),
        (MINI_POPOVER.0, MINI_POPOVER.1, 320)
    );
    bed.shows(mark, &["selected=2026-11-04"])?;
    // It stays when the pointer leaves, as a click opened it.
    bed.hp.pointer_move(400.0, 600.0);
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(common::drawn_popovers(&bed.hp).len(), 1);
    Ok(())
}

#[test]
fn today_is_chosen_its_all_day_events_first_then_by_start_those_over_greyed() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    let (popover, shown) = bed.open()?;
    // The hidden calendar's and the cancelled event left out.
    assert!(
        shown.contains(
            "day=«Mercredi 4 novembre» rel=«aujourd'hui» count=«6 événements» weeks=44,45,46,47,48,49"
        ),
        "{shown}"
    );
    let rows = shown.split(" rows=").nth(1).unwrap_or_default();
    let order = [
        "all-day «Télétravail» toute la journée",
        "09:30–09:45 «Point d'équipe quotidien» video=meet.google.com/abc-mnop-xyz past",
        "10:00–11:00 «Revue de sprint» video=meet.google.com/xvr-pqat-kmd h=",
        "12:30–13:30 «Déjeuner d'équipe» place=Brasserie du Marché h=",
        "14:00–15:30 «Atelier conception du tableau…» place=Salle Jupiter · 3e étage",
        "17:15–17:45 «Rendez-vous chez le dentiste» place=12 rue des Lilas",
    ];
    let mut at = 0;
    for row in order {
        let found = rows[at..].find(row).map(|i| at + i);
        assert!(found.is_some(), "{row} after {at} in {rows}");
        at = found.unwrap_or(at) + row.len();
    }
    assert!(
        !rows.contains("Secret") && !rows.contains("Annulée"),
        "{rows}"
    );
    // A dot under each day with events, those around too; none on the
    // hidden calendar's or the cancelled event's alone.
    let busy = field(&shown, "busy").unwrap_or_default();
    assert_eq!(
        busy,
        "[2026-11-03,2026-11-04,2026-11-05,2026-11-07,2026-11-08,2026-11-10,2026-11-12,2026-12-03]"
    );
    // Today's ring in green (`#6CC287`), inside its button (week 1,
    // Wednesday), filled green as it is chosen.
    let (x, y) = day_at(1, 2);
    let top = (
        popover.x + x.round() as i32,
        popover.y + (y - 15.0).round() as i32,
    );
    let ring = bed.pixel(top.0, top.1)?;
    assert!(ring[1] > ring[0] + 50 && ring[1] > ring[2] + 30, "{ring:?}");
    // A dot of another day's (Tuesday 10), 4 px under its number.
    let (x, y) = day_at(2, 1);
    let dot = bed.pixel(
        popover.x + x.floor() as i32,
        popover.y + (y + 8.5).floor() as i32,
    )?;
    assert!(dot.iter().all(|c| *c > 120), "{dot:?}");
    Ok(())
}

#[test]
fn a_click_on_another_day_shows_its_agenda_and_a_day_around_turns_the_month() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    let (popover, _) = bed.open()?;
    // Thursday 12: its two events, and the popover to its new height.
    let mark = bed.mark();
    bed.click_in(popover, day_at(2, 3))?;
    let shown = bed.shows(mark, &["selected=2026-11-12"])?;
    assert!(
        shown.contains("today_button=text")
            && shown.contains("day=«Jeudi 12 novembre» count=«2 événements»"),
        "{shown}"
    );
    assert!(!shown.contains("rel="), "{shown}");
    // Tomorrow says so.
    let mark = bed.mark();
    bed.click_in(popover, day_at(1, 3))?;
    bed.shows(
        mark,
        &["selected=2026-11-05", "rel=«demain» count=«1 événement»"],
    )?;
    // An empty day.
    let mark = bed.mark();
    bed.click_in(popover, day_at(2, 4))?;
    bed.shows(
        mark,
        &["selected=2026-11-13", "note=«Rien de prévu ce jour-là.»"],
    )?;
    // Thursday 3 December, of the month around: its month shows.
    let mark = bed.mark();
    bed.click_in(popover, day_at(5, 3))?;
    let shown = bed.shows(mark, &["selected=2026-12-03"])?;
    assert!(
        shown.contains("month=2026-12 title=«Décembre 2026»")
            && shown.contains("«Spectacle de l'école»"),
        "{shown}"
    );
    Ok(())
}

#[test]
fn the_arrows_turn_the_month_and_aujourdhui_brings_today_back() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    let mark = bed.mark();
    let (popover, _) = bed.open()?;
    // The arrows, 26 × 26, 6 px apart, against the header's right.
    let next = bed.target(mark, "Next")?;
    let previous = bed.target(mark, "Previous")?;
    assert_eq!((next, previous), ((290.0, 29.0), (258.0, 29.0)));
    let today = bed.target(mark, "Today")?;
    // The month turns, the day chosen stays.
    let mark = bed.mark();
    bed.click_in(popover, next)?;
    let shown = bed.shows(mark, &["month=2026-12", "selected=2026-11-04"])?;
    assert!(shown.contains("today_button=text"), "{shown}");
    assert!(shown.contains("weeks=49,50,51,52,53,1"), "{shown}");
    let mark = bed.mark();
    bed.click_in(popover, previous)?;
    bed.click_in(popover, previous)?;
    bed.shows(
        mark,
        &["month=2026-10 title=«Octobre 2026»", "selected=2026-11-04"],
    )?;
    // "Aujourd'hui": today's month and today.
    let mark = bed.mark();
    bed.click_in(popover, today)?;
    bed.shows(
        mark,
        &["month=2026-11", "today_button=faint", "selected=2026-11-04"],
    )?;
    Ok(())
}

#[test]
fn a_busy_days_list_scrolls_past_236_px_and_the_wheel_scrolls_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    let (popover, _) = bed.open()?;
    let mark = bed.mark();
    bed.click_in(popover, day_at(2, 1))?;
    let shown = bed.shows(mark, &["selected=2026-11-10", "count=«10 événements»"])?;
    let list = field(&shown, "list").unwrap_or_default();
    assert!(list.starts_with("236/"), "{shown}");
    assert!(shown.contains("scroll=0 "), "{shown}");
    // Its height at most about 560.
    let popover = bed.popover()?;
    assert!((540..=580).contains(&popover.height), "{popover:?}");
    // The wheel over the list.
    let mark = bed.mark();
    let over = (
        f64::from(popover.x + 150),
        f64::from(popover.y + popover.height - 60),
    );
    common::point(&mut bed.hp, over)?;
    bed.hp.wheel(2);
    let shown = bed.shows(mark, &["selected=2026-11-10"])?;
    assert!(!shown.contains("scroll=0 "), "{shown}");
    Ok(())
}

#[test]
fn an_events_call_or_page_opens_and_closes_it_and_one_without_unfolds() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    // The sprint review's video call: opened, the calendar closed.
    let mark = bed.mark();
    let (popover, _) = bed.open()?;
    let sprint = bed.target(mark, "Event(2)")?;
    bed.click_in(popover, sprint)?;
    assert_eq!(bed.opened(1)?, ["https://meet.google.com/xvr-pqat-kmd"]);
    bed.closed()?;
    // The dentist's: no call, its page.
    let (popover, _) = bed.open()?;
    // Under the list's 236 px: scrolled to.
    let mark = bed.mark();
    common::point(
        &mut bed.hp,
        (f64::from(popover.x + 150), f64::from(popover.y + 500)),
    )?;
    bed.hp.wheel(5);
    let dentist = bed.target(mark, "Event(5)")?;
    bed.click_in(popover, dentist)?;
    assert_eq!(
        bed.opened(2)?[1],
        "https://calendar.example.org/event/dentist"
    );
    bed.closed()?;
    // The workshop's: neither, its details unfold under its row, and fold
    // again.
    let opening = bed.mark();
    let (popover, _) = bed.open()?;
    let workshop = bed.target(opening, "Event(4)")?;
    let mark = bed.mark();
    bed.click_in(popover, workshop)?;
    let shown = bed.shows(mark, &["unfolded=["])?;
    assert!(
        shown.contains("unfolded=[14:00 – 15:30 | Travail | Revoir les maquettes"),
        "{shown}"
    );
    assert_eq!(common::drawn_popovers(&bed.hp).len(), 1);
    let workshop = bed.target(mark, "Event(4)")?;
    let mark = bed.mark();
    bed.click_in(popover, workshop)?;
    let shown = bed.shows(mark, &["selected=2026-11-04"])?;
    assert!(!shown.contains("unfolded="), "{shown}");
    assert_eq!(bed.opened(2)?.len(), 2);
    Ok(())
}

#[test]
fn esc_and_a_click_outside_close_it_and_the_keyboard_goes_back_to_the_clock() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    bed.open()?;
    let mark = bed.mark();
    bed.press(KEY_ESC)?;
    bed.closed()?;
    // The keyboard on the clock's group, ringed; `Enter` opens it again.
    bed.logged_since(mark, "the clock's group is drawn", &["focused: true"])?;
    let mark = bed.mark();
    bed.press(KEY_ENTER)?;
    bed.popover()?;
    bed.shows(mark, &["selected=2026-11-04"])?;
    // A click outside, on the desktop.
    common::click(&mut bed.hp, (400.0, 600.0))?;
    bed.closed()?;
    Ok(())
}

#[test]
fn the_keyboard_moves_a_day_a_week_a_month_and_enter_chooses_the_day() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    let (_, shown) = bed.open()?;
    // On today as it opens, not outlined until a key moves it.
    assert!(
        shown.contains("focus=Some(Days) focus_day=2026-11-04 keyed=false"),
        "{shown}"
    );
    let mark = bed.mark();
    bed.press(KEY_RIGHT)?;
    bed.shows(
        mark,
        &["focus_day=2026-11-05 keyed=true", "selected=2026-11-04"],
    )?;
    bed.logged_since(
        mark,
        "the calendar's keyboard is on",
        &["jeudi 5 novembre, 1 événement"],
    )?;
    let mark = bed.mark();
    bed.press(KEY_DOWN)?;
    bed.shows(mark, &["focus_day=2026-11-12"])?;
    // A month on, past the month shown: it turns.
    let mark = bed.mark();
    bed.press(KEY_PAGEDOWN)?;
    bed.shows(mark, &["month=2026-12", "focus_day=2026-12-12"])?;
    let mark = bed.mark();
    bed.press(KEY_HOME)?;
    bed.shows(mark, &["focus_day=2026-12-07"])?;
    let mark = bed.mark();
    bed.press(KEY_END)?;
    bed.shows(mark, &["focus_day=2026-12-13"])?;
    // Enter chooses it: its agenda shows.
    let mark = bed.mark();
    bed.press(KEY_ENTER)?;
    bed.shows(
        mark,
        &["selected=2026-12-13", "note=«Rien de prévu ce jour-là.»"],
    )?;
    // Tab: from the days round to the arrows, then "Aujourd'hui".
    let mark = bed.mark();
    bed.press(KEY_TAB)?;
    bed.shows(mark, &["focus=Some(Previous)"])?;
    let mark = bed.mark();
    bed.press(KEY_TAB)?;
    bed.press(KEY_TAB)?;
    bed.shows(mark, &["focus=Some(Today)"])?;
    let mark = bed.mark();
    bed.press(KEY_ENTER)?;
    bed.shows(mark, &["selected=2026-11-04", "month=2026-11"])?;
    // Then the days, then each event, where Enter acts as a click.
    let mark = bed.mark();
    bed.press(KEY_TAB)?;
    bed.press(KEY_TAB)?;
    bed.press(KEY_TAB)?;
    bed.shows(mark, &["focus=Some(Event(EventId(\"daily\")))"])?;
    bed.press(KEY_ENTER)?;
    assert_eq!(bed.opened(1)?, ["https://meet.google.com/abc-mnop-xyz"]);
    bed.closed()?;
    Ok(())
}

#[test]
fn the_entry_point_opens_it_on_a_day_with_its_event_chosen() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    // Day.2's card's way: a day, an event of it; the last of a busy day,
    // scrolled to.
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Day(DayAsk::Calendar {
        day: "2026-11-10".to_owned(),
        event: Some("yoga".to_owned()),
    }));
    let popover = bed.popover()?;
    assert_eq!((popover.x, popover.y), POPOVER);
    let shown = bed.shows(
        mark,
        &[
            "selected=2026-11-10",
            "focus=Some(Event(EventId(\"yoga\")))",
        ],
    )?;
    assert!(!shown.contains("scroll=0 "), "{shown}");
    // Open already: it turns to the next one asked, an event without a
    // link unfolded.
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Day(DayAsk::Calendar {
        day: "2026-11-04".to_owned(),
        event: Some("atelier".to_owned()),
    }));
    bed.shows(
        mark,
        &[
            "selected=2026-11-04",
            "focus=Some(Event(EventId(\"atelier\")))",
            "unfolded=[14:00 – 15:30",
        ],
    )?;
    assert_eq!(common::drawn_popovers(&bed.hp).len(), 1);
    Ok(())
}

#[test]
fn a_day_outside_the_window_says_it_is_not_loaded_and_its_month_has_no_dots() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Day(DayAsk::Calendar {
        day: "2028-01-05".to_owned(),
        event: None,
    }));
    bed.popover()?;
    bed.shows(
        mark,
        &[
            "month=2028-01",
            "selected=2028-01-05",
            "busy=[]",
            "note=«Ce jour n'est pas chargé.»",
        ],
    )?;
    Ok(())
}

#[test]
fn with_no_calendar_the_agenda_says_none_is_connected_and_offers_day_1s_popover()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "", "", FormChoice::Full)?;
    let mark = bed.mark();
    let (popover, shown) = bed.open()?;
    assert!(
        shown.contains("month=2026-11") && shown.contains("note=«Aucun agenda branché.»"),
        "{shown}"
    );
    // "Choisir des agendas…" under it: Day.1's popover, at its first step,
    // in the calendar's place.
    assert!(shown.contains("choose=«Choisir des agendas…»"), "{shown}");
    let targets = bed.logged_since(mark, "the calendar's targets", &["Choose@"])?;
    let at = targets
        .split(" Choose@")
        .nth(1)
        .and_then(|a| a.split_whitespace().next())
        .and_then(|a| a.split_once(','))
        .and_then(|(x, y)| Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)))
        .ok_or_else(|| anyhow::anyhow!("no Choose in {targets}"))?;
    let mark = bed.mark();
    bed.click_in(popover, at)?;
    bed.logged_since(mark, "the sources' popover shows it", &["step=accounts"])?;
    let replaced = bed.hp.dispatch_until(common::WAIT, |hp| {
        common::drawn_popovers(hp)
            .first()
            .is_some_and(|r| r.x == popover.x && r.y == popover.y && r.height != popover.height)
    })?;
    assert!(
        replaced,
        "{:?} {popover:?}",
        common::drawn_popovers(&bed.hp)
    );
    Ok(())
}

#[test]
fn at_rest_it_draws_nothing_and_a_change_of_the_day_draws_it_once_open() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    // Closed: a change of the day draws no popover.
    std::fs::write(&bed.fixture, FIXTURE.replace("Revue de sprint", "Revue"))?;
    bed.logged_since(0, "the day holds", &["«Revue»"])?;
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.popover_commits(), 0);
    // Opened: drawn, then nothing at rest (today's next event ends at
    // 11:00).
    bed.open()?;
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let commits = bed.hp.popover_commits();
    let mark = bed.mark();
    common::wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(
        bed.hp.popover_commits(),
        commits,
        "the calendar drew at rest"
    );
    assert!(bed.lines_since(mark, "the calendar shows it").is_empty());
    // The day changes: drawn again, with it.
    let new = bed.fixture.with_extension("new");
    std::fs::write(&new, FIXTURE.replace("Revue de sprint", "Démo de sprint"))?;
    std::fs::rename(&new, &bed.fixture)?;
    bed.shows(mark, &["«Démo de sprint»"])?;
    let drawn = bed
        .hp
        .dispatch_until(common::WAIT, |hp| hp.popover_commits() > commits)?;
    assert!(drawn);
    common::wait(&mut bed.hp, Duration::from_secs(1))?;
    let more = bed.hp.popover_commits() - commits;
    assert!((1..=3).contains(&more), "{more} commits for one change");
    Ok(())
}

/// The pull request's screenshots, the shell on the CPU, its clock at
/// 09:47: today, a busy day, an event unfolded, another month, the mini
/// form. Written to `$HOCUS_SHOTS` when it names a folder. Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    let Some(shell) = ready() else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let shoot = |bed: &mut Bed, name: &str, crop: (u32, u32, u32, u32)| {
        common::at_rest(&mut bed.hp)?;
        common::wait(&mut bed.hp, Duration::from_millis(300))?;
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let (x, y, cw, ch) = crop;
        let part = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let doubled =
            image::imageops::resize(&part, cw * 2, ch * 2, image::imageops::FilterType::Nearest);
        let file = out.join(format!("{name}.png"));
        doubled.save(&file)?;
        eprintln!("{}", file.display());
        anyhow::Ok(())
    };
    let crop = (SCREEN.0 as u32 - 560, 0, 560, 620);
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Full)?;
    let (popover, _) = bed.open()?;
    bed.hp.pointer_move(400.0, 600.0);
    shoot(&mut bed, "today", crop)?;
    let mark = bed.mark();
    bed.click_in(popover, day_at(2, 1))?;
    bed.shows(mark, &["selected=2026-11-10"])?;
    bed.hp.pointer_move(400.0, 600.0);
    shoot(&mut bed, "busy-day", crop)?;
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Day(DayAsk::Calendar {
        day: "2026-11-04".to_owned(),
        event: Some("atelier".to_owned()),
    }));
    bed.shows(mark, &["unfolded=["])?;
    shoot(&mut bed, "unfolded", crop)?;
    let mark = bed.mark();
    let next = bed.target(0, "Next")?;
    bed.click_in(popover, next)?;
    bed.shows(mark, &["month=2026-12"])?;
    let mark = bed.mark();
    bed.click_in(popover, day_at(0, 3))?;
    bed.shows(mark, &["selected=2026-12-03"])?;
    bed.hp.pointer_move(400.0, 600.0);
    shoot(&mut bed, "another-month", crop)?;
    drop(bed);
    let mut bed = start(&shell, WITH_FIXTURE, FIXTURE, FormChoice::Mini)?;
    bed.click_clock(MINI_CLOCK)?;
    bed.popover()?;
    bed.hp.pointer_move(400.0, 600.0);
    shoot(&mut bed, "mini", (SCREEN.0 as u32 - 420, 0, 420, 620))?;
    Ok(())
}
