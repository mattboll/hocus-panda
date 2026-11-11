//! Today in the bar (issue #427, Day.3, variant A): the real shell on the
//! CPU, headless, its day a fixture's file (docs/design/day.md) read
//! against its clock (`HOCUS_CLOCK_AT`, 4 November 2026, in UTC), in
//! French. The shell says what Today shows in its log ("today is drawn at
//! rest"), which these tests read, with the screen's pixels and the
//! addresses its `xdg-open` (the test's own) is given.
//!
//! One test per point of the spec that can be measured: what it lists and
//! where; nothing after the card, no group; too many for its room, "+ N";
//! no room, no group; at the card's end, the first line becomes the card;
//! a line's click; its tooltip; the mini form, nothing; at rest and at a
//! minute's change that changes nothing, nothing drawn.

use crate::common;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::{DESKTOPS_NAMESPACE, TIP_NAMESPACE, TODAY_NAMESPACE};

/// Events that take no slot: all day, declined, free, cancelled, over.
const SKIPPED: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[[calendars]]
id = "rh"
name = "Recrutement"
color = "#C792EA"

[[calendars]]
id = "perso"
name = "Perso"
color = "#4FC1C9"

[[events]]
title = "Congés de Lou"
date = "today"

[[events]]
title = "Refusée"
start = "today 15:10"
answer = "declined"

[[events]]
title = "Créneau libre"
start = "today 15:20"
free = true

[[events]]
title = "Annulée"
start = "today 15:30"
status = "cancelled"

[[events]]
title = "Point quotidien"
start = "today 09:30"
end = "today 09:45"
"##;

/// The card's "Point produit" at 14:25, then three events.
fn ordinary() -> String {
    format!(
        "{SKIPPED}\n\
         [[events]]\ntitle = \"Point produit\"\nstart = \"today 14:25\"\nend = \"today 14:55\"\n\n\
         [[events]]\nid = \"interview\"\ncalendar = \"rh\"\ntitle = \"Entretien — poste back-end\"\n\
         start = \"today 15:00\"\nend = \"today 15:45\"\nvideo = \"https://meet.example.org/entretien\"\n\
         page = \"https://calendar.example.org/event/interview\"\n\n\
         [[events]]\nid = \"workshop\"\ntitle = \"Atelier planification T1\"\n\
         start = \"today 16:30\"\nend = \"today 17:30\"\npage = \"https://calendar.example.org/event/workshop\"\n\n\
         [[events]]\ncalendar = \"perso\"\ntitle = \"Piscine\"\nstart = \"today 18:30\"\nend = \"today 19:30\"\n"
    )
}

/// The card, then `n` events, every 10 min from 15:00.
fn busy(n: usize) -> String {
    let mut day = format!(
        "{SKIPPED}\n[[events]]\ntitle = \"Point produit\"\nstart = \"today 14:25\"\n\
         end = \"today 14:55\"\n"
    );
    for i in 0..n {
        let minutes = 15 * 60 + 10 * i;
        day.push_str(&format!(
            "\n[[events]]\ntitle = \"Réunion {i:02}\"\nstart = \"today {:02}:{:02}\"\n\
             end = \"today 23:30\"\n",
            minutes / 60,
            minutes % 60
        ));
    }
    day
}

const WITH_FIXTURE: &str = "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    fixture: PathBuf,
    launched: PathBuf,
}

fn ready() -> Option<PathBuf> {
    Some(common::shell_program())
}

/// The compositor on a `screen` with the real shell on the CPU, its clock
/// at `clock` (today's `HH:MM:SS`), its day `fixture`, its bar in `form`.
fn start(
    shell: &Path,
    screen: (u32, u32),
    clock: &str,
    fixture: &str,
    form: FormChoice,
) -> anyhow::Result<Bed> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "state", "config", "bin", "launched"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let file = root.join("config/config.toml");
    std::fs::write(
        &file,
        format!("{WITH_FIXTURE}\n[panel]\nform = \"{}\"\n", form.name()),
    )?;
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
             XDG_STATE_HOME='{state}' TZ=UTC HOCUS_CLOCK_AT='2026-11-04T{clock}' \
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
            outputs: vec![format!("{}x{}", screen.0, screen.1).parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        settings,
        Some(script),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(drawn, "no bar");
    hp.pointer_move(400.0, 600.0);
    Ok(Bed {
        _dir: dir,
        hp,
        log,
        fixture: fixture_file,
        launched,
    })
}

impl Bed {
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

    /// Waits until Today is drawn at rest showing `shows` (its rows, as
    /// the log says them, whole).
    fn today_since(&mut self, mark: usize, shows: &str) -> anyhow::Result<String> {
        self.logged_since(
            mark,
            "today is drawn at rest",
            &[&format!("shows={shows} lit=")],
        )
    }

    /// Waits until Today is drawn at rest, its rows starting with
    /// `shows`.
    fn today_from(&mut self, mark: usize, shows: &str) -> anyhow::Result<String> {
        self.logged_since(mark, "today is drawn at rest", &[&format!("shows={shows}")])
    }

    fn pixel(&mut self, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
        common::at_rest(&mut self.hp)?;
        let (w, _, px) = self.hp.screenshot(0)?;
        let at = ((y * w + x) * 4) as usize;
        Ok([px[at + 2], px[at + 1], px[at]])
    }

    /// Waits until the pixel at `at` is about `colour`.
    fn pixel_near(&mut self, at: (i32, i32), colour: [u8; 3]) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let seen = self.pixel(at.0, at.1)?;
            if seen.iter().zip(colour).all(|(a, b)| a.abs_diff(b) <= 3) {
                return Ok(());
            }
            anyhow::ensure!(
                start.elapsed() < common::WAIT,
                "at {at:?}: {seen:?}, not {colour:?}"
            );
            common::wait(&mut self.hp, Duration::from_millis(50))?;
        }
    }

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
        anyhow::ensure!(done, "opened {:?}, not {n}", read());
        Ok(read())
    }

    fn surface(&mut self) -> anyhow::Result<Rect> {
        Ok(common::drawn(&mut self.hp, TODAY_NAMESPACE)?[0].1)
    }

    /// Writes the fixture anew, as an editor saves it.
    fn rewrite(&self, fixture: &str) -> anyhow::Result<()> {
        let new = self.fixture.with_extension("new");
        std::fs::write(&new, fixture)?;
        std::fs::rename(&new, &self.fixture)?;
        Ok(())
    }
}

/// The group's line `i`'s middle on a 1920 px wide screen, its top `top`
/// (the screen's), and where its dot is.
fn line_middle(top: i32, i: i32) -> i32 {
    top + 18 + 20 * i + 10
}
const LINE_X: i32 = 1920 - 8 - 196 + 12;
const DOT_X: i32 = LINE_X + 38 + 7 + 3;

#[test]
fn today_lists_the_events_after_the_card_in_order_under_the_card() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        (1920, 1080),
        "14:00:00",
        &ordinary(),
        FormChoice::Full,
    )?;
    // The card's meeting not repeated; all day, declined, free, cancelled
    // and over skipped.
    let line = bed.today_since(
        0,
        "AUJOURD'HUI 15:00 «Entretien — poste back-end» #C792EA; \
         16:30 «Atelier planification T1» #6F9BFF; 18:30 «Piscine» #4FC1C9",
    )?;
    assert!(
        line.contains(
            "names=[\"15:00, Entretien — poste back-end, visio\", \
             \"16:30, Atelier planification T1\", \"18:30, Piscine\"]"
        ),
        "{line}"
    );
    // 14 px under the card, the first of the day's groups (#599), 18 px
    // and 20 a line.
    let surface = bed.surface()?;
    assert_eq!(
        (surface.x, surface.y, surface.width, surface.height),
        (1920 - 9 - 194, 8 + 100 + 58 - 5, 194, 18 + 60 + 10)
    );
    // Each line's dot in its calendar's colour.
    let top = surface.y + 5;
    bed.pixel_near((DOT_X, line_middle(top, 0)), [0xC7, 0x92, 0xEA])?;
    bed.pixel_near((DOT_X, line_middle(top, 2)), [0x4F, 0xC1, 0xC9])?;
    Ok(())
}

#[test]
fn nothing_after_the_cards_meeting_no_group() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fixture = format!(
        "{SKIPPED}\n[[events]]\ntitle = \"Piscine\"\nstart = \"today 18:30\"\nend = \"today 19:30\"\n"
    );
    let mut bed = start(&shell, (1920, 1080), "14:00:00", &fixture, FormChoice::Full)?;
    bed.logged_since(0, "the meeting card is drawn at rest", &["«Piscine»"])?;
    common::drawn(&mut bed.hp, hocus_link::MEETING_NAMESPACE)?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.hp.layer_rects(TODAY_NAMESPACE).is_empty());
    Ok(())
}

#[test]
fn with_more_events_than_room_the_last_line_says_how_many_more() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let n = 40;
    let mut bed = start(&shell, (1920, 1080), "14:00:00", &busy(n), FormChoice::Full)?;
    bed.logged_since(0, "today is drawn at rest", &["autres aujourd'hui"])?;
    // Its place settled (the bar's height, the desktops' known): the last
    // rows drawn.
    bed.surface()?;
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let line = bed
        .lines_since(0, "today is drawn at rest")
        .pop()
        .unwrap_or_default();
    let shown = line.matches("«Réunion").count();
    assert!(
        line.contains(&format!("+ {} autres aujourd'hui lit=", n - shown)),
        "{line}"
    );
    // Its lines in its order, the first ones.
    assert!(
        line.contains("15:00 «Réunion 00» #6F9BFF; 15:10 «Réunion 01» #6F9BFF;"),
        "{line}"
    );
    // As many as fit: its bottom 14 px or more over the desktops' group,
    // and one more line would not have fitted.
    let surface = bed.surface()?;
    let desktops = common::drawn(&mut bed.hp, DESKTOPS_NAMESPACE)?[0].1;
    let (bottom, under) = (surface.y + surface.height - 5, desktops.y + 5);
    assert!(bottom + 14 <= under, "{bottom} over {under}");
    assert!(bottom + 20 + 14 > under, "room left: {bottom} over {under}");
    assert_eq!(surface.height - 10, 18 + 20 * (shown as i32 + 1));
    Ok(())
}

#[test]
fn on_a_short_screen_today_gives_way_first() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        (1920, 320),
        "14:00:00",
        &ordinary(),
        FormChoice::Full,
    )?;
    bed.logged_since(0, "the meeting card is drawn at rest", &["«Point produit»"])?;
    common::drawn(&mut bed.hp, hocus_link::MEETING_NAMESPACE)?;
    let desktops = common::drawn(&mut bed.hp, DESKTOPS_NAMESPACE)?[0].1;
    // Not even one line's room between the card and the desktops.
    let room = desktops.y + 5 - 14 - (8 + 100 + 58);
    assert!(room < 18 + 20, "room for {room} px");
    // Today may come first, placed while the groups under it are not yet,
    // and go once they are: time for it to come, then as long as it takes
    // to go. Looked at once after 500 ms, it was still there on a loaded
    // machine.
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let gone = bed.hp.dispatch_until(common::WAIT, |hp| {
        hp.layer_rects(TODAY_NAMESPACE).is_empty()
    })?;
    assert!(
        gone,
        "today stays at {:?}; the shell said:\n{}",
        bed.hp.layer_rects(TODAY_NAMESPACE),
        bed.lines_since(0, "oday").join("\n")
    );
    // What the shell last said of its rows, if it had any: none.
    let rows = bed.lines_since(0, "today's rows");
    assert!(
        rows.last().is_none_or(|l| l.contains("shows=nothing")),
        "{rows:?}"
    );
    Ok(())
}

#[test]
fn at_the_cards_end_its_first_line_becomes_the_card() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // "Atelier" ends at 14:00, 10 s after the shell starts.
    let fixture = format!(
        "{SKIPPED}\n[[events]]\ntitle = \"Atelier\"\nstart = \"today 13:30\"\nend = \"today 14:00\"\n\n\
         [[events]]\ntitle = \"Entretien\"\nstart = \"today 14:20\"\nend = \"today 15:00\"\n\n\
         [[events]]\ntitle = \"Piscine\"\ncalendar = \"perso\"\nstart = \"today 18:30\"\n\
         end = \"today 19:30\"\n"
    );
    let mut bed = start(&shell, (1920, 1080), "13:59:50", &fixture, FormChoice::Full)?;
    bed.today_since(
        0,
        "AUJOURD'HUI 14:20 «Entretien» #6F9BFF; 18:30 «Piscine» #4FC1C9",
    )?;
    let mark = bed.mark();
    bed.logged_since(
        mark,
        "the meeting card is drawn at rest",
        &["«Entretien» 14:20 · dans 20"],
    )?;
    bed.today_since(mark, "AUJOURD'HUI 18:30 «Piscine» #4FC1C9")?;
    // One line shorter.
    let shorter = bed.hp.dispatch_until(common::WAIT, |hp| {
        hp.drawn_layers(TODAY_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| r.height == 18 + 20 + 10)
    })?;
    assert!(shorter, "{:?}", bed.hp.layer_rects(TODAY_NAMESPACE));
    Ok(())
}

#[test]
fn a_line_opens_its_call_else_the_calendar_on_its_event_and_more_the_calendar() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        (1920, 1080),
        "14:00:00",
        &ordinary(),
        FormChoice::Full,
    )?;
    bed.today_from(0, "AUJOURD'HUI 15:00")?;
    let top = bed.surface()?.y + 5;
    let at = |i| (f64::from(LINE_X + 80), f64::from(line_middle(top, i)));
    // With a video link: the call.
    common::click_on(&mut bed.hp, at(0), TODAY_NAMESPACE)?;
    assert_eq!(bed.opened(1)?, ["https://meet.example.org/entretien"]);
    // Without: the calendar (Day.5) on today, the event chosen, a page or
    // none.
    let mark = bed.mark();
    common::click_on(&mut bed.hp, at(1), TODAY_NAMESPACE)?;
    bed.logged_since(
        mark,
        "the calendar opens",
        &["day=2026-11-04", "focus=Some(Event(EventId(\"workshop\")))"],
    )?;
    let mark = bed.mark();
    common::click_on(&mut bed.hp, at(2), TODAY_NAMESPACE)?;
    bed.logged_since(
        mark,
        "the calendar opens",
        &["day=2026-11-04", "focus=Some(Event("],
    )?;
    // Closed (`Esc`) before the day changes under it.
    bed.hp.key(common::KEY_ESC, true);
    bed.hp.key(common::KEY_ESC, false);
    let closed = bed
        .hp
        .dispatch_until(common::WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the calendar stayed open");
    // "+ N": the calendar on today.
    let mark = bed.mark();
    bed.rewrite(&busy(40))?;
    let line = bed.logged_since(mark, "today is drawn at rest", &["autres aujourd'hui"])?;
    let shown = line.matches("«Réunion").count() as i32;
    let top = bed.surface()?.y + 5;
    let mark = bed.mark();
    common::click_on(
        &mut bed.hp,
        (f64::from(LINE_X + 40), f64::from(line_middle(top, shown))),
        TODAY_NAMESPACE,
    )?;
    bed.logged_since(
        mark,
        "the calendar opens",
        &["day=2026-11-04", "focus=Some(Days)"],
    )?;
    assert_eq!(bed.opened(1)?.len(), 1);
    Ok(())
}

#[test]
fn resting_on_a_line_shows_its_times_and_its_title() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        (1920, 1080),
        "14:00:00",
        &ordinary(),
        FormChoice::Full,
    )?;
    bed.today_from(0, "AUJOURD'HUI 15:00")?;
    let top = bed.surface()?.y + 5;
    let mark = bed.mark();
    common::over(
        &mut bed.hp,
        (f64::from(LINE_X + 80), f64::from(line_middle(top, 0))),
        TODAY_NAMESPACE,
    )?;
    bed.logged_since(
        mark,
        "a tooltip shows",
        &["name=\"15:00–15:45\"", "tip=\"Entretien — poste back-end\""],
    )?;
    let tip = common::drawn(&mut bed.hp, TIP_NAMESPACE)?[0].1;
    // Beside the bar, on the line.
    assert!(tip.x + tip.width <= 1920 - 8 - 196, "{tip:?}");
    assert!(
        (tip.y..tip.y + tip.height).contains(&line_middle(top, 0)),
        "{tip:?}"
    );
    // Under the pointer the line is lit, and the pointer leaving takes the
    // tooltip.
    bed.logged_since(mark, "today is drawn at rest", &["lit=Some(0)"])?;
    bed.hp.pointer_move(400.0, 600.0);
    let gone = bed
        .hp
        .dispatch_until(common::WAIT, |hp| hp.layer_rects(TIP_NAMESPACE).is_empty())?;
    assert!(gone);
    Ok(())
}

#[test]
fn the_mini_form_has_no_today() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        (1920, 1080),
        "14:00:00",
        &ordinary(),
        FormChoice::Mini,
    )?;
    common::drawn(&mut bed.hp, hocus_link::MEETING_NAMESPACE)?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.hp.layer_rects(TODAY_NAMESPACE).is_empty());
    assert!(bed.lines_since(0, "today is drawn at rest").is_empty());
    Ok(())
}

#[test]
fn at_rest_and_at_a_minutes_change_that_changes_nothing_it_draws_nothing() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        (1920, 1080),
        "14:00:52",
        &ordinary(),
        FormChoice::Full,
    )?;
    bed.today_from(0, "AUJOURD'HUI 15:00")?;
    bed.surface()?;
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    let commits = bed.hp.today_commits();
    let mark = bed.mark();
    // Through 14:01: the card's minutes change, Today's lines do not.
    bed.logged_since(mark, "the meeting card is drawn at rest", &["dans 24"])?;
    common::wait(&mut bed.hp, Duration::from_secs(1))?;
    assert_eq!(bed.hp.today_commits(), commits, "today drew at rest");
    assert!(bed.lines_since(mark, "today's rows").is_empty());
    Ok(())
}

/// The pull request's screenshots, the shell on the CPU, its clock at
/// 14:00: the bar's top with three lines, two, "+ N" on a fuller day, and
/// none after the card; Today 3 times larger. Written to `$HOCUS_SHOTS`
/// when it names a folder. Run by hand (`--run-ignored only`).
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
    let shoot = |bed: &mut Bed, name: &str, crop: (u32, u32, u32, u32), zoom: Option<Rect>| {
        common::at_rest(&mut bed.hp)?;
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let (x, y, cw, ch) = crop;
        let bar = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let file = out.join(format!("{name}.png"));
        bar.save(&file)?;
        eprintln!("{}", file.display());
        if let Some(r) = zoom {
            let (x, y) = ((r.x - 8).max(0) as u32, (r.y - 8).max(0) as u32);
            let (cw, ch) = ((r.width + 16) as u32, (r.height + 16) as u32);
            let zoomed = image::imageops::resize(
                &image::imageops::crop_imm(&image, x, y, cw, ch).to_image(),
                cw * 3,
                ch * 3,
                image::imageops::FilterType::Nearest,
            );
            let file = out.join(format!("{name}-zoom.png"));
            zoomed.save(&file)?;
            eprintln!("{}", file.display());
        }
        anyhow::Ok(())
    };
    let crop = (1920 - 420, 0, 420, 400);
    let mut bed = start(
        &shell,
        (1920, 1080),
        "14:00:00",
        &ordinary(),
        FormChoice::Full,
    )?;
    bed.today_from(0, "AUJOURD'HUI 15:00")?;
    let surface = bed.surface()?;
    shoot(&mut bed, "three", crop, Some(surface))?;
    // Two: the swim gone.
    let mark = bed.mark();
    let two = ordinary().replace(
        "\n[[events]]\ncalendar = \"perso\"\ntitle = \"Piscine\"\nstart = \"today 18:30\"\nend = \"today 19:30\"\n",
        "\n",
    );
    bed.rewrite(&two)?;
    bed.today_since(
        mark,
        "AUJOURD'HUI 15:00 «Entretien — poste back-end» #C792EA; \
         16:30 «Atelier planification T1» #6F9BFF",
    )?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let surface = bed.surface()?;
    shoot(&mut bed, "two", crop, Some(surface))?;
    // A line lit under the pointer, its tooltip.
    let top = surface.y + 5;
    let mark = bed.mark();
    common::over(
        &mut bed.hp,
        (f64::from(LINE_X + 80), f64::from(line_middle(top, 0))),
        TODAY_NAMESPACE,
    )?;
    bed.logged_since(mark, "a tooltip shows", &[])?;
    common::drawn(&mut bed.hp, TIP_NAMESPACE)?;
    shoot(&mut bed, "tooltip", (1920 - 520, 0, 520, 400), None)?;
    bed.hp.pointer_move(400.0, 600.0);
    // A fuller day on a shorter screen: "+ N".
    drop(bed);
    let mut bed = start(&shell, (1920, 420), "14:00:00", &busy(9), FormChoice::Full)?;
    bed.logged_since(0, "today is drawn at rest", &["autres aujourd'hui"])?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let surface = bed.surface()?;
    shoot(&mut bed, "more", (1920 - 420, 0, 420, 420), Some(surface))?;
    // Nothing after the card.
    let mark = bed.mark();
    bed.rewrite(&format!(
        "{SKIPPED}\n[[events]]\ntitle = \"Point produit\"\nstart = \"today 14:25\"\nend = \"today 14:55\"\n"
    ))?;
    bed.logged_since(mark, "today's rows", &["shows=nothing"])?;
    bed.hp.dispatch_until(common::WAIT, |hp| {
        hp.layer_rects(TODAY_NAMESPACE).is_empty()
    })?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    shoot(&mut bed, "none", (1920 - 420, 0, 420, 420), None)?;
    Ok(())
}
