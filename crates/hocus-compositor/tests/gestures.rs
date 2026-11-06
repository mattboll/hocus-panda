//! Touchpad gestures (6.13, issue #447): what the desktop takes of the
//! touchpad's swipes and what it leaves to the applications. Headless has
//! no libinput: the swipes are the harness's, as libinput gives them.
//!
//! One test per point of the spec: three fingers across with
//! `swipe-desktops` off, or three fingers that start vertical, reach the
//! client whole; four fingers up open the launcher past a third of their
//! travel, or on a flick, and it springs back short of it; four fingers
//! down close it (with none open, they open the overview: 8.5 A,
//! `tests/overview.rs`); it follows the fingers (its opacity and size at half
//! their travel), and with reduced motion only fades on release; the
//! direction is the fingers' whatever natural scrolling says;
//! `swipe-launcher` off, live, leaves four fingers to the application.
//!
//! The launcher is the real shell's, with no applications (a data
//! directory of the test's own), on the CPU.

mod common;

use std::{path::PathBuf, time::Duration};

use common::{WAIT, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};

/// A swipe's moves, as libinput gives them about every 10 ms.
const EVERY: u32 = 10;

/// A compositor without a shell, its windows' desktops what moves.
fn bare(config: Config) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1280x800".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.pointer_move(640.0, 400.0);
    Ok(hp)
}

/// A compositor with the real shell, its directories the test's own; its
/// configuration `file`, read as the session reads it.
struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
}

impl Bed {
    fn start(file: &str) -> anyhow::Result<Option<Self>> {
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state", "system"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        // A few applications, their icons plain squares.
        let apps = root.join("data/applications");
        let icons = root.join("data/icons/hicolor/scalable/apps");
        std::fs::create_dir_all(&apps)?;
        std::fs::create_dir_all(&icons)?;
        for (id, name, colour) in [
            ("files", "Fichiers", "#5b8def"),
            ("notes", "Notes", "#e0b341"),
            ("terminal", "Terminal", "#4caf7a"),
            ("web", "Navigateur", "#d9534f"),
        ] {
            std::fs::write(
                icons.join(format!("{id}.svg")),
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"48\" height=\"48\">\
                     <rect width=\"48\" height=\"48\" rx=\"10\" fill=\"{colour}\"/></svg>"
                ),
            )?;
            std::fs::write(
                apps.join(format!("{id}.desktop")),
                format!(
                    "[Desktop Entry]\nType=Application\nName={name}\nExec=/bin/true\nIcon={id}\n"
                ),
            )?;
        }
        let at = |sub: &str| root.join(sub).display().to_string();
        let script = common::shell_script(
            root,
            &shell,
            &[
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("config")),
                ("XDG_CACHE_HOME", at("cache")),
                ("XDG_STATE_HOME", at("state")),
                ("XDG_DATA_HOME", at("data")),
                ("XDG_DATA_DIRS", at("system")),
                ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ],
        )?;
        let path = root.join("config.toml");
        std::fs::write(&path, file)?;
        let mut config = Config::load(&path);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(path);
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        let mut bed = Self { hp, dir };
        let up = bed
            .hp
            .dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        bed.hp.pointer_move(600.0, 500.0);
        wait(&mut bed.hp, Duration::from_millis(300))?;
        Ok(Some(bed))
    }

    fn file(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    /// Four fingers up by `travel` in `steps` moves `every` ms apart,
    /// held there.
    fn up(&mut self, travel: f64, steps: u32, every: u32) {
        self.hp.swipe_begin(4);
        for _ in 0..steps {
            self.hp.swipe_move(0.0, -travel / f64::from(steps), every);
        }
    }

    /// The launcher drawn once the shell has drawn it, as the fingers hold
    /// it: its opacity and its panel's scale.
    fn held_look(&mut self) -> anyhow::Result<(f32, f64)> {
        let drawn = self
            .hp
            .dispatch_until(WAIT, |hp| hp.launcher_look().is_some())?;
        anyhow::ensure!(drawn, "the shell never drew the launcher");
        self.hp
            .launcher_look()
            .ok_or_else(|| anyhow::anyhow!("no launcher"))
    }

    /// Runs until the launcher is open and still, holding the keyboard.
    fn opened(&mut self) -> anyhow::Result<()> {
        let open = self.hp.dispatch_until(WAIT, |hp| {
            hp.launcher_at_rest() && hp.keyboard_layer().as_deref() == Some("hocus-launcher")
        })?;
        anyhow::ensure!(
            open,
            "the launcher did not open: {:?}",
            self.hp.launcher_look()
        );
        Ok(())
    }

    /// Runs until no launcher shows, the keyboard back.
    fn closed(&mut self) -> anyhow::Result<()> {
        let closed = self.hp.dispatch_until(WAIT, |hp| {
            hp.launcher_surfaces().is_empty() && hp.keyboard_layer().is_none()
        })?;
        anyhow::ensure!(closed, "the launcher stays: {:?}", self.hp.launcher_look());
        Ok(())
    }
}

#[test]
fn three_finger_swipe_off_leaves_the_desktop_and_reaches_the_client() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.touchpad.swipe_desktops = false;
    let mut hp = bare(config)?;
    hp.swipe(3, &[(-30.0, 0.0); 5], 40);
    assert_eq!(hp.desktop(), (0, 0.0), "three fingers moved the desktops");
    assert_eq!(hp.swipes_given(), (1, 5), "the client has it whole");
    // On, the desktops take a swipe across; one that starts vertical is
    // the client's for its whole length, from its start, even if it then
    // goes across.
    let mut hp = bare(Config::default())?;
    hp.swipe(3, &[(-30.0, 0.0); 5], 40);
    assert_eq!(hp.desktop().0, 1, "three fingers across");
    assert_eq!(hp.swipes_given(), (0, 0));
    hp.swipe(
        3,
        &[(0.0, 5.0), (0.0, 10.0), (-60.0, 0.0), (-60.0, 0.0)],
        40,
    );
    assert_eq!(hp.desktop().0, 1, "a vertical swipe moved the desktops");
    assert_eq!(hp.swipes_given(), (1, 4));
    // Two fingers and five are never the desktop's.
    hp.swipe(2, &[(-30.0, 0.0); 3], 40);
    hp.swipe(5, &[(-30.0, 0.0); 3], 40);
    assert_eq!(hp.swipes_given(), (3, 10));
    Ok(())
}

#[test]
fn four_fingers_up_opens_the_launcher_past_a_third() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("")? else {
        return Ok(());
    };
    // Slowly, half its travel (300 units): it opens on release, its
    // search holding the keyboard.
    bed.up(150.0, 10, 40);
    bed.held_look()?;
    bed.hp.swipe_end(40, false);
    bed.opened()?;
    assert_eq!(bed.hp.launcher_look(), Some((1.0, 1.0)));
    assert_eq!(bed.hp.swipes_given(), (0, 0), "the client saw it");
    // Four fingers down with it open close it (below); up again with it
    // open are the application's.
    bed.hp.swipe(4, &[(0.0, -40.0); 4], EVERY);
    assert_eq!(bed.hp.swipes_given(), (1, 4));
    wait(&mut bed.hp, Duration::from_millis(200))?;
    assert_eq!(bed.hp.launcher_surfaces().len(), 1, "it closed");
    // Closed, a short flick up opens it too.
    chord_escape(&mut bed.hp);
    bed.closed()?;
    bed.up(60.0, 4, EVERY);
    bed.hp.swipe_end(EVERY, false);
    bed.opened()?;
    Ok(())
}

/// Escape closes the launcher.
fn chord_escape(hp: &mut Headless) {
    hp.key(common::KEY_ESC, true);
    hp.key(common::KEY_ESC, false);
}

#[test]
fn springs_back_short_of_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("")? else {
        return Ok(());
    };
    // A quarter of its travel, slowly: it shows a quarter, then fades back
    // out on release, the keyboard where it was.
    bed.up(75.0, 5, 80);
    let (alpha, _) = bed.held_look()?;
    assert!((alpha - 0.25).abs() < 0.01, "{alpha}");
    bed.hp.swipe_end(80, false);
    bed.closed()?;
    // A swipe cancelled (a fifth finger) goes back too, however far.
    bed.up(240.0, 8, 40);
    bed.held_look()?;
    bed.hp.swipe_end(40, true);
    bed.closed()?;
    // Down, with nothing open, is the overview's (8.5 A), not the
    // application's.
    bed.hp.swipe(4, &[(0.0, 40.0); 4], EVERY);
    assert_eq!(bed.hp.swipes_given(), (0, 0));
    assert_eq!(bed.hp.overview(), Some(true));
    wait(&mut bed.hp, Duration::from_millis(200))?;
    assert!(bed.hp.launcher_surfaces().is_empty());
    Ok(())
}

#[test]
fn four_fingers_down_closes_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("")? else {
        return Ok(());
    };
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, 57]);
    bed.opened()?;
    // Down a fifth, slowly: it follows, then comes back whole.
    bed.hp.swipe_begin(4);
    for _ in 0..4 {
        bed.hp.swipe_move(0.0, 15.0, 80);
    }
    let (alpha, _) = bed.hp.launcher_look().unwrap_or_default();
    assert!((alpha - 0.8).abs() < 0.01, "{alpha}");
    bed.hp.swipe_end(80, false);
    bed.opened()?;
    // Down half the way: it goes, following the fingers out.
    bed.hp.swipe_begin(4);
    for _ in 0..10 {
        bed.hp.swipe_move(0.0, 15.0, 40);
    }
    let (alpha, _) = bed.hp.launcher_look().unwrap_or_default();
    assert!((alpha - 0.5).abs() < 0.01, "{alpha}");
    bed.hp.swipe_end(40, false);
    bed.closed()?;
    Ok(())
}

#[test]
fn the_launcher_follows_the_fingers() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("")? else {
        return Ok(());
    };
    // Half its travel: half there, its panel halfway from 0.97 to whole;
    // and nothing moves while the fingers rest.
    bed.up(150.0, 10, 40);
    let (alpha, scale) = bed.held_look()?;
    assert!((alpha - 0.5).abs() < 0.01, "{alpha}");
    assert!((scale - 0.985).abs() < 0.001, "{scale}");
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.launcher_look(), Some((alpha, scale)));
    assert!(bed.hp.launcher_held());
    // Back down a little: it follows them back.
    bed.hp.swipe_move(0.0, 60.0, 40);
    let (back, _) = bed.hp.launcher_look().unwrap_or_default();
    assert!((back - 0.3).abs() < 0.01, "{back}");
    bed.hp.swipe_end(40, false);
    bed.closed()?;
    Ok(())
}

#[test]
fn with_reduced_motion_it_only_fades_on_release() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("[appearance]\nreduced-motion = true\n")? else {
        return Ok(());
    };
    // Held half way: nothing shows yet.
    bed.up(150.0, 10, 40);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.launcher_look().map(|l| l.0), Some(0.0));
    // Let go: it fades in, whole, never grown.
    bed.hp.swipe_end(40, false);
    let fading = bed.hp.dispatch_until(WAIT, |hp| {
        hp.launcher_look()
            .is_some_and(|(alpha, scale)| alpha > 0.0 && alpha < 1.0 && scale == 1.0)
    })?;
    assert!(fading, "{:?}", bed.hp.launcher_look());
    bed.opened()?;
    Ok(())
}

#[test]
fn the_swipe_ignores_natural_scroll() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.touchpad.natural_scroll = true;
    // Fingers going left bring the desktop on the right, natural or not.
    let mut hp = bare(config)?;
    hp.swipe(3, &[(-30.0, 0.0); 5], 40);
    assert_eq!(hp.desktop().0, 1);
    let Some(mut bed) = Bed::start("[touchpad]\nnatural-scroll = true\n")? else {
        return Ok(());
    };
    // Up opens it; down is the overview's (8.5 A), down again closes it.
    bed.hp.swipe(4, &[(0.0, 40.0); 4], EVERY);
    assert_eq!(bed.hp.swipes_given(), (0, 0));
    assert_eq!(bed.hp.overview(), Some(true));
    bed.hp.swipe(4, &[(0.0, 40.0); 4], EVERY);
    assert_eq!(bed.hp.overview(), Some(false));
    bed.up(150.0, 10, 40);
    bed.hp.swipe_end(40, false);
    bed.opened()?;
    Ok(())
}

#[test]
fn swipe_launcher_off_live_leaves_four_fingers_to_the_application() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("")? else {
        return Ok(());
    };
    // As the Settings write it: the compositor follows the file.
    std::fs::write(bed.file(), "[touchpad]\nswipe-launcher = false\n")?;
    let read = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.state().config.touchpad.swipe_launcher)?;
    assert!(read, "the compositor never read the file");
    // Four fingers up are the application's, and nothing opens.
    bed.hp.swipe(4, &[(0.0, -60.0); 5], EVERY);
    assert_eq!(bed.hp.swipes_given(), (1, 5));
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(bed.hp.launcher_surfaces().is_empty());
    // On again: they open it.
    std::fs::write(bed.file(), "[touchpad]\nswipe-launcher = true\n")?;
    let read = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.state().config.touchpad.swipe_launcher)?;
    assert!(read, "the compositor never read the file again");
    bed.up(150.0, 10, 40);
    bed.hp.swipe_end(40, false);
    bed.opened()?;
    Ok(())
}

/// The pull request's screenshots: the launcher held at a third and at
/// two thirds of the fingers' travel, then let go. Written to
/// `$HOCUS_SHOTS` when it names a folder. Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context as _;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start("")? else {
        return Ok(());
    };
    let shoot = |hp: &mut Headless, name: &str| -> anyhow::Result<()> {
        let frames = hp.frames();
        hp.dispatch_until(WAIT, |hp| hp.frames() > frames)?;
        wait(hp, Duration::from_millis(100))?;
        let (w, h, px) = hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    common::open(&mut bed.hp, "test.under", (900, 600))?;
    bed.hp.pointer_move(600.0, 500.0);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    shoot(&mut bed.hp, "before.png")?;
    bed.up(100.0, 5, 40);
    bed.held_look()?;
    shoot(&mut bed.hp, "launcher-a-third.png")?;
    for _ in 0..5 {
        bed.hp.swipe_move(0.0, -20.0, 40);
    }
    shoot(&mut bed.hp, "launcher-two-thirds.png")?;
    bed.hp.swipe_end(40, false);
    bed.opened()?;
    shoot(&mut bed.hp, "launcher-let-go.png")?;
    Ok(())
}
