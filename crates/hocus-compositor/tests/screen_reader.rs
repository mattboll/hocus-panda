//! What screen readers read (docs/design/screen-reader.md, #421, #734), as
//! `hocus msg a11y` prints it: the real shell, against the private fakes of
//! its services (`levels/fakes.rs`), with three outputs. The bar's sound
//! is a slider; a click opens its popover, a dialog with its rows by their
//! kinds that holds the keyboard; `Tab` moves the focus along its rows;
//! `Esc` gives the focus back to the bar's sound.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

mod common;
#[path = "levels/fakes.rs"]
mod fakes;

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::mpsc,
    time::Duration,
};

use anyhow::Context;
use common::{KEY_ESC, KEY_TAB, WAIT, shell_program};
use fakes::{FakeSink, Fakes};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};

const SPEAKERS: &str = "hocus-test-speakers";
const HEADSET: &str = "hocus-test-headset";
const SCREEN: &str = "hocus-test-hdmi";

const OUTPUTS: [FakeSink; 3] = [
    FakeSink::new(SPEAKERS, "Haut-parleurs du portable"),
    FakeSink {
        name: HEADSET,
        description: "Casque Bluetooth",
        form_factor: Some("headset"),
    },
    FakeSink::new(SCREEN, "Écran externe (HDMI)"),
];

/// On a 1920×1080 screen, the full bar's levels' surface, and the sound's
/// row's middle (as tests/popover.rs).
const LEVELS: Rect = Rect::new(1920 - 8 - 1 - 194, 1080 - 8 - 41 - 175, 194, 175);
const ROW: (f64, f64) = (1800.0, (LEVELS.y + 57) as f64);

fn ready() -> Option<std::path::PathBuf> {
    let Some(shell) = shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    if !fakes::available() {
        eprintln!("dbus-daemon, pipewire, pw-dump, wpctl or pw-metadata missing: skipped");
        return None;
    }
    Some(shell)
}

fn start(fakes: &Fakes, shell: &std::path::Path) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
        Some(fakes.shell(shell, "fr_FR.UTF-8")?),
    )?;
    let placed = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.levels_surfaces() == vec![("HEADLESS-1".to_owned(), LEVELS)] && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(placed, "no levels: {:?}", hp.levels_surfaces());
    // The sound's row is the levels' (not the connections' or the
    // desktops', which follow the levels up a moment later).
    common::over(&mut hp, ROW, hocus_link::LEVELS_NAMESPACE)?;
    hp.pointer_move(600.0, 500.0);
    Ok(hp)
}

/// `hocus msg a11y`'s answer, from a thread while the loop runs.
fn ask(hp: &mut Headless) -> anyhow::Result<String> {
    let path = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let asked = (|| -> std::io::Result<String> {
            let mut stream = UnixStream::connect(&path)?;
            stream.set_read_timeout(Some(WAIT))?;
            stream.write_all(b"msg a11y\n")?;
            let mut answer = String::new();
            stream.read_to_string(&mut answer)?;
            Ok(answer)
        })();
        let _ = said.send(asked);
    });
    let mut answer = None;
    hp.dispatch_until(WAIT, |_| {
        if answer.is_none() {
            answer = heard.try_recv().ok();
        }
        answer.is_some()
    })?;
    Ok(answer.context("no answer")??)
}

/// Asks until the tree holds every line of `want`, or says what it holds.
fn until_said(hp: &mut Headless, want: &[&str]) -> anyhow::Result<String> {
    let start = std::time::Instant::now();
    loop {
        let said = ask(hp)?;
        if want.iter().all(|w| said.contains(w)) {
            return Ok(said);
        }
        if start.elapsed() > Duration::from_secs(10) {
            let missing: Vec<&&str> = want.iter().filter(|w| !said.contains(**w)).collect();
            anyhow::bail!("no {missing:?} in:\n{said}");
        }
        common::wait(hp, Duration::from_millis(100))?;
    }
}

fn press(hp: &mut Headless, key: u32) {
    hp.key(key, true);
    hp.key(key, false);
}

#[test]
fn the_sound_reads_as_a_slider_its_popover_as_a_dialog_and_esc_gives_the_focus_back()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start_with(&OUTPUTS)?;
    let mut hp = start(&fakes, &shell)?;

    // The bar: its window, its clock, the sound as a slider.
    let said = until_said(&mut hp, &["window \"Barre — ", "slider \"Volume\" = \""])?;
    assert!(said.contains("<click, increment, decrement>"), "{said}");
    assert!(!said.contains("dialog"), "no popover yet:\n{said}");

    // A click opens the popover, which holds the keyboard: a dialog named
    // by its title, its subtitle the default output, its rows by kinds.
    common::click(&mut hp, ROW)?;
    let said = until_said(
        &mut hp,
        &[
            "window \"Son\" [active]",
            "dialog \"Son\" — \"Haut-parleurs du portable\"",
            "toggle button \"Couper le son\" [not pressed]",
            "slider \"Volume\" = \"",
            "radio button \"Haut-parleurs du portable\" [checked] 1/3",
            "radio button \"Casque Bluetooth\" [not checked] 2/3",
            "button \"Réglages du son…\"",
        ],
    )?;
    assert!(
        said.contains("expanded"),
        "the bar's sound says its popover is open:\n{said}"
    );

    // Tab: the mute button, then the volume.
    press(&mut hp, KEY_TAB);
    until_said(&mut hp, &["window \"Son\" [active] focus=Mute"])?;
    press(&mut hp, KEY_TAB);
    until_said(&mut hp, &["window \"Son\" [active] focus=Slider"])?;

    // Esc: the popover goes, the bar's sound holds the keyboard.
    press(&mut hp, KEY_ESC);
    let said = until_said(&mut hp, &["[active] focus=Sound"])?;
    assert!(!said.contains("window \"Son\""), "{said}");
    Ok(())
}

#[test]
fn without_a_shell_it_says_so() -> anyhow::Result<()> {
    let mut hp = Headless::start(HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })?;
    assert_eq!(ask(&mut hp)?, "no shell\n");
    Ok(())
}
