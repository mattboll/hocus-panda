//! Typing into the shell while its surfaces come (#623): the real shell,
//! headless and on the CPU, in French, its notifications on a D-Bus daemon
//! of the test's own (never the session's), its day and its tasks a
//! fixture's file (docs/design/day.md) read against its clock
//! (`HOCUS_CLOCK_AT`, 4 November 2026, 14:00, in UTC).
//!
//! iced_layershell handed the keys to the shell's surface made last: a
//! bubble, a reminder or the meeting card made while the launcher, the
//! emoji picker or the task field held the keyboard took the typing. The
//! keys now follow the compositor's `wl_keyboard.enter`.
//!
//! One test per surface holding the keyboard, as the acceptance says: the
//! user types while a notification's bubble, the meeting card and a
//! reminder come in turn, and every key reaches the field. For the
//! launcher, the bubble comes before the first key: the compositor gives
//! it the keyboard as soon as it knows it, before the shell's toolkit has
//! it. The compositor never moves the keyboard meanwhile: what is measured
//! is the shell's.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

use crate::common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Child,
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFTMETA, WAIT, chord, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use zbus::OwnedVariant;

const KEY_SPACE: u32 = 57;
const KEY_DOT: u32 = 52;

/// The day: a calendar without events, and a board of two tasks.
const CALENDAR: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[tasks]
board = "Sprint"

[[tasks.cards]]
id = "t1"
title = "Relire la spec"

[[tasks.cards]]
id = "t2"
title = "Écrire le ticket"
"##;

/// A meeting at `start` (the fixture's time), with a video call.
fn meeting(id: &str, title: &str, start: &str, end: &str) -> String {
    format!(
        "\n[[events]]\nid = \"{id}\"\ntitle = \"{title}\"\nstart = \"{start}\"\nend = \"{end}\"\n\
         video = \"https://meet.example.org/abc-defg\"\n"
    )
}

const CONFIG: &str = "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\n\
                      [[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    fixture: PathBuf,
    /// The test's own D-Bus daemon, stopped by its process id.
    bus: Child,
    runtime: tokio::runtime::Runtime,
    client: zbus::Connection,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

/// The shell's program, or `None` (the test skips) when it is not built
/// or there is no `dbus-daemon`.
fn ready() -> Option<PathBuf> {
    let shell = common::shell_program();
    let daemon = std::process::Command::new("sh")
        .args(["-c", "command -v dbus-daemon"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !daemon {
        eprintln!("no dbus-daemon: skipped");
        return None;
    }
    Some(shell)
}

impl Bed {
    /// The compositor on a 1920×1080 screen (the full bar) with the real
    /// shell, serving the notifications on the test's bus, its day
    /// [`CALENDAR`].
    fn start(shell: &Path) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state/hocuspanda", "data"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        // Day.1's first-run card set aside ("Plus tard"): the list shows.
        std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
        let config = root.join("config/config.toml");
        std::fs::write(&config, CONFIG)?;
        let fixture = root.join("config/day.toml");
        std::fs::write(&fixture, CALENDAR)?;
        let (bus, address) = common::private_bus(root)?;
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
                 RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("data"),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut settings = Config {
            file: Some(config),
            ..Config::default()
        };
        settings.keyboard.layouts = vec!["us".to_owned()];
        let mut hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            settings,
            Some(script),
            Some(address.clone()),
        )?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let client =
            runtime.block_on(zbus::connection::Builder::address(address.as_str()).build())?;
        let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(drawn, "no bar");
        hp.pointer_move(400.0, 600.0);
        let mut bed = Self {
            _dir: dir,
            hp,
            log,
            fixture,
            bus,
            runtime,
            client,
        };
        bed.logged_after(0, &["the notifications are served"])?;
        bed.logged_after(0, &["the tasks' group shows it"])?;
        Ok(bed)
    }

    fn mark(&self) -> usize {
        plain(&self.log).len()
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

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// Types `text`, lower-case letters, on the US layout.
    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for ch in text.chars() {
            let code = key_code(ch).ok_or_else(|| anyhow::anyhow!("no key for {ch:?}"))?;
            self.press(code);
        }
        Ok(())
    }

    /// Waits until the compositor gives the keyboard to the shell's
    /// surface of `namespace`.
    fn keyboard_on(&mut self, namespace: &str) -> anyhow::Result<()> {
        let held = self
            .hp
            .dispatch_until(WAIT, |hp| hp.keyboard_layer().as_deref() == Some(namespace))?;
        anyhow::ensure!(
            held,
            "{namespace} has no keyboard: {:?}",
            self.hp.keyboard_layer()
        );
        Ok(())
    }

    /// The keyboard still on the shell's surface of `namespace`, as the
    /// compositor has it.
    fn keyboard_still_on(&self, namespace: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.hp.keyboard_layer().as_deref() == Some(namespace),
            "the keyboard moved to {:?}",
            self.hp.keyboard_layer()
        );
        Ok(())
    }

    /// A notification from Slack; its bubble, once drawn at its place.
    fn bubble(&mut self) -> anyhow::Result<()> {
        let hints: HashMap<&str, OwnedVariant> = HashMap::new();
        let body = (
            "Slack",
            0u32,
            "",
            "Camille dans #design",
            "J'ai mis à jour la maquette du lanceur.",
            Vec::<&str>::new(),
            hints,
            -1i32,
        );
        let client = self.client.clone();
        self.runtime.block_on(async move {
            client
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &body,
                )
                .await
        })?;
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            let bubbles = hp.bubble_surfaces();
            bubbles.len() == 1
                && bubbles
                    .iter()
                    .all(|(namespace, _, r)| hp.drawn_layers(namespace).iter().any(|(_, d)| d == r))
        })?;
        anyhow::ensure!(shown, "no bubble: {:?}", self.hp.bubble_surfaces());
        Ok(())
    }

    /// The day's file holds `events` after [`CALENDAR`], replaced at once
    /// as an editor does.
    fn day(&self, events: &str) -> anyhow::Result<()> {
        let new = self.fixture.with_extension("new");
        std::fs::write(&new, format!("{CALENDAR}{events}"))?;
        std::fs::rename(&new, &self.fixture)?;
        Ok(())
    }

    /// A meeting at 16:00: the meeting card comes, drawn.
    fn meeting_card(&mut self) -> anyhow::Result<()> {
        self.day(&meeting(
            "revue",
            "Revue de sprint",
            "today 16:00",
            "today 17:00",
        ))?;
        common::drawn(&mut self.hp, hocus_link::MEETING_NAMESPACE)?;
        Ok(())
    }

    /// And a meeting in 3 min: its reminder comes, drawn.
    fn reminder(&mut self) -> anyhow::Result<()> {
        let events = format!(
            "{}{}",
            meeting("revue", "Revue de sprint", "today 16:00", "today 17:00"),
            meeting("point", "Point produit", "now+3m", "now+33m"),
        );
        self.day(&events)?;
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            let tabs = hp.reminder_surfaces();
            tabs.len() == 1
                && tabs
                    .iter()
                    .all(|(namespace, _, r)| hp.drawn_layers(namespace).iter().any(|(_, d)| d == r))
        })?;
        anyhow::ensure!(shown, "no reminder: {:?}", self.hp.reminder_surfaces());
        Ok(())
    }

    /// Types while a bubble, the meeting card and a reminder come in turn,
    /// the field of the surface of `namespace` holding the keyboard: each
    /// key reaches it, `shows` the line it then logs. With `bubble_first`,
    /// the bubble comes before the first key.
    fn types_through_new_surfaces(
        &mut self,
        namespace: &str,
        bubble_first: bool,
        shows: impl Fn(&str) -> Vec<String>,
    ) -> anyhow::Result<()> {
        let mut typed = String::new();
        let mut step = |bed: &mut Self, letters: &str, after: &str| -> anyhow::Result<()> {
            let from = bed.mark();
            bed.type_text(letters)?;
            typed.push_str(letters);
            let holds = shows(&typed);
            let holds: Vec<&str> = holds.iter().map(String::as_str).collect();
            bed.logged_after(from, &holds)
                .map_err(|e| e.context(format!("typed {typed:?} after {after}")))?;
            bed.keyboard_still_on(namespace)
        };
        if bubble_first {
            self.bubble()?;
            step(self, "pa", "the bubble")?;
        } else {
            step(self, "pa", "nothing new")?;
            self.bubble()?;
            step(self, "n", "the bubble")?;
        }
        self.meeting_card()?;
        step(self, "d", "the meeting card")?;
        self.reminder()?;
        step(self, "a", "the reminder")?;
        Ok(())
    }
}

/// A lower-case letter's key on the US layout.
fn key_code(ch: char) -> Option<u32> {
    const LETTERS: [u32; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    match ch {
        'a'..='z' => Some(LETTERS[(ch as u8 - b'a') as usize]),
        _ => None,
    }
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
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

#[test]
fn the_launcher_keeps_the_keys_while_a_bubble_the_meeting_card_and_a_reminder_come()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let from = bed.mark();
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_SPACE]);
    let opened = bed.hp.dispatch_until(WAIT, |hp| {
        hp.launcher_commits() > 0 && hp.launcher_surfaces().len() == 1
    })?;
    anyhow::ensure!(opened, "no launcher");
    bed.logged_after(from, &["the launcher shows it"])?;
    bed.keyboard_on(hocus_link::LAUNCHER_NAMESPACE)?;
    // The bubble before the first key: the launcher is entered as it is
    // made, before the shell's toolkit has it.
    bed.types_through_new_surfaces(hocus_link::LAUNCHER_NAMESPACE, true, |typed| {
        vec![
            "the launcher shows it".to_owned(),
            format!("query=\"{typed}\""),
        ]
    })
}

#[test]
fn the_emoji_picker_keeps_the_keys_while_a_bubble_the_meeting_card_and_a_reminder_come()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let from = bed.mark();
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_DOT]);
    let opened = bed.hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::EMOJI_NAMESPACE).len() == 1
    })?;
    anyhow::ensure!(opened, "no emoji picker");
    bed.logged_after(from, &["the emoji picker shows it", "holding=true"])?;
    bed.keyboard_on(hocus_link::EMOJI_NAMESPACE)?;
    bed.types_through_new_surfaces(hocus_link::EMOJI_NAMESPACE, false, |typed| {
        vec![
            "the emoji picker shows it".to_owned(),
            format!("query=\"{typed}\""),
        ]
    })
}

#[test]
fn the_task_field_keeps_the_keys_while_a_bubble_the_meeting_card_and_a_reminder_come()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    // `+` opens the field as the group's first line, holding the keyboard.
    let at = task_add(&mut bed)?;
    let from = bed.mark();
    common::point(&mut bed.hp, at)?;
    wait(&mut bed.hp, Duration::from_millis(60))?;
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    bed.logged_after(from, &["the field to add a task opens"])?;
    bed.keyboard_on(hocus_link::TASKS_NAMESPACE)?;
    bed.types_through_new_surfaces(hocus_link::TASKS_NAMESPACE, false, |typed| {
        vec![
            "the tasks show".to_owned(),
            format!("«{typed}» focused=true"),
        ]
    })
}

/// Where the group's `+` is on the screen (its middle), as last logged,
/// once the surface the compositor has reaches there.
fn task_add(bed: &mut Bed) -> anyhow::Result<(f64, f64)> {
    let log = bed.log.clone();
    let found = |text: &str| {
        text.lines()
            .rev()
            .find(|l| l.contains("the tasks' items"))
            .and_then(|l| {
                let l = l.replace("items=", "items= ");
                let at = l.split(" TaskAdd@").nth(1)?;
                let at = at.split_whitespace().next()?;
                let (x, y) = at.split_once(',')?;
                Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
            })
    };
    let done = bed
        .hp
        .dispatch_until(WAIT, |_| found(&plain(&log)).is_some())?;
    anyhow::ensure!(done, "no TaskAdd in the group: {}", plain(&log));
    let (x, y) = found(&plain(&log)).unwrap_or_default();
    let right = f64::from(bed.hp.outputs()[0].1.width) - 8.0;
    let at = (right + x, 8.0 + y);
    let inside = |r: &hocus_compositor::model::Rect| {
        at.0 >= f64::from(r.x)
            && at.1 >= f64::from(r.y)
            && at.0 < f64::from(r.x + r.width)
            && at.1 < f64::from(r.y + r.height)
    };
    let reached = bed.hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::TASKS_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| inside(r))
    })?;
    anyhow::ensure!(
        reached,
        "TaskAdd at {at:?} is off {:?}",
        bed.hp.drawn_layers(hocus_link::TASKS_NAMESPACE)
    );
    Ok(at)
}
