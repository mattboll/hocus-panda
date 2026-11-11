//! The next meeting in the bar (issue #426, Day.2, variant B): the real
//! shell on the CPU, headless, its day a fixture's file (docs/design/
//! day.md) read against its clock (`HOCUS_CLOCK_AT`, 4 November 2026, in
//! UTC), in French. The shell says what its card shows in its log ("the
//! meeting card is drawn at rest"), which is what these tests read, with
//! the screen's pixels, the compositor's halo and the addresses its
//! `xdg-open` (the test's own) is given.
//!
//! One test per point of the spec that can be measured: which meeting
//! takes the card, "dans 25 min"; none left, no card and the agents' group
//! 58 px higher; within 10 min and 2 min, the card blue and the halo; in
//! progress, "fin dans" and the share gone; at its end, the next one at
//! the minute's change; the card's click and its button's; the mini
//! form's mark and its popover; at rest nothing drawn, the minute's change
//! drawn once.

use crate::common;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use hocus_compositor::{
    Config, FormChoice, Headless, HeadlessConfig, RendererChoice, halo::Concern, model::Rect,
};
use hocus_link::MEETING_NAMESPACE;

/// The calendars, and events that never take the card: all day,
/// declined, free, cancelled, over.
const CALENDARS: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[[calendars]]
id = "perso"
name = "Perso"
color = "#4FC1C9"

[[events]]
title = "Congés de Lou"
date = "today"

[[events]]
title = "Refusée"
start = "now-5m"
end = "now+40m"
answer = "declined"

[[events]]
title = "Créneau libre"
start = "now-2m"
end = "now+40m"
free = true

[[events]]
title = "Annulée"
start = "now+1m"
status = "cancelled"

[[events]]
title = "Déjà finie"
start = "now-90m"
end = "now-30m"
"##;

/// "Point produit" `start` to `end` (fixture times), with a video call and
/// a page; then "Revue de sprint" at 16:00.
fn day(start: &str, end: &str) -> String {
    format!(
        "{CALENDARS}\n\
         [[events]]\nid = \"point\"\ntitle = \"Point produit\"\nstart = \"{start}\"\nend = \"{end}\"\n\
         video = \"https://meet.example.org/abc-defg\"\n\
         page = \"https://calendar.example.org/event/point\"\nplace = \"Salle Jupiter\"\n\n\
         [[events]]\ncalendar = \"perso\"\ntitle = \"Revue de sprint\"\n\
         start = \"today 16:00\"\nend = \"today 17:00\"\n"
    )
}

const WITH_FIXTURE: &str = "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

/// The screen.
const SCREEN: (i32, i32) = (1920, 1080);

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    fixture: PathBuf,
    /// What the shell's `xdg-open` was given, a line each.
    launched: PathBuf,
}

/// The shell's program: the test fails when it is not built.
fn ready() -> Option<PathBuf> {
    Some(common::shell_program())
}

/// The compositor with the real shell on the CPU, its clock at `clock`
/// (today's `HH:MM:SS`), its day `fixture`, its bar in `form`.
fn start(shell: &Path, clock: &str, fixture: &str, form: FormChoice) -> anyhow::Result<Bed> {
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
        lang: hocus_words::Lang::French,
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
    Ok(Bed {
        _dir: dir,
        hp,
        log,
        fixture: fixture_file,
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

    /// Waits until the card is drawn at rest showing all of `holds`.
    fn card_since(&mut self, mark: usize, holds: &[&str]) -> anyhow::Result<String> {
        self.logged_since(mark, "the meeting card is drawn at rest", holds)
    }

    /// The pixel at `(x, y)` of the screen, RGB, once at rest.
    fn pixel(&mut self, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
        common::at_rest(&mut self.hp)?;
        let (w, _, px) = self.hp.screenshot(0)?;
        let at = ((y * w + x) * 4) as usize;
        Ok([px[at + 2], px[at + 1], px[at]])
    }

    /// Waits until the pixel at `(x, y)` is about `colour`: the shell logs
    /// what it draws a moment before the compositor has it.
    fn pixel_near(&mut self, (x, y): (i32, i32), colour: [u8; 3]) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let seen = self.pixel(x, y)?;
            if near(seen, colour) {
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

    /// The bar's colour beside the card, once the card's surface is drawn.
    fn bar_beside(&mut self) -> anyhow::Result<[u8; 3]> {
        self.card_surface()?;
        // The bar there drawn too: before its buffer, the background shows.
        common::drawn(&mut self.hp, hocus_link::NAMESPACE)?;
        let (x, y) = (f64::from(BAR_BESIDE.0), f64::from(BAR_BESIDE.1));
        let drawn = self
            .hp
            .dispatch_until(common::WAIT, |hp| hp.undrawn_at(x, y).is_empty())?;
        anyhow::ensure!(
            drawn,
            "never drawn beside the card: {:?}",
            self.hp.undrawn_at(x, y)
        );
        self.pixel(BAR_BESIDE.0, BAR_BESIDE.1)
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

    /// A click at `at` on the card's `part` (`Card`, `Join`), once the
    /// shell has it under the pointer (it draws it lit): a surface just
    /// shown on a loaded machine may not hear the pointer at once.
    fn click_part(&mut self, at: (i32, i32), part: &str) -> anyhow::Result<()> {
        let mark = self.mark();
        let lit = format!("lit=Some({part})");
        let start = Instant::now();
        loop {
            common::over(
                &mut self.hp,
                (f64::from(at.0), f64::from(at.1)),
                MEETING_NAMESPACE,
            )?;
            common::wait(&mut self.hp, Duration::from_millis(100))?;
            if self
                .lines_since(mark, "the meeting card is drawn at rest")
                .iter()
                .any(|l| l.contains(&lit))
            {
                break;
            }
            anyhow::ensure!(
                start.elapsed() < common::WAIT,
                "the card never drew its {part} lit:\n{}",
                self.log()
            );
            self.hp.pointer_move(400.0, 600.0);
            common::wait(&mut self.hp, Duration::from_millis(50))?;
        }
        self.hp.button(common::BTN_LEFT, true);
        self.hp.button(common::BTN_LEFT, false);
        Ok(())
    }

    /// The card's surface, once drawn.
    fn card_surface(&mut self) -> anyhow::Result<Rect> {
        let drawn = common::drawn(&mut self.hp, MEETING_NAMESPACE)?;
        Ok(drawn[0].1)
    }

    /// The agents' group's surface's top.
    fn agents_top(&self) -> Option<i32> {
        self.hp.agents_surfaces().first().map(|(_, r)| r.y)
    }

    /// Waits until the agents' surface's top is `top`.
    fn agents_at(&mut self, top: i32) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(common::WAIT, |hp| {
            hp.agents_surfaces().first().map(|(_, r)| r.y) == Some(top)
        })?;
        anyhow::ensure!(
            done,
            "the agents' surface at {:?}, not {top}",
            self.agents_top()
        );
        Ok(())
    }

    /// Waits until the halo shows `concern`.
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

/// The full form's card on a 1920 × 1080 screen: 172 × 44, 12 px in from
/// the bar's sides (the bar 196 wide, 8 px from the edge), 100 px under
/// the bar's top.
const CARD: (i32, i32, i32, i32) = (1920 - 8 - 196 + 12, 8 + 100, 172, 44);
/// Its stripe's middle, and its join button's left inside (its fill).
const STRIPE: (i32, i32) = (CARD.0 + 1 + 6 + 3, CARD.1 + 22);
const JOIN_FILL: (i32, i32) = (CARD.0 + 172 - 7 - 30 + 3, CARD.1 + 22);
/// Where the card shows its own colour (above its title), and the bar's
/// beside it.
const CARD_FILL: (i32, i32) = (CARD.0 + 80, CARD.1 + 3);
const BAR_BESIDE: (i32, i32) = (CARD.0 - 5, CARD.1 + 3);

/// `colour` at `alpha` over `under`, as the screen shows it.
fn over(under: [u8; 3], colour: [u8; 3], alpha: f32) -> [u8; 3] {
    [0, 1, 2]
        .map(|i| (f32::from(under[i]) * (1.0 - alpha) + f32::from(colour[i]) * alpha).round() as u8)
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3)
}

const WHITE: [u8; 3] = [255, 255, 255];
const MEETING_BLUE: [u8; 3] = [0x8F, 0xB3, 0xFF];
const JOIN_BLUE: [u8; 3] = [0x5B, 0x8D, 0xEF];

#[test]
fn the_card_is_todays_first_meeting_left_with_its_minutes_and_its_join_button() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:00:00",
        &day("now+25m", "now+55m"),
        FormChoice::Full,
    )?;
    // All day, declined, free, cancelled and over: none takes it.
    let line = bed.card_since(
        0,
        &[
            "«Point produit» 14:25 · dans 25\u{a0}min look=Far join=true page=true",
            "colour=#6F9BFF",
            "form=Some(Full)",
            "name=Prochaine réunion : Point produit, 14:25, dans 25 minutes, visio",
            "join=\"Rejoindre la visio\"",
        ],
    )?;
    assert!(!line.contains("Refusée"), "{line}");
    // Its surface under the clock, the card 5 px in it.
    let surface = bed.card_surface()?;
    assert_eq!(
        (surface.x, surface.y, surface.width, surface.height),
        (1920 - 9 - 194, 8 + 95, 194, 54)
    );
    // The stripe in the calendar's colour; the card's fill and the join
    // button's as the spec says, over the bar.
    bed.pixel_near(STRIPE, [0x6F, 0x9B, 0xFF])?;
    let bar = bed.bar_beside()?;
    let fill = over(bar, WHITE, 0.05);
    bed.pixel_near(CARD_FILL, fill)?;
    bed.pixel_near(JOIN_FILL, over(fill, WHITE, 0.09))?;
    // The column under it (#599): Today's header and its line (the
    // 16:00 review) 14 px under the card, the agents' group 14 px under
    // Today.
    bed.agents_at(8 + 100 + 58 + 18 + 20 + 14 - 5)?;
    // No halo more than 10 min ahead.
    assert_eq!(bed.hp.halo_concern(), None);
    Ok(())
}

#[test]
fn from_an_hour_the_minutes_read_1_h_05() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:00:00",
        &day("now+65m", "now+95m"),
        FormChoice::Full,
    )?;
    bed.card_since(
        0,
        &["«Point produit» 15:05 · dans 1\u{a0}h\u{a0}05 look=Far"],
    )?;
    Ok(())
}

#[test]
fn with_nothing_left_today_no_card_and_the_agents_group_58_px_higher() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // The last meeting ends at 14:00, 10 s after the shell starts.
    let fixture = format!(
        "{CALENDARS}\n[[events]]\ntitle = \"Point produit\"\nstart = \"today 13:30\"\n\
         end = \"today 14:00\"\n"
    );
    let mut bed = start(&shell, "13:59:50", &fixture, FormChoice::Full)?;
    bed.card_since(0, &["«Point produit» fin dans 1\u{a0}min look=Under"])?;
    bed.agents_at(8 + 100 + 58 - 5)?;
    // Its end: no card, its surface gone, the agents' group up 58 px.
    let mark = bed.mark();
    bed.logged_since(mark, "no meeting left today: no card", &[])?;
    let gone = bed.hp.dispatch_until(common::WAIT, |hp| {
        hp.layer_rects(MEETING_NAMESPACE).is_empty()
    })?;
    assert!(gone, "{:?}", bed.hp.layer_rects(MEETING_NAMESPACE));
    bed.agents_at(8 + 100 - 5)?;
    // Where the card was, the bar.
    let bar = bed.pixel(BAR_BESIDE.0, BAR_BESIDE.1)?;
    bed.pixel_near(CARD_FILL, bar)?;
    Ok(())
}

#[test]
fn within_10_min_the_card_turns_blue_with_the_halo_and_within_2_min_more() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // 14:02: in 3 min, then in 2 at the clock's next minute.
    let mut bed = start(
        &shell,
        "13:59:50",
        &day("today 14:02", "today 14:30"),
        FormChoice::Full,
    )?;
    bed.card_since(0, &["«Point produit» 14:02 · dans 3\u{a0}min look=Soon"])?;
    bed.halo(Some(Concern::MeetingSoon))?;
    let bar = bed.bar_beside()?;
    bed.pixel_near(CARD_FILL, over(bar, MEETING_BLUE, 0.12))?;
    // The join button blue.
    bed.pixel_near(JOIN_FILL, JOIN_BLUE)?;
    // At 14:00, 2 min: stronger, the halo at its first priority.
    let mark = bed.mark();
    bed.card_since(mark, &["«Point produit» 14:02 · dans 2\u{a0}min look=Now"])?;
    bed.halo(Some(Concern::MeetingNow))?;
    bed.pixel_near(CARD_FILL, over(bar, MEETING_BLUE, 0.18))?;
    Ok(())
}

#[test]
fn in_progress_it_says_fin_dans_shows_the_share_gone_and_no_halo() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:12:00",
        &day("today 14:00", "today 14:30"),
        FormChoice::Full,
    )?;
    bed.card_since(
        0,
        &["«Point produit» fin dans 18\u{a0}min look=Under elapsed=0.40"],
    )?;
    let bar = bed.bar_beside()?;
    bed.pixel_near(CARD_FILL, over(bar, MEETING_BLUE, 0.09))?;
    // The 2 px line along its bottom: blue to 40 % of its width.
    let line_y = CARD.1 + CARD.3 - 2;
    let inner = (CARD.0 + 1, CARD.2 - 2);
    bed.pixel_near((inner.0 + inner.1 * 30 / 100, line_y), MEETING_BLUE)?;
    let left = bed.pixel(inner.0 + inner.1 * 55 / 100, line_y)?;
    assert!(!near(left, MEETING_BLUE), "{left:?}");
    // It has started: no halo.
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.halo_concern(), None);
    Ok(())
}

#[test]
fn at_its_end_the_next_meeting_takes_the_card_at_the_minutes_change() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // One ends at 14:00, the next starts at 14:20.
    let fixture = format!(
        "{CALENDARS}\n[[events]]\ntitle = \"Atelier\"\nstart = \"today 13:30\"\n\
         end = \"today 14:00\"\n\n[[events]]\ntitle = \"Entretien\"\ncalendar = \"perso\"\n\
         start = \"today 14:20\"\nend = \"today 15:00\"\n"
    );
    let mut bed = start(&shell, "13:59:50", &fixture, FormChoice::Full)?;
    bed.card_since(
        0,
        &[
            "«Atelier» fin dans 1\u{a0}min look=Under",
            "join=false page=false",
        ],
    )?;
    // The shell logs a draw before its buffer comes: counted from once
    // the card's first is there.
    bed.card_surface()?;
    common::at_rest(&mut bed.hp)?;
    let commits = bed.hp.meeting_commits();
    let mark = bed.mark();
    bed.card_since(
        mark,
        &[
            "«Entretien» 14:20 · dans 20\u{a0}min look=Far",
            "colour=#4FC1C9",
        ],
    )?;
    // No join button without a link: the card's fill where it would be.
    let bar = bed.bar_beside()?;
    bed.pixel_near(JOIN_FILL, over(bar, WHITE, 0.05))?;
    // Drawn once for it.
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let drawn = bed.hp.meeting_commits() - commits;
    assert!((1..=2).contains(&drawn), "{drawn} commits of the card");
    Ok(())
}

#[test]
fn the_card_opens_the_calendar_on_its_meeting_and_its_button_the_call_ending_the_halo()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:00:00",
        &day("now+8m", "now+38m"),
        FormChoice::Full,
    )?;
    bed.card_since(0, &["«Point produit» 14:08 · dans 8\u{a0}min look=Soon"])?;
    bed.halo(Some(Concern::MeetingSoon))?;
    // The card, on its title: the calendar (Day.5) on today, the meeting
    // chosen.
    let mark = bed.mark();
    let title = (CARD.0 + 40, CARD.1 + 14);
    bed.click_part(title, "Card")?;
    bed.logged_since(
        mark,
        "the calendar opens",
        &["day=2026-11-04", "focus=Some(Event(EventId(\"point\")))"],
    )?;
    let shown = bed.hp.dispatch_until(common::WAIT, |hp| {
        common::drawn_popovers(hp).iter().any(|p| p.width == 320)
    })?;
    anyhow::ensure!(shown, "no calendar drawn");
    // Opened: its halo ends (§6).
    bed.halo(None)?;
    // The button: the video call.
    let button = (JOIN_FILL.0 + 12, JOIN_FILL.1);
    bed.click_part(button, "Join")?;
    let opened = bed.opened(1)?;
    assert_eq!(opened[0], "https://meet.example.org/abc-defg");
    Ok(())
}

#[test]
fn the_mini_forms_mark_glows_within_10_min_and_opens_its_popover() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:00:00",
        &day("now+8m", "now+38m"),
        FormChoice::Mini,
    )?;
    bed.card_since(0, &["look=Soon", "form=Some(Mini)"])?;
    // Its 48 × 16 button under the clock, the mark in its middle.
    let surface = bed.card_surface()?;
    assert_eq!(
        (surface.x, surface.y, surface.width, surface.height),
        (1920 - 9 - 54, 8 + 63, 54, 26)
    );
    let mark = (1920 - 8 - 28, 8 + 68 + 8);
    bed.pixel_near(mark, MEETING_BLUE)?;
    // The agents' item 30 px lower.
    bed.agents_at(8 + 68 + 30 - 5)?;
    bed.halo(Some(Concern::MeetingSoon))?;
    // A 150 ms hover opens its popover, its top on the mark's.
    let from = bed.mark();
    common::over(
        &mut bed.hp,
        (f64::from(mark.0), f64::from(mark.1)),
        MEETING_NAMESPACE,
    )?;
    let said = bed.logged_since(from, "the meeting's popover shows it", &[])?;
    assert!(
        said.contains(
            "PROCHAINE RÉUNION | Point produit | 14:08 – 14:38 · dans 8\u{a0}min | \
             meet.example.org/abc-defg | Rejoindre | Voir dans le calendrier | \
             ENSUITE 16:00 Revue de sprint"
        ),
        "{said}"
    );
    let shown = bed
        .hp
        .dispatch_until(common::WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
    anyhow::ensure!(shown, "no popover drawn");
    let popover = common::drawn_popovers(&bed.hp)[0];
    assert_eq!(popover.width, 280);
    assert_eq!(popover.y, 8 + 68);
    // "Rejoindre": the call, and the popover closes.
    // Its label, the title's line, the times', the link's, 10 px apart
    // and 14 px in: the button's middle.
    let join = (popover.x + 140, popover.y + 128);
    common::click(&mut bed.hp, (f64::from(join.0), f64::from(join.1)))?;
    let opened = bed.opened(1)?;
    assert_eq!(opened, ["https://meet.example.org/abc-defg"]);
    bed.halo(None)?;
    Ok(())
}

#[test]
fn at_rest_the_card_draws_nothing_and_the_minutes_change_draws_it_once() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:00:52",
        &day("today 14:30", "today 15:00"),
        FormChoice::Full,
    )?;
    bed.card_since(0, &["«Point produit» 14:30 · dans 30\u{a0}min"])?;
    bed.card_surface()?;
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(300))?;
    let commits = bed.hp.meeting_commits();
    let mark = bed.mark();
    common::wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.meeting_commits(), commits, "the card drew at rest");
    assert!(bed.lines_since(mark, "the meeting card").is_empty());
    // 14:01: "dans 29 min", drawn once.
    bed.card_since(mark, &["«Point produit» 14:30 · dans 29\u{a0}min"])?;
    common::wait(&mut bed.hp, Duration::from_secs(1))?;
    let drawn = bed.hp.meeting_commits() - commits;
    assert!((1..=2).contains(&drawn), "{drawn} commits for the minute");
    // A change of the day elsewhere draws nothing of it.
    let commits = bed.hp.meeting_commits();
    let fixture = day("today 14:30", "today 15:00").replace("Revue de sprint", "Démo");
    let new = bed.fixture.with_extension("new");
    std::fs::write(&new, fixture)?;
    std::fs::rename(&new, &bed.fixture)?;
    common::wait(&mut bed.hp, Duration::from_secs(2))?;
    assert_eq!(
        bed.hp.meeting_commits(),
        commits,
        "the card drew for another event"
    );
    Ok(())
}

/// The pull request's screenshots, the shell on the CPU, its clock at
/// 14:00: the bar's top, and the card three times larger, a meeting in 25
/// min, in 8, one in progress, none left; the mini form's popover. Written
/// to `$HOCUS_SHOTS` when it names a folder. Run by hand (`--run-ignored
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
            let (x, y) = ((r.x - 12).max(0) as u32, (r.y - 12).max(0) as u32);
            let (cw, ch) = ((r.width + 24) as u32, (r.height + 24) as u32);
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
    let past = format!(
        "{CALENDARS}\n[[events]]\ntitle = \"Point quotidien\"\nstart = \"today 09:30\"\n\
         end = \"today 09:45\"\n"
    );
    let states = [
        ("soon", day("now+25m", "now+55m"), "dans 25"),
        ("near", day("now+8m", "now+38m"), "dans 8"),
        ("now", day("today 13:48", "today 14:18"), "fin dans 18"),
    ];
    for form in [FormChoice::Full, FormChoice::Mini] {
        let suffix = if form == FormChoice::Full {
            "full"
        } else {
            "mini"
        };
        let crop = (SCREEN.0 as u32 - 420, 0, 420, 320);
        let mut bed = start(&shell, "14:00:00", &states[0].1, form)?;
        for (name, fixture, says) in &states {
            let mark = bed.mark();
            let new = bed.fixture.with_extension("new");
            std::fs::write(&new, fixture)?;
            std::fs::rename(&new, &bed.fixture)?;
            bed.card_since(if *name == "soon" { 0 } else { mark }, &[says])?;
            let card = bed.card_surface()?;
            shoot(&mut bed, &format!("{name}-{suffix}"), crop, Some(card))?;
        }
        if form == FormChoice::Mini {
            // The popover of a meeting in 8 min.
            let mark = bed.mark();
            let new = bed.fixture.with_extension("new");
            std::fs::write(&new, &states[1].1)?;
            std::fs::rename(&new, &bed.fixture)?;
            bed.card_since(mark, &["dans 8"])?;
            let from = bed.mark();
            common::over(&mut bed.hp, (1884.0, 84.0), MEETING_NAMESPACE)?;
            bed.logged_since(from, "the meeting's popover shows it", &[])?;
            bed.hp
                .dispatch_until(common::WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
            shoot(
                &mut bed,
                "popover-mini",
                (SCREEN.0 as u32 - 400, 0, 400, 360),
                None,
            )?;
            bed.hp.pointer_move(400.0, 600.0);
        }
        let mark = bed.mark();
        let new = bed.fixture.with_extension("new");
        std::fs::write(&new, &past)?;
        std::fs::rename(&new, &bed.fixture)?;
        bed.logged_since(mark, "no meeting left today: no card", &[])?;
        bed.hp.dispatch_until(common::WAIT, |hp| {
            hp.layer_rects(MEETING_NAMESPACE).is_empty()
        })?;
        common::wait(&mut bed.hp, Duration::from_millis(500))?;
        shoot(&mut bed, &format!("none-{suffix}"), crop, None)?;
    }
    Ok(())
}

/// The mini form's popover in its main state (a meeting in 8 min, a call,
/// what comes next), shot under `HOCUS_KIT_SHOTS` (#772's side-by-sides;
/// see `wifi::kit_shot`).
#[test]
#[ignore = "writes screenshots under HOCUS_KIT_SHOTS"]
fn kit_shot() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:00:00",
        &day("now+8m", "now+38m"),
        FormChoice::Mini,
    )?;
    bed.card_since(0, &["look=Soon", "form=Some(Mini)"])?;
    let from = bed.mark();
    common::over(&mut bed.hp, (1884.0, 84.0), MEETING_NAMESPACE)?;
    bed.logged_since(from, "the meeting's popover shows it", &[])?;
    bed.hp
        .dispatch_until(common::WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
    common::popover_shot(&mut bed.hp, "meeting")
}

/// The next meeting's card at rest, hovered and ringed, shot under
/// `HOCUS_KIT_SHOTS` (#785's side-by-sides).
#[test]
#[ignore = "writes screenshots under HOCUS_KIT_SHOTS"]
fn bar_shot() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        "14:00:00",
        &day("now+8m", "now+38m"),
        FormChoice::Full,
    )?;
    bed.card_since(0, &["look=Soon"])?;
    let card = bed.card_surface()?;
    let at = (
        f64::from(card.x + card.width / 3),
        f64::from(card.y + card.height / 2),
    );
    let log = bed.log.clone();
    common::bar_shots(
        &mut bed.hp,
        &log,
        (MEETING_NAMESPACE, "meeting"),
        at,
        "Meeting",
    )
}
