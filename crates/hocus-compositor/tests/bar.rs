//! The bar's base (docs/design/bar.md §2, §3; issue #358): the space kept
//! for it on the primary screen, its two forms and the switch between
//! them, and the shell that draws it.

mod common;

use std::{path::PathBuf, time::Duration};

use common::{BTN_LEFT, KEY_LEFTMETA, KEY_RIGHT, WAIT, chord, open, rect_of, shell_program, wait};
use hocus_compositor::{
    BarForm, Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect,
};

const KEY_B: u32 = 48;

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

/// The bar's screen and form, and whether it is still switching.
fn bar(hp: &Headless) -> Option<(String, BarForm)> {
    hp.bars()
        .into_iter()
        .next()
        .map(|(name, form, _)| (name, form))
}

fn reduced() -> Config {
    let mut config = Config::default();
    config.appearance.reduced_motion = true;
    config
}

#[test]
fn the_primary_screen_keeps_the_bars_space_and_the_others_do_not() -> anyhow::Result<()> {
    let hp = start(&["2560x1600@1", "1920x1080"], Config::default())?;
    assert_eq!(hp.bars().len(), 1);
    assert_eq!(bar(&hp), Some(("HEADLESS-1".into(), BarForm::Full)));
    // 8 px from the edge, 196 px of bar, 8 px to the windows.
    assert_eq!(
        hp.work_area_of("HEADLESS-1"),
        Some(Rect::new(8, 8, 2560 - 8 - 196 - 8 - 8, 1600 - 16))
    );
    assert_eq!(
        hp.work_area_of("HEADLESS-2"),
        Some(Rect::new(2560 + 8, 8, 1920 - 16, 1080 - 16))
    );
    Ok(())
}

#[test]
fn the_default_form_follows_the_screens_logical_width() -> anyhow::Result<()> {
    for (spec, form) in [
        ("1366x768", BarForm::Mini),
        ("1920x1080", BarForm::Full),
        ("2560x1600@2", BarForm::Mini),
        ("2560x1600@1", BarForm::Full),
        ("3440x1440", BarForm::Full),
    ] {
        let hp = start(&[spec], Config::default())?;
        assert_eq!(bar(&hp).map(|b| b.1), Some(form), "{spec}");
    }
    // The configuration's choice wins, on every screen or on one.
    let mut config = Config::default();
    config.panel.form = hocus_compositor::FormChoice::Mini;
    let hp = start(&["2560x1600"], config.clone())?;
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    config.outputs.insert(
        "HEADLESS-1".into(),
        hocus_compositor::OutputSettings {
            panel_form: hocus_compositor::FormChoice::Full,
            ..Default::default()
        },
    );
    let hp = start(&["2560x1600"], config)?;
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Full));
    Ok(())
}

#[test]
fn super_b_switches_the_form_and_the_windows_beside_it_follow() -> anyhow::Result<()> {
    let mut hp = start(&["2560x1600@1"], Config::default())?;
    open(&mut hp, "right", (600, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    let half = (2560 - 8 - 196 - 8 - 8 - 8) / 2;
    common::wait_for_rect(&mut hp, "right", Rect::new(8 + half + 8, 8, half, 1584))?;
    hp.pointer_move(1000.0, 800.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    let area = Rect::new(8, 8, 2560 - 8 - 56 - 8 - 8, 1584);
    assert_eq!(hp.work_area(), Some(area));
    // The bar moves over 240 ms: it is on its way right after.
    let width = hp.bars()[0].2;
    assert!(width > 56.0, "{width}");
    let half = (area.width - 8) / 2;
    common::wait_for_rect(&mut hp, "right", Rect::new(8 + half + 8, 8, half, 1584))?;
    let settled = hp.dispatch_until(WAIT, |hp| hp.bars()[0].2 == 56.0)?;
    assert!(settled, "{:?}", hp.bars());
    // Again: back to full.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Full));
    assert_eq!(hp.work_area().map(|a| a.right()), Some(2560 - 8 - 196 - 8));
    Ok(())
}

#[test]
fn with_reduced_motion_the_form_changes_at_once() -> anyhow::Result<()> {
    let mut hp = start(&["2560x1600@1"], reduced())?;
    hp.pointer_move(1000.0, 800.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    assert_eq!(hp.bars()[0].2, 56.0);
    Ok(())
}

/// A left press and its release at (`x`, `y`); whether the press went on to
/// a client.
fn click(hp: &mut Headless, x: f64, y: f64) -> bool {
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    let forwarded = hp.pointer_grabbed();
    hp.button(BTN_LEFT, false);
    forwarded
}

#[test]
fn a_press_on_the_bars_edge_switches_its_form_and_no_client_sees_it() -> anyhow::Result<()> {
    let mut hp = start(&["1920x1080"], reduced())?;
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Full));
    // Pushed against the screen's edge, straight from the middle of the
    // screen: no motion inside the bar before the press.
    hp.pointer_move(900.0, 500.0);
    let forwarded = click(&mut hp, 1919.0, 500.0);
    assert!(!forwarded, "the press reached a client");
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    assert_eq!(hp.work_area().map(|a| a.right()), Some(1920 - 8 - 56 - 8));
    // The bar's outer edge is 8 px from the screen's: 4 px inside it.
    assert!(!click(&mut hp, f64::from(1920 - 8 - 4), 500.0));
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Full));
    // In the strip, above the bar's top.
    assert!(!click(&mut hp, 1915.0, 2.0));
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    // 10 px inside: the bar's own, not its edge.
    click(&mut hp, f64::from(1920 - 8 - 10), 500.0);
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    Ok(())
}

#[test]
fn with_a_shell_on_the_cpu_the_space_and_the_windows_switch_at_once() -> anyhow::Result<()> {
    use std::io::Write;
    let mut hp = start(&["1920x1080"], Config::default())?;
    open(&mut hp, "beside", (600, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait(&mut hp, Duration::from_millis(300))?;
    // A shell that says it draws on the CPU.
    let mut socket = hp.socket().as_os_str().to_owned();
    socket.push(".shell");
    let mut shell = std::os::unix::net::UnixStream::connect(socket)?;
    shell.write_all(b"software\n")?;
    let told = hp.dispatch_until(WAIT, Headless::shell_on_cpu)?;
    assert!(told, "the compositor did not read the shell's line");
    click(&mut hp, 1919.0, 500.0);
    assert_eq!(hp.bars()[0].1, BarForm::Mini);
    assert_eq!(hp.bars()[0].2, 56.0, "the bar is at its new width");
    let area = Rect::new(8, 8, 1920 - 8 - 56 - 8 - 8, 1064);
    assert_eq!(hp.work_area(), Some(area));
    let half = (area.width - 8) / 2;
    assert_eq!(
        hp.drawn_rect("beside"),
        Some(Rect::new(8 + half + 8, 8, half, 1064)),
        "the window is drawn where it goes"
    );
    Ok(())
}

#[test]
fn the_bar_leaves_with_its_screen_and_comes_back_with_it() -> anyhow::Result<()> {
    let mut hp = start(&["2560x1600@1", "1366x768"], Config::default())?;
    assert_eq!(bar(&hp), Some(("HEADLESS-1".into(), BarForm::Full)));
    hp.unplug("HEADLESS-1")?;
    // On the screen left, in its own form.
    assert_eq!(bar(&hp), Some(("HEADLESS-2".into(), BarForm::Mini)));
    assert_eq!(
        hp.work_area_of("HEADLESS-2").map(|a| a.right()),
        Some(2560 + 1366 - 8 - 56 - 8)
    );
    hp.plug("HEADLESS-1")?;
    assert_eq!(bar(&hp), Some(("HEADLESS-1".into(), BarForm::Full)));
    assert_eq!(
        hp.work_area_of("HEADLESS-2").map(|a| a.right()),
        Some(2560 + 1366 - 8)
    );
    Ok(())
}

#[test]
fn the_named_screen_is_primary_and_all_screens_may_have_a_bar() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.panel.output = "HEADLESS-2".into();
    let hp = start(&["1920x1080", "1920x1080"], config)?;
    assert_eq!(bar(&hp).map(|b| b.0), Some("HEADLESS-2".into()));
    let mut config = Config::default();
    config.panel.screens = hocus_compositor::PanelScreens::All;
    let hp = start(&["1920x1080", "1366x768"], config)?;
    let forms: Vec<(String, BarForm)> = hp.bars().into_iter().map(|b| (b.0, b.1)).collect();
    assert_eq!(
        forms,
        [
            ("HEADLESS-1".into(), BarForm::Full),
            ("HEADLESS-2".into(), BarForm::Mini)
        ]
    );
    Ok(())
}

#[test]
fn a_chosen_form_is_remembered_for_the_next_session() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let mut config = reduced();
    config.state_dir = Some(state.path().join("hocuspanda"));
    let mut hp = start(&["2560x1600@1"], config.clone())?;
    hp.pointer_move(1000.0, 800.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    let file = state.path().join("hocuspanda/panel.toml");
    let written = hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&file).is_ok_and(|t| t.contains("mini"))
    })?;
    assert!(written);
    drop(hp);
    let hp = start(&["2560x1600@1"], config)?;
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    Ok(())
}

#[test]
fn the_shell_draws_the_bar_as_wide_as_its_form_and_rests() -> anyhow::Result<()> {
    let Some(shell) = shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return Ok(());
    };
    let dir = tempfile::tempdir()?;
    let shell = common::shell_script(dir.path(), &shell, &[])?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
        Some(shell),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.shell_connected() && hp.bar_commits() > 0
    })?;
    assert!(drawn, "the shell drew no bar");
    // Its surface is the bar, 8 px from the screen's edges: nothing
    // transparent around it.
    let surface = |width| {
        vec![(
            "HEADLESS-1".to_owned(),
            Rect::new(1920 - 8 - width, 8, width, 1064),
        )]
    };
    let placed = hp.dispatch_until(WAIT, |hp| hp.bar_surfaces() == surface(196))?;
    assert!(placed, "{:?}", hp.bar_surfaces());
    // A window beside the bar.
    open(&mut hp, "beside", (600, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait(&mut hp, Duration::from_millis(300))?;
    // Against the screen's edge, and a click: the compositor's, not the
    // shell's.
    hp.pointer_move(1919.0, 500.0);
    hp.button(BTN_LEFT, true);
    assert!(!hp.pointer_grabbed(), "the press reached the shell");
    hp.button(BTN_LEFT, false);
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    let area = Rect::new(8, 8, 1920 - 8 - 56 - 8 - 8, 1064);
    assert_eq!(hp.work_area(), Some(area));
    let half = (area.width - 8) / 2;
    common::wait_for_rect(&mut hp, "beside", Rect::new(8 + half + 8, 8, half, 1064))?;
    let narrowed = hp.dispatch_until(WAIT, |hp| hp.bar_surfaces() == surface(56))?;
    assert!(narrowed, "{:?}", hp.bar_surfaces());
    // A click in the bar, off its edge, goes to the shell and switches
    // nothing.
    hp.pointer_move(1880.0, 500.0);
    hp.button(BTN_LEFT, true);
    assert!(hp.pointer_grabbed());
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    // At rest, the shell draws only its clock, once a second, and the
    // compositor only the clock's place.
    let clock = Rect::new(1920 - 8 - 1 - 54, 8 + 12, 54, 40);
    let placed = hp.dispatch_until(WAIT, |hp| {
        hp.clock_surfaces() == vec![("HEADLESS-1".to_owned(), clock)]
    })?;
    assert!(placed, "{:?}", hp.clock_surfaces());
    wait(&mut hp, Duration::from_millis(500))?;
    let (commits, clocks, frames) = (hp.bar_commits(), hp.clock_commits(), hp.frames());
    hp.take_damage(0);
    wait(&mut hp, Duration::from_secs(3))?;
    assert_eq!(hp.bar_commits(), commits);
    let ticks = hp.clock_commits() - clocks;
    assert!((2..=4).contains(&ticks), "{ticks} clock commits in 3 s");
    assert!((2..=4).contains(&(hp.frames() - frames)));
    assert_eq!(hp.take_damage(0), Some(clock));
    assert!(rect_of(&hp, "beside").is_some());
    Ok(())
}

/// The shell, started through a script that sets its clock to `at` (a
/// wall-clock time of today): the system's clock is never touched.
fn shell_at(shell: &std::path::Path, at: &str, dir: &std::path::Path) -> anyhow::Result<PathBuf> {
    common::shell_script(dir, shell, &[("HOCUS_CLOCK_AT", at.to_owned())])
}

#[test]
fn the_clock_shows_in_its_place_in_both_forms() -> anyhow::Result<()> {
    let Some(shell) = shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return Ok(());
    };
    let dir = tempfile::tempdir()?;
    let shell = common::shell_script(dir.path(), &shell, &[])?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        reduced(),
        Some(shell),
    )?;
    // Inside the bar's rim, 12 px below its top.
    let full = Rect::new(1920 - 8 - 1 - 194, 8 + 12, 194, 74);
    let placed = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.clock_surfaces() == vec![("HEADLESS-1".to_owned(), full)] && hp.clock_commits() > 0
    })?;
    assert!(placed, "{:?}", hp.clock_surfaces());
    // Text in the full clock's place: light pixels on the dark bar.
    wait(&mut hp, Duration::from_millis(300))?;
    let light = |hp: &mut Headless, r: Rect| -> anyhow::Result<usize> {
        let (w, _, pixels) = hp.screenshot(0)?;
        let mut n = 0;
        for y in r.y..r.y + r.height {
            for x in r.x..r.x + r.width {
                let i = ((y * w + x) * 4) as usize;
                if pixels[i..i + 3].iter().all(|c| *c > 0xC0) {
                    n += 1;
                }
            }
        }
        Ok(n)
    };
    assert!(light(&mut hp, full)? > 200, "no time drawn");
    // The mini form's clock, in its narrower place.
    hp.pointer_move(1000.0, 500.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    let mini = Rect::new(1920 - 8 - 1 - 54, 8 + 12, 54, 40);
    let placed = hp.dispatch_until(WAIT, |hp| {
        hp.clock_surfaces() == vec![("HEADLESS-1".to_owned(), mini)]
    })?;
    assert!(placed, "{:?}", hp.clock_surfaces());
    wait(&mut hp, Duration::from_millis(300))?;
    assert!(light(&mut hp, mini)? > 40, "no time drawn");
    Ok(())
}

/// Clock commits and when, for `time`, after `start`.
fn clock_commits_for(hp: &mut Headless, time: Duration) -> anyhow::Result<Vec<Duration>> {
    let start = std::time::Instant::now();
    let mut seen = hp.clock_commits();
    let mut at = Vec::new();
    while start.elapsed() < time {
        hp.dispatch(Duration::from_millis(2))?;
        let now = hp.clock_commits();
        for _ in seen..now {
            at.push(start.elapsed());
        }
        seen = now;
    }
    Ok(at)
}

/// The commits that come closer together than a second's ticks: the
/// motion's frames, and how long they last.
fn motion_of(commits: &[Duration]) -> (usize, Duration) {
    let close: Vec<Duration> = commits
        .windows(2)
        .filter(|p| p[1] - p[0] < Duration::from_millis(100))
        .map(|p| p[1])
        .collect();
    let span = match (close.first(), close.last()) {
        (Some(a), Some(b)) => *b - *a,
        _ => Duration::ZERO,
    };
    (close.len(), span)
}

/// On a GPU (the shell draws with wgpu): a minute change moves once, for
/// about 520 ms, then the clock rests; with reduced motion it does not
/// move. Skipped without a render node.
#[test]
fn a_minute_change_moves_once_then_rests() -> anyhow::Result<()> {
    let Some(shell) = shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return Ok(());
    };
    // The first render node: on a machine with two GPUs, as often as not
    // the integrated one, which this keeps the other asleep.
    let node = PathBuf::from("/dev/dri/renderD128");
    if !node.exists() {
        eprintln!("no render node: the shell would draw on the CPU, without motion");
        return Ok(());
    }
    let dir = tempfile::tempdir()?;
    for (config, moves) in [(Config::default(), true), (reduced(), false)] {
        // The minute changes about two seconds after the shell starts.
        let script = shell_at(&shell, "09:59:58", dir.path())?;
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Gles,
                render_node: Some(node.clone()),
            },
            config,
            Some(script),
        )?;
        let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.clock_commits() > 0)?;
        assert!(drawn, "no clock");
        if hp.shell_on_cpu() {
            eprintln!("the shell draws on the CPU here: no motion to see");
            return Ok(());
        }
        let commits = clock_commits_for(&mut hp, Duration::from_secs(5))?;
        let (frames, span) = motion_of(&commits);
        if moves {
            assert!(frames >= 10, "{frames} frames of motion: {commits:?}");
            assert!(
                (Duration::from_millis(400)..Duration::from_millis(650)).contains(&span),
                "{span:?}"
            );
        } else {
            assert_eq!(frames, 0, "{commits:?}");
        }
        // Then once a second.
        assert!(commits.len() - frames <= 7, "{commits:?}");
    }
    Ok(())
}
