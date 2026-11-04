//! The to-do list in the bar (Day.4, issue #428, variant C): the real
//! shell, headless and on the CPU, its tasks from a fixture's board
//! (docs/design/day.md) in a folder of the test's own, its language
//! French, no session bus.
//!
//! One test per point of the approved spec that can be measured: the
//! group's three tasks in the column's order and "+ N autres", which opens
//! the tasks' popover; a tick reaching the source at once, the line going
//! 1.5 s after, a second click within them unticking; a refused change
//! unticking and saying "Pas enregistré" for 3 s; adding through the
//! field, `Enter` and `Esc`, a click elsewhere; the mini form's item, its
//! count, its popover after a 150 ms hover; the keyboard (`Tab`, `Space`,
//! `Enter`); no board, no group, an empty board "Rien à faire."; at rest,
//! nothing drawn.
//!
//! The shell runs on its own clock: what it waits (1.5 s, 3 s, 150 ms) it
//! says in its log as it measured it, and the tests read that, waiting on
//! what the shell says, never on a guessed time. With `HOCUS_SHOTS` naming
//! a folder, each test saves what the bar shows there.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{
    BTN_LEFT, Clip, Command, Event, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_TAB, WAIT, chord,
    wait,
};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};

const KEY_ENTER: u32 = 28;
const KEY_SPACE: u32 = 57;
const KEY_V: u32 = 47;

/// Seven tasks: three in the group, "+ 4 autres".
const SEVEN: &str = r#"
[tasks]
board = "Sprint"

[[tasks.cards]]
id = "t1"
title = "Relire la spec"

[[tasks.cards]]
id = "t2"
title = "Écrire le ticket"

[[tasks.cards]]
id = "t3"
title = "Mettre à jour le changelog et la documentation du paiement"

[[tasks.cards]]
id = "t4"
title = "Préparer la démo"

[[tasks.cards]]
id = "t5"
title = "Réserver la salle"

[[tasks.cards]]
id = "t6"
title = "Envoyer le compte rendu"

[[tasks.cards]]
id = "t7"
title = "Appeler le client"
"#;

const WITH_TASKS: &str = "[[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

struct Bed {
    hp: Headless,
    _dir: tempfile::TempDir,
    log: PathBuf,
    fixture: PathBuf,
    name: &'static str,
}

impl Bed {
    /// The compositor on a `screen` screen with the real shell, its
    /// configuration `config` and, beside it, `day.toml` holding
    /// `fixture`.
    fn start(
        name: &'static str,
        screen: &str,
        config: &str,
        fixture: &str,
    ) -> anyhow::Result<Option<Self>> {
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let settings = root.join("config").join("config.toml");
        std::fs::write(&settings, config)?;
        let fixture_file = root.join("config").join("day.toml");
        std::fs::write(&fixture_file, fixture)?;
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
        let mut settings_config = Config {
            file: Some(settings),
            ..Config::default()
        };
        settings_config.keyboard.layouts = vec!["us".to_owned()];
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec![screen.parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            settings_config,
            Some(script),
        )?;
        let mut bed = Self {
            hp,
            _dir: dir,
            log,
            fixture: fixture_file,
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

    /// Waits until the group shows all of `holds` (after `from`).
    fn group_after(&mut self, from: usize, holds: &[&str]) -> anyhow::Result<String> {
        let mut all = vec!["the tasks' group shows it"];
        all.extend_from_slice(holds);
        self.logged_after(from, &all)
    }

    /// Where the group's `item` is on the screen (its middle), as last
    /// logged, once the surface the compositor has reaches there.
    fn item(&mut self, item: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!(" {item}@");
        let log = self.log.clone();
        let found = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the tasks' items"))
                .and_then(|l| {
                    let l = l.replace("items=", "items= ");
                    let at = l.split(&wanted).nth(1)?;
                    let at = at.split_whitespace().next()?;
                    let (x, y) = at.split_once(',')?;
                    Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
                })
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| found(&plain(&log)).is_some())?;
        anyhow::ensure!(done, "no {item} in the group: {}", plain(&log));
        let (x, y) = found(&plain(&log)).unwrap_or_default();
        let right = f64::from(self.hp.outputs()[0].1.width) - 8.0;
        let at = (right + x, 8.0 + y);
        let inside = |r: &hocus_compositor::model::Rect| {
            at.0 >= f64::from(r.x)
                && at.1 >= f64::from(r.y)
                && at.0 < f64::from(r.x + r.width)
                && at.1 < f64::from(r.y + r.height)
        };
        let reached = self.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(hocus_link::TASKS_NAMESPACE)
                .first()
                .is_some_and(|(_, r)| inside(r))
        })?;
        anyhow::ensure!(
            reached,
            "{item} at {at:?} is off {:?}",
            self.hp.drawn_layers(hocus_link::TASKS_NAMESPACE)
        );
        Ok(at)
    }

    /// A click on the group's `item`.
    fn click_item(&mut self, item: &str) -> anyhow::Result<()> {
        let at = self.item(item)?;
        common::click_on(&mut self.hp, at, hocus_link::TASKS_NAMESPACE)
    }

    /// A click at `at`, once what is there is drawn.
    fn click(&mut self, at: (f64, f64)) -> anyhow::Result<()> {
        common::point(&mut self.hp, at)?;
        wait(&mut self.hp, Duration::from_millis(60))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    fn shift_tab(&mut self) {
        self.hp.key(KEY_LEFTSHIFT, true);
        self.press(KEY_TAB);
        self.hp.key(KEY_LEFTSHIFT, false);
    }

    /// Types `text`, lower-case letters and spaces, on the US layout.
    fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            if let Some(code) = key_code(ch) {
                self.press(code);
            }
        }
    }

    /// Waits for the field to open (after `from`) and hold the keyboard:
    /// keys typed before the compositor gives it go elsewhere.
    fn field_opens(&mut self, from: usize) -> anyhow::Result<()> {
        self.logged_after(from, &["the field to add a task opens"])?;
        self.keyboard_in_field()
    }

    fn keyboard_in_field(&mut self) -> anyhow::Result<()> {
        let held = self.hp.dispatch_until(WAIT, |hp| {
            hp.keyboard_layer().as_deref() == Some(hocus_link::TASKS_NAMESPACE)
        })?;
        anyhow::ensure!(
            held,
            "the field has no keyboard: {:?}",
            self.hp.keyboard_layer()
        );
        Ok(())
    }

    /// Waits for the group's surface to be `height` px tall, drawn.
    fn group_height(&mut self, height: i32) -> anyhow::Result<hocus_compositor::model::Rect> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(hocus_link::TASKS_NAMESPACE)
                .first()
                .is_some_and(|(_, r)| r.height == height)
        })?;
        anyhow::ensure!(
            done,
            "not {height} px: {:?}",
            self.hp.drawn_layers(hocus_link::TASKS_NAMESPACE)
        );
        Ok(self.hp.drawn_layers(hocus_link::TASKS_NAMESPACE)[0].1)
    }

    /// The popover open, once it is there and drawn.
    fn popover(&mut self) -> anyhow::Result<hocus_compositor::model::Rect> {
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

    /// With `HOCUS_SHOTS` naming a folder: the bar's top right as it is
    /// now, the popover's room beside it, saved there as `NAME-what.png`.
    fn shot(&mut self, what: &str) -> anyhow::Result<()> {
        let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
            return Ok(());
        };
        common::at_rest(&mut self.hp)?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let (w, h) = (560.min(width), 560.min(height));
        let x0 = width - w;
        let mut image = image::RgbaImage::new(w as u32, h as u32);
        for y in 0..h {
            for x in 0..w {
                let at = usize::try_from(y * width + x0 + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
            }
        }
        let path = Path::new(&folder).join(format!("{}-{what}.png", self.name));
        image.save(&path)?;
        Ok(())
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

/// A lower-case letter's or a space's key on the US layout.
fn key_code(ch: char) -> Option<u32> {
    const LETTERS: [u32; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    match ch {
        ' ' => Some(KEY_SPACE),
        'a'..='z' => Some(LETTERS[(ch as u8 - b'a') as usize]),
        _ => None,
    }
}

/// The number a line says after `name=`.
fn number(line: &str, name: &str) -> Option<u64> {
    let at = line.find(&format!("{name}="))? + name.len() + 1;
    line[at..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// The rows a "shows it" line gives.
fn rows(line: &str) -> Vec<String> {
    let Some(start) = line.find("rows=[") else {
        return Vec::new();
    };
    let rest = &line[start + 6..];
    let end = rest.find("] more=").unwrap_or(rest.len());
    if rest[..end].is_empty() {
        return Vec::new();
    }
    rest[..end].split("; ").map(str::to_owned).collect()
}

#[test]
fn the_group_shows_three_tasks_in_the_columns_order_and_the_rest_in_its_popover()
-> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("group", "1920x1080", WITH_TASKS, SEVEN)? else {
        return Ok(());
    };
    // The column's order, three of them, the rest under "+ 4 autres".
    let line = bed.group_after(0, &["form=full", "more=+ 4 autres"])?;
    assert_eq!(
        rows(&line),
        [
            "Cocher « Relire la spec »",
            "Cocher « Écrire le ticket »",
            "Cocher « Mettre à jour le changelog et la documentation du paiement »",
        ],
        "{line}"
    );
    assert!(line.contains("add=«Ajouter une tâche»"), "{line}");
    // 18 px of header, three 22 px lines and the 20 px "+ N" line, with
    // 5 px around; 14 px under where the agents' group would be (hidden:
    // nothing followed), the bar inside its rim.
    let rect = bed.group_height(18 + 3 * 22 + 20 + 10)?;
    assert_eq!(rect.width, 194);
    assert_eq!(rect.x, 1920 - 8 - 1 - 194);
    assert_eq!(rect.y, 8 + 100 - 5);
    bed.shot("full")?;
    // The third title is cut: resting on it shows it whole.
    let from = bed.mark();
    let third = bed.item("Task(2)")?;
    common::point(&mut bed.hp, (third.0 + 40.0, third.1))?;
    bed.logged_after(
        from,
        &[
            "a tooltip shows",
            "name=\"Mettre à jour le changelog et la documentation du paiement\"",
        ],
    )?;
    bed.shot("tooltip")?;
    bed.hp.pointer_move(400.0, 300.0);
    // "+ 4 autres" opens the popover beside it, every task, six at once.
    let from = bed.mark();
    let more = bed.item("TasksMore")?;
    bed.click_item("TasksMore")?;
    let pop = bed.popover()?;
    let line = bed.logged_after(from, &["the tasks' popover shows it", "count=7"])?;
    assert_eq!(rows(&line).len(), 6, "{line}");
    assert!(line.contains("more=+ 1 autre "), "{line}");
    // 280 wide, 8 px left of the bar, its top on the line that opened it.
    assert_eq!(pop.width, 280);
    assert_eq!(pop.x + pop.width, 1920 - 8 - 196 - 8);
    assert!(
        (f64::from(pop.y) - (more.1 - 10.0)).abs() <= 1.0,
        "{pop:?} {more:?}"
    );
    bed.shot("popover")?;
    // A click elsewhere closes it.
    bed.click((400.0, 300.0))?;
    bed.closed()?;
    Ok(())
}

#[test]
fn a_tick_reaches_the_source_at_once_and_its_line_goes_after_1_5_s() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("tick", "1920x1080", WITH_TASKS, SEVEN)? else {
        return Ok(());
    };
    bed.group_after(0, &["more=+ 4 autres"])?;
    let from = bed.mark();
    bed.click_item("Task(0)")?;
    bed.logged_after(from, &["a task ticked", "task=\"t1\""])?;
    let ticked = Instant::now();
    // Off the box, for the screenshot.
    bed.hp.pointer_move(400.0, 300.0);
    // The source told at once: the fixture's board no longer lists it.
    bed.logged_after(from, &["the day holds", "tasks=6=["])?;
    // Ticked, struck through, still there.
    let line = bed.group_after(from, &["Décocher « Relire la spec »"])?;
    assert_eq!(rows(&line).len(), 3, "{line}");
    bed.shot("ticked")?;
    if ticked.elapsed() < Duration::from_secs(1) {
        assert!(
            bed.lines_since(from, "a ticked task's line goes")
                .is_empty(),
            "gone within a second"
        );
    }
    // Its line goes 1.5 s after the tick, as the shell measured it; the
    // next comes in at the bottom.
    let gone = bed.logged_after(from, &["a ticked task's line goes", "task=\"t1\""])?;
    let after = number(&gone, "after_ms").unwrap_or_default();
    assert!((1500..2500).contains(&after), "{gone}");
    let line = bed.group_after(from, &["more=+ 3 autres"])?;
    assert_eq!(
        rows(&line),
        [
            "Cocher « Écrire le ticket »",
            "Cocher « Mettre à jour le changelog et la documentation du paiement »",
            "Cocher « Préparer la démo »",
        ],
        "{line}"
    );
    Ok(())
}

#[test]
fn a_second_click_within_1_5_s_unticks_and_tells_the_source_again() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("untick", "1920x1080", WITH_TASKS, SEVEN)? else {
        return Ok(());
    };
    bed.group_after(0, &["more=+ 4 autres"])?;
    let from = bed.mark();
    bed.click_item("Task(0)")?;
    bed.logged_after(from, &["a task ticked", "task=\"t1\""])?;
    bed.logged_after(from, &["the day holds", "tasks=6=["])?;
    bed.click_item("Task(0)")?;
    let untick = bed.logged_after(from, &["a task unticked", "task=\"t1\""])?;
    assert!(
        number(&untick, "after_ms").is_some_and(|ms| ms < 1500),
        "{untick}"
    );
    // Back where it was in the column, and in the group, unticked.
    bed.logged_after(
        from,
        &["the day holds", "tasks=7=[t1 «Relire la spec»", "pending=0"],
    )?;
    let line = bed.group_after(from, &["Cocher « Relire la spec »", "more=+ 4 autres"])?;
    assert_eq!(rows(&line)[0], "Cocher « Relire la spec »", "{line}");
    // Its line stays: past 1.5 s, nothing went.
    wait(&mut bed.hp, Duration::from_secs(2))?;
    assert!(
        bed.lines_since(from, "a ticked task's line goes")
            .is_empty(),
        "{}",
        bed.plain()
    );
    Ok(())
}

#[test]
fn a_refused_change_unticks_and_says_not_saved_for_3_s() -> anyhow::Result<()> {
    let refusing = SEVEN.replace("board = \"Sprint\"", "board = \"Sprint\"\nrefuse = true");
    let Some(mut bed) = Bed::start("refused", "1920x1080", WITH_TASKS, &refusing)? else {
        return Ok(());
    };
    bed.group_after(0, &["more=+ 4 autres"])?;
    let from = bed.mark();
    bed.click_item("Task(0)")?;
    bed.logged_after(from, &["a change of a task was not saved", "task=\"t1\""])?;
    bed.hp.pointer_move(400.0, 300.0);
    // The box unticked, "Pas enregistré" in the title's place.
    let line = bed.group_after(from, &["Cocher « Relire la spec »: Pas enregistré"])?;
    assert_eq!(rows(&line).len(), 3, "{line}");
    bed.shot("not-saved")?;
    // For 3 s, then the title again; the task stays, its change kept to
    // be tried again.
    let ended = bed.logged_after(from, &["\"not saved\" ends", "task=\"t1\""])?;
    let shown = number(&ended, "shown_ms").unwrap_or_default();
    assert!((3000..4000).contains(&shown), "{ended}");
    let line = bed.group_after(from, &["rows=[Cocher « Relire la spec »;"])?;
    assert!(!line.contains("Pas enregistré"), "{line}");
    assert!(
        bed.lines_since(from, "a ticked task's line goes")
            .is_empty(),
        "the refused task's line went"
    );
    let day = bed
        .plain()
        .lines()
        .rev()
        .find(|l| l.contains("the day holds"))
        .unwrap_or_default()
        .to_owned();
    assert!(day.contains("tasks=7=[t1 «Relire la spec»"), "{day}");
    assert!(day.contains("pending=1"), "{day}");
    Ok(())
}

#[test]
fn enter_adds_the_task_at_the_top_and_esc_or_a_click_elsewhere_closes_the_empty_field()
-> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("add", "1920x1080", WITH_TASKS, SEVEN)? else {
        return Ok(());
    };
    bed.group_after(0, &["more=+ 4 autres"])?;
    // `+` opens the field as the first line, holding the keyboard.
    let from = bed.mark();
    bed.click_item("TaskAdd")?;
    bed.field_opens(from)?;
    bed.group_height(18 + 30 + 3 * 22 + 20 + 10)?;
    bed.shot("field-empty")?;
    bed.type_text("acheter du pain");
    bed.logged_after(from, &["the tasks show", "«acheter du pain» focused=true"])?;
    let commits = bed.hp.tasks_commits();
    bed.hp
        .dispatch_until(WAIT, |hp| hp.tasks_commits() > commits)?;
    bed.shot("field")?;
    // `Enter`: at the top of the column, no due date; the third goes into
    // "+ N".
    let from = bed.mark();
    bed.press(KEY_ENTER);
    bed.logged_after(from, &["a task added", "acheter du pain"])?;
    bed.logged_after(from, &["the field to add a task closes", "Enter"])?;
    bed.logged_after(
        from,
        &["the day holds", "tasks=8=[", "«acheter du pain»; t1"],
    )?;
    let line = bed.group_after(from, &["more=+ 5 autres"])?;
    assert_eq!(
        rows(&line),
        [
            "Cocher « acheter du pain »",
            "Cocher « Relire la spec »",
            "Cocher « Écrire le ticket »",
        ],
        "{line}"
    );
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.keyboard_layer().is_none())?;
    assert!(gone, "the keyboard stayed: {:?}", bed.hp.keyboard_layer());
    // `Esc` closes it, adding nothing.
    let from = bed.mark();
    bed.click_item("TaskAdd")?;
    bed.field_opens(from)?;
    bed.type_text("rien");
    bed.logged_after(from, &["the tasks show", "«rien»"])?;
    bed.press(KEY_ESC);
    bed.logged_after(from, &["the field to add a task closes", "Escape"])?;
    // Empty, a click elsewhere closes it.
    let from = bed.mark();
    bed.click_item("TaskAdd")?;
    bed.field_opens(from)?;
    bed.click((400.0, 300.0))?;
    bed.logged_after(
        from,
        &["the field to add a task closes", "a press elsewhere"],
    )?;
    // With text, a click elsewhere keeps it, the keyboard let go; a click
    // in it takes the keyboard again, and `Enter` adds.
    let from = bed.mark();
    bed.click_item("TaskAdd")?;
    bed.field_opens(from)?;
    bed.type_text("lait");
    bed.logged_after(from, &["the tasks show", "«lait» focused=true"])?;
    bed.click((400.0, 300.0))?;
    bed.logged_after(from, &["the tasks show", "«lait» focused=false"])?;
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.keyboard_layer().is_none())?;
    assert!(gone, "the keyboard stayed: {:?}", bed.hp.keyboard_layer());
    let add = bed.item("TaskAdd")?;
    // The field's middle: under the header, across the group.
    bed.click((add.0 - 80.0, add.1 + 9.0 + 2.0 + 13.0))?;
    bed.logged_after(from, &["the tasks show", "«lait» focused=true"])?;
    bed.keyboard_in_field()?;
    bed.press(KEY_ENTER);
    bed.logged_after(from, &["the day holds", "tasks=9=[", "«lait»; "])?;
    assert_eq!(bed.lines_since(from, "a task added").len(), 1);
    Ok(())
}

#[test]
fn ctrl_v_pastes_the_clipboards_text_into_the_field_without_its_line_breaks() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start("paste", "1920x1080", WITH_TASKS, SEVEN)? else {
        return Ok(());
    };
    bed.group_after(0, &["more=+ 4 autres"])?;
    // A window copies a text on two lines.
    let (rect, copier) = common::open_with(&mut bed.hp, "test.copy", (300, 200), false)?;
    bed.click((
        f64::from(rect.x + rect.width / 2),
        f64::from(rect.y + rect.height / 2),
    ))?;
    copier.commands.send(Command::Copy(Clip {
        types: &["text/plain;charset=utf-8", "text/plain"],
        text: "Appeler\nInès\n",
        hang: false,
    }))?;
    let copied = bed.hp.dispatch_until(WAIT, |_| {
        copier.events.try_iter().any(|e| e == Event::Copied)
    })?;
    anyhow::ensure!(copied, "the clipboard was never taken");
    let from = bed.mark();
    bed.click_item("TaskAdd")?;
    bed.field_opens(from)?;
    bed.type_text("a ");
    bed.logged_after(from, &["the tasks show", "«a » focused=true"])?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_V]);
    bed.logged_after(
        from,
        &["the clipboard's text pasted into the field to add a task"],
    )?;
    bed.logged_after(from, &["the tasks show", "«a AppelerInès» focused=true"])?;
    bed.press(KEY_ENTER);
    bed.logged_after(from, &["the day holds", "«a AppelerInès»; t1"])?;
    Ok(())
}

#[test]
fn the_mini_item_counts_the_open_tasks_and_opens_the_popover_after_150_ms() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("mini", "1366x768", WITH_TASKS, SEVEN)? else {
        return Ok(());
    };
    let line = bed.group_after(0, &["form=mini", "count=7"])?;
    assert!(line.contains("item=«À faire : 7 tâches»"), "{line}");
    // 44 × 34 with 5 px around, in the 54 px column, 14 px under where
    // the agents' item would be.
    let rect = bed.group_height(44)?;
    assert_eq!(rect.width, 54);
    assert_eq!(rect.x, 1366 - 8 - 1 - 54);
    assert_eq!(rect.y, 8 + 68 - 5);
    bed.shot("mini")?;
    // Resting on it: after 150 ms, as the shell measured, its popover.
    let from = bed.mark();
    let at = bed.item("Tasks")?;
    common::over(&mut bed.hp, at, hocus_link::TASKS_NAMESPACE)?;
    let hovered = bed.logged_after(from, &["hovered long enough", "item=Tasks"])?;
    assert!(
        number(&hovered, "hovered_ms").is_some_and(|ms| ms >= 150),
        "{hovered}"
    );
    let pop = bed.popover()?;
    let line = bed.logged_after(from, &["the tasks' popover shows it", "count=7"])?;
    assert_eq!(rows(&line).len(), 6, "{line}");
    assert_eq!(pop.x + pop.width, 1366 - 8 - 56 - 8);
    bed.shot("mini-popover")?;
    // Its boxes tick as the group's: the count goes down.
    let first = (
        f64::from(pop.x) + 15.0 - 5.0 + 13.0,
        f64::from(pop.y) + 13.0 + 30.0 + 15.0,
    );
    let from = bed.mark();
    common::point(&mut bed.hp, (first.0, first.1))?;
    wait(&mut bed.hp, Duration::from_millis(60))?;
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    bed.logged_after(from, &["a task ticked", "task=\"t1\""])?;
    bed.logged_after(
        from,
        &[
            "the tasks' popover shows it",
            "count=6",
            "Décocher « Relire la spec »",
        ],
    )?;
    bed.group_after(from, &["form=mini", "count=6"])?;
    // The pointer leaving both: it closes.
    bed.hp.pointer_move(300.0, 300.0);
    bed.hp.pointer_move(301.0, 300.0);
    bed.closed()?;
    Ok(())
}

#[test]
fn tab_goes_through_the_add_and_each_box_space_ticks_and_enter_opens_the_field()
-> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("keys", "1920x1080", WITH_TASKS, SEVEN)? else {
        return Ok(());
    };
    bed.group_after(0, &["more=+ 4 autres"])?;
    // The keyboard comes to the bar's items from a popover: "+ 4 autres"'s,
    // which `Esc` closes, giving it to its line.
    bed.click_item("TasksMore")?;
    bed.popover()?;
    let from = bed.mark();
    bed.press(KEY_ESC);
    bed.closed()?;
    bed.logged_after(from, &["the keyboard on an item", "item=TasksMore"])?;
    // Back up through the boxes to the `+`.
    for item in ["Task(2)", "Task(1)", "Task(0)", "TaskAdd"] {
        let from = bed.mark();
        bed.shift_tab();
        bed.logged_after(from, &["the keyboard on an item", &format!("item={item}")])?;
    }
    // And down: `Space` on the first box ticks it.
    let from = bed.mark();
    let commits = bed.hp.tasks_commits();
    bed.press(KEY_TAB);
    bed.logged_after(from, &["the keyboard on an item", "item=Task(0)"])?;
    // Its ring drawn.
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.tasks_commits() > commits)?;
    assert!(drawn, "the ring was not drawn");
    bed.shot("focused")?;
    bed.press(KEY_SPACE);
    bed.logged_after(from, &["a task ticked", "task=\"t1\""])?;
    bed.logged_after(from, &["the day holds", "tasks=6=["])?;
    // `Enter` on the `+` opens the field, which takes the keys.
    let from = bed.mark();
    bed.shift_tab();
    bed.logged_after(from, &["the keyboard on an item", "item=TaskAdd"])?;
    bed.press(KEY_ENTER);
    bed.field_opens(from)?;
    bed.type_text("ok");
    bed.logged_after(from, &["the tasks show", "«ok» focused=true"])?;
    bed.press(KEY_ENTER);
    bed.logged_after(from, &["the day holds", "«ok»"])?;
    Ok(())
}

#[test]
fn no_board_no_group_and_an_empty_board_says_nothing_to_do() -> anyhow::Result<()> {
    // No board set, and the local list empty: no group, no item.
    let Some(mut bed) = Bed::start("none", "1920x1080", "", "")? else {
        return Ok(());
    };
    let line = bed.group_after(0, &["form=full", "shows=false"])?;
    assert!(line.contains("count=none"), "{line}");
    let one = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::TASKS_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| r.height == 1)
    })?;
    assert!(one, "{:?}", bed.hp.layer_rects(hocus_link::TASKS_NAMESPACE));
    drop(bed);
    // A board with nothing open: its header and "Rien à faire.", so that
    // `+` stays at hand.
    let empty = "[tasks]\nboard = \"Sprint\"\n";
    let Some(mut bed) = Bed::start("empty", "1920x1080", WITH_TASKS, empty)? else {
        return Ok(());
    };
    let line = bed.group_after(0, &["form=full", "shows=true", "empty=true"])?;
    assert!(rows(&line).is_empty(), "{line}");
    bed.group_height(18 + 22 + 10)?;
    bed.shot("nothing")?;
    Ok(())
}

#[test]
fn at_rest_the_list_draws_nothing_and_another_change_of_the_day_neither() -> anyhow::Result<()> {
    let with_events = format!(
        "[[calendars]]\nname = \"Travail\"\n[[events]]\ntitle = \"Point produit\"\n\
         start = \"now+25m\"\n{SEVEN}"
    );
    let config =
        format!("{WITH_TASKS}[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n");
    let Some(mut bed) = Bed::start("rest", "1920x1080", &config, &with_events)? else {
        return Ok(());
    };
    bed.group_after(0, &["more=+ 4 autres"])?;
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let commits = bed.hp.tasks_commits();
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.tasks_commits(), commits, "the list drew at rest");
    // An event changes: the day does, the tasks do not; nothing drawn.
    let from = bed.mark();
    std::fs::write(
        &bed.fixture,
        with_events.replace("Point produit", "Point d'équipe"),
    )?;
    bed.logged_after(from, &["the day holds", "«Point d'équipe»"])?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(
        bed.hp.tasks_commits(),
        commits,
        "the list drew for an event"
    );
    // A task changes: drawn, then at rest again.
    let from = bed.mark();
    std::fs::write(
        &bed.fixture,
        with_events
            .replace("Point produit", "Point d'équipe")
            .replace("Relire la spec", "Relire la maquette"),
    )?;
    bed.group_after(from, &["Cocher « Relire la maquette »"])?;
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.tasks_commits() > commits)?;
    assert!(drawn, "the list did not draw the change");
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let commits = bed.hp.tasks_commits();
    wait(&mut bed.hp, Duration::from_secs(2))?;
    assert_eq!(bed.hp.tasks_commits(), commits, "the list kept drawing");
    Ok(())
}

#[test]
fn the_group_goes_14_px_under_today_when_today_lists_something() -> anyhow::Result<()> {
    // The meeting card, then two events for Today.
    let day = format!(
        "[[calendars]]\nname = \"Travail\"\n\
         [[events]]\ntitle = \"Point produit\"\nstart = \"today 14:25\"\nend = \"today 14:55\"\n\
         [[events]]\ntitle = \"Entretien\"\nstart = \"today 15:00\"\nend = \"today 15:45\"\n\
         [[events]]\ntitle = \"Atelier\"\nstart = \"today 16:30\"\nend = \"today 17:30\"\n{SEVEN}"
    );
    let config =
        format!("{WITH_TASKS}[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n");
    let Some(mut bed) = Bed::start("today", "1920x1080", &config, &day)? else {
        return Ok(());
    };
    bed.group_after(0, &["more=+ 4 autres"])?;
    let today = common::drawn(&mut bed.hp, hocus_link::TODAY_NAMESPACE)?[0].1;
    // Today's group ends 5 px over its surface's bottom; this one starts
    // 14 px under it, 5 px under its surface's top.
    let under = bed.hp.dispatch_until(WAIT, |hp| {
        let today = hp.drawn_layers(hocus_link::TODAY_NAMESPACE);
        let tasks = hp.drawn_layers(hocus_link::TASKS_NAMESPACE);
        match (today.first(), tasks.first()) {
            (Some((_, t)), Some((_, g))) => g.y == t.y + t.height - 5 + 14 - 5 && g.height > 1,
            _ => false,
        }
    })?;
    assert!(
        under,
        "today {today:?}, the list {:?}",
        bed.hp.drawn_layers(hocus_link::TASKS_NAMESPACE)
    );
    bed.shot("under-today")?;
    Ok(())
}
