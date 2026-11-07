//! The bar's column in its order (issue #599): the clock, the next
//! meeting's card, the sign of what is in use, Today, To do, then the
//! agents' group, 14 px apart; in the mini form the mark, the to-do item,
//! then the agents' item. The real shell on the CPU, headless, in French,
//! its day and its tasks a fixture's file (docs/design/day.md) read
//! against its clock (`HOCUS_CLOCK_AT`, 4 November 2026 at 14:00, in UTC),
//! its agents said with the `hocus` command, in a home of the test's own,
//! no session bus.
//!
//! One test per point of the issue: the order and the 14 px in both
//! forms, with and without a meeting, with an empty Today, with no task
//! source; the peek and the briefing beside the agents' group where it
//! now is; on a short screen, Today giving way first, then To do's lines,
//! then the agents' rows. Each waits for what the compositor has drawn to
//! be where it should, never for a time. With `HOCUS_SHOTS` naming a
//! folder, the tests save the bar's top right there.

mod common;

use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use common::WAIT;
use hocus_agents::tools::Places;
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::{
    AGENTS_NAMESPACE, DESKTOPS_NAMESPACE, MEETING_NAMESPACE, TASKS_NAMESPACE, TODAY_NAMESPACE,
};

/// The next meeting's card at 14:25.
const MEETING: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[[events]]
title = "Point produit"
start = "today 14:25"
end = "today 14:55"
video = "https://meet.example.org/point"
"##;

/// Two events after the card: Today's.
const AFTER: &str = r##"
[[events]]
title = "Entretien — poste back-end"
start = "today 15:00"
end = "today 15:45"

[[events]]
title = "Atelier planification T1"
start = "today 16:30"
end = "today 17:30"
"##;

/// A calendar with nothing left today.
const NOTHING_LEFT: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[[events]]
title = "Point quotidien"
start = "today 09:30"
end = "today 09:45"
"##;

/// Five tasks: three lines and "+ 2 autres".
const TASKS: &str = r#"
[tasks]
board = "Sprint"

[[tasks.cards]]
title = "Relire la spec"

[[tasks.cards]]
title = "Écrire le ticket"

[[tasks.cards]]
title = "Préparer la démo"

[[tasks.cards]]
title = "Réserver la salle"

[[tasks.cards]]
title = "Appeler le client"
"#;

const CALENDARS: &str = "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";
const WITH_TASKS: &str = "[[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

/// The test's screen: the full bar's left, its surfaces' (the bar inside
/// its rim) and the mini's.
const SCREEN: (u32, u32) = (1920, 1080);
const FULL_X: i32 = 1920 - 9 - 194;
const MINI_X: i32 = 1920 - 9 - 54;
/// Around each group on its surface: the keyboard's ring.
const ROOM: i32 = 5;
/// The bar's top on the screen, and the column's start under it: under
/// the clock.
const EDGE: i32 = 8;
const FULL_TOP: i32 = 100;
const MINI_TOP: i32 = 68;
const GAP: i32 = 14;

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    hocus: PathBuf,
}

/// The programs the tests need, built beside them.
fn ready() -> Option<(PathBuf, PathBuf)> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    let exe = std::env::current_exe().ok()?;
    let hocus = exe.parent()?.parent()?.join("hocus");
    if !hocus.exists() {
        eprintln!("hocus is not built: build the workspace to run this test");
        return None;
    }
    Some((shell, hocus))
}

/// What the test's machine has.
struct Machine<'a> {
    screen: (u32, u32),
    form: FormChoice,
    /// The day's fixture: its calendars, its events, its tasks.
    day: &'a str,
    /// Whether the configuration names the fixture's tasks.
    tasks: bool,
}

impl Default for Machine<'_> {
    fn default() -> Self {
        Self {
            screen: SCREEN,
            form: FormChoice::Full,
            day: "",
            tasks: true,
        }
    }
}

/// The compositor with the real shell on the CPU, for `machine`.
fn start(machine: &Machine) -> anyhow::Result<Option<Bed>> {
    let Some((shell, hocus)) = ready() else {
        return Ok(None);
    };
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let home = root.join("home");
    for sub in ["home/bin", "config", "state/hocuspanda", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    // Day.1's first-run card set aside ("Plus tard"): the list shows.
    std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
    let settings = root.join("config/config.toml");
    let mut config = CALENDARS.to_owned();
    if machine.tasks {
        config.push_str(WITH_TASKS);
    }
    config.push_str(&format!("\n[panel]\nform = \"{}\"\n", machine.form.name()));
    std::fs::write(&settings, config)?;
    std::fs::write(root.join("config/day.toml"), machine.day)?;
    // This test's process in the test's own `/proc`: no tool's client is
    // looked for in the machine's.
    let mine = home.join("proc").join(std::process::id().to_string());
    std::fs::create_dir_all(&mine)?;
    std::os::unix::fs::symlink(std::env::current_dir()?, mine.join("cwd"))?;
    let log = root.join("shell.log");
    let script = root.join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{config}' XDG_CACHE_HOME='{cache}' \
             XDG_STATE_HOME='{state}' XDG_DATA_HOME='{home}/.local/share' \
             TZ=UTC HOCUS_CLOCK_AT='2026-11-04T14:00:00' \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             PATH='{home}/bin:/usr/bin:/bin' RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            config = root.join("config").display(),
            cache = root.join("cache").display(),
            state = root.join("state").display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut config = Config {
        file: Some(settings),
        agent_places: Some(Places {
            home: home.clone(),
            config_home: home.join(".config"),
            codex_home: home.join(".codex"),
            path: vec![home.join("bin")],
            herdr_socket: home.join(".config/herdr/herdr.sock"),
            processes: home.join("proc"),
            hocus: hocus.clone(),
            state_dir: Some(home.join("state")),
        }),
        plain_background: true,
        french: true,
        ..Config::default()
    };
    config.keyboard.layouts = vec!["us".to_owned()];
    config.panel.form = machine.form;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![format!("{}x{}", machine.screen.0, machine.screen.1).parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    hp.pointer_move(400.0, 600.0);
    Ok(Some(Bed {
        _dir: dir,
        hp,
        log,
        hocus,
    }))
}

/// A group's surface, as the compositor has it drawn: its group's top and
/// bottom on the screen (its ring's room left out), its left and width.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Group {
    top: i32,
    bottom: i32,
    x: i32,
    width: i32,
}

impl Group {
    fn of(r: Rect) -> Self {
        Self {
            top: r.y + ROOM,
            bottom: r.y + r.height - ROOM,
            x: r.x,
            width: r.width,
        }
    }
}

/// Bar `hp`'s group of `namespace` as drawn, if it shows something (a
/// group showing nothing keeps a one-pixel surface).
fn group(hp: &Headless, namespace: &str) -> Option<Group> {
    hp.drawn_layers(namespace)
        .first()
        .filter(|(_, r)| r.height > 1)
        .map(|(_, r)| Group::of(*r))
}

/// Whether `namespaces` show in that order from `start` under the bar's
/// top, each 14 px under the one before, at `x`; and none of `hidden`
/// shows. What is wrong, else.
fn column_is(
    hp: &Headless,
    start: i32,
    x: i32,
    namespaces: &[&str],
    hidden: &[&str],
) -> Result<Vec<Group>, String> {
    let mut groups = Vec::new();
    let mut top = EDGE + start;
    for ns in namespaces {
        let Some(g) = group(hp, ns) else {
            return Err(format!("{ns} not drawn"));
        };
        if g.top != top || g.x != x {
            return Err(format!("{ns} at {g:?}, not at {x}, {top}"));
        }
        top = g.bottom + GAP;
        groups.push(g);
    }
    for ns in hidden {
        if let Some(g) = group(hp, ns) {
            return Err(format!("{ns} shows at {g:?}"));
        }
    }
    Ok(groups)
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

impl Bed {
    /// Waits until the shell logged a line holding all of `holds`: the
    /// last such line.
    fn logged(&mut self, holds: &[&str]) -> anyhow::Result<String> {
        let log = self.log.clone();
        let find = |text: &str| {
            text.lines()
                .rev()
                .find(|l| holds.iter().all(|h| l.contains(h)))
                .map(str::to_owned)
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| find(&plain(&log)).is_some())?;
        anyhow::ensure!(done, "the shell never logged {holds:?}:\n{}", plain(&log));
        Ok(find(&plain(&log)).unwrap_or_default())
    }

    /// `hocus agent NAME STATE`, through the control socket, once the
    /// compositor's model has it.
    fn agent_is(&mut self, name: &str, state: &str) -> anyhow::Result<()> {
        let socket = self
            .hp
            .control_socket()
            .ok_or_else(|| anyhow::anyhow!("no control socket"))?
            .to_path_buf();
        let mut child = Command::new(&self.hocus)
            .arg("--socket")
            .arg(socket)
            .args(["agent", name, state])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.flush()?;
        }
        // The command waits for the compositor's answer: run it meanwhile.
        let done = self
            .hp
            .dispatch_until(WAIT, |_| matches!(child.try_wait(), Ok(Some(_))))?;
        anyhow::ensure!(done, "hocus agent {name} {state} never ended");
        let status = child.wait()?;
        anyhow::ensure!(status.success(), "hocus agent {name} {state}: {status}");
        let known = self.hp.dispatch_until(WAIT, |hp| {
            hp.agents_peek()
                .iter()
                .any(|a| a.name == name && a.state == state)
        })?;
        anyhow::ensure!(known, "{name} not {state}: {:?}", self.hp.agents());
        Ok(())
    }

    /// Waits until the column is `namespaces` in that order from `start`,
    /// none of `hidden` showing: their groups.
    fn column(
        &mut self,
        start: i32,
        x: i32,
        namespaces: &[&str],
        hidden: &[&str],
    ) -> anyhow::Result<Vec<Group>> {
        self.sized(start, x, namespaces, hidden, None)
    }

    /// The same, each group `heights` tall when they are given (once what
    /// it shows is laid out).
    fn sized(
        &mut self,
        start: i32,
        x: i32,
        namespaces: &[&str],
        hidden: &[&str],
        heights: Option<&[i32]>,
    ) -> anyhow::Result<Vec<Group>> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            column_is(hp, start, x, namespaces, hidden).is_ok_and(|groups| {
                heights.is_none_or(|h| {
                    groups
                        .iter()
                        .map(|g| g.bottom - g.top)
                        .eq(h.iter().copied())
                })
            })
        })?;
        let now = column_is(&self.hp, start, x, namespaces, hidden);
        let placed: Vec<_> = namespaces
            .iter()
            .chain(hidden)
            .map(|ns| (ns, self.hp.layer_rects(ns), self.hp.drawn_layers(ns)))
            .collect();
        anyhow::ensure!(
            done,
            "the column: {now:?}\nplaced and drawn: {placed:?}\n{}",
            plain(&self.log)
                .lines()
                .filter(|l| l.contains("DEBUG"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        now.map_err(anyhow::Error::msg)
    }

    /// Waits until the group, as it last logged its items, has agent
    /// `name`'s row's middle at `y` on the screen.
    fn row_middle_at(&mut self, name: &str, y: i32) -> anyhow::Result<()> {
        let id = self
            .hp
            .agents()
            .into_iter()
            .find(|a| a.name == name)
            .map(|a| a.id)
            .ok_or_else(|| anyhow::anyhow!("no agent {name}"))?;
        let item = format!("Agent({id})@");
        let log = self.log.clone();
        let at = || {
            let text = plain(&log);
            let line = text
                .lines()
                .rev()
                .find(|l| l.contains("the agents group's items"))?;
            let at = line.split(&item).nth(1)?.split_whitespace().next()?;
            at.split_once(',')?.1.parse::<f64>().ok()
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| at() == Some(f64::from(y - EDGE)))?;
        anyhow::ensure!(done, "{name}'s row's middle at {:?}, not {y}", at());
        Ok(())
    }

    /// With `HOCUS_SHOTS` naming a folder: the bar's top right (and the
    /// room for its popovers beside it), saved there as `bar-order-WHAT.png`.
    fn shot(&mut self, what: &str) -> anyhow::Result<()> {
        let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
            return Ok(());
        };
        common::at_rest(&mut self.hp)?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let (w, h) = (620.min(width), 900.min(height));
        let x0 = width - w;
        let mut image = image::RgbaImage::new(w as u32, h as u32);
        for y in 0..h {
            for x in 0..w {
                let at = usize::try_from(y * width + x0 + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
            }
        }
        image.save(Path::new(&folder).join(format!("bar-order-{what}.png")))?;
        Ok(())
    }
}

/// Two agents at work: the group's header and two rows.
fn two_agents(bed: &mut Bed) -> anyhow::Result<()> {
    bed.agent_is("build", "working")?;
    bed.agent_is("api", "working")?;
    Ok(())
}

#[test]
fn the_full_form_goes_card_today_to_do_then_the_agents_14_px_apart() -> anyhow::Result<()> {
    let day = format!("{MEETING}{AFTER}{TASKS}");
    let Some(mut bed) = start(&Machine {
        day: &day,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    two_agents(&mut bed)?;
    // Each at its height: the card 44, Today's header and two lines, To
    // do's three lines and "+ 2 autres", the agents' header and two rows.
    let groups = bed.sized(
        FULL_TOP,
        FULL_X,
        &[
            MEETING_NAMESPACE,
            TODAY_NAMESPACE,
            TASKS_NAMESPACE,
            AGENTS_NAMESPACE,
        ],
        &[],
        Some(&[44, 18 + 2 * 20, 18 + 3 * 22 + 20, 60 + 6 + 2 * 46]),
    )?;
    bed.logged(&["the tasks' group shows it", "more=+ 2 autres"])?;
    // The spacer under the agents' group: the desktops further down.
    let desktops = common::drawn(&mut bed.hp, DESKTOPS_NAMESPACE)?[0].1;
    assert!(groups[3].bottom + GAP <= desktops.y + ROOM);
    bed.shot("full")?;
    Ok(())
}

#[test]
fn without_a_meeting_to_do_and_the_agents_start_under_the_clock() -> anyhow::Result<()> {
    let day = format!("{NOTHING_LEFT}{TASKS}");
    let Some(mut bed) = start(&Machine {
        day: &day,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    two_agents(&mut bed)?;
    bed.column(
        FULL_TOP,
        FULL_X,
        &[TASKS_NAMESPACE, AGENTS_NAMESPACE],
        &[MEETING_NAMESPACE, TODAY_NAMESPACE],
    )?;
    Ok(())
}

#[test]
fn with_an_empty_today_the_agents_come_under_to_do_under_the_card() -> anyhow::Result<()> {
    let day = format!("{MEETING}{TASKS}");
    let Some(mut bed) = start(&Machine {
        day: &day,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    two_agents(&mut bed)?;
    bed.column(
        FULL_TOP,
        FULL_X,
        &[MEETING_NAMESPACE, TASKS_NAMESPACE, AGENTS_NAMESPACE],
        &[TODAY_NAMESPACE],
    )?;
    Ok(())
}

#[test]
fn with_no_task_source_the_agents_come_under_today() -> anyhow::Result<()> {
    let day = format!("{MEETING}{AFTER}");
    let Some(mut bed) = start(&Machine {
        day: &day,
        tasks: false,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    two_agents(&mut bed)?;
    bed.column(
        FULL_TOP,
        FULL_X,
        &[MEETING_NAMESPACE, TODAY_NAMESPACE, AGENTS_NAMESPACE],
        &[TASKS_NAMESPACE],
    )?;
    Ok(())
}

#[test]
fn the_mini_form_goes_mark_to_do_item_then_the_agents_item() -> anyhow::Result<()> {
    let Some((_, _)) = ready() else {
        return Ok(());
    };
    let mini = |day: &'static str, tasks: bool| Machine {
        day,
        tasks,
        form: FormChoice::Mini,
        ..Machine::default()
    };
    let with_all: &'static str = Box::leak(format!("{MEETING}{AFTER}{TASKS}").into_boxed_str());
    let no_meeting: &'static str = Box::leak(format!("{NOTHING_LEFT}{TASKS}").into_boxed_str());
    let cases: [(Machine, &[&str], &[&str], &str); 3] = [
        (
            mini(with_all, true),
            &[MEETING_NAMESPACE, TASKS_NAMESPACE, AGENTS_NAMESPACE],
            &[TODAY_NAMESPACE],
            "mini",
        ),
        (
            mini(no_meeting, true),
            &[TASKS_NAMESPACE, AGENTS_NAMESPACE],
            &[MEETING_NAMESPACE, TODAY_NAMESPACE],
            "",
        ),
        (
            mini(with_all, false),
            &[MEETING_NAMESPACE, AGENTS_NAMESPACE],
            &[TODAY_NAMESPACE, TASKS_NAMESPACE],
            "",
        ),
    ];
    for (machine, shown, hidden, shot) in cases {
        let Some(mut bed) = start(&machine)? else {
            return Ok(());
        };
        two_agents(&mut bed)?;
        let groups = bed.column(MINI_TOP, MINI_X, shown, hidden)?;
        let heights: Vec<i32> = groups.iter().map(|g| g.bottom - g.top).collect();
        // The mark's 16 px button, the to-do item's 34, the agents' item.
        let expected: Vec<i32> = shown
            .iter()
            .map(|ns| match *ns {
                MEETING_NAMESPACE => 16,
                TASKS_NAMESPACE => 34,
                _ => heights[heights.len() - 1],
            })
            .collect();
        assert_eq!(heights, expected, "{shown:?}");
        assert!(groups.iter().all(|g| g.width == 54), "{groups:?}");
        if !shot.is_empty() {
            bed.shot(shot)?;
        }
    }
    Ok(())
}

#[test]
fn the_peek_comes_out_beside_its_agents_row_under_to_do() -> anyhow::Result<()> {
    let day = format!("{MEETING}{AFTER}{TASKS}");
    let Some(mut bed) = start(&Machine {
        day: &day,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    two_agents(&mut bed)?;
    let groups = bed.column(
        FULL_TOP,
        FULL_X,
        &[
            MEETING_NAMESPACE,
            TODAY_NAMESPACE,
            TASKS_NAMESPACE,
            AGENTS_NAMESPACE,
        ],
        &[],
    )?;
    bed.agent_is("api", "waiting")?;
    // Waiting first: its row heads the group, under its header.
    let agents = groups[3];
    // Its header 60 px, a waiting agent's card 68 (#649).
    let row = agents.top + 60 + 6 + 34;
    bed.row_middle_at("api", row)?;
    let shown = bed.hp.dispatch_until(WAIT, |hp| {
        hp.peek_commits() > 0 && hp.peek_surfaces().len() == 1 && hp.peek().is_some()
    })?;
    anyhow::ensure!(shown, "no peek: {:?}", bed.hp.peek_surfaces());
    // On the CPU a card, its 200 px panda's middle on the row's, beside the
    // bar.
    let peek = bed.hp.peek_surfaces()[0].1;
    assert_eq!(peek.y + 100, row, "{peek:?}");
    assert!(peek.x + peek.width <= FULL_X, "{peek:?}");
    bed.shot("peek")?;
    Ok(())
}

#[test]
fn her_briefing_opens_beside_the_agents_group_under_to_do() -> anyhow::Result<()> {
    let day = format!("{MEETING}{AFTER}{TASKS}");
    let Some(mut bed) = start(&Machine {
        day: &day,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    two_agents(&mut bed)?;
    let groups = bed.column(
        FULL_TOP,
        FULL_X,
        &[
            MEETING_NAMESPACE,
            TODAY_NAMESPACE,
            TASKS_NAMESPACE,
            AGENTS_NAMESPACE,
        ],
        &[],
    )?;
    let agents = groups[3];
    // A click on her row, the group's head.
    let at = (f64::from(FULL_X + 40), f64::from(agents.top + 22));
    common::click_on(&mut bed.hp, at, AGENTS_NAMESPACE)?;
    let open = bed
        .hp
        .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).len() == 1)?;
    anyhow::ensure!(open, "no briefing: {:?}", bed.hp.popover_surfaces());
    bed.logged(&["a popover opens", "Secretary"])?;
    // Its top on hers, beside the bar.
    let briefing = common::drawn_popovers(&bed.hp)[0];
    assert_eq!(briefing.y, agents.top, "{briefing:?}");
    assert!(briefing.x + briefing.width <= FULL_X, "{briefing:?}");
    Ok(())
}

#[test]
fn on_a_short_screen_today_gives_way_then_to_dos_lines_then_the_agents_rows() -> anyhow::Result<()>
{
    let day = format!("{MEETING}{AFTER}{TASKS}");
    // Room for the agents and To do's three lines, not for Today (the
    // agents' header and rows 52 px taller since #649).
    let Some(mut bed) = start(&Machine {
        screen: (1920, 642),
        day: &day,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    for name in ["build", "api", "docs"] {
        bed.agent_is(name, "working")?;
    }
    // To do's three lines and "+ 2 autres", the agents' three rows.
    let groups = bed.sized(
        FULL_TOP,
        FULL_X,
        &[MEETING_NAMESPACE, TASKS_NAMESPACE, AGENTS_NAMESPACE],
        &[TODAY_NAMESPACE],
        Some(&[44, 18 + 3 * 22 + 20, 60 + 6 + 3 * 46]),
    )?;
    let desktops = common::drawn(&mut bed.hp, DESKTOPS_NAMESPACE)?[0].1;
    let under = desktops.y + ROOM;
    assert!(groups[2].bottom + GAP <= under, "{groups:?} over {under}");
    drop(bed);
    // Shorter: To do's lines go into "+ N" first, then the agents' rows.
    let Some(mut bed) = start(&Machine {
        screen: (1920, 497),
        day: &day,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    for name in ["build", "api", "docs"] {
        bed.agent_is(name, "working")?;
    }
    bed.logged(&["the tasks' group shows it", "rows=[] more=+ 5 autres"])?;
    // The agents' rows as many as their room has, the others in "+ N".
    let line = bed.logged(&["shows=AGENTS 3 ·", "autres"])?;
    // The agents' group laid out again for its room: fewer rows over its
    // "+ N" line (20 px), in place under To do's header and "+ N". Each
    // group is a surface of its own, laid out in turn: the agents' room
    // counts To do without its lines, so the agents may fold while To do
    // still shows a line, the agents under it for a frame or two. The
    // column is read once both are laid out.
    let shown = &[MEETING_NAMESPACE, TASKS_NAMESPACE, AGENTS_NAMESPACE];
    let folded = |hp: &Headless| {
        column_is(hp, FULL_TOP, FULL_X, shown, &[TODAY_NAMESPACE]).is_ok_and(|g| {
            let h = g[2].bottom - g[2].top;
            g[1].bottom - g[1].top == 18 + 20 && h < 60 + 6 + 3 * 46 && (h - 60 - 6 - 20) % 46 == 0
        })
    };
    let done = bed.hp.dispatch_until(WAIT, |hp| folded(hp))?;
    let groups = bed.column(FULL_TOP, FULL_X, shown, &[TODAY_NAMESPACE])?;
    anyhow::ensure!(done, "not folded: {groups:?}\n{line}");
    let desktops = common::drawn(&mut bed.hp, DESKTOPS_NAMESPACE)?[0].1;
    let under = desktops.y + ROOM;
    let agents = groups[2];
    // As many rows as fit over "+ N": one more would not.
    let rows = (agents.bottom - agents.top - 60 - 6 - 20) / 46;
    assert!(rows < 3, "{line}");
    assert!(agents.bottom + GAP <= under, "{agents:?} over {under}");
    assert!(
        agents.bottom + GAP + 46 > under,
        "room left: {agents:?} over {under}"
    );
    assert_eq!(groups[1].bottom - groups[1].top, 18 + 20);
    bed.shot("short")?;
    Ok(())
}
