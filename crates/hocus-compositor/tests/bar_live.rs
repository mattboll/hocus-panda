//! The bar follows `[panel]` as the file changes (6.4, issue #404): which
//! groups of its column show and the order of Today, the to-do list and
//! the agents, in both forms; the clock's look; the screens with a bar;
//! each screen's form, a changed choice forgetting the one made with a
//! click. The Settings' "La barre" page writes these keys; here they are
//! written by hand, as the page's worker would.
//!
//! The real shell on the CPU where the bar's drawing is checked,
//! headless, in French, its day and its tasks a fixture's file read
//! against its clock (`HOCUS_CLOCK_AT`, 4 November 2026 at 14:00, in
//! UTC), its agents said with the `hocus` command, in a home of the
//! test's own, no session bus. Each test waits for what it checks, never
//! for a time.

mod common;

use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use common::{KEY_LEFTMETA, WAIT, chord};
use hocus_agents::tools::Places;
use hocus_compositor::{
    BarForm, Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect,
};
use hocus_link::{AGENTS_NAMESPACE, MEETING_NAMESPACE, TASKS_NAMESPACE, TODAY_NAMESPACE};

const KEY_B: u32 = 48;

/// The next meeting's card at 14:25, two events after it, five tasks.
const DAY: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[[events]]
title = "Point produit"
start = "today 14:25"
end = "today 14:55"
video = "https://meet.example.org/point"

[[events]]
title = "Entretien — poste back-end"
start = "today 15:00"
end = "today 15:45"

[[events]]
title = "Atelier planification T1"
start = "today 16:30"
end = "today 17:30"

[tasks]
board = "Sprint"

[[tasks.cards]]
title = "Relire la spec"

[[tasks.cards]]
title = "Écrire le ticket"

[[tasks.cards]]
title = "Préparer la démo"
"##;

/// The day's sources, as every file of these tests starts.
const SOURCES: &str = "[keyboard]\nlayouts = [\"us\"]\n\n[[day.calendars]]\nkind = \"fixture\"\n\
                       file = \"day.toml\"\n\n[[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

/// The full bar's left on a 1920 px screen, the mini's; the bar's top,
/// the column's start under it in each form, the ring's room around each
/// group, and the 14 px between them.
const FULL_X: i32 = 1920 - 9 - 194;
const MINI_X: i32 = 1920 - 9 - 54;
const EDGE: i32 = 8;
const FULL_TOP: i32 = 100;
const MINI_TOP: i32 = 68;
const ROOM: i32 = 5;
const GAP: i32 = 14;

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    hocus: PathBuf,
    file: PathBuf,
}

/// The compositor with the real shell on the CPU on a 1920 × 1080 screen,
/// its file `panel` after the day's sources; none when the programs are
/// not built.
fn start(panel: &str) -> anyhow::Result<Option<Bed>> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return Ok(None);
    };
    let exe = std::env::current_exe()?;
    let Some(hocus) = exe
        .parent()
        .and_then(Path::parent)
        .map(|d| d.join("hocus"))
        .filter(|h| h.exists())
    else {
        eprintln!("hocus is not built: build the workspace to run this test");
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
    let file = root.join("config/config.toml");
    std::fs::write(&file, format!("{SOURCES}{panel}"))?;
    std::fs::write(root.join("config/day.toml"), DAY)?;
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
        file: Some(file.clone()),
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
    // As the file says it at first: the compositor reads it at once.
    config.panel = toml::from_str::<Config>(&format!("{SOURCES}{panel}"))?.panel;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    hp.pointer_move(400.0, 600.0);
    let mut bed = Bed {
        _dir: dir,
        hp,
        log,
        hocus,
        file,
    };
    bed.agent_is("build", "working")?;
    Ok(Some(bed))
}

/// A group's surface as the compositor has it drawn: its group's top and
/// bottom on the screen (its ring's room left out), and its left.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Group {
    top: i32,
    bottom: i32,
    x: i32,
}

/// `namespace`'s group as drawn, if it shows something (a group showing
/// nothing keeps a one-pixel surface).
fn group(hp: &Headless, namespace: &str) -> Option<Group> {
    hp.drawn_layers(namespace)
        .first()
        .filter(|(_, r)| r.height > 1)
        .map(|(_, r)| Group {
            top: r.y + ROOM,
            bottom: r.y + r.height - ROOM,
            x: r.x,
        })
}

/// Whether `namespaces` show in that order from `start` under the bar's
/// top, each 14 px under the one before, at `x`, and none of `hidden`
/// shows; what is wrong, else.
fn column_is(
    hp: &Headless,
    start: i32,
    x: i32,
    namespaces: &[&str],
    hidden: &[&str],
) -> Result<(), String> {
    let mut top = EDGE + start;
    for ns in namespaces {
        let Some(g) = group(hp, ns) else {
            return Err(format!("{ns} not drawn"));
        };
        if g.top != top || g.x != x {
            return Err(format!("{ns} at {g:?}, not at {x}, {top}"));
        }
        top = g.bottom + GAP;
    }
    match hidden.iter().find_map(|ns| group(hp, ns).map(|g| (ns, g))) {
        Some((ns, g)) => Err(format!("{ns} shows at {g:?}")),
        None => Ok(()),
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

impl Bed {
    /// The file becomes the day's sources and `panel`, as the Settings
    /// write it: beside itself, then renamed over.
    fn write(&self, panel: &str) -> anyhow::Result<()> {
        let new = self.file.with_extension("new");
        std::fs::write(&new, format!("{SOURCES}{panel}"))?;
        std::fs::rename(&new, &self.file)?;
        Ok(())
    }

    /// Waits until the column is `namespaces` in that order from `start`
    /// at `x`, none of `hidden` showing.
    fn column(
        &mut self,
        start: i32,
        x: i32,
        namespaces: &[&str],
        hidden: &[&str],
    ) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            column_is(hp, start, x, namespaces, hidden).is_ok()
        })?;
        let now = column_is(&self.hp, start, x, namespaces, hidden);
        anyhow::ensure!(done, "the column: {now:?}\n{}", plain(&self.log));
        Ok(())
    }

    /// Waits until the shell logged a line holding all of `holds`.
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
        anyhow::ensure!(child.wait()?.success(), "hocus agent {name} {state}");
        let known = self.hp.dispatch_until(WAIT, |hp| {
            hp.agents_peek()
                .iter()
                .any(|a| a.name == name && a.state == state)
        })?;
        anyhow::ensure!(known, "{name} not {state}");
        Ok(())
    }

    /// The screen's pixels in `rect`, as red, green, blue.
    fn pixels(&mut self, rect: Rect) -> anyhow::Result<Vec<[u8; 3]>> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let mut out = Vec::new();
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let at = usize::try_from(y * width + x)? * 4;
                if let Some(&[b, g, r, _]) = bytes.get(at..at + 4) {
                    out.push([r, g, b]);
                }
            }
        }
        Ok(out)
    }
}

const ALL: [&str; 4] = [
    MEETING_NAMESPACE,
    TODAY_NAMESPACE,
    TASKS_NAMESPACE,
    AGENTS_NAMESPACE,
];

#[test]
fn hiding_today_and_the_agents_gives_their_room_back_and_showing_them_brings_them_back()
-> anyhow::Result<()> {
    let Some(mut bed) = start("\n[panel]\nform = \"full\"\n")? else {
        return Ok(());
    };
    bed.column(FULL_TOP, FULL_X, &ALL, &[])?;
    bed.write("\n[panel]\nform = \"full\"\ntoday = false\nagents = false\n")?;
    bed.column(
        FULL_TOP,
        FULL_X,
        &[MEETING_NAMESPACE, TASKS_NAMESPACE],
        &[TODAY_NAMESPACE, AGENTS_NAMESPACE],
    )?;
    // To do hidden too; the agents back, under the card.
    bed.write("\n[panel]\nform = \"full\"\ntoday = false\ntasks = false\n")?;
    bed.column(
        FULL_TOP,
        FULL_X,
        &[MEETING_NAMESPACE, AGENTS_NAMESPACE],
        &[TODAY_NAMESPACE, TASKS_NAMESPACE],
    )?;
    bed.write("\n[panel]\nform = \"full\"\n")?;
    bed.column(FULL_TOP, FULL_X, &ALL, &[])?;
    Ok(())
}

#[test]
fn hiding_the_meeting_takes_its_card_and_its_mark_away() -> anyhow::Result<()> {
    let Some(mut bed) = start("\n[panel]\nform = \"full\"\nmeeting = false\n")? else {
        return Ok(());
    };
    bed.column(
        FULL_TOP,
        FULL_X,
        &[TODAY_NAMESPACE, TASKS_NAMESPACE, AGENTS_NAMESPACE],
        &[MEETING_NAMESPACE],
    )?;
    bed.write("\n[panel]\nform = \"mini\"\nmeeting = false\n")?;
    bed.column(
        MINI_TOP,
        MINI_X,
        &[TASKS_NAMESPACE, AGENTS_NAMESPACE],
        &[MEETING_NAMESPACE, TODAY_NAMESPACE],
    )?;
    bed.write("\n[panel]\nform = \"mini\"\n")?;
    bed.column(
        MINI_TOP,
        MINI_X,
        &[MEETING_NAMESPACE, TASKS_NAMESPACE, AGENTS_NAMESPACE],
        &[TODAY_NAMESPACE],
    )?;
    Ok(())
}

#[test]
fn the_order_moves_the_groups_in_both_forms() -> anyhow::Result<()> {
    let Some(mut bed) = start("\n[panel]\nform = \"full\"\n")? else {
        return Ok(());
    };
    bed.column(FULL_TOP, FULL_X, &ALL, &[])?;
    bed.write("\n[panel]\nform = \"full\"\norder = [\"agents\", \"tasks\", \"today\"]\n")?;
    bed.column(
        FULL_TOP,
        FULL_X,
        &[
            MEETING_NAMESPACE,
            AGENTS_NAMESPACE,
            TASKS_NAMESPACE,
            TODAY_NAMESPACE,
        ],
        &[],
    )?;
    // The mini form the same, Today not in it.
    bed.write("\n[panel]\nform = \"mini\"\norder = [\"agents\", \"tasks\", \"today\"]\n")?;
    bed.column(
        MINI_TOP,
        MINI_X,
        &[MEETING_NAMESPACE, AGENTS_NAMESPACE, TASKS_NAMESPACE],
        &[TODAY_NAMESPACE],
    )?;
    Ok(())
}

#[test]
fn the_clock_takes_its_colour_and_loses_its_seconds_live() -> anyhow::Result<()> {
    let Some(mut bed) = start("\n[panel]\nform = \"full\"\n")? else {
        return Ok(());
    };
    bed.logged(&["the clock's look", "seconds=true"])?;
    let green = |[r, g, b]: [u8; 3]| r < 0x80 && g > 0xD0 && b > 0x80 && b < 0xC8;
    // The clock's line: the bar's top, 72 px.
    let clock = Rect::new(FULL_X, EDGE + 14, 194, 60);
    anyhow::ensure!(
        !bed.pixels(clock)?.into_iter().any(green),
        "the clock is green before"
    );
    bed.write(
        "\n[panel]\nform = \"full\"\n\n[panel.clock]\ncolor = \"#3DFFA2\"\nseconds = false\n",
    )?;
    bed.logged(&["the clock's look", "seconds=false"])?;
    let mut seen = 0;
    let mut done = false;
    let until = std::time::Instant::now() + WAIT;
    while !done && std::time::Instant::now() < until {
        bed.hp.dispatch(std::time::Duration::from_millis(25))?;
        seen = bed.pixels(clock)?.into_iter().filter(|p| green(*p)).count();
        done = seen > 50;
    }
    anyhow::ensure!(done, "the clock never turned green: {seen} pixels");
    Ok(())
}

/// The compositor alone, on `outputs`, following `file` (its text
/// `text`), its state in `state`.
fn compositor(outputs: &[&str], file: &Path, text: &str, state: &Path) -> anyhow::Result<Headless> {
    std::fs::write(file, text)?;
    let mut config = toml::from_str::<Config>(text)?;
    config.file = Some(file.to_path_buf());
    config.state_dir = Some(state.to_path_buf());
    config.appearance.reduced_motion = true;
    Headless::start_with(
        HeadlessConfig {
            outputs: outputs
                .iter()
                .map(|o| o.parse::<OutputSpec>())
                .collect::<Result<_, _>>()?,
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )
}

/// `text` written over `file` as the Settings write it.
fn rewrite(file: &Path, text: &str) -> anyhow::Result<()> {
    let new = file.with_extension("new");
    std::fs::write(&new, text)?;
    std::fs::rename(&new, file)?;
    Ok(())
}

/// The bars, by screen, and their forms.
fn forms(hp: &Headless) -> Vec<(String, BarForm)> {
    hp.bars().into_iter().map(|(n, f, _)| (n, f)).collect()
}

#[test]
fn every_screen_takes_a_bar_and_gives_it_back_as_the_file_says() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    let mut hp = compositor(
        &["1920x1080", "1366x768"],
        &file,
        "[panel]\nscreens = \"primary\"\n",
        &dir.path().join("state"),
    )?;
    assert_eq!(forms(&hp), [("HEADLESS-1".to_owned(), BarForm::Full)]);
    rewrite(&file, "[panel]\nscreens = \"all\"\n")?;
    let all = [
        ("HEADLESS-1".to_owned(), BarForm::Full),
        ("HEADLESS-2".to_owned(), BarForm::Mini),
    ];
    let done = hp.dispatch_until(WAIT, |hp| forms(hp) == all)?;
    anyhow::ensure!(done, "bars: {:?}", forms(&hp));
    // The second screen's windows make room for its bar.
    assert_eq!(
        hp.work_area_of("HEADLESS-2").map(|a| a.right()),
        Some(1920 + 1366 - 8 - 56 - 8)
    );
    rewrite(&file, "[panel]\nscreens = \"primary\"\n")?;
    let done = hp.dispatch_until(WAIT, |hp| hp.bars().len() == 1)?;
    anyhow::ensure!(done, "bars: {:?}", forms(&hp));
    assert_eq!(
        hp.work_area_of("HEADLESS-2").map(|a| a.right()),
        Some(1920 + 1366 - 8)
    );
    Ok(())
}

#[test]
fn a_form_written_for_a_screen_switches_its_bar_and_forgets_the_click() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    let state = dir.path().join("state");
    let remembered = state.join("panel.toml");
    let mut hp = compositor(&["1920x1080"], &file, "", &state)?;
    assert_eq!(forms(&hp), [("HEADLESS-1".to_owned(), BarForm::Full)]);
    // A click's choice, remembered in automatic.
    hp.pointer_move(400.0, 400.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    let done = hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&remembered).is_ok_and(|t| t.contains("HEADLESS-1 = \"mini\""))
    })?;
    anyhow::ensure!(done, "the click's choice is not remembered");
    assert_eq!(forms(&hp), [("HEADLESS-1".to_owned(), BarForm::Mini)]);
    // "Pleine" chosen for the screen: full at once, the click forgotten.
    let text = "[outputs.HEADLESS-1]\npanel-form = \"full\"\n";
    rewrite(&file, text)?;
    let done = hp.dispatch_until(WAIT, |hp| {
        forms(hp) == [("HEADLESS-1".to_owned(), BarForm::Full)]
            && std::fs::read_to_string(&remembered).is_ok_and(|t| !t.contains("HEADLESS-1"))
    })?;
    anyhow::ensure!(
        done,
        "bars {:?}, remembered {:?}",
        forms(&hp),
        std::fs::read_to_string(&remembered)
    );
    assert_eq!(
        hp.work_area_of("HEADLESS-1").map(|a| a.right()),
        Some(1920 - 8 - 196 - 8)
    );
    // "Mini", then "Auto": the screen's width decides again, over a
    // `[panel] form` for the screens never seen.
    rewrite(&file, "[outputs.HEADLESS-1]\npanel-form = \"mini\"\n")?;
    let done = hp.dispatch_until(WAIT, |hp| {
        forms(hp) == [("HEADLESS-1".to_owned(), BarForm::Mini)]
    })?;
    anyhow::ensure!(done, "bars {:?}", forms(&hp));
    rewrite(
        &file,
        "[panel]\nform = \"mini\"\n\n[outputs.HEADLESS-1]\npanel-form = \"auto\"\n",
    )?;
    let done = hp.dispatch_until(WAIT, |hp| {
        forms(hp) == [("HEADLESS-1".to_owned(), BarForm::Full)]
    })?;
    anyhow::ensure!(done, "bars {:?}", forms(&hp));
    Ok(())
}

/// The bar's right side, saved as `DIR/bar-live-WHAT.png`.
fn shot(bed: &mut Bed, folder: &Path, what: &str) -> anyhow::Result<()> {
    common::at_rest(&mut bed.hp)?;
    let (width, height, bytes) = bed.hp.screenshot(0)?;
    let (w, h) = (260.min(width), height);
    let x0 = width - w;
    let mut image = image::RgbaImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let at = usize::try_from(y * width + x0 + x)? * 4;
            let p = &bytes[at..at + 4];
            image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
        }
    }
    image.save(folder.join(format!("bar-live-{what}.png")))?;
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`: the bar as it
/// comes, then with the agents first and Today hidden, then in the mini
/// form with that order.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(folder) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&folder)?;
    let Some(mut bed) = start("\n[panel]\nform = \"full\"\n")? else {
        return Ok(());
    };
    bed.column(FULL_TOP, FULL_X, &ALL, &[])?;
    shot(&mut bed, &folder, "default")?;
    let panel =
        "\n[panel]\nform = \"full\"\ntoday = false\norder = [\"agents\", \"tasks\", \"today\"]\n";
    bed.write(panel)?;
    bed.column(
        FULL_TOP,
        FULL_X,
        &[MEETING_NAMESPACE, AGENTS_NAMESPACE, TASKS_NAMESPACE],
        &[TODAY_NAMESPACE],
    )?;
    shot(&mut bed, &folder, "agents-first")?;
    bed.write(&panel.replace("\"full\"", "\"mini\""))?;
    bed.column(
        MINI_TOP,
        MINI_X,
        &[MEETING_NAMESPACE, AGENTS_NAMESPACE, TASKS_NAMESPACE],
        &[TODAY_NAMESPACE],
    )?;
    shot(&mut bed, &folder, "agents-first-mini")?;
    Ok(())
}
