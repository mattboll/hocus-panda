//! Featured calendars (issue #607, Day.7): only some calendars feed what
//! is about to start. The real shell on the CPU, headless, in French, its
//! day a fixture's file of two calendars (docs/design/day.md) read against
//! its clock (`HOCUS_CLOCK_AT`, 4 November 2026 at 14:25, in UTC), its
//! configuration and folders the test's own, its `xdg-open` a script that
//! keeps what it is given. The shell says what the meeting card, the
//! reminders, Today and the calendar show in its log, which is what these
//! tests read, with the reminders' surfaces and the compositor's halo.
//!
//! One test per point of the issue that can be measured: an event of a
//! calendar not featured, starting first, takes neither the card, nor a
//! reminder, nor the halo, nor `Super+J`, and still shows in Today and in
//! the calendar; without `featured`, every shown calendar feeds them, as
//! before. Day.1's step 3 writing `featured` is `day_sources.rs`'s.

use crate::common;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, halo::Concern};
use hocus_link::{DayAsk, ToShell};

/// Two calendars: "Travail", and a colleague's leave, "Congés"; Lou's
/// leave starts first, in 3 min (within the halo's 10 and the reminder's
/// 5), "Point produit" in 35.
const DAY: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[[calendars]]
id = "leave"
name = "Congés"
color = "#F28FB1"

[[events]]
id = "lou"
calendar = "leave"
title = "Congés de Lou"
start = "today 14:28"
end = "today 18:00"
video = "https://meet.example.org/lou"

[[events]]
id = "point"
calendar = "work"
title = "Point produit"
start = "today 15:00"
end = "today 15:30"
video = "https://meet.example.org/point"
"##;

const KEY_J: u32 = 36;

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    launched: PathBuf,
}

fn ready() -> Option<PathBuf> {
    Some(common::shell_program())
}

/// The compositor with the real shell on the CPU at 14:25, its day
/// [`DAY`] through a `[[day.calendars]]` with `entry`'s lines.
fn start(shell: &Path, entry: &str) -> anyhow::Result<Bed> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "state", "config", "bin", "launched"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let file = root.join("config/config.toml");
    std::fs::write(
        &file,
        format!("[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n{entry}"),
    )?;
    std::fs::write(root.join("config/day.toml"), DAY)?;
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
             XDG_STATE_HOME='{state}' TZ=UTC HOCUS_CLOCK_AT='2026-11-04T14:25:00' \
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
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
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
        launched,
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

    /// Waits until the meeting card is drawn at rest showing all of
    /// `holds`.
    fn card(&mut self, holds: &[&str]) -> anyhow::Result<String> {
        self.logged_since(0, "the meeting card is drawn at rest", holds)
    }

    /// Waits until Today is drawn at rest showing `shows`, whole.
    fn today(&mut self, shows: &str) -> anyhow::Result<String> {
        self.logged_since(
            0,
            "today is drawn at rest",
            &[&format!("shows={shows} lit=")],
        )
    }

    /// The calendar opened on today: what it shows, once it holds all of
    /// `holds`.
    fn calendar(&mut self, holds: &[&str]) -> anyhow::Result<String> {
        let mark = self.mark();
        self.hp.tell_shell(ToShell::Day(DayAsk::Calendar {
            day: "2026-11-04".to_owned(),
            event: None,
        }));
        self.logged_since(mark, "the calendar shows it", holds)
    }

    fn halo(&mut self, concern: Option<Concern>) -> anyhow::Result<()> {
        let done = self
            .hp
            .dispatch_until(common::WAIT, |hp| hp.halo_concern() == concern)?;
        anyhow::ensure!(
            done,
            "the halo {:?}, not {concern:?}",
            self.hp.halo_concern()
        );
        Ok(())
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
}

#[test]
fn an_event_of_a_calendar_not_featured_takes_no_card_reminder_nor_halo_and_still_shows_in_today_and_the_calendar()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "featured = [\"Travail\"]\n")?;
    bed.logged_since(0, "the day holds", &["«Congés de Lou»", "«Point produit»"])?;
    // The card: "Point produit", 35 min ahead, though Lou's leave starts
    // first.
    let card = bed.card(&["«Point produit» 15:00 · dans 35\u{a0}min look=Far"])?;
    assert!(!card.contains("Congés"), "{card}");
    // No halo: the card's meeting is far, Lou's leave is not featured.
    bed.logged_since(0, "the next meeting's halo", &["near=None"])?;
    bed.halo(None)?;
    assert!(
        bed.lines_since(0, "the next meeting's halo")
            .iter()
            .all(|l| l.contains("near=None")),
        "{}",
        bed.log()
    );
    // No reminder: the next is "Point produit"'s, 5 min before it; Lou's
    // would have come at 14:23, and shown at once.
    bed.logged_since(0, "the reminders wait", &["at=14:55"])?;
    assert!(
        bed.hp.reminder_surfaces().is_empty(),
        "{:?}",
        bed.hp.reminder_surfaces()
    );
    assert!(
        bed.lines_since(0, "the reminder is drawn").is_empty(),
        "{}",
        bed.log()
    );
    // Super+J: nothing to join, though Lou's leave has a link and starts
    // within 10 min.
    let mark = bed.mark();
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, KEY_J]);
    bed.logged_since(mark, "Super+J: no meeting to join", &[])?;
    assert!(!bed.launched.exists(), "{}", bed.log());
    // Today and the calendar still show it.
    bed.today("AUJOURD'HUI 14:28 «Congés de Lou» #F28FB1")?;
    let shown = bed.calendar(&["selected=2026-11-04", "«Congés de Lou»"])?;
    assert!(shown.contains("«Point produit»"), "{shown}");
    Ok(())
}

#[test]
fn without_featured_every_shown_calendar_feeds_the_card_the_reminders_and_the_halo()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "")?;
    // As before: Lou's leave takes the card, 3 min ahead, with the halo
    // and its reminder; "Point produit" is Today's.
    bed.card(&["«Congés de Lou» 14:28 · dans 3\u{a0}min look=Soon"])?;
    bed.halo(Some(Concern::MeetingSoon))?;
    bed.logged_since(0, "the reminder is drawn at rest", &["«Congés de Lou»"])?;
    let tabs = bed
        .hp
        .dispatch_until(common::WAIT, |hp| hp.reminder_surfaces().len() == 1)?;
    anyhow::ensure!(tabs, "{:?}", bed.hp.reminder_surfaces());
    bed.today("AUJOURD'HUI 15:00 «Point produit» #6F9BFF")?;
    bed.calendar(&["«Congés de Lou»", "«Point produit»"])?;
    // Super+J joins it.
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, KEY_J]);
    assert_eq!(bed.opened(1)?, ["https://meet.example.org/lou"]);
    Ok(())
}
