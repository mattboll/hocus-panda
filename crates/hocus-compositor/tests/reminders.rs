//! A meeting's reminder (issue #430, Day.6, variant B): the real shell on
//! the CPU, headless, its day a fixture's file (docs/design/day.md) read
//! against its clock (`HOCUS_CLOCK_AT`, 4 November 2026, in UTC), in
//! French. The shell says what each tab shows in its log ("the reminder
//! is drawn at rest") and when it waits ("the reminders wait"), which is
//! what these tests read, with the tabs' surfaces, the screen's pixels, the
//! compositor's halo and the addresses its `xdg-open` (the test's own) is
//! given.
//!
//! One test per point of the spec that can be measured: the tab 5 min
//! before, beside the bar at the card's height; at the event's own alarm;
//! none for a declined, a cancelled or an all-day event; Join opening the
//! link, `Super+J` too; ✕ ending the halo, Later bringing it back; a
//! resume after its time; through do not disturb; its title hidden on a
//! shared screen; at the screen's edge over a fullscreen window; to the
//! history 10 min after the start; the mini form's place; at rest nothing
//! wakes before the next reminder.

mod common;

use std::{
    path::{Path, PathBuf},
    process::Child,
    time::{Duration, Instant},
};

use hocus_compositor::{
    Config, FormChoice, Headless, HeadlessConfig, RendererChoice, halo::Concern, model::Rect,
};
use hocus_link::{DayAsk, ToShell};

const CALENDAR: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"
"##;

/// "Point produit" at 14:30 with a video call (and `extra` lines).
fn day(extra: &str) -> String {
    format!(
        "{CALENDAR}\n[[events]]\nid = \"point\"\ntitle = \"Point produit\"\n\
         start = \"today 14:30\"\nend = \"today 15:00\"\n\
         video = \"https://meet.example.org/abc-defg\"\n\
         page = \"https://calendar.example.org/event/point\"\n{extra}"
    )
}

const WITH_FIXTURE: &str = "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

const SCREEN: (i32, i32) = (1920, 1080);
/// The tab in the full form, on the CPU (the tab alone): 340 × 44, 8 px
/// left of the 196 px bar (itself 8 px from the edge), its top on the
/// meeting card's (100 px under the bar's top).
const TAB: (i32, i32, i32, i32) = (1920 - 8 - 196 - 8 - 340, 8 + 100, 340, 44);
/// Its ✕ and its Later, 30 × 30, 6 px apart from 6 px in on the right.
const DISMISS: (i32, i32) = (TAB.0 + 340 - 7 - 15, TAB.1 + 22);
const LATER: (i32, i32) = (DISMISS.0 - 36, TAB.1 + 22);
/// Inside Rejoindre, 6 px before its right end (no text there).
const JOIN_FILL: (i32, i32) = (LATER.0 - 15 - 6 - 6, TAB.1 + 22);
const JOIN_BLUE: [u8; 3] = [0x3F, 0x6F, 0xD8];

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    launched: PathBuf,
    bus: Option<Child>,
}

impl Drop for Bed {
    fn drop(&mut self) {
        if let Some(bus) = &mut self.bus {
            let _ = bus.kill();
            let _ = bus.wait();
        }
    }
}

fn ready() -> Option<PathBuf> {
    let shell = common::shell_program();
    if shell.is_none() {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
    }
    shell
}

#[derive(Default)]
struct Options {
    form: Option<FormChoice>,
    /// Notifications served on a private bus, do not disturb on.
    dnd: bool,
}

/// The compositor with the real shell on the CPU, its clock at `clock`
/// (today's `HH:MM:SS`), its day `fixture`.
fn start(shell: &Path, clock: &str, fixture: &str, options: Options) -> anyhow::Result<Bed> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "state", "config", "bin", "launched"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let file = root.join("config/config.toml");
    std::fs::write(&file, WITH_FIXTURE)?;
    std::fs::write(root.join("config/day.toml"), fixture)?;
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
    // Do not disturb on for this session (the compositor's, the shell's
    // parent), where the notifications keep it.
    let dnd = if options.dnd {
        "mkdir -p \"$XDG_RUNTIME_DIR/hocuspanda\" && \
         printf 'session %s\\nchoice always\\n' \"$PPID\" > \"$XDG_RUNTIME_DIR/hocuspanda/dnd\"\n"
    } else {
        ""
    };
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
             {dnd}exec '{shell}' \"$@\" 2>>'{log}'\n",
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
    settings.panel.form = options.form.unwrap_or(FormChoice::Full);
    let (bus, address) = if options.dnd {
        let (bus, address) = common::private_bus(root)?;
        (Some(bus), Some(address))
    } else {
        (None, None)
    };
    let mut hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec![format!("{}x{}", SCREEN.0, SCREEN.1).parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        settings,
        Some(script),
        address,
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(drawn, "no bar");
    hp.pointer_move(400.0, 600.0);
    Ok(Bed {
        _dir: dir,
        hp,
        log,
        launched,
        bus,
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

    /// Waits until a tab is drawn at rest showing all of `holds`.
    fn tab_since(&mut self, mark: usize, holds: &[&str]) -> anyhow::Result<String> {
        self.logged_since(mark, "the reminder is drawn at rest", holds)
    }

    /// The tabs' surfaces, once `n` are there.
    fn tabs(&mut self, n: usize) -> anyhow::Result<Vec<(String, String, Rect)>> {
        let done = self
            .hp
            .dispatch_until(common::WAIT, |hp| hp.reminder_surfaces().len() == n)?;
        anyhow::ensure!(
            done,
            "tabs {:?}, not {n}:\n{}",
            self.hp.reminder_surfaces(),
            self.log()
        );
        Ok(self.hp.reminder_surfaces())
    }

    /// The pixel at `(x, y)` of the screen, RGB, once at rest.
    fn pixel(&mut self, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
        common::at_rest(&mut self.hp)?;
        let (w, _, px) = self.hp.screenshot(0)?;
        let at = ((y * w + x) * 4) as usize;
        Ok([px[at + 2], px[at + 1], px[at]])
    }

    /// Waits until the pixel at `at` is about `colour`.
    fn pixel_near(&mut self, (x, y): (i32, i32), colour: [u8; 3]) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let seen = self.pixel(x, y)?;
            if seen.iter().zip(colour).all(|(a, b)| a.abs_diff(b) <= 3) {
                return Ok(());
            }
            anyhow::ensure!(
                start.elapsed() < common::WAIT,
                "at {:?}: {seen:?}, not {colour:?}",
                (x, y)
            );
            common::wait(&mut self.hp, Duration::from_millis(50))?;
        }
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

    /// A click at `at` on the first tab, once it takes the pointer there
    /// and has drawn the part lit (`part`, as the log names it).
    fn click_tab(&mut self, at: (i32, i32), part: &str) -> anyhow::Result<()> {
        let namespace = self.tabs(1)?[0].0.clone();
        let mark = self.mark();
        let lit = format!("lit=Some({part})");
        let start = Instant::now();
        loop {
            common::over(&mut self.hp, (f64::from(at.0), f64::from(at.1)), &namespace)?;
            common::wait(&mut self.hp, Duration::from_millis(100))?;
            if self
                .lines_since(mark, "the reminder is drawn at rest")
                .iter()
                .any(|l| l.contains(&lit))
            {
                break;
            }
            anyhow::ensure!(
                start.elapsed() < common::WAIT,
                "the tab never drew its {part} lit:\n{}",
                self.log()
            );
            self.hp.pointer_move(400.0, 600.0);
            common::wait(&mut self.hp, Duration::from_millis(50))?;
        }
        self.hp.button(common::BTN_LEFT, true);
        self.hp.button(common::BTN_LEFT, false);
        Ok(())
    }

    /// Waits until the tabs are gone.
    fn no_tab(&mut self) -> anyhow::Result<()> {
        let gone = self
            .hp
            .dispatch_until(common::WAIT, |hp| hp.reminder_surfaces().is_empty())?;
        anyhow::ensure!(gone, "{:?}", self.hp.reminder_surfaces());
        Ok(())
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
}

const KEY_J: u32 = 36;

#[test]
fn five_minutes_before_the_tab_comes_beside_the_bar_at_the_cards_height() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // 14:24:48: nothing yet, then the tab at 14:25.
    let mut bed = start(&shell, "14:24:48", &day(""), Options::default())?;
    bed.logged_since(0, "the reminders wait", &["at=14:25"])?;
    assert!(bed.hp.reminder_surfaces().is_empty());
    let line = bed.tab_since(
        0,
        &[
            "«Point produit» 14:30 · dans 5\u{a0}min · visio",
            "main=Rejoindre",
            "later=Me le redire 1 min avant",
        ],
    )?;
    assert!(
        line.contains("name=Point produit, dans 5 minutes, visio"),
        "{line}"
    );
    bed.logged_since(
        0,
        "the reminder is announced",
        &["Point produit, dans 5 minutes, visio"],
    )?;
    // 340 × 44, 8 px left of the bar, its top on the card's.
    let tabs = bed.tabs(1)?;
    let r = tabs[0].2;
    assert_eq!((r.x, r.y, r.width, r.height), TAB);
    // Rejoindre in its blue.
    bed.pixel_near(JOIN_FILL, JOIN_BLUE)?;
    // The halo, the card's: within 10 min.
    bed.halo(Some(Concern::MeetingSoon))?;
    Ok(())
}

#[test]
fn at_the_events_own_alarm_rather_than_5_min_before() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:14:50",
        &day("alarms = [15]\n"),
        Options::default(),
    )?;
    bed.logged_since(0, "the reminders wait", &["at=14:15"])?;
    bed.tab_since(0, &["«Point produit» 14:30 · dans 15\u{a0}min · visio"])?;
    Ok(())
}

#[test]
fn none_for_a_declined_a_cancelled_or_an_all_day_event() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // Each would remind now (14:25, within 5 min of its start, or its own
    // alarm); only "Point produit" at 14:40 will, at 14:35.
    let fixture = format!(
        "{CALENDAR}\n\
         [[events]]\ntitle = \"Refusée\"\nstart = \"today 14:28\"\nanswer = \"declined\"\n\n\
         [[events]]\ntitle = \"Annulée\"\nstart = \"today 14:28\"\nstatus = \"cancelled\"\n\
         alarms = [10]\n\n\
         [[events]]\ntitle = \"Congés de Lou\"\ndate = \"today\"\n\n\
         [[events]]\ntitle = \"Point produit\"\nstart = \"today 14:40\"\n"
    );
    let mut bed = start(&shell, "14:25:00", &fixture, Options::default())?;
    bed.logged_since(0, "the day holds", &["Refusée", "Point produit"])?;
    // The next is 14:35's: none due now.
    bed.logged_since(0, "the reminders wait", &["at=14:35"])?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
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
    Ok(())
}

#[test]
fn join_opens_the_video_link_and_ends_the_halo_the_card_staying() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "14:25:00", &day(""), Options::default())?;
    bed.tab_since(0, &["«Point produit»", "main=Rejoindre"])?;
    bed.halo(Some(Concern::MeetingSoon))?;
    bed.click_tab(JOIN_FILL, "Main")?;
    assert_eq!(bed.opened(1)?, ["https://meet.example.org/abc-defg"]);
    // The tab goes; the halo's meeting state ends; the card stays.
    bed.no_tab()?;
    bed.halo(None)?;
    assert!(!bed.hp.layer_rects(hocus_link::MEETING_NAMESPACE).is_empty());
    Ok(())
}

#[test]
fn super_j_joins_the_meeting_starting_next_within_10_min_else_nothing() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // At 14:25, "Démo" at 14:33 (its tab at 14:28), "Loin" at 14:40.
    let loin = "[[events]]\nid = \"loin\"\ntitle = \"Loin\"\nstart = \"today 14:40\"\n\
                video = \"https://meet.example.org/loin\"\n";
    let fixture = format!(
        "{CALENDAR}\n[[events]]\nid = \"demo\"\ntitle = \"Démo\"\nstart = \"today 14:33\"\n\
         video = \"https://meet.example.org/demo\"\n\n{loin}"
    );
    let mut bed = start(&shell, "14:25:00", &fixture, Options::default())?;
    bed.logged_since(0, "the reminders wait", &["at=14:28"])?;
    assert!(bed.hp.reminder_surfaces().is_empty());
    let mark = bed.mark();
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, KEY_J]);
    bed.logged_since(mark, "Super+J joins the meeting", &["id=\"demo\""])?;
    assert_eq!(bed.opened(1)?, ["https://meet.example.org/demo"]);
    // Joined: its tab will not come.
    bed.logged_since(mark, "the reminders wait", &["at=14:35"])?;
    drop(bed);
    // "Loin" alone, 15 min ahead: nothing.
    let mut bed = start(
        &shell,
        "14:25:00",
        &format!("{CALENDAR}\n{loin}"),
        Options::default(),
    )?;
    bed.logged_since(0, "the reminders wait", &["at=14:35"])?;
    let mark = bed.mark();
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, KEY_J]);
    bed.logged_since(mark, "Super+J: no meeting to join", &[])?;
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(!bed.launched.exists());
    Ok(())
}

#[test]
fn super_j_joins_the_meeting_whose_tab_shows() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "14:25:00", &day(""), Options::default())?;
    bed.tab_since(0, &["«Point produit»"])?;
    bed.tabs(1)?;
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, KEY_J]);
    assert_eq!(bed.opened(1)?, ["https://meet.example.org/abc-defg"]);
    bed.no_tab()?;
    bed.halo(None)?;
    Ok(())
}

#[test]
fn dismissing_it_takes_it_away_and_ends_its_halo() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "14:25:00", &day(""), Options::default())?;
    bed.tab_since(0, &["«Point produit»"])?;
    bed.halo(Some(Concern::MeetingSoon))?;
    // Resting on ✕: its name after 500 ms, under the tab, centred on it.
    let namespace = bed.tabs(1)?[0].0.clone();
    let mark = bed.mark();
    common::over(
        &mut bed.hp,
        (f64::from(DISMISS.0), f64::from(DISMISS.1)),
        &namespace,
    )?;
    bed.logged_since(mark, "a tooltip shows", &["name=\"Ignorer ce rappel\""])?;
    let tip = common::drawn(&mut bed.hp, hocus_link::TIP_NAMESPACE)?[0].1;
    assert_eq!(tip.y, TAB.1 + 44 + 6, "{tip:?}");
    assert!((tip.x + tip.width / 2 - DISMISS.0).abs() <= 1, "{tip:?}");
    let mark = bed.mark();
    bed.click_tab(DISMISS, "Dismiss")?;
    bed.logged_since(mark, "the reminder is dismissed", &["id=\"point\""])?;
    // Its tooltip goes with the press.
    let gone = bed.hp.dispatch_until(common::WAIT, |hp| {
        hp.layer_rects(hocus_link::TIP_NAMESPACE).is_empty()
    })?;
    assert!(gone, "the tooltip stayed");
    bed.no_tab()?;
    // So that the 2-min wave does not run; the card stays.
    bed.halo(None)?;
    assert!(!bed.hp.layer_rects(hocus_link::MEETING_NAMESPACE).is_empty());
    assert!(bed.lines_since(0, "the launcher opens").is_empty());
    Ok(())
}

#[test]
fn later_takes_it_away_until_1_min_before_the_halo_going_on() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // 14:28:45: "dans 2 min"; back at 14:29, "dans 1 min".
    let mut bed = start(&shell, "14:28:45", &day(""), Options::default())?;
    bed.tab_since(0, &["dans 2\u{a0}min", "later=Me le redire 1 min avant"])?;
    bed.halo(Some(Concern::MeetingNow))?;
    let mark = bed.mark();
    bed.click_tab(LATER, "Later")?;
    bed.logged_since(mark, "the reminder comes back later", &[])?;
    bed.no_tab()?;
    assert_eq!(bed.hp.halo_concern(), Some(Concern::MeetingNow));
    bed.logged_since(mark, "the reminders wait", &["at=14:29"])?;
    // Back at 14:29: Later now says "à l'heure".
    bed.tab_since(mark, &["dans 1\u{a0}min", "later=Me le redire à l'heure"])?;
    bed.tabs(1)?;
    Ok(())
}

#[test]
fn after_a_sleep_past_its_time_it_shows_unless_the_meeting_ran_10_min() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // "Point produit" at 14:30, "Atelier" at 14:20: both ahead at 14:00.
    let fixture = day(
        "\n[[events]]\nid = \"atelier\"\ntitle = \"Atelier\"\nstart = \"today 14:20\"\n\
         end = \"today 15:00\"\n",
    );
    let mut bed = start(&shell, "14:00:00", &fixture, Options::default())?;
    bed.logged_since(0, "the reminders wait", &["at=14:15"])?;
    // Asleep 32 min: the monotonic clock stood still, the wall clock is at
    // 14:32. "Point produit" started 2 min ago: it shows; "Atelier" has run
    // 12 min: it does not.
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Day(DayAsk::Slept(32 * 60)));
    bed.logged_since(mark, "the reminders look again", &["resumed"])?;
    bed.tab_since(
        mark,
        &[
            "«Point produit» 14:30 · a commencé il y a 2\u{a0}min · visio",
            "later=none",
        ],
    )?;
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.tabs(1)?.len(), 1);
    let shows = bed.lines_since(mark, "the reminders show");
    assert!(shows.iter().all(|l| !l.contains("Atelier")), "{shows:?}");
    Ok(())
}

#[test]
fn reminders_get_through_do_not_disturb() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:25:00",
        &day(""),
        Options {
            dnd: true,
            ..Options::default()
        },
    )?;
    bed.logged_since(0, "the history read back", &["dnd=true"])?;
    bed.tab_since(0, &["«Point produit» 14:30 · dans 5\u{a0}min"])?;
    bed.tabs(1)?;
    bed.pixel_near(JOIN_FILL, JOIN_BLUE)?;
    Ok(())
}

#[test]
fn on_a_shared_screen_its_title_is_hidden() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "14:25:00", &day(""), Options::default())?;
    bed.tab_since(0, &["«Point produit»"])?;
    let screen = bed.tabs(1)?[0].1.clone();
    // Shared: "Réunion", its line as it was.
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::SharedScreens(vec![screen]));
    let line = bed.tab_since(mark, &["«Réunion» 14:30 · dans 5\u{a0}min · visio"])?;
    assert!(
        line.contains("name=Réunion, dans 5 minutes, visio"),
        "{line}"
    );
    assert!(!line.contains("Point produit"), "{line}");
    // Shared no more: its title.
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::SharedScreens(Vec::new()));
    bed.tab_since(mark, &["«Point produit»"])?;
    Ok(())
}

#[test]
fn over_a_fullscreen_window_it_shows_8_px_from_the_screens_edge() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "14:25:00", &day(""), Options::default())?;
    bed.tab_since(0, &["«Point produit»"])?;
    let screen = bed.tabs(1)?[0].1.clone();
    bed.hp.tell_shell(ToShell::Fullscreen(vec![screen]));
    let moved = bed.hp.dispatch_until(common::WAIT, |hp| {
        hp.reminder_surfaces()
            .first()
            .is_some_and(|(_, _, r)| r.x == SCREEN.0 - 8 - 340)
    })?;
    assert!(moved, "{:?}", bed.hp.reminder_surfaces());
    let r = bed.tabs(1)?[0].2;
    assert_eq!((r.y, r.width, r.height), (TAB.1, 340, 44));
    bed.hp.tell_shell(ToShell::Fullscreen(Vec::new()));
    let back = bed.hp.dispatch_until(common::WAIT, |hp| {
        hp.reminder_surfaces()
            .first()
            .is_some_and(|(_, _, r)| r.x == TAB.0)
    })?;
    assert!(back, "{:?}", bed.hp.reminder_surfaces());
    Ok(())
}

#[test]
fn ten_minutes_after_the_start_it_goes_to_the_history() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "14:39:45", &day(""), Options::default())?;
    bed.tab_since(0, &["a commencé il y a 9\u{a0}min"])?;
    bed.tabs(1)?;
    let mark = bed.mark();
    bed.logged_since(
        mark,
        "the reminder in the history",
        &["live=true", "ended=false"],
    )?;
    bed.no_tab()?;
    Ok(())
}

#[test]
fn in_the_mini_form_it_comes_out_at_the_marks_height() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:25:00",
        &day(""),
        Options {
            form: Some(FormChoice::Mini),
            ..Options::default()
        },
    )?;
    bed.tab_since(0, &["«Point produit»"])?;
    let r = bed.tabs(1)?[0].2;
    // From the mini bar's left edge (56 wide), centred on the mark (its
    // 16 px button 68 px under the bar's top).
    assert_eq!(
        (r.x, r.y, r.width, r.height),
        (1920 - 8 - 56 - 8 - 340, 8 + 68 + 8 - 22, 340, 44)
    );
    Ok(())
}

#[test]
fn at_rest_nothing_wakes_before_the_next_reminder() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, "14:24:45", &day(""), Options::default())?;
    bed.logged_since(0, "the reminders wait", &["at=14:25"])?;
    common::at_rest(&mut bed.hp)?;
    let mark = bed.mark();
    common::wait(&mut bed.hp, Duration::from_secs(3))?;
    // Nothing looked, nothing drawn, until it is due.
    assert!(
        bed.lines_since(mark, "the reminders look again").is_empty(),
        "{}",
        bed.log()
    );
    assert_eq!(bed.hp.reminder_commits(), 0);
    // Due: woken once, the tab drawn.
    bed.tab_since(mark, &["«Point produit» 14:30 · dans 5\u{a0}min"])?;
    let looks = bed.lines_since(mark, "the reminders look again");
    assert_eq!(looks.len(), 1, "{looks:?}");
    assert!(looks[0].contains("due"), "{looks:?}");
    // Shown, at rest: nothing until its next minute.
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let (commits, mark) = (bed.hp.reminder_commits(), bed.mark());
    common::wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.reminder_commits(), commits, "the tab drew at rest");
    assert!(bed.lines_since(mark, "the reminders look again").is_empty());
    Ok(())
}

/// The pull request's screenshots, the shell on the CPU: the tab with a
/// meeting in 5 min, the same while its screen is shared, and the mini
/// form's, each the bar's top and the tab 2× larger. Written to
/// `$HOCUS_SHOTS` when it names a folder. Run by hand (`--run-ignored
/// only`).
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
    let shoot = |bed: &mut Bed, name: &str| {
        common::at_rest(&mut bed.hp)?;
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let crop = (SCREEN.0 as u32 - 620, 0, 620, 260);
        let top = image::imageops::crop_imm(&image, crop.0, crop.1, crop.2, crop.3).to_image();
        let file = out.join(format!("{name}.png"));
        top.save(&file)?;
        eprintln!("{}", file.display());
        let r = bed.tabs(1)?[0].2;
        let (x, y) = ((r.x - 12).max(0) as u32, (r.y - 12).max(0) as u32);
        let (cw, ch) = ((r.width + 24) as u32, (r.height + 24) as u32);
        let zoomed = image::imageops::resize(
            &image::imageops::crop_imm(&image, x, y, cw, ch).to_image(),
            cw * 2,
            ch * 2,
            image::imageops::FilterType::Nearest,
        );
        let file = out.join(format!("{name}-zoom.png"));
        zoomed.save(&file)?;
        eprintln!("{}", file.display());
        anyhow::Ok(())
    };
    let mut bed = start(&shell, "14:25:00", &day(""), Options::default())?;
    bed.tab_since(0, &["dans 5"])?;
    bed.tabs(1)?;
    shoot(&mut bed, "tab-full")?;
    let screen = bed.tabs(1)?[0].1.clone();
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::SharedScreens(vec![screen]));
    bed.tab_since(mark, &["«Réunion»"])?;
    shoot(&mut bed, "tab-shared")?;
    drop(bed);
    let mut bed = start(
        &shell,
        "14:25:00",
        &day(""),
        Options {
            form: Some(FormChoice::Mini),
            ..Options::default()
        },
    )?;
    bed.tab_since(0, &["dans 5"])?;
    bed.tabs(1)?;
    shoot(&mut bed, "tab-mini")?;
    Ok(())
}
