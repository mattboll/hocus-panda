//! Day.1 (#425, variant B): choosing the calendar and the tasks' sources
//! from the bar. The real shell, headless and on the CPU, in French, its
//! folders and its configuration file the test's own; GNOME Online
//! Accounts faked on a private bus, a Calendar API and a Planka server
//! faked on 127.0.0.1, `gnome-control-center` a script that says how it
//! was run: never the network, the session's bus or the user's files.
//!
//! One test per point of the spec that can be measured: the first-run
//! card shows only without a source; "Plus tard" hides it for good;
//! "Choisir…" opens step 1 beside it; Google's row lists Online Accounts'
//! accounts and goes to step 3, its calendars and their colours, or opens
//! Online Accounts' panel without one, and is disabled without Online
//! Accounts; the Planka step takes a good token (pasted) and says a
//! refused one, no server, a board not shared; a server's address lists
//! its boards; "Terminé" writes the configuration, its other sections and
//! comments kept, and the bar then shows the day; `Tab` and `Esc`; at rest
//! nothing draws. With `HOCUS_SHOTS` naming a folder, each test saves what
//! the bar shows there.

mod common;
#[path = "../../hocus-day/tests/fakes/mod.rs"]
mod fakes;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command as Process,
    time::Duration,
};

use common::{BTN_LEFT, Clip, Command, Event, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_TAB, WAIT};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::{POPOVER_NAMESPACE, TASKS_NAMESPACE};

const KEY_ENTER: u32 = 28;
const KEY_V: u32 = 47;
const GOOD: &str = "tok-good";
/// The good token as the browser's tools copy it, with another cookie.
const PASTED: &str = "accessToken=tok-good; lang=fr\n";

/// What else a test gives the shell.
#[derive(Default)]
struct With<'a> {
    /// A session bus (Online Accounts' and the keyring's).
    bus: Option<&'a str>,
    /// The Calendar API's address.
    api: Option<&'a str>,
    /// "Plus tard" already said.
    later: bool,
    /// `gnome-control-center` in its `PATH`.
    panel: bool,
    /// `day.toml` beside the configuration.
    fixture: Option<&'a str>,
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    config: PathBuf,
    name: &'static str,
}

impl Bed {
    fn start(
        name: &'static str,
        screen: &str,
        config: &str,
        with: With<'_>,
    ) -> anyhow::Result<Option<Self>> {
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state/hocuspanda", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let settings = root.join("config").join("config.toml");
        std::fs::write(&settings, config)?;
        if let Some(fixture) = with.fixture {
            std::fs::write(root.join("config/day.toml"), fixture)?;
        }
        if with.later {
            std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
        }
        if with.panel {
            let panel = root.join("bin/gnome-control-center");
            std::fs::write(
                &panel,
                format!(
                    "#!/bin/sh\necho \"$@\" >> '{}'\n",
                    root.join("panel.args").display()
                ),
            )?;
            std::fs::set_permissions(&panel, std::fs::Permissions::from_mode(0o755))?;
        }
        let log = root.join("shell.log");
        let script = root.join("shell");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 XDG_DATA_DIRS='{}' TZ=UTC HOCUS_CLOCK_AT='2026-11-04T14:00:00' \
                 PATH='{}':\"$PATH\" {} RUST_LOG=warn,hocuspanda_shell=debug,hocus_day=debug\n\
                 echo $$ > '{}'\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("data"),
                at("bin"),
                with.api
                    .map(|api| format!("HOCUS_TEST_GOOGLE_API='{api}'"))
                    .unwrap_or_default(),
                at("shell.pid"),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut settings_config = Config {
            file: Some(settings.clone()),
            ..Config::default()
        };
        settings_config.keyboard.layouts = vec!["us".to_owned()];
        let hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec![screen.parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            settings_config,
            Some(script),
            with.bus.map(str::to_owned),
        )?;
        let mut bed = Self {
            hp,
            dir,
            log,
            config: settings,
            name,
        };
        let up = bed.hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        bed.hp.pointer_move(400.0, 300.0);
        Ok(Some(bed))
    }

    fn plain(&self) -> String {
        plain(&self.log)
    }

    fn mark(&self) -> usize {
        self.plain().len()
    }

    /// Waits for the shell to log a line holding all of `holds` after
    /// `from`: the last such line.
    fn logged_after(&mut self, from: usize, holds: &[&str]) -> anyhow::Result<String> {
        let log = self.log.clone();
        let find = |text: &str| {
            text.lines()
                .rev()
                .find(|l| holds.iter().all(|h| l.contains(h)))
                .map(str::to_owned)
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| plain(&log).get(from..).and_then(find).is_some())?;
        let after = plain(&log).get(from..).unwrap_or_default().to_owned();
        anyhow::ensure!(
            done,
            "the shell never logged {holds:?}; it logged:\n{after}"
        );
        Ok(find(&after).unwrap_or_default())
    }

    /// The lines logged after `from` that hold `what`.
    fn lines_since(&self, from: usize, what: &str) -> Vec<String> {
        self.plain()
            .get(from..)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains(what))
            .map(str::to_owned)
            .collect()
    }

    /// Waits until the to-do list's surface shows all of `holds`.
    fn group_after(&mut self, from: usize, holds: &[&str]) -> anyhow::Result<String> {
        let mut all = vec!["the tasks' group shows it"];
        all.extend_from_slice(holds);
        self.logged_after(from, &all)
    }

    /// Waits until what the popover shows now (its last word) holds all
    /// of `holds`: that line.
    fn shows(&mut self, holds: &[&str]) -> anyhow::Result<String> {
        let log = self.log.clone();
        let last = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the sources' popover shows it"))
                .map(str::to_owned)
        };
        let done = self.hp.dispatch_until(WAIT, |_| {
            last(&plain(&log)).is_some_and(|l| holds.iter().all(|h| l.contains(h)))
        })?;
        let line = last(&plain(&log)).unwrap_or_default();
        anyhow::ensure!(
            done,
            "the popover never showed {holds:?}; it shows:\n{line}"
        );
        Ok(line)
    }

    /// Where the card's `item` is on the screen, as last logged, once its
    /// surface reaches there.
    fn item(&mut self, item: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!(" {item}@");
        let log = self.log.clone();
        let found = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the tasks' items"))
                .and_then(|l| at_of(&l.replace("items=", "items= "), &wanted))
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| found(&plain(&log)).is_some())?;
        anyhow::ensure!(done, "no {item} on the card: {}", plain(&log));
        let (x, y) = found(&plain(&log)).unwrap_or_default();
        let right = f64::from(self.hp.outputs()[0].1.width) - 8.0;
        let at = (right + x, 8.0 + y);
        let reached = self.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(TASKS_NAMESPACE)
                .first()
                .is_some_and(|(_, r)| inside(r, at))
        })?;
        anyhow::ensure!(
            reached,
            "{item} at {at:?} is off {:?}",
            self.hp.drawn_layers(TASKS_NAMESPACE)
        );
        Ok(at)
    }

    fn click_item(&mut self, item: &str) -> anyhow::Result<()> {
        let at = self.item(item)?;
        common::click_on(&mut self.hp, at, TASKS_NAMESPACE)
    }

    /// The popover, once it is there, drawn, at rest.
    fn popover(&mut self) -> anyhow::Result<Rect> {
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).len() == 1)?;
        anyhow::ensure!(open, "no popover: {:?}", self.hp.popover_surfaces());
        common::at_rest(&mut self.hp)?;
        Ok(self.hp.popover_surfaces()[0].1)
    }

    fn closed(&mut self) -> anyhow::Result<()> {
        let closed = self
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
        anyhow::ensure!(closed, "the popover stayed");
        Ok(())
    }

    /// Where the popover's `target` is on the screen, as last logged, once
    /// the popover is drawn at its size.
    fn target(&mut self, target: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!(" {target}@");
        let log = self.log.clone();
        let found = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the sources' popover's targets"))
                .and_then(|l| at_of(&l.replace("targets=", "targets= "), &wanted))
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| found(&plain(&log)).is_some())?;
        anyhow::ensure!(done, "no {target} in the popover: {}", plain(&log));
        let (x, y) = found(&plain(&log)).unwrap_or_default();
        let pop = self.popover()?;
        Ok((f64::from(pop.x) + x, f64::from(pop.y) + y))
    }

    fn click_target(&mut self, target: &str) -> anyhow::Result<()> {
        let at = self.target(target)?;
        common::click_on(&mut self.hp, at, POPOVER_NAMESPACE)
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// Types `text` on the US layout: letters, digits and `.:/-=`.
    fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            let Some((code, shift)) = key_code(ch) else {
                continue;
            };
            if shift {
                self.hp.key(KEY_LEFTSHIFT, true);
            }
            self.press(code);
            if shift {
                self.hp.key(KEY_LEFTSHIFT, false);
            }
        }
    }

    /// The popover open on step 1, from the card's "Choisir…".
    fn open(&mut self) -> anyhow::Result<Rect> {
        self.group_after(0, &["card=true"])?;
        self.click_item("DayChoose")?;
        self.shows(&["step=accounts"])?;
        let held = self.hp.dispatch_until(WAIT, |hp| {
            hp.keyboard_layer().as_deref() == Some(POPOVER_NAMESPACE)
        })?;
        anyhow::ensure!(held, "the popover has no keyboard");
        self.popover()
    }

    /// Step 2, `address` typed and `token` typed, signed in: what the
    /// popover then says.
    fn sign_in(&mut self, address: &str, token: &str) -> anyhow::Result<String> {
        self.click_target("Field(Address)")?;
        self.shows(&["focus=Field(Address)"])?;
        // Ctrl+Backspace clears what was there.
        common::chord(&mut self.hp, &[KEY_LEFTCTRL, 14]);
        self.shows(&["address=«»"])?;
        self.type_text(address);
        self.shows(&[&format!("address=«{address}»")])?;
        self.press(KEY_TAB);
        self.shows(&["focus=Field(Token)"])?;
        common::chord(&mut self.hp, &[KEY_LEFTCTRL, 14]);
        self.shows(&["token=none"])?;
        self.type_text(token);
        self.shows(&["token=typed"])?;
        let from = self.mark();
        self.press(KEY_ENTER);
        self.logged_after(from, &["a Planka board's check"])
    }

    /// With `HOCUS_SHOTS` naming a folder: the bar's top right and the
    /// popover's room, saved there as `day-sources-NAME-what.png`.
    fn shot(&mut self, what: &str) -> anyhow::Result<()> {
        let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
            return Ok(());
        };
        // What the shell said it shows, drawn: its commit follows its word.
        common::wait(&mut self.hp, Duration::from_millis(400))?;
        common::at_rest(&mut self.hp)?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let (w, h) = (600.min(width), 760.min(height));
        let x0 = width - w;
        let mut image = image::RgbaImage::new(w as u32, h as u32);
        for y in 0..h {
            for x in 0..w {
                let at = usize::try_from(y * width + x0 + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
            }
        }
        let path = Path::new(&folder).join(format!("day-sources-{}-{what}.png", self.name));
        image.save(&path)?;
        Ok(())
    }

    /// The shell told to end (SIGTERM, by its own process id): the
    /// compositor starts it again; once linked.
    fn restart_shell(&mut self) -> anyhow::Result<()> {
        let pid_file = self.dir.path().join("shell.pid");
        let pid = std::fs::read_to_string(&pid_file)?.trim().to_owned();
        let killed = Process::new("kill").arg(&pid).status()?;
        anyhow::ensure!(killed.success(), "kill {pid}");
        let again = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&pid_file).is_ok_and(|p| p.trim() != pid)
        })?;
        anyhow::ensure!(again, "the shell was not started again");
        let linked = self.hp.dispatch_until(WAIT, |hp| hp.shell_connected())?;
        anyhow::ensure!(linked, "the shell started again never linked");
        Ok(())
    }
}

fn inside(r: &Rect, at: (f64, f64)) -> bool {
    at.0 >= f64::from(r.x)
        && at.1 >= f64::from(r.y)
        && at.0 < f64::from(r.x + r.width)
        && at.1 < f64::from(r.y + r.height)
}

/// `NAME@X,Y` in a line of names and places: the place.
fn at_of(line: &str, wanted: &str) -> Option<(f64, f64)> {
    let at = line.split(wanted).nth(1)?;
    let at = at.split_whitespace().next()?;
    let (x, y) = at.split_once(',')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A character's key on the US layout, and whether it takes Shift.
fn key_code(ch: char) -> Option<(u32, bool)> {
    const LETTERS: [u32; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    Some(match ch {
        'a'..='z' => (LETTERS[(ch as u8 - b'a') as usize], false),
        'A'..='Z' => (LETTERS[(ch as u8 - b'A') as usize], true),
        '1'..='9' => (u32::from(ch as u8 - b'1') + 2, false),
        '0' => (11, false),
        '-' => (12, false),
        '=' => (13, false),
        '.' => (52, false),
        '/' => (53, false),
        ':' => (39, true),
        _ => return None,
    })
}

/// The rows a "calendars=[…]" or "tasks=[…]" says.
fn listed(line: &str, name: &str) -> Vec<String> {
    let Some(start) = line.find(&format!("{name}=[")) else {
        return Vec::new();
    };
    let rest = &line[start + name.len() + 2..];
    // Up to its own closing bracket: its rows hold some.
    let mut depth = 0usize;
    let end = rest
        .char_indices()
        .find(|(_, c)| match c {
            '[' => {
                depth += 1;
                false
            }
            ']' if depth == 0 => true,
            ']' => {
                depth -= 1;
                false
            }
            _ => false,
        })
        .map_or(rest.len(), |(i, _)| i);
    rest[..end].split("; ").map(str::to_owned).collect()
}

/// Online Accounts on a private bus with Google account `alex@example.org`
/// (when `account`), and a Calendar API with three calendars.
struct Google {
    // Dropped in this order: the fakes, then the bus.
    goa: fakes::FakeGoa,
    api: fakes::FakeApi,
    bus: fakes::PrivateBus,
}

impl Google {
    fn start(dir: &Path, account: bool) -> anyhow::Result<Self> {
        let bus = fakes::PrivateBus::start(dir)?;
        let goa = fakes::FakeGoa::start(&bus.address)?;
        if account {
            goa.add("account_1", "google", "alex@example.org")?;
        }
        let api = fakes::FakeApi::start()?;
        api.with(|api| {
            let mut work = fakes::calendar(
                "alex@example.org",
                "alex@example.org",
                "#16a765",
                "owner",
                true,
            );
            work["summaryOverride"] = serde_json::json!("Travail");
            let family = fakes::calendar("family", "Famille", "#f691b2", "writer", true);
            let birthdays =
                fakes::calendar("birthdays", "Anniversaires", "#9fe1e7", "reader", false);
            api.calendars = vec![work, family, birthdays];
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
            api.events
                .insert("family".to_owned(), (Vec::new(), "s2".to_owned()));
            api.events
                .insert("birthdays".to_owned(), (Vec::new(), "s3".to_owned()));
        });
        Ok(Self { goa, api, bus })
    }
}

/// A configuration of the user's, written by hand.
const MINE: &str = "# My settings, by hand.\n\
                    [recording]\n\
                    format = \"mp4\"   # the small files\n\
                    \n\
                    # Keys.\n\
                    [shortcuts]\n\
                    terminal = \"Super+Return\"\n";

#[test]
fn the_first_run_card_shows_only_without_a_source() -> anyhow::Result<()> {
    // No source, nothing said: the card, where the day's groups go.
    let Some(mut bed) = Bed::start("card", "1920x1080", "", With::default())? else {
        return Ok(());
    };
    let line = bed.group_after(0, &["form=full", "card=true"])?;
    assert!(line.contains("rows=[]"), "{line}");
    bed.item("DayChoose")?;
    bed.item("DayLater")?;
    // 172 wide in the bar's 194 px surface, 14 px under where the agents'
    // group would be; as tall as the card and 5 px around.
    let rect = common::drawn(&mut bed.hp, TASKS_NAMESPACE)?[0].1;
    assert_eq!(rect.width, 194);
    assert_eq!(rect.y, 8 + 100 - 5);
    assert!(rect.height > 150, "{rect:?}");
    bed.shot("full")?;
    drop(bed);
    // A source set: no card, the tasks.
    let Some(mut bed) = Bed::start(
        "sourced",
        "1920x1080",
        "[[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n",
        With {
            fixture: Some("[tasks]\n[[tasks.cards]]\ntitle = \"Relire\"\n"),
            ..With::default()
        },
    )?
    else {
        return Ok(());
    };
    bed.group_after(0, &["form=full", "card=false", "Cocher « Relire »"])?;
    assert!(
        bed.lines_since(0, "card=true").is_empty(),
        "{}",
        bed.plain()
    );
    drop(bed);
    // "Plus tard" said before: no card.
    let Some(mut bed) = Bed::start(
        "later-said",
        "1920x1080",
        "",
        With {
            later: true,
            ..With::default()
        },
    )?
    else {
        return Ok(());
    };
    bed.logged_after(0, &["\"later\" read", "later=true"])?;
    bed.group_after(0, &["form=full", "card=false"])?;
    common::at_rest(&mut bed.hp)?;
    assert!(
        bed.lines_since(0, "card=true").is_empty(),
        "{}",
        bed.plain()
    );
    drop(bed);
    // The mini form shows no card.
    let Some(mut bed) = Bed::start("mini", "1366x768", "", With::default())? else {
        return Ok(());
    };
    bed.group_after(0, &["form=mini", "card=false"])?;
    let one = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(TASKS_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| r.height == 1)
    })?;
    assert!(one, "{:?}", bed.hp.layer_rects(TASKS_NAMESPACE));
    bed.shot("mini")?;
    Ok(())
}

#[test]
fn later_removes_the_card_for_good() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("later", "1920x1080", "", With::default())? else {
        return Ok(());
    };
    bed.group_after(0, &["card=true"])?;
    let from = bed.mark();
    bed.click_item("DayLater")?;
    bed.logged_after(from, &["the day's first-run card: later, for good"])?;
    bed.group_after(from, &["form=full", "card=false"])?;
    bed.logged_after(from, &["\"later\" kept"])?;
    assert!(bed.dir.path().join("state/hocuspanda/day-sources").exists());
    // No popover opened.
    assert!(bed.hp.popover_surfaces().is_empty());
    // For good: the shell started again shows none either.
    let from = bed.mark();
    bed.restart_shell()?;
    bed.logged_after(from, &["\"later\" read", "later=true"])?;
    common::at_rest(&mut bed.hp)?;
    assert!(
        bed.lines_since(from, "card=true").is_empty(),
        "{}",
        bed.plain()
    );
    Ok(())
}

#[test]
fn choose_opens_step_1_beside_the_card_without_online_accounts_google_is_disabled()
-> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("step-1", "1920x1080", "", With::default())? else {
        return Ok(());
    };
    let card = common::drawn(&mut bed.hp, TASKS_NAMESPACE)?[0].1;
    let pop = bed.open()?;
    // 320 wide, 8 px left of the bar, its top on the card's.
    assert_eq!(pop.width, 320);
    assert_eq!(pop.x + pop.width, 1920 - 8 - 196 - 8);
    assert_eq!(pop.y, card.y + 5, "{pop:?} {card:?}");
    // No session bus: Online Accounts missing, Google's row disabled; the
    // Planka row and the note.
    let line = bed.shows(&[
        "step=accounts",
        "google=«Comptes en ligne de GNOME absents»",
        "disabled=true",
        "title=«Ajouter un compte»",
    ])?;
    assert!(line.contains("buttons=«Fermer»"), "{line}");
    let targets = bed.lines_since(0, "the sources' popover's targets");
    let last = targets.last().cloned().unwrap_or_default();
    assert!(
        !last.contains("Online@") && !last.contains("Account("),
        "{last}"
    );
    assert!(last.contains("Planka@"), "{last}");
    // The card's "Choisir…" lit while it shows.
    bed.shot("step-1-no-online-accounts")?;
    // ✕ closes it.
    bed.click_target("Close")?;
    bed.closed()?;
    Ok(())
}

#[test]
fn the_google_row_lists_online_accounts_and_step_3_its_calendars_and_colours() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let google = Google::start(dir.path(), true)?;
    let Some(mut bed) = Bed::start(
        "google",
        "1920x1080",
        MINE,
        With {
            bus: Some(&google.bus.address),
            api: Some(&google.api.address),
            ..With::default()
        },
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(&[
        "step=accounts",
        "google=«Tes réunions et rendez-vous.»",
        "accounts=[alex@example.org]",
    ])?;
    bed.shot("step-1-google")?;
    // Its account: straight to step 3, its calendars as the account marks
    // them, each colour the nearest of the eight.
    bed.click_target("Account(0)")?;
    let line = bed.shows(&[
        "step=show",
        "account=«alex@example.org · Google Agenda»",
        "done=true",
    ])?;
    assert_eq!(
        listed(&line, "calendars"),
        [
            "[x] ★ Travail #6CC287",
            "[x] ★ Famille #F28FB1",
            "[ ] ★ Anniversaires #6FD3D8 lecture seule",
        ],
        "{line}"
    );
    assert!(line.contains("title=«Quoi montrer ?»"), "{line}");
    assert!(line.contains("buttons=«Retour» «Fermer»"), "{line}");
    // The swatch unfolds the eight colours; one chosen folds it.
    bed.click_target("Swatch(0)")?;
    bed.shows(&["colours of Travail"])?;
    bed.shot("step-3-colours")?;
    bed.click_target("Colour(0,3)")?;
    let line = bed.shows(&["[x] ★ Travail #C89BFF"])?;
    assert!(!line.contains("colours of"), "{line}");
    // Unticked, ticked.
    bed.click_target("Calendar(1)")?;
    bed.shows(&["[ ] ★ Famille #F28FB1"])?;
    bed.click_target("Calendar(2)")?;
    bed.shows(&["[x] ★ Anniversaires #6FD3D8"])?;
    bed.shot("step-3-google")?;
    // "Terminé": written, the rest of the file kept, the bar shows the day.
    let from = bed.mark();
    bed.click_target("Done")?;
    bed.closed()?;
    bed.logged_after(from, &["the day's sources written"])?;
    let written = std::fs::read_to_string(&bed.config)?;
    assert!(written.starts_with(MINE), "{written}");
    assert!(written.contains("[[day.calendars]]"), "{written}");
    assert!(
        written.contains("account = \"alex@example.org\""),
        "{written}"
    );
    assert!(
        written.contains("shown = [\"Travail\", \"Anniversaires\"]"),
        "{written}"
    );
    assert!(written.contains("Travail = \"#C89BFF\""), "{written}");
    bed.logged_after(
        from,
        &[
            "the day holds",
            "google-calendars-alex@example.org fresh",
            "today=2=[14:25 «Point produit» (Travail); 16:00 «Revue de sprint» (Travail)]",
        ],
    )?;
    bed.group_after(from, &["form=full", "card=false"])?;
    Ok(())
}

#[test]
fn step_3s_stars_feature_calendars_and_done_writes_featured_keeping_the_files_other_lines()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let google = Google::start(dir.path(), true)?;
    let Some(mut bed) = Bed::start(
        "featured",
        "1920x1080",
        MINE,
        With {
            bus: Some(&google.bus.address),
            api: Some(&google.api.address),
            ..With::default()
        },
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.click_target("Account(0)")?;
    // Every calendar featured at first, its star after its swatch.
    let line = bed.shows(&["step=show", "done=true", "star=«Mettre en avant»"])?;
    assert_eq!(
        listed(&line, "calendars"),
        [
            "[x] ★ Travail #6CC287",
            "[x] ★ Famille #F28FB1",
            "[ ] ★ Anniversaires #6FD3D8 lecture seule",
        ],
        "{line}"
    );
    // Its star put out, lit again, put out.
    bed.click_target("Star(1)")?;
    bed.shows(&["[x] ☆ Famille #F28FB1"])?;
    bed.click_target("Star(1)")?;
    bed.shows(&["[x] ★ Famille #F28FB1"])?;
    bed.click_target("Star(1)")?;
    let line = bed.shows(&["[x] ☆ Famille #F28FB1"])?;
    assert!(line.contains("[x] ★ Travail #6CC287"), "{line}");
    bed.hp.pointer_move(400.0, 700.0);
    bed.shot("step-3-stars")?;
    // "Terminé": `featured` written beside `shown`, the rest of the file
    // kept; a star on a calendar not shown counts for nothing.
    let from = bed.mark();
    bed.click_target("Done")?;
    bed.closed()?;
    bed.logged_after(from, &["the day's sources written"])?;
    let written = std::fs::read_to_string(&bed.config)?;
    assert!(written.starts_with(MINE), "{written}");
    assert!(
        written.contains("shown = [\"Travail\", \"Famille\"]"),
        "{written}"
    );
    assert!(written.contains("featured = [\"Travail\"]"), "{written}");
    // The card is the featured calendar's meeting.
    bed.logged_after(
        from,
        &["the day holds", "google-calendars-alex@example.org fresh"],
    )?;
    bed.logged_after(from, &["the meeting card shows", "Point produit"])?;
    Ok(())
}

#[test]
fn without_an_account_the_google_row_opens_online_accounts_and_comes_back_with_one()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let google = Google::start(dir.path(), false)?;
    let Some(mut bed) = Bed::start(
        "no-account",
        "1920x1080",
        "",
        With {
            bus: Some(&google.bus.address),
            api: Some(&google.api.address),
            panel: true,
            ..With::default()
        },
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(&[
        "step=accounts",
        "google=«Ajoute un compte Google dans les comptes en ligne»",
        "accounts=[]",
        "panel=true",
        "disabled=false",
    ])?;
    bed.shot("step-1-no-account")?;
    // Its row runs Online Accounts' panel, through the compositor.
    let from = bed.mark();
    bed.click_target("Online")?;
    bed.logged_after(from, &["Online Accounts' panel opens"])?;
    let args = bed.dir.path().join("panel.args");
    let ran = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&args).is_ok_and(|a| a.trim() == "online-accounts")
    })?;
    assert!(ran, "{:?}", std::fs::read_to_string(&args));
    // The account added there: the row lists it by itself.
    google.goa.add("account_1", "google", "alex@example.org")?;
    bed.shows(&["step=accounts", "accounts=[alex@example.org]"])?;
    Ok(())
}

#[test]
fn the_planka_step_takes_a_pasted_token_and_says_a_refused_one_no_server_or_a_board_not_shared()
-> anyhow::Result<()> {
    let planka = fakes::FakePlanka::start(GOOD)?;
    let Some(mut bed) = Bed::start("planka", "1920x1080", MINE, With::default())? else {
        return Ok(());
    };
    // A window copies the token, the way it is copied from the browser.
    let (window, copier) = common::open_with(&mut bed.hp, "test.copy", (300, 200), false)?;
    common::click(
        &mut bed.hp,
        (
            f64::from(window.x + window.width / 2),
            f64::from(window.y + window.height / 2),
        ),
    )?;
    copier.commands.send(Command::Copy(Clip {
        types: &["text/plain;charset=utf-8", "text/plain"],
        text: PASTED,
        hang: false,
    }))?;
    let copied = bed.hp.dispatch_until(WAIT, |_| {
        copier.events.try_iter().any(|e| e == Event::Copied)
    })?;
    anyhow::ensure!(copied, "the clipboard was never taken");
    bed.open()?;
    bed.click_target("Planka")?;
    let line = bed.shows(&[
        "step=planka",
        "focus=Field(Address)",
        "title=«Tableau Planka»",
    ])?;
    assert!(line.contains("token=none"), "{line}");
    bed.shot("planka-empty")?;
    let board = format!("{}/boards/1", planka.address);
    // A refused token: said, the token's field orange, what was typed kept.
    let said = bed.sign_in(&board, "tok-bad")?;
    assert!(said.contains("refused=Token"), "{said}");
    let line = bed.shows(&["refused=«Jeton refusé ou expiré : colle un nouveau jeton.»"])?;
    assert!(line.contains(&format!("address=«{board}»")), "{line}");
    assert!(line.contains("token=typed"), "{line}");
    bed.shot("planka-refused")?;
    // No server at the address.
    let nobody = format!("{}/boards/1", fakes::nothing_there()?);
    let said = bed.sign_in(&nobody, GOOD)?;
    assert!(said.contains("refused=NoServer"), "{said}");
    bed.shows(&["refused=«Aucun serveur Planka ne répond à cette adresse.»"])?;
    // A board not shared with the user.
    let said = bed.sign_in(&format!("{}/boards/9", planka.address), GOOD)?;
    assert!(said.contains("refused=NotShared"), "{said}");
    bed.shows(&["refused=«Ce tableau n'existe pas ou ne t'est pas partagé.»"])?;
    bed.shot("planka-not-shared")?;
    // The good one, pasted with Ctrl+V: step 3, the board's columns, TODO's
    // cards counted.
    let from = bed.mark();
    bed.click_target("Field(Address)")?;
    common::chord(&mut bed.hp, &[KEY_LEFTCTRL, 14]);
    bed.type_text(&board);
    bed.press(KEY_TAB);
    bed.shows(&["focus=Field(Token)"])?;
    common::chord(&mut bed.hp, &[KEY_LEFTCTRL, 14]);
    bed.shows(&["token=none"])?;
    common::chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_V]);
    bed.logged_after(from, &["the clipboard's text pasted into the Planka step"])?;
    bed.shows(&["token=typed", "refused=none"])?;
    // Never in the log.
    assert!(!bed.plain().contains(GOOD), "the token was logged");
    let from = bed.mark();
    bed.press(KEY_ENTER);
    bed.logged_after(from, &["a Planka board's check", "board=Sprint"])?;
    let line = bed.shows(&["step=show", "done=true"])?;
    assert!(
        line.contains(&format!(
            "account=«Alex · {}»",
            planka.address.trim_start_matches("http://")
        )),
        "{line}"
    );
    assert_eq!(
        listed(&line, "tasks"),
        [
            "board «Sprint»",
            "Colonne des tâches «TODO»",
            "Colonne des terminées «Done»",
            "Ajout dans «TODO»",
            "2 cartes",
        ],
        "{line}"
    );
    bed.shot("step-3-planka")?;
    // Its menus: the done column's lists the board's columns.
    bed.click_target("Choice(1)")?;
    let line = bed.shows(&["option «Backlog»"])?;
    assert!(line.contains("option «TODO»; option «Done»"), "{line}");
    bed.shot("step-3-planka-menu")?;
    bed.click_target("Option(1,2)")?;
    bed.shows(&["Colonne des terminées «Done»; Ajout dans"])?;
    // "Terminé": the board written, its token kept apart from the file,
    // the bar shows its tasks.
    let from = bed.mark();
    bed.click_target("Done")?;
    bed.closed()?;
    bed.logged_after(from, &["the board's token kept"])?;
    bed.logged_after(from, &["the day's sources written"])?;
    let written = std::fs::read_to_string(&bed.config)?;
    assert!(written.starts_with(MINE), "{written}");
    assert!(
        written.contains(&format!(
            "[[day.tasks]]\nkind = \"planka\"\naddress = \"{board}\""
        )),
        "{written}"
    );
    assert!(written.contains("tasks = \"TODO\""), "{written}");
    assert!(written.contains("done = \"Done\""), "{written}");
    assert!(!written.contains(GOOD), "the token went into the file");
    let accounts = bed.dir.path().join("state/hocuspanda/accounts");
    let kept: Vec<String> = std::fs::read_dir(&accounts)?
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect();
    assert_eq!(kept, [format!("accessToken={GOOD}; lang=fr")], "{kept:?}");
    bed.logged_after(
        from,
        &[
            "the day holds",
            "fresh",
            "tasks=2=[c1 «Relire la spec»; c2 «Écrire le ticket»]",
        ],
    )?;
    let line = bed.group_after(
        from,
        &["form=full", "card=false", "Cocher « Relire la spec »"],
    )?;
    assert!(line.contains("Cocher « Écrire le ticket »"), "{line}");
    bed.shot("after")?;
    Ok(())
}

#[test]
fn a_servers_address_alone_lists_its_boards() -> anyhow::Result<()> {
    let planka = fakes::FakePlanka::start(GOOD)?;
    let Some(mut bed) = Bed::start("boards", "1920x1080", "", With::default())? else {
        return Ok(());
    };
    bed.open()?;
    bed.click_target("Planka")?;
    bed.shows(&["step=planka"])?;
    let said = bed.sign_in(&planka.address, GOOD)?;
    assert!(said.contains("board=none boards=1"), "{said}");
    let line = bed.shows(&["step=show", "board «Choisir…»", "done=false"])?;
    assert!(!line.contains("Colonne"), "{line}");
    bed.click_target("Board")?;
    bed.shows(&["option «Équipe › Sprint»"])?;
    bed.click_target("BoardOption(0)")?;
    let line = bed.shows(&["board «Sprint»", "Colonne des tâches «TODO»", "done=true"])?;
    assert!(line.contains("2 cartes"), "{line}");
    Ok(())
}

#[test]
fn tab_goes_through_the_rows_and_esc_goes_back_a_step_then_closes() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("keys", "1920x1080", "", With::default())? else {
        return Ok(());
    };
    bed.open()?;
    // Google's row disabled (no Online Accounts): Tab goes to Planka's,
    // then ✕, and round.
    bed.press(KEY_TAB);
    bed.shows(&["step=accounts", "focus=Planka"])?;
    bed.press(KEY_TAB);
    bed.shows(&["focus=Close"])?;
    bed.hp.key(KEY_LEFTSHIFT, true);
    bed.press(KEY_TAB);
    bed.hp.key(KEY_LEFTSHIFT, false);
    bed.shows(&["focus=Planka"])?;
    bed.shot("keys-focus")?;
    // Enter chooses it: the address's field holds the keyboard; Tab goes
    // to the token's.
    bed.press(KEY_ENTER);
    bed.shows(&["step=planka", "focus=Field(Address)"])?;
    bed.press(KEY_TAB);
    bed.shows(&["step=planka", "focus=Field(Token)"])?;
    // Esc: back to step 1, then the popover closes, the keyboard on the
    // card's "Choisir…".
    bed.press(KEY_ESC);
    bed.shows(&["step=accounts"])?;
    let from = bed.mark();
    bed.press(KEY_ESC);
    bed.closed()?;
    bed.logged_after(from, &["the keyboard on an item", "item=DayChoose"])?;
    Ok(())
}

#[test]
fn at_rest_the_card_and_the_popover_draw_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("rest", "1920x1080", "", With::default())? else {
        return Ok(());
    };
    bed.group_after(0, &["card=true"])?;
    common::drawn(&mut bed.hp, TASKS_NAMESPACE)?;
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let commits = bed.hp.tasks_commits();
    common::wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.tasks_commits(), commits, "the card drew at rest");
    // The popover open, at rest: neither draws.
    bed.open()?;
    bed.hp.pointer_move(400.0, 300.0);
    common::at_rest(&mut bed.hp)?;
    common::wait(&mut bed.hp, Duration::from_millis(500))?;
    let (card, popover) = (bed.hp.tasks_commits(), bed.hp.popover_commits());
    common::wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.tasks_commits(), card, "the card drew at rest");
    assert_eq!(
        bed.hp.popover_commits(),
        popover,
        "the popover drew at rest"
    );
    // A press elsewhere closes it.
    common::click(&mut bed.hp, (400.0, 300.0))?;
    bed.closed()?;
    let _ = BTN_LEFT;
    Ok(())
}
