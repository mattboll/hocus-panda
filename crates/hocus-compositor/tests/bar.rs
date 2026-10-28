//! The bar's base (docs/design/bar.md §2, §3; issue #358): the space kept
//! for it on the primary screen, its two forms and the switch between
//! them, and the shell that draws it.

mod common;

use std::{path::PathBuf, time::Duration};

use common::{BTN_LEFT, KEY_LEFTMETA, KEY_RIGHT, WAIT, chord, open, rect_of, wait};
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

/// The shell's program, built next to the test's when the workspace is.
fn shell_program() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let program = exe.parent()?.parent()?.join("hocuspanda-shell");
    program.exists().then_some(program)
}

#[test]
fn the_shell_draws_the_bar_switches_on_a_click_at_the_edge_and_rests() -> anyhow::Result<()> {
    let Some(shell) = shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return Ok(());
    };
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
    // A window beside the bar.
    open(&mut hp, "beside", (600, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait(&mut hp, Duration::from_millis(300))?;
    // Into the bar, then against the screen's edge, and a click.
    hp.pointer_move(1800.0, 500.0);
    hp.pointer_move(1919.0, 500.0);
    wait(&mut hp, Duration::from_millis(100))?;
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let switched = hp.dispatch_until(WAIT, |hp| bar(hp).map(|b| b.1) == Some(BarForm::Mini))?;
    assert!(switched, "{:?}", hp.bars());
    let area = Rect::new(8, 8, 1920 - 8 - 56 - 8 - 8, 1064);
    assert_eq!(hp.work_area(), Some(area));
    let half = (area.width - 8) / 2;
    common::wait_for_rect(&mut hp, "beside", Rect::new(8 + half + 8, 8, half, 1064))?;
    // A click in the bar, off its edge, does nothing.
    hp.pointer_move(1880.0, 500.0);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(bar(&hp).map(|b| b.1), Some(BarForm::Mini));
    // At rest, the shell draws nothing (no clock yet), nor the compositor.
    let (commits, frames) = (hp.bar_commits(), hp.frames());
    wait(&mut hp, Duration::from_secs(2))?;
    assert_eq!(hp.bar_commits(), commits);
    assert_eq!(hp.frames(), frames);
    assert!(rect_of(&hp, "beside").is_some());
    Ok(())
}
