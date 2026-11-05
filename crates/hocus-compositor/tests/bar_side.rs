//! The bar on the left of the screens (`[panel] side`, 6.4, issue #404):
//! the space kept for it, its edge to click, the push that brings it in
//! front, the windows that touch it, the popovers on its inner side, the
//! halo and the peeking panda; and a change of side followed live, the bar
//! and the windows it touches alone moving, in 240 ms (at once with
//! reduced motion).
//!
//! The compositor alone where it decides; the real shell on the CPU where
//! the bar's drawing is checked, headless, in French, its agents said with
//! the `hocus` command, in a home of the test's own, no session bus. Each
//! test waits for what it checks, never for a time.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFT, KEY_LEFTMETA, KEY_RIGHT, WAIT, chord, open, rect_of};
use hocus_agents::tools::Places;
use hocus_compositor::{
    BarForm, Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect,
};
use hocus_link::{
    AGENTS_NAMESPACE, CLOCK_NAMESPACE, LEVELS_NAMESPACE, MEETING_NAMESPACE, NAMESPACE,
    TASKS_NAMESPACE,
};

const KEY_B: u32 = 48;

/// A 1920 × 1080 screen, the full bar on its left: 8 px from the edge,
/// 196 px of bar, 8 px to the windows.
const LEFT_AREA: Rect = Rect::new(8 + 196 + 8, 8, 1920 - 8 - 196 - 8 - 8, 1080 - 16);
const RIGHT_AREA: Rect = Rect::new(8, 8, 1920 - 8 - 196 - 8 - 8, 1080 - 16);

const LEFT: &str = "[panel]\nside = \"left\"\n";

fn start(outputs: &[&str], config: Config) -> anyhow::Result<Headless> {
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

/// The configuration `text` says, with reduced motion or not.
fn config(text: &str, reduced: bool) -> anyhow::Result<Config> {
    let mut config = toml::from_str::<Config>(text)?;
    config.appearance.reduced_motion = reduced;
    Ok(config)
}

/// The compositor alone on one 1920 × 1080 screen, following `file` (its
/// text `text`).
fn following(file: &Path, text: &str) -> anyhow::Result<Headless> {
    std::fs::write(file, text)?;
    let mut config = toml::from_str::<Config>(text)?;
    config.file = Some(file.to_path_buf());
    config.state_dir = file.parent().map(|d| d.join("state"));
    start(&["1920x1080"], config)
}

/// `text` written over `file` as the Settings write it: beside it, then
/// renamed over.
fn rewrite(file: &Path, text: &str) -> anyhow::Result<()> {
    let new = file.with_extension("new");
    std::fs::write(&new, text)?;
    std::fs::rename(&new, file)?;
    Ok(())
}

fn form(hp: &Headless) -> Option<BarForm> {
    hp.bars().first().map(|b| b.1)
}

/// A left press and its release at (`x`, `y`); whether the press went on
/// to a client.
fn click(hp: &mut Headless, x: f64, y: f64) -> bool {
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    let forwarded = hp.pointer_grabbed();
    hp.button(BTN_LEFT, false);
    forwarded
}

#[test]
fn on_the_left_every_screen_keeps_the_bars_room_on_its_left() -> anyhow::Result<()> {
    let hp = start(
        &["1920x1080", "1366x768"],
        config("[panel]\nside = \"left\"\nscreens = \"all\"\n", false)?,
    )?;
    assert_eq!(hp.work_area_of("HEADLESS-1"), Some(LEFT_AREA));
    // The second screen, its mini bar on its own left.
    assert_eq!(
        hp.work_area_of("HEADLESS-2"),
        Some(Rect::new(
            1920 + 8 + 56 + 8,
            8,
            1366 - 8 - 56 - 8 - 8,
            768 - 16
        ))
    );
    // Where its glass and its halo go.
    assert_eq!(
        hp.bar_place("HEADLESS-1"),
        Some(Rect::new(8, 8, 196, 1080 - 16))
    );
    assert_eq!(
        hp.bar_place("HEADLESS-2"),
        Some(Rect::new(1920 + 8, 8, 56, 768 - 16))
    );
    Ok(())
}

#[test]
fn a_press_on_the_left_edge_switches_the_form_and_no_client_sees_it() -> anyhow::Result<()> {
    let mut hp = start(&["1920x1080"], config(LEFT, true)?)?;
    assert_eq!(form(&hp), Some(BarForm::Full));
    // Pushed against the screen's left edge, straight from the middle.
    hp.pointer_move(900.0, 500.0);
    assert!(!click(&mut hp, 0.0, 500.0), "the press reached a client");
    assert_eq!(form(&hp), Some(BarForm::Mini));
    assert_eq!(hp.work_area().map(|a| a.x), Some(8 + 56 + 8));
    // 4 px inside the bar's outer edge.
    assert!(!click(&mut hp, f64::from(8 + 4), 500.0));
    assert_eq!(form(&hp), Some(BarForm::Full));
    // In the strip, above the bar's top.
    assert!(!click(&mut hp, 4.0, 2.0));
    assert_eq!(form(&hp), Some(BarForm::Mini));
    // 10 px inside: the bar's own, not its edge; and the right edge is
    // no bar's.
    click(&mut hp, f64::from(8 + 10), 500.0);
    click(&mut hp, 1919.0, 500.0);
    assert_eq!(form(&hp), Some(BarForm::Mini));
    Ok(())
}

#[test]
fn super_b_on_the_left_moves_the_windows_that_touch_the_bar() -> anyhow::Result<()> {
    let mut hp = start(&["1920x1080"], config(LEFT, false)?)?;
    // A window on the left half, and one floating against the bar's room.
    open(&mut hp, "half", (600, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    let half = (LEFT_AREA.width - 8) / 2;
    common::wait_for_rect(
        &mut hp,
        "half",
        Rect::new(LEFT_AREA.x, 8, half, LEFT_AREA.height),
    )?;
    open(&mut hp, "beside", (500, 300))?;
    let centred = rect_of(&hp, "beside").ok_or_else(|| anyhow::anyhow!("no window"))?;
    hp.pointer_move(1000.0, 800.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    assert_eq!(form(&hp), Some(BarForm::Mini));
    let area = Rect::new(8 + 56 + 8, 8, 1920 - 8 - 56 - 8 - 8, 1064);
    assert_eq!(hp.work_area(), Some(area));
    let half = (area.width - 8) / 2;
    common::wait_for_rect(&mut hp, "half", Rect::new(area.x, 8, half, area.height))?;
    // The one in the middle touches nothing: it stays.
    assert_eq!(rect_of(&hp, "beside"), Some(centred));
    Ok(())
}

#[test]
fn pushed_against_the_left_edge_the_bar_comes_in_front_after_300_ms() -> anyhow::Result<()> {
    let mut hp = start(&["1920x1080"], config(LEFT, true)?)?;
    hp.pointer_move(900.0, 400.0);
    let pushed = hp.now();
    hp.pointer_move(0.0, 400.0);
    let raised = hp.dispatch_until(WAIT, |hp| hp.bar_in_front().is_some())?;
    anyhow::ensure!(raised, "the bar never came in front");
    assert!(hp.now() - pushed >= Duration::from_millis(300));
    assert_eq!(hp.bar_in_front().as_deref(), Some("HEADLESS-1"));
    // Across the bar, still in its column; out of it, back under.
    hp.pointer_move(150.0, 400.0);
    assert_eq!(hp.bar_in_front().as_deref(), Some("HEADLESS-1"));
    hp.pointer_move(8.0 + 196.0 + 20.0, 400.0);
    assert_eq!(hp.bar_in_front(), None);
    Ok(())
}

/// The windows of the live tests: one on the right half, one floating in
/// the middle, one floating against the left edge of the work area.
fn three_windows(hp: &mut Headless) -> anyhow::Result<(Rect, Rect)> {
    open(hp, "half", (600, 400))?;
    chord(hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    let half = (RIGHT_AREA.width - 8) / 2;
    common::wait_for_rect(
        hp,
        "half",
        Rect::new(RIGHT_AREA.x + half + 8, 8, half, RIGHT_AREA.height),
    )?;
    let middle = open(hp, "middle", (500, 300))?;
    let middle = rect_of(hp, "middle").unwrap_or(middle);
    // Snapped to the left half then floating again keeps it there; a
    // floating window opened at the edge is simpler: dragged with Alt.
    open(hp, "edge", (400, 300))?;
    let edge = drag_to(hp, "edge", RIGHT_AREA.x)?;
    Ok((middle, edge))
}

/// Moves `app_id`'s window with `Alt`+drag until its left edge is at `x`.
fn drag_to(hp: &mut Headless, app_id: &str, x: i32) -> anyhow::Result<Rect> {
    let start = rect_of(hp, app_id).ok_or_else(|| anyhow::anyhow!("no {app_id}"))?;
    let held = (100, start.height / 2);
    let y = f64::from(start.y + held.1);
    hp.pointer_move(f64::from(start.x + held.0), y);
    hp.key(common::KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
    let mut at = start.x;
    while at != x {
        at = if at < x {
            (at + 40).min(x)
        } else {
            (at - 40).max(x)
        };
        hp.pointer_move(f64::from(at + held.0), y);
        hp.dispatch(Duration::ZERO)?;
    }
    hp.button(BTN_LEFT, false);
    hp.key(common::KEY_LEFTALT, false);
    let want = Rect::new(x, start.y, start.width, start.height);
    common::wait_for_rect(hp, app_id, want)?;
    hp.pointer_move(1000.0, 1000.0);
    Ok(want)
}

#[test]
fn the_side_follows_the_file_and_only_the_windows_the_bar_touches_move() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    let mut hp = following(&file, "")?;
    assert_eq!(hp.work_area(), Some(RIGHT_AREA));
    let (middle, edge) = three_windows(&mut hp)?;
    common::at_rest(&mut hp)?;
    let half_before = hp
        .drawn_rect("half")
        .ok_or_else(|| anyhow::anyhow!("no half"))?;
    rewrite(&file, LEFT)?;
    let moved = hp.dispatch_until(WAIT, |hp| hp.work_area() == Some(LEFT_AREA))?;
    anyhow::ensure!(moved, "the work area: {:?}", hp.work_area());
    assert_eq!(hp.bar_place("HEADLESS-1").map(|b| b.x), Some(8));
    // The right half takes the new area's; the window in the bar's new
    // room moves out of it; the one in the middle does not move.
    let half = (LEFT_AREA.width - 8) / 2;
    let half_after = Rect::new(LEFT_AREA.x + half + 8, 8, half, LEFT_AREA.height);
    let edge_after = Rect::new(LEFT_AREA.x, edge.y, edge.width, edge.height);
    let frames = common::sample(&mut hp, Duration::from_millis(600), |hp| {
        (
            hp.drawn_rect("half"),
            hp.drawn_rect("edge"),
            hp.drawn_rect("middle"),
        )
    })?;
    hp.release_clock();
    // Along the switch's motion: on its way between, done by 240 ms and
    // a frame.
    let between = |r: Option<Rect>, from: Rect, to: Rect| {
        r.is_some_and(|r| r.x != from.x && r.x != to.x && (r.x - from.x) * (r.x - to.x) < 0)
    };
    assert!(
        frames
            .iter()
            .any(|(_, (h, e, _))| between(*h, half_before, half_after)
                && between(*e, edge, edge_after)),
        "never on their way: {frames:?}"
    );
    let done = frames
        .iter()
        .find(|(_, (h, e, _))| *h == Some(half_after) && *e == Some(edge_after))
        .map(|(t, _)| *t);
    assert!(
        done.is_some_and(|t| t <= Duration::from_millis(240) + common::STEP * 2),
        "{done:?}: {frames:?}"
    );
    assert!(
        frames.iter().all(|(_, (_, _, m))| *m == Some(middle)),
        "the middle one moved: {frames:?}"
    );
    common::wait_for_rect(&mut hp, "half", half_after)?;
    common::wait_for_rect(&mut hp, "edge", edge_after)?;
    // And back to the right: the right half back to its place.
    rewrite(&file, "[panel]\nside = \"right\"\n")?;
    let back = hp.dispatch_until(WAIT, |hp| hp.work_area() == Some(RIGHT_AREA))?;
    anyhow::ensure!(back, "the work area: {:?}", hp.work_area());
    common::wait_for_rect(&mut hp, "half", half_before)?;
    assert_eq!(rect_of(&hp, "middle"), Some(middle));
    Ok(())
}

#[test]
fn with_reduced_motion_the_side_changes_at_once() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    let reduced = "[appearance]\nreduced-motion = true\n\n";
    let mut hp = following(&file, reduced)?;
    let (_, edge) = three_windows(&mut hp)?;
    common::at_rest(&mut hp)?;
    rewrite(&file, &format!("{reduced}{LEFT}"))?;
    let moved = hp.dispatch_until(WAIT, |hp| hp.work_area() == Some(LEFT_AREA))?;
    anyhow::ensure!(moved, "the work area: {:?}", hp.work_area());
    let edge_after = Rect::new(LEFT_AREA.x, edge.y, edge.width, edge.height);
    let frames = common::sample(&mut hp, Duration::from_millis(400), |hp| {
        hp.drawn_rect("edge")
    })?;
    hp.release_clock();
    assert!(
        frames
            .iter()
            .all(|(_, r)| *r == Some(edge) || *r == Some(edge_after)),
        "on its way: {frames:?}"
    );
    common::wait_for_rect(&mut hp, "edge", edge_after)?;
    Ok(())
}

// --- With the shell ----------------------------------------------------------

/// The next meeting's card at 14:25, an event after it, three tasks.
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
                       file = \"day.toml\"\n\n[[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\n";

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    hocus: PathBuf,
    file: PathBuf,
}

/// The compositor with the real shell on the CPU on a 1920 × 1080 screen,
/// its file `text`; none when the programs are not built.
fn with_shell(text: &str) -> anyhow::Result<Option<Bed>> {
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
    std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
    let file = root.join("config/config.toml");
    let text = format!("{SOURCES}{text}");
    std::fs::write(&file, &text)?;
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
    let mut config = toml::from_str::<Config>(&text)?;
    config.file = Some(file.clone());
    config.agent_places = Some(Places {
        home: home.clone(),
        config_home: home.join(".config"),
        codex_home: home.join(".codex"),
        path: vec![home.join("bin")],
        herdr_socket: home.join(".config/herdr/herdr.sock"),
        processes: home.join("proc"),
        hocus: hocus.clone(),
        state_dir: Some(home.join("state")),
    });
    config.plain_background = true;
    config.french = true;
    let hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
        Some(script),
    )?;
    let mut bed = Bed {
        _dir: dir,
        hp,
        log,
        hocus,
        file,
    };
    let up = bed.hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar\n{}", common::logged(&bed.log));
    bed.hp.pointer_move(900.0, 600.0);
    Ok(Some(bed))
}

/// The first surface of `namespace` as drawn.
fn drawn(hp: &Headless, namespace: &str) -> Option<Rect> {
    hp.drawn_layers(namespace)
        .first()
        .map(|(_, r)| *r)
        .filter(|r| r.width > 1)
}

impl Bed {
    /// The file becomes the day's sources and `panel`.
    fn write(&self, panel: &str) -> anyhow::Result<()> {
        rewrite(&self.file, &format!("{SOURCES}{panel}"))
    }

    /// Waits until the bar's surface and its clock's, its levels' and its
    /// agents' are drawn from `x`, the bar `width` wide.
    fn bar_at(&mut self, x: i32, width: i32) -> anyhow::Result<()> {
        let placed = |hp: &Headless| {
            drawn(hp, NAMESPACE) == Some(Rect::new(x, 8, width, 1080 - 16))
                && [CLOCK_NAMESPACE, LEVELS_NAMESPACE, AGENTS_NAMESPACE]
                    .iter()
                    .all(|ns| drawn(hp, ns).is_some_and(|r| r.x == x + 1))
        };
        let done = self.hp.dispatch_until(WAIT, placed)?;
        anyhow::ensure!(
            done,
            "the bar: {:?}, the clock {:?}, the levels {:?}, the agents {:?}\n{}",
            drawn(&self.hp, NAMESPACE),
            drawn(&self.hp, CLOCK_NAMESPACE),
            drawn(&self.hp, LEVELS_NAMESPACE),
            drawn(&self.hp, AGENTS_NAMESPACE),
            common::logged(&self.log)
        );
        Ok(())
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
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?;
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

    /// Clicks the date under the clock, once its group lights under the
    /// pointer, and waits for the calendar's popover.
    fn open_calendar(&mut self, date: (f64, f64)) -> anyhow::Result<Rect> {
        let lit = |text: &str| {
            text.lines()
                .any(|l| l.contains("the clock's group is drawn") && l.contains("hover: true"))
        };
        let before = common::logged(&self.log).len();
        common::point(&mut self.hp, (date.0, 500.0))?;
        common::point(&mut self.hp, date)?;
        let log = self.log.clone();
        let hovered = self.hp.dispatch_until(WAIT, |_| {
            common::logged(&log).get(before..).is_some_and(lit)
        })?;
        anyhow::ensure!(hovered, "the clock's group never lit under the pointer");
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).len() == 1)?;
        anyhow::ensure!(
            open,
            "no calendar: {:?}\n{}",
            self.hp.popover_surfaces(),
            common::logged(&self.log)
        );
        common::at_rest(&mut self.hp)?;
        Ok(self.hp.popover_surfaces()[0].1)
    }

    /// The screen into `HOCUS_SHOTS`, if set, as `bar-side-WHAT.png`.
    fn shot(&mut self, what: &str) -> anyhow::Result<()> {
        let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
            return Ok(());
        };
        // The day read: the meeting's card and the to-do list drawn.
        let read = self.hp.dispatch_until(WAIT, |hp| {
            drawn(hp, MEETING_NAMESPACE).is_some_and(|r| r.height > 1)
                && drawn(hp, TASKS_NAMESPACE).is_some_and(|r| r.height > 1)
        })?;
        anyhow::ensure!(read, "the day never showed");
        common::at_rest(&mut self.hp)?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let mut image = image::RgbaImage::new(width as u32, height as u32);
        for y in 0..height {
            for x in 0..width {
                let at = usize::try_from(y * width + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
            }
        }
        image.save(Path::new(&folder).join(format!("bar-side-{what}.png")))?;
        Ok(())
    }
}

#[test]
fn the_shell_moves_the_bar_and_its_groups_to_the_left_and_back_live() -> anyhow::Result<()> {
    let Some(mut bed) = with_shell("[panel]\nform = \"full\"\n")? else {
        return Ok(());
    };
    bed.agent_is("build", "working")?;
    bed.agent_is("api", "working")?;
    bed.bar_at(1920 - 8 - 196, 196)?;
    bed.shot("right")?;
    bed.write("[panel]\nform = \"full\"\nside = \"left\"\n")?;
    bed.bar_at(8, 196)?;
    bed.shot("full")?;
    // The mini form, on the left too.
    bed.write("[panel]\nform = \"mini\"\nside = \"left\"\n")?;
    bed.bar_at(8, 56)?;
    bed.shot("mini")?;
    bed.write("[panel]\nform = \"mini\"\n")?;
    bed.bar_at(1920 - 8 - 56, 56)?;
    Ok(())
}

#[test]
fn a_popover_opens_on_the_inner_side_of_a_bar_on_the_left() -> anyhow::Result<()> {
    let Some(mut bed) = with_shell("[panel]\nform = \"full\"\nside = \"left\"\n")? else {
        return Ok(());
    };
    bed.bar_at(8, 196)?;
    // The date under the clock, in the middle of the bar.
    let popover = bed.open_calendar((8.0 + 98.0, 82.0))?;
    // 8 px right of the bar.
    assert_eq!(popover.x, 8 + 196 + 8, "{popover:?}");
    bed.shot("popover")?;
    Ok(())
}

#[test]
fn the_halo_and_the_panda_come_out_of_a_bar_on_the_left() -> anyhow::Result<()> {
    let Some(mut bed) = with_shell("[panel]\nform = \"full\"\nside = \"left\"\n")? else {
        return Ok(());
    };
    bed.bar_at(8, 196)?;
    bed.agent_is("build", "working")?;
    bed.agent_is("api", "working")?;
    bed.agent_is("api", "waiting")?;
    // The halo lights the bar where it is: around its place (drawn on a
    // GPU only).
    let lit = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.halo_concern().is_some())?;
    anyhow::ensure!(lit, "no halo");
    assert_eq!(
        bed.hp.bar_place("HEADLESS-1"),
        Some(Rect::new(8, 8, 196, 1080 - 16))
    );
    // The panda peeks out on the bar's inner side.
    let shown = bed.hp.dispatch_until(WAIT, |hp| {
        hp.peek_commits() > 0 && hp.peek_surfaces().len() == 1 && hp.peek().is_some()
    })?;
    anyhow::ensure!(
        shown,
        "no peek: {:?}\n{}",
        bed.hp.peek_surfaces(),
        common::logged(&bed.log)
    );
    let peek = bed.hp.peek_surfaces()[0].1;
    assert!(peek.x >= 8 + 196, "{peek:?}");
    bed.shot("peek")?;
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`: a window beside
/// the bar on the left.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    if std::env::var_os("HOCUS_SHOTS").is_none() {
        return Ok(());
    }
    let Some(mut bed) = with_shell("[panel]\nform = \"full\"\nside = \"left\"\n")? else {
        return Ok(());
    };
    bed.agent_is("build", "working")?;
    bed.agent_is("api", "working")?;
    bed.bar_at(8, 196)?;
    open(&mut bed.hp, "window", (600, 400))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFT]);
    let half = (LEFT_AREA.width - 8) / 2;
    common::wait_for_rect(
        &mut bed.hp,
        "window",
        Rect::new(LEFT_AREA.x, 8, half, LEFT_AREA.height),
    )?;
    bed.shot("window")?;
    Ok(())
}
